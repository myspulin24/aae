//! Peníze v celých haléřích (`i64`). Žádná desetinná čísla ve stavu hry,
//! aby výpočty a kontrolní hash byly přesné.

use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Money(pub i64);

impl Money {
    pub const ZERO: Money = Money(0);

    pub const fn kc(kc: i64) -> Money {
        Money(kc * 100)
    }

    pub const fn haler(h: i64) -> Money {
        Money(h)
    }

    /// Celé koruny (zaokrouhleno k nule).
    pub const fn whole_kc(self) -> i64 {
        self.0 / 100
    }

    pub const fn times(self, n: i64) -> Money {
        Money(self.0 * n)
    }

    /// Násobek v promile se zaokrouhlením k nule (např. 500 = polovina).
    pub const fn permille(self, p: i64) -> Money {
        Money(self.0 * p / 1000)
    }

    /// Převod z desetinné částky v korunách, zaokrouhleno na celé haléře.
    pub fn from_kc_f64(kc: f64) -> Money {
        Money((kc * 100.0).round() as i64)
    }

    pub fn as_kc_f64(self) -> f64 {
        self.0 as f64 / 100.0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Money) -> Money {
        Money(self.0 + rhs.0)
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Money) -> Money {
        Money(self.0 - rhs.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Money) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Money) {
        self.0 -= rhs.0;
    }
}

impl Neg for Money {
    type Output = Money;
    fn neg(self) -> Money {
        Money(-self.0)
    }
}

impl std::iter::Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        iter.fold(Money::ZERO, |a, b| a + b)
    }
}

/// „18 412 000 Kč“ – celé koruny s mezerami mezi řády.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kc = self.whole_kc();
        let digits = kc.unsigned_abs().to_string();
        let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                grouped.push(' ');
            }
            grouped.push(ch);
        }
        if kc < 0 {
            write!(f, "-{grouped} Kč")
        } else {
            write!(f, "{grouped} Kč")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_whole_crowns_with_groups() {
        assert_eq!(Money::kc(18_412_000).to_string(), "18 412 000 Kč");
        assert_eq!(Money::kc(-1_500).to_string(), "-1 500 Kč");
        assert_eq!(Money::kc(0).to_string(), "0 Kč");
        assert_eq!(Money::haler(250).to_string(), "2 Kč");
    }

    #[test]
    fn permille_halves() {
        assert_eq!(Money::kc(25_000_000).permille(500), Money::kc(12_500_000));
    }
}
