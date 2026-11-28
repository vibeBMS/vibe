//! Where everything goes (`place`, which is also what taps hit) and how
//! it is drawn: the top level's cards, a page's rail and rows, the header
//! with its breadcrumb.

use super::row::{Act, Kind, Row, Tone};
use super::{online, skins, Page, SettingsScene, Target, View, SESSION};
use crate::ctx::Ctx;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::theme::{ACCENT, BACKGROUND, PANEL, TEXT, TEXT_DIM};
use crate::gfx::{mix_opaque, rgba, with_alpha, Color, Draw, Rect};
use crate::ui;
use crate::ui::{ellipsize, wrap};

/// A row with a description (UI units); the others are sized by kind.
pub(super) const ROW_H: f32 = 52.0;
const HEADER: f32 = 56.0;
const GAP: f32 = 6.0;
/// Rows inside a group are indented this much.
const INDENT: f32 = 24.0;
const CELL_H: f32 = 40.0;

const AMBER: Color = rgba(0xffc94aff);
const RED: Color = rgba(0xff5460ff);
const BLUE: Color = rgba(0x409cffff);
const VIOLET: Color = rgba(0xaa78ffff);
const GREEN: Color = rgba(0x5adc78ff);
const PINK: Color = rgba(0xff4fa3ff);
const ORANGE: Color = rgba(0xff963cff);
const SLATE: Color = rgba(0x9aa4c4ff);

fn page_color(p: Page) -> Color {
    match p {
        Page::Gameplay => ACCENT,
        Page::Controls => AMBER,
        Page::Display => BLUE,
        Page::Skins => VIOLET,
        Page::Library => GREEN,
        Page::Courses => PINK,
        Page::Online => ORANGE,
        Page::System => SLATE,
    }
}

fn tone_color(t: Tone) -> Color {
    match t {
        Tone::Accent => ACCENT,
        Tone::Amber => AMBER,
        Tone::Red => RED,
        Tone::Dim => TEXT_DIM,
    }
}

fn solid() -> Color {
    [PANEL[0], PANEL[1], PANEL[2], 1.0]
}

fn edge() -> Color {
    with_alpha(TEXT_DIM, 0.35)
}

/// What the top level and the rail show besides the rows: each page's
/// chips and number, the recent changes, what needs the player.
#[derive(Default)]
pub(super) struct Status {
    chips: Vec<Vec<String>>,
    /// The rail's number beside a page.
    badges: Vec<String>,
    recent: Vec<String>,
    attention: Vec<String>,
}

impl Status {
    pub(super) fn read(ctx: &Ctx) -> Status {
        let updates = skins::updates(ctx);
        let waiting = online::waiting(ctx);
        let mut attention = Vec::new();
        match updates {
            0 => {}
            1 => attention.push(tr!("1 skin update").into()),
            n => attention.push(tr!("{n} skin updates", n = n)),
        }
        online::attention(ctx, &mut attention);
        if let crate::update::Status::Available(o) = &ctx.net.updater.status {
            attention.push(tr!("vibeBMS {version} to install", version = o.version));
        }
        Status {
            chips: Page::ALL.iter().map(|p| p.chips(ctx)).collect(),
            badges: Page::ALL
                .iter()
                .map(|p| match p {
                    Page::Skins if updates > 0 => format!("{updates} ↓"),
                    Page::Online if waiting > 0 => waiting.to_string(),
                    _ => String::new(),
                })
                .collect(),
            recent: SESSION.with(|s| s.borrow().recent.clone()),
            attention,
        }
    }
}

/// Where the last frame put things.
#[derive(Default)]
pub(super) struct Layout {
    /// The rows' area (a page's, or search's results).
    pub(super) content: Rect,
    /// Each row, scrolled (rows out of the area too).
    pub(super) pos: Vec<Rect>,
    pub(super) rail: Vec<(Rect, Page)>,
    pub(super) cards: Vec<(Rect, Page)>,
    pub(super) search: Rect,
    pub(super) back: Rect,
    /// The header's "SETTINGS" (to the top level).
    pub(super) home: Rect,
    pub(super) links: Vec<(Rect, &'static str)>,
}

fn height(row: &Row, portrait: bool) -> f32 {
    match row.kind {
        Kind::Header(_) => 38.0,
        Kind::Group { .. } => 58.0,
        Kind::Banner if portrait => 64.0,
        Kind::Banner => 46.0,
        Kind::Card { .. } => 74.0,
        Kind::Cell { .. } => CELL_H,
        Kind::Item if row.desc.is_empty() => 44.0,
        Kind::Item if two_lines(row, portrait) => 72.0,
        Kind::Item => ROW_H,
    }
}

/// A long description takes two lines in portrait.
fn two_lines(row: &Row, portrait: bool) -> bool {
    portrait && row.desc.chars().count() > 96
}

/// The value as drawn: ◀ ▶ around one ←→ change, › after one Enter opens.
fn value_text(row: &Row) -> String {
    let v = &row.value;
    match row.act {
        _ if matches!(row.kind, Kind::Cell { .. } | Kind::Card { .. }) => String::new(),
        a if a.adjusts() => format!("◀  {v}  ▶"),
        _ if row.open.is_some() && v.is_empty() => "›".into(),
        Act::Text(_) | Act::Bind(_) | Act::Open(_) => format!("{v}  ›"),
        _ if row.open.is_some() => format!("{v}  ›"),
        _ => v.clone(),
    }
}

/// Text pieces in their colours, one after another; the width drawn.
fn spans(d: &mut Draw, x: f32, y: f32, size: f32, parts: &[(&str, Color)]) -> f32 {
    let mut at = x;
    for (s, c) in parts {
        at += d.text(s, at, y, TextStyle::new(size, *c, Align::Left));
    }
    at - x
}

/// A small magnifier.
fn magnifier(d: &mut Draw, x: f32, cy: f32, ui: f32, color: Color) {
    let r = 7.0 * ui;
    d.b.circle(x + r, cy - 1.5 * ui, r, r - 2.0 * ui, color, 24);
    for k in 0..4 {
        let t = (5.0 + k as f32 * 1.6) * ui;
        d.b.rect(
            Rect::new(
                x + r + t * 0.75,
                cy - 1.5 * ui + t * 0.75,
                2.4 * ui,
                2.4 * ui,
            ),
            color,
        );
    }
}

/// A rounded tag with `s` in it, right-aligned at `right`; its width.
fn chip(d: &mut Draw, right: f32, cy: f32, s: &str, color: Color, ui: f32) -> f32 {
    let size = 14.0 * ui;
    let w = d.measure(s, size) + 22.0 * ui;
    let r = Rect::new(right - w, cy - 13.0 * ui, w, 26.0 * ui);
    d.b.rounded_rect(r, 13.0 * ui, with_alpha(color, 0.14));
    d.text(
        s,
        r.x + 11.0 * ui,
        cy - size * 0.58,
        TextStyle::new(size, color, Align::Left),
    );
    w
}

/// A chip's colour: what waits is amber (the chip has one of these words:
/// their translations are words the translated chips have).
fn chip_color(s: &str) -> Color {
    if [tr!("update"), tr!("waiting"), tr!("failed")]
        .iter()
        .any(|w| s.contains(w))
    {
        AMBER
    } else {
        TEXT
    }
}

impl SettingsScene {
    /// The top level's cards, unless search is open.
    fn cards_shown(&self) -> bool {
        self.view == View::Top && self.search.is_none()
    }

    /// Lay the frame out: rects for drawing, and for taps.
    pub(super) fn place(&mut self, ctx: &Ctx) {
        let ui = ctx.ui();
        let w = ctx.view.width;
        let portrait = ctx.portrait();
        let u = |v: f32| v * ui;
        let mut lay = Layout {
            back: Rect::new(w - u(124.0), u(9.0), u(112.0), u(38.0)),
            home: Rect::new(0.0, 0.0, u(200.0), u(HEADER)),
            ..Layout::default()
        };
        self.hits.clear();
        if self.cards_shown() {
            let cols = if portrait { 2 } else { 4 };
            let (x0, gap) = (u(if portrait { 16.0 } else { 24.0 }), u(14.0));
            lay.search = Rect::new(x0, u(72.0), w - 2.0 * x0, u(52.0));
            let cw = (w - 2.0 * x0 - gap * (cols - 1) as f32) / cols as f32;
            let ch = u(if portrait { 196.0 } else { 200.0 });
            for (i, p) in Page::ALL.into_iter().enumerate() {
                let (c, l) = (i % cols, i / cols);
                let r = Rect::new(
                    x0 + c as f32 * (cw + gap),
                    u(140.0) + l as f32 * (ch + gap),
                    cw,
                    ch,
                );
                lay.cards.push((r, p));
                self.hits.push(r, Target::Page(p));
            }
            self.card_cols = cols;
        } else {
            self.place_rows(ctx, &mut lay);
        }
        self.hits.push(lay.search, Target::Search);
        if self.view != View::Top || self.search.is_some() {
            self.hits.push(lay.home, Target::Home);
        }
        self.hits.push(lay.back, Target::Back);
        self.lay = lay;
    }

    fn place_rows(&mut self, ctx: &Ctx, lay: &mut Layout) {
        let ui = ctx.ui();
        let (w, h) = (ctx.view.width, ctx.view.height);
        let portrait = ctx.portrait();
        let u = |v: f32| v * ui;
        if portrait {
            lay.search = Rect::new(u(16.0), u(64.0), w - u(32.0), u(44.0));
            let bw = (w - u(32.0) - u(18.0)) / 4.0;
            for (i, p) in Page::ALL.into_iter().enumerate() {
                let r = Rect::new(
                    u(16.0) + (i % 4) as f32 * (bw + u(6.0)),
                    u(116.0) + (i / 4) as f32 * u(46.0),
                    bw,
                    u(40.0),
                );
                lay.rail.push((r, p));
            }
            lay.content = Rect::new(u(16.0), u(214.0), w - u(32.0), h - u(220.0));
        } else {
            let rw = u(236.0);
            lay.search = Rect::new(u(12.0), u(68.0), rw, u(44.0));
            for (i, p) in Page::ALL.into_iter().enumerate() {
                let r = Rect::new(u(12.0), u(124.0) + i as f32 * u(56.0), rw, u(48.0));
                lay.rail.push((r, p));
            }
            lay.content = Rect::new(u(268.0), u(68.0), w - u(268.0) - u(18.0), h - u(76.0));
        }
        let c = lay.content;
        let cols = if portrait { 2 } else { 3 };
        self.grid_cols = cols;
        let cell_w = (c.w - u(GAP) * (cols - 1) as f32) / cols as f32;
        let mut y = 0.0;
        let mut i = 0;
        while i < self.rows.len() {
            let row = &self.rows[i];
            if let Some((start, len)) = self.grid(i) {
                for k in 0..len {
                    let (col, line) = ((k % cols) as f32, (k / cols) as f32);
                    lay.pos.push(Rect::new(
                        c.x + col * (cell_w + u(GAP)),
                        y + line * u(CELL_H + GAP),
                        cell_w,
                        u(CELL_H),
                    ));
                }
                y += len.div_ceil(cols) as f32 * u(CELL_H + GAP);
                i = start + len;
                continue;
            }
            if matches!(row.kind, Kind::Header(_)) && i > 0 {
                y += u(8.0);
            }
            let indent = if row.group.is_some() && self.search.is_none() {
                u(INDENT)
            } else {
                0.0
            };
            let rh = u(height(row, portrait));
            lay.pos.push(Rect::new(c.x + indent, y, c.w - indent, rh));
            y += rh + u(GAP);
            i += 1;
        }
        let content_h = y + u(8.0);
        if let Some(r) = lay.pos.get(self.list.sel) {
            // keep the chosen row in view, with a row below it
            self.list.fit(r.y, r.bottom() + u(ROW_H), c.h, content_h);
        } else {
            self.list.fit(0.0, 0.0, c.h, content_h);
        }
        let dy = c.y - self.list.scroll;
        for (i, r) in lay.pos.iter_mut().enumerate() {
            r.y += dy;
            let vis = Rect::new(
                r.x,
                r.y.max(c.y),
                r.w,
                r.bottom().min(c.bottom()) - r.y.max(c.y),
            );
            if vis.h <= 0.0 {
                continue;
            }
            let row = &self.rows[i];
            if let Kind::Header(links) = &row.kind {
                let mut right = r.right();
                for (name, act) in links.iter().rev() {
                    let lw = u(12.0 + 9.0 * name.len() as f32);
                    let lr = Rect::new(right - lw, r.y, lw, r.h);
                    lay.links.push((lr, name));
                    self.hits.push(lr, Target::Link(*act));
                    right -= lw + u(4.0);
                }
                continue;
            }
            // the value's middle, about: a tap left of it lowers it
            let chars = value_text(row).chars().count() as f32;
            let mid = r.right() - u(16.0) - chars * u(4.6);
            self.hits.push(vis, Target::Row(i, mid));
        }
        for (r, p) in &lay.rail {
            self.hits.push(*r, Target::Page(*p));
        }
    }

    pub(super) fn draw_all(&mut self, ctx: &Ctx, d: &mut Draw) {
        let (w, h) = (ctx.view.width, ctx.view.height);
        d.b.gradient(
            Rect::new(0.0, 0.0, w, h),
            ui::lift(BACKGROUND, 0.04),
            BACKGROUND,
        );
        if self.cards_shown() {
            self.draw_top(ctx, d);
        } else {
            self.draw_rail(ctx, d);
            self.draw_rows(ctx, d);
        }
        self.draw_header(ctx, d);
        if let Some((tf, t)) = self.edit.as_mut() {
            t.draw_modal(
                ctx,
                d,
                tf.title(),
                matches!(
                    tf,
                    super::TextField::IrKey(_) | super::TextField::ObsPassword
                ),
            );
        }
        if let Some((q, n)) = self.ask.as_mut() {
            n.draw(ctx, d, q.title(), q.text(), tr!("Cancel"));
        }
        if let Some(r) = self.reader.as_mut() {
            r.draw(ctx, d, None);
        }
        if let Some(slot) = self.learn.slot {
            let ui = ctx.ui();
            let r = Rect::new(w * 0.2, h * 0.38, w * 0.6, 90.0 * ui);
            ui::modal(d, w, h, r);
            let what = if matches!(slot, crate::scene::keybind::Slot::ScratchAxis(_)) {
                tr!("Spin the turntable")
            } else if matches!(
                self.view,
                super::View::Sub(super::Sub::Keys(super::KeySet::SideSub(_)))
            ) {
                tr!("Press a key")
            } else {
                tr!("Press a key or controller button")
            };
            let x = r.x + r.w * 0.5;
            d.text(
                what,
                x,
                r.y + 18.0 * ui,
                TextStyle::new(22.0 * ui, TEXT, Align::Center),
            );
            d.text(
                tr!("Esc = cancel, Delete = clear"),
                x,
                r.y + 54.0 * ui,
                TextStyle::new(15.0 * ui, TEXT_DIM, Align::Center),
            );
        }
    }

    fn draw_header(&mut self, ctx: &Ctx, d: &mut Draw) {
        let ui = ctx.ui();
        let w = ctx.view.width;
        let u = |v: f32| v * ui;
        d.b.rect(Rect::new(0.0, 0.0, w, u(HEADER)), solid());
        d.b.rect(
            Rect::new(0.0, u(HEADER) - 2.0, w, 2.0),
            with_alpha(ACCENT, 0.6),
        );
        let x = u(24.0);
        let tw = d.text(
            tr!("SETTINGS"),
            x,
            u(12.0),
            TextStyle::new(u(28.0), ACCENT, Align::Left),
        );
        let mut crumbs: Vec<String> = if self.search.is_some() {
            vec![tr!("SEARCH").into()]
        } else {
            let parents = self.parents.iter().map(|p| p.0.title(ctx));
            parents
                .chain([self.view.title(ctx)])
                .filter(|t| !t.is_empty())
                .collect()
        };
        let last = crumbs.pop().unwrap_or_default();
        let back = self.lay.back;
        let portrait = ctx.portrait();
        let room = back.x - x - tw - u(36.0);
        let mut cx = x + tw + u(18.0);
        let size = u(19.0);
        for c in &crumbs {
            cx += spans(d, cx, u(18.0), size, &[(c, TEXT_DIM), (" › ", TEXT_DIM)]);
        }
        let last = ellipsize(
            d,
            &last,
            size,
            (room - (cx - x - tw - u(18.0))).max(u(40.0)),
        );
        cx += d.text(&last, cx, u(18.0), TextStyle::new(size, TEXT, Align::Left));
        if !portrait {
            let hints = if self.cards_shown() {
                tr!("↑↓←→ move   Enter open   / search   Esc back to the game")
            } else if self.search.is_some() {
                tr!("↑↓ results   ←→ change   Enter go there   Esc close")
            } else if self.in_grid() {
                tr!("←→↑↓ move in the grid   Enter toggle   Esc back")
            } else {
                tr!("↑↓ select   ←→ change   Enter open   Esc back   / search")
            };
            let hs = u(15.0);
            if d.measure(hints, hs) < back.x - cx - u(40.0) {
                d.text(
                    hints,
                    back.x - u(20.0),
                    u(19.0),
                    TextStyle::new(hs, TEXT_DIM, Align::Right),
                );
            }
        }
        ui::button(d, back, tr!("◀ BACK"), ui, false);
    }

    /// The search field at `r` (the wide one on the top level).
    fn draw_search(&mut self, ctx: &Ctx, d: &mut Draw, r: Rect, wide: bool) {
        let ui = ctx.ui();
        let on = self.search.is_some();
        let border = if on { ACCENT } else { edge() };
        d.b.rounded_panel(r, 8.0 * ui, solid(), border, if on { 1.5 } else { 1.0 });
        let cy = r.y + r.h * 0.5;
        magnifier(
            d,
            r.x + 16.0 * ui,
            cy,
            ui,
            if on { ACCENT } else { TEXT_DIM },
        );
        let size = (if wide { 19.0 } else { 16.0 }) * ui;
        let tx = r.x + 44.0 * ui;
        let tw = r.w - 56.0 * ui;
        match &self.search {
            Some(s) => {
                // an on-screen keyboard comes up
                d.text_input = true;
                let shown = ellipsize(d, &format!("{}_", s.input.buf), size, tw);
                d.text(
                    &shown,
                    tx,
                    cy - size * 0.6,
                    TextStyle::new(size, TEXT, Align::Left),
                );
            }
            None => {
                let hint = if wide {
                    tr!("Search every setting — e.g. “offset”, “volume”, “BMS-IR”")
                } else {
                    tr!("Search settings")
                };
                let hint = ellipsize(d, hint, size, tw);
                d.text(
                    &hint,
                    tx,
                    cy - size * 0.6,
                    TextStyle::new(size, TEXT_DIM, Align::Left),
                );
                if wide {
                    let k = Rect::new(r.right() - 44.0 * ui, cy - 13.0 * ui, 26.0 * ui, 26.0 * ui);
                    d.b.rounded_panel(k, 5.0 * ui, solid(), edge(), 1.0);
                    d.text(
                        "/",
                        k.x + k.w * 0.5,
                        k.y + 4.0 * ui,
                        TextStyle::new(15.0 * ui, TEXT_DIM, Align::Center),
                    );
                }
            }
        }
    }

    fn draw_top(&mut self, ctx: &Ctx, d: &mut Draw) {
        let ui = ctx.ui();
        let (w, h) = (ctx.view.width, ctx.view.height);
        let u = |v: f32| v * ui;
        let portrait = ctx.portrait();
        let search = self.lay.search;
        self.draw_search(ctx, d, search, true);
        for (i, (r, p)) in self.lay.cards.iter().enumerate() {
            let (r, p) = (*r, *p);
            let sel = i == self.list.sel;
            let color = page_color(p);
            let (border, t) = if sel {
                (ACCENT, 2.0 * ui)
            } else {
                (edge(), 1.0)
            };
            d.b.rounded_panel(r, u(10.0), solid(), border, t);
            d.b.rect(
                Rect::new(r.x + u(10.0), r.y + t, r.w - u(20.0), u(3.0)),
                color,
            );
            let (badge, sub, about) = p.about();
            let tile = Rect::new(r.x + u(22.0), r.y + u(26.0), u(48.0), u(48.0));
            d.b.rounded_panel(
                tile,
                u(8.0),
                mix_opaque(solid(), color, 0.16),
                with_alpha(color, 0.6),
                1.0,
            );
            d.text(
                badge,
                tile.x + tile.w * 0.5,
                tile.y + u(13.0),
                TextStyle::new(u(19.0), color, Align::Center),
            );
            let tx = tile.right() + u(16.0);
            let title_w = r.right() - tx - u(12.0);
            d.text(
                p.name(),
                tx,
                r.y + u(26.0),
                TextStyle::new(u(22.0), TEXT, Align::Left).fit(title_w),
            );
            d.text(
                sub,
                tx,
                r.y + u(56.0),
                TextStyle::new(u(13.0), TEXT_DIM, Align::Left).fit(title_w),
            );
            let size = u(14.5);
            let lines = wrap(d, about, size, r.w - u(44.0), 3);
            for (k, line) in lines.iter().enumerate() {
                d.text(
                    line,
                    r.x + u(22.0),
                    r.y + u(88.0) + k as f32 * u(20.0),
                    TextStyle::new(size, mix_opaque(TEXT, solid(), 0.12), Align::Left),
                );
            }
            // the chips, left to right
            let cy = r.bottom() - u(26.0);
            let mut x = r.x + u(22.0);
            for c in self.status.chips.get(i).into_iter().flatten() {
                let cw = d.measure(c, u(14.0)) + u(22.0);
                if x + cw > r.right() - u(12.0) {
                    break;
                }
                chip(d, x + cw, cy, c, chip_color(c), ui);
                x += cw + u(8.0);
            }
        }
        let x0 = search.x;
        let full = w - 2.0 * x0;
        let last = self.lay.cards.last().map_or(u(560.0), |c| c.0.bottom());
        let by = last + u(14.0);
        let bh = u(if portrait { 92.0 } else { 50.0 });
        let banner = Rect::new(x0, by, full, bh);
        d.b.rounded_panel(
            banner,
            u(8.0),
            mix_opaque(solid(), ACCENT, 0.07),
            with_alpha(ACCENT, 0.45),
            1.0,
        );
        let parts: [(&str, Color); 7] = [
            (tr!("Random, gauge, hi-speed, lane cover, target: "), TEXT),
            ("PLAY OPTION", ACCENT),
            (tr!(" (hold START), "), TEXT),
            ("ASSIST", ACCENT),
            (tr!(" (hold SELECT), "), TEXT),
            ("SYSTEM OPTION", ACCENT),
            (tr!(" (hold both), or tap the buttons."), TEXT),
        ];
        if portrait {
            d.text(
                tr!("ON THE SONG SELECT"),
                x0 + u(20.0),
                by + u(12.0),
                TextStyle::new(u(14.0), ACCENT, Align::Left),
            );
            let size = u(14.0);
            spans(d, x0 + u(20.0), by + u(38.0), size, &parts[..1]);
            spans(d, x0 + u(20.0), by + u(62.0), size, &parts[1..6]);
        } else {
            let tw = spans(
                d,
                x0 + u(22.0),
                by + u(16.0),
                u(15.0),
                &[(&format!("{}   ", tr!("ON THE SONG SELECT")), ACCENT)],
            );
            let room = full - tw - u(44.0);
            let size = if d.measure(&parts.iter().map(|p| p.0).collect::<String>(), u(16.0)) > room
            {
                u(14.0)
            } else {
                u(16.0)
            };
            spans(d, x0 + u(22.0) + tw, by + u(15.0), size, &parts);
        }
        // recently changed, needs attention
        let sy = by + bh + u(12.0);
        let strips = [
            (
                tr!("RECENTLY CHANGED"),
                &self.status.recent,
                tr!("Nothing yet"),
            ),
            (
                tr!("NEEDS ATTENTION"),
                &self.status.attention,
                tr!("Nothing"),
            ),
        ];
        let sh = u(84.0);
        for (k, (title, items, none)) in strips.into_iter().enumerate() {
            let r = if portrait {
                Rect::new(x0, sy + k as f32 * (sh + u(10.0)), full, sh)
            } else {
                let sw = (full - u(14.0)) / 2.0;
                Rect::new(
                    x0 + k as f32 * (sw + u(14.0)),
                    sy,
                    sw,
                    (h - sy - u(12.0)).min(sh),
                )
            };
            if r.h < u(48.0) || r.y > h {
                continue;
            }
            d.b.rounded_panel(r, u(8.0), solid(), edge(), 1.0);
            d.text(
                title,
                r.x + u(20.0),
                r.y + u(12.0),
                TextStyle::new(u(13.0), TEXT_DIM, Align::Left),
            );
            let line = if items.is_empty() {
                none.to_string()
            } else {
                items.join(" · ")
            };
            let line = ellipsize(d, &line, u(17.0), r.w - u(40.0));
            let c = if items.is_empty() { TEXT_DIM } else { TEXT };
            d.text(
                &line,
                r.x + u(20.0),
                r.y + u(40.0),
                TextStyle::new(u(17.0), c, Align::Left),
            );
        }
    }

    fn draw_rail(&mut self, ctx: &Ctx, d: &mut Draw) {
        let ui = ctx.ui();
        let u = |v: f32| v * ui;
        let portrait = ctx.portrait();
        let search = self.lay.search;
        self.draw_search(ctx, d, search, false);
        let cur = if self.search.is_some() {
            None
        } else {
            self.page()
        };
        let rail_focus = self.rail && self.search.is_none();
        for (i, &(r, p)) in self.lay.rail.iter().enumerate() {
            let active = Some(p) == cur;
            let (fill, border) = if active {
                (mix_opaque(solid(), ACCENT, 0.2), ACCENT)
            } else {
                (ui::lift(solid(), 0.04), edge())
            };
            d.b.rounded_panel(r, u(7.0), fill, border, if active { 1.5 } else { 1.0 });
            let size = u(if portrait { 13.0 } else { 16.0 });
            let label = TextStyle::new(size, TEXT, Align::Left);
            if portrait {
                d.text(
                    p.name(),
                    r.x + r.w * 0.5,
                    r.y + (r.h - size) * 0.5,
                    TextStyle::new(size, TEXT, Align::Center).fit(r.w - u(8.0)),
                );
            } else {
                d.text(
                    p.name(),
                    r.x + u(20.0),
                    r.y + (r.h - size) * 0.5 - u(1.0),
                    label,
                );
            }
            if let Some(b) = self.status.badges.get(i).filter(|b| !b.is_empty()) {
                let bs = u(13.0);
                d.text(
                    b,
                    r.right() - u(10.0),
                    r.y + u(4.0).max((r.h - bs) * 0.5),
                    TextStyle::new(bs, AMBER, Align::Right),
                );
            }
            if active && rail_focus {
                d.b.rect(
                    Rect::new(r.x - u(5.0), r.y + u(8.0), u(3.0), r.h - u(16.0)),
                    ACCENT,
                );
            }
        }
        if !portrait {
            if let Some(last) = self.lay.rail.last() {
                let y = last.0.bottom() + u(18.0);
                let w = last.0.w;
                let lines = wrap(
                    d,
                    tr!("Per-play options (random, gauge, hi-speed, lane cover, target) are on the song select: PLAY OPTION."),
                    u(13.0),
                    w,
                    4,
                );
                for (k, l) in lines.iter().enumerate() {
                    d.text(
                        l,
                        u(12.0),
                        y + k as f32 * u(19.0),
                        TextStyle::new(u(13.0), TEXT_DIM, Align::Left),
                    );
                }
            }
        }
    }

    fn draw_rows(&mut self, ctx: &Ctx, d: &mut Draw) {
        let ui = ctx.ui();
        let u = |v: f32| v * ui;
        let c = self.lay.content;
        let focus = self.on_rows();
        let portrait = ctx.portrait();
        d.b.set_clip(Some(c));
        if self.rows.is_empty() {
            let msg = match &self.search {
                Some(s) if s.input.buf.trim().is_empty() => {
                    tr!("Type to search every setting: a name, a word of its description, a key.")
                        .to_string()
                }
                Some(s) => tr!("Nothing found for “{query}”", query = s.input.buf.trim()),
                None => String::new(),
            };
            d.text(
                &msg,
                c.x + u(8.0),
                c.y + u(14.0),
                TextStyle::new(u(16.0), TEXT_DIM, Align::Left),
            );
        }
        for i in 0..self.rows.len() {
            let Some(&r) = self.lay.pos.get(i) else {
                break;
            };
            if r.bottom() < c.y || r.y > c.bottom() {
                continue;
            }
            let sel = focus && i == self.list.sel;
            self.draw_row(d, i, r, sel, ui, portrait);
        }
        d.b.set_clip(None);
        // a scrollbar when the rows go past the area
        let content_h = self
            .lay
            .pos
            .last()
            .map_or(0.0, |r| r.bottom() + self.list.scroll - c.y);
        if content_h > c.h + 1.0 {
            let track = Rect::new(c.right() + u(8.0), c.y, u(4.0), c.h);
            d.b.rounded_rect(track, u(2.0), with_alpha(TEXT_DIM, 0.15));
            let th = (c.h / content_h * c.h).max(u(30.0));
            let ty = c.y + self.list.scroll / (content_h - c.h) * (c.h - th);
            d.b.rounded_rect(
                Rect::new(track.x, ty, track.w, th),
                u(2.0),
                with_alpha(TEXT_DIM, 0.6),
            );
        }
    }

    fn draw_row(&self, d: &mut Draw, i: usize, r: Rect, sel: bool, ui: f32, portrait: bool) {
        let u = |v: f32| v * ui;
        let row = &self.rows[i];
        match &row.kind {
            Kind::Header(_) => {
                d.text(
                    &row.label,
                    r.x + u(2.0),
                    r.y + u(14.0),
                    TextStyle::new(u(14.0), TEXT_DIM, Align::Left),
                );
                d.b.rect(
                    Rect::new(r.x, r.bottom() - u(4.0), r.w, 1.0),
                    with_alpha(TEXT_DIM, 0.25),
                );
                for (lr, name) in &self.lay.links {
                    if lr.y == r.y {
                        d.text(
                            name,
                            lr.right(),
                            r.y + u(13.0),
                            TextStyle::new(u(15.0), ACCENT, Align::Right),
                        );
                    }
                }
            }
            Kind::Banner => {
                d.b.rounded_panel(
                    r,
                    u(8.0),
                    mix_opaque(solid(), ACCENT, 0.08),
                    with_alpha(ACCENT, 0.45),
                    1.0,
                );
                let size = u(16.0);
                if portrait {
                    d.text(
                        &row.label,
                        r.x + u(18.0),
                        r.y + u(10.0),
                        TextStyle::new(size, ACCENT, Align::Left),
                    );
                    d.text(
                        &row.value,
                        r.x + u(18.0),
                        r.y + u(34.0),
                        TextStyle::new(u(14.0), TEXT, Align::Left).fit(r.w - u(36.0)),
                    );
                } else {
                    let y = r.y + (r.h - size) * 0.5 - u(1.0);
                    spans(
                        d,
                        r.x + u(18.0),
                        y,
                        size,
                        &[(&row.label, ACCENT), ("   ", TEXT), (&row.value, TEXT)],
                    );
                }
            }
            Kind::Group { open, .. } => {
                let border = if sel {
                    ACCENT
                } else {
                    with_alpha(TEXT_DIM, 0.4)
                };
                d.b.rounded_panel(
                    r,
                    u(8.0),
                    mix_opaque(solid(), BACKGROUND, 0.4),
                    border,
                    if sel { 1.5 } else { 1.0 },
                );
                let arrow = if *open { "▾" } else { "▸" };
                spans(
                    d,
                    r.x + u(18.0),
                    r.y + u(9.0),
                    u(17.0),
                    &[(arrow, TEXT), ("  ", TEXT), (&row.label, TEXT)],
                );
                let desc = ellipsize(d, &row.desc, u(13.5), r.w - u(180.0));
                d.text(
                    &desc,
                    r.x + u(42.0),
                    r.y + u(33.0),
                    TextStyle::new(u(13.5), TEXT_DIM, Align::Left),
                );
                let hint = if *open {
                    tr!("Enter: fold")
                } else {
                    tr!("Enter: open")
                };
                d.text(
                    hint,
                    r.right() - u(18.0),
                    r.y + (r.h - u(15.0)) * 0.5,
                    TextStyle::new(u(15.0), TEXT_DIM, Align::Right),
                );
            }
            Kind::Cell { on } => {
                let (fill, border, t) = match (*on, sel) {
                    (_, true) => (
                        mix_opaque(solid(), ACCENT, if *on { 0.16 } else { 0.06 }),
                        ACCENT,
                        2.0,
                    ),
                    (true, false) => (
                        mix_opaque(solid(), ACCENT, 0.1),
                        with_alpha(ACCENT, 0.55),
                        1.0,
                    ),
                    (false, false) => (ui::lift(solid(), 0.03), edge(), 1.0),
                };
                d.b.rounded_panel(r, u(6.0), fill, border, t);
                let b = Rect::new(r.x + u(12.0), r.y + (r.h - u(18.0)) * 0.5, u(18.0), u(18.0));
                if *on {
                    d.b.rounded_rect(b, u(4.0), ACCENT);
                    d.text(
                        "✓",
                        b.x + b.w * 0.5,
                        b.y + u(1.0),
                        TextStyle::new(u(15.0), BACKGROUND, Align::Center),
                    );
                } else {
                    d.b.rounded_panel(b, u(4.0), solid(), with_alpha(TEXT_DIM, 0.7), 1.0);
                }
                let name = ellipsize(d, &row.label, u(16.0), r.w - u(52.0));
                d.text(
                    &name,
                    r.x + u(40.0),
                    r.y + (r.h - u(16.0)) * 0.5 - u(1.0),
                    TextStyle::new(u(16.0), TEXT, Align::Left),
                );
            }
            Kind::Card { badge, state, tone } => {
                ui::row(d, r, sel);
                let dim = *tone == Tone::Dim && matches!(row.act, Act::None) && row.open.is_none();
                let fade = |c: Color| if dim { with_alpha(c, 0.45) } else { c };
                let color = tone_color(*tone);
                let tile = Rect::new(r.x + u(16.0), r.y + (r.h - u(44.0)) * 0.5, u(44.0), u(44.0));
                let tc = if dim { TEXT_DIM } else { ACCENT };
                d.b.rounded_panel(
                    tile,
                    u(8.0),
                    mix_opaque(solid(), tc, 0.12),
                    fade(with_alpha(tc, 0.5)),
                    1.0,
                );
                d.text(
                    badge,
                    tile.x + tile.w * 0.5,
                    tile.y + u(12.0),
                    TextStyle::new(u(17.0), fade(tc), Align::Center),
                );
                let tx = tile.right() + u(16.0);
                let cw = d.measure(state, u(14.0)) + u(22.0);
                let room = r.right() - tx - cw - u(56.0);
                let label = ellipsize(d, &row.label, u(20.0), room);
                d.text(
                    &label,
                    tx,
                    r.y + u(12.0),
                    TextStyle::new(u(20.0), fade(TEXT), Align::Left),
                );
                let desc = ellipsize(d, &row.desc, u(14.5), room);
                d.text(
                    &desc,
                    tx,
                    r.y + u(42.0),
                    TextStyle::new(u(14.5), fade(TEXT_DIM), Align::Left),
                );
                let cy = r.y + r.h * 0.5;
                chip(d, r.right() - u(44.0), cy, state, fade(color), ui);
                d.text(
                    "›",
                    r.right() - u(18.0),
                    cy - u(12.0),
                    TextStyle::new(u(22.0), fade(TEXT_DIM), Align::Right),
                );
            }
            Kind::Item => {
                ui::row(d, r, sel);
                let value = value_text(row);
                let vsize = u(17.0);
                let vcolor = if row.acts() { ACCENT } else { TEXT_DIM };
                let vw = d.measure(&value, vsize).min(r.w * 0.45);
                let right = r.right() - u(16.0);
                let lx = r.x + u(16.0);
                let lw = (right - vw - u(20.0) - lx).max(r.w * 0.3);
                let two = two_lines(row, portrait);
                let (ly, vy) = if row.desc.is_empty() {
                    let y = r.y + (r.h - vsize) * 0.5 - u(1.0);
                    (y, y)
                } else if portrait {
                    (r.y + u(8.0), r.y + u(8.0))
                } else {
                    (r.y + u(7.0), r.y + (r.h - vsize) * 0.5 - u(1.0))
                };
                let label = ellipsize(d, &row.label, u(17.0), lw);
                d.text(&label, lx, ly, TextStyle::new(u(17.0), TEXT, Align::Left));
                if !row.desc.is_empty() {
                    let ds = u(13.0);
                    if two {
                        for (k, l) in wrap(d, &row.desc, ds, r.w - u(32.0), 2).iter().enumerate() {
                            d.text(
                                l,
                                lx,
                                r.y + u(32.0) + k as f32 * u(18.0),
                                TextStyle::new(ds, TEXT_DIM, Align::Left),
                            );
                        }
                    } else {
                        let dw = if portrait { r.w - u(32.0) } else { lw };
                        let desc = ellipsize(d, &row.desc, ds, dw);
                        d.text(
                            &desc,
                            lx,
                            r.y + u(30.0),
                            TextStyle::new(ds, TEXT_DIM, Align::Left),
                        );
                    }
                }
                d.text(
                    &value,
                    right,
                    vy,
                    TextStyle::new(vsize, vcolor, Align::Right).fit(r.w * 0.45),
                );
            }
        }
    }
}
