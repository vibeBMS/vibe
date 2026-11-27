//! The touch perspective skin's note field: each player's lanes as a fan
//! (`crate::perspective`), with notes, long notes, bar lines, the judge
//! line, key beams, bombs and the lane covers drawn through its mapping.
//! Notes move in lane space as on a flat lane (the green number holds);
//! only where a depth is on the screen differs.

use super::scroll::Scroll;
use super::skin::PlayState;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{rgba, with_alpha, Blend, Color, Draw};
use crate::perspective::{Depths, Fan, FanLane};
use crate::platform::Instant;
use crate::touch::Shape;
use vibebms_core::chart::NoteKind;
use vibebms_core::PlayMode;

/// Note thickness at the base, as a share of the fan's base width.
const NOTE_H: f32 = 0.032;
/// How long a bomb and a released key's beam last (ms).
const BOMB_MS: f32 = 260.0;
const BEAM_FADE_MS: f32 = 120.0;

const EDGE: Color = rgba(0x58e0d0ff);
const LINE: Color = rgba(0xf4fbffff);
const COVER: Color = rgba(0x080a12f8);

/// pop'n music style button colours (9 buttons).
const POPN: [u32; 5] = [0xeef0f4ff, 0xffd34aff, 0x52dc86ff, 0x4f94ffff, 0xff5a68ff];

/// A lane's note colour.
fn note_color(mode: PlayMode, l: &FanLane) -> Color {
    if l.scratch {
        return rgba(0xff5c78ff);
    }
    let k = l.lane % mode.lanes_per_player();
    if mode == PlayMode::Popn9K {
        return rgba(POPN[k.min(8 - k)]);
    }
    if k % 2 == 1 {
        rgba(0x4aa6ffff)
    } else {
        rgba(0xeef3ffff)
    }
}

/// A lane's floor colour (near the line; it fades with distance).
fn floor_color(mode: PlayMode, l: &FanLane) -> Color {
    if l.scratch {
        return rgba(0x2a1220f0);
    }
    if mode != PlayMode::Popn9K && l.lane % mode.lanes_per_player() % 2 == 1 {
        rgba(0x111a2cf0)
    } else {
        rgba(0x151a26f0)
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// A piece of a fan between depths `d0` (near) and `d1` (far).
fn band(d: &mut Draw, f: &Fan, u: (f32, f32), d0: f32, d1: f32, near: Color, far: Color) {
    if d1 > d0 {
        d.b.quad4(f.quad(u.0, u.1, d0, d1), [far, far, near, near]);
    }
}

/// The depth a screen distance `px` takes at depth `at`.
fn depth_of_px(f: &Fan, px: f32, at: f32) -> f32 {
    let g = 1.0 + f.k * at;
    px * g * g / (f.len * (1.0 + f.k)).max(1e-3)
}

fn ms_since(now: Instant, t: Option<Instant>) -> Option<f32> {
    Some(now.checked_duration_since(t?)?.as_secs_f32() * 1000.0)
}

/// Draw the note field (the skin's `notes` object).
pub(super) fn draw(st: &PlayState, d: &mut Draw) {
    let scene = st.s;
    let s = &scene.session;
    let ctx = st.ctx;
    if s.engine().is_none() {
        return;
    }
    let Some(sk) = scene.skin.as_ref() else {
        return;
    };
    let fans = &scene.touch.fans;
    let p = &ctx.config.play;
    let dep = Depths::of(p);
    let mode = s.mode;
    let now = st.now;
    let t_vis = s.chart_us(now) + p.visual_offset_ms as i64 * 1000;
    let sc = Scroll::new(&s.chart, p, scene.hs_shown(0, now), t_vis);
    let depth = |ahead: f64| dep.at(ahead, 1.0);
    let bpms = scene.bpms_at(now);
    // notes are hidden under HIDDEN+ and once past the line
    let floor = if p.hidden > 0.0 {
        dep.hidden
    } else {
        dep.judge
    };
    for f in fans {
        let d_bot = f.d_bottom();
        let note_dd = depth_of_px(f, f.width * NOTE_H, dep.judge);
        let px = |n: f32, at: f32| depth_of_px(f, n, at);
        // the floor: each lane, fading into the distance
        for l in &f.lanes {
            let c = floor_color(mode, l);
            band(d, f, (l.u0, l.u1), d_bot, 1.0, c, with_alpha(c, 0.25));
        }
        // lane edges (thinner far away) and the fan's outline
        for w in f.lanes.windows(2) {
            let u = (w[0].u1 + w[1].u0) * 0.5;
            let half = 1.0 / f.width;
            let c = rgba(0x3a4462c0);
            band(
                d,
                f,
                (u - half, u + half),
                d_bot,
                1.0,
                c,
                with_alpha(c, 0.2),
            );
        }
        for u in [0.0, 1.0] {
            let half = 2.0 / f.width;
            band(
                d,
                f,
                (u - half, u + half),
                d_bot,
                1.0,
                EDGE,
                with_alpha(EDGE, 0.1),
            );
        }
        // the touch area (what a touch hits) and the scratch's swipe marks
        let opacity =
            crate::perspective::touch_area_opacity(&sk.skin.options_on, ctx.config.touch.opacity);
        if ctx.touch_enabled() && opacity > 0.0 {
            for z in scene
                .touch
                .zones
                .iter()
                .filter(|z| z.lane < crate::input::SYSTEM)
            {
                let (Shape::Strip(rows), Some(l)) =
                    (&z.shape, f.lanes.iter().find(|l| l.lane == z.lane as usize))
                else {
                    continue;
                };
                let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
                    continue;
                };
                let (y0, y1) = (first[0], last[0]);
                let pressed = scene.touch.input.is_pressed(z.lane);
                let c = note_color(mode, l);
                let a = opacity * if pressed { 0.22 } else { 0.07 };
                // fainter toward the top
                let shade = |y: f32| with_alpha(c, a * (0.3 + 0.7 * (y - y0) / (y1 - y0).max(1.0)));
                crate::touch::draw_strip(d.b, rows, shade);
                // its edges: where a key's zone meets the next one's
                crate::touch::strip_outline(d.b, rows, 1.5, with_alpha(c, opacity * 0.3));
                let top = f.depth(y0).unwrap_or(0.0);
                if l.scratch {
                    // chevrons pointing both ways
                    let mid = (d_bot + top) * 0.5;
                    let cu = (l.u0 + l.u1) * 0.5;
                    let du = (l.u1 - l.u0) * 0.18;
                    let dd = px(10.0, mid);
                    let col = with_alpha(c, opacity * 0.55);
                    for side in [-1.0f32, 1.0] {
                        let tip = f.point(cu + side * du * 1.6, mid);
                        let a0 = f.point(cu + side * du * 0.4, mid + dd);
                        let a1 = f.point(cu + side * du * 0.4, mid - dd);
                        d.b.quad4([a0, tip, tip, a1], [col; 4]);
                    }
                }
            }
        }
        // key beams: lit while pressed, fading once let go
        for l in &f.lanes {
            let lane = s.lanes.get(l.lane);
            let k = match (lane.and_then(|l| l.pressed), lane.and_then(|l| l.released)) {
                (Some(a), Some(b)) if b >= a => {
                    1.0 - ms_since(now, Some(b)).unwrap_or(0.0) / BEAM_FADE_MS
                }
                (Some(_), _) => 1.0,
                _ => 0.0,
            };
            if k > 0.0 {
                let c = note_color(mode, l);
                d.b.set_blend(Blend::Add);
                band(
                    d,
                    f,
                    (l.u0, l.u1),
                    dep.judge,
                    dep.judge + 0.22,
                    with_alpha(c, 0.25 * k),
                    with_alpha(c, 0.0),
                );
                d.b.set_blend(Blend::Alpha);
            }
        }
        // bar lines
        for bd in sc.bar_lines().map(depth).filter(|&bd| bd >= floor) {
            let c = rgba(0x8a96b8a0);
            band(d, f, (0.0, 1.0), bd, bd + px(1.5, bd), c, c);
        }
        // notes
        for l in &f.lanes {
            let lane = l.lane;
            let (Some(ln), Some(engine)) = (s.lanes.get(lane), s.engine_for(lane)) else {
                continue;
            };
            let notes = engine.notes();
            let c = note_color(mode, l);
            let m = (l.u1 - l.u0) * 0.04;
            let u = (l.u0 + m, l.u1 - m);
            // a note's head (or tail) piece from depth `at` up
            let head = |d: &mut Draw, at: f32, color: Color| {
                let (d0, d1) = (at.max(floor), (at + note_dd).min(dep.cover));
                if d1 > d0 {
                    band(d, f, u, d0, d1, color, mix(color, rgba(0x6070a0ff), 0.35));
                }
            };
            for (ni, ahead) in sc.notes(ln, notes, p.hold_at_line) {
                let n = &notes[ni];
                let nd = depth(ahead);
                match &s.chart.notes[n.chart_index].kind {
                    NoteKind::Long {
                        end_pos, end_time, ..
                    } => {
                        if n.is_done() {
                            continue;
                        }
                        let holding = engine.holding(lane) == Some(ni)
                            || engine.hcn_passing(lane).is_some_and(|(p, _)| p == ni);
                        let start = if holding { dep.judge } else { nd };
                        let end = depth(sc.ahead(*end_pos, *end_time));
                        let (b0, b1) = (start.max(floor), end.min(dep.cover));
                        if b1 > b0 {
                            let a = if holding { 0.8 } else { 0.5 };
                            let near = with_alpha(c, a);
                            let far = with_alpha(mix(c, rgba(0x404a70ff), 0.4), a);
                            band(d, f, (u.0 + m, u.1 - m), b0, b1, near, far);
                        }
                        head(d, end, c);
                        head(d, start, if holding { LINE } else { c });
                    }
                    NoteKind::Mine { .. } => {
                        let mu = (u.0 + (u.1 - u.0) * 0.2, u.1 - (u.1 - u.0) * 0.2);
                        let (d0, d1) = (nd.max(floor), (nd + note_dd * 0.7).min(dep.cover));
                        if d1 > d0 {
                            band(d, f, mu, d0, d1, rgba(0xe0304cff), rgba(0x801830ff));
                        }
                    }
                    _ => {
                        let mark = n.is_done() && p.mark_notes && n.time > sc.t_judge;
                        if n.is_done() && !mark {
                            continue;
                        }
                        head(d, nd, if mark { rgba(0x737380b0) } else { c });
                    }
                }
            }
        }
        // the covers: LIFT under the line, HIDDEN+ above it, SUDDEN+ at
        // the far end (with its edge lit)
        if dep.judge > 0.0 {
            band(d, f, (0.0, 1.0), 0.0, dep.judge, COVER, COVER);
        }
        if p.hidden > 0.0 {
            band(d, f, (0.0, 1.0), dep.judge, dep.hidden, COVER, COVER);
            band(
                d,
                f,
                (0.0, 1.0),
                dep.hidden - px(2.0, dep.hidden),
                dep.hidden,
                EDGE,
                EDGE,
            );
        }
        if dep.cover < 1.0 {
            band(d, f, (0.0, 1.0), dep.cover, 1.0, COVER, COVER);
            band(
                d,
                f,
                (0.0, 1.0),
                dep.cover,
                dep.cover + px(2.0, dep.cover),
                EDGE,
                EDGE,
            );
            if scene.controls.panel[0].open() {
                let [x, y] = f.point(0.5, dep.cover);
                let size = (f.width * 0.035).max(10.0);
                let text = format!(
                    "{}   GN {}",
                    (p.sudden * 1000.0).round() as i64,
                    p.green(&bpms)
                );
                d.text(
                    &text,
                    x,
                    y - size * 1.4,
                    TextStyle::new(size, crate::theme::TEXT, Align::Center),
                );
            }
        }
        // the judge line, with a glow
        let lh = px(4.0, dep.judge);
        d.b.set_blend(Blend::Add);
        band(
            d,
            f,
            (0.0, 1.0),
            dep.judge,
            dep.judge + lh * 4.0,
            with_alpha(EDGE, 0.35),
            with_alpha(EDGE, 0.0),
        );
        d.b.set_blend(Blend::Alpha);
        band(d, f, (0.0, 1.0), dep.judge, dep.judge + lh, LINE, LINE);
        // bombs: a ring spreading over the floor; a glow while holding
        d.b.set_blend(Blend::Add);
        for l in &f.lanes {
            let c = note_color(mode, l);
            let cu = (l.u0 + l.u1) * 0.5;
            let lw = l.u1 - l.u0;
            if let Some(age) = ms_since(now, s.lanes.get(l.lane).and_then(|l| l.bomb)) {
                if age < BOMB_MS {
                    let k = age / BOMB_MS;
                    let r = lw * (0.45 + 0.9 * k);
                    let ring = lw * 0.18 * (1.0 - k) + lw * 0.04;
                    ellipse(d, f, cu, dep.judge, r, ring, with_alpha(c, 0.9 * (1.0 - k)));
                    band(
                        d,
                        f,
                        (l.u0, l.u1),
                        dep.judge,
                        dep.judge + 0.18 * (1.0 - k),
                        with_alpha(c, 0.5 * (1.0 - k)),
                        with_alpha(c, 0.0),
                    );
                }
            }
            if s.engine_for(l.lane)
                .and_then(|e| e.holding(l.lane))
                .is_some()
            {
                let pulse = 0.6 + 0.4 * ((s.chart_us(now) / 1000) as f32 * 0.02).sin().abs();
                ellipse(
                    d,
                    f,
                    cu,
                    dep.judge,
                    lw * 0.55,
                    lw * 0.2,
                    with_alpha(c, 0.5 * pulse),
                );
            }
        }
        d.b.set_blend(Blend::Alpha);
    }
}

/// A ring lying on the floor around (u, at): `r` across, `t` thick (in
/// lane widths), drawn through the perspective.
fn ellipse(d: &mut Draw, f: &Fan, u: f32, at: f32, r: f32, t: f32, color: Color) {
    const SEG: usize = 24;
    // round on the screen near the line: a lane width across is this deep
    let aspect = depth_of_px(f, f.width, at) * 0.45;
    let pt = |a: f32, rr: f32| {
        let (sn, cs) = a.sin_cos();
        f.point(u + cs * rr, at + sn * rr * aspect)
    };
    let clear = with_alpha(color, 0.0);
    for i in 0..SEG {
        let a0 = i as f32 / SEG as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / SEG as f32 * std::f32::consts::TAU;
        d.b.quad4(
            [pt(a0, r + t), pt(a1, r + t), pt(a1, r), pt(a0, r)],
            [clear, clear, color, color],
        );
        d.b.quad4(
            [
                pt(a0, r),
                pt(a1, r),
                pt(a1, (r - t).max(0.0)),
                pt(a0, (r - t).max(0.0)),
            ],
            [color, color, clear, clear],
        );
    }
}
