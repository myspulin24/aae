//! Statická data světa: letiště, typy letadel, vzdálenosti.
//! Data jsou zabudovaná v binárce (`data/*.csv`) a svět se ukládá do stavu
//! kampaně, takže uložená hra nezávisí na pozdější změně dat.

use serde::{Deserialize, Serialize};

use crate::model::{AircraftTypeId, AirportId};
use crate::money::Money;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Airport {
    pub code: String,
    pub country: String,
    /// Zeměpisná šířka a délka v desetitisícinách stupně.
    pub lat_e4: i32,
    pub lon_e4: i32,
    pub city: String,
    /// Spádová populace v tisících (přibližná herní hodnota).
    pub population_k: u32,
    /// HDP na obyvatele v PPP, EU = 100 (přibližná herní hodnota).
    pub wealth: u32,
    /// Turistická přitažlivost 0–3.
    pub tourism: u32,
    /// Přetížené letiště: odlety vyžadují sloty z aukcí.
    pub slot_constrained: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AircraftType {
    pub code: String,
    pub name: String,
    pub seats: u32,
    pub range_km: u32,
    pub speed_kmh: u32,
    pub fuel_dl_per_km: u32,
    pub turnaround_min: u32,
    pub lease_per_day: Money,
    pub price: Money,
    pub crew_per_hour: Money,
    pub movement_fee: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    pub airports: Vec<Airport>,
    pub aircraft_types: Vec<AircraftType>,
    /// Vzdálenosti po ortodromě v km, matice `n × n`.
    distance_km: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataError(pub String);

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "chyba herních dat: {}", self.0)
    }
}

impl std::error::Error for DataError {}

const AIRPORTS_CSV: &str = include_str!("../data/airports.csv");
const AIRCRAFT_CSV: &str = include_str!("../data/aircraft.csv");

impl World {
    /// Mapa Evropa v1 ze zabudovaných dat.
    pub fn europe_v1() -> Result<World, DataError> {
        World::from_csv(AIRPORTS_CSV, AIRCRAFT_CSV)
    }

    pub fn from_csv(airports_csv: &str, aircraft_csv: &str) -> Result<World, DataError> {
        let airports = parse_rows(airports_csv, 9)?
            .into_iter()
            .map(|f| {
                Ok(Airport {
                    code: f[0].to_string(),
                    country: f[1].to_string(),
                    lat_e4: parse_coord(f[2])?,
                    lon_e4: parse_coord(f[3])?,
                    city: f[4].to_string(),
                    population_k: parse_num(f[5])?,
                    wealth: parse_num(f[6])?,
                    tourism: parse_num(f[7])?,
                    slot_constrained: parse_num::<u32>(f[8])? == 1,
                })
            })
            .collect::<Result<Vec<_>, DataError>>()?;
        let aircraft_types = parse_rows(aircraft_csv, 11)?
            .into_iter()
            .map(|f| {
                Ok(AircraftType {
                    code: f[0].to_string(),
                    name: f[1].to_string(),
                    seats: parse_num(f[2])?,
                    range_km: parse_num(f[3])?,
                    speed_kmh: parse_num(f[4])?,
                    fuel_dl_per_km: parse_num(f[5])?,
                    turnaround_min: parse_num(f[6])?,
                    lease_per_day: Money::kc(parse_num(f[7])?),
                    price: Money::kc(parse_num::<i64>(f[8])? * 1_000_000),
                    crew_per_hour: Money::kc(parse_num(f[9])?),
                    movement_fee: Money::kc(parse_num(f[10])?),
                })
            })
            .collect::<Result<Vec<_>, DataError>>()?;
        if airports.is_empty() || aircraft_types.is_empty() {
            return Err(DataError("chybí letiště nebo typy letadel".into()));
        }
        if airports.len() > u16::MAX as usize || aircraft_types.len() > u8::MAX as usize {
            return Err(DataError("příliš mnoho letišť nebo typů letadel".into()));
        }
        let mut codes: Vec<&str> = airports.iter().map(|a| a.code.as_str()).collect();
        codes.sort_unstable();
        if codes.windows(2).any(|w| w[0] == w[1]) {
            return Err(DataError("duplicitní kód letiště".into()));
        }

        let n = airports.len();
        let mut distance_km = vec![0u32; n * n];
        for i in 0..n {
            for j in (i + 1)..n {
                let d = great_circle_km(&airports[i], &airports[j]);
                distance_km[i * n + j] = d;
                distance_km[j * n + i] = d;
            }
        }
        Ok(World {
            airports,
            aircraft_types,
            distance_km,
        })
    }

    pub fn airport_count(&self) -> usize {
        self.airports.len()
    }

    pub fn airport(&self, id: AirportId) -> &Airport {
        &self.airports[id.0 as usize]
    }

    pub fn airport_ids(&self) -> impl Iterator<Item = AirportId> + '_ {
        (0..self.airports.len()).map(|i| AirportId(i as u16))
    }

    pub fn airport_by_code(&self, code: &str) -> Option<AirportId> {
        self.airports
            .iter()
            .position(|a| a.code == code)
            .map(|i| AirportId(i as u16))
    }

    pub fn is_valid_airport(&self, id: AirportId) -> bool {
        (id.0 as usize) < self.airports.len()
    }

    pub fn distance(&self, a: AirportId, b: AirportId) -> u32 {
        self.distance_km[a.0 as usize * self.airports.len() + b.0 as usize]
    }

    pub fn aircraft_type(&self, id: AircraftTypeId) -> &AircraftType {
        &self.aircraft_types[id.0 as usize]
    }

    pub fn aircraft_type_by_code(&self, code: &str) -> Option<AircraftTypeId> {
        self.aircraft_types
            .iter()
            .position(|t| t.code == code)
            .map(|i| AircraftTypeId(i as u8))
    }

    pub fn is_valid_aircraft_type(&self, id: AircraftTypeId) -> bool {
        (id.0 as usize) < self.aircraft_types.len()
    }
}

fn parse_rows(csv: &str, columns: usize) -> Result<Vec<Vec<&str>>, DataError> {
    let mut rows = Vec::new();
    let mut header_seen = false;
    for (n, line) in csv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header_seen {
            header_seen = true;
            continue;
        }
        let fields: Vec<&str> = line.split(';').map(str::trim).collect();
        if fields.len() != columns {
            return Err(DataError(format!(
                "řádek {}: čekám {columns} sloupců, je jich {}",
                n + 1,
                fields.len()
            )));
        }
        rows.push(fields);
    }
    Ok(rows)
}

fn parse_num<T: std::str::FromStr>(s: &str) -> Result<T, DataError> {
    s.parse()
        .map_err(|_| DataError(format!("„{s}“ není číslo")))
}

/// „50.1009“ → 501009 (desetitisíciny stupně), bez desetinných čísel ve stavu.
fn parse_coord(s: &str) -> Result<i32, DataError> {
    let value: f64 = parse_num(s)?;
    if !(-180.0..=180.0).contains(&value) {
        return Err(DataError(format!("souřadnice „{s}“ mimo rozsah")));
    }
    Ok((value * 10_000.0).round() as i32)
}

fn great_circle_km(a: &Airport, b: &Airport) -> u32 {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    let to_rad = |e4: i32| (e4 as f64 / 10_000.0).to_radians();
    let (lat1, lon1, lat2, lon2) = (
        to_rad(a.lat_e4),
        to_rad(a.lon_e4),
        to_rad(b.lat_e4),
        to_rad(b.lon_e4),
    );
    let h = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    (2.0 * EARTH_RADIUS_KM * h.sqrt().asin()).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn europe_v1_loads() {
        let w = World::europe_v1().unwrap();
        assert_eq!(w.airport_count(), 75);
        assert_eq!(w.aircraft_types.len(), 7);
        let prg = w.airport_by_code("PRG").unwrap();
        assert_eq!(w.airport(prg).city, "Praha");
        assert!(w.airports.iter().filter(|a| a.slot_constrained).count() == 4);
    }

    #[test]
    fn distances_are_plausible_and_symmetric() {
        let w = World::europe_v1().unwrap();
        let prg = w.airport_by_code("PRG").unwrap();
        let lhr = w.airport_by_code("LHR").unwrap();
        let d = w.distance(prg, lhr);
        // Praha–Heathrow po ortodromě zhruba 1 040 km.
        assert!((1000..1080).contains(&d), "PRG–LHR {d} km");
        assert_eq!(d, w.distance(lhr, prg));
        assert_eq!(w.distance(prg, prg), 0);
    }

    #[test]
    fn rejects_bad_rows() {
        let bad = "code;country;lat;lon;city;population_k;wealth;tourism;slots\nPRG;CZ;x;14;Praha;1;1;1;0\n";
        assert!(World::from_csv(bad, AIRCRAFT_CSV).is_err());
    }
}
