//! Window, event loop, shared context and the scene stack.

use crate::config::{Config, TouchMode};
use crate::gfx::text::{Align, Text};
use crate::gfx::{rgba, with_alpha, Batch, Gfx, Rect, TexId};
use crate::input::{InputEv, Phase};
use crate::library::{BestScore, Library, ScanProgress, Song};
use crate::paths::Paths;
use crate::theme::Theme;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vibebms_audio::{AudioConfig, Mixer};
use vibebms_tables::DifficultyTable;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
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
    /// Wants every frame redrawn ASAP (gameplay).
    fn realtime(&self) -> bool {
        false
    }
}

pub enum TableMsg {
    Loaded(DifficultyTable),
    Failed(String, String),
    Done,
}

/// Shared state available to every scene.
pub struct Ctx {
    pub paths: Paths,
    pub config: Config,
    pub theme: Theme,
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
    pub library_version: u64,
    pub fps: f32,
    pub want_fullscreen: Option<bool>,
    pub want_present_mode: bool,
    pub want_theme_reload: bool,
    pub gpu_name: String,
    pub bg_texture: Option<TexId>,
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
        let db = self.paths.db_file();
        let roots = self.scan_roots();
        let skip: HashSet<PathBuf> = crate::paths::INTERNAL_DIRS
            .iter()
            .map(|d| self.paths.root.join(d))
            .collect();
        std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || crate::library::scan(db, roots, skip, progress))
            .ok();
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
            if let Some(dt) = std::fs::read(&f)
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let todo: Vec<(String, String, PathBuf)> = self
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
            .map(|t| (t.name.clone(), t.url.clone(), self.table_cache_file(&t.url)))
            .collect();
        if todo.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        self.table_rx = Some(rx);
        std::thread::Builder::new()
            .name("tables".into())
            .spawn(move || {
                let f = vibebms_tables::UreqFetcher::default();
                for (name, url, cache) in todo {
                    match vibebms_tables::fetch_table(&f, &url) {
                        Ok(t) => {
                            if let Ok(j) = serde_json::to_vec(&t) {
                                let _ = std::fs::write(&cache, j);
                            }
                            let _ = tx.send(TableMsg::Loaded(t));
                        }
                        Err(e) => {
                            let _ = tx.send(TableMsg::Failed(name, e.to_string()));
                        }
                    }
                }
                let _ = tx.send(TableMsg::Done);
            })
            .ok();
    }

    pub fn poll_jobs(&mut self) {
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
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    gilrs: Option<gilrs::Gilrs>,
    last_touch: Option<Instant>,
    mouse_down: bool,
    cursor: (f32, f32),
    frame_count: u32,
    fps_since: Instant,
    next_frame: Instant,
}

pub struct App {
    run: Option<Running>,
    paths: Paths,
}

impl App {
    pub fn new(paths: Paths) -> App {
        App { run: None, paths }
    }
}

pub fn run() {
    let paths = Paths::detect();
    log::info!("data root: {}", paths.root.display());
    let event_loop = match EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            log::error!("cannot create event loop: {e}");
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(paths);
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("event loop: {e}");
    }
}

impl Running {
    fn dispatch(&mut self, el: &ActiveEventLoop, ev: InputEv) {
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
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let mut evs = Vec::new();
            if let Some(g) = self.gilrs.as_mut() {
                let now_i = Instant::now();
                let now_s = std::time::SystemTime::now();
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
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let _ = el;
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        self.poll_gamepads(el);
        self.ctx.poll_jobs();
        if let Some(fs) = self.ctx.want_fullscreen.take() {
            self.window
                .set_fullscreen(fs.then_some(Fullscreen::Borderless(None)));
        }
        if std::mem::take(&mut self.ctx.want_present_mode) {
            self.gfx
                .set_present_mode(self.ctx.config.display.present_mode);
        }
        if std::mem::take(&mut self.ctx.want_theme_reload) {
            self.ctx.theme = Theme::load(&self.ctx.paths.themes, &self.ctx.config.theme);
            if let Some(t) = self.ctx.bg_texture.take() {
                self.gfx.free_texture(t);
            }
            self.ctx.bg_texture = self
                .ctx
                .theme
                .background_image
                .clone()
                .and_then(|p| self.gfx.load_image(&p, false));
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
        if let Err(e) = self.gfx.render(&self.batch, self.ctx.theme.background) {
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
            with_alpha(ctx.theme.accent, a),
        );
        d.text(
            msg,
            r.x + 12.0 * ui,
            r.y + 7.0 * ui,
            size,
            with_alpha(ctx.theme.text, a),
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
            ctx.theme.text_dim,
            Align::Left,
        );
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.run.is_some() {
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
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("window: {e}");
                el.exit();
                return;
            }
        };
        let mut gfx = match Gfx::new(window.clone(), config.display.present_mode) {
            Ok(g) => g,
            Err(e) => {
                log::error!("graphics init failed: {e}");
                el.exit();
                return;
            }
        };
        let theme = Theme::load(&self.paths.themes, &config.theme);
        let theme_fonts = theme.dir.as_ref().map(|d| d.join("fonts"));
        let mut font_dirs = vec![self.paths.fonts.as_path()];
        if let Some(t) = theme_fonts.as_deref() {
            font_dirs.insert(0, t);
        }
        let text = Text::new(&font_dirs);
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
        let lib = Library::open(&self.paths.db_file())
            .map_err(|e| log::error!("library: {e}"))
            .ok();
        let size = window.inner_size();
        let bg_texture = theme
            .background_image
            .clone()
            .and_then(|p| gfx.load_image(&p, false));
        let mut ctx = Ctx {
            paths: self.paths.clone(),
            config,
            theme,
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
            library_version: 0,
            fps: 0.0,
            want_fullscreen: None,
            want_present_mode: false,
            want_theme_reload: false,
            gpu_name: gfx.adapter_name.clone(),
            bg_texture,
        };
        ctx.reload_library();
        ctx.load_cached_tables();
        ctx.start_scan();
        ctx.refresh_tables(false);
        ctx.save_config(); // writes defaults on first launch
        let select = crate::scene::select::SelectScene::new(&mut ctx);
        let mut scenes: Vec<Box<dyn Scene>> = vec![Box::new(select)];
        // `vibebms <chart> [--autoplay]` plays a chart directly
        let args: Vec<String> = std::env::args().skip(1).collect();
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
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            gilrs: gilrs::Gilrs::new()
                .map_err(|e| log::warn!("gamepads unavailable: {e}"))
                .ok(),
            last_touch: None,
            mouse_down: false,
            cursor: (0.0, 0.0),
            frame_count: 0,
            fps_since: now,
            next_frame: now,
        });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(r) = self.run.as_mut() else { return };
        let now = Instant::now();
        match event {
            WindowEvent::CloseRequested => {
                r.ctx.save_config();
                el.exit();
            }
            WindowEvent::Resized(s) => {
                r.gfx.resize(s.width, s.height);
                r.ctx.width = s.width as f32;
                r.ctx.height = s.height as f32;
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
