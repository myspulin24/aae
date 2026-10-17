//! Uložené kampaně (R10). Každá kampaň má složku v `Dokumenty\AAE\Kampaně`:
//!
//! - `kampan.aae` – poslední stav (atomický zápis: dočasný soubor + přejmenování),
//! - `zalohy\auto-*.aae` – 10 posledních automatických uložení (každý herní den),
//! - `zalohy\relace-*.aae` – stav na začátku 5 posledních relací,
//! - `logy\relace-*.jsonl` – log záměrů 5 posledních relací (R9).
//!
//! Ukládá jen hostitel; hosté mají jen svůj filtrovaný pohled.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aae_net::host::Campaign;
use anyhow::{Context, Result, bail};

use crate::dto::CampaignInfoDto;

const MAGIC: &[u8; 4] = b"AAEK";
const FILE_VERSION: u32 = 1;
pub const MAIN_FILE: &str = "kampan.aae";
const BACKUPS: &str = "zalohy";
const LOGS: &str = "logy";
const KEEP_AUTO: usize = 10;
const KEEP_SESSIONS: usize = 5;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f =
            File::create(&tmp).with_context(|| format!("nelze zapsat soubor {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("nelze uložit {}", path.display()))?;
    Ok(())
}

pub fn encode(c: &Campaign) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 * 1024);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FILE_VERSION.to_le_bytes());
    out.extend_from_slice(&aae_sim::engine::FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&postcard::to_allocvec(c).expect("serializace do paměti nemůže selhat"));
    out
}

pub fn decode(bytes: &[u8]) -> Result<Campaign> {
    if bytes.len() < 12 || &bytes[0..4] != MAGIC {
        bail!("Soubor není uložená kampaň AAE.");
    }
    let file_version = u32::from_le_bytes(bytes[4..8].try_into().expect("4 bajty"));
    let format = u32::from_le_bytes(bytes[8..12].try_into().expect("4 bajty"));
    if file_version != FILE_VERSION || format != aae_sim::engine::FORMAT_VERSION {
        bail!(
            "Kampaň je uložená jinou verzí hry (formát {format}, tato verze čte formát {}). Aktualizujte AAE.",
            aae_sim::engine::FORMAT_VERSION
        );
    }
    postcard::from_bytes(&bytes[12..]).context("Uložená kampaň je poškozená.")
}

pub fn load(path: &Path) -> Result<Campaign> {
    let bytes = fs::read(path).with_context(|| format!("nelze přečíst {}", path.display()))?;
    decode(&bytes)
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Výchozí název nové kampaně podle dnešního data („Kampaň 17. 10. 2026“).
pub fn default_campaign_name() -> String {
    // Občanské datum z počtu dní od 1. 1. 1970 (algoritmus H. Hinnanta, UTC).
    let z = (unix_secs() / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("Kampaň {day}. {month}. {year}")
}

/// Bezpečný název složky z názvu kampaně.
pub fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    let short: String = trimmed.chars().take(60).collect();
    if short.is_empty() {
        "Kampaň".into()
    } else {
        short
    }
}

/// Nová složka kampaně; když název existuje, přidá „(2)“, „(3)“…
pub fn new_campaign_dir(root: &Path, name: &str) -> Result<PathBuf> {
    let base = folder_name(name);
    let mut dir = root.join(&base);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{base} ({n})"));
        n += 1;
    }
    fs::create_dir_all(&dir).with_context(|| format!("nelze založit {}", dir.display()))?;
    Ok(dir)
}

/// Ukládání jedné relace hostitele.
#[derive(Clone, Debug)]
pub struct Saver {
    pub dir: PathBuf,
    session: u64,
}

impl Saver {
    pub fn new(dir: PathBuf) -> Saver {
        Saver {
            dir,
            session: unix_secs(),
        }
    }

    /// Hlavní soubor + log relace.
    pub fn save(&self, mut c: Campaign) -> Result<()> {
        let log = std::mem::take(&mut c.log);
        fs::create_dir_all(&self.dir)?;
        write_atomic(&self.dir.join(MAIN_FILE), &encode(&c))?;
        self.write_log(&log)
    }

    /// Automatické uložení na začátku herního dne.
    pub fn autosave(&self, mut c: Campaign, day: u64) -> Result<()> {
        let log = std::mem::take(&mut c.log);
        let bytes = encode(&c);
        let backups = self.dir.join(BACKUPS);
        fs::create_dir_all(&backups)?;
        write_atomic(&self.dir.join(MAIN_FILE), &bytes)?;
        write_atomic(&backups.join(format!("auto-{day:05}.aae")), &bytes)?;
        rotate(&backups, "auto-", KEEP_AUTO)?;
        self.write_log(&log)
    }

    /// Záloha stavu na začátku relace.
    pub fn session_start(&self, mut c: Campaign) -> Result<()> {
        c.log.clear();
        let bytes = encode(&c);
        let backups = self.dir.join(BACKUPS);
        fs::create_dir_all(&backups)?;
        write_atomic(&self.dir.join(MAIN_FILE), &bytes)?;
        write_atomic(
            &backups.join(format!("relace-{}.aae", self.session)),
            &bytes,
        )?;
        rotate(&backups, "relace-", KEEP_SESSIONS)
    }

    fn write_log(&self, log: &[aae_net::host::LogEntry]) -> Result<()> {
        let logs = self.dir.join(LOGS);
        fs::create_dir_all(&logs)?;
        let mut text = String::with_capacity(log.len() * 160);
        for entry in log {
            text.push_str(&serde_json::to_string(entry)?);
            text.push('\n');
        }
        write_atomic(
            &logs.join(format!("relace-{}.jsonl", self.session)),
            text.as_bytes(),
        )?;
        rotate(&logs, "relace-", KEEP_SESSIONS)
    }
}

/// Smaže nejstarší soubory s předponou nad limit (jména se řadí podle čísla).
fn rotate(dir: &Path, prefix: &str, keep: usize) -> Result<()> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && !n.ends_with(".tmp"))
        })
        .collect();
    files.sort();
    while files.len() > keep {
        let _ = fs::remove_file(files.remove(0));
    }
    Ok(())
}

/// Kampaně ve složce, nejnovější první.
pub fn list(root: &Path) -> Vec<CampaignInfoDto> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<(u64, CampaignInfoDto)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path().join(MAIN_FILE);
            let modified = fs::metadata(&path)
                .ok()?
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis() as u64;
            let c = load(&path).ok()?;
            let day = aae_sim::time::day_of(c.state.now);
            Some((
                modified,
                CampaignInfoDto {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path: path.to_string_lossy().into_owned(),
                    date: crate::texts::date(day),
                    day: day as u32,
                    duration_days: c.state.rules.duration_days as u32,
                    players: c.names.values().cloned().collect(),
                    modified: modified as f64,
                },
            ))
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    out.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aae_sim::data::World;
    use aae_sim::{AirlineId, CampaignRules, Controller, GameState, NewAirline};
    use std::collections::BTreeMap;

    fn campaign() -> Campaign {
        let world = World::europe_v1().unwrap();
        let prg = world.airport_by_code("PRG").unwrap();
        let state = GameState::with_world(
            world,
            CampaignRules::default(),
            7,
            vec![NewAirline {
                name: "Petr Air".into(),
                home: prg,
                controller: Controller::Human,
            }],
        )
        .unwrap();
        Campaign {
            state,
            roster: BTreeMap::new(),
            names: BTreeMap::from([(AirlineId(0), "Petr".to_string())]),
            log: Vec::new(),
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aae-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_keeps_state_hash() {
        let c = campaign();
        let back = decode(&encode(&c)).unwrap();
        assert_eq!(back.state.hash(), c.state.hash());
        assert_eq!(back.names, c.names);
    }

    #[test]
    fn rejects_foreign_and_damaged_files() {
        assert!(decode(b"neni to kampan").is_err());
        let mut bytes = encode(&campaign());
        bytes.truncate(bytes.len() / 2);
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn autosave_rotates_and_lists() {
        let root = temp_dir("saves");
        let dir = new_campaign_dir(&root, "Středeční: liga?").unwrap();
        assert!(dir.ends_with("Středeční- liga-"));
        let saver = Saver::new(dir.clone());
        saver.session_start(campaign()).unwrap();
        for day in 0..13 {
            saver.autosave(campaign(), day).unwrap();
        }
        let autos = fs::read_dir(dir.join(BACKUPS))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("auto-")
            })
            .count();
        assert_eq!(autos, KEEP_AUTO);
        let listed = list(&root);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].players, vec!["Petr".to_string()]);
        let second = new_campaign_dir(&root, "Středeční: liga?").unwrap();
        assert!(second.ends_with("Středeční- liga- (2)"));
        let _ = fs::remove_dir_all(&root);
    }
}
