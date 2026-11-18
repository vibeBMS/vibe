//! Gameplay: a chart loaded, judged on one or two sides and drawn with a
//! play skin, with touch controls and the hi-speed panel; the result at
//! the end.

mod bga;
mod control;
mod finish;
mod hsview;
mod hud;
mod input;
mod layout;
mod loading;
mod notes;
mod persp;
mod practice;
mod session;
mod setup;
mod skin;
mod sound;
#[cfg(test)]
mod tests;

pub use practice::{PracticeResult, PracticeRun};
pub(crate) use session::Session;
#[cfg(test)]
pub(crate) use {
    session::{Role, SideState},
    skin::{ghost_ex, PlayState},
};

use crate::app::{Ctx, Draw, Next, Scene};
use crate::gfx::Gfx;
use crate::input::{InputEv, LaneEv};
use crate::library::Song;
use crate::platform::{Duration, Instant};
use crate::replay::Replay;
use session::{Out, Show};
use std::sync::Arc;
use vibebms_audio::Bus;
use vibebms_core::chart::Chart;
use vibebms_core::course::{CourseDef, CourseKind, CourseTotals};
use vibebms_core::gauge::{GaugePreset, GaugeType};
use vibebms_core::judge::Judge;
use vibebms_core::play::RuleOptions;
use vibebms_core::record::{ChartFacts, InputSource, RuleFacts};
use vibebms_core::score::{ClearType, Score};
use vibebms_core::{LnType, PlayMode};

/// A course in progress (`docs/LR2_COURSES.md`): an expert or class
/// course a stage at a time, a NONSTOP course as one play.
#[derive(Clone, Debug)]
pub struct CourseRun {
    pub name: String,
    /// Its kind, joins and stages (its identity, `CourseDef::id`).
    pub def: CourseDef,
    pub songs: Vec<Song>,
    /// The stage played (expert, class).
    pub index: usize,
    /// A table course's constraints (`ln`, `gauge_lr2`, `grade_mirror`, ...).
    pub constraints: Vec<String>,
    /// The gauges as the last stage left them.
    pub gauge_values: Option<Vec<f32>>,
    pub stages: Vec<(String, Score, ClearType)>,
    /// What the stages played add up to, and the course combo.
    pub totals: CourseTotals,
    /// The seed every stage is read and arranged with (set at the first).
    pub seed: Option<i64>,
    /// The gauge the last stage ended on, and its value.
    pub last_gauge: Option<(GaugeType, f32)>,
}

impl CourseRun {
    pub fn new(
        name: String,
        def: CourseDef,
        songs: Vec<Song>,
        constraints: Vec<String>,
    ) -> CourseRun {
        CourseRun {
            name,
            def,
            songs,
            index: 0,
            constraints,
            gauge_values: None,
            stages: Vec::new(),
            totals: CourseTotals::default(),
            seed: None,
            last_gauge: None,
        }
    }

    pub fn has(&self, c: &str) -> bool {
        self.constraints.iter().any(|x| x.eq_ignore_ascii_case(c))
    }

    pub fn kind(&self) -> CourseKind {
        self.def.kind
    }

    pub fn nonstop(&self) -> bool {
        self.kind() == CourseKind::Nonstop
    }

    /// The course from its start again (a NONSTOP play retried).
    pub fn restart(&self) -> CourseRun {
        CourseRun::new(
            self.name.clone(),
            self.def.clone(),
            self.songs.clone(),
            self.constraints.clone(),
        )
    }

    /// The rules a stage plays under on the player's `rules`: the course
    /// kind's (`RuleOptions::for_course`), then a table course's
    /// constraints (an LN mode, the LR2 gauges).
    pub fn rules(&self, rules: &RuleOptions) -> RuleOptions {
        let mut r = rules.clone();
        if self.has("gauge_lr2") {
            r.gauge_preset = GaugePreset::Lr2;
        }
        let mut r = r.for_course(self.kind());
        for (c, ln) in [("ln", LnType::Ln), ("cn", LnType::Cn), ("hcn", LnType::Hcn)] {
            if self.has(c) {
                r.ln_mode = ln;
                break;
            }
        }
        r
    }

    /// The lane options a stage may have (`play::build_chart`'s `course`):
    /// a class course MIRROR at most (with a table's `grade_mirror`), the
    /// others any.
    pub fn lanes(&self) -> Option<bool> {
        (self.kind() == CourseKind::Class).then(|| self.has("grade_mirror"))
    }

    /// LR2's settings forced on the player's view: a class course under
    /// the LR2 gauges has no lane cover or HIDDEN (S-OLR2b 5.1); a NONSTOP
    /// course scrolls at the hi-speed set, HS-FIX off (3.3).
    pub fn view(&self, p: &mut std::borrow::Cow<'_, crate::config::PlayConfig>) {
        match self.kind() {
            CourseKind::Class if self.rules(&p.rules).gauge_preset == GaugePreset::Lr2 => {
                let p = p.to_mut();
                p.sudden = 0.0;
                p.hidden = 0.0;
                p.lane_cover = false;
            }
            CourseKind::Nonstop => p.to_mut().hs_fix = crate::config::HsFix::Off,
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub struct PlayRequest {
    pub song: Song,
    pub autoplay: bool,
    pub course: Option<CourseRun>,
    /// Watch this replay (its options and seed make the chart and the rules;
    /// the keys do nothing but START / SELECT).
    pub replay: Option<Arc<Replay>>,
    /// An opponent on the second side.
    pub vs: Option<Versus>,
    /// A section practised (no records).
    pub practice: Option<PracticeRun>,
}

impl PlayRequest {
    /// What its records are kept under: a NONSTOP course's identity (it is
    /// recorded as one song, S-OLR2b 3.8), else the chart's SHA-256.
    pub fn record_key(&self) -> std::borrow::Cow<'_, str> {
        match &self.course {
            Some(c) if c.nonstop() => c.def.id().into(),
            _ => self.song.sha256.as_str().into(),
        }
    }

    /// What its replays are kept under: a NONSTOP course's by the MD5 of
    /// its identity (as LR2 names them), an expert or class course's
    /// stage by that and its number, else the chart's SHA-256.
    pub fn replay_key(&self) -> String {
        match &self.course {
            Some(c) => {
                let id = vibebms_core::bms::md5_hex(c.def.id().as_bytes());
                match c.nonstop() {
                    true => id,
                    false => format!("{id}-{}", c.index + 1),
                }
            }
            None => self.song.sha256.clone(),
        }
    }

    /// VS: two players here, one per side.
    pub fn is_vs(&self) -> bool {
        matches!(self.vs, Some(Versus::Local))
    }
}

/// The chart's replay `slot` as a G-BATTLE opponent, or why it cannot be
/// one.
pub fn ghost_for(ctx: &Ctx, song: &Song, slot: crate::replay::Slot) -> Result<Arc<Replay>, String> {
    let ln = ctx.config.play.ln_mode;
    let r = crate::replay::load_filed(
        &ctx.paths.root,
        &song.sha256,
        song.score_ln(ln) as i32,
        ctx.config.play.judge_preset,
        song.fixed_ln(),
        slot,
    )
    .ok_or_else(|| {
        let which = match slot {
            crate::replay::Slot::Best => "MY BEST",
            crate::replay::Slot::Last => "LAST",
        };
        format!("G-BATTLE: no {which} replay of {}", song.title)
    })?;
    match crate::replay::ghost_refusal(&r, song, ln) {
        Some(why) => Err(format!("G-BATTLE: {why}")),
        None => Ok(Arc::new(r)),
    }
}

/// Who plays the second side of a battle.
#[derive(Clone, Debug)]
pub enum Versus {
    /// A second player here (VS).
    Local,
    /// G-BATTLE: a replay of the chart (its seed and lane option make the
    /// chart; its inputs play again on the second side), and which.
    Ghost(Arc<Replay>, crate::replay::Slot),
}

/// The single play mode of a double play mode (a battle chart's side).
pub(crate) fn single_mode(mode: PlayMode) -> PlayMode {
    match mode {
        PlayMode::Beat14K => PlayMode::Beat7K,
        PlayMode::Beat10K => PlayMode::Beat5K,
        m => m,
    }
}

/// Why VS cannot be played on `song` here (`None`: it can): a 5 or 7 key
/// chart; with touch controls, a tablet-sized screen (two control sets).
pub fn vs_refusal(ctx: &Ctx, song: &Song) -> Option<&'static str> {
    if !matches!(song.mode, PlayMode::Beat7K | PlayMode::Beat5K) {
        return Some("VS needs a 5 or 7 key chart");
    }
    vs_touch_too_small(ctx).then_some(VS_TOUCH_TOO_SMALL)
}

pub const VS_TOUCH_TOO_SMALL: &str =
    "VS on a touch screen needs a tablet: this screen is too small for two players";

/// Touch controls on a screen too small for two control sets (a phone:
/// its short side under 12 cm).
pub fn vs_touch_too_small(ctx: &Ctx) -> bool {
    let short_mm = ctx.view.width.min(ctx.view.height) / ctx.px_per_mm().max(0.1);
    ctx.touch_enabled() && short_mm < 120.0
}

/// The screen has room for both sides of a battle: not a phone held
/// upright (narrower than 15 cm).
pub(crate) fn battle_skin_fits(ctx: &Ctx) -> bool {
    !(ctx.portrait() && ctx.view.width / ctx.px_per_mm().max(0.1) < 150.0)
}

enum Stage {
    Loading(Box<loading::Loading>),
    Playing,
    /// `ended_ms`: when the play ended (Unix ms); `wall_ms`: the real time
    /// it ran from chart time 0.
    Ending {
        since: Instant,
        failed: bool,
        ended_ms: i64,
        wall_ms: u64,
    },
}

/// What a play is, decided at its setup.
pub(crate) struct Facts {
    /// The mode the play skin is for (G-BATTLE on a phone held upright: the
    /// single play skin, the opponent only in numbers).
    pub skin_mode: PlayMode,
    /// The mode the keys and touch controls play (G-BATTLE: the single
    /// play chart's, the player on the first side).
    pub input_mode: PlayMode,
    /// The LN mode the play is filed under (`LnProfile::score_ln`).
    pub score_ln: LnType,
    /// The TOTAL the gauge uses.
    pub total: f64,
    /// Why this play is not recorded (`None`: it is).
    pub unranked: Option<&'static str>,
    pub options_label: String,
    /// The best play's EX score after each of its judgements (`ghost_ex`),
    /// the pacemaker for MY BEST at the same note.
    pub best_ex: Option<Vec<u32>>,
    /// The chart and the rules its judge was given (the play's record).
    pub chart: ChartFacts,
    pub rules: RuleFacts,
    /// The seed of its `#RANDOM` and lane options.
    pub seed: i64,
    /// The gauges' values a course's stage started from.
    pub gauge_in: Option<Vec<f32>>,
}

pub struct PlayScene {
    pub(crate) req: PlayRequest,
    stage: Stage,
    pub(crate) facts: Facts,
    pub(crate) session: Session,
    audio: sound::Audio,
    bga: bga::Bga,
    controls: control::Controls,
    touch: layout::Touch,
    skin: Option<crate::skin::render::LoadedSkin>,
    /// The skin draws the panel itself (objects on timer 21).
    skin_panel: bool,
    notes_graph: skin::NotesGraph,
    scene_start: Instant,
    /// What a step left for the mixer and the config (kept for its buffer).
    out: Vec<Out>,
    /// Lane events of the turntables let go (kept for its buffer).
    released: Vec<LaneEv>,
    /// Each player's hi-speed as the notes show it.
    hs_view: [hsview::HsView; 2],
    /// A practice play: what starts it again.
    practice: Option<practice::Again>,
}

impl PlayScene {
    pub fn new(ctx: &mut Ctx, req: PlayRequest) -> PlayScene {
        let now = Instant::now();
        PlayScene {
            stage: Stage::Loading(Box::new(loading::Loading::start(ctx, &req))),
            facts: Facts {
                skin_mode: PlayMode::Beat7K,
                input_mode: PlayMode::Beat7K,
                score_ln: LnType::Ln,
                total: 0.0,
                unranked: None,
                options_label: String::new(),
                best_ex: None,
                chart: ChartFacts::default(),
                rules: RuleFacts::default(),
                seed: 0,
                gauge_in: None,
            },
            session: Session::new(Chart::default(), Vec::new(), req.is_vs(), now),
            audio: Default::default(),
            bga: Default::default(),
            controls: Default::default(),
            touch: Default::default(),
            skin: None,
            skin_panel: false,
            notes_graph: Default::default(),
            scene_start: now,
            out: Vec::new(),
            released: Vec::new(),
            hs_view: Default::default(),
            practice: None,
            req,
        }
    }

    /// The chart's BPMs at `now` (FLOATING keeps the green number at the
    /// BPM of the moment).
    pub(crate) fn bpms_at(&self, now: Instant) -> crate::config::Bpms {
        let s = &self.session;
        let bpm = s.chart.timing.bpm_at_time(s.chart_us(now).max(0) as f64);
        crate::config::Bpms::of_meta(&s.chart.meta).at(bpm)
    }

    /// A player's options (VS: the second player's own; a course's view
    /// over them).
    pub(crate) fn side_config<'a>(
        &self,
        p: &'a crate::config::PlayConfig,
        side: usize,
    ) -> std::borrow::Cow<'a, crate::config::PlayConfig> {
        let mut p = match self.session.vs {
            true => std::borrow::Cow::Owned(p.for_side(side)),
            false => std::borrow::Cow::Borrowed(p),
        };
        if let Some(c) = &self.req.course {
            c.view(&mut p);
        }
        p
    }

    /// The hi-speed each player's notes are drawn at.
    pub(crate) fn hs_shown(&self, side: usize, now: Instant) -> f32 {
        self.hs_view[side.min(1)].at(now)
    }

    /// The course stage playing at `now` (NONSTOP: at every join).
    pub(crate) fn course_stage(&self, now: Instant) -> Option<usize> {
        let c = self.req.course.as_ref()?;
        match self.session.stage_at(self.session.chart_us(now)) {
            Some(k) => Some(k),
            None => Some(c.index),
        }
    }

    /// The combo shown for the stage's `combo` on judge `e`: in an expert
    /// or class course the course's, run on from the stages before until
    /// this one breaks it (S-OLR2b 4.2).
    pub(crate) fn course_combo(&self, combo: u32, e: &vibebms_core::engine::Engine) -> u32 {
        match &self.req.course {
            Some(c) if !c.nonstop() && e.score.combo_breaks == 0 => c.totals.combo + combo,
            _ => combo,
        }
    }

    fn ending_at(&self) -> Option<Instant> {
        match self.stage {
            Stage::Ending { since, .. } => Some(since),
            _ => None,
        }
    }

    /// Each side's judge offset (µs; a replay's times have it in them).
    fn offsets(&self, ctx: &Ctx) -> [i64; 2] {
        if self.req.replay.is_some() {
            return [0, 0];
        }
        let p = &ctx.config.play;
        let one = p.judge_offset_ms as i64 * 1000;
        match self.session.vs {
            true => [one, p.p2.judge_offset_ms as i64 * 1000],
            false => [one, one],
        }
    }

    /// A player's key into the judge (`src`: where a press came from).
    fn feed(&mut self, ctx: &Ctx, ev: LaneEv, src: Option<InputSource>) {
        if self.req.replay.is_some() {
            return;
        }
        self.controls.touched |= ev.pressed;
        self.session.key(ev, self.offsets(ctx), src);
    }

    /// What the judges left: sounds to the mixer, the timing auto adjust
    /// to the config.
    fn play_out(&mut self, ctx: &mut Ctx) {
        for o in self.out.drain(..) {
            match o {
                Out::Adjust(d) => {
                    let p = &mut ctx.config.play;
                    if p.timing_auto_adjust {
                        p.judge_offset_ms = (p.judge_offset_ms - d).clamp(-500, 500);
                    }
                }
                o => self.audio.out(&ctx.audio.mixer, &self.session, o),
            }
        }
    }

    /// A perspective skin has the screen's shape: made again when the
    /// screen turns or is resized.
    fn reshape_skin(&mut self, ctx: &Ctx, gfx: &mut Gfx) {
        let Some(sk) = self.skin.as_ref().filter(|s| s.skin.perspective) else {
            return;
        };
        let aspect = ctx.view.width / ctx.view.height.max(1.0);
        if (sk.skin.width / sk.skin.height - aspect).abs() > aspect * 0.01 {
            let new = skin::load(ctx, gfx, self);
            if let Some(mut old) = std::mem::replace(&mut self.skin, new) {
                old.free(gfx);
            }
            self.touch.zones.clear();
        }
    }

    /// The play at `now`: keys let go, sounds scheduled, the judges on;
    /// whether it ended.
    fn play(&mut self, ctx: &mut Ctx, now: Instant) {
        let offsets = self.offsets(ctx);
        let t = self.session.chart_us(now);
        // release turntables that stopped
        let mut released = std::mem::take(&mut self.released);
        if let Some(m) = self.controls.mapper.as_mut() {
            m.tick(now, &mut released);
        }
        for e in released.drain(..) {
            self.feed(ctx, e, None);
        }
        self.released = released;
        self.audio.fade(&ctx.audio.mixer, &self.session, t);
        self.audio.schedule(&ctx.audio.mixer, &self.session, t);
        let skin = self.skin.as_ref().map(|s| &s.skin);
        let show = Show {
            bomb_upto: skin.and_then(|s| s.judge_timer).unwrap_or(Judge::Great),
            three_regions: self.session.mode == PlayMode::Popn9K
                && skin.is_some_and(|s| s.judges[2].judge.iter().any(Option::is_some)),
        };
        let failed = self.session.step(t, now, offsets, show, &mut self.out);
        self.play_out(ctx);
        let quit = self.controls.quit;
        // practice with LOOP: the section again at once
        let repeat = self.req.practice.as_ref().is_some_and(|p| p.repeat);
        if repeat && !quit && (failed || self.session.over(t)) {
            self.practice_again(ctx, now);
            return;
        }
        // the play's end: the wall clock once, the real time it ran
        let ended = || (crate::platform::unix_ms(), t.max(0) as u64 / 1000);
        if failed || quit {
            // (quit: the fade starts halfway)
            let since = if failed {
                now
            } else {
                now - Duration::from_millis(1000)
            };
            let (ended_ms, wall_ms) = ended();
            self.stage = Stage::Ending {
                since,
                failed: true,
                ended_ms,
                wall_ms,
            };
            ctx.audio.mixer.stop_bus(Bus::Bgm);
        } else if self.session.over(t) {
            let (ended_ms, wall_ms) = ended();
            self.stage = Stage::Ending {
                since: now,
                failed: false,
                ended_ms,
                wall_ms,
            };
        }
    }
}

impl Scene for PlayScene {
    fn realtime(&self) -> bool {
        true
    }
    fn screenshot_label(&self, _ctx: &Ctx) -> Vec<String> {
        let s = &self.req.song;
        vec![s.title.clone(), crate::screenshot::difficulty(s.difficulty)]
    }
    fn full_screen(&self) -> bool {
        true
    }

    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        let bpm = self.bpms_at(now);
        self.controls.tick(&mut ctx.config.play, &bpm, now);
        for side in 0..2 {
            let p = self.side_config(&ctx.config.play, side).into_owned();
            self.hs_view[side].update(&p, &bpm, now);
        }
        self.reshape_skin(ctx, gfx);
        // no game events are run from play skin scripts yet
        if let Some(sk) = self.skin.as_mut() {
            sk.script_events();
            sk.load_pending(gfx);
        }
        self.bga.update(gfx, &self.session, now);
        if let Stage::Loading(l) = &mut self.stage {
            match l.poll() {
                None => return Next::Stay,
                Some(Ok(l)) => self.setup(ctx, gfx, l),
                Some(Err(e)) => {
                    ctx.toast(format!("Cannot load chart: {e}"));
                    return Next::Pop;
                }
            }
        }
        match self.stage {
            Stage::Playing => self.play(ctx, now),
            Stage::Ending {
                since,
                failed,
                ended_ms,
                wall_ms,
            } => {
                let wait = match failed {
                    true => self
                        .skin
                        .as_ref()
                        .and_then(|s| s.skin.close_ms)
                        .unwrap_or(1500),
                    false => 1200,
                };
                if now.duration_since(since) > Duration::from_millis(wait as u64) {
                    ctx.audio.mixer.stop_bus(Bus::Bgm);
                    ctx.audio.mixer.stop_bus(Bus::Key);
                    return self.finish(ctx, failed, ended_ms, wall_ms);
                }
            }
            Stage::Loading(_) => {}
        }
        Next::Stay
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        self.on_input(ctx, ev)
    }

    fn exit(&mut self, ctx: &mut Ctx, gfx: &mut Gfx) {
        if let Some(mut sk) = self.skin.take() {
            sk.free(gfx);
        }
        ctx.audio.mixer.stop_bus(Bus::Bgm);
        ctx.audio.mixer.stop_bus(Bus::Key);
        crate::loudness::end_play(&ctx.audio.mixer);
        self.bga.free(gfx);
        ctx.save_config();
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        match &self.stage {
            Stage::Loading(l) => l.draw(ctx, d, &self.req),
            _ if self.session.sides.is_empty() => {}
            _ => self.draw_play(ctx, d, now),
        }
    }
}
