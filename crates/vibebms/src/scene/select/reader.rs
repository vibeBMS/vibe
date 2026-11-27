//! READ TEXT: the selected chart's text files (LR2's readme viewer) in a
//! scrolling box over the select screen. Skins cannot scroll text, so the
//! scene draws it.

use super::SelectScene;
use crate::ctx::Ctx;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::theme;
use crate::gfx::{Draw, Rect};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::platform::vfs::{self, Prefetch};
use crate::ui::{HitMap, DRAG_SLOP};
use std::path::PathBuf;
use winit::keyboard::KeyCode;

/// Text size and line height (UI units).
const SIZE: f32 = 16.0;
const LINE: f32 = 23.0;

pub(super) struct Reader {
    title: String,
    /// The files while they are read (the web: from the browser's storage).
    fetch: Option<(Prefetch, Vec<PathBuf>)>,
    lines: Vec<String>,
    /// The file (index in `files`) each line is of.
    line_file: Vec<usize>,
    files: Vec<PathBuf>,
    /// `lines` wrapped to a width (px), and the width.
    wrapped: Vec<String>,
    /// The line of `lines` each wrapped line is of.
    wrapped_line: Vec<usize>,
    width: f32,
    /// The first line shown.
    scroll: f32,
    /// Lines that fit, as last drawn.
    rows: f32,
    /// A press: its pointer, where it went down, the scroll then, moved.
    drag: Option<(u64, f32, f32, bool)>,
    /// The box, its close button and OPEN (desktop), as last drawn.
    hits: HitMap<Hit>,
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Box,
    Close,
    Open,
}

impl Reader {
    fn new(title: String, files: Vec<PathBuf>) -> Reader {
        Reader {
            title,
            fetch: Some((Prefetch::files(files.clone()), files.clone())),
            lines: Vec::new(),
            line_file: Vec::new(),
            files,
            wrapped: Vec::new(),
            wrapped_line: Vec::new(),
            width: 0.0,
            scroll: 0.0,
            rows: 1.0,
            drag: None,
            hits: HitMap::default(),
        }
    }

    /// The files read: their lines, each file after its name when there
    /// are several.
    fn update(&mut self) {
        let Some((_, files)) = self.fetch.take_if(|(f, _)| f.finished()) else {
            return;
        };
        let several = files.len() > 1;
        for (k, p) in files.iter().enumerate() {
            if several {
                if k > 0 {
                    self.lines.push(String::new());
                }
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                self.lines
                    .push(format!("\u{2014} {} \u{2014}", name.unwrap_or_default()));
            }
            match vfs::read(p) {
                Ok(b) => {
                    let text = vibebms_core::encoding::decode_text(&b);
                    self.lines
                        .extend(text.lines().map(|l| l.replace('\t', "    ")));
                }
                Err(e) => self.lines.push(format!("({e})")),
            }
            self.line_file.resize(self.lines.len(), k);
        }
        self.width = 0.0;
    }

    /// `lines` wrapped to `w` px at the text size `size`.
    fn wrap(&mut self, d: &mut Draw, w: f32, size: f32) {
        if (self.width - w).abs() < 0.5 {
            return;
        }
        self.width = w;
        self.wrapped.clear();
        self.wrapped_line.clear();
        for (n, line) in self.lines.iter().enumerate() {
            let mut cur = String::new();
            for c in line.chars() {
                cur.push(c);
                if d.measure(&cur, size) > w {
                    cur.pop();
                    // at a space when there is one
                    let cut = cur.rfind(' ').filter(|i| *i > 0).unwrap_or(cur.len());
                    let rest = cur.split_off(cut);
                    self.wrapped.push(std::mem::take(&mut cur));
                    self.wrapped_line.push(n);
                    cur = rest.trim_start().to_string();
                    cur.push(c);
                }
            }
            self.wrapped.push(cur);
            self.wrapped_line.push(n);
        }
    }

    /// The file shown at the top of the view (the only one, or the one
    /// whose text is there).
    fn file_shown(&self) -> Option<&PathBuf> {
        let line = self.wrapped_line.get(self.scroll.floor() as usize).copied();
        let k = line.and_then(|l| self.line_file.get(l)).copied();
        self.files.get(k.unwrap_or(0))
    }

    /// The file shown in the system's text viewer (desktop; one in an
    /// archive: the archive in the file manager).
    fn open_shown(&self) {
        let Some(p) = self.file_shown() else { return };
        let disk = super::files::on_disk(p);
        if disk == *p {
            crate::platform::open::open_file(p);
        } else {
            crate::platform::open::reveal(&disk);
        }
    }

    fn scroll_by(&mut self, d: f32) {
        let max = (self.wrapped.len() as f32 - self.rows).max(0.0);
        self.scroll = (self.scroll + d).clamp(0.0, max);
    }

    fn draw(&mut self, ctx: &Ctx, d: &mut Draw) {
        let (w, h, ui) = (ctx.view.width, ctx.view.height, ctx.ui());
        d.b.rect(Rect::new(0.0, 0.0, w, h), [0.0, 0.0, 0.0, 0.66]);
        let m = 24.0 * ui;
        let r = Rect::new(m, m, w - 2.0 * m, h - 2.0 * m);
        crate::ui::panel(d, r);
        let pad = 18.0 * ui;
        d.text(
            tr!("READ TEXT"),
            r.x + pad,
            r.y + 14.0 * ui,
            TextStyle::new(20.0 * ui, theme::TEXT, Align::Left),
        );
        d.text(
            &self.title,
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
        if crate::platform::open::CAN_OPEN_FILES {
            // OPEN (O): the file shown in the system's viewer
            let open = Rect::new(close.x - 96.0 * ui, r.y + 12.0 * ui, 88.0 * ui, 32.0 * ui);
            crate::ui::button(d, open, tr!("OPEN (O)"), ui, false);
            self.hits.push(open, Hit::Open);
        }
        let body = Rect::new(
            r.x + pad,
            r.y + 56.0 * ui,
            r.w - 2.0 * pad,
            r.h - 56.0 * ui - pad,
        );
        let (size, line) = (SIZE * ui, LINE * ui);
        if self.fetch.is_some() {
            d.text(
                tr!("Reading\u{2026}"),
                body.x,
                body.y,
                TextStyle::new(size, theme::TEXT_DIM, Align::Left),
            );
            return;
        }
        self.wrap(d, body.w - 14.0 * ui, size);
        self.rows = (body.h / line).floor().max(1.0);
        self.scroll_by(0.0);
        d.b.set_clip(Some(body));
        let first = self.scroll.floor() as usize;
        let off = (self.scroll - first as f32) * line;
        for (k, l) in self.wrapped.iter().enumerate().skip(first) {
            let y = body.y + (k - first) as f32 * line - off;
            if y > body.bottom() {
                break;
            }
            d.text(l, body.x, y, TextStyle::new(size, theme::TEXT, Align::Left));
        }
        d.b.set_clip(None);
        // where the view is in the text
        let n = self.wrapped.len() as f32;
        if n > self.rows {
            let th = (body.h * self.rows / n).max(24.0 * ui);
            let ty = body.y + (body.h - th) * self.scroll / (n - self.rows);
            let track = Rect::new(body.right() - 5.0 * ui, body.y, 5.0 * ui, body.h);
            d.b.rect(track, [1.0, 1.0, 1.0, 0.06]);
            d.b.rect(Rect::new(track.x, ty, track.w, th), theme::ACCENT);
        }
    }

    /// ↑↓ (the turntable) scroll, the other keys close; false: closed.
    fn nav(&mut self, nav: Option<Nav>) -> bool {
        match nav {
            Some(Nav::Up) => self.scroll_by(-1.0),
            Some(Nav::Down) => self.scroll_by(1.0),
            Some(_) => return false,
            None => {}
        }
        true
    }

    /// An input while the text is open; false: it closes.
    fn input(&mut self, ctx: &Ctx, ev: &InputEv) -> bool {
        let page = (self.rows - 1.0).max(1.0);
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                ..
            } => match code {
                KeyCode::PageUp => self.scroll_by(-page),
                KeyCode::PageDown | KeyCode::Space => self.scroll_by(page),
                KeyCode::Home => self.scroll = 0.0,
                KeyCode::End => self.scroll_by(f32::MAX),
                KeyCode::KeyT => return false,
                KeyCode::KeyO => self.open_shown(),
                c => return self.nav(nav_from_key(*c, &ctx.config.keys)),
            },
            InputEv::Pad {
                id, pressed: true, ..
            } => return self.nav(nav_from_pad(*id, &ctx.config.keys)),
            InputEv::Wheel { dy } => self.scroll_by(-dy * 3.0),
            InputEv::Pointer {
                id, phase, x, y, ..
            } => {
                let line = LINE * ctx.ui();
                match phase {
                    Phase::Down => self.drag = Some((*id, *y, self.scroll, false)),
                    Phase::Move => {
                        if let Some((i, y0, s0, moved)) = self.drag.as_mut() {
                            if *i == *id {
                                *moved |= (y - *y0).abs() > DRAG_SLOP * ctx.ui();
                                if *moved {
                                    let s = *s0 - (y - *y0) / line;
                                    self.scroll = s;
                                    self.scroll_by(0.0);
                                }
                            }
                        }
                    }
                    Phase::Up => {
                        let tap = self.drag.take().is_some_and(|d| d.0 == *id && !d.3);
                        // a tap outside the box or on its close button
                        match self.hits.at(*x, *y).copied().filter(|_| tap) {
                            None if tap => return false,
                            Some(Hit::Close) => return false,
                            Some(Hit::Open) => self.open_shown(),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
        true
    }
}

impl SelectScene {
    /// Open the selected chart's text files.
    pub(super) fn read_text(&mut self, ctx: &mut Ctx) {
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        let files = self.chart_files(ctx, song).texts.clone();
        if files.is_empty() {
            ctx.toast(tr!("{title}: no text files", title = song.title));
            return;
        }
        self.reader = Some(Reader::new(song.full_title(), files));
    }

    pub(super) fn update_reader(&mut self) {
        if let Some(r) = self.reader.as_mut() {
            r.update();
        }
    }

    pub(super) fn draw_reader(&mut self, ctx: &Ctx, d: &mut Draw) {
        if let Some(r) = self.reader.as_mut() {
            r.draw(ctx, d);
        }
    }

    /// An input for the open text: true when it had one.
    pub(super) fn reader_input(&mut self, ctx: &Ctx, ev: &InputEv) -> bool {
        let Some(r) = self.reader.as_mut() else {
            return false;
        };
        if !r.input(ctx, ev) {
            self.reader = None;
        }
        true
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The chart's text files are read (Shift_JIS too), each after its
    /// name when there are several.
    #[test]
    fn reads_the_files() {
        let dir = std::env::temp_dir().join(format!("vibebms-reader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        // "曲" in Shift_JIS
        std::fs::write(&a, b"\x8b\xc8\r\nline\ttwo").unwrap();
        std::fs::write(&b, "third").unwrap();
        let mut r = Reader::new("song".into(), vec![a, b]);
        r.update();
        assert_eq!(
            r.lines,
            [
                "\u{2014} a.txt \u{2014}",
                "\u{66f2}",
                "line    two",
                "",
                "\u{2014} b.txt \u{2014}",
                "third"
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
