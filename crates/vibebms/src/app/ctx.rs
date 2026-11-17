//! The state every scene shares, and the background work it polls.

use crate::config::{Config, TouchMode};
use crate::library::{BestScore, Library, ScanProgress, Song};
use crate::paths::Paths;
use crate::platform::{Instant, Job};
use bmseeker::table::DifficultyTable;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(test)]
use vibebms_audio::AudioConfig;
use vibebms_audio::Mixer;

pub enum TableMsg {
    Loaded(Box<DifficultyTable>),
    Failed(String, String),
}

/// Shared state available to every scene.
pub struct Ctx {
    pub paths: Paths,
    pub config: Config,
    /// The library and the player's records.
    pub lib: Lib,
    /// Difficulty tables and the other links known for their songs.
    pub tables: Tables,
    /// Downloads and the other network work the player starts.
    pub net: Net,
    pub audio: Audio,
    pub toasts: Vec<(String, Instant)>,
    /// The window and the screen.
    pub view: View,
    pub touch_seen: bool,
    /// A text field is being edited (scenes set it as they draw): the
    /// on-screen keyboard is shown on iOS and Android.
    pub text_input: bool,
    /// The skin catalog and the skins downloaded from it.
    pub skin_store: crate::skinstore::SkinStore,
    /// Custom options read from skin files (path -> options).
    skin_options_cache:
        std::cell::RefCell<HashMap<std::path::PathBuf, Vec<crate::skin::CustomOption>>>,
    skin_offsets_cache:
        std::cell::RefCell<HashMap<std::path::PathBuf, Vec<crate::skin::CustomOffset>>>,
}

/// The library: its database, the charts as last read, the records, and
/// the scan.
#[derive(Default)]
pub struct Lib {
    pub db: Option<Library>,
    /// Sorted by title, then level.
    pub songs: Arc<Vec<Song>>,
    pub scores: HashMap<(String, i32), BestScore>,
    /// Counts changes to what the music select lists (the songs, tables,
    /// events).
    pub version: u64,
    pub scan: Option<Arc<ScanProgress>>,
    /// Scan again once the running scan ends (files came in during it).
    scan_again: bool,
    /// Web: picked charts being read before the scan.
    pub prefetch: Option<crate::platform::vfs::Prefetch>,
}

/// The difficulty tables read, and the links that fill theirs.
#[derive(Default)]
pub struct Tables {
    /// In the configuration's order.
    pub list: Vec<DifficultyTable>,
    /// Tables being fetched.
    pub job: Option<Job<TableMsg>>,
    /// Links for table songs whose own are missing or dead.
    pub links: crate::links::LinkLists,
}

/// Downloads, and the self-update.
#[derive(Default)]
pub struct Net {
    /// Mirrored event entries (`archive`).
    pub archives: crate::archive::Archives,
    /// Difficulty-table songs downloading.
    pub song_fetch: crate::songfetch::SongFetch,
    /// The desktop executable's self-update.
    pub updater: crate::update::Updater,
    /// Download pages to show in the in-app browser (song, then chart).
    pub browse: Option<Vec<String>>,
}

/// The mixer, and the sound set's sounds played on it.
pub struct Audio {
    pub mixer: Mixer,
    pub sounds: crate::sysound::SystemSounds,
}

/// The window and the screen it is on.
pub struct View {
    /// The window's size in device pixels.
    pub width: f32,
    pub height: f32,
    /// Device pixels per logical pixel (point).
    pub scale: f32,
    pub screen: crate::platform::screen::Screen,
    pub fps: f32,
    pub gpu_name: String,
    /// Asked of the window by a scene (taken by the event loop).
    pub want_fullscreen: Option<bool>,
    pub want_present_mode: bool,
}

impl Ctx {
    /// A context with nothing loaded yet.
    pub fn new(paths: Paths, config: Config, mixer: Mixer, view: View) -> Ctx {
        Ctx {
            paths,
            config,
            lib: Lib::default(),
            tables: Tables::default(),
            net: Net::default(),
            audio: Audio {
                mixer,
                sounds: Default::default(),
            },
            toasts: Vec::new(),
            view,
            touch_seen: false,
            text_input: false,
            skin_store: Default::default(),
            skin_options_cache: Default::default(),
            skin_offsets_cache: Default::default(),
        }
    }

    /// A context for tests: default settings, data under `root`, no
    /// library, silent audio.
    #[cfg(test)]
    pub fn for_test(root: PathBuf) -> Ctx {
        let mixer = Mixer::new(&AudioConfig {
            buffer_frames: 0,
            sample_rate: 44100,
            master: 0.0,
            key_volume: 0.0,
            bgm_volume: 0.0,
            system_volume: 0.0,
            max_voices: 16,
        });
        let view = View {
            width: 1280.0,
            height: 720.0,
            scale: 1.0,
            screen: crate::platform::screen::Screen {
                size_px: (1280.0, 720.0),
                points_per_inch: 96.0,
            },
            fps: 0.0,
            gpu_name: String::new(),
            want_fullscreen: None,
            want_present_mode: false,
        };
        Ctx::new(Paths::at(root), Config::default(), mixer, view)
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        let s = s.into();
        log::info!("{s}");
        self.toasts.push((s, Instant::now()));
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    pub fn save_config(&self) {
        self.config.save(&self.paths.config_file());
    }

    /// UI scale relative to a 720p layout.
    pub fn ui(&self) -> f32 {
        let base = if self.portrait() {
            self.view.width / 720.0
        } else {
            self.view.width.min(self.view.height * 16.0 / 9.0) / 1280.0
        };
        base.max(0.45) * self.config.display.ui_scale.clamp(0.5, 3.0)
    }

    /// Device pixels per millimetre (for controls with a physical size).
    pub fn px_per_mm(&self) -> f32 {
        self.view
            .screen
            .px_per_mm(self.view.scale, self.config.touch.screen_inches)
    }

    pub fn portrait(&self) -> bool {
        self.view.height > self.view.width
    }

    pub fn touch_enabled(&self) -> bool {
        match self.config.touch.mode {
            TouchMode::On => true,
            TouchMode::Off => false,
            TouchMode::Auto => cfg!(mobile) || self.touch_seen,
        }
    }

    /// Make sure `dir` (under the root) is scanned.
    pub fn ensure_scanned(&mut self, dir: &str) {
        let p = self.paths.root.join(dir);
        if !self.scan_roots().iter().any(|r| p.starts_with(r)) {
            self.config.bms_dirs.push(dir.to_string());
            self.save_config();
        }
    }

    pub fn scan_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if self.config.scan_root {
            roots.push(self.paths.root.clone());
        }
        for d in &self.config.bms_dirs {
            let p = PathBuf::from(d);
            let p = if p.is_absolute() {
                p
            } else {
                self.paths.root.join(p)
            };
            if !roots.iter().any(|r| p.starts_with(r)) {
                roots.push(p);
            }
        }
        roots
    }

    /// A scan that has ended: its songs into the list (always before another
    /// starts, or the songs it found would be lost: the next one finds no
    /// change). `again`: start the scan asked for meanwhile.
    fn finish_scan(&mut self, again: bool) {
        let Some(s) = &self.lib.scan else { return };
        if !s.done.load(Ordering::Acquire) {
            return;
        }
        let changed = s.changed.load(Ordering::Acquire);
        self.lib.scan = None;
        if changed || self.lib.songs.is_empty() {
            self.reload_library();
        }
        let n = self.lib.songs.len();
        self.toast(format!("Library: {n} charts"));
        if !again {
            return;
        }
        if std::mem::take(&mut self.lib.scan_again) {
            self.start_scan();
        } else {
            // what was downloaded before it started is in now
            self.net.song_fetch.scanned();
        }
    }

    pub fn start_scan(&mut self) {
        self.finish_scan(false);
        self.lib.scan_again = false;
        if self
            .lib
            .scan
            .as_ref()
            .map(|s| !s.done.load(Ordering::Acquire))
            .unwrap_or(false)
        {
            // it may have passed what just came in: again after it
            self.lib.scan_again = true;
            return;
        }
        let progress = Arc::new(ScanProgress::default());
        self.lib.scan = Some(progress.clone());
        let db = self.paths.songs_cache();
        let roots = self.scan_roots();
        let skip: HashSet<PathBuf> = crate::paths::INTERNAL_DIRS
            .iter()
            .map(|d| self.paths.root.join(d))
            .collect();
        crate::platform::spawn("scan", move || {
            crate::library::scan(db, roots, skip, progress)
        });
    }

    pub fn reload_library(&mut self) {
        if let Some(lib) = &mut self.lib.db {
            let mut songs = lib.songs();
            lib.merge_fixed_ln(&songs);
            songs.sort_by(|a, b| {
                a.title
                    .to_lowercase()
                    .cmp(&b.title.to_lowercase())
                    .then(a.level_num().cmp(&b.level_num()))
            });
            self.lib.songs = Arc::new(songs);
            self.net.song_fetch.set_library(self.lib.songs.clone());
            self.lib.scores = lib.scores();
        }
        self.lib.version += 1;
    }

    fn table_cache_file(&self, url: &str) -> PathBuf {
        let h = vibebms_core::bms::md5_hex(url.as_bytes());
        self.paths.tables.join(format!("{h}.json"))
    }

    /// The tables' missing or dead links filled from the other links known
    /// for their charts (the other tables', the Stella uploader's).
    pub fn fill_links(&mut self) {
        self.tables.links.rebuild(&self.tables.list);
        for t in &mut self.tables.list {
            self.tables.links.known.fill(&mut t.entries);
        }
        self.lib.version += 1;
    }

    pub fn load_cached_tables(&mut self) {
        self.tables.list.clear();
        for t in self.config.tables.iter().filter(|t| t.enabled) {
            let f = self.table_cache_file(&t.url);
            if let Some(dt) = crate::platform::vfs::read(&f)
                .ok()
                .and_then(|b| serde_json::from_slice::<DifficultyTable>(&b).ok())
            {
                self.tables.list.push(dt);
            }
        }
        self.fill_links();
    }

    /// Fetch tables in the background. `force` refetches even fresh caches.
    pub fn refresh_tables(&mut self, force: bool) {
        if self.tables.job.is_some() {
            return;
        }
        let now = crate::platform::unix_now();
        let todo: Vec<(String, String)> = self
            .config
            .tables
            .iter()
            .filter(|t| t.enabled)
            .filter(|t| {
                force
                    || !self.tables.list.iter().any(|dt| {
                        dt.source_url == t.url && now.saturating_sub(dt.fetched_at) < 86_400
                    })
            })
            .map(|t| (t.name.clone(), t.url.clone()))
            .collect();
        if todo.is_empty() {
            return;
        }
        self.tables.job = Some(Job::start(|tx, _| {
            for (name, url) in todo {
                let tx = tx.clone();
                crate::platform::load_table(&url, move |r| {
                    let _ = tx.send(match r {
                        Ok(t) => TableMsg::Loaded(Box::new(t)),
                        Err(e) => TableMsg::Failed(name, e),
                    });
                });
            }
        }));
    }

    pub fn poll_jobs(&mut self) {
        let (toasts, mut rescan, changed, mut pages) = self.net.archives.poll(&self.paths);
        for t in toasts {
            self.toast(t);
        }
        // difficulty-table songs
        for (title, done) in self.net.song_fetch.poll() {
            use crate::songfetch::Done;
            for d in done {
                let name = |p: &std::path::Path| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                };
                match d {
                    Done::Added(dir, n) => {
                        self.toast(format!("Added {} ({n} files)", name(&dir)));
                        rescan = true;
                    }
                    // (files its folder lacked may have come in)
                    Done::Owned(dir) => {
                        self.toast(format!("{title}: already installed ({})", name(&dir)));
                        rescan = true;
                    }
                    Done::Placed(p) => {
                        let dir = p.parent().map(name).unwrap_or_default();
                        self.toast(format!("{} → {dir}", name(&p)));
                        rescan = true;
                    }
                    Done::Waiting(p) => self.toast(format!("{}: download its song too", name(&p))),
                    Done::Page(url) => {
                        self.toast(format!("{title}: no direct download, opening its page"));
                        pages.push(url);
                    }
                    // (cancelled: said when it was)
                    Done::Failed(e) if e.cancelled() => {}
                    Done::Failed(crate::songfetch::Failure::NoChartLink) => self.toast(format!(
                        "{title}: the song is in the library, but none of this chart's links works"
                    )),
                    Done::Failed(e) => self.toast(format!("{title}: download failed ({e})")),
                    other => log::warn!("{title}: {other:?}"),
                }
            }
        }
        // the self-update
        for t in self.net.updater.poll() {
            self.toast(t);
        }
        // skins downloaded
        let skins = self.skin_store.poll(&self.paths);
        for t in skins.toasts {
            self.toast(t);
        }
        // (not the in-app browser: what it downloads goes to the library)
        for p in &skins.pages {
            crate::platform::open_url(p);
        }
        if skins.changed {
            self.skins_changed();
        }
        // a work's link that needs a person on its page
        if !pages.is_empty() {
            if super::IN_APP_BROWSER {
                self.net.browse = Some(pages);
            } else {
                for p in &pages {
                    crate::platform::open_url(p);
                }
            }
        }
        if changed {
            self.lib.version += 1;
        }
        if rescan {
            #[cfg(not(target_arch = "wasm32"))]
            self.ensure_scanned("bms");
            self.start_scan();
        }
        #[cfg(target_arch = "wasm32")]
        for m in crate::platform::opfs::take_notes() {
            self.toast(m);
        }
        // web: newly picked files -> read the chart files, then scan
        if crate::platform::vfs::take_added() {
            let skip: HashSet<PathBuf> = crate::paths::INTERNAL_DIRS
                .iter()
                .map(|d| self.paths.root.join(d))
                .collect();
            let charts = crate::library::find_charts(&self.scan_roots(), &skip);
            self.toast(format!("Reading {} charts…", charts.len()));
            self.lib.prefetch = Some(crate::platform::vfs::Prefetch::files(charts));
        }
        if self
            .lib
            .prefetch
            .as_ref()
            .map(|p| p.finished())
            .unwrap_or(false)
        {
            self.lib.prefetch = None;
            // charts picked without their song's folder go into it
            #[cfg(target_arch = "wasm32")]
            for m in crate::web::place_picked_charts(&self.paths.root, &self.lib.songs) {
                self.toast(m);
            }
            self.start_scan();
        }
        self.finish_scan(true);
        if self.tables.links.poll() {
            self.fill_links();
        }
        let mut loaded = false;
        for m in Job::drain_slot(&mut self.tables.job) {
            match m {
                TableMsg::Loaded(t) => {
                    let t = *t;
                    loaded = true;
                    if let Ok(j) = serde_json::to_vec(&t) {
                        let _ =
                            crate::platform::vfs::write(&self.table_cache_file(&t.source_url), &j);
                    }
                    // (cached as the table has them; filled below)
                    self.toast(format!(
                        "Table updated: {} ({} charts)",
                        t.header.name,
                        t.entries.len()
                    ));
                    if let Some(old) = self
                        .tables
                        .list
                        .iter_mut()
                        .find(|o| o.source_url == t.source_url)
                    {
                        *old = t;
                    } else {
                        self.tables.list.push(t);
                    }
                    // keep config order
                    let order: Vec<String> =
                        self.config.tables.iter().map(|t| t.url.clone()).collect();
                    self.tables.list.sort_by_key(|t| {
                        order
                            .iter()
                            .position(|u| *u == t.source_url)
                            .unwrap_or(usize::MAX)
                    });
                    self.lib.version += 1;
                }
                TableMsg::Failed(n, e) => self.toast(format!("Table {n}: {e}")),
            }
        }
        if loaded {
            self.fill_links();
        }
    }

    /// Play a system sound of the chosen sound set.
    pub fn play_sound(&mut self, s: crate::sysound::Sound) {
        let v = self
            .config
            .skins
            .get("SOUND_SET")
            .cloned()
            .unwrap_or_default();
        self.audio
            .sounds
            .sync(&v, &self.paths.root, self.audio.mixer.sample_rate);
        self.audio.sounds.play(&self.audio.mixer, s);
    }

    pub fn stop_sound(&mut self, s: crate::sysound::Sound) {
        self.audio.sounds.stop(&self.audio.mixer, s);
    }

    /// Skins were added, replaced or deleted: what was read of them goes.
    pub fn skins_changed(&mut self) {
        self.skin_options_cache.borrow_mut().clear();
        self.skin_offsets_cache.borrow_mut().clear();
    }

    /// Custom options of a skin file (cached).
    pub fn skin_custom_options(&self, path: &std::path::Path) -> Vec<crate::skin::CustomOption> {
        self.skin_options_cache
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| crate::skin::custom_options(path))
            .clone()
    }

    pub fn skin_custom_offsets(&self, path: &std::path::Path) -> Vec<crate::skin::CustomOffset> {
        self.skin_offsets_cache
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| crate::skin::custom_offsets(path))
            .clone()
    }

    /// The user's offset values for the skin configured as `value`.
    pub fn skin_user_offsets(&self, value: &str) -> HashMap<String, [i32; 6]> {
        self.config
            .skin_offsets
            .get(value)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect())
            .unwrap_or_default()
    }

    /// What the user chose for the skin configured as `value` (a
    /// `config.skins` value) at `path`.
    pub fn skin_user_settings(
        &self,
        value: &str,
        path: &std::path::Path,
    ) -> crate::skin::UserSettings {
        let opts = self.skin_custom_options(path);
        let chosen = self.config.skin_options.get(value);
        crate::skin::UserSettings {
            options: crate::skin::user_options(&opts, chosen),
            files: crate::skin::user_files(&opts, chosen),
            offsets: self.skin_user_offsets(value),
        }
    }

    /// The chart's record with the player's LN mode `pref` (filed under
    /// the chart's own when its long notes all have their type).
    pub fn best(&self, song: &Song, pref: vibebms_core::LnType) -> Option<&BestScore> {
        self.best_key(&song.sha256, song.score_ln(pref) as i32)
    }

    /// The record of `sha256` under the LN mode `ln` (as filed).
    pub fn best_key(&self, sha256: &str, ln: i32) -> Option<&BestScore> {
        self.lib.scores.get(&(sha256, ln) as &dyn ScoreKey)
    }
}

/// A key of `Ctx::scores`, so that skins (which ask for the record per
/// value, many times a frame) look one up without making a `String`.
trait ScoreKey {
    fn key(&self) -> (&str, i32);
}

impl ScoreKey for (String, i32) {
    fn key(&self) -> (&str, i32) {
        (&self.0, self.1)
    }
}

impl ScoreKey for (&str, i32) {
    fn key(&self) -> (&str, i32) {
        *self
    }
}

impl<'a> std::borrow::Borrow<dyn ScoreKey + 'a> for (String, i32) {
    fn borrow(&self) -> &(dyn ScoreKey + 'a) {
        self
    }
}

// (hashes as `(String, i32)` does)
impl std::hash::Hash for dyn ScoreKey + '_ {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.key().hash(h)
    }
}

impl PartialEq for dyn ScoreKey + '_ {
    fn eq(&self, o: &Self) -> bool {
        self.key() == o.key()
    }
}

impl Eq for dyn ScoreKey + '_ {}
