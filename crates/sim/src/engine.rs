//! Stav hry a jeho jediné dvě změnové operace: `apply` a `step`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ai::AiState;
use crate::calendar::date_of_day;
use crate::data::{DataError, World};
use crate::economy::{self, BaseDemand};
use crate::event::Event;
use crate::hash;
use crate::intent::{Intent, Reject};
use crate::model::*;
use crate::money::Money;
use crate::queue::{EventQueue, Scheduled};
use crate::rng::Rng;
use crate::rules::CampaignRules;
use crate::schedule::{self, ScheduleError};
use crate::time::{self, MINUTES_PER_DAY, Speed, day_of, minute_of_day, real_secs_to_steps};

/// Verze formátu uložené kampaně; mění se s každou změnou stavu.
pub const FORMAT_VERSION: u32 = 1;
const SAVE_MAGIC: &[u8; 4] = b"AAE\x01";

pub const MAX_AIRLINES: usize = 8;
const START_FUEL_HALER_PER_L: i64 = 2130;
const BASE_LOAN_LIMIT: Money = Money::kc(30_000_000);

/// Zakázky a aukce starší než tolik dní se ze stavu mažou (velikost uložené hry).
const HISTORY_DAYS: u64 = 14;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAirline {
    pub name: String,
    pub home: AirportId,
    pub controller: Controller,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupError {
    NoAirlines,
    TooManyAirlines,
    UnknownAirport,
    UnknownAircraftType(String),
    Data(DataError),
}

impl From<DataError> for SetupError {
    fn from(e: DataError) -> Self {
        SetupError::Data(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepError {
    /// Množina důvodů pauzy není prázdná.
    Paused,
    Finished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    NotASave,
    UnsupportedVersion(u32),
    Corrupt,
}

/// Let zařazený do dnešního dne na lince (pro dělení poptávky podle R8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightSlot {
    pub airline: AirlineId,
    pub aircraft: AircraftId,
    pub dep_min: u32,
    pub seats: u32,
    /// Přitažlivost zafixovaná při odletu (cena platná při odletu).
    pub departed_attr: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDay {
    pub demand: u32,
    pub reference: Money,
    pub flights: Vec<FlightSlot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub format_version: u32,
    pub rules: CampaignRules,
    pub world: World,
    pub base_demand: BaseDemand,
    /// Aktuální herní minuta (hranice kroku, na které se aplikují záměry).
    pub now: u64,
    pub speed: Speed,
    pub rng: Rng,
    pub pause: BTreeSet<PauseReason>,
    pub airlines: BTreeMap<AirlineId, Airline>,
    pub contracts: BTreeMap<ContractId, Contract>,
    pub auctions: BTreeMap<AuctionId, Auction>,
    /// Zakázky na burze (nabídka nebo otevřené okno).
    pub open_contracts: BTreeSet<ContractId>,
    /// Přidělené zakázky čekající na let.
    pub active_contracts: BTreeSet<ContractId>,
    pub open_auctions: BTreeSet<AuctionId>,
    pub queue: EventQueue,
    pub fuel_haler_per_l: i64,
    /// Oznámené uzávěry (dnes a zítra).
    pub disruptions: Vec<Disruption>,
    pub route_days: BTreeMap<Route, RouteDay>,
    pub next_aircraft: u32,
    pub next_contract: u32,
    pub next_auction: u32,
    pub ai: BTreeMap<AirlineId, AiState>,
    pub finished: bool,
}

impl GameState {
    // ------------------------------------------------------------ založení

    pub fn new(
        rules: CampaignRules,
        seed: u64,
        airlines: Vec<NewAirline>,
    ) -> Result<GameState, SetupError> {
        GameState::with_world(World::europe_v1()?, rules, seed, airlines)
    }

    pub fn with_world(
        world: World,
        rules: CampaignRules,
        seed: u64,
        new_airlines: Vec<NewAirline>,
    ) -> Result<GameState, SetupError> {
        if new_airlines.is_empty() {
            return Err(SetupError::NoAirlines);
        }
        if new_airlines.len() > MAX_AIRLINES {
            return Err(SetupError::TooManyAirlines);
        }
        for (code, _) in &rules.starting_fleet {
            if world.aircraft_type_by_code(code).is_none() {
                return Err(SetupError::UnknownAircraftType(code.clone()));
            }
        }
        if new_airlines.iter().any(|a| !world.is_valid_airport(a.home)) {
            return Err(SetupError::UnknownAirport);
        }
        let base_demand = BaseDemand::compute(&world);
        let mut state = GameState {
            format_version: FORMAT_VERSION,
            speed: rules.initial_speed,
            rules,
            world,
            base_demand,
            now: 0,
            rng: Rng::from_seed(seed),
            pause: BTreeSet::new(),
            airlines: BTreeMap::new(),
            contracts: BTreeMap::new(),
            auctions: BTreeMap::new(),
            open_contracts: BTreeSet::new(),
            active_contracts: BTreeSet::new(),
            open_auctions: BTreeSet::new(),
            queue: EventQueue::default(),
            fuel_haler_per_l: START_FUEL_HALER_PER_L,
            disruptions: Vec::new(),
            route_days: BTreeMap::new(),
            next_aircraft: 1,
            next_contract: 1,
            next_auction: 1,
            ai: BTreeMap::new(),
            finished: false,
        };
        for (i, spec) in new_airlines.into_iter().enumerate() {
            let id = AirlineId(i as u8);
            let airline = state.fresh_airline(
                id,
                spec.name,
                spec.home,
                spec.controller,
                state.rules.starting_capital,
                0,
            );
            state.airlines.insert(id, airline);
            state.add_starting_fleet(id);
            if let Controller::Ai(strategy) = spec.controller {
                let rng = state.rng.fork();
                state.ai.insert(id, AiState::new(strategy, rng));
            }
        }
        let mut events = Vec::new();
        state.start_day(0, &mut events);
        Ok(state)
    }

    fn fresh_airline(
        &self,
        id: AirlineId,
        name: String,
        home: AirportId,
        controller: Controller,
        cash: Money,
        generation: u32,
    ) -> Airline {
        let service = match controller {
            Controller::Ai(Strategy::Levna) => 1,
            Controller::Ai(Strategy::Premiova) => 3,
            _ => 2,
        };
        Airline {
            id,
            name,
            home,
            controller,
            cash,
            loans: Money::ZERO,
            service,
            fleet: BTreeMap::new(),
            prices: BTreeMap::new(),
            slots: BTreeMap::new(),
            generation,
            negative_days: 0,
            stats: AirlineStats::default(),
        }
    }

    fn add_starting_fleet(&mut self, id: AirlineId) {
        let fleet: Vec<(String, u32)> = self.rules.starting_fleet.clone();
        for (code, count) in fleet {
            let type_id = self
                .world
                .aircraft_type_by_code(&code)
                .expect("ověřeno při založení");
            let deposit = self
                .world
                .aircraft_type(type_id)
                .lease_per_day
                .times(economy::LEASE_DEPOSIT_DAYS);
            for _ in 0..count {
                self.add_aircraft(id, type_id, Ownership::Leased { deposit });
            }
        }
    }

    fn add_aircraft(
        &mut self,
        airline: AirlineId,
        type_id: AircraftTypeId,
        ownership: Ownership,
    ) -> AircraftId {
        let id = AircraftId(self.next_aircraft);
        self.next_aircraft += 1;
        let aircraft = Aircraft {
            id,
            type_id,
            ownership,
            schedule: Vec::new(),
            schedule_today: Vec::new(),
            charters: BTreeMap::new(),
        };
        self.airlines
            .get_mut(&airline)
            .expect("existující aerolinka")
            .fleet
            .insert(id, aircraft);
        id
    }

    // ------------------------------------------------------------- dotazy

    pub fn day(&self) -> u64 {
        day_of(self.now)
    }

    pub fn is_paused(&self) -> bool {
        !self.pause.is_empty()
    }

    pub fn hash(&self) -> u64 {
        hash::state_hash(self)
    }

    pub fn reference_price(&self, route: Route) -> Money {
        economy::reference_fare(self.world.distance(route.0, route.1))
    }

    pub fn price_of(&self, airline: &Airline, route: Route) -> Money {
        airline
            .prices
            .get(&route)
            .copied()
            .unwrap_or_else(|| self.reference_price(route))
    }

    /// Hodnota firmy: hotovost + zálohy + účetní hodnota vlastních letadel − úvěry.
    pub fn firm_value(&self, id: AirlineId) -> Money {
        let a = &self.airlines[&id];
        let assets: Money = a
            .fleet
            .values()
            .map(|p| match p.ownership {
                Ownership::Leased { deposit } => deposit,
                Ownership::Owned { book_value } => book_value,
            })
            .sum();
        a.cash + assets - a.loans
    }

    pub fn loan_limit(&self, a: &Airline) -> Money {
        let owned: Money = a
            .fleet
            .values()
            .filter_map(|p| match p.ownership {
                Ownership::Owned { book_value } => Some(book_value),
                Ownership::Leased { .. } => None,
            })
            .sum();
        BASE_LOAN_LIMIT + owned.permille(500)
    }

    /// Minimální příhoz v aukci (rezervace nebo nejvyšší příhoz + navýšení, zaokrouhleno na 10 000 Kč).
    pub fn min_bid(&self, auction: &Auction) -> Money {
        match auction.high_bid {
            None => self.rules.auction_reserve,
            Some((_, high)) => {
                let raw = high.permille(1000 + self.rules.auction_increment_permille);
                let step = Money::kc(10_000).0;
                Money((raw.0 + step - 1) / step * step)
            }
        }
    }

    /// Počet denních odletů aerolinky z letiště, případně s náhradou plánu jednoho letadla.
    pub fn departures_from(
        &self,
        a: &Airline,
        airport: AirportId,
        replace: Option<(AircraftId, &[Leg])>,
    ) -> u32 {
        a.fleet
            .values()
            .map(|p| {
                let legs: &[Leg] = match replace {
                    Some((id, legs)) if id == p.id => legs,
                    _ => &p.schedule,
                };
                legs.iter().filter(|l| l.from == airport).count() as u32
            })
            .sum()
    }

    /// Obsluhuje aerolinka linku (veřejná informace: síť linek).
    pub fn serves_route(&self, a: &Airline, route: Route) -> bool {
        a.fleet
            .values()
            .any(|p| p.schedule.iter().any(|l| (l.from, l.to) == route))
    }

    // --------------------------------------------------------------- apply

    /// Aplikuje záměr na aktuální hranici kroku. Při pauze se plánovací
    /// záměry aplikují bez posunu času, burza a aukce se odmítnou.
    pub fn apply(&mut self, actor: Actor, intent: Intent) -> Result<Vec<Event>, Reject> {
        if self.finished {
            return Err(Reject::CampaignFinished);
        }
        if self.is_paused() && intent.is_market() {
            return Err(Reject::MarketClosedDuringPause);
        }
        let mut events = Vec::new();
        match intent {
            Intent::Pause(reason) => {
                self.check_pause_actor(actor, reason, false)?;
                if !self.pause.insert(reason) {
                    return Err(Reject::AlreadyPaused);
                }
                events.push(Event::Paused(reason));
            }
            Intent::Resume(reason) => {
                self.check_pause_actor(actor, reason, true)?;
                if !self.pause.remove(&reason) {
                    return Err(Reject::NotPaused);
                }
                events.push(Event::Resumed(reason));
            }
            Intent::SetSpeed(speed) => {
                if actor != Actor::System {
                    return Err(Reject::NotPermitted);
                }
                if speed == self.speed {
                    return Err(Reject::SameSpeed);
                }
                self.rescale_real_time_deadlines(self.speed, speed);
                self.speed = speed;
                events.push(Event::SpeedChanged(speed));
            }
            Intent::SetController {
                airline,
                controller,
            } => {
                if actor != Actor::System {
                    return Err(Reject::NotPermitted);
                }
                let a = self
                    .airlines
                    .get_mut(&airline)
                    .ok_or(Reject::UnknownAirline)?;
                a.controller = controller;
                match controller {
                    Controller::Human => {
                        self.ai.remove(&airline);
                    }
                    Controller::Ai(strategy) => {
                        let rng = self.rng.fork();
                        let mut ai = AiState::new(strategy, rng);
                        ai.reset(self.now);
                        self.ai.insert(airline, ai);
                    }
                }
                events.push(Event::ControllerChanged {
                    airline,
                    controller,
                });
            }
            other => {
                let Actor::Airline(id) = actor else {
                    return Err(Reject::NotPermitted);
                };
                if !self.airlines.contains_key(&id) {
                    return Err(Reject::UnknownAirline);
                }
                self.apply_airline(id, other, &mut events)?;
            }
        }
        Ok(events)
    }

    fn check_pause_actor(
        &self,
        actor: Actor,
        reason: PauseReason,
        resume: bool,
    ) -> Result<(), Reject> {
        match (reason, actor) {
            (PauseReason::Player(p), Actor::Airline(a)) if p == a => Ok(()),
            // Hostitel smí ukončit cizí hráčskou pauzu (jako systémový záměr).
            (PauseReason::Player(_), Actor::System) if resume => Ok(()),
            (PauseReason::Technical | PauseReason::Auto, Actor::System) => Ok(()),
            _ => Err(Reject::NotPermitted),
        }
    }

    fn rescale_real_time_deadlines(&mut self, old: Speed, new: Speed) {
        let now = self.now;
        let rescale = |due: u64| now + time::rescale_remaining(due.saturating_sub(now), old, new);
        for id in &self.open_contracts {
            if let Some(w) = self.contracts.get_mut(id).and_then(|c| c.window.as_mut()) {
                w.due = rescale(w.due);
            }
        }
        for id in &self.open_auctions {
            if let Some(a) = self.auctions.get_mut(id) {
                if let Some(p) = a.protection_due {
                    if p > now {
                        a.protection_due = Some(rescale(p));
                    }
                }
            }
        }
    }

    fn apply_airline(
        &mut self,
        id: AirlineId,
        intent: Intent,
        events: &mut Vec<Event>,
    ) -> Result<(), Reject> {
        let today = self.day();
        match intent {
            Intent::SetSchedule { aircraft, legs } => {
                let a = &self.airlines[&id];
                let plane = a.fleet.get(&aircraft).ok_or(Reject::UnknownAircraft)?;
                schedule::validate(&self.world, plane.type_id, &legs)
                    .map_err(Reject::InvalidSchedule)?;
                for airport in self
                    .world
                    .airport_ids()
                    .filter(|ap| self.world.airport(*ap).slot_constrained)
                {
                    let needed = self.departures_from(a, airport, Some((aircraft, &legs)));
                    let owned = a.slots.get(&airport).copied().unwrap_or(0);
                    if needed > owned {
                        return Err(Reject::InvalidSchedule(ScheduleError::MissingSlots {
                            airport,
                            needed,
                            owned,
                        }));
                    }
                }
                self.airline_mut(id)
                    .fleet
                    .get_mut(&aircraft)
                    .expect("ověřeno")
                    .schedule = legs;
                events.push(Event::ScheduleSet {
                    airline: id,
                    aircraft,
                });
            }
            Intent::SetPrice { from, to, price } => {
                if !self.world.is_valid_airport(from)
                    || !self.world.is_valid_airport(to)
                    || from == to
                {
                    return Err(Reject::UnknownAirport);
                }
                let (min, max) = economy::price_bounds(self.world.distance(from, to));
                if price < min || price > max {
                    return Err(Reject::PriceOutOfBounds { min, max });
                }
                self.airline_mut(id).prices.insert((from, to), price);
                events.push(Event::PriceSet {
                    airline: id,
                    from,
                    to,
                    price,
                });
            }
            Intent::SetService { level } => {
                if !(1..=3).contains(&level) {
                    return Err(Reject::InvalidServiceLevel);
                }
                self.airline_mut(id).service = level;
            }
            Intent::LeaseAircraft { type_id } => {
                if !self.world.is_valid_aircraft_type(type_id) {
                    return Err(Reject::UnknownAircraftType);
                }
                let deposit = self
                    .world
                    .aircraft_type(type_id)
                    .lease_per_day
                    .times(economy::LEASE_DEPOSIT_DAYS);
                self.pay(id, deposit)?;
                let aircraft = self.add_aircraft(id, type_id, Ownership::Leased { deposit });
                events.push(Event::AircraftAdded {
                    airline: id,
                    aircraft,
                });
            }
            Intent::BuyAircraft { type_id } => {
                if !self.world.is_valid_aircraft_type(type_id) {
                    return Err(Reject::UnknownAircraftType);
                }
                let price = self.world.aircraft_type(type_id).price;
                self.pay(id, price)?;
                let aircraft =
                    self.add_aircraft(id, type_id, Ownership::Owned { book_value: price });
                events.push(Event::AircraftAdded {
                    airline: id,
                    aircraft,
                });
            }
            Intent::ReturnAircraft { aircraft } | Intent::SellAircraft { aircraft } => {
                let selling = matches!(intent, Intent::SellAircraft { .. });
                let plane = self.airlines[&id]
                    .fleet
                    .get(&aircraft)
                    .ok_or(Reject::UnknownAircraft)?;
                if plane.charters.keys().any(|d| *d >= today) {
                    return Err(Reject::AircraftHasCharter);
                }
                let proceeds = match (plane.ownership, selling) {
                    (Ownership::Leased { deposit }, false) => deposit,
                    (Ownership::Owned { book_value }, true) => {
                        book_value.permille(economy::SALE_PERMILLE)
                    }
                    _ => return Err(Reject::NotPermitted),
                };
                let airline = self.airline_mut(id);
                airline.fleet.remove(&aircraft);
                airline.cash += proceeds;
                events.push(Event::AircraftRemoved {
                    airline: id,
                    aircraft,
                });
            }
            Intent::TakeLoan { amount } => {
                if amount.0 <= 0 {
                    return Err(Reject::InvalidAmount);
                }
                let a = &self.airlines[&id];
                let max = self.loan_limit(a) - a.loans;
                if amount > max {
                    return Err(Reject::LoanLimit {
                        max: if max.is_negative() { Money::ZERO } else { max },
                    });
                }
                let airline = self.airline_mut(id);
                airline.cash += amount;
                airline.loans += amount;
                events.push(Event::LoanChanged {
                    airline: id,
                    loans: airline.loans,
                });
            }
            Intent::RepayLoan { amount } => {
                let a = &self.airlines[&id];
                if amount.0 <= 0 || amount > a.loans {
                    return Err(Reject::InvalidAmount);
                }
                self.pay(id, amount)?;
                let airline = self.airline_mut(id);
                airline.loans -= amount;
                events.push(Event::LoanChanged {
                    airline: id,
                    loans: airline.loans,
                });
            }
            Intent::AssignCharter { contract, aircraft } => {
                self.assign_charter(id, contract, aircraft, today)?;
                events.push(Event::CharterAssigned { contract, aircraft });
            }
            Intent::TakeContract { contract } => self.take_contract(id, contract, events)?,
            Intent::Bid { auction, amount } => self.bid(id, auction, amount, events)?,
            Intent::Pause(_)
            | Intent::Resume(_)
            | Intent::SetSpeed(_)
            | Intent::SetController { .. } => {
                unreachable!("vyřízeno v apply")
            }
        }
        Ok(())
    }

    fn airline_mut(&mut self, id: AirlineId) -> &mut Airline {
        self.airlines.get_mut(&id).expect("existující aerolinka")
    }

    fn pay(&mut self, id: AirlineId, amount: Money) -> Result<(), Reject> {
        let airline = self.airline_mut(id);
        if airline.cash < amount {
            return Err(Reject::InsufficientFunds { needed: amount });
        }
        airline.cash -= amount;
        Ok(())
    }

    fn assign_charter(
        &mut self,
        id: AirlineId,
        contract: ContractId,
        aircraft: AircraftId,
        today: u64,
    ) -> Result<(), Reject> {
        let c = self
            .contracts
            .get(&contract)
            .ok_or(Reject::UnknownContract)?;
        let previous = match c.status {
            ContractStatus::Awarded(owner) if owner == id => None,
            ContractStatus::Assigned(owner, plane) if owner == id => Some(plane),
            _ => return Err(Reject::ContractNotYours),
        };
        if c.day <= today {
            return Err(Reject::ContractDayPassed);
        }
        let (day, pax, distance) = (c.day, c.pax, self.world.distance(c.from, c.to));
        let plane = self.airlines[&id]
            .fleet
            .get(&aircraft)
            .ok_or(Reject::UnknownAircraft)?;
        let t = self.world.aircraft_type(plane.type_id);
        if t.seats < pax {
            return Err(Reject::AircraftTooSmall);
        }
        if t.range_km < distance {
            return Err(Reject::OutOfRange);
        }
        if plane
            .charters
            .get(&day)
            .is_some_and(|other| *other != contract)
        {
            return Err(Reject::AircraftBusyThatDay);
        }
        let airline = self.airline_mut(id);
        if let Some(old) = previous {
            if let Some(p) = airline.fleet.get_mut(&old) {
                p.charters.remove(&day);
            }
        }
        airline
            .fleet
            .get_mut(&aircraft)
            .expect("ověřeno")
            .charters
            .insert(day, contract);
        self.contracts.get_mut(&contract).expect("ověřeno").status =
            ContractStatus::Assigned(id, aircraft);
        Ok(())
    }

    fn take_contract(
        &mut self,
        id: AirlineId,
        contract: ContractId,
        events: &mut Vec<Event>,
    ) -> Result<(), Reject> {
        let now = self.now;
        let window_steps = real_secs_to_steps(self.rules.contract_window_secs, self.speed);
        if !self.open_contracts.contains(&contract) {
            return Err(Reject::ContractNotOffered);
        }
        let c = self
            .contracts
            .get_mut(&contract)
            .ok_or(Reject::UnknownContract)?;
        match &mut c.window {
            None => {
                if now >= c.offer_until {
                    return Err(Reject::ContractNotOffered);
                }
                let due = now + window_steps;
                c.window = Some(CollectionWindow {
                    due,
                    candidates: BTreeSet::from([id]),
                });
                events.push(Event::ContractWindowOpened {
                    contract,
                    by: id,
                    due,
                });
            }
            Some(w) => {
                if now >= w.due {
                    return Err(Reject::ContractNotOffered);
                }
                if !w.candidates.insert(id) {
                    return Err(Reject::AlreadyCandidate);
                }
                events.push(Event::ContractCandidate { contract, by: id });
            }
        }
        Ok(())
    }

    fn bid(
        &mut self,
        id: AirlineId,
        auction: AuctionId,
        amount: Money,
        events: &mut Vec<Event>,
    ) -> Result<(), Reject> {
        let now = self.now;
        let protection = real_secs_to_steps(self.rules.auction_protection_secs, self.speed);
        let a = self.auctions.get(&auction).ok_or(Reject::UnknownAuction)?;
        if a.status != AuctionStatus::Open || now >= a.effective_end() {
            return Err(Reject::AuctionClosed);
        }
        if a.high_bid.is_some_and(|(who, _)| who == id) {
            return Err(Reject::AlreadyHighestBidder);
        }
        let min = self.min_bid(a);
        if amount < min {
            return Err(Reject::BidTooLow { min });
        }
        if self.airlines[&id].cash < amount {
            return Err(Reject::InsufficientFunds { needed: amount });
        }
        let a = self.auctions.get_mut(&auction).expect("ověřeno");
        let end = a.effective_end();
        // Příhoz v ochranné lhůtě prodlouží odpočet o další ochrannou lhůtu.
        if now + protection > end {
            a.protection_due = Some(end + protection);
        }
        a.high_bid = Some((id, amount));
        events.push(Event::BidPlaced {
            auction,
            by: id,
            amount,
            effective_end: a.effective_end(),
        });
        Ok(())
    }

    // ---------------------------------------------------------------- step

    /// Posune herní čas o jednu minutu a zpracuje vše, co v ní nastane.
    pub fn step(&mut self) -> Result<Vec<Event>, StepError> {
        if self.finished {
            return Err(StepError::Finished);
        }
        if self.is_paused() {
            return Err(StepError::Paused);
        }
        let mut events = Vec::new();
        self.now += 1;
        if minute_of_day(self.now) == 0 {
            let day = self.day();
            self.end_day(day - 1, &mut events);
            self.start_day(day, &mut events);
            if self.finished {
                return Ok(events);
            }
        }
        while let Some((_, item)) = self.queue.pop_due(self.now) {
            match item {
                Scheduled::Departure {
                    airline,
                    aircraft,
                    generation,
                    leg,
                } => self.depart(airline, aircraft, generation, leg as usize, &mut events),
                Scheduled::ContractSpawn => self.spawn_contract(&mut events),
                Scheduled::AuctionSpawn => self.spawn_auction(&mut events),
            }
        }
        self.resolve_contracts(&mut events);
        self.resolve_auctions(&mut events);
        Ok(events)
    }

    fn resolve_contracts(&mut self, events: &mut Vec<Event>) {
        let now = self.now;
        let due: Vec<ContractId> = self
            .open_contracts
            .iter()
            .copied()
            .filter(|id| {
                let c = &self.contracts[id];
                match &c.window {
                    Some(w) => now >= w.due,
                    None => now >= c.offer_until,
                }
            })
            .collect();
        for id in due {
            self.open_contracts.remove(&id);
            let window = self
                .contracts
                .get_mut(&id)
                .expect("otevřená zakázka")
                .window
                .take();
            match window {
                Some(w) => {
                    let candidates: Vec<AirlineId> = w.candidates.iter().copied().collect();
                    let winner = candidates[self.rng.below(candidates.len() as u64) as usize];
                    let c = self.contracts.get_mut(&id).expect("otevřená zakázka");
                    c.status = ContractStatus::Awarded(winner);
                    c.window = Some(w);
                    self.active_contracts.insert(id);
                    if let Some(a) = self.airlines.get_mut(&winner) {
                        a.stats.contracts_won += 1;
                    }
                    events.push(Event::ContractAwarded {
                        contract: id,
                        winner,
                        candidates,
                    });
                }
                None => {
                    self.contracts
                        .get_mut(&id)
                        .expect("otevřená zakázka")
                        .status = ContractStatus::Expired;
                    events.push(Event::ContractExpired { contract: id });
                }
            }
        }
    }

    fn resolve_auctions(&mut self, events: &mut Vec<Event>) {
        let now = self.now;
        let due: Vec<AuctionId> = self
            .open_auctions
            .iter()
            .copied()
            .filter(|id| now >= self.auctions[id].effective_end())
            .collect();
        for id in due {
            self.open_auctions.remove(&id);
            let a = self.auctions.get_mut(&id).expect("otevřená aukce");
            match a.high_bid {
                Some((winner, price)) => {
                    a.status = AuctionStatus::Won(winner, price);
                    let airport = a.airport;
                    if let Some(airline) = self.airlines.get_mut(&winner) {
                        airline.cash -= price;
                        *airline.slots.entry(airport).or_insert(0) += 1;
                        airline.stats.slots_won += 1;
                    }
                    events.push(Event::AuctionWon {
                        auction: id,
                        airport,
                        winner,
                        price,
                    });
                }
                None => {
                    a.status = AuctionStatus::NoBids;
                    events.push(Event::AuctionNoBids { auction: id });
                }
            }
        }
    }

    // -------------------------------------------------------------- den

    fn end_day(&mut self, day: u64, events: &mut Vec<Event>) {
        let ids: Vec<AirlineId> = self.airlines.keys().copied().collect();
        for id in ids {
            let a = self.airline_mut(id);
            a.stats.yesterday = std::mem::take(&mut a.stats.today);
            a.stats.revenue_yesterday = std::mem::take(&mut a.stats.revenue_today);
            a.stats.cost_yesterday = std::mem::take(&mut a.stats.cost_today);
            if a.cash.is_negative() {
                a.negative_days += 1;
                let negative_days = a.negative_days;
                if negative_days >= self.rules.bankruptcy_days {
                    self.bankrupt(id, events);
                } else {
                    events.push(Event::BankruptcyWarning {
                        airline: id,
                        negative_days,
                    });
                }
            } else {
                a.negative_days = 0;
            }
        }
        self.route_days.clear();
        let _ = day;
    }

    fn bankrupt(&mut self, id: AirlineId, events: &mut Vec<Event>) {
        let old = &self.airlines[&id];
        let capital = self
            .rules
            .starting_capital
            .permille(self.rules.restart_capital_permille);
        let fresh = self.fresh_airline(
            id,
            old.name.clone(),
            old.home,
            old.controller,
            capital,
            old.generation + 1,
        );
        self.airlines.insert(id, fresh);
        // Přidělené zakázky propadají bez penále.
        let lost: Vec<ContractId> = self
            .active_contracts
            .iter()
            .copied()
            .filter(|c| matches!(self.contracts[c].status, ContractStatus::Awarded(o) | ContractStatus::Assigned(o, _) if o == id))
            .collect();
        for c in lost {
            self.active_contracts.remove(&c);
            self.contracts.get_mut(&c).expect("aktivní zakázka").status =
                ContractStatus::Failed(id);
        }
        if let Some(ai) = self.ai.get_mut(&id) {
            ai.reset(self.now);
        }
        events.push(Event::Bankruptcy {
            airline: id,
            new_capital: capital,
        });
    }

    fn start_day(&mut self, day: u64, events: &mut Vec<Event>) {
        events.push(Event::DayStarted { day });
        if day >= self.rules.duration_days {
            self.finished = true;
            events.push(Event::CampaignEnded { day });
            return;
        }
        let day_start = day * MINUTES_PER_DAY;
        let month = date_of_day(day).month;
        if self.rules.events && day > 0 {
            self.walk_fuel_price(events);
        }
        self.disruptions.retain(|d| d.day() >= day);
        if self.rules.events {
            self.announce_disruptions(day + 1, events);
        }
        self.prune_history(day);
        self.daily_finance();
        self.settle_charters(day, events);

        // Dnešní plán a lety do fronty.
        let ids: Vec<AirlineId> = self.airlines.keys().copied().collect();
        for id in ids {
            let generation = self.airlines[&id].generation;
            let plane_ids: Vec<AircraftId> = self.airlines[&id].fleet.keys().copied().collect();
            for pid in plane_ids {
                let plane = self
                    .airlines
                    .get_mut(&id)
                    .and_then(|a| a.fleet.get_mut(&pid))
                    .expect("letadlo");
                plane.schedule_today = if plane.charters.contains_key(&day) {
                    Vec::new()
                } else {
                    plane.schedule.clone()
                };
                let legs = plane.schedule_today.clone();
                let type_id = plane.type_id;
                let seats = self.world.aircraft_type(type_id).seats;
                for (i, leg) in legs.iter().enumerate() {
                    let reference = self.reference_price((leg.from, leg.to));
                    self.route_days
                        .entry((leg.from, leg.to))
                        .or_insert_with(|| RouteDay {
                            demand: 0,
                            reference,
                            flights: Vec::new(),
                        })
                        .flights
                        .push(FlightSlot {
                            airline: id,
                            aircraft: pid,
                            dep_min: leg.dep_min,
                            seats,
                            departed_attr: None,
                        });
                    self.queue.push(
                        day_start + leg.dep_min as u64,
                        Scheduled::Departure {
                            airline: id,
                            aircraft: pid,
                            generation,
                            leg: i as u16,
                        },
                    );
                }
            }
        }
        // Denní poptávka linek podle sezóny a průměrné ceny.
        let routes: Vec<Route> = self.route_days.keys().copied().collect();
        for route in routes {
            let rd = &self.route_days[&route];
            let prices: Vec<Money> = rd
                .flights
                .iter()
                .map(|f| self.price_of(&self.airlines[&f.airline], route))
                .collect();
            let average =
                Money(prices.iter().map(|p| p.0).sum::<i64>() / prices.len().max(1) as i64);
            let seasonal = self.base_demand.seasonal(route.0, route.1, month) as f64;
            let demand = (seasonal * economy::price_factor(rd.reference, average)).round() as u32;
            self.route_days.get_mut(&route).expect("linka").demand = demand;
        }
        // Burza a aukce.
        for _ in 0..self.rules.contracts_per_day {
            let at = day_start + self.rng.below(MINUTES_PER_DAY);
            self.queue.push(at, Scheduled::ContractSpawn);
        }
        if self.rules.auction_every_days > 0 && day % self.rules.auction_every_days == 0 {
            let at = day_start + self.rng.range(6 * 60, 16 * 60);
            self.queue.push(at, Scheduled::AuctionSpawn);
        }
    }

    fn daily_finance(&mut self) {
        let ids: Vec<AirlineId> = self.airlines.keys().copied().collect();
        for id in ids {
            let mut cost = Money::ZERO;
            let types: Vec<(AircraftId, AircraftTypeId)> = self.airlines[&id]
                .fleet
                .values()
                .map(|p| (p.id, p.type_id))
                .collect();
            for (pid, type_id) in types {
                let t = self.world.aircraft_type(type_id);
                let (lease, depreciation, floor) = (
                    t.lease_per_day,
                    t.price.permille(economy::DEPRECIATION_PERMILLE).0 / 365,
                    t.price.permille(100),
                );
                let plane = self.airline_mut(id).fleet.get_mut(&pid).expect("letadlo");
                match &mut plane.ownership {
                    Ownership::Leased { .. } => cost += lease,
                    Ownership::Owned { book_value } => {
                        *book_value = (*book_value - Money(depreciation)).max(floor);
                    }
                }
            }
            let a = self.airline_mut(id);
            cost += economy::OVERHEAD_PER_AIRCRAFT_PER_DAY.times(a.fleet.len() as i64);
            cost += Money(a.loans.0 * economy::LOAN_RATE_PERMILLE / 1000 / 365);
            a.cash -= cost;
            a.stats.cost_today += cost;
        }
    }

    fn settle_charters(&mut self, day: u64, events: &mut Vec<Event>) {
        let ids: Vec<ContractId> = self.active_contracts.iter().copied().collect();
        for cid in ids {
            let c = self.contracts[&cid].clone();
            if c.day > day {
                continue;
            }
            self.active_contracts.remove(&cid);
            match c.status {
                ContractStatus::Assigned(owner, plane_id)
                    if self.airlines[&owner].fleet.contains_key(&plane_id) =>
                {
                    let a = &self.airlines[&owner];
                    let plane = &a.fleet[&plane_id];
                    let t = self.world.aircraft_type(plane.type_id);
                    let base = plane.schedule.first().map(|l| l.from).unwrap_or(a.home);
                    let mut cost = Money::ZERO;
                    for (from, to, pax) in
                        [(base, c.from, 0), (c.from, c.to, c.pax), (c.to, base, 0)]
                    {
                        if from == to {
                            continue;
                        }
                        let block = schedule::block_minutes(&self.world, plane.type_id, from, to);
                        let slot = self.world.airport(from).slot_constrained
                            || self.world.airport(to).slot_constrained;
                        cost += economy::flight_cost(
                            t,
                            self.world.distance(from, to),
                            block,
                            self.fuel_haler_per_l,
                            pax,
                            a.service,
                            slot,
                        );
                    }
                    let airline = self.airline_mut(owner);
                    airline.cash += c.reward - cost;
                    airline.stats.revenue_today += c.reward;
                    airline.stats.cost_today += cost;
                    airline.stats.charters_flown += 1;
                    if let Some(p) = airline.fleet.get_mut(&plane_id) {
                        p.charters.remove(&day);
                    }
                    self.contracts.get_mut(&cid).expect("zakázka").status =
                        ContractStatus::Done(owner);
                    events.push(Event::CharterFlown {
                        airline: owner,
                        contract: cid,
                        reward: c.reward,
                        cost,
                    });
                }
                ContractStatus::Awarded(owner) | ContractStatus::Assigned(owner, _) => {
                    let penalty = c.reward.permille(500);
                    if let Some(a) = self.airlines.get_mut(&owner) {
                        a.cash -= penalty;
                        a.stats.cost_today += penalty;
                    }
                    self.contracts.get_mut(&cid).expect("zakázka").status =
                        ContractStatus::Failed(owner);
                    events.push(Event::ContractFailed {
                        contract: cid,
                        airline: owner,
                        penalty,
                    });
                }
                _ => {}
            }
        }
    }

    fn prune_history(&mut self, day: u64) {
        let horizon = day.saturating_sub(HISTORY_DAYS);
        let (open, active) = (&self.open_contracts, &self.active_contracts);
        self.contracts
            .retain(|id, c| open.contains(id) || active.contains(id) || c.day >= horizon);
        let open = &self.open_auctions;
        self.auctions
            .retain(|id, a| open.contains(id) || day_of(a.end_due) >= horizon);
    }

    fn walk_fuel_price(&mut self, events: &mut Vec<Event>) {
        let p = self.fuel_haler_per_l;
        let noise = self.rng.range(0, 30) as i64 - 15; // ±1,5 %
        let reversion = (START_FUEL_HALER_PER_L - p) * 2 / 100;
        let mut next = p + p * noise / 1000 + reversion;
        if self.rng.chance(1, 90) {
            next = next * 110 / 100; // krize
        }
        next = next.clamp(1500, 3500);
        if next != p {
            self.fuel_haler_per_l = next;
            events.push(Event::FuelPriceChanged { haler_per_l: next });
        }
    }

    fn announce_disruptions(&mut self, day: u64, events: &mut Vec<Event>) {
        if self.rng.chance(1, 45) {
            let i = self.rng.below(self.world.airport_count() as u64) as usize;
            let country = self.world.airports[i].country.clone();
            events.push(Event::DisruptionAnnounced {
                day,
                description: format!(
                    "Stávka řízení letového provozu ({country}), {}",
                    date_of_day(day)
                ),
            });
            self.disruptions.push(Disruption::Strike { day, country });
        }
        if self.rng.chance(1, 30) {
            let airport = AirportId(self.rng.below(self.world.airport_count() as u64) as u16);
            let from_min = self.rng.range(0, 20 * 60) as u32;
            let to_min = from_min + self.rng.range(180, 360) as u32;
            events.push(Event::DisruptionAnnounced {
                day,
                description: format!(
                    "Počasí: {} zavřeno {:02}:{:02}–{:02}:{:02}, {}",
                    self.world.airport(airport).code,
                    from_min / 60,
                    from_min % 60,
                    (to_min / 60).min(24),
                    to_min % 60,
                    date_of_day(day)
                ),
            });
            self.disruptions.push(Disruption::Weather {
                day,
                airport,
                from_min,
                to_min,
            });
        }
    }

    fn is_disrupted(
        &self,
        day: u64,
        from: AirportId,
        to: AirportId,
        dep_min: u32,
        arr_min: u32,
    ) -> bool {
        self.disruptions.iter().any(|d| match d {
            Disruption::Strike { day: d, country } => {
                *d == day
                    && (&self.world.airport(from).country == country
                        || &self.world.airport(to).country == country)
            }
            Disruption::Weather {
                day: d,
                airport,
                from_min,
                to_min,
            } => {
                *d == day
                    && ((*airport == from && (*from_min..*to_min).contains(&dep_min))
                        || (*airport == to && (*from_min..*to_min).contains(&arr_min)))
            }
        })
    }

    // ------------------------------------------------------------- lety

    fn depart(
        &mut self,
        airline_id: AirlineId,
        aircraft: AircraftId,
        generation: u32,
        leg_index: usize,
        events: &mut Vec<Event>,
    ) {
        let Some(a) = self.airlines.get(&airline_id) else {
            return;
        };
        if a.generation != generation {
            return;
        }
        let Some(plane) = a.fleet.get(&aircraft) else {
            return;
        };
        let Some(leg) = plane.schedule_today.get(leg_index).copied() else {
            return;
        };
        let route = (leg.from, leg.to);
        let type_id = plane.type_id;
        let t = self.world.aircraft_type(type_id);
        let seats = t.seats;
        let service = a.service;
        let price = self.price_of(a, route);
        let Some(rd) = self.route_days.get(&route) else {
            return;
        };
        let attr = economy::attractiveness(rd.reference, price, service, leg.dep_min);
        let total: u64 = rd
            .flights
            .iter()
            .map(|f| {
                if f.airline == airline_id && f.aircraft == aircraft && f.dep_min == leg.dep_min {
                    attr as u64
                } else if let Some(fixed) = f.departed_attr {
                    fixed as u64
                } else {
                    let other = &self.airlines[&f.airline];
                    economy::attractiveness(
                        rd.reference,
                        self.price_of(other, route),
                        other.service,
                        f.dep_min,
                    ) as u64
                }
            })
            .sum();
        let pax = economy::passengers(rd.demand, attr, total, seats);
        let distance = self.world.distance(leg.from, leg.to);
        let block = schedule::block_minutes(&self.world, type_id, leg.from, leg.to);
        let day = self.day();
        let disrupted = self.is_disrupted(day, leg.from, leg.to, leg.dep_min, leg.dep_min + block);
        let slot = self.world.airport(leg.from).slot_constrained
            || self.world.airport(leg.to).slot_constrained;
        let cost = if disrupted {
            economy::COMPENSATION_PER_PAX.times(pax as i64)
        } else {
            economy::flight_cost(
                t,
                distance,
                block,
                self.fuel_haler_per_l,
                pax,
                service,
                slot,
            )
        };
        if let Some(rd) = self.route_days.get_mut(&route) {
            if let Some(f) = rd.flights.iter_mut().find(|f| {
                f.airline == airline_id && f.aircraft == aircraft && f.dep_min == leg.dep_min
            }) {
                f.departed_attr = Some(attr);
            }
        }
        let airline = self.airline_mut(airline_id);
        if disrupted {
            airline.cash -= cost;
            airline.stats.cost_today += cost;
            airline.stats.cancelled_flights += 1;
            events.push(Event::FlightCancelled {
                airline: airline_id,
                aircraft,
                from: leg.from,
                to: leg.to,
                compensation: cost,
            });
            return;
        }
        let revenue = price.times(pax as i64);
        airline.cash += revenue - cost;
        airline.stats.revenue_today += revenue;
        airline.stats.cost_today += cost;
        airline.stats.total_pax += pax as u64;
        airline.stats.total_seats += seats as u64;
        airline.stats.flights += 1;
        let rs = airline.stats.today.entry(route).or_default();
        rs.pax += pax;
        rs.seats += seats;
        rs.revenue += revenue;
        events.push(Event::FlightDeparted {
            airline: airline_id,
            aircraft,
            from: leg.from,
            to: leg.to,
            pax,
            seats,
            revenue,
            cost,
        });
    }

    fn spawn_contract(&mut self, events: &mut Vec<Event>) {
        let world = &self.world;
        let origin_w: Vec<u64> = world
            .airports
            .iter()
            .map(|a| a.population_k as u64 * a.wealth as u64 / 100)
            .collect();
        let Some(from) = self.rng.weighted(&origin_w) else {
            return;
        };
        let from = AirportId(from as u16);
        let dest_w: Vec<u64> = world
            .airport_ids()
            .map(|to| {
                let d = world.distance(from, to);
                if to == from || !(300..=3500).contains(&d) {
                    0
                } else {
                    let ap = world.airport(to);
                    (ap.tourism as u64 + 1) * (ap.population_k as u64 / 100 + 5)
                }
            })
            .collect();
        let Some(to) = self.rng.weighted(&dest_w) else {
            return;
        };
        let to = AirportId(to as u16);
        let pax = self.rng.range(40, 180) as u32;
        let day = self.day() + self.rng.range(3, 14);
        let fare = economy::reference_fare(self.world.distance(from, to));
        // Charter platí celé letadlo včetně prázdných přeletů, proto násobek ceny letenky.
        let reward = fare
            .times(pax as i64)
            .permille(self.rng.range(2000, 3000) as i64);
        let offer_until = self.now + self.rng.range(120, 480);
        let id = ContractId(self.next_contract);
        self.next_contract += 1;
        self.contracts.insert(
            id,
            Contract {
                id,
                from,
                to,
                pax,
                day,
                reward,
                offer_until,
                window: None,
                status: ContractStatus::Offered,
            },
        );
        self.open_contracts.insert(id);
        events.push(Event::ContractOffered { contract: id });
    }

    fn spawn_auction(&mut self, events: &mut Vec<Event>) {
        let candidates: Vec<AirportId> = self
            .world
            .airport_ids()
            .filter(|ap| self.world.airport(*ap).slot_constrained)
            .filter(|ap| {
                !self
                    .open_auctions
                    .iter()
                    .any(|id| self.auctions[id].airport == *ap)
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        let airport = candidates[self.rng.below(candidates.len() as u64) as usize];
        let id = AuctionId(self.next_auction);
        self.next_auction += 1;
        let end_due = self.now + self.rules.auction_duration_min;
        self.auctions.insert(
            id,
            Auction {
                id,
                airport,
                end_due,
                protection_due: None,
                high_bid: None,
                status: AuctionStatus::Open,
            },
        );
        self.open_auctions.insert(id);
        events.push(Event::AuctionOpened {
            auction: id,
            airport,
            end_due,
        });
    }

    // ------------------------------------------------------ uložení hry

    pub fn save_bytes(&self) -> Vec<u8> {
        let mut out = SAVE_MAGIC.to_vec();
        out.extend(hash::canonical_bytes(self));
        out
    }

    pub fn load_bytes(bytes: &[u8]) -> Result<GameState, LoadError> {
        let body = bytes
            .strip_prefix(SAVE_MAGIC.as_slice())
            .ok_or(LoadError::NotASave)?;
        let state: GameState = postcard::from_bytes(body).map_err(|_| LoadError::Corrupt)?;
        if state.format_version != FORMAT_VERSION {
            return Err(LoadError::UnsupportedVersion(state.format_version));
        }
        Ok(state)
    }
}
