//! Gameplay: loading, audio scheduling, judging, rendering, touch controls.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::{HiSpeedMode, Side};
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Color, Gfx, Rect, TexId};
use crate::input::{InputEv, LaneEv, LaneMapper, Phase};
use crate::library::Song;
use crate::platform::{Duration, Instant};
use crate::scene::result::{ResultData, ResultScene};
use crate::touch::{self, TouchInput, Zone};
use crate::ui;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use vibebms_audio::{Bus, PlayOptions, SoundBank};
use vibebms_core::chart::{BgaLayer, Chart, NoteKind, SoundId, NO_SOUND};
use vibebms_core::engine::{Engine, EngineConfig, EngineEvent, InputEvent};
use vibebms_core::gauge::{GaugeType, GrooveGauge};
use vibebms_core::judge::{judge_rules, BAD, GOOD, GREAT, MISS, PGREAT, POOR};
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
    stagefile: Option<Rgba>,
    seed: i64,
}

enum Stage {
    Loading,
    Playing,
    Ending { since: Instant, failed: bool },
}

struct LaneGeom {
    lane: usize,
    x: f32,
    w: f32,
}

struct Field {
    player: usize,
    rect: Rect,
    lanes: Vec<LaneGeom>,
    judge_y: f32,
    gauge: Rect,
}

struct Layout {
    fields: Vec<Field>,
    touch: Vec<(Rect, usize, bool)>,
    side: Option<Rect>,
}

pub struct PlayScene {
    req: PlayRequest,
    stage: Stage,
    /// Chart/sound loading job, started once the files are available.
    job: Option<Box<dyn FnOnce() + Send>>,
    prefetch: crate::platform::vfs::Prefetch,
    rx: Option<Receiver<Result<Box<Loaded>, String>>>,
    progress: Arc<AtomicUsize>,
    total_files: Arc<AtomicUsize>,
    loading_since: Instant,
    stage_tex: Option<TexId>,
    chart: Chart,
    bank: SoundBank,
    bga_tex: Vec<Option<TexId>>,
    engine: Option<Engine>,
    mapper: Option<LaneMapper>,
    touch: TouchInput,
    zones: Vec<Zone>,
    zones_key: (u32, u32, bool),
    start: Instant,
    base_frame: i64,
    bgm_i: usize,
    auto_i: usize,
    bga_i: usize,
    auto_notes: Vec<(i64, SoundId)>,
    bga: [Option<u32>; 4],
    poor_until: Option<Instant>,
    last_judge: [Option<(usize, i64, Instant, u32)>; 2],
    bombs: Vec<(u8, Instant, usize)>,
    gauge_log: Vec<f32>,
    next_sample: i64,
    lane_notes: Vec<Vec<usize>>,
    lane_cursor: Vec<usize>,
    mode: PlayMode,
    ln: LnType,
    seed: i64,
    error: Option<String>,
    hud_rects: Vec<(Rect, usize)>,
    assist: bool,
    options_label: String,
    finished_at: Option<i64>,
    key_flash: Vec<Option<Instant>>,
    quit: bool,
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
                if play.battle && !chart.mode().is_double() {
                    lane_option::battle(&mut chart);
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
                    lane_option::remove_mines(&mut chart);
                }
                let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                t2.store(chart.sound_files.len(), Ordering::Relaxed);
                let resolve = |name: &str| {
                    crate::platform::vfs::find_by_stem(&dir, name, crate::platform::vfs::AUDIO_EXTS)
                };
                let read = |p: &Path| crate::platform::vfs::read(p).ok();
                let bank = SoundBank::load_with(&chart.sound_files, rate, &p2, &resolve, &read);
                let images = if play.bga {
                    chart
                        .images
                        .iter()
                        .map(|f| rgba_of(&dir.join(f.replace('\\', "/")), true))
                        .collect()
                } else {
                    Vec::new()
                };
                let stagefile = (!chart.meta.stagefile.is_empty())
                    .then(|| rgba_of(&dir.join(&chart.meta.stagefile), false))
                    .flatten();
                Ok(Box::new(Loaded {
                    chart,
                    bank,
                    images,
                    stagefile,
                    seed,
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
            options_label: String::new(),
            finished_at: None,
            key_flash: Vec::new(),
            quit: false,
        }
    }

    fn chart_us(&self, t: Instant) -> i64 {
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
            stagefile,
            seed,
        } = *l;
        self.seed = seed;
        self.mode = chart.mode();
        let p = &ctx.config.play;
        let course = self.req.course.as_ref();
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
        }
    }

    fn layout(&self, ctx: &Ctx) -> Layout {
        let (w, h) = (ctx.width, ctx.height);
        let ui = ctx.ui();
        let touch_on = ctx.touch_enabled();
        let ctrl_h = if touch_on {
            (h * ctx.config.touch.height).clamp(80.0, h * 0.6)
        } else {
            0.0
        };
        let play_h = h - ctrl_h;
        let dp = self.mode.is_double();
        let portrait = ctx.portrait();
        let side2 = ctx.config.play.side == Side::P2;
        let gauge_h = 26.0 * ui;
        let lw = ctx.theme.lane_width;
        let mut fields = Vec::new();
        let mut rects: Vec<(Rect, usize)> = Vec::new();
        let margin = 16.0 * ui;
        if dp {
            let fw = if portrait {
                w * 0.49
            } else {
                (w * 0.36).min(play_h * 0.85)
            };
            rects.push((
                Rect::new(if portrait { w * 0.005 } else { margin }, 0.0, fw, play_h),
                0,
            ));
            rects.push((
                Rect::new(
                    w - fw - if portrait { w * 0.005 } else { margin },
                    0.0,
                    fw,
                    play_h,
                ),
                1,
            ));
        } else {
            let fw = if portrait {
                w * 0.96
            } else {
                (w * 0.34).min(play_h * 0.8).max(w * 0.25)
            };
            let x = if portrait {
                (w - fw) * 0.5
            } else if side2 {
                w - fw - margin
            } else {
                margin
            };
            rects.push((Rect::new(x, 0.0, fw, play_h), 0));
        }
        for (r, player) in rects {
            let scratch_right = if dp { player == 1 } else { side2 };
            let order = self.mode.display_order(player, scratch_right);
            let units: f32 = order
                .iter()
                .map(|l| {
                    if self.mode.is_scratch(*l) {
                        lw[1]
                    } else {
                        lw[0]
                    }
                })
                .sum();
            let mut x = r.x;
            let mut lanes = Vec::new();
            for l in order {
                let lwid =
                    r.w * if self.mode.is_scratch(l) {
                        lw[1]
                    } else {
                        lw[0]
                    } / units;
                lanes.push(LaneGeom {
                    lane: l,
                    x,
                    w: lwid,
                });
                x += lwid;
            }
            let base = r.bottom() - gauge_h;
            let lane_len = base - r.y;
            let judge_y = base - ctx.config.play.lift * lane_len;
            fields.push(Field {
                player,
                rect: Rect::new(r.x, r.y, r.w, base - r.y),
                lanes,
                judge_y,
                gauge: Rect::new(r.x, base + 4.0 * ui, r.w, gauge_h - 8.0 * ui),
            });
        }
        let mut touch = Vec::new();
        if touch_on {
            let area = Rect::new(0.0, play_h, w, ctrl_h);
            if dp && ctx.config.touch.allow_dp {
                touch.push((Rect::new(area.x, area.y, area.w * 0.5, area.h), 0, false));
                touch.push((
                    Rect::new(area.x + area.w * 0.5, area.y, area.w * 0.5, area.h),
                    1,
                    true,
                ));
            } else {
                touch.push((area, 0, if dp { false } else { side2 }));
            }
        }
        let side = if portrait {
            None
        } else if dp {
            let l = fields[0].rect.right() + margin;
            let r = fields[1].rect.x - margin;
            (r - l > 80.0).then(|| Rect::new(l, 8.0 * ui, r - l, play_h - 16.0 * ui))
        } else {
            let f = &fields[0].rect;
            let (l, r) = if side2 {
                (margin, f.x - margin)
            } else {
                (f.right() + margin, w - margin)
            };
            (r - l > 80.0).then(|| Rect::new(l, 8.0 * ui, r - l, play_h - 16.0 * ui))
        };
        Layout {
            fields,
            touch,
            side,
        }
    }

    fn rebuild_zones(&mut self, ctx: &Ctx, lay: &Layout) {
        let key = (ctx.width as u32, ctx.height as u32, ctx.touch_enabled());
        if key == self.zones_key && !self.zones.is_empty() {
            return;
        }
        self.zones_key = key;
        self.zones.clear();
        for (area, player, right) in &lay.touch {
            let player = (*player).min(self.mode.players() - 1);
            self.zones.extend(touch::layout(
                self.mode,
                ctx.config.touch.style,
                *area,
                player,
                *right,
                ctx.theme.lane_width,
            ));
        }
    }

    fn note_color(&self, ctx: &Ctx, lane: usize) -> Color {
        let th = &ctx.theme;
        if self.mode.is_scratch(lane) {
            return th.note_scratch;
        }
        let k = lane % self.mode.lanes_per_player();
        if self.mode == PlayMode::Popn9K {
            return rgba(
                [
                    0xe8ecf2ff, 0xffd84aff, 0x4fe36bff, 0x4fa3ffff, 0xff5a5aff, 0x4fa3ffff,
                    0x4fe36bff, 0xffd84aff, 0xe8ecf2ff,
                ][k.min(8)],
            );
        }
        if k % 2 == 1 {
            th.note_blue
        } else {
            th.note_white
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
            autoplay: self.req.autoplay,
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

    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
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
                self.next_sample = t + (self.chart.length() / 120).max(250_000);
            }
            if engine.is_finished(t - offset) && self.finished_at.is_none() {
                self.finished_at = Some(t);
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
                    if judge <= GREAT {
                        self.bombs.push((lane, now, judge));
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
        ctx.mixer.stop_bus(Bus::Bgm);
        ctx.mixer.stop_bus(Bus::Key);
        for t in self.bga_tex.drain(..).flatten() {
            gfx.free_texture(t);
        }
        if let Some(t) = self.stage_tex.take() {
            gfx.free_texture(t);
        }
        ctx.save_config();
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let ui = ctx.ui();
        let th = ctx.theme.clone();
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
                th.text_dim,
                Align::Center,
                w * 0.9,
            );
            d.text_fit(
                &s.full_title(),
                w * 0.5,
                h * 0.36,
                34.0 * ui,
                th.text,
                Align::Center,
                w * 0.9,
            );
            d.text_fit(
                &s.artist,
                w * 0.5,
                h * 0.36 + 48.0 * ui,
                20.0 * ui,
                th.text_dim,
                Align::Center,
                w * 0.9,
            );
            if let Some(c) = &self.req.course {
                d.text(
                    &format!("{}  STAGE {}/{}", c.name, c.index + 1, c.songs.len()),
                    w * 0.5,
                    h * 0.22,
                    22.0 * ui,
                    th.accent,
                    Align::Center,
                );
            }
            let total = self.total_files.load(Ordering::Relaxed).max(1);
            let done = self.progress.load(Ordering::Relaxed);
            let bar = Rect::new(w * 0.25, h * 0.62, w * 0.5, 6.0 * ui);
            d.b.rect(bar, with_alpha(th.text_dim, 0.3));
            d.b.rect(
                Rect::new(
                    bar.x,
                    bar.y,
                    bar.w * (done as f32 / total as f32).min(1.0),
                    bar.h,
                ),
                th.accent,
            );
            let dots = ".".repeat((self.loading_since.elapsed().as_millis() / 300 % 4) as usize);
            d.text(
                &format!("LOADING{dots}  {done}/{total}"),
                w * 0.5,
                bar.y + 16.0 * ui,
                16.0 * ui,
                th.text_dim,
                Align::Center,
            );
            return;
        }
        if self.engine.is_none() {
            return;
        }
        let lay = self.layout(ctx);
        self.rebuild_zones(ctx, &lay);
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        let t_vis = self.chart_us(now) + ctx.config.play.visual_offset_ms as i64 * 1000;
        let p = &ctx.config.play;
        let cur_pos = self.chart.timing.pos_at_time(t_vis as f64);
        let main_bpm = if self.chart.meta.main_bpm > 0.0 {
            self.chart.meta.main_bpm
        } else {
            130.0
        };
        let notes = engine.notes();

        // BGA / side panel
        if let Some(side) = lay.side {
            let bga_r = Rect::new(side.x, side.y, side.w, (side.w * 0.75).min(side.h * 0.62));
            d.b.rect(bga_r, rgba(0x000000ff));
            if p.bga {
                let poor =
                    self.poor_until.map(|u| now < u).unwrap_or(false) && self.bga[3].is_some();
                let layers: &[usize] = if poor { &[3] } else { &[0, 1, 2] };
                for &l in layers {
                    if let Some(Some(tex)) = self.bga[l].and_then(|i| self.bga_tex.get(i as usize))
                    {
                        d.image_fit(*tex, bga_r, 1.0);
                    }
                }
            }
            if !p.bga || self.bga.iter().all(|b| b.is_none()) {
                if let Some(t) = self.stage_tex {
                    d.image_fit(t, bga_r, 0.6);
                }
            }
            self.draw_info(
                ctx,
                d,
                Rect::new(
                    side.x,
                    bga_r.bottom() + 10.0 * ui,
                    side.w,
                    side.bottom() - bga_r.bottom() - 10.0 * ui,
                ),
                engine,
                t_vis,
            );
        }

        let note_h = (ctx.height * th.note_height).max(6.0);
        let dur_us = p.duration_ms as f64 * 1000.0;
        let mut flash = Vec::new();
        for f in &lay.fields {
            let lane_len = f.judge_y - f.rect.y;
            let cover_top = f.rect.y
                + if p.lane_cover || p.sudden > 0.0 {
                    p.sudden * lane_len
                } else {
                    0.0
                };
            let visible = (f.judge_y - cover_top).max(1.0);
            let ppb = visible as f64 / (dur_us / 1_000_000.0 * main_bpm / 60.0);
            let y_of = |pos: f64, time: i64| -> f32 {
                match p.hispeed_mode {
                    HiSpeedMode::Floating => f.judge_y - ((pos - cur_pos) * ppb) as f32,
                    HiSpeedMode::Constant => {
                        f.judge_y - ((time - t_vis) as f64 / dur_us * visible as f64) as f32
                    }
                }
            };
            // lanes
            d.b.rect(f.rect, th.lane_bg);
            for lg in &f.lanes {
                d.b.rect(Rect::new(lg.x, f.rect.y, 1.0, f.rect.h), th.lane_line);
                let pressed = engine.key_down(lg.lane) || self.touch.is_pressed(lg.lane as u8);
                if pressed {
                    d.b.gradient(
                        Rect::new(
                            lg.x,
                            f.rect.y + lane_len * 0.35,
                            lg.w,
                            f.judge_y - f.rect.y - lane_len * 0.35,
                        ),
                        with_alpha(th.key_beam, 0.0),
                        with_alpha(th.key_beam, 0.35),
                    );
                }
                if self.mode.is_scratch(lg.lane) {
                    d.b.rect(
                        Rect::new(lg.x, f.rect.y, lg.w, f.rect.h),
                        with_alpha(th.note_scratch, 0.04),
                    );
                }
            }
            d.b.rect(
                Rect::new(f.rect.right() - 1.0, f.rect.y, 1.0, f.rect.h),
                th.lane_line,
            );
            d.b.set_clip(Some(Rect::new(f.rect.x, f.rect.y, f.rect.w, f.rect.h)));
            // bar lines
            for bl in &self.chart.bar_lines {
                let y = y_of(bl.pos, bl.time);
                if y < f.rect.y - 2.0 {
                    if bl.time > t_vis {
                        break;
                    }
                    continue;
                }
                if y <= f.judge_y {
                    d.b.rect(
                        Rect::new(f.rect.x, y - 0.5, f.rect.w, 1.0),
                        with_alpha(th.text_dim, 0.45),
                    );
                }
            }
            // notes
            for lg in &f.lanes {
                let lane = lg.lane;
                let col = self.note_color(ctx, lane);
                let list = &self.lane_notes[lane];
                let cursor = self.lane_cursor[lane];
                let mut k = cursor;
                while k < list.len() {
                    let n = &notes[list[k]];
                    k += 1;
                    let cn = &self.chart.notes[n.chart_index];
                    let y = y_of(cn.pos, n.time);
                    if n.time > t_vis + 2_000_000 && y < f.rect.y - lane_len {
                        break;
                    }
                    let nx = lg.x + 1.0;
                    let nw = lg.w - 2.0;
                    match &cn.kind {
                        NoteKind::Long {
                            end_pos, end_time, ..
                        } => {
                            if n.is_done() {
                                continue;
                            }
                            let holding = engine.holding(lane) == Some(list[k - 1]);
                            let ye = y_of(*end_pos, *end_time);
                            let yh = if holding {
                                y.max(f.judge_y).min(f.judge_y)
                            } else {
                                y
                            };
                            if ye > f.judge_y + note_h && !holding && n.state != 0 {
                                continue;
                            }
                            let missed = n.state > 4;
                            let body_a = if holding {
                                th.ln_body_alpha + 0.3
                            } else if missed {
                                0.25
                            } else {
                                th.ln_body_alpha
                            };
                            let hcn = engine.hcn_active(lane);
                            let bc = if hcn { rgba(0x7fffd4ff) } else { col };
                            d.b.rect(
                                Rect::new(nx + nw * 0.12, ye, nw * 0.76, (yh - ye).max(0.0)),
                                with_alpha(bc, body_a.min(1.0)),
                            );
                            d.b.rect(
                                Rect::new(nx, ye - note_h, nw, note_h),
                                with_alpha(col, if missed { 0.4 } else { 1.0 }),
                            );
                            d.b.rect(
                                Rect::new(nx, yh - note_h, nw, note_h),
                                with_alpha(col, if missed { 0.4 } else { 1.0 }),
                            );
                        }
                        NoteKind::Mine { .. } => {
                            if y > f.judge_y + note_h || y < f.rect.y {
                                continue;
                            }
                            d.b.rect(
                                Rect::new(nx + nw * 0.2, y - note_h, nw * 0.6, note_h),
                                th.note_mine,
                            );
                        }
                        _ => {
                            if n.is_done() || y < f.rect.y - note_h || y > f.rect.bottom() + note_h
                            {
                                continue;
                            }
                            d.b.rect(Rect::new(nx, y - note_h, nw, note_h), col);
                        }
                    }
                }
            }
            d.b.set_clip(None);
            // lane covers
            if p.lane_cover || p.sudden > 0.0 {
                let r = Rect::new(f.rect.x, f.rect.y, f.rect.w, cover_top - f.rect.y);
                d.b.rect(r, rgba(0x1a1d26ff));
                let gn = format!("GN {}  {}ms", p.duration_ms * 3 / 5, p.duration_ms);
                d.text(
                    &gn,
                    f.rect.x + f.rect.w * 0.5,
                    (cover_top - 24.0 * ui).max(f.rect.y),
                    16.0 * ui,
                    rgba(0x6fff6fff),
                    Align::Center,
                );
            }
            if p.hidden > 0.0 {
                let top = f.judge_y - p.hidden * lane_len;
                d.b.rect(
                    Rect::new(f.rect.x, top, f.rect.w, f.judge_y - top),
                    rgba(0x1a1d26ff),
                );
            }
            if p.lift > 0.0 {
                d.b.rect(
                    Rect::new(f.rect.x, f.judge_y, f.rect.w, f.rect.bottom() - f.judge_y),
                    rgba(0x101218ff),
                );
            }
            // judge line + bombs
            d.b.rect(
                Rect::new(f.rect.x, f.judge_y - 2.0, f.rect.w, 4.0),
                th.judge_line,
            );
            for (lane, t, j) in &self.bombs {
                if let Some(lg) = f.lanes.iter().find(|l| l.lane == *lane as usize) {
                    let a = 1.0 - now.duration_since(*t).as_secs_f32() / 0.25;
                    let c = if *j == 9 {
                        th.note_mine
                    } else if *j == PGREAT {
                        rgba(0xfff3a0ff)
                    } else {
                        rgba(0xa0e8ffff)
                    };
                    let hgt = lane_len * 0.22 * (0.6 + 0.4 * a);
                    d.b.gradient(
                        Rect::new(lg.x, f.judge_y - hgt, lg.w, hgt),
                        with_alpha(c, 0.0),
                        with_alpha(c, a * 0.7),
                    );
                    let s = lg.w * (0.6 + 0.8 * (1.0 - a));
                    d.b.circle(
                        lg.x + lg.w * 0.5,
                        f.judge_y,
                        s * 0.5,
                        s * 0.38,
                        with_alpha(c, a),
                        24,
                    );
                }
            }
            for lg in &f.lanes {
                if let Some(Some(t)) = self.key_flash.get(lg.lane) {
                    let a = 1.0 - now.duration_since(*t).as_secs_f32() / 0.12;
                    if a > 0.0 {
                        flash.push((Rect::new(lg.x, f.judge_y - 3.0, lg.w, 6.0), a));
                    }
                }
            }
            // judge text
            if let Some((j, diff, t, combo)) = self.last_judge[f.player.min(1)] {
                let age = now.duration_since(t).as_secs_f32();
                if age < 0.6 {
                    let (label, c) = match j {
                        PGREAT => (
                            "GREAT",
                            if (now.elapsed().as_millis() / 50) % 2 == 0 {
                                rgba(0xfffbd0ff)
                            } else {
                                rgba(0x9ff6ffff)
                            },
                        ),
                        GREAT => ("GREAT", rgba(0xffe14dff)),
                        GOOD => ("GOOD", rgba(0xffe14dff)),
                        BAD => ("BAD", rgba(0xff6b6bff)),
                        POOR | MISS => ("POOR", rgba(0xff6b6bff)),
                        _ => ("", th.text),
                    };
                    let jy = f.rect.y + lane_len * 0.62;
                    let text = if j <= GOOD && combo > 0 {
                        format!("{label} {combo}")
                    } else {
                        label.to_string()
                    };
                    d.text_shadow(
                        &text,
                        f.rect.x + f.rect.w * 0.5,
                        jy,
                        30.0 * ui,
                        c,
                        Align::Center,
                    );
                    if p.show_fast_slow && (GREAT..=BAD).contains(&j) {
                        let (fs, fc) = if diff > 0 {
                            ("FAST", rgba(0x4fc3f7ff))
                        } else {
                            ("SLOW", rgba(0xff8a65ff))
                        };
                        d.text_shadow(
                            &format!("{fs} {:+.1}ms", -(diff as f64) / 1000.0),
                            f.rect.x + f.rect.w * 0.5,
                            jy - 26.0 * ui,
                            16.0 * ui,
                            fc,
                            Align::Center,
                        );
                    }
                }
            }
            // gauge
            let g = engine.gauge.current();
            let gt = g.ty;
            let segs = 50;
            let sw = f.gauge.w / segs as f32;
            let filled = (g.value() / 2.0).floor() as i32;
            for s in 0..segs {
                let on = s < filled;
                let border = g.elem.border;
                let c = match gt {
                    GaugeType::Hard | GaugeType::ExClass | GaugeType::Class => rgba(0xff4040ff),
                    GaugeType::ExHard | GaugeType::ExHardClass | GaugeType::Hazard => {
                        rgba(0xffd23fff)
                    }
                    GaugeType::AssistEasy | GaugeType::Easy => {
                        if (s as f32 + 1.0) * 2.0 > border {
                            rgba(0xff4040ff)
                        } else {
                            rgba(0xb070ffff)
                        }
                    }
                    GaugeType::Normal => {
                        if (s as f32 + 1.0) * 2.0 > border {
                            rgba(0xff4040ff)
                        } else {
                            rgba(0x3fa0ffff)
                        }
                    }
                };
                d.b.rect(
                    Rect::new(
                        f.gauge.x + s as f32 * sw + 0.5,
                        f.gauge.y,
                        sw - 1.0,
                        f.gauge.h,
                    ),
                    with_alpha(c, if on { 1.0 } else { 0.18 }),
                );
            }
            d.text_shadow(
                &format!("{} {:.1}%", gt.label(), g.value()),
                f.gauge.right(),
                f.gauge.y - 18.0 * ui,
                14.0 * ui,
                th.text,
                Align::Right,
            );
        }
        for (r, a) in flash {
            d.b.rect(r, with_alpha(th.key_beam, a));
        }
        // touch controls
        if ctx.touch_enabled() {
            for (area, _, _) in &lay.touch {
                d.b.rect(*area, rgba(0x05060aff));
            }
            self.touch.draw(
                d.b,
                &self.zones,
                ctx.config.touch.opacity,
                th.note_white,
                th.note_blue,
                th.note_scratch,
            );
        }
        // HUD buttons (touch friendly)
        self.hud_rects.clear();
        let bw = 54.0 * ui;
        let bh = 34.0 * ui;
        let labels = ["HS−", "HS+", "SUD−", "SUD+", "×"];
        let base_x = if let Some(s) = lay.side {
            s.right() - bw * labels.len() as f32
        } else {
            ctx.width - bw * labels.len() as f32 - 8.0
        };
        for (i, l) in labels.iter().enumerate() {
            let r = Rect::new(base_x + i as f32 * bw, 6.0 * ui, bw - 4.0, bh);
            if ctx.touch_enabled() || lay.side.is_some() {
                ui::button(d, r, l, &th, ui * 0.9, false);
                self.hud_rects.push((r, i));
            }
        }
        // fades
        match self.stage {
            Stage::Ending { since, failed } => {
                let a = (now.duration_since(since).as_secs_f32() / 1.2).min(1.0);
                d.b.rect(
                    Rect::new(0.0, 0.0, ctx.width, ctx.height),
                    with_alpha(rgba(0x000000ff), a * 0.85),
                );
                let msg = if failed {
                    "FAILED"
                } else if engine.score.count(BAD) + engine.score.count(POOR) == 0 {
                    "FULL COMBO"
                } else {
                    "CLEAR"
                };
                let c = if failed { rgba(0xff5050ff) } else { th.accent };
                d.text_shadow(
                    msg,
                    ctx.width * 0.5,
                    ctx.height * 0.42,
                    48.0 * ui,
                    with_alpha(c, a.max(0.3)),
                    Align::Center,
                );
            }
            _ => {
                if t_vis < 0 {
                    let a = (-t_vis as f32 / 1_500_000.0).min(1.0);
                    d.text_shadow(
                        &self.chart.meta.title,
                        ctx.width * 0.5,
                        ctx.height * 0.3,
                        28.0 * ui,
                        with_alpha(th.text, a),
                        Align::Center,
                    );
                }
            }
        }
        // advance render cursors (notes long gone)
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

impl PlayScene {
    fn draw_info(&self, ctx: &Ctx, d: &mut Draw, r: Rect, e: &Engine, t: i64) {
        let ui = ctx.ui();
        let th = &ctx.theme;
        if r.h < 40.0 {
            return;
        }
        ui::panel(d, r, th);
        let x = r.x + 12.0 * ui;
        let mut y = r.y + 10.0 * ui;
        let mw = r.w - 24.0 * ui;
        let m = &self.chart.meta;
        d.text_fit(
            &format!("{} {}", m.title, m.subtitle),
            x,
            y,
            20.0 * ui,
            th.text,
            Align::Left,
            mw,
        );
        y += 26.0 * ui;
        d.text_fit(&m.artist, x, y, 15.0 * ui, th.text_dim, Align::Left, mw);
        y += 24.0 * ui;
        let bpm = self.chart.timing.bpm_at_time(t.max(0) as f64);
        let s = &e.score;
        let len = self.chart.length().max(1);
        let lines = [
            format!(
                "{}  LV {}  BPM {:.0}  {}",
                self.mode.label(),
                m.level,
                bpm,
                self.options_label
            ),
            format!(
                "EX SCORE {}   {:.2}%   {}",
                s.ex_score(),
                s.current_rate() * 100.0,
                vibebms_core::score::DjLevel::from_rate(s.ex_score(), s.passed * 2).label()
            ),
            format!(
                "PG {}  GR {}  GD {}  BD {}  PR {}",
                s.count(PGREAT),
                s.count(GREAT),
                s.count(GOOD),
                s.count(BAD),
                s.count(POOR) + s.count(MISS)
            ),
            format!(
                "COMBO {} / MAX {}   FAST {} SLOW {}",
                e.combo(),
                s.max_combo,
                s.fast(),
                s.slow()
            ),
            format!(
                "OFFSET {:+}ms   {}",
                ctx.config.play.judge_offset_ms,
                if self.req.autoplay { "AUTOPLAY" } else { "" }
            ),
        ];
        for l in &lines {
            if y + 20.0 * ui > r.bottom() {
                break;
            }
            d.text_fit(l, x, y, 16.0 * ui, th.text, Align::Left, mw);
            y += 22.0 * ui;
        }
        let pr = Rect::new(x, r.bottom() - 10.0 * ui, mw, 4.0 * ui);
        d.b.rect(pr, with_alpha(th.text_dim, 0.3));
        d.b.rect(
            Rect::new(
                pr.x,
                pr.y,
                pr.w * (t.max(0) as f32 / len as f32).min(1.0),
                pr.h,
            ),
            th.accent,
        );
    }
}
