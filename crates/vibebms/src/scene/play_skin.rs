//! Drawing the play scene with an LR2 skin.

use super::play::PlayScene;
use super::skinvals::*;
use crate::app::{Ctx, Draw};
use crate::config::{HiSpeedMode, PlayConfig};
use crate::gfx::{Gfx, Rect, TexId};
use crate::platform::Instant;
use crate::skin::render::{Hooks, LoadedSkin, Xf};
use crate::skin::{SkinState, SkinType, OFFSET_HIDDEN_COVER, OFFSET_LANECOVER, OFFSET_LIFT};
use std::borrow::Cow;
use std::cell::OnceCell;
use std::path::PathBuf;
use vibebms_core::chart::NoteKind;
use vibebms_core::judge::*;
use vibebms_core::PlayMode;

pub fn skin_type(mode: PlayMode) -> SkinType {
    match mode {
        PlayMode::Beat5K => SkinType::Play5,
        PlayMode::Beat7K => SkinType::Play7,
        PlayMode::Beat10K => SkinType::Play10,
        PlayMode::Beat14K => SkinType::Play14,
        PlayMode::Popn9K => SkinType::Play9,
    }
}

/// LR2 timer lane index (0 = scratch, 1.. = keys, +10 for 2P) -> our lane.
fn lane_from_lr2(mode: PlayMode, idx: i32) -> Option<usize> {
    let (p, k) = ((idx / 10) as usize, (idx % 10) as usize);
    if p >= mode.players() {
        return None;
    }
    if k == 0 {
        mode.scratch_lane(p)
    } else {
        (k - 1 < mode.keys_per_player()).then(|| p * mode.lanes_per_player() + k - 1)
    }
}

/// The built-in default skin for a mode, side and screen orientation
/// (`touch`: the landscape variant above on-screen controls).
pub fn default_skin(mode: PlayMode, p2: bool, portrait: bool, touch: bool) -> PathBuf {
    let variant = if portrait {
        "_portrait"
    } else if touch {
        "_touch"
    } else {
        ""
    };
    let name = match mode {
        // double play has one landscape layout (portrait shows it too)
        PlayMode::Beat14K | PlayMode::Beat10K => format!(
            "play{}{}",
            mode.keys_per_player() * 2,
            if touch && !portrait { "_touch" } else { "" }
        ),
        // 9 buttons have no scratch, so one centred layout serves both sides
        m => format!(
            "play{}{variant}{}",
            m.keys_per_player(),
            if p2 && m.has_scratch() { "_2p" } else { "" }
        ),
    };
    PathBuf::from(crate::platform::vfs::BUILTIN)
        .join("skins/default")
        .join(name + ".json")
}

/// The built-in battle skin (both sides' gauges and scores) for a double
/// play mode.
pub fn battle_skin(mode: PlayMode, touch: bool) -> PathBuf {
    let name = format!(
        "play{}_battle{}.json",
        mode.keys_per_player() * 2,
        if touch { "_touch" } else { "" }
    );
    PathBuf::from(crate::platform::vfs::BUILTIN)
        .join("skins/default")
        .join(name)
}

/// The battle skin type of a double play mode.
pub fn battle_type(mode: PlayMode) -> Option<SkinType> {
    skin_type(mode).battle_pair()
}

/// `config.skins` value of the built-in touch perspective skin.
pub const PERSPECTIVE: &str = "builtin:perspective";

/// The built-in touch perspective skin for a mode and orientation (the
/// play side is the game's: it draws the lanes).
pub fn perspective_skin(mode: PlayMode, portrait: bool) -> PathBuf {
    let keys = mode.keys_per_player() * mode.players();
    let name = format!(
        "persp{keys}{}.json",
        if portrait { "_portrait" } else { "" }
    );
    PathBuf::from(crate::platform::vfs::BUILTIN)
        .join("skins/default")
        .join(name)
}

/// Resolve a `config.skins` value: empty = the built-in default; anything
/// else is a skin file relative to the data root (or absolute).
pub fn configured_path(ctx: &Ctx, value: &str) -> Option<PathBuf> {
    // "classic" was the removed code-drawn view: use the default
    if value.is_empty() || value == "classic" || value == PERSPECTIVE {
        return None;
    }
    let p = PathBuf::from(value);
    Some(if p.is_absolute() {
        p
    } else {
        ctx.paths.root.join(p)
    })
}

/// The play skin's width / height as far as it is known without loading
/// it (the touch layout editor): a JSON skin's own (the default skins
/// are), else 16:9.
pub fn aspect(ctx: &Ctx, mode: PlayMode) -> f32 {
    let value = ctx
        .config
        .skins
        .get(mode.label())
        .cloned()
        .unwrap_or_default();
    // the perspective skin takes the screen's shape
    if value == PERSPECTIVE {
        return ctx.width / ctx.height.max(1.0);
    }
    let path = configured_path(ctx, &value).unwrap_or_else(|| {
        let p2 = ctx.config.play.side == crate::config::Side::P2;
        default_skin(mode, p2, ctx.portrait(), true)
    });
    let json = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    json.then(|| crate::skin::beatoraja::size(&path))
        .flatten()
        .map_or(16.0 / 9.0, |(w, h)| w / h.max(1.0))
}

/// Load the configured skin for this chart's mode, falling back to the
/// built-in default when it is missing, broken or of another type.
pub fn load_play_skin(ctx: &Ctx, gfx: &mut Gfx, scene: &PlayScene) -> Option<LoadedSkin> {
    let mode = scene.skin_mode;
    // a battle with both sides shown: the battle skin slot (types 12 / 13,
    // double play skins too), else the built-in battle skin
    let battle = scene.opponent().is_some() && mode.is_double();
    let kind = match battle {
        true => battle_type(mode).unwrap_or(skin_type(mode)),
        false => skin_type(mode),
    };
    let default = if battle {
        battle_skin(mode, ctx.touch_enabled())
    } else {
        default_skin(
            mode,
            ctx.config.play.side == crate::config::Side::P2,
            ctx.height > ctx.width,
            ctx.touch_enabled(),
        )
    };
    let value = ctx
        .config
        .skins
        .get(kind.key().unwrap_or(mode.label()))
        .cloned()
        .unwrap_or_default();
    // (a battle's sides play on a flat skin)
    let flat = scene.input_mode != scene.mode;
    let configured = if value == PERSPECTIVE && flat {
        None
    } else if value == PERSPECTIVE {
        Some(perspective_skin(mode, ctx.portrait()))
    } else {
        configured_path(ctx, &value)
    };
    let st = SceneState::new(scene, ctx, Instant::now());
    let stat = |id: i32| st.static_option(id);
    for path in configured.into_iter().chain(std::iter::once(default)) {
        match crate::skin::load_file(
            &path,
            &ctx.paths.skins,
            &ctx.skin_user_settings(&value, &path),
            &stat,
            Some(&st),
            &crate::skin::image_size,
        ) {
            Ok(mut skin)
                if skin.kind == kind || (battle && skin.kind.battle_pair() == Some(kind)) =>
            {
                // perspective skins fill the screen: widened or made taller
                if skin.perspective {
                    let aspect = ctx.width / ctx.height.max(1.0);
                    let (w, h) = crate::perspective::reflowed_size(skin.width, skin.height, aspect);
                    skin.reflow(w, h);
                }
                crate::skin::lr2::keep_images(&skin);
                return Some(LoadedSkin::load(skin, gfx));
            }
            Ok(_) => log::warn!("{}: not a {} skin", path.display(), mode.label()),
            Err(e) => log::warn!("skin: {e}"),
        }
    }
    None
}

/// The pacemakers' EX scores: over the chart, and at the notes judged so
/// far.
#[derive(Clone, Copy, Default)]
struct Pace {
    best: i64,
    target: i64,
    best_now: i64,
    target_now: i64,
}

/// The play screen's values in one frame: the chart's record, its facts
/// and the pacemakers are looked up once.
pub struct SceneState<'a> {
    pub s: &'a PlayScene,
    pub ctx: &'a Ctx,
    pub now: Instant,
    engine: Option<&'a vibebms_core::engine::Engine>,
    /// The chart's record before this play.
    best: Option<&'a crate::library::BestScore>,
    chart: ChartFacts<'a>,
    bpms: crate::config::Bpms,
    pace: OnceCell<Pace>,
    table: OnceCell<Option<(String, String)>>,
}

impl<'a> SceneState<'a> {
    pub fn new(s: &'a PlayScene, ctx: &'a Ctx, now: Instant) -> SceneState<'a> {
        SceneState {
            s,
            ctx,
            now,
            engine: s.engine(),
            best: ctx.best_key(&s.req.song.sha256, s.score_ln as i32),
            chart: ChartFacts::of_meta(&s.chart.meta, s.skin_mode),
            bpms: crate::config::Bpms::of_meta(&s.chart.meta),
            pace: OnceCell::new(),
            table: OnceCell::new(),
        }
    }

    fn pace(&self) -> Pace {
        *self.pace.get_or_init(|| self.make_pace())
    }

    /// The pacemakers: the personal best and the target EX scores, over
    /// the chart and at the notes judged so far. The best play's ghost (its
    /// replay's judgements in order) gives the best at the same note; else
    /// it, as the other targets, runs at the share of the notes judged.
    fn make_pace(&self) -> Pace {
        let s = self.s;
        let best = self.best.map_or(0, |b| b.ex_score) as i64;
        let Some(sc) = self.engine.map(|e| &e.score) else {
            return Pace {
                best,
                ..Pace::default()
            };
        };
        let done = sc.passed as f64 / sc.total_notes.max(1) as f64;
        let at = |v: i64| (v as f64 * done).round() as i64;
        let best_now = match &s.best_ghost {
            Some(g) => crate::replay::ex_of(&g[..(sc.passed as usize).min(g.len())]) as i64,
            None => at(best),
        };
        // a battle: the target is the opponent, as it plays
        if let Some(ex) = s.opponent_ex() {
            return Pace {
                best,
                target: ex,
                best_now,
                target_now: ex,
            };
        }
        let target = self.ctx.config.play.target.score(sc.max_ex(), best as u32) as i64;
        let target_now = if self.ctx.config.play.target == crate::config::Target::Best {
            best_now
        } else {
            at(target)
        };
        Pace {
            best,
            target,
            best_now,
            target_now,
        }
    }

    /// The table that lists the chart.
    fn table(&self) -> Option<&(String, String)> {
        let song = &self.s.req.song;
        self.table
            .get_or_init(|| table_of(self.ctx, &song.md5, &song.sha256))
            .as_ref()
    }

    /// A battle's opponent (the second side's judge).
    fn opp(&self) -> Option<&vibebms_core::engine::Engine> {
        self.s.opponent().map(|o| &o.engine)
    }

    /// The second side's values (LR2 2P numbers) when there is one.
    fn opponent_number(&self, id: i32) -> Option<i64> {
        let o = self.opp()?;
        let sc = &o.score;
        let ex = self.s.opponent_ex()?;
        let rate = || percent_parts(ex as f64 / sc.max_ex().max(1) as f64);
        Some(match id {
            // 1P - 2P
            108 => self.engine?.score.ex_score() as i64 - ex,
            120 => lr2_score(sc),
            122 => percent_parts(sc.current_rate()).0,
            123 => percent_parts(sc.current_rate()).1,
            124 => o.combo() as i64,
            125 => sc.max_combo as i64,
            126 => sc.total_notes as i64,
            127 => o.gauge.value() as i64,
            129 => diff_next_rank(sc)?,
            130 => sc.count(PGREAT) as i64,
            131 => sc.count(GREAT) as i64,
            132 => sc.count(GOOD) as i64,
            133 => sc.count(BAD) as i64,
            134 => (sc.count(POOR) + sc.count(MISS)) as i64,
            135 => rate().0,
            136 => rate().1,
            _ => return None,
        })
    }

    fn ms(&self, t: Instant) -> i64 {
        ms_since(self.s.scene_start, t)
    }

    /// Options that do not change during play (also used at load time).
    fn static_option(&self, id: i32) -> Option<bool> {
        let s = self.s;
        let p = &self.ctx.config.play;
        let course = s.req.course.as_ref();
        Some(match id {
            30 | 34 | 39 | 50 => true,
            // (38: no score graph)
            31 | 35..=38 | 51 => false,
            32 | 33 | 40..=43 | 1046 => return play_option(s.req.autoplay, p.bga, p.gauge, id),
            // the gauge in use (vibeBMS ids, as on the result screen)
            118..=125 => {
                let g = self.engine.map_or(p.gauge, |e| e.gauge.current_type());
                gauge_type_option(g, id).unwrap_or(false)
            }
            // the song's lamp so far (skins pick images by it at load time)
            100..=105 | 1100..=1104 => {
                return lamp_option(self.best.map(|b| b.clear).unwrap_or_default(), id)
            }
            150..=195 | 1177 => return chart_option(&self.chart, id),
            // a difficulty table lists the chart
            1008 => self.table().is_some(),
            // course stage 1..4, the last stage, a grade course
            280..=283 => course.is_some_and(|c| c.index == (id - 280) as usize),
            289 => course.is_some_and(|c| c.index + 1 == c.songs.len()),
            293 => course.is_some_and(|c| !c.constraints.is_empty()),
            271 => p.lane_cover || p.sudden > 0.0,
            P2_COVER => {
                let p = self.side_play(1);
                p.lane_cover || p.sudden > 0.0
            }
            272 => p.lift > 0.0,
            273 => p.hidden > 0.0,
            290 => course.is_some(),
            400 => p.hispeed_mode == HiSpeedMode::Constant,
            _ => return None,
        })
    }

    /// A player's options (VS: the second player's own).
    fn side_play(&self, side: usize) -> Cow<'_, PlayConfig> {
        side_play(self.s, &self.ctx.config.play, side)
    }

    /// The hi-speed (the setting, or what keeps a fixed green number).
    fn hispeed(&self) -> f64 {
        self.ctx.config.play.hispeed_at(&self.bpms) as f64
    }
}

impl SkinState for SceneState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        let s = self.s;
        let mode = s.mode();
        let lane_t = |base: i32, v: &[Option<Instant>]| -> Option<i64> {
            let side = (id - base) / 10;
            let lane = lane_from_lr2(mode, (id - base) % 10 + side * 10)?;
            v.get(lane).copied().flatten().map(|t| self.ms(t))
        };
        match id {
            0 => Some(0),
            1 => Some(0),
            // the hi-speed panel (START / SELECT held) opened / closed
            21 => s.panel_timers().0.map(|t| self.ms(t)),
            31 => s.panel_timers().1.map(|t| self.ms(t)),
            40 => s.ready_at.map(|t| self.ms(t)),
            41 => s.ready_at.map(|_| self.ms(s.start)),
            // judge / combo 1P, 2P, 3P (the PMS middle region)
            46 | 47 | 446 | 447 => s.last_judge((id % 2 == 1) as usize).map(|j| self.ms(j.2)),
            247 | 448 => s.last_judge(2).map(|j| self.ms(j.2)),
            2 | 3 => s.ending_at().map(|t| self.ms(t)),
            44 => self
                .engine
                .filter(|e| e.gauge.current().is_max())
                .map(|_| 0),
            // a battle's second side: its gauge full, its notes ended (with a
            // full combo)
            45 => s
                .opponent()
                .filter(|o| o.engine.gauge.current().is_max())
                .map(|_| 0),
            // a side's gauge rose
            42 => s.sides.first()?.gauge_up.map(|t| self.ms(t)),
            43 => s.opponent()?.gauge_up.map(|t| self.ms(t)),
            143 => s.finished_instant.map(|t| self.ms(t)),
            144 => s.opponent()?.finished.map(|t| self.ms(t)),
            // the last note judged with no combo broken
            48 => {
                let full = full_combo(&self.engine?.score);
                s.finished_instant.filter(|_| full).map(|t| self.ms(t))
            }
            49 => {
                let o = s.opponent()?;
                o.finished
                    .filter(|_| full_combo(&o.engine.score))
                    .map(|t| self.ms(t))
            }
            50..=69 => lane_t(50, &s.bomb_at),
            70..=89 => {
                let lane = lane_from_lr2(mode, id - 70)?;
                s.engine_for(lane)?.holding(lane).map(|_| 0)
            }
            100..=119 => {
                let v = lane_t(100, &s.key_flash)?;
                let off = s
                    .key_off
                    .get(lane_from_lr2(mode, id - 100)?)
                    .copied()
                    .flatten()
                    .map(|t| self.ms(t));
                (off.map(|o| o < v).unwrap_or(true)).then_some(v)
            }
            120..=139 => {
                let lane = lane_from_lr2(mode, id - 120)?;
                let off = s.key_off.get(lane).copied().flatten()?;
                let on = s.key_flash.get(lane).copied().flatten();
                (on.map(|o| o <= off).unwrap_or(true)).then(|| self.ms(off))
            }
            140 => {
                let t = s.chart_us(self.now) as f64;
                let tm = &s.chart.timing;
                let beat_t = tm.time_at_beat(tm.beat_at_time(t).floor());
                Some(self.ms(s.start) + beat_t / 1000)
            }
            _ => None,
        }
    }

    fn button(&self, id: i32) -> Option<i64> {
        let s = self.s;
        match id {
            // last judgement of a lane / of a side
            500..=519 => {
                let lane = lane_from_lr2(s.mode(), id - 500)?;
                s.lane_judge.get(lane).copied().flatten().map(|j| j as i64)
            }
            520 | 521 => s.last_judge((id - 520) as usize).map(|j| j.0 as i64),
            40 => self.engine.map(|e| gauge_index(e.gauge.current_type())),
            _ => config_button(self.ctx, id).or_else(|| self.number(id)),
        }
    }

    fn offset(&self, id: i32) -> Option<[f32; 6]> {
        // (the second player's own cover in VS)
        let two = id == P2_COVER_OFFSET;
        let p = &self.side_play(two as usize);
        let id = if two { OFFSET_LANECOVER } else { id };
        // lane height in skin pixels (the first lane of the note field)
        let lane_h = self
            .s
            .skin
            .as_ref()?
            .skin
            .lanes
            .iter()
            .find_map(|l| l.rect)
            .map(|r| r.h)?;
        let span = lane_h * (1.0 - p.lift);
        Some(match id {
            OFFSET_LIFT => [0.0, p.lift * lane_h, 0.0, 0.0, 0.0, 0.0],
            OFFSET_LANECOVER if p.lane_cover || p.sudden > 0.0 => {
                [0.0, -p.sudden * span, 0.0, 0.0, 0.0, 0.0]
            }
            OFFSET_LANECOVER => [0.0; 6],
            OFFSET_HIDDEN_COVER if p.hidden > 0.0 => [0.0, p.hidden * span, 0.0, 0.0, 0.0, 0.0],
            OFFSET_HIDDEN_COVER => [0.0, 0.0, 0.0, 0.0, 0.0, -255.0],
            _ => return None,
        })
    }

    fn option(&self, id: i32) -> bool {
        let (abs, neg) = (id.abs(), id < 0);
        let v = self.static_option(abs).unwrap_or_else(|| {
            let Some(e) = self.engine else { return false };
            let sc = &e.score;
            let lj = self.s.last_judge[0];
            match abs {
                80 => self.s.engine().is_none(),
                81 => self.s.engine().is_some(),
                200..=207 => {
                    vibebms_core::score::DjLevel::from_rate(sc.ex_score(), sc.passed * 2) as i32
                        == 7 - (abs - 200)
                }
                230..=240 => {
                    let g = e.gauge.value();
                    let b = ((g / 10.0).floor() as i32).min(10);
                    b == abs - 230
                }
                1240 => e.gauge.current().is_qualified(),
                // the lane cover / hi-speed is being changed
                270 => self.s.panel_open(),
                // a judgement of the kind has happened
                2241..=2246 => sc.count((abs - 2241) as usize) > 0,
                // the second side's gauge (groove / survival), AUTO
                // SCRATCH and DJ level; the first side winning, losing,
                // level with the second
                44 | 45 => {
                    let g = self.s.side(1).map(|o| o.engine.gauge.current_type());
                    g.is_some_and(|g| g.is_survival() == (abs == 45))
                }
                56 | 57 => {
                    let o = self.s.side(1).map(|o| &o.engine);
                    o.is_some_and(|o| {
                        let lane = self.s.mode().scratch_lane(1).unwrap_or(0);
                        o.is_auto(lane) == (abs == 57)
                    })
                }
                210..=217 => self.opp().is_some_and(|o| {
                    let sc = &o.score;
                    vibebms_core::score::DjLevel::from_rate(sc.ex_score(), sc.passed * 2) as i32
                        == 7 - (abs - 210)
                }),
                352..=354 => match self.s.opponent_ex() {
                    Some(theirs) => {
                        let mine = sc.ex_score() as i64;
                        match abs {
                            352 => mine > theirs,
                            353 => mine < theirs,
                            _ => mine == theirs,
                        }
                    }
                    None => false,
                },
                241..=246 => lj.map(|j| j.0 == (abs - 241) as usize).unwrap_or(false),
                1242 => lj.map(|j| j.1 > 0).unwrap_or(false),
                1243 => lj.map(|j| j.1 < 0).unwrap_or(false),
                _ => false,
            }
        });
        v != neg
    }

    fn number(&self, id: i32) -> Option<i64> {
        if let Some(v) = clock_number(id).or_else(|| player_number(self.ctx, id)) {
            return Some(v);
        }
        if let Some(v) = self.opponent_number(id) {
            return Some(v);
        }
        let s = self.s;
        let p = &self.ctx.config.play;
        let e = self.engine;
        let sc = e.map(|e| &e.score);
        let t_ms = s.chart_us(self.now).max(0) / 1000;
        // (the chart's length goes over every note)
        let len_ms = || s.chart.length() / 1000;
        // BAD, POOR and MISS so far (the result counts the notes never
        // reached too)
        let bp = || sc.map(|s| (s.count(BAD) + s.count(POOR) + s.count(MISS)) as i64);
        // a share of the maximum EX score, as a percentage
        let of_max = |v: i64| sc.map(|s| percent_parts(v as f64 / s.max_ex().max(1) as f64));
        Some(match id {
            10 => (self.hispeed() * 100.0).round() as i64,
            310 => (self.hispeed() * 100.0).round() as i64 / 100,
            311 => (self.hispeed() * 100.0).round() as i64 % 100,
            12 => p.judge_offset_ms as i64,
            14 => (p.sudden * 1000.0) as i64,
            P2_WHITE => (self.side_play(1).sudden * 1000.0) as i64,
            P2_GREEN => self.side_play(1).green(&self.bpms),
            312 | 313 => return speed_number(p, &self.bpms, id),
            // duration / green number with the lane cover on / off
            1312..=1315 => {
                let d = p.duration_ms_with(&self.bpms, id < 1314);
                (if id % 2 == 0 { d } else { d * 0.6 }).round() as i64
            }
            314 => (p.lift * 1000.0) as i64,
            315 => (p.hidden * 1000.0) as i64,
            20 => self.ctx.fps as i64,
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
            | 410..=420
            | 423..=427 => return score_number(sc?, id),
            76 | 177 => bp()?,
            90..=92 | 96 | 400 => return chart_number(&self.chart, id),
            100 => lr2_score(sc?),
            102 => percent_parts(sc?.current_rate()).0,
            103 => percent_parts(sc?.current_rate()).1,
            104 => e?.combo() as i64,
            107 => e?.gauge.value() as i64,
            407 => ((e?.gauge.value() * 10.0) as i64) % 10,
            114 => sc?.count(POOR) as i64 + sc?.count(MISS) as i64,
            // the personal best's EX score, rate, max combo and miss count,
            // and the differences to them
            150 | 170 | 173 | 176 => return best_number(self.best, id),
            183 => of_max(self.pace().best)?.0,
            184 => of_max(self.pace().best)?.1,
            175 => sc?.max_combo as i64 - best_number(self.best, 173)?,
            178 => bp()? - best_number(self.best, 176)?,
            368 => s.total as i64,
            350..=367 => return song_number(&s.req.song, id),
            121 | 151 => self.pace().target,
            // differences to the pacemakers (best / target at this point)
            152 | 172 => sc?.ex_score() as i64 - self.pace().best_now,
            128 | 153 => sc?.ex_score() as i64 - self.pace().target_now,
            154 => diff_next_rank(sc?)?,
            122 | 135 | 157 => of_max(self.pace().target)?.0,
            123 | 136 | 158 => of_max(self.pace().target)?.1,
            // timing of the last judgement (ms, + = early as in beatoraja)
            525 | 526 => s.last_judge((id - 525) as usize)?.1 / 1000,
            160 => s
                .chart
                .timing
                .bpm_at_time(s.chart_us(self.now).max(0) as f64)
                .round() as i64,
            161 => t_ms / 60000,
            162 => (t_ms / 1000) % 60,
            163 => (len_ms() - t_ms).max(0) / 60000,
            164 => ((len_ms() - t_ms).max(0) / 1000) % 60,
            1163 => len_ms() / 60000,
            1164 => (len_ms() / 1000) % 60,
            165 => 100,
            _ => return None,
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let s = self.s;
        Some(match id {
            2 => self.ctx.config.player_name.clone(),
            1001..=1003 => return table_text(self.table()?, id),
            // course stages' titles
            150..=159 => s
                .req
                .course
                .as_ref()?
                .songs
                .get((id - 150) as usize)?
                .full_title(),
            _ => return chart_text(&ChartText::of_meta(&s.chart.meta, &s.req.song), id),
        })
    }

    fn rate(&self, id: i32) -> f32 {
        let s = self.s;
        let sc = self.engine.map(|e| &e.score);
        match id {
            6 | 101 => {
                (s.chart_us(self.now).max(0) as f32 / s.chart.length().max(1) as f32).min(1.0)
            }
            4 => self.ctx.config.play.sudden,
            P2_SUDDEN => self.side_play(1).sudden,
            // vibeBMS: hi-speed 0.5 .. 10.0 as 0 .. 1 (the hi-speed panel)
            HISPEED_RATE => ((self.hispeed() as f32 - 0.5) / 9.5).clamp(0.0, 1.0),
            102 => 1.0,
            110 => sc.map(|s| s.current_rate() as f32).unwrap_or(0.0),
            111 => sc.map(|s| s.rate() as f32).unwrap_or(0.0),
            // best (112, 113) and target (114, 115) so far / overall, as a
            // share of the maximum
            112..=115 => {
                let max = sc.map(|s| s.max_ex()).unwrap_or(0).max(1) as f32;
                let p = self.pace();
                (match id {
                    112 => p.best_now,
                    113 => p.best,
                    114 => p.target_now,
                    _ => p.target,
                }) as f32
                    / max
            }
            _ => 0.0,
        }
    }
}

/// A player's options (VS: the second player's own).
fn side_play<'a>(s: &PlayScene, base: &'a PlayConfig, side: usize) -> Cow<'a, PlayConfig> {
    match s.vs {
        true => Cow::Owned(base.for_side(side)),
        false => Cow::Borrowed(base),
    }
}

/// EX score minus the next DJ level's border at the notes judged so far.
fn diff_next_rank(sc: &vibebms_core::score::Score) -> Option<i64> {
    let ex = sc.ex_score() as i64;
    rank_borders(sc.passed as i64 * 2)
        .find(|t| *t > ex)
        .map(|t| ex - t)
}

/// Whether a full combo stands (no BAD or worse, no combo broken).
fn full_combo(sc: &vibebms_core::score::Score) -> bool {
    sc.combo_breaks == 0 && (3..=5).all(|j| sc.count(j) == 0)
}

/// vibeBMS ids of the second player's own lane cover in VS (the first
/// player's: 4, 271, 14, 313 and offset 4): its SUDDEN+, shown, white and
/// green numbers, its cover offset.
pub const P2_SUDDEN: i32 = 9404;
pub const P2_COVER: i32 = 9271;
pub const P2_WHITE: i32 = 9014;
pub const P2_GREEN: i32 = 9313;
pub const P2_COVER_OFFSET: i32 = 9004;

/// Slider / graph value of the hi-speed position (vibeBMS).
pub const HISPEED_RATE: i32 = 9400;

pub struct SceneHooks<'a> {
    pub st: &'a SceneState<'a>,
}

impl Hooks for SceneHooks<'_> {
    fn graph(&mut self, d: &mut Draw, graph: &crate::skin::Graph, r: Rect, color: [f32; 4]) {
        use crate::skin::Graph;
        let s = self.st.s;
        let a = super::graphs::Area {
            r,
            color,
            reveal: 1.0,
        };
        match graph {
            Graph::Notes(g) => {
                let Some(e) = s.engine() else {
                    return;
                };
                let cols = super::graphs::columns(e.notes(), s.mode, s.chart.length());
                // play position, once the chart runs
                let cursor = s
                    .ready_at
                    .map(|_| s.chart_us(self.st.now).max(0) as f64 / 1000.0);
                super::graphs::notes(d, &a, g, &cols[g.kind.min(2)], cursor);
            }
            Graph::Bpm(g) => {
                let log: Vec<(i64, f64)> = s
                    .chart
                    .timing
                    .bpm_changes()
                    .into_iter()
                    .map(|c| (c.time, c.bpm))
                    .collect();
                super::graphs::bpm(d, &a, g, &log, s.chart.length(), s.chart.meta.main_bpm);
            }
            Graph::Visualizer(g) => {
                let Some(e) = s.engine() else {
                    return;
                };
                let recent: Vec<i64> = e.timing_log.iter().map(|t| t.1).collect();
                super::graphs::visualizer(d, &a, g, &recent, &e.windows().note);
            }
            Graph::Gauge(_) | Graph::Timing(_) => {}
        }
    }

    fn notes(&mut self, d: &mut Draw, skin: &LoadedSkin, xf: &Xf, now: i64, st: &dyn SkinState) {
        if skin.skin.perspective {
            return super::play_persp::draw(self.st, d, xf);
        }
        let s = self.st.s;
        if s.engine().is_none() {
            return;
        }
        let p = &self.st.ctx.config.play;
        // each player's hi-speed, covers and LIFT (VS: the second's own)
        let per = [0, 1].map(|k| side_play(s, p, k));
        let t_vis = s.chart_us(self.st.now) + p.visual_offset_ms as i64 * 1000;
        let cur_pos = s.chart.timing.pos_at_time(t_vis as f64);
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
        let bpms = crate::config::Bpms::of_meta(&s.chart.meta);
        let dur_of = |p: &PlayConfig| p.duration_at(&bpms, main_bpm, true) * 1000.0;
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
            let tm = &s.chart.timing;
            let changes = tm.bpm_changes();
            let bpms = changes.iter().skip(1).filter(|c| ahead.contains(&c.time));
            let stops = tm.stops.iter().filter(|st| ahead.contains(&st.time));
            bpms.map(|c| {
                let label = format!("BPM {}", c.bpm.round() as i64);
                (c.time, label, [0.25, 0.85, 0.3, 1.0])
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
            let p = &per[s.mode.player_of_lane(lane).min(1)];
            let dur_us = dur_of(p);
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
                    HiSpeedMode::Floating => judge_y - ((pos - cur_pos) * ppb) as f32,
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
            let Some(list) = s.lane_notes.get(lane) else {
                continue;
            };
            let cursor = s
                .lane_cursor
                .get(lane)
                .copied()
                .unwrap_or(0)
                .min(list.len());
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
                            !n.is_long()
                                && !n.is_mine()
                                && (n.state == 0 || n.state as usize > POOR)
                        }),
                );
            }
            for &ni in &list[cursor..] {
                let n = &notes[ni];
                let cn = &s.chart.notes[n.chart_index];
                let y = y_of(cn.pos, n.time);
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
                            && (n.state == 0 || n.state as usize > POOR)
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
                            let nr =
                                Rect::new(r.x - (w2 - r.w) * 0.5, y - h - (h2 - h) * 0.5, w2, h2);
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
                    .get(BAD)
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
                    size,
                    *c,
                    crate::gfx::text::Align::Center,
                );
            }
        }
        // bar lines span each lane group
        for (i, (src, dst)) in skin.skin.bar_lines.iter().enumerate() {
            let p = &per[i.min(1)];
            let dur_us = dur_of(p);
            if let Some(fr) = dst.eval(now, st) {
                let field = xf.rect(fr.rect);
                let judge_y = field.bottom() - p.lift * field.h;
                let top = field.y + p.sudden * field.h;
                let ppb =
                    (judge_y - top).max(1.0) as f64 / (dur_us / 1_000_000.0 * main_bpm / 60.0);
                for bl in &s.chart.bar_lines {
                    let y = judge_y - ((bl.pos - cur_pos) * ppb) as f32;
                    if y > judge_y || y < top {
                        continue;
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

    fn bga(&mut self, d: &mut Draw, rect: Rect) {
        let s = self.st.s;
        d.b.rect(rect, [0.0, 0.0, 0.0, 1.0]);
        if !self.st.ctx.config.play.bga {
            return;
        }
        let layers = s.bga_textures(self.st.now);
        if layers.is_empty() && !s.chart.meta.has_bga {
            // charts without BGA show their stage file instead
            if let Some(t) = s.stage_tex {
                d.image_fit(t, rect, 0.8);
            }
        }
        for t in layers {
            d.image_fit(t, rect, 1.0);
        }
    }

    fn ref_image(&self, id: i32) -> Option<TexId> {
        match id {
            100 | 101 => self.st.s.stage_tex,
            _ => None,
        }
    }

    fn gauge(&self, side: usize) -> (f32, f32, bool, usize) {
        match self.st.s.side(side).map(|s| &s.engine) {
            Some(e) => {
                let g = e.gauge.current();
                (g.value(), g.elem.border, g.ty.is_survival(), g.ty as usize)
            }
            None => (0.0, 80.0, false, 2),
        }
    }

    fn judge(&self, side: usize) -> Option<(usize, u32)> {
        let s = self.st.s;
        s.last_judge(side).map(|j| (j.0, j.3))
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
