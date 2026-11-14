//! Window, event loop, shared context and the scene stack.

use crate::config::{Config, TouchMode};
use crate::gfx::text::{Align, Text};
use crate::gfx::{rgba, with_alpha, Batch, Gfx, Rect, TexId};
use crate::input::{InputEv, Phase};
use crate::library::{BestScore, Library, ScanProgress, Song};
use crate::paths::Paths;
use crate::platform::{Duration, Instant};
use crate::theme;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use vibebms_audio::{AudioConfig, Mixer};
use vibebms_tables::DifficultyTable;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

pub enum Next {
    Stay,
    Push(Box<dyn Scene>),
    Replace(Box<dyn Scene>),
    Pop,
    Quit,
}

pub struct Draw<'a> {
    pub gfx: &'a Gfx,
    pub text: &'a mut Text,
    pub b: &'a mut Batch,
}

impl Draw<'_> {
    pub fn text(
        &mut self,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        color: [f32; 4],
        align: Align,
    ) -> f32 {
        self.text
            .draw(self.gfx, self.b, s, x, y, size, color, align)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn text_fit(
        &mut self,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        color: [f32; 4],
        align: Align,
        max_w: f32,
    ) -> f32 {
        self.text
            .draw_scaled(self.gfx, self.b, s, x, y, size, color, align, max_w)
    }
    pub fn text_shadow(
        &mut self,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        color: [f32; 4],
        align: Align,
    ) -> f32 {
        self.text
            .draw_shadow(self.gfx, self.b, s, x, y, size, color, align)
    }
    pub fn measure(&mut self, s: &str, size: f32) -> f32 {
        self.text.measure(self.gfx, s, size)
    }
    /// Draw a texture letterboxed into `r`.
    pub fn image_fit(&mut self, tex: TexId, r: Rect, alpha: f32) {
        let Some((w, h)) = self.gfx.texture_size(tex) else {
            return;
        };
        let s = (r.w / w as f32).min(r.h / h as f32);
        let (dw, dh) = (w as f32 * s, h as f32 * s);
        self.b.quad(
            tex,
            Rect::new(r.x + (r.w - dw) * 0.5, r.y + (r.h - dh) * 0.5, dw, dh),
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, alpha],
        );
    }
    /// Draw a texture covering `r` (cropped).
    pub fn image_cover(&mut self, tex: TexId, r: Rect, alpha: f32) {
        let Some((w, h)) = self.gfx.texture_size(tex) else {
            return;
        };
        let (tw, th) = (w as f32, h as f32);
        let s = (r.w / tw).max(r.h / th);
        let (uw, uh) = (r.w / (tw * s), r.h / (th * s));
        let (u0, v0) = ((1.0 - uw) * 0.5, (1.0 - uh) * 0.5);
        self.b
            .quad(tex, r, [u0, v0, u0 + uw, v0 + uh], [1.0, 1.0, 1.0, alpha]);
    }
}

pub trait Scene {
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next;
    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant);
    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next;
    /// Became the top scene again.
    fn resumed(&mut self, _ctx: &mut Ctx) {}
    /// About to be removed: release textures/sounds.
    fn exit(&mut self, _ctx: &mut Ctx, _gfx: &mut Gfx) {}
    /// The screen needs all of it (play): the web page hides its own
    /// buttons meanwhile.
    fn full_screen(&self) -> bool {
        false
    }
    /// Wants every frame redrawn ASAP (gameplay).
    fn realtime(&self) -> bool {
        false
    }
}

pub enum TableMsg {
    Loaded(Box<DifficultyTable>),
    Failed(String, String),
    Done,
}

/// Shared state available to every scene.
/// Songs are downloaded in an in-app browser window (desktop).
pub const IN_APP_BROWSER: bool = cfg!(all(
    feature = "browser",
    any(windows, target_os = "macos", target_os = "linux")
));

pub struct Ctx {
    pub paths: Paths,
    pub config: Config,
    pub lib: Option<Library>,
    pub songs: Arc<Vec<Song>>,
    pub scores: HashMap<(String, i32), BestScore>,
    pub tables: Vec<DifficultyTable>,
    pub mixer: Mixer,
    pub scan: Option<Arc<ScanProgress>>,
    pub table_rx: Option<Receiver<TableMsg>>,
    pub toasts: Vec<(String, Instant)>,
    pub touch_seen: bool,
    pub width: f32,
    pub height: f32,
    /// Device pixels per logical pixel (point).
    pub scale: f32,
    pub screen: crate::platform::screen::Screen,
    pub library_version: u64,
    pub fps: f32,
    pub want_fullscreen: Option<bool>,
    pub want_present_mode: bool,
    pub gpu_name: String,
    pub prefetch: Option<crate::platform::vfs::Prefetch>,
    /// Download pages to show in the in-app browser (song, then chart).
    pub browse: Option<Vec<String>>,
    /// Custom options read from skin files (path -> options).
    skin_options_cache:
        std::cell::RefCell<HashMap<std::path::PathBuf, Vec<crate::skin::CustomOption>>>,
    skin_offsets_cache:
        std::cell::RefCell<HashMap<std::path::PathBuf, Vec<crate::skin::CustomOffset>>>,
}

impl Ctx {
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
            self.width / 720.0
        } else {
            self.width.min(self.height * 16.0 / 9.0) / 1280.0
        };
        base.max(0.45) * self.config.display.ui_scale.clamp(0.5, 3.0)
    }

    /// Device pixels per millimetre (for controls with a physical size).
    pub fn px_per_mm(&self) -> f32 {
        self.screen
            .px_per_mm(self.scale, self.config.touch.screen_inches)
    }

    pub fn portrait(&self) -> bool {
        self.height > self.width
    }

    pub fn touch_enabled(&self) -> bool {
        match self.config.touch.mode {
            TouchMode::On => true,
            TouchMode::Off => false,
            TouchMode::Auto => {
                cfg!(any(target_os = "android", target_os = "ios")) || self.touch_seen
            }
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

    pub fn start_scan(&mut self) {
        if self
            .scan
            .as_ref()
            .map(|s| !s.done.load(Ordering::Acquire))
            .unwrap_or(false)
        {
            return;
        }
        let progress = Arc::new(ScanProgress::default());
        self.scan = Some(progress.clone());
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
        if let Some(lib) = &self.lib {
            let mut songs = lib.songs();
            songs.sort_by(|a, b| {
                a.title
                    .to_lowercase()
                    .cmp(&b.title.to_lowercase())
                    .then(a.level_num().cmp(&b.level_num()))
            });
            self.songs = Arc::new(songs);
            self.scores = lib.scores();
        }
        self.library_version += 1;
    }

    fn table_cache_file(&self, url: &str) -> PathBuf {
        let h = vibebms_core::bms::md5_hex(url.as_bytes());
        self.paths.tables.join(format!("{h}.json"))
    }

    pub fn load_cached_tables(&mut self) {
        self.tables.clear();
        for t in self.config.tables.iter().filter(|t| t.enabled) {
            let f = self.table_cache_file(&t.url);
            if let Some(dt) = crate::platform::vfs::read(&f)
                .ok()
                .and_then(|b| serde_json::from_slice::<DifficultyTable>(&b).ok())
            {
                self.tables.push(dt);
            }
        }
        self.library_version += 1;
    }

    /// Fetch tables in the background. `force` refetches even fresh caches.
    pub fn refresh_tables(&mut self, force: bool) {
        if self.table_rx.is_some() {
            return;
        }
        let now = crate::platform::SystemTime::now()
            .duration_since(crate::platform::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let todo: Vec<(String, String)> = self
            .config
            .tables
            .iter()
            .filter(|t| t.enabled)
            .filter(|t| {
                force
                    || !self.tables.iter().any(|dt| {
                        dt.source_url == t.url && now.saturating_sub(dt.fetched_at) < 86_400
                    })
            })
            .map(|t| (t.name.clone(), t.url.clone()))
            .collect();
        if todo.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        self.table_rx = Some(rx);
        let remaining = Arc::new(std::sync::atomic::AtomicUsize::new(todo.len()));
        for (name, url) in todo {
            let (tx, remaining) = (tx.clone(), remaining.clone());
            crate::platform::load_table(&url, move |r| {
                let _ = tx.send(match r {
                    Ok(t) => TableMsg::Loaded(Box::new(t)),
                    Err(e) => TableMsg::Failed(name, e),
                });
                if remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                    let _ = tx.send(TableMsg::Done);
                }
            });
        }
    }

    pub fn poll_jobs(&mut self) {
        // web: newly picked files -> read the chart files, then scan
        if crate::platform::vfs::take_added() {
            let skip: HashSet<PathBuf> = crate::paths::INTERNAL_DIRS
                .iter()
                .map(|d| self.paths.root.join(d))
                .collect();
            let charts = crate::library::find_charts(&self.scan_roots(), &skip);
            self.toast(format!("Reading {} charts…", charts.len()));
            self.prefetch = Some(crate::platform::vfs::Prefetch::files(charts));
        }
        if self
            .prefetch
            .as_ref()
            .map(|p| p.finished())
            .unwrap_or(false)
        {
            self.prefetch = None;
            // charts picked without their song's folder go into it
            #[cfg(target_arch = "wasm32")]
            for m in crate::web::place_picked_charts(&self.paths.root) {
                self.toast(m);
            }
            self.start_scan();
        }
        if let Some(s) = &self.scan {
            if s.done.load(Ordering::Acquire) {
                let changed = s.changed.load(Ordering::Acquire);
                self.scan = None;
                if changed || self.songs.is_empty() {
                    self.reload_library();
                }
                let n = self.songs.len();
                self.toast(format!("Library: {n} charts"));
            }
        }
        let mut done = false;
        let mut msgs = Vec::new();
        if let Some(rx) = &self.table_rx {
            while let Ok(m) = rx.try_recv() {
                msgs.push(m);
            }
        }
        for m in msgs {
            match m {
                TableMsg::Loaded(t) => {
                    let t = *t;
                    if let Ok(j) = serde_json::to_vec(&t) {
                        let _ =
                            crate::platform::vfs::write(&self.table_cache_file(&t.source_url), &j);
                    }
                    self.toast(format!(
                        "Table updated: {} ({} charts)",
                        t.header.name,
                        t.entries.len()
                    ));
                    if let Some(old) = self
                        .tables
                        .iter_mut()
                        .find(|o| o.source_url == t.source_url)
                    {
                        *old = t;
                    } else {
                        self.tables.push(t);
                    }
                    // keep config order
                    let order: Vec<String> =
                        self.config.tables.iter().map(|t| t.url.clone()).collect();
                    self.tables.sort_by_key(|t| {
                        order
                            .iter()
                            .position(|u| *u == t.source_url)
                            .unwrap_or(usize::MAX)
                    });
                    self.library_version += 1;
                }
                TableMsg::Failed(n, e) => self.toast(format!("Table {n}: {e}")),
                TableMsg::Done => done = true,
            }
        }
        if done {
            self.table_rx = None;
        }
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

    /// Option ids to pass to a skin loader for the skin configured as
    /// `value` (a `config.skins` value) at `path`.
    pub fn skin_user_options(&self, value: &str, path: &std::path::Path) -> HashMap<i32, bool> {
        crate::skin::user_options(
            &self.skin_custom_options(path),
            self.config.skin_options.get(value),
        )
    }

    pub fn best(&self, song: &Song, ln: i32) -> Option<&BestScore> {
        self.scores
            .get(&(song.sha256.clone(), if song.has_ln { ln } else { 0 }))
    }
}

struct Running {
    window: Arc<Window>,
    gfx: Gfx,
    text: Text,
    batch: Batch,
    ctx: Ctx,
    scenes: Vec<Box<dyn Scene>>,
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    gilrs: Option<gilrs::Gilrs>,
    last_touch: Option<Instant>,
    /// Fonts downloaded in the background (web build).
    font_rx: Option<Receiver<Vec<u8>>>,
    mouse_down: bool,
    cursor: (f32, f32),
    frame_count: u32,
    fps_since: Instant,
    next_frame: Instant,
    #[cfg(all(
        feature = "browser",
        any(windows, target_os = "macos", target_os = "linux")
    ))]
    downloads: crate::browser::Downloads,
}

pub enum UserEvent {
    /// GPU initialisation finished (asynchronous in browsers).
    GfxReady(Arc<Window>, Box<Result<Gfx, String>>),
}

pub struct App {
    run: Option<Running>,
    paths: Paths,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    proxy: EventLoopProxy<UserEvent>,
    /// The configuration while the window and renderer are being created.
    pending: Option<Config>,
}

impl App {
    pub fn new(paths: Paths, proxy: EventLoopProxy<UserEvent>) -> App {
        App {
            run: None,
            paths,
            proxy,
            pending: None,
        }
    }
}

pub fn run() {
    let paths = Paths::detect();
    log::info!("data root: {}", paths.root.display());
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(e) => e,
        Err(e) => {
            log::error!("cannot create event loop: {e}");
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    #[allow(unused_mut)]
    let mut app = App::new(paths, event_loop.create_proxy());
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        crate::web::install_file_input(app.paths.root.clone());
        event_loop.spawn_app(app);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("event loop: {e}");
    }
}

impl Running {
    fn dispatch(&mut self, el: &ActiveEventLoop, ev: InputEv) {
        // browsers only start audio after a user gesture
        if crate::platform::IS_WEB
            && matches!(
                ev,
                InputEv::Key { pressed: true, .. }
                    | InputEv::Pointer {
                        phase: Phase::Down,
                        ..
                    }
            )
        {
            self.ctx.mixer.resume();
        }
        let Some(top) = self.scenes.last_mut() else {
            return;
        };
        let next = top.input(&mut self.ctx, &ev);
        self.apply(el, next);
    }

    fn apply(&mut self, el: &ActiveEventLoop, next: Next) {
        match next {
            Next::Stay => {}
            Next::Push(s) => self.scenes.push(s),
            Next::Replace(s) => {
                if let Some(mut old) = self.scenes.pop() {
                    old.exit(&mut self.ctx, &mut self.gfx);
                }
                self.scenes.push(s);
            }
            Next::Pop => {
                if self.scenes.len() > 1 {
                    if let Some(mut old) = self.scenes.pop() {
                        old.exit(&mut self.ctx, &mut self.gfx);
                    }
                    if let Some(top) = self.scenes.last_mut() {
                        top.resumed(&mut self.ctx);
                    }
                }
            }
            Next::Quit => {
                self.ctx.save_config();
                el.exit();
            }
        }
    }

    fn poll_gamepads(&mut self, el: &ActiveEventLoop) {
        #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
        {
            let mut evs = Vec::new();
            if let Some(g) = self.gilrs.as_mut() {
                let now_i = Instant::now();
                let now_s = crate::platform::SystemTime::now();
                while let Some(e) = g.next_event() {
                    let age = now_s
                        .duration_since(e.time)
                        .unwrap_or_default()
                        .min(Duration::from_millis(50));
                    let time = now_i.checked_sub(age).unwrap_or(now_i);
                    let pad = usize::from(e.id) as u64 * 1_000_000;
                    match e.event {
                        gilrs::EventType::ButtonPressed(_, code) => evs.push(InputEv::Pad {
                            id: pad + code.into_u32() as u64,
                            pressed: true,
                            time,
                        }),
                        gilrs::EventType::ButtonReleased(_, code) => evs.push(InputEv::Pad {
                            id: pad + code.into_u32() as u64,
                            pressed: false,
                            time,
                        }),
                        gilrs::EventType::AxisChanged(_, value, code) => evs.push(InputEv::Axis {
                            id: pad + code.into_u32() as u64,
                            value,
                            time,
                        }),
                        gilrs::EventType::Connected => self.ctx.toast("Controller connected"),
                        gilrs::EventType::Disconnected => self.ctx.toast("Controller disconnected"),
                        _ => {}
                    }
                }
            }
            for e in evs {
                self.dispatch(el, e);
            }
        }
        #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
        let _ = el;
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        if let Some(rx) = &self.font_rx {
            while let Ok(f) = rx.try_recv() {
                self.text.add_font(f);
            }
        }
        self.poll_gamepads(el);
        self.ctx.poll_jobs();
        #[cfg(target_arch = "wasm32")]
        crate::web::show_page_buttons(!self.scenes.last().is_some_and(|s| s.full_screen()));
        if let Some(fs) = self.ctx.want_fullscreen.take() {
            self.window
                .set_fullscreen(fs.then_some(Fullscreen::Borderless(None)));
        }
        if std::mem::take(&mut self.ctx.want_present_mode) {
            self.gfx
                .set_present_mode(self.ctx.config.display.present_mode);
        }
        let next = match self.scenes.last_mut() {
            Some(top) => top.update(&mut self.ctx, &mut self.gfx, now),
            None => Next::Quit,
        };
        self.apply(el, next);

        self.batch.clear();
        let mut d = Draw {
            gfx: &self.gfx,
            text: &mut self.text,
            b: &mut self.batch,
        };
        if let Some(top) = self.scenes.last_mut() {
            top.draw(&mut self.ctx, &mut d, now);
        }
        draw_overlay(&mut self.ctx, &mut d, now);
        if let Err(e) = self.gfx.render(&self.batch, theme::BACKGROUND) {
            log::warn!("render: {e}");
        }
        self.frame_count += 1;
        let el_t = now.duration_since(self.fps_since);
        if el_t >= Duration::from_millis(500) {
            self.ctx.fps = self.frame_count as f32 / el_t.as_secs_f32();
            self.frame_count = 0;
            self.fps_since = now;
        }
    }
}

/// Web build: fonts listed in `fonts/fonts.txt` next to index.html.
fn web_fonts() -> Option<Receiver<Vec<u8>>> {
    if !crate::platform::IS_WEB {
        return None;
    }
    let (tx, rx) = channel();
    crate::platform::http_get("fonts/fonts.txt", move |r| {
        let Ok(list) = r else { return };
        for name in String::from_utf8_lossy(&list)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let tx = tx.clone();
            crate::platform::http_get(&format!("fonts/{name}"), move |r| {
                if let Ok(b) = r {
                    let _ = tx.send(b);
                }
            });
        }
    });
    Some(rx)
}

fn draw_overlay(ctx: &mut Ctx, d: &mut Draw, now: Instant) {
    let ui = ctx.ui();
    ctx.toasts
        .retain(|(_, t)| now.duration_since(*t) < Duration::from_secs(4));
    let mut y = 12.0 * ui;
    for (msg, t) in &ctx.toasts {
        let age = now.duration_since(*t).as_secs_f32();
        let a = (4.0 - age).clamp(0.0, 1.0);
        let size = 18.0 * ui;
        let w = d.measure(msg, size) + 24.0 * ui;
        let r = Rect::new(ctx.width - w - 12.0 * ui, y, w, size + 14.0 * ui);
        d.b.rect(r, with_alpha(rgba(0x000000d0), a));
        d.b.rect(
            Rect::new(r.x, r.y, 3.0 * ui, r.h),
            with_alpha(theme::ACCENT, a),
        );
        d.text(
            msg,
            r.x + 12.0 * ui,
            r.y + 7.0 * ui,
            size,
            with_alpha(theme::TEXT, a),
            Align::Left,
        );
        y += r.h + 6.0 * ui;
    }
    if ctx.config.display.show_fps {
        let s = format!("{:.0} FPS", ctx.fps);
        d.text_shadow(
            &s,
            6.0,
            ctx.height - 22.0,
            16.0,
            theme::TEXT_DIM,
            Align::Left,
        );
    }
}

impl App {
    fn init(&mut self, window: Arc<Window>, mut gfx: Gfx) {
        let config = self
            .pending
            .take()
            .unwrap_or_else(|| Config::load(&self.paths.config_file()));
        let text = Text::new(&[self.paths.fonts.as_path()]);
        let a = &config.audio;
        let mixer = Mixer::new(&AudioConfig {
            buffer_frames: a.buffer_frames,
            sample_rate: a.sample_rate,
            master: a.master,
            key_volume: a.key,
            bgm_volume: a.bgm,
            system_volume: a.system,
            max_voices: 512,
        });
        let lib = Some(Library::open(&self.paths.cache, &self.paths.scores));
        let size = {
            let (width, height) = surface_size(&window);
            // the renderer started from winit's idea of the size
            gfx.resize(width, height);
            winit::dpi::PhysicalSize::new(width, height)
        };
        let mut ctx = Ctx {
            paths: self.paths.clone(),
            config,
            lib,
            songs: Arc::new(Vec::new()),
            scores: HashMap::new(),
            tables: Vec::new(),
            mixer,
            scan: None,
            table_rx: None,
            toasts: Vec::new(),
            touch_seen: false,
            width: size.width as f32,
            height: size.height as f32,
            scale: window.scale_factor() as f32,
            screen: {
                let s = crate::platform::screen::detect(&window);
                let scale = window.scale_factor() as f32;
                log::info!(
                    "screen {}x{} px, about {:.1}\" ({} points per inch, guessed)",
                    s.size_px.0,
                    s.size_px.1,
                    s.guessed_inches(scale),
                    s.points_per_inch
                );
                s
            },
            library_version: 0,
            fps: 0.0,
            want_fullscreen: None,
            want_present_mode: false,
            gpu_name: gfx.adapter_name.clone(),
            prefetch: None,
            browse: None,
            skin_options_cache: Default::default(),
            skin_offsets_cache: Default::default(),
        };
        ctx.reload_library();
        ctx.load_cached_tables();
        ctx.start_scan();
        ctx.refresh_tables(false);
        ctx.save_config(); // writes defaults on first launch
        let select = crate::scene::select::SelectScene::new(&mut ctx);
        let mut scenes: Vec<Box<dyn Scene>> = vec![Box::new(select)];
        // `vibebms <chart> [--autoplay]` plays a chart directly
        let args: Vec<String> = if crate::platform::IS_WEB {
            Vec::new()
        } else {
            std::env::args().skip(1).collect()
        };
        let autoplay = args.iter().any(|a| a == "--autoplay" || a == "-a");
        if let Some(p) = args.iter().find(|a| !a.starts_with('-')) {
            let path = PathBuf::from(p);
            match vibebms_core::load_chart(&path, &Default::default()) {
                Ok(c) => {
                    let song = crate::library::song_from_chart(&path, String::new(), &c);
                    let req = crate::scene::play::PlayRequest {
                        song,
                        autoplay,
                        course: None,
                    };
                    scenes.push(Box::new(crate::scene::play::PlayScene::new(&mut ctx, req)));
                }
                Err(e) => ctx.toast(format!("{}: {e}", path.display())),
            }
        }
        let now = Instant::now();
        self.run = Some(Running {
            window,
            gfx,
            text,
            batch: Batch::default(),
            ctx,
            scenes,
            #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
            gilrs: gilrs::Gilrs::new()
                .map_err(|e| log::warn!("gamepads unavailable: {e}"))
                .ok(),
            last_touch: None,
            font_rx: web_fonts(),
            mouse_down: false,
            cursor: (0.0, 0.0),
            frame_count: 0,
            fps_since: now,
            next_frame: now,
            #[cfg(all(
                feature = "browser",
                any(windows, target_os = "macos", target_os = "linux")
            ))]
            downloads: Default::default(),
        });
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::GfxReady(window, gfx) => match *gfx {
                Ok(gfx) => self.init(window, gfx),
                Err(e) => {
                    log::error!("graphics init failed: {e}");
                    el.exit();
                }
            },
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.run.is_some() || self.pending.is_some() {
            return;
        }
        let config = Config::load(&self.paths.config_file());
        let mut attrs = Window::default_attributes()
            .with_title("vibeBMS")
            .with_inner_size(winit::dpi::LogicalSize::new(
                config.display.width.max(320),
                config.display.height.max(240),
            ));
        if config.display.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            attrs = attrs.with_window_icon(window_icon());
        }
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            attrs = attrs
                .with_append(true)
                .with_prevent_default(true)
                .with_focusable(true);
        }
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("window: {e}");
                el.exit();
                return;
            }
        };
        let mode = config.display.present_mode;
        self.pending = Some(config);
        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let g = Gfx::create(window.clone(), mode).await;
                let _ = proxy.send_event(UserEvent::GfxReady(window, Box::new(g)));
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        match Gfx::new(window.clone(), mode) {
            Ok(g) => self.init(window, g),
            Err(e) => {
                log::error!("graphics init failed: {e}");
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(r) = self.run.as_mut() else { return };
        #[cfg(all(
            feature = "browser",
            any(windows, target_os = "macos", target_os = "linux")
        ))]
        if r.downloads.window_event(id, &event) {
            return;
        }
        let _ = id;
        let now = Instant::now();
        match event {
            WindowEvent::CloseRequested => {
                r.ctx.save_config();
                el.exit();
            }
            WindowEvent::Resized(s) => {
                let (w, h) = surface_size(&r.window);
                let _ = s;
                r.gfx.resize(w, h);
                r.ctx.width = w as f32;
                r.ctx.height = h as f32;
                r.ctx.scale = r.window.scale_factor() as f32;
                r.ctx.screen = crate::platform::screen::detect(&r.window);
            }
            WindowEvent::RedrawRequested => r.frame(el),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let pressed = event.state == ElementState::Pressed;
                if pressed && !event.repeat && (code == KeyCode::F11) {
                    r.ctx.config.display.fullscreen = !r.ctx.config.display.fullscreen;
                    r.ctx.want_fullscreen = Some(r.ctx.config.display.fullscreen);
                    return;
                }
                let text = event.text.as_ref().map(|t| t.to_string());
                r.dispatch(
                    el,
                    InputEv::Key {
                        code,
                        pressed,
                        repeat: event.repeat,
                        time: now,
                        text,
                    },
                );
            }
            WindowEvent::CursorMoved { position, .. } => {
                r.cursor = (position.x as f32, position.y as f32);
                let recent_touch = r
                    .last_touch
                    .map(|t| now.duration_since(t) < Duration::from_millis(600))
                    .unwrap_or(false);
                if r.mouse_down && !recent_touch {
                    r.dispatch(
                        el,
                        InputEv::Pointer {
                            id: 0,
                            phase: Phase::Move,
                            x: r.cursor.0,
                            y: r.cursor.1,
                            time: now,
                            touch: false,
                        },
                    );
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let recent_touch = r
                    .last_touch
                    .map(|t| now.duration_since(t) < Duration::from_millis(600))
                    .unwrap_or(false);
                if recent_touch {
                    return;
                }
                r.mouse_down = state == ElementState::Pressed;
                let phase = if r.mouse_down { Phase::Down } else { Phase::Up };
                r.dispatch(
                    el,
                    InputEv::Pointer {
                        id: 0,
                        phase,
                        x: r.cursor.0,
                        y: r.cursor.1,
                        time: now,
                        touch: false,
                    },
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                r.dispatch(el, InputEv::Wheel { dy });
            }
            WindowEvent::Touch(t) => {
                r.ctx.touch_seen = true;
                r.last_touch = Some(now);
                let phase = match t.phase {
                    TouchPhase::Started => Phase::Down,
                    TouchPhase::Moved => Phase::Move,
                    TouchPhase::Ended | TouchPhase::Cancelled => Phase::Up,
                };
                r.dispatch(
                    el,
                    InputEv::Pointer {
                        id: t.id + 1,
                        phase,
                        x: t.location.x as f32,
                        y: t.location.y as f32,
                        time: now,
                        touch: true,
                    },
                );
            }
            WindowEvent::DroppedFile(p) => r.dispatch(el, InputEv::Dropped(p)),
            WindowEvent::Focused(false) => r.ctx.save_config(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(r) = self.run.as_mut() else { return };
        #[cfg(all(
            feature = "browser",
            any(windows, target_os = "macos", target_os = "linux")
        ))]
        {
            if let Some(pages) = r.ctx.browse.take() {
                r.downloads.browse(el, &mut r.ctx, pages);
            }
            r.downloads.poll(&mut r.ctx);
        }
        let realtime = r.scenes.last().map(|s| s.realtime()).unwrap_or(false);
        let fps = r.ctx.config.display.max_fps;
        let now = Instant::now();
        if fps > 0 || !realtime {
            // menus: cap at the display rate (or 120) to save power; play: user cap
            let cap = if fps > 0 { fps } else { 120 };
            let interval = Duration::from_secs_f64(1.0 / cap as f64);
            if now >= r.next_frame {
                r.next_frame = (r.next_frame + interval).max(now);
                r.window.request_redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(r.next_frame));
        } else {
            el.set_control_flow(ControlFlow::Poll);
            r.window.request_redraw();
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        if let Some(r) = self.run.as_mut() {
            r.ctx.save_config();
        }
    }
}

/// Size to render at, in device pixels (as pointer positions are). On the
/// web winit can report the canvas size in CSS pixels, which on high-DPI
/// screens (iPad, phones) put every touch in the wrong place and rendered
/// at half resolution: there it is the canvas' CSS size times the device
/// pixel ratio, and the canvas' drawing buffer is resized to it.
/// The app icon (`tools/gen_logo.py`) for the window and task bar.
#[cfg(not(target_arch = "wasm32"))]
fn window_icon() -> Option<winit::window::Icon> {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

fn surface_size(window: &Window) -> (u32, u32) {
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowExtWebSys;
        if let (Some(canvas), Some(win)) = (window.canvas(), web_sys::window()) {
            let r = canvas.get_bounding_client_rect();
            let dpr = win.device_pixel_ratio();
            let (w, h) = ((r.width() * dpr).round(), (r.height() * dpr).round());
            if w >= 1.0 && h >= 1.0 {
                let (w, h) = (w as u32, h as u32);
                if canvas.width() != w || canvas.height() != h {
                    canvas.set_width(w);
                    canvas.set_height(h);
                }
                return (w, h);
            }
        }
    }
    let s = window.inner_size();
    (s.width.max(1), s.height.max(1))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn window_icon_decodes() {
        assert!(super::window_icon().is_some());
    }
}
