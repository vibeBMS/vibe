//! Result screen: lamp, score, judge breakdown, gauge graph, timing histogram,
//! saving best scores and IR submission.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::gfx::text::Align;
use crate::gfx::{Gfx, Rect};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::library::BestScore;
use crate::platform::{Duration, Instant};
use crate::scene::play::{CourseRun, PlayRequest, PlayScene};
use crate::scene::result_skin::{ResultHooks, ResultState, EVENT_CONTINUE, EVENT_RETRY};
use crate::scene::skinned;
use crate::skin::render::LoadedSkin;
use crate::skin::SkinType;
use std::sync::mpsc::{channel, Receiver};
use vibebms_core::gauge::GaugeType;
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
    /// Note judge windows (late, early) in µs, PG first.
    pub windows: Vec<(i64, i64)>,
    pub judge_log: Vec<(i64, u8, i8)>,
    /// beatoraja notes graph columns (note type / judge / FAST-SLOW).
    pub notes_graph: [Vec<super::graphs::Column>; 3],
    pub meta: vibebms_core::chart::ChartMeta,
    /// Chart length (us) and BPM changes (time us, bpm) for the graphs.
    pub length_us: i64,
    pub bpm_log: Vec<(i64, f64)>,
    pub autoplay: bool,
    pub options: String,
    pub judge: String,
    pub seed: i64,
    pub course: Option<CourseRun>,
}

pub struct ResultScene {
    pub(crate) d: ResultData,
    pub(crate) before: Option<BestScore>,
    pub(crate) after: Option<BestScore>,
    ir_rx: Option<Receiver<String>>,
    ir_status: String,
    pub(crate) created: Instant,
    /// Clickable skin objects from the last frame: (rect, event).
    clicks: Vec<(Rect, i32)>,
    pub(crate) skin: Option<LoadedSkin>,
    skin_tried: bool,
    /// The chart's stage file (skin reference image 100).
    pub(crate) stage_tex: Option<crate::gfx::TexId>,
    /// Leaving (skin fade-out running): when it started and what follows.
    pub(crate) leaving: Option<Instant>,
    leave_to: Option<Leave>,
}

#[derive(Clone, Copy)]
enum Leave {
    Proceed,
    Retry,
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
            clicks: Vec::new(),
            skin: None,
            skin_tried: false,
            stage_tex: None,
            leaving: None,
            leave_to: None,
        }
    }

    /// Start leaving: with a skin, its fade-out runs first.
    fn leave(&mut self, ctx: &mut Ctx, to: Leave) -> Next {
        if self.created.elapsed() < Duration::from_millis(500) || self.leaving.is_some() {
            return Next::Stay;
        }
        if matches!(to, Leave::Retry) && self.d.course.is_some() {
            return Next::Stay;
        }
        let fade = self.skin.as_ref().map(|s| s.skin.fadeout_ms).unwrap_or(0);
        if fade > 0 {
            self.leaving = Some(Instant::now());
            self.leave_to = Some(to);
            return Next::Stay;
        }
        self.go(ctx, to)
    }

    fn go(&mut self, ctx: &mut Ctx, to: Leave) -> Next {
        match to {
            Leave::Proceed => self.proceed(ctx),
            Leave::Retry => self.retry(ctx),
        }
    }

    fn next_stage(&self) -> bool {
        matches!(&self.d.course, Some(c) if c.index + 1 < c.songs.len() && self.d.clear != ClearType::Failed)
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
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        if let Some(rx) = &self.ir_rx {
            if let Ok(m) = rx.try_recv() {
                self.ir_status = m;
                self.ir_rx = None;
            }
        }
        if !self.skin_tried {
            self.skin_tried = true;
            if !self.d.meta.stagefile.is_empty() {
                let p = std::path::Path::new(&self.d.request.song.path)
                    .parent()
                    .unwrap_or(std::path::Path::new(""))
                    .join(&self.d.meta.stagefile);
                self.stage_tex = crate::scene::select::resolve_image(&p)
                    .and_then(|rp| gfx.load_image(&rp, false));
            }
            let kind = if self.d.course.is_some() && !self.next_stage() {
                SkinType::CourseResult
            } else {
                SkinType::Result
            };
            let skin = {
                let st = ResultState { s: self, ctx, now };
                let stat = |id: i32| st.static_option(id);
                skinned::load(ctx, gfx, kind, &stat)
            };
            self.skin = skin;
        }
        // events skin scripts asked for
        let events = self.skin.as_ref().map(|s| s.script_events());
        for ev in events.unwrap_or_default() {
            match ev {
                EVENT_RETRY => return self.leave(ctx, Leave::Retry),
                EVENT_CONTINUE => return self.leave(ctx, Leave::Proceed),
                _ => {}
            }
        }
        if let (Some(t), Some(to)) = (self.leaving, self.leave_to) {
            let fade = self.skin.as_ref().map(|s| s.skin.fadeout_ms).unwrap_or(0);
            if now.duration_since(t).as_millis() as i64 >= fade {
                self.leave_to = None;
                return self.go(ctx, to);
            }
        }
        Next::Stay
    }

    fn exit(&mut self, _ctx: &mut Ctx, gfx: &mut Gfx) {
        if let Some(mut s) = self.skin.take() {
            s.free(gfx);
        }
        if let Some(t) = self.stage_tex.take() {
            gfx.free_texture(t);
        }
    }

    fn realtime(&self) -> bool {
        // skin animations need every frame
        self.skin.is_some()
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
                match nav_from_key(*code, &ctx.config.keys) {
                    Some(Nav::Decide) | Some(Nav::Back) => return self.leave(ctx, Leave::Proceed),
                    _ => {}
                }
            }
            InputEv::Pad {
                id, pressed: true, ..
            } => {
                if let Some(Nav::Decide | Nav::Back) = nav_from_pad(*id, &ctx.config.keys) {
                    return self.leave(ctx, Leave::Proceed);
                }
            }
            InputEv::Pointer {
                phase: Phase::Up,
                x,
                y,
                ..
            } => {
                // skin buttons, else a tap anywhere continues
                let act = self
                    .clicks
                    .iter()
                    .rev()
                    .find(|(r, _)| r.contains(*x, *y))
                    .map(|c| c.1);
                if let (Some(a), Some(sk)) = (act, self.skin.as_ref()) {
                    if sk.click(a) {
                        return Next::Stay; // a skin script event
                    }
                }
                return match act {
                    Some(EVENT_RETRY) => self.leave(ctx, Leave::Retry),
                    _ => self.leave(ctx, Leave::Proceed),
                };
            }
            _ => {}
        }
        Next::Stay
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let view = Rect::new(0.0, 0.0, ctx.width, ctx.height);
        let clicks = {
            let st = ResultState { s: self, ctx, now };
            let mut hooks = ResultHooks {
                st: &st,
                clicks: Vec::new(),
            };
            let ms = now.saturating_duration_since(self.created).as_millis() as i64;
            if let Some(sk) = self.skin.as_ref() {
                sk.draw(d, view, ms, &st, &mut hooks);
            }
            hooks.clicks
        };
        self.clicks = clicks;
        if !self.ir_status.is_empty() {
            let ui = ctx.ui();
            d.text(
                &self.ir_status,
                12.0 * ui,
                ctx.height - 26.0 * ui,
                14.0 * ui,
                ctx.theme.accent,
                Align::Left,
            );
        }
    }
}
