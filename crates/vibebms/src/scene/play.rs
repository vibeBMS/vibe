//! Gameplay: loading, audio scheduling, judging, rendering, touch controls.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::{Bpms, PlayConfig, Side, HISPEED_STEP};
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Gfx, Rect, TexId};
use crate::input::{InputEv, LaneEv, LaneMapper};
use crate::library::Song;
use crate::platform::{Duration, Instant};
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
use vibebms_core::engine::{Engine, EngineConfig, EngineEvent, InputEvent};
use vibebms_core::gauge::GrooveGauge;
use vibebms_core::judge::{judge_rules, BAD, GREAT, MISS};
use vibebms_core::lane_option::{self, DoubleOption, LaneOption};
use vibebms_core::score::{ClearType, Score};
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
    /// Why the play is not the chart as written (it is then not recorded).
    altered: Option<&'static str>,
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
    pub(crate) engine: Option<Engine>,
    pub(crate) mapper: Option<LaneMapper>,
    pub(crate) touch: TouchInput,
    pub(crate) zones: Vec<Zone>,
    pub(crate) zones_key: (u32, u32, bool, bool),
    pub(crate) start: Instant,
    pub(crate) base_frame: i64,
    pub(crate) bgm_i: usize,
    pub(crate) auto_i: usize,
    pub(crate) bga_i: usize,
    pub(crate) auto_notes: Vec<(i64, SoundId)>,
    pub(crate) bga: [Option<u32>; 4],
    pub(crate) poor_until: Option<Instant>,
    pub(crate) last_judge: [Option<(usize, i64, Instant, u32)>; 2],
    pub(crate) bombs: Vec<(u8, Instant, usize)>,
    pub(crate) gauge_log: Vec<f32>,
    pub(crate) next_sample: i64,
    pub(crate) lane_notes: Vec<Vec<usize>>,
    pub(crate) lane_cursor: Vec<usize>,
    pub(crate) mode: PlayMode,
    pub(crate) ln: LnType,
    pub(crate) seed: i64,
    pub(crate) error: Option<String>,
    /// START / SELECT: the hi-speed panel.
    ctl: Control,
    /// The skin draws the panel itself (objects on timer 21).
    skin_panel: bool,
    pub(crate) assist: bool,
    /// Why this play is not recorded (`None`: it is).
    unranked: Option<&'static str>,
    pub(crate) options_label: String,
    pub(crate) finished_at: Option<i64>,
    pub(crate) key_flash: Vec<Option<Instant>>,
    pub(crate) quit: bool,
    pub(crate) scene_start: Instant,
    pub(crate) ready_at: Option<Instant>,
    pub(crate) finished_instant: Option<Instant>,
    pub(crate) key_off: Vec<Option<Instant>>,
    pub(crate) bomb_at: Vec<Option<Instant>>,
    /// Last judgement per lane (skin values 500-519).
    pub(crate) lane_judge: Vec<Option<usize>>,
    pub(crate) skin: Option<crate::skin::render::LoadedSkin>,
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

impl PlayScene {
    pub fn new(ctx: &mut Ctx, req: PlayRequest) -> PlayScene {
        let progress = Arc::new(AtomicUsize::new(0));
        let total_files = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = channel();
        let path = PathBuf::from(&req.song.path);
        let play = ctx.config.play.clone();
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
                let seed = crate::platform::SystemTime::now()
                    .duration_since(crate::platform::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(1);
                let bytes = crate::platform::vfs::read(&path).map_err(|e| e.to_string())?;
                let mut chart = vibebms_core::load_chart_bytes(
                    &path,
                    &bytes,
                    &vibebms_core::bms::ParseOptions {
                        random_seed: Some(seed),
                        random_selections: None,
                    },
                )
                .map_err(|e| e.to_string())?;
                // chart transforms (options)
                let mut altered = None;
                if play.battle && !chart.mode().is_double() {
                    lane_option::battle(&mut chart);
                    altered = Some("BATTLE");
                }
                let thr = 60_000_000 / (play.hran_threshold_bpm.max(1) as i64 * 4);
                let allowed = |o: LaneOption| {
                    if in_course && !(course_mirror && o == LaneOption::Mirror) {
                        LaneOption::Off
                    } else {
                        o
                    }
                };
                lane_option::apply(&mut chart, 0, allowed(play.random_1p), seed, thr);
                if chart.mode().is_double() {
                    lane_option::apply(
                        &mut chart,
                        1,
                        allowed(play.random_2p),
                        seed.wrapping_add(1),
                        thr,
                    );
                    if play.dp_option == DoubleOption::Flip {
                        lane_option::flip(&mut chart);
                    }
                }
                if play.legacy_note {
                    lane_option::legacy_note(&mut chart);
                }
                if play.no_mine {
                    let n = chart.notes.len();
                    lane_option::remove_mines(&mut chart);
                    if chart.notes.len() != n {
                        altered = altered.or(Some("NO MINE"));
                    }
                }
                let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                t2.store(chart.sound_files.len(), Ordering::Relaxed);
                let resolve = |name: &str| {
                    crate::platform::vfs::find_by_stem(&dir, name, crate::platform::vfs::AUDIO_EXTS)
                };
                let read = |p: &Path| crate::platform::vfs::read(p).ok();
                let bank = SoundBank::load_with(&chart.sound_files, rate, &p2, &resolve, &read);
                // an image or a video (shown when ffmpeg is installed)
                let (images, videos) = if play.bga {
                    use crate::platform::vfs::{find_by_stem, IMAGE_EXTS};
                    use crate::video::{is_video, VIDEO_EXTS};
                    chart
                        .images
                        .iter()
                        .map(|f| match find_by_stem(&dir, f, IMAGE_EXTS) {
                            Some(p) if is_video(&p) => (None, Some(p)),
                            Some(p) => (crate::gfx::load_rgba(&p, true), None),
                            None => (None, find_by_stem(&dir, f, VIDEO_EXTS)),
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
                    altered,
                }))
            })();
            let _ = tx.send(r);
        };
        let mode = PlayMode::Beat7K;
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
            engine: None,
            mapper: None,
            touch: TouchInput::default(),
            zones: Vec::new(),
            zones_key: (0, 0, false, false),
            start: Instant::now(),
            base_frame: 0,
            bgm_i: 0,
            auto_i: 0,
            bga_i: 0,
            auto_notes: Vec::new(),
            bga: [None; 4],
            poor_until: None,
            last_judge: [None, None],
            bombs: Vec::new(),
            gauge_log: Vec::new(),
            next_sample: 0,
            lane_notes: Vec::new(),
            lane_cursor: Vec::new(),
            mode,
            ln: ctx.config.play.ln_mode,
            seed: 0,
            error: None,
            ctl: Control::default(),
            skin_panel: false,
            assist: false,
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
        }
    }

    pub(crate) fn mode(&self) -> PlayMode {
        self.mode
    }

    pub(crate) fn ln_mode(&self) -> LnType {
        self.ln
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
            altered,
        } = *l;
        self.seed = seed;
        self.mode = chart.mode();
        let p = &ctx.config.play;
        let course = self.req.course.as_ref();
        self.unranked = altered
            .or(p.custom_judge.map(|_| "CUSTOM JUDGE"))
            .or(course.map(|_| "COURSE"));
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
        let fam = self.mode.family();
        let gauge_preset = if course.map(|c| c.has("gauge_lr2")).unwrap_or(false) {
            vibebms_core::gauge::GaugePreset::Lr2
        } else {
            p.gauge_preset
        };
        let rules = judge_rules(p.judge_preset, fam, chart.meta.judge_rank, p.custom_judge);
        let total = chart.total(self.ln);
        let notes = chart.total_judgements(self.ln);
        let gauge_type = if course.is_some() {
            p.gauge.to_class()
        } else {
            p.gauge
        };
        let gas = if course.is_some() {
            vibebms_core::gauge::GaugeAutoShift::None
        } else {
            p.gauge_auto_shift
        };
        let mut gauge = GrooveGauge::new(gauge_preset, fam, gauge_type, gas, total, notes);
        if let Some(v) = course.and_then(|c| c.gauge_values.as_ref()) {
            gauge.set_values(v);
        }
        let lanes = self.mode.lane_count();
        let auto_lanes: Vec<bool> = (0..lanes)
            .map(|l| self.req.autoplay || (p.auto_scratch && self.mode.is_scratch(l)))
            .collect();
        self.assist = p.auto_scratch || p.legacy_note;
        let cfg = EngineConfig {
            rules,
            default_ln: self.ln,
            algorithm: p.judge_algorithm,
            auto_lanes: auto_lanes.clone(),
            scratch_release_ends_bss: true,
        };
        let engine = Engine::new(&chart, cfg, gauge);
        let mut lane_notes = vec![Vec::new(); lanes];
        for (i, n) in engine.notes().iter().enumerate() {
            lane_notes[n.lane as usize].push(i);
        }
        self.lane_notes = lane_notes;
        self.lane_cursor = vec![0; lanes];
        self.key_flash = vec![None; lanes];
        self.key_off = vec![None; lanes];
        self.bomb_at = vec![None; lanes];
        self.lane_judge = vec![None; lanes];
        // keysounds of auto lanes are scheduled ahead like BGM for exact timing
        self.auto_notes = chart
            .notes
            .iter()
            .filter(|n| {
                auto_lanes.get(n.lane as usize).copied().unwrap_or(false)
                    && n.is_playable()
                    && n.sound != NO_SOUND
            })
            .map(|n| (n.time, n.sound))
            .collect();
        self.engine = Some(engine);
        self.mapper = Some(LaneMapper::new(&ctx.config.keys, self.mode));
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
        let mut opts = vec![p.random_1p.label().to_string()];
        if self.mode.is_double() {
            opts.push(p.random_2p.label().to_string());
        }
        if p.auto_scratch {
            opts.push("AUTO-SCR".into());
        }
        if p.legacy_note {
            opts.push("LEGACY".into());
        }
        self.options_label = opts.join(" / ");
        self.chart = chart;
        self.bank = bank;
        // start: 1.5 s lead-in so the first notes scroll in
        let lead = Duration::from_millis(1500);
        let now = Instant::now();
        self.start = now + lead;
        self.base_frame = ctx.mixer.current_frame() as i64
            + ctx.mixer.us_to_frames(lead.as_micros() as i64)
            + ctx.mixer.buffer_frames as i64;
        self.stage = Stage::Playing;
        self.ready_at = Some(now);
        self.skin = super::play_skin::load_play_skin(ctx, gfx, self);
        self.skin_panel = self
            .skin
            .as_ref()
            .is_some_and(|sk| sk.skin.objects.iter().any(|o| o.dst.timer == 21));
        if let Some(sk) = &self.skin {
            log::info!("skin: {}", sk.skin.name);
        }
    }

    fn play_sound(&self, ctx: &Ctx, sound: SoundId, at: Option<i64>, bus: Bus) {
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
                key: sound as u64 + 1,
                volume: vol,
                bus,
                start_frame: at.map(|t| (self.base_frame + m.us_to_frames(t)).max(0) as u64),
                offset: m.us_to_frames(slice.start).max(0) as usize,
                end: slice.end.map(|e| m.us_to_frames(e).max(0) as usize),
                looping: false,
            },
        );
    }

    fn schedule_audio(&mut self, ctx: &Ctx, t: i64) {
        let horizon = t + 200_000;
        while let Some(b) = self.chart.bgm.get(self.bgm_i) {
            if b.time > horizon {
                break;
            }
            if b.time >= t - 100_000 {
                self.play_sound(ctx, b.sound, Some(b.time), Bus::Bgm);
            }
            self.bgm_i += 1;
        }
        while let Some(&(time, s)) = self.auto_notes.get(self.auto_i) {
            if time > horizon {
                break;
            }
            if time >= t - 100_000 {
                self.play_sound(ctx, s, Some(time), Bus::Key);
            }
            self.auto_i += 1;
        }
    }

    fn feed(&mut self, ev: LaneEv, offset: i64) {
        let t = self.chart_us(ev.time) - offset;
        if let Some(e) = self.engine.as_mut() {
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

    /// Screen areas of the touch controls: (rect, player, scratch on the right).
    fn touch_areas(&self, ctx: &Ctx) -> Vec<(Rect, usize, bool)> {
        let (w, h) = (ctx.width, ctx.height);
        if !ctx.touch_enabled() {
            return Vec::new();
        }
        let t = &ctx.config.touch;
        let share = if h > w {
            t.portrait_height
        } else {
            t.landscape_height
        };
        let ctrl_h = (h * share).clamp(80.0, h * 0.6);
        let area = Rect::new(0.0, h - ctrl_h, w, ctrl_h);
        let dp = self.mode.is_double();
        let side2 = ctx.config.play.side == Side::P2;
        if dp && ctx.config.touch.allow_dp {
            // the two keypads a white key's gap (12 mm) apart
            let half = (area.w - 12.0 * ctx.px_per_mm()).max(area.w * 0.9) * 0.5;
            vec![
                (Rect::new(area.x, area.y, half, area.h), 0, false),
                (
                    Rect::new(area.right() - half, area.y, half, area.h),
                    1,
                    true,
                ),
            ]
        } else {
            vec![(area, 0, !dp && side2)]
        }
    }

    /// Where the play skin goes (above the touch controls) and, with touch
    /// controls, the START / SELECT buttons beside it.
    fn play_view(
        &self,
        ctx: &Ctx,
        areas: &[(Rect, usize, bool)],
    ) -> (Rect, Option<[touch::Zone; 2]>) {
        let screen = Rect::new(
            0.0,
            0.0,
            ctx.width,
            areas.first().map(|t| t.0.y).unwrap_or(ctx.height),
        );
        if areas.is_empty() {
            return (screen, None);
        }
        let aspect = self
            .skin
            .as_ref()
            .map(|s| s.skin.width / s.skin.height.max(1.0))
            .unwrap_or(16.0 / 9.0);
        // on the scratch side, which moves the note field towards the middle:
        // left for 1P and double play, right for 2P
        let right = !self.mode.is_double() && ctx.config.play.side == Side::P2;
        let (view, buttons) = touch::system_buttons(screen, aspect, ctx.px_per_mm(), right);
        (view, Some(buttons))
    }

    fn rebuild_zones(&mut self, ctx: &Ctx, areas: &[(Rect, usize, bool)]) {
        let key = (
            ctx.width as u32,
            ctx.height as u32,
            ctx.touch_enabled(),
            self.skin.is_some(),
        );
        if key == self.zones_key && !self.zones.is_empty() {
            return;
        }
        self.zones_key = key;
        self.zones.clear();
        for (area, player, right) in areas {
            let player = (*player).min(self.mode.players() - 1);
            self.zones.extend(touch::layout(
                self.mode,
                ctx.config.touch.style,
                *area,
                player,
                *right,
                ctx.px_per_mm(),
            ));
        }
        if let (_, Some(buttons)) = self.play_view(ctx, areas) {
            self.zones.extend(buttons);
        }
    }

    fn finish(&mut self, ctx: &mut Ctx, failed: bool) -> Next {
        let Some(e) = self.engine.as_ref() else {
            return Next::Pop;
        };
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
        let data = ResultData {
            request: PlayRequest {
                course: None,
                ..self.req.clone()
            },
            title: self.chart.meta.title.clone(),
            subtitle: self.chart.meta.subtitle.clone(),
            artist: self.chart.meta.artist.clone(),
            level: self.chart.meta.level.clone(),
            mode: self.mode,
            ln: if self.chart.meta.has_long {
                self.ln
            } else {
                LnType::Ln
            },
            has_ln: self.chart.meta.has_long,
            score,
            clear,
            gauge_type: gauge.current_type(),
            gauge_value: gauge.value(),
            gauge_log: self.gauge_log.clone(),
            timing_log: e.timing_log.clone(),
            windows: e.windows().note.clone(),
            judge_log: e.judge_log.clone(),
            notes_graph: super::graphs::columns(e.notes(), self.mode, self.chart.length()),
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
        };
        Next::Replace(Box::new(ResultScene::new(ctx, data)))
    }

    /// START / SELECT and what they do to the keys: `true` when the event
    /// was taken (it does not reach the judge).
    fn control(&mut self, ctx: &mut Ctx, e: &LaneEv) -> bool {
        let bpm = &Bpms::of_meta(&self.chart.meta);
        let p = &mut ctx.config.play;
        let c = &mut self.ctl;
        let was_open = c.open();
        match e.lane {
            touch::START => {
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
            touch::SELECT => c.select = e.pressed,
            _ if !c.open() => return false,
            // releases go on to the judge (the key may have been down first)
            _ if !e.pressed => {
                if self.mode.is_scratch(e.lane as usize) {
                    c.scratch = None;
                }
                return false;
            }
            lane if self.mode.is_scratch(lane as usize) => {
                let dir = if e.sub == 0 { -1 } else { 1 };
                c.turn(p, dir, bpm);
                c.scratch = Some((dir, e.time));
            }
            lane => {
                let blue = lane as usize % self.mode.lanes_per_player() % 2 == 1;
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

    /// When the hi-speed panel opened and last closed (skin timers 21 / 31).
    pub(crate) fn panel_timers(&self) -> (Option<Instant>, Option<Instant>) {
        (self.ctl.opened, self.ctl.closed)
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
        // the turntable keeps turning the panel's value while held
        if let Some((dir, last)) = self.ctl.scratch.filter(|_| self.ctl.open()) {
            if now.duration_since(last) >= Duration::from_millis(50) {
                self.ctl
                    .turn(&mut ctx.config.play, dir, &Bpms::of_meta(&self.chart.meta));
                self.ctl.scratch = Some((dir, now));
            }
        }
        if self
            .ctl
            .both_since
            .is_some_and(|t| now.duration_since(t) >= Duration::from_secs(1))
        {
            self.quit = true;
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
                    > Duration::from_millis(if failed { 1500 } else { 1200 })
                {
                    ctx.mixer.stop_bus(Bus::Bgm);
                    ctx.mixer.stop_bus(Bus::Key);
                    return self.finish(ctx, failed);
                }
                return Next::Stay;
            }
            Stage::Playing => {}
        }
        let offset = ctx.config.play.judge_offset_ms as i64 * 1000;
        let t = self.chart_us(now);
        // release turntables that stopped
        let mut evs = Vec::new();
        if let Some(m) = self.mapper.as_mut() {
            m.tick(now, &mut evs);
        }
        for e in evs {
            self.feed(e, offset);
        }
        self.schedule_audio(ctx, t);
        let mut failed = false;
        let mut events: Vec<EngineEvent> = Vec::new();
        let mut auto: Vec<bool> = Vec::new();
        if let Some(engine) = self.engine.as_mut() {
            engine.update(t - offset);
            events = engine.drain_events().collect();
            auto = (0..self.mode.lane_count())
                .map(|l| engine.is_auto(l))
                .collect();
            if t >= self.next_sample {
                self.gauge_log.push(engine.gauge.value());
                // every 500 ms of play time, like beatoraja
                self.next_sample = t + 500_000;
            }
            if engine.is_finished(t - offset) && self.finished_at.is_none() {
                self.finished_at = Some(t);
                self.finished_instant = Some(now);
            }
        }
        for ev in events {
            match ev {
                EngineEvent::Sound { sound, lane } => {
                    if !auto.get(lane as usize).copied().unwrap_or(false) {
                        self.play_sound(ctx, sound, None, Bus::Key);
                    }
                }
                EngineEvent::Volume { sound, volume } => {
                    ctx.mixer.set_volume(sound as u64 + 1, volume)
                }
                EngineEvent::Judge {
                    lane,
                    judge,
                    diff,
                    combo,
                    ..
                } => {
                    let player = self.mode.player_of_lane(lane as usize).min(1);
                    self.last_judge[player] = Some((judge, diff, now, combo));
                    if let Some(j) = self.lane_judge.get_mut(lane as usize) {
                        *j = Some(judge);
                    }
                    if judge <= GREAT {
                        self.bombs.push((lane, now, judge));
                        if let Some(b) = self.bomb_at.get_mut(lane as usize) {
                            *b = Some(now);
                        }
                    }
                    if judge >= BAD && judge != MISS {
                        self.poor_until = Some(now + Duration::from_millis(500));
                    }
                }
                EngineEvent::Mine { lane, .. } => self.bombs.push((lane, now, 9)),
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
        self.bombs
            .retain(|b| now.duration_since(b.1) < Duration::from_millis(250));
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
        let offset = ctx.config.play.judge_offset_ms as i64 * 1000;
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
            _ => {}
        }
        for e in lane_evs {
            if !matches!(self.stage, Stage::Loading) && self.control(ctx, &e) {
                continue;
            }
            if matches!(self.stage, Stage::Playing) {
                self.feed(e, offset);
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
        if self.engine.is_none() {
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
        let lpp = self.mode.lanes_per_player();
        (0..self.mode.players())
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
    fn draw_panel(&self, ctx: &Ctx, d: &mut Draw, view: Rect, now: Instant) {
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
        if first.x - view.x > view.right() - last.right() {
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
        let p = &ctx.config.play;
        let st = super::play_skin::SceneState { s: self, ctx, now };
        let hs = st.hispeed() as f32;
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
        let hint = match (self.ctl.start, self.ctl.select) {
            (true, true) => ["HOLD:", "QUIT"],
            (true, false) => ["WHITE -0.25 / BLUE +0.25", "TURNTABLE: COVER"],
            _ => ["WHITE -0.01 / BLUE +0.01", "TURNTABLE: HI-SPEED"],
        };
        for h in hint {
            line(d, h, small * 0.8, theme::TEXT_DIM, &mut y);
        }
        if let Some(t) = self.ctl.both_since {
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
            self.touch.draw(d.b, &self.zones, ctx.config.touch.opacity);
        }
        if ctx.touch_enabled() {
            let ui = ctx.ui();
            for z in self.zones.iter().filter(|z| z.lane >= touch::START) {
                if let touch::Shape::Rect(r) = z.shape {
                    let label = if z.lane == touch::START {
                        "START"
                    } else {
                        "SELECT"
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
        if self.ctl.open() && !self.skin_panel {
            self.draw_panel(ctx, d, view, now);
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
        if let Some(e) = self.engine.as_ref() {
            let notes = e.notes();
            for (lane, list) in self.lane_notes.iter().enumerate() {
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
}
