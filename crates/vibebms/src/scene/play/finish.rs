//! The end of a play: its result, its replay.

use super::session::Role;
use super::{single_mode, CourseRun, PlayRequest, PlayScene, Versus};
use crate::app::{Ctx, Next};
use crate::config::PlayConfig;
use crate::replay::Replay;
use crate::scene::result::{ResultData, ResultScene, SideResult};
use vibebms_core::gauge::GaugeAutoShift;
use vibebms_core::record::{
    self, thin_gauge_log, CourseStage, DeviceFacts, Platform, PlayFlags, PlayRecord, PlayResult,
};
use vibebms_core::replay::CourseReplay;
use vibebms_core::score::{Assist, ClearType};
use vibebms_ir::rule::Profile;

/// The gauge graph's samples (every 500 ms) with the gauge as the play
/// ended: a FAILED play's graph reaches 0, a clear's its last value.
pub(super) fn settled_log(log: &[f32], last: f32) -> Vec<f32> {
    let mut v = log.to_vec();
    if v.last() != Some(&last) {
        v.push(last);
    }
    v
}

impl PlayScene {
    /// The play's result; `ended_ms` and `wall_ms`: when it ended (Unix
    /// ms) and the real time it ran.
    pub(super) fn finish(
        &mut self,
        ctx: &mut Ctx,
        failed: bool,
        ended_ms: i64,
        wall_ms: u64,
    ) -> Next {
        // practice: back to its screen, the attempt's result there
        if self.req.practice.is_some() {
            self.keep_practice_result(failed);
            return Next::Pop;
        }
        let Some(side) = self.session.sides.first() else {
            return Next::Pop;
        };
        let e = &side.engine;
        // quit (START + SELECT, Esc) before a key was played: no play to
        // record or count, no results; back to song select
        if self.controls.quit && !self.controls.touched {
            return Next::Pop;
        }
        let gauge = &e.gauge;
        let died = failed || e.is_failed() && gauge.auto_shift != GaugeAutoShift::Continue;
        let gauge_clear = if died {
            None
        } else {
            gauge.final_clear_gauge().map(|g| g.clear_type())
        };
        let mut score = e.score.clone();
        let assist = self.facts.rules.assist.level();
        // a play of the vibe-LR2 profile has LR2's lamp (also what IR gets)
        let lr2 = Profile::VibeLr2
            .rules()
            .matches(&self.facts.rules, &self.facts.chart);
        let clear = if self.controls.quit {
            ClearType::Failed
        } else if lr2 {
            score.lr2_clear_type(died, gauge_clear, assist)
        } else {
            score.clear_type(gauge_clear, assist)
        };
        if self.controls.quit {
            score.passed = score.passed.min(score.total_notes);
        }
        // an expert or class course: what the stage adds to the course
        // (S-OLR2b 4.2); a quit stage ends it as a failed one
        let mut course = self.req.course.clone();
        if let Some(c) = course.as_mut().filter(|c| !c.nonstop()) {
            c.gauge_values = Some(gauge.values());
            c.seed.get_or_insert(self.facts.seed);
            c.stages
                .push((self.session.chart.meta.title.clone(), score.clone(), clear));
            let mines = e
                .notes()
                .iter()
                .filter(|n| n.is_mine() && n.state.is_some())
                .count() as u32;
            let first_break = e.ghost.iter().position(|j| !e.rules().combo[*j as usize]);
            c.totals
                .add(&score, mines, e.combo(), first_break.map(|i| i as u32));
            let left = if died || self.controls.quit {
                0.0
            } else {
                gauge.value()
            };
            c.last_gauge = Some((gauge.current_type(), left));
        }
        let gauge_log = settled_log(&side.gauge_log, gauge.value());
        let nonstop = self.req.course.as_ref().filter(|c| c.nonstop());
        // played up to the last judgement (as beatoraja counts it)
        let last_us = e.judge_log.iter().map(|j| j.0).max().unwrap_or(0);
        let record = PlayRecord {
            v: record::VERSION,
            play_id: String::new(),
            chart: std::mem::take(&mut self.facts.chart),
            rules: std::mem::take(&mut self.facts.rules),
            device: DeviceFacts {
                touch_ui: ctx.touch_enabled(),
                platform: Platform::current(),
                ..self.session.device
            },
            flags: PlayFlags {
                autoplay: self.req.autoplay,
                replay: self.req.replay.is_some(),
                versus: self.req.is_vs(),
                ghost_battle: matches!(self.req.vs, Some(Versus::Ghost(..))),
                aborted: self.controls.quit,
                practice: self.req.practice.is_some(),
            },
            result: PlayResult {
                score: score.clone(),
                clear,
                gauge: gauge.current_type(),
                gauge_value: gauge.value(),
                gauge_values: gauge.values(),
                gauge_log: thin_gauge_log(&gauge_log),
                play_ms: last_us.max(0) as u64 / 1000,
            },
            client: crate::ir::build_id::client(),
            ended_ms,
            wall_ms,
            course: self.req.course.as_ref().map(|c| CourseStage {
                name: c.name.clone(),
                index: c.index as u32,
                of: c.songs.len() as u32,
            }),
        };
        // a battle's sides are single plays
        let played_mode = if self.session.sides.len() > 1 {
            single_mode(self.session.mode)
        } else {
            self.session.mode
        };
        let data = ResultData {
            // (a NONSTOP course is retried from its start, with its seed:
            // S-OLR2b 3.7)
            request: PlayRequest {
                course: nonstop.map(|c| CourseRun {
                    seed: Some(self.facts.seed),
                    ..c.restart()
                }),
                replay: None,
                ..self.req.clone()
            },
            // (a NONSTOP course: the course's name, its stages)
            title: match nonstop {
                Some(c) => c.name.clone(),
                None => self.session.chart.meta.title.clone(),
            },
            subtitle: match nonstop {
                Some(_) => "NONSTOP COURSE".into(),
                None => self.session.chart.meta.subtitle.clone(),
            },
            artist: match nonstop {
                Some(c) => c
                    .songs
                    .iter()
                    .map(|s| s.title.as_str())
                    .collect::<Vec<_>>()
                    .join(" / "),
                None => self.session.chart.meta.artist.clone(),
            },
            level: self.session.chart.meta.level.clone(),
            mode: played_mode,
            ln: self.facts.score_ln,
            score,
            clear,
            gauge_type: gauge.current_type(),
            gauge_value: gauge.value(),
            gauge_log,
            timing_log: e.timing_log.clone(),
            windows: e.windows().note.clone(),
            judge_log: e.judge_log.clone(),
            notes_graph: crate::scene::graphs::columns(
                e.notes(),
                played_mode,
                self.session.chart.length(),
            ),
            meta: vibebms_core::ChartMeta {
                title: match nonstop {
                    Some(c) => c.name.clone(),
                    None => self.session.chart.meta.title.clone(),
                },
                ..self.session.chart.meta.clone()
            },
            length_us: self.session.chart.length(),
            bpm_log: self.session.bpm_log.clone(),
            autoplay: self.req.autoplay,
            unranked: self.facts.unranked,
            assist,
            options: self.facts.options_label.clone(),
            course,
            p2: self.opponent_result(),
            replay: (!e.recorded().is_empty()).then(|| Replay {
                version: crate::replay::VERSION,
                sha256: self.req.replay_key(),
                md5: self.req.song.md5.clone(),
                title: self.session.chart.meta.title.clone(),
                ln: self.facts.score_ln as i32,
                date: ended_ms / 1000,
                seed: record.rules.seed,
                play: self.played(ctx),
                ex_score: e.score.ex_score(),
                clear,
                ghost: e.ghost.clone(),
                ops: e.recorded().to_vec(),
                course: self.req.course.as_ref().map(|c| CourseReplay {
                    course: c.def.clone(),
                    sha256: c.songs.iter().map(|s| s.sha256.clone()).collect(),
                    stage: c.index as u32,
                    gauge: self.facts.gauge_in.clone(),
                }),
            }),
            course_total: false,
            record,
        };
        Next::Replace(Box::new(ResultScene::new(ctx, data)))
    }

    /// The play options as they made this play (a replay's): G-BATTLE
    /// borrowed the opponent's lane option for a single play; a course's
    /// stage had the course's rules.
    fn played(&self, ctx: &Ctx) -> PlayConfig {
        let mut p = ctx.config.play.clone();
        if let Some(c) = &self.req.course {
            p.rules = c.rules(&p.rules);
        }
        if let Some(Versus::Ghost(g, _)) = &self.req.vs {
            p.random_1p = g.play.random_1p;
            p.freq = g.play.freq;
            p.key_convert = g.play.key_convert;
            p.trainer_order = g.play.trainer_order.clone();
            p.battle = false;
        }
        p
    }

    /// The second side's results (G-BATTLE's opponent).
    fn opponent_result(&self) -> Option<SideResult> {
        let side = self.session.sides.get(1)?;
        let e = &side.engine;
        let (name, desync) = match (&side.role, &self.req.vs) {
            (Role::Ghost { desync, .. }, Some(Versus::Ghost(_, slot))) => (slot.label(), *desync),
            _ => ("2P".to_string(), false),
        };
        let gauge = &e.gauge;
        let gauge_clear = if e.is_failed() && gauge.auto_shift != GaugeAutoShift::Continue {
            None
        } else {
            gauge.final_clear_gauge().map(|g| g.clear_type())
        };
        let mut score = e.score.clone();
        score.passed = score.passed.min(score.total_notes);
        let ex = match (&self.req.vs, desync) {
            (Some(Versus::Ghost(g, _)), true) => g.ex_score,
            _ => score.ex_score(),
        };
        Some(SideResult {
            name,
            clear: score.clear_type(gauge_clear, Assist::None),
            score,
            ex,
            desync,
            gauge_type: gauge.current_type(),
            gauge_value: gauge.value(),
            gauge_log: settled_log(&side.gauge_log, gauge.value()),
            timing_log: e.timing_log.clone(),
            judge_log: e.judge_log.clone(),
        })
    }
}
