//! Události, které simulace vrací ze `step` a `apply`: zprávy pro hráče,
//! podklady pro statistiky a laboratoř.

use serde::{Deserialize, Serialize};

use crate::model::{AircraftId, AirlineId, AirportId, AuctionId, ContractId, PauseReason};
use crate::money::Money;
use crate::time::Speed;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    DayStarted {
        day: u64,
    },
    CampaignEnded {
        day: u64,
    },

    Paused(PauseReason),
    Resumed(PauseReason),
    SpeedChanged(Speed),

    FlightDeparted {
        airline: AirlineId,
        aircraft: AircraftId,
        from: AirportId,
        to: AirportId,
        pax: u32,
        seats: u32,
        revenue: Money,
        cost: Money,
    },
    FlightCancelled {
        airline: AirlineId,
        aircraft: AircraftId,
        from: AirportId,
        to: AirportId,
        compensation: Money,
    },
    CharterFlown {
        airline: AirlineId,
        contract: ContractId,
        reward: Money,
        cost: Money,
    },

    ContractOffered {
        contract: ContractId,
    },
    ContractWindowOpened {
        contract: ContractId,
        by: AirlineId,
        due: u64,
    },
    ContractCandidate {
        contract: ContractId,
        by: AirlineId,
    },
    ContractAwarded {
        contract: ContractId,
        winner: AirlineId,
        candidates: Vec<AirlineId>,
    },
    ContractExpired {
        contract: ContractId,
    },
    ContractFailed {
        contract: ContractId,
        airline: AirlineId,
        penalty: Money,
    },
    CharterAssigned {
        contract: ContractId,
        aircraft: AircraftId,
    },

    AuctionOpened {
        auction: AuctionId,
        airport: AirportId,
        end_due: u64,
    },
    BidPlaced {
        auction: AuctionId,
        by: AirlineId,
        amount: Money,
        effective_end: u64,
    },
    AuctionWon {
        auction: AuctionId,
        airport: AirportId,
        winner: AirlineId,
        price: Money,
    },
    AuctionNoBids {
        auction: AuctionId,
    },

    FuelPriceChanged {
        haler_per_l: i64,
    },
    DisruptionAnnounced {
        day: u64,
        description: String,
    },

    AircraftAdded {
        airline: AirlineId,
        aircraft: AircraftId,
    },
    AircraftRemoved {
        airline: AirlineId,
        aircraft: AircraftId,
    },
    ScheduleSet {
        airline: AirlineId,
        aircraft: AircraftId,
    },
    PriceSet {
        airline: AirlineId,
        from: AirportId,
        to: AirportId,
        price: Money,
    },
    LoanChanged {
        airline: AirlineId,
        loans: Money,
    },

    BankruptcyWarning {
        airline: AirlineId,
        negative_days: u32,
    },
    Bankruptcy {
        airline: AirlineId,
        new_capital: Money,
    },
}
