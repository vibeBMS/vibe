//! Gameplay: a chart loaded, judged on one or two sides and drawn with a
//! play skin, with touch controls and the hi-speed panel; the result at
//! the end.

mod bga;
mod control;
mod finish;
mod hud;
mod input;
mod layout;
mod loading;
mod notes;
mod persp;
mod session;
mod setup;
mod skin;
mod sound;
#[cfg(test)]
mod tests;

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
use vibebms_core::judge::GREAT;
use vibebms_core::score::{Assist, ClearType, Score};
use vibebms_core::{LnType, PlayMode};

/// A dan/course in progress.
#[derive(Clone, Debug)]
pub struct CourseRun {
    pub name: String,
    pub songs: Vec<Song>,
    pub index: usize,
    pub constraints: Vec<String>,
    pub gauge_values: Option<Vec<f32>>,
    pub stages: Vec<(String, Score, ClearType)>,
}

impl CourseRun {
    pub fn new(name: String, songs: Vec<Song>, constraints: Vec<String>) -> CourseRun {
        CourseRun {
            name,
            songs,
            index: 0,
            constraints,
            gauge_values: None,
            stages: Vec::new(),
        }
    }
    pub fn has(&self, c: &str) -> bool {
        self.constraints.iter().any(|x| x.eq_ignore_ascii_case(c))
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
}

impl PlayRequest {
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
    let short_mm = ctx.width.min(ctx.height) / ctx.px_per_mm().max(0.1);
    ctx.touch_enabled() && short_mm < 120.0
}

/// The screen has room for both sides of a battle: not a phone held
/// upright (narrower than 15 cm).
pub(crate) fn battle_skin_fits(ctx: &Ctx) -> bool {
    !(ctx.portrait() && ctx.width / ctx.px_per_mm().max(0.1) < 150.0)
}

enum Stage {
    Loading(Box<loading::Loading>),
    Playing,
    Ending { since: Instant, failed: bool },
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
    pub seed: i64,
    pub assist: Assist,
    /// Why this play is not recorded (`None`: it is).
    pub unranked: Option<&'static str>,
    pub options_label: String,
    /// The best play's EX score after each of its judgements (`ghost_ex`),
    /// the pacemaker for MY BEST at the same note.
    pub best_ex: Option<Vec<u32>>,
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
                seed: 0,
                assist: Assist::None,
                unranked: None,
                options_label: String::new(),
                best_ex: None,
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
            req,
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

    /// A player's key into the judge.
    fn feed(&mut self, ctx: &Ctx, ev: LaneEv) {
        if self.req.replay.is_some() {
            return;
        }
        self.controls.touched |= ev.pressed;
        self.session.key(ev, self.offsets(ctx));
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
                o => self.audio.out(&ctx.mixer, &self.session, o),
            }
        }
    }

    /// A perspective skin has the screen's shape: made again when the
    /// screen turns or is resized.
    fn reshape_skin(&mut self, ctx: &Ctx, gfx: &mut Gfx) {
        let Some(sk) = self.skin.as_ref().filter(|s| s.skin.perspective) else {
            return;
        };
        let aspect = ctx.width / ctx.height.max(1.0);
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
            self.feed(ctx, e);
        }
        self.released = released;
        self.audio.schedule(&ctx.mixer, &self.session, t);
        let skin = self.skin.as_ref().map(|s| &s.skin);
        let show = Show {
            bomb_upto: skin.and_then(|s| s.judge_timer).unwrap_or(GREAT),
            three_regions: self.session.mode == PlayMode::Popn9K
                && skin.is_some_and(|s| s.judges[2].judge.iter().any(Option::is_some)),
        };
        let failed = self.session.step(t, now, offsets, show, &mut self.out);
        self.play_out(ctx);
        let quit = self.controls.quit;
        if failed || quit {
            // (quit: the fade starts halfway)
            let since = if failed {
                now
            } else {
                now - Duration::from_millis(1000)
            };
            self.stage = Stage::Ending {
                since,
                failed: true,
            };
            ctx.mixer.stop_bus(Bus::Bgm);
        } else if self.session.over(t) {
            self.stage = Stage::Ending {
                since: now,
                failed: false,
            };
        }
    }
}

impl Scene for PlayScene {
    fn realtime(&self) -> bool {
        true
    }
    fn full_screen(&self) -> bool {
        true
    }

    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        let bpm = crate::config::Bpms::of_meta(&self.session.chart.meta);
        self.controls.tick(&mut ctx.config.play, &bpm, now);
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
            Stage::Ending { since, failed } => {
                let wait = match failed {
                    true => self
                        .skin
                        .as_ref()
                        .and_then(|s| s.skin.close_ms)
                        .unwrap_or(1500),
                    false => 1200,
                };
                if now.duration_since(since) > Duration::from_millis(wait as u64) {
                    ctx.mixer.stop_bus(Bus::Bgm);
                    ctx.mixer.stop_bus(Bus::Key);
                    return self.finish(ctx, failed);
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
        ctx.mixer.stop_bus(Bus::Bgm);
        ctx.mixer.stop_bus(Bus::Key);
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
