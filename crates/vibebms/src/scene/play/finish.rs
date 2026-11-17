//! The end of a play: its result, its replay.

use super::session::Role;
use super::{single_mode, PlayRequest, PlayScene, Versus};
use crate::app::{Ctx, Next};
use crate::config::PlayConfig;
use crate::replay::Replay;
use crate::scene::result::{ResultData, ResultScene, SideResult};
use vibebms_core::gauge::GaugeAutoShift;
use vibebms_core::score::{Assist, ClearType};

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
    pub(super) fn finish(&mut self, ctx: &mut Ctx, failed: bool) -> Next {
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
        let gauge_clear = if failed || e.is_failed() && gauge.auto_shift != GaugeAutoShift::Continue
        {
            None
        } else {
            gauge.final_clear_gauge().map(|g| g.clear_type())
        };
        let mut score = e.score.clone();
        let clear = if self.controls.quit {
            ClearType::Failed
        } else {
            score.clear_type(gauge_clear, self.facts.assist)
        };
        if self.controls.quit {
            score.passed = score.passed.min(score.total_notes);
        }
        let mut course = self.req.course.clone();
        if let Some(c) = course.as_mut() {
            c.gauge_values = Some(gauge.values());
            c.stages
                .push((self.session.chart.meta.title.clone(), score.clone(), clear));
        }
        // a battle's sides are single plays
        let played_mode = if self.session.sides.len() > 1 {
            single_mode(self.session.mode)
        } else {
            self.session.mode
        };
        let data = ResultData {
            request: PlayRequest {
                course: None,
                replay: None,
                ..self.req.clone()
            },
            title: self.session.chart.meta.title.clone(),
            subtitle: self.session.chart.meta.subtitle.clone(),
            artist: self.session.chart.meta.artist.clone(),
            level: self.session.chart.meta.level.clone(),
            mode: played_mode,
            ln: self.facts.score_ln,
            score,
            clear,
            gauge_type: gauge.current_type(),
            gauge_value: gauge.value(),
            gauge_log: settled_log(&side.gauge_log, gauge.value()),
            timing_log: e.timing_log.clone(),
            windows: e.windows().note.clone(),
            judge_log: e.judge_log.clone(),
            notes_graph: crate::scene::graphs::columns(
                e.notes(),
                played_mode,
                self.session.chart.length(),
            ),
            meta: self.session.chart.meta.clone(),
            length_us: self.session.chart.length(),
            bpm_log: self.session.bpm_log.clone(),
            autoplay: self.req.autoplay,
            unranked: self.facts.unranked,
            assist: self.facts.assist,
            options: self.facts.options_label.clone(),
            judge: format!(
                "{} / {}",
                ctx.config.play.judge_preset.label(),
                ctx.config.play.gauge_preset.label()
            ),
            seed: self.facts.seed,
            course,
            p2: self.opponent_result(),
            replay: (!e.recorded().is_empty()).then(|| Replay {
                version: crate::replay::VERSION,
                sha256: self.req.song.sha256.clone(),
                md5: self.req.song.md5.clone(),
                title: self.session.chart.meta.title.clone(),
                ln: self.facts.score_ln as i32,
                date: crate::platform::unix_now() as i64,
                seed: self.facts.seed,
                play: self.played(ctx),
                ex_score: e.score.ex_score(),
                clear: clear.label().to_string(),
                ghost: e.ghost.clone(),
                ops: e.recorded().to_vec(),
            }),
        };
        Next::Replace(Box::new(ResultScene::new(ctx, data)))
    }

    /// The play options as they made this play (a replay's): G-BATTLE
    /// borrowed the opponent's lane option for a single play.
    fn played(&self, ctx: &Ctx) -> PlayConfig {
        let mut p = ctx.config.play.clone();
        if let Some(Versus::Ghost(g, _)) = &self.req.vs {
            p.random_1p = g.play.random_1p;
            p.battle = false;
        }
        p
    }

    /// The second side's results (G-BATTLE's opponent).
    fn opponent_result(&self) -> Option<SideResult> {
        let side = self.session.sides.get(1)?;
        let e = &side.engine;
        let (name, desync) = match (&side.role, &self.req.vs) {
            (Role::Ghost { desync, .. }, Some(Versus::Ghost(_, slot))) => {
                let name = match slot {
                    crate::replay::Slot::Best => "MY BEST",
                    crate::replay::Slot::Last => "LAST",
                };
                (name.to_string(), *desync)
            }
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
