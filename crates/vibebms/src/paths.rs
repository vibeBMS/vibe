//! Portable directory layout.
//!
//! Everything (config, song database, table cache) lives
//! next to the executable so the whole install can be copied around:
//!
//! ```text
//! <root>/vibebms(.exe)
//! <root>/config/config.toml
//! <root>/cache/songs.json        (scan cache, safe to delete)
//! <root>/cache/loudness.txt      (charts' measured loudness, safe to delete)
//! <root>/scores/scores.json      (your records)
//! <root>/scores/player.json      (totals over all plays)
//! <root>/scores/history/         (every play, a file per month: `ir::store`)
//! <root>/scores/outbox/          (IR submissions waiting to be sent)
//! <root>/cache/tables/*.json
//! <root>/fonts/*.ttf|otf|ttc        (optional font overrides)
//! <root>/<any folder with BMS files> (scanned automatically)
//! ```

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
    pub tables: PathBuf,
    pub fonts: PathBuf,
    pub scores: PathBuf,
    /// Made as they are first written (`platform::store`).
    pub history: PathBuf,
    pub outbox: PathBuf,
    pub skins: PathBuf,
    /// Where IR keys are kept (`ir::creds`): `config`, Android's private
    /// storage where the root is the shared one.
    pub private: PathBuf,
}

/// Directory names under the root that are never scanned for charts.
pub const INTERNAL_DIRS: &[&str] = &[
    "config",
    "cache",
    "fonts",
    "scores",
    "target",
    "skins",
    "sounds",
    "crates",
    "docs",
    "downloads",
];

impl Paths {
    pub fn detect() -> Paths {
        if crate::platform::IS_WEB {
            return Paths::at(PathBuf::from("/vibebms"));
        }
        let root = std::env::var_os("VIBEBMS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(default_root);
        let root = if crate::platform::vfs::writable(&root) {
            root
        } else {
            fallback_root()
        };
        Paths::at(root)
    }

    pub fn at(root: PathBuf) -> Paths {
        let p = Paths {
            config: root.join("config"),
            cache: root.join("cache"),
            tables: root.join("cache").join("tables"),
            fonts: root.join("fonts"),
            scores: root.join("scores"),
            history: root.join("scores").join("history"),
            outbox: root.join("scores").join("outbox"),
            skins: root.join("skins"),
            private: root.join("config"),
            root,
        };
        for d in [&p.config, &p.cache, &p.tables, &p.scores, &p.skins] {
            let _ = crate::platform::vfs::create_dir_all(d);
        }
        p
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn songs_cache(&self) -> PathBuf {
        self.cache.join("songs.json")
    }
}

fn default_root() -> PathBuf {
    #[cfg(target_os = "ios")]
    {
        // app sandbox: Documents is visible in the Files app
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join("Documents");
        }
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok());
    let Some(exe) = exe else {
        return PathBuf::from(".");
    };
    let dir = exe
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    // macOS bundle: <root>/vibeBMS.app/Contents/MacOS/vibebms -> <root>;
    // in an Applications folder, or where it cannot write (a disk image, App
    // Translocation), the user's Application Support
    #[cfg(target_os = "macos")]
    if let Some(r) = crate::update::app_bundle(&exe).and_then(Path::parent) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        return match home {
            Some(h) if r.ends_with("Applications") || !crate::platform::vfs::writable(r) => {
                h.join("Library/Application Support/vibeBMS")
            }
            _ => r.to_path_buf(),
        };
    }
    // `cargo run`: use the workspace root instead of target/debug
    let s = dir.to_string_lossy().replace('\\', "/");
    if s.ends_with("/target/debug") || s.ends_with("/target/release") {
        if let Some(r) = dir.parent().and_then(Path::parent) {
            return r.to_path_buf();
        }
    }
    dir
}

fn fallback_root() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    log::warn!(
        "executable directory is not writable; using {}",
        base.display()
    );
    base.join("vibeBMS")
}
