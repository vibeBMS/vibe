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
//! <root>/config/profiles.toml   (the profiles, the one in use: `profiles`)
//! <root>/profiles/<id>/config/config.toml, scores/...  (another profile)
//! ```
//!
//! The player's own files (`config`, `scores`, `history`, `outbox`,
//! `private`, the replays) are the profile's in use: at the root for the
//! default profile (as before there were profiles), under
//! `profiles/<id>/` for another. The library, its caches, skins, sound
//! sets and fonts are the root's, shared by every profile.

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
    /// The profile in use: its folder's name under `profiles/`, "" for
    /// the default profile (whose files are at the root).
    pub profile: String,
    /// The profile's data folder: the root, or `profiles/<id>`.
    pub player: PathBuf,
    /// The default profile's `private` (another's is below it).
    private_root: PathBuf,
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
    "profiles",
];

impl Paths {
    /// The data root: `VIBEBMS_ROOT`, else next to the executable (a
    /// folder of the user's where that cannot be written to).
    pub fn detect_root() -> PathBuf {
        if crate::platform::IS_WEB {
            return PathBuf::from("/vibebms");
        }
        let root = std::env::var_os("VIBEBMS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(default_root);
        if crate::platform::vfs::writable(&root) {
            root
        } else {
            fallback_root()
        }
    }

    /// The folders at `root`, of the default profile.
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
            profile: String::new(),
            player: root.clone(),
            private_root: root.join("config"),
            root,
        };
        for d in [&p.config, &p.cache, &p.tables, &p.scores, &p.skins] {
            let _ = crate::platform::vfs::create_dir_all(d);
        }
        p
    }

    /// The same root, the player's files those of profile `id` ("": the
    /// default).
    pub fn with_profile(&self, id: &str) -> Paths {
        let player = profile_dir(&self.root, id);
        let p = Paths {
            config: player.join("config"),
            scores: player.join("scores"),
            history: player.join("scores").join("history"),
            outbox: player.join("scores").join("outbox"),
            private: profile_private(&self.private_root, id),
            profile: id.to_string(),
            player,
            ..self.clone()
        };
        for d in [&p.config, &p.scores] {
            let _ = crate::platform::vfs::create_dir_all(d);
        }
        p
    }

    /// Where profile `id` keeps its IR keys.
    pub fn private_of(&self, id: &str) -> PathBuf {
        profile_private(&self.private_root, id)
    }

    /// Where the default profile keeps its IR keys (Android: storage of
    /// the app's own); the profile in use's follows.
    pub fn set_private_root(&mut self, dir: PathBuf) {
        self.private = profile_private(&dir, &self.profile);
        self.private_root = dir;
    }

    /// The profile's settings (with the app-wide ones of the root's:
    /// `profiles::load_config`).
    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    /// The root's settings: the default profile's, and the app-wide ones
    /// of every profile.
    pub fn app_config_file(&self) -> PathBuf {
        self.root.join("config").join("config.toml")
    }

    /// The profile's replays (`library::replay`).
    pub fn replays(&self) -> PathBuf {
        crate::library::replay::dir(&self.player)
    }

    pub fn songs_cache(&self) -> PathBuf {
        self.cache.join("songs.json")
    }
}

/// Profile `id`'s data folder under `root` (the root itself for "").
pub fn profile_dir(root: &Path, id: &str) -> PathBuf {
    match id {
        "" => root.to_path_buf(),
        id => root.join("profiles").join(id),
    }
}

fn profile_private(private_root: &Path, id: &str) -> PathBuf {
    match id {
        "" => private_root.to_path_buf(),
        id => private_root.join("profiles").join(id),
    }
}

/// The app `exe` is the executable of, by the macOS bundle layout
/// (`<name>.app/Contents/MacOS/<exe>`).
pub fn app_bundle(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let ok = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app";
    ok.then_some(app)
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
    if let Some(r) = app_bundle(&exe).and_then(Path::parent) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bundles() {
        let exe = Path::new("/Applications/vibeBMS Nightly.app/Contents/MacOS/vibebms");
        assert_eq!(
            app_bundle(exe),
            Some(Path::new("/Applications/vibeBMS Nightly.app"))
        );
        for p in [
            "/Applications/vibeBMS.app/Contents/Resources/vibebms",
            "/Applications/vibeBMS/Contents/MacOS/vibebms",
            "/home/me/vibeBMS/vibebms",
            "vibebms",
        ] {
            assert_eq!(app_bundle(Path::new(p)), None, "{p}");
        }
    }
}
