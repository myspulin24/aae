//! Integrační testy hostitele a klientů nad sítí v paměti (R14): férovost
//! burzy při zpoždění, výpadky a automatická pauza, filtr stavu, kadence,
//! pauzy, hlasování o rychlosti, čekárna. Jeden test nad skutečným iroh.

use std::time::Duration;

use aae_net::PeerKey;
use aae_net::client::{ClientConfig, ClientHandle, ClientUpdate, spawn_client};
use aae_net::host::{Campaign, HostCommand, HostConfig, HostHandle, HostNotice, spawn_host};
use aae_net::protocol::{ClientMsg, HostReject, LobbyRequest, RefuseReason};
use aae_net::transport::{Connection, mem};
use aae_net::view::View;
use aae_sim::model::{Contract, ContractStatus};
use aae_sim::{
    AirlineId, CampaignRules, ContractId, Controller, Event, GameState, Intent, Money, NewAirline,
    PauseReason, Reject, Speed, Strategy,
};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

const VERSION: &str = "0.1.0";
const HOST: PeerKey = PeerKey([1; 32]);

fn key(n: u8) -> PeerKey {
    PeerKey([n; 32])
}

struct Net {
    listener: mpsc::UnboundedSender<Connection>,
    host: HostHandle,
}

fn start(loaded: Option<Campaign>) -> Net {
    let (tx, rx) = mpsc::unbounded_channel();
    let host = spawn_host(
        HostConfig {
            app_version: VERSION.into(),
            host_key: HOST,
            seed: 42,
        },
        rx,
        loaded,
    );
    Net { listener: tx, host }
}

impl Net {
    fn join_local(&self) -> ClientHandle {
        let (host_side, client_side) = mem::local_pair(HOST);
        self.listener.send(host_side).unwrap();
        spawn_client(
            client_side,
            ClientConfig {
                app_version: VERSION.into(),
                name: "Hostitel".into(),
            },
        )
    }

    fn join(&self, k: PeerKey, latency_ms: u64, version: &str) -> (ClientHandle, mem::Link) {
        let (client_side, host_side, link) = mem::pair(k, HOST, Duration::from_millis(latency_ms));
        self.listener.send(host_side).unwrap();
        let client = spawn_client(
            client_side,
            ClientConfig {
                app_version: version.into(),
                name: format!("Hráč {}", k.0[0]),
            },
        );
        (client, link)
    }

    fn cmd(&self, c: HostCommand) {
        self.host.commands.send(c).unwrap();
    }
}

async fn wait_for(c: &mut ClientHandle, pred: impl Fn(&ClientUpdate) -> bool) -> ClientUpdate {
    timeout(Duration::from_secs(900), async {
        loop {
            let u = c
                .updates
                .recv()
                .await
                .expect("klient skončil dřív, než přišla očekávaná zpráva");
            if pred(&u) {
                return u;
            }
        }
    })
    .await
    .expect("očekávaná zpráva nepřišla")
}

async fn wait_view(c: &mut ClientHandle, pred: impl Fn(&View) -> bool) -> View {
    match wait_for(c, |u| matches!(u, ClientUpdate::View(v) if pred(v))).await {
        ClientUpdate::View(v) => *v,
        _ => unreachable!(),
    }
}

/// Kampaň s lidskými hráči (klíče) a AI, bez náhodných událostí a burzy.
fn campaign(humans: &[PeerKey], ai: &[Strategy]) -> Campaign {
    let world = aae_sim::data::World::europe_v1().unwrap();
    let homes = ["PRG", "VIE", "MUC", "WAW", "BUD", "BER"];
    let mut airlines = Vec::new();
    for (i, _) in humans.iter().enumerate() {
        airlines.push(NewAirline {
            name: format!("Hráč {i}"),
            home: world.airport_by_code(homes[i]).unwrap(),
            controller: Controller::Human,
        });
    }
    for (j, s) in ai.iter().enumerate() {
        airlines.push(NewAirline {
            name: format!("AI {j}"),
            home: world.airport_by_code(homes[humans.len() + j]).unwrap(),
            controller: Controller::Ai(*s),
        });
    }
    let rules = CampaignRules {
        events: false,
        contracts_per_day: 0,
        auction_every_days: 0,
        ..CampaignRules::default()
    };
    let state = GameState::with_world(world, rules, 5, airlines).unwrap();
    Campaign {
        state,
        roster: humans
            .iter()
            .enumerate()
            .map(|(i, k)| (AirlineId(i as u8), *k))
            .collect(),
        names: Default::default(),
        log: Vec::new(),
    }
}

fn insert_contract(c: &mut Campaign) -> ContractId {
    let s = &mut c.state;
    let id = ContractId(500);
    let (from, to) = (
        s.world.airport_by_code("PRG").unwrap(),
        s.world.airport_by_code("PMI").unwrap(),
    );
    s.contracts.insert(
        id,
        Contract {
            id,
            from,
            to,
            pax: 60,
            day: 5,
            reward: Money::kc(500_000),
            offer_until: s.now + 240,
            window: None,
            status: ContractStatus::Offered,
        },
    );
    s.open_contracts.insert(id);
    id
}

// ----------------------------------------------------------------- lobby

#[tokio::test(start_paused = true)]
async fn lobby_start_and_filtered_snapshot() {
    let net = start(None);
    let mut host = net.join_local();
    wait_for(
        &mut host,
        |u| matches!(u, ClientUpdate::Lobby(l) if l.players.len() == 1 && l.players[0].host),
    )
    .await;
    let (mut b, _link) = net.join(key(2), 50, VERSION);
    wait_for(
        &mut b,
        |u| matches!(u, ClientUpdate::Lobby(l) if l.players.len() == 2),
    )
    .await;
    let bud = aae_sim::data::World::europe_v1()
        .unwrap()
        .airport_by_code("BUD")
        .unwrap();
    b.sender.send(ClientMsg::Lobby(LobbyRequest::SetProfile {
        name: "Petr".into(),
        home: bud,
    }));
    wait_for(&mut host, |u| matches!(u, ClientUpdate::Lobby(l) if l.players.iter().any(|p| p.name == "Petr" && p.home == bud))).await;

    net.cmd(HostCommand::SetAi(vec![Strategy::Hub]));
    net.cmd(HostCommand::Start);
    let bv = wait_view(&mut b, |_| true).await;
    assert_eq!(bv.me, AirlineId(1));
    assert_eq!(bv.my().name, "Petr");
    // Startovní kapitál minus nájem a režie prvního dne.
    assert!(bv.my().cash > Money::kc(24_000_000) && bv.my().cash < Money::kc(25_000_000));
    // Hostitelova aerolinka je pro B jen veřejná (R4).
    let rival = &bv.airlines[&AirlineId(0)];
    assert!(!rival.full);
    assert_eq!(rival.airline.cash, Money::ZERO);
    assert_eq!(bv.airlines.len(), 3, "2 lidé + 1 AI");
    let hv = wait_view(&mut host, |_| true).await;
    assert_eq!(hv.me, AirlineId(0));
}

#[tokio::test(start_paused = true)]
async fn version_mismatch_is_refused_with_direction() {
    let net = start(None);
    let (mut b, _link) = net.join(key(2), 10, "0.0.9");
    let u = wait_for(&mut b, |u| matches!(u, ClientUpdate::Refused(_))).await;
    assert_eq!(
        u,
        ClientUpdate::Refused(RefuseReason::VersionMismatch {
            host: VERSION.into(),
            client: "0.0.9".into()
        })
    );
}

// ------------------------------------------------------- záměry a burza

#[tokio::test(start_paused = true)]
async fn intents_are_acked_or_rejected() {
    let net = start(Some(campaign(&[HOST, key(2)], &[])));
    let _host = net.join_local();
    let (mut b, _link) = net.join(key(2), 30, VERSION);
    let v = wait_view(&mut b, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    let home = v.my().home;
    let vie = v.airlines[&AirlineId(0)].airline.home;
    let price = aae_sim::economy::reference_fare(1000);
    let ok = b.sender.intent(Intent::SetPrice {
        from: home,
        to: vie,
        price,
    });
    wait_for(&mut b, |u| *u == ClientUpdate::Ack(ok)).await;
    let bad = b.sender.intent(Intent::SetSpeed(Speed::X4));
    let u = wait_for(
        &mut b,
        |u| matches!(u, ClientUpdate::Reject(id, _) if *id == bad),
    )
    .await;
    assert_eq!(
        u,
        ClientUpdate::Reject(bad, HostReject::Sim(Reject::NotPermitted))
    );
}

#[tokio::test(start_paused = true)]
async fn slow_players_are_in_the_collection_window() {
    let mut c = campaign(&[HOST, key(2), key(3)], &[]);
    let contract = insert_contract(&mut c);
    let net = start(Some(c));
    let mut host = net.join_local();
    let (mut b, _l2) = net.join(key(2), 250, VERSION);
    let (mut cc, _l3) = net.join(key(3), 900, VERSION);
    wait_view(&mut host, |_| true).await;
    wait_view(&mut b, |_| true).await;
    wait_view(&mut cc, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    wait_view(&mut host, |v| v.header.pause.is_empty()).await;
    // Všichni klikli ve stejný okamžik; hostitel bez zpoždění, B +250 ms, C +900 ms.
    for client in [&host, &b, &cc] {
        client.sender.intent(Intent::TakeContract { contract });
    }
    let awarded = wait_for(&mut host, |u| {
        matches!(u, ClientUpdate::Events(e) if e.iter().any(|e| matches!(e, Event::ContractAwarded { .. })))
    })
    .await;
    let ClientUpdate::Events(events) = awarded else {
        unreachable!()
    };
    let candidates = events
        .iter()
        .find_map(|e| match e {
            Event::ContractAwarded { candidates, .. } => Some(candidates.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(candidates, vec![AirlineId(0), AirlineId(1), AirlineId(2)]);
}

// --------------------------------------------------------------- výpadky

#[tokio::test(start_paused = true)]
async fn losing_the_only_client_pauses_and_return_resumes() {
    let net = start(Some(campaign(&[HOST, key(2)], &[])));
    let mut host = net.join_local();
    let (mut b, link) = net.join(key(2), 40, VERSION);
    wait_view(&mut b, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    wait_view(&mut host, |v| v.header.pause.is_empty()).await;
    link.cut();
    let paused = wait_view(&mut host, |v| v.header.pause.contains(&PauseReason::Auto)).await;
    assert!(paused.header.online == vec![AirlineId(0)]);
    wait_for(&mut b, |u| *u == ClientUpdate::Lost).await;
    // Návrat se stejným klíčem: automatická pauza skončí.
    let (mut b2, _l) = net.join(key(2), 40, VERSION);
    wait_view(&mut b2, |_| true).await;
    wait_view(&mut host, |v| !v.header.pause.contains(&PauseReason::Auto)).await;
}

#[tokio::test(start_paused = true)]
async fn losing_one_of_two_clients_does_not_pause() {
    let net = start(Some(campaign(&[HOST, key(2), key(3)], &[])));
    let mut host = net.join_local();
    let (mut b, link_b) = net.join(key(2), 40, VERSION);
    let (mut c, _link_c) = net.join(key(3), 40, VERSION);
    wait_view(&mut b, |_| true).await;
    wait_view(&mut c, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    wait_view(&mut host, |v| v.header.pause.is_empty()).await;
    link_b.cut();
    wait_view(&mut host, |v| v.header.online.len() == 2).await;
    sleep(Duration::from_secs(5)).await;
    let v = wait_view(&mut host, |_| true).await;
    assert!(
        v.header.pause.is_empty(),
        "jeden z více klientů vypadl, hra běží dál"
    );
}

// -------------------------------------------------- kadence a soukromí

#[tokio::test(start_paused = true)]
async fn deltas_at_most_ten_per_second() {
    let net = start(Some(campaign(&[HOST, key(2)], &[])));
    let _host = net.join_local();
    let (mut b, _link) = net.join(key(2), 20, VERSION);
    wait_view(&mut b, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    sleep(Duration::from_millis(300)).await;
    while b.updates.try_recv().is_ok() {}
    sleep(Duration::from_secs(3)).await;
    let mut views = 0;
    while let Ok(u) = b.updates.try_recv() {
        if matches!(u, ClientUpdate::View(_)) {
            views += 1;
        }
    }
    assert!((20..=31).contains(&views), "{views} pohledů za 3 s");
}

#[tokio::test(start_paused = true)]
async fn rival_flights_stay_private() {
    let mut c = campaign(&[HOST, key(2)], &[]);
    {
        let s = &mut c.state;
        let a = AirlineId(0);
        let plane = *s.airlines[&a].fleet.keys().next().unwrap();
        let t = s.airlines[&a].fleet[&plane].type_id;
        let (prg, vie) = (
            s.world.airport_by_code("PRG").unwrap(),
            s.world.airport_by_code("VIE").unwrap(),
        );
        let legs = aae_sim::schedule::shuttle(&s.world, t, prg, vie, 6 * 60, 23 * 60);
        s.airlines
            .get_mut(&a)
            .unwrap()
            .fleet
            .get_mut(&plane)
            .unwrap()
            .schedule = legs;
    }
    let net = start(Some(c));
    let mut host = net.join_local();
    let (mut b, _link) = net.join(key(2), 20, VERSION);
    wait_view(&mut b, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    // Dva herní dny (90 s každý při 1×).
    wait_for(&mut host, |u| {
        matches!(u, ClientUpdate::Events(e) if e.iter().any(|e| matches!(e, Event::FlightDeparted { airline, .. } if *airline == AirlineId(0))))
    })
    .await;
    let mut leaked = false;
    while let Ok(u) = b.updates.try_recv() {
        if let ClientUpdate::Events(e) = u {
            leaked |= e.iter().any(
                |e| matches!(e, Event::FlightDeparted { airline, .. } if *airline == AirlineId(0)),
            );
        }
    }
    assert!(
        !leaked,
        "lety hostitele (obsazenost) nesmí dorazit k soupeři"
    );
}

// ----------------------------------------------------- pauzy a rychlost

#[tokio::test(start_paused = true)]
async fn player_pauses_are_limited_to_three_per_session() {
    let net = start(Some(campaign(&[HOST, key(2)], &[])));
    let _host = net.join_local();
    let (mut b, _link) = net.join(key(2), 20, VERSION);
    wait_view(&mut b, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    let me = PauseReason::Player(AirlineId(1));
    for _ in 0..3 {
        let p = b.sender.intent(Intent::Pause(me));
        wait_for(&mut b, |u| *u == ClientUpdate::Ack(p)).await;
        let r = b.sender.intent(Intent::Resume(me));
        wait_for(&mut b, |u| *u == ClientUpdate::Ack(r)).await;
    }
    let fourth = b.sender.intent(Intent::Pause(me));
    let u = wait_for(
        &mut b,
        |u| matches!(u, ClientUpdate::Reject(id, _) | ClientUpdate::Ack(id) if *id == fourth),
    )
    .await;
    assert_eq!(u, ClientUpdate::Reject(fourth, HostReject::PauseLimit));
}

#[tokio::test(start_paused = true)]
async fn player_pause_ends_after_five_minutes() {
    let net = start(Some(campaign(&[HOST, key(2)], &[])));
    let _host = net.join_local();
    let (mut b, _link) = net.join(key(2), 20, VERSION);
    wait_view(&mut b, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    let p = b
        .sender
        .intent(Intent::Pause(PauseReason::Player(AirlineId(1))));
    wait_for(&mut b, |u| *u == ClientUpdate::Ack(p)).await;
    wait_view(&mut b, |v| !v.header.pause.is_empty()).await;
    sleep(Duration::from_secs(301)).await;
    wait_view(&mut b, |v| v.header.pause.is_empty()).await;
}

#[tokio::test(start_paused = true)]
async fn speed_change_needs_everyone_slowing_down_does_not() {
    let net = start(Some(campaign(&[HOST, key(2)], &[])));
    let mut host = net.join_local();
    let (mut b, _link) = net.join(key(2), 20, VERSION);
    wait_view(&mut b, |_| true).await;
    net.cmd(HostCommand::TechnicalPause(false));
    host.sender.send(ClientMsg::ProposeSpeed(Speed::X2));
    let v = wait_view(&mut b, |v| v.header.vote.is_some()).await;
    assert_eq!(v.header.speed, Speed::X1);
    b.sender.send(ClientMsg::Vote { yes: true });
    wait_view(&mut host, |v| {
        v.header.speed == Speed::X2 && v.header.vote.is_none()
    })
    .await;
    b.sender.send(ClientMsg::ProposeSpeed(Speed::X1));
    wait_view(&mut host, |v| v.header.speed == Speed::X1).await;
}

// --------------------------------------------------------------- čekárna

#[tokio::test(start_paused = true)]
async fn unknown_key_waits_and_can_take_over_an_ai_airline() {
    let mut net = start(Some(campaign(&[HOST], &[Strategy::Levna])));
    let _host = net.join_local();
    let (mut d, _link) = net.join(key(9), 20, VERSION);
    wait_for(&mut d, |u| *u == ClientUpdate::Waiting).await;
    let notice = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(HostNotice::Waiting(w)) = net.host.notices.recv().await {
                if !w.is_empty() {
                    return w;
                }
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(notice[0].key, key(9));
    net.cmd(HostCommand::Assign {
        peer: key(9),
        airline: AirlineId(1),
    });
    let v = wait_view(&mut d, |v| v.my().controller == Controller::Human).await;
    assert_eq!(v.me, AirlineId(1));
}

// -------------------------------------------------------- skutečný iroh

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn iroh_loopback_lobby() {
    use aae_net::transport::iroh_net::IrohNode;
    let host_node = IrohNode::bind_local([7; 32]).await.unwrap();
    let client_node = IrohNode::bind_local([8; 32]).await.unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let host = spawn_host(
        HostConfig {
            app_version: VERSION.into(),
            host_key: host_node.key(),
            seed: 1,
        },
        rx,
        None,
    );
    let mut incoming = host_node.listen();
    tokio::spawn(async move {
        while let Some(c) = incoming.recv().await {
            if tx.send(c).is_err() {
                break;
            }
        }
    });
    let conn = client_node.connect(&host_node.ticket_now()).await.unwrap();
    assert_eq!(conn.peer, host_node.key());
    let mut client = spawn_client(
        conn,
        ClientConfig {
            app_version: VERSION.into(),
            name: "Kamarád".into(),
        },
    );
    let lobby = wait_for(&mut client, |u| matches!(u, ClientUpdate::Lobby(_))).await;
    let ClientUpdate::Lobby(l) = lobby else {
        unreachable!()
    };
    assert_eq!(l.players.len(), 1);
    assert_eq!(l.players[0].key, client_node.key());
    let _ = host.commands.send(HostCommand::Shutdown);
    client_node.close().await;
    host_node.close().await;
}
