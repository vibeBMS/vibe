//! Small immediate-mode widgets shared by the menu scenes.

use crate::app::Draw;
use crate::gfx::text::Align;
use crate::gfx::{with_alpha, Rect};
use crate::theme::Theme;

pub fn panel(d: &mut Draw, r: Rect, theme: &Theme) {
    d.b.rect(r, theme.panel);
    d.b.rect_outline(r, 1.0, with_alpha(theme.text_dim, 0.25));
}

/// A button; returns its rect for hit-testing.
pub fn button(d: &mut Draw, r: Rect, label: &str, theme: &Theme, ui: f32, active: bool) -> Rect {
    let bg = if active {
        with_alpha(theme.accent, 0.35)
    } else {
        with_alpha(theme.panel, 0.95)
    };
    d.b.rect(r, bg);
    d.b.rect_outline(
        r,
        1.0,
        with_alpha(if active { theme.accent } else { theme.text_dim }, 0.6),
    );
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

/// Two-line option button: small caption + value.
pub fn option_button(
    d: &mut Draw,
    r: Rect,
    caption: &str,
    value: &str,
    theme: &Theme,
    ui: f32,
) -> Rect {
    d.b.rect(r, with_alpha(theme.panel, 0.95));
    d.b.rect_outline(r, 1.0, with_alpha(theme.text_dim, 0.5));
    let cs = (12.0 * ui).min(r.h * 0.3);
    let vs = (17.0 * ui).min(r.h * 0.42);
    d.text_fit(
        caption,
        r.x + r.w * 0.5,
        r.y + r.h * 0.08,
        cs,
        theme.text_dim,
        Align::Center,
        r.w - 6.0,
    );
    d.text_fit(
        value,
        r.x + r.w * 0.5,
        r.y + r.h * 0.42,
        vs,
        theme.accent,
        Align::Center,
        r.w - 6.0,
    );
    r
}

pub fn hit(rects: &[(Rect, usize)], x: f32, y: f32) -> Option<usize> {
    rects
        .iter()
        .rev()
        .find(|(r, _)| r.contains(x, y))
        .map(|(_, i)| *i)
}
