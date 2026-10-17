//! Pohled hráče na stav hry a jeho změny (Delta).
//!
//! Hostitel skládá pro každého hráče filtrovaný pohled (R4): vlastní
//! aerolinka celá, soupeři jen veřejně (ceny, síť linek, flotila, pořadí).
//! Hotovost, úvěry, letový plán a obsazenost soupeřů do počítače hráče
//! vůbec neodcházejí. Delta nese jen části, které se od minula změnily (R3).

use std::collections::BTreeMap;

use aae_sim::engine::GameState;
use aae_sim::hash::state_hash;
use aae_sim::model::{Airline, AirlineStats, Auction, Contract, Disruption, Ownership, Route};
use aae_sim::{AirlineId, CampaignRules, ContractId, Money, PauseReason, Speed};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub now: u64,
    pub speed: Speed,
    pub pause: Vec<PauseReason>,
    pub fuel_haler_per_l: i64,
    pub finished: bool,
    /// Pořadí aerolinek podle hodnoty firmy (bez hodnot – veřejné jen pořadí).
    pub ranking: Vec<AirlineId>,
    pub pauses_left: u8,
    pub vote: Option<SpeedVote>,
    /// Kdo si prohlíží kterou zakázku.
    pub presence: Vec<(AirlineId, ContractId)>,
    /// Lidští hráči právě připojení.
    pub online: Vec<AirlineId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeedVote {
    pub speed: Speed,
    pub proposer: AirlineId,
    pub yes: Vec<AirlineId>,
    pub needed: Vec<AirlineId>,
}

/// Údaje hlavičky, které zná jen hostitelská smyčka.
#[derive(Clone, Debug, Default)]
pub struct SessionInfo {
    pub pauses_left: u8,
    pub vote: Option<SpeedVote>,
    pub presence: Vec<(AirlineId, ContractId)>,
    pub online: Vec<AirlineId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AirlineView {
    pub airline: Airline,
    /// Síť linek (veřejná informace).
    pub network: Vec<Route>,
    /// `true` jen pro vlastní aerolinku.
    pub full: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub me: AirlineId,
    pub rules: CampaignRules,
    pub header: Header,
    pub airlines: BTreeMap<AirlineId, AirlineView>,
    pub contracts: Vec<Contract>,
    pub auctions: Vec<Auction>,
    pub disruptions: Vec<Disruption>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub header: Header,
    pub airlines: Vec<AirlineView>,
    pub contracts: Option<Vec<Contract>>,
    pub auctions: Option<Vec<Auction>>,
    pub disruptions: Option<Vec<Disruption>>,
}

impl View {
    pub fn my(&self) -> &Airline {
        &self.airlines[&self.me].airline
    }

    pub fn apply(&mut self, d: Delta) {
        self.header = d.header;
        for a in d.airlines {
            self.airlines.insert(a.airline.id, a);
        }
        if let Some(c) = d.contracts {
            self.contracts = c;
        }
        if let Some(a) = d.auctions {
            self.auctions = a;
        }
        if let Some(x) = d.disruptions {
            self.disruptions = x;
        }
    }
}

pub fn ranking(state: &GameState) -> Vec<AirlineId> {
    let mut ids: Vec<(Money, AirlineId)> = state
        .airlines
        .keys()
        .map(|id| (state.firm_value(*id), *id))
        .collect();
    ids.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    ids.into_iter().map(|(_, id)| id).collect()
}

pub fn header(state: &GameState, info: &SessionInfo) -> Header {
    Header {
        now: state.now,
        speed: state.speed,
        pause: state.pause.iter().copied().collect(),
        fuel_haler_per_l: state.fuel_haler_per_l,
        finished: state.finished,
        ranking: ranking(state),
        pauses_left: info.pauses_left,
        vote: info.vote.clone(),
        presence: info.presence.clone(),
        online: info.online.clone(),
    }
}

/// Veřejná nebo úplná podoba aerolinky.
pub fn airline_view(airline: &Airline, full: bool) -> AirlineView {
    let mut network: Vec<Route> = airline
        .fleet
        .values()
        .flat_map(|p| p.schedule.iter().map(|l| (l.from, l.to)))
        .collect();
    network.sort();
    network.dedup();
    let airline = if full {
        airline.clone()
    } else {
        sanitize(airline)
    };
    AirlineView {
        airline,
        network,
        full,
    }
}

/// Skryje vše, co soupeř vidět nesmí (R4).
fn sanitize(a: &Airline) -> Airline {
    let mut s = a.clone();
    s.cash = Money::ZERO;
    s.loans = Money::ZERO;
    s.negative_days = 0;
    s.slots.clear();
    s.stats = AirlineStats::default();
    for p in s.fleet.values_mut() {
        p.schedule.clear();
        p.schedule_today.clear();
        p.charters.clear();
        p.ownership = Ownership::Leased {
            deposit: Money::ZERO,
        };
    }
    s
}

pub fn build_view(state: &GameState, me: AirlineId, info: &SessionInfo) -> View {
    View {
        me,
        rules: state.rules.clone(),
        header: header(state, info),
        airlines: state
            .airlines
            .values()
            .map(|a| (a.id, airline_view(a, a.id == me)))
            .collect(),
        contracts: state.contracts.values().cloned().collect(),
        auctions: state.auctions.values().cloned().collect(),
        disruptions: state.disruptions.clone(),
    }
}

/// Pamatuje si, co hráč už dostal, a skládá z toho Deltu.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    airlines: BTreeMap<AirlineId, u64>,
    contracts: u64,
    auctions: u64,
    disruptions: u64,
}

impl Tracker {
    pub fn reset(&mut self, view: &View) {
        self.airlines = view
            .airlines
            .iter()
            .map(|(id, a)| (*id, state_hash(a)))
            .collect();
        self.contracts = state_hash(&view.contracts);
        self.auctions = state_hash(&view.auctions);
        self.disruptions = state_hash(&view.disruptions);
    }

    pub fn delta(&mut self, view: &View) -> Delta {
        let mut airlines = Vec::new();
        for (id, a) in &view.airlines {
            let h = state_hash(a);
            if self.airlines.get(id) != Some(&h) {
                self.airlines.insert(*id, h);
                airlines.push(a.clone());
            }
        }
        let changed = |slot: &mut u64, h: u64| {
            let differs = *slot != h;
            *slot = h;
            differs
        };
        let contracts = changed(&mut self.contracts, state_hash(&view.contracts))
            .then(|| view.contracts.clone());
        let auctions =
            changed(&mut self.auctions, state_hash(&view.auctions)).then(|| view.auctions.clone());
        let disruptions = changed(&mut self.disruptions, state_hash(&view.disruptions))
            .then(|| view.disruptions.clone());
        Delta {
            header: view.header.clone(),
            airlines,
            contracts,
            auctions,
            disruptions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aae_sim::{CampaignRules, Controller, NewAirline};

    fn state() -> GameState {
        let world = aae_sim::data::World::europe_v1().unwrap();
        let prg = world.airport_by_code("PRG").unwrap();
        let vie = world.airport_by_code("VIE").unwrap();
        GameState::with_world(
            world,
            CampaignRules::default(),
            1,
            vec![
                NewAirline {
                    name: "A".into(),
                    home: prg,
                    controller: Controller::Human,
                },
                NewAirline {
                    name: "B".into(),
                    home: vie,
                    controller: Controller::Human,
                },
            ],
        )
        .unwrap()
    }

    #[test]
    fn rival_secrets_are_not_in_view() {
        let mut s = state();
        let b = AirlineId(1);
        let plane = *s.airlines[&b].fleet.keys().next().unwrap();
        let t = s.airlines[&b].fleet[&plane].type_id;
        let (vie, prg) = (s.airlines[&b].home, s.airlines[&AirlineId(0)].home);
        let legs = aae_sim::schedule::shuttle(&s.world, t, vie, prg, 420, 1320);
        s.airlines
            .get_mut(&b)
            .unwrap()
            .fleet
            .get_mut(&plane)
            .unwrap()
            .schedule = legs;
        s.airlines.get_mut(&b).unwrap().loans = Money::kc(5_000_000);
        let view = build_view(&s, AirlineId(0), &SessionInfo::default());
        let rival = &view.airlines[&b];
        assert!(!rival.full);
        assert_eq!(rival.airline.cash, Money::ZERO);
        assert_eq!(rival.airline.loans, Money::ZERO);
        assert!(rival.airline.fleet.values().all(|p| p.schedule.is_empty()));
        assert_eq!(rival.airline.stats, AirlineStats::default());
        // Síť linek je veřejná.
        assert!(rival.network.contains(&(vie, prg)));
        // Vlastní aerolinka je celá.
        assert!(view.airlines[&AirlineId(0)].full);
        assert_eq!(view.my().cash, s.airlines[&AirlineId(0)].cash);
    }

    #[test]
    fn delta_carries_only_changes_and_applies() {
        let mut s = state();
        let info = SessionInfo::default();
        let mut client = build_view(&s, AirlineId(0), &info);
        let mut tracker = Tracker::default();
        tracker.reset(&client);
        let unchanged = tracker.delta(&build_view(&s, AirlineId(0), &info));
        assert!(unchanged.airlines.is_empty() && unchanged.contracts.is_none());
        s.airlines.get_mut(&AirlineId(0)).unwrap().cash = Money::kc(1);
        let d = tracker.delta(&build_view(&s, AirlineId(0), &info));
        assert_eq!(d.airlines.len(), 1);
        client.apply(d);
        assert_eq!(client.my().cash, Money::kc(1));
    }
}
