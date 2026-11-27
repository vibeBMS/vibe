//! Result screen values and graphs for result skins (LR2 / beatoraja ids).

use super::graphs::{self, Area};
use super::result::{Outcome, ResultScene, SideResult};
use super::skinvals::*;
use crate::app::Ctx;
use crate::config::Bpms;
use crate::gfx::{Draw, Rect};
use crate::library::BestScore;
use crate::platform::Instant;
use crate::skin::render::Hooks;
use crate::skin::{Graph, SkinState};
use std::cell::OnceCell;
use vibebms_core::judge::Judge;
use vibebms_core::score::{ClearType, DjLevel};

/// Click events of result skins (vibeBMS ids): retry the chart / continue.
pub const EVENT_RETRY: i32 = 9500;
pub const EVENT_CONTINUE: i32 = 9501;
/// vibeBMS: watch this play again (its replay).
pub const EVENT_REPLAY: i32 = 9502;
/// vibeBMS: keep this play's replay in slot 1..4 (`EVENT_SAVE_SLOT + n -
/// 1`, as keys 1-4 do).
pub const EVENT_SAVE_SLOT: i32 = 9510;

/// vibeBMS option: the result of a battle (a second side to show).
pub const OPTION_BATTLE: i32 = 9350;
/// vibeBMS option: the play has a replay (it can be kept in a slot).
pub const OPTION_HAS_REPLAY: i32 = 9352;
/// vibeBMS text: who the second side was ("MY BEST", "LAST", "2P").
pub const TEXT_OPPONENT: i32 = 9351;

/// Timer 151 (result graph end) fires this long after the scene starts.
pub const GRAPH_MS: i64 = 1000;

/// The result screen's values in one frame.
pub struct ResultState<'a> {
    pub s: &'a ResultScene,
    pub ctx: &'a Ctx,
    pub now: Instant,
    chart: ChartFacts<'a>,
    table: OnceCell<Option<(String, String)>>,
}

impl<'a> ResultState<'a> {
    pub fn new(s: &'a ResultScene, ctx: &'a Ctx, now: Instant) -> ResultState<'a> {
        ResultState {
            s,
            ctx,
            now,
            chart: ChartFacts::of_meta(&s.d.meta, s.d.mode),
            table: OnceCell::new(),
        }
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
        Some(match id {
            OPTION_BATTLE => d.p2.is_some(),
            OPTION_HAS_REPLAY => d.replay.is_some(),
            // the first side won, lost, drew (by EX score)
            352 => d.outcome() == Some(Outcome::Win),
            353 => d.outcome() == Some(Outcome::Lose),
            354 => d.outcome() == Some(Outcome::Draw),
            // the second side's gauge and DJ level
            44 | 45 => {
                d.p2.as_ref()
                    .is_some_and(|p| p.gauge_type.is_survival() == (id == 45))
            }
            210..=217 => d.p2.as_ref().is_some_and(|p| {
                rank_index(DjLevel::from_rate(p.ex, p.score.max_ex())) == id - 210
            }),
            150..=195 => return chart_option(&self.chart, id),
            32 | 33 | 42 | 43 | 1046 => return play_option(d.autoplay, false, d.gauge_type, id),
            118..=125 => return gauge_type_option(d.gauge_type, id),
            50 => self.ctx.config.ir.primary().is_none(),
            51 => self.ctx.config.ir.primary().is_some(),
            60 => d.autoplay,
            61 => !d.autoplay,
            90 => d.clear != ClearType::Failed,
            91 => d.clear == ClearType::Failed,
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
            2241..=2246 => sc.count(Judge::ALL[(id - 2241) as usize]) > 0,
            _ => return None,
        })
    }
}

/// A battle's second side's values (LR2 2P numbers, the target being it).
fn p2_number(mine: u32, p: &SideResult, id: i32) -> Option<i64> {
    let sc = &p.score;
    let rate = percent_parts(p.ex as f64 / sc.max_ex().max(1) as f64);
    let ex = p.ex as i64;
    Some(match id {
        108 | 128 | 153 => mine as i64 - ex,
        120 => lr2_score(sc),
        121 | 151 => ex,
        122 | 135 => rate.0,
        123 | 136 => rate.1,
        125 => sc.max_combo as i64,
        126 => sc.total_notes as i64,
        127 => p.gauge_value as i64,
        130 => sc.count(Judge::PGreat) as i64,
        131 => sc.count(Judge::Great) as i64,
        132 => sc.count(Judge::Good) as i64,
        133 => sc.count(Judge::Bad) as i64,
        134 => (sc.count(Judge::Poor) + sc.count(Judge::Miss)) as i64,
        // vibeBMS: its miss count and clear type
        9376 => sc.miss_count() as i64,
        9370 => p.clear as i64,
        _ => return None,
    })
}

impl SkinState for ResultState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        let s = self.s;
        match id {
            150 => Some(0),
            3 => (s.d.clear == ClearType::Failed).then_some(0),
            151 => Some(GRAPH_MS),
            152 => (s.d.score.ex_score() > self.best_ex() && !s.d.autoplay).then_some(GRAPH_MS),
            _ => s.host.timer(id),
        }
    }

    fn button(&self, id: i32) -> Option<i64> {
        match id {
            // the gauge played
            40 => Some(gauge_index(self.s.d.gauge_type)),
            _ => config_button(self.ctx, id).or_else(|| self.number(id)),
        }
    }

    fn option(&self, id: i32) -> bool {
        let v = self.static_option(id.abs()).unwrap_or(false);
        v != (id < 0)
    }

    fn number(&self, id: i32) -> Option<i64> {
        if let Some(v) = clock_number(id).or_else(|| player_number(self.ctx, id)) {
            return Some(v);
        }
        let d = &self.s.d;
        if let Some(v) =
            d.p2.as_ref()
                .and_then(|p| p2_number(d.score.ex_score(), p, id))
        {
            return Some(v);
        }
        let sc = &d.score;
        let before = self.s.before.as_ref();
        // a play not recorded: the chart's record as it stands
        let after = self.s.after.as_ref().or(before);
        let ex = sc.ex_score() as i64;
        let best = self.best_ex() as i64;
        let target = || {
            let song = &self.s.d.request.song;
            crate::scene::target::score(self.ctx, song, sc.max_ex(), self.best_ex()) as i64
        };
        let (avg, dev) = self.s.timing_stats;
        Some(match id {
            20 => self.ctx.view.fps as i64,
            71
            | 72
            | 74
            | 75
            | 101
            | 105
            | 106
            | 110..=113
            | 115
            | 116
            | 155
            | 156
            | 171
            | 174
            | 410..=427 => return score_number(sc, id),
            76 | 177 => sc.miss_count() as i64,
            77..=79 => return best_number(after, id),
            122 | 123 | 150 | 157 | 158 | 170 | 173 | 176 | 183 | 184 | 371 | BEST_MIN_CB => {
                return best_number(before, id)
            }
            90..=92 | 96 | 400 => return chart_number(&self.chart, id),
            102 => percent_parts(sc.rate()).0,
            103 => percent_parts(sc.rate()).1,
            107 => d.gauge_value as i64,
            407 => ((d.gauge_value * 10.0) as i64) % 10,
            114 => sc.count(Judge::Poor) as i64,
            121 | 151 => target(),
            108 | 152 | 172 => ex - best,
            128 | 153 => ex - target(),
            // EX score minus the next DJ level's (or the maximum's)
            154 => {
                let max = sc.max_ex() as i64;
                rank_borders(max)
                    .chain([max])
                    .find(|t| *t > ex)
                    .map_or(0, |t| ex - t)
            }
            175 => sc.max_combo as i64 - best_number(before, 173)?,
            178 => sc.miss_count() as i64 - best_number(before, 176)?,
            312 => return speed_number(&self.ctx.config.play, &Bpms::of_meta(&d.meta), id),
            350..=368 => return song_number(&d.request.song, id),
            370 => d.clear as i64,
            374 => decimals2(avg).0,
            375 => decimals2(avg).1,
            376 => decimals2(dev).0,
            377 => decimals2(dev).1,
            _ => return None,
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let d = &self.s.d;
        let song = &d.request.song;
        Some(match id {
            TEXT_OPPONENT => d.p2.as_ref()?.name.clone(),
            2 => self.ctx.config.player_name.clone(),
            1 => crate::scene::target::name(self.ctx).1?,
            3 => match &d.p2 {
                Some(o) => o.name.clone(),
                None => crate::scene::target::name(self.ctx).0,
            },
            1001..=1003 => {
                let t = self
                    .table
                    .get_or_init(|| table_of(self.ctx, &song.md5, &song.sha256));
                return table_text(t.as_ref()?, id);
            }
            1010 => format!("vibeBMS {}", env!("CARGO_PKG_VERSION")),
            _ => return chart_text(&ChartText::of_meta(&d.meta, song), id),
        })
    }

    fn rate(&self, id: i32) -> f32 {
        let sc = &self.s.d.score;
        let n = sc.total_notes.max(1) as f32;
        let best = self.s.before.as_ref().map(BestScore::rate).unwrap_or(0.0) as f32;
        match id {
            110 | 111 | 147 => sc.rate() as f32,
            112..=115 => best,
            140..=144 => sc.count(Judge::ALL[(id - 140) as usize]) as f32 / n,
            145 => sc.max_combo as f32 / n,
            _ => 0.0,
        }
    }
}

pub struct ResultHooks<'a> {
    pub st: &'a ResultState<'a>,
}

impl Hooks for ResultHooks<'_> {
    fn ref_image(&self, id: i32) -> Option<crate::gfx::TexId> {
        match id {
            100 | 101 => self.st.s.stage_tex.as_ref().map(|t| t.tex),
            _ => None,
        }
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
            reveal: (s.host.ms(self.st.now) as f32 / dur as f32).clamp(0.0, 1.0),
        };
        match graph {
            Graph::Gauge(g) => {
                let course = data.record.rules.gauge_set == vibebms_core::gauge::GaugePreset::Lr2
                    && data.course.is_some();
                graphs::gauge(d, &a, g, &data.gauge_log, data.gauge_type, course)
            }
            Graph::Notes(g) => graphs::notes(d, &a, g, &data.notes_graph[g.kind.min(2)], None),
            Graph::Bpm(g) => {
                graphs::bpm(d, &a, g, &data.bpm_log, data.length_us, data.meta.main_bpm)
            }
            Graph::Timing(g) => graphs::timing(d, &a, g, &data.timing_log, &data.windows),
            Graph::Visualizer(_) => {}
        }
    }
}
