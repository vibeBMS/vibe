//! The graphs skins place (`skin::Graph`), drawn by the play and result
//! scenes: gauge, notes distribution, BPM, timing distribution and the
//! timing / hit error visualizer. Without skin colours they take the
//! built-in skin's look.

use crate::app::Draw;
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Color, Rect};
use crate::skin::{BpmGraph, GaugeGraph, NotesGraph, TimingGraph, VisualizerGraph};
use vibebms_core::engine::PlayNote;
use vibebms_core::gauge::GaugeType;
use vibebms_core::{LnType, PlayMode};

/// Where a graph goes: its rect, the object's colour (multiplies the
/// graph's colours) and how much of it has appeared (0..1, left to right).
pub struct Area {
    pub r: Rect,
    pub color: Color,
    pub reveal: f32,
}

impl Area {
    fn tint(&self, c: [f32; 4]) -> Color {
        let k = self.color;
        [c[0] * k[0], c[1] * k[1], c[2] * k[2], c[3] * k[3]]
    }

    /// `c` at `a` times the object's alpha.
    fn fade(&self, c: Color, a: f32) -> Color {
        with_alpha(c, a * self.color[3])
    }

    /// The right edge of what has appeared.
    fn reveal_x(&self) -> f32 {
        self.r.x + self.r.w * self.reveal.clamp(0.0, 1.0)
    }
}

/// The gauge's built-in colour (above or below the border).
fn gauge_color(t: GaugeType, above: bool) -> Color {
    rgba(match t {
        GaugeType::Hard | GaugeType::Class => 0xff5060ff,
        GaugeType::ExHard | GaugeType::ExHardClass | GaugeType::ExClass => 0xffd34aff,
        GaugeType::Hazard => 0xff963cff,
        _ if above => 0xff5c96ff,
        GaugeType::Easy => 0x46dc82ff,
        GaugeType::AssistEasy => 0xb280ffff,
        _ => 0x3caaffff,
    })
}

/// The gauge over the play: `log` sampled every 500 ms, shown in 2% steps
/// as a stepped line whose colour changes across the border, over a
/// background split at the border.
pub fn gauge(d: &mut Draw, a: &Area, g: &GaugeGraph, log: &[f32], ty: GaugeType) {
    let r = a.r;
    let border = match ty {
        GaugeType::AssistEasy | GaugeType::Easy => 60.0,
        GaugeType::Normal => 80.0,
        _ => 0.0,
    };
    // line above the border, background above, line below, background below
    let [line_hi, bg_hi, line_lo, bg_lo] = match &g.colors {
        Some(c) if c.len() >= 24 => {
            // beatoraja's colours per gauge kind (and its table for class gauges)
            let k = [0, 1, 2, 3, 4, 5, 3, 4, 5][ty as usize];
            [0, 1, 2, 3].map(|i| a.tint(c[k * 4 + i]))
        }
        _ => {
            let (hi, lo) = (gauge_color(ty, true), gauge_color(ty, false));
            [
                a.fade(hi, 1.0),
                a.fade(hi, 0.1),
                a.fade(lo, 1.0),
                a.fade(lo, 0.1),
            ]
        }
    };
    let y_of = |v: f32| r.bottom() - r.h * (v / 100.0).clamp(0.0, 1.0);
    let by = y_of(border);
    d.b.rect(Rect::new(r.x, r.y, r.w, by - r.y), bg_hi);
    if border > 0.0 {
        d.b.rect(Rect::new(r.x, by, r.w, r.bottom() - by), bg_lo);
    }
    let built_in = g.colors.is_none();
    if built_in {
        if border > 0.0 {
            d.b.rect(Rect::new(r.x, by, r.w, 1.0), a.fade(line_hi, 0.55));
        }
        for v in [25.0, 50.0, 75.0] {
            if v != border {
                d.b.rect(Rect::new(r.x, y_of(v), r.w, 1.0), a.fade([1.0; 4], 0.06));
            }
        }
    }
    if log.is_empty() {
        return;
    }
    let lw = if g.line_width > 0.0 {
        g.line_width
    } else {
        (r.h / 100.0).clamp(1.5, 3.0)
    };
    let col = |v: f32| if v >= border { line_hi } else { line_lo };
    let n = log.len() as f32;
    let reveal = a.reveal_x();
    let mut prev: Option<f32> = None;
    for (i, v) in log.iter().enumerate() {
        let v = (v / 2.0).floor() * 2.0;
        let x1 = r.x + r.w * i as f32 / n;
        if x1 >= reveal {
            break;
        }
        let x2 = (r.x + r.w * (i + 1) as f32 / n).min(reveal);
        let y = y_of(v);
        if built_in {
            d.b.rect(
                Rect::new(x1, y, x2 - x1, r.bottom() - y),
                with_alpha(col(v), 0.18 * a.color[3]),
            );
        }
        // vertical step from the previous value, split at the border
        if let Some(p) = prev.filter(|p| *p != v) {
            let (lo, hi) = (p.min(v), p.max(v));
            let mut step = |v0: f32, v1: f32, c: Color| {
                d.b.rect(
                    Rect::new(
                        x1 - lw * 0.5,
                        y_of(v1) - lw * 0.5,
                        lw,
                        y_of(v0) - y_of(v1) + lw,
                    ),
                    c,
                );
            };
            if border > 0.0 && lo < border && hi > border {
                step(lo, border, line_lo);
                step(border, hi, line_hi);
            } else {
                step(lo, hi, col(hi));
            }
        }
        d.b.rect(Rect::new(x1, y - lw * 0.5, x2 - x1, lw), col(v));
        prev = Some(v);
    }
}

/// Chip categories of a column, bottom first.
pub type Column = [u32; 10];

/// beatoraja's chip colours per graph type.
const PALETTE: [&[u32]; 3] = [
    // scratch LN end, scratch LN, scratch, LN end, LN, note, mine
    &[
        0x44ff44ff, 0x228822ff, 0xff4444ff, 0x4444ffff, 0x222288ff, 0xccccccff, 0x880000ff,
    ],
    // not yet judged, PG, GR, GD, BD, PR
    &[
        0x555555ff, 0x0088ffff, 0x00ff88ff, 0xffff00ff, 0xff8800ff, 0xff0000ff,
    ],
    // not yet judged, PG, FAST GR / GD / BD / PR, SLOW GR / GD / BD / PR
    &[
        0x555555ff, 0x44ff44ff, 0x0088ffff, 0x0066ccff, 0x004488ff, 0x002244ff, 0xff8800ff,
        0xcc6600ff, 0x884400ff, 0x442200ff,
    ],
];

/// The columns of the three graph types from the engine's notes (`state`
/// 0 = not judged yet, else judge + 1; `play_time` > 0 = early).
pub fn columns(notes: &[PlayNote], mode: PlayMode, length_us: i64) -> [Vec<Column>; 3] {
    let secs = (length_us.max(0) / 1_000_000) as usize + 1;
    let mut out = [
        vec![[0; 10]; secs],
        vec![[0; 10]; secs],
        vec![[0; 10]; secs],
    ];
    let sec = |t: i64| ((t.max(0) / 1_000_000) as usize).min(secs - 1);
    // beatoraja buckets: 1..=5 (a miss counts as poor)
    let judged = |st: u8, early: bool| -> (usize, usize) {
        let st = st.min(5) as usize;
        let el = if st <= 1 || early { st } else { st + 4 };
        (st, el)
    };
    for n in notes {
        let i = sec(n.time);
        let scr = mode.is_scratch(n.lane as usize);
        let k = |scratch: usize, key: usize| if scr { scratch } else { key };
        if n.is_mine() {
            out[0][i][6] += 1;
            continue;
        }
        if n.is_long() {
            // LN body through every second it covers; with LN (not CN /
            // HCN) the end is no note of its own and shows as "LN end"
            let ln_end_only = n.ln_type() == Some(LnType::Ln);
            let e = sec(n.end_time);
            for col in &mut out[0][i..=e] {
                col[k(1, 4)] += 1;
            }
            if ln_end_only {
                out[0][e][k(1, 4)] -= 1;
                out[0][e][k(0, 3)] += 1;
            } else {
                let (st, el) = judged(n.end_state, n.end_play_time.unwrap_or(0) > 0);
                out[1][e][st] += 1;
                out[2][e][el] += 1;
            }
        } else {
            out[0][i][k(2, 5)] += 1;
        }
        let (st, el) = judged(n.state, n.play_time.unwrap_or(0) > 0);
        out[1][i][st] += 1;
        out[2][i][el] += 1;
    }
    out
}

/// A notes graph (beatoraja's): one column per second, one 5×5 chip per
/// note; `cursor_ms` is the play position.
pub fn notes(d: &mut Draw, a: &Area, g: &NotesGraph, cols: &[Column], cursor_ms: Option<f64>) {
    let r = a.r;
    let alpha = a.color[3];
    let palette = PALETTE[g.kind.min(2)];
    let n = cols.len().max(1);
    let tallest = cols
        .iter()
        .map(|c| c.iter().sum::<u32>() as usize)
        .max()
        .unwrap_or(0);
    let max = if tallest > 20 {
        ((tallest / 10) * 10 + 10).min(100)
    } else {
        20
    };
    // the graph is an n*5 x max*5 pixel image stretched over the rect
    let (sx, sy) = (r.w / (n * 5) as f32, r.h / (max * 5) as f32);
    if g.back {
        d.b.rect(r, [0.0, 0.0, 0.0, 0.8 * alpha]);
        for i in (10..max).step_by(10) {
            let c = 0.007 * i as f32;
            let y = r.bottom() - (i * 5) as f32 * sy;
            d.b.rect(
                Rect::new(r.x, y - 50.0 * sy, r.w, 50.0 * sy),
                [c, c, 0.0, alpha],
            );
        }
        for i in (0..n).step_by(10) {
            let c = if i % 60 == 0 { 0.25 } else { 0.125 };
            d.b.rect(
                Rect::new(r.x + (i * 5) as f32 * sx, r.y, sx.max(1.0), r.h),
                [c, c, c, alpha],
            );
        }
    }
    let cw = if g.no_gap_x { 5.0 } else { 4.0 } * sx;
    let ch = if g.no_gap { 5.0 } else { 4.0 } * sy;
    let shown = ((n as f32 * a.reveal).ceil() as usize).min(cols.len());
    for (i, col) in cols.iter().take(shown).enumerate() {
        let x = r.x + (i * 5) as f32 * sx;
        let mut order: Vec<usize> = (0..palette.len()).collect();
        if g.order_reverse {
            order.reverse();
        }
        let chips = order
            .into_iter()
            .flat_map(|k| std::iter::repeat_n(k, col[k] as usize))
            .take(max);
        for (j, k) in chips.enumerate() {
            let y = r.bottom() - (j * 5) as f32 * sy - ch;
            d.b.rect(Rect::new(x, y, cw, ch), with_alpha(rgba(palette[k]), alpha));
        }
    }
    if let Some(ms) = cursor_ms {
        let x = r.x + (ms / 1000.0 * 5.0) as f32 * sx;
        if x >= r.x && x <= r.right() {
            d.b.rect(Rect::new(x, r.y, 3.0 * sx, r.h), [1.0, 1.0, 1.0, alpha]);
        }
    }
}

/// BPM over the chart: a line per BPM segment coloured main / min / max /
/// other, with transition lines between segments. `log`: (µs, BPM).
pub fn bpm(
    d: &mut Draw,
    a: &Area,
    g: &BpmGraph,
    log: &[(i64, f64)],
    length_us: i64,
    main_bpm: f64,
) {
    let r = a.r;
    let (lo, hi) = log.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (_, b)| {
        (lo.min(*b), hi.max(*b))
    });
    if log.is_empty() || hi <= 0.0 {
        return;
    }
    let col = |k: usize| g.colors.get(k).map_or(a.color, |c| a.tint(*c));
    let span = (hi - lo).max(1.0);
    let lw = g.line_width.max(1.0);
    let reveal = a.reveal_x();
    let x_of = |t: i64| r.x + r.w * (t as f32 / length_us.max(1) as f32).clamp(0.0, 1.0);
    let y_of = |b: f64| r.bottom() - lw - (r.h - 2.0 * lw) * ((b - lo) / span) as f32;
    for (i, (t, b)) in log.iter().enumerate() {
        let x0 = x_of(*t);
        if x0 >= reveal {
            break;
        }
        let x1 = log.get(i + 1).map_or(r.right(), |n| x_of(n.0)).min(reveal);
        let k = if (b - main_bpm).abs() < 0.01 {
            0
        } else if (b - lo).abs() < 0.01 {
            1
        } else if (b - hi).abs() < 0.01 {
            2
        } else {
            3
        };
        d.b.rect(Rect::new(x0, y_of(*b), (x1 - x0).max(1.0), lw), col(k));
        if let Some((_, pb)) = i.checked_sub(1).map(|p| log[p]) {
            let (y0, y1) = (y_of(pb).min(y_of(*b)), y_of(pb).max(y_of(*b)));
            d.b.rect(Rect::new(x0, y0, lw, y1 - y0 + lw), col(5));
        }
    }
}

/// (average, standard deviation) of timing errors in ms (+ = late);
/// `log`: (time, error in µs with + = early).
pub fn timing_stats(log: &[(i64, i64)]) -> (f64, f64) {
    if log.is_empty() {
        return (0.0, 0.0);
    }
    let ms = || log.iter().map(|(_, d)| -(*d as f64) / 1000.0);
    let n = log.len() as f64;
    let avg = ms().sum::<f64>() / n;
    let avg = if avg.abs() < 0.005 { 0.0 } else { avg };
    let var = ms().map(|x| (x - avg) * (x - avg)).sum::<f64>() / n;
    (avg, var.sqrt())
}

/// Distribution of timing errors in 1 ms bins (late to the right), over
/// the judge windows (`windows`: (late, early) in µs, PG first).
pub fn timing(d: &mut Draw, a: &Area, g: &TimingGraph, log: &[(i64, i64)], windows: &[(i64, i64)]) {
    let r = a.r;
    let range = g.bins.max(3) / 2;
    let bw = r.w / (range * 2 + 1) as f32;
    // the centre of the bin of `ms`
    let x_of = |ms: f64| r.x + r.w * 0.5 + ms as f32 * bw;
    let skin = g.colors.as_deref().filter(|c| c.len() >= 8);
    if let Some(c) = skin {
        for (j, (late, early)) in windows.iter().enumerate().rev() {
            let x0 = x_of(-(*early as f64) / 1000.0 - 0.5).max(r.x);
            let x1 = x_of(-(*late as f64) / 1000.0 + 0.5).min(r.right());
            if x1 > x0 {
                d.b.rect(Rect::new(x0, r.y, x1 - x0, r.h), a.tint(c[3 + j.min(4)]));
            }
        }
    }
    let mut hist = vec![0u32; (range * 2 + 1) as usize];
    for (_, diff) in log {
        let ms = (-diff as f64 / 1000.0).round() as i64;
        if (-range..=range).contains(&ms) {
            hist[(ms + range) as usize] += 1;
        }
    }
    let max = hist.iter().copied().max().unwrap_or(1).max(1) as f32;
    for (i, n) in hist.iter().enumerate() {
        let h = r.h * (*n as f32 / max) * a.reveal;
        let c = match skin {
            Some(c) => a.tint(c[0]),
            None => a.fade(
                rgba(match (i as i64 - range).signum() {
                    -1 => 0x4fa8ffff,
                    1 => 0xff6070ff,
                    _ => 0xe6ecf5ff,
                }),
                1.0,
            ),
        };
        d.b.rect(
            Rect::new(r.x + i as f32 * bw, r.bottom() - h, bw.max(1.0), h),
            c,
        );
    }
    let (avg, dev) = timing_stats(log);
    let lw = g.line_width.max(1.0);
    let mut line = |ms: f64, c: Color, w: f32| {
        let x = x_of(ms);
        if x >= r.x && x <= r.right() {
            d.b.rect(Rect::new(x - w * 0.5, r.y, w, r.h), c);
        }
    };
    let Some(c) = skin else {
        // built-in: centre and average lines, the numbers as text (the
        // skin's integer + decimal number pair cannot show the sign of
        // |avg| < 1 ms)
        line(0.0, a.fade([1.0; 4], 0.35), 1.0);
        line(avg, a.fade(rgba(0xffd34aff), 0.8), 2.0);
        d.text(
            &format!("AVG {avg:+.2} ms    SD {dev:.2} ms"),
            r.x + 6.0,
            r.y + 4.0,
            (r.h * 0.11).clamp(10.0, 18.0),
            a.fade(rgba(0xe6ecf5ff), 1.0),
            Align::Left,
        );
        return;
    };
    if g.average {
        line(avg, a.tint(c[1]), lw);
    }
    if g.deviation {
        line(avg - dev, a.tint(c[2]), lw);
        line(avg + dev, a.tint(c[2]), lw);
    }
}

/// beatoraja's timing / hit error visualizer: the judge windows as bands,
/// the latest timing errors as ticks (newest tallest when decaying).
/// `recent`: timing errors in µs (+ = early), oldest first; `windows`:
/// note windows (late, early) in µs, PG first. Late is to the right.
pub fn visualizer(
    d: &mut Draw,
    a: &Area,
    g: &VisualizerGraph,
    recent: &[i64],
    windows: &[(i64, i64)],
) {
    const SLOTS: usize = 100;
    let r = a.r;
    let col = |k: usize| a.tint(g.colors.get(k).copied().unwrap_or([1.0; 4]));
    let range = g.range_ms.max(1.0);
    let rate = r.w / (range * 2.0 + 1.0);
    let x_of = |ms: f32| r.x + r.w * 0.5 + ms * rate;
    // judge bands, each only where the narrower ones are not
    let (mut left, mut right) = (0.0f32, 0.0f32);
    for (j, (late, early)) in windows.iter().take(5).enumerate() {
        if j == 4 && g.transparent_poor {
            break;
        }
        let l = (-(*early as f32) / 1000.0).clamp(-range, range);
        let rr = (-(*late as f32) / 1000.0).clamp(-range, range);
        if l < left {
            d.b.rect(Rect::new(x_of(l), r.y, (left - l) * rate, r.h), col(2 + j));
            left = l;
        }
        if rr > right {
            d.b.rect(
                Rect::new(x_of(right), r.y, (rr - right) * rate, r.h),
                col(2 + j),
            );
            right = rr;
        }
    }
    // 10 ms ticks, centre line
    let mut ms = -((range / 10.0).floor() * 10.0);
    while ms <= range {
        d.b.rect(
            Rect::new(x_of(ms), r.y, 1.0, r.h),
            a.fade([0.0, 0.0, 0.0, 1.0], 0.25),
        );
        ms += 10.0;
    }
    d.b.rect(Rect::new(x_of(0.0), r.y, 1.0, r.h), col(1));
    let judge_of = |diff: i64| {
        windows
            .iter()
            .position(|(l, e)| (*l..=*e).contains(&diff))
            .unwrap_or(4)
            .min(4)
    };
    let recent = &recent[recent.len().saturating_sub(SLOTS)..];
    let n = recent.len();
    let lw = g.line_width.max(1.0);
    for (i, diff) in recent.iter().enumerate() {
        let ms = -(*diff as f32) / 1000.0;
        if ms.abs() > range {
            continue;
        }
        // position among the last SLOTS judgements (newest = SLOTS - 1)
        let slot = SLOTS - n + i;
        let mut c = if g.by_judge {
            col(2 + judge_of(*diff))
        } else {
            col(0)
        };
        c[3] *= (slot + 1) as f32 / SLOTS as f32;
        let h = if g.decay {
            r.h * slot as f32 / SLOTS as f32
        } else {
            r.h
        };
        d.b.rect(
            Rect::new(x_of(ms) - lw * 0.5, r.y + (r.h - h) * 0.5, lw, h),
            c,
        );
    }
    if g.ema && n > 0 {
        let avg = recent
            .iter()
            .map(|d| -(*d as f32) / 1000.0)
            .reduce(|e, ms| e + 0.1 * (ms - e))
            .unwrap_or(0.0)
            .clamp(-range, range);
        d.b.rect(Rect::new(x_of(avg) - 1.0, r.y, 2.0, r.h), col(7));
    }
}
