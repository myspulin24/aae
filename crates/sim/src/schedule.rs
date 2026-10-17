//! Denní letový plán letadla: bloková doba a kontrola plánu.
//!
//! Plán je cyklický den: lety jdou po sobě v čase, každý začíná tam, kde
//! předchozí skončil, mezi nimi je doba obrátky a poslední let končí na
//! letišti prvního letu včas na zítřejší první odlet.

use serde::{Deserialize, Serialize};

use crate::data::World;
use crate::model::{AircraftTypeId, AirportId, Leg};
use crate::time::MINUTES_PER_DAY;

pub const MAX_LEGS_PER_DAY: usize = 24;

/// Pojíždění, vzlet a přistání navíc k času letu.
const TAXI_MIN: u32 = 25;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleError {
    TooManyLegs,
    DepartureOutOfDay {
        leg: usize,
    },
    NotSorted {
        leg: usize,
    },
    UnknownAirport {
        leg: usize,
    },
    SameAirport {
        leg: usize,
    },
    OutOfRange {
        leg: usize,
        distance_km: u32,
        range_km: u32,
    },
    NotChained {
        leg: usize,
    },
    TurnaroundTooShort {
        leg: usize,
    },
    NotCyclic,
    CycleTooLong,
    MissingSlots {
        airport: AirportId,
        needed: u32,
        owned: u32,
    },
}

/// Bloková doba letu v minutách (let + pojíždění).
pub fn block_minutes(
    world: &World,
    type_id: AircraftTypeId,
    from: AirportId,
    to: AirportId,
) -> u32 {
    let t = world.aircraft_type(type_id);
    let dist = world.distance(from, to);
    TAXI_MIN + (dist * 60).div_ceil(t.speed_kmh)
}

pub fn validate(world: &World, type_id: AircraftTypeId, legs: &[Leg]) -> Result<(), ScheduleError> {
    if legs.len() > MAX_LEGS_PER_DAY {
        return Err(ScheduleError::TooManyLegs);
    }
    if legs.is_empty() {
        return Ok(());
    }
    let t = world.aircraft_type(type_id);
    for (i, leg) in legs.iter().enumerate() {
        if leg.dep_min as u64 >= MINUTES_PER_DAY {
            return Err(ScheduleError::DepartureOutOfDay { leg: i });
        }
        if i > 0 && leg.dep_min <= legs[i - 1].dep_min {
            return Err(ScheduleError::NotSorted { leg: i });
        }
        if !world.is_valid_airport(leg.from) || !world.is_valid_airport(leg.to) {
            return Err(ScheduleError::UnknownAirport { leg: i });
        }
        if leg.from == leg.to {
            return Err(ScheduleError::SameAirport { leg: i });
        }
        let distance_km = world.distance(leg.from, leg.to);
        if distance_km > t.range_km {
            return Err(ScheduleError::OutOfRange {
                leg: i,
                distance_km,
                range_km: t.range_km,
            });
        }
    }
    for i in 0..legs.len() - 1 {
        let (a, b) = (&legs[i], &legs[i + 1]);
        if a.to != b.from {
            return Err(ScheduleError::NotChained { leg: i + 1 });
        }
        let arrival = a.dep_min + block_minutes(world, type_id, a.from, a.to);
        if arrival + t.turnaround_min > b.dep_min {
            return Err(ScheduleError::TurnaroundTooShort { leg: i + 1 });
        }
    }
    let (first, last) = (&legs[0], &legs[legs.len() - 1]);
    if last.to != first.from {
        return Err(ScheduleError::NotCyclic);
    }
    let last_arrival = last.dep_min + block_minutes(world, type_id, last.from, last.to);
    if last_arrival + t.turnaround_min > first.dep_min + MINUTES_PER_DAY as u32 {
        return Err(ScheduleError::CycleTooLong);
    }
    Ok(())
}

/// Sestaví plán kyvadlových rotací `base ⇄ dest` od `start_min`, dokud se
/// vejdou do dne (pomůcka pro AI a testy).
pub fn shuttle(
    world: &World,
    type_id: AircraftTypeId,
    base: AirportId,
    dest: AirportId,
    start_min: u32,
    last_arrival_min: u32,
) -> Vec<Leg> {
    let t = world.aircraft_type(type_id);
    let block = block_minutes(world, type_id, base, dest);
    let mut legs = Vec::new();
    let mut dep = start_min;
    loop {
        let back_dep = dep + block + t.turnaround_min;
        let back_arrival = back_dep + block;
        if back_arrival > last_arrival_min || legs.len() + 2 > MAX_LEGS_PER_DAY {
            break;
        }
        legs.push(Leg {
            from: base,
            to: dest,
            dep_min: dep,
        });
        legs.push(Leg {
            from: dest,
            to: base,
            dep_min: back_dep,
        });
        dep = back_arrival + t.turnaround_min;
    }
    legs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (World, AircraftTypeId, AirportId, AirportId, AirportId) {
        let w = World::europe_v1().unwrap();
        let atr = w.aircraft_type_by_code("AT72").unwrap();
        let prg = w.airport_by_code("PRG").unwrap();
        let vie = w.airport_by_code("VIE").unwrap();
        let lis = w.airport_by_code("LIS").unwrap();
        (w, atr, prg, vie, lis)
    }

    #[test]
    fn shuttle_is_valid() {
        let (w, atr, prg, vie, _) = setup();
        let legs = shuttle(&w, atr, prg, vie, 6 * 60, 23 * 60);
        assert!(legs.len() >= 4);
        assert_eq!(validate(&w, atr, &legs), Ok(()));
    }

    #[test]
    fn rejects_broken_plans() {
        let (w, atr, prg, vie, lis) = setup();
        let out = Leg {
            from: prg,
            to: vie,
            dep_min: 360,
        };
        // Nekončí doma.
        assert_eq!(validate(&w, atr, &[out]), Err(ScheduleError::NotCyclic));
        // Obrátka příliš krátká.
        let back = Leg {
            from: vie,
            to: prg,
            dep_min: 361,
        };
        assert_eq!(
            validate(&w, atr, &[out, back]),
            Err(ScheduleError::TurnaroundTooShort { leg: 1 })
        );
        // Mimo dolet ATR.
        let far = Leg {
            from: prg,
            to: lis,
            dep_min: 360,
        };
        assert!(matches!(
            validate(&w, atr, &[far]),
            Err(ScheduleError::OutOfRange { .. })
        ));
        // Nenavazuje.
        let a = Leg {
            from: prg,
            to: vie,
            dep_min: 360,
        };
        let b = Leg {
            from: prg,
            to: vie,
            dep_min: 600,
        };
        assert_eq!(
            validate(&w, atr, &[a, b]),
            Err(ScheduleError::NotChained { leg: 1 })
        );
    }
}
