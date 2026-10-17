//! Kontrolní hash stavu z kanonické serializace (postcard nad uspořádanými
//! kolekcemi), FNV-1a 64 bit. Slouží k ověření determinismu a shody stavu
//! po uložení a načtení, ne k zabezpečení.

use serde::Serialize;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn canonical_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).expect("serializace stavu do paměti nemůže selhat")
}

pub fn state_hash<T: Serialize>(value: &T) -> u64 {
    fnv1a64(&canonical_bytes(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
