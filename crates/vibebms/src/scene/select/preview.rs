//! What the select screen fetches for the chosen bar once the cursor
//! rests on it: the song's preview, its banner and its IR ranking; and the
//! sound set's BGM and panel sounds.

use super::genpreview::Preview;
use super::SelectScene;
use crate::audio::sysound::Sound;
use crate::ctx::Ctx;
use crate::gfx::Gfx;
use crate::gfx::ImageJobs;
use crate::ir::NotAsked;
use crate::library::Song;
use crate::platform::vfs::{self, Prefetch};
use crate::platform::{unix_ms, Duration, Instant, Job};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vibebms_audio::{Bus, PlayOptions};
use vibebms_ir::lamps::RankView;

/// How long the cursor rests before the preview plays, the ranking is
/// asked for and the banner is decoded.
const PREVIEW_AFTER: Duration = Duration::from_millis(450);
/// A preview is made of the chart's sounds only once the cursor rests
/// longer (on the web the work holds up the page while it runs).
const GENERATE_AFTER: Duration = Duration::from_millis(if cfg!(target_arch = "wasm32") {
    1200
} else {
    600
});
const RANKING_AFTER: Duration = Duration::from_millis(700);
const BANNER_AFTER: Duration = Duration::from_millis(120);

/// A chart's IR ranking.
pub(crate) enum Rank {
    Loading(Job<Result<RankView, String>>),
    Done(RankView),
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

impl SelectScene {
    /// The sound set's BGM while no song preview plays, and the option
    /// panels' open / close sounds.
    pub(super) fn update_sounds(&mut self, ctx: &mut Ctx) {
        let ended = self.preview_until.is_some_and(|t| Instant::now() >= t);
        let want = self.preview_for.is_none() || ended || self.preview_wait;
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
        ctx.audio.mixer.stop(crate::audio::PREVIEW_KEY);
        self.preview_for = None;
        self.preview_until = None;
        self.preview_wait = false;
        self.stop_bgm(ctx);
    }

    /// Play preview `s` (once, or looping; at the common loudness when
    /// normalising), or with none the select BGM.
    fn play_preview(&mut self, ctx: &mut Ctx, s: Option<Preview>) {
        self.preview_wait = false;
        let once = ctx.config.audio.preview_once;
        let rate = ctx.audio.mixer.sample_rate.max(1) as u64;
        self.preview_until = match &s {
            None => Some(Instant::now()),
            Some((s, _)) if once => {
                Some(Instant::now() + Duration::from_millis(s.frames as u64 * 1000 / rate))
            }
            Some(_) => None,
        };
        if let Some((s, l)) = s {
            let gain = crate::audio::loudness::chart_target(&ctx.config.audio)
                .map_or(1.0, |t| crate::audio::loudness::gain(&l, t));
            let opts = PlayOptions {
                key: crate::audio::PREVIEW_KEY,
                bus: Bus::Bgm,
                volume: 0.8 * gain,
                looping: !once,
                ..Default::default()
            };
            ctx.audio.mixer.play(&s, opts);
        }
    }

    pub(super) fn update_preview(&mut self, ctx: &mut Ctx) {
        // a preview made of a chart's sounds: played when its chart still
        // waits for it
        let rate = ctx.audio.mixer.sample_rate;
        if let Some((path, s)) = self.generated.poll(rate) {
            if self.preview_wait && self.preview_for.as_deref() == Some(path.as_str()) {
                self.play_preview(ctx, s);
            }
        }
        if let Some(s) = self.preview_job.as_mut().and_then(Job::poll) {
            self.preview_job = None;
            self.play_preview(ctx, s);
        }
        // its file read: decoded (off the main thread natively)
        if let Some((_, p)) = self.preview_fetch.take_if(|(f, _)| f.finished()) {
            let rate = ctx.audio.mixer.sample_rate;
            self.preview_job = Some(Job::spawn("preview", move |_| {
                let ext = p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let s = vfs::read(&p)
                    .ok()
                    .and_then(|bytes| vibebms_audio::decode_bytes(bytes, &ext, rate).ok())?;
                let l = crate::audio::loudness::measure_sound(&s, rate);
                Some((Arc::new(s), l))
            }));
            return;
        }
        // (none while the app is silenced: not in use)
        if !ctx.config.audio.preview
            || self.last_move.elapsed() < PREVIEW_AFTER
            || ctx.audio.mute.on()
        {
            return;
        }
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        if self.preview_for.is_some() {
            // a preview to be made once the one before is done
            if self.preview_wait
                && !self.generated.busy()
                && self.last_move.elapsed() >= GENERATE_AFTER
            {
                self.generated.start(&song.path);
            }
            return;
        }
        self.preview_for = Some(song.path.clone());
        if let Some(p) = preview_file(song) {
            self.preview_fetch = Some((Prefetch::files(vec![p.clone()]), p));
        } else if let Some(s) = self.generated.cached(&song.path) {
            self.play_preview(ctx, Some(s));
        } else {
            self.preview_wait = true;
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
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        if self.ranking.contains_key(&song.sha256) || ctx.config.ir.primary().is_none() {
            return;
        }
        let chart = vibebms_ir::provider::ChartId {
            md5: song.md5.clone(),
            sha256: song.sha256.clone(),
            mode: song.mode,
            ln: song.score_ln(ctx.config.play.ln_pref()),
            remote: None,
        };
        let sha = chart.sha256.clone();
        let rank = match ctx.net.ir.ranking(&ctx.config.ir, chart, unix_ms()) {
            Ok(job) => Rank::Loading(job),
            // (asked again next frame)
            Err(NotAsked::Later) => return,
            // this IR cannot be asked about the chart
            Err(NotAsked::Never) => Rank::Error,
        };
        self.ranking.insert(sha, rank);
    }

    /// The banner's texture freed (and its decode not waited for).
    pub(super) fn free_banner(&mut self, gfx: &mut Gfx) {
        self.banner_job = None;
        if let Some((_, Some(t))) = self.banner.take() {
            t.free(gfx);
        }
    }

    pub(super) fn update_banner(&mut self, ctx: &Ctx, gfx: &mut Gfx) {
        // (the song's folder, the image's name in it)
        let want = self.selected_song(ctx).map(|(_, s)| {
            let img = if !s.stagefile.is_empty() {
                &s.stagefile
            } else {
                &s.banner
            };
            let dir = Path::new(&s.path).parent().unwrap_or(Path::new(""));
            (dir, img.as_str())
        });
        if let Some(job) = self.banner_job.as_mut() {
            let done = job.poll(false, |pic| crate::gfx::ImageTex::new(gfx, pic));
            if let (Some((_, t)), Some(b)) = (done.into_iter().next(), self.banner.as_mut()) {
                b.1 = t;
            }
            if job.done() {
                self.banner_job = None;
            }
        }
        if let Some((_, Some(t))) = self.banner.as_mut() {
            t.tick(gfx);
        }
        let key = want.map(|(dir, img)| dir.join(img).to_string_lossy().into_owned());
        if key.as_deref() == self.banner.as_ref().map(|b| b.0.as_str()) {
            return;
        }
        if key.is_some() && self.last_move.elapsed() < BANNER_AFTER {
            return; // don't decode while scrolling fast
        }
        self.free_banner(gfx);
        self.banner_job = want
            .and_then(|(dir, img)| crate::library::chart_image(dir, img))
            .map(|rp| ImageJobs::start(vec![rp], None));
        self.banner = key.map(|p| (p, None));
    }
}
