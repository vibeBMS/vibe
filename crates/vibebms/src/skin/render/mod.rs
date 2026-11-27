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

/// A side's gauge as a skin shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GaugeView {
    /// 0..100.
    pub value: f32,
    /// Where it clears (a groove gauge's 80).
    pub border: f32,
    /// A survival gauge: all of it is above the border.
    pub survival: bool,
    /// The gauge type, in beatoraja's order (assist, easy, normal, hard,
    /// ex-hard, hazard).
    pub kind: usize,
}

impl Default for GaugeView {
    /// An empty NORMAL gauge.
    fn default() -> GaugeView {
        GaugeView {
            value: 0.0,
            border: 80.0,
            survival: false,
            kind: 2,
        }
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
    /// A side's gauge.
    fn gauge(&self, side: usize) -> GaugeView {
        GaugeView::default()
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
