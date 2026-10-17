//! AAE – deterministické jádro simulace.
//!
//! Stav hry mění jen dvě operace: [`GameState::apply`] (záměr hráče, AI nebo
//! hostitelské smyčky) a [`GameState::step`] (posun o jednu herní minutu).
//! Simulace nečte systémové hodiny, nepoužívá vlákna ani `HashMap` a veškerou
//! náhodu bere z generátoru uloženého ve stavu, takže stejný počáteční stav
//! a stejná posloupnost záměrů s čísly kroků dají vždy stejný výsledek
//! (návrh `notes/letecka-hra-navrh.md`, Komponenty 1 a Section 2).

pub mod ai;
pub mod calendar;
pub mod data;
pub mod economy;
pub mod engine;
pub mod event;
pub mod hash;
pub mod intent;
pub mod model;
pub mod money;
pub mod queue;
pub mod rng;
pub mod rules;
pub mod schedule;
pub mod time;

pub use engine::{GameState, NewAirline, SetupError, StepError};
pub use event::Event;
pub use intent::{Intent, Reject};
pub use model::{
    Actor, AircraftId, AirlineId, AirportId, AuctionId, ContractId, Controller, Leg, PauseReason,
    Strategy,
};
pub use money::Money;
pub use rules::CampaignRules;
pub use time::Speed;
