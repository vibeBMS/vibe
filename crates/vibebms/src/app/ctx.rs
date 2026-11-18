//! The state every scene shares, and the background work it polls.

use super::tables::Tables;
use crate::config::{Config, TouchMode};
use crate::library::{BestScore, Library, RecordKey, ScanProgress, Song};
use crate::paths::Paths;
use crate::platform::Instant;
use crate::skin::{CustomOffset, CustomOption};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(test)]
use vibebms_audio::AudioConfig;
use vibebms_audio::Mixer;
use vibebms_core::judge::JudgePreset;

/// What a background subsystem's `poll` reports, for the app to act on.
#[derive(Debug, PartialEq)]
pub enum AppEvent {
    Toast(String),
    /// Files came into the library: scan it again.
    Rescan,
    /// What the music select lists changed (an event's works, a table).
    LibraryChanged,
    /// A download page that needs a person: in the in-app browser where
    /// there is one (what it downloads goes to the library).
    Browse(String),
    /// A page for the system's browser.
    OpenUrl(String),
    /// Skins were added or replaced.
    SkinsChanged,
    /// An IR account was checked (who it is, whether its key holds): the
    /// configuration keeps it. (The outbox's progress is read as it is
    /// shown.)
    IrChanged,
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
    /// The window and the screen.
    pub view: View,
    pub input: Input,
    /// Messages shown for a few seconds, and when each came.
    pub toasts: Vec<(String, Instant)>,
    /// The skin catalog, and what was read of skin files.
    pub skins: Skins,
}

/// The library: its database, the charts as last read, the records, and
/// the scan.
#[derive(Default)]
pub struct Lib {
    pub db: Option<Library>,
    /// The player's courses.
    pub courses: crate::courses::CourseStore,
    /// Sorted by title, then level.
    pub songs: Arc<Vec<Song>>,
    pub scores: HashMap<RecordKey, BestScore>,
    /// Counts changes to what the music select lists (the songs, tables,
    /// events).
    pub version: u64,
    pub scan: Option<Arc<ScanProgress>>,
    /// Scan again once the running scan ends (files came in during it).
    scan_again: bool,
    /// The charts a scan could not read are being read in for one more
    /// scan (once: if that one cannot read them either, they wait for the
    /// next scan).
    reading_unread: bool,
    /// Web: picked charts being read before the scan.
    pub prefetch: Option<crate::platform::vfs::Prefetch>,
}

/// Downloads, the self-update and the internet ranking.
pub struct Net {
    /// How links resolve and songs install (bmseeker's settings with the
    /// player's), made again when those settings change.
    pub hosts: crate::hosts::Context,
    /// Mirrored event entries (`archive`).
    pub archives: crate::archive::Archives,
    /// Difficulty-table songs downloading.
    pub song_fetch: crate::songfetch::SongFetch,
    /// The desktop executable's self-update.
    pub updater: crate::update::Updater,
    /// Download pages to show in the in-app browser (song, then chart).
    pub browse: Option<Vec<String>>,
    pub ir: crate::ir::IrService,
}

/// The mixer, and the sound set's sounds played on it.
pub struct Audio {
    pub mixer: Mixer,
    pub sounds: crate::sysound::SystemSounds,
    /// The charts' loudness measured so far.
    pub loudness: crate::loudness::LoudnessCache,
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
    /// A screenshot of the next frame is wanted (`screenshot`).
    pub want_screenshot: bool,
}

/// What the input devices have told.
#[derive(Default)]
pub struct Input {
    /// A touch screen has been touched.
    pub touch_seen: bool,
    /// A text field is being edited (scenes set it as they draw): the
    /// on-screen keyboard is shown on iOS and Android.
    pub text_input: bool,
    /// Where the mouse is (screen pixels); `None` before it moves and once
    /// it has left the window.
    pub pointer: Option<(f32, f32)>,
}

/// The skin catalog and its downloads, and the custom options and offsets
/// read from skin files (by path; read again once skins change).
#[derive(Default)]
pub struct Skins {
    pub store: crate::skinstore::SkinStore,
    options: RefCell<HashMap<PathBuf, Rc<[CustomOption]>>>,
    offsets: RefCell<HashMap<PathBuf, Rc<[CustomOffset]>>>,
}

impl Skins {
    /// Custom options of a skin file.
    pub fn custom_options(&self, path: &Path) -> Rc<[CustomOption]> {
        self.options
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| crate::skin::custom_options(path).into())
            .clone()
    }

    pub fn custom_offsets(&self, path: &Path) -> Rc<[CustomOffset]> {
        self.offsets
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| crate::skin::custom_offsets(path).into())
            .clone()
    }

    /// Skins were added, replaced or deleted: what was read of them goes.
    pub fn files_changed(&mut self) {
        self.options.get_mut().clear();
        self.offsets.get_mut().clear();
    }
}

impl Ctx {
    /// A context with nothing loaded yet.
    pub fn new(paths: Paths, mut config: Config, mixer: Mixer, view: View) -> Ctx {
        let mut net = Net {
            hosts: crate::hosts::context(&config),
            archives: Default::default(),
            song_fetch: Default::default(),
            updater: Default::default(),
            browse: None,
            ir: crate::ir::IrService::new(&paths, crate::platform::unix_ms()),
        };
        if net.ir.take_old_tokens(&mut config.ir) {
            config.save(&paths.config_file());
        }
        let loudness = crate::loudness::LoudnessCache::new(&paths.cache);
        Ctx {
            paths,
            config,
            lib: Lib::default(),
            tables: Tables::default(),
            net,
            audio: Audio {
                mixer,
                sounds: Default::default(),
                loudness,
            },
            view,
            input: Input::default(),
            toasts: Vec::new(),
            skins: Skins::default(),
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
            #[cfg(windows)]
            exclusive: false,
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
            want_screenshot: false,
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
            TouchMode::Auto => cfg!(mobile) || self.input.touch_seen,
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

    /// The app's own folders, which the scan skips.
    fn internal_dirs(&self) -> HashSet<PathBuf> {
        crate::paths::INTERNAL_DIRS
            .iter()
            .map(|d| self.paths.root.join(d))
            .collect()
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
        let unread = std::mem::take(&mut *s.unread.lock().unwrap());
        self.lib.scan = None;
        if changed || self.lib.songs.is_empty() {
            self.reload_library();
        }
        self.read_unread(unread);
        // (with charts being read in, another scan follows: the count then)
        if self.lib.prefetch.is_none() {
            let n = self.lib.songs.len();
            self.toast(format!("Library: {n} charts"));
        }
        if !again {
            return;
        }
        if std::mem::take(&mut self.lib.scan_again) {
            self.start_scan();
        } else {
            // what was downloaded before it started is in now
            self.net.song_fetch.scanned();
            if self.lib.prefetch.is_none() {
                // web: the charts kept in memory for this scan leave it
                crate::platform::vfs::release_charts();
            }
        }
    }

    /// Charts the scan could not read (web: a chart whose bytes left
    /// memory for the browser's stored copy after it was read in, before
    /// the scan): read them in, then scan again (`poll_scan`). Only once
    /// in a row, so a chart that cannot be read at all does not keep the
    /// library scanning; only on the web, where reading in is what helps.
    fn read_unread(&mut self, unread: Vec<std::path::PathBuf>) {
        // natively a file the scan could not read stays unreadable until it
        // changes (and then the next scan reads it): nothing to read in
        if unread.is_empty() || !cfg!(target_arch = "wasm32") {
            if !unread.is_empty() {
                log::warn!("scan: {} charts could not be read", unread.len());
            }
            self.lib.reading_unread = false;
            return;
        }
        // (a read already under way scans again when it is done)
        if self.lib.prefetch.is_some() {
            return;
        }
        if std::mem::take(&mut self.lib.reading_unread) {
            log::warn!("scan: {} charts could not be read", unread.len());
            return;
        }
        log::info!("scan: {} charts not read yet: reading them", unread.len());
        self.lib.reading_unread = true;
        #[cfg(target_arch = "wasm32")]
        crate::platform::opfs::hold_library(true);
        self.lib.prefetch = Some(crate::platform::vfs::Prefetch::files(unread));
    }

    pub fn start_scan(&mut self) {
        self.finish_scan(false);
        self.lib.scan_again = false;
        if self
            .lib
            .scan
            .as_ref()
            .is_some_and(|s| !s.done.load(Ordering::Acquire))
        {
            // it may have passed what just came in: again after it
            self.lib.scan_again = true;
            return;
        }
        let progress = Arc::new(ScanProgress::default());
        self.lib.scan = Some(progress.clone());
        let db = self.paths.songs_cache();
        let roots = self.scan_roots();
        let skip = self.internal_dirs();
        crate::platform::spawn("scan", move || {
            crate::library::scan(db, roots, skip, progress)
        });
    }

    pub fn reload_library(&mut self) {
        if let Some(lib) = &mut self.lib.db {
            let mut songs = lib.songs();
            lib.merge_fixed_ln(&songs);
            songs.sort_by_cached_key(|s| (s.title.to_lowercase(), s.level_num()));
            self.lib.songs = Arc::new(songs);
            self.lib.scores = lib.scores();
        }
        self.lib.version += 1;
    }

    /// The enabled tables from their caches.
    pub fn load_cached_tables(&mut self) {
        self.tables.load_cached(&self.paths, &self.config.tables);
        self.lib.version += 1;
    }

    /// What the background work did since the last frame.
    pub fn poll_jobs(&mut self) {
        let mut events = self.net.archives.poll(&self.paths);
        events.extend(self.net.song_fetch.poll());
        events.extend(self.net.updater.poll().into_iter().map(AppEvent::Toast));
        events.extend(self.skins.store.poll(&self.paths));
        let now = crate::platform::unix_ms();
        events.extend(self.net.ir.poll(now, &mut self.config.ir));
        self.handle(events);
        self.poll_scan();
        let events = self.tables.poll(&self.paths, &self.config.tables);
        self.handle(events);
    }

    fn handle(&mut self, events: Vec<AppEvent>) {
        let (mut rescan, mut pages) = (false, Vec::new());
        for e in events {
            match e {
                AppEvent::Toast(t) => self.toast(t),
                AppEvent::Rescan => rescan = true,
                AppEvent::LibraryChanged => self.lib.version += 1,
                AppEvent::Browse(url) => pages.push(url),
                AppEvent::OpenUrl(url) => {
                    crate::platform::open_url(&url);
                }
                AppEvent::SkinsChanged => self.skins.files_changed(),
                AppEvent::IrChanged => self.save_config(),
            }
        }
        if !pages.is_empty() {
            if super::IN_APP_BROWSER {
                self.net.browse = Some(pages);
            } else {
                for p in &pages {
                    crate::platform::open_url(p);
                }
            }
        }
        if rescan {
            #[cfg(not(target_arch = "wasm32"))]
            self.ensure_scanned("bms");
            self.start_scan();
        }
    }

    /// The scan, and on the web the files picked or kept in the browser.
    fn poll_scan(&mut self) {
        #[cfg(target_arch = "wasm32")]
        for m in crate::platform::opfs::take_notes() {
            self.toast(m);
        }
        // web: newly picked files -> read the chart files, then scan
        if crate::platform::vfs::take_added() {
            let charts = crate::library::find_charts(&self.scan_roots(), &self.internal_dirs());
            self.toast(format!("Reading {} charts…", charts.len()));
            #[cfg(target_arch = "wasm32")]
            crate::platform::opfs::hold_library(true);
            self.lib.prefetch = Some(crate::platform::vfs::Prefetch::files(charts));
        }
        if self.lib.prefetch.as_ref().is_some_and(|p| p.finished()) {
            self.lib.prefetch = None;
            #[cfg(target_arch = "wasm32")]
            crate::platform::opfs::hold_library(false);
            // charts picked without their song's folder go into it
            #[cfg(target_arch = "wasm32")]
            for m in crate::web::place_picked_charts(&self.paths.root, &self.lib.songs) {
                self.toast(m);
            }
            self.start_scan();
        }
        self.finish_scan(true);
    }

    /// Play a system sound of the chosen sound set (loaded when the choice
    /// has changed: a string compare otherwise).
    pub fn play_sound(&mut self, s: crate::sysound::Sound) {
        self.sync_sounds();
        let norm = crate::loudness::system_target(&self.config.audio);
        self.audio.sounds.play(&self.audio.mixer, s, norm);
    }

    pub fn stop_sound(&mut self, s: crate::sysound::Sound) {
        self.audio.sounds.stop(&self.audio.mixer, s);
    }

    /// The sound set chosen, loaded if it is not.
    fn sync_sounds(&mut self) {
        let set = self
            .config
            .skins
            .get("SOUND_SET")
            .map_or("", String::as_str);
        let rate = self.audio.mixer.sample_rate;
        self.audio.sounds.sync(set, &self.paths.root, rate);
    }

    /// The song select comes (back): a sound set drawn at random (or one of
    /// its variants) drawn again, and its `select.enter` sound played.
    pub fn select_entered(&mut self) {
        self.sync_sounds();
        let rate = self.audio.mixer.sample_rate;
        self.audio.sounds.redraw(&self.paths.root, rate);
        let norm = crate::loudness::system_target(&self.config.audio);
        let s = crate::sysound::Sound::Select;
        self.audio.sounds.play_enter(&self.audio.mixer, s, norm);
    }

    /// A result comes: its music by outcome (`sounds`, in the order they
    /// are looked for).
    pub fn play_result(&mut self, sounds: &[crate::sysound::Sound]) {
        self.sync_sounds();
        let norm = crate::loudness::system_target(&self.config.audio);
        self.audio
            .sounds
            .play_result(&self.audio.mixer, sounds, norm);
    }

    pub fn stop_result(&mut self) {
        self.audio.sounds.stop_result(&self.audio.mixer);
    }

    /// What the user chose for the skin configured as `value` (a
    /// `config.skins` value) at `path`.
    pub fn skin_user_settings(&self, value: &str, path: &Path) -> crate::skin::UserSettings {
        let opts = self.skins.custom_options(path);
        let chosen = self.config.skin_options.get(value);
        crate::skin::UserSettings {
            options: crate::skin::user_options(&opts, chosen),
            files: crate::skin::user_files(&opts, chosen),
            offsets: self
                .config
                .skin_offsets
                .get(value)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect())
                .unwrap_or_default(),
        }
    }

    /// The chart's record with the player's LN mode `pref` (filed under
    /// the chart's own when its long notes all have their type) under
    /// the player's rule mode.
    pub fn best(&self, song: &Song, pref: vibebms_core::LnType) -> Option<&BestScore> {
        let rule = self.config.play.judge_preset;
        self.best_key(&song.sha256, song.score_ln(pref) as i32, rule)
    }

    /// The record of `id` (a chart's SHA-256, a course's identity) under
    /// the LN mode `ln` (as filed) and rule mode `rule`.
    pub fn best_key(&self, id: &str, ln: i32, rule: JudgePreset) -> Option<&BestScore> {
        self.lib.scores.get(&(id, ln, rule) as &dyn ScoreKey)
    }
}

/// A key of `Lib::scores`, so that skins (which ask for the record per
/// value, many times a frame) look one up without making a `String`.
trait ScoreKey {
    fn key(&self) -> (&str, i32, JudgePreset);
}

impl ScoreKey for RecordKey {
    fn key(&self) -> (&str, i32, JudgePreset) {
        (&self.id, self.ln, self.rule)
    }
}

impl ScoreKey for (&str, i32, JudgePreset) {
    fn key(&self) -> (&str, i32, JudgePreset) {
        *self
    }
}

impl<'a> std::borrow::Borrow<dyn ScoreKey + 'a> for RecordKey {
    fn borrow(&self) -> &(dyn ScoreKey + 'a) {
        self
    }
}

// (hashes as `RecordKey` does: its fields in order)
impl std::hash::Hash for dyn ScoreKey + '_ {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let (id, ln, rule) = self.key();
        id.hash(h);
        ln.hash(h);
        rule.hash(h);
    }
}

impl PartialEq for dyn ScoreKey + '_ {
    fn eq(&self, o: &Self) -> bool {
        self.key() == o.key()
    }
}

impl Eq for dyn ScoreKey + '_ {}
