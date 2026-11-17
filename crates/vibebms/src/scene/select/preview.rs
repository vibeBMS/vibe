//! What the select screen fetches for the chosen bar once the cursor
//! rests on it: the song's preview, its banner and its IR ranking; and the
//! sound set's BGM and panel sounds.

use super::SelectScene;
use crate::app::Ctx;
use crate::gfx::Gfx;
use crate::library::Song;
use crate::platform::vfs::{self, Prefetch};
use crate::platform::{Duration, Job};
use crate::skin::render::ImageJobs;
use crate::sysound::Sound;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vibebms_audio::{Bus, PlayOptions};
use vibebms_ir::IrScore;

pub(super) const PREVIEW_KEY: u64 = u64::MAX - 1;

/// How long the cursor rests before the preview plays, the ranking is
/// asked for and the banner is decoded.
const PREVIEW_AFTER: Duration = Duration::from_millis(450);
const RANKING_AFTER: Duration = Duration::from_millis(700);
const BANNER_AFTER: Duration = Duration::from_millis(120);

/// A chart's IR ranking.
pub(crate) enum Rank {
    Loading(Job<Result<Vec<IrScore>, String>>),
    Done(Vec<IrScore>),
    Error,
}

/// A song's preview: the chart's `#PREVIEW`, else a sound in its folder
/// named `preview…` (as beatoraja finds one).
fn preview_file(song: &Song) -> Option<PathBuf> {
    let dir = Path::new(&song.path).parent()?;
    if !song.preview.is_empty() {
        if let Some(p) = vfs::find_by_stem(dir, &song.preview, vfs::AUDIO_EXTS) {
            return Some(p);
        }
    }
    let mut found: Vec<PathBuf> = vfs::read_dir(dir)
        .ok()?
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.path)
        .filter(|p| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase());
            let ext = p.extension().map(|s| s.to_string_lossy().to_lowercase());
            stem.is_some_and(|s| s.starts_with("preview"))
                && ext.is_some_and(|e| vfs::AUDIO_EXTS.contains(&e.as_str()))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Resolve an image path, trying common extension substitutions.
pub fn resolve_image(p: &Path) -> Option<PathBuf> {
    let dir = p.parent()?;
    let name = p.file_name()?.to_string_lossy();
    vfs::find_by_stem(dir, &name, vfs::IMAGE_EXTS)
}

impl SelectScene {
    /// The sound set's BGM while no song preview plays, and the option
    /// panels' open / close sounds.
    pub(super) fn update_sounds(&mut self, ctx: &mut Ctx) {
        let want = self.preview_for.is_none();
        if want != self.bgm_on {
            self.bgm_on = want;
            if want {
                ctx.play_sound(Sound::Select);
            } else {
                ctx.stop_sound(Sound::Select);
            }
        }
        if self.panel != self.sound_panel {
            ctx.play_sound(if self.panel > 0 {
                Sound::OptionOpen
            } else {
                Sound::OptionClose
            });
            self.sound_panel = self.panel;
        }
    }

    pub(super) fn stop_bgm(&mut self, ctx: &mut Ctx) {
        ctx.stop_sound(Sound::Select);
        self.bgm_on = true; // until resumed
    }

    /// The preview and the BGM stop (another screen comes).
    pub(super) fn stop_sounds(&mut self, ctx: &mut Ctx) {
        ctx.audio.mixer.stop(PREVIEW_KEY);
        self.preview_for = None;
        self.stop_bgm(ctx);
    }

    pub(super) fn update_preview(&mut self, ctx: &mut Ctx) {
        if let Some(s) = self.preview_job.as_mut().and_then(Job::poll) {
            self.preview_job = None;
            if let Some(s) = s {
                let opts = PlayOptions {
                    key: PREVIEW_KEY,
                    bus: Bus::Bgm,
                    volume: 0.8,
                    looping: true,
                    ..Default::default()
                };
                ctx.audio.mixer.play(&s, opts);
            }
        }
        // its file read: decoded (off the main thread natively)
        if let Some((_, p)) = self.preview_fetch.take_if(|(f, _)| f.finished()) {
            let rate = ctx.audio.mixer.sample_rate;
            self.preview_job = Some(Job::spawn("preview", move |_| {
                let ext = p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                vfs::read(&p)
                    .ok()
                    .and_then(|bytes| vibebms_audio::decode_bytes(bytes, &ext, rate).ok())
                    .map(Arc::new)
            }));
            return;
        }
        if !ctx.config.audio.preview
            || self.preview_for.is_some()
            || self.last_move.elapsed() < PREVIEW_AFTER
        {
            return;
        }
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        self.preview_for = Some(song.path.clone());
        if let Some(p) = preview_file(song) {
            self.preview_fetch = Some((Prefetch::files(vec![p.clone()]), p));
        }
    }

    pub(super) fn update_ranking(&mut self, ctx: &mut Ctx) {
        for r in self.ranking.values_mut() {
            if let Rank::Loading(job) = r {
                if let Some(res) = job.poll() {
                    *r = match res {
                        Ok(v) => Rank::Done(v),
                        Err(e) => {
                            log::warn!("IR ranking: {e}");
                            Rank::Error
                        }
                    };
                }
            }
        }
        if self.last_move.elapsed() < RANKING_AFTER {
            return;
        }
        let Some(client) = vibebms_ir::JsonIr::new(&ctx.config.ir_server) else {
            return;
        };
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        if self.ranking.contains_key(&song.sha256) {
            return;
        }
        let chart = vibebms_ir::ChartRef {
            md5: song.md5.clone(),
            sha256: song.sha256.clone(),
            ln_type: format!("{:?}", ctx.config.play.ln_mode).to_uppercase(),
        };
        let job = Job::start(|tx, _| {
            crate::platform::ir_send(client.ranking_request(&chart), move |body| {
                let _ = tx.send(body.and_then(|b| vibebms_ir::JsonIr::parse_ranking(&b)));
            })
        });
        self.ranking.insert(song.sha256.clone(), Rank::Loading(job));
    }

    pub(super) fn update_banner(&mut self, ctx: &Ctx, gfx: &mut Gfx) {
        let want = self.selected_song(ctx).and_then(|(_, s)| {
            let img = if !s.stagefile.is_empty() {
                &s.stagefile
            } else {
                &s.banner
            };
            let dir = Path::new(&s.path).parent().unwrap_or(Path::new(""));
            vfs::inside(dir, img).map(|p| p.to_string_lossy().into_owned())
        });
        if let Some(job) = self.banner_job.as_mut() {
            let done = job.poll(false, |(w, h, px)| gfx.create_texture(w, h, &px));
            if let (Some((_, t)), Some(b)) = (done.into_iter().next(), self.banner.as_mut()) {
                b.1 = t;
            }
            if job.done() {
                self.banner_job = None;
            }
        }
        if want.as_deref() == self.banner.as_ref().map(|b| b.0.as_str()) {
            return;
        }
        if want.is_some() && self.last_move.elapsed() < BANNER_AFTER {
            return; // don't decode while scrolling fast
        }
        if let Some((_, Some(t))) = self.banner.take() {
            gfx.free_texture(t);
        }
        self.banner_job = want
            .as_ref()
            .and_then(|p| resolve_image(Path::new(p)))
            .map(|rp| ImageJobs::start(vec![rp], None));
        self.banner = want.map(|p| (p, None));
    }
}
