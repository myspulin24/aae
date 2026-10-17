//! Typy pro okno (JSON přes IPC). Typy pro TypeScript generuje ts-rs
//! do `src/api/bindings` (R13). Peníze jsou celé koruny, čas herní minuty.

use std::collections::BTreeSet;

use aae_net::PeerKey;
use aae_net::protocol::{LobbyPlayer, LobbyView};
use aae_net::transport::{PathInfo, PathKind};
use aae_net::view::View;
use aae_sim::data::World;
use aae_sim::economy::{price_bounds, reference_fare};
use aae_sim::engine::{firm_value, loan_limit, min_bid};
use aae_sim::model::{
    Aircraft, AirlineId, AirportId, AuctionStatus, ContractStatus, Disruption, Leg, Ownership,
    PauseReason, Route, RouteStats,
};
use aae_sim::schedule::block_minutes;
use aae_sim::time::{day_of, minute_of_day};
use aae_sim::{Controller, Money, Speed, Strategy};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::texts;

pub fn kc(m: Money) -> f64 {
    m.whole_kc() as f64
}

pub fn money(kc: f64) -> Money {
    Money::kc(kc.round() as i64)
}

pub fn hex(key: &PeerKey) -> String {
    key.0.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).ok()?;
        out[i] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

// ------------------------------------------------------------------ svět

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    pub name: String,
    pub world: WorldDto,
    pub campaigns_dir: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorldDto {
    pub airports: Vec<AirportDto>,
    pub aircraft_types: Vec<AircraftTypeDto>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AirportDto {
    pub id: u16,
    pub code: String,
    pub city: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
    pub population_k: u32,
    pub slot_constrained: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AircraftTypeDto {
    pub id: u8,
    pub code: String,
    pub name: String,
    pub seats: u32,
    pub range_km: u32,
    pub speed_kmh: u32,
    pub turnaround_min: u32,
    pub lease_per_day: f64,
    pub deposit: f64,
    pub price: f64,
    pub crew_per_hour: f64,
}

pub fn world_dto(world: &World) -> WorldDto {
    WorldDto {
        airports: world
            .airport_ids()
            .map(|id| {
                let a = world.airport(id);
                AirportDto {
                    id: id.0,
                    code: a.code.clone(),
                    city: a.city.clone(),
                    country: a.country.clone(),
                    lat: a.lat_e4 as f64 / 10_000.0,
                    lon: a.lon_e4 as f64 / 10_000.0,
                    population_k: a.population_k,
                    slot_constrained: a.slot_constrained,
                }
            })
            .collect(),
        aircraft_types: world
            .aircraft_types
            .iter()
            .enumerate()
            .map(|(i, t)| AircraftTypeDto {
                id: i as u8,
                code: t.code.clone(),
                name: t.name.clone(),
                seats: t.seats,
                range_km: t.range_km,
                speed_kmh: t.speed_kmh,
                turnaround_min: t.turnaround_min,
                lease_per_day: kc(t.lease_per_day),
                deposit: kc(t.lease_per_day.times(aae_sim::economy::LEASE_DEPOSIT_DAYS)),
                price: kc(t.price),
                crew_per_hour: kc(t.crew_per_hour),
            })
            .collect(),
    }
}

// ------------------------------------------------------------------ relace

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Phase {
    Menu,
    Connecting,
    Lobby,
    Waiting,
    Game,
    Reconnecting,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StatusDto {
    pub phase: Phase,
    pub host: bool,
    /// Pozvánka k rozeslání (jen hostitel).
    pub invitation: Option<String>,
    pub message: Option<String>,
    pub path: PathDto,
    pub host_version: Option<String>,
    pub campaign_name: Option<String>,
    /// Kdy se kampaň naposledy uložila (text pro hostitele).
    pub saved: Option<String>,
}

impl StatusDto {
    pub fn menu() -> StatusDto {
        StatusDto {
            phase: Phase::Menu,
            host: false,
            invitation: None,
            message: None,
            path: PathDto::from(PathInfo::UNKNOWN),
            host_version: None,
            campaign_name: None,
            saved: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PathKindDto {
    Local,
    Direct,
    Relay,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PathDto {
    pub kind: PathKindDto,
    pub rtt_ms: u32,
}

impl From<PathInfo> for PathDto {
    fn from(p: PathInfo) -> PathDto {
        PathDto {
            kind: match p.kind {
                PathKind::Local => PathKindDto::Local,
                PathKind::Direct => PathKindDto::Direct,
                PathKind::Relay => PathKindDto::Relay,
                PathKind::Unknown => PathKindDto::Unknown,
            },
            rtt_ms: p.rtt_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum StrategyDto {
    Levna,
    Premiova,
    Hub,
    Charter,
}

impl From<Strategy> for StrategyDto {
    fn from(s: Strategy) -> StrategyDto {
        match s {
            Strategy::Levna => StrategyDto::Levna,
            Strategy::Premiova => StrategyDto::Premiova,
            Strategy::Hub => StrategyDto::Hub,
            Strategy::Charter => StrategyDto::Charter,
        }
    }
}

impl From<StrategyDto> for Strategy {
    fn from(s: StrategyDto) -> Strategy {
        match s {
            StrategyDto::Levna => Strategy::Levna,
            StrategyDto::Premiova => Strategy::Premiova,
            StrategyDto::Hub => Strategy::Hub,
            StrategyDto::Charter => Strategy::Charter,
        }
    }
}

pub fn speed_of(n: u8) -> Option<Speed> {
    match n {
        1 => Some(Speed::X1),
        2 => Some(Speed::X2),
        4 => Some(Speed::X4),
        _ => None,
    }
}

// ------------------------------------------------------------------ lobby

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RulesDto {
    pub duration_days: u32,
    pub starting_capital: f64,
    pub initial_speed: u8,
    pub events: bool,
    pub starting_fleet: String,
}

#[derive(Clone, Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RulesInput {
    pub duration_days: u32,
    pub starting_capital: f64,
    pub initial_speed: u8,
    pub events: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LobbyDto {
    pub rules: RulesDto,
    pub ai: Vec<StrategyDto>,
    pub players: Vec<LobbyPlayerDto>,
    pub loaded: bool,
    pub host_version: String,
    pub max_airlines: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LobbyPlayerDto {
    pub key: String,
    pub name: String,
    pub home: u16,
    pub host: bool,
    pub connected: bool,
    pub path: PathDto,
    pub version: String,
    pub airline: Option<u8>,
    pub me: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WaitingPeerDto {
    pub key: String,
    pub name: String,
    pub version: String,
}

pub fn lobby_dto(world: &World, l: &LobbyView, me: &PeerKey) -> LobbyDto {
    let fleet = l
        .rules
        .starting_fleet
        .iter()
        .map(|(code, n)| {
            let name = world
                .aircraft_type_by_code(code)
                .map(|t| world.aircraft_type(t).name.clone())
                .unwrap_or_else(|| code.clone());
            format!("{n}× {name}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    LobbyDto {
        rules: RulesDto {
            duration_days: l.rules.duration_days as u32,
            starting_capital: kc(l.rules.starting_capital),
            initial_speed: l.rules.initial_speed.multiplier() as u8,
            events: l.rules.events,
            starting_fleet: fleet,
        },
        ai: l.ai.iter().map(|s| (*s).into()).collect(),
        players: l.players.iter().map(|p| lobby_player(p, me)).collect(),
        loaded: l.loaded,
        host_version: l.host_version.clone(),
        max_airlines: aae_sim::engine::MAX_AIRLINES as u32,
    }
}

fn lobby_player(p: &LobbyPlayer, me: &PeerKey) -> LobbyPlayerDto {
    LobbyPlayerDto {
        key: hex(&p.key),
        name: p.name.clone(),
        home: p.home.0,
        host: p.host,
        connected: p.connected,
        path: p.path.into(),
        version: p.version.clone(),
        airline: p.airline.map(|a| a.0),
        me: p.key == *me,
    }
}

// ------------------------------------------------------------------ hra

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameDto {
    pub me: u8,
    pub clock: ClockDto,
    pub vote: Option<VoteDto>,
    /// Cena paliva v Kč za litr.
    pub fuel_per_l: f64,
    /// Aerolinky v pořadí podle hodnoty firmy.
    pub airlines: Vec<AirlineDto>,
    pub my: MyAirlineDto,
    pub contracts: Vec<ContractDto>,
    pub auctions: Vec<AuctionDto>,
    pub disruptions: Vec<DisruptionDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClockDto {
    pub now: u32,
    pub day: u32,
    pub minute: u32,
    pub date: String,
    pub speed: u8,
    pub paused: bool,
    pub pauses: Vec<PauseDto>,
    pub my_pause: bool,
    pub pauses_left: u8,
    pub finished: bool,
    pub duration_days: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PauseKind {
    Player,
    Technical,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PauseDto {
    pub kind: PauseKind,
    pub airline: Option<u8>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VoteDto {
    pub speed: u8,
    pub proposer: u8,
    pub yes: Vec<u8>,
    pub needed: Vec<u8>,
    /// Hráč ještě nehlasoval a jeho hlas je potřeba.
    pub waiting_for_me: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AirlineDto {
    pub id: u8,
    pub name: String,
    pub home: u16,
    pub human: bool,
    pub strategy: Option<StrategyDto>,
    pub online: bool,
    pub me: bool,
    pub rank: u32,
    pub fleet: Vec<FleetCountDto>,
    pub network: Vec<RouteDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FleetCountDto {
    pub model: u8,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RouteDto {
    pub from: u16,
    pub to: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MyAirlineDto {
    pub name: String,
    pub home: u16,
    pub cash: f64,
    pub loans: f64,
    pub loan_limit: f64,
    pub firm_value: f64,
    pub service: u8,
    pub negative_days: u32,
    pub bankruptcy_days: u32,
    pub generation: u32,
    pub fleet: Vec<AircraftDto>,
    pub routes: Vec<MyRouteDto>,
    pub slots: Vec<SlotDto>,
    pub stats: StatsDto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OwnershipDto {
    Leased,
    Owned,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AircraftDto {
    pub id: u32,
    pub model: u8,
    pub ownership: OwnershipDto,
    /// Záloha (pronájem) nebo účetní hodnota (vlastní).
    pub value: f64,
    /// Plán od zítřka.
    pub schedule: Vec<LegDto>,
    /// Plán platný dnes.
    pub today: Vec<LegDto>,
    pub charters: Vec<CharterDto>,
    /// Kde letadlo stojí, když zrovna neletí a nemá dnes lety.
    pub base: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LegDto {
    pub from: u16,
    pub to: u16,
    pub dep: u32,
    pub arr: u32,
    pub distance_km: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LegInput {
    pub from: u16,
    pub to: u16,
    pub dep: u32,
}

impl From<LegInput> for Leg {
    fn from(l: LegInput) -> Leg {
        Leg {
            from: AirportId(l.from),
            to: AirportId(l.to),
            dep_min: l.dep,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CharterDto {
    pub day: u32,
    pub contract: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MyRouteDto {
    pub from: u16,
    pub to: u16,
    pub distance_km: u32,
    pub served: bool,
    pub price: f64,
    pub reference: f64,
    pub min: f64,
    pub max: f64,
    pub today: Option<RouteStatsDto>,
    pub yesterday: Option<RouteStatsDto>,
    pub rivals: Vec<RivalPriceDto>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RouteStatsDto {
    pub pax: u32,
    pub seats: u32,
    pub revenue: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RivalPriceDto {
    pub airline: u8,
    pub price: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SlotDto {
    pub airport: u16,
    pub owned: u32,
    pub used: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StatsDto {
    pub revenue_today: f64,
    pub cost_today: f64,
    pub revenue_yesterday: f64,
    pub cost_yesterday: f64,
    pub total_pax: f64,
    pub total_seats: f64,
    pub flights: f64,
    pub cancelled_flights: f64,
    pub charters_flown: u32,
    pub contracts_won: u32,
    pub slots_won: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ContractStatusDto {
    Offered,
    Awarded,
    Assigned,
    Done,
    Expired,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContractDto {
    pub id: u32,
    pub from: u16,
    pub to: u16,
    pub pax: u32,
    pub distance_km: u32,
    pub day: u32,
    pub date: String,
    pub reward: f64,
    pub offer_until: u32,
    pub status: ContractStatusDto,
    pub airline: Option<u8>,
    pub aircraft: Option<u32>,
    /// Konec sběrného okna (herní minuta), když už se někdo přihlásil.
    pub window_due: Option<u32>,
    pub candidates: Vec<u8>,
    /// Kdo si zakázku právě prohlíží.
    pub watching: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AuctionStatusDto {
    Open,
    Won,
    NoBids,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AuctionDto {
    pub id: u32,
    pub airport: u16,
    pub end_due: u32,
    pub effective_end: u32,
    pub protected: bool,
    pub high_bidder: Option<u8>,
    pub high_bid: Option<f64>,
    pub min_bid: f64,
    pub status: AuctionStatusDto,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DisruptionDto {
    pub day: u32,
    pub date: String,
    pub text: String,
    pub airport: Option<u16>,
    pub country: Option<String>,
    pub from_min: Option<u32>,
    pub to_min: Option<u32>,
}

fn legs(world: &World, model: aae_sim::model::AircraftTypeId, legs: &[Leg]) -> Vec<LegDto> {
    legs.iter()
        .map(|l| LegDto {
            from: l.from.0,
            to: l.to.0,
            dep: l.dep_min,
            arr: l.dep_min + block_minutes(world, model, l.from, l.to),
            distance_km: world.distance(l.from, l.to),
        })
        .collect()
}

fn aircraft(world: &World, home: AirportId, p: &Aircraft) -> AircraftDto {
    let (ownership, value) = match p.ownership {
        Ownership::Leased { deposit } => (OwnershipDto::Leased, kc(deposit)),
        Ownership::Owned { book_value } => (OwnershipDto::Owned, kc(book_value)),
    };
    let base = p
        .schedule_today
        .first()
        .or(p.schedule.first())
        .map(|l| l.from)
        .unwrap_or(home);
    AircraftDto {
        id: p.id.0,
        model: p.type_id.0,
        ownership,
        value,
        schedule: legs(world, p.type_id, &p.schedule),
        today: legs(world, p.type_id, &p.schedule_today),
        charters: p
            .charters
            .iter()
            .map(|(d, c)| CharterDto {
                day: *d as u32,
                contract: c.0,
            })
            .collect(),
        base: base.0,
    }
}

fn route_stats(s: Option<&RouteStats>) -> Option<RouteStatsDto> {
    s.map(|s| RouteStatsDto {
        pax: s.pax,
        seats: s.seats,
        revenue: kc(s.revenue),
    })
}

pub fn game_dto(world: &World, view: &View) -> GameDto {
    let me = view.me;
    let h = &view.header;
    let mine = &view.airlines[&me];
    let my = &mine.airline;

    let clock = ClockDto {
        now: h.now as u32,
        day: day_of(h.now) as u32,
        minute: minute_of_day(h.now) as u32,
        date: texts::date(day_of(h.now)),
        speed: h.speed.multiplier() as u8,
        paused: !h.pause.is_empty(),
        pauses: h
            .pause
            .iter()
            .map(|p| PauseDto {
                kind: match p {
                    PauseReason::Player(_) => PauseKind::Player,
                    PauseReason::Technical => PauseKind::Technical,
                    PauseReason::Auto => PauseKind::Auto,
                },
                airline: match p {
                    PauseReason::Player(a) => Some(a.0),
                    _ => None,
                },
                text: texts::pause_reason(Some(view), p),
            })
            .collect(),
        my_pause: h.pause.contains(&PauseReason::Player(me)),
        pauses_left: h.pauses_left,
        finished: h.finished,
        duration_days: view.rules.duration_days as u32,
    };

    let vote = h.vote.as_ref().map(|v| VoteDto {
        speed: v.speed.multiplier() as u8,
        proposer: v.proposer.0,
        yes: v.yes.iter().map(|a| a.0).collect(),
        needed: v.needed.iter().map(|a| a.0).collect(),
        waiting_for_me: v.needed.contains(&me) && !v.yes.contains(&me),
    });

    let airlines = h
        .ranking
        .iter()
        .enumerate()
        .filter_map(|(rank, id)| view.airlines.get(id).map(|a| (rank, a)))
        .map(|(rank, av)| {
            let a = &av.airline;
            let mut counts: Vec<FleetCountDto> = Vec::new();
            for p in a.fleet.values() {
                match counts.iter_mut().find(|c| c.model == p.type_id.0) {
                    Some(c) => c.count += 1,
                    None => counts.push(FleetCountDto {
                        model: p.type_id.0,
                        count: 1,
                    }),
                }
            }
            counts.sort_by_key(|c| c.model);
            AirlineDto {
                id: a.id.0,
                name: a.name.clone(),
                home: a.home.0,
                human: a.controller == Controller::Human,
                strategy: match a.controller {
                    Controller::Ai(s) => Some(s.into()),
                    Controller::Human => None,
                },
                online: h.online.contains(&a.id),
                me: a.id == me,
                rank: rank as u32 + 1,
                fleet: counts,
                network: av
                    .network
                    .iter()
                    .map(|(f, t)| RouteDto { from: f.0, to: t.0 })
                    .collect(),
            }
        })
        .collect();

    // Linky hráče: obsluhované, s nastavenou cenou nebo se včerejšími čísly.
    let mut routes: BTreeSet<Route> = mine.network.iter().copied().collect();
    routes.extend(my.prices.keys().copied());
    routes.extend(my.stats.yesterday.keys().copied());
    routes.extend(my.stats.today.keys().copied());
    let my_routes = routes
        .into_iter()
        .filter(|(f, t)| world.is_valid_airport(*f) && world.is_valid_airport(*t))
        .map(|route| {
            let distance = world.distance(route.0, route.1);
            let reference = reference_fare(distance);
            let (min, max) = price_bounds(distance);
            let rivals = view
                .airlines
                .values()
                .filter(|o| o.airline.id != me && o.network.contains(&route))
                .map(|o| RivalPriceDto {
                    airline: o.airline.id.0,
                    price: kc(o.airline.prices.get(&route).copied().unwrap_or(reference)),
                })
                .collect();
            MyRouteDto {
                from: route.0.0,
                to: route.1.0,
                distance_km: distance,
                served: mine.network.contains(&route),
                price: kc(my.prices.get(&route).copied().unwrap_or(reference)),
                reference: kc(reference),
                min: kc(min),
                max: kc(max),
                today: route_stats(my.stats.today.get(&route)),
                yesterday: route_stats(my.stats.yesterday.get(&route)),
                rivals,
            }
        })
        .collect();

    let slots = my
        .slots
        .iter()
        .map(|(airport, owned)| SlotDto {
            airport: airport.0,
            owned: *owned,
            used: my
                .fleet
                .values()
                .map(|p| p.schedule.iter().filter(|l| l.from == *airport).count() as u32)
                .sum(),
        })
        .collect();

    let s = &my.stats;
    let my_dto = MyAirlineDto {
        name: my.name.clone(),
        home: my.home.0,
        cash: kc(my.cash),
        loans: kc(my.loans),
        loan_limit: kc(loan_limit(my)),
        firm_value: kc(firm_value(my)),
        service: my.service,
        negative_days: my.negative_days,
        bankruptcy_days: view.rules.bankruptcy_days,
        generation: my.generation,
        fleet: my
            .fleet
            .values()
            .map(|p| aircraft(world, my.home, p))
            .collect(),
        routes: my_routes,
        slots,
        stats: StatsDto {
            revenue_today: kc(s.revenue_today),
            cost_today: kc(s.cost_today),
            revenue_yesterday: kc(s.revenue_yesterday),
            cost_yesterday: kc(s.cost_yesterday),
            total_pax: s.total_pax as f64,
            total_seats: s.total_seats as f64,
            flights: s.flights as f64,
            cancelled_flights: s.cancelled_flights as f64,
            charters_flown: s.charters_flown,
            contracts_won: s.contracts_won,
            slots_won: s.slots_won,
        },
    };

    let contracts = view
        .contracts
        .iter()
        .map(|c| {
            let (status, airline, aircraft) = match c.status {
                ContractStatus::Offered => (ContractStatusDto::Offered, None, None),
                ContractStatus::Awarded(a) => (ContractStatusDto::Awarded, Some(a.0), None),
                ContractStatus::Assigned(a, p) => {
                    (ContractStatusDto::Assigned, Some(a.0), Some(p.0))
                }
                ContractStatus::Done(a) => (ContractStatusDto::Done, Some(a.0), None),
                ContractStatus::Expired => (ContractStatusDto::Expired, None, None),
                ContractStatus::Failed(a) => (ContractStatusDto::Failed, Some(a.0), None),
            };
            ContractDto {
                id: c.id.0,
                from: c.from.0,
                to: c.to.0,
                pax: c.pax,
                distance_km: world.distance(c.from, c.to),
                day: c.day as u32,
                date: texts::date(c.day),
                reward: kc(c.reward),
                offer_until: c.offer_until as u32,
                status,
                airline,
                aircraft,
                window_due: c.window.as_ref().map(|w| w.due as u32),
                candidates: c
                    .window
                    .as_ref()
                    .map(|w| w.candidates.iter().map(|a| a.0).collect())
                    .unwrap_or_default(),
                watching: h
                    .presence
                    .iter()
                    .filter(|(_, id)| *id == c.id)
                    .map(|(a, _)| a.0)
                    .collect(),
            }
        })
        .collect();

    let auctions = view
        .auctions
        .iter()
        .map(|a| AuctionDto {
            id: a.id.0,
            airport: a.airport.0,
            end_due: a.end_due as u32,
            effective_end: a.effective_end() as u32,
            protected: a.protection_due.is_some_and(|p| p > a.end_due),
            high_bidder: a.high_bid.map(|(b, _)| b.0),
            high_bid: a.high_bid.map(|(_, m)| kc(m)),
            min_bid: kc(min_bid(&view.rules, a)),
            status: match a.status {
                AuctionStatus::Open => AuctionStatusDto::Open,
                AuctionStatus::Won(..) => AuctionStatusDto::Won,
                AuctionStatus::NoBids => AuctionStatusDto::NoBids,
            },
        })
        .collect();

    let disruptions = view
        .disruptions
        .iter()
        .map(|d| match d {
            Disruption::Strike { day, country } => DisruptionDto {
                day: *day as u32,
                date: texts::date(*day),
                text: format!("Stávka řízení letového provozu ({country})"),
                airport: None,
                country: Some(country.clone()),
                from_min: None,
                to_min: None,
            },
            Disruption::Weather {
                day,
                airport,
                from_min,
                to_min,
            } => DisruptionDto {
                day: *day as u32,
                date: texts::date(*day),
                text: format!(
                    "Počasí: {} zavřeno {}–{}",
                    world.airport(*airport).code,
                    texts::clock(*from_min as u64),
                    texts::clock((*to_min).min(1440) as u64)
                ),
                airport: Some(airport.0),
                country: None,
                from_min: Some(*from_min),
                to_min: Some(*to_min),
            },
        })
        .collect();

    GameDto {
        me: me.0,
        clock,
        vote,
        fuel_per_l: h.fuel_haler_per_l as f64 / 100.0,
        airlines,
        my: my_dto,
        contracts,
        auctions,
        disruptions,
    }
}

// ------------------------------------------------------------------ zprávy a odpovědi

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Tone {
    Info,
    Good,
    Warn,
    Bad,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewsDto {
    pub seq: u32,
    pub day: u32,
    pub time: String,
    pub tone: Tone,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReplyDto {
    pub id: u32,
    pub ok: bool,
    pub text: Option<String>,
}

// ------------------------------------------------------------------ záměry z okna

#[derive(Clone, Debug, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum UiIntent {
    SetSchedule {
        aircraft: u32,
        legs: Vec<LegInput>,
    },
    SetPrice {
        from: u16,
        to: u16,
        price: f64,
    },
    SetService {
        level: u8,
    },
    Lease {
        model: u8,
    },
    Buy {
        model: u8,
    },
    Return {
        aircraft: u32,
    },
    Sell {
        aircraft: u32,
    },
    TakeLoan {
        amount: f64,
    },
    RepayLoan {
        amount: f64,
    },
    AssignCharter {
        contract: u32,
        aircraft: u32,
    },
    TakeContract {
        contract: u32,
    },
    Bid {
        auction: u32,
        amount: f64,
    },
    Pause,
    /// Ukončit pauzu hráče (`airline` = None: vlastní).
    Resume {
        airline: Option<u8>,
    },
}

pub fn intent(me: AirlineId, ui: UiIntent) -> aae_sim::Intent {
    use aae_sim::Intent;
    use aae_sim::model::{AircraftId, AircraftTypeId, AuctionId, ContractId};
    match ui {
        UiIntent::SetSchedule { aircraft, legs } => Intent::SetSchedule {
            aircraft: AircraftId(aircraft),
            legs: legs.into_iter().map(Leg::from).collect(),
        },
        UiIntent::SetPrice { from, to, price } => Intent::SetPrice {
            from: AirportId(from),
            to: AirportId(to),
            price: money(price),
        },
        UiIntent::SetService { level } => Intent::SetService { level },
        UiIntent::Lease { model } => Intent::LeaseAircraft {
            type_id: AircraftTypeId(model),
        },
        UiIntent::Buy { model } => Intent::BuyAircraft {
            type_id: AircraftTypeId(model),
        },
        UiIntent::Return { aircraft } => Intent::ReturnAircraft {
            aircraft: AircraftId(aircraft),
        },
        UiIntent::Sell { aircraft } => Intent::SellAircraft {
            aircraft: AircraftId(aircraft),
        },
        UiIntent::TakeLoan { amount } => Intent::TakeLoan {
            amount: money(amount),
        },
        UiIntent::RepayLoan { amount } => Intent::RepayLoan {
            amount: money(amount),
        },
        UiIntent::AssignCharter { contract, aircraft } => Intent::AssignCharter {
            contract: ContractId(contract),
            aircraft: AircraftId(aircraft),
        },
        UiIntent::TakeContract { contract } => Intent::TakeContract {
            contract: ContractId(contract),
        },
        UiIntent::Bid { auction, amount } => Intent::Bid {
            auction: AuctionId(auction),
            amount: money(amount),
        },
        UiIntent::Pause => Intent::Pause(PauseReason::Player(me)),
        UiIntent::Resume { airline } => {
            Intent::Resume(PauseReason::Player(airline.map(AirlineId).unwrap_or(me)))
        }
    }
}

// ------------------------------------------------------------------ plánování

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RouteInfoDto {
    pub from: u16,
    pub to: u16,
    pub distance_km: u32,
    pub reference: f64,
    pub min: f64,
    pub max: f64,
    /// Denní poptávka tento měsíc v jednom směru (sdílí se s konkurencí).
    pub demand: u32,
    pub demand_back: u32,
    pub my_price: Option<f64>,
    pub competitors: Vec<RivalPriceDto>,
    pub fits: Vec<TypeFitDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TypeFitDto {
    pub model: u8,
    pub block_min: u32,
    pub in_range: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanPreviewDto {
    pub legs: Vec<LegDto>,
    pub error: Option<String>,
    pub error_leg: Option<u32>,
    pub slots: Vec<SlotDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CampaignInfoDto {
    pub name: String,
    pub path: String,
    pub date: String,
    pub day: u32,
    pub duration_days: u32,
    pub players: Vec<String>,
    /// Poslední uložení (unixový čas v ms).
    pub modified: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateDto {
    pub version: String,
    pub notes: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let key = PeerKey([0xab; 32]);
        assert_eq!(parse_hex(&hex(&key)), Some([0xab; 32]));
        assert_eq!(parse_hex("xyz"), None);
    }
}
