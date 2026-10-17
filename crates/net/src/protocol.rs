//! Zprávy mezi klientem a hostitelem (postcard). Rámce dělí transport.

use aae_sim::model::AirlineId;
use aae_sim::{AirportId, CampaignRules, ContractId, Event, Intent, Reject, Speed, Strategy};
use serde::{Deserialize, Serialize};

use crate::PeerKey;
use crate::transport::PathInfo;
use crate::view::{Delta, View};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// První zpráva po připojení.
    Hello {
        app_version: String,
        protocol: u16,
        name: String,
        world_hash: u64,
    },
    Heartbeat,
    Lobby(LobbyRequest),
    /// Herní záměr; hostitel ho aplikuje na hranici kroku a odpoví Ack/Reject.
    Intent {
        id: u64,
        intent: Intent,
    },
    /// Návrh rychlosti. Zpomalení na 1× platí hned, zrychlení po hlasování všech.
    ProposeSpeed(Speed),
    Vote {
        yes: bool,
    },
    /// Co si hráč právě prohlíží (zakázka na burze).
    Presence {
        contract: Option<ContractId>,
    },
    Leave,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LobbyRequest {
    SetProfile { name: String, home: AirportId },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HostMsg {
    Welcome {
        host_version: String,
    },
    Refused(RefuseReason),
    /// Neznámý klíč během rozehrané kampaně: čeká na rozhodnutí hostitele.
    Waiting,
    Lobby(LobbyView),
    Snapshot(Box<View>),
    Delta(Box<Delta>),
    Events(Vec<Event>),
    Ack {
        id: u64,
    },
    Reject {
        id: u64,
        reason: HostReject,
    },
    Heartbeat,
    Ended(EndReason),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefuseReason {
    /// Různé verze aplikace; aktualizovat má ten se starší verzí.
    VersionMismatch {
        host: String,
        client: String,
    },
    WorldMismatch,
    ProtocolMismatch,
    Refused,
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostReject {
    Sim(Reject),
    /// Vyčerpané 3 hráčské pauzy za relaci.
    PauseLimit,
    NotInGame,
    VoteRunning,
    NoVote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndReason {
    HostQuit,
    Refused,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LobbyView {
    pub rules: CampaignRules,
    pub ai: Vec<Strategy>,
    pub players: Vec<LobbyPlayer>,
    /// Kampaň načtená ze souboru (hráči se párují podle klíčů).
    pub loaded: bool,
    pub host_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LobbyPlayer {
    pub key: PeerKey,
    pub name: String,
    pub home: AirportId,
    pub host: bool,
    pub connected: bool,
    pub path: PathInfo,
    pub version: String,
    /// U načtené kampaně: aerolinka hráče.
    pub airline: Option<AirlineId>,
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).expect("serializace zprávy do paměti nemůže selhat")
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> anyhow::Result<T> {
    Ok(postcard::from_bytes(bytes)?)
}

/// Porovná verze „0.1.10“ a „0.1.9“ číselně po částech.
pub fn version_is_newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parse(a) > parse(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let msg = ClientMsg::Hello {
            app_version: "0.1.0".into(),
            protocol: PROTOCOL_VERSION,
            name: "Petr".into(),
            world_hash: 7,
        };
        let back: ClientMsg = decode(&encode(&msg)).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn version_ordering() {
        assert!(version_is_newer("0.1.10", "0.1.9"));
        assert!(!version_is_newer("0.1.0", "0.1.0"));
        assert!(version_is_newer("1.0.0", "0.9.9"));
    }
}
