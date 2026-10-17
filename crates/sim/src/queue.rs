//! Fronta naplánovaných událostí řazená podle (krok, pořadí vložení).
//! Pořadí vložení dělá řazení událostí ve stejném kroku pevným a přežije
//! serializaci (návrh: Section 2 nález 1).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{AircraftId, AirlineId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scheduled {
    /// Odlet letu `leg` z dnešního plánu letadla.
    Departure {
        airline: AirlineId,
        aircraft: AircraftId,
        generation: u32,
        leg: u16,
    },
    /// Nová zakázka na burze.
    ContractSpawn,
    /// Nová aukce slotu.
    AuctionSpawn,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventQueue {
    items: BTreeMap<(u64, u64), Scheduled>,
    next_seq: u64,
}

impl EventQueue {
    pub fn push(&mut self, due: u64, item: Scheduled) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.items.insert((due, seq), item);
    }

    /// Vyjme nejbližší událost splatnou nejpozději v kroku `now`.
    pub fn pop_due(&mut self, now: u64) -> Option<(u64, Scheduled)> {
        let (&(due, _), _) = self.items.first_key_value()?;
        if due > now {
            return None;
        }
        self.items.pop_first().map(|((due, _), item)| (due, item))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_step_keeps_insertion_order() {
        let mut q = EventQueue::default();
        q.push(10, Scheduled::AuctionSpawn);
        q.push(5, Scheduled::ContractSpawn);
        q.push(10, Scheduled::ContractSpawn);
        assert_eq!(q.pop_due(4), None);
        assert_eq!(q.pop_due(10), Some((5, Scheduled::ContractSpawn)));
        assert_eq!(q.pop_due(10), Some((10, Scheduled::AuctionSpawn)));
        assert_eq!(q.pop_due(10), Some((10, Scheduled::ContractSpawn)));
        assert!(q.is_empty());
    }

    #[test]
    fn order_survives_serialization() {
        let mut q = EventQueue::default();
        q.push(7, Scheduled::ContractSpawn);
        q.push(7, Scheduled::AuctionSpawn);
        let mut back: EventQueue =
            postcard::from_bytes(&postcard::to_allocvec(&q).unwrap()).unwrap();
        assert_eq!(back.pop_due(7), Some((7, Scheduled::ContractSpawn)));
        assert_eq!(back.pop_due(7), Some((7, Scheduled::AuctionSpawn)));
    }
}
