//! AI soupeři. Rozhodují mimo `step`, nejvýš jednou za `ai_decision_interval`
//! herních minut, a jejich záměry dorazí s reakční dobou 50–150 herních
//! minut stejnou cestou jako záměry hráčů (návrh: Férovost a AI soupeři).
//! Vidí jen veřejná data soupeřů (ceny, síť linek, aukce) a svá vlastní.

use serde::{Deserialize, Serialize};

use crate::calendar::date_of_day;
use crate::economy;
use crate::engine::GameState;
use crate::intent::Intent;
use crate::model::{
    Actor, Aircraft, AircraftTypeId, Airline, AirlineId, AirportId, ContractStatus, Leg, Ownership,
    Strategy,
};
use crate::money::Money;
use crate::rng::Rng;
use crate::schedule;
use crate::time::minute_of_day;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiState {
    pub strategy: Strategy,
    /// Vlastní generátor AI, uložený s kampaní.
    pub rng: Rng,
    pub next_decision: u64,
    /// Rozhodnuté záměry čekající na reakční dobu: (krok splatnosti, záměr).
    pub pending: Vec<(u64, Intent)>,
}

impl AiState {
    pub fn new(strategy: Strategy, rng: Rng) -> AiState {
        AiState {
            strategy,
            rng,
            next_decision: 0,
            pending: Vec::new(),
        }
    }

    /// Po bankrotu: zahodit rozhodnutí staré aerolinky.
    pub fn reset(&mut self, now: u64) {
        self.pending.clear();
        self.next_decision = now;
    }
}

impl GameState {
    /// Rozhodne AI aerolinky (je-li čas) a vrátí jejich záměry splatné na
    /// aktuální hranici kroku. Volající je aplikuje stejnou frontou jako
    /// záměry hráčů a zapíše do logu.
    pub fn ai_tick(&mut self) -> Vec<(Actor, Intent)> {
        let now = self.now;
        let (lo, hi) = self.rules.ai_reaction_min;
        let interval = self.rules.ai_decision_interval;
        let ids: Vec<AirlineId> = self.ai.keys().copied().collect();
        let mut out = Vec::new();
        for id in ids {
            if self.ai[&id].next_decision <= now && self.airlines.contains_key(&id) {
                let mut rng = self.ai[&id].rng.clone();
                let decided = decide(self, id, &mut rng);
                let st = self.ai.get_mut(&id).expect("AI aerolinka");
                st.rng = rng;
                st.next_decision = now + interval;
                for intent in decided {
                    let delay = st.rng.range(lo, hi);
                    st.pending.push((now + delay, intent));
                }
            }
            let st = self.ai.get_mut(&id).expect("AI aerolinka");
            if st.pending.iter().any(|(due, _)| *due <= now) {
                let mut keep = Vec::with_capacity(st.pending.len());
                for (due, intent) in std::mem::take(&mut st.pending) {
                    if due <= now {
                        out.push((Actor::Airline(id), intent));
                    } else {
                        keep.push((due, intent));
                    }
                }
                st.pending = keep;
            }
        }
        out
    }
}

/// Cenové pásmo strategie: (min, max, výchozí) v promile referenční ceny.
fn price_band(strategy: Strategy) -> (i64, i64, i64) {
    match strategy {
        Strategy::Levna => (700, 1050, 850),
        Strategy::Premiova => (950, 1600, 1150),
        Strategy::Hub => (800, 1300, 1000),
        Strategy::Charter => (700, 1200, 950),
    }
}

fn preferred_types(strategy: Strategy) -> &'static [&'static str] {
    match strategy {
        Strategy::Levna => &["B738", "A20N", "AT72"],
        Strategy::Premiova => &["BCS3", "E295", "AT72"],
        Strategy::Hub => &["A20N", "E295", "DH8D", "AT72"],
        Strategy::Charter => &["B738", "A21N", "AT72"],
    }
}

/// Kolik je AI ochotná zaplatit za slot.
fn slot_willingness(strategy: Strategy) -> Money {
    match strategy {
        Strategy::Hub => Money::kc(15_000_000),
        Strategy::Premiova => Money::kc(10_000_000),
        Strategy::Levna => Money::kc(4_000_000),
        Strategy::Charter => Money::ZERO,
    }
}

fn pending_has(pending: &[(u64, Intent)], pred: impl Fn(&Intent) -> bool) -> bool {
    pending.iter().any(|(_, i)| pred(i))
}

fn decide(s: &GameState, id: AirlineId, rng: &mut Rng) -> Vec<Intent> {
    let a = &s.airlines[&id];
    let st = &s.ai[&id];
    let strategy = st.strategy;
    let pending = &st.pending;
    let mut out = Vec::new();
    let today = s.day();
    let month = date_of_day(today).month;

    // Finance: zalepit díru úvěrem, splatit, když přebývá.
    if a.cash.is_negative() {
        let available = s.loan_limit(a) - a.loans;
        let amount = (-a.cash + Money::kc(3_000_000)).min(available);
        if amount.0 > 0 && !pending_has(pending, |i| matches!(i, Intent::TakeLoan { .. })) {
            out.push(Intent::TakeLoan { amount });
        }
    } else if a.loans.0 > 0
        && a.cash > a.loans + Money::kc(40_000_000)
        && !pending_has(pending, |i| matches!(i, Intent::RepayLoan { .. }))
    {
        out.push(Intent::RepayLoan { amount: a.loans });
    }

    let first_of_day = minute_of_day(s.now) < s.rules.ai_decision_interval;
    let losing = a.stats.cost_yesterday > a.stats.revenue_yesterday;

    // Zmenšení flotily: když včera prodělala, vrátí nejhůř vytížené pronajaté letadlo.
    if first_of_day
        && losing
        && a.fleet.len() > 2
        && !pending_has(pending, |i| matches!(i, Intent::ReturnAircraft { .. }))
    {
        let worst = a
            .fleet
            .values()
            .filter(|p| {
                matches!(p.ownership, Ownership::Leased { .. })
                    && !p.charters.keys().any(|d| *d >= today)
            })
            .min_by_key(|p| (yesterday_load(a, p), p.id));
        if let Some(p) = worst
            && (yesterday_load(a, p) < 60 || a.cash < Money::kc(5_000_000))
        {
            out.push(Intent::ReturnAircraft { aircraft: p.id });
        }
    }

    // Ceny jednou denně podle včerejší obsazenosti.
    if first_of_day {
        let (lo, hi, _) = price_band(strategy);
        for (route, rs) in &a.stats.yesterday {
            if rs.seats == 0 {
                continue;
            }
            let load = rs.pax * 100 / rs.seats;
            let factor = match load {
                93.. => 1060,
                0..=59 => 940,
                _ => continue,
            };
            let reference = s.reference_price(*route);
            let current = s.price_of(a, *route);
            let next = current
                .permille(factor)
                .max(reference.permille(lo))
                .min(reference.permille(hi));
            if next != current {
                out.push(Intent::SetPrice {
                    from: route.0,
                    to: route.1,
                    price: next,
                });
            }
        }
    }

    // Nevyužitá letadla: naplánovat kyvadlo z domovského letiště.
    let idle: Vec<&Aircraft> = a.fleet.values().filter(|p| p.schedule.is_empty()).collect();
    // Charterová strategie drží jedno letadlo volné pro zakázky.
    let reserve = usize::from(strategy == Strategy::Charter && a.fleet.len() > 1);
    let mut planned_dests: Vec<AirportId> = Vec::new();
    for plane in idle.iter().skip(reserve) {
        if pending_has(
            pending,
            |i| matches!(i, Intent::SetSchedule { aircraft, .. } if *aircraft == plane.id),
        ) {
            continue;
        }
        if let Some((dest, legs)) = plan_route(s, a, strategy, plane, &planned_dests, month, rng) {
            planned_dests.push(dest);
            out.push(Intent::SetSchedule {
                aircraft: plane.id,
                legs,
            });
            let (_, _, start) = price_band(strategy);
            for route in [(a.home, dest), (dest, a.home)] {
                if !a.prices.contains_key(&route) {
                    out.push(Intent::SetPrice {
                        from: route.0,
                        to: route.1,
                        price: s.reference_price(route).permille(start),
                    });
                }
            }
        }
    }

    // Růst flotily, když jsou všechna letadla v provozu a vydělávají.
    let fleet_busy = idle.len() <= reserve;
    let (pax, seats) = a.stats.yesterday.values().fold((0u64, 0u64), |(p, c), r| {
        (p + r.pax as u64, c + r.seats as u64)
    });
    let load_ok = seats == 0 || pax * 100 / seats >= 65 || a.fleet.len() < 3;
    if fleet_busy
        && load_ok
        && !losing
        && !pending_has(pending, |i| {
            matches!(i, Intent::LeaseAircraft { .. } | Intent::BuyAircraft { .. })
        })
    {
        let leases: Money = a
            .fleet
            .values()
            .filter(|p| matches!(p.ownership, Ownership::Leased { .. }))
            .map(|p| s.world.aircraft_type(p.type_id).lease_per_day)
            .sum();
        let pick = preferred_types(strategy)
            .iter()
            .filter_map(|code| s.world.aircraft_type_by_code(code))
            .find(|t| {
                let lease = s.world.aircraft_type(*t).lease_per_day;
                let deposit = lease.times(economy::LEASE_DEPOSIT_DAYS);
                a.cash >= deposit + (leases + lease).times(30) + Money::kc(2_000_000)
            });
        if let Some(type_id) = pick {
            out.push(Intent::LeaseAircraft { type_id });
        }
    }

    // Burza: charterová strategie bere, co obslouží, ostatní občas.
    for cid in &s.open_contracts {
        let c = &s.contracts[cid];
        if c.window
            .as_ref()
            .is_some_and(|w| w.candidates.contains(&id))
            || (c.window.is_none() && s.now >= c.offer_until)
            || pending_has(
                pending,
                |i| matches!(i, Intent::TakeContract { contract } if contract == cid),
            )
        {
            continue;
        }
        // Bez volného letadla na ten den by výhra skončila penále.
        if free_aircraft_for_day(s, a, pending, c.pax, c.from, c.to, c.day) == 0 {
            continue;
        }
        let want = match strategy {
            Strategy::Charter => true,
            _ => rng.chance(1, 4),
        };
        if want {
            out.push(Intent::TakeContract { contract: *cid });
            // Jedno rozhodnutí smí na den zabrat jen tolik zakázek, kolik je letadel.
            break;
        }
    }
    for cid in &s.active_contracts {
        let c = &s.contracts[cid];
        if c.status != ContractStatus::Awarded(id)
            || c.day <= today
            || pending_has(
                pending,
                |i| matches!(i, Intent::AssignCharter { contract, .. } if contract == cid),
            )
        {
            continue;
        }
        if let Some(aircraft) = suitable_aircraft(s, a, c.pax, c.from, c.to, c.day) {
            out.push(Intent::AssignCharter {
                contract: *cid,
                aircraft,
            });
        }
    }

    // Aukce slotů: jen když nemá nevyužité sloty (nekupovat do zásoby).
    let willingness = slot_willingness(strategy);
    let unused_slots: u32 = a
        .slots
        .iter()
        .map(|(airport, owned)| owned.saturating_sub(s.departures_from(a, *airport, None)))
        .sum();
    for aid in &s.open_auctions {
        let auction = &s.auctions[aid];
        if unused_slots > 0
            || auction.high_bid.is_some_and(|(who, _)| who == id)
            || pending_has(
                pending,
                |i| matches!(i, Intent::Bid { auction, .. } if auction == aid),
            )
        {
            continue;
        }
        let min = s.min_bid(auction);
        if min <= willingness && a.cash >= min + Money::kc(5_000_000) {
            out.push(Intent::Bid {
                auction: *aid,
                amount: min,
            });
        }
    }
    out
}

/// Včerejší obsazenost linek v plánu letadla v procentech (nevyužité = 0).
fn yesterday_load(a: &Airline, plane: &Aircraft) -> u32 {
    let (pax, seats) = plane
        .schedule
        .iter()
        .filter_map(|l| a.stats.yesterday.get(&(l.from, l.to)))
        .fold((0u32, 0u32), |(p, c), r| (p + r.pax, c + r.seats));
    (pax * 100).checked_div(seats).unwrap_or(0)
}

/// Kolik vhodných letadel zbývá na den zakázky po odečtení zakázek, které
/// už aerolinka vyhrála nebo o ně právě žádá.
fn free_aircraft_for_day(
    s: &GameState,
    a: &Airline,
    pending: &[(u64, Intent)],
    pax: u32,
    from: AirportId,
    to: AirportId,
    day: u64,
) -> usize {
    let distance = s.world.distance(from, to);
    let suitable = a
        .fleet
        .values()
        .filter(|p| {
            let t = s.world.aircraft_type(p.type_id);
            t.seats >= pax && t.range_km >= distance && !p.charters.contains_key(&day)
        })
        .count();
    let same_day =
        |cid: &crate::model::ContractId| s.contracts.get(cid).is_some_and(|c| c.day == day);
    let won = s
        .active_contracts
        .iter()
        .filter(|cid| same_day(cid) && s.contracts[*cid].status == ContractStatus::Awarded(a.id))
        .count();
    let candidate = s
        .open_contracts
        .iter()
        .filter(|cid| {
            same_day(cid)
                && s.contracts[*cid]
                    .window
                    .as_ref()
                    .is_some_and(|w| w.candidates.contains(&a.id))
        })
        .count();
    let asking = pending
        .iter()
        .filter(|(_, i)| matches!(i, Intent::TakeContract { contract } if same_day(contract)))
        .count();
    suitable.saturating_sub(won + candidate + asking)
}

/// Nejmenší vhodné letadlo pro zakázku, přednostně nevyužité.
fn suitable_aircraft(
    s: &GameState,
    a: &Airline,
    pax: u32,
    from: AirportId,
    to: AirportId,
    day: u64,
) -> Option<crate::model::AircraftId> {
    let distance = s.world.distance(from, to);
    a.fleet
        .values()
        .filter(|p| {
            let t = s.world.aircraft_type(p.type_id);
            t.seats >= pax && t.range_km >= distance && !p.charters.contains_key(&day)
        })
        .min_by_key(|p| {
            (
                !p.schedule.is_empty(),
                s.world.aircraft_type(p.type_id).seats,
                p.id,
            )
        })
        .map(|p| p.id)
}

/// Vybere nejvýnosnější kyvadlovou linku z domovského letiště podle odhadu.
fn plan_route(
    s: &GameState,
    a: &Airline,
    strategy: Strategy,
    plane: &Aircraft,
    already_planned: &[AirportId],
    month: u32,
    rng: &mut Rng,
) -> Option<(AirportId, Vec<Leg>)> {
    let base = a.home;
    let type_id: AircraftTypeId = plane.type_id;
    let t = s.world.aircraft_type(type_id);
    let (_, _, start_permille) = price_band(strategy);
    let start = 6 * 60 + rng.below(45) as u32;
    let mut best: Option<(i64, AirportId, Vec<Leg>)> = None;
    for dest in s.world.airport_ids() {
        let distance = s.world.distance(base, dest);
        if dest == base || distance < 250 || distance > t.range_km {
            continue;
        }
        let mut legs = schedule::shuttle(&s.world, type_id, base, dest, start, 23 * 60 + 30);
        if legs.is_empty() {
            continue;
        }
        for airport in [base, dest] {
            if s.world.airport(airport).slot_constrained {
                let owned = a.slots.get(&airport).copied().unwrap_or(0);
                let used = s.departures_from(a, airport, None);
                let free = owned.saturating_sub(used) as usize;
                legs.truncate(legs.len().min(2 * free));
            }
        }
        if legs.is_empty() {
            continue;
        }
        let rotations = (legs.len() / 2) as u64;
        let route = (base, dest);
        let rivals = s
            .airlines
            .values()
            .filter(|o| {
                o.id != a.id && (s.serves_route(o, route) || s.serves_route(o, (dest, base)))
            })
            .count() as u64;
        let mine = a
            .fleet
            .values()
            .filter(|p| p.schedule.iter().any(|l| (l.from, l.to) == route))
            .count() as u64;
        let demand = s.base_demand.seasonal(base, dest, month) as u64;
        let pax_per_leg = (demand / (1 + rivals + mine) / rotations).min(t.seats as u64) as u32;
        let price = s.reference_price(route).permille(start_permille);
        let block = schedule::block_minutes(&s.world, type_id, base, dest);
        let slot = s.world.airport(base).slot_constrained || s.world.airport(dest).slot_constrained;
        let leg_cost = economy::flight_cost(
            t,
            distance,
            block,
            s.fuel_haler_per_l,
            pax_per_leg,
            a.service,
            slot,
        );
        let legs_n = 2 * rotations as i64;
        let mut profit = (price.times(pax_per_leg as i64) - leg_cost).times(legs_n).0;
        match strategy {
            Strategy::Levna if distance > 1800 => profit = profit * 7 / 10,
            Strategy::Premiova => profit = profit * s.world.airport(dest).wealth as i64 / 100,
            Strategy::Hub if mine > 0 => profit /= 2,
            _ => {}
        }
        if already_planned.contains(&dest) {
            profit /= 2;
        }
        // Mírný šum, aby AI nebyly předvídatelné.
        profit = profit * (900 + rng.below(200) as i64) / 1000;
        if profit > 0 && best.as_ref().is_none_or(|(p, _, _)| profit > *p) {
            best = Some((profit, dest, legs));
        }
    }
    best.map(|(_, dest, legs)| (dest, legs))
}
