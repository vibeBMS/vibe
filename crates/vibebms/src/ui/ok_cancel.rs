//! The OK / Cancel buttons of a dialog.

use super::HitMap;
use crate::gfx::{Draw, Rect};

/// A dialog's OK button and its Cancel to the left of it, as last drawn,
/// for the pointer.
#[derive(Debug, Default)]
pub struct OkCancel {
    /// true OK, false Cancel.
    pub(super) hits: HitMap<bool>,
}

impl OkCancel {
    /// OK at `ok`, Cancel (`cancel`) as large, 10 UI units to its left;
    /// `ok_on`: OK is the chosen one (else Cancel).
    pub fn draw(&mut self, d: &mut Draw, ok: Rect, cancel: &str, ui: f32, ok_on: bool) {
        let c = Rect::new(ok.x - 10.0 * ui - ok.w, ok.y, ok.w, ok.h);
        self.hits.clear();
        self.hits.push(super::button(d, ok, "OK", ui, ok_on), true);
        self.hits
            .push(super::button(d, c, cancel, ui, !ok_on), false);
    }

    /// All of `r` is OK (a box a tap takes), and there is no Cancel.
    pub fn ok_only(&mut self, r: Rect) {
        self.hits.clear();
        self.hits.push(r, true);
    }

    /// The button at (`x`, `y`): true OK, false Cancel.
    pub fn at(&self, x: f32, y: f32) -> Option<bool> {
        self.hits.at(x, y).copied()
    }
}
