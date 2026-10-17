//! Profil hráče: jméno a tajný klíč identity (R6). Leží v datech aplikace
//! a klíč počítač nikdy neopouští; hostitel hráče pozná podle veřejného klíče.

use std::fs;
use std::path::{Path, PathBuf};

use aae_net::transport::iroh_net::generate_secret;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::dto::parse_hex;
use crate::saves::write_atomic;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// Tajný klíč iroh (hex).
    secret: String,
}

impl Profile {
    /// Soubor profilu. Proměnná `AAE_PROFIL` dovolí druhý profil na jednom
    /// počítači (zkouška hostitele a hosta ve dvou oknech).
    pub fn path(app_data: &Path) -> PathBuf {
        match std::env::var("AAE_PROFIL") {
            Ok(p) if !p.trim().is_empty() => app_data.join(format!("profil-{}.json", p.trim())),
            _ => app_data.join("profil.json"),
        }
    }

    pub fn load_or_create(path: &Path) -> Result<Profile> {
        let existing = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Profile>(&text).ok())
            .filter(|p| parse_hex(&p.secret).is_some());
        if let Some(p) = existing {
            return Ok(p);
        }
        let p = Profile {
            name: String::new(),
            secret: crate::dto::hex(&aae_net::PeerKey(generate_secret())),
        };
        p.save(path)?;
        Ok(p)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).context("nelze založit složku s profilem")?;
        }
        let text = serde_json::to_string_pretty(self)?;
        write_atomic(path, text.as_bytes())
    }

    pub fn secret(&self) -> [u8; 32] {
        parse_hex(&self.secret).expect("klíč profilu se kontroluje při načtení")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_and_keeps_key() {
        let dir = std::env::temp_dir().join(format!("aae-profil-{}", std::process::id()));
        let path = dir.join("profil.json");
        let _ = fs::remove_dir_all(&dir);
        let a = Profile::load_or_create(&path).unwrap();
        let mut b = Profile::load_or_create(&path).unwrap();
        assert_eq!(a.secret(), b.secret());
        b.name = "Petr".into();
        b.save(&path).unwrap();
        let c = Profile::load_or_create(&path).unwrap();
        assert_eq!(c.name, "Petr");
        assert_eq!(c.secret(), a.secret());
        let _ = fs::remove_dir_all(&dir);
    }
}
