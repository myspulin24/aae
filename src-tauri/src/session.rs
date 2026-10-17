//! Běh relace: síťový uzel iroh, hostitelská smyčka, vlastní klient
//! a předávání stavu do okna (události `aae://…`).
//!
//! Hostitel spouští smyčku z `aae-net` a připojuje se k ní vlastním klientem
//! v paměti; host se připojuje přes iroh a po výpadku to zkouší znovu každých
//! 5 s, dokud se hráč nevrátí do menu (R5). Ukládá jen hostitel (R10).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use aae_net::PeerKey;
use aae_net::client::{ClientConfig, ClientHandle, ClientSender, ClientUpdate, spawn_client};
use aae_net::host::{Campaign, HostCommand, HostConfig, HostNotice, spawn_host};
use aae_net::protocol::{ClientMsg, LobbyRequest, LobbyView};
use aae_net::transport::iroh_net::{IrohNode, generate_secret};
use aae_net::transport::mem;
use aae_net::view::View;
use aae_sim::calendar::date_of_day;
use aae_sim::data::World;
use aae_sim::economy::{BaseDemand, price_bounds, reference_fare};
use aae_sim::model::{AircraftId, AircraftTypeId, AirportId, ContractId, Leg};
use aae_sim::schedule::{self, ScheduleError, block_minutes};
use aae_sim::{CampaignRules, Event};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

use crate::dto::{
    self, AppInfo, CampaignInfoDto, LegDto, LegInput, Phase, PlanPreviewDto, ReplyDto,
    RivalPriceDto, RouteInfoDto, RulesInput, SlotDto, StatusDto, StrategyDto, TypeFitDto, UiIntent,
    WaitingPeerDto,
};
use crate::profile::Profile;
use crate::saves::{self, Saver};
use crate::texts;

pub const EV_STATUS: &str = "aae://status";
pub const EV_LOBBY: &str = "aae://lobby";
pub const EV_GAME: &str = "aae://game";
pub const EV_NEWS: &str = "aae://news";
pub const EV_REPLY: &str = "aae://reply";
pub const EV_WAITING: &str = "aae://waiting";
pub const EV_ERROR: &str = "aae://error";

const RECONNECT_EVERY: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(3);
const LEAVE_GRACE: Duration = Duration::from_millis(800);

#[derive(Clone)]
pub struct Runtime {
    app: AppHandle,
    world: Arc<World>,
    demand: Arc<BaseDemand>,
    version: String,
    profile_path: PathBuf,
    campaigns_root: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    profile: Profile,
    /// Číslo relace; úlohy staré relace podle něj poznají, že mají skončit.
    generation: u64,
    session: Option<Session>,
    status: StatusDto,
    view: Option<View>,
    lobby: Option<LobbyView>,
    news_seq: u32,
    /// Události čekající na další stav hry: hostitel posílá události hned,
    /// stav nejvýš po 100 ms, a zpráva o nové zakázce potřebuje i její detail.
    pending: Vec<Event>,
}

struct Session {
    node: IrohNode,
    key: PeerKey,
    client: Option<ClientSender>,
    host: Option<HostSide>,
}

struct HostSide {
    commands: mpsc::UnboundedSender<HostCommand>,
    saver: Option<Saver>,
    campaign_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpEnd {
    /// Hráč mezitím odešel nebo začal jinou relaci.
    Stale,
    /// Spojení vypadlo.
    Lost,
    /// Hostitel spojení ukončil nebo odmítl.
    Closed,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Runtime {
    pub fn new(
        app: AppHandle,
        profile_path: PathBuf,
        campaigns_root: PathBuf,
    ) -> anyhow::Result<Runtime> {
        let world = World::europe_v1().map_err(|e| anyhow::anyhow!("herní data: {}", e.0))?;
        let demand = BaseDemand::compute(&world);
        let profile = Profile::load_or_create(&profile_path)?;
        Ok(Runtime {
            version: app.package_info().version.to_string(),
            app,
            world: Arc::new(world),
            demand: Arc::new(demand),
            profile_path,
            campaigns_root,
            inner: Arc::new(Mutex::new(Inner {
                profile,
                generation: 0,
                session: None,
                status: StatusDto::menu(),
                view: None,
                lobby: None,
                news_seq: 0,
                pending: Vec::new(),
            })),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        let _ = self.app.emit(event, payload);
    }

    fn is_current(&self, generation: u64) -> bool {
        self.lock().generation == generation
    }

    /// Upraví stav relace a pošle ho oknu (jen pro aktuální relaci).
    fn update_status(&self, generation: u64, f: impl FnOnce(&mut StatusDto)) {
        let status = {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            let before = inner.status.clone();
            f(&mut inner.status);
            if inner.status == before {
                return;
            }
            inner.status.clone()
        };
        self.emit(EV_STATUS, status);
    }

    // ------------------------------------------------------------ dotazy

    pub fn info(&self) -> AppInfo {
        AppInfo {
            version: self.version.clone(),
            name: self.lock().profile.name.clone(),
            world: dto::world_dto(&self.world),
            campaigns_dir: self.campaigns_root.to_string_lossy().into_owned(),
        }
    }

    pub fn status(&self) -> StatusDto {
        self.lock().status.clone()
    }

    pub fn campaigns(&self) -> Vec<CampaignInfoDto> {
        saves::list(&self.campaigns_root)
    }

    pub fn campaigns_root(&self) -> PathBuf {
        self.campaigns_root.clone()
    }

    pub fn set_name(&self, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        let mut inner = self.lock();
        inner.profile.name = name;
        inner.profile.save(&self.profile_path).map_err(err)
    }

    // ------------------------------------------------------------ relace

    /// Začne novou relaci: zvýší číslo relace a ověří jméno hráče.
    fn begin(&self, host: bool) -> Result<(u64, [u8; 32], String), String> {
        let (generation, secret, name, status) = {
            let mut inner = self.lock();
            if inner.profile.name.trim().is_empty() {
                return Err("Nejdřív si nastavte jméno.".into());
            }
            inner.generation += 1;
            inner.status = StatusDto {
                phase: Phase::Connecting,
                host,
                ..StatusDto::menu()
            };
            inner.news_seq = 0;
            inner.pending.clear();
            (
                inner.generation,
                inner.profile.secret(),
                inner.profile.name.clone(),
                inner.status.clone(),
            )
        };
        self.emit(EV_STATUS, status);
        Ok((generation, secret, name))
    }

    /// Vrátí okno do menu s chybou (nepovedené připojení).
    fn fail(&self, generation: u64, message: String) -> String {
        self.update_status(generation, |s| {
            *s = StatusDto::menu();
        });
        message
    }

    pub async fn host_start(&self, load: Option<PathBuf>) -> Result<(), String> {
        self.leave().await;
        let loaded = match &load {
            Some(p) => Some(saves::load(p).map_err(err)?),
            None => None,
        };
        let (generation, secret, name) = self.begin(true)?;
        let node = match IrohNode::bind(secret).await {
            Ok(n) => n,
            Err(e) => return Err(self.fail(generation, format!("{e:#}"))),
        };
        let invitation = node.ticket().await;
        if !self.is_current(generation) {
            node.close().await;
            return Ok(());
        }
        let key = node.key();

        // Příchozí spojení: vlastní klient v paměti + hosté přes iroh.
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let (host_conn, client_conn) = mem::local_pair(key);
        let _ = in_tx.send(host_conn);
        let mut listen = node.listen();
        tokio::spawn(async move {
            while let Some(conn) = listen.recv().await {
                if in_tx.send(conn).is_err() {
                    break;
                }
            }
        });

        let seed = u64::from_le_bytes(generate_secret()[..8].try_into().expect("8 bajtů"));
        let mut host = spawn_host(
            HostConfig {
                app_version: self.version.clone(),
                host_key: key,
                seed,
            },
            in_rx,
            loaded.clone(),
        );
        let client = spawn_client(
            client_conn,
            ClientConfig {
                app_version: self.version.clone(),
                name,
            },
        );

        let (saver, campaign_name) = match (&load, loaded) {
            (Some(path), Some(campaign)) => {
                let dir = path.parent().map(PathBuf::from).unwrap_or_default();
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let saver = Saver::new(dir);
                let s = saver.clone();
                let _ = tokio::task::spawn_blocking(move || s.session_start(campaign)).await;
                (Some(saver), name)
            }
            _ => (None, saves::default_campaign_name()),
        };

        let status = {
            let mut inner = self.lock();
            if inner.generation != generation {
                None
            } else {
                inner.session = Some(Session {
                    node: node.clone(),
                    key,
                    client: Some(client.sender.clone()),
                    host: Some(HostSide {
                        commands: host.commands.clone(),
                        saver,
                        campaign_name: campaign_name.clone(),
                    }),
                });
                inner.status.invitation = Some(invitation);
                inner.status.campaign_name = Some(campaign_name);
                Some(inner.status.clone())
            }
        };
        let Some(status) = status else {
            let _ = host.commands.send(HostCommand::Shutdown);
            node.close().await;
            return Ok(());
        };
        self.emit(EV_STATUS, status);

        let rt = self.clone();
        tokio::spawn(async move {
            while let Some(notice) = host.notices.recv().await {
                if !rt.is_current(generation) {
                    break;
                }
                rt.on_host_notice(generation, notice);
            }
        });
        let rt = self.clone();
        tokio::spawn(async move {
            if rt.pump(generation, client.updates).await == PumpEnd::Lost {
                rt.close(generation, "Hostitelská smyčka nečekaně skončila.".into());
            }
        });
        Ok(())
    }

    pub async fn join(&self, invitation: String) -> Result<(), String> {
        let invitation = invitation.trim().to_string();
        if invitation.is_empty() {
            return Err("Vložte pozvánku od hostitele.".into());
        }
        self.leave().await;
        let (generation, secret, name) = self.begin(false)?;
        let node = match IrohNode::bind(secret).await {
            Ok(n) => n,
            Err(e) => return Err(self.fail(generation, format!("{e:#}"))),
        };
        let conn = match tokio::time::timeout(CONNECT_TIMEOUT, node.connect(&invitation)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                node.close().await;
                return Err(self.fail(generation, format!("{e:#}")));
            }
            Err(_) => {
                node.close().await;
                return Err(self.fail(
                    generation,
                    "Hostitel neodpovídá. Zkontrolujte pozvánku a jestli má hostitel hru otevřenou."
                        .into(),
                ));
            }
        };
        let handle = spawn_client(
            conn,
            ClientConfig {
                app_version: self.version.clone(),
                name: name.clone(),
            },
        );
        let current = {
            let mut inner = self.lock();
            let current = inner.generation == generation;
            if current {
                inner.session = Some(Session {
                    node: node.clone(),
                    key: node.key(),
                    client: Some(handle.sender.clone()),
                    host: None,
                });
            }
            current
        };
        if !current {
            node.close().await;
            return Ok(());
        }
        let rt = self.clone();
        tokio::spawn(rt.client_loop(generation, node, invitation, name, handle));
        Ok(())
    }

    /// Host: předává aktualizace a po výpadku se připojuje znovu.
    async fn client_loop(
        self,
        generation: u64,
        node: IrohNode,
        ticket: String,
        name: String,
        mut handle: ClientHandle,
    ) {
        loop {
            match self.pump(generation, handle.updates).await {
                PumpEnd::Stale | PumpEnd::Closed => return,
                PumpEnd::Lost => {}
            }
            self.update_status(generation, |s| {
                s.phase = Phase::Reconnecting;
                s.message =
                    Some("Spojení s hostitelem se přerušilo. Zkouším se znovu připojit…".into());
            });
            handle = loop {
                tokio::time::sleep(RECONNECT_EVERY).await;
                if !self.is_current(generation) {
                    return;
                }
                if let Ok(Ok(conn)) =
                    tokio::time::timeout(CONNECT_TIMEOUT, node.connect(&ticket)).await
                {
                    let h = spawn_client(
                        conn,
                        ClientConfig {
                            app_version: self.version.clone(),
                            name: name.clone(),
                        },
                    );
                    let mut inner = self.lock();
                    if inner.generation != generation {
                        return;
                    }
                    if let Some(s) = inner.session.as_mut() {
                        s.client = Some(h.sender.clone());
                    }
                    break h;
                }
            };
        }
    }

    async fn pump(
        &self,
        generation: u64,
        mut updates: mpsc::UnboundedReceiver<ClientUpdate>,
    ) -> PumpEnd {
        while let Some(update) = updates.recv().await {
            if !self.is_current(generation) {
                return PumpEnd::Stale;
            }
            match update {
                ClientUpdate::Lost => return PumpEnd::Lost,
                ClientUpdate::Refused(r) => {
                    self.close(generation, texts::refuse(&r));
                    return PumpEnd::Closed;
                }
                ClientUpdate::Ended(r) => {
                    self.close(generation, texts::ended(&r));
                    return PumpEnd::Closed;
                }
                other => self.on_update(generation, other),
            }
        }
        if self.is_current(generation) {
            PumpEnd::Lost
        } else {
            PumpEnd::Stale
        }
    }

    fn on_update(&self, generation: u64, update: ClientUpdate) {
        match update {
            ClientUpdate::Welcome { host_version } => {
                self.update_status(generation, |s| s.host_version = Some(host_version));
            }
            ClientUpdate::Lobby(l) => {
                let me = match self.lock().session.as_ref() {
                    Some(s) => s.key,
                    None => return,
                };
                let lobby = dto::lobby_dto(&self.world, &l, &me);
                self.lock().lobby = Some(l);
                self.update_status(generation, |s| {
                    s.phase = Phase::Lobby;
                    s.message = None;
                });
                self.emit(EV_LOBBY, lobby);
            }
            ClientUpdate::View(v) => {
                let game = dto::game_dto(&self.world, &v);
                let news = {
                    let mut inner = self.lock();
                    inner.view = Some(*v);
                    self.take_news(&mut inner)
                };
                self.update_status(generation, |s| {
                    s.phase = Phase::Game;
                    s.message = None;
                });
                self.emit(EV_GAME, game);
                if !news.is_empty() {
                    self.emit(EV_NEWS, news);
                }
            }
            ClientUpdate::Events(events) => {
                let news = {
                    let mut inner = self.lock();
                    inner.pending.extend(events.iter().cloned());
                    if inner.view.is_some() {
                        Vec::new()
                    } else {
                        self.take_news(&mut inner)
                    }
                };
                if !news.is_empty() {
                    self.emit(EV_NEWS, news);
                }
                for e in &events {
                    match e {
                        Event::DayStarted { day } if *day > 0 => self.autosave(generation, *day),
                        _ => {}
                    }
                }
            }
            ClientUpdate::Ack(id) => self.emit(
                EV_REPLY,
                ReplyDto {
                    id: id as u32,
                    ok: true,
                    text: None,
                },
            ),
            ClientUpdate::Reject(id, reason) => self.emit(
                EV_REPLY,
                ReplyDto {
                    id: id as u32,
                    ok: false,
                    text: Some(texts::host_reject(&self.world, &reason)),
                },
            ),
            ClientUpdate::Waiting => {
                self.update_status(generation, |s| {
                    s.phase = Phase::Waiting;
                    s.message = None;
                });
            }
            ClientUpdate::Path(p) => {
                self.update_status(generation, |s| s.path = p.into());
            }
            ClientUpdate::Lost | ClientUpdate::Refused(_) | ClientUpdate::Ended(_) => {}
        }
    }

    fn take_news(&self, inner: &mut Inner) -> Vec<dto::NewsDto> {
        let events = std::mem::take(&mut inner.pending);
        let mut seq = inner.news_seq;
        let news = texts::news(&self.world, inner.view.as_ref(), &events, &mut seq);
        inner.news_seq = seq;
        news
    }

    /// Konec relace z druhé strany: okno ukáže důvod a tlačítko do menu.
    fn close(&self, generation: u64, message: String) {
        let session = {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            inner.session.take()
        };
        self.update_status(generation, |s| {
            s.phase = Phase::Closed;
            s.message = Some(message);
            s.invitation = None;
        });
        if let Some(s) = session {
            tokio::spawn(async move { s.node.close().await });
        }
    }

    /// Odchod do menu. Hostitel kampaň před ukončením uloží.
    pub async fn leave(&self) {
        let session = {
            let mut inner = self.lock();
            inner.generation += 1;
            inner.view = None;
            inner.lobby = None;
            inner.status = StatusDto::menu();
            inner.session.take()
        };
        self.emit(EV_STATUS, StatusDto::menu());
        let Some(session) = session else { return };
        if let Some(client) = &session.client {
            client.send(ClientMsg::Leave);
        }
        if let Some(host) = session.host {
            if let Some(saver) = host.saver.clone()
                && let Some(c) = snapshot(&host.commands).await
            {
                let _ = tokio::task::spawn_blocking(move || saver.save(c)).await;
            }
            let _ = host.commands.send(HostCommand::Shutdown);
        }
        // Poslední zprávy (konec hry, odchod) musí dojít, než se uzel zavře.
        tokio::time::sleep(LEAVE_GRACE).await;
        session.node.close().await;
    }

    // ------------------------------------------------------------ hostitel

    fn host_commands(&self) -> Result<mpsc::UnboundedSender<HostCommand>, String> {
        self.lock()
            .session
            .as_ref()
            .and_then(|s| s.host.as_ref())
            .map(|h| h.commands.clone())
            .ok_or_else(|| "Tohle smí jen hostitel.".to_string())
    }

    pub fn host_command(&self, cmd: HostCommand) -> Result<(), String> {
        self.host_commands()?
            .send(cmd)
            .map_err(|_| "Hostitelská smyčka už neběží.".to_string())
    }

    pub fn host_set_rules(&self, input: RulesInput) -> Result<(), String> {
        if !(30..=3650).contains(&input.duration_days) {
            return Err("Délka kampaně musí být 30 až 3650 dní.".into());
        }
        if !(5_000_000.0..=500_000_000.0).contains(&input.starting_capital) {
            return Err("Počáteční kapitál musí být 5 až 500 milionů Kč.".into());
        }
        let speed = dto::speed_of(input.initial_speed).ok_or("Neznámá rychlost.")?;
        let base = self
            .lock()
            .lobby
            .as_ref()
            .map(|l| l.rules.clone())
            .unwrap_or_default();
        self.host_command(HostCommand::SetRules(CampaignRules {
            duration_days: input.duration_days as u64,
            starting_capital: dto::money(input.starting_capital),
            initial_speed: speed,
            events: input.events,
            ..base
        }))
    }

    pub fn host_set_ai(&self, ai: Vec<StrategyDto>) -> Result<(), String> {
        self.host_command(HostCommand::SetAi(ai.into_iter().map(Into::into).collect()))
    }

    pub fn host_set_campaign_name(&self, name: String) -> Result<(), String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("Kampaň potřebuje název.".into());
        }
        let generation = {
            let mut inner = self.lock();
            let generation = inner.generation;
            let host = inner
                .session
                .as_mut()
                .and_then(|s| s.host.as_mut())
                .ok_or("Tohle smí jen hostitel.")?;
            if host.saver.is_some() {
                return Err("Rozehraná kampaň už má složku.".into());
            }
            host.campaign_name = name.clone();
            generation
        };
        self.update_status(generation, |s| s.campaign_name = Some(name));
        Ok(())
    }

    pub fn host_peer(&self, peer: &str) -> Result<PeerKey, String> {
        dto::parse_hex(peer)
            .map(PeerKey)
            .ok_or_else(|| "Neznámý hráč.".to_string())
    }

    fn on_host_notice(&self, generation: u64, notice: HostNotice) {
        match notice {
            HostNotice::Waiting(list) => self.emit(
                EV_WAITING,
                list.iter()
                    .map(|w| WaitingPeerDto {
                        key: dto::hex(&w.key),
                        name: w.name.clone(),
                        version: w.version.clone(),
                    })
                    .collect::<Vec<_>>(),
            ),
            HostNotice::Started => {
                let name = {
                    let inner = self.lock();
                    inner
                        .session
                        .as_ref()
                        .and_then(|s| s.host.as_ref())
                        .map(|h| h.campaign_name.clone())
                };
                let Some(name) = name else { return };
                match saves::new_campaign_dir(&self.campaigns_root, &name) {
                    Ok(dir) => {
                        let folder = dir
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or(name);
                        {
                            let mut inner = self.lock();
                            if let Some(h) = inner.session.as_mut().and_then(|s| s.host.as_mut()) {
                                h.saver = Some(Saver::new(dir));
                                h.campaign_name = folder.clone();
                            }
                        }
                        self.update_status(generation, |s| s.campaign_name = Some(folder));
                        let rt = self.clone();
                        tokio::spawn(async move { rt.save_now(generation, SaveKind::Start).await });
                    }
                    Err(e) => self.emit(
                        EV_ERROR,
                        format!("Kampaň nejde uložit: {e:#}. Hra poběží bez ukládání."),
                    ),
                }
            }
            HostNotice::Error(e) => self.emit(EV_ERROR, e),
        }
    }

    fn autosave(&self, generation: u64, day: u64) {
        let rt = self.clone();
        tokio::spawn(async move { rt.save_now(generation, SaveKind::Auto(day)).await });
    }

    pub async fn save_manual(&self) -> Result<(), String> {
        let generation = self.lock().generation;
        self.save_now(generation, SaveKind::Manual).await
    }

    /// Uloží kampaň mimo hostitelskou smyčku (kopie stavu, zápis ve vlákně pro I/O).
    async fn save_now(&self, generation: u64, kind: SaveKind) -> Result<(), String> {
        let (commands, saver) = {
            let inner = self.lock();
            let host = inner
                .session
                .as_ref()
                .and_then(|s| s.host.as_ref())
                .ok_or("Ukládá jen hostitel.")?;
            (
                host.commands.clone(),
                host.saver.clone().ok_or("Kampaň ještě nezačala.")?,
            )
        };
        let campaign = snapshot(&commands)
            .await
            .ok_or("Hostitel teď stav nevydal, zkuste to znovu.")?;
        let day = aae_sim::time::day_of(campaign.state.now);
        let result = tokio::task::spawn_blocking(move || match kind {
            SaveKind::Auto(day) => saver.autosave(campaign, day),
            SaveKind::Start => saver.session_start(campaign),
            SaveKind::Manual => saver.save(campaign),
        })
        .await
        .map_err(err)?;
        match result {
            Ok(()) => {
                self.update_status(generation, |s| {
                    s.saved = Some(format!("Uloženo: {}", texts::date(day)));
                });
                Ok(())
            }
            Err(e) => {
                let text = format!("Uložení kampaně selhalo: {e:#}");
                self.emit(EV_ERROR, text.clone());
                Err(text)
            }
        }
    }

    // ------------------------------------------------------------ hráč

    fn client(&self) -> Result<ClientSender, String> {
        self.lock()
            .session
            .as_ref()
            .and_then(|s| s.client.clone())
            .ok_or_else(|| "Nejste připojeni ke hře.".to_string())
    }

    pub fn send_intent(&self, ui: UiIntent) -> Result<u32, String> {
        let me = self
            .lock()
            .view
            .as_ref()
            .map(|v| v.me)
            .ok_or("Kampaň ještě nezačala.")?;
        Ok(self.client()?.intent(dto::intent(me, ui)) as u32)
    }

    pub fn client_send(&self, msg: ClientMsg) -> Result<(), String> {
        self.client()?.send(msg);
        Ok(())
    }

    pub fn lobby_set_profile(&self, name: String, home: u16) -> Result<(), String> {
        let name = clean_name(&name)?;
        if !self.world.is_valid_airport(AirportId(home)) {
            return Err("Neznámé letiště.".into());
        }
        self.set_name(&name)?;
        self.client_send(ClientMsg::Lobby(LobbyRequest::SetProfile {
            name,
            home: AirportId(home),
        }))
    }

    pub fn propose_speed(&self, speed: u8) -> Result<(), String> {
        let speed = dto::speed_of(speed).ok_or("Neznámá rychlost.")?;
        self.client_send(ClientMsg::ProposeSpeed(speed))
    }

    pub fn presence(&self, contract: Option<u32>) -> Result<(), String> {
        self.client_send(ClientMsg::Presence {
            contract: contract.map(ContractId),
        })
    }

    // ------------------------------------------------------------ plánování

    fn airport(&self, id: u16) -> Result<AirportId, String> {
        let id = AirportId(id);
        if self.world.is_valid_airport(id) {
            Ok(id)
        } else {
            Err("Neznámé letiště.".into())
        }
    }

    fn model(&self, id: u8) -> Result<AircraftTypeId, String> {
        let id = AircraftTypeId(id);
        if self.world.is_valid_aircraft_type(id) {
            Ok(id)
        } else {
            Err("Neznámý typ letadla.".into())
        }
    }

    pub fn route_info(&self, from: u16, to: u16) -> Result<RouteInfoDto, String> {
        let (from, to) = (self.airport(from)?, self.airport(to)?);
        let world = &self.world;
        let distance = world.distance(from, to);
        let reference = reference_fare(distance);
        let (min, max) = price_bounds(distance);
        let inner = self.lock();
        let view = inner.view.as_ref();
        let day = view
            .map(|v| aae_sim::time::day_of(v.header.now))
            .unwrap_or(0);
        let month = date_of_day(day).month;
        let route = (from, to);
        let my_price = view.and_then(|v| v.my().prices.get(&route).copied());
        let competitors = view
            .map(|v| {
                v.airlines
                    .values()
                    .filter(|a| a.airline.id != v.me && a.network.contains(&route))
                    .map(|a| RivalPriceDto {
                        airline: a.airline.id.0,
                        price: dto::kc(a.airline.prices.get(&route).copied().unwrap_or(reference)),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(RouteInfoDto {
            from: from.0,
            to: to.0,
            distance_km: distance,
            reference: dto::kc(reference),
            min: dto::kc(min),
            max: dto::kc(max),
            demand: self.demand.seasonal(from, to, month),
            demand_back: self.demand.seasonal(to, from, month),
            my_price: my_price.map(dto::kc),
            competitors,
            fits: (0..world.aircraft_types.len())
                .map(|i| {
                    let t = AircraftTypeId(i as u8);
                    TypeFitDto {
                        model: i as u8,
                        block_min: block_minutes(world, t, from, to),
                        in_range: distance <= world.aircraft_type(t).range_km,
                    }
                })
                .collect(),
        })
    }

    /// Kontrola plánu letadla před odesláním (stejná pravidla jako simulace).
    pub fn plan_preview(
        &self,
        aircraft: Option<u32>,
        model: u8,
        legs: Vec<LegInput>,
    ) -> Result<PlanPreviewDto, String> {
        let model = self.model(model)?;
        let world = &self.world;
        let legs: Vec<Leg> = legs.into_iter().map(Leg::from).collect();
        let valid = legs
            .iter()
            .all(|l| world.is_valid_airport(l.from) && world.is_valid_airport(l.to));
        let dto_legs = if valid {
            legs.iter()
                .map(|l| LegDto {
                    from: l.from.0,
                    to: l.to.0,
                    dep: l.dep_min,
                    arr: l.dep_min + block_minutes(world, model, l.from, l.to),
                    distance_km: world.distance(l.from, l.to),
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut error = schedule::validate(world, model, &legs)
            .err()
            .map(|e| texts::schedule_error(world, &e));

        // Sloty na přetížených letištích podle celé flotily s novým plánem.
        let mut slots = Vec::new();
        if let Some(view) = self.lock().view.as_ref() {
            let my = view.my();
            let replace = aircraft.map(AircraftId);
            let mut airports: Vec<AirportId> = legs
                .iter()
                .map(|l| l.from)
                .filter(|a| world.is_valid_airport(*a) && world.airport(*a).slot_constrained)
                .collect();
            airports.sort();
            airports.dedup();
            for airport in airports {
                let needed: u32 = my
                    .fleet
                    .values()
                    .map(|p| {
                        let plan: &[Leg] = if Some(p.id) == replace {
                            &legs
                        } else {
                            &p.schedule
                        };
                        plan.iter().filter(|l| l.from == airport).count() as u32
                    })
                    .sum::<u32>()
                    + if replace.is_none() {
                        legs.iter().filter(|l| l.from == airport).count() as u32
                    } else {
                        0
                    };
                let owned = my.slots.get(&airport).copied().unwrap_or(0);
                if needed > owned && error.is_none() {
                    error = Some(texts::schedule_error(
                        world,
                        &ScheduleError::MissingSlots {
                            airport,
                            needed,
                            owned,
                        },
                    ));
                }
                slots.push(SlotDto {
                    airport: airport.0,
                    owned,
                    used: needed,
                });
            }
        }
        Ok(PlanPreviewDto {
            legs: dto_legs,
            error_leg: error.as_ref().and_then(|(leg, _)| leg.map(|l| l as u32)),
            error: error.map(|(_, text)| text),
            slots,
        })
    }

    /// Kyvadlo `base ⇄ dest` od `start` do posledního příletu `until`.
    pub fn plan_shuttle(
        &self,
        model: u8,
        base: u16,
        dest: u16,
        start: u32,
        until: u32,
    ) -> Result<Vec<LegInput>, String> {
        let model = self.model(model)?;
        let (base, dest) = (self.airport(base)?, self.airport(dest)?);
        if base == dest {
            return Err("Vyberte jiné cílové letiště.".into());
        }
        let legs = schedule::shuttle(&self.world, model, base, dest, start.min(1439), until);
        if legs.is_empty() {
            return Err("Do zvoleného času se nevejde ani jedna otočka.".into());
        }
        Ok(legs
            .into_iter()
            .map(|l| LegInput {
                from: l.from.0,
                to: l.to.0,
                dep: l.dep_min,
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug)]
enum SaveKind {
    Auto(u64),
    Start,
    Manual,
}

/// Kopie kampaně z hostitelské smyčky.
async fn snapshot(commands: &mpsc::UnboundedSender<HostCommand>) -> Option<Campaign> {
    let (tx, rx) = oneshot::channel();
    commands.send(HostCommand::Snapshot(tx)).ok()?;
    tokio::time::timeout(SNAPSHOT_TIMEOUT, rx)
        .await
        .ok()?
        .ok()?
}

fn clean_name(name: &str) -> Result<String, String> {
    let name: String = name.trim().chars().take(24).collect();
    if name.is_empty() {
        Err("Zadejte jméno.".into())
    } else {
        Ok(name)
    }
}
