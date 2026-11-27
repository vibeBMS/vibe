pub mod decide;
pub mod graphs;
pub mod keybind;
pub mod keyconfig;
pub mod option_panel;
pub mod play;
pub mod play_opts;
pub mod play_skin;
pub mod practice;
pub mod result;
pub mod result_skin;
pub mod select;
pub mod settings;
pub mod skin_host;
pub mod skinned;
pub mod skinselect;
pub mod skinvals;
pub mod target;
pub mod touch_layout;

use crate::ctx::Ctx;
use crate::gfx::{Draw, Gfx};
use crate::input::InputEv;
use crate::platform::Instant;

pub enum Next {
    Stay,
    Push(Box<dyn Scene>),
    Replace(Box<dyn Scene>),
    Pop,
    Quit,
}

pub trait Scene {
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next;
    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant);
    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next;
    /// Another scene was pushed over it (it is not drawn until
    /// [`resumed`](Self::resumed)): textures may be let go.
    fn covered(&mut self, _gfx: &mut Gfx) {}
    /// Became the top scene again.
    fn resumed(&mut self, _ctx: &mut Ctx) {}
    /// About to be removed: release textures/sounds.
    fn exit(&mut self, _ctx: &mut Ctx, _gfx: &mut Gfx) {}
    /// The screen needs all of it (play): the web page hides its own
    /// buttons meanwhile.
    fn full_screen(&self) -> bool {
        false
    }
    /// A chart is being played (`activity`: heard without the focus).
    fn in_play(&self) -> bool {
        false
    }
    /// Wants every frame redrawn ASAP (gameplay).
    fn realtime(&self) -> bool {
        false
    }
    /// What a screenshot of it shows, for its file name: title,
    /// difficulty, lamp, EX score (as far as the scene has them).
    fn screenshot_label(&self, _ctx: &Ctx) -> Vec<String> {
        Vec::new()
    }
    /// The screen it is, for Discord and OBS (`integrations`); `None`: the
    /// one under it says (settings over song select is song select).
    fn activity(&self) -> Option<crate::integrations::Activity> {
        None
    }
}

/// Position of `v` in `all` (an image set index).
pub fn index_of<T: PartialEq>(all: &[T], v: T) -> i64 {
    all.iter().position(|x| *x == v).unwrap_or(0) as i64
}

/// `v` moved `d` places along `all`, around the end.
pub fn cycle<T: Copy + PartialEq>(all: &[T], v: T, d: i32) -> T {
    let n = all.len() as i32;
    let i = all.iter().position(|x| *x == v).unwrap_or(0) as i32;
    all[(i + d).rem_euclid(n) as usize]
}

/// ON / OFF as skins and toasts show them (untranslated: settings rows
/// have their own, `settings::row::onoff`).
pub fn onoff(b: bool) -> &'static str {
    if b {
        "ON"
    } else {
        "OFF"
    }
}
