//! Window, event loop and the scene stack (the state they share: `ctx`).

mod activity;
mod cli;
mod diag;
mod events;
#[cfg(desktop)]
pub mod export;
mod pacing;
pub mod screenshot;
mod startup;
mod viewer;
#[cfg(target_arch = "wasm32")]
pub mod web;

use crate::config::Config;
use crate::ctx::Ctx;
use crate::gfx::text::{Align, Text, TextStyle};
use crate::gfx::theme;
use crate::gfx::{rgba, with_alpha, Batch, Draw, Gfx, Rect};
use crate::input::{InputEv, Phase};
use crate::paths::Paths;
use crate::platform::{Duration, Instant, Job};
use crate::scene::{Next, Scene};
use std::sync::Arc;
use vibebms_audio::Mixer;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Fullscreen, Window, WindowId};

struct Running {
    window: Arc<Window>,
    gfx: Gfx,
    text: Text,
    batch: Batch,
    ctx: Ctx,
    /// The caches being read at start.
    startup: startup::Startup,
    scenes: Vec<Box<dyn Scene>>,
    /// Third parties' copies are on without the player having said OK to
    /// what they are (a build that has them on, a setting from before it
    /// asked): said once over the song select, OK or turn them off.
    third_party_notice: Option<crate::ui::Notice>,
    gamepads: crate::input::gamepad::Gamepads,
    /// The scenes changed: Discord and OBS are told the screen.
    activity_dirty: bool,
    last_touch: Option<Instant>,
    /// The page's fingers down and where they last were (web).
    #[cfg(target_arch = "wasm32")]
    touches: std::collections::HashMap<u64, (f32, f32)>,
    fonts: WebFonts,
    /// A screenshot's file name while its frame is read back, then its
    /// saving.
    shot_name: Option<String>,
    shot_job: Option<Job<Result<String, String>>>,
    mouse_down: bool,
    cursor: (f32, f32),
    frame_count: u32,
    fps_since: Instant,
    /// When the last frame started (the latency overlay's frame time).
    last_frame: Option<Instant>,
    /// No frame drawn yet (the start's last phase).
    first_frame: bool,
    /// When to start the next frame (a frame rate cap, or with vsync when
    /// its image will be free).
    next_frame: Instant,
    pacer: pacing::Pacer,
    /// The display's refresh rate (Hz; 0: not known).
    refresh_hz: f64,
    /// A redraw asked for and not drawn yet (the web: asking again would
    /// wake the event loop again at once, and it would spin).
    redraw_pending: bool,
    /// The on-screen keyboard is asked for.
    ime_on: bool,
    downloads: crate::browser::Downloads,
    /// The chart viewer's requests (`-P`: it stays running for them).
    viewer: Option<viewer::Server>,
    /// Whether the app is the one in use (silent when not).
    activity: activity::Activity,
    /// When to ask next whether the window is minimised (while it has no
    /// focus: not every platform tells).
    #[cfg(desktop)]
    minimized_check: Instant,
    /// iOS: the system's edge gestures wait for a second swipe (a screen
    /// that takes the whole of it: play).
    #[cfg(target_os = "ios")]
    edges_deferred: bool,
}

pub enum UserEvent {
    /// GPU initialisation finished (asynchronous in browsers).
    GfxReady(Arc<Window>, Box<Result<Gfx, String>>),
    /// Hardware keyboard keys came in (`platform::ios::take_keys`).
    #[cfg(target_os = "ios")]
    Keys,
    /// The page got or lost the focus (`web::install_page_focus`).
    #[cfg(target_arch = "wasm32")]
    PageFocus(bool),
}

pub struct App {
    run: Option<Running>,
    paths: Paths,
    /// Until the window's start takes them.
    profiles: crate::profiles::Profiles,
    #[cfg_attr(not(any(target_arch = "wasm32", target_os = "ios")), allow(dead_code))]
    proxy: EventLoopProxy<UserEvent>,
    /// The command line's, and the configuration until the window's start
    /// takes it.
    launch: cli::Launch,
    /// The window and renderer are being created.
    creating: bool,
}

impl App {
    pub fn new(
        paths: Paths,
        profiles: crate::profiles::Profiles,
        proxy: EventLoopProxy<UserEvent>,
    ) -> App {
        // now: a self-update replaces the file later (Windows), while this
        // build runs on
        #[cfg(desktop)]
        crate::platform::spawn("exe hash", || {
            crate::ir::build_id::exe_sha256();
        });
        App {
            run: None,
            paths,
            profiles,
            proxy,
            launch: Default::default(),
            creating: false,
        }
    }
}

pub fn run() {
    diag::startup("");
    let (paths, profiles) = crate::profiles::Profiles::open_at(Paths::detect_root());
    // the command line's subcommands, its help and its errors
    let launch = cli::run_before_window(&paths).unwrap_or_else(|code| std::process::exit(code));
    log::info!("data root: {}", paths.root.display());
    if !paths.profile.is_empty() {
        log::info!("profile: {}", paths.player.display());
    }
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(e) => e,
        Err(e) => {
            log::error!("cannot create event loop: {e}");
            return;
        }
    };
    diag::startup("event loop");
    event_loop.set_control_flow(if crate::platform::IS_WEB {
        ControlFlow::Wait
    } else {
        ControlFlow::Poll
    });
    #[allow(unused_mut)]
    let mut app = App::new(paths, profiles, event_loop.create_proxy());
    app.launch = launch;
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        crate::app::web::install_file_input(app.paths.root.clone());
        crate::app::web::install_input_stamps();
        crate::app::web::install_touch_watch();
        crate::app::web::install_touch_input();
        {
            let proxy = app.proxy.clone();
            crate::app::web::install_page_focus(move |f| {
                let _ = proxy.send_event(UserEvent::PageFocus(f));
            });
        }
        // the songs kept in the browser: mounted, then read as picked ones
        // (the first scan waits for them)
        let replays = app.paths.replays();
        let skins = app.paths.skins.clone();
        let root = &app.paths.root;
        let root_replays = crate::library::replay::dir(root);
        crate::platform::opfs::start(root, root_replays, crate::platform::vfs::mount, move |n| {
            if n > 0 {
                log::info!("library: {n} files kept in the browser");
            }
            crate::platform::vfs::mark_added();
            // the replays are small: read now, ready when a play wants one;
            // the skins downloaded: read now, as skins are read at once
            let _ = crate::platform::vfs::Prefetch::dirs(&[replays, skins]);
        });
        event_loop.spawn_app(app);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("event loop: {e}");
    }
}

impl Running {
    /// A screenshot read back: saved in the background; once saved, said.
    fn poll_screenshot(&mut self) {
        if let Some(r) = self
            .shot_name
            .is_some()
            .then(|| self.gfx.take_capture())
            .flatten()
        {
            let name = self.shot_name.take().unwrap_or_default();
            match r {
                Ok(img) => {
                    self.shot_job = Some(crate::app::screenshot::save(
                        img,
                        name,
                        &self.ctx.paths.root,
                    ))
                }
                Err(e) => self.ctx.toast(tr!("Screenshot: {e}", e = e)),
            }
        }
        for r in Job::drain_slot(&mut self.shot_job) {
            match r {
                Ok(path) => self.ctx.toast(tr!("Screenshot: {path}", path = path)),
                Err(e) => self.ctx.toast(tr!("Screenshot: {e}", e = e)),
            }
        }
    }

    /// The display paces the frames, uncapped, at a rate it tells (the
    /// web's browser paces them itself).
    fn vsync_paced(&self) -> bool {
        self.gfx.vsync()
            && self.ctx.config.display.max_fps == 0
            && self.refresh_hz > 0.0
            && !crate::platform::IS_WEB
    }

    /// The window may be on another display: its refresh rate.
    fn display_changed(&mut self) {
        self.refresh_hz = refresh_hz(&self.window);
        self.pacer.set_refresh(self.refresh_hz);
    }

    /// iOS: while a screen takes the whole of it (play, the touch layout),
    /// a swipe from the top or bottom edge (a touch lane's) is the game's:
    /// the system's gesture (Control Center, the notifications, home) waits
    /// for a second one.
    #[cfg(target_os = "ios")]
    fn defer_edges(&mut self) {
        use winit::platform::ios::{ScreenEdge, WindowExtIOS};
        let want = self.scenes.last().is_some_and(|s| s.full_screen());
        if want != self.edges_deferred {
            self.edges_deferred = want;
            let edges = match want {
                true => ScreenEdge::TOP | ScreenEdge::BOTTOM,
                false => ScreenEdge::NONE,
            };
            self.window
                .set_preferred_screen_edges_deferring_system_gestures(edges);
        }
    }

    /// Ask for a frame (on the web, once until it is drawn).
    fn redraw(&mut self) {
        self.redraw_pending = crate::platform::IS_WEB;
        self.window.request_redraw();
    }

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
            self.ctx.audio.mixer.resume();
        }
        if let Some(n) = self.third_party_notice.as_mut() {
            // (keys let go unseen still reach the scene)
            if !matches!(ev, InputEv::Released { .. }) {
                if let Some(ok) = n.input(&ev, &self.ctx.config.keys) {
                    self.third_party_notice = None;
                    answer_third_party_notice(&mut self.ctx, ok);
                }
                return;
            }
        }
        if self.downloads.input(&mut self.ctx, &ev) {
            return;
        }
        let Some(top) = self.scenes.last_mut() else {
            return;
        };
        diag::input(&ev, Instant::now());
        let next = top.input(&mut self.ctx, &ev);
        self.apply(el, next);
    }

    fn apply(&mut self, el: &ActiveEventLoop, next: Next) {
        if !matches!(next, Next::Stay) {
            self.activity_dirty = true;
        }
        match next {
            Next::Stay => {}
            Next::Push(s) => {
                if let Some(top) = self.scenes.last_mut() {
                    top.covered(&mut self.gfx);
                }
                self.scenes.push(s);
            }
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
                self.ctx.streaming.stop();
                el.exit();
            }
        }
    }

    /// A request to the chart viewer: what plays stops (back to song
    /// select), the chart read again plays from its measure.
    fn poll_viewer(&mut self, el: &ActiveEventLoop) {
        let Some(req) = self.viewer.as_ref().and_then(viewer::Server::poll) else {
            return;
        };
        while self.scenes.len() > 1 {
            self.apply(el, Next::Pop);
        }
        if let viewer::Request::Play { path, measure } = req {
            if let Some(s) = cli::viewer_scene(&mut self.ctx, &path, measure) {
                self.apply(el, Next::Push(s));
            }
            self.window.focus_window();
        }
    }

    /// A profile chosen (settings, song select): back to song select,
    /// the profile's settings and records read, the song select made
    /// again with them (its skin, key config, the IR's).
    fn poll_profile(&mut self, el: &ActiveEventLoop) {
        let Some(id) = self.ctx.profiles.switch_to.take() else {
            return;
        };
        if id == self.ctx.paths.profile {
            return;
        }
        while self.scenes.len() > 1 {
            self.apply(el, Next::Pop);
        }
        if self.ctx.switch_profile(&id) {
            // (the display settings are app-wide: the window stays)
            let select = crate::scene::select::SelectScene::new(&mut self.ctx);
            self.apply(el, Next::Replace(Box::new(select)));
        }
    }

    /// Discord and OBS: started or stopped as the settings say, and told
    /// the screen once it changed.
    fn poll_streaming(&mut self) {
        if std::mem::take(&mut self.activity_dirty) {
            if let Some(a) = self.scenes.iter().rev().find_map(|s| s.activity()) {
                self.ctx.streaming.set_activity(a);
            }
        }
        self.ctx.streaming.sync(&self.ctx.config);
    }

    fn poll_gamepads(&mut self, el: &ActiveEventLoop) {
        let mut toasts = Vec::new();
        let evs = (self.gamepads).poll(&self.ctx.config.keys.controllers, |m| toasts.push(m));
        for t in toasts {
            self.ctx.toast(t);
        }
        if let Some(c) = self.gamepads.take_connected() {
            self.ctx.input.pads = c;
        }
        for e in evs {
            self.dispatch(el, e);
        }
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        diag::set(self.ctx.config.display.show_latency);
        diag::frame(self.last_frame.replace(now), now);
        for (f, own) in self.fonts.poll(crate::i18n::current()) {
            self.text.add_font(f, own);
        }
        self.poll_gamepads(el);
        #[cfg(target_os = "ios")]
        crate::platform::ios::poll_keyboard();
        // (in this order a frame's toasts and config saves come as they always have)
        self.ctx.poll_jobs();
        crate::scene::skinned::settle(&mut self.ctx, Instant::now());
        self.ctx.poll_library();
        self.startup.poll(&mut self.ctx);
        self.poll_viewer(el);
        self.poll_profile(el);
        self.poll_streaming();
        if let Some(m) = self.ctx.audio.mixer.keep_alive() {
            self.ctx.toast(m);
        }
        let norm = crate::audio::loudness::system_target(&self.ctx.config.audio);
        self.ctx.audio.sounds.poll(&self.ctx.audio.mixer, norm);
        self.ctx.audio.loudness.poll();
        #[cfg(target_arch = "wasm32")]
        crate::app::web::show_page_buttons(!self.scenes.last().is_some_and(|s| s.full_screen()));
        if let Some(fs) = self.ctx.view.want_fullscreen.take() {
            self.window
                .set_fullscreen(fs.then_some(Fullscreen::Borderless(None)));
        }
        if std::mem::take(&mut self.ctx.view.want_present_mode) {
            self.gfx
                .set_present_mode(self.ctx.config.display.present_mode);
        }
        let next = match self.scenes.last_mut() {
            Some(top) => top.update(&mut self.ctx, &mut self.gfx, now),
            None => Next::Quit,
        };
        // scenes are drawn for when the frame will be shown, updated
        // (judged, sounds scheduled) for now
        let shown = self.pacer.frame(now, self.gfx.frames_ahead());
        self.apply(el, next);
        // in use again: heard from here (the scenes had their frame to stop
        // what played meanwhile), or a scene of play over or begun
        self.update_mute(true);
        #[cfg(target_os = "ios")]
        self.defer_edges();

        self.batch.clear();
        let mut d = Draw {
            gfx: &self.gfx,
            text: &mut self.text,
            b: &mut self.batch,
            pointer: self.ctx.input.pointer,
            text_input: false,
        };
        if let Some(top) = self.scenes.last_mut() {
            top.draw(&self.ctx, &mut d, shown);
        }
        // the on-screen keyboard (iOS, Android) while a text field is edited
        if d.text_input != self.ime_on {
            self.ime_on = d.text_input;
            if cfg!(mobile) {
                self.window.set_ime_allowed(self.ime_on);
            }
        }
        if let Some(n) = self.third_party_notice.as_mut() {
            n.draw(
                &self.ctx,
                &mut d,
                tr!("Third-party sources are on"),
                crate::net::hosts::third_party_notice(),
                tr!("Turn off"),
            );
        }
        self.downloads.draw(&self.ctx, &mut d);
        (self.ctx.toasts).retain(|(_, t)| now.duration_since(*t) < Duration::from_secs(4));
        draw_overlay(&self.ctx, &mut d, now);
        if std::mem::take(&mut self.ctx.view.want_screenshot) && self.shot_name.is_none() {
            if self.gfx.request_capture() {
                let label = self.scenes.last().map(|s| s.screenshot_label(&self.ctx));
                let name = crate::app::screenshot::file_name(
                    &crate::app::screenshot::stamp(),
                    &label.unwrap_or_default(),
                );
                self.shot_name = Some(name);
            } else {
                self.ctx
                    .toast(tr!("Screenshots: this GPU's screen cannot be read"));
            }
        }
        let waited = self
            .gfx
            .render(&self.batch, theme::BACKGROUND)
            .unwrap_or_else(|e| {
                log::warn!("render: {e}");
                Duration::ZERO
            });
        if self.vsync_paced() {
            self.next_frame = self.pacer.next_start(waited);
        }
        crate::skin::guard::frame_drawn();
        self.poll_screenshot();
        if self.first_frame {
            self.first_frame = false;
            diag::startup("first frame");
        }
        self.frame_count += 1;
        let el_t = now.duration_since(self.fps_since);
        if el_t >= Duration::from_millis(500) {
            self.ctx.view.fps = self.frame_count as f32 / el_t.as_secs_f32();
            self.frame_count = 0;
            self.fps_since = now;
        }
    }
}

/// `fonts.txt`'s lines: a file name, and its language's tag.
type FontList = Vec<(String, Option<String>)>;

/// Web build: fonts listed in `fonts/fonts.txt` next to index.html, a file
/// name a line. A line `file lang` (`NotoSansSC-Regular.otf zh-Hans`) is a
/// font of that UI language's, loaded only once the UI is in it.
#[derive(Default)]
struct WebFonts {
    /// The list (read once).
    list: Option<FontList>,
    list_job: Option<Job<FontList>>,
    /// Downloading: each font, and whether it is a language's own (drawn
    /// with before the others).
    jobs: Vec<Job<(Vec<u8>, bool)>>,
    /// The languages whose fonts were asked for (`None`: those of none).
    asked: Vec<Option<&'static str>>,
}

impl WebFonts {
    fn start() -> WebFonts {
        let list_job = crate::platform::IS_WEB.then(|| {
            Job::get("fonts/fonts.txt", |r| {
                let text = String::from_utf8_lossy(&r.unwrap_or_default()).into_owned();
                (text.lines().map(str::trim))
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .filter_map(|l| {
                        let mut words = l.split_whitespace();
                        Some((words.next()?.to_string(), words.next().map(str::to_string)))
                    })
                    .collect()
            })
        });
        WebFonts {
            list_job,
            ..Default::default()
        }
    }

    /// The fonts that came; those of none and of the UI language `lang`
    /// asked for once the list is read.
    fn poll(&mut self, lang: crate::i18n::Lang) -> Vec<(Vec<u8>, bool)> {
        if let Some(list) = Job::drain_slot(&mut self.list_job).pop() {
            self.list = Some(list);
        }
        if let Some(list) = &self.list {
            for tag in [None, Some(lang.tag())] {
                if self.asked.contains(&tag) {
                    continue;
                }
                self.asked.push(tag);
                for (name, _) in list.iter().filter(|(_, of)| of.as_deref() == tag) {
                    let url = format!("fonts/{name}");
                    self.jobs.push(Job::start(|tx, _| {
                        crate::platform::http_get(&url, move |r| {
                            if let Ok(b) = r {
                                let _ = tx.send((b, tag.is_some()));
                            }
                        })
                    }));
                }
            }
        }
        Job::drain_all(&mut self.jobs)
    }
}

/// Whether to say at start what third parties' copies are: they are on and
/// the player has not said OK to them (in the settings or here). Not
/// over a chart played from the command line.
fn third_party_notice_due(config: &Config, scenes: usize) -> bool {
    config.third_party_mirrors && !config.third_party_acknowledged && scenes == 1
}

/// The start's third-party notice answered: OK keeps them on (and is not
/// asked again), Turn off turns them off (the settings ask before they
/// are on again).
fn answer_third_party_notice(ctx: &mut Ctx, ok: bool) {
    if ok {
        ctx.config.third_party_acknowledged = true;
    } else {
        ctx.config.third_party_mirrors = false;
        ctx.net.hosts = crate::net::hosts::context(&ctx.config);
    }
    ctx.save_config();
}

fn draw_overlay(ctx: &Ctx, d: &mut Draw, now: Instant) {
    let ui = ctx.ui();
    let mut y = 12.0 * ui;
    for (msg, t) in &ctx.toasts {
        let age = now.duration_since(*t).as_secs_f32();
        let a = (4.0 - age).clamp(0.0, 1.0);
        let size = 18.0 * ui;
        let w = d.measure(msg, size) + 24.0 * ui;
        let r = Rect::new(ctx.view.width - w - 12.0 * ui, y, w, size + 14.0 * ui);
        d.b.rect(r, with_alpha(rgba(0x000000d0), a));
        d.b.rect(
            Rect::new(r.x, r.y, 3.0 * ui, r.h),
            with_alpha(theme::ACCENT, a),
        );
        d.text(
            msg,
            r.x + 12.0 * ui,
            r.y + 7.0 * ui,
            TextStyle::new(size, with_alpha(theme::TEXT, a), Align::Left),
        );
        y += r.h + 6.0 * ui;
    }
    diag::draw(ctx, d);
    if ctx.config.display.show_fps {
        let s = format!("{:.0} FPS", ctx.view.fps);
        d.text(
            &s,
            6.0,
            ctx.view.height - 22.0,
            TextStyle::new(16.0, theme::TEXT_DIM, Align::Left).shadow(),
        );
    }
}

impl App {
    fn init(&mut self, window: Arc<Window>, mut gfx: Gfx) {
        diag::startup("graphics");
        let config = (self.launch.config.take())
            .unwrap_or_else(|| crate::profiles::load_config(&self.paths));
        crate::i18n::apply(config.language);
        let text = Text::new(&[self.paths.fonts.as_path()]);
        let mixer = Mixer::new(&config.audio.mixer_config(512));
        diag::startup("audio");
        let size = {
            let (width, height) = surface_size(&window);
            // the renderer started from winit's idea of the size
            gfx.resize(width, height);
            winit::dpi::PhysicalSize::new(width, height)
        };
        let screen = crate::platform::screen::detect(&window);
        let scale = window.scale_factor() as f32;
        log::info!(
            "screen {}x{} px, about {:.1}\" ({} points per inch, guessed)",
            screen.size_px.0,
            screen.size_px.1,
            screen.guessed_inches(scale),
            screen.points_per_inch
        );
        let view = crate::ctx::View::new(size, scale, screen, &gfx.adapter_name);
        // this launch's options (not saved)
        let overrides = std::mem::take(&mut self.launch.overrides);
        for l in overrides.describe() {
            log::info!("command line: {l}");
        }
        let profiles = std::mem::take(&mut self.profiles);
        let mut ctx = Ctx::new(self.paths.clone(), profiles, config, overrides, mixer, view);
        // the graphics API chosen did not start: the one that did is kept
        // (saved below), and said once
        if let Some(f) = gfx.fallback.take() {
            log::error!(
                "gpu: {} could not start ({}); {} is used and kept",
                f.wanted.label(),
                f.error,
                f.used.label()
            );
            ctx.config.display.backend = f.used;
            ctx.toast(tr!(
                "Graphics: {wanted} could not start, {used} is used instead \
                 (SETTINGS › DISPLAY & AUDIO › ADVANCED)",
                wanted = f.wanted.label(),
                used = f.used.label()
            ));
        }
        ctx.open_records();
        ctx.reload_library();
        diag::startup("library");
        let startup = startup::Startup::begin(&mut ctx);
        crate::scene::skinned::recover(&mut ctx);
        ctx.save_config(); // writes defaults on first launch
        let select = crate::scene::select::SelectScene::new(&mut ctx);
        let mut scenes: Vec<Box<dyn Scene>> = vec![Box::new(select)];
        scenes.extend(cli::play_scene(&mut ctx, &self.launch.cli));
        let third_party_notice =
            third_party_notice_due(&ctx.config, scenes.len()).then(crate::ui::Notice::default);
        diag::startup("song select");
        #[cfg(target_os = "ios")]
        {
            let proxy = self.proxy.clone();
            crate::platform::ios::install(move || {
                let _ = proxy.send_event(UserEvent::Keys);
            });
        }
        #[cfg(target_os = "android")]
        crate::platform::android::hide_system_bars();
        let now = Instant::now();
        let refresh_hz = refresh_hz(&window);
        log::info!("display: {refresh_hz:.1} Hz");
        let mut pacer = pacing::Pacer::default();
        pacer.set_refresh(refresh_hz);
        self.run = Some(Running {
            window,
            gfx,
            text,
            batch: Batch::default(),
            ctx,
            startup,
            scenes,
            third_party_notice,
            gamepads: crate::input::gamepad::Gamepads::open(),
            activity_dirty: true,
            last_touch: None,
            #[cfg(target_arch = "wasm32")]
            touches: Default::default(),
            fonts: WebFonts::start(),
            shot_name: None,
            shot_job: None,
            mouse_down: false,
            cursor: (0.0, 0.0),
            frame_count: 0,
            fps_since: now,
            last_frame: None,
            first_frame: true,
            next_frame: now,
            pacer,
            refresh_hz,
            redraw_pending: false,
            ime_on: false,
            downloads: Default::default(),
            viewer: (self.launch.cli.viewer)
                .then(|| viewer::Server::start(viewer::PORT))
                .flatten(),
            activity: Default::default(),
            #[cfg(desktop)]
            minimized_check: now,
            #[cfg(target_os = "ios")]
            edges_deferred: false,
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
            #[cfg(target_os = "ios")]
            UserEvent::Keys => {
                if let Some(r) = self.run.as_mut() {
                    r.ios_keys(el);
                }
            }
            #[cfg(target_arch = "wasm32")]
            UserEvent::PageFocus(focused) => {
                if let Some(r) = self.run.as_mut() {
                    r.set_activity(|a| a.focused = focused);
                }
            }
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        // (iOS: active again; Android: a surface again)
        if let Some(r) = self.run.as_mut() {
            r.set_activity(|a| a.suspended = false);
            crate::library::going_on(&r.ctx.paths.songs_cache());
            crate::skin::guard::away(false);
        }
        if self.run.is_some() || self.creating {
            return;
        }
        let paths = &self.paths;
        let config =
            (self.launch.config).get_or_insert_with(|| crate::profiles::load_config(paths));
        let mut attrs = Window::default_attributes().with_title("vibeBMS");
        // a phone or tablet window fills the screen (a size given here
        // would be used as is, and the screen would show part of it)
        #[cfg(not(mobile))]
        {
            attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(
                config.display.width.max(320),
                config.display.height.max(240),
            ));
        }
        if config.display.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            attrs = attrs.with_window_icon(window_icon());
        }
        // a game: the whole screen, without the status bar and with the
        // home indicator out of sight until the screen is touched (edge
        // gestures: `defer_edges`)
        #[cfg(target_os = "ios")]
        {
            use winit::platform::ios::WindowAttributesExtIOS;
            attrs = attrs
                .with_prefers_status_bar_hidden(true)
                .with_prefers_home_indicator_hidden(true);
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
        diag::startup("window");
        let mode = config.display.present_mode;
        #[cfg(not(target_arch = "wasm32"))]
        let backend = config.display.backend;
        self.creating = true;
        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let g = Gfx::create(window.clone(), mode).await;
                let _ = proxy.send_event(UserEvent::GfxReady(window, Box::new(g)));
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        match Gfx::new(window.clone(), mode, backend) {
            Ok(g) => self.init(window, g),
            Err(e) => {
                log::error!("graphics init failed: {e}");
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(r) = self.run.as_mut() else { return };
        if r.downloads.window_event(id, &event) {
            return;
        }
        r.window_event(el, event);
    }

    /// (Android: the app put in the background; iOS: inactive; the web:
    /// the page left)
    fn suspended(&mut self, _el: &ActiveEventLoop) {
        if let Some(r) = self.run.as_mut() {
            r.set_activity(|a| a.suspended = true);
            // (the system may end the app from here without a word)
            crate::library::stopping(&r.ctx.paths.songs_cache());
            crate::skin::guard::away(true);
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(r) = self.run.as_mut() else { return };
        #[cfg(target_arch = "wasm32")]
        r.web_touches(el);
        #[cfg(desktop)]
        r.check_minimized(Instant::now());
        if let Some(pages) = r.ctx.net.take_pages() {
            r.downloads.browse(el, &r.window, &mut r.ctx, pages);
        }
        r.downloads.poll(&mut r.ctx);
        let browsing = r.downloads.is_open();
        r.set_activity(|a| a.browsing = browsing);
        let realtime = r.scenes.last().is_some_and(|s| s.realtime());
        let fps = r.ctx.config.display.max_fps;
        let now = Instant::now();
        if crate::platform::IS_WEB && fps == 0 {
            // the browser paces the redraws (requestAnimationFrame); Poll
            // there makes winit post a task again and again between frames
            // (Firefox: ~34000 a second, each with an AbortController), which
            // takes the main thread and leaves garbage for second-long GCs.
            // A redraw asked for wakes the loop at once too: ask once a frame.
            el.set_control_flow(ControlFlow::Wait);
            if !r.redraw_pending {
                r.redraw();
            }
        } else if r.vsync_paced() {
            // the next frame when its image is about to be free
            // (`Running::frame`), waiting for input meanwhile rather than
            // in the GPU's queue
            if now >= r.next_frame {
                r.redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(r.next_frame));
        } else if fps == 0 && realtime {
            // play without vsync: as many frames as the GPU makes
            el.set_control_flow(ControlFlow::Poll);
            r.window.request_redraw();
        } else {
            // the user's cap, or in menus the display's rate
            let cap = match fps {
                0 if r.refresh_hz > 0.0 => r.refresh_hz,
                0 => 120.0,
                f => f as f64,
            };
            if now >= r.next_frame && !r.redraw_pending {
                r.next_frame = (r.next_frame + Duration::from_secs_f64(1.0 / cap)).max(now);
                r.redraw()
            }
            el.set_control_flow(ControlFlow::WaitUntil(r.next_frame));
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        if let Some(r) = self.run.as_mut() {
            r.ctx.save_config();
            crate::library::stopping(&r.ctx.paths.songs_cache());
        }
        // "Restart now" after an update (scores are saved as they are made)
        crate::update::relaunch_if_asked();
    }
}

/// The refresh rate (Hz) of the window's display, 0 when it is not known;
/// on Android the highest the display has, asked for.
fn refresh_hz(window: &Window) -> f64 {
    // (winit does not know it there)
    #[cfg(target_os = "android")]
    if let Some(hz) = crate::platform::android::use_highest_refresh_rate() {
        return hz;
    }
    window
        .current_monitor()
        .and_then(|m| m.refresh_rate_millihertz())
        .map_or(0.0, |m| m as f64 / 1000.0)
}

/// Size to render at, in device pixels (as pointer positions are). On the
/// web winit can report the canvas size in CSS pixels, which on high-DPI
/// screens (iPad, phones) put every touch in the wrong place and rendered
/// at half resolution: there it is the canvas' CSS size times the device
/// pixel ratio, and the canvas' drawing buffer is resized to it.
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
    let s = crate::platform::screen::drawable_size(window);
    (s.width.max(1), s.height.max(1))
}

/// The app icon (`tools/gen_logo.py`) for the window and task bar.
#[cfg(not(target_arch = "wasm32"))]
fn window_icon() -> Option<winit::window::Icon> {
    let img = image::load_from_memory(include_bytes!("../../assets/icon.png"))
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn window_icon_decodes() {
        assert!(super::window_icon().is_some());
    }

    /// Third parties' copies on without the player's OK (a build that has
    /// them on): said once at start; OK is kept, Turn off turns them off.
    #[test]
    fn a_build_with_third_parties_on_says_so_once() {
        use super::*;
        let root = std::env::temp_dir().join(format!("vibebms-tp-notice-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        ctx.config.third_party_mirrors = true;
        ctx.config.third_party_acknowledged = false;
        assert!(third_party_notice_due(&ctx.config, 1));
        // not over a chart played from the command line
        assert!(!third_party_notice_due(&ctx.config, 2));
        answer_third_party_notice(&mut ctx, true);
        assert!(ctx.config.third_party_mirrors && ctx.config.third_party_acknowledged);
        assert!(!third_party_notice_due(&ctx.config, 1));
        assert!(crate::profiles::load_config(&ctx.paths).third_party_acknowledged);
        // Turn off
        ctx.config.third_party_acknowledged = false;
        answer_third_party_notice(&mut ctx, false);
        assert!(!ctx.config.third_party_mirrors && !ctx.net.hosts.config.third_party);
        assert!(!third_party_notice_due(&ctx.config, 1));
        // off, or said OK to: nothing to say
        let mut c = Config {
            third_party_mirrors: false,
            ..Default::default()
        };
        assert!(!third_party_notice_due(&c, 1));
        (c.third_party_mirrors, c.third_party_acknowledged) = (true, true);
        assert!(!third_party_notice_due(&c, 1));
        std::fs::remove_dir_all(&root).ok();
    }
}
