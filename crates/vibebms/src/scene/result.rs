//! Result screen: lamp, score, judge breakdown, gauge graph, timing histogram,
//! saving best scores and IR submission.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Gfx, Rect};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::library::BestScore;
use crate::platform::{Duration, Instant};
use crate::scene::play::{CourseRun, PlayRequest, PlayScene};
use crate::theme::Theme;
use crate::ui;
use std::sync::mpsc::{channel, Receiver};
use vibebms_core::gauge::GaugeType;
use vibebms_core::judge::JUDGE_NAMES;
use vibebms_core::score::{ClearType, Score};
use vibebms_core::{LnType, PlayMode};
use winit::keyboard::KeyCode;

pub struct ResultData {
    pub request: PlayRequest,
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub level: String,
    pub mode: PlayMode,
    pub ln: LnType,
    pub has_ln: bool,
    pub score: Score,
    pub clear: ClearType,
    pub gauge_type: GaugeType,
    pub gauge_value: f32,
    pub gauge_log: Vec<f32>,
    pub timing_log: Vec<(i64, i64)>,
    pub autoplay: bool,
    pub options: String,
    pub judge: String,
    pub seed: i64,
    pub course: Option<CourseRun>,
}

pub struct ResultScene {
    d: ResultData,
    before: Option<BestScore>,
    after: Option<BestScore>,
    ir_rx: Option<Receiver<String>>,
    ir_status: String,
    created: Instant,
    buttons: Vec<(Rect, usize)>,
}

impl ResultScene {
    pub fn new(ctx: &mut Ctx, d: ResultData) -> ResultScene {
        let song = &d.request.song;
        let ln = if d.has_ln { d.ln as i32 } else { 0 };
        let before = ctx.best(song, ln).cloned();
        let mut after = None;
        let mut ir_rx = None;
        let mut ir_status = String::new();
        if !d.autoplay {
            if let Some(lib) = ctx.lib.as_mut() {
                let b = lib.record_play(&song.sha256, ln, &d.score, d.clear);
                ctx.scores.insert((song.sha256.clone(), ln), b.clone());
                after = Some(b);
            }
            if let Some(cfg) = ctx
                .config
                .ir
                .iter()
                .find(|c| c.enabled && c.auto_submit)
                .cloned()
            {
                let chart = vibebms_ir::ChartRef {
                    md5: song.md5.clone(),
                    sha256: song.sha256.clone(),
                    title: song.title.clone(),
                    artist: song.artist.clone(),
                    mode: song.mode.clone(),
                    ln_type: format!("{:?}", d.ln).to_uppercase(),
                    notes: d.score.total_notes,
                };
                let mut sub = vibebms_ir::ScoreSubmission::from_score(&chart, &d.score, d.clear);
                sub.option = d.options.clone();
                sub.seed = d.seed;
                sub.gauge = d.gauge_type.label().into();
                sub.judge_preset = d.judge.clone();
                sub.device = if ctx.touch_enabled() {
                    "TOUCH".into()
                } else {
                    "KEYBOARD/CONTROLLER".into()
                };
                let (tx, rx) = channel();
                ir_rx = Some(rx);
                ir_status = format!("{}: sending…", cfg.name);
                let name = cfg.name.clone();
                match vibebms_ir::create_client(&cfg)
                    .map(|c| (c.can_submit(), c.submit_request(&sub)))
                {
                    Some((true, Ok(req))) => crate::platform::ir_send(req, move |r| {
                        let _ = tx.send(match r {
                            Ok(_) => format!("{name}: submitted"),
                            Err(e) => format!("{name}: {e}"),
                        });
                    }),
                    Some((_, Err(e))) => {
                        let _ = tx.send(format!("{name}: {e}"));
                    }
                    Some((false, _)) => {
                        let _ = tx.send(format!("{name}: score submission not supported"));
                    }
                    None => {}
                }
            }
        }
        ResultScene {
            d,
            before,
            after,
            ir_rx,
            ir_status,
            created: Instant::now(),
            buttons: Vec::new(),
        }
    }

    fn next_stage(&self) -> bool {
        matches!(&self.d.course, Some(c) if c.index + 1 < c.songs.len() && self.d.clear != ClearType::Failed)
    }

    fn proceed(&mut self, ctx: &mut Ctx) -> Next {
        if self.created.elapsed() < Duration::from_millis(500) {
            return Next::Stay;
        }
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
                },
            )));
        }
        Next::Pop
    }

    fn retry(&mut self, ctx: &mut Ctx) -> Next {
        if self.d.course.is_some() {
            return Next::Stay;
        }
        Next::Replace(Box::new(PlayScene::new(ctx, self.d.request.clone())))
    }
}

impl Scene for ResultScene {
    fn update(&mut self, _ctx: &mut Ctx, _gfx: &mut Gfx, _now: Instant) -> Next {
        if let Some(rx) = &self.ir_rx {
            if let Ok(m) = rx.try_recv() {
                self.ir_status = m;
                self.ir_rx = None;
            }
        }
        Next::Stay
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
                    return self.retry(ctx);
                }
                match nav_from_key(*code, &ctx.config.keys) {
                    Some(Nav::Decide) | Some(Nav::Back) => return self.proceed(ctx),
                    _ => {}
                }
            }
            InputEv::Pad {
                id, pressed: true, ..
            } => {
                if let Some(Nav::Decide | Nav::Back) = nav_from_pad(*id, &ctx.config.keys) {
                    return self.proceed(ctx);
                }
            }
            InputEv::Pointer {
                phase: Phase::Up,
                x,
                y,
                ..
            } => match ui::hit(&self.buttons, *x, *y) {
                Some(0) => return self.proceed(ctx),
                Some(1) => return self.retry(ctx),
                _ => {}
            },
            _ => {}
        }
        Next::Stay
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, _now: Instant) {
        let ui = ctx.ui();
        let th = ctx.theme.clone();
        let (w, h) = (ctx.width, ctx.height);
        let s = &self.d.score;
        let pad = 16.0 * ui;
        let portrait = ctx.portrait();
        let left = if portrait {
            Rect::new(pad, pad, w - 2.0 * pad, h * 0.5 - pad)
        } else {
            Rect::new(pad, pad, w * 0.5 - 1.5 * pad, h - 2.0 * pad - 60.0 * ui)
        };
        let right = if portrait {
            Rect::new(
                pad,
                h * 0.5 + pad * 0.5,
                w - 2.0 * pad,
                h * 0.5 - 2.0 * pad - 60.0 * ui,
            )
        } else {
            Rect::new(
                w * 0.5 + 0.5 * pad,
                pad,
                w * 0.5 - 1.5 * pad,
                h - 2.0 * pad - 60.0 * ui,
            )
        };
        ui::panel(d, left, &th);
        ui::panel(d, right, &th);
        let x = left.x + 16.0 * ui;
        let mut y = left.y + 14.0 * ui;
        let mw = left.w - 32.0 * ui;
        if let Some(c) = &self.d.course {
            d.text(
                &format!("{}  STAGE {}/{}", c.name, c.index + 1, c.songs.len()),
                x,
                y,
                16.0 * ui,
                th.accent,
                Align::Left,
            );
            y += 24.0 * ui;
        }
        d.text_fit(
            &format!("{} {}", self.d.title, self.d.subtitle),
            x,
            y,
            24.0 * ui,
            th.text,
            Align::Left,
            mw,
        );
        y += 30.0 * ui;
        d.text_fit(
            &format!(
                "{}   {}  LV {}",
                self.d.artist,
                self.d.mode.label(),
                self.d.level
            ),
            x,
            y,
            15.0 * ui,
            th.text_dim,
            Align::Left,
            mw,
        );
        y += 34.0 * ui;
        let lamp_c = Theme::lamp(self.d.clear);
        d.text_shadow(self.d.clear.label(), x, y, 40.0 * ui, lamp_c, Align::Left);
        d.text_shadow(
            s.dj_level().label(),
            left.right() - 16.0 * ui,
            y,
            40.0 * ui,
            th.text,
            Align::Right,
        );
        y += 56.0 * ui;
        let best_ex = self.before.as_ref().map(|b| b.ex_score).unwrap_or(0);
        let diff = s.ex_score() as i64 - best_ex as i64;
        d.text(
            &format!("EX SCORE  {}", s.ex_score()),
            x,
            y,
            24.0 * ui,
            th.text,
            Align::Left,
        );
        d.text(
            &format!("{:+}", diff),
            left.right() - 16.0 * ui,
            y + 4.0 * ui,
            18.0 * ui,
            if diff >= 0 {
                rgba(0x7fff7fff)
            } else {
                rgba(0xff7f7fff)
            },
            Align::Right,
        );
        y += 32.0 * ui;
        d.text(
            &format!("RATE  {:.2}%", s.rate() * 100.0),
            x,
            y,
            20.0 * ui,
            th.text,
            Align::Left,
        );
        y += 30.0 * ui;
        let rows = [
            (0usize, rgba(0xfff3a0ff)),
            (1, rgba(0xffe14dff)),
            (2, rgba(0x9be564ff)),
            (3, rgba(0xff8a65ff)),
            (4, rgba(0xff5a5aff)),
            (5, rgba(0xc05050ff)),
        ];
        for (j, c) in rows {
            let name = if j == 5 { "EMPTY POOR" } else { JUDGE_NAMES[j] };
            d.text(name, x, y, 18.0 * ui, c, Align::Left);
            d.text(
                &format!("{}", s.count(j)),
                x + 220.0 * ui,
                y,
                18.0 * ui,
                th.text,
                Align::Right,
            );
            if j > 0 && j < 4 {
                d.text(
                    &format!("F{} / S{}", s.counts[j][0], s.counts[j][1]),
                    x + 330.0 * ui,
                    y + 2.0 * ui,
                    14.0 * ui,
                    th.text_dim,
                    Align::Right,
                );
            }
            y += 25.0 * ui;
        }
        y += 6.0 * ui;
        d.text(
            &format!(
                "MAX COMBO {}   MISS COUNT {}   FAST {} / SLOW {}",
                s.max_combo,
                s.miss_count(),
                s.fast(),
                s.slow()
            ),
            x,
            y,
            16.0 * ui,
            th.text,
            Align::Left,
        );
        y += 24.0 * ui;
        if let Some(b) = &self.after {
            d.text_fit(
                &format!(
                    "BEST: {}  EX {}  BP {}   PLAYS {}",
                    b.clear.label(),
                    b.ex_score,
                    if b.min_bp == u32::MAX { 0 } else { b.min_bp },
                    b.play_count
                ),
                x,
                y,
                15.0 * ui,
                th.text_dim,
                Align::Left,
                mw,
            );
            y += 22.0 * ui;
        }
        if self.d.autoplay {
            d.text(
                "AUTOPLAY (not saved)",
                x,
                y,
                15.0 * ui,
                rgba(0xffd23fff),
                Align::Left,
            );
            y += 22.0 * ui;
        }
        if !self.ir_status.is_empty() {
            d.text_fit(&self.ir_status, x, y, 15.0 * ui, th.accent, Align::Left, mw);
        }
        // right: gauge graph + timing histogram + options
        let gx = right.x + 16.0 * ui;
        let gw = right.w - 32.0 * ui;
        let mut gy = right.y + 14.0 * ui;
        d.text(
            &format!(
                "GAUGE  {}  {:.1}%",
                self.d.gauge_type.label(),
                self.d.gauge_value
            ),
            gx,
            gy,
            16.0 * ui,
            th.text,
            Align::Left,
        );
        gy += 24.0 * ui;
        let gh = right.h * 0.36;
        let gr = Rect::new(gx, gy, gw, gh);
        d.b.rect(gr, rgba(0x05060aff));
        d.b.rect(
            Rect::new(gr.x, gr.y + gr.h * 0.2, gr.w, 1.0),
            with_alpha(th.text_dim, 0.4),
        );
        let n = self.d.gauge_log.len();
        if n > 1 {
            let col = match self.d.gauge_type {
                GaugeType::Hard | GaugeType::Class | GaugeType::ExClass => rgba(0xff4040ff),
                GaugeType::ExHard | GaugeType::ExHardClass | GaugeType::Hazard => rgba(0xffd23fff),
                GaugeType::Easy | GaugeType::AssistEasy => rgba(0xb070ffff),
                GaugeType::Normal => rgba(0x3fa0ffff),
            };
            let bw = gr.w / n as f32;
            for (i, v) in self.d.gauge_log.iter().enumerate() {
                let hh = gr.h * (v / 100.0).clamp(0.0, 1.0);
                d.b.rect(
                    Rect::new(gr.x + i as f32 * bw, gr.bottom() - hh, bw.max(1.0), hh),
                    with_alpha(col, 0.85),
                );
            }
        }
        gy += gh + 16.0 * ui;
        // timing histogram (-60..+60 ms, 2 ms bins)
        d.text(
            "TIMING (FAST ← → SLOW)",
            gx,
            gy,
            15.0 * ui,
            th.text,
            Align::Left,
        );
        gy += 22.0 * ui;
        let hh = right.h * 0.26;
        let hr = Rect::new(gx, gy, gw, hh);
        d.b.rect(hr, rgba(0x05060aff));
        let bins = 61;
        let mut hist = vec![0u32; bins];
        let mut sum = 0i64;
        for (_, diff) in &self.d.timing_log {
            let ms = -diff / 1000; // negative = early
            sum += ms;
            let b = (ms + 60) / 2;
            if (0..bins as i64).contains(&b) {
                hist[b as usize] += 1;
            }
        }
        let maxc = hist.iter().copied().max().unwrap_or(1).max(1);
        let bw = hr.w / bins as f32;
        for (i, c) in hist.iter().enumerate() {
            let hgt = hr.h * (*c as f32 / maxc as f32);
            let col = if i < bins / 2 {
                rgba(0x4fc3f7ff)
            } else if i > bins / 2 {
                rgba(0xff8a65ff)
            } else {
                rgba(0xfff3a0ff)
            };
            d.b.rect(
                Rect::new(hr.x + i as f32 * bw, hr.bottom() - hgt, bw - 1.0, hgt),
                col,
            );
        }
        d.b.rect(
            Rect::new(hr.x + hr.w * 0.5, hr.y, 1.0, hr.h),
            with_alpha(th.text, 0.5),
        );
        let avg = if self.d.timing_log.is_empty() {
            0.0
        } else {
            sum as f64 / self.d.timing_log.len() as f64
        };
        gy += hh + 8.0 * ui;
        d.text(
            &format!("AVG {:+.1} ms  (+ = late)", avg),
            gx,
            gy,
            15.0 * ui,
            th.text_dim,
            Align::Left,
        );
        gy += 22.0 * ui;
        d.text_fit(
            &format!("{}   {}", self.d.options, self.d.judge),
            gx,
            gy,
            14.0 * ui,
            th.text_dim,
            Align::Left,
            gw,
        );
        if let Some(c) = &self.d.course {
            if !self.next_stage() {
                gy += 22.0 * ui;
                let total: u32 = c.stages.iter().map(|s| s.1.ex_score()).sum();
                let max: u32 = c.stages.iter().map(|s| s.1.max_ex()).sum();
                let cleared = c.stages.len() == c.songs.len()
                    && c.stages.iter().all(|s| s.2 != ClearType::Failed);
                d.text(
                    &format!(
                        "COURSE {}  EX {} / {}",
                        if cleared { "CLEARED" } else { "FAILED" },
                        total,
                        max
                    ),
                    gx,
                    gy,
                    17.0 * ui,
                    if cleared { th.accent } else { rgba(0xff5050ff) },
                    Align::Left,
                );
            }
        }
        // buttons
        self.buttons.clear();
        let by = h - 56.0 * ui;
        let label = if self.next_stage() {
            "NEXT STAGE"
        } else {
            "OK"
        };
        self.buttons.push((
            ui::button(
                d,
                Rect::new(w * 0.5 - 210.0 * ui, by, 200.0 * ui, 44.0 * ui),
                label,
                &th,
                ui,
                true,
            ),
            0,
        ));
        if self.d.course.is_none() {
            self.buttons.push((
                ui::button(
                    d,
                    Rect::new(w * 0.5 + 10.0 * ui, by, 200.0 * ui, 44.0 * ui),
                    "RETRY (R)",
                    &th,
                    ui,
                    false,
                ),
                1,
            ));
        }
    }
}
