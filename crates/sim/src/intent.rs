//! Záměry (jediný způsob, jak hráč, AI nebo hostitelská smyčka mění stav)
//! a důvody odmítnutí.

use serde::{Deserialize, Serialize};

use crate::model::{
    AircraftId, AircraftTypeId, AirportId, AuctionId, ContractId, Leg, PauseReason,
};
use crate::money::Money;
use crate::schedule::ScheduleError;
use crate::time::Speed;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    // --- herní čas ---
    /// Přidá důvod pauzy. Hráč smí jen `Player(své id)`, ostatní jen systém.
    Pause(PauseReason),
    /// Odebere jen tento důvod pauzy.
    Resume(PauseReason),
    /// Jen systém (po hlasování nebo okamžitém zpomalení na 1×).
    SetSpeed(Speed),

    // --- plánování (povolené i během pauzy) ---
    /// Nový denní plán letadla, platí od následujícího dne.
    SetSchedule {
        aircraft: AircraftId,
        legs: Vec<Leg>,
    },
    SetPrice {
        from: AirportId,
        to: AirportId,
        price: Money,
    },
    SetService {
        level: u8,
    },
    LeaseAircraft {
        type_id: AircraftTypeId,
    },
    BuyAircraft {
        type_id: AircraftTypeId,
    },
    /// Vrátí pronajaté letadlo (záloha zpět).
    ReturnAircraft {
        aircraft: AircraftId,
    },
    /// Prodá vlastní letadlo (80 % účetní hodnoty).
    SellAircraft {
        aircraft: AircraftId,
    },
    TakeLoan {
        amount: Money,
    },
    RepayLoan {
        amount: Money,
    },
    AssignCharter {
        contract: ContractId,
        aircraft: AircraftId,
    },

    // --- trh (během pauzy zavřený) ---
    TakeContract {
        contract: ContractId,
    },
    Bid {
        auction: AuctionId,
        amount: Money,
    },
}

impl Intent {
    /// Záměry burzy a aukcí; během pauzy se odmítají.
    pub fn is_market(&self) -> bool {
        matches!(self, Intent::TakeContract { .. } | Intent::Bid { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reject {
    /// Aktér na tento záměr nemá právo (cizí aerolinka, systémový záměr od hráče).
    NotPermitted,
    /// Burza a aukce jsou během pauzy zavřené.
    MarketClosedDuringPause,
    CampaignFinished,
    UnknownAirline,
    UnknownAircraft,
    UnknownAircraftType,
    UnknownAirport,
    UnknownContract,
    UnknownAuction,
    InvalidSchedule(ScheduleError),
    InsufficientFunds {
        needed: Money,
    },
    PriceOutOfBounds {
        min: Money,
        max: Money,
    },
    InvalidServiceLevel,
    LoanLimit {
        max: Money,
    },
    InvalidAmount,
    AlreadyPaused,
    NotPaused,
    SameSpeed,
    ContractNotOffered,
    AlreadyCandidate,
    ContractNotYours,
    AircraftTooSmall,
    OutOfRange,
    AircraftBusyThatDay,
    ContractDayPassed,
    AircraftHasCharter,
    AuctionClosed,
    BidTooLow {
        min: Money,
    },
    AlreadyHighestBidder,
}
