//! Loading a play's chart, sounds and images off the main thread, and the
//! screen shown meanwhile.

use super::{PlayRequest, Versus};
use crate::app::{Ctx, Draw};
use crate::config::PlayConfig;
use crate::gfx::text::Align;
use crate::gfx::{with_alpha, Rect};
use crate::platform::vfs::Prefetch;
use crate::platform::Instant;
use crate::theme;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use vibebms_audio::SoundBank;
use vibebms_core::chart::Chart;
use vibebms_core::lane_option::DoubleOption;
use vibebms_core::play::{build_battle_chart, build_chart, PlayChart, RuleOptions};
use vibebms_core::score::AssistFacts;

pub(super) type Rgba = (u32, u32, Vec<u8>);

pub(super) struct Loaded {
    pub chart: Chart,
    pub bank: SoundBank,
    pub images: Vec<Option<Rgba>>,
    /// Chart images that are videos (same indices).
    pub videos: Vec<Option<PathBuf>>,
    pub stagefile: Option<Rgba>,
    pub seed: i64,
    /// What the options did to the chart (for the assist level).
    pub facts: AssistFacts,
    /// The chart's long notes as written (before options).
    pub ln_profile: vibebms_core::LnProfile,
}

pub(super) struct Loading {
    /// The job, started once the song's files are available.
    job: Option<Box<dyn FnOnce() + Send>>,
    prefetch: Prefetch,
    rx: Receiver<Result<Box<Loaded>, String>>,
    /// Sound files read, of how many.
    progress: Arc<AtomicUsize>,
    total_files: Arc<AtomicUsize>,
    since: Instant,
}

impl Loading {
    pub fn start(ctx: &Ctx, req: &PlayRequest) -> Loading {
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
        let play = if req.is_vs() {
            PlayConfig {
                rules: RuleOptions {
                    battle: true,
                    dp_option: DoubleOption::Off,
                    ..play.rules
                },
                ..play
            }
        } else {
            play
        };
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
                let PlayChart {
                    chart,
                    facts,
                    ln_profile,
                } = match &ghost {
                    Some(g) => build_battle_chart(&path, &bytes, &play, &g.play, seed)?,
                    None => build_chart(&path, &bytes, &play, seed, lanes)?,
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
                    .and_then(|p| crate::scene::select::resolve_image(&p))
                    .and_then(|p| crate::gfx::load_rgba(&p, false));
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
        Loading {
            job: Some(Box::new(job)),
            prefetch: Prefetch::dirs(&[song_dir]),
            rx,
            progress,
            total_files,
            since: Instant::now(),
        }
    }

    /// The job started once the files are there; what it loaded once done.
    pub fn poll(&mut self) -> Option<Result<Box<Loaded>, String>> {
        if self.job.is_some() && self.prefetch.finished() {
            if let Some(job) = self.job.take() {
                crate::platform::spawn("load", job);
            }
        }
        self.rx.try_recv().ok()
    }

    pub fn draw(&self, ctx: &Ctx, d: &mut Draw, req: &PlayRequest) {
        let ui = ctx.ui();
        let (w, h) = (ctx.view.width, ctx.view.height);
        let s = &req.song;
        let line = |d: &mut Draw, text: &str, y: f32, size: f32, color| {
            d.text_fit(text, w * 0.5, y, size * ui, color, Align::Center, w * 0.9);
        };
        line(d, &s.genre, h * 0.3, 18.0, theme::TEXT_DIM);
        line(d, &s.full_title(), h * 0.36, 34.0, theme::TEXT);
        line(d, &s.artist, h * 0.36 + 48.0 * ui, 20.0, theme::TEXT_DIM);
        if req.is_vs() {
            // which controller is 1P (a button on it now picks it)
            let pads = if ctx.config.keys.p1.keys.iter().any(|k| !k.pad.is_empty()) {
                "  ·  press a button on the 1P controller"
            } else {
                ""
            };
            let text = format!("VS  ·  1P left, 2P right{pads}");
            line(d, &text, h * 0.22, 20.0, theme::ACCENT);
        }
        if let Some(c) = &req.course {
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
        let k = (done as f32 / total as f32).min(1.0);
        d.b.rect(Rect::new(bar.x, bar.y, bar.w * k, bar.h), theme::ACCENT);
        let dots = ".".repeat((self.since.elapsed().as_millis() / 300 % 4) as usize);
        d.text(
            &format!("LOADING{dots}  {done}/{total}"),
            w * 0.5,
            bar.y + 16.0 * ui,
            16.0 * ui,
            theme::TEXT_DIM,
            Align::Center,
        );
    }
}
