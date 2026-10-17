//! Hostitelská smyčka (R2): lobby, fronta záměrů, krokování herního času,
//! pauzy, hlasování o rychlosti, heartbeat a automatická pauza, čekárna
//! neznámých klíčů a rozesílání filtrovaných pohledů.
//!
//! Smyčka čte reálný čas jen proto, aby věděla, kolik kroků simulace spustit;
//! samotná simulace hodiny nečte.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use aae_sim::model::Controller;
use aae_sim::{
    Actor, AirlineId, AirportId, CampaignRules, ContractId, Event, GameState, Intent, NewAirline,
    PauseReason, Reject, Speed, Strategy,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

use crate::PeerKey;
use crate::protocol::{
    ClientMsg, EndReason, HostMsg, HostReject, LobbyPlayer, LobbyRequest, LobbyView,
    PROTOCOL_VERSION, RefuseReason, decode, encode,
};
use crate::transport::{Connection, PathInfo};
use crate::view::{SessionInfo, SpeedVote, Tracker, build_view};

pub const HEARTBEAT: Duration = Duration::from_secs(1);
/// Spojení bez zprávy déle než 3 s je ztracené (R5).
pub const LOST_AFTER: Duration = Duration::from_secs(3);
/// Delta nejvýš 10× za sekundu (R3).
pub const DELTA_EVERY: Duration = Duration::from_millis(100);
pub const PLAYER_PAUSES_PER_SESSION: u8 = 3;
pub const PLAYER_PAUSE_MAX: Duration = Duration::from_secs(5 * 60);
pub const VOTE_TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_HUMANS: usize = 4;
const TICK: Duration = Duration::from_millis(10);
const LOBBY_REFRESH: Duration = Duration::from_secs(2);

const AI_HOMES: [&str; 12] = [
    "VIE", "MUC", "WAW", "BUD", "BER", "CPH", "ZRH", "DUS", "MAD", "FCO", "ARN", "DUB",
];
const AI_NAMES: [&str; 8] = [
    "Baltic Express",
    "Alpenjet",
    "Danube Wings",
    "Iberia Sol",
    "Nordic Air",
    "Adria Link",
    "Rhein Air",
    "Atlas Avia",
];
const DEFAULT_HOMES: [&str; 4] = ["PRG", "VIE", "MUC", "WAW"];

/// Záznam logu záměrů (R9): krok, pořadí, aktér, záměr, výsledek; losy burzy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LogEntry {
    Intent {
        step: u64,
        seq: u64,
        actor: Actor,
        intent: Intent,
        result: Result<(), Reject>,
    },
    Lottery {
        step: u64,
        contract: ContractId,
        winner: AirlineId,
        candidates: Vec<AirlineId>,
    },
}

/// Rozehraná kampaň: stav simulace, přiřazení klíčů k aerolinkám a log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    pub state: GameState,
    pub roster: BTreeMap<AirlineId, PeerKey>,
    pub names: BTreeMap<AirlineId, String>,
    pub log: Vec<LogEntry>,
}

pub struct HostConfig {
    pub app_version: String,
    pub host_key: PeerKey,
    pub seed: u64,
}

pub enum HostCommand {
    SetRules(CampaignRules),
    SetAi(Vec<Strategy>),
    Start,
    /// Neznámý klíč z čekárny přiřadit k aerolince (i AI nebo převod ztraceného klíče).
    Assign {
        peer: PeerKey,
        airline: AirlineId,
    },
    Refuse {
        peer: PeerKey,
    },
    TechnicalPause(bool),
    /// Ukončit automatickou pauzu dřív, než se vrátí všichni.
    ResumeAuto,
    /// Kopie kampaně pro uložení.
    Snapshot(oneshot::Sender<Option<Campaign>>),
    Shutdown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HostNotice {
    Waiting(Vec<WaitingPeer>),
    Started,
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaitingPeer {
    pub key: PeerKey,
    pub name: String,
    pub version: String,
}

pub struct HostHandle {
    pub commands: mpsc::UnboundedSender<HostCommand>,
    pub notices: mpsc::UnboundedReceiver<HostNotice>,
    pub task: JoinHandle<()>,
}

pub fn spawn_host(
    cfg: HostConfig,
    incoming: mpsc::UnboundedReceiver<Connection>,
    loaded: Option<Campaign>,
) -> HostHandle {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (notice_tx, notice_rx) = mpsc::unbounded_channel();
    let mut host = Host::new(cfg, notice_tx);
    if let Some(campaign) = loaded {
        host.load(campaign);
    }
    let task = tokio::spawn(host.run(incoming, cmd_rx));
    HostHandle {
        commands: cmd_tx,
        notices: notice_rx,
        task,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    New,
    Lobby,
    Game(AirlineId),
    Waiting,
}

struct Peer {
    key: PeerKey,
    local: bool,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    path: watch::Receiver<PathInfo>,
    last_seen: Instant,
    last_sent: Instant,
    name: String,
    version: String,
    role: Role,
    tracker: Tracker,
}

struct Queued {
    conn: Option<u64>,
    actor: Actor,
    id: u64,
    intent: Intent,
}

struct Vote {
    speed: Speed,
    proposer: AirlineId,
    yes: BTreeSet<AirlineId>,
    needed: BTreeSet<AirlineId>,
    deadline: Instant,
}

struct Game {
    campaign: Campaign,
    queue: VecDeque<Queued>,
    seq: u64,
    acc_nanos: u128,
    last_tick: Instant,
    last_delta: Instant,
    pauses_used: u8,
    pause_started: BTreeMap<AirlineId, Instant>,
    vote: Option<Vote>,
    presence: BTreeMap<AirlineId, ContractId>,
    /// Vzdálení hráči připojení v této relaci.
    seen: BTreeSet<AirlineId>,
    /// Vzdálení hráči, na jejichž návrat čeká automatická pauza.
    missing: BTreeSet<AirlineId>,
    events: Vec<Event>,
}

struct Host {
    cfg: HostConfig,
    rules: CampaignRules,
    ai: Vec<Strategy>,
    peers: BTreeMap<u64, Peer>,
    next_conn: u64,
    lobby: Vec<(PeerKey, String, AirportId)>,
    last_lobby: Instant,
    game: Option<Game>,
    notices: mpsc::UnboundedSender<HostNotice>,
    world: aae_sim::data::World,
}

impl Host {
    fn new(cfg: HostConfig, notices: mpsc::UnboundedSender<HostNotice>) -> Host {
        Host {
            cfg,
            rules: CampaignRules::default(),
            ai: vec![Strategy::Levna, Strategy::Hub],
            peers: BTreeMap::new(),
            next_conn: 1,
            lobby: Vec::new(),
            last_lobby: Instant::now(),
            game: None,
            notices,
            world: aae_sim::data::World::europe_v1().expect("zabudovaná herní data"),
        }
    }

    fn load(&mut self, mut campaign: Campaign) {
        // Načtená kampaň začíná technickou pauzou, než se hráči připojí.
        if !campaign.state.is_paused() {
            let _ = campaign
                .state
                .apply(Actor::System, Intent::Pause(PauseReason::Technical));
        }
        self.game = Some(Game::new(campaign));
    }

    async fn run(
        mut self,
        mut incoming: mpsc::UnboundedReceiver<Connection>,
        mut commands: mpsc::UnboundedReceiver<HostCommand>,
    ) {
        let (inbox_tx, mut inbox) = mpsc::unbounded_channel::<(u64, Option<Vec<u8>>)>();
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut incoming_open = true;
        loop {
            tokio::select! {
                conn = incoming.recv(), if incoming_open => match conn {
                    Some(conn) => self.add_conn(conn, inbox_tx.clone()),
                    None => incoming_open = false,
                },
                Some((cid, frame)) = inbox.recv() => match frame {
                    Some(bytes) => self.on_frame(cid, &bytes),
                    None => self.drop_peer(cid),
                },
                cmd = commands.recv() => match cmd {
                    Some(HostCommand::Shutdown) | None => {
                        self.shutdown();
                        return;
                    }
                    Some(cmd) => self.on_command(cmd),
                },
                _ = tick.tick() => self.on_tick(),
            }
        }
    }

    // ------------------------------------------------------------ spojení

    fn add_conn(
        &mut self,
        mut conn: Connection,
        inbox: mpsc::UnboundedSender<(u64, Option<Vec<u8>>)>,
    ) {
        let cid = self.next_conn;
        self.next_conn += 1;
        let now = Instant::now();
        self.peers.insert(
            cid,
            Peer {
                key: conn.peer,
                local: conn.local,
                tx: conn.tx.clone(),
                path: conn.path.clone(),
                last_seen: now,
                last_sent: now,
                name: String::new(),
                version: String::new(),
                role: Role::New,
                tracker: Tracker::default(),
            },
        );
        tokio::spawn(async move {
            while let Some(frame) = conn.rx.recv().await {
                if inbox.send((cid, Some(frame))).is_err() {
                    return;
                }
            }
            let _ = inbox.send((cid, None));
        });
    }

    fn send(&mut self, cid: u64, msg: &HostMsg) {
        if let Some(p) = self.peers.get_mut(&cid) {
            let _ = p.tx.send(encode(msg));
            p.last_sent = Instant::now();
        }
    }

    fn drop_peer(&mut self, cid: u64) {
        let Some(peer) = self.peers.remove(&cid) else {
            return;
        };
        match peer.role {
            Role::Lobby => {
                if peer.key != self.cfg.host_key {
                    self.lobby.retain(|(k, _, _)| *k != peer.key);
                }
                self.broadcast_lobby();
            }
            Role::Game(airline) => {
                if let Some(g) = self.game.as_mut() {
                    g.presence.remove(&airline);
                    if let Some(v) = g.vote.as_mut() {
                        v.needed.remove(&airline);
                    }
                    g.resolve_vote();
                }
                self.update_auto_pause();
            }
            Role::Waiting => self.notify_waiting(),
            Role::New => {}
        }
    }

    // -------------------------------------------------------------- zprávy

    fn on_frame(&mut self, cid: u64, bytes: &[u8]) {
        let Ok(msg) = decode::<ClientMsg>(bytes) else {
            return;
        };
        let Some(peer) = self.peers.get_mut(&cid) else {
            return;
        };
        peer.last_seen = Instant::now();
        let role = peer.role;
        match msg {
            ClientMsg::Hello {
                app_version,
                protocol,
                name,
                world_hash,
            } => self.on_hello(cid, app_version, protocol, name, world_hash),
            ClientMsg::Heartbeat => {}
            ClientMsg::Lobby(LobbyRequest::SetProfile { name, home }) => {
                if role == Role::Lobby && self.world.is_valid_airport(home) {
                    let key = self.peers[&cid].key;
                    let name = clean_name(&name);
                    if let Some(entry) = self.lobby.iter_mut().find(|(k, _, _)| *k == key) {
                        entry.1 = name;
                        entry.2 = home;
                    }
                    self.broadcast_lobby();
                }
            }
            ClientMsg::Intent { id, intent } => match role {
                Role::Game(airline) => self.on_intent(cid, airline, id, intent),
                _ => self.send(
                    cid,
                    &HostMsg::Reject {
                        id,
                        reason: HostReject::NotInGame,
                    },
                ),
            },
            ClientMsg::ProposeSpeed(speed) => {
                if let Role::Game(airline) = role {
                    self.on_propose_speed(airline, speed);
                }
            }
            ClientMsg::Vote { yes } => {
                if let Role::Game(airline) = role {
                    self.on_vote(airline, yes);
                }
            }
            ClientMsg::Presence { contract } => {
                if let (Role::Game(airline), Some(g)) = (role, self.game.as_mut()) {
                    match contract {
                        Some(c) => g.presence.insert(airline, c),
                        None => g.presence.remove(&airline),
                    };
                }
            }
            ClientMsg::Leave => self.drop_peer(cid),
        }
    }

    fn on_hello(
        &mut self,
        cid: u64,
        version: String,
        protocol: u16,
        name: String,
        world_hash: u64,
    ) {
        let refuse = if protocol != PROTOCOL_VERSION {
            Some(RefuseReason::ProtocolMismatch)
        } else if version != self.cfg.app_version {
            // Premisa 5: všichni musí mít stejnou verzi aplikace.
            Some(RefuseReason::VersionMismatch {
                host: self.cfg.app_version.clone(),
                client: version.clone(),
            })
        } else if world_hash != crate::world_hash() {
            Some(RefuseReason::WorldMismatch)
        } else {
            None
        };
        if let Some(reason) = refuse {
            self.send(cid, &HostMsg::Refused(reason));
            self.peers.remove(&cid);
            return;
        }
        let key = self.peers[&cid].key;
        // Starší spojení stejného klíče nahradí nové (opětovné připojení).
        let stale: Vec<u64> = self
            .peers
            .iter()
            .filter(|(c, p)| **c != cid && p.key == key)
            .map(|(c, _)| *c)
            .collect();
        for c in stale {
            self.peers.remove(&c);
        }
        let name = clean_name(&name);
        {
            let p = self.peers.get_mut(&cid).expect("spojení");
            p.name = name.clone();
            p.version = version;
        }
        let host_version = self.cfg.app_version.clone();
        self.send(cid, &HostMsg::Welcome { host_version });
        match self.game.as_ref() {
            None => {
                let humans = self.lobby.len();
                if !self.lobby.iter().any(|(k, _, _)| *k == key) {
                    if humans >= MAX_HUMANS {
                        self.send(cid, &HostMsg::Refused(RefuseReason::Full));
                        self.peers.remove(&cid);
                        return;
                    }
                    let home = self.free_home();
                    if key == self.cfg.host_key {
                        self.lobby.insert(0, (key, name, home));
                    } else {
                        self.lobby.push((key, name, home));
                    }
                }
                self.peers.get_mut(&cid).expect("spojení").role = Role::Lobby;
                self.broadcast_lobby();
            }
            Some(g) => {
                let airline = g
                    .campaign
                    .roster
                    .iter()
                    .find(|(_, k)| **k == key)
                    .map(|(a, _)| *a);
                match airline {
                    Some(airline) => self.enter_game(cid, airline),
                    None => {
                        self.peers.get_mut(&cid).expect("spojení").role = Role::Waiting;
                        self.send(cid, &HostMsg::Waiting);
                        self.notify_waiting();
                    }
                }
            }
        }
    }

    fn enter_game(&mut self, cid: u64, airline: AirlineId) {
        let local = self.peers[&cid].local;
        self.peers.get_mut(&cid).expect("spojení").role = Role::Game(airline);
        if let Some(g) = self.game.as_mut() {
            if !local {
                g.seen.insert(airline);
                g.missing.remove(&airline);
            }
        }
        let view = self.view_for(airline);
        let p = self.peers.get_mut(&cid).expect("spojení");
        p.tracker.reset(&view);
        self.send(cid, &HostMsg::Snapshot(Box::new(view)));
        self.update_auto_pause();
    }

    fn on_intent(&mut self, cid: u64, airline: AirlineId, id: u64, intent: Intent) {
        let local = self.peers[&cid].local;
        let Some(g) = self.game.as_mut() else { return };
        let mut actor = Actor::Airline(airline);
        match &intent {
            Intent::Pause(PauseReason::Player(p)) if *p == airline => {
                if g.pauses_used >= PLAYER_PAUSES_PER_SESSION {
                    self.send(
                        cid,
                        &HostMsg::Reject {
                            id,
                            reason: HostReject::PauseLimit,
                        },
                    );
                    return;
                }
            }
            // Hostitel smí ukončit cizí hráčskou pauzu.
            Intent::Resume(PauseReason::Player(p)) if *p != airline && local => {
                actor = Actor::System
            }
            // Systémové záměry od hráčů se odmítají (simulace vrátí NotPermitted).
            _ => {}
        }
        g.queue.push_back(Queued {
            conn: Some(cid),
            actor,
            id,
            intent,
        });
    }

    fn humans_online(&self) -> BTreeSet<AirlineId> {
        self.peers
            .values()
            .filter_map(|p| match p.role {
                Role::Game(a) => Some(a),
                _ => None,
            })
            .collect()
    }

    fn on_propose_speed(&mut self, airline: AirlineId, speed: Speed) {
        let needed = self.humans_online();
        let Some(g) = self.game.as_mut() else { return };
        if speed == Speed::X1 {
            // Zpomalit na 1× smí kdokoli hned.
            g.vote = None;
            if g.campaign.state.speed != Speed::X1 {
                g.queue.push_back(Queued {
                    conn: None,
                    actor: Actor::System,
                    id: 0,
                    intent: Intent::SetSpeed(Speed::X1),
                });
            }
            return;
        }
        if g.vote.is_some() || speed == g.campaign.state.speed {
            return;
        }
        let vote = Vote {
            speed,
            proposer: airline,
            yes: BTreeSet::from([airline]),
            needed,
            deadline: Instant::now() + VOTE_TIMEOUT,
        };
        g.vote = Some(vote);
        g.resolve_vote();
    }

    fn on_vote(&mut self, airline: AirlineId, yes: bool) {
        let Some(g) = self.game.as_mut() else { return };
        let Some(v) = g.vote.as_mut() else { return };
        if !v.needed.contains(&airline) {
            return;
        }
        if yes {
            v.yes.insert(airline);
            g.resolve_vote();
        } else {
            g.vote = None;
        }
    }

    // ------------------------------------------------------------ příkazy

    fn on_command(&mut self, cmd: HostCommand) {
        match cmd {
            HostCommand::SetRules(rules) => {
                if self.game.is_none() {
                    self.rules = rules;
                    self.broadcast_lobby();
                }
            }
            HostCommand::SetAi(ai) => {
                if self.game.is_none() {
                    self.ai = ai
                        .into_iter()
                        .take(aae_sim::engine::MAX_AIRLINES - self.lobby.len())
                        .collect();
                    self.broadcast_lobby();
                }
            }
            HostCommand::Start => self.start(),
            HostCommand::Assign { peer, airline } => self.assign(peer, airline),
            HostCommand::Refuse { peer } => {
                let cids: Vec<u64> = self
                    .peers
                    .iter()
                    .filter(|(_, p)| p.key == peer && p.role == Role::Waiting)
                    .map(|(c, _)| *c)
                    .collect();
                for cid in cids {
                    self.send(cid, &HostMsg::Refused(RefuseReason::Refused));
                    self.peers.remove(&cid);
                }
                self.notify_waiting();
            }
            HostCommand::TechnicalPause(on) => {
                if let Some(g) = self.game.as_mut() {
                    let intent = if on {
                        Intent::Pause(PauseReason::Technical)
                    } else {
                        Intent::Resume(PauseReason::Technical)
                    };
                    g.queue.push_back(Queued {
                        conn: None,
                        actor: Actor::System,
                        id: 0,
                        intent,
                    });
                }
            }
            HostCommand::ResumeAuto => {
                if let Some(g) = self.game.as_mut() {
                    g.missing.clear();
                    g.queue.push_back(Queued {
                        conn: None,
                        actor: Actor::System,
                        id: 0,
                        intent: Intent::Resume(PauseReason::Auto),
                    });
                }
            }
            HostCommand::Snapshot(reply) => {
                let _ = reply.send(self.game.as_ref().map(|g| g.campaign.clone()));
            }
            HostCommand::Shutdown => unreachable!("vyřízeno ve smyčce"),
        }
    }

    fn start(&mut self) {
        if self.game.is_some() || self.lobby.is_empty() {
            return;
        }
        let mut airlines = Vec::new();
        let mut used_homes: Vec<AirportId> = Vec::new();
        for (_, name, home) in &self.lobby {
            airlines.push(NewAirline {
                name: name.clone(),
                home: *home,
                controller: Controller::Human,
            });
            used_homes.push(*home);
        }
        for (i, strategy) in self.ai.iter().enumerate() {
            let home = AI_HOMES
                .iter()
                .filter_map(|c| self.world.airport_by_code(c))
                .find(|h| !used_homes.contains(h))
                .unwrap_or(used_homes[0]);
            used_homes.push(home);
            airlines.push(NewAirline {
                name: AI_NAMES[i % AI_NAMES.len()].to_string(),
                home,
                controller: Controller::Ai(*strategy),
            });
        }
        let state = match GameState::with_world(
            self.world.clone(),
            self.rules.clone(),
            self.cfg.seed,
            airlines,
        ) {
            Ok(s) => s,
            Err(e) => {
                let _ = self
                    .notices
                    .send(HostNotice::Error(format!("kampaň nelze založit: {e:?}")));
                return;
            }
        };
        let mut roster = BTreeMap::new();
        let mut names = BTreeMap::new();
        for (i, (key, name, _)) in self.lobby.iter().enumerate() {
            roster.insert(AirlineId(i as u8), *key);
            names.insert(AirlineId(i as u8), name.clone());
        }
        self.game = Some(Game::new(Campaign {
            state,
            roster,
            names,
            log: Vec::new(),
        }));
        let _ = self.notices.send(HostNotice::Started);
        let entries: Vec<(u64, PeerKey)> = self
            .peers
            .iter()
            .filter(|(_, p)| p.role == Role::Lobby)
            .map(|(c, p)| (*c, p.key))
            .collect();
        for (cid, key) in entries {
            let airline = self.game.as_ref().and_then(|g| {
                g.campaign
                    .roster
                    .iter()
                    .find(|(_, k)| **k == key)
                    .map(|(a, _)| *a)
            });
            if let Some(airline) = airline {
                self.enter_game(cid, airline);
            }
        }
    }

    fn assign(&mut self, peer: PeerKey, airline: AirlineId) {
        let Some(g) = self.game.as_mut() else { return };
        let Some(a) = g.campaign.state.airlines.get(&airline) else {
            return;
        };
        let was_ai = matches!(a.controller, Controller::Ai(_));
        g.campaign.roster.insert(airline, peer);
        if was_ai {
            g.queue.push_back(Queued {
                conn: None,
                actor: Actor::System,
                id: 0,
                intent: Intent::SetController {
                    airline,
                    controller: Controller::Human,
                },
            });
        }
        let cids: Vec<u64> = self
            .peers
            .iter()
            .filter(|(_, p)| p.key == peer && p.role == Role::Waiting)
            .map(|(c, _)| *c)
            .collect();
        for cid in cids {
            let name = self.peers[&cid].name.clone();
            if let Some(g) = self.game.as_mut() {
                g.campaign.names.insert(airline, name);
            }
            self.enter_game(cid, airline);
        }
        self.notify_waiting();
    }

    fn shutdown(&mut self) {
        let cids: Vec<u64> = self.peers.keys().copied().collect();
        for cid in cids {
            self.send(cid, &HostMsg::Ended(EndReason::HostQuit));
        }
        self.peers.clear();
    }

    // ---------------------------------------------------------------- tick

    fn on_tick(&mut self) {
        let now = Instant::now();
        let lost: Vec<u64> = self
            .peers
            .iter()
            .filter(|(_, p)| !p.local && now - p.last_seen > LOST_AFTER)
            .map(|(c, _)| *c)
            .collect();
        for cid in lost {
            self.drop_peer(cid);
        }
        if self.game.is_some() {
            self.tick_game(now);
        } else if now - self.last_lobby >= LOBBY_REFRESH {
            self.broadcast_lobby();
        }
        let idle: Vec<u64> = self
            .peers
            .iter()
            .filter(|(_, p)| now - p.last_sent >= HEARTBEAT)
            .map(|(c, _)| *c)
            .collect();
        for cid in idle {
            self.send(cid, &HostMsg::Heartbeat);
        }
    }

    fn tick_game(&mut self, now: Instant) {
        let g = self.game.as_mut().expect("hra");
        // Hráčská pauza nejvýš 5 minut, pak hra sama pokračuje.
        let expired: Vec<AirlineId> = g
            .pause_started
            .iter()
            .filter(|(_, t)| now - **t >= PLAYER_PAUSE_MAX)
            .map(|(a, _)| *a)
            .collect();
        for a in expired {
            g.pause_started.remove(&a);
            if g.campaign.state.pause.contains(&PauseReason::Player(a)) {
                g.queue.push_back(Queued {
                    conn: None,
                    actor: Actor::System,
                    id: 0,
                    intent: Intent::Resume(PauseReason::Player(a)),
                });
            }
        }
        if g.vote.as_ref().is_some_and(|v| now >= v.deadline) {
            g.vote = None;
        }

        let elapsed = now - g.last_tick;
        g.last_tick = now;
        let mut responses: Vec<(u64, HostMsg)> = Vec::new();
        // Záměry na aktuální hranici (i během pauzy, bez posunu času).
        g.apply_queue(&mut responses);
        if g.campaign.state.is_paused() || g.campaign.state.finished {
            g.acc_nanos = 0;
        } else {
            let sps = g.campaign.state.speed.steps_per_second() as u128;
            g.acc_nanos += elapsed.as_nanos() * sps;
            // gstack-shortcut(dec-2469d5ac): dohánění bez stropu, upgrade when hráči nahlásí přeskočené dny po zaseknutí hostitele
            let steps = g.acc_nanos / 1_000_000_000;
            g.acc_nanos %= 1_000_000_000;
            for _ in 0..steps {
                g.apply_queue(&mut responses);
                for (actor, intent) in g.campaign.state.ai_tick() {
                    g.apply_logged(actor, intent, None, 0, &mut responses);
                }
                match g.campaign.state.step() {
                    Ok(events) => g.record_events(events),
                    Err(_) => {
                        g.acc_nanos = 0;
                        break;
                    }
                }
            }
        }
        for (cid, msg) in responses {
            self.send(cid, &msg);
        }
        self.flush_events();
        if now - self.game.as_ref().expect("hra").last_delta >= DELTA_EVERY {
            self.game.as_mut().expect("hra").last_delta = now;
            self.send_deltas();
        }
    }

    fn flush_events(&mut self) {
        let events = std::mem::take(&mut self.game.as_mut().expect("hra").events);
        if events.is_empty() {
            return;
        }
        let targets: Vec<(u64, AirlineId)> = self
            .peers
            .iter()
            .filter_map(|(c, p)| match p.role {
                Role::Game(a) => Some((*c, a)),
                _ => None,
            })
            .collect();
        for (cid, airline) in targets {
            let mine: Vec<Event> = events
                .iter()
                .filter(|e| e.owner().is_none_or(|o| o == airline))
                .cloned()
                .collect();
            if !mine.is_empty() {
                self.send(cid, &HostMsg::Events(mine));
            }
        }
    }

    fn session_info(&self) -> SessionInfo {
        let g = self.game.as_ref().expect("hra");
        SessionInfo {
            pauses_left: PLAYER_PAUSES_PER_SESSION.saturating_sub(g.pauses_used),
            vote: g.vote.as_ref().map(|v| SpeedVote {
                speed: v.speed,
                proposer: v.proposer,
                yes: v.yes.iter().copied().collect(),
                needed: v.needed.iter().copied().collect(),
            }),
            presence: g.presence.iter().map(|(a, c)| (*a, *c)).collect(),
            online: self.humans_online().into_iter().collect(),
        }
    }

    fn view_for(&self, airline: AirlineId) -> crate::view::View {
        let info = self.session_info();
        build_view(
            &self.game.as_ref().expect("hra").campaign.state,
            airline,
            &info,
        )
    }

    fn send_deltas(&mut self) {
        let targets: Vec<(u64, AirlineId)> = self
            .peers
            .iter()
            .filter_map(|(c, p)| match p.role {
                Role::Game(a) => Some((*c, a)),
                _ => None,
            })
            .collect();
        for (cid, airline) in targets {
            let view = self.view_for(airline);
            let delta = self
                .peers
                .get_mut(&cid)
                .expect("spojení")
                .tracker
                .delta(&view);
            self.send(cid, &HostMsg::Delta(Box::new(delta)));
        }
    }

    /// Automatická pauza (R5): když jsou odpojení všichni vzdálení hráči,
    /// kteří v této relaci byli připojení (i ve hře dvou lidí). Skončí, až se
    /// všichni vrátí, nebo dřív příkazem hostitele.
    fn update_auto_pause(&mut self) {
        let online_remote: BTreeSet<AirlineId> = self
            .peers
            .values()
            .filter(|p| !p.local)
            .filter_map(|p| match p.role {
                Role::Game(a) => Some(a),
                _ => None,
            })
            .collect();
        let Some(g) = self.game.as_mut() else { return };
        let auto = g.campaign.state.pause.contains(&PauseReason::Auto);
        if !auto && !g.seen.is_empty() && online_remote.is_empty() {
            g.missing = g.seen.clone();
            g.queue.push_back(Queued {
                conn: None,
                actor: Actor::System,
                id: 0,
                intent: Intent::Pause(PauseReason::Auto),
            });
        } else if auto {
            g.missing.retain(|a| !online_remote.contains(a));
            if g.missing.is_empty() {
                g.queue.push_back(Queued {
                    conn: None,
                    actor: Actor::System,
                    id: 0,
                    intent: Intent::Resume(PauseReason::Auto),
                });
            }
        }
    }

    // --------------------------------------------------------------- lobby

    fn free_home(&self) -> AirportId {
        DEFAULT_HOMES
            .iter()
            .filter_map(|c| self.world.airport_by_code(c))
            .find(|h| !self.lobby.iter().any(|(_, _, home)| home == h))
            .unwrap_or(AirportId(0))
    }

    fn lobby_view(&self) -> LobbyView {
        let players = self
            .lobby
            .iter()
            .map(|(key, name, home)| {
                let peer = self.peers.values().find(|p| p.key == *key);
                LobbyPlayer {
                    key: *key,
                    name: name.clone(),
                    home: *home,
                    host: *key == self.cfg.host_key,
                    connected: peer.is_some(),
                    path: peer.map(|p| *p.path.borrow()).unwrap_or(PathInfo::UNKNOWN),
                    version: peer.map(|p| p.version.clone()).unwrap_or_default(),
                    airline: None,
                }
            })
            .collect();
        LobbyView {
            rules: self.rules.clone(),
            ai: self.ai.clone(),
            players,
            loaded: false,
            host_version: self.cfg.app_version.clone(),
        }
    }

    fn broadcast_lobby(&mut self) {
        self.last_lobby = Instant::now();
        let view = self.lobby_view();
        let cids: Vec<u64> = self
            .peers
            .iter()
            .filter(|(_, p)| p.role == Role::Lobby)
            .map(|(c, _)| *c)
            .collect();
        for cid in cids {
            self.send(cid, &HostMsg::Lobby(view.clone()));
        }
    }

    fn notify_waiting(&mut self) {
        let waiting: Vec<WaitingPeer> = self
            .peers
            .values()
            .filter(|p| p.role == Role::Waiting)
            .map(|p| WaitingPeer {
                key: p.key,
                name: p.name.clone(),
                version: p.version.clone(),
            })
            .collect();
        let _ = self.notices.send(HostNotice::Waiting(waiting));
    }
}

impl Game {
    fn new(campaign: Campaign) -> Game {
        let now = Instant::now();
        Game {
            seq: campaign.log.len() as u64,
            campaign,
            queue: VecDeque::new(),
            acc_nanos: 0,
            last_tick: now,
            last_delta: now,
            pauses_used: 0,
            pause_started: BTreeMap::new(),
            vote: None,
            presence: BTreeMap::new(),
            seen: BTreeSet::new(),
            missing: BTreeSet::new(),
            events: Vec::new(),
        }
    }

    fn apply_queue(&mut self, responses: &mut Vec<(u64, HostMsg)>) {
        while let Some(q) = self.queue.pop_front() {
            self.apply_logged(q.actor, q.intent, q.conn, q.id, responses);
        }
    }

    fn apply_logged(
        &mut self,
        actor: Actor,
        intent: Intent,
        conn: Option<u64>,
        id: u64,
        responses: &mut Vec<(u64, HostMsg)>,
    ) {
        let step = self.campaign.state.now;
        let result = self.campaign.state.apply(actor, intent.clone());
        self.seq += 1;
        self.campaign.log.push(LogEntry::Intent {
            step,
            seq: self.seq,
            actor,
            intent: intent.clone(),
            result: result.as_ref().map(|_| ()).map_err(Clone::clone),
        });
        match result {
            Ok(events) => {
                if let (Actor::Airline(a), Intent::Pause(PauseReason::Player(p))) = (actor, &intent)
                {
                    if a == *p {
                        self.pauses_used += 1;
                        self.pause_started.insert(a, Instant::now());
                    }
                }
                if matches!(intent, Intent::SetSpeed(_)) {
                    self.vote = None;
                }
                self.record_events(events);
                if let Some(cid) = conn {
                    responses.push((cid, HostMsg::Ack { id }));
                }
            }
            Err(reason) => {
                if let Some(cid) = conn {
                    responses.push((
                        cid,
                        HostMsg::Reject {
                            id,
                            reason: HostReject::Sim(reason),
                        },
                    ));
                }
            }
        }
    }

    fn record_events(&mut self, events: Vec<Event>) {
        for e in &events {
            if let Event::ContractAwarded {
                contract,
                winner,
                candidates,
            } = e
            {
                self.campaign.log.push(LogEntry::Lottery {
                    step: self.campaign.state.now,
                    contract: *contract,
                    winner: *winner,
                    candidates: candidates.clone(),
                });
            }
        }
        self.events.extend(events);
    }

    fn resolve_vote(&mut self) {
        let Some(v) = self.vote.as_ref() else { return };
        if v.needed.iter().all(|a| v.yes.contains(a)) {
            let speed = v.speed;
            self.vote = None;
            self.queue.push_back(Queued {
                conn: None,
                actor: Actor::System,
                id: 0,
                intent: Intent::SetSpeed(speed),
            });
        }
    }
}

fn clean_name(name: &str) -> String {
    let trimmed: String = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(32)
        .collect();
    if trimmed.is_empty() {
        "Hráč".to_string()
    } else {
        trimmed
    }
}
