//! Spojení mezi hostitelem a klientem jako dvojice kanálů s celými zprávami.
//! Dvě implementace: síť v paměti ([`mem`], testy se zpožděním a výpadkem)
//! a iroh ([`iroh_net`], skutečná hra). Rozhodnutí R14.

pub mod iroh_net;
pub mod mem;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::PeerKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathKind {
    /// Hostitelův vlastní klient ve stejném procesu.
    Local,
    Direct,
    Relay,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathInfo {
    pub kind: PathKind,
    pub rtt_ms: u32,
}

impl PathInfo {
    pub const UNKNOWN: PathInfo = PathInfo {
        kind: PathKind::Unknown,
        rtt_ms: 0,
    };
    pub const LOCAL: PathInfo = PathInfo {
        kind: PathKind::Local,
        rtt_ms: 0,
    };
}

/// Jedno spojení. Zavření: zahodit `tx`; `rx` vrátí `None`, když druhá strana
/// spojení ukončila nebo vypadla síť.
pub struct Connection {
    pub peer: PeerKey,
    pub local: bool,
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
    pub rx: mpsc::UnboundedReceiver<Vec<u8>>,
    pub path: watch::Receiver<PathInfo>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("peer", &self.peer)
            .field("local", &self.local)
            .finish()
    }
}
