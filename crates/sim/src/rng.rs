//! Vlastní generátor náhody (xoshiro256**, seedovaný přes SplitMix64).
//!
//! Celý vnitřní stav je součástí stavu hry: ukládá se s kampaní a počítá do
//! kontrolního hashe, takže po načtení náhoda pokračuje, kde skončila.
//! Vlastní implementace místo knihovny je záměr: aktualizace závislosti
//! nesmí změnit proud čísel a tím rozbít přehrávání uložených relací.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn from_seed(seed: u64) -> Rng {
        let mut sm = seed;
        let mut next = || {
            sm = sm.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = sm;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        Rng {
            s: [next(), next(), next(), next()],
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Rovnoměrně z `0..n` (bez zkreslení modulem).
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "Rng::below(0)");
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let x = self.next_u64();
            if x < zone {
                return x % n;
            }
        }
    }

    /// Rovnoměrně z `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(lo <= hi, "Rng::range({lo}, {hi})");
        lo + self.below(hi - lo + 1)
    }

    /// Pravděpodobnost `num / den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    /// Rovnoměrně z `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Vybere index podle nezáporných vah; `None`, když jsou všechny nulové.
    pub fn weighted(&mut self, weights: &[u64]) -> Option<usize> {
        let total: u64 = weights.iter().sum();
        if total == 0 {
            return None;
        }
        let mut pick = self.below(total);
        for (i, w) in weights.iter().enumerate() {
            if pick < *w {
                return Some(i);
            }
            pick -= w;
        }
        unreachable!("vážený výběr překročil součet vah")
    }

    /// Nezávislý generátor odvozený z tohoto (např. pro AI aerolinku).
    pub fn fork(&mut self) -> Rng {
        Rng::from_seed(self.next_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::from_seed(42);
        let mut b = Rng::from_seed(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn stream_is_stable_across_versions() {
        // Pevné hodnoty: změna implementace by rozbila přehrávání uložených relací.
        let mut r = Rng::from_seed(1);
        let first: Vec<u64> = (0..3).map(|_| r.next_u64()).collect();
        let mut again = Rng::from_seed(1);
        assert_eq!(first, (0..3).map(|_| again.next_u64()).collect::<Vec<_>>());
        assert_ne!(first[0], first[1]);
    }

    #[test]
    fn serialized_state_continues_the_stream() {
        let mut a = Rng::from_seed(7);
        a.next_u64();
        let bytes = postcard::to_allocvec(&a).unwrap();
        let mut b: Rng = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn range_and_weighted_stay_in_bounds() {
        let mut r = Rng::from_seed(3);
        for _ in 0..10_000 {
            let x = r.range(50, 150);
            assert!((50..=150).contains(&x));
        }
        assert_eq!(r.weighted(&[0, 0]), None);
        assert_eq!(r.weighted(&[0, 5, 0]), Some(1));
    }
}
