//! The play skin: loading it for the chart, the values it shows in a
//! frame, the parts the scene draws for it.

use super::PlayScene;
use crate::app::{Ctx, Draw};
use crate::config::{HiSpeedMode, PlayConfig};
use crate::gfx::{Gfx, Rect, TexId};
use crate::platform::Instant;
use crate::scene::graphs::{self, Column};
use crate::scene::play_skin::{
    battle_skin, battle_type, configured_path, default_skin, perspective_skin, skin_type,
    PERSPECTIVE,
};
use crate::scene::skinvals::*;
use crate::skin::render::{Hooks, LoadedSkin, Xf};
use crate::skin::{SkinState, OFFSET_HIDDEN_COVER, OFFSET_LANECOVER, OFFSET_LIFT};
use std::borrow::Cow;
use std::cell::OnceCell;
use vibebms_core::course::CourseKind;
use vibebms_core::engine::Engine;
use vibebms_core::judge::Judge;
use vibebms_core::score::Score;
use vibebms_core::PlayMode;

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

/// Load the configured skin for this chart's mode, falling back to the
/// built-in default when it is missing, broken or of another type.
pub(super) fn load(ctx: &Ctx, gfx: &mut Gfx, scene: &PlayScene) -> Option<LoadedSkin> {
    let mode = scene.facts.skin_mode;
    // a battle with both sides shown: the battle skin slot (types 12 / 13,
    // double play skins too), else the built-in battle skin
    let battle = scene.session.opponent().is_some() && mode.is_double();
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
            ctx.view.height > ctx.view.width,
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
    let flat = scene.facts.input_mode != scene.session.mode;
    let configured = if value == PERSPECTIVE && flat {
        None
    } else if value == PERSPECTIVE {
        Some(perspective_skin(mode, ctx.portrait()))
    } else {
        configured_path(ctx, &value)
    };
    let st = PlayState::new(scene, ctx, Instant::now());
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
                    let aspect = ctx.view.width / ctx.view.height.max(1.0);
                    let (w, h) = crate::perspective::reflowed_size(skin.width, skin.height, aspect);
                    skin.reflow(w, h);
                }
                crate::skin::lr2::keep_images(&skin);
                // the chart's lead-in starts now: the skin must be there
                let mut sk = LoadedSkin::load(skin, gfx);
                sk.wait(gfx);
                return Some(sk);
            }
            Ok(_) => log::warn!("{}: not a {} skin", path.display(), mode.label()),
            Err(e) => log::warn!("skin: {e}"),
        }
    }
    None
}

/// The EX score after each judgement of a play's ghost (its replay's
/// judgements in order), from none: MY BEST's pacemaker at the same note.
pub(crate) fn ghost_ex(ghost: &[Judge]) -> Vec<u32> {
    let mut ex = 0;
    std::iter::once(0)
        .chain(ghost.iter().map(|j| {
            ex += j.ex();
            ex
        }))
        .collect()
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

/// The play screen's values in one frame: the chart's record, its facts,
/// each player's options and the pacemakers are looked up once.
pub struct PlayState<'a> {
    pub s: &'a PlayScene,
    pub ctx: &'a Ctx,
    pub now: Instant,
    engine: Option<&'a Engine>,
    /// The chart's record before this play.
    best: Option<&'a crate::library::BestScore>,
    chart: ChartFacts<'a>,
    bpms: crate::config::Bpms,
    /// Each player's options (VS: the second player's own).
    play: [Cow<'a, PlayConfig>; 2],
    /// The second side's EX score.
    opp_ex: Option<i64>,
    /// The height of the note field's lanes (skin pixels).
    lane_h: Option<f32>,
    pace: OnceCell<Pace>,
    table: OnceCell<Option<(String, String)>>,
}

impl<'a> PlayState<'a> {
    pub fn new(s: &'a PlayScene, ctx: &'a Ctx, now: Instant) -> PlayState<'a> {
        let p = &ctx.config.play;
        PlayState {
            s,
            ctx,
            now,
            engine: s.session.engine(),
            best: ctx.best_key(
                &s.req.record_key(),
                s.facts.score_ln as i32,
                s.facts.rules.judge,
            ),
            chart: ChartFacts::of_meta(&s.session.chart.meta, s.facts.skin_mode),
            bpms: s.bpms_at(now),
            play: [s.side_config(p, 0), s.side_config(p, 1)],
            opp_ex: s.session.opponent_ex(),
            lane_h: s
                .skin
                .as_ref()
                .and_then(|sk| sk.skin.lanes.iter().find_map(|l| l.rect))
                .map(|r| r.h),
            pace: OnceCell::new(),
            table: OnceCell::new(),
        }
    }

    /// A player's options (VS: the second player's own).
    pub fn side_play(&self, side: usize) -> &PlayConfig {
        &self.play[side.min(1)]
    }

    /// The last judgement of a judge region as the skin shows it: a
    /// single play skin over a battle (a phone held upright) has the
    /// player's alone, never the opponent's.
    pub fn last_judge(&self, region: usize) -> Option<super::session::LastJudge> {
        let s = self.s;
        if region > 0 && s.facts.skin_mode.players() < s.session.mode.players() {
            return None;
        }
        s.session.last_judge[region.min(2)]
    }

    fn pace(&self) -> Pace {
        *self.pace.get_or_init(|| self.make_pace())
    }

    /// The pacemakers: the personal best and the target EX scores, over
    /// the chart and at the notes judged so far. The best play's ghost (its
    /// replay's judgements in order) gives the best at the same note; else
    /// it, as the other targets, runs at the share of the notes judged.
    fn make_pace(&self) -> Pace {
        let best = self.best.map_or(0, |b| b.ex_score) as i64;
        let Some(sc) = self.engine.map(|e| &e.score) else {
            return Pace {
                best,
                ..Pace::default()
            };
        };
        let done = sc.passed as f64 / sc.total_notes.max(1) as f64;
        let at = |v: i64| (v as f64 * done).round() as i64;
        let best_now = match &self.s.facts.best_ex {
            Some(ex) => ex[(sc.passed as usize).min(ex.len() - 1)] as i64,
            None => at(best),
        };
        // a battle: the target is the opponent, as it plays
        if let Some(ex) = self.opp_ex {
            return Pace {
                best,
                target: ex,
                best_now,
                target_now: ex,
            };
        }
        let song = &self.s.req.song;
        let target = crate::scene::target::score(
            self.ctx,
            &song.md5,
            &song.sha256,
            sc.max_ex(),
            best as u32,
        ) as i64;
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
    fn opp(&self) -> Option<&Engine> {
        self.s.session.opponent().map(|o| &o.engine)
    }

    /// The second side's values (LR2 2P numbers) when there is one.
    fn opponent_number(&self, id: i32) -> Option<i64> {
        let o = self.opp()?;
        let sc = &o.score;
        let ex = self.opp_ex?;
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
            130 => sc.count(Judge::PGreat) as i64,
            131 => sc.count(Judge::Great) as i64,
            132 => sc.count(Judge::Good) as i64,
            133 => sc.count(Judge::Bad) as i64,
            134 => (sc.count(Judge::Poor) + sc.count(Judge::Miss)) as i64,
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
            30 | 31 | 34..=39 => return display_option(p, id),
            50 => true,
            51 => false,
            32 | 33 | 40..=43 | 1046 => {
                return play_option(s.req.autoplay, s.bga_shown(self.ctx), p.gauge, id)
            }
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
            280..=283 => s.course_stage(self.now) == Some((id - 280) as usize),
            289 => course.is_some_and(|c| s.course_stage(self.now) == Some(c.songs.len() - 1)),
            293 => course.is_some_and(|c| c.kind() == CourseKind::Class),
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

    /// The hi-speed (the setting, or what keeps a fixed green number).
    fn hispeed(&self) -> f64 {
        self.ctx.config.play.hispeed_shown(&self.bpms) as f64
    }
}

impl SkinState for PlayState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        let s = self.s;
        let ses = &s.session;
        let mode = ses.mode;
        let lane_t = |base: i32, t: fn(&super::session::Lane) -> Option<Instant>| {
            let lane = lane_from_lr2(mode, id - base)?;
            ses.lanes.get(lane).and_then(t).map(|t| self.ms(t))
        };
        let panel = &s.controls.panel[0];
        match id {
            0 => Some(0),
            1 => Some(0),
            // the hi-speed panel (START / SELECT held) opened / closed
            21 => panel.opened.map(|t| self.ms(t)),
            31 => panel.closed.map(|t| self.ms(t)),
            40 => ses.ready_at.map(|t| self.ms(t)),
            41 => ses.ready_at.map(|_| self.ms(ses.start)),
            // judge / combo 1P, 2P, 3P (the PMS middle region)
            46 | 47 | 446 | 447 => self
                .last_judge((id % 2 == 1) as usize)
                .map(|j| self.ms(j.at)),
            247 | 448 => self.last_judge(2).map(|j| self.ms(j.at)),
            2 | 3 => s.ending_at().map(|t| self.ms(t)),
            44 => self
                .engine
                .filter(|e| e.gauge.current().is_max())
                .map(|_| 0),
            // a battle's second side: its gauge full, its notes ended (with a
            // full combo)
            45 => ses
                .opponent()
                .filter(|o| o.engine.gauge.current().is_max())
                .map(|_| 0),
            // a side's gauge rose
            42 => ses.sides.first()?.gauge_up.map(|t| self.ms(t)),
            43 => ses.opponent()?.gauge_up.map(|t| self.ms(t)),
            143 => ses.finished_instant.map(|t| self.ms(t)),
            144 => ses.opponent()?.finished.map(|t| self.ms(t)),
            // the last note judged with no combo broken
            48 => {
                let full = full_combo(&self.engine?.score);
                ses.finished_instant.filter(|_| full).map(|t| self.ms(t))
            }
            49 => {
                let o = ses.opponent()?;
                o.finished
                    .filter(|_| full_combo(&o.engine.score))
                    .map(|t| self.ms(t))
            }
            50..=69 => lane_t(50, |l| l.bomb),
            70..=89 => {
                let lane = lane_from_lr2(mode, id - 70)?;
                ses.engine_for(lane)?.holding(lane).map(|_| 0)
            }
            100..=119 => {
                let v = lane_t(100, |l| l.pressed)?;
                let off = lane_t(100, |l| l.released);
                (off.map(|o| o < v).unwrap_or(true)).then_some(v)
            }
            120..=139 => {
                let l = ses.lanes.get(lane_from_lr2(mode, id - 120)?)?;
                let off = l.released?;
                (l.pressed.map(|o| o <= off).unwrap_or(true)).then(|| self.ms(off))
            }
            140 => {
                let t = ses.chart_us(self.now) as f64;
                let tm = &ses.chart.timing;
                let beat_t = tm.time_at_beat(tm.beat_at_time(t).floor());
                Some(self.ms(ses.start) + beat_t / 1000)
            }
            _ => None,
        }
    }

    fn button(&self, id: i32) -> Option<i64> {
        let ses = &self.s.session;
        match id {
            // last judgement of a lane / of a side
            500..=519 => {
                let lane = lane_from_lr2(ses.mode, id - 500)?;
                ses.lanes.get(lane)?.judge.map(|j| j as i64)
            }
            520 | 521 => self.last_judge((id - 520) as usize).map(|j| j.judge as i64),
            40 => self.engine.map(|e| gauge_index(e.gauge.current_type())),
            _ => config_button(self.ctx, id).or_else(|| self.number(id)),
        }
    }

    fn offset(&self, id: i32) -> Option<[f32; 6]> {
        // (the second player's own cover in VS)
        let two = id == P2_COVER_OFFSET;
        let p = self.side_play(two as usize);
        let id = if two { OFFSET_LANECOVER } else { id };
        // lane height in skin pixels (the first lane of the note field)
        let lane_h = self.lane_h?;
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
            let ses = &self.s.session;
            let lj = ses.last_judge[0];
            match abs {
                80 => ses.engine().is_none(),
                81 => ses.engine().is_some(),
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
                270 => self.s.controls.panel[0].open(),
                // a judgement of the kind has happened
                2241..=2246 => sc.count(Judge::ALL[(abs - 2241) as usize]) > 0,
                // the second side's gauge (groove / survival), AUTO
                // SCRATCH and DJ level; the first side winning, losing,
                // level with the second
                44 | 45 => {
                    let g = ses.side(1).map(|o| o.engine.gauge.current_type());
                    g.is_some_and(|g| g.is_survival() == (abs == 45))
                }
                56 | 57 => {
                    let o = ses.side(1).map(|o| &o.engine);
                    o.is_some_and(|o| {
                        let lane = ses.mode.scratch_lane(1).unwrap_or(0);
                        o.is_auto(lane) == (abs == 57)
                    })
                }
                210..=217 => self.opp().is_some_and(|o| {
                    let sc = &o.score;
                    vibebms_core::score::DjLevel::from_rate(sc.ex_score(), sc.passed * 2) as i32
                        == 7 - (abs - 210)
                }),
                352..=354 => match self.opp_ex {
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
                241..=246 => lj.is_some_and(|j| j.judge == Judge::ALL[(abs - 241) as usize]),
                // FAST / SLOW, as set (on, beyond its threshold)
                1242 | 1243 => lj
                    .and_then(|j| self.ctx.config.play.fast_slow(j.diff))
                    .is_some_and(|fast| fast == (abs == 1242)),
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
        let ses = &s.session;
        let p = &self.ctx.config.play;
        let e = self.engine;
        let sc = e.map(|e| &e.score);
        let t_ms = ses.chart_us(self.now).max(0) / 1000;
        // (the chart's length goes over every note)
        let len_ms = || ses.chart.length() / 1000;
        // BAD, POOR and MISS so far (the result counts the notes never
        // reached too)
        let bp = || {
            sc.map(|s| (s.count(Judge::Bad) + s.count(Judge::Poor) + s.count(Judge::Miss)) as i64)
        };
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
            | 410..=420
            | 423..=427 => return score_number(sc?, id),
            76 | 177 => bp()?,
            90..=92 | 96 | 400 => return chart_number(&self.chart, id),
            100 => lr2_score(sc?),
            102 => percent_parts(sc?.current_rate()).0,
            103 => percent_parts(sc?.current_rate()).1,
            104 => self.s.course_combo(e?.combo(), e?) as i64,
            107 => e?.gauge.value() as i64,
            407 => ((e?.gauge.value() * 10.0) as i64) % 10,
            114 => sc?.count(Judge::Poor) as i64 + sc?.count(Judge::Miss) as i64,
            // the personal best's EX score, rate, max combo and miss count,
            // and the differences to them
            150 | 170 | 173 | 176 => return best_number(self.best, id),
            183 => of_max(self.pace().best)?.0,
            184 => of_max(self.pace().best)?.1,
            175 => sc?.max_combo as i64 - best_number(self.best, 173)?,
            178 => bp()? - best_number(self.best, 176)?,
            368 => s.facts.total as i64,
            350..=367 => return song_number(&s.req.song, id),
            121 | 151 => self.pace().target,
            // differences to the pacemakers (best / target at this point)
            152 | 172 => sc?.ex_score() as i64 - self.pace().best_now,
            128 | 153 => sc?.ex_score() as i64 - self.pace().target_now,
            154 => diff_next_rank(sc?)?,
            122 | 135 | 157 => of_max(self.pace().target)?.0,
            123 | 136 | 158 => of_max(self.pace().target)?.1,
            // timing of the last judgement (ms, + = early as in beatoraja)
            525 | 526 => self.last_judge((id - 525) as usize)?.diff / 1000,
            160 => ses
                .chart
                .timing
                .bpm_at_time(ses.chart_us(self.now).max(0) as f64)
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
            // the IR rival TARGET chose, the target's name
            1 => crate::scene::target::name(self.ctx).1?,
            // (a battle's target is the opponent: the result names it)
            3 if self.opp_ex.is_none() => crate::scene::target::name(self.ctx).0,
            1001..=1003 => return table_text(self.table()?, id),
            // course stages' titles
            150..=159 => s
                .req
                .course
                .as_ref()?
                .songs
                .get((id - 150) as usize)?
                .full_title(),
            // a NONSTOP course: its stage's
            _ => match s.session.stage_at(s.session.chart_us(self.now)) {
                Some(k) => {
                    return chart_text(
                        &ChartText::of_song(s.req.course.as_ref()?.songs.get(k)?),
                        id,
                    )
                }
                None => {
                    return chart_text(&ChartText::of_meta(&s.session.chart.meta, &s.req.song), id)
                }
            },
        })
    }

    fn rate(&self, id: i32) -> f32 {
        let ses = &self.s.session;
        let sc = self.engine.map(|e| &e.score);
        match id {
            6 | 101 => {
                (ses.chart_us(self.now).max(0) as f32 / ses.chart.length().max(1) as f32).min(1.0)
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

/// EX score minus the next DJ level's border at the notes judged so far.
fn diff_next_rank(sc: &Score) -> Option<i64> {
    let ex = sc.ex_score() as i64;
    rank_borders(sc.passed as i64 * 2)
        .find(|t| *t > ex)
        .map(|t| ex - t)
}

/// Whether a full combo stands (no BAD or worse, no combo broken).
fn full_combo(sc: &Score) -> bool {
    sc.combo_breaks == 0
        && [Judge::Bad, Judge::Poor, Judge::Miss]
            .into_iter()
            .all(|j| sc.count(j) == 0)
}

/// vibeBMS ids of the second player's own lane cover in VS (the first
/// player's: 4, 271, 14, 313 and offset 4): its SUDDEN+, shown, white and
/// green numbers, its cover offset.
const P2_SUDDEN: i32 = 9404;
const P2_COVER: i32 = 9271;
const P2_WHITE: i32 = 9014;
const P2_GREEN: i32 = 9313;
const P2_COVER_OFFSET: i32 = 9004;

/// Slider / graph value of the hi-speed position (vibeBMS).
const HISPEED_RATE: i32 = 9400;

/// The notes graph of the first side, made again once it judged.
#[derive(Default)]
pub(super) struct NotesGraph {
    /// Judgements and notes judged when it was made.
    made_at: Option<(usize, usize)>,
    cols: [Vec<Column>; 3],
}

impl NotesGraph {
    fn of(&mut self, e: &Engine, mode: PlayMode, length_us: i64) -> &[Vec<Column>; 3] {
        let now = (e.judge_log.len(), e.ghost.len());
        if self.made_at != Some(now) {
            self.cols = graphs::columns(e.notes(), mode, length_us);
            self.made_at = Some(now);
        }
        &self.cols
    }
}

pub(super) struct PlayHooks<'a> {
    pub st: &'a PlayState<'a>,
    pub notes_graph: &'a mut NotesGraph,
}

impl Hooks for PlayHooks<'_> {
    fn graph(&mut self, d: &mut Draw, graph: &crate::skin::Graph, r: Rect, color: [f32; 4]) {
        use crate::skin::Graph;
        let ses = &self.st.s.session;
        let a = graphs::Area {
            r,
            color,
            reveal: 1.0,
        };
        match graph {
            Graph::Notes(g) => {
                let Some(e) = ses.engine() else {
                    return;
                };
                let cols = self.notes_graph.of(e, ses.mode, ses.chart.length());
                // play position, once the chart runs
                let cursor = ses
                    .ready_at
                    .map(|_| ses.chart_us(self.st.now).max(0) as f64 / 1000.0);
                graphs::notes(d, &a, g, &cols[g.kind.min(2)], cursor);
            }
            Graph::Bpm(g) => {
                let len = ses.chart.length();
                graphs::bpm(d, &a, g, &ses.bpm_log, len, ses.chart.meta.main_bpm);
            }
            Graph::Visualizer(g) => {
                let Some(e) = ses.engine() else {
                    return;
                };
                // (it shows the last 100)
                let log = &e.timing_log[e.timing_log.len().saturating_sub(100)..];
                let mut recent = [0; 100];
                for (r, t) in recent.iter_mut().zip(log) {
                    *r = t.1;
                }
                graphs::visualizer(d, &a, g, &recent[..log.len()], &e.windows().note);
            }
            Graph::Gauge(_) | Graph::Timing(_) => {}
        }
    }

    fn notes(&mut self, d: &mut Draw, skin: &LoadedSkin, xf: &Xf, now: i64, st: &dyn SkinState) {
        if skin.skin.perspective {
            super::persp::draw(self.st, d, xf);
        } else {
            super::notes::draw(self.st, d, skin, xf, now, st);
        }
    }

    fn bga(&mut self, d: &mut Draw, rect: Rect) {
        let s = self.st.s;
        d.b.rect(rect, [0.0, 0.0, 0.0, 1.0]);
        if !s.bga_shown(self.st.ctx) {
            return;
        }
        let mut shown = false;
        let extend = self.st.ctx.config.play.bga_size == crate::config::BgaSize::Extend;
        for (t, alpha) in s.bga.textures(&s.session, self.st.now) {
            match extend {
                true => d.image_cover(t, rect, alpha),
                false => d.image_fit(t, rect, alpha),
            }
            shown = true;
        }
        // charts without BGA show their stage file instead
        if !shown && !s.session.chart.meta.has_bga {
            if let Some(t) = s.bga.stage {
                d.image_fit(t, rect, 0.8);
            }
        }
    }

    fn ref_image(&self, id: i32) -> Option<TexId> {
        match id {
            100 => self.st.s.bga.stage,
            101 => self.st.s.bga.back,
            _ => None,
        }
    }

    fn gauge(&self, side: usize) -> (f32, f32, bool, usize) {
        match self.st.s.session.side(side).map(|s| &s.engine) {
            Some(e) => {
                let g = e.gauge.current();
                let ty = crate::scene::skinvals::skin_gauge(g.ty);
                (g.value(), g.elem.border, g.ty.is_survival(), ty)
            }
            None => (0.0, 80.0, false, 2),
        }
    }

    fn judge(&self, side: usize) -> Option<(usize, u32)> {
        let j = self.st.last_judge(side)?;
        let combo = match self.st.engine {
            Some(e) if side == 0 => self.st.s.course_combo(j.combo, e),
            _ => j.combo,
        };
        Some((j.judge as usize, combo))
    }
}
