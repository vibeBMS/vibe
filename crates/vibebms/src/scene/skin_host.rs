//! What every skinned screen does with its skin: load it once, load its
//! textures as they are wanted, run its scripts' events, fade out, and
//! hit-test its clickable objects.

use super::skinvals::ms_since;
use crate::app::Draw;
use crate::gfx::{Gfx, Rect, TexId};
use crate::platform::Instant;
use crate::skin::render::{Hooks, LoadedSkin, Xf};
use crate::skin::{Graph, SkinState};
use crate::ui::HitMap;
use std::cell::RefCell;

pub struct SkinHost {
    pub skin: Option<LoadedSkin>,
    tried: bool,
    /// What the skin was loaded from (`skinned::skin_key`): a screen over
    /// this one (SETTINGS, the skin select) may change it.
    from: Option<String>,
    /// The scene's start (skin time 0).
    pub created: Instant,
    /// The fade-out started (timer 2).
    pub leaving: Option<Instant>,
    /// Clickable objects of the last frame (set while drawing, which only
    /// borrows the scene).
    clicks: RefCell<HitMap<Click>>,
}

/// A clickable skin object: its event and beatoraja click mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Click {
    pub act: i32,
    pub mode: i32,
}

/// What a pointer at a point hit.
#[derive(Debug, PartialEq)]
pub enum Tap {
    Miss,
    /// An object the skin's scripts handle.
    Script,
    /// An object's event and the direction of the click (+1 / -1).
    Event(i32, i32),
}

impl Default for SkinHost {
    fn default() -> Self {
        SkinHost::new()
    }
}

impl SkinHost {
    pub fn new() -> SkinHost {
        SkinHost {
            skin: None,
            tried: false,
            from: None,
            created: Instant::now(),
            leaving: None,
            clicks: RefCell::default(),
        }
    }

    /// True on the first call only: when the scene loads its skin.
    pub fn first_update(&mut self) -> bool {
        !std::mem::replace(&mut self.tried, true)
    }

    /// The skin is made from `key` (`skinned::skin_key`).
    pub fn loaded_from(&mut self, key: String) {
        self.from = Some(key);
    }

    /// The skin's settings are now `key`: when they are not what it was
    /// loaded from, [`first_update`](Self::first_update) is true again
    /// (the scene frees the old skin and loads the new one).
    pub fn reload_if_changed(&mut self, key: &str) {
        if self.tried && self.from.as_deref() != Some(key) {
            self.tried = false;
        }
    }

    /// Per frame: textures the skin asked for; the game events its scripts
    /// asked for since the last frame.
    pub fn tick(&mut self, gfx: &mut Gfx) -> Vec<i32> {
        let Some(sk) = self.skin.as_mut() else {
            return Vec::new();
        };
        sk.load_pending(gfx);
        sk.script_events()
    }

    pub fn ms(&self, t: Instant) -> i64 {
        ms_since(self.created, t)
    }

    /// Input is taken (the skin's input delay has passed).
    pub fn ready(&self, now: Instant) -> bool {
        self.ms(now) >= self.skin.as_ref().map_or(0, |s| s.skin.input_ms)
    }

    pub fn fadeout_ms(&self) -> i64 {
        self.skin.as_ref().map_or(0, |s| s.skin.fadeout_ms)
    }

    /// Start the fade-out (once).
    pub fn leave(&mut self, now: Instant) {
        self.leaving.get_or_insert(now);
    }

    /// The fade-out has run.
    pub fn faded(&self, now: Instant) -> bool {
        self.leaving
            .is_some_and(|t| ms_since(t, now) >= self.fadeout_ms())
    }

    /// The timers every skinned screen has: 0 the start, 1 input taken, 2
    /// the fade-out.
    pub fn timer(&self, id: i32) -> Option<i64> {
        match id {
            0 => Some(0),
            1 => Some(self.skin.as_ref().map_or(0, |s| s.skin.input_ms)),
            2 => self.leaving.map(|t| self.ms(t)),
            _ => None,
        }
    }

    /// The skin's size, else LR2's screen.
    pub fn size(&self) -> (f32, f32) {
        self.skin
            .as_ref()
            .map_or((1280.0, 720.0), |s| (s.skin.width, s.skin.height))
    }

    pub fn xf(&self, view: Rect) -> Xf {
        let (w, h) = self.size();
        Xf::fit(w, h, view)
    }

    /// Draw the skin in `view` at `now`, keeping its clickable objects.
    pub fn draw(
        &self,
        d: &mut Draw,
        view: Rect,
        now: Instant,
        st: &dyn SkinState,
        hooks: &mut dyn Hooks,
    ) {
        let mut clicks = self.clicks.borrow_mut();
        clicks.clear();
        if let Some(sk) = self.skin.as_ref() {
            let mut c = Collect {
                inner: hooks,
                clicks: &mut clicks,
            };
            sk.draw(d, view, self.ms(now), st, &mut c);
        }
    }

    /// The clickable object at (`x`, `y`) (or within `slop` px of a
    /// smaller one: `HitMap::hit_near`): a script's, else its event.
    pub fn tap(&self, x: f32, y: f32, slop: f32) -> Tap {
        let clicks = self.clicks.borrow();
        let Some((r, c)) = clicks.hit_near(x, y, slop) else {
            return Tap::Miss;
        };
        if self.skin.as_ref().is_some_and(|s| s.click(c.act)) {
            return Tap::Script;
        }
        Tap::Event(c.act, click_dir(r, c.mode, x, y))
    }

    /// Where object `act` was drawn last.
    pub fn rect_of(&self, act: i32) -> Option<Rect> {
        let clicks = self.clicks.borrow();
        let found = clicks.iter().find(|(_, c)| c.act == act);
        found.map(|(r, _)| *r)
    }

    pub fn free(&mut self, gfx: &mut Gfx) {
        if let Some(mut s) = self.skin.take() {
            s.free(gfx);
        }
    }

    /// Free the skin's textures but keep the skin: they load again on the
    /// next [`tick`](Self::tick).
    pub fn free_textures(&mut self, gfx: &mut Gfx) {
        if let Some(s) = self.skin.as_mut() {
            s.free(gfx);
        }
    }
}

/// +1 / -1 for a click at (x, y) on `r` (beatoraja click modes: 0 adds, 1
/// subtracts, 2 splits left / right, 3 top / bottom).
pub fn click_dir(r: Rect, mode: i32, x: f32, y: f32) -> i32 {
    match mode {
        1 => -1,
        2 if x < r.x + r.w * 0.5 => -1,
        3 if y > r.y + r.h * 0.5 => -1,
        _ => 1,
    }
}

/// A scene's hooks, with the clickable objects kept.
struct Collect<'a, 'b> {
    inner: &'a mut dyn Hooks,
    clicks: &'b mut HitMap<Click>,
}

impl Hooks for Collect<'_, '_> {
    fn notes(&mut self, d: &mut Draw, skin: &LoadedSkin, xf: &Xf, now: i64, st: &dyn SkinState) {
        self.inner.notes(d, skin, xf, now, st)
    }
    fn bga(&mut self, d: &mut Draw, rect: Rect) {
        self.inner.bga(d, rect)
    }
    fn ref_image(&self, id: i32) -> Option<TexId> {
        self.inner.ref_image(id)
    }
    fn gauge(&self, side: usize) -> (f32, f32, bool, usize) {
        self.inner.gauge(side)
    }
    fn judge(&self, side: usize) -> Option<(usize, u32)> {
        self.inner.judge(side)
    }
    fn graph(&mut self, d: &mut Draw, graph: &Graph, rect: Rect, color: [f32; 4]) {
        self.inner.graph(d, graph, rect, color)
    }
    fn song_list(
        &mut self,
        d: &mut Draw,
        skin: &LoadedSkin,
        xf: &Xf,
        now: i64,
        st: &dyn SkinState,
    ) {
        self.inner.song_list(d, skin, xf, now, st)
    }
    fn clickable(&mut self, rect: Rect, act: i32, mode: i32) {
        self.clicks.push(rect, Click { act, mode });
    }
    fn preview(&mut self, d: &mut Draw, rect: Rect, color: [f32; 4]) {
        self.inner.preview(d, rect, color)
    }
}

/// Hooks of a screen that draws nothing of its own into its skin.
pub struct NoHooks;

impl Hooks for NoHooks {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_directions() {
        let r = Rect::new(0.0, 0.0, 100.0, 20.0);
        assert_eq!(click_dir(r, 0, 10.0, 5.0), 1);
        assert_eq!(click_dir(r, 1, 90.0, 5.0), -1);
        assert_eq!(click_dir(r, 2, 10.0, 5.0), -1);
        assert_eq!(click_dir(r, 2, 90.0, 5.0), 1);
        assert_eq!(click_dir(r, 3, 50.0, 2.0), 1);
        assert_eq!(click_dir(r, 3, 50.0, 18.0), -1);
    }

    #[test]
    fn taps_hit_the_topmost_object() {
        let host = SkinHost::new();
        {
            let mut c = host.clicks.borrow_mut();
            c.push(Rect::new(0.0, 0.0, 100.0, 100.0), Click { act: 1, mode: 0 });
            c.push(Rect::new(0.0, 0.0, 100.0, 50.0), Click { act: 2, mode: 2 });
        }
        assert_eq!(host.tap(10.0, 10.0, 0.0), Tap::Event(2, -1));
        assert_eq!(host.tap(90.0, 10.0, 0.0), Tap::Event(2, 1));
        assert_eq!(host.tap(10.0, 80.0, 0.0), Tap::Event(1, 1));
        assert_eq!(host.tap(10.0, 120.0, 0.0), Tap::Miss);
        assert_eq!(host.rect_of(1).map(|r| r.h), Some(100.0));
    }

    #[test]
    fn fade_out() {
        let mut host = SkinHost::new();
        let t0 = host.created;
        assert_eq!(host.timer(2), None);
        assert!(!host.faded(t0));
        host.leave(t0 + crate::platform::Duration::from_millis(100));
        host.leave(t0 + crate::platform::Duration::from_millis(900));
        assert_eq!(host.timer(2), Some(100));
        // no skin: no fade-out to wait for
        assert!(host.faded(t0 + crate::platform::Duration::from_millis(100)));
        assert!(host.first_update());
        assert!(!host.first_update());
        host.loaded_from("a".into());
        host.reload_if_changed("a");
        assert!(!host.first_update(), "the same settings: kept");
        host.reload_if_changed("b");
        assert!(host.first_update(), "other settings: loaded again");
        assert!(!host.first_update());
    }
}
