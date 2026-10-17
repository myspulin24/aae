//! Ekonomický model: poptávka (gravitační model), přitažlivost letu,
//! referenční ceny a náklady.
//!
//! Všechny konstanty jsou první odhady k vyladění v laboratoři. Výpočty
//! běží v `f64`, do stavu se ukládají jen celá čísla (kontrolní hash).

use serde::{Deserialize, Serialize};

use crate::data::{AircraftType, World};
use crate::model::AirportId;
use crate::money::Money;

/// Měřítko obchodní poptávky (cestujících za den a směr).
const K_BUSINESS: f64 = 9.5;
/// Měřítko turistické poptávky.
const K_LEISURE: f64 = 4.2;
/// Pod touto vzdáleností vyhrává vlak a auto.
const SHORT_HAUL_FLOOR_KM: f64 = 150.0;
const SHORT_HAUL_FULL_KM: f64 = 600.0;

pub const PAX_FEE: Money = Money::kc(300);
/// Denní režie na letadlo (administrativa, prodej, pojištění).
pub const OVERHEAD_PER_AIRCRAFT_PER_DAY: Money = Money::kc(30_000);
pub const COMPENSATION_PER_PAX: Money = Money::kc(400);
/// Přirážka k poplatkům na přetížených letištích (promile).
pub const SLOT_AIRPORT_FEE_PERMILLE: i64 = 1400;
/// Roční úrok z úvěru v promile (8 %).
pub const LOAN_RATE_PERMILLE: i64 = 80;
/// Roční odpis vlastních letadel v promile (6 %).
pub const DEPRECIATION_PERMILLE: i64 = 60;
/// Záloha při pronájmu: tolik dní nájmu.
pub const LEASE_DEPOSIT_DAYS: i64 = 30;
/// Prodej vlastního letadla za 80 % účetní hodnoty.
pub const SALE_PERMILLE: i64 = 800;

/// Náklad služeb na cestujícího a přitažlivost podle úrovně služeb 1–3.
pub fn service_cost_per_pax(level: u8) -> Money {
    match level {
        1 => Money::kc(60),
        2 => Money::kc(120),
        _ => Money::kc(220),
    }
}

fn service_quality(level: u8) -> f64 {
    match level {
        1 => 0.9,
        2 => 1.0,
        _ => 1.12,
    }
}

/// Referenční cena jednosměrné letenky podle vzdálenosti.
pub fn reference_fare(distance_km: u32) -> Money {
    Money::kc(1000) + Money::haler(120 * distance_km as i64)
}

pub fn price_bounds(distance_km: u32) -> (Money, Money) {
    let r = reference_fare(distance_km);
    (r.permille(300), r.permille(3000))
}

/// Základní denní poptávka v jednom směru pro všechny dvojice letišť
/// (obchodní a turistická složka zvlášť, sezónnost se násobí denně).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseDemand {
    n: usize,
    business: Vec<u32>,
    leisure: Vec<u32>,
}

impl BaseDemand {
    pub fn compute(world: &World) -> BaseDemand {
        let n = world.airport_count();
        let mass: Vec<f64> = world
            .airports
            .iter()
            .map(|a| (a.population_k as f64 / 1000.0).powf(0.8) * (a.wealth as f64 / 100.0))
            .collect();
        let tourism: Vec<f64> = world.airports.iter().map(|a| a.tourism as f64).collect();
        let mut business = vec![0u32; n * n];
        let mut leisure = vec![0u32; n * n];
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let d = world.distance(AirportId(i as u16), AirportId(j as u16)) as f64;
                let f = distance_factor(d);
                let b = K_BUSINESS * mass[i] * mass[j] * f;
                let l = K_LEISURE * (mass[i] * tourism[j] + mass[j] * tourism[i]) * f;
                business[i * n + j] = b.round() as u32;
                leisure[i * n + j] = l.round() as u32;
            }
        }
        BaseDemand {
            n,
            business,
            leisure,
        }
    }

    pub fn business(&self, from: AirportId, to: AirportId) -> u32 {
        self.business[from.0 as usize * self.n + to.0 as usize]
    }

    pub fn leisure(&self, from: AirportId, to: AirportId) -> u32 {
        self.leisure[from.0 as usize * self.n + to.0 as usize]
    }

    /// Denní poptávka v daném měsíci bez vlivu cen.
    pub fn seasonal(&self, from: AirportId, to: AirportId, month: u32) -> u32 {
        let b = self.business(from, to) as f64 * business_season(month);
        let l = self.leisure(from, to) as f64 * leisure_season(month);
        (b + l).round() as u32
    }
}

fn distance_factor(d: f64) -> f64 {
    let ramp =
        ((d - SHORT_HAUL_FLOOR_KM) / (SHORT_HAUL_FULL_KM - SHORT_HAUL_FLOOR_KM)).clamp(0.05, 1.0);
    ramp * (800.0 / d.max(800.0)).powf(0.6)
}

fn leisure_season(month: u32) -> f64 {
    const F: [f64; 12] = [0.6, 0.6, 0.75, 0.9, 1.1, 1.4, 1.7, 1.7, 1.3, 0.9, 0.65, 0.8];
    F[(month.clamp(1, 12) - 1) as usize]
}

fn business_season(month: u32) -> f64 {
    match month {
        8 | 12 => 0.85,
        _ => 1.0,
    }
}

/// Vliv průměrné ceny na lince na celkovou poptávku dne.
pub fn price_factor(reference: Money, average_price: Money) -> f64 {
    if average_price.0 <= 0 {
        return 1.8;
    }
    (reference.0 as f64 / average_price.0 as f64)
        .powf(1.2)
        .clamp(0.3, 1.8)
}

/// Přitažlivost letu v tisícinách (cena, služby, denní doba).
pub fn attractiveness(reference: Money, price: Money, service: u8, dep_min: u32) -> u32 {
    let price_term = (reference.0 as f64 / price.0.max(1) as f64).powi(2);
    let time_term = match dep_min / 60 {
        6..=21 => 1.0,
        5 | 22 => 0.75,
        _ => 0.4,
    };
    let a = price_term * service_quality(service) * time_term * 1000.0;
    (a.round() as u32).clamp(1, 100_000)
}

/// Podíl letu na denní poptávce podle R8:
/// `poptávka × přitažlivost / součet přitažlivostí všech letů dne`, omezený sedačkami.
pub fn passengers(
    daily_demand: u32,
    attractiveness: u32,
    total_attractiveness: u64,
    seats: u32,
) -> u32 {
    if total_attractiveness == 0 {
        return 0;
    }
    let share = daily_demand as u64 * attractiveness as u64 / total_attractiveness;
    (share as u32).min(seats)
}

/// Náklady jednoho letu (palivo, poplatky, posádka, služby).
pub fn flight_cost(
    t: &AircraftType,
    distance_km: u32,
    block_min: u32,
    fuel_haler_per_l: i64,
    pax: u32,
    service: u8,
    slot_airport_involved: bool,
) -> Money {
    let litres = distance_km as i64 * t.fuel_dl_per_km as i64 / 10;
    let fuel = Money::haler(litres * fuel_haler_per_l);
    let mut fees = t.movement_fee + PAX_FEE.times(pax as i64);
    if slot_airport_involved {
        fees = fees.permille(SLOT_AIRPORT_FEE_PERMILLE);
    }
    let crew = Money::haler(t.crew_per_hour.0 * block_min as i64 / 60);
    let service = service_cost_per_pax(service).times(pax as i64);
    fuel + fees + crew + service
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demand_is_symmetric_and_plausible() {
        let w = World::europe_v1().unwrap();
        let d = BaseDemand::compute(&w);
        let prg = w.airport_by_code("PRG").unwrap();
        let lhr = w.airport_by_code("LHR").unwrap();
        let brq = w.airport_by_code("BRQ").unwrap();
        assert_eq!(d.seasonal(prg, lhr, 3), d.seasonal(lhr, prg, 3));
        let big = d.seasonal(prg, lhr, 3);
        assert!((150..800).contains(&big), "PRG–LHR {big}/den");
        // Praha–Brno je po zemi: poptávka po letu minimální.
        assert!(d.seasonal(prg, brq, 3) < big / 20);
    }

    #[test]
    fn leisure_peaks_in_summer() {
        let w = World::europe_v1().unwrap();
        let d = BaseDemand::compute(&w);
        let prg = w.airport_by_code("PRG").unwrap();
        let pmi = w.airport_by_code("PMI").unwrap();
        // Přesný poměr ladí laboratoř; tady jen, že sezónnost působí.
        assert!(d.seasonal(prg, pmi, 7) * 2 > d.seasonal(prg, pmi, 1) * 3);
    }

    #[test]
    fn cheaper_flight_is_more_attractive() {
        let r = reference_fare(1000);
        assert!(attractiveness(r, r.permille(800), 2, 600) > attractiveness(r, r, 2, 600));
        assert!(attractiveness(r, r, 2, 600) > attractiveness(r, r, 2, 120));
    }

    #[test]
    fn share_is_capped_by_seats() {
        assert_eq!(passengers(300, 1000, 2000, 70), 70);
        assert_eq!(passengers(100, 1000, 2000, 189), 50);
        assert_eq!(passengers(100, 1000, 0, 70), 0);
    }
}
