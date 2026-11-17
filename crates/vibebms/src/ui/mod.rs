//! Small immediate-mode widgets shared by the menu scenes.

mod confirm;
mod hit;
mod list;
mod text_input;

pub use confirm::Armed;
pub use hit::HitMap;
pub use list::ListView;
pub use text_input::{TextEvent, TextInput};

use crate::app::Draw;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{with_alpha, Rect};
use crate::theme::{ACCENT, BACKGROUND, PANEL, TEXT, TEXT_DIM};

/// How far (in UI units) a press may move and still be a tap, not a drag.
pub const DRAG_SLOP: f32 = 10.0;

/// Panel look shared with the default skins: rounded, thin border, a short
/// accent tab on the top edge.
pub fn panel(d: &mut Draw, r: Rect) {
    let t = 1.0;
    d.b.rounded_panel(r, 8.0, opaque(PANEL), edge(), t);
    let tab = (r.w * 0.15).clamp(12.0, 50.0);
    d.b.rect(Rect::new(r.x + 10.0, r.y, tab, 2.0), ACCENT);
}

fn edge() -> crate::gfx::Color {
    with_alpha(TEXT_DIM, 0.4)
}

/// The panel colour without its transparency (a translucent fill would let
/// the border show through).
fn opaque(c: crate::gfx::Color) -> crate::gfx::Color {
    [c[0], c[1], c[2], 1.0]
}

/// A button; returns its rect for hit-testing.
pub fn button(d: &mut Draw, r: Rect, label: &str, ui: f32, active: bool) -> Rect {
    let (fill, border) = if active {
        (mix(PANEL, ACCENT, 0.22), ACCENT)
    } else {
        (lighter(PANEL), edge())
    };
    d.b.rounded_panel(r, 7.0, fill, border, if active { 1.5 } else { 1.0 });
    let size = (16.0 * ui).min(r.h * 0.6);
    d.text(
        label,
        r.x + r.w * 0.5,
        r.y + (r.h - size) * 0.5 - size * 0.08,
        TextStyle::new(size, TEXT, Align::Center).fit(r.w - 8.0),
    );
    r
}

fn mix(a: crate::gfx::Color, b: crate::gfx::Color, t: f32) -> crate::gfx::Color {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        1.0,
    ]
}

fn lighter(c: crate::gfx::Color) -> crate::gfx::Color {
    lift(c, 0.05)
}

/// `c` moved towards white by `t` (opaque).
pub fn lift(c: crate::gfx::Color, t: f32) -> crate::gfx::Color {
    mix(c, [1.0, 1.0, 1.0, 1.0], t)
}

/// A list row (settings items, menus); `selected` uses the accent.
pub fn row(d: &mut Draw, r: Rect, selected: bool) {
    if selected {
        d.b.rounded_panel(r, 6.0, mix(PANEL, ACCENT, 0.18), ACCENT, 1.5);
    } else {
        d.b.rounded_panel(r, 6.0, lighter(PANEL), edge(), 1.0);
    }
}

/// A panel `r` over the whole `w` × `h` screen, which it dims.
pub fn modal(d: &mut Draw, w: f32, h: f32, r: Rect) {
    d.b.rect(Rect::new(0.0, 0.0, w, h), with_alpha(BACKGROUND, 0.7));
    panel(d, r);
}

/// The menu screens' button that leaves them, at the top right of a
/// screen `w` wide.
pub fn back_button(d: &mut Draw, w: f32, ui: f32) -> Rect {
    button(
        d,
        Rect::new(w - 110.0 * ui, 7.0 * ui, 100.0 * ui, 34.0 * ui),
        "◀ BACK",
        ui,
        false,
    )
}
