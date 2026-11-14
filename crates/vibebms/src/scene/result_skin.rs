//! Result screen values and graphs for result skins (LR2 / beatoraja ids).

use super::graphs::{self, Area};
use super::result::ResultScene;
use super::skinned::{chart_option, ms_since, rank_index};
use crate::app::{Ctx, Draw};
use crate::gfx::Rect;
use crate::platform::Instant;
use crate::skin::render::Hooks;
use crate::skin::{Graph, SkinState};
use vibebms_core::gauge::GaugeType;
use vibebms_core::judge::*;
use vibebms_core::score::{ClearType, DjLevel};

/// Click events of result skins (vibeBMS ids): retry the chart / continue.
pub const EVENT_RETRY: i32 = 9500;
pub const EVENT_CONTINUE: i32 = 9501;

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
            50 => !self.ctx.config.ir_server.enabled,
            51 => self.ctx.config.ir_server.enabled,
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
        if let Some(v) = super::skinned::clock_number(id) {
            return Some(v);
        }
        let d = &self.s.d;
        let sc = &d.score;
        let before = self.s.before.as_ref();
        let after = self.s.after.as_ref();
        let ex = sc.ex_score() as i64;
        let best = self.best_ex() as i64;
        let target = self
            .ctx
            .config
            .play
            .target
            .score(sc.max_ex(), self.best_ex()) as i64;
        let rate = split2(sc.rate() * 100.0);
        let m = &d.meta;
        let (avg, dev) = graphs::timing_stats(&d.timing_log);
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
            150 | 170 => best,
            121 | 151 => target,
            108 | 152 | 172 => ex - best,
            128 | 153 => ex - target,
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
    /// Clickable objects drawn: (rect, event).
    pub clicks: Vec<(Rect, i32)>,
}

impl Hooks for ResultHooks<'_> {
    fn ref_image(&self, id: i32) -> Option<crate::gfx::TexId> {
        match id {
            100 | 101 => self.st.s.stage_tex,
            _ => None,
        }
    }

    fn clickable(&mut self, rect: Rect, act: i32, _click: i32) {
        self.clicks.push((rect, act));
    }

    fn graph(&mut self, d: &mut Draw, graph: &Graph, r: Rect, color: [f32; 4]) {
        let s = self.st.s;
        let data = &s.d;
        // graphs grow in (the skin's delay, else during the first second)
        let dur = match graph.delay() {
            0 => GRAPH_MS,
            ms => ms,
        };
        let a = Area {
            r,
            color,
            reveal: (ms_since(s.created, self.st.now) as f32 / dur as f32).clamp(0.0, 1.0),
        };
        match graph {
            Graph::Gauge(g) => graphs::gauge(d, &a, g, &data.gauge_log, data.gauge_type),
            Graph::Notes(g) => graphs::notes(d, &a, g, &data.notes_graph[g.kind.min(2)], None),
            Graph::Bpm(g) => {
                graphs::bpm(d, &a, g, &data.bpm_log, data.length_us, data.meta.main_bpm)
            }
            Graph::Timing(g) => graphs::timing(d, &a, g, &data.timing_log, &data.windows),
            Graph::Visualizer(_) => {}
        }
    }
}
