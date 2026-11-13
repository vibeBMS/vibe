//! beatoraja-style notes distribution graph shared by play and result
//! skins: one column per second, one 5×5 "chip" per note, coloured by note
//! type (type 0), judgement (1) or judgement + FAST / SLOW (2).

use crate::app::Draw;
use crate::gfx::{rgba, with_alpha, Rect};
use crate::skin::GraphStyle;
use vibebms_core::engine::PlayNote;
use vibebms_core::{LnType, PlayMode};

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

/// Draw a notes graph: `kind` 0..=2, `reveal` 0..1 (left to right),
/// `cursor_ms` the play position line.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    d: &mut Draw,
    r: Rect,
    data: &[Column],
    kind: usize,
    style: &GraphStyle,
    alpha: f32,
    reveal: f32,
    cursor_ms: Option<f64>,
) {
    let palette = PALETTE[kind.min(2)];
    let [no_gap, reverse, back, no_gap_x] = style.flags;
    let n = data.len().max(1);
    let tallest = data
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
    if back {
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
    let (cw, ch) = (
        (4.0 + no_gap_x as u8 as f32) * sx,
        (4.0 + no_gap as u8 as f32) * sy,
    );
    let shown = ((n as f32 * reveal).ceil() as usize).min(data.len());
    for (i, col) in data.iter().take(shown).enumerate() {
        let x = r.x + (i * 5) as f32 * sx;
        let mut j = 0;
        let order: Vec<usize> = if reverse {
            (0..palette.len()).rev().collect()
        } else {
            (0..palette.len()).collect()
        };
        for k in order {
            let c = with_alpha(rgba(palette[k]), alpha);
            for _ in 0..col[k] {
                if j >= max {
                    break;
                }
                let y = r.bottom() - (j * 5) as f32 * sy - ch;
                d.b.rect(Rect::new(x, y, cw, ch), c);
                j += 1;
            }
        }
    }
    if let Some(ms) = cursor_ms {
        let x = r.x + (ms / 1000.0 * 5.0) as f32 * sx;
        if x >= r.x && x <= r.right() {
            d.b.rect(Rect::new(x, r.y, 3.0 * sx, r.h), [1.0, 1.0, 1.0, alpha]);
        }
    }
}

/// BPM graph: a line per BPM segment. With skin colours (beatoraja):
/// main / min / max / other BPM and transition lines.
#[allow(clippy::too_many_arguments)]
pub fn bpm(
    d: &mut Draw,
    r: Rect,
    log: &[(i64, f64)],
    length_us: i64,
    main_bpm: f64,
    style: &GraphStyle,
    color: [f32; 4],
    grow: f32,
) {
    let a = color[3];
    let skin_col = |k: usize| {
        style.colors.get(k).map(|c| {
            [
                c[0] * color[0],
                c[1] * color[1],
                c[2] * color[2],
                c[3] * color[3],
            ]
        })
    };

    let (lo, hi) = log.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (_, b)| {
        (lo.min(*b), hi.max(*b))
    });
    if log.is_empty() || hi <= 0.0 {
        return;
    }
    let span = (hi - lo).max(1.0);
    let len = length_us.max(1) as f32;
    let lw = if style.line_width > 0.0 {
        style.line_width
    } else {
        2.0
    };
    let reveal = r.x + r.w * grow;
    let y_of = |b: f64| r.bottom() - lw - (r.h - 2.0 * lw) * ((b - lo) / span) as f32;
    if !style.colors.is_empty() {
        // beatoraja: main / min / max / other BPM colours and
        // transition lines between segments
        for (i, (t, b)) in log.iter().enumerate() {
            let x0 = r.x + r.w * (*t as f32 / len).clamp(0.0, 1.0);
            if x0 >= reveal {
                break;
            }
            let x1 = log
                .get(i + 1)
                .map(|n| r.x + r.w * (n.0 as f32 / len).clamp(0.0, 1.0))
                .unwrap_or(r.right())
                .min(reveal);
            let k = if (b - main_bpm).abs() < 0.01 {
                0
            } else if (b - lo).abs() < 0.01 {
                1
            } else if (b - hi).abs() < 0.01 {
                2
            } else {
                3
            };
            let c = skin_col(k).unwrap_or(color);
            d.b.rect(Rect::new(x0, y_of(*b), (x1 - x0).max(1.0), lw), c);
            if let Some((_, pb)) = i.checked_sub(1).map(|p| log[p]) {
                let (y0, y1) = (y_of(pb).min(y_of(*b)), y_of(pb).max(y_of(*b)));
                if let Some(tc) = skin_col(5) {
                    d.b.rect(Rect::new(x0, y0, lw, y1 - y0 + lw), tc);
                }
            }
        }
        return;
    }
    for (i, (t, b)) in log.iter().enumerate() {
        let x0 = r.x + r.w * (*t as f32 / len).clamp(0.0, 1.0);
        let x1 = log
            .get(i + 1)
            .map(|n| r.x + r.w * (n.0 as f32 / len).clamp(0.0, 1.0))
            .unwrap_or(r.right());
        let y = r.bottom() - 2.0 - (r.h - 4.0) * ((b - lo) / span) as f32;
        let c = if (b - main_bpm).abs() < 0.01 {
            0x5fdc8aff
        } else {
            0xffd34aff
        };
        d.b.rect(
            Rect::new(x0, y, (x1 - x0).max(1.0), 2.0),
            with_alpha(rgba(c), a),
        );
    }
}

/// beatoraja timing / hit error visualizer: the judge windows as bands,
/// the latest timing errors as ticks (newest tallest when decaying).
/// `recent`: timing errors in µs (+ = early), oldest first; `windows`:
/// note windows (late, early) in µs, PG first. Shown with late to the right.
#[allow(clippy::too_many_arguments)]
pub fn timing_visualizer(
    d: &mut Draw,
    r: Rect,
    recent: &[i64],
    windows: &[(i64, i64)],
    style: &GraphStyle,
    color: [f32; 4],
    hit_error: bool,
) {
    const SLOTS: usize = 100;
    let col = |k: usize| {
        let c = style.colors.get(k).copied().unwrap_or([1.0; 4]);
        [
            c[0] * color[0],
            c[1] * color[1],
            c[2] * color[2],
            c[3] * color[3],
        ]
    };
    let [transparent_poor, decay, by_judge, ema] = style.flags;
    let center = style.width.max(1) as f32;
    let rate = r.w / (center * 2.0 + 1.0);
    let x_of = |ms: f32| r.x + r.w * 0.5 + ms * rate;
    // judge bands, each only where the narrower ones are not
    let (mut left, mut right) = (0.0f32, 0.0f32);
    for (j, (late, early)) in windows.iter().take(5).enumerate() {
        if j == 4 && transparent_poor {
            break;
        }
        let l = (-(*early as f32) / 1000.0).clamp(-center, center);
        let rr = (-(*late as f32) / 1000.0).clamp(-center, center);
        let c = col(2 + j);
        if l < left {
            d.b.rect(Rect::new(x_of(l), r.y, (left - l) * rate, r.h), c);
            left = l;
        }
        if rr > right {
            d.b.rect(Rect::new(x_of(right), r.y, (rr - right) * rate, r.h), c);
            right = rr;
        }
    }
    // 10 ms ticks, centre line
    let mut ms = -((center / 10.0).floor() * 10.0);
    while ms <= center {
        d.b.rect(
            Rect::new(x_of(ms), r.y, 1.0, r.h),
            [0.0, 0.0, 0.0, 0.25 * color[3]],
        );
        ms += 10.0;
    }
    d.b.rect(Rect::new(x_of(0.0), r.y, 1.0, r.h), col(1));
    let judge_of = |diff: i64| {
        windows
            .iter()
            .position(|(l, e)| diff >= *l && diff <= *e)
            .unwrap_or(4)
            .min(4)
    };
    let recent = &recent[recent.len().saturating_sub(SLOTS)..];
    let n = recent.len();
    let lw = style.line_width.max(1.0);
    for (i, diff) in recent.iter().enumerate() {
        let ms = -(*diff as f32) / 1000.0;
        if ms.abs() > center {
            continue;
        }
        // position among the last SLOTS judgements (newest = SLOTS - 1)
        let slot = SLOTS - n + i;
        let mut c = if hit_error && by_judge {
            col(2 + judge_of(*diff))
        } else {
            col(0)
        };
        c[3] *= (slot + 1) as f32 / SLOTS as f32;
        let h = if decay {
            r.h * slot as f32 / SLOTS as f32
        } else {
            r.h
        };
        d.b.rect(
            Rect::new(x_of(ms) - lw * 0.5, r.y + (r.h - h) * 0.5, lw, h),
            c,
        );
    }
    if hit_error && ema && n > 0 {
        let a = 0.1f32;
        let avg = recent
            .iter()
            .fold(None, |acc: Option<f32>, d| {
                let ms = -(*d as f32) / 1000.0;
                Some(acc.map_or(ms, |e| e + a * (ms - e)))
            })
            .unwrap_or(0.0)
            .clamp(-center, center);
        d.b.rect(Rect::new(x_of(avg) - 1.0, r.y, 2.0, r.h), col(7));
    }
}
