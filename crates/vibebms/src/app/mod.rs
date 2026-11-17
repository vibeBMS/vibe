//! Window, event loop, shared context and the scene stack.

mod cli;
mod ctx;

pub use ctx::{Ctx, TableMsg};

use crate::config::Config;
use crate::gfx::text::{Align, Text};
use crate::gfx::{rgba, with_alpha, Batch, Gfx, Rect, TexId};
use crate::input::{InputEv, Phase};
use crate::library::Library;
use crate::paths::Paths;
use crate::platform::{Duration, Instant, Job};
use crate::theme;
use std::sync::Arc;
use vibebms_audio::{AudioConfig, Mixer};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
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
    /// Where the mouse is ([`ctx::Input::pointer`]), for skins' hover images.
    pub pointer: Option<(f32, f32)>,
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

/// Songs are downloaded in an in-app browser (desktop, Android, iOS).
pub const IN_APP_BROWSER: bool = cfg!(in_app_browser);

struct Running {
    window: Arc<Window>,
    gfx: Gfx,
    text: Text,
    batch: Batch,
    ctx: Ctx,
    scenes: Vec<Box<dyn Scene>>,
    gamepads: crate::input::gamepad::Gamepads,
    last_touch: Option<Instant>,
    /// The page's fingers down and where they last were (web).
    #[cfg(target_arch = "wasm32")]
    touches: std::collections::HashMap<u64, (f32, f32)>,
    /// Fonts downloaded in the background (web build).
    font_job: Option<Job<Vec<u8>>>,
    mouse_down: bool,
    cursor: (f32, f32),
    frame_count: u32,
    fps_since: Instant,
    next_frame: Instant,
    /// A redraw asked for and not drawn yet (the web: asking again would
    /// wake the event loop again at once, and it would spin).
    redraw_pending: bool,
    /// The on-screen keyboard is asked for.
    ime_on: bool,
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
    event_loop.set_control_flow(if crate::platform::IS_WEB {
        ControlFlow::Wait
    } else {
        ControlFlow::Poll
    });
    #[allow(unused_mut)]
    let mut app = App::new(paths, event_loop.create_proxy());
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        crate::web::install_file_input(app.paths.root.clone());
        crate::web::install_input_stamps();
        crate::web::install_touch_watch();
        crate::web::install_touch_input();
        // the songs kept in the browser: mounted, then read as picked ones
        // (the first scan waits for them)
        let replays = crate::replay::dir(&app.paths.root);
        let skins = app.paths.skins.clone();
        crate::platform::opfs::start(&app.paths.root, crate::platform::vfs::mount, move |n| {
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
        let ctx = &mut self.ctx;
        for e in self.gamepads.poll(|m| ctx.toast(m)) {
            self.dispatch(el, e);
        }
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        for f in Job::drain_slot(&mut self.font_job) {
            self.text.add_font(f);
        }
        self.poll_gamepads(el);
        self.ctx.poll_jobs();
        self.ctx.audio.mixer.keep_alive();
        self.ctx.audio.sounds.poll(&self.ctx.audio.mixer);
        #[cfg(target_arch = "wasm32")]
        crate::web::show_page_buttons(!self.scenes.last().is_some_and(|s| s.full_screen()));
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
        self.apply(el, next);

        self.batch.clear();
        let mut d = Draw {
            gfx: &self.gfx,
            text: &mut self.text,
            b: &mut self.batch,
            pointer: self.ctx.input.pointer,
        };
        self.ctx.input.text_input = false;
        if let Some(top) = self.scenes.last_mut() {
            top.draw(&mut self.ctx, &mut d, now);
        }
        // the on-screen keyboard (iOS, Android) while a text field is edited
        if self.ctx.input.text_input != self.ime_on {
            self.ime_on = self.ctx.input.text_input;
            if cfg!(mobile) {
                self.window.set_ime_allowed(self.ime_on);
            }
        }
        draw_overlay(&mut self.ctx, &mut d, now);
        if let Err(e) = self.gfx.render(&self.batch, theme::BACKGROUND) {
            log::warn!("render: {e}");
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

/// Web build: fonts listed in `fonts/fonts.txt` next to index.html.
fn web_fonts() -> Option<Job<Vec<u8>>> {
    if !crate::platform::IS_WEB {
        return None;
    }
    Some(Job::start(|tx, _| {
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
    }))
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
            size,
            with_alpha(theme::TEXT, a),
            Align::Left,
        );
        y += r.h + 6.0 * ui;
    }
    if ctx.config.display.show_fps {
        let s = format!("{:.0} FPS", ctx.view.fps);
        d.text_shadow(
            &s,
            6.0,
            ctx.view.height - 22.0,
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
        crate::hosts::set_third_party(config.third_party_mirrors);
        crate::hosts::set_folder_naming(config.name_song_folders);
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
        let view = ctx::View {
            width: size.width as f32,
            height: size.height as f32,
            scale,
            screen,
            fps: 0.0,
            gpu_name: gfx.adapter_name.clone(),
            want_fullscreen: None,
            want_present_mode: false,
        };
        let mut ctx = Ctx::new(self.paths.clone(), config, mixer, view);
        ctx.lib.db = Some(Library::open(&ctx.paths.cache, &ctx.paths.scores));
        ctx.reload_library();
        ctx.load_cached_tables();
        // the web build's first scan comes once the songs kept in the
        // browser are mounted
        if !crate::platform::IS_WEB {
            ctx.start_scan();
        }
        ctx.refresh_tables(false);
        ctx.tables.links.start(&ctx.paths.cache);
        ctx.net.archives = crate::archive::Archives::load(&ctx.paths, &ctx.config.archives);
        ctx.net.archives.refresh(false);
        ctx.skins.store = crate::skinstore::SkinStore::load(&ctx.paths);
        ctx.skins.store.refresh(false);
        // a newer nightly (desktop CI builds), in the background
        ctx.net.updater.check(ctx.config.check_updates);
        // tests: open the download browser on this page at start
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(url) = std::env::var("VIBEBMS_TEST_BROWSE") {
            ctx.net.browse = Some(vec![url]);
        }
        ctx.save_config(); // writes defaults on first launch
        let select = crate::scene::select::SelectScene::new(&mut ctx);
        let mut scenes: Vec<Box<dyn Scene>> = vec![Box::new(select)];
        scenes.extend(cli::play_scene(&mut ctx));
        let now = Instant::now();
        self.run = Some(Running {
            window,
            gfx,
            text,
            batch: Batch::default(),
            ctx,
            scenes,
            gamepads: crate::input::gamepad::Gamepads::open(),
            last_touch: None,
            #[cfg(target_arch = "wasm32")]
            touches: Default::default(),
            font_job: web_fonts(),
            mouse_down: false,
            cursor: (0.0, 0.0),
            frame_count: 0,
            fps_since: now,
            next_frame: now,
            redraw_pending: false,
            ime_on: false,
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
        if r.downloads.window_event(id, &event) {
            return;
        }
        // (the web: when the browser got the event, not when this runs)
        #[cfg(target_arch = "wasm32")]
        let now = crate::web::input_time();
        #[cfg(not(target_arch = "wasm32"))]
        let now = Instant::now();
        match event {
            WindowEvent::CloseRequested => {
                r.ctx.save_config();
                el.exit();
            }
            WindowEvent::Resized(_) => {
                let (w, h) = surface_size(&r.window);
                r.gfx.resize(w, h);
                r.ctx.view.width = w as f32;
                r.ctx.view.height = h as f32;
                r.ctx.view.scale = r.window.scale_factor() as f32;
                r.ctx.view.screen = crate::platform::screen::detect(&r.window);
            }
            WindowEvent::RedrawRequested => {
                r.redraw_pending = false;
                r.frame(el)
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    // an on-screen keyboard's characters (iOS) have no key
                    if event.state == ElementState::Pressed {
                        if let Some(t) = event.text.as_ref().filter(|t| !t.is_empty()) {
                            r.dispatch(el, InputEv::Text(t.to_string()));
                        }
                    }
                    return;
                };
                let pressed = event.state == ElementState::Pressed;
                if pressed && !event.repeat && (code == KeyCode::F11) {
                    r.ctx.config.display.fullscreen = !r.ctx.config.display.fullscreen;
                    r.ctx.view.want_fullscreen = Some(r.ctx.config.display.fullscreen);
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
            WindowEvent::Ime(winit::event::Ime::Commit(t)) if !t.is_empty() => {
                r.dispatch(el, InputEv::Text(t))
            }
            WindowEvent::CursorLeft { .. } => r.ctx.input.pointer = None,
            WindowEvent::CursorMoved { position, .. } => {
                r.cursor = (position.x as f32, position.y as f32);
                r.ctx.input.pointer = Some(r.cursor);
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
            // the page reads touches itself (`web::install_touch_input`)
            #[cfg(target_arch = "wasm32")]
            WindowEvent::Touch(_) => {}
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::Touch(t) => {
                r.ctx.input.touch_seen = true;
                r.last_touch = Some(now);
                use winit::event::TouchPhase;
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
            WindowEvent::Focused(false) => {
                r.ctx.save_config();
                // its key and touch releases go elsewhere now
                r.dispatch(
                    el,
                    InputEv::Released {
                        time: now,
                        touch_only: false,
                    },
                );
            }
            WindowEvent::Occluded(true) => r.dispatch(
                el,
                InputEv::Released {
                    time: now,
                    touch_only: false,
                },
            ),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(r) = self.run.as_mut() else { return };
        #[cfg(target_arch = "wasm32")]
        for t in crate::web::take_touches() {
            match t {
                crate::web::TouchIn::Point {
                    id,
                    phase,
                    x,
                    y,
                    time,
                } => {
                    r.ctx.input.touch_seen = true;
                    r.last_touch = Some(Instant::now());
                    if phase == Phase::Up {
                        r.touches.remove(&id);
                    } else {
                        r.touches.insert(id, (x, y));
                    }
                    r.dispatch(
                        el,
                        InputEv::Pointer {
                            id,
                            phase,
                            x,
                            y,
                            time,
                            touch: true,
                        },
                    );
                }
                // fingers held here that are no longer on the screen
                crate::web::TouchIn::Left(left) => {
                    let lost: Vec<(u64, (f32, f32))> = r
                        .touches
                        .iter()
                        .filter(|(id, _)| !left.contains(id))
                        .map(|(id, p)| (*id, *p))
                        .collect();
                    for (id, (x, y)) in lost {
                        r.touches.remove(&id);
                        r.dispatch(
                            el,
                            InputEv::Pointer {
                                id,
                                phase: Phase::Up,
                                x,
                                y,
                                time: Instant::now(),
                                touch: true,
                            },
                        );
                    }
                }
            }
        }
        // no finger on the screen: keys still held lost their release
        #[cfg(target_arch = "wasm32")]
        if let Some(time) = crate::web::take_touches_gone() {
            r.dispatch(
                el,
                InputEv::Released {
                    time,
                    touch_only: true,
                },
            );
        }
        if let Some(pages) = r.ctx.net.browse.take() {
            r.downloads.browse(el, &r.window, &mut r.ctx, pages);
        }
        r.downloads.poll(&mut r.ctx);
        let realtime = r.scenes.last().map(|s| s.realtime()).unwrap_or(false);
        let fps = r.ctx.config.display.max_fps;
        let now = Instant::now();
        if fps > 0 || !realtime {
            // menus: cap at the display rate (or 120) to save power; play: user cap
            let cap = if fps > 0 { fps } else { 120 };
            let interval = Duration::from_secs_f64(1.0 / cap as f64);
            if now >= r.next_frame && !r.redraw_pending {
                r.next_frame = (r.next_frame + interval).max(now);
                r.redraw()
            }
            el.set_control_flow(ControlFlow::WaitUntil(r.next_frame));
        } else if crate::platform::IS_WEB {
            // the browser paces the redraws (requestAnimationFrame); Poll
            // there makes winit post a task again and again between frames
            // (Firefox: ~34000 a second, each with an AbortController), which
            // takes the main thread and leaves garbage for second-long GCs.
            // A redraw asked for wakes the loop at once too: ask once a frame.
            el.set_control_flow(ControlFlow::Wait);
            if !r.redraw_pending {
                r.redraw();
            }
        } else {
            el.set_control_flow(ControlFlow::Poll);
            r.window.request_redraw();
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        if let Some(r) = self.run.as_mut() {
            r.ctx.save_config();
        }
        // "Restart now" after an update (scores are saved as they are made)
        crate::update::relaunch_if_asked();
    }
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
}
