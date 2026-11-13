//! Result screen values and graphs for result skins (LR2 / beatoraja ids).

use super::result::ResultScene;
use super::skinned::{chart_option, ms_since, rank_index};
use crate::app::{Ctx, Draw};
use crate::gfx::{rgba, with_alpha, Color, Rect};
use crate::platform::Instant;
use crate::skin::render::Hooks;
use crate::skin::{GraphKind, SkinState};
use vibebms_core::gauge::GaugeType;
use vibebms_core::judge::*;
use vibebms_core::score::{ClearType, DjLevel};

/// Timer 151 (result graph end) fires this long after the scene starts.
pub const GRAPH_MS: i64 = 1000;

pub struct ResultState<'a> {
    pub s: &'a ResultScene,
    pub ctx: &'a Ctx,
    pub now: Instant,
}

impl ResultState<'_> {
    fn ms(&self, t: Instant) -> i64 {
        ms_since(self.s.created, t)
    }

    fn best_ex(&self) -> u32 {
        self.s.before.as_ref().map(|b| b.ex_score).unwrap_or(0)
    }

    /// Options that do not change while the screen is shown.
    pub fn static_option(&self, id: i32) -> Option<bool> {
        let d = &self.s.d;
        let sc = &d.score;
        let before = self.s.before.as_ref();
        let ex = sc.ex_score();
        let rank = rank_index(sc.dj_level());
        if let Some(v) = chart_option(&d.meta, d.mode, id) {
            return Some(v);
        }
        Some(match id {
            32 => !d.autoplay,
            33 => d.autoplay,
            42 => !d.gauge_type.is_survival(),
            43 => d.gauge_type.is_survival(),
            1046 => matches!(d.gauge_type, GaugeType::ExHard | GaugeType::ExHardClass),
            50 => !self.ctx.config.ir.iter().any(|c| c.enabled),
            51 => self.ctx.config.ir.iter().any(|c| c.enabled),
            60 => d.autoplay,
            61 => !d.autoplay,
            90 => d.clear != ClearType::Failed,
            91 => d.clear == ClearType::Failed,
            118 => d.gauge_type == GaugeType::Normal,
            119 => matches!(d.gauge_type, GaugeType::Hard | GaugeType::Class),
            120 => d.gauge_type == GaugeType::Hazard,
            121 => d.gauge_type == GaugeType::Easy,
            124 => d.gauge_type == GaugeType::AssistEasy,
            125 => matches!(
                d.gauge_type,
                GaugeType::ExHard | GaugeType::ExHardClass | GaugeType::ExClass
            ),
            290 => d.course.is_some(),
            300..=307 => rank == id - 300,
            340..=347 => rank == id - 340,
            308 => ex == 0,
            320..=327 => before
                .map(|b| rank_index(DjLevel::from_rate(b.ex_score, b.notes * 2)) == id - 320)
                .unwrap_or(id == 327),
            330 => ex > self.best_ex(),
            1330 => ex == self.best_ex(),
            331 => before.map(|b| sc.max_combo > b.max_combo).unwrap_or(true),
            1331 => before.map(|b| sc.max_combo == b.max_combo).unwrap_or(false),
            332 => before.map(|b| sc.miss_count() < b.min_bp).unwrap_or(true),
            1332 => before.map(|b| sc.miss_count() == b.min_bp).unwrap_or(false),
            335 => before
                .map(|b| sc.dj_level() > DjLevel::from_rate(b.ex_score, b.notes * 2))
                .unwrap_or(true),
            2241..=2246 => sc.count((id - 2241) as usize) > 0,
            _ => return None,
        })
    }

    fn diff_next_rank(&self) -> i64 {
        let sc = &self.s.d.score;
        let max = sc.max_ex() as i64;
        let ex = sc.ex_score() as i64;
        (2..=9)
            .map(|k| (max * k + 8) / 9)
            .chain(std::iter::once(max))
            .find(|t| *t > ex)
            .map(|t| ex - t)
            .unwrap_or(0)
    }

    /// (average, standard deviation) of the timing error in ms (+ = late).
    pub fn timing_stats(&self) -> (f64, f64) {
        let v: Vec<f64> = self
            .s
            .d
            .timing_log
            .iter()
            .map(|(_, d)| -(*d as f64) / 1000.0)
            .collect();
        if v.is_empty() {
            return (0.0, 0.0);
        }
        let n = v.len() as f64;
        let avg = v.iter().sum::<f64>() / n;
        let avg = if avg.abs() < 0.005 { 0.0 } else { avg };
        let var = v.iter().map(|x| (x - avg) * (x - avg)).sum::<f64>() / n;
        (avg, var.sqrt())
    }
}

fn split2(v: f64) -> (i64, i64) {
    let x = (v * 100.0).round() as i64;
    (x / 100, (x % 100).abs())
}

impl SkinState for ResultState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        let s = self.s;
        match id {
            0 | 150 => Some(0),
            1 => Some(s.skin.as_ref().map(|k| k.skin.input_ms).unwrap_or(0)),
            2 => s.leaving.map(|t| self.ms(t)),
            3 => (s.d.clear == ClearType::Failed).then_some(0),
            151 => Some(GRAPH_MS),
            152 => (s.d.score.ex_score() > self.best_ex() && !s.d.autoplay).then_some(GRAPH_MS),
            _ => None,
        }
    }

    fn option(&self, id: i32) -> bool {
        let v = self.static_option(id.abs()).unwrap_or(false);
        v != (id < 0)
    }

    fn number(&self, id: i32) -> Option<i64> {
        let d = &self.s.d;
        let sc = &d.score;
        let before = self.s.before.as_ref();
        let after = self.s.after.as_ref();
        let ex = sc.ex_score() as i64;
        let best = self.best_ex() as i64;
        let rate = split2(sc.rate() * 100.0);
        let m = &d.meta;
        let (avg, dev) = self.timing_stats();
        let bp = |b: &crate::library::BestScore| {
            if b.min_bp == u32::MAX {
                0
            } else {
                b.min_bp as i64
            }
        };
        Some(match id {
            20 => self.ctx.fps as i64,
            71 | 101 | 171 => ex,
            72 => sc.max_ex() as i64,
            74 | 106 => sc.total_notes as i64,
            75 | 105 | 174 => sc.max_combo as i64,
            76 | 177 => sc.miss_count() as i64,
            77 => after.map(|b| b.play_count as i64).unwrap_or(0),
            78 => after.map(|b| b.clear_count as i64).unwrap_or(0),
            79 => after
                .map(|b| (b.play_count - b.clear_count) as i64)
                .unwrap_or(0),
            90 => m.max_bpm.round() as i64,
            91 => m.min_bpm.round() as i64,
            92 => m.main_bpm.round() as i64,
            96 => m.level.trim().parse::<f64>().ok()? as i64,
            102 | 115 | 155 => rate.0,
            103 | 116 | 156 => rate.1,
            107 => d.gauge_value as i64,
            407 => ((d.gauge_value * 10.0) as i64) % 10,
            110 => sc.count(PGREAT) as i64,
            111 => sc.count(GREAT) as i64,
            112 => sc.count(GOOD) as i64,
            113 => sc.count(BAD) as i64,
            114 => sc.count(POOR) as i64,
            410..=419 => sc.counts[((id - 410) / 2) as usize][((id - 410) % 2) as usize] as i64,
            420 => sc.count(MISS) as i64,
            421 | 422 => sc.counts[MISS][(id - 421) as usize] as i64,
            423 => sc.fast() as i64,
            424 => sc.slow() as i64,
            425 => sc.combo_breaks as i64,
            426 => (sc.count(POOR) + sc.count(MISS)) as i64,
            427 => (sc.count(BAD) + sc.count(POOR) + sc.count(MISS)) as i64,
            121 | 150 | 151 | 170 => best,
            108 | 128 | 152 | 153 | 172 => ex - best,
            154 => self.diff_next_rank(),
            173 => before.map(|b| b.max_combo as i64).unwrap_or(0),
            175 => sc.max_combo as i64 - before.map(|b| b.max_combo as i64).unwrap_or(0),
            176 => before.map(bp).unwrap_or(0),
            178 => sc.miss_count() as i64 - before.map(bp).unwrap_or(0),
            122 | 157 | 183 => split2(before.map(|b| b.rate()).unwrap_or(0.0) * 100.0).0,
            123 | 158 | 184 => split2(before.map(|b| b.rate()).unwrap_or(0.0) * 100.0).1,
            312 => self.ctx.config.play.duration_ms as i64,
            370 => d.clear as i64,
            371 => before.map(|b| b.clear as i64).unwrap_or(0),
            374 => split2(avg).0,
            375 => split2(avg).1,
            376 => split2(dev).0,
            377 => split2(dev).1,
            _ => return None,
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let m = &self.s.d.meta;
        Some(match id {
            2 => self.ctx.config.player_name.clone(),
            10 => m.title.clone(),
            11 => m.subtitle.clone(),
            12 => format!("{} {}", m.title, m.subtitle).trim().to_string(),
            13 => m.genre.clone(),
            14 => m.artist.clone(),
            15 => m.subartist.clone(),
            16 => format!("{} {}", m.artist, m.subartist).trim().to_string(),
            1010 => format!("vibeBMS {}", env!("CARGO_PKG_VERSION")),
            _ => return None,
        })
    }

    fn rate(&self, id: i32) -> f32 {
        let sc = &self.s.d.score;
        let n = sc.total_notes.max(1) as f32;
        let best = self
            .s
            .before
            .as_ref()
            .map(|b| b.rate() as f32)
            .unwrap_or(0.0);
        match id {
            110 | 111 | 147 => sc.rate() as f32,
            112..=115 => best,
            140..=144 => sc.count((id - 140) as usize) as f32 / n,
            145 => sc.max_combo as f32 / n,
            _ => 0.0,
        }
    }
}

pub struct ResultHooks<'a> {
    pub st: &'a ResultState<'a>,
}

const JUDGE_COLORS: [u32; 6] = [
    0x6fe3ffff, 0xffd34aff, 0x5fdc8aff, 0xb58cffff, 0xff5a6eff, 0x80303aff,
];

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

impl Hooks for ResultHooks<'_> {
    fn graph(&mut self, d: &mut Draw, kind: GraphKind, r: Rect, color: [f32; 4]) {
        let s = self.st.s;
        let data = &s.d;
        // graphs grow in during the first second
        let grow = (ms_since(s.created, self.st.now) as f32 / GRAPH_MS as f32).clamp(0.0, 1.0);
        let a = color[3];
        match kind {
            GraphKind::Gauge => {
                // beatoraja / IIDX style: the gauge sampled every 500 ms and
                // shown in 2% steps as a stepped line; the background is
                // split at the border and the line changes colour across it
                let log = &data.gauge_log;
                let border = match data.gauge_type {
                    GaugeType::AssistEasy | GaugeType::Easy => 60.0,
                    GaugeType::Normal => 80.0,
                    _ => 0.0,
                };
                let below = gauge_color(data.gauge_type, false);
                let above = gauge_color(data.gauge_type, true);
                let y_of = |v: f32| r.bottom() - r.h * (v / 100.0).clamp(0.0, 1.0);
                let by = y_of(border);
                if border > 0.0 {
                    d.b.rect(
                        Rect::new(r.x, by, r.w, r.bottom() - by),
                        with_alpha(below, 0.10 * a),
                    );
                    d.b.rect(
                        Rect::new(r.x, r.y, r.w, by - r.y),
                        with_alpha(above, 0.10 * a),
                    );
                    d.b.rect(Rect::new(r.x, by, r.w, 1.0), with_alpha(above, 0.55 * a));
                } else {
                    d.b.rect(r, with_alpha(below, 0.08 * a));
                }
                // 25 / 50 / 75 % guides
                for g in [25.0, 50.0, 75.0] {
                    if (g - border).abs() > 0.1 {
                        d.b.rect(
                            Rect::new(r.x, y_of(g), r.w, 1.0),
                            with_alpha(rgba(0xffffffff), 0.06 * a),
                        );
                    }
                }
                if log.is_empty() {
                    return;
                }
                let q = |v: f32| (v / 2.0).floor() * 2.0;
                let n = log.len();
                let lw = (r.h / 100.0).clamp(1.5, 3.0);
                let col = |v: f32| {
                    if border > 0.0 && v >= border {
                        above
                    } else {
                        below
                    }
                };
                let reveal = r.x + r.w * grow;
                let mut prev: Option<f32> = None;
                for (i, v) in log.iter().enumerate() {
                    let v = q(*v);
                    let x1 = r.x + r.w * i as f32 / n as f32;
                    let x2 = r.x + r.w * (i + 1) as f32 / n as f32;
                    if x1 >= reveal {
                        break;
                    }
                    let x2 = x2.min(reveal);
                    let y = y_of(v);
                    // area under the line, faint
                    d.b.rect(
                        Rect::new(x1, y, x2 - x1, r.bottom() - y),
                        with_alpha(col(v), 0.18 * a),
                    );
                    // vertical step from the previous value, split at the border
                    if let Some(p) = prev.filter(|p| *p != v) {
                        let (lo, hi) = if p < v { (p, v) } else { (v, p) };
                        let mut seg = |a0: f32, a1: f32, c| {
                            if a1 > a0 {
                                d.b.rect(
                                    Rect::new(
                                        x1 - lw * 0.5,
                                        y_of(a1) - lw * 0.5,
                                        lw,
                                        y_of(a0) - y_of(a1) + lw,
                                    ),
                                    with_alpha(c, a),
                                );
                            }
                        };
                        if border > 0.0 && lo < border && hi > border {
                            seg(lo, border, below);
                            seg(border, hi, above);
                        } else {
                            seg(lo, hi, col(hi));
                        }
                    }
                    d.b.rect(
                        Rect::new(x1, y - lw * 0.5, x2 - x1, lw),
                        with_alpha(col(v), a),
                    );
                    prev = Some(v);
                }
            }
            GraphKind::Judge(t) => {
                let len = data.length_us.max(1) as f32;
                let cols = (r.w / 4.0).max(1.0) as usize;
                let mut buckets = vec![[0u32; 8]; cols];
                for (time, j, sign) in &data.judge_log {
                    let i = ((*time as f32 / len) * cols as f32) as usize;
                    let b = &mut buckets[i.min(cols - 1)];
                    match t {
                        2 => {
                            b[match sign {
                                1 => 0,
                                -1 => 1,
                                _ => 2,
                            }] += 1
                        }
                        _ => b[(*j as usize).min(5)] += 1,
                    }
                }
                let max = buckets
                    .iter()
                    .map(|b| b.iter().sum::<u32>())
                    .max()
                    .unwrap_or(1)
                    .max(1) as f32;
                let shown = ((cols as f32) * grow) as usize;
                let bw = r.w / cols as f32;
                for (i, b) in buckets.iter().take(shown).enumerate() {
                    let mut y = r.bottom();
                    let order: &[(usize, u32)] = match t {
                        0 => &[
                            (0, 0x7f8cffff),
                            (1, 0x7f8cffff),
                            (2, 0x7f8cffff),
                            (3, 0x7f8cffff),
                            (4, 0x7f8cffff),
                            (5, 0x7f8cffff),
                        ],
                        2 => &[(2, 0xe6ecf5ff), (0, 0x4fa8ffff), (1, 0xff6070ff)],
                        _ => &[
                            (0, JUDGE_COLORS[0]),
                            (1, JUDGE_COLORS[1]),
                            (2, JUDGE_COLORS[2]),
                            (3, JUDGE_COLORS[3]),
                            (4, JUDGE_COLORS[4]),
                            (5, JUDGE_COLORS[5]),
                        ],
                    };
                    for (k, c) in order {
                        let h = r.h * b[*k] as f32 / max;
                        if h > 0.0 {
                            d.b.rect(
                                Rect::new(r.x + i as f32 * bw, y - h, (bw - 1.0).max(1.0), h),
                                with_alpha(rgba(*c), a),
                            );
                            y -= h;
                        }
                    }
                }
            }
            GraphKind::Bpm => {
                let log = &data.bpm_log;
                let (lo, hi) = log.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (_, b)| {
                    (lo.min(*b), hi.max(*b))
                });
                if log.is_empty() || hi <= 0.0 {
                    return;
                }
                let span = (hi - lo).max(1.0);
                let len = data.length_us.max(1) as f32;
                for (i, (t, b)) in log.iter().enumerate() {
                    let x0 = r.x + r.w * (*t as f32 / len).clamp(0.0, 1.0);
                    let x1 = log
                        .get(i + 1)
                        .map(|n| r.x + r.w * (n.0 as f32 / len).clamp(0.0, 1.0))
                        .unwrap_or(r.right());
                    let y = r.bottom() - 2.0 - (r.h - 4.0) * ((b - lo) / span) as f32;
                    let c = if (b - data.meta.main_bpm).abs() < 0.01 {
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
            GraphKind::TimingDistribution => {
                // -range..+range ms in 1 ms bins (+ = late)
                let range = 40i64;
                let bins = (range * 2 + 1) as usize;
                let mut hist = vec![0u32; bins];
                for (_, diff) in &data.timing_log {
                    let ms = (-diff as f64 / 1000.0).round() as i64;
                    if (-range..=range).contains(&ms) {
                        hist[(ms + range) as usize] += 1;
                    }
                }
                let max = hist.iter().copied().max().unwrap_or(1).max(1) as f32;
                let bw = r.w / bins as f32;
                for (i, c) in hist.iter().enumerate() {
                    let h = r.h * (*c as f32 / max) * grow;
                    let col = match (i as i64 - range).signum() {
                        -1 => 0x4fa8ffff,
                        1 => 0xff6070ff,
                        _ => 0xe6ecf5ff,
                    };
                    d.b.rect(
                        Rect::new(r.x + i as f32 * bw, r.bottom() - h, (bw - 0.5).max(1.0), h),
                        with_alpha(rgba(col), a),
                    );
                }
                d.b.rect(
                    Rect::new(r.x + r.w * 0.5, r.y, 1.0, r.h),
                    with_alpha(rgba(0xffffffff), 0.35 * a),
                );
                // average (line) and spread, as text: the integer + decimal
                // number pair cannot show a sign for |avg| < 1 ms
                let (avg, dev) = self.st.timing_stats();
                let ax = r.x + r.w * (0.5 + (avg / (2 * range + 1) as f64) as f32);
                d.b.rect(
                    Rect::new(ax, r.y, 2.0, r.h),
                    with_alpha(rgba(0xffd34aff), 0.8 * a),
                );
                let sz = (r.h * 0.11).clamp(10.0, 18.0);
                d.text(
                    &format!("AVG {avg:+.2} ms    SD {dev:.2} ms"),
                    r.x + 6.0,
                    r.y + 4.0,
                    sz,
                    with_alpha(rgba(0xe6ecf5ff), a),
                    crate::gfx::text::Align::Left,
                );
            }
        }
    }
}
