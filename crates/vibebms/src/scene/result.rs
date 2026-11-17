//! Result screen: lamp, score, judge breakdown, gauge graph, timing histogram,
//! saving best scores, and what became of the play at each IR.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::SubmitPolicy;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{Gfx, Rect};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::library::BestScore;
use crate::platform::{Duration, Instant};
use crate::scene::play::{CourseRun, PlayRequest, PlayScene};
use crate::scene::result_skin::{
    ResultHooks, ResultState, EVENT_CONTINUE, EVENT_REPLAY, EVENT_RETRY,
};
use crate::scene::skin_host::{SkinHost, Tap};
use crate::scene::skinned;
use crate::skin::SkinType;
use crate::sysound::Sound;
use crate::theme;
use vibebms_core::gauge::GaugeType;
use vibebms_core::judge::Judge;
use vibebms_core::record::PlayRecord;
use vibebms_core::score::{Assist, ClearType, Score};
use vibebms_core::{LnType, PlayMode};
use winit::keyboard::KeyCode;

/// The other side of a battle: G-BATTLE's opponent (VS: the 2P player).
#[derive(Clone, Debug)]
pub struct SideResult {
    /// Who it is: "MY BEST", "LAST", "2P".
    pub name: String,
    pub score: Score,
    /// Its EX score (G-BATTLE out of step: its replay's).
    pub ex: u32,
    /// G-BATTLE: its judgements went other than its replay's.
    pub desync: bool,
    pub clear: ClearType,
    pub gauge_type: GaugeType,
    pub gauge_value: f32,
    pub gauge_log: Vec<f32>,
    pub timing_log: Vec<(i64, i64)>,
    pub judge_log: Vec<(i64, Judge, i8)>,
}

/// A battle's outcome for the first side, by EX score.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Win,
    Lose,
    Draw,
}

impl Outcome {
    pub fn of(mine: u32, theirs: u32) -> Outcome {
        match mine.cmp(&theirs) {
            std::cmp::Ordering::Greater => Outcome::Win,
            std::cmp::Ordering::Less => Outcome::Lose,
            std::cmp::Ordering::Equal => Outcome::Draw,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Outcome::Win => "WIN",
            Outcome::Lose => "LOSE",
            Outcome::Draw => "DRAW",
        }
    }
}

#[derive(Clone)]
pub struct ResultData {
    pub request: PlayRequest,
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub level: String,
    pub mode: PlayMode,
    /// The LN mode the play is filed under (scores, replays, IR).
    pub ln: LnType,
    pub score: Score,
    pub clear: ClearType,
    pub gauge_type: GaugeType,
    pub gauge_value: f32,
    pub gauge_log: Vec<f32>,
    pub timing_log: Vec<(i64, i64)>,
    /// Note judge windows (late, early) in µs, PG first.
    pub windows: Vec<(i64, i64)>,
    pub judge_log: Vec<(i64, Judge, i8)>,
    /// beatoraja notes graph columns (note type / judge / FAST-SLOW).
    pub notes_graph: [Vec<super::graphs::Column>; 3],
    pub meta: vibebms_core::chart::ChartMeta,
    /// Chart length (us) and BPM changes (time us, bpm) for the graphs.
    pub length_us: i64,
    pub bpm_log: Vec<(i64, f64)>,
    pub autoplay: bool,
    /// Why the play is not recorded (options that change the chart or
    /// the judge, a course stage).
    pub unranked: Option<&'static str>,
    /// Assist options took effect: only the lamp (ASSIST / L-ASSIST) and
    /// the play count are kept, nothing is sent.
    pub assist: Assist,
    pub options: String,
    pub course: Option<CourseRun>,
    /// A battle's other side.
    pub p2: Option<SideResult>,
    /// The play's replay (a play of its own, not one watched).
    pub replay: Option<crate::replay::Replay>,
    /// What the play was: its rules, devices and end time among them.
    pub record: PlayRecord,
    /// An expert or class course's own result (its stages' totals).
    pub course_total: bool,
}

/// Whether a play goes into the records and the player's totals: not
/// autoplay, not a replay or another unranked play. (A play quit before
/// any key reached the judge has no result.)
fn recorded(autoplay: bool, unranked: Option<&str>) -> bool {
    !autoplay && unranked.is_none()
}

pub struct ResultScene {
    pub(crate) d: ResultData,
    pub(crate) before: Option<BestScore>,
    pub(crate) after: Option<BestScore>,
    /// (average, standard deviation) of the timing log (ms).
    pub(crate) timing_stats: (f64, f64),
    /// What became of the play's record here ("not saved", "lamp only").
    note: String,
    /// Where the IR lines are (a tap there sends the play when the player
    /// is asked to).
    ir_rect: Rect,
    /// The play's profile and rules (`rule::describe`).
    rules_line: String,
    pub(crate) host: SkinHost,
    /// The chart's stage file (skin reference image 100).
    pub(crate) stage_tex: Option<crate::gfx::TexId>,
    /// Leaving (the skin's fade-out running): what follows.
    leave_to: Option<Leave>,
}

#[derive(Clone, Copy)]
enum Leave {
    Proceed,
    Retry,
}

impl ResultData {
    /// A battle's outcome for the first side.
    pub fn outcome(&self) -> Option<Outcome> {
        let p2 = self.p2.as_ref()?;
        Some(Outcome::of(self.score.ex_score(), p2.ex))
    }
}

impl ResultScene {
    pub fn new(ctx: &mut Ctx, mut d: ResultData) -> ResultScene {
        if !d.course_total {
            ctx.net.ir.record(&mut d.record);
        }
        let song = &d.request.song;
        let ln = d.ln as i32;
        // a NONSTOP course and a course's result: under the course's
        // identity (S-OLR2b 3.8, 4.4)
        let key = match (&d.course, d.course_total) {
            (Some(c), true) => c.def.id(),
            _ => d.request.record_key().into_owned(),
        };
        let stage = d.course.as_ref().is_some_and(|c| !c.nonstop()) && !d.course_total;
        let before = ctx.best_key(&key, ln).cloned();
        let mut after = None;
        let mut note = String::new();
        // every play of its own keeps its replay as the chart's latest
        // (a BATTLE play's would replace a single play's: it is not kept)
        if let Some(r) = d.replay.as_ref().filter(|r| r.keepable()) {
            if let Err(e) = crate::replay::save(r, &ctx.paths.root, crate::replay::Slot::Last) {
                log::warn!("replay: {e}");
            }
        }
        if stage && !d.autoplay {
            // a course's stage: its chart's record, partly (S-OLR2b 4.4)
            note = match (d.assist, ctx.lib.db.as_mut()) {
                (Assist::None, Some(lib)) => {
                    let b = crate::courses::save_stage(lib, &song.sha256, ln, &d.score, d.clear);
                    ctx.lib.scores.insert((song.sha256.clone(), ln), b.clone());
                    after = Some(b);
                    "COURSE STAGE: best score and FULL COMBO kept".into()
                }
                _ => "COURSE STAGE: not saved".into(),
            };
        } else if let Some(why) = d.unranked.filter(|_| !d.autoplay) {
            note = format!("{why}: not saved");
        } else if recorded(d.autoplay, d.unranked) {
            let ranked = d.assist == Assist::None || d.course_total;
            if let Some(lib) = ctx.lib.db.as_mut() {
                let play_ms = d.record.result.play_ms;
                let b = if d.course.is_some() && ranked {
                    crate::courses::save_course(lib, &key, ln, &d.score, d.clear, play_ms)
                } else if ranked {
                    lib.record_play(&key, ln, &d.score, d.clear, play_ms)
                } else {
                    lib.record_assisted(&key, ln, &d.score, d.clear, play_ms)
                };
                ctx.lib.scores.insert((key.clone(), ln), b.clone());
                after = Some(b);
            }
            if !ranked {
                note = format!("{}: lamp only", d.clear.label());
            }
            // a new best EX score: its replay is the chart's best (MY BEST's
            // ghost)
            let best_before = before.as_ref().map_or(0, |b| b.ex_score);
            if let Some(r) = d
                .replay
                .as_ref()
                .filter(|r| r.keepable() && ranked && d.score.ex_score() > best_before)
            {
                if let Err(e) = crate::replay::save(r, &ctx.paths.root, crate::replay::Slot::Best) {
                    log::warn!("replay: {e}");
                }
            }
        }
        if ctx.config.ir.submit == SubmitPolicy::Auto && !d.course_total {
            let now = crate::platform::unix_ms();
            ctx.net.ir.submit(&d.record, &ctx.config.ir, false, now);
        }
        ctx.play_sound(result_sound(&d).0);
        let rules_line = vibebms_ir::rule::describe(&d.record);
        ResultScene {
            timing_stats: super::graphs::timing_stats(&d.timing_log),
            d,
            before,
            after,
            note,
            ir_rect: Rect::default(),
            rules_line,
            host: SkinHost::new(),
            stage_tex: None,
            leave_to: None,
        }
    }

    /// Start leaving: with a skin, its fade-out runs first.
    fn leave(&mut self, ctx: &mut Ctx, to: Leave) -> Next {
        if matches!(to, Leave::Retry) && !self.retries() {
            return Next::Stay;
        }
        let h = &mut self.host;
        if h.created.elapsed() < Duration::from_millis(500) || h.leaving.is_some() {
            return Next::Stay;
        }
        if h.fadeout_ms() > 0 {
            h.leave(Instant::now());
            self.leave_to = Some(to);
            return Next::Stay;
        }
        self.go(ctx, to)
    }

    fn go(&mut self, ctx: &mut Ctx, to: Leave) -> Next {
        let (playing, close) = result_sound(&self.d);
        ctx.stop_sound(playing);
        ctx.play_sound(close);
        match to {
            Leave::Proceed => self.proceed(ctx),
            Leave::Retry => self.retry(ctx),
        }
    }

    /// A course's stage (expert, class), not its result.
    fn course_stage(&self) -> bool {
        self.d.course.as_ref().is_some_and(|c| !c.nonstop()) && !self.d.course_total
    }

    /// Retry is offered: not in an expert or class course (S-OLR2b 4.3);
    /// a NONSTOP course plays again from its start (3.8).
    fn retries(&self) -> bool {
        !self.course_stage() && !self.d.course_total
    }

    fn next_stage(&self) -> bool {
        matches!(&self.d.course, Some(c) if self.course_stage() && c.index + 1 < c.songs.len() && self.d.clear != ClearType::Failed)
    }

    fn proceed(&mut self, ctx: &mut Ctx) -> Next {
        if self.next_stage() {
            let mut c = self.d.course.clone().unwrap();
            c.index += 1;
            let song = c.songs[c.index].clone();
            return Next::Replace(Box::new(PlayScene::new(
                ctx,
                PlayRequest {
                    song,
                    autoplay: self.d.autoplay,
                    course: Some(c),
                    replay: None,
                    vs: None,
                },
            )));
        }
        // the last stage played (or failed): the course's result
        if self.course_stage() {
            return Next::Replace(Box::new(ResultScene::new(ctx, course_result(&self.d))));
        }
        Next::Pop
    }

    /// Watch this play again (its replay).
    fn watch(&mut self, ctx: &mut Ctx) -> Next {
        let Some(r) = self.d.replay.clone() else {
            ctx.toast("No replay of this play".to_string());
            return Next::Stay;
        };
        // (a NONSTOP play's chart is its course's)
        Next::Replace(Box::new(PlayScene::new(
            ctx,
            PlayRequest {
                replay: Some(std::sync::Arc::new(r)),
                autoplay: false,
                vs: None,
                ..self.d.request.clone()
            },
        )))
    }

    /// Send the play now: the player was asked to (send policy ASK). (Not
    /// said so on the web: a server's preflight may refuse the header.)
    fn send_asked(&mut self, ctx: &mut Ctx) -> Next {
        let now = crate::platform::unix_ms();
        let intent = !crate::platform::IS_WEB;
        ctx.net
            .ir
            .submit(&self.d.record, &ctx.config.ir, intent, now);
        Next::Stay
    }

    fn retry(&mut self, ctx: &mut Ctx) -> Next {
        if !self.retries() {
            return Next::Stay;
        }
        Next::Replace(Box::new(PlayScene::new(ctx, self.d.request.clone())))
    }
}

impl Scene for ResultScene {
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        if self.host.first_update() {
            if !self.d.meta.stagefile.is_empty() {
                let p = std::path::Path::new(&self.d.request.song.path)
                    .parent()
                    .unwrap_or(std::path::Path::new(""))
                    .join(&self.d.meta.stagefile);
                self.stage_tex = crate::scene::select::resolve_image(&p)
                    .and_then(|rp| gfx.load_image(&rp, false));
            }
            let kind = if self.d.course_total {
                SkinType::CourseResult
            } else {
                SkinType::Result
            };
            let skin = {
                let st = ResultState::new(self, ctx, now);
                let stat = |id: i32| st.static_option(id);
                skinned::load(ctx, gfx, kind, &stat, &st)
            };
            self.host.skin = skin;
        }
        // events skin scripts asked for
        for ev in self.host.tick(gfx) {
            match ev {
                EVENT_RETRY => return self.leave(ctx, Leave::Retry),
                EVENT_CONTINUE => return self.leave(ctx, Leave::Proceed),
                EVENT_REPLAY => return self.watch(ctx),
                _ => {}
            }
        }
        if let Some(to) = self.leave_to.filter(|_| self.host.faded(now)) {
            self.leave_to = None;
            return self.go(ctx, to);
        }
        Next::Stay
    }

    fn exit(&mut self, _ctx: &mut Ctx, gfx: &mut Gfx) {
        self.host.free(gfx);
        if let Some(t) = self.stage_tex.take() {
            gfx.free_texture(t);
        }
    }

    fn realtime(&self) -> bool {
        // skin animations need every frame
        self.host.skin.is_some()
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                repeat: false,
                ..
            } => {
                if matches!(code, KeyCode::KeyR | KeyCode::F5) {
                    return self.leave(ctx, Leave::Retry);
                }
                if *code == KeyCode::KeyP {
                    return self.watch(ctx);
                }
                if *code == KeyCode::KeyI && ctx.config.ir.submit == SubmitPolicy::Ask {
                    return self.send_asked(ctx);
                }
                if let Some(Nav::Decide | Nav::Back | Nav::Close) =
                    nav_from_key(*code, &ctx.config.keys)
                {
                    return self.leave(ctx, Leave::Proceed);
                }
            }
            InputEv::Pad {
                id, pressed: true, ..
            } => {
                if let Some(Nav::Decide | Nav::Back | Nav::Close) =
                    nav_from_pad(*id, &ctx.config.keys)
                {
                    return self.leave(ctx, Leave::Proceed);
                }
            }
            InputEv::Pointer {
                phase: Phase::Up,
                x,
                y,
                ..
            } => {
                if self.ir_rect.contains(*x, *y) && ctx.config.ir.submit == SubmitPolicy::Ask {
                    return self.send_asked(ctx);
                }
                // skin buttons, else a tap anywhere continues
                return match self.host.tap(*x, *y) {
                    Tap::Script => Next::Stay,
                    Tap::Event(EVENT_RETRY, _) => self.leave(ctx, Leave::Retry),
                    Tap::Event(EVENT_REPLAY, _) => self.watch(ctx),
                    _ => self.leave(ctx, Leave::Proceed),
                };
            }
            _ => {}
        }
        Next::Stay
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let view = Rect::new(0.0, 0.0, ctx.view.width, ctx.view.height);
        {
            let st = ResultState::new(self, ctx, now);
            let mut hooks = ResultHooks { st: &st };
            self.host.draw(d, view, now, &st, &mut hooks);
        }
        // what became of the play here and at each IR, upwards from the
        // bottom
        let ui = ctx.ui();
        let lines = ctx.net.ir.status(&self.d.record, &ctx.config.ir);
        let line_h = 20.0 * ui;
        let mut y = ctx.view.height - 26.0 * ui;
        let style = TextStyle::new(14.0 * ui, theme::ACCENT, Align::Left);
        let mut width: f32 = 0.0;
        for l in std::iter::once(&self.note)
            .chain(&lines)
            .rev()
            .filter(|l| !l.is_empty())
        {
            width = width.max(d.text(l, 12.0 * ui, y, style));
            y -= line_h;
        }
        self.ir_rect = Rect::new(
            0.0,
            y + line_h,
            12.0 * ui + width,
            ctx.view.height - y - line_h,
        );
        // a battle's outcome (skins without a second side show it so)
        let shown = self.host.skin.as_ref().is_some_and(|s| {
            s.skin.objects.iter().any(|o| {
                o.dst
                    .ops
                    .contains(&crate::scene::result_skin::OPTION_BATTLE)
            })
        });
        if let (Some(p2), Some(o), false) = (&self.d.p2, self.d.outcome(), shown) {
            let what = if p2.desync {
                format!("{} (out of step)", p2.name)
            } else {
                p2.name.clone()
            };
            d.text(
                &format!(
                    "{}  {}  {} - {}  vs {what}",
                    match self.d.request.vs {
                        Some(crate::scene::play::Versus::Ghost(..)) => "G-BATTLE",
                        _ => "VS",
                    },
                    o.label(),
                    self.d.score.ex_score(),
                    p2.ex
                ),
                12.0 * ui,
                y - 2.0 * ui,
                TextStyle::new(16.0 * ui, theme::ACCENT, Align::Left),
            );
        }
        // the rules the play was judged under, and their profile
        d.text(
            &self.rules_line,
            ctx.view.width - 12.0 * ui,
            ctx.view.height - 26.0 * ui,
            TextStyle::new(14.0 * ui, theme::ACCENT, Align::Right),
        );
    }
}

/// The sound set's sounds of a result: the one it starts with (clear /
/// fail; the course's once a course ends) and the one it closes with.
fn result_sound(d: &ResultData) -> (Sound, Sound) {
    let failed = d.clear == ClearType::Failed;
    match (d.course_total, failed) {
        (true, false) => (Sound::CourseClear, Sound::CourseClose),
        (true, true) => (Sound::CourseFail, Sound::CourseClose),
        (false, false) => (Sound::ResultClear, Sound::ResultClose),
        (false, true) => (Sound::ResultFail, Sound::ResultClose),
    }
}

/// An expert or class course's result once its last stage is played (or
/// a stage failed), from that stage's: the stages' totals, the notes of
/// those not played counted as not judged, the course's lamp (S-OLR2b
/// 4.4; under the other gauge sets the worst stage's).
fn course_result(last: &ResultData) -> ResultData {
    let Some(c) = last.course.clone() else {
        return last.clone();
    };
    let mut score = c.totals.score.clone();
    score.total_notes += c.songs[c.stages.len().min(c.songs.len())..]
        .iter()
        .map(|s| s.notes.max(0) as u32)
        .sum::<u32>();
    let finished =
        c.stages.len() == c.songs.len() && c.stages.iter().all(|s| s.2 != ClearType::Failed);
    let (gauge_type, gauge_value) = c.last_gauge.unwrap_or((last.gauge_type, 0.0));
    let clear = if last.record.rules.gauge_set == vibebms_core::gauge::GaugePreset::Lr2 {
        c.totals
            .lamp(finished, gauge_value, gauge_type.clear_type())
    } else if finished {
        c.stages
            .iter()
            .map(|s| s.2)
            .min()
            .unwrap_or(ClearType::Failed)
    } else {
        ClearType::Failed
    };
    let subtitle = format!("{} COURSE", c.kind().label());
    let artist = c
        .stages
        .iter()
        .map(|s| s.0.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    ResultData {
        request: PlayRequest {
            course: None,
            ..last.request.clone()
        },
        meta: vibebms_core::ChartMeta {
            title: c.name.clone(),
            subtitle: String::new(),
            artist: artist.clone(),
            ..last.meta.clone()
        },
        title: c.name.clone(),
        subtitle,
        artist,
        score,
        clear,
        gauge_type,
        gauge_value,
        unranked: None,
        replay: None,
        p2: None,
        course: Some(c),
        course_total: true,
        ..last.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_own_ranked_plays_count() {
        assert!(recorded(false, None));
        assert!(!recorded(true, None));
        assert!(!recorded(false, Some("REPLAY")));
        assert!(!recorded(true, Some("COURSE")));
    }
}
