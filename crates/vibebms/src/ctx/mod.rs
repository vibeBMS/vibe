//! The state every scene shares, and the background work it polls.

mod event;
mod mute;
pub(crate) mod overrides;
pub(crate) mod tables;

pub(crate) use event::AppEvent;
pub use mute::Mute;
pub use overrides::Overrides;
use tables::Tables;

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
use vibebms_audio::Mixer;
use vibebms_core::judge::JudgePreset;
use vibebms_core::replay::Slot;

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
    /// This launch's command-line options, which the saved config leaves
    /// out.
    pub overrides: Overrides,
    /// The player profiles, the others' records as local rivals.
    pub profiles: crate::profiles::Profiles,
    /// Discord Rich Presence and OBS (desktop).
    pub streaming: crate::integrations::Integrations,
}

/// The library: its database, the charts as last read, the records, and
/// the scan.
#[derive(Default)]
pub struct Lib {
    pub db: Option<Library>,
    /// The player's courses.
    pub courses: crate::library::courses::CourseStore,
    /// Sorted by title, then level.
    pub songs: Arc<Vec<Song>>,
    /// How many charts `songs` are, a chart in several folders once.
    pub charts: usize,
    pub scores: HashMap<RecordKey, BestScore>,
    /// Counts changes to what the music select lists (the songs, tables,
    /// events).
    pub version: u64,
    pub scan: Option<Arc<ScanProgress>>,
    /// Scan again once the running scan ends (files came in during it).
    scan_again: bool,
    /// The next scan forgets the files left out ([`Ctx::rescan`]).
    forget: bool,
    /// The charts a scan could not read are being read in for one more
    /// scan (once: if that one cannot read them either, they wait for the
    /// next scan).
    reading_unread: bool,
    /// Web: picked charts being read before the scan.
    pub prefetch: Option<crate::platform::vfs::Prefetch>,
}

/// Songs are downloaded in an in-app browser (desktop, Android, iOS):
/// [`Net::browse`].
pub const IN_APP_BROWSER: bool = cfg!(in_app_browser);

/// Downloads, the self-update and the internet ranking.
pub struct Net {
    /// How links resolve and songs install (bmseeker's settings with the
    /// player's), made again when those settings change.
    pub hosts: crate::net::hosts::Context,
    /// Mirrored event entries (`net::events`).
    pub archives: crate::net::events::Archives,
    /// Difficulty-table songs downloading.
    pub song_fetch: crate::net::songfetch::SongFetch,
    /// A table's or a level's missing charts, downloaded in turn.
    pub bulk: crate::net::bulk::Bulk,
    /// The desktop executable's self-update.
    pub updater: crate::update::Updater,
    /// Download pages to show in the in-app browser (song, then chart).
    pages: Option<Vec<String>>,
    pub ir: crate::ir::IrService,
}

impl Net {
    /// Show `pages` in the in-app browser, one after another (song, then
    /// chart). They are links from tables and indexes: only web links are
    /// kept (`platform::is_web_url`), a web view loads any scheme.
    pub fn browse(&mut self, pages: Vec<String>) {
        let pages: Vec<String> = (pages.into_iter())
            .filter(|u| crate::platform::is_web_url(u))
            .collect();
        if !pages.is_empty() {
            self.pages = Some(pages);
        }
    }

    /// The pages asked for since the last call.
    pub fn take_pages(&mut self) -> Option<Vec<String>> {
        self.pages.take()
    }
}

/// The mixer, and the sound set's sounds played on it.
pub struct Audio {
    pub mixer: Mixer,
    pub sounds: crate::audio::sysound::SystemSounds,
    /// Silenced while the app is not the one in use (`app::activity`).
    pub mute: Mute,
    /// The charts' loudness measured so far.
    pub loudness: crate::audio::loudness::LoudnessCache,
}

impl Audio {
    /// The master volume `v`, heard once the app is in use again if it
    /// is silenced now.
    pub fn set_master(&self, v: f32) {
        if !self.mute.on() {
            self.mixer.set_master(v);
        }
    }

    /// Silence the output (it fades out), from `now`.
    pub fn silence(&mut self, now: Instant) {
        if self.mute.start(now) {
            self.mixer.set_master(0.0);
        }
    }

    /// Heard again, at master volume `master`.
    pub fn unsilence(&mut self, master: f32) {
        if self.mute.end() {
            self.mixer.set_master(master);
        }
    }
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
    /// A screenshot of the next frame is wanted (`app::screenshot`).
    pub want_screenshot: bool,
}

impl View {
    /// The window's `size` in device pixels, `scale` of them a point, on
    /// `screen`, drawn by the GPU `gpu_name`.
    pub fn new(
        size: winit::dpi::PhysicalSize<u32>,
        scale: f32,
        screen: crate::platform::screen::Screen,
        gpu_name: &str,
    ) -> View {
        View {
            width: size.width as f32,
            height: size.height as f32,
            scale,
            screen,
            fps: 0.0,
            gpu_name: gpu_name.to_string(),
            want_fullscreen: None,
            want_present_mode: false,
            want_screenshot: false,
        }
    }
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
    /// The game controllers connected, by number (0: 1P, 1: 2P).
    pub pads: Vec<crate::input::gamepad::Connected>,
}

/// The skin catalog and its downloads, and the custom options and offsets
/// read from skin files (by path; read again once skins change).
#[derive(Default)]
pub struct Skins {
    pub store: crate::net::skinstore::SkinStore,
    /// A skin chosen and not loaded yet, and when (`skinned::choose`).
    pub(crate) trial: Option<(crate::skin::SkinType, String, crate::platform::Instant)>,
    options: RefCell<HashMap<PathBuf, Rc<[CustomOption]>>>,
    offsets: RefCell<HashMap<PathBuf, Rc<[CustomOffset]>>>,
}

impl Skins {
    /// Custom options of a skin file (`skins_root`: the skins folder).
    pub fn custom_options(&self, path: &Path, skins_root: &Path) -> Rc<[CustomOption]> {
        self.options
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| crate::skin::custom_options(path, skins_root).into())
            .clone()
    }

    pub fn custom_offsets(&self, path: &Path, skins_root: &Path) -> Rc<[CustomOffset]> {
        self.offsets
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| crate::skin::custom_offsets(path, skins_root).into())
            .clone()
    }

    /// Skins were added, replaced or deleted: what was read of them goes.
    pub fn files_changed(&mut self) {
        self.options.get_mut().clear();
        self.offsets.get_mut().clear();
    }
}

/// The folders the scan reads: the root (when chosen), then the BMS
/// folders not inside one already.
pub fn scan_roots(config: &Config, paths: &Paths) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if config.scan_root {
        roots.push(paths.root.clone());
    }
    for d in &config.bms_dirs {
        let p = PathBuf::from(d);
        let p = if p.is_absolute() {
            p
        } else {
            paths.root.join(p)
        };
        if !roots.iter().any(|r| p.starts_with(r)) {
            roots.push(p);
        }
    }
    roots
}

/// The app's own folders, which the scan skips.
pub fn internal_dirs(paths: &Paths) -> HashSet<PathBuf> {
    crate::paths::INTERNAL_DIRS
        .iter()
        .map(|d| paths.root.join(d))
        .collect()
}

impl Ctx {
    /// A context with nothing loaded yet: the settings of the profile in
    /// use, with this launch's options in them (`overrides`).
    pub fn new(
        paths: Paths,
        profiles: crate::profiles::Profiles,
        config: Config,
        overrides: Overrides,
        mixer: Mixer,
        view: View,
    ) -> Ctx {
        let net = Net {
            hosts: crate::net::hosts::context(&config),
            archives: Default::default(),
            song_fetch: Default::default(),
            bulk: Default::default(),
            updater: Default::default(),
            pages: None,
            ir: crate::ir::IrService::new(&paths, crate::platform::unix_ms()),
        };
        let loudness = crate::audio::loudness::LoudnessCache::new(&paths.cache);
        let streaming = crate::integrations::Integrations::new(&paths);
        let mut ctx = Ctx {
            paths,
            config,
            lib: Lib::default(),
            tables: Tables::default(),
            net,
            audio: Audio {
                mixer,
                sounds: Default::default(),
                mute: Default::default(),
                loudness,
            },
            view,
            input: Input::default(),
            toasts: Vec::new(),
            skins: Skins::default(),
            overrides,
            profiles,
            streaming,
        };
        ctx.take_old_tokens();
        ctx
    }

    /// The IR keys and OBS's password a configuration of before kept,
    /// moved out of it (and the configuration saved without them).
    fn take_old_tokens(&mut self) {
        let moved = self.net.ir.take_old_tokens(&mut self.config.ir);
        #[cfg(desktop)]
        let moved = self.streaming.take_old_password(&mut self.config.obs) | moved;
        if moved {
            self.save_config();
        }
    }

    /// A context for tests: default settings, data under `root`, no
    /// library, silent audio.
    #[cfg(test)]
    pub fn for_test(root: PathBuf) -> Ctx {
        let silent = crate::config::AudioSettings {
            buffer_frames: 0,
            sample_rate: 44100,
            master: 0.0,
            key: 0.0,
            bgm: 0.0,
            system: 0.0,
            ..Default::default()
        };
        let mixer = Mixer::new(&silent.mixer_config(16));
        let view = View::new(
            winit::dpi::PhysicalSize::new(1280, 720),
            1.0,
            crate::platform::screen::Screen {
                size_px: (1280.0, 720.0),
                points_per_inch: 96.0,
            },
            "",
        );
        let paths = Paths::at(root);
        let profiles = crate::profiles::Profiles::open(&paths);
        Ctx::new(
            paths,
            profiles,
            Config::default(),
            Default::default(),
            mixer,
            view,
        )
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        let s = s.into();
        log::info!("{s}");
        self.toasts.push((s, Instant::now()));
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    /// The config saved, without this launch's command-line options.
    pub fn save_config(&self) {
        crate::profiles::save_config(&self.saved_config(), &self.paths);
    }

    /// The config as it is saved: without this launch's command-line
    /// options.
    pub fn saved_config(&self) -> Config {
        (self.overrides.for_saving(&self.config)).unwrap_or_else(|| self.config.clone())
    }

    /// The profile's records (the library database's score files) and its
    /// courses; `lib.scores` follows with `reload_library` (or
    /// `reload_scores`).
    pub fn open_records(&mut self) {
        let player = self.paths.player.clone();
        self.lib.db = Some(Library::open(
            &self.paths.cache,
            &self.paths.scores,
            |sha, ln| crate::library::replay::rule_of_old(&player, sha, ln, Slot::Best),
        ));
        self.lib.courses = crate::library::courses::CourseStore::open(&self.paths.scores);
    }

    /// `lib.scores` from the database, the charts as they are.
    fn reload_scores(&mut self) {
        if let Some(db) = &mut self.lib.db {
            db.merge_fixed_ln(&self.lib.songs);
            self.lib.scores = db.scores();
        }
        self.lib.version += 1;
    }

    /// Use profile `id` from now on: the one in use saved, then the
    /// other's settings, records, IR accounts and outbox read (what depends
    /// on them, the song select, the app makes again: `Running::
    /// poll_profile`). False when there is no such profile.
    pub fn switch_profile(&mut self, id: &str) -> bool {
        if !self.profiles.registry.has(id) {
            self.toast(tr!("No such profile"));
            return false;
        }
        self.save_config();
        self.paths = self.paths.with_profile(id);
        self.profiles.set_active(&self.paths);
        self.config = crate::profiles::load_config(&self.paths);
        self.overrides = Default::default();
        self.net.ir = crate::ir::IrService::new(&self.paths, crate::platform::unix_ms());
        self.take_old_tokens();
        self.net.hosts = crate::net::hosts::context(&self.config);
        self.open_records();
        self.reload_scores();
        let a = &self.config.audio;
        self.audio.set_master(a.master);
        let m = &self.audio.mixer;
        m.set_bus_volume(vibebms_audio::Bus::Key, a.key);
        m.set_bus_volume(vibebms_audio::Bus::Bgm, a.bgm);
        m.set_bus_volume(vibebms_audio::Bus::System, a.system);
        self.skins.files_changed();
        // web: the replays read now (see `app::run`)
        #[cfg(target_arch = "wasm32")]
        let _ = crate::platform::vfs::Prefetch::dirs(&[self.paths.replays()]);
        let name = self.profiles.registry.name(id).to_string();
        log::info!("profile: {name} ({})", self.paths.player.display());
        self.toast(tr!("Profile: {name}", name = name));
        true
    }

    /// A new profile named `name`, its settings the ones in use but the
    /// player's name and the IR accounts; its id.
    pub fn create_profile(&mut self, name: &str) -> Result<String, String> {
        let from = self.saved_config();
        let r = self.profiles.registry.create(&self.paths, name, &from);
        self.profiles = crate::profiles::Profiles::open(&self.paths);
        r
    }

    pub fn rename_profile(&mut self, id: &str, name: &str) -> Result<(), String> {
        let r = self.profiles.registry.rename(&self.paths.root, id, name);
        self.profiles = crate::profiles::Profiles::open(&self.paths);
        r
    }

    /// Delete profile `id` (not the default, not the one in use).
    pub fn delete_profile(&mut self, id: &str) -> Result<(), String> {
        let r = self.profiles.registry.delete(&self.paths, id);
        self.profiles = crate::profiles::Profiles::open(&self.paths);
        r
    }

    /// Local rival `profile`'s record of `id` under LN mode `ln` and rule
    /// mode `rule` (`best_key`'s, of another profile).
    pub fn rival_best(
        &self,
        profile: &str,
        id: &str,
        ln: i32,
        rule: JudgePreset,
    ) -> Option<&BestScore> {
        let recs = self.profiles.rival(profile)?;
        recs.get(&(id, ln, rule) as &dyn ScoreKey)
    }

    /// Local rival `profile`'s record of `song` as the player's own is
    /// looked up ([`Ctx::best`]).
    pub fn rival_best_of(&self, profile: &str, song: &Song) -> Option<&BestScore> {
        let ln = song.score_ln(self.config.play.ln_pref()) as i32;
        self.rival_best(profile, &song.sha256, ln, self.config.play.judge_preset)
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
        if !scan_roots(&self.config, &self.paths)
            .iter()
            .any(|r| p.starts_with(r))
        {
            self.config.bms_dirs.push(dir.to_string());
            self.save_config();
        }
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
            let n = self.lib.charts;
            self.toast(tr!("Library: {n} charts", n = n));
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

    /// A rescan the player asked for: the files left out for stopping the
    /// game are read again too.
    pub fn rescan(&mut self) {
        self.lib.forget = true;
        self.start_scan();
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
        let progress = Arc::new(ScanProgress {
            forget: std::mem::take(&mut self.lib.forget),
            ..Default::default()
        });
        self.lib.scan = Some(progress.clone());
        let db = self.paths.songs_cache();
        let roots = scan_roots(&self.config, &self.paths);
        let skip = internal_dirs(&self.paths);
        crate::platform::spawn("scan", move || {
            crate::library::scan(db, roots, skip, progress)
        });
    }

    pub fn reload_library(&mut self) {
        if let Some(lib) = &mut self.lib.db {
            let mut songs = lib.songs();
            lib.merge_fixed_ln(&songs);
            songs.sort_by_cached_key(|s| (s.title.to_lowercase(), s.level_num()));
            self.lib.charts = crate::library::charts::Charts::new(&songs).count();
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

    /// What the downloads, the self-update and the IR did since the last
    /// frame.
    pub fn poll_jobs(&mut self) {
        let archives = self.net.archives.poll(&self.paths, &self.net.hosts);
        let mut events: Vec<AppEvent> = archives.into_iter().map(AppEvent::from).collect();
        events.extend(self.net.song_fetch.poll().into_iter().map(AppEvent::from));
        if crate::net::bulk::tick(
            &mut self.net.bulk,
            &mut self.net.song_fetch,
            &self.paths.root,
            &self.lib.songs,
            &self.net.hosts,
            &self.tables.links,
            Instant::now(),
        ) {
            self.lib.version += 1;
        }
        events.extend(self.net.updater.poll().into_iter().map(AppEvent::Toast));
        let skins = self.skins.store.poll(&self.paths);
        events.extend(skins.into_iter().map(AppEvent::from));
        let now = crate::platform::unix_ms();
        let ir = self.net.ir.poll(now, &mut self.config.ir);
        events.extend(ir.into_iter().map(AppEvent::from));
        self.handle(events);
    }

    /// What the scan and the tables did since the last frame.
    pub fn poll_library(&mut self) {
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
                AppEvent::IrScores(d) => self.take_ir_scores(&d),
            }
        }
        if !pages.is_empty() {
            if IN_APP_BROWSER {
                self.net.browse(pages);
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

    /// The player's own IR scores taken into the records, as a score
    /// database's are (better values only; the same again changes
    /// nothing); what came of it told.
    fn take_ir_scores(&mut self, d: &crate::ir::download::Download) {
        let f = crate::ir::download::records(&d.rows, &self.lib.songs);
        let Some(db) = self.lib.db.as_mut() else {
            return;
        };
        let (new, changed) = db.import(&f.records);
        self.lib.scores = db.scores();
        self.lib.version += 1;
        let mut said = tr!(
            "{title}: {n} scores read: {new} new, {changed} improved, {unknown} of charts not here",
            title = d.title,
            n = d.rows.len() + d.left_out,
            new = new,
            changed = changed,
            unknown = f.unknown
        );
        if f.ln_unknown > 0 {
            said += &tr!(", {n} left out (LN mode not said)", n = f.ln_unknown);
        }
        if d.left_out > 0 {
            said += &tr!(", {n} left out (no rule mode)", n = d.left_out);
        }
        for w in &d.why {
            said += &format!("; {w}");
        }
        self.toast(said);
    }

    /// The scan, and on the web the files picked or kept in the browser.
    fn poll_scan(&mut self) {
        #[cfg(target_arch = "wasm32")]
        for m in crate::platform::opfs::take_notes() {
            self.toast(m);
        }
        // web: newly picked files -> read the chart files, then scan
        if crate::platform::vfs::take_added() {
            let roots = scan_roots(&self.config, &self.paths);
            let charts = crate::library::find_charts(&roots, &internal_dirs(&self.paths));
            self.toast(tr!("Reading {n} charts…", n = charts.len()));
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
            for m in crate::app::web::place_picked_charts(&self.paths.root, &self.lib.songs) {
                self.toast(m);
            }
            self.start_scan();
        }
        self.finish_scan(true);
    }

    /// Play a system sound of the chosen sound set (loaded when the choice
    /// has changed: a string compare otherwise).
    pub fn play_sound(&mut self, s: crate::audio::sysound::Sound) {
        self.sync_sounds();
        let norm = crate::audio::loudness::system_target(&self.config.audio);
        self.audio.sounds.play(&self.audio.mixer, s, norm);
    }

    pub fn stop_sound(&mut self, s: crate::audio::sysound::Sound) {
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
        let norm = crate::audio::loudness::system_target(&self.config.audio);
        let s = crate::audio::sysound::Sound::Select;
        self.audio.sounds.play_enter(&self.audio.mixer, s, norm);
    }

    /// A result comes: its music by outcome (`sounds`, in the order they
    /// are looked for).
    pub fn play_result(&mut self, sounds: &[crate::audio::sysound::Sound]) {
        self.sync_sounds();
        let norm = crate::audio::loudness::system_target(&self.config.audio);
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
        let opts = self.skins.custom_options(path, &self.paths.skins);
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
    pub fn best(&self, song: &Song, pref: impl Into<vibebms_core::LnPref>) -> Option<&BestScore> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A download page comes from an index or a table: the in-app browser
    /// is given web links only.
    #[test]
    fn only_web_links_reach_the_in_app_browser() {
        let root = std::env::temp_dir().join(format!("vibebms-browse-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let bad = ["javascript:alert(1)", "file:///etc/passwd", "content://x/y"];
        ctx.handle(bad.map(|u| AppEvent::Browse(u.into())).into());
        assert_eq!(ctx.net.take_pages(), None);
        ctx.net.browse(bad.map(String::from).into());
        assert_eq!(ctx.net.take_pages(), None);
        let pages = [
            bad[0],
            "https://example.com/song",
            bad[1],
            "http://example.com/chart",
        ];
        ctx.net.browse(pages.map(String::from).into());
        assert_eq!(
            ctx.net.take_pages(),
            Some(vec![pages[1].to_string(), pages[3].to_string()])
        );
        assert_eq!(ctx.net.take_pages(), None);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A step of the bulk queue that changes what the music select lists
    /// counts as a change of them; one that changes nothing does not.
    #[test]
    fn a_bulk_step_that_changes_the_lists_counts() {
        let root = std::env::temp_dir().join(format!("vibebms-bulk-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let v = ctx.lib.version;
        ctx.poll_jobs();
        assert_eq!(ctx.lib.version, v);
        // a chart the library holds: done at once, nothing downloaded
        let md5 = format!("{:0>32}", "held");
        ctx.lib.songs = Arc::new(vec![Song {
            md5: md5.clone(),
            ..Default::default()
        }]);
        let held = crate::net::songfetch::Wanted {
            title: "held".into(),
            md5,
            ..Default::default()
        };
        assert_eq!(ctx.net.bulk.queue([held]), 1);
        ctx.poll_jobs();
        assert_eq!(ctx.lib.version, v + 1);
        ctx.poll_jobs();
        assert_eq!(ctx.lib.version, v + 1);
        std::fs::remove_dir_all(&root).ok();
    }
}
