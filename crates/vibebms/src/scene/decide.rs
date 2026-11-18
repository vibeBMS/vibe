//! The decide screen: a skin shown between choosing a chart and playing it
//! (LR2 / beatoraja skin type 6), when one is configured.

use super::play::{PlayRequest, PlayScene};
use super::skin_host::SkinHost;
use super::skinned;
use super::skinvals::*;
use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::Bpms;
use crate::gfx::{Gfx, ImageTex, Rect, TexId};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::library::{BestScore, Song};
use crate::platform::Instant;
use crate::skin::render::Hooks;
use crate::skin::{SkinState, SkinType};
use crate::sysound::Sound;
use std::cell::OnceCell;

/// How long the screen shows when the skin does not say (beatoraja's
/// default decide skin).
const SCENE_MS: i64 = 3000;

/// The screen to go to once a chart is chosen: the decide screen when a
/// decide skin is configured, else straight to play.
pub fn start(ctx: &mut Ctx, req: PlayRequest) -> Box<dyn Scene> {
    if skinned::configured_skin(ctx, SkinType::Decide).is_some() {
        Box::new(DecideScene::new(req))
    } else {
        Box::new(PlayScene::new(ctx, req))
    }
}

pub struct DecideScene {
    req: Option<PlayRequest>,
    /// (Play follows when the fade-out ends.)
    host: SkinHost,
    stage_tex: Option<ImageTex>,
    banner_tex: Option<ImageTex>,
}

impl DecideScene {
    pub(super) fn new(req: PlayRequest) -> DecideScene {
        DecideScene {
            req: Some(req),
            host: SkinHost::new(),
            stage_tex: None,
            banner_tex: None,
        }
    }

    fn song(&self) -> Option<&Song> {
        self.req.as_ref().map(|r| &r.song)
    }

    fn play(&mut self, ctx: &mut Ctx) -> Next {
        ctx.stop_sound(Sound::Decide);
        match self.req.take() {
            Some(req) => Next::Replace(Box::new(PlayScene::new(ctx, req))),
            None => Next::Pop,
        }
    }
}

/// The decide screen's values in one frame.
pub(super) struct DecideState<'a> {
    pub s: &'a DecideScene,
    pub ctx: &'a Ctx,
    song: Option<&'a Song>,
    best: Option<&'a BestScore>,
    chart: Option<ChartFacts<'a>>,
    table: OnceCell<Option<(String, String)>>,
}

impl<'a> DecideState<'a> {
    pub fn new(s: &'a DecideScene, ctx: &'a Ctx) -> DecideState<'a> {
        let song = s.song();
        DecideState {
            s,
            ctx,
            song,
            best: song.and_then(|s| ctx.best(s, ctx.config.play.ln_mode)),
            chart: song.map(ChartFacts::of_song),
            table: OnceCell::new(),
        }
    }

    fn static_option(&self, id: i32) -> Option<bool> {
        if let Some(v) = chart_option(self.chart.as_ref()?, id) {
            return Some(v);
        }
        let p = &self.ctx.config.play;
        let auto = self.s.req.as_ref().is_some_and(|r| r.autoplay);
        match id {
            32 | 33 | 40..=43 => play_option(auto, p.bga.shown(auto), p.gauge, id),
            100..=105 | 1100..=1104 => {
                lamp_option(self.best.map(|b| b.clear).unwrap_or_default(), id)
            }
            _ => None,
        }
    }
}

impl SkinState for DecideState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        self.s.host.timer(id)
    }

    fn option(&self, id: i32) -> bool {
        self.static_option(id.abs()).unwrap_or(false) != (id < 0)
    }

    fn number(&self, id: i32) -> Option<i64> {
        if let Some(v) = clock_number(id).or_else(|| player_number(self.ctx, id)) {
            return Some(v);
        }
        let song = self.song?;
        Some(match id {
            20 => self.ctx.view.fps as i64,
            10 | 310..=313 => return speed_number(&self.ctx.config.play, &Bpms::of_song(song), id),
            71 | 75..=78 | 102 | 103 | 150 | 173 | 176 | 183 | 184 | BEST_MIN_CB => {
                return best_number(self.best, id)
            }
            72 => song.notes * 2,
            74 => song.notes,
            90..=92 | 96 | 400 => return chart_number(self.chart.as_ref()?, id),
            1163 => song.length_ms / 60000,
            1164 => (song.length_ms / 1000) % 60,
            350..=368 => return song_number(song, id),
            _ => return None,
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let s = self.song?;
        match id {
            2 => Some(self.ctx.config.player_name.clone()),
            1001..=1003 => {
                let t = self
                    .table
                    .get_or_init(|| table_of(self.ctx, &s.md5, &s.sha256));
                table_text(t.as_ref()?, id)
            }
            _ => chart_text(&ChartText::of_song(s), id),
        }
    }

    fn rate(&self, _: i32) -> f32 {
        0.0
    }

    fn button(&self, id: i32) -> Option<i64> {
        config_button(self.ctx, id).or_else(|| self.number(id))
    }
}

struct DecideHooks {
    stage: Option<TexId>,
    banner: Option<TexId>,
}

impl Hooks for DecideHooks {
    fn ref_image(&self, id: i32) -> Option<TexId> {
        match id {
            100 | 101 => self.stage,
            102 => self.banner,
            _ => None,
        }
    }
}

impl Scene for DecideScene {
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        if self.host.first_update() {
            let skin = {
                let st = DecideState::new(self, ctx);
                let stat = |id: i32| st.static_option(id);
                skinned::load(ctx, gfx, SkinType::Decide, &stat, &st)
            };
            let Some(skin) = skin else {
                return self.play(ctx);
            };
            self.host.skin = Some(skin);
            let song = self.song().cloned().unwrap_or_default();
            let dir = std::path::Path::new(&song.path)
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .to_path_buf();
            let mut load = |name: &str| {
                (!name.is_empty())
                    .then(|| super::select::resolve_image(&dir.join(name)))
                    .flatten()
                    .and_then(|p| ImageTex::load(gfx, &p, &|_| {}))
            };
            self.stage_tex = load(&song.stagefile);
            self.banner_tex = load(&song.banner);
            self.host.created = now;
            ctx.play_sound(Sound::Decide);
        }
        let Some(sk) = self.host.skin.as_ref() else {
            return self.play(ctx);
        };
        let scene = match sk.skin.scene_ms {
            0 => SCENE_MS,
            n => n,
        };
        self.host.tick(gfx);
        for t in [&mut self.stage_tex, &mut self.banner_tex]
            .into_iter()
            .flatten()
        {
            t.tick(gfx);
        }
        if self.host.ms(now) >= scene {
            self.host.leave(now);
        }
        if self.host.faded(now) {
            return self.play(ctx);
        }
        Next::Stay
    }

    fn exit(&mut self, _ctx: &mut Ctx, gfx: &mut Gfx) {
        self.host.free(gfx);
        for t in [self.stage_tex.take(), self.banner_tex.take()]
            .into_iter()
            .flatten()
        {
            t.free(gfx);
        }
    }

    fn realtime(&self) -> bool {
        true
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        let nav = match ev {
            InputEv::Key {
                code,
                pressed: true,
                repeat: false,
                ..
            } => nav_from_key(*code, &ctx.config.keys),
            InputEv::Pad {
                id, pressed: true, ..
            } => nav_from_pad(*id, &ctx.config.keys),
            InputEv::Pointer {
                phase: Phase::Up, ..
            } => Some(Nav::Decide),
            _ => None,
        };
        let now = Instant::now();
        match nav {
            // back to the music select
            Some(Nav::Back | Nav::Close) => {
                ctx.stop_sound(Sound::Decide);
                Next::Pop
            }
            // on to play now
            Some(Nav::Decide) if self.host.ready(now) => {
                self.host.leave(now);
                Next::Stay
            }
            _ => Next::Stay,
        }
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let view = Rect::new(0.0, 0.0, ctx.view.width, ctx.view.height);
        let st = DecideState::new(self, ctx);
        let mut hooks = DecideHooks {
            stage: self.stage_tex.as_ref().map(|t| t.tex),
            banner: self.banner_tex.as_ref().map(|t| t.tex),
        };
        self.host.draw(d, view, now, &st, &mut hooks);
    }
}
