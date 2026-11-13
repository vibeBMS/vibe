//! Gameplay: loading, audio scheduling, judging, rendering, touch controls.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::Side;
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Gfx, Rect, TexId};
use crate::input::{InputEv, LaneEv, LaneMapper, Phase};
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
    pub(crate) zones_key: (u32, u32, bool),
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
    pub(crate) hud_rects: Vec<(Rect, usize)>,
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
            zones_key: (0, 0, false),
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
            hud_rects: Vec::new(),
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

    fn rebuild_zones(&mut self, ctx: &Ctx, areas: &[(Rect, usize, bool)]) {
        let key = (ctx.width as u32, ctx.height as u32, ctx.touch_enabled());
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

    fn hud_action(&mut self, ctx: &mut Ctx, i: usize) {
        let p = &mut ctx.config.play;
        match i {
            0 => p.duration_ms = p.duration_ms.saturating_sub(25).max(100),
            1 => p.duration_ms = (p.duration_ms + 25).min(5000),
            2 => p.sudden = (p.sudden - 0.02).max(0.0),
            3 => p.sudden = (p.sudden + 0.02).min(0.8),
            4 => self.quit = true,
            _ => {}
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
                        KeyCode::ArrowUp => self.hud_action(ctx, 1),
                        KeyCode::ArrowDown => self.hud_action(ctx, 0),
                        KeyCode::PageUp => self.hud_action(ctx, 3),
                        KeyCode::PageDown => self.hud_action(ctx, 2),
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
                let handled = ctx.touch_enabled()
                    && self.touch.pointer(
                        &self.zones,
                        *id,
                        *phase,
                        *x,
                        *y,
                        *time,
                        thr,
                        &mut lane_evs,
                    );
                if !handled && *phase == Phase::Down {
                    if let Some(i) = ui::hit(&self.hud_rects, *x, *y) {
                        self.hud_action(ctx, i);
                    }
                }
            }
            _ => {}
        }
        if matches!(self.stage, Stage::Playing) {
            for e in lane_evs {
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
    fn draw_skinned(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let areas = self.touch_areas(ctx);
        self.rebuild_zones(ctx, &areas);
        let view_h = areas.first().map(|t| t.0.y).unwrap_or(ctx.height);
        let view = Rect::new(0.0, 0.0, ctx.width, view_h);
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
            self.touch.draw(
                d.b,
                &self.zones,
                ctx.config.touch.opacity,
                theme::NOTE_WHITE,
                theme::NOTE_BLUE,
                theme::NOTE_SCRATCH,
            );
        }
        // touch HUD: hi-speed, lane cover and quit buttons
        self.hud_rects.clear();
        if ctx.touch_enabled() {
            let ui = ctx.ui();
            let (bw, bh) = (54.0 * ui, 34.0 * ui);
            let labels = ["HS−", "HS+", "SUD−", "SUD+", "×"];
            let x0 = ctx.width - bw * labels.len() as f32 - 8.0;
            for (i, l) in labels.iter().enumerate() {
                let r = Rect::new(x0 + i as f32 * bw, 6.0 * ui, bw - 4.0, bh);
                ui::button(d, r, l, ui * 0.9, false);
                self.hud_rects.push((r, i));
            }
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
