//! The settings, in the user's config folder (`bmseeker/config.json`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The BMS folder songs go into.
    pub library: PathBuf,
    /// Difficulty table URLs.
    pub tables: Vec<String>,
    pub fill_links: bool,
    /// Where LR2 custom folders are written.
    pub lr2_out: PathBuf,
    /// beatoraja's table folder.
    pub bmt_out: PathBuf,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            library: PathBuf::new(),
            tables: bmseeker::table::DEFAULT_TABLES
                .iter()
                .map(|(_, u)| u.to_string())
                .collect(),
            fill_links: true,
            lr2_out: PathBuf::new(),
            bmt_out: PathBuf::new(),
        }
    }
}

/// The user's config folder for bmseeker.
pub fn dir() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("bmseeker")
}

impl Config {
    pub fn load() -> Config {
        std::fs::read(dir().join("config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let d = dir();
        let _ = std::fs::create_dir_all(&d);
        if let Ok(j) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(d.join("config.json"), j);
        }
    }
}
