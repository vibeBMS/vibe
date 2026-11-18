//! A practice play (`vibebms_core::practice`): its section, played again
//! at once when it ends or fails with LOOP on, or on R / Backspace; Esc
//! goes back to the practice screen with the last attempt's result.
//! Nothing of it is recorded: no score, lamp, replay or IR entry.

use super::session::{Role, SideState};
use super::{PlayScene, Session, Stage};
use crate::app::{Ctx, Draw};
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{rgba, with_alpha, Rect};
use crate::platform::{Duration, Instant};
use crate::theme;
use std::sync::{Arc, Mutex};
use vibebms_audio::Bus;
use vibebms_core::gauge::{GaugeAutoShift, GaugeType};
use vibebms_core::play::RuleOptions;
use vibebms_core::practice::{Practice, Section};
use vibebms_core::score::Score;

/// What a practice play is: its section and settings, and where its
/// result goes.
#[derive(Clone, Debug)]
pub struct PracticeRun {
    pub practice: Practice,
    pub gauge: GaugeType,
    /// The gauge's value at the start (`None`: its own).
    pub gauge_start: Option<f32>,
    /// LOOP: the section again at once when it ends or fails.
    pub repeat: bool,
    /// Every attempt's `#RANDOM` and lane options are rolled from it.
    pub seed: i64,
    /// The last attempt's result, for the practice screen.
    pub result: Arc<Mutex<Option<PracticeResult>>>,
}

/// What an attempt came to.
#[derive(Clone, Debug, Default)]
pub struct PracticeResult {
    pub score: Score,
    /// (note time, diff) of every PGREAT..BAD, and the windows (late,
    /// early), as the result's timing graph has them.
    pub timing_log: Vec<(i64, i64)>,
    pub windows: Vec<(i64, i64)>,
    pub gauge: GaugeType,
    pub gauge_value: f32,
    /// "CLEAR", "NOT CLEAR" (the gauge under its border), "FAILED" (it
    /// emptied) or "QUIT".
    pub outcome: &'static str,
    /// Which attempt it was.
    pub attempt: u32,
}

impl PracticeRun {
    /// The rules of its plays on the player's `rules`: its gauge, no
    /// GAUGE AUTO SHIFT.
    pub fn rules(&self, rules: &RuleOptions) -> RuleOptions {
        RuleOptions {
            gauge: self.gauge,
            gauge_auto_shift: GaugeAutoShift::None,
            ..rules.clone()
        }
    }

    /// "PRACTICE  #012-#024  75%": what it is, for the screen.
    pub fn label(&self) -> String {
        let p = &self.practice;
        format!("PRACTICE  #{:03}-#{:03}  {}%", p.from, p.to, p.speed)
    }
}

/// What starts a practice play again.
pub(super) struct Again {
    rules: RuleOptions,
    section: Section,
    /// The wait before the lead-in (the skin's `#PLAYSTART`).
    lead: Duration,
    pub attempt: u32,
}

impl Again {
    pub fn new(rules: RuleOptions, section: Section, lead: Duration) -> Again {
        Again {
            rules,
            section,
            lead,
            attempt: 1,
        }
    }
}

impl PlayScene {
    /// The attempt's result for the practice screen (one quit before a
    /// key was played or a note judged: none; the one before stays).
    pub(super) fn keep_practice_result(&self, failed: bool) {
        let (Some(run), Some(again), Some(side)) = (
            &self.req.practice,
            &self.practice,
            self.session.sides.first(),
        ) else {
            return;
        };
        let e = &side.engine;
        if !self.controls.touched && e.score.passed == 0 {
            return;
        }
        let result = PracticeResult {
            score: e.score.clone(),
            timing_log: e.timing_log.clone(),
            windows: e.windows().note.clone(),
            gauge: e.gauge.current_type(),
            gauge_value: e.gauge.value(),
            outcome: if self.controls.quit {
                "QUIT"
            } else if failed || e.is_failed() {
                "FAILED"
            } else if e.gauge.current().is_qualified() {
                "CLEAR"
            } else {
                "NOT CLEAR"
            },
            attempt: again.attempt,
        };
        if let Ok(mut r) = run.result.lock() {
            *r = Some(result);
        }
    }

    /// The section again from its lead-in, at once (LOOP, R).
    pub(super) fn practice_again(&mut self, ctx: &mut Ctx, now: Instant) {
        let (failed, gauge_start) = match (&self.stage, &self.req.practice) {
            (Stage::Ending { failed, .. }, Some(run)) => (*failed, run.gauge_start),
            (_, Some(run)) => (false, run.gauge_start),
            _ => return,
        };
        self.keep_practice_result(failed);
        let Some(again) = self.practice.as_mut() else {
            return;
        };
        again.attempt += 1;
        let m = &ctx.audio.mixer;
        m.stop_bus(Bus::Bgm);
        m.stop_bus(Bus::Key);
        let chart = std::mem::take(&mut self.session.chart);
        let engine =
            vibebms_core::practice::engine(&chart, &again.section, &again.rules, gauge_start);
        let sides = vec![SideState::new(engine, Role::Player)];
        self.session = Session::new(chart, sides, false, now + again.lead);
        self.session.ready_at = Some(now);
        self.audio.start_at(m, &self.session, 0);
        self.controls.quit = false;
        self.controls.touched = false;
        self.stage = Stage::Playing;
    }

    /// "PRACTICE #012-#024 75%" over the top of the play, the attempt
    /// and LOOP beside it.
    pub(super) fn draw_practice(&self, ctx: &Ctx, d: &mut Draw, view: Rect) {
        let (Some(run), Some(again)) = (&self.req.practice, &self.practice) else {
            return;
        };
        let ui = ctx.ui();
        let mut text = run.label();
        text.push_str(&format!("  ·  TRY {}", again.attempt));
        if run.repeat {
            text.push_str("  ·  LOOP");
        }
        let size = 13.0 * ui;
        let w = d.measure(&text, size) + 24.0 * ui;
        let r = Rect::new(view.x + 8.0 * ui, view.y + 8.0 * ui, w, size + 12.0 * ui);
        d.b.rounded_panel(
            r,
            6.0 * ui,
            with_alpha(rgba(0x05060aff), 0.75),
            theme::ACCENT,
            1.0,
        );
        d.text(
            &text,
            r.x + 12.0 * ui,
            r.y + 6.0 * ui,
            TextStyle::new(size, theme::ACCENT, Align::Left),
        );
    }
}
