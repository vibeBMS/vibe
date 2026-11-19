//! The flat note field: notes, long notes, mines, bar lines, the judge
//! area and BPM guide, drawn with the play skin's images.
//!
//! An LR2 `#HORIZONTAL` skin scrolls right to left: the field is laid out
//! as a vertical one in a frame turned a quarter (`Axis`), so that down to
//! the judge line is left to the judge position.

use super::scroll::Scroll;
use super::skin::PlayState;
use crate::app::Draw;
use crate::config::PlayConfig;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{Color, Rect};
use crate::skin::render::{LoadedSkin, Xf};
use crate::skin::{Frame, LaneSkin, SkinState, Src};
use vibebms_core::chart::NoteKind;
use vibebms_core::engine::Engine;
use vibebms_core::judge::Judge;

pub(super) fn draw(
    ps: &PlayState,
    d: &mut Draw,
    skin: &LoadedSkin,
    xf: &Xf,
    now: i64,
    st: &dyn SkinState,
) {
    let s = &ps.s.session;
    if s.engine().is_none() {
        return;
    }
    let p = &ps.ctx.config.play;
    // each player's hi-speed, covers and LIFT (VS: the second's own)
    let per = [ps.side_play(0), ps.side_play(1)];
    let t_vis = s.chart_us(ps.now) + p.visual_offset_ms as i64 * 1000;
    // (each player's hi-speed as eased to a change)
    let scroll =
        [0, 1].map(|side| Scroll::new(&s.chart, per[side], ps.s.hs_shown(side, ps.now), t_vis));
    let axis = Axis {
        horizontal: skin.skin.horizontal,
        edge: xf.ox + skin.skin.width * xf.sx,
    };
    let pen = Pen {
        skin,
        xf,
        now,
        st,
        axis,
    };
    // (a note's width and height %; laid out, horizontal, its height and width)
    let pulse = note_pulse(skin.skin.note_expansion, &s.chart.timing, t_vis);
    let pulse = if axis.horizontal {
        (pulse.1, pulse.0)
    } else {
        pulse
    };
    // (horizontal: the x the notes go to)
    let fall_y = skin.skin.note_fall_y.map(|y| match axis.horizontal {
        true => axis.edge - (xf.ox + y * xf.sx),
        false => xf.rect(Rect::new(0.0, y, 0.0, 0.0)).y,
    });
    // BPM GUIDE: where each player's lanes span and the lines' y (player,
    // y, guide)
    let guides = match p.bpm_guide {
        true => bpm_guides(s, t_vis),
        false => Vec::new(),
    };
    let mut guide_span: [Option<(f32, f32)>; 2] = [None, None];
    let mut guide_y: Vec<(usize, f32, usize)> = Vec::new();
    // a lane laid out and what its notes are clipped to
    let field = |lane: usize| {
        let lr = skin.skin.lanes.get(lane)?.rect?;
        let p = per[s.mode.player_of_lane(lane).min(1)];
        let f = Field::new(axis.unmap(xf.rect(lr)), p);
        // (HIDDEN+ hides notes above the judge line; wide enough for
        // pulsing notes)
        let bottom = f.judge_y + 2.0 - p.hidden * (f.judge_y - f.top);
        let grow = f.r.w * (pulse.0 - 1.0).max(0.0) * 0.5;
        let clip = Rect::new(
            f.r.x - grow,
            f.top,
            f.r.w + 2.0 * grow,
            (bottom - f.top).max(0.0),
        );
        Some((f, clip))
    };
    for (lane, ls) in skin.skin.lanes.iter().enumerate() {
        let Some((f, own)) = field(lane) else {
            continue;
        };
        // the lane's judge (a side's in battle)
        let Some(engine) = s.engine_for(lane) else {
            continue;
        };
        let side = s.mode.player_of_lane(lane).min(1);
        let (p, sc) = (per[side], &scroll[side]);
        let r = f.r;
        d.b.set_clip(Some(axis.map(own)));
        let at = |t: i64| f.y(sc.ahead_at(t));
        if p.judge_area {
            let scratch = s.mode.is_scratch(lane);
            judge_area(d, axis, &f, engine, scratch, |t| at(sc.t_judge + t));
        }
        if !guides.is_empty() {
            let span = guide_span[side].get_or_insert((r.x, r.right()));
            span.0 = span.0.min(r.x);
            span.1 = span.1.max(r.right());
            if guide_y.iter().all(|g| g.0 != side) {
                for (k, g) in guides.iter().enumerate() {
                    let y = at(g.0);
                    if y >= f.top && y <= f.judge_y {
                        guide_y.push((side, y, k));
                    }
                }
            }
        }
        let Some(l) = s.lanes.get(lane) else {
            continue;
        };
        let notes = engine.notes();
        let note_h = |src: &Src| axis.length(ls.note_h, src, xf, 4.0);
        // PMS (#DST_NOTE2): missed notes, unjudged or POOR, fall
        let mut falling: Vec<usize> = Vec::new();
        if fall_y.is_some() {
            falling.extend(
                l.notes[..l.cursor.min(l.notes.len())]
                    .iter()
                    .rev()
                    .copied()
                    .take_while(|&ni| notes[ni].time > t_vis - FALL_US)
                    .filter(|&ni| {
                        let n = &notes[ni];
                        !n.is_long() && !n.is_mine() && n.state.is_none_or(|j| j >= Judge::Poor)
                    }),
            );
        }
        for (ni, ahead) in sc.notes(l, notes, p.hold_at_line) {
            let n = &notes[ni];
            let cn = &s.chart.notes[n.chart_index];
            let y = f.y(ahead);
            match &cn.kind {
                NoteKind::Long {
                    end_pos, end_time, ..
                } => {
                    if !n.is_done() {
                        let ye = f.y(sc.ahead(*end_pos, *end_time));
                        long_note(d, &pen, ls, engine, lane, ni, &f, (y, ye));
                    }
                }
                NoteKind::Mine { .. } => {
                    if let Some(m) = ls.mine.as_ref() {
                        let h = note_h(m);
                        pen.blit(d, m, Rect::new(r.x, y - h, r.w, h), &IDENT);
                    }
                }
                _ => {
                    if fall_y.is_some()
                        && n.time < sc.t_judge
                        && n.state.is_none_or(|j| j >= Judge::Poor)
                    {
                        falling.push(ni);
                        continue;
                    }
                    let mark = n.is_done() && p.mark_notes && n.time > sc.t_judge;
                    if n.is_done() && !mark {
                        continue;
                    }
                    if let Some(src) = ls.note.as_ref() {
                        let h = note_h(src);
                        let fr = if mark { &MARKED } else { &IDENT };
                        let (w2, h2) = (r.w * pulse.0, h * pulse.1);
                        let nr = Rect::new(r.x - (w2 - r.w) * 0.5, y - h - (h2 - h) * 0.5, w2, h2);
                        pen.blit(d, src, nr, fr);
                    }
                }
            }
        }
        d.b.set_clip(None);
        // they wait at the line through the late BAD window, then fall
        // at the scroll speed until they pass the skin's vanishing point
        if let (Some(vanish), Some(src)) = (fall_y, ls.note.as_ref()) {
            let bad = engine
                .windows()
                .note
                .get(Judge::Bad as usize)
                .map(|w| -w.0)
                .unwrap_or(0)
                .max(0);
            let h = note_h(src);
            for ni in falling {
                let y = at(notes[ni].time + bad).max(f.judge_y);
                if y <= vanish {
                    pen.blit(d, src, Rect::new(r.x, y - h, r.w, h), &IDENT);
                }
            }
        }
    }
    for &(pl, y, k) in &guide_y {
        if let Some((x0, x1)) = guide_span[pl] {
            let (_, label, c) = &guides[k];
            let line = axis.map(Rect::new(x0, y - 1.0, x1 - x0, 2.0));
            d.b.rect(line, *c);
            let size = (14.0 * xf.sy).max(10.0);
            // (horizontal: above the lanes)
            let (tx, ty) = match axis.horizontal {
                true => (line.x + line.w * 0.5, line.y - size - 4.0),
                false => ((x0 + x1) * 0.5, y - size - 4.0),
            };
            d.text(label, tx, ty, TextStyle::new(size, *c, Align::Center));
        }
    }
    bar_lines(d, &pen, &per, &scroll);
}

/// A frame that draws an image as it is.
const IDENT: Frame = Frame {
    rect: Rect::new(0.0, 0.0, 0.0, 0.0),
    color: [1.0; 4],
    angle: 0.0,
    blend: crate::gfx::Blend::Alpha,
    center: (0.5, 0.5),
};

/// MARK NOTE: notes hit early fall on, greyed.
const MARKED: Frame = Frame {
    color: [0.45, 0.45, 0.5, 0.7],
    ..IDENT
};

/// What the field's images are drawn with.
struct Pen<'a> {
    skin: &'a LoadedSkin,
    xf: &'a Xf,
    now: i64,
    st: &'a dyn SkinState,
    axis: Axis,
}

impl Pen<'_> {
    /// `src` over rect `r` of the layout.
    fn blit(&self, d: &mut Draw, src: &Src, r: Rect, fr: &Frame) {
        let r = self.axis.map(r);
        self.skin.blit(d, src, self.now, self.st, r, fr);
    }
}

/// A lane (or a lane group's bar lines) laid out (`Axis`): the judge
/// line's y, the top of what shows (below SUDDEN+) and the length between
/// them, which [`Scroll::ahead`] is a share of.
struct Field {
    r: Rect,
    judge_y: f32,
    top: f32,
    visible: f32,
}

impl Field {
    fn new(r: Rect, p: &PlayConfig) -> Field {
        let judge_y = r.bottom() - p.lift * r.h;
        let top = r.y + p.sudden * r.h;
        Field {
            r,
            judge_y,
            top,
            visible: (judge_y - top).max(1.0),
        }
    }

    /// The y of what is `ahead` up the lane.
    fn y(&self, ahead: f64) -> f32 {
        self.judge_y - (ahead * self.visible as f64) as f32
    }
}

/// BPM GUIDE: each BPM change and stop in the next 10 s (time, label,
/// colour).
fn bpm_guides(s: &super::session::Session, t_vis: i64) -> Vec<(i64, String, Color)> {
    let ahead = t_vis..t_vis + 10_000_000;
    let bpms = s.bpm_log.iter().skip(1).filter(|c| ahead.contains(&c.0));
    let stops = s.chart.timing.stops.iter();
    let stops = stops.filter(|st| ahead.contains(&st.time));
    bpms.map(|&(time, bpm)| {
        let label = format!("BPM {}", bpm.round() as i64);
        (time, label, [0.25, 0.85, 0.3, 1.0])
    })
    .chain(stops.map(|st| {
        let label = format!("STOP {}ms", st.duration / 1000);
        (st.time, label, [0.85, 0.85, 0.2, 1.0])
    }))
    .collect()
}

/// JUDGE AREA: each judgement's early window as a band above the line
/// (beatoraja's colours: PG blue .. POOR red); `y_in(t)` is the y of
/// what is `t` µs ahead of the judge's clock.
fn judge_area(
    d: &mut Draw,
    axis: Axis,
    f: &Field,
    engine: &Engine,
    scratch: bool,
    y_in: impl Fn(i64) -> f32,
) {
    const COLORS: [Color; 5] = [
        [0.0, 0.0, 1.0, 0.125],
        [0.0, 1.0, 0.0, 0.125],
        [1.0, 1.0, 0.0, 0.125],
        [1.0, 0.5, 0.0, 0.125],
        [1.0, 0.0, 0.0, 0.125],
    ];
    let w = engine.windows();
    let win = if scratch { &w.scratch } else { &w.note };
    let mut lo = f.judge_y;
    for (c, (_, early)) in COLORS.iter().zip(win.iter()) {
        let hi = y_in(*early).max(f.top);
        if hi < lo {
            d.b.rect(axis.map(Rect::new(f.r.x, hi, f.r.w, lo - hi)), *c);
            lo = hi;
        }
    }
}

/// A long note (`ni` of the lane's judge) whose head is at `y` and end
/// at `ye`.
#[allow(clippy::too_many_arguments)]
fn long_note(
    d: &mut Draw,
    pen: &Pen,
    ls: &LaneSkin,
    engine: &Engine,
    lane: usize,
    ni: usize,
    f: &Field,
    (y, ye): (f32, f32),
) {
    let r = f.r;
    let note_h = |src: &Src| pen.axis.length(ls.note_h, src, pen.xf, 4.0);
    let holding = engine.holding(lane) == Some(ni);
    // hell charge notes: their own images (else the LN ones); passing the
    // line, held or not (damage)
    let hcn = engine.notes()[ni].is_hcn();
    let passing = engine
        .hcn_passing(lane)
        .filter(|(p, _)| hcn && *p == ni)
        .map(|(_, held)| held);
    let pick = |h, l| if hcn && Option::is_some(h) { h } else { l };
    let yh = if holding || passing.is_some() {
        f.judge_y
    } else {
        y
    };
    let inactive = pick(&ls.hcn_body, &ls.ln_body).as_ref();
    let active = pick(&ls.hcn_body_active, &ls.ln_body_active).as_ref();
    let body = match passing {
        Some(false) => ls.hcn_damage.as_ref().or(inactive),
        Some(true) if engine.hcn_damaged(ni) => ls.hcn_reactive.as_ref().or(active).or(inactive),
        _ if holding || passing.is_some() => active.or(inactive),
        _ => inactive,
    };
    let ln_end = pick(&ls.hcn_end, &ls.ln_end).as_ref();
    let ln_start = pick(&ls.hcn_start, &ls.ln_start).as_ref();
    if let Some(b) = body {
        pen.blit(d, b, Rect::new(r.x, ye, r.w, (yh - ye).max(0.0)), &IDENT);
    }
    if let Some(e) = ln_end.or(ls.note.as_ref()) {
        let h = note_h(e);
        pen.blit(d, e, Rect::new(r.x, ye - h, r.w, h), &IDENT);
    }
    if let Some(h0) = ln_start.or(ls.note.as_ref()) {
        let h = note_h(h0);
        pen.blit(d, h0, Rect::new(r.x, yh - h, r.w, h), &IDENT);
    }
}

/// Bar lines span each lane group, placed as its notes are.
fn bar_lines(d: &mut Draw, pen: &Pen, per: &[&PlayConfig; 2], scroll: &[Scroll; 2]) {
    let skin = &pen.skin.skin;
    for (i, (src, dst)) in skin.bar_lines.iter().enumerate() {
        let Some(fr) = dst.eval(pen.now, pen.st) else {
            continue;
        };
        // (horizontal: the line at the judge position, the field on to the
        // right edge of the canvas)
        let mut r = fr.rect;
        if pen.axis.horizontal {
            r.w = (skin.width - r.x).max(1.0);
        }
        let f = Field::new(pen.axis.unmap(pen.xf.rect(r)), per[i.min(1)]);
        let h = pen.axis.length(None, src, pen.xf, 1.0).max(1.0);
        for ahead in scroll[i.min(1)].bar_lines() {
            let y = f.y(ahead);
            pen.blit(d, src, Rect::new(f.r.x, y - h, f.r.w, h), &fr);
        }
    }
}

/// The direction of the note field. Vertical, rects are drawn as they
/// are; horizontal, the field is laid out as a vertical one whose y is the
/// distance left from the right `edge` of the canvas and whose x is the
/// screen's y.
#[derive(Clone, Copy)]
struct Axis {
    horizontal: bool,
    edge: f32,
}

impl Axis {
    /// A rect of the layout on screen.
    fn map(&self, r: Rect) -> Rect {
        match self.horizontal {
            true => Rect::new(self.edge - r.bottom(), r.x, r.h, r.w),
            false => r,
        }
    }

    /// A screen rect in the layout.
    fn unmap(&self, r: Rect) -> Rect {
        match self.horizontal {
            true => Rect::new(r.y, self.edge - r.right(), r.h, r.w),
            false => r,
        }
    }

    /// How long a note (or line) image is along the scroll on screen: the
    /// skin's size, else its source frame's (height, horizontal: width).
    fn length(&self, skin_len: Option<f32>, src: &crate::skin::Src, xf: &Xf, or: f32) -> f32 {
        let (k, scale) = match self.horizontal {
            true => (2, xf.sx),
            false => (3, xf.sy),
        };
        match skin_len {
            Some(l) => l * scale,
            None => src.frames.first().map(|f| f[k] * scale).unwrap_or(or),
        }
    }
}

/// How long missed notes are followed as they fall (µs).
const FALL_US: i64 = 5_000_000;

/// beatoraja's PMS note pulse: the size scale of the notes `t` µs into the
/// chart, growing to the skin's rate over 9 ms from each beat and back over
/// 150 ms.
fn note_pulse(
    rate: Option<(f32, f32)>,
    tm: &vibebms_core::timing::TimingMap,
    t: i64,
) -> (f32, f32) {
    const GROW_MS: f64 = 9.0;
    const SHRINK_MS: f64 = 150.0;
    let Some((w, h)) = rate else {
        return (1.0, 1.0);
    };
    let t = t as f64;
    let since = (t - tm.time_at_beat(tm.beat_at_time(t).floor()) as f64) / 1000.0;
    let k = if since < GROW_MS {
        since / GROW_MS
    } else if since <= GROW_MS + SHRINK_MS {
        (SHRINK_MS - (since - GROW_MS)) / SHRINK_MS
    } else {
        0.0
    }
    .max(0.0) as f32;
    (1.0 + (w / 100.0 - 1.0) * k, 1.0 + (h / 100.0 - 1.0) * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Horizontal: the layout's lane runs down to the judge line as the
    /// screen's runs left to its judge position; a note at the line is
    /// drawn to the right of it, the far end is the canvas's right edge.
    #[test]
    fn a_horizontal_field_is_a_vertical_one_turned() {
        let axis = Axis {
            horizontal: true,
            edge: 1280.0,
        };
        let lane = Rect::new(100.0, 200.0, 1180.0, 30.0);
        let v = axis.unmap(lane);
        assert_eq!(v, Rect::new(200.0, 0.0, 30.0, 1180.0));
        assert_eq!(axis.map(v), lane);
        // a 12 px note whose bottom is at the judge line
        let note = Rect::new(v.x, v.bottom() - 12.0, v.w, 12.0);
        assert_eq!(axis.map(note), Rect::new(100.0, 200.0, 12.0, 30.0));
        // further up the layout is further right on screen
        let later = Rect::new(v.x, v.bottom() - 512.0, v.w, 12.0);
        assert_eq!(axis.map(later).x, 600.0);
        let vertical = Axis {
            horizontal: false,
            edge: 1280.0,
        };
        assert_eq!(vertical.map(lane), lane);
        assert_eq!(vertical.unmap(lane), lane);
    }
}
