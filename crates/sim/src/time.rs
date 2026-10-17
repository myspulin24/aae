//! Herní čas: 1 krok simulace = 1 herní minuta.
//!
//! Lhůty definované v reálném čase (sběrné okno burzy, ochrana aukce) se při
//! vzniku převedou na kroky podle aktuální rychlosti a při změně rychlosti
//! se jejich zbytek přepočítá, aby zůstal zachovaný reálný čas
//! (návrh: „Časovače definované v reálném čase“).

use serde::{Deserialize, Serialize};

pub const MINUTES_PER_DAY: u64 = 1440;

/// Rychlost hry. Při 1× trvá herní den 90 s, tedy 16 kroků za sekundu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Speed {
    X1,
    X2,
    X4,
}

impl Speed {
    pub const fn steps_per_second(self) -> u64 {
        match self {
            Speed::X1 => 16,
            Speed::X2 => 32,
            Speed::X4 => 64,
        }
    }

    pub const fn multiplier(self) -> u64 {
        match self {
            Speed::X1 => 1,
            Speed::X2 => 2,
            Speed::X4 => 4,
        }
    }
}

/// Lhůta `secs` reálných sekund vyjádřená v krocích při dané rychlosti.
pub const fn real_secs_to_steps(secs: u64, speed: Speed) -> u64 {
    secs * speed.steps_per_second()
}

/// Zbývající kroky lhůty po změně rychlosti:
/// `ceil(zbývá_dosud × nová_rychlost / stará_rychlost)`.
pub const fn rescale_remaining(remaining: u64, old: Speed, new: Speed) -> u64 {
    (remaining * new.steps_per_second()).div_ceil(old.steps_per_second())
}

pub const fn day_of(minute: u64) -> u64 {
    minute / MINUTES_PER_DAY
}

pub const fn minute_of_day(minute: u64) -> u64 {
    minute % MINUTES_PER_DAY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_in_steps() {
        assert_eq!(real_secs_to_steps(1, Speed::X1), 16);
        assert_eq!(real_secs_to_steps(1, Speed::X2), 32);
        assert_eq!(real_secs_to_steps(3, Speed::X4), 192);
    }

    #[test]
    fn rescale_keeps_real_time() {
        // 48 kroků při 1× = 3 s; po přepnutí na 4× musí zbývat 192 kroků = stále 3 s.
        assert_eq!(rescale_remaining(48, Speed::X1, Speed::X4), 192);
        assert_eq!(rescale_remaining(192, Speed::X4, Speed::X1), 48);
        // Zaokrouhlení nahoru: 5 kroků při 4× → 1,25 kroku při 1× → 2.
        assert_eq!(rescale_remaining(5, Speed::X4, Speed::X1), 2);
    }
}
