//! AAE – síťová vrstva.
//!
//! Hostitel počítá svět (`aae-sim`), klienti posílají jen záměry. Hostitelská
//! smyčka ([`host`]) i klient ([`client`]) pracují nad abstraktním spojením
//! ([`transport::Connection`]), takže testy pravidel běží nad sítí v paměti
//! s nastavitelným zpožděním a výpadkem a skutečná hra nad iroh
//! (návrh: R2, R3, R4, R5, R14).

pub mod client;
pub mod host;
pub mod protocol;
pub mod transport;
pub mod view;

use serde::{Deserialize, Serialize};

/// Identita hráče = veřejný klíč iroh (32 bajtů).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PeerKey(pub [u8; 32]);

impl PeerKey {
    /// Krátký zápis pro zobrazení (prvních 5 bajtů hexadecimálně).
    pub fn short(&self) -> String {
        self.0[..5].iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Debug for PeerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PeerKey({})", self.short())
    }
}

/// Hash zabudovaných herních dat; klient i hostitel musí mít stejná data.
pub fn world_hash() -> u64 {
    let world = aae_sim::data::World::europe_v1().expect("zabudovaná herní data");
    aae_sim::hash::state_hash(&world)
}
