//! Datový model stavu hry. Všechny kolekce jsou uspořádané (`BTreeMap`,
//! `BTreeSet`, `Vec`), aby iterace a serializace byly deterministické.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::money::Money;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AirportId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AircraftTypeId(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AirlineId(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AircraftId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ContractId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AuctionId(pub u32);

/// Směrová linka.
pub type Route = (AirportId, AirportId);

/// Kdo záměr posílá. Hráč u záměru vždy pochází z ověřené identity spojení
/// (dosadí hostitelská smyčka), systémové záměry posílá jen hostitelská smyčka.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Actor {
    Airline(AirlineId),
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Strategy {
    /// Nízké ceny, vysoká frekvence, levný provoz.
    Levna,
    /// Vyšší ceny a služby, větší letadla.
    Premiova,
    /// Síť z domovského letiště, sloty na velkých letištích.
    Hub,
    /// Zakázky z burzy.
    Charter,
}

impl Strategy {
    pub const ALL: [Strategy; 4] = [
        Strategy::Levna,
        Strategy::Premiova,
        Strategy::Hub,
        Strategy::Charter,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Strategy::Levna => "levná",
            Strategy::Premiova => "prémiová",
            Strategy::Hub => "hub",
            Strategy::Charter => "charter",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Controller {
    Human,
    Ai(Strategy),
}

/// Důvod pauzy. Herní čas běží jen při prázdné množině důvodů.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PauseReason {
    /// Hráčská pauza (id aerolinky hráče, který ji spustil).
    Player(AirlineId),
    /// Technická pauza hostitele.
    Technical,
    /// Automatická pauza při výpadku spojení.
    Auto,
}

/// Jeden let pravidelného denního plánu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Leg {
    pub from: AirportId,
    pub to: AirportId,
    /// Minuta odletu v rámci dne (0–1439).
    pub dep_min: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ownership {
    /// Pronájem: denní nájem, vratná záloha.
    Leased { deposit: Money },
    /// Vlastnictví: účetní hodnota klesá odpisy.
    Owned { book_value: Money },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aircraft {
    pub id: AircraftId,
    pub type_id: AircraftTypeId,
    pub ownership: Ownership,
    /// Pravidelný denní plán; změna platí od následujícího dne.
    pub schedule: Vec<Leg>,
    /// Plán platný dnes (zkopírovaný při začátku dne).
    pub schedule_today: Vec<Leg>,
    /// Dny vyhrazené pro charterové zakázky.
    pub charters: BTreeMap<u64, ContractId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteStats {
    pub pax: u32,
    pub seats: u32,
    pub revenue: Money,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirlineStats {
    pub today: BTreeMap<crate::model::RouteKey, RouteStats>,
    pub yesterday: BTreeMap<crate::model::RouteKey, RouteStats>,
    pub revenue_today: Money,
    pub cost_today: Money,
    pub revenue_yesterday: Money,
    pub cost_yesterday: Money,
    pub total_pax: u64,
    pub total_seats: u64,
    pub flights: u64,
    pub cancelled_flights: u64,
    pub charters_flown: u32,
    pub contracts_won: u32,
    pub slots_won: u32,
}

/// Klíč linky ve statistikách (serializovatelný jako dvojice).
pub type RouteKey = (AirportId, AirportId);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Airline {
    pub id: AirlineId,
    pub name: String,
    pub home: AirportId,
    pub controller: Controller,
    pub cash: Money,
    pub loans: Money,
    /// Úroveň služeb 1–3 (náklad na cestujícího a přitažlivost letu).
    pub service: u8,
    pub fleet: BTreeMap<AircraftId, Aircraft>,
    /// Ceny letenek podle směrové linky; chybí-li, platí referenční cena.
    pub prices: BTreeMap<Route, Money>,
    /// Vlastněné sloty na přetížených letištích (1 slot = 1 denní odlet).
    pub slots: BTreeMap<AirportId, u32>,
    /// Pořadí aerolinky hráče (po bankrotu se zvyšuje).
    pub generation: u32,
    /// Po sobě jdoucí dny se zápornou hotovostí.
    pub negative_days: u32,
    pub stats: AirlineStats,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractStatus {
    /// Na burze; lze vzít do `offer_until` (nebo do konce otevřeného okna).
    Offered,
    /// Přidělena losem, čeká na přiřazení letadla.
    Awarded(AirlineId),
    /// Letadlo přiřazeno, let proběhne v den zakázky.
    Assigned(AirlineId, AircraftId),
    Done(AirlineId),
    /// Nikdo nevzal.
    Expired,
    /// Vítěz nepřiřadil letadlo včas, platí penále.
    Failed(AirlineId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionWindow {
    /// Krok, ve kterém se okno uzavře a proběhne los.
    pub due: u64,
    pub candidates: BTreeSet<AirlineId>,
}

/// Charterová zakázka z burzy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub id: ContractId,
    pub from: AirportId,
    pub to: AirportId,
    pub pax: u32,
    /// Herní den letu.
    pub day: u64,
    pub reward: Money,
    /// Konec nabídky (herní čas).
    pub offer_until: u64,
    pub window: Option<CollectionWindow>,
    pub status: ContractStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuctionStatus {
    Open,
    Won(AirlineId, Money),
    NoBids,
}

/// Aukce jednoho slotu na přetíženém letišti (otevřené přihazování).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Auction {
    pub id: AuctionId,
    pub airport: AirportId,
    /// Konec odpočtu v herním čase.
    pub end_due: u64,
    /// Konec ochranné lhůty po pozdním příhozu (lhůta v reálném čase převedená na kroky).
    pub protection_due: Option<u64>,
    pub high_bid: Option<(AirlineId, Money)>,
    pub status: AuctionStatus,
}

impl Auction {
    /// Aukce se uzavře, až uplyne odpočet i případná ochranná lhůta.
    pub fn effective_end(&self) -> u64 {
        self.end_due.max(self.protection_due.unwrap_or(0))
    }
}

/// Uzávěra provozu na jeden den (oznámená den předem).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Disruption {
    /// Stávka řízení letového provozu v zemi: zrušené lety z a do země.
    Strike { day: u64, country: String },
    /// Počasí: letiště zavřené v minutách dne `[from_min, to_min)`.
    Weather {
        day: u64,
        airport: AirportId,
        from_min: u32,
        to_min: u32,
    },
}

impl Disruption {
    pub fn day(&self) -> u64 {
        match self {
            Disruption::Strike { day, .. } | Disruption::Weather { day, .. } => *day,
        }
    }
}
