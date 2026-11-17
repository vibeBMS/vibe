//! Gameplay: loading, audio scheduling, judging, rendering, touch controls.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::{Bpms, PlayConfig, Side, HISPEED_STEP};
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Gfx, Rect, TexId};
use crate::input::{InputEv, LaneEv, LaneMapper};
use crate::library::Song;
use crate::platform::{Duration, Instant};
use crate::replay::Replay;
use crate::scene::result::{ResultData, ResultScene};
use crate::theme;
use crate::touch::{self, TouchInput, Zone};
use crate::ui;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use vibebms_audio::{Bus, PlayOptions, SoundBank};
use vibebms_core::chart::{BgaLayer, Chart, SoundId, NO_SOUND};
use vibebms_core::engine::{Engine, EngineConfig, EngineEvent, InputEvent, Op};
use vibebms_core::gauge::GrooveGauge;
use vibebms_core::judge::{judge_rules, GOOD, GREAT};
use vibebms_core::lane_option::{self, DoubleOption, LaneOption};
use vibebms_core::score::{Assist, AssistFacts, ClearType, Score};
use vibebms_core::{LnType, PlayMode};
use winit::keyboard::KeyCode;

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

/// The chart's replay `slot` as a G-BATTLE opponent, or why it cannot be
/// one.
pub fn ghost_for(ctx: &Ctx, song: &Song, slot: crate::replay::Slot) -> Result<Arc<Replay>, String> {
    let ln = ctx.config.play.ln_mode;
    let r = Replay::load_filed(
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

type Rgba = (u32, u32, Vec<u8>);

struct Loaded {
    chart: Chart,
    bank: SoundBank,
    images: Vec<Option<Rgba>>,
    /// Chart images that are videos (same indices).
    videos: Vec<Option<PathBuf>>,
    stagefile: Option<Rgba>,
    seed: i64,
    /// What the options did to the chart (for the assist level).
    facts: AssistFacts,
    /// The chart's long notes as written (before options).
    ln_profile: vibebms_core::LnProfile,
}

enum Stage {
    Loading,
    Playing,
    Ending { since: Instant, failed: bool },
}

pub struct PlayScene {
    pub(crate) req: PlayRequest,
    stage: Stage,
    /// Chart/sound loading job, started once the files are available.
    pub(crate) job: Option<Box<dyn FnOnce() + Send>>,
    pub(crate) prefetch: crate::platform::vfs::Prefetch,
    rx: Option<Receiver<Result<Box<Loaded>, String>>>,
    pub(crate) progress: Arc<AtomicUsize>,
    pub(crate) total_files: Arc<AtomicUsize>,
    pub(crate) loading_since: Instant,
    pub(crate) stage_tex: Option<TexId>,
    pub(crate) chart: Chart,
    pub(crate) bank: SoundBank,
    pub(crate) bga_tex: Vec<Option<TexId>>,
    /// Video BGA by chart image, and the chart time (µs) each was first shown.
    bga_video: Vec<Option<crate::video::Video>>,
    bga_start: Vec<Option<i64>>,
    /// The judges: one per side (a single play, BATTLE: one for both).
    pub(crate) sides: Vec<SideState>,
    pub(crate) mapper: Option<LaneMapper>,
    pub(crate) touch: TouchInput,
    pub(crate) zones: Vec<Zone>,
    pub(crate) zones_key: (u32, u32, bool, bool),
    pub(crate) start: Instant,
    pub(crate) bgm_i: usize,
    pub(crate) auto_i: usize,
    pub(crate) bga_i: usize,
    /// Keysounds of auto lanes: (time, sound, side).
    pub(crate) auto_notes: Vec<(i64, SoundId, usize)>,
    pub(crate) bga: [Option<u32>; 4],
    pub(crate) poor_until: Option<Instant>,
    /// The last judge of each judge region (`judge_region`).
    pub(crate) last_judge: [Option<(usize, i64, Instant, u32)>; 3],
    pub(crate) next_sample: i64,
    pub(crate) lane_notes: Vec<Vec<usize>>,
    pub(crate) lane_cursor: Vec<usize>,
    /// The chart's mode (its lanes).
    pub(crate) mode: PlayMode,
    /// The mode the play skin is for (G-BATTLE on a phone held upright: the
    /// single play skin, the opponent only in numbers).
    pub(crate) skin_mode: PlayMode,
    /// The mode the keys and touch controls play (G-BATTLE: the single
    /// play chart's, the player on the first side).
    pub(crate) input_mode: PlayMode,
    pub(crate) ln: LnType,
    /// The LN mode the play is filed under (`LnProfile::score_ln`).
    pub(crate) score_ln: LnType,
    /// The TOTAL the gauge uses.
    pub(crate) total: f64,
    pub(crate) seed: i64,
    pub(crate) error: Option<String>,
    /// The best play's judgements in order (its replay), the pacemaker for
    /// MY BEST at the same note.
    pub(crate) best_ghost: Option<Vec<u8>>,
    /// START / SELECT: the hi-speed panel (VS: each player's).
    ctl: [Control; 2],
    /// VS: two players here, one per side.
    pub(crate) vs: bool,
    /// VS: the first two controllers swapped (the 1P controller pressed
    /// while loading was the second one).
    swap_pads: bool,
    /// The skin draws the panel itself (objects on timer 21).
    skin_panel: bool,
    pub(crate) assist: Assist,
    /// Why this play is not recorded (`None`: it is).
    unranked: Option<&'static str>,
    pub(crate) options_label: String,
    pub(crate) finished_at: Option<i64>,
    pub(crate) key_flash: Vec<Option<Instant>>,
    pub(crate) quit: bool,
    /// A key went to the judge (a play quit before any did is not one).
    touched: bool,
    pub(crate) scene_start: Instant,
    pub(crate) ready_at: Option<Instant>,
    pub(crate) finished_instant: Option<Instant>,
    pub(crate) key_off: Vec<Option<Instant>>,
    pub(crate) bomb_at: Vec<Option<Instant>>,
    /// Last judgement per lane (skin values 500-519).
    pub(crate) lane_judge: Vec<Option<usize>>,
    pub(crate) skin: Option<crate::skin::render::LoadedSkin>,
}

/// Who plays a side.
#[derive(Clone, Debug)]
pub(crate) enum Role {
    /// A person (or autoplay): the keys reach its judge.
    Player,
    /// A replay being watched.
    Replay(Feed),
    /// G-BATTLE's opponent: a replay of the chart played again, silent.
    /// `desync`: its judgements went other than the replay's (another
    /// version judged it): its replay's EX score is shown instead.
    Ghost { feed: Feed, desync: bool },
}

/// Replay ops going into a judge as the chart plays.
#[derive(Clone, Debug)]
pub(crate) struct Feed {
    pub replay: Arc<Replay>,
    /// The next op.
    pub at: usize,
    /// Lanes its inputs move by (a single play's replay on the 2P side).
    pub shift: u8,
}

impl Feed {
    pub fn new(replay: Arc<Replay>, shift: u8) -> Feed {
        Feed {
            replay,
            at: 0,
            shift,
        }
    }

    /// The ops up to chart time `t` into `e` (an input waits in the judge
    /// for the update after it); `lit` sees each input (lane, pressed).
    pub fn run(&mut self, e: &mut Engine, t: i64, mut lit: impl FnMut(u8, bool)) {
        while let Some(op) = self.replay.ops.get(self.at) {
            let op = op.shifted(self.shift);
            match op {
                Op::Update(u) if u > t => break,
                Op::Input(_, lane, _, pressed) => lit(lane, pressed),
                Op::Update(_) => {}
            }
            e.apply(op);
            self.at += 1;
        }
    }
}

/// One side of the play: its judge and what is kept of it.
pub(crate) struct SideState {
    pub engine: Engine,
    pub role: Role,
    /// The gauge every 500 ms (the result's graph).
    pub gauge_log: Vec<f32>,
    /// When its gauge last rose (skin timers 42 / 43), and its value then.
    pub gauge_up: Option<Instant>,
    pub last_gauge: f32,
    /// When its last note was judged (timers 143 / 144).
    pub finished: Option<Instant>,
    /// VS: when the side failed (it plays no more, the other goes on).
    pub failed: Option<Instant>,
}

impl SideState {
    fn new(engine: Engine, role: Role) -> SideState {
        let last_gauge = engine.gauge.value();
        SideState {
            engine,
            role,
            gauge_log: Vec::new(),
            gauge_up: None,
            last_gauge,
            finished: None,
            failed: None,
        }
    }
}

impl SideState {
    fn player(&self) -> bool {
        matches!(self.role, Role::Player)
    }

    fn ghost(&self) -> bool {
        matches!(self.role, Role::Ghost { .. })
    }
}

/// A sound's mixer voice: VS keeps each side's keysounds apart (a side's
/// keysound, or its HCN muting, never cuts the other's).
fn voice(sound: SoundId, side: usize, vs: bool) -> u64 {
    sound as u64 + 1 + if vs { (side as u64) << 32 } else { 0 }
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
    if !matches!(song.mode.as_str(), "7KEYS" | "5KEYS") {
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

/// The gauge graph's samples (every 500 ms) with the gauge as the play
/// ended: a FAILED play's graph reaches 0, a clear's its last value.
fn settled_log(log: &[f32], last: f32) -> Vec<f32> {
    let mut v = log.to_vec();
    if v.last() != Some(&last) {
        v.push(last);
    }
    v
}

fn rgba_of(path: &Path, key: bool) -> Option<Rgba> {
    let p = crate::scene::select::resolve_image(path)?;
    crate::gfx::load_rgba(&p, key)
}

/// beatoraja's controls while playing: with START held the white keys
/// lower the hi-speed and the blue keys raise it (0.25 steps), the
/// turntable moves the lane cover (LIFT when no cover is shown); with
/// SELECT held the keys and turntable change the hi-speed by 0.01. START twice shows / hides the
/// cover; START and SELECT held for a second end the play.
#[derive(Default)]
struct Control {
    start: bool,
    select: bool,
    last_start: Option<Instant>,
    both_since: Option<Instant>,
    /// Turntable held with the panel open: direction and last step.
    scratch: Option<(i32, Instant)>,
    /// SUDDEN+ to bring back with the cover.
    stash: f32,
    /// When the panel opened / last closed (skin timers 21 / 31).
    opened: Option<Instant>,
    closed: Option<Instant>,
}

impl Control {
    fn open(&self) -> bool {
        self.start || self.select
    }

    /// One turntable step in `dir` (START: cover, SELECT: hi-speed).
    fn turn(&self, p: &mut PlayConfig, dir: i32, bpm: &Bpms) {
        if self.start {
            adjust_cover(p, dir as f32 * 0.005);
        } else {
            p.add_hispeed(dir as f32 * 0.01, bpm);
        }
    }
}

/// The lane cover when it shows (or there is no LIFT), else the LIFT.
fn adjust_cover(p: &mut PlayConfig, d: f32) {
    if p.lane_cover || p.sudden > 0.0 || p.lift <= 0.0 {
        p.sudden = (p.sudden + d).clamp(0.0, 0.8);
    } else {
        p.lift = (p.lift - d).clamp(0.0, 0.6);
    }
}

/// The chart a play is judged on: read with `seed` (its `#RANDOM`) and
/// arranged by the play's options, what they did (for the assist level)
/// and its long notes as written. `course`: in a course, whether it
/// allows MIRROR (no other lane option).
pub(crate) fn load_chart(
    path: &Path,
    bytes: &[u8],
    play: &PlayConfig,
    seed: i64,
    course: Option<bool>,
) -> Result<(Chart, AssistFacts, vibebms_core::LnProfile), String> {
    let mut chart = vibebms_core::load_chart_bytes(
        path,
        bytes,
        &vibebms_core::bms::ParseOptions {
            random_seed: Some(seed),
            random_selections: None,
        },
    )
    .map_err(|e| e.to_string())?;
    let ln_profile = chart.ln_profile();
    // chart transforms (options)
    let mut facts = AssistFacts::default();
    if play.battle && !chart.mode().is_double() {
        lane_option::battle(&mut chart);
        facts.battle = true;
    }
    let thr = 60_000_000 / (play.hran_threshold_bpm.max(1) as i64 * 4);
    let allowed = |o: LaneOption| match course {
        Some(mirror) if !(mirror && o == LaneOption::Mirror) => LaneOption::Off,
        _ => o,
    };
    lane_option::apply(&mut chart, 0, allowed(play.random_1p), seed, thr);
    facts.lane.push(allowed(play.random_1p));
    if chart.mode().is_double() {
        lane_option::apply(
            &mut chart,
            1,
            allowed(play.random_2p),
            seed.wrapping_add(1),
            thr,
        );
        facts.lane.push(allowed(play.random_2p));
        if play.dp_option == DoubleOption::Flip {
            lane_option::flip(&mut chart);
        }
    }
    if play.legacy_note {
        facts.legacy_note = chart.meta.has_long;
        lane_option::legacy_note_of(&mut chart, None);
    }
    if play.no_mine {
        let n = chart.notes.len();
        lane_option::remove_mines_of(&mut chart, None);
        facts.no_mine = chart.notes.len() != n;
    }
    Ok((chart, facts, ln_profile))
}

/// G-BATTLE's chart: the single play chart read with the opponent's seed
/// and arranged by its lane option, on both sides (as LR2 lends a ghost's
/// random); each side's LEGACY NOTE and NO MINE apply to its own lanes.
/// What the player's options did (the player's side is a single play).
pub(crate) fn load_battle_chart(
    path: &Path,
    bytes: &[u8],
    mine: &PlayConfig,
    theirs: &PlayConfig,
    seed: i64,
) -> Result<(Chart, AssistFacts, vibebms_core::LnProfile), String> {
    let p = PlayConfig {
        random_1p: theirs.random_1p,
        battle: false,
        legacy_note: false,
        no_mine: false,
        ..mine.clone()
    };
    let (mut chart, mut facts, ln_profile) = load_chart(path, bytes, &p, seed, None)?;
    if chart.mode().is_double() || chart.mode() == PlayMode::Popn9K {
        return Err("G-BATTLE needs a 5 or 7 key chart".into());
    }
    lane_option::battle(&mut chart);
    if mine.legacy_note {
        facts.legacy_note = chart.meta.has_long;
        lane_option::legacy_note_of(&mut chart, Some(0));
    }
    for (player, no_mine) in [(0, mine.no_mine), (1, theirs.no_mine)] {
        if no_mine {
            let n = chart.notes.len();
            lane_option::remove_mines_of(&mut chart, Some(player));
            facts.no_mine |= player == 0 && chart.notes.len() != n;
        }
    }
    Ok((chart, facts, ln_profile))
}

/// What judges a side: the play options' rules, as a course may change
/// them.
#[derive(Clone, Debug)]
pub(crate) struct Rules {
    pub judge: vibebms_core::judge::JudgePreset,
    pub judge_rates: Option<[i64; 3]>,
    pub gauge_preset: vibebms_core::gauge::GaugePreset,
    pub gauge: vibebms_core::gauge::GaugeType,
    pub gas: vibebms_core::gauge::GaugeAutoShift,
    pub ln: LnType,
    pub algorithm: vibebms_core::engine::JudgeAlgorithm,
    pub auto_scratch: bool,
    pub autoplay: bool,
}

impl Rules {
    pub fn of(p: &PlayConfig) -> Rules {
        Rules {
            judge: p.judge_preset,
            judge_rates: p.expand_judge.then_some(p.judge_rates),
            gauge_preset: p.gauge_preset,
            gauge: p.gauge,
            gas: p.gauge_auto_shift,
            ln: p.ln_mode,
            algorithm: p.judge_algorithm,
            auto_scratch: p.auto_scratch,
            autoplay: false,
        }
    }

    /// The lanes played automatically (autoplay, AUTO SCRATCH).
    pub fn auto_lanes(&self, mode: PlayMode) -> Vec<bool> {
        (0..mode.lane_count())
            .map(|l| self.autoplay || (self.auto_scratch && mode.is_scratch(l)))
            .collect()
    }
}

/// The judge of `player`'s lanes (`None`: all) of `chart` under `r`, and
/// the TOTAL its gauge uses.
pub(crate) fn make_engine(chart: &Chart, r: &Rules, player: Option<usize>) -> (Engine, f64) {
    let mode = chart.mode();
    let fam = mode.family();
    let rules = judge_rules(r.judge, fam, chart.meta.judge_rank, r.judge_rates);
    let total = chart.total_of(r.ln, r.gauge_preset, player);
    let notes = chart.total_judgements_of(r.ln, player);
    let gauge = GrooveGauge::new(r.gauge_preset, fam, r.gauge, r.gas, total, notes);
    let cfg = EngineConfig {
        rules,
        default_ln: r.ln,
        algorithm: r.algorithm,
        auto_lanes: r.auto_lanes(mode),
        scratch_release_ends_bss: true,
        player,
    };
    (Engine::new(chart, cfg, gauge), total)
}

impl PlayScene {
    pub fn new(ctx: &mut Ctx, req: PlayRequest) -> PlayScene {
        let progress = Arc::new(AtomicUsize::new(0));
        let total_files = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = channel();
        let path = PathBuf::from(&req.song.path);
        // watching a replay: its options make the chart
        let play = req
            .replay
            .as_ref()
            .map_or_else(|| ctx.config.play.clone(), |r| r.play.clone());
        // G-BATTLE: the opponent's seed and lane option make the chart
        let ghost = match &req.vs {
            Some(Versus::Ghost(g, _)) => Some(g.clone()),
            _ => None,
        };
        let replay_seed = req.replay.as_ref().or(ghost.as_ref()).map(|r| r.seed);
        // VS: the single play chart on both sides, each side's own lane
        // option (RANDOM, RANDOM 2P)
        let play = if matches!(req.vs, Some(Versus::Local)) {
            PlayConfig {
                battle: true,
                dp_option: DoubleOption::Off,
                ..play
            }
        } else {
            play
        };
        let play_ln = play.ln_mode;
        let rate = ctx.mixer.sample_rate;
        let (p2, t2) = (progress.clone(), total_files.clone());
        let course_mirror = req
            .course
            .as_ref()
            .map(|c| c.has("grade_mirror"))
            .unwrap_or(true);
        let in_course = req.course.is_some();
        let song_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let job = move || {
            let r = (|| -> Result<Box<Loaded>, String> {
                let seed = replay_seed.unwrap_or_else(|| {
                    crate::platform::SystemTime::now()
                        .duration_since(crate::platform::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(1)
                });
                let bytes = crate::platform::vfs::read(&path).map_err(|e| e.to_string())?;
                let lanes = in_course.then_some(course_mirror);
                let (chart, facts, ln_profile) = match &ghost {
                    Some(g) => load_battle_chart(&path, &bytes, &play, &g.play, seed)?,
                    None => load_chart(&path, &bytes, &play, seed, lanes)?,
                };
                let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                t2.store(chart.sound_files.len(), Ordering::Relaxed);
                let index = crate::platform::vfs::DirIndex::new(&dir);
                let resolve = |name: &str| index.find(name, crate::platform::vfs::AUDIO_EXTS);
                let read = |p: &Path| crate::platform::vfs::read(p).ok();
                let bank = SoundBank::load_with(&chart.sound_files, rate, &p2, &resolve, &read);
                // an image or a video (shown when ffmpeg is installed)
                let (images, videos) = if play.bga {
                    use crate::platform::vfs::IMAGE_EXTS;
                    use crate::video::{is_video, VIDEO_EXTS};
                    chart
                        .images
                        .iter()
                        .map(|f| match index.find(f, IMAGE_EXTS) {
                            Some(p) if is_video(&p) => (None, Some(p)),
                            Some(p) => (crate::gfx::load_rgba(&p, true), None),
                            None => (None, index.find(f, VIDEO_EXTS)),
                        })
                        .unzip()
                } else {
                    (Vec::new(), Vec::new())
                };
                let stagefile = crate::platform::vfs::inside(&dir, &chart.meta.stagefile)
                    .filter(|_| !chart.meta.stagefile.is_empty())
                    .and_then(|p| rgba_of(&p, false));
                Ok(Box::new(Loaded {
                    chart,
                    bank,
                    images,
                    videos,
                    stagefile,
                    seed,
                    facts,
                    ln_profile,
                }))
            })();
            let _ = tx.send(r);
        };
        let mode = PlayMode::Beat7K;
        let vs = matches!(req.vs, Some(Versus::Local));
        PlayScene {
            req,
            stage: Stage::Loading,
            job: Some(Box::new(job)),
            prefetch: crate::platform::vfs::Prefetch::dirs(&[song_dir]),
            rx: Some(rx),
            progress,
            total_files,
            loading_since: Instant::now(),
            stage_tex: None,
            chart: Chart::default(),
            bank: SoundBank::default(),
            bga_tex: Vec::new(),
            bga_video: Vec::new(),
            bga_start: Vec::new(),
            sides: Vec::new(),
            mapper: None,
            touch: TouchInput::default(),
            zones: Vec::new(),
            zones_key: (0, 0, false, false),
            start: Instant::now(),
            bgm_i: 0,
            auto_i: 0,
            bga_i: 0,
            auto_notes: Vec::new(),
            bga: [None; 4],
            poor_until: None,
            last_judge: [None; 3],
            next_sample: 0,
            lane_notes: Vec::new(),
            lane_cursor: Vec::new(),
            mode,
            skin_mode: mode,
            input_mode: mode,
            ln: play_ln,
            score_ln: LnType::Ln,
            total: 0.0,
            seed: 0,
            error: None,
            best_ghost: None,
            ctl: Default::default(),
            vs,
            swap_pads: false,
            skin_panel: false,
            assist: Assist::None,
            unranked: None,
            options_label: String::new(),
            finished_at: None,
            key_flash: Vec::new(),
            scene_start: Instant::now(),
            ready_at: None,
            finished_instant: None,
            key_off: Vec::new(),
            bomb_at: Vec::new(),
            lane_judge: Vec::new(),
            skin: None,
            quit: false,
            touched: false,
        }
    }

    pub(crate) fn mode(&self) -> PlayMode {
        self.mode
    }

    /// The first side's judge (1P: the scores the skin shows).
    pub(crate) fn engine(&self) -> Option<&Engine> {
        self.sides.first().map(|s| &s.engine)
    }

    /// The judge of a lane.
    pub(crate) fn engine_for(&self, lane: usize) -> Option<&Engine> {
        self.sides
            .iter()
            .map(|s| &s.engine)
            .find(|e| e.judges(lane as u8))
    }

    /// beatoraja judge regions: one per player, or for a PMS skin with a
    /// third judge display, three (keys 1-3, 4-6 and 7-9).
    fn judge_region(&self, lane: usize) -> usize {
        let three = self.mode == PlayMode::Popn9K
            && self
                .skin
                .as_ref()
                .is_some_and(|s| s.skin.judges[2].judge.iter().any(Option::is_some));
        if three {
            (lane / 3).min(2)
        } else {
            self.mode.player_of_lane(lane).min(1)
        }
    }

    pub(crate) fn ending_at(&self) -> Option<Instant> {
        match self.stage {
            Stage::Ending { since, .. } => Some(since),
            _ => None,
        }
    }

    /// BGA layer textures to show now (POOR layer replaces them briefly).
    pub(crate) fn bga_textures(&self, now: Instant) -> Vec<TexId> {
        let poor = self.poor_until.map(|u| now < u).unwrap_or(false) && self.bga[3].is_some();
        let layers: &[usize] = if poor { &[3] } else { &[0, 1, 2] };
        layers
            .iter()
            .filter_map(|l| {
                let i = self.bga[*l]? as usize;
                match self.bga_video.get(i) {
                    Some(Some(v)) => v.texture(),
                    _ => self.bga_tex.get(i).copied().flatten(),
                }
            })
            .collect()
    }

    /// The moment of chart time `t` (µs).
    fn instant_at(&self, t: i64) -> Instant {
        if t >= 0 {
            self.start + Duration::from_micros(t as u64)
        } else {
            self.start - Duration::from_micros(t.unsigned_abs())
        }
    }

    pub(crate) fn chart_us(&self, t: Instant) -> i64 {
        if t >= self.start {
            t.duration_since(self.start).as_micros() as i64
        } else {
            -(self.start.duration_since(t).as_micros() as i64)
        }
    }

    fn setup(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, l: Box<Loaded>) {
        let Loaded {
            chart,
            bank,
            images,
            videos,
            stagefile,
            seed,
            mut facts,
            ln_profile,
        } = *l;
        self.seed = seed;
        self.mode = chart.mode();
        let ghost = match &self.req.vs {
            Some(Versus::Ghost(g, _)) => Some(g.clone()),
            _ => None,
        };
        // G-BATTLE: the player plays the first side as a single play
        self.input_mode = match ghost {
            Some(_) => single_mode(self.mode),
            None => self.mode,
        };
        self.skin_mode = match ghost {
            Some(_) if !battle_skin_fits(ctx) => self.input_mode,
            _ => self.mode,
        };
        let replay_play = self.req.replay.as_ref().map(|r| r.play.clone());
        let p = replay_play.as_ref().unwrap_or(&ctx.config.play);
        let course = self.req.course.as_ref();
        // beatoraja: windows wider than standard and a BPM guide on a chart
        // that changes BPM are assists (only the lamp is kept)
        facts.wide_judge = p.expand_judge && p.judge_rates.iter().any(|r| *r > 100);
        facts.bpm_guide = p.bpm_guide && chart.meta.min_bpm < chart.meta.max_bpm;
        // VS: each side is a single play (with its own lane option)
        if self.vs {
            facts.battle = false;
            facts.lane.truncate(1);
        }
        self.unranked = course
            .map(|_| "COURSE")
            .or(self.req.replay.as_ref().map(|_| "REPLAY"))
            .or(self.vs.then_some("VS"));
        // LN mode: course constraints may force one
        if let Some(c) = course {
            if c.has("ln") {
                self.ln = LnType::Ln;
            } else if c.has("cn") {
                self.ln = LnType::Cn;
            } else if c.has("hcn") {
                self.ln = LnType::Hcn;
            }
        }
        self.score_ln = ln_profile.score_ln(self.ln);
        let mut rules = Rules::of(p);
        rules.ln = self.ln;
        rules.autoplay = self.req.autoplay;
        if course.map(|c| c.has("gauge_lr2")).unwrap_or(false) {
            rules.gauge_preset = vibebms_core::gauge::GaugePreset::Lr2;
        }
        if course.is_some() {
            rules.gauge = p.gauge.to_class();
            rules.gas = vibebms_core::gauge::GaugeAutoShift::None;
        }
        let lanes = self.mode.lane_count();
        // AUTO SCRATCH is an assist when it plays scored notes
        facts.auto_scratch = p.auto_scratch
            && chart
                .notes
                .iter()
                .any(|n| n.is_playable() && self.mode.is_scratch(n.lane as usize));
        self.assist = facts.level();
        // G-BATTLE, VS: the player judges the first side alone
        let side = (ghost.is_some() || self.vs).then_some(0);
        let (mut engine, total) = make_engine(&chart, &rules, side);
        self.total = total;
        if let Some(v) = course.and_then(|c| c.gauge_values.as_ref()) {
            engine.gauge.set_values(v);
        }
        // a play of its own (not a course stage): kept as a replay
        if self.req.replay.is_none() && !self.req.autoplay && course.is_none() && !self.vs {
            engine.record();
        }
        // the best play's ghost: MY BEST's pacemaker at the same note
        if self.req.replay.is_none() {
            self.best_ghost = crate::replay::Replay::load_filed(
                &ctx.paths.root,
                &self.req.song.sha256,
                self.score_ln as i32,
                ln_profile.has_long() && !ln_profile.undefined,
                crate::replay::Slot::Best,
            )
            .filter(|r| r.keepable())
            .map(|r| r.ghost);
        }
        let role = match self.req.replay.clone() {
            Some(r) => Role::Replay(Feed::new(r, 0)),
            None => Role::Player,
        };
        self.sides = vec![SideState::new(engine, role)];
        // VS: the second player with their own options
        if self.vs {
            let mut r = Rules::of(&p.for_side(1));
            r.ln = self.ln;
            r.autoplay = self.req.autoplay;
            let (engine, _) = make_engine(&chart, &r, Some(1));
            self.sides.push(SideState::new(engine, Role::Player));
        }
        // the opponent: its replay's own rules on the second side
        if let Some(g) = &ghost {
            let mut r = Rules::of(&g.play);
            r.autoplay = false;
            let (engine, _) = make_engine(&chart, &r, Some(1));
            let lpp = self.input_mode.lanes_per_player() as u8;
            self.sides.push(SideState::new(
                engine,
                Role::Ghost {
                    feed: Feed::new(g.clone(), lpp),
                    desync: false,
                },
            ));
        }
        // each lane's notes in its judge
        let mut lane_notes = vec![Vec::new(); lanes];
        for side in &self.sides {
            for (i, n) in side.engine.notes().iter().enumerate() {
                lane_notes[n.lane as usize].push(i);
            }
        }
        self.lane_notes = lane_notes;
        self.lane_cursor = vec![0; lanes];
        self.key_flash = vec![None; lanes];
        self.key_off = vec![None; lanes];
        self.bomb_at = vec![None; lanes];
        self.lane_judge = vec![None; lanes];
        // keysounds of auto lanes are scheduled ahead like BGM for exact
        // timing (the players' lanes: an opponent is silent)
        let sides = &self.sides;
        self.auto_notes = chart
            .notes
            .iter()
            .filter(|n| n.is_playable() && n.sound != NO_SOUND)
            .filter_map(|n| {
                let lane = n.lane as usize;
                let k = sides.iter().position(|s| {
                    !s.ghost() && s.engine.judges(n.lane) && s.engine.is_auto(lane)
                })?;
                Some((n.time, n.sound, k))
            })
            .collect();
        let mut mapper = LaneMapper::new(&ctx.config.keys, self.input_mode);
        mapper.swap_pads = self.swap_pads;
        self.mapper = Some(mapper);
        self.bga_tex = images
            .into_iter()
            .map(|i| i.map(|(w, h, px)| gfx.create_texture(w, h, &px)))
            .collect();
        self.bga_video = videos
            .iter()
            .map(|v| {
                v.as_deref()
                    .and_then(|p| crate::video::Video::open(p, false))
            })
            .collect();
        self.bga_start = vec![None; self.bga_video.len()];
        if let Some((w, h, px)) = stagefile {
            self.stage_tex = Some(gfx.create_texture(w, h, &px));
        }
        // (G-BATTLE: the opponent's lane option)
        let random_1p = ghost.as_ref().map_or(p.random_1p, |g| g.play.random_1p);
        let mut opts = vec![random_1p.label().to_string()];
        if self.input_mode.is_double() {
            opts.push(p.random_2p.label().to_string());
        }
        if ghost.is_some() {
            opts.push("G-BATTLE".into());
        }
        if self.vs {
            opts.push("VS".into());
        }
        if p.auto_scratch {
            opts.push("AUTO-SCR".into());
        }
        if p.legacy_note {
            opts.push("LEGACY".into());
        }
        self.options_label = opts.join(" / ");
        // a song from an older library: its note counts and densities
        // (skin numbers 350-365) from the chart
        let song = &mut self.req.song;
        if song.note_kinds.iter().all(|n| *n == 0) {
            (song.note_kinds, song.density) = crate::library::chart_info(&chart);
        }
        self.chart = chart;
        self.bank = bank;
        self.skin = super::play_skin::load_play_skin(ctx, gfx, self);
        // start: a lead-in so the first notes scroll in (the skin's
        // #PLAYSTART, else 1.5 s)
        let lead_ms = self.skin.as_ref().and_then(|s| s.skin.play_start_ms);
        let lead = Duration::from_millis(lead_ms.unwrap_or(1500) as u64);
        let now = Instant::now();
        self.start = now + lead;
        self.stage = Stage::Playing;
        self.ready_at = Some(now);
        self.skin_panel = self
            .skin
            .as_ref()
            .is_some_and(|sk| sk.skin.objects.iter().any(|o| o.dst.timer == 21));
        if let Some(sk) = &self.skin {
            log::info!("skin: {}", sk.skin.name);
        }
    }

    fn voice(&self, sound: SoundId, side: usize) -> u64 {
        voice(sound, side, self.vs)
    }

    fn play_sound(&self, ctx: &Ctx, sound: SoundId, at: Option<i64>, bus: Bus, side: usize) {
        let Some(slice) = self.chart.sounds.get(sound as usize) else {
            return;
        };
        let Some(sample) = self.bank.get(slice.file) else {
            return;
        };
        let m = &ctx.mixer;
        let vol = 1.0;
        m.play(
            sample,
            PlayOptions {
                key: self.voice(sound, side),
                volume: vol,
                bus,
                at: at.map(|t| self.instant_at(t)),
                offset: m.us_to_frames(slice.start).max(0) as usize,
                end: slice.end.map(|e| m.us_to_frames(e).max(0) as usize),
                looping: false,
            },
        );
    }

    fn sound_event(&self, ctx: &Ctx, ev: EngineEvent, side: usize) {
        match ev {
            EngineEvent::Sound { sound, lane } => {
                if !self.sides[side].engine.is_auto(lane as usize) {
                    self.play_sound(ctx, sound, None, Bus::Key, side);
                }
            }
            EngineEvent::Volume { sound, volume } => {
                ctx.mixer.set_volume(self.voice(sound, side), volume)
            }
            _ => {}
        }
    }

    fn schedule_audio(&mut self, ctx: &Ctx, t: i64) {
        let horizon = t + 200_000;
        while let Some(b) = self.chart.bgm.get(self.bgm_i) {
            if b.time > horizon {
                break;
            }
            if b.time >= t - 100_000 {
                self.play_sound(ctx, b.sound, Some(b.time), Bus::Bgm, 0);
            }
            self.bgm_i += 1;
        }
        while let Some(&(time, s, side)) = self.auto_notes.get(self.auto_i) {
            if time > horizon {
                break;
            }
            if time >= t - 100_000 {
                self.play_sound(ctx, s, Some(time), Bus::Key, side);
            }
            self.auto_i += 1;
        }
    }

    /// The sides that replays drive: their ops up to chart time `t`,
    /// the keys they press lit.
    fn run_feeds(&mut self, t: i64, now: Instant) {
        for side in &mut self.sides {
            let (f, desync) = match &mut side.role {
                Role::Player => continue,
                Role::Replay(f) => (f, None),
                Role::Ghost { feed, desync } => (feed, Some(desync)),
            };
            let (flash, off) = (&mut self.key_flash, &mut self.key_off);
            f.run(&mut side.engine, t, |lane, pressed| {
                let lit = if pressed { &mut *flash } else { &mut *off };
                if let Some(f) = lit.get_mut(lane as usize) {
                    *f = Some(now);
                }
            });
            // an opponent that judges other than its replay did
            if let Some(d) = desync.filter(|d| !**d) {
                let (e, r) = (&side.engine, &f.replay);
                let done = f.at >= r.ops.len();
                *d = !r.ghost.starts_with(&e.ghost) || (done && e.score.ex_score() != r.ex_score);
                if *d {
                    log::warn!("G-BATTLE: the opponent's replay went out of step");
                }
            }
        }
    }

    /// The judge offset of a side (µs; a replay's times have it in them).
    fn offset_of(&self, ctx: &Ctx, side: usize) -> i64 {
        let p = &ctx.config.play;
        match side {
            _ if self.req.replay.is_some() => 0,
            1 if self.vs => p.p2.judge_offset_ms as i64 * 1000,
            _ => p.judge_offset_ms as i64 * 1000,
        }
    }

    fn feed(&mut self, ctx: &Ctx, ev: LaneEv) {
        if self.req.replay.is_some() {
            return;
        }
        self.touched |= ev.pressed;
        // the player side judging the lane (one that failed plays no more)
        let Some(k) = self
            .sides
            .iter()
            .position(|s| s.player() && s.engine.judges(ev.lane))
        else {
            return;
        };
        if self.sides[k].failed.is_some() {
            return;
        }
        let t = self.chart_us(ev.time) - self.offset_of(ctx, k);
        if let Some(e) = self.sides.get_mut(k).map(|s| &mut s.engine) {
            e.input(InputEvent {
                time: t,
                lane: ev.lane,
                key: ev.sub,
                pressed: ev.pressed,
            });
        }
        if ev.pressed {
            if let Some(f) = self.key_flash.get_mut(ev.lane as usize) {
                *f = Some(ev.time);
            }
        } else if let Some(f) = self.key_off.get_mut(ev.lane as usize) {
            *f = Some(ev.time);
        }
    }

    /// The skin draws the lanes in perspective: they are the touch
    /// controls (no strip of controls under the play screen).
    pub(crate) fn perspective(&self) -> bool {
        self.skin.as_ref().is_some_and(|s| s.skin.perspective)
    }

    /// The perspective skin's fans on the screen when it is drawn in `view`.
    pub(crate) fn persp_fans(&self, ctx: &Ctx, view: Rect) -> Vec<crate::perspective::Fan> {
        let Some(sk) = self.skin.as_ref() else {
            return Vec::new();
        };
        let xf = crate::skin::render::Xf::fit(sk.skin.width, sk.skin.height, view);
        let side2 = ctx.config.play.side == Side::P2;
        crate::perspective::layout(
            self.mode,
            side2,
            sk.skin.width,
            sk.skin.height,
            &ctx.config.perspective,
        )
        .iter()
        .map(|f| f.transformed(xf.sx, xf.ox, xf.oy))
        .collect()
    }

    /// The top of the perspective lanes' touch area: the share of the
    /// screen set, at least a little above the judge line.
    pub(crate) fn persp_touch_top(
        &self,
        ctx: &Ctx,
        fans: &[crate::perspective::Fan],
        view: Rect,
    ) -> f32 {
        let (lo, hi) = crate::config::TOUCH_HEIGHT_RANGE;
        let share = ctx.config.perspective.touch_height.clamp(lo, hi);
        let judge = crate::perspective::Depths::of(&ctx.config.play).judge;
        let line = fans
            .iter()
            .map(|f| f.y(judge))
            .fold(view.bottom(), f32::min);
        (view.bottom() - share * view.h).min(line - 0.03 * view.h)
    }

    /// Screen areas of the touch controls: (rect, player, scratch on the right).
    fn touch_areas(&self, ctx: &Ctx) -> Vec<(Rect, usize, bool)> {
        if !ctx.touch_enabled() || self.perspective() {
            return Vec::new();
        }
        let t = &self.touch_config(ctx);
        let area = touch::controls_area(ctx.width, ctx.height, t);
        let side2 = ctx.config.play.side == Side::P2;
        touch::player_areas(self.input_mode, area, t, side2, ctx.px_per_mm())
    }

    /// The touch settings for this play (VS: two control sets).
    fn touch_config(&self, ctx: &Ctx) -> crate::config::TouchConfig {
        let mut t = ctx.config.touch.clone();
        t.allow_dp |= self.vs;
        t
    }

    /// Where the play skin goes (above the touch controls) and, with touch
    /// controls, the START / SELECT buttons beside it.
    fn play_view(
        &self,
        ctx: &Ctx,
        areas: &[(Rect, usize, bool)],
    ) -> (Rect, Option<Vec<touch::Zone>>) {
        let screen = Rect::new(
            0.0,
            0.0,
            ctx.width,
            areas.first().map(|t| t.0.y).unwrap_or(ctx.height),
        );
        if self.perspective() {
            // START / SELECT in the top corner the skin leaves for them
            let Some(sk) = self.skin.as_ref().filter(|_| ctx.touch_enabled()) else {
                return (screen, None);
            };
            let xf = crate::skin::render::Xf::fit(sk.skin.width, sk.skin.height, screen);
            let side2 = ctx.config.play.side == Side::P2;
            let right =
                touch::system_right(&ctx.config.touch, self.input_mode, side2, ctx.portrait());
            let b = crate::perspective::system_buttons(sk.skin.width, sk.skin.height, right);
            let zone = |lane, [x, y, w, h]: [f32; 4]| touch::Zone {
                shape: touch::Shape::Rect(xf.rect(Rect::new(x, y, w, h))),
                lane,
                scratch: false,
                black: false,
                color: None,
            };
            return (
                screen,
                Some(vec![zone(touch::START, b[0]), zone(touch::SELECT, b[1])]),
            );
        }
        if areas.is_empty() {
            return (screen, None);
        }
        let aspect = self
            .skin
            .as_ref()
            .map(|s| s.skin.width / s.skin.height.max(1.0))
            .unwrap_or(16.0 / 9.0);
        if self.vs {
            // VS: 1P's buttons on the left, 2P's on the right
            let mm = ctx.px_per_mm();
            let (view, one) = touch::system_buttons(screen, aspect, mm, false);
            let (view, mut two) = touch::system_buttons(view, aspect, mm, true);
            two[0].lane = touch::START2;
            two[1].lane = touch::SELECT2;
            return (view, Some(one.into_iter().chain(two).collect()));
        }
        let side2 = ctx.config.play.side == Side::P2;
        let right = touch::system_right(&ctx.config.touch, self.input_mode, side2, ctx.portrait());
        let (view, buttons) = touch::system_buttons(screen, aspect, ctx.px_per_mm(), right);
        (view, Some(buttons.to_vec()))
    }

    fn rebuild_zones(&mut self, ctx: &Ctx, areas: &[(Rect, usize, bool)]) {
        let key = (
            ctx.width as u32,
            ctx.height as u32,
            ctx.touch_enabled(),
            self.skin.is_some(),
        );
        // the perspective lanes follow the settings (LIFT, the touch area)
        if key == self.zones_key && !self.zones.is_empty() && !self.perspective() {
            return;
        }
        self.zones_key = key;
        self.zones.clear();
        if self.perspective() && ctx.touch_enabled() {
            let view = Rect::new(0.0, 0.0, ctx.width, ctx.height);
            let fans = self.persp_fans(ctx, view);
            let top = self.persp_touch_top(ctx, &fans, view);
            self.zones = touch::fan_zones(self.input_mode, &fans, top);
        }
        if !areas.is_empty() {
            self.zones = touch::zones(
                self.input_mode,
                &self.touch_config(ctx),
                touch::controls_area(ctx.width, ctx.height, &ctx.config.touch),
                ctx.config.play.side == Side::P2,
                ctx.portrait(),
                ctx.px_per_mm(),
            );
        }
        if let (_, Some(buttons)) = self.play_view(ctx, areas) {
            self.zones.extend(buttons);
        }
    }

    fn finish(&mut self, ctx: &mut Ctx, failed: bool) -> Next {
        let Some(side) = self.sides.first() else {
            return Next::Pop;
        };
        let e = &side.engine;
        // quit (START + SELECT, Esc) before a key was played: no play to
        // record or count, no results; back to song select
        if self.quit && !self.touched {
            return Next::Pop;
        }
        let gauge = &e.gauge;
        let gauge_clear = if failed
            || e.is_failed() && gauge.auto_shift != vibebms_core::gauge::GaugeAutoShift::Continue
        {
            None
        } else {
            gauge.final_clear_gauge().map(|g| g.clear_type())
        };
        let mut score = e.score.clone();
        let clear = if self.quit {
            ClearType::Failed
        } else {
            score.clear_type(gauge_clear, self.assist)
        };
        if self.quit {
            score.passed = score.passed.min(score.total_notes);
        }
        let mut course = self.req.course.clone();
        if let Some(c) = course.as_mut() {
            c.gauge_values = Some(gauge.values());
            c.stages
                .push((self.chart.meta.title.clone(), score.clone(), clear));
        }
        // a battle's sides are single plays
        let played_mode = if self.sides.len() > 1 {
            single_mode(self.mode)
        } else {
            self.mode
        };
        let data = ResultData {
            request: PlayRequest {
                course: None,
                replay: None,
                ..self.req.clone()
            },
            title: self.chart.meta.title.clone(),
            subtitle: self.chart.meta.subtitle.clone(),
            artist: self.chart.meta.artist.clone(),
            level: self.chart.meta.level.clone(),
            mode: played_mode,
            ln: self.score_ln,
            score,
            clear,
            gauge_type: gauge.current_type(),
            gauge_value: gauge.value(),
            gauge_log: settled_log(&side.gauge_log, gauge.value()),
            timing_log: e.timing_log.clone(),
            windows: e.windows().note.clone(),
            judge_log: e.judge_log.clone(),
            notes_graph: super::graphs::columns(e.notes(), played_mode, self.chart.length()),
            meta: self.chart.meta.clone(),
            length_us: self.chart.length(),
            bpm_log: self
                .chart
                .timing
                .bpm_changes()
                .into_iter()
                .map(|c| (c.time, c.bpm))
                .collect(),
            autoplay: self.req.autoplay,
            unranked: self.unranked,
            assist: self.assist,
            options: self.options_label.clone(),
            judge: format!(
                "{} / {}",
                ctx.config.play.judge_preset.label(),
                ctx.config.play.gauge_preset.label()
            ),
            seed: self.seed,
            course,
            p2: self.opponent_result(),
            replay: (!e.recorded().is_empty()).then(|| Replay {
                version: crate::replay::VERSION,
                sha256: self.req.song.sha256.clone(),
                md5: self.req.song.md5.clone(),
                title: self.chart.meta.title.clone(),
                ln: self.score_ln as i32,
                date: crate::platform::unix_now() as i64,
                seed: self.seed,
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
    fn opponent_result(&self) -> Option<crate::scene::result::SideResult> {
        let side = self.sides.get(1)?;
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
        let gauge_clear =
            if e.is_failed() && gauge.auto_shift != vibebms_core::gauge::GaugeAutoShift::Continue {
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
        Some(crate::scene::result::SideResult {
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

    /// The last judgement of a judge region as the skin shows it: a
    /// single play skin over a battle (a phone held upright) has the
    /// player's alone, never the opponent's.
    pub(crate) fn last_judge(&self, region: usize) -> Option<(usize, i64, Instant, u32)> {
        if region > 0 && self.skin_mode.players() < self.mode.players() {
            return None;
        }
        self.last_judge[region.min(2)]
    }

    /// The opponent's EX score now: its judge's, or once a G-BATTLE
    /// replay went out of step, its replay's at the player's note.
    pub(crate) fn opponent_ex(&self) -> Option<i64> {
        let side = self.sides.get(1)?;
        Some(match &side.role {
            Role::Ghost { feed, desync: true } => {
                let passed = self.engine().map_or(0, |e| e.score.passed as usize);
                feed.replay.ex_at(passed) as i64
            }
            _ => side.engine.score.ex_score() as i64,
        })
    }

    /// A battle's second side (G-BATTLE's opponent, VS's 2P).
    pub(crate) fn opponent(&self) -> Option<&SideState> {
        self.sides.get(1)
    }

    /// A side for skin values: 2P is the opponent, else the one judge.
    pub(crate) fn side(&self, side: usize) -> Option<&SideState> {
        self.sides.get(side).or(self.sides.first())
    }

    /// G-BATTLE's opponent went out of step with its replay.
    pub(crate) fn ghost_desync(&self) -> bool {
        self.sides
            .iter()
            .any(|s| matches!(s.role, Role::Ghost { desync: true, .. }))
    }

    /// A replay played again here (watched, or G-BATTLE's opponent) was
    /// recorded with an older judge.
    fn older_replay(&self) -> bool {
        self.sides.iter().any(|s| match &s.role {
            Role::Replay(f) | Role::Ghost { feed: f, .. } => f.replay.older_engine(),
            Role::Player => false,
        })
    }

    /// The player side a key or button belongs to (VS: each player's
    /// START / SELECT and keys; else the one player's).
    fn control_side(&self, lane: u8) -> usize {
        if !self.vs {
            return 0;
        }
        match lane {
            touch::START | touch::SELECT => 0,
            touch::START2 | touch::SELECT2 => 1,
            l => self.mode.player_of_lane(l as usize).min(1),
        }
    }

    /// START / SELECT and what they do to the keys: `true` when the event
    /// was taken (it does not reach the judge).
    fn control(&mut self, ctx: &mut Ctx, e: &LaneEv) -> bool {
        // the second player's START / SELECT do nothing out of VS
        if !self.vs && matches!(e.lane, touch::START2 | touch::SELECT2) {
            return true;
        }
        let bpm = &Bpms::of_meta(&self.chart.meta);
        let side = self.control_side(e.lane);
        // the side's own options (VS: the second player's)
        let mut p = ctx.config.play.for_side(side);
        let taken = control(&mut self.ctl[side], self.mode, &mut p, bpm, e);
        if taken {
            ctx.config.play.set_side(side, &p);
        }
        taken
    }

    /// When the hi-speed panel opened and last closed (skin timers 21 / 31).
    pub(crate) fn panel_timers(&self) -> (Option<Instant>, Option<Instant>) {
        (self.ctl[0].opened, self.ctl[0].closed)
    }

    /// The hi-speed / lane cover panel is up (START or SELECT held).
    pub(crate) fn panel_open(&self) -> bool {
        self.ctl[0].open()
    }
}

/// What START / SELECT and the keys do to `p` with them held (one player's
/// `Control`): `true` when the event was taken.
fn control(c: &mut Control, mode: PlayMode, p: &mut PlayConfig, bpm: &Bpms, e: &LaneEv) -> bool {
    let was_open = c.open();
    match e.lane {
        touch::START | touch::START2 => {
            c.start = e.pressed;
            if e.pressed {
                let twice = c
                    .last_start
                    .is_some_and(|t| e.time.duration_since(t) < Duration::from_millis(500));
                if twice {
                    // show / hide the lane cover
                    if p.lane_cover || p.sudden > 0.0 {
                        c.stash = p.sudden;
                        p.lane_cover = false;
                        p.sudden = 0.0;
                    } else {
                        p.lane_cover = true;
                        p.sudden = if c.stash > 0.0 { c.stash } else { 0.2 };
                    }
                }
                c.last_start = (!twice).then_some(e.time);
            }
        }
        touch::SELECT | touch::SELECT2 => c.select = e.pressed,
        _ if !c.open() => return false,
        // releases go on to the judge (the key may have been down first)
        _ if !e.pressed => {
            if mode.is_scratch(e.lane as usize) {
                c.scratch = None;
            }
            return false;
        }
        lane if mode.is_scratch(lane as usize) => {
            let dir = if e.sub == 0 { -1 } else { 1 };
            c.turn(p, dir, bpm);
            c.scratch = Some((dir, e.time));
        }
        lane => {
            let blue = lane as usize % mode.lanes_per_player() % 2 == 1;
            // START: 0.25 steps, SELECT: 0.01; white slower, black faster
            let step = if c.start { HISPEED_STEP } else { 0.01 };
            p.add_hispeed(if blue { step } else { -step }, bpm);
        }
    }
    c.both_since = (c.start && c.select).then(|| c.both_since.unwrap_or(e.time));
    match (was_open, c.open()) {
        (false, true) => (c.opened, c.closed) = (Some(e.time), None),
        (true, false) => (c.opened, c.closed) = (None, Some(e.time)),
        _ => {}
    }
    true
}

impl Scene for PlayScene {
    fn realtime(&self) -> bool {
        true
    }
    fn full_screen(&self) -> bool {
        true
    }

    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        // the turntable keeps turning the panel's value while held
        for side in 0..2 {
            let c = &mut self.ctl[side];
            if let Some((dir, last)) = c.scratch.filter(|_| c.open()) {
                if now.duration_since(last) >= Duration::from_millis(50) {
                    let mut p = ctx.config.play.for_side(side);
                    c.turn(&mut p, dir, &Bpms::of_meta(&self.chart.meta));
                    c.scratch = Some((dir, now));
                    ctx.config.play.set_side(side, &p);
                }
            }
        }
        // START + SELECT held for a second (either player's): quit
        if self.ctl.iter().any(|c| {
            c.both_since
                .is_some_and(|t| now.duration_since(t) >= Duration::from_secs(1))
        }) {
            self.quit = true;
        }
        // a perspective skin has the screen's shape: made again when the
        // screen turns or is resized
        if let Some(sk) = self.skin.as_ref().filter(|s| s.skin.perspective) {
            let aspect = ctx.width / ctx.height.max(1.0);
            if (sk.skin.width / sk.skin.height - aspect).abs() > aspect * 0.01 {
                let new = super::play_skin::load_play_skin(ctx, gfx, self);
                if let Some(mut old) = std::mem::replace(&mut self.skin, new) {
                    old.free(gfx);
                }
                self.zones.clear();
            }
        }
        // no game events are run from play skin scripts yet
        if let Some(sk) = self.skin.as_mut() {
            sk.script_events();
            sk.load_pending(gfx);
        }
        if !self.bga_video.is_empty() {
            let t = self.chart_us(now);
            for (v, s) in self.bga_video.iter_mut().zip(&self.bga_start) {
                if let (Some(v), Some(s)) = (v, s) {
                    v.update(gfx, (t - s) / 1000);
                }
            }
        }
        if self.job.is_some() && self.prefetch.finished() {
            if let Some(job) = self.job.take() {
                crate::platform::spawn("load", job);
            }
        }
        if let Some(rx) = &self.rx {
            if let Ok(r) = rx.try_recv() {
                self.rx = None;
                match r {
                    Ok(l) => self.setup(ctx, gfx, l),
                    Err(e) => {
                        ctx.toast(format!("Cannot load chart: {e}"));
                        self.error = Some(e);
                        return Next::Pop;
                    }
                }
            }
        }
        match self.stage {
            Stage::Loading => return Next::Stay,
            Stage::Ending { since, failed } => {
                if now.duration_since(since)
                    > Duration::from_millis(if failed {
                        self.skin
                            .as_ref()
                            .and_then(|s| s.skin.close_ms)
                            .unwrap_or(1500) as u64
                    } else {
                        1200
                    })
                {
                    ctx.mixer.stop_bus(Bus::Bgm);
                    ctx.mixer.stop_bus(Bus::Key);
                    return self.finish(ctx, failed);
                }
                return Next::Stay;
            }
            Stage::Playing => {}
        }
        // each side's judge offset (a replay's times have it in them)
        let offsets: Vec<i64> = (0..self.sides.len())
            .map(|k| self.offset_of(ctx, k))
            .collect();
        let t = self.chart_us(now);
        // release turntables that stopped
        let mut evs = Vec::new();
        if let Some(m) = self.mapper.as_mut() {
            m.tick(now, &mut evs);
        }
        for e in evs {
            self.feed(ctx, e);
        }
        self.schedule_audio(ctx, t);
        self.run_feeds(t, now);
        // (side, event): a side whose players all failed ends the play
        let mut events: Vec<(usize, EngineEvent)> = Vec::new();
        let sample = t >= self.next_sample;
        let mut finished = true;
        for (k, side) in self.sides.iter_mut().enumerate() {
            let (player, ghost) = (side.player(), side.ghost());
            let offset = offsets[k];
            let out = side.failed.is_some();
            let engine = &mut side.engine;
            // (a side that failed in VS plays no more)
            if player && !out {
                engine.update(t - offset);
            }
            events.extend(engine.drain_events().map(|e| (k, e)));
            if sample {
                side.gauge_log.push(engine.gauge.value());
            }
            let g = engine.gauge.value();
            if g > side.last_gauge {
                side.gauge_up = Some(now);
            }
            side.last_gauge = g;
            let done = engine.is_finished(t - offset);
            if done && side.finished.is_none() {
                side.finished = Some(now);
            }
            finished &= ghost || out || done;
        }
        if sample {
            // every 500 ms of play time, like beatoraja
            self.next_sample = t + 500_000;
        }
        if !self.sides.is_empty() && finished && self.finished_at.is_none() {
            self.finished_at = Some(t);
            self.finished_instant = Some(now);
        }
        let mut failed = false;
        for (k, ev) in events {
            let ghost = self.sides[k].ghost();
            match ev {
                // the opponent is silent, its failing ends nothing
                EngineEvent::Sound { .. } | EngineEvent::Volume { .. } | EngineEvent::Failed
                    if ghost => {}
                EngineEvent::Sound { .. } | EngineEvent::Volume { .. } => {
                    self.sound_event(ctx, ev, k)
                }
                // VS: a player who fails stops; the play ends when both have
                EngineEvent::Failed if self.vs => {
                    self.sides[k].failed.get_or_insert(now);
                    failed = self
                        .sides
                        .iter()
                        .filter(|s| s.player())
                        .all(|s| s.failed.is_some());
                }
                EngineEvent::Judge {
                    lane,
                    judge,
                    diff,
                    combo,
                    ..
                } => {
                    let region = self.judge_region(lane as usize);
                    self.last_judge[region] = Some((judge, diff, now, combo));
                    // beatoraja timing auto adjust: PG..GOOD within 150 ms
                    // move the timing a ms per 30 ms off (diff > 0: early)
                    let p = &mut ctx.config.play;
                    if p.timing_auto_adjust
                        && k == 0
                        && !self.sides[k].engine.is_auto(lane as usize)
                        && judge <= GOOD
                        && diff.abs() <= 150_000
                    {
                        let d = (diff + if diff >= 0 { 15_000 } else { -15_000 }) / 30_000;
                        p.judge_offset_ms = (p.judge_offset_ms - d as i32).clamp(-500, 500);
                    }
                    if let Some(j) = self.lane_judge.get_mut(lane as usize) {
                        *j = Some(judge);
                    }
                    // the skin's bomb timers (#JUDGETIMER: up to which judge)
                    let bomb_upto = self
                        .skin
                        .as_ref()
                        .and_then(|s| s.skin.judge_timer)
                        .unwrap_or(GREAT);
                    if judge <= bomb_upto {
                        if let Some(b) = self.bomb_at.get_mut(lane as usize) {
                            *b = Some(now);
                        }
                    }
                    // the POOR BGA: a judgement that leaves no combo
                    // (beatoraja; an empty POOR in 7KEYS keeps the combo)
                    if combo == 0 && !ghost {
                        self.poor_until = Some(now + Duration::from_millis(500));
                    }
                }
                EngineEvent::Mine { .. } => {}
                EngineEvent::Failed => failed = true,
            }
        }
        // BGA
        while let Some(b) = self.chart.bga.get(self.bga_i) {
            if b.time > t {
                break;
            }
            let i = match b.layer {
                BgaLayer::Base => 0,
                BgaLayer::Layer => 1,
                BgaLayer::Layer2 => 2,
                BgaLayer::Poor => 3,
            };
            self.bga[i] = b.image;
            // a video plays from the first event showing it
            if let Some(s) = b.image.and_then(|m| self.bga_start.get_mut(m as usize)) {
                s.get_or_insert(b.time);
            }
            self.bga_i += 1;
        }
        if failed {
            self.stage = Stage::Ending {
                since: now,
                failed: true,
            };
            ctx.mixer.stop_bus(Bus::Bgm);
        } else if self.quit {
            self.stage = Stage::Ending {
                since: now - Duration::from_millis(1000),
                failed: true,
            };
            ctx.mixer.stop_bus(Bus::Bgm);
        } else if let Some(ft) = self.finished_at {
            let tail = self
                .chart
                .bgm
                .last()
                .map(|b| b.time)
                .unwrap_or(0)
                .max(self.chart.length());
            if t > ft + 1_000_000 && t > tail + 500_000 {
                self.stage = Stage::Ending {
                    since: now,
                    failed: false,
                };
            }
        }
        Next::Stay
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        let mut lane_evs = Vec::new();
        match ev {
            InputEv::Key {
                code,
                pressed,
                repeat,
                time,
                ..
            } => {
                if *repeat {
                    return Next::Stay;
                }
                if let Some(m) = &self.mapper {
                    if let Some((lane, sub)) = m.key(*code) {
                        lane_evs.push(LaneEv {
                            lane,
                            sub,
                            pressed: *pressed,
                            time: *time,
                        });
                    }
                }
                if lane_evs.is_empty() && *pressed {
                    match code {
                        KeyCode::Escape => {
                            if matches!(self.stage, Stage::Loading) {
                                return Next::Pop;
                            }
                            self.quit = true;
                        }
                        KeyCode::ArrowUp => ctx
                            .config
                            .play
                            .add_hispeed(HISPEED_STEP, &Bpms::of_meta(&self.chart.meta)),
                        KeyCode::ArrowDown => ctx
                            .config
                            .play
                            .add_hispeed(-HISPEED_STEP, &Bpms::of_meta(&self.chart.meta)),
                        KeyCode::PageUp => adjust_cover(&mut ctx.config.play, 0.02),
                        KeyCode::PageDown => adjust_cover(&mut ctx.config.play, -0.02),
                        KeyCode::ArrowLeft => ctx.config.play.judge_offset_ms -= 1,
                        KeyCode::ArrowRight => ctx.config.play.judge_offset_ms += 1,
                        _ => {}
                    }
                }
            }
            InputEv::Pad { id, pressed, time } => {
                // VS: a button pressed while loading picks the 1P controller
                if self.vs && *pressed && matches!(self.stage, Stage::Loading) && *id < 2_000_000 {
                    let swap = *id >= 1_000_000;
                    if swap != self.swap_pads {
                        self.swap_pads = swap;
                        ctx.toast(format!("1P: controller {}", swap as u8 + 1));
                    }
                    return Next::Stay;
                }
                if let Some((lane, sub)) = self.mapper.as_ref().and_then(|m| m.pad(*id)) {
                    lane_evs.push(LaneEv {
                        lane,
                        sub,
                        pressed: *pressed,
                        time: *time,
                    });
                }
            }
            InputEv::Axis { id, value, time } => {
                if let Some(m) = self.mapper.as_mut() {
                    m.axis(*id, *value, *time, &mut lane_evs);
                }
            }
            InputEv::Pointer {
                id,
                phase,
                x,
                y,
                time,
                ..
            } => {
                let thr = ctx.config.touch.scratch_threshold_deg;
                if ctx.touch_enabled() {
                    self.touch
                        .pointer(&self.zones, *id, *phase, *x, *y, *time, thr, &mut lane_evs);
                }
            }
            // releases that never came: let go of what is held
            InputEv::Released { time, touch_only } => {
                self.touch.release_all(*time, &mut lane_evs);
                if !touch_only {
                    let held: Vec<(u8, u8)> = self
                        .sides
                        .iter()
                        .filter(|s| s.player())
                        .flat_map(|s| s.engine.held())
                        .collect();
                    for (lane, sub) in held {
                        if !lane_evs.iter().any(|e| e.lane == lane && e.sub == sub) {
                            lane_evs.push(LaneEv {
                                lane,
                                sub,
                                pressed: false,
                                time: *time,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        let mut fed = false;
        for e in lane_evs {
            if !matches!(self.stage, Stage::Loading) && self.control(ctx, &e) {
                continue;
            }
            if matches!(self.stage, Stage::Playing) {
                self.feed(ctx, e);
                fed = true;
            }
        }
        if fed && self.req.replay.is_none() {
            // judge now and start the keys' sounds without waiting for the
            // next frame; the rest of the events wait for it
            let t = self.chart_us(Instant::now());
            let offsets: Vec<i64> = (0..self.sides.len())
                .map(|k| self.offset_of(ctx, k))
                .collect();
            let mut sounds = Vec::new();
            for (k, side) in self.sides.iter_mut().enumerate() {
                if !side.player() || side.failed.is_some() {
                    continue;
                }
                let e = &mut side.engine;
                e.update(t - offsets[k]);
                sounds.push((k, e.take_sounds()));
            }
            for (k, evs) in sounds {
                for ev in evs {
                    self.sound_event(ctx, ev, k);
                }
            }
        }
        Next::Stay
    }

    fn exit(&mut self, ctx: &mut Ctx, gfx: &mut Gfx) {
        if let Some(mut sk) = self.skin.take() {
            sk.free(gfx);
        }
        ctx.mixer.stop_bus(Bus::Bgm);
        ctx.mixer.stop_bus(Bus::Key);
        for t in self.bga_tex.drain(..).flatten() {
            gfx.free_texture(t);
        }
        for mut v in self.bga_video.drain(..).flatten() {
            v.free(gfx);
        }
        if let Some(t) = self.stage_tex.take() {
            gfx.free_texture(t);
        }
        ctx.save_config();
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let ui = ctx.ui();
        if let Stage::Loading = self.stage {
            let (w, h) = (ctx.width, ctx.height);
            if let Some(t) = self.stage_tex {
                d.image_cover(t, Rect::new(0.0, 0.0, w, h), 0.5);
            }
            let s = &self.req.song;
            d.text_fit(
                &s.genre,
                w * 0.5,
                h * 0.3,
                18.0 * ui,
                theme::TEXT_DIM,
                Align::Center,
                w * 0.9,
            );
            d.text_fit(
                &s.full_title(),
                w * 0.5,
                h * 0.36,
                34.0 * ui,
                theme::TEXT,
                Align::Center,
                w * 0.9,
            );
            d.text_fit(
                &s.artist,
                w * 0.5,
                h * 0.36 + 48.0 * ui,
                20.0 * ui,
                theme::TEXT_DIM,
                Align::Center,
                w * 0.9,
            );
            if self.vs {
                // which controller is 1P (a button on it now picks it)
                let pads = if ctx.config.keys.p1.keys.iter().any(|k| !k.pad.is_empty()) {
                    "  ·  press a button on the 1P controller"
                } else {
                    ""
                };
                d.text_fit(
                    &format!("VS  ·  1P left, 2P right{pads}"),
                    w * 0.5,
                    h * 0.22,
                    20.0 * ui,
                    theme::ACCENT,
                    Align::Center,
                    w * 0.9,
                );
            }
            if let Some(c) = &self.req.course {
                d.text(
                    &format!("{}  STAGE {}/{}", c.name, c.index + 1, c.songs.len()),
                    w * 0.5,
                    h * 0.22,
                    22.0 * ui,
                    theme::ACCENT,
                    Align::Center,
                );
            }
            let total = self.total_files.load(Ordering::Relaxed).max(1);
            let done = self.progress.load(Ordering::Relaxed);
            let bar = Rect::new(w * 0.25, h * 0.62, w * 0.5, 6.0 * ui);
            d.b.rect(bar, with_alpha(theme::TEXT_DIM, 0.3));
            d.b.rect(
                Rect::new(
                    bar.x,
                    bar.y,
                    bar.w * (done as f32 / total as f32).min(1.0),
                    bar.h,
                ),
                theme::ACCENT,
            );
            let dots = ".".repeat((self.loading_since.elapsed().as_millis() / 300 % 4) as usize);
            d.text(
                &format!("LOADING{dots}  {done}/{total}"),
                w * 0.5,
                bar.y + 16.0 * ui,
                16.0 * ui,
                theme::TEXT_DIM,
                Align::Center,
            );
            return;
        }
        if self.sides.is_empty() {
            return;
        }
        self.draw_skinned(ctx, d, now);
    }
}

impl PlayScene {
    /// The skin's note fields on screen (one per player).
    fn fields(&self, view: Rect) -> Vec<Rect> {
        let Some(sk) = self.skin.as_ref() else {
            return Vec::new();
        };
        let xf = crate::skin::render::Xf::fit(sk.skin.width, sk.skin.height, view);
        let lpp = self.skin_mode.lanes_per_player();
        (0..self.skin_mode.players())
            .filter_map(|p| {
                sk.skin.lanes[p * lpp..((p + 1) * lpp).min(sk.skin.lanes.len())]
                    .iter()
                    .filter_map(|l| l.rect)
                    .map(|r| xf.rect(r))
                    .reduce(|a, b| {
                        let (x, y) = (a.x.min(b.x), a.y.min(b.y));
                        Rect::new(
                            x,
                            y,
                            a.right().max(b.right()) - x,
                            a.bottom().max(b.bottom()) - y,
                        )
                    })
            })
            .collect()
    }

    /// The hi-speed panel for skins without their own, beside the note
    /// field (between the fields in double play), never over the lanes.
    fn draw_panel(&self, ctx: &Ctx, d: &mut Draw, view: Rect, now: Instant, who: usize) {
        let ui = ctx.ui();
        let fields = self.fields(view);
        let (Some(first), Some(last)) = (fields.first(), fields.last()) else {
            return;
        };
        // free columns: between the fields, right of them, left of them;
        // the panel goes against the field (centred between two)
        #[derive(Clone, Copy)]
        enum Side {
            Between,
            Right,
            Left,
        }
        let mut spaces = Vec::new();
        if fields.len() == 2 {
            spaces.push((first.right(), last.x, Side::Between));
        }
        let right = (last.right(), view.right(), Side::Right);
        let left = (view.x, first.x, Side::Left);
        if self.vs {
            // VS: each player's beside their own field
            spaces.insert(0, if who == 0 { left } else { right });
        } else if first.x - view.x > view.right() - last.right() {
            spaces.extend([left, right]);
        } else {
            spaces.extend([right, left]);
        }
        let (want, gap) = (120.0 * ui, 6.0 * ui);
        let Some((x0, x1, side)) = spaces
            .iter()
            .copied()
            .find(|(a, b, _)| b - a >= want * 0.6 + gap * 2.0)
            .or_else(|| {
                spaces
                    .iter()
                    .copied()
                    .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
            })
        else {
            return;
        };
        let w = (x1 - x0 - gap * 2.0).min(want).max(40.0 * ui);
        let x = match side {
            Side::Between => (x0 + x1 - w) * 0.5,
            Side::Right => x0 + gap,
            Side::Left => x1 - gap - w,
        };
        let h = (first.h * 0.7).min(340.0 * ui);
        let r = Rect::new(x, first.y + first.h * 0.12, w, h);
        ui::panel(d, r);
        let p = &ctx.config.play.for_side(who);
        let st = super::play_skin::SceneState { s: self, ctx, now };
        let hs = p.hispeed_at(&st.bpms());
        let cx = r.x + w * 0.5;
        let small = (11.0 * ui).min(w * 0.12);
        let mut y = r.y + 10.0 * ui;
        let line = |d: &mut Draw, s: &str, size: f32, color, y: &mut f32| {
            d.text_fit(s, cx, *y, size, color, Align::Center, w - 8.0 * ui);
            *y += size * 1.3;
        };
        line(d, "HI-SPEED", small, theme::TEXT_DIM, &mut y);
        line(d, &format!("{hs:.2}"), small * 2.2, theme::ACCENT, &mut y);
        line(
            d,
            &format!("GN {}", p.green(&st.bpms())),
            small,
            theme::TEXT,
            &mut y,
        );
        // 0.5 .. 10.0 track with the current value
        let bottom_rows = 4.0 * small * 1.3 + 10.0 * ui;
        let (t0, t1) = (y + small * 1.4, r.bottom() - bottom_rows - small * 1.6);
        if t1 - t0 > 20.0 * ui {
            d.text("10.0", cx, y, small * 0.85, theme::TEXT_DIM, Align::Center);
            d.b.rect(
                Rect::new(cx - 2.0 * ui, t0, 4.0 * ui, t1 - t0),
                with_alpha(theme::TEXT_DIM, 0.4),
            );
            let k = ((hs - 0.5) / 9.5).clamp(0.0, 1.0);
            let my = t1 - (t1 - t0) * k;
            d.b.rect(
                Rect::new(cx - w * 0.3, my - 3.0 * ui, w * 0.6, 6.0 * ui),
                theme::ACCENT,
            );
            d.text(
                "0.5",
                cx,
                t1 + 3.0 * ui,
                small * 0.85,
                theme::TEXT_DIM,
                Align::Center,
            );
        }
        let mut y = r.bottom() - bottom_rows;
        let cover = if p.lane_cover || p.sudden > 0.0 {
            format!("SUD+ {:.0}%", p.sudden * 100.0)
        } else {
            "SUD+ OFF".into()
        };
        line(d, &cover, small, theme::TEXT, &mut y);
        line(
            d,
            &format!("LIFT {:.0}%", p.lift * 100.0),
            small,
            theme::TEXT,
            &mut y,
        );
        let c = &self.ctl[who];
        let hint = match (c.start, c.select) {
            (true, true) => ["HOLD:", "QUIT"],
            (true, false) => ["WHITE -0.25 / BLUE +0.25", "TURNTABLE: COVER"],
            _ => ["WHITE -0.01 / BLUE +0.01", "TURNTABLE: HI-SPEED"],
        };
        for h in hint {
            line(d, h, small * 0.8, theme::TEXT_DIM, &mut y);
        }
        if let Some(t) = c.both_since {
            let k = now.duration_since(t).as_secs_f32().min(1.0);
            d.b.rect(
                Rect::new(r.x, r.bottom() - 4.0 * ui, r.w * k, 4.0 * ui),
                theme::ACCENT,
            );
        }
    }

    fn draw_skinned(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let areas = self.touch_areas(ctx);
        self.rebuild_zones(ctx, &areas);
        let (view, _) = self.play_view(ctx, &areas);
        {
            let st = super::play_skin::SceneState { s: self, ctx, now };
            let mut hooks = super::play_skin::SceneHooks { st: &st };
            let ms = if now >= self.scene_start {
                now.duration_since(self.scene_start).as_millis() as i64
            } else {
                0
            };
            if let Some(sk) = self.skin.as_ref() {
                sk.draw(d, view, ms, &st, &mut hooks);
            }
        }
        if ctx.touch_enabled() {
            for (area, _, _) in &areas {
                d.b.rect(*area, rgba(0x05060aff));
            }
            // (the perspective lanes draw their own touch areas)
            let shown = if self.perspective() {
                let first = self.zones.iter().position(|z| z.lane >= touch::SYSTEM);
                &self.zones[first.unwrap_or(self.zones.len())..]
            } else {
                &self.zones[..]
            };
            self.touch.draw(d.b, shown, ctx.config.touch.opacity);
        }
        if ctx.touch_enabled() {
            let ui = ctx.ui();
            for z in self.zones.iter().filter(|z| z.lane >= touch::SYSTEM) {
                if let touch::Shape::Rect(r) = z.shape {
                    let label = match z.lane {
                        touch::START | touch::START2 => "START",
                        _ => "SELECT",
                    };
                    let size = (r.w * 0.22).min(14.0 * ui);
                    d.text_fit(
                        label,
                        r.x + r.w * 0.5,
                        r.y + (r.h - size) * 0.5,
                        size,
                        theme::TEXT,
                        Align::Center,
                        r.w * 0.9,
                    );
                }
            }
        }
        for who in 0..2 {
            // (a skin's own panel is the first player's)
            if self.ctl[who].open() && (!self.skin_panel || who == 1) {
                self.draw_panel(ctx, d, view, now, who);
            }
        }
        // VS: a player who failed plays no more
        if self.vs {
            let ui = ctx.ui();
            let fields = self.fields(view);
            for (side, f) in self.sides.iter().zip(&fields) {
                if side.failed.is_some() {
                    d.b.rect(*f, with_alpha(rgba(0x000000ff), 0.6));
                    d.text(
                        "FAILED",
                        f.x + f.w * 0.5,
                        f.y + f.h * 0.4,
                        32.0 * ui,
                        theme::ACCENT,
                        Align::Center,
                    );
                }
            }
        }
        let ui = ctx.ui();
        let notices = [
            (self.ghost_desync(), "GHOST DESYNC"),
            (self.older_replay(), "REPLAY OF AN OLDER ENGINE"),
        ];
        let mut y = view.y + 8.0 * ui;
        for (_, what) in notices.iter().filter(|(on, _)| *on) {
            d.text(
                what,
                view.x + view.w * 0.5,
                y,
                16.0 * ui,
                theme::ACCENT,
                Align::Center,
            );
            y += 20.0 * ui;
        }
        if let Stage::Ending { since, .. } = self.stage {
            let a = (now.duration_since(since).as_secs_f32() / 1.2).min(1.0);
            d.b.rect(
                Rect::new(0.0, 0.0, ctx.width, ctx.height),
                with_alpha(rgba(0x000000ff), a * 0.85),
            );
        }
        // advance render cursors
        let t_vis = self.chart_us(now);
        for (lane, list) in self.lane_notes.iter().enumerate() {
            let judge = self.sides.iter().find(|s| s.engine.judges(lane as u8));
            let Some(notes) = judge.map(|s| s.engine.notes()) else {
                continue;
            };
            let c = &mut self.lane_cursor[lane];
            while *c < list.len() {
                let n = &notes[list[*c]];
                if n.end_time < t_vis - 1_000_000 && (n.is_done() || n.is_mine()) {
                    *c += 1;
                } else {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibebms_core::bot;
    use vibebms_core::gauge::{GaugePreset, GaugeType};
    use vibebms_core::judge::JudgePreset;

    /// FNV-1a over what a play leaves: judgements, logs, score, gauge.
    fn digest(e: &Engine) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut put = |v: i64| {
            for b in v.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
        };
        for g in &e.ghost {
            put(*g as i64);
        }
        for (t, j, s) in &e.judge_log {
            put(*t);
            put(*j as i64 * 3 + *s as i64);
        }
        for (t, d) in &e.timing_log {
            put(*t);
            put(*d);
        }
        let sc = &e.score;
        for j in 0..6 {
            put(sc.counts[j][0] as i64);
            put(sc.counts[j][1] as i64);
            put(sc.just[j] as i64);
        }
        put(sc.max_combo as i64);
        put(sc.combo_breaks as i64);
        put(sc.total_notes as i64);
        put(e.gauge.value().to_bits() as i64);
        put(e.is_failed() as i64);
        h
    }

    /// Plays made up chart `keys` with `play` and the bot, then watches
    /// the replay as the play scene does (the replay's options and seed
    /// make the chart and the rules): both must judge the same. The
    /// digest of the play.
    fn play_and_watch(keys: usize, play: &PlayConfig) -> (u64, u32) {
        let text = bot::chart_text(keys, keys as i64 * 7);
        let path = Path::new("song/chart.bme");
        let seed = 4242;
        let (chart, _, _) = load_chart(path, text.as_bytes(), play, seed, None).unwrap();
        let (mut e, _) = make_engine(&chart, &Rules::of(play), None);
        e.record();
        let inputs = bot::inputs(&chart, None, 99);
        bot::drive(&mut e, &inputs, chart.length() + 2_000_000);
        // the replay as saved and loaded
        let r = Replay {
            version: crate::replay::VERSION,
            sha256: String::new(),
            md5: String::new(),
            title: String::new(),
            ln: 0,
            date: 0,
            seed,
            play: play.clone(),
            ex_score: e.score.ex_score(),
            clear: String::new(),
            ghost: e.ghost.clone(),
            ops: e.recorded().to_vec(),
        };
        let r: Replay = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        let (chart, _, _) = load_chart(path, text.as_bytes(), &r.play, r.seed, None).unwrap();
        let (mut w, _) = make_engine(&chart, &Rules::of(&r.play), None);
        // fed frame by frame as the scene does
        let mut feed = Feed::new(Arc::new(r.clone()), 0);
        let mut t = -1_500_000;
        while feed.at < r.ops.len() {
            feed.run(&mut w, t, |_, _| {});
            t += 16_667;
        }
        assert_eq!(w.ghost, r.ghost);
        assert_eq!(digest(&w), digest(&e));
        (digest(&e), e.score.ex_score())
    }

    /// Autoplay through the scene's chart and judge making, frame by
    /// frame: every key mode and LN mode, with the lane options, battle
    /// and HCN bodies; every note PGREAT, the play finished.
    #[test]
    fn autoplay_plays_every_chart_through() {
        for keys in [5, 7, 10, 14] {
            for (ln, random) in [
                (LnType::Ln, LaneOption::Off),
                (LnType::Cn, LaneOption::SRandom),
                (LnType::Hcn, LaneOption::HRandom),
            ] {
                let play = PlayConfig {
                    ln_mode: ln,
                    random_1p: random,
                    battle: keys < 10 && ln == LnType::Cn,
                    ..PlayConfig::default()
                };
                let text = bot::chart_text(keys, keys as i64 * 3);
                let path = Path::new("song/chart.bme");
                let (chart, _, _) = load_chart(path, text.as_bytes(), &play, 9, None).unwrap();
                let mut r = Rules::of(&play);
                r.autoplay = true;
                let players = if chart.mode().is_double() && play.battle {
                    vec![Some(0), Some(1)]
                } else {
                    vec![None]
                };
                for player in players {
                    let (mut e, _) = make_engine(&chart, &r, player);
                    let end = chart.length() + 2_000_000;
                    let mut t = -1_000_000;
                    while t < end {
                        e.update(t);
                        e.drain_events().count();
                        t += 16_667;
                    }
                    let what = format!("{keys}K {ln:?} {random:?} {player:?}");
                    assert!(e.score.total_notes > 0, "{what}");
                    assert!(e.ghost.iter().all(|j| *j == 0), "{what}: {:?}", e.ghost);
                    assert!(e.is_finished(end) && !e.is_failed(), "{what}");
                }
            }
        }
    }

    /// Replays of plays with these options judge the same as before the
    /// play scene took several judges (the digests were taken then).
    #[test]
    fn replays_judge_as_before() {
        let d = PlayConfig::default;
        let cases: Vec<(usize, PlayConfig)> = vec![
            (7, d()),
            (5, d()),
            (14, d()),
            (10, d()),
            (
                7,
                PlayConfig {
                    random_1p: LaneOption::Random,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    random_1p: LaneOption::SRandom,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    random_1p: LaneOption::HRandom,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    random_1p: LaneOption::AllScratch,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    ln_mode: LnType::Cn,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    ln_mode: LnType::Hcn,
                    gauge: GaugeType::Hard,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    judge_preset: JudgePreset::Lr2,
                    gauge_preset: GaugePreset::Lr2,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    judge_preset: JudgePreset::Iidx,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    auto_scratch: true,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    legacy_note: true,
                    no_mine: true,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    expand_judge: true,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    judge_algorithm: vibebms_core::engine::JudgeAlgorithm::Duration,
                    gauge: GaugeType::ExHard,
                    ..d()
                },
            ),
            (
                7,
                PlayConfig {
                    battle: true,
                    ..d()
                },
            ),
            (
                5,
                PlayConfig {
                    battle: true,
                    auto_scratch: true,
                    random_2p: LaneOption::Mirror,
                    ..d()
                },
            ),
            (
                14,
                PlayConfig {
                    random_1p: LaneOption::Random,
                    random_2p: LaneOption::RRandom,
                    dp_option: DoubleOption::Flip,
                    ..d()
                },
            ),
        ];
        let got: Vec<(u64, u32)> = cases.iter().map(|(k, p)| play_and_watch(*k, p)).collect();
        let want = [
            (0x6e7befe494f86dc7, 474),
            (0xbf6ca1516b2dc3a6, 336),
            (0x758a4f4820ee6ab0, 932),
            (0x941e7fce3f15bed2, 654),
            (0x5fc6086c2dda718e, 480),
            (0x40624123b4d9a015, 470),
            (0xce76d9ba6ff28c18, 471),
            (0x8cd4c2f2d3cbd667, 493),
            (0xd42210a62c598fbb, 558),
            (0x32364c7127b745df, 558),
            (0x7ba9f6a676c617c1, 441),
            (0xbcd1d1647dd9fff4, 434),
            (0xffe1584f208e5414, 550),
            (0x47dc09ae4c89eab1, 468),
            (0xe4283ac5977abf9e, 950),
            (0xbf481f7d42daafcf, 473),
            (0x4135924bdf310df1, 945),
            (0xe349c363d5d76f7b, 780),
            (0x6a8d0564b558f304, 947),
        ];
        if want.len() != got.len() {
            panic!("digests: {got:x?}");
        }
        assert_eq!(got, want);
    }

    /// G-BATTLE: a replay made on the single play chart plays again as the
    /// opponent on the battle chart's second side, judging the same; the
    /// player's side judges as the single play chart with the opponent's
    /// lane option (what the player's replay keeps).
    #[test]
    fn a_replay_plays_again_as_the_opponent() {
        let d = PlayConfig::default;
        let cases: Vec<(usize, PlayConfig, PlayConfig)> = vec![
            (7, d(), d()),
            (
                7,
                PlayConfig {
                    random_1p: LaneOption::SRandom,
                    ln_mode: LnType::Cn,
                    auto_scratch: true,
                    ..d()
                },
                PlayConfig {
                    no_mine: true,
                    gauge: GaugeType::Hard,
                    ..d()
                },
            ),
            (
                5,
                PlayConfig {
                    random_1p: LaneOption::HRandom,
                    ln_mode: LnType::Hcn,
                    no_mine: true,
                    judge_preset: JudgePreset::Lr2,
                    ..d()
                },
                PlayConfig {
                    random_1p: LaneOption::Mirror,
                    legacy_note: true,
                    judge_preset: JudgePreset::Iidx,
                    ..d()
                },
            ),
        ];
        for (keys, theirs, mine) in cases {
            let text = bot::chart_text(keys, 11);
            let path = Path::new("song/chart.bme");
            let seed = 777;
            // the opponent's play
            let (sp, _, _) = load_chart(path, text.as_bytes(), &theirs, seed, None).unwrap();
            let (mut e, _) = make_engine(&sp, &Rules::of(&theirs), None);
            e.record();
            bot::drive(&mut e, &bot::inputs(&sp, None, 5), sp.length() + 2_000_000);
            let r = Arc::new(Replay {
                version: crate::replay::VERSION,
                sha256: String::new(),
                md5: String::new(),
                title: String::new(),
                ln: 0,
                date: 0,
                seed,
                play: theirs.clone(),
                ex_score: e.score.ex_score(),
                clear: String::new(),
                ghost: e.ghost.clone(),
                ops: e.recorded().to_vec(),
            });
            // the battle
            let (bt, facts, _) =
                load_battle_chart(path, text.as_bytes(), &mine, &theirs, seed).unwrap();
            assert!(!facts.battle);
            assert_eq!(facts.lane, vec![theirs.random_1p]);
            let lpp = bt.mode().lanes_per_player() as u8;
            let (mut g, _) = make_engine(&bt, &Rules::of(&theirs), Some(1));
            let mut feed = Feed::new(r.clone(), lpp);
            let mut t = -1_500_000;
            let mut lit = 0;
            while feed.at < r.ops.len() {
                feed.run(&mut g, t, |lane, _| {
                    assert!(lane >= lpp);
                    lit += 1;
                });
                t += 16_667;
            }
            assert!(lit > 100);
            assert_eq!(g.ghost, r.ghost);
            assert_eq!(digest(&g), digest(&e), "{keys}K opponent");
            // the player's side and the player's replay
            let (mut me, _) = make_engine(&bt, &Rules::of(&mine), Some(0));
            let mut played = mine.clone();
            played.random_1p = theirs.random_1p;
            let (solo, _, _) = load_chart(path, text.as_bytes(), &played, seed, None).unwrap();
            let (mut alone, _) = make_engine(&solo, &Rules::of(&played), None);
            let inputs = bot::inputs(&solo, None, 8);
            bot::drive(&mut me, &inputs, solo.length() + 2_000_000);
            bot::drive(&mut alone, &inputs, solo.length() + 2_000_000);
            assert_eq!(digest(&me), digest(&alone), "{keys}K player");
        }
    }

    /// For trying G-BATTLE by hand: the test bot plays a chart and its
    /// replay becomes the chart's best and latest.
    /// `VIBEBMS_BOT_CHART=<chart> VIBEBMS_BOT_ROOT=<data folder> cargo test
    /// -p vibebms bot_replay -- --ignored`
    #[test]
    #[ignore]
    fn bot_replay() {
        let chart = std::env::var("VIBEBMS_BOT_CHART").unwrap();
        let root = std::path::PathBuf::from(std::env::var("VIBEBMS_BOT_ROOT").unwrap());
        let path = Path::new(&chart);
        let bytes = std::fs::read(path).unwrap();
        let c = vibebms_core::load_chart_bytes(path, &bytes, &Default::default()).unwrap();
        let song = crate::library::song_from_chart(path, String::new(), &c);
        let play = PlayConfig {
            random_1p: LaneOption::Random,
            ..PlayConfig::default()
        };
        let seed = 31337;
        let (c, _, prof) = load_chart(path, &bytes, &play, seed, None).unwrap();
        let (mut e, _) = make_engine(&c, &Rules::of(&play), None);
        e.record();
        bot::drive(&mut e, &bot::inputs(&c, None, 3), c.length() + 2_000_000);
        let r = Replay {
            version: crate::replay::VERSION,
            sha256: song.sha256.clone(),
            md5: song.md5.clone(),
            title: song.title.clone(),
            ln: prof.score_ln(play.ln_mode) as i32,
            date: 0,
            seed,
            play,
            ex_score: e.score.ex_score(),
            clear: "CLEAR".into(),
            ghost: e.ghost.clone(),
            ops: e.recorded().to_vec(),
        };
        r.save(&root, crate::replay::Slot::Best).unwrap();
        r.save(&root, crate::replay::Slot::Last).unwrap();
        println!("EX {} of {}", r.ex_score, e.score.max_ex());
    }

    #[test]
    fn vs_players_have_their_own_voices_and_controls() {
        // a keysound and an HCN mute of one side never reach the other's
        assert_ne!(voice(3, 0, true), voice(3, 1, true));
        assert_eq!(voice(3, 0, true), voice(3, 0, false));
        assert_eq!(voice(3, 1, false), voice(3, 0, false));
        // each player's START + keys change their own hi-speed
        let bpm = Bpms::flat(150.0);
        let now = Instant::now();
        let mut p = PlayConfig::default();
        p.p2.hispeed = 3.0;
        let mut c = [Control::default(), Control::default()];
        let ev = |lane, pressed| LaneEv {
            lane,
            sub: 0,
            pressed,
            time: now,
        };
        let mode = PlayMode::Beat14K;
        let mut two = p.for_side(1);
        assert!(control(
            &mut c[1],
            mode,
            &mut two,
            &bpm,
            &ev(touch::START2, true)
        ));
        // a blue key of 2P: +0.25
        assert!(control(&mut c[1], mode, &mut two, &bpm, &ev(9, true)));
        p.set_side(1, &two);
        assert_eq!((p.hispeed, p.p2.hispeed), (2.5, 3.25));
        // 1P's panel is not open: its key goes to the judge
        let mut one = p.for_side(0);
        assert!(!control(&mut c[0], mode, &mut one, &bpm, &ev(1, true)));
    }

    #[test]
    fn only_fitting_replays_are_opponents() {
        use crate::replay::ghost_refusal;
        let song = crate::library::Song {
            mode: "7KEYS".into(),
            has_ln: true,
            ..Default::default()
        };
        let r = Replay {
            version: crate::replay::VERSION,
            sha256: String::new(),
            md5: String::new(),
            title: String::new(),
            ln: 0,
            date: 0,
            seed: 1,
            play: PlayConfig::default(),
            ex_score: 0,
            clear: String::new(),
            ghost: Vec::new(),
            ops: vec![Op::Update(0)],
        };
        assert_eq!(ghost_refusal(&r, &song, LnType::Ln), None);
        assert!(ghost_refusal(&r, &song, LnType::Cn).is_some());
        let bad = |f: fn(&mut Replay)| {
            let mut x = r.clone();
            f(&mut x);
            ghost_refusal(&x, &song, LnType::Ln).is_some()
        };
        assert!(bad(|r| r.play.battle = true));
        assert!(bad(|r| r.play.legacy_note = true));
        assert!(bad(|r| r.ops.clear()));
        assert!(bad(|r| r.play.ln_mode = LnType::Hcn));
        let dp = crate::library::Song {
            mode: "14KEYS".into(),
            ..song.clone()
        };
        assert!(ghost_refusal(&r, &dp, LnType::Ln).is_some());
    }

    #[test]
    fn gauge_graph_ends_with_the_final_gauge() {
        // failed between two samples: the graph reaches 0
        assert_eq!(settled_log(&[100.0, 40.0], 0.0), vec![100.0, 40.0, 0.0]);
        assert_eq!(settled_log(&[20.0, 82.0], 82.0), vec![20.0, 82.0]);
        assert_eq!(settled_log(&[], 20.0), vec![20.0]);
    }
}
