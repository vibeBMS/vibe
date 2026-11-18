//! Song select: folders, difficulty tables, courses, search, options.

mod genpreview;
mod input;
mod launch;
mod options;
mod preview;
mod reader;
mod skin;
mod status;
mod tree;

pub use preview::{resolve_image, PREVIEW_KEY};
pub(crate) use skin::SelectState;
pub(crate) use tree::{Bar, Node};

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::Bpms;
use crate::gfx::{Gfx, Rect};
use crate::input::{AxisStepper, InputEv, Nav, Repeat};
use crate::library::Song;
use crate::platform::{Instant, Job};
use crate::scene::skin_host::SkinHost;
use crate::scene::skinned;
use crate::skin::SkinType;
use crate::ui::{HitMap, TextInput};
use input::{Drag, PanelKey};
use preview::Rank;
use skin::{SelectHooks, PANELS};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use tree::Distribution;

/// The BPMs hi-speeds are shown and set for without a chart (beatoraja's
/// 150).
pub(crate) const REFERENCE_BPMS: Bpms = Bpms {
    start: 150.0,
    min: 150.0,
    max: 150.0,
    main: 150.0,
    now: None,
};

/// Time constant (s) of the list easing toward the cursor: 30 % of the
/// way per frame at 60 Hz, as it went by frames before.
const SCROLL_TAU: f32 = 0.0467;

pub struct SelectScene {
    version: u64,
    /// Web: the kept song folder a first Delete press asked about.
    #[cfg(target_arch = "wasm32")]
    delete_armed: crate::ui::Armed<PathBuf>,
    /// The turntable scrolls the list.
    axis: AxisStepper,
    node: Node,
    stack: Vec<(Node, usize)>,
    pub(crate) bars: Vec<Bar>,
    pub(crate) cursor: usize,
    /// The list's position in bars (eases toward `cursor`, wraps around).
    scroll: f32,
    /// When `scroll` last eased toward the cursor.
    scrolled_at: Instant,
    search_edit: Option<TextInput>,
    /// READ TEXT's text, open.
    reader: Option<reader::Reader>,
    bar_rects: HitMap<usize>,
    /// The skin's distance between bars on screen (`SelectHooks::pitch`).
    bar_pitch: Option<f32>,
    drag: Option<Drag>,
    last_move: Instant,
    /// Up / down held: the cursor moves on.
    held_nav: Option<Repeat<Nav>>,
    md5_map: HashMap<String, usize>,
    sha_map: HashMap<String, usize>,
    /// The library's charts by the folder of their file (a song's charts).
    song_dirs: HashMap<String, Vec<usize>>,
    /// The selected chart's replay and text files.
    files: RefCell<status::ChartFiles>,
    preview_for: Option<String>,
    /// A preview played once ends then (the select BGM again).
    preview_until: Option<Instant>,
    /// The chart waits for a preview made of its sounds.
    preview_wait: bool,
    preview_job: Option<Job<Option<genpreview::Preview>>>,
    /// Previews made of charts' sounds (`genpreview`).
    generated: genpreview::Generated,
    /// The preview's file being read (the web: from the browser's storage)
    /// before it is decoded.
    preview_fetch: Option<(crate::platform::vfs::Prefetch, PathBuf)>,
    banner: Option<(String, Option<crate::gfx::ImageTex>)>,
    /// The banner's image while it is decoded.
    banner_job: Option<crate::skin::render::ImageJobs>,
    ranking: HashMap<String, Rank>,
    quit_armed: Option<Instant>,
    host: SkinHost,
    /// The sound set's BGM is playing; the panel its sounds last saw.
    bgm_on: bool,
    sound_panel: usize,
    /// Open skin panel (1..=`PANELS`) and when panels last opened / closed.
    panel: usize,
    panel_on: [Option<Instant>; PANELS],
    panel_off: [Option<Instant>; PANELS],
    /// The focused tile of the open panel (`skin::PANEL_TILES`).
    focus: usize,
    /// START / SELECT held: the option panels (see `option_panel`).
    start_held: bool,
    select_held: bool,
    /// Number key 5 held: the detail panel, as START + SELECT.
    num5_held: bool,
    /// A held panel key that repeats.
    panel_repeat: Option<Repeat<PanelKey>>,
    /// Lamp / rank counts per folder, counted when first shown.
    dist_cache: RefCell<HashMap<Node, Distribution>>,
}

impl SelectScene {
    pub fn new(ctx: &mut Ctx) -> SelectScene {
        let mut s = SelectScene {
            version: u64::MAX,
            #[cfg(target_arch = "wasm32")]
            delete_armed: Default::default(),
            axis: AxisStepper::default(),
            node: Node::Root,
            stack: Vec::new(),
            bars: Vec::new(),
            cursor: 0,
            scroll: 0.0,
            scrolled_at: Instant::now(),
            search_edit: None,
            reader: None,
            bar_rects: HitMap::default(),
            bar_pitch: None,
            drag: None,
            last_move: Instant::now(),
            held_nav: None,
            md5_map: HashMap::new(),
            sha_map: HashMap::new(),
            song_dirs: HashMap::new(),
            files: Default::default(),
            preview_for: None,
            preview_until: None,
            preview_wait: false,
            preview_job: None,
            generated: Default::default(),
            preview_fetch: None,
            banner: None,
            banner_job: None,
            ranking: HashMap::new(),
            quit_armed: None,
            host: SkinHost::new(),
            bgm_on: false,
            sound_panel: 0,
            panel: 0,
            panel_on: [None; PANELS],
            panel_off: [None; PANELS],
            focus: 0,
            start_held: false,
            select_held: false,
            num5_held: false,
            panel_repeat: None,
            dist_cache: Default::default(),
        };
        s.rebuild(ctx);
        ctx.select_entered();
        s
    }

    /// The library or the tables changed: the lists again.
    fn rebuild(&mut self, ctx: &Ctx) {
        self.version = ctx.lib.version;
        self.dist_cache.borrow_mut().clear();
        self.md5_map.clear();
        self.sha_map.clear();
        self.song_dirs.clear();
        *self.files.borrow_mut() = Default::default();
        for (i, s) in ctx.lib.songs.iter().enumerate() {
            if !s.md5.is_empty() {
                self.md5_map.insert(s.md5.clone(), i);
            }
            self.sha_map.insert(s.sha256.clone(), i);
            self.song_dirs
                .entry(status::song_dir(s))
                .or_default()
                .push(i);
        }
        let key = self.bars.get(self.cursor).map(|b| self.bar_key(ctx, b));
        self.bars = self.listing(ctx, &self.node);
        // keep the cursor on the same item if possible
        self.cursor = key
            .and_then(|k| self.bars.iter().position(|b| self.bar_key(ctx, b) == k))
            .unwrap_or(self.cursor.min(self.bars.len().saturating_sub(1)));
    }

    fn enter(&mut self, ctx: &mut Ctx, node: Node) {
        let bars = self.listing(ctx, &node);
        if bars.is_empty() {
            ctx.toast("Empty folder");
            return;
        }
        self.stack
            .push((std::mem::replace(&mut self.node, node), self.cursor));
        ctx.play_sound(crate::sysound::Sound::FolderOpen);
        self.bars = bars;
        // the folder's own bar (LR2: deciding on it closes the folder)
        self.cursor = 0;
        self.scroll = 0.0;
        self.moved(ctx);
    }

    fn back(&mut self, ctx: &mut Ctx) -> bool {
        let Some((n, c)) = self.stack.pop() else {
            return false;
        };
        ctx.play_sound(crate::sysound::Sound::FolderClose);
        self.node = n;
        self.bars = self.listing(ctx, &self.node);
        self.cursor = c.min(self.bars.len().saturating_sub(1));
        self.scroll = self.cursor as f32;
        self.moved(ctx);
        true
    }

    /// The cursor moved: the preview of the bar left stops.
    fn moved(&mut self, ctx: &mut Ctx) {
        self.last_move = Instant::now();
        if self.preview_for.take().is_some() {
            ctx.audio.mixer.stop(preview::PREVIEW_KEY);
        }
        self.preview_until = None;
        self.preview_wait = false;
        self.preview_job = None;
        self.preview_fetch = None;
    }

    fn step(&mut self, ctx: &mut Ctx, d: i32) {
        if self.bars.is_empty() {
            return;
        }
        let n = self.bars.len() as i32;
        self.cursor = (self.cursor as i32 + d).rem_euclid(n) as usize;
        if d != 0 {
            ctx.play_sound(crate::sysound::Sound::Scratch);
        }
        self.moved(ctx);
    }

    fn selected_song<'a>(&self, ctx: &'a Ctx) -> Option<(usize, &'a Song)> {
        match self.bars.get(self.cursor) {
            Some(Bar::Song(i)) => ctx.lib.songs.get(*i).map(|s| (*i, s)),
            _ => None,
        }
    }

    /// The selected chart's BPMs, else [`REFERENCE_BPMS`].
    fn bpms(&self, ctx: &Ctx) -> Bpms {
        self.selected_song(ctx)
            .map_or(REFERENCE_BPMS, |(_, s)| Bpms::of_song(s))
    }

    /// The list wraps around: scroll the short way to the cursor, the same
    /// share of the way per second at any frame rate.
    fn ease_scroll(&mut self, now: Instant) {
        let n = self.bars.len().max(1) as f32;
        let target = self.cursor as f32;
        let mut diff = (target - self.scroll).rem_euclid(n);
        if diff > n * 0.5 {
            diff -= n;
        }
        let dt = now
            .saturating_duration_since(self.scrolled_at)
            .as_secs_f32();
        self.scrolled_at = now;
        if diff.abs() < 0.01 {
            self.scroll = target;
        } else {
            let k = 1.0 - (-dt / SCROLL_TAU).exp();
            self.scroll = (self.scroll + diff * k).rem_euclid(n);
        }
    }

    fn draw_skinned(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let view = Rect::new(0.0, 0.0, ctx.view.width, ctx.view.height);
        let (bars, pitch) = {
            let st = SelectState::new(self, ctx);
            let mut hooks = SelectHooks {
                st: &st,
                banner: self
                    .banner
                    .as_ref()
                    .and_then(|b| b.1.as_ref())
                    .map(|t| t.tex),
                bars: HitMap::default(),
                pitch: None,
            };
            self.host.draw(d, view, now, &st, &mut hooks);
            (hooks.bars, hooks.pitch)
        };
        self.bar_rects = bars;
        self.bar_pitch = pitch;
        // search box (typing is not part of skins), in the upper half,
        // above an on-screen keyboard
        if let Some(t) = self.search_edit.as_mut() {
            let (w, h, ui) = (ctx.view.width, ctx.view.height, ctx.ui());
            let r = Rect::new(w * 0.25, h * 0.22, w * 0.5, 44.0 * ui);
            t.draw_box(ctx, d, r, "SEARCH: ");
        }
        self.draw_reader(ctx, d);
    }
}

impl Scene for SelectScene {
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        if self.version != ctx.lib.version {
            self.rebuild(ctx);
        }
        // a newer nightly was found (once)
        if let Some(n) = ctx.net.updater.take_notice() {
            ctx.toast(n);
        }
        self.repeat_keys(ctx, now);
        self.ease_scroll(now);
        // the app not in use and silent: the preview stops, to start again
        // from its beginning once it is heard (`app::activity`)
        if self.preview_for.is_some() && ctx.audio.mute.faded(now) {
            self.moved(ctx);
        }
        self.update_preview(ctx);
        self.update_sounds(ctx);
        self.update_ranking(ctx);
        self.update_reader();
        self.update_banner(ctx, gfx);
        if self.host.first_update() {
            let skin = {
                let st = SelectState::new(self, ctx);
                let stat = |id: i32| st.static_option(id);
                skinned::load(ctx, gfx, SkinType::Select, &stat, &st)
            };
            self.host.skin = skin;
        }
        // events skin scripts asked for
        for ev in self.host.tick(gfx) {
            let next = self.skin_event(ctx, ev, 1);
            if !matches!(next, Next::Stay) {
                return next;
            }
        }
        Next::Stay
    }

    fn realtime(&self) -> bool {
        self.host.skin.is_some()
    }

    /// The selected chart, its lamp and EX score.
    fn screenshot_label(&self, ctx: &Ctx) -> Vec<String> {
        let Some((_, s)) = self.selected_song(ctx) else {
            return Vec::new();
        };
        let mut v = vec![s.title.clone(), crate::screenshot::difficulty(s.difficulty)];
        if let Some(b) = ctx.best(s, ctx.config.play.ln_pref()) {
            v.extend([b.clear.label().to_string(), b.ex_score.to_string()]);
        }
        v
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        // the library read again since the last frame (an earlier input
        // rescanned): the bars' song indices are of the old list
        if self.version != ctx.lib.version {
            self.rebuild(ctx);
        }
        self.input_event(ctx, ev)
    }

    fn resumed(&mut self, ctx: &mut Ctx) {
        // the BGM again (a screen over this one may have stopped it), of
        // the set drawn again if it is drawn at random
        ctx.select_entered();
        self.bgm_on = false;
        // scores may have changed (rebuild forgets the counts)
        self.rebuild(ctx);
        self.moved(ctx);
    }

    fn exit(&mut self, ctx: &mut Ctx, gfx: &mut Gfx) {
        ctx.audio.mixer.stop(preview::PREVIEW_KEY);
        self.stop_bgm(ctx);
        self.host.free(gfx);
        if let Some((_, Some(t))) = self.banner.take() {
            t.free(gfx);
        }
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        self.draw_skinned(ctx, d, now);
    }
}
