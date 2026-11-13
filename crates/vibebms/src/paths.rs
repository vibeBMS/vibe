//! Portable directory layout.
//!
//! Everything (config, song database, table cache, replays) lives
//! next to the executable so the whole install can be copied around:
//!
//! ```text
//! <root>/vibebms(.exe)
//! <root>/config/config.toml
//! <root>/cache/songs.json        (scan cache, safe to delete)
//! <root>/scores/scores.json      (your records)
//! <root>/cache/tables/*.json
//! <root>/fonts/*.ttf|otf|ttc        (optional font overrides)
//! <root>/replays/
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
    pub replays: PathBuf,
    pub scores: PathBuf,
    pub skins: PathBuf,
}

/// Directory names under the root that are never scanned for charts.
pub const INTERNAL_DIRS: &[&str] = &[
    "config", "cache", "fonts", "replays", "scores", "target", "skins", "crates", "docs",
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
            replays: root.join("replays"),
            scores: root.join("scores"),
            skins: root.join("skins"),
            root,
        };
        for d in [
            &p.config, &p.cache, &p.tables, &p.replays, &p.scores, &p.skins,
        ] {
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
    // macOS bundle: <root>/vibeBMS.app/Contents/MacOS/vibebms -> <root>
    #[cfg(target_os = "macos")]
    {
        let s = dir.to_string_lossy();
        if s.ends_with(".app/Contents/MacOS") {
            if let Some(r) = dir.parent().and_then(Path::parent).and_then(Path::parent) {
                return r.to_path_buf();
            }
        }
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
