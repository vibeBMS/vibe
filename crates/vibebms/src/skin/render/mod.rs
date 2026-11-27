//! Drawing a loaded skin.

mod loaded;
mod number;
mod objects;
mod text;

pub use loaded::LoadedSkin;
pub use text::bmfont_scale;

use crate::gfx::{Draw, Rect, TexId};
use crate::skin::*;

/// Skin → screen transform (letterboxed, aspect preserved).
#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub sx: f32,
    pub sy: f32,
    pub ox: f32,
    pub oy: f32,
}

impl Xf {
    pub fn fit(skin_w: f32, skin_h: f32, view: Rect) -> Xf {
        let s = (view.w / skin_w).min(view.h / skin_h);
        Xf {
            sx: s,
            sy: s,
            ox: view.x + (view.w - skin_w * s) * 0.5,
            oy: view.y + (view.h - skin_h * s) * 0.5,
        }
    }
    pub fn rect(&self, r: Rect) -> Rect {
        Rect::new(
            self.ox + r.x * self.sx,
            self.oy + r.y * self.sy,
            r.w * self.sx,
            r.h * self.sy,
        )
    }
}

/// Scene-specific pieces the generic renderer cannot draw itself. Every
/// method has a no-op default so a scene implements only what it shows.
#[allow(unused_variables)]
pub trait Hooks {
    fn notes(&mut self, d: &mut Draw, skin: &LoadedSkin, xf: &Xf, now: i64, st: &dyn SkinState) {}
    fn bga(&mut self, d: &mut Draw, rect: Rect) {}
    fn ref_image(&self, id: i32) -> Option<TexId> {
        None
    }
    /// A side's gauge: (value, border, survival, gauge type in beatoraja
    /// order).
    fn gauge(&self, side: usize) -> (f32, f32, bool, usize) {
        (0.0, 80.0, false, 2)
    }
    /// Current judgement for a side: (judge 0..5, combo).
    fn judge(&self, side: usize) -> Option<(usize, u32)> {
        None
    }
    fn graph(&mut self, d: &mut Draw, graph: &Graph, rect: Rect, color: [f32; 4]) {}
    /// The music select bar list.
    fn song_list(
        &mut self,
        d: &mut Draw,
        skin: &LoadedSkin,
        xf: &Xf,
        now: i64,
        st: &dyn SkinState,
    ) {
    }
    /// A clickable object was drawn at `rect` (screen space).
    fn clickable(&mut self, rect: Rect, act: i32, click: i32) {}
    /// The skin select screen's preview of the chosen skin.
    fn preview(&mut self, d: &mut Draw, rect: Rect, color: [f32; 4]) {}
}
