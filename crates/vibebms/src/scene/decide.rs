//! The decide screen: a skin shown between choosing a chart and playing it
//! (LR2 / beatoraja skin type 6), when one is configured.

use super::play::{PlayRequest, PlayScene};
use super::skinned::{self, ms_since};
use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::Bpms;
use crate::gfx::{Gfx, Rect, TexId};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::library::Song;
use crate::platform::Instant;
use crate::skin::render::{Hooks, LoadedSkin};
use crate::skin::{SkinState, SkinType};
use vibebms_core::PlayMode;

/// How long the screen shows when the skin does not say (beatoraja's
/// default decide skin).
const SCENE_MS: i64 = 3000;

/// The screen to go to once a chart is chosen: the decide screen when a
/// decide skin is configured, else straight to play.
pub fn start(ctx: &mut Ctx, req: PlayRequest) -> Box<dyn Scene> {
    if skinned::configured_skin(ctx, SkinType::Decide).is_some() {
        Box::new(DecideScene {
            req: Some(req),
            skin: None,
            tried: false,
            created: Instant::now(),
            leaving: None,
            stage_tex: None,
            banner_tex: None,
        })
    } else {
        Box::new(PlayScene::new(ctx, req))
    }
}

pub struct DecideScene {
    req: Option<PlayRequest>,
    skin: Option<LoadedSkin>,
    tried: bool,
    created: Instant,
    /// The fade out started (timer 2): play follows when it ends.
    leaving: Option<Instant>,
    stage_tex: Option<TexId>,
    banner_tex: Option<TexId>,
}

impl DecideScene {
    fn song(&self) -> Option<&Song> {
        self.req.as_ref().map(|r| &r.song)
    }

    fn input_ms(&self) -> i64 {
        self.skin.as_ref().map(|s| s.skin.input_ms).unwrap_or(0)
    }

    fn play(&mut self, ctx: &mut Ctx) -> Next {
        match self.req.take() {
            Some(req) => Next::Replace(Box::new(PlayScene::new(ctx, req))),
            None => Next::Pop,
        }
    }
}

struct DecideState<'a> {
    s: &'a DecideScene,
    ctx: &'a Ctx,
}

impl DecideState<'_> {
    fn mode(&self) -> Option<PlayMode> {
        let m = &self.s.song()?.mode;
        PlayMode::ALL.iter().copied().find(|p| p.label() == m)
    }

    fn best(&self) -> Option<&crate::library::BestScore> {
        let s = self.s.song()?;
        self.ctx.best(
            s,
            if s.has_ln {
                self.ctx.config.play.ln_mode as i32
            } else {
                0
            },
        )
    }

    fn static_option(&self, id: i32) -> Option<bool> {
        let song = self.s.song()?;
        let p = &self.ctx.config.play;
        let auto = self.s.req.as_ref().is_some_and(|r| r.autoplay);
        Some(match id {
            32 => !auto,
            33 => auto,
            40 => !p.bga,
            41 => p.bga,
            42 => !p.gauge.is_survival(),
            43 => p.gauge.is_survival(),
            100..=105 | 1100..=1104 => {
                let c = self.best().map(|b| b.clear).unwrap_or_default();
                return skinned::lamp_option(c, id);
            }
            150..=155 => song.difficulty == id - 150,
            160..=164 => {
                let m = [
                    PlayMode::Beat7K,
                    PlayMode::Beat5K,
                    PlayMode::Beat14K,
                    PlayMode::Beat10K,
                    PlayMode::Popn9K,
                ][(id - 160) as usize];
                self.mode() == Some(m)
            }
            170 => !song.has_bga,
            171 => song.has_bga,
            172 => !song.has_ln,
            173 => song.has_ln,
            180..=184 => song.judge.is_some_and(|j| skinned::judge_option(j, id)),
            176 => song.min_bpm == song.max_bpm,
            177 => song.min_bpm != song.max_bpm,
            1177 => song.has_stop,
            178 => !song.has_random,
            179 => song.has_random,
            190 => song.stagefile.is_empty(),
            191 => !song.stagefile.is_empty(),
            192 => song.banner.is_empty(),
            193 => !song.banner.is_empty(),
            _ => return None,
        })
    }
}

impl SkinState for DecideState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        match id {
            0 => Some(0),
            1 => Some(self.s.input_ms()),
            2 => self.s.leaving.map(|t| ms_since(self.s.created, t)),
            _ => None,
        }
    }

    fn option(&self, id: i32) -> bool {
        self.static_option(id.abs()).unwrap_or(false) != (id < 0)
    }

    fn number(&self, id: i32) -> Option<i64> {
        if let Some(v) = skinned::clock_number(id) {
            return Some(v);
        }
        if let Some(v) = skinned::player_number(self.ctx, id) {
            return Some(v);
        }
        let song = self.s.song()?;
        let p = &self.ctx.config.play;
        let best = self.best();
        let b = |f: fn(&crate::library::BestScore) -> i64| best.map(f).unwrap_or(0);
        let bpms = Bpms::of_song(song);
        let rate = best.map(|b| b.rate()).unwrap_or(0.0) * 10000.0;
        Some(match id {
            20 => self.ctx.fps as i64,
            10 | 310 | 311 => {
                let hs = (p.hispeed_at(&bpms) * 100.0).round() as i64;
                match id {
                    310 => hs / 100,
                    311 => hs % 100,
                    _ => hs,
                }
            }
            312 => p.duration_ms(&bpms).round() as i64,
            313 => p.green(&bpms),
            71 | 150 => b(|b| b.ex_score as i64),
            72 => song.notes * 2,
            74 => song.notes,
            75 | 173 => b(|b| b.max_combo as i64),
            76 | 176 => b(|b| {
                if b.min_bp == u32::MAX {
                    0
                } else {
                    b.min_bp as i64
                }
            }),
            77 => b(|b| b.play_count as i64),
            78 => b(|b| b.clear_count as i64),
            90 => song.max_bpm.round() as i64,
            91 => song.min_bpm.round() as i64,
            92 => song.main_bpm.round() as i64,
            96 => song.level_num(),
            102 | 183 => rate as i64 / 100,
            103 | 184 => rate as i64 % 100,
            1163 => song.length_ms / 60000,
            1164 => (song.length_ms / 1000) % 60,
            350..=368 | 400 => return skinned::song_number(song, id),
            _ => return None,
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let s = self.s.song()?;
        if let Some(t) = skinned::table_text(self.ctx, &s.md5, &s.sha256, id) {
            return Some(t);
        }
        Some(match id {
            2 => self.ctx.config.player_name.clone(),
            10 => s.title.clone(),
            11 => s.subtitle.clone(),
            12 => s.full_title(),
            13 => s.genre.clone(),
            14 => s.artist.clone(),
            15 => s.subartist.clone(),
            16 => format!("{} {}", s.artist, s.subartist).trim().to_string(),
            1030 => s.md5.clone(),
            1031 => s.sha256.clone(),
            _ => return None,
        })
    }

    fn rate(&self, _: i32) -> f32 {
        0.0
    }

    fn button(&self, id: i32) -> Option<i64> {
        skinned::config_button(self.ctx, id).or_else(|| self.number(id))
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
        if !self.tried {
            self.tried = true;
            let skin = {
                let st = DecideState { s: self, ctx };
                let stat = |id: i32| st.static_option(id);
                skinned::load(ctx, gfx, SkinType::Decide, &stat, &st)
            };
            let Some(skin) = skin else {
                return self.play(ctx);
            };
            self.skin = Some(skin);
            let song = self.song().cloned().unwrap_or_default();
            let dir = std::path::Path::new(&song.path)
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .to_path_buf();
            let mut load = |name: &str| {
                (!name.is_empty())
                    .then(|| super::select::resolve_image(&dir.join(name)))
                    .flatten()
                    .and_then(|p| gfx.load_image(&p, false))
            };
            self.stage_tex = load(&song.stagefile);
            self.banner_tex = load(&song.banner);
            self.created = now;
        }
        let Some(sk) = self.skin.as_mut() else {
            return self.play(ctx);
        };
        sk.load_pending(gfx);
        let scene = match sk.skin.scene_ms {
            0 => SCENE_MS,
            n => n,
        };
        let fade = sk.skin.fadeout_ms;
        if self.leaving.is_none() && ms_since(self.created, now) >= scene {
            self.leaving = Some(now);
        }
        match self.leaving {
            Some(t) if ms_since(t, now) >= fade => self.play(ctx),
            _ => Next::Stay,
        }
    }

    fn exit(&mut self, _ctx: &mut Ctx, gfx: &mut Gfx) {
        if let Some(mut s) = self.skin.take() {
            s.free(gfx);
        }
        for t in [self.stage_tex.take(), self.banner_tex.take()]
            .into_iter()
            .flatten()
        {
            gfx.free_texture(t);
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
        let ready = ms_since(self.created, Instant::now()) >= self.input_ms();
        match nav {
            // back to the music select
            Some(Nav::Back | Nav::Close) => Next::Pop,
            // on to play now
            Some(Nav::Decide) if ready && self.leaving.is_none() => {
                self.leaving = Some(Instant::now());
                Next::Stay
            }
            _ => Next::Stay,
        }
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let view = Rect::new(0.0, 0.0, ctx.width, ctx.height);
        let st = DecideState { s: self, ctx };
        let mut hooks = DecideHooks {
            stage: self.stage_tex,
            banner: self.banner_tex,
        };
        if let Some(sk) = self.skin.as_ref() {
            sk.draw(d, view, ms_since(self.created, now), &st, &mut hooks);
        }
    }
}
