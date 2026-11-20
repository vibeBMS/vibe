//! Player profiles: several players (or one player's separate starts) on
//! one library, switched without restarting.
//!
//! The default profile is the data at the root, as it was before there were
//! profiles (nothing moves): `config/config.toml`, `scores/` (records,
//! totals, favourites, courses, history, the IR outbox, replays). Another
//! profile keeps the same files under `profiles/<id>/`. `config/
//! profiles.toml` lists them, by folder (`id`) and name, and says which is
//! in use.
//!
//! Shared by every profile: the library and its caches (`cache/`), skins
//! and sound sets, fonts, and the app-wide settings ([`take_app_wide`]:
//! the window and graphics, the audio device, which controller is 1P and
//! which 2P, the song folders, tables and download sources, updates,
//! Discord and OBS), which stay in the root's `config.toml`. The
//! rest of the settings is the profile's: play options and offsets, keys
//! and touch, volumes, skins and their options, IR accounts (their keys
//! too: `ir::creds` under `paths.private`).

use crate::config::Config;
use crate::library::{BestScore, RecordKey};
use crate::paths::{profile_dir, Paths};
use crate::platform::vfs;
use serde::{Deserialize, Serialize};
use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The default profile's name until it is renamed.
pub const DEFAULT_NAME: &str = "DEFAULT";
/// The longest name (in characters).
pub const NAME_MAX: usize = 32;

/// A profile: its folder under `profiles/` ("" the default's, the root),
/// and its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
}

/// `config/profiles.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Registry {
    /// The profile in use.
    pub active: String,
    /// The default first.
    #[serde(rename = "profile")]
    pub profiles: Vec<Profile>,
}

/// The app-wide settings, each listed once: the TOML keys a profile's file
/// leaves out ([`profile_toml`]) and the fields [`take_app_wide`] copies
/// are made from the same names.
macro_rules! app_wide {
    (
        $($top:ident),* ;
        audio: $($(#[$m:meta])* $audio:ident),* ;
        keys: $($keys:ident),* $(;)?
    ) => {
        const APP_KEYS: &[&str] = &[$(stringify!($top)),*];
        const APP_AUDIO_KEYS: &[&str] = &[$(stringify!($audio)),*];
        const APP_KEYS_KEYS: &[&str] = &[$(stringify!($keys)),*];

        /// The app-wide settings of `from` into `into` (the rest is the
        /// profile's).
        pub fn take_app_wide(into: &mut Config, from: &Config) {
            $(into.$top.clone_from(&from.$top);)*
            $($(#[$m])* into.audio.$audio.clone_from(&from.audio.$audio);)*
            $(into.keys.$keys.clone_from(&from.keys.$keys);)*
        }
    };
}

// What belongs to the machine, its screen and sound device, and to the
// library; this machine's other apps (Discord, OBS); of
// `[keys]`, which physical controller is 1P and which 2P (the bindings are
// the profile's).
app_wide! {
    display, bms_dirs, scan_root, tables, archives, third_party_mirrors,
    third_party_acknowledged, name_song_folders, check_updates, language,
    discord, obs;
    audio: buffer_frames, sample_rate, mute_inactive, #[cfg(windows)] exclusive;
    keys: controllers;
}

/// The settings of the profile in use: its file, the app-wide ones from
/// the root's.
pub fn load_config(paths: &Paths) -> Config {
    let mut c = Config::load(&paths.config_file());
    if !paths.profile.is_empty() {
        take_app_wide(&mut c, &Config::load(&paths.app_config_file()));
    }
    c
}

/// Save `c` as the profile's settings: another profile's file without the
/// app-wide ones, which go into the root's (when they changed).
pub fn save_config(c: &Config, paths: &Paths) {
    if paths.profile.is_empty() {
        return c.save(&paths.config_file());
    }
    match profile_toml(c) {
        Ok(s) => {
            if let Err(e) = vfs::write(&paths.config_file(), s.as_bytes()) {
                log::error!("saving config: {e}");
            }
        }
        Err(e) => log::error!("config serialize error: {e}"),
    }
    let file = paths.app_config_file();
    let mut app = Config::load(&file);
    let before = app.clone();
    take_app_wide(&mut app, c);
    if app != before || !vfs::is_file(&file) {
        app.save(&file);
    }
}

/// `c`'s text without the app-wide settings.
fn profile_toml(c: &Config) -> Result<String, toml::ser::Error> {
    let mut v = toml::Value::try_from(c)?;
    if let Some(t) = v.as_table_mut() {
        for k in APP_KEYS {
            t.remove(*k);
        }
        if let Some(a) = t.get_mut("audio").and_then(toml::Value::as_table_mut) {
            for k in APP_AUDIO_KEYS {
                a.remove(*k);
            }
        }
        if let Some(a) = t.get_mut("keys").and_then(toml::Value::as_table_mut) {
            for k in APP_KEYS_KEYS {
                a.remove(*k);
            }
        }
    }
    toml::to_string_pretty(&v)
}

/// A folder name a profile can have: letters, digits, `-` and `_`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A name as it is kept (trimmed), or why it cannot be one.
fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(tr!("A profile needs a name").into());
    }
    if name.chars().any(char::is_control) {
        return Err(tr!("A profile's name is one line").into());
    }
    if name.chars().count() > NAME_MAX {
        return Err(tr!(
            "A profile's name has at most {n} characters",
            n = NAME_MAX
        ));
    }
    Ok(name.to_string())
}

/// The folder name made from `name` (the letters and digits of ASCII,
/// lowercase; spaces as `-`).
fn id_of(name: &str) -> String {
    let mut id: String = name
        .chars()
        .filter_map(|c| match c {
            c if c.is_ascii_alphanumeric() => Some(c.to_ascii_lowercase()),
            '-' | '_' => Some(c),
            c if c.is_whitespace() => Some('-'),
            _ => None,
        })
        .take(24)
        .collect();
    id = id.trim_matches('-').to_string();
    // names Windows keeps for devices
    const DEVICES: &[&str] = &["con", "prn", "aux", "nul", "com1", "lpt1"];
    if id.is_empty() || DEVICES.contains(&id.as_str()) {
        id.insert_str(0, "profile");
    }
    id
}

impl Registry {
    pub fn file(root: &Path) -> PathBuf {
        root.join("config").join("profiles.toml")
    }

    /// The profiles at `root`: the file's, the default first; natively
    /// also a folder under `profiles/` it does not list (one copied in).
    pub fn load(root: &Path) -> Registry {
        let mut r: Registry = vfs::read_to_string(&Registry::file(root))
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        r.profiles
            .retain(|p| (p.id.is_empty() || valid_id(&p.id)) && seen.insert(p.id.clone()));
        match r.profiles.iter().position(|p| p.id.is_empty()) {
            Some(0) => {}
            Some(i) => {
                let d = r.profiles.remove(i);
                r.profiles.insert(0, d);
            }
            None => r.profiles.insert(
                0,
                Profile {
                    id: String::new(),
                    name: DEFAULT_NAME.into(),
                },
            ),
        }
        if !crate::platform::IS_WEB {
            let mut found: Vec<String> = std::fs::read_dir(root.join("profiles"))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .filter(|id| valid_id(id) && !r.has(id))
                .collect();
            found.sort();
            for id in found {
                let name: String = id.chars().take(NAME_MAX).collect();
                r.profiles.push(Profile { id, name });
            }
        }
        if !r.has(&r.active.clone()) {
            r.active.clear();
        }
        r
    }

    pub fn save(&self, root: &Path) {
        match toml::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = vfs::write(&Registry::file(root), s.as_bytes()) {
                    log::error!("saving the profiles: {e}");
                }
            }
            Err(e) => log::error!("saving the profiles: {e}"),
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// Profile `id`'s name ("" when there is none).
    pub fn name(&self, id: &str) -> &str {
        self.get(id).map_or("", |p| p.name.as_str())
    }

    /// A name no other profile than `id` has (case aside).
    fn free_name(&self, name: &str, id: Option<&str>) -> Result<(), String> {
        let taken = (self.profiles.iter())
            .any(|p| Some(p.id.as_str()) != id && p.name.to_lowercase() == name.to_lowercase());
        match taken {
            true => Err(tr!("There is a profile named {name} already", name = name)),
            false => Ok(()),
        }
    }

    /// A new profile named `name`, its settings `from`'s (the profile it
    /// was made from) but the player's name and the IR accounts, no
    /// records; its id.
    pub fn create(&mut self, paths: &Paths, name: &str, from: &Config) -> Result<String, String> {
        let name = clean_name(name)?;
        self.free_name(&name, None)?;
        let base = id_of(&name);
        let used = |id: &str| {
            let dir = profile_dir(&paths.root, id);
            self.has(id)
                || vfs::is_dir(&dir)
                || vfs::is_file(&dir.join("config").join("config.toml"))
                || vfs::is_file(&dir.join("scores").join("scores.json"))
        };
        let id = (1..)
            .map(|n| match n {
                1 => base.clone(),
                n => format!("{base}-{n}"),
            })
            .find(|id| !used(id))
            .unwrap_or(base);
        let p = paths.with_profile(&id);
        let mut c = from.clone();
        c.player_name.clone_from(&name);
        c.ir = Default::default();
        c.play.target_rival = None;
        c.play.target_profile = None;
        save_config(&c, &p);
        self.profiles.push(Profile {
            id: id.clone(),
            name,
        });
        self.save(&paths.root);
        Ok(id)
    }

    pub fn rename(&mut self, root: &Path, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        self.free_name(&name, Some(id))?;
        let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) else {
            return Err(tr!("No such profile").into());
        };
        p.name = name;
        self.save(root);
        Ok(())
    }

    /// Delete profile `id` and its files (records, replays, settings, IR
    /// keys). The default profile (the data at the root) is never
    /// deleted, so there is always one left; nor is the one in use.
    pub fn delete(&mut self, paths: &Paths, id: &str) -> Result<(), String> {
        if id.is_empty() {
            return Err(tr!("The default profile is not deleted (its data is the root's)").into());
        }
        if id == paths.profile {
            return Err(tr!("The profile in use is not deleted: switch to another first").into());
        }
        if !self.has(id) {
            return Err(tr!("No such profile").into());
        }
        let dir = profile_dir(&paths.root, id);
        crate::platform::store::remove_below(&dir);
        vfs::remove_dir(&dir);
        let private = paths.private_of(id);
        if !private.starts_with(&dir) {
            vfs::remove_dir(&private);
        }
        self.profiles.retain(|p| p.id != id);
        if self.active == id {
            self.active.clear();
        }
        self.save(&paths.root);
        Ok(())
    }
}

/// The profiles and the other profiles' records (local rivals), read when
/// first asked for.
#[derive(Default)]
pub struct Profiles {
    pub registry: Registry,
    rivals: HashMap<String, OnceCell<HashMap<RecordKey, BestScore>>>,
    root: PathBuf,
    /// The profile to switch to (the app does it: `Ctx::switch_profile`).
    pub switch_to: Option<String>,
}

impl Profiles {
    /// The profiles at `root`, and the folders of the one in use.
    pub fn open_at(root: PathBuf) -> (Paths, Profiles) {
        let registry = Registry::load(&root);
        let paths = Paths::at(root).with_profile(&registry.active);
        let profiles = Profiles::new(registry, &paths);
        (paths, profiles)
    }

    /// The profiles at `paths`' root; the others than the one in use are
    /// the local rivals.
    pub fn open(paths: &Paths) -> Profiles {
        Profiles::new(Registry::load(&paths.root), paths)
    }

    fn new(registry: Registry, paths: &Paths) -> Profiles {
        let rivals = (registry.profiles.iter())
            .filter(|p| p.id != paths.profile)
            .map(|p| (p.id.clone(), OnceCell::new()))
            .collect();
        Profiles {
            registry,
            rivals,
            root: paths.root.clone(),
            switch_to: None,
        }
    }

    /// The profile of `paths` in use from now on (saved); the one left is a
    /// local rival.
    pub fn set_active(&mut self, paths: &Paths) {
        let mut registry = std::mem::take(&mut self.registry);
        registry.active.clone_from(&paths.profile);
        registry.save(&paths.root);
        *self = Profiles::new(registry, paths);
    }

    /// Local rival `id`'s records (another profile's), as they were when
    /// first asked for.
    pub fn rival(&self, id: &str) -> Option<&HashMap<RecordKey, BestScore>> {
        let cell = self.rivals.get(id)?;
        Some(cell.get_or_init(|| {
            crate::library::read_records(&profile_dir(&self.root, id).join("scores"))
        }))
    }

    /// The other profiles, as rivals: id and name.
    pub fn rivals(&self) -> impl Iterator<Item = &Profile> {
        (self.registry.profiles.iter()).filter(|p| self.rivals.contains_key(&p.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("vibebms-profiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn ids_from_names() {
        assert_eq!(id_of("Alice"), "alice");
        assert_eq!(id_of("  two words "), "two-words");
        assert_eq!(id_of("ゆう"), "profile");
        assert_eq!(id_of("CON"), "profilecon");
        assert!(valid_id(&id_of("a/b\\..c")));
        assert!(clean_name(" ").is_err());
        assert!(clean_name(&"x".repeat(NAME_MAX + 1)).is_err());
    }

    /// Without `profiles.toml` the data at the root is the default (and
    /// only) profile, its files where they always were.
    #[test]
    fn default_profile_is_the_old_layout() {
        let root = temp("default");
        let p = Profiles::open_at(root.clone()).0;
        assert_eq!(p.profile, "");
        assert_eq!(p.player, root);
        assert_eq!(p.config_file(), root.join("config/config.toml"));
        assert_eq!(p.scores, root.join("scores"));
        assert_eq!(p.history, root.join("scores/history"));
        assert_eq!(p.outbox, root.join("scores/outbox"));
        assert_eq!(p.replays(), root.join("scores/replays"));
        assert_eq!(p.private, root.join("config"));
        let r = Registry::load(&root);
        assert_eq!(r.profiles.len(), 1);
        assert_eq!((r.active.as_str(), r.name("")), ("", DEFAULT_NAME));
        // the default profile's settings are the whole file, as before
        let mut c = Config::default();
        c.display.width = 1600;
        c.play.sudden = 0.25;
        save_config(&c, &p);
        assert_eq!(Config::load(&root.join("config/config.toml")), c);
        assert_eq!(load_config(&p), c);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn create_rename_switch_delete() {
        let root = temp("crud");
        let paths = Profiles::open_at(root.clone()).0;
        let mut r = Registry::load(&root);
        let mut mine = Config::default();
        mine.play.judge_offset_ms = 12;
        mine.display.width = 1600;
        mine.ir.accounts.push(Default::default());
        let id = r.create(&paths, "Alice B", &mine).unwrap();
        assert_eq!(id, "alice-b");
        // a second of the same name: refused; of the same folder: another
        assert!(r.create(&paths, "alice b", &mine).is_err());
        assert_eq!(r.create(&paths, "Alice-B", &mine).unwrap(), "alice-b-2");
        assert!(root.join("profiles/alice-b/config/config.toml").is_file());
        // the new profile has the settings it was made from, but the name
        // and the IR accounts
        let a = paths.with_profile(&id);
        assert_eq!(a.player, root.join("profiles/alice-b"));
        assert_eq!(a.replays(), root.join("profiles/alice-b/scores/replays"));
        let c = load_config(&a);
        assert_eq!(c.play.judge_offset_ms, 12);
        assert_eq!(c.player_name, "Alice B");
        assert!(c.ir.accounts.is_empty());
        // switched to: the file says so, and the paths follow
        r.active.clone_from(&id);
        r.save(&root);
        assert_eq!(Profiles::open_at(root.clone()).0.player, a.player);
        // renamed: the folder stays
        r.rename(&root, &id, "Carol").unwrap();
        assert!(r.rename(&root, &id, "  ").is_err());
        assert!(r.rename(&root, "", "carol").is_err());
        assert_eq!(Registry::load(&root).name(&id), "Carol");
        // the default and the one in use are never deleted
        let using = paths.with_profile(&id);
        assert!(r.delete(&using, "").is_err());
        assert!(r.delete(&using, &id).is_err());
        // deleted from another: its files go, the default is in use
        r.delete(&paths, &id).unwrap();
        assert!(!root.join("profiles/alice-b").exists());
        let back = Registry::load(&root);
        assert_eq!(back.active, "");
        assert!(!back.has(&id) && back.has("alice-b-2"));
        std::fs::remove_dir_all(&root).ok();
    }

    /// The app-wide settings are the root's whichever profile changes
    /// them; the others are each profile's.
    #[test]
    fn app_wide_settings_are_shared() {
        let root = temp("shared");
        let paths = Profiles::open_at(root.clone()).0;
        let mut r = Registry::load(&root);
        let mut def = Config::default();
        def.play.judge_offset_ms = 5;
        save_config(&def, &paths);
        let b = paths.with_profile(&r.create(&paths, "B", &def).unwrap());
        let mut c = load_config(&b);
        c.play.judge_offset_ms = -20;
        c.audio.master = 0.3;
        c.display.fullscreen = true;
        c.audio.buffer_frames = 1024;
        c.bms_dirs.push("D:/bms".into());
        c.obs.host = "obs.lan".into();
        c.discord.enabled = true;
        c.third_party_acknowledged = true;
        c.keys.controllers.p1 = Some(crate::config::PadId {
            name: "pad".into(),
            id: "1".into(),
        });
        c.keys.k4[0].keys.clear();
        // (a local rival as TARGET and beside: written and read back)
        c.play.target_profile = Some("friend".into());
        c.ir.beside = crate::config::Beside::Profile("friend".into());
        save_config(&c, &b);
        // the profile's file has its own settings only
        let text = std::fs::read_to_string(b.config_file()).unwrap();
        assert!(
            !text.contains("[display]") && !text.contains("bms_dirs"),
            "{text}"
        );
        assert!(
            !text.contains("buffer_frames") && text.contains("master"),
            "{text}"
        );
        assert!(
            !text.contains("obs.lan") && !text.contains("controllers"),
            "{text}"
        );
        assert!(!text.contains("third_party"), "{text}");
        assert!(text.contains("k4"), "the 4K keys are the profile's: {text}");
        let d = load_config(&paths);
        assert_eq!(d.play.judge_offset_ms, 5);
        assert_eq!(d.audio.master, Config::default().audio.master);
        assert!(d.display.fullscreen);
        assert_eq!(d.audio.buffer_frames, 1024);
        assert_eq!(d.bms_dirs, ["D:/bms"]);
        assert_eq!((d.obs.host.as_str(), d.discord.enabled), ("obs.lan", true));
        assert_eq!(d.keys.controllers, c.keys.controllers);
        assert!(d.third_party_acknowledged);
        assert_eq!(d.keys.k4, Config::default().keys.k4);
        assert_eq!(load_config(&b), c);
        std::fs::remove_dir_all(&root).ok();
    }

    /// Every app-wide setting set off its default goes to the root's file
    /// and none to the profile's; the profile reads them all back, and its
    /// own settings stay its own.
    #[test]
    fn every_app_wide_setting_round_trips() {
        let root = temp("app-wide");
        let paths = Profiles::open_at(root.clone()).0;
        let b = paths.with_profile(
            &Registry::load(&root)
                .create(&paths, "B", &Config::default())
                .unwrap(),
        );
        let mut c = load_config(&b);
        c.display.width = 1600;
        c.bms_dirs.push("D:/bms".into());
        c.scan_root = !c.scan_root;
        c.tables.clear();
        c.archives.push("https://events.invalid/index.json".into());
        c.third_party_mirrors = !c.third_party_mirrors;
        c.third_party_acknowledged = true;
        c.name_song_folders = !c.name_song_folders;
        c.check_updates = !c.check_updates;
        c.language = crate::i18n::Language::Ja;
        c.discord.enabled = true;
        c.obs.host = "obs.lan".into();
        c.audio.buffer_frames = 1024;
        c.audio.sample_rate = 48_000;
        c.audio.mute_inactive = false;
        #[cfg(windows)]
        {
            c.audio.exclusive = true;
        }
        c.keys.controllers.p1 = Some(crate::config::PadId {
            name: "pad".into(),
            id: "1".into(),
        });
        c.play.judge_offset_ms = -20;
        // (each listed setting is off its default here: one added to the
        // list is to be set above too)
        let value = |c: &Config| toml::Value::try_from(c).unwrap();
        let (mine, def) = (value(&c), value(&Config::default()));
        let keys = (APP_KEYS.iter().map(|k| vec![*k]))
            .chain(APP_AUDIO_KEYS.iter().map(|k| vec!["audio", k]))
            .chain(APP_KEYS_KEYS.iter().map(|k| vec!["keys", k]));
        let at = |v: &toml::Value, path: &[&str]| {
            path.iter().try_fold(v.clone(), |v, k| v.get(k).cloned())
        };
        for path in keys {
            if !cfg!(windows) && path == ["audio", "exclusive"] {
                continue;
            }
            assert_ne!(
                at(&mine, &path),
                at(&def, &path),
                "{path:?} is at its default"
            );
        }
        save_config(&c, &b);
        let text = std::fs::read_to_string(b.config_file()).unwrap();
        let file: toml::Value = toml::from_str(&text).unwrap();
        for k in APP_KEYS {
            assert!(file.get(k).is_none(), "{k} in the profile's file: {text}");
        }
        for k in APP_AUDIO_KEYS {
            assert!(
                file["audio"].get(k).is_none(),
                "audio.{k} in the profile's file"
            );
        }
        assert!(file["keys"].get("controllers").is_none());
        assert_eq!(load_config(&b), c);
        // the default profile has them, not the other's own
        let d = load_config(&paths);
        let mut want = c.clone();
        want.play = Config::default().play;
        want.player_name = d.player_name.clone();
        assert_eq!(d, want);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A profile folder copied in is listed; one another profile's
    /// records are read from, not written.
    #[test]
    fn found_folders_and_rival_records() {
        let root = temp("found");
        let scores = root.join("profiles/friend/scores");
        std::fs::create_dir_all(&scores).unwrap();
        let rec = r#"[{"sha256":"aa","ln":0,"rule":"Lr2","clear":"Hard","ex_score":1500,
            "notes":1000,"judges":[700,100,0,0,0,0],"max_combo":800,"min_bp":3,
            "play_count":4,"clear_count":4,"last_played":0},
            {"sha256":5}]"#;
        std::fs::write(scores.join("scores.json"), rec).unwrap();
        let paths = Profiles::open_at(root.clone()).0;
        let p = Profiles::open(&paths);
        assert_eq!(p.registry.name("friend"), "friend");
        let names: Vec<&str> = p.rivals().map(|r| r.id.as_str()).collect();
        assert_eq!(names, ["friend"]);
        let recs = p.rival("friend").unwrap();
        let k = RecordKey::new("aa", 0, vibebms_core::judge::JudgePreset::Lr2);
        assert_eq!(recs[&k].ex_score, 1500);
        assert_eq!(recs.len(), 1);
        assert!(p.rival("").is_none(), "the profile in use is no rival");
        assert!(!scores.join("scores.json.bak").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    /// A play of `ex` EX score (`ex` PGREATs) recorded for chart `sha`.
    fn play(ctx: &mut crate::app::Ctx, sha: &str, ex: u32) {
        use vibebms_core::judge::{Judge, JudgePreset};
        use vibebms_core::score::{ClearType, Score};
        let mut sc = Score {
            total_notes: ex,
            passed: ex,
            ..Default::default()
        };
        for _ in 0..ex {
            sc.add(Judge::PGreat, 0);
        }
        let db = ctx.lib.db.as_mut().unwrap();
        let k = RecordKey::new(sha, 0, JudgePreset::Beatoraja);
        db.record_play(&k, &sc, ClearType::Hard, 60_000);
        ctx.lib.scores = db.scores();
    }

    /// Switching reads the other profile's settings and records; each
    /// keeps its own files; the profile left is a local rival (its lamp
    /// beside the player's, its EX score as TARGET).
    #[test]
    fn switching_keeps_records_apart_and_the_other_is_a_rival() {
        use crate::config::{Beside, Target};
        use crate::scene::target::{self, RivalKey};
        let root = temp("switch");
        let mut ctx = crate::app::Ctx::for_test(root.clone());
        ctx.open_records();
        ctx.reload_library();
        play(&mut ctx, "aa", 30);
        ctx.config.play.judge_offset_ms = 7;
        ctx.save_config();
        let id = ctx.create_profile("Two").unwrap();
        assert!(ctx.switch_profile(&id));
        assert_eq!(ctx.paths.player, root.join("profiles/two"));
        assert_eq!(ctx.paths.scores, root.join("profiles/two/scores"));
        assert!(ctx.lib.scores.is_empty(), "no records of the default's");
        assert_eq!(
            ctx.config.play.judge_offset_ms, 7,
            "its settings made from the default's"
        );
        assert_eq!(ctx.config.player_name, "Two");
        ctx.config.play.judge_offset_ms = -3;
        play(&mut ctx, "bb", 10);
        ctx.save_config();
        // the default as a local rival: TARGET and the lamp beside
        let rivals = target::rivals(&ctx);
        assert_eq!(rivals.len(), 1);
        assert_eq!(rivals[0].key, RivalKey::Local(String::new()));
        let label = (0..Target::ALL.len())
            .map(|_| target::step(&mut ctx.config.play, &rivals, 1))
            .find(|l| l.starts_with("RIVAL"));
        assert_eq!(label.as_deref(), Some("RIVAL DEFAULT"));
        assert_eq!(ctx.config.play.target, Target::Rival);
        assert_eq!(ctx.config.play.target_profile.as_deref(), Some(""));
        let song = |sha: &str| crate::library::Song {
            sha256: sha.into(),
            ..Default::default()
        };
        assert_eq!(target::score(&ctx, &song("aa"), 100, 0), 60);
        assert_eq!(target::score(&ctx, &song("aa"), 50, 0), 50, "at most MAX");
        assert_eq!(target::score(&ctx, &song("bb"), 100, 0), 0);
        assert_eq!(target::name(&ctx).1.as_deref(), Some("DEFAULT"));
        ctx.config.ir.beside = Beside::Profile(String::new());
        let b = ctx.rival_best_of("", &song("aa")).unwrap();
        assert_eq!(
            (b.ex_score, b.clear),
            (60, vibebms_core::score::ClearType::Hard)
        );
        // back: the default's own again
        assert!(ctx.switch_profile(""));
        assert_eq!(ctx.config.play.judge_offset_ms, 7);
        assert_eq!(ctx.lib.scores.len(), 1);
        assert!(ctx.best_key("aa", 0, Default::default()).is_some());
        assert!(target::rivals(&ctx)
            .iter()
            .any(|r| r.key == RivalKey::Local(id.clone())));
        assert_eq!(ctx.rival_best_of(&id, &song("bb")).unwrap().ex_score, 20);
        let other = std::fs::read_to_string(root.join("profiles/two/scores/scores.json")).unwrap();
        let mine = std::fs::read_to_string(root.join("scores/scores.json")).unwrap();
        assert!(other.contains("bb") && !other.contains("\"aa\""), "{other}");
        assert!(mine.contains("aa") && !mine.contains("\"bb\""), "{mine}");
        assert_eq!(Registry::load(&root).active, "");
        // the profile in use cannot go; another can
        assert!(ctx.delete_profile("").is_err());
        ctx.delete_profile(&id).unwrap();
        assert!(target::rivals(&ctx).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
