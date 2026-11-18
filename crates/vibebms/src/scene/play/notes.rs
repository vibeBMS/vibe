//! The flat note field: notes, long notes, mines, bar lines, the judge
//! area and BPM guide, drawn with the play skin's images.

use super::session::held_at_line;
use super::skin::PlayState;
use crate::app::Draw;
use crate::config::HiSpeedMode;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::Rect;
use crate::skin::render::{LoadedSkin, Xf};
use crate::skin::SkinState;
use vibebms_core::chart::NoteKind;
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
    let cur_pos = s.chart.timing.pos_at_time(t_vis as f64);
    // `#SPEED` now: the whole lane faster or slower (not under CONSTANT)
    let speed = s.chart.timing.speed_at_time(t_vis as f64);
    let pulse = note_pulse(skin.skin.note_expansion, &s.chart.timing, t_vis);
    let fall_y = skin
        .skin
        .note_fall_y
        .map(|y| xf.rect(Rect::new(0.0, y, 0.0, 0.0)).y);
    let main_bpm = if s.chart.meta.main_bpm > 0.0 {
        s.chart.meta.main_bpm
    } else {
        130.0
    };
    // (each player's hi-speed as eased to a change)
    let dur_of =
        |side: usize| per[side].duration_with(ps.s.hs_shown(side, ps.now), main_bpm, true) * 1000.0;
    let ident = crate::skin::Frame {
        rect: Rect::default(),
        color: [1.0; 4],
        angle: 0.0,
        blend: crate::gfx::Blend::Alpha,
        center: (0.5, 0.5),
    };
    // MARK NOTE: notes hit early fall on, greyed
    let marked = crate::skin::Frame {
        color: [0.45, 0.45, 0.5, 0.7],
        ..ident
    };
    // BPM GUIDE: where each player's lanes span and the line's y, per
    // BPM change / stop in the next 10 s (time, label, colour)
    let mut guide_span: [Option<(f32, f32)>; 2] = [None, None];
    let mut guide_y: Vec<(usize, f32, String, [f32; 4])> = Vec::new();
    let ahead = t_vis..t_vis + 10_000_000;
    let guides: Vec<(i64, String, [f32; 4])> = if p.bpm_guide {
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
    } else {
        Vec::new()
    };
    for (lane, ls) in skin.skin.lanes.iter().enumerate() {
        let Some(lr) = ls.rect else { continue };
        // the lane's judge (a side's in battle)
        let Some(engine) = s.engine_for(lane) else {
            continue;
        };
        let notes = engine.notes();
        let side = s.mode.player_of_lane(lane).min(1);
        let p = per[side];
        let dur_us = dur_of(side);
        let r = xf.rect(lr);
        let judge_y = r.bottom() - p.lift * r.h;
        let top = r.y
            + if p.lane_cover || p.sudden > 0.0 {
                p.sudden * r.h
            } else {
                0.0
            };
        let visible = (judge_y - top).max(1.0);
        let ppb = visible as f64 / (dur_us / 1_000_000.0 * main_bpm / 60.0);
        let y_of = |pos: f64, time: i64| -> f32 {
            match p.hispeed_mode {
                HiSpeedMode::Floating => judge_y - ((pos - cur_pos) * ppb * speed) as f32,
                HiSpeedMode::Constant => {
                    judge_y - ((time - t_vis) as f64 / dur_us * visible as f64) as f32
                }
            }
        };
        let note_h = |src: &crate::skin::Src| match ls.note_h {
            Some(h) => h * xf.sy,
            None => src.frames.first().map(|f| f[3] * xf.sy).unwrap_or(4.0),
        };
        // HIDDEN+ hides notes above the judge line
        let bottom = judge_y + 2.0 - p.hidden * (judge_y - top);
        // (wide enough for pulsing notes)
        let grow = r.w * (pulse.0 - 1.0).max(0.0) * 0.5;
        d.b.set_clip(Some(Rect::new(
            r.x - grow,
            top,
            r.w + 2.0 * grow,
            (bottom - top).max(0.0),
        )));
        let at = |t: i64| y_of(s.chart.timing.pos_at_time(t as f64), t);
        // JUDGE AREA: each judgement's early window as a band above the
        // line (beatoraja's colours: PG blue .. POOR red)
        if p.judge_area {
            const COLORS: [[f32; 4]; 5] = [
                [0.0, 0.0, 1.0, 0.125],
                [0.0, 1.0, 0.0, 0.125],
                [1.0, 1.0, 0.0, 0.125],
                [1.0, 0.5, 0.0, 0.125],
                [1.0, 0.0, 0.0, 0.125],
            ];
            let w = engine.windows();
            let win = if s.mode.is_scratch(lane) {
                &w.scratch
            } else {
                &w.note
            };
            let mut lo = judge_y;
            for (k, (_, early)) in win.iter().enumerate().take(COLORS.len()) {
                let hi = at(t_vis + early).max(top);
                if hi < lo {
                    d.b.rect(Rect::new(r.x, hi, r.w, lo - hi), COLORS[k]);
                    lo = hi;
                }
            }
        }
        if !guides.is_empty() {
            let pl = s.mode.player_of_lane(lane).min(1);
            let span = guide_span[pl].get_or_insert((r.x, r.right()));
            span.0 = span.0.min(r.x);
            span.1 = span.1.max(r.right());
            if guide_y.iter().all(|g| g.0 != pl) {
                for (t, label, c) in &guides {
                    let y = at(*t);
                    if y >= top && y <= judge_y {
                        guide_y.push((pl, y, label.clone(), *c));
                    }
                }
            }
        }
        let Some(l) = s.lanes.get(lane) else {
            continue;
        };
        let (list, cursor) = (&l.notes, l.cursor.min(l.notes.len()));
        // PMS (#DST_NOTE2): missed notes, unjudged or POOR, fall
        let mut falling: Vec<usize> = Vec::new();
        if fall_y.is_some() {
            falling.extend(
                list[..cursor]
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
        for &ni in &list[cursor..] {
            let n = &notes[ni];
            let cn = &s.chart.notes[n.chart_index];
            let y = y_of(cn.pos, n.time);
            // LR2: an unjudged note waits at the line
            let y = match p.hold_at_line && held_at_line(n, t_vis) {
                true => y.min(judge_y),
                false => y,
            };
            if n.time > t_vis + 2_000_000 && y < r.y - r.h {
                break;
            }
            match &cn.kind {
                NoteKind::Long {
                    end_pos, end_time, ..
                } => {
                    if n.is_done() {
                        continue;
                    }
                    let holding = engine.holding(lane) == Some(ni);
                    // hell charge notes: their own images (else the LN
                    // ones); passing the line, held or not (damage)
                    let hcn = n.is_hcn();
                    let passing = engine
                        .hcn_passing(lane)
                        .filter(|(p, _)| hcn && *p == ni)
                        .map(|(_, held)| held);
                    let pick = |h, l| if hcn && Option::is_some(h) { h } else { l };
                    let ye = y_of(*end_pos, *end_time);
                    let yh = if holding || passing.is_some() {
                        judge_y
                    } else {
                        y
                    };
                    let inactive = pick(&ls.hcn_body, &ls.ln_body).as_ref();
                    let active = pick(&ls.hcn_body_active, &ls.ln_body_active).as_ref();
                    let body = match passing {
                        Some(false) => ls.hcn_damage.as_ref().or(inactive),
                        Some(true) if engine.hcn_damaged(ni) => {
                            ls.hcn_reactive.as_ref().or(active).or(inactive)
                        }
                        _ if holding || passing.is_some() => active.or(inactive),
                        _ => inactive,
                    };
                    let ln_end = pick(&ls.hcn_end, &ls.ln_end).as_ref();
                    let ln_start = pick(&ls.hcn_start, &ls.ln_start).as_ref();
                    if let Some(b) = body {
                        skin.blit(
                            d,
                            b,
                            now,
                            st,
                            Rect::new(r.x, ye, r.w, (yh - ye).max(0.0)),
                            &ident,
                        );
                    }
                    if let Some(e) = ln_end.or(ls.note.as_ref()) {
                        let h = note_h(e);
                        skin.blit(d, e, now, st, Rect::new(r.x, ye - h, r.w, h), &ident);
                    }
                    if let Some(h0) = ln_start.or(ls.note.as_ref()) {
                        let h = note_h(h0);
                        skin.blit(d, h0, now, st, Rect::new(r.x, yh - h, r.w, h), &ident);
                    }
                }
                NoteKind::Mine { .. } => {
                    if let Some(m) = ls.mine.as_ref() {
                        let h = note_h(m);
                        skin.blit(d, m, now, st, Rect::new(r.x, y - h, r.w, h), &ident);
                    }
                }
                _ => {
                    if fall_y.is_some()
                        && n.time < t_vis
                        && n.state.is_none_or(|j| j >= Judge::Poor)
                    {
                        falling.push(ni);
                        continue;
                    }
                    let mark = n.is_done() && p.mark_notes && n.time > t_vis;
                    if n.is_done() && !mark {
                        continue;
                    }
                    if let Some(src) = ls.note.as_ref() {
                        let h = note_h(src);
                        let fr = if mark { &marked } else { &ident };
                        let (w2, h2) = (r.w * pulse.0, h * pulse.1);
                        let nr = Rect::new(r.x - (w2 - r.w) * 0.5, y - h - (h2 - h) * 0.5, w2, h2);
                        skin.blit(d, src, now, st, nr, fr);
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
                let y = at(notes[ni].time + bad).max(judge_y);
                if y <= vanish {
                    skin.blit(d, src, now, st, Rect::new(r.x, y - h, r.w, h), &ident);
                }
            }
        }
    }
    for (pl, y, label, c) in &guide_y {
        if let Some((x0, x1)) = guide_span[*pl] {
            d.b.rect(Rect::new(x0, *y - 1.0, x1 - x0, 2.0), *c);
            let size = (14.0 * xf.sy).max(10.0);
            d.text(
                label,
                (x0 + x1) * 0.5,
                *y - size - 4.0,
                TextStyle::new(size, *c, Align::Center),
            );
        }
    }
    // bar lines span each lane group
    for (i, (src, dst)) in skin.skin.bar_lines.iter().enumerate() {
        let p = per[i.min(1)];
        let dur_us = dur_of(i.min(1));
        if let Some(fr) = dst.eval(now, st) {
            let field = xf.rect(fr.rect);
            let judge_y = field.bottom() - p.lift * field.h;
            let top = field.y + p.sudden * field.h;
            let ppb = (judge_y - top).max(1.0) as f64 / (dur_us / 1_000_000.0 * main_bpm / 60.0);
            // (y falls as the position grows: from the first at or above
            // the line until one is past the top)
            let y_of = |pos: f64| judge_y - ((pos - cur_pos) * ppb) as f32;
            let lines = &s.chart.bar_lines;
            let first = lines.partition_point(|bl| y_of(bl.pos) > judge_y);
            for bl in &lines[first..] {
                let y = y_of(bl.pos);
                if y < top {
                    break;
                }
                let h = src
                    .frames
                    .first()
                    .map(|f| f[3] * xf.sy)
                    .unwrap_or(1.0)
                    .max(1.0);
                skin.blit(d, src, now, st, Rect::new(field.x, y - h, field.w, h), &fr);
            }
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
