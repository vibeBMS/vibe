//! A box of text over a screen: a chart's text files, the licences. ↑↓ (the
//! turntable), Page Up / Down, Home / End, the wheel and drags scroll it; the
//! other keys, its close button and a tap outside it close it. Only the
//! lines in view are wrapped and drawn: a text of any length costs a frame
//! what a screenful of it does.

use super::{DragScroll, HitMap, DRAG_SLOP};
use crate::ctx::Ctx;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::theme;
use crate::gfx::{Draw, Rect};
use crate::input::{AxisStepper, InputEv, Nav, Phase};
use std::borrow::Cow;
use winit::keyboard::KeyCode;

/// Text size and line height (UI units).
const SIZE: f32 = 16.0;
const LINE: f32 = 23.0;

/// A place in the text: `row` rows (as wrapped, with a fraction) into
/// line `line`. Earlier places compare less.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
struct At {
    line: usize,
    row: f32,
}

/// The width (px) and text size lines are wrapped at.
#[derive(Clone, Copy, PartialEq)]
struct Fit {
    w: f32,
    size: f32,
}

pub struct Reader {
    title: String,
    subtitle: String,
    /// None while they are read.
    lines: Option<Vec<Cow<'static, str>>>,
    /// The place at the top of the view.
    top: At,
    /// The scroll asked for since the last frame (in rows, after a jump
    /// to the start or the end): wrapping takes the frame's fonts.
    by: f32,
    jump: Option<Jump>,
    /// What the text was last wrapped to, the rows in view then, and the
    /// last place the top can be at for those (with how far into the
    /// text, in lines).
    fit: Option<(Fit, f32)>,
    end: Option<(At, f32)>,
    /// Rows that fit, as last drawn.
    rows: f32,
    /// The lines drawn in the last frame (first, last).
    shown: (usize, usize),
    /// A press, and the place at the top when it went down.
    drag: Option<(DragScroll, At)>,
    axis: AxisStepper,
    /// The box, its close button and the caller's button, as last drawn.
    hits: HitMap<Hit>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Jump {
    Start,
    End,
    /// Rows on from a place (a drag).
    From(At, f32),
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Box,
    Close,
    Button,
}

/// What an input did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Read {
    /// It is still open.
    Open,
    Closed,
    /// The caller's button (`draw`'s `button`) was tapped.
    Button,
}

impl Reader {
    /// `title` over the text, `subtitle` beside it; the text to come
    /// (`set_lines`).
    pub fn new(title: String, subtitle: String) -> Reader {
        Reader {
            title,
            subtitle,
            lines: None,
            top: At::default(),
            by: 0.0,
            jump: None,
            fit: None,
            end: None,
            rows: 1.0,
            shown: (0, 0),
            drag: None,
            axis: AxisStepper::default(),
            hits: HitMap::default(),
        }
    }

    pub fn set_lines(&mut self, lines: Vec<Cow<'static, str>>) {
        self.lines = Some(lines);
        self.top = At::default();
        self.end = None;
    }

    pub fn lines(&self) -> Option<&[Cow<'static, str>]> {
        self.lines.as_deref()
    }

    /// The line at the top of the view.
    pub fn top_line(&self) -> usize {
        self.top.line
    }

    /// The place at the top: a line, and rows into it.
    #[cfg(test)]
    pub fn top(&self) -> (usize, f32) {
        (self.top.line, self.top.row)
    }

    /// The first and the last line drawn in the last frame.
    pub fn shown(&self) -> (usize, usize) {
        self.shown
    }

    fn scroll(&mut self, rows: f32) {
        self.by += rows;
    }

    fn jump(&mut self, to: Jump) {
        self.jump = Some(to);
        self.by = 0.0;
    }

    /// The box over the whole screen, with `button` (a label) left of
    /// its close button.
    pub fn draw(&mut self, ctx: &Ctx, d: &mut Draw, button: Option<&str>) {
        let (w, h, ui) = (ctx.view.width, ctx.view.height, ctx.ui());
        d.b.rect(Rect::new(0.0, 0.0, w, h), [0.0, 0.0, 0.0, 0.66]);
        let m = 24.0 * ui;
        let r = Rect::new(m, m, w - 2.0 * m, h - 2.0 * m);
        super::panel(d, r);
        let pad = 18.0 * ui;
        d.text(
            &self.title,
            r.x + pad,
            r.y + 14.0 * ui,
            TextStyle::new(20.0 * ui, theme::TEXT, Align::Left),
        );
        d.text(
            &self.subtitle,
            r.x + pad + 130.0 * ui,
            r.y + 18.0 * ui,
            TextStyle::new(14.0 * ui, theme::TEXT_DIM, Align::Left).fit(r.w - 360.0 * ui),
        );
        let close = Rect::new(r.right() - 52.0 * ui, r.y + 6.0 * ui, 44.0 * ui, 44.0 * ui);
        d.text(
            "\u{00d7}",
            close.x + close.w * 0.5,
            close.y + 6.0 * ui,
            TextStyle::new(28.0 * ui, theme::TEXT_DIM, Align::Center),
        );
        self.hits.clear();
        self.hits.push(r, Hit::Box);
        self.hits.push(close, Hit::Close);
        if let Some(label) = button {
            let b = Rect::new(close.x - 96.0 * ui, r.y + 12.0 * ui, 88.0 * ui, 32.0 * ui);
            super::button(d, b, label, ui, false);
            self.hits.push(b, Hit::Button);
        }
        let body = Rect::new(
            r.x + pad,
            r.y + 56.0 * ui,
            r.w - 2.0 * pad,
            r.h - 56.0 * ui - pad,
        );
        let (size, line) = (SIZE * ui, LINE * ui);
        let style = TextStyle::new(size, theme::TEXT, Align::Left);
        let Some(lines) = self.lines.as_deref().filter(|l| !l.is_empty()) else {
            if self.lines.is_none() {
                let dim = TextStyle::new(size, theme::TEXT_DIM, Align::Left);
                d.text(tr!("Reading\u{2026}"), body.x, body.y, dim);
            }
            return;
        };
        self.rows = (body.h / line).floor().max(1.0);
        let fit = Fit {
            w: body.w - 14.0 * ui,
            size,
        };
        if self.fit != Some((fit, self.rows)) {
            // the top stays on its line
            self.fit = Some((fit, self.rows));
            self.end = None;
            self.top.row = 0.0;
        }
        let (end, end_at) = *self.end.get_or_insert_with(|| {
            let last = lines.len() - 1;
            let bottom = At {
                line: last,
                row: rows_of(d, &lines[last], fit).len() as f32,
            };
            let end = walk(d, lines, fit, bottom, -self.rows, None);
            (
                end,
                end.line as f32 + end.row / count(d, &lines[end.line], fit),
            )
        });
        match self.jump.take() {
            Some(Jump::Start) => self.top = At::default(),
            Some(Jump::End) => self.top = end,
            Some(Jump::From(at, by)) => self.top = walk(d, lines, fit, at, by, Some(end)),
            None => {}
        }
        if self.by != 0.0 {
            self.top = walk(d, lines, fit, self.top, self.by, Some(end));
            self.by = 0.0;
        }
        if self.top > end {
            self.top = end;
        }
        d.b.set_clip(Some(body));
        let first = self.top.row.floor();
        let mut y = body.y - (self.top.row - first) * line;
        let (mut at, mut row) = (self.top.line, first as usize);
        self.shown = (at, at);
        'lines: while at < lines.len() {
            let s = &lines[at];
            let starts = rows_of(d, s, fit);
            for (k, &a) in starts.iter().enumerate().skip(row) {
                if y >= body.bottom() {
                    break 'lines;
                }
                let b = starts.get(k + 1).copied().unwrap_or(s.len());
                d.text(&s[a..b], body.x, y, style);
                self.shown.1 = at;
                y += line;
            }
            (at, row) = (at + 1, 0);
        }
        d.b.set_clip(None);
        // where the view is in the text
        if end > At::default() {
            let top_at = self.top.line as f32 + self.top.row / count(d, &lines[self.top.line], fit);
            let th = (body.h * self.rows / (end_at + self.rows)).max(24.0 * ui);
            let ty = body.y + (body.h - th) * (top_at / end_at).min(1.0);
            let track = Rect::new(body.right() - 5.0 * ui, body.y, 5.0 * ui, body.h);
            d.b.rect(track, [1.0, 1.0, 1.0, 0.06]);
            d.b.rect(Rect::new(track.x, ty, track.w, th), theme::ACCENT);
        }
    }

    /// ↑↓ (the turntable) scroll, the other keys close (not one held
    /// down from before it opened).
    fn nav(&mut self, ctx: &Ctx, ev: &InputEv, repeat: bool) -> Read {
        let keys = &ctx.config.keys;
        let axis = keys.any_axis_is_scratch.then_some(&mut self.axis);
        match ev.nav_rows(keys, axis) {
            Some(Nav::Up) => self.scroll(-1.0),
            Some(Nav::Down) => self.scroll(1.0),
            Some(_) if !repeat => return Read::Closed,
            _ => {}
        }
        Read::Open
    }

    pub fn input(&mut self, ctx: &Ctx, ev: &InputEv) -> Read {
        let page = (self.rows - 1.0).max(1.0);
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                repeat,
                ..
            } => match code {
                KeyCode::PageUp => self.scroll(-page),
                KeyCode::PageDown | KeyCode::Space => self.scroll(page),
                KeyCode::Home => self.jump(Jump::Start),
                KeyCode::End => self.jump(Jump::End),
                _ => return self.nav(ctx, ev, *repeat),
            },
            InputEv::Wheel { dy } => self.scroll(-dy * 3.0),
            InputEv::Pointer {
                id, phase, x, y, ..
            } => match phase {
                Phase::Down => self.drag = Some((DragScroll::new(*id, *x, *y, 0.0), self.top)),
                Phase::Move => {
                    let (slop, line) = (DRAG_SLOP * ctx.ui(), LINE * ctx.ui());
                    let to = self.drag.as_mut().and_then(|(d, from)| {
                        d.to(*id, *y, slop, line).map(|s| Jump::From(*from, s))
                    });
                    if let Some(to) = to {
                        self.jump(to);
                    }
                }
                Phase::Up => {
                    let tap = self.drag.take().is_some_and(|(d, _)| d.tap(*id));
                    // a tap outside the box or on its close button
                    match self.hits.at(*x, *y).copied().filter(|_| tap) {
                        None if tap => return Read::Closed,
                        Some(Hit::Close) => return Read::Closed,
                        Some(Hit::Button) => return Read::Button,
                        _ => {}
                    }
                }
            },
            _ => return self.nav(ctx, ev, false),
        }
        Read::Open
    }
}

/// Where the rows of `s` start when it is wrapped to `fit`.
fn rows_of(d: &mut Draw, s: &str, fit: Fit) -> Vec<usize> {
    breaks(s, fit.w, |c| {
        d.measure(c.encode_utf8(&mut [0; 4]), fit.size)
    })
}

/// How many rows `s` takes (at least one).
fn count(d: &mut Draw, s: &str, fit: Fit) -> f32 {
    rows_of(d, s, fit).len() as f32
}

/// From `at`, `by` rows on (or back), not past `end`; only the lines
/// passed are wrapped.
fn walk(d: &mut Draw, lines: &[Cow<str>], fit: Fit, mut at: At, by: f32, end: Option<At>) -> At {
    at.row += by;
    while at.row < 0.0 {
        if at.line == 0 {
            return At::default();
        }
        at.line -= 1;
        at.row += count(d, &lines[at.line], fit);
    }
    loop {
        if end.is_some_and(|e| at.line > e.line) {
            return end.unwrap_or_default();
        }
        let n = count(d, &lines[at.line], fit);
        if at.row < n || at.line + 1 == lines.len() {
            at.row = at.row.min(n);
            return at;
        }
        at.row -= n;
        at.line += 1;
    }
}

/// Where the rows of `s` start when it is wrapped to `w`, each character
/// as wide as `width` says: after the last space before the character
/// that does not fit (spaces hang past the end), else where the row is
/// full (a long address). The first row starts at 0.
fn breaks(s: &str, w: f32, mut width: impl FnMut(char) -> f32) -> Vec<usize> {
    let mut starts = vec![0];
    // the row's width so far, and where after its last space (and the
    // width up to there)
    let mut used = 0.0;
    let mut space: Option<(usize, f32)> = None;
    for (i, c) in s.char_indices() {
        let cw = width(c);
        while c != ' ' && used + cw > w && starts.last().is_some_and(|&st| i > st) {
            match space.take() {
                Some((at, upto)) => {
                    starts.push(at);
                    used -= upto;
                }
                None => {
                    starts.push(i);
                    used = 0.0;
                }
            }
        }
        used += cw;
        if c == ' ' {
            space = Some((i + 1, used));
        }
    }
    starts
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::gfx::text::Text;
    use crate::gfx::{Batch, Gfx};
    use crate::platform::Instant;

    #[test]
    fn lines_wrap_at_spaces_else_where_full() {
        let one = |_: char| 1.0;
        assert_eq!(breaks("", 4.0, one), [0]);
        assert_eq!(breaks("abcd", 4.0, one), [0]);
        assert_eq!(breaks("ab cd ef", 5.0, one), [0, 6]);
        assert_eq!(breaks("ab cd ef", 4.0, one), [0, 3, 6]);
        // a word longer than the row: cut where it is full
        assert_eq!(breaks("abcdefghij", 4.0, one), [0, 4, 8]);
        assert_eq!(breaks("a bcdefghij", 4.0, one), [0, 2, 6, 10]);
        // too narrow for one character: one a row
        assert_eq!(breaks("abc", 0.5, one), [0, 1, 2]);
    }

    fn ptr(id: u64, phase: Phase, x: f32, y: f32) -> InputEv {
        InputEv::Pointer {
            id,
            phase,
            x,
            y,
            time: Instant::now(),
            touch: true,
        }
    }

    fn key(code: KeyCode) -> InputEv {
        InputEv::Key {
            code,
            pressed: true,
            repeat: false,
            time: Instant::now(),
            text: None,
        }
    }

    struct Frame {
        gfx: Gfx,
        text: Text,
        batch: Batch,
    }

    impl Frame {
        fn new() -> Frame {
            Frame {
                gfx: Gfx::without_gpu(),
                text: Text::built_in(),
                batch: Batch::default(),
            }
        }

        fn draw(&mut self, ctx: &Ctx, r: &mut Reader) {
            self.batch.clear();
            let mut d = Draw {
                gfx: &self.gfx,
                text: &mut self.text,
                b: &mut self.batch,
                pointer: None,
                text_input: false,
            };
            r.draw(ctx, &mut d, None);
        }
    }

    /// 100 short lines in a view of some 25: the keys, the wheel and
    /// drags scroll by rows, never past either end.
    #[test]
    fn it_scrolls_within_the_text() {
        let root = std::env::temp_dir().join(format!("vibebms-uireader-{}", std::process::id()));
        let ctx = Ctx::for_test(root.clone());
        let mut f = Frame::new();
        let mut r = Reader::new("t".into(), String::new());
        r.set_lines((0..100).map(|i| Cow::Owned(format!("line {i}"))).collect());
        f.draw(&ctx, &mut r);
        let rows = r.rows as usize;
        assert!(rows > 10 && rows < 50, "{rows}");
        // (and part of the row below)
        assert!(r.shown().0 == 0 && (rows - 1..=rows).contains(&r.shown().1));
        let at = |r: &mut Reader, f: &mut Frame, ev: InputEv| {
            assert_eq!(r.input(&ctx, &ev), Read::Open);
            f.draw(&ctx, r);
            r.top_line()
        };
        assert_eq!(at(&mut r, &mut f, key(KeyCode::ArrowDown)), 1);
        assert_eq!(at(&mut r, &mut f, key(KeyCode::ArrowUp)), 0);
        assert_eq!(at(&mut r, &mut f, key(KeyCode::ArrowUp)), 0);
        assert_eq!(at(&mut r, &mut f, InputEv::Wheel { dy: -2.0 }), 6);
        assert_eq!(at(&mut r, &mut f, key(KeyCode::End)), 100 - rows);
        assert_eq!(r.shown().1, 99);
        assert_eq!(at(&mut r, &mut f, key(KeyCode::PageDown)), 100 - rows);
        assert_eq!(at(&mut r, &mut f, key(KeyCode::Home)), 0);
        assert_eq!(at(&mut r, &mut f, key(KeyCode::PageDown)), rows - 1);
        assert_eq!(at(&mut r, &mut f, key(KeyCode::PageUp)), 0);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A drag scrolls the text by lines (within it) once past the slop; a
    /// tap outside the box closes it, a drag's lift or a tap inside does
    /// not; any lift ends the press.
    #[test]
    fn a_drag_scrolls_the_text() {
        let root = std::env::temp_dir().join(format!("vibebms-readdrag-{}", std::process::id()));
        let ctx = Ctx::for_test(root.clone());
        let mut f = Frame::new();
        let mut r = Reader::new("t".into(), String::new());
        r.set_lines(vec![Cow::Borrowed("x"); 100]);
        f.draw(&ctx, &mut r);
        let end = 100 - r.rows as usize;
        let line = LINE * ctx.ui();
        let slop = DRAG_SLOP * ctx.ui();
        let mut on = |id, phase, x, y| {
            let read = r.input(&ctx, &ptr(id, phase, x, y));
            f.draw(&ctx, &mut r);
            (read, r.top_line())
        };
        // (in the box, below its title)
        let (x, y) = (50.0, 300.0);
        assert_eq!(on(1, Phase::Down, x, y), (Read::Open, 0));
        assert_eq!(on(1, Phase::Move, x, y - slop), (Read::Open, 0));
        assert_eq!(on(1, Phase::Move, x, y - 3.0 * line), (Read::Open, 3));
        assert_eq!(on(2, Phase::Move, x, 0.0), (Read::Open, 3));
        // not past the end, nor above the start
        assert_eq!(on(1, Phase::Move, x, y - 1000.0 * line), (Read::Open, end));
        assert_eq!(on(1, Phase::Move, x, y - 2.0 * line), (Read::Open, 2));
        assert_eq!(on(1, Phase::Move, x, y + 1000.0 * line), (Read::Open, 0));
        assert_eq!(on(1, Phase::Up, x, y).0, Read::Open);
        assert_eq!(on(1, Phase::Down, x, y).0, Read::Open);
        assert_eq!(on(1, Phase::Up, x, y).0, Read::Open);
        // outside the box
        assert_eq!(on(1, Phase::Down, 2.0, 2.0).0, Read::Open);
        assert_eq!(on(2, Phase::Up, 2.0, 2.0).0, Read::Open);
        assert_eq!(on(1, Phase::Up, 2.0, 2.0).0, Read::Open);
        assert_eq!(on(1, Phase::Down, 2.0, 2.0).0, Read::Open);
        assert_eq!(on(1, Phase::Up, 2.0, 2.0).0, Read::Closed);
        std::fs::remove_dir_all(&root).ok();
    }

    /// Esc closes; a held key's repeat does not; the turntable's keys
    /// scroll.
    #[test]
    fn keys_close_it_but_a_held_one() {
        let root = std::env::temp_dir().join(format!("vibebms-readkeys-{}", std::process::id()));
        let ctx = Ctx::for_test(root.clone());
        let mut r = Reader::new("t".into(), String::new());
        let held = InputEv::Key {
            code: KeyCode::Enter,
            pressed: true,
            repeat: true,
            time: Instant::now(),
            text: None,
        };
        assert_eq!(r.input(&ctx, &held), Read::Open);
        let scratch = ctx.config.keys.p1.scratch_down.keys.first().copied();
        if let Some(k) = scratch {
            assert_eq!(r.input(&ctx, &key(k)), Read::Open);
            assert_eq!(r.by, 1.0);
        }
        assert_eq!(r.input(&ctx, &key(KeyCode::Escape)), Read::Closed);
        assert_eq!(r.input(&ctx, &key(KeyCode::Enter)), Read::Closed);
        std::fs::remove_dir_all(&root).ok();
    }
}
