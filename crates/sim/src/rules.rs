//! Pravidla kampaně (nastavitelná při založení) a výchozí hodnoty z návrhu.

use serde::{Deserialize, Serialize};

use crate::money::Money;
use crate::time::Speed;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignRules {
    /// Délka kampaně v herních dnech (výchozí 1. 1. 2028 – 31. 12. 2029).
    pub duration_days: u64,
    pub starting_capital: Money,
    /// Startovní flotila: (kód typu, počet), pronajatá se zaplacenou zálohou.
    pub starting_fleet: Vec<(String, u32)>,
    pub initial_speed: Speed,
    /// Po kolika dnech v minusu nastane bankrot (návrh R11: 7).
    pub bankruptcy_days: u32,
    /// Kapitál nové aerolinky po bankrotu v promile startovního (návrh R11: 500).
    pub restart_capital_permille: i64,
    /// Sběrné okno burzy v reálných sekundách (návrh: 1 s).
    pub contract_window_secs: u64,
    /// Ochranná lhůta aukce v reálných sekundách (návrh: 3 s).
    pub auction_protection_secs: u64,
    /// Reakční doba AI v herních minutách (návrh: 50–150).
    pub ai_reaction_min: (u64, u64),
    /// AI rozhoduje nejvýš jednou za tolik herních minut (návrh: 60).
    pub ai_decision_interval: u64,
    pub contracts_per_day: u64,
    pub auction_every_days: u64,
    /// Odpočet aukce v herních minutách.
    pub auction_duration_min: u64,
    pub auction_reserve: Money,
    /// Minimální navýšení příhozu v promile.
    pub auction_increment_permille: i64,
    /// Náhodné události (kerosin, stávky, počasí).
    pub events: bool,
}

impl Default for CampaignRules {
    fn default() -> Self {
        CampaignRules {
            duration_days: 731,
            starting_capital: Money::kc(25_000_000),
            starting_fleet: vec![("AT72".to_string(), 2)],
            initial_speed: Speed::X1,
            bankruptcy_days: 7,
            restart_capital_permille: 500,
            contract_window_secs: 1,
            auction_protection_secs: 3,
            ai_reaction_min: (50, 150),
            ai_decision_interval: 60,
            contracts_per_day: 4,
            auction_every_days: 4,
            auction_duration_min: 360,
            auction_reserve: Money::kc(3_000_000),
            auction_increment_permille: 50,
            events: true,
        }
    }
}
