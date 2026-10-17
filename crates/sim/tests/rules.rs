//! Testy schválených pravidel simulace (návrh, Test Plan Artifact).

use aae_sim::engine::FORMAT_VERSION;
use aae_sim::model::{Auction, AuctionStatus, Contract, ContractStatus};
use aae_sim::schedule::{self, ScheduleError};
use aae_sim::{
    Actor, AirlineId, AuctionId, CampaignRules, ContractId, Controller, Event, GameState, Intent,
    Money, NewAirline, PauseReason, Reject, Speed, StepError,
};

const A: AirlineId = AirlineId(0);
const B: AirlineId = AirlineId(1);

/// Dva lidští hráči (Praha, Vídeň), bez náhodných událostí, burzy a aukcí,
/// aby testy řídily vše samy.
fn two_humans() -> GameState {
    two_humans_seeded(7)
}

fn two_humans_seeded(seed: u64) -> GameState {
    let rules = CampaignRules {
        duration_days: 60,
        events: false,
        contracts_per_day: 0,
        auction_every_days: 0,
        ..CampaignRules::default()
    };
    let world = aae_sim::data::World::europe_v1().unwrap();
    let prg = world.airport_by_code("PRG").unwrap();
    let vie = world.airport_by_code("VIE").unwrap();
    GameState::with_world(
        world,
        rules,
        seed,
        vec![
            NewAirline {
                name: "Vltava Air".into(),
                home: prg,
                controller: Controller::Human,
            },
            NewAirline {
                name: "Danube Wings".into(),
                home: vie,
                controller: Controller::Human,
            },
        ],
    )
    .unwrap()
}

fn ap(s: &GameState, code: &str) -> aae_sim::AirportId {
    s.world.airport_by_code(code).unwrap()
}

fn steps(s: &mut GameState, n: u64) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..n {
        all.extend(s.step().expect("krok"));
    }
    all
}

fn to_next_day(s: &mut GameState) -> Vec<Event> {
    let remaining = 1440 - s.now % 1440;
    steps(s, remaining)
}

fn insert_contract(s: &mut GameState, offer_minutes: u64, pax: u32, day: u64) -> ContractId {
    let id = ContractId(900);
    let (from, to) = (ap(s, "PRG"), ap(s, "PMI"));
    s.contracts.insert(
        id,
        Contract {
            id,
            from,
            to,
            pax,
            day,
            reward: Money::kc(500_000),
            offer_until: s.now + offer_minutes,
            window: None,
            status: ContractStatus::Offered,
        },
    );
    s.open_contracts.insert(id);
    id
}

fn insert_auction(s: &mut GameState, minutes: u64) -> AuctionId {
    let id = AuctionId(900);
    let airport = ap(s, "LHR");
    s.auctions.insert(
        id,
        Auction {
            id,
            airport,
            end_due: s.now + minutes,
            protection_due: None,
            high_bid: None,
            status: AuctionStatus::Open,
        },
    );
    s.open_auctions.insert(id);
    id
}

/// Kampaň se 4 AI jako v laboratoři.
fn ai_campaign(seed: u64, days: u64) -> GameState {
    let rules = CampaignRules {
        duration_days: days,
        ..CampaignRules::default()
    };
    let world = aae_sim::data::World::europe_v1().unwrap();
    let homes = ["PRG", "VIE", "MUC", "WAW"];
    let airlines = aae_sim::Strategy::ALL
        .iter()
        .zip(homes)
        .map(|(s, h)| NewAirline {
            name: s.name().into(),
            home: world.airport_by_code(h).unwrap(),
            controller: Controller::Ai(*s),
        })
        .collect();
    GameState::with_world(world, rules, seed, airlines).unwrap()
}

fn run_until(s: &mut GameState, day: u64) {
    while s.day() < day {
        for (actor, intent) in s.ai_tick() {
            let _ = s.apply(actor, intent);
        }
        if s.step() == Err(StepError::Finished) {
            break;
        }
    }
}

// ------------------------------------------------------------ determinismus

#[test]
fn same_seed_same_hash_different_seed_differs() {
    let mut a = ai_campaign(11, 30);
    let mut b = ai_campaign(11, 30);
    let mut c = ai_campaign(12, 30);
    run_until(&mut a, 20);
    run_until(&mut b, 20);
    run_until(&mut c, 20);
    assert_eq!(a.hash(), b.hash());
    assert_ne!(a.hash(), c.hash());
}

#[test]
fn save_load_continues_identically() {
    let mut continuous = ai_campaign(3, 40);
    run_until(&mut continuous, 10);
    let bytes = continuous.save_bytes();
    let mut loaded = GameState::load_bytes(&bytes).unwrap();
    assert_eq!(loaded.hash(), continuous.hash());
    assert_eq!(loaded.format_version, FORMAT_VERSION);
    run_until(&mut continuous, 20);
    run_until(&mut loaded, 20);
    // Náhoda (generátor ve stavu) i AI pokračují stejně, neopakují se od začátku.
    assert_eq!(loaded.hash(), continuous.hash());
}

#[test]
fn load_rejects_garbage() {
    assert!(GameState::load_bytes(b"nic").is_err());
    let mut bytes = two_humans().save_bytes();
    bytes.truncate(bytes.len() / 2);
    assert!(GameState::load_bytes(&bytes).is_err());
}

#[test]
fn two_processes_produce_the_same_hash() {
    let lab = env!("CARGO_BIN_EXE_lab");
    let run = || {
        let out = std::process::Command::new(lab)
            .args(["--hash", "--seed", "5", "--days", "15"])
            .output()
            .expect("spuštění laboratoře");
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    let first = run();
    assert_eq!(first.len(), 16, "hash: {first}");
    assert_eq!(first, run());
}

// ------------------------------------------------------------------ pauza

#[test]
fn pause_is_a_set_of_reasons() {
    let mut s = two_humans();
    s.apply(Actor::Airline(A), Intent::Pause(PauseReason::Player(A)))
        .unwrap();
    assert_eq!(s.step(), Err(StepError::Paused));
    s.apply(Actor::System, Intent::Pause(PauseReason::Auto))
        .unwrap();
    // Hráčova pauza skončí, automatická trvá dál.
    s.apply(Actor::Airline(A), Intent::Resume(PauseReason::Player(A)))
        .unwrap();
    assert_eq!(s.step(), Err(StepError::Paused));
    s.apply(Actor::System, Intent::Resume(PauseReason::Auto))
        .unwrap();
    assert!(s.step().is_ok());
}

#[test]
fn pause_permissions() {
    let mut s = two_humans();
    assert_eq!(
        s.apply(Actor::Airline(A), Intent::Pause(PauseReason::Technical)),
        Err(Reject::NotPermitted)
    );
    assert_eq!(
        s.apply(Actor::Airline(A), Intent::Pause(PauseReason::Player(B))),
        Err(Reject::NotPermitted)
    );
    s.apply(Actor::Airline(A), Intent::Pause(PauseReason::Player(A)))
        .unwrap();
    assert_eq!(
        s.apply(Actor::Airline(B), Intent::Resume(PauseReason::Player(A))),
        Err(Reject::NotPermitted)
    );
    // Hostitel smí cizí hráčskou pauzu ukončit.
    s.apply(Actor::System, Intent::Resume(PauseReason::Player(A)))
        .unwrap();
    assert!(!s.is_paused());
}

#[test]
fn market_closed_during_pause_but_planning_allowed() {
    let mut s = two_humans();
    let contract = insert_contract(&mut s, 120, 60, 5);
    s.apply(Actor::System, Intent::Pause(PauseReason::Technical))
        .unwrap();
    let (prg, vie) = (ap(&s, "PRG"), ap(&s, "VIE"));
    let price = s.reference_price((prg, vie));
    assert!(
        s.apply(
            Actor::Airline(A),
            Intent::SetPrice {
                from: prg,
                to: vie,
                price
            }
        )
        .is_ok()
    );
    assert_eq!(
        s.apply(Actor::Airline(A), Intent::TakeContract { contract }),
        Err(Reject::MarketClosedDuringPause)
    );
}

// ------------------------------------------------------------------ burza

#[test]
fn collection_window_draws_among_candidates() {
    let mut s = two_humans();
    let contract = insert_contract(&mut s, 120, 60, 5);
    let opened = s.now;
    s.apply(Actor::Airline(A), Intent::TakeContract { contract })
        .unwrap();
    assert_eq!(
        s.contracts[&contract].window.as_ref().unwrap().due,
        opened + 16
    );
    steps(&mut s, 5);
    s.apply(Actor::Airline(B), Intent::TakeContract { contract })
        .unwrap();
    assert_eq!(
        s.apply(Actor::Airline(B), Intent::TakeContract { contract }),
        Err(Reject::AlreadyCandidate)
    );
    let events = steps(&mut s, 11);
    let awarded = events.iter().find_map(|e| match e {
        Event::ContractAwarded {
            winner, candidates, ..
        } => Some((*winner, candidates.clone())),
        _ => None,
    });
    let (winner, candidates) = awarded.expect("los proběhl v termínu okna");
    assert_eq!(candidates, vec![A, B]);
    assert!(winner == A || winner == B);
    assert_eq!(
        s.apply(Actor::Airline(A), Intent::TakeContract { contract }),
        Err(Reject::ContractNotOffered)
    );
}

#[test]
fn lottery_is_not_biased_to_first_candidate() {
    let mut wins = [0u32; 2];
    for seed in 0..40 {
        let mut s = two_humans_seeded(seed);
        let contract = insert_contract(&mut s, 120, 60, 5);
        s.apply(Actor::Airline(A), Intent::TakeContract { contract })
            .unwrap();
        s.apply(Actor::Airline(B), Intent::TakeContract { contract })
            .unwrap();
        steps(&mut s, 16);
        match s.contracts[&contract].status {
            ContractStatus::Awarded(w) => wins[w.0 as usize] += 1,
            other => panic!("los neproběhl: {other:?}"),
        }
    }
    // Pravděpodobnost, že férový los dá 40× stejného vítěze, je ~2e-12.
    assert!(wins[0] > 0 && wins[1] > 0, "výhry {wins:?}");
}

#[test]
fn open_window_holds_contract_past_offer_end() {
    let mut s = two_humans();
    let contract = insert_contract(&mut s, 3, 60, 5);
    s.apply(Actor::Airline(A), Intent::TakeContract { contract })
        .unwrap();
    steps(&mut s, 10);
    assert_eq!(
        s.contracts[&contract].status,
        ContractStatus::Offered,
        "okno ještě běží"
    );
    steps(&mut s, 6);
    assert_eq!(s.contracts[&contract].status, ContractStatus::Awarded(A));
}

#[test]
fn offer_without_takers_expires() {
    let mut s = two_humans();
    let contract = insert_contract(&mut s, 3, 60, 5);
    steps(&mut s, 3);
    assert_eq!(s.contracts[&contract].status, ContractStatus::Expired);
}

#[test]
fn speed_change_rescales_open_window() {
    let mut s = two_humans();
    let contract = insert_contract(&mut s, 120, 60, 5);
    let now = s.now;
    s.apply(Actor::Airline(A), Intent::TakeContract { contract })
        .unwrap();
    assert_eq!(
        s.apply(Actor::Airline(A), Intent::SetSpeed(Speed::X4)),
        Err(Reject::NotPermitted)
    );
    s.apply(Actor::System, Intent::SetSpeed(Speed::X4)).unwrap();
    // 16 kroků při 1× = 1 s; při 4× je to 64 kroků.
    assert_eq!(
        s.contracts[&contract].window.as_ref().unwrap().due,
        now + 64
    );
}

// ------------------------------------------------------------------ aukce

#[test]
fn late_bid_extends_auction_by_protection() {
    let mut s = two_humans();
    let auction = insert_auction(&mut s, 100);
    let end = s.auctions[&auction].end_due;
    assert_eq!(
        s.apply(
            Actor::Airline(A),
            Intent::Bid {
                auction,
                amount: Money::kc(1_000_000)
            }
        ),
        Err(Reject::BidTooLow {
            min: Money::kc(3_000_000)
        })
    );
    s.apply(
        Actor::Airline(A),
        Intent::Bid {
            auction,
            amount: Money::kc(3_000_000),
        },
    )
    .unwrap();
    assert_eq!(
        s.auctions[&auction].protection_due, None,
        "příhoz mimo ochrannou lhůtu"
    );
    steps(&mut s, 90);
    let min = s.min_bid(&s.auctions[&auction]);
    assert_eq!(min, Money::kc(3_150_000));
    s.apply(
        Actor::Airline(B),
        Intent::Bid {
            auction,
            amount: min,
        },
    )
    .unwrap();
    // 3 s při 1× = 48 kroků navíc k původnímu konci.
    assert_eq!(s.auctions[&auction].effective_end(), end + 48);
    let cash_before = s.airlines[&B].cash;
    steps(&mut s, 10 + 48);
    assert_eq!(s.auctions[&auction].status, AuctionStatus::Won(B, min));
    assert_eq!(s.airlines[&B].slots.get(&ap(&s, "LHR")), Some(&1));
    assert_eq!(s.airlines[&B].cash, cash_before - min);
}

#[test]
fn protection_is_rescaled_on_speed_change() {
    let mut s = two_humans();
    let auction = insert_auction(&mut s, 20);
    s.apply(
        Actor::Airline(A),
        Intent::Bid {
            auction,
            amount: Money::kc(3_000_000),
        },
    )
    .unwrap();
    let protection = s.auctions[&auction]
        .protection_due
        .expect("příhoz v ochranné lhůtě");
    let remaining = protection - s.now;
    s.apply(Actor::System, Intent::SetSpeed(Speed::X2)).unwrap();
    assert_eq!(
        s.auctions[&auction].protection_due,
        Some(s.now + remaining * 2)
    );
}

// ------------------------------------------------- plán, poptávka, sloty

#[test]
fn schedule_applies_from_next_day() {
    let mut s = two_humans();
    let (prg, vie) = (ap(&s, "PRG"), ap(&s, "VIE"));
    let plane = *s.airlines[&A].fleet.keys().next().unwrap();
    let t = s.airlines[&A].fleet[&plane].type_id;
    let legs = schedule::shuttle(&s.world, t, prg, vie, 8 * 60, 22 * 60);
    s.apply(
        Actor::Airline(A),
        Intent::SetSchedule {
            aircraft: plane,
            legs: legs.clone(),
        },
    )
    .unwrap();
    let today = to_next_day(&mut s);
    assert!(
        !today
            .iter()
            .any(|e| matches!(e, Event::FlightDeparted { .. }))
    );
    let tomorrow = to_next_day(&mut s);
    let flown = tomorrow
        .iter()
        .filter(|e| matches!(e, Event::FlightDeparted { airline, .. } if *airline == A))
        .count();
    assert_eq!(flown, legs.len());
}

#[test]
fn demand_split_favours_cheaper_flight() {
    let mut s = two_humans();
    let (prg, vie) = (ap(&s, "PRG"), ap(&s, "VIE"));
    // Obě aerolinky létají na lince PRG→VIE, A je o 30 % levnější.
    for (airline, base, dest) in [(A, prg, vie), (B, vie, prg)] {
        let plane = *s.airlines[&airline].fleet.keys().next().unwrap();
        let t = s.airlines[&airline].fleet[&plane].type_id;
        let legs = schedule::shuttle(&s.world, t, base, dest, 7 * 60, 22 * 60);
        s.apply(
            Actor::Airline(airline),
            Intent::SetSchedule {
                aircraft: plane,
                legs,
            },
        )
        .unwrap();
    }
    let reference = s.reference_price((prg, vie));
    s.apply(
        Actor::Airline(A),
        Intent::SetPrice {
            from: prg,
            to: vie,
            price: reference.permille(700),
        },
    )
    .unwrap();
    to_next_day(&mut s);
    let events = to_next_day(&mut s);
    let pax_of = |who: AirlineId| -> u32 {
        events
            .iter()
            .filter_map(|e| match e {
                Event::FlightDeparted {
                    airline,
                    from,
                    to,
                    pax,
                    ..
                } if *airline == who && *from == prg && *to == vie => Some(*pax),
                _ => None,
            })
            .sum()
    };
    let (a, b) = (pax_of(A), pax_of(B));
    assert!(a > b, "levnější A {a}, dražší B {b}");
}

#[test]
fn constrained_airport_needs_slots() {
    let mut s = two_humans();
    let (prg, lhr) = (ap(&s, "PRG"), ap(&s, "LHR"));
    let jet = s.world.aircraft_type_by_code("A20N").unwrap();
    s.airlines.get_mut(&A).unwrap().cash = Money::kc(100_000_000);
    s.apply(Actor::Airline(A), Intent::LeaseAircraft { type_id: jet })
        .unwrap();
    let plane = *s.airlines[&A].fleet.keys().last().unwrap();
    let legs = schedule::shuttle(&s.world, jet, prg, lhr, 7 * 60, 22 * 60);
    assert!(legs.len() >= 4);
    let err = s.apply(
        Actor::Airline(A),
        Intent::SetSchedule {
            aircraft: plane,
            legs: legs.clone(),
        },
    );
    assert!(matches!(
        err,
        Err(Reject::InvalidSchedule(ScheduleError::MissingSlots {
            owned: 0,
            ..
        }))
    ));
    s.airlines.get_mut(&A).unwrap().slots.insert(lhr, 1);
    let one_rotation = legs[..2].to_vec();
    assert!(
        s.apply(
            Actor::Airline(A),
            Intent::SetSchedule {
                aircraft: plane,
                legs: one_rotation
            }
        )
        .is_ok()
    );
}

// ------------------------------------------------------ chartery, bankrot

#[test]
fn awarded_charter_flies_on_its_day() {
    let mut s = two_humans();
    let contract = insert_contract(&mut s, 120, 60, 3);
    s.apply(Actor::Airline(A), Intent::TakeContract { contract })
        .unwrap();
    steps(&mut s, 16);
    assert_eq!(s.contracts[&contract].status, ContractStatus::Awarded(A));
    let plane = *s.airlines[&A].fleet.keys().next().unwrap();
    s.apply(
        Actor::Airline(A),
        Intent::AssignCharter {
            contract,
            aircraft: plane,
        },
    )
    .unwrap();
    let mut flown = false;
    while s.day() < 4 {
        flown |= steps(&mut s, 1)
            .iter()
            .any(|e| matches!(e, Event::CharterFlown { contract: c, .. } if *c == contract));
    }
    assert!(flown);
    assert_eq!(s.contracts[&contract].status, ContractStatus::Done(A));
}

#[test]
fn unassigned_charter_fails_with_penalty() {
    let mut s = two_humans();
    let contract = insert_contract(&mut s, 120, 60, 2);
    s.apply(Actor::Airline(A), Intent::TakeContract { contract })
        .unwrap();
    steps(&mut s, 16);
    let mut penalty = None;
    while s.day() < 3 {
        for e in steps(&mut s, 1) {
            if let Event::ContractFailed { penalty: p, .. } = e {
                penalty = Some(p);
            }
        }
    }
    assert_eq!(penalty, Some(Money::kc(250_000)));
}

#[test]
fn bankruptcy_after_seven_negative_days() {
    let mut s = two_humans();
    s.airlines.get_mut(&A).unwrap().cash = Money::kc(-100_000_000);
    let mut warnings = 0;
    let mut bankrupt = false;
    while s.day() < 8 && !bankrupt {
        for e in steps(&mut s, 1) {
            match e {
                Event::BankruptcyWarning { airline, .. } if airline == A => warnings += 1,
                Event::Bankruptcy {
                    airline,
                    new_capital,
                } if airline == A => {
                    bankrupt = true;
                    assert_eq!(new_capital, Money::kc(12_500_000));
                }
                _ => {}
            }
        }
    }
    assert!(bankrupt);
    assert_eq!(warnings, 6);
    let a = &s.airlines[&A];
    assert_eq!(a.generation, 1);
    assert!(a.fleet.is_empty());
    assert_eq!(a.loans, Money::ZERO);
}

// ------------------------------------------------------------------- AI

#[test]
fn ai_intents_arrive_after_reaction_time() {
    let mut s = ai_campaign(9, 30);
    let decided_at = s.now;
    assert!(
        s.ai_tick().is_empty(),
        "nic není splatné hned po rozhodnutí"
    );
    for ai in s.ai.values() {
        for (due, _) in &ai.pending {
            assert!((decided_at + 50..=decided_at + 150).contains(due));
        }
    }
    assert!(s.ai.values().any(|ai| !ai.pending.is_empty()));
}

#[test]
fn players_cannot_act_for_others_or_as_system() {
    let mut s = two_humans();
    assert_eq!(
        s.apply(Actor::System, Intent::SetService { level: 3 }),
        Err(Reject::NotPermitted)
    );
    let foreign = *s.airlines[&B].fleet.keys().next().unwrap();
    assert_eq!(
        s.apply(
            Actor::Airline(A),
            Intent::ReturnAircraft { aircraft: foreign }
        ),
        Err(Reject::UnknownAircraft)
    );
}

/// Doplňkový test stability: 10 herních let bez přetečení. Pomalejší, proto
/// mimo běžné `cargo test` (spustit `cargo test --release -- --ignored`).
#[test]
#[ignore]
fn ten_years_without_overflow() {
    let mut s = ai_campaign(1, 3653);
    run_until(&mut s, 3653);
    for id in s.airlines.keys() {
        let _ = s.firm_value(*id);
    }
}
