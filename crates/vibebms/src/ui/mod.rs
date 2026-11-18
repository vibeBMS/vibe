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
        tr!("◀ BACK"),
        ui,
        false,
    )
}

/// Where a line of `s` may break: its words, each with whether a space
/// came before it. A Chinese or Japanese character is a word of its own
/// (those languages put no spaces between words); closing punctuation
/// stays with the word before it, opening punctuation with the one after.
pub fn break_words(s: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut space = false;
    // the word so far ends after this one (a CJK character, opening
    // punctuation does not)
    let mut ends = false;
    let mut after_opening = false;
    for (i, c) in s.char_indices() {
        if c == ' ' {
            if let Some(st) = start.take() {
                out.push((space, &s[st..i]));
            }
            space = true;
            ends = false;
            after_opening = false;
            continue;
        }
        let closing = matches!(
            c,
            '、' | '。'
                | '，'
                | '．'
                | '！'
                | '？'
                | '：'
                | '；'
                | '）'
                | '」'
                | '』'
                | '】'
                | '〕'
                | '・'
                | 'ー'
                | '…'
                | ')'
                | ','
                | '.'
                | ':'
                | ';'
                | '!'
                | '?'
                | 'ぁ'
                | 'ぃ'
                | 'ぅ'
                | 'ぇ'
                | 'ぉ'
                | 'っ'
                | 'ゃ'
                | 'ゅ'
                | 'ょ'
                | 'ァ'
                | 'ィ'
                | 'ゥ'
                | 'ェ'
                | 'ォ'
                | 'ッ'
                | 'ャ'
                | 'ュ'
                | 'ョ'
        );
        let opening = matches!(c, '（' | '「' | '『' | '【' | '〔' | '(');
        if let Some(st) = start {
            if !closing && (ends || (is_cjk(c) && !after_opening)) {
                out.push((space, &s[st..i]));
                space = false;
                start = Some(i);
            }
        } else {
            start = Some(i);
        }
        ends = !opening && (is_cjk(c) || (ends && closing));
        after_opening = opening;
    }
    if let Some(st) = start {
        out.push((space, &s[st..]));
    }
    out
}

/// A Chinese or Japanese character (ideographs, kana, their punctuation).
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x30FF // punctuation, hiragana, katakana
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFF60 // full-width forms
        | 0x20000..=0x2FFFF)
}

#[cfg(test)]
mod break_tests {
    use super::break_words;

    #[test]
    fn breaks_at_spaces_and_between_cjk() {
        assert_eq!(
            break_words("a bc  d"),
            vec![(false, "a"), (true, "bc"), (true, "d")]
        );
        assert_eq!(
            break_words("判定を、ずらす。OK"),
            vec![
                (false, "判"),
                (false, "定"),
                (false, "を、"),
                (false, "ず"),
                (false, "ら"),
                (false, "す。"),
                (false, "OK")
            ]
        );
        assert_eq!(
            break_words("「設定」"),
            vec![(false, "「設"), (false, "定」")]
        );
        assert_eq!(break_words("LN判"), vec![(false, "LN"), (false, "判")]);
        assert_eq!(
            break_words("판정 오프셋"),
            vec![(false, "판정"), (true, "오프셋")]
        );
    }
}
