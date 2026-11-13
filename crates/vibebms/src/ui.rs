//! Small immediate-mode widgets shared by the menu scenes.

use crate::app::Draw;
use crate::gfx::text::Align;
use crate::gfx::{with_alpha, Rect};
use crate::theme::Theme;

/// Panel look shared with the default skins: rounded, thin border, a short
/// accent tab on the top edge.
pub fn panel(d: &mut Draw, r: Rect, theme: &Theme) {
    let t = 1.0;
    d.b.rounded_panel(r, 8.0, opaque(theme.panel), edge(theme), t);
    let tab = (r.w * 0.15).clamp(12.0, 50.0);
    d.b.rect(Rect::new(r.x + 10.0, r.y, tab, 2.0), theme.accent);
}

fn edge(theme: &Theme) -> crate::gfx::Color {
    with_alpha(theme.text_dim, 0.4)
}

/// The panel colour without its transparency (a translucent fill would let
/// the border show through).
fn opaque(c: crate::gfx::Color) -> crate::gfx::Color {
    [c[0], c[1], c[2], 1.0]
}

/// A button; returns its rect for hit-testing.
pub fn button(d: &mut Draw, r: Rect, label: &str, theme: &Theme, ui: f32, active: bool) -> Rect {
    let (fill, border) = if active {
        (mix(theme.panel, theme.accent, 0.22), theme.accent)
    } else {
        (lighter(theme.panel), edge(theme))
    };
    d.b.rounded_panel(r, 7.0, fill, border, if active { 1.5 } else { 1.0 });
    let size = (16.0 * ui).min(r.h * 0.6);
    d.text_fit(
        label,
        r.x + r.w * 0.5,
        r.y + (r.h - size) * 0.5 - size * 0.08,
        size,
        theme.text,
        Align::Center,
        r.w - 8.0,
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
pub fn row(d: &mut Draw, r: Rect, theme: &Theme, selected: bool) {
    if selected {
        d.b.rounded_panel(
            r,
            6.0,
            mix(theme.panel, theme.accent, 0.18),
            theme.accent,
            1.5,
        );
    } else {
        d.b.rounded_panel(r, 6.0, lighter(theme.panel), edge(theme), 1.0);
    }
}

pub fn hit(rects: &[(Rect, usize)], x: f32, y: f32) -> Option<usize> {
    rects
        .iter()
        .rev()
        .find(|(r, _)| r.contains(x, y))
        .map(|(_, i)| *i)
}
