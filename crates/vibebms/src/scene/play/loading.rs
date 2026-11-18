//! Loading a play's chart, sounds and images off the main thread, and the
//! screen shown meanwhile.

use super::{PlayRequest, Versus};
use crate::app::{Ctx, Draw};
use crate::config::PlayConfig;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{with_alpha, Picture, Rect};
use crate::platform::vfs::Prefetch;
use crate::platform::{Instant, Job};
use crate::theme;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use vibebms_audio::SoundBank;
use vibebms_core::chart::Chart;
use vibebms_core::lane_option::DoubleOption;
use vibebms_core::nonstop::Nonstop;
use vibebms_core::play::{
    build_battle_chart, build_chart, build_nonstop_chart, PlayChart, RuleOptions,
};
use vibebms_core::practice::Section;
use vibebms_core::score::AssistFacts;

pub(super) struct Loaded {
    pub chart: Chart,
    pub bank: SoundBank,
    pub images: Vec<Option<Picture>>,
    /// Chart images that are videos (same indices).
    pub videos: Vec<Option<PathBuf>>,
    pub stagefile: Option<Picture>,
    /// `#BACKBMP` (the play skin's background image 101).
    pub backbmp: Option<Picture>,
    pub seed: i64,
    /// What the options did to the chart (for the assist level).
    pub facts: AssistFacts,
    /// The chart's long notes as written (before options).
    pub ln_profile: vibebms_core::LnProfile,
    /// A NONSTOP course's stages in the chart.
    pub nonstop: Option<Nonstop>,
    /// A practice play's section (the chart is the section's).
    pub section: Option<Section>,
}

type Work = Box<dyn FnOnce() -> Result<Box<Loaded>, String> + Send>;

pub(super) struct Loading {
    /// The work, started once the song's files are available.
    work: Option<Work>,
    prefetch: Prefetch,
    job: Option<Job<Result<Box<Loaded>, String>>>,
    /// Sound files read, of how many.
    progress: Arc<AtomicUsize>,
    total_files: Arc<AtomicUsize>,
    since: Instant,
}

impl Loading {
    pub fn start(ctx: &Ctx, req: &PlayRequest) -> Loading {
        let progress = Arc::new(AtomicUsize::new(0));
        let total_files = Arc::new(AtomicUsize::new(0));
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
        // a course: its rules (a replay's play options have them); a
        // practice play's
        let mut play = play;
        if let (Some(c), None) = (&req.course, &req.replay) {
            play.rules = c.rules(&play.rules);
        }
        let practice = req.practice.clone();
        if let Some(run) = &practice {
            play.rules = run.rules(&play.rules);
        }
        let rate = ctx.audio.mixer.sample_rate;
        let (p2, t2) = (progress.clone(), total_files.clone());
        let lanes = req.course.as_ref().and_then(|c| c.lanes());
        // (an expert or class course: every stage from the first's seed;
        // practice: every attempt from one)
        let replay_seed = replay_seed
            .or(req.course.as_ref().and_then(|c| c.seed))
            .or(practice.as_ref().map(|p| p.seed));
        let song_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let seed_now = move || {
            replay_seed.unwrap_or_else(|| {
                crate::platform::SystemTime::now()
                    .duration_since(crate::platform::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(1)
            })
        };
        if let Some(c) = req.course.as_ref().filter(|c| c.nonstop()) {
            let paths: Vec<PathBuf> = c.songs.iter().map(|s| PathBuf::from(&s.path)).collect();
            let dirs: Vec<PathBuf> = paths
                .iter()
                .map(|p| p.parent().map(Path::to_path_buf).unwrap_or_default())
                .collect();
            let joins = c.def.joins.clone();
            let work = move || -> Result<Box<Loaded>, String> {
                let seed = seed_now();
                load_nonstop(&paths, &joins, &play, seed, rate, &p2, &t2)
            };
            return Loading {
                work: Some(Box::new(work)),
                prefetch: Prefetch::dirs(&dirs),
                job: None,
                progress,
                total_files,
                since: Instant::now(),
            };
        }
        let work = move || -> Result<Box<Loaded>, String> {
            let seed = seed_now();
            let bytes = crate::platform::vfs::read(&path).map_err(|e| e.to_string())?;
            let PlayChart {
                chart,
                facts,
                ln_profile,
            } = match &ghost {
                Some(g) => build_battle_chart(&path, &bytes, &play, &g.play, seed)?,
                None => build_chart(&path, &bytes, &play, seed, lanes)?,
            };
            let (chart, section) = match &practice {
                Some(run) => {
                    let pc = vibebms_core::practice::prepare(chart, &run.practice, &play.rules);
                    (pc.chart, Some(pc.section))
                }
                None => (chart, None),
            };
            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            t2.store(chart.sound_files.len(), Ordering::Relaxed);
            let index = crate::platform::vfs::DirIndex::new(&dir);
            let resolve = |name: &str| index.find(name, crate::platform::vfs::AUDIO_EXTS);
            let read = |p: &Path| crate::platform::vfs::read(p).ok();
            let mut bank = SoundBank::load_with(&chart.sound_files, rate, &p2, &resolve, &read);
            // practice at another speed: the sounds at it, their pitch too
            let speed = practice.as_ref().map_or(1.0, |p| p.practice.rate());
            if speed != 1.0 {
                resample(&mut bank, speed);
            }
            let (images, videos) = if play.bga {
                load_images(&index, &chart.images)
            } else {
                (Vec::new(), Vec::new())
            };
            let stagefile = chart_image(&dir, &chart.meta.stagefile);
            let backbmp = chart_image(&dir, &chart.meta.backbmp);
            Ok(Box::new(Loaded {
                chart,
                bank,
                images,
                videos,
                stagefile,
                backbmp,
                seed,
                facts,
                ln_profile,
                nonstop: None,
                section,
            }))
        };
        Loading {
            work: Some(Box::new(work)),
            prefetch: Prefetch::dirs(&[song_dir]),
            job: None,
            progress,
            total_files,
            since: Instant::now(),
        }
    }

    /// The job started once the files are there; what it loaded once done.
    pub fn poll(&mut self) -> Option<Result<Box<Loaded>, String>> {
        if self.work.is_some() && self.prefetch.finished() {
            let work = self.work.take();
            self.job = work.map(|work| Job::spawn("load", |_| work()));
        }
        self.job.as_mut()?.poll()
    }

    pub fn draw(&self, ctx: &Ctx, d: &mut Draw, req: &PlayRequest) {
        let ui = ctx.ui();
        let (w, h) = (ctx.view.width, ctx.view.height);
        let s = &req.song;
        let line = |d: &mut Draw, text: &str, y: f32, size: f32, color| {
            d.text(
                text,
                w * 0.5,
                y,
                TextStyle::new(size * ui, color, Align::Center).fit(w * 0.9),
            );
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
            let stage = match c.nonstop() {
                true => format!("NONSTOP {} STAGES", c.songs.len()),
                false => format!("STAGE {}/{}", c.index + 1, c.songs.len()),
            };
            d.text(
                &format!("{}  {stage}", c.name),
                w * 0.5,
                h * 0.22,
                TextStyle::new(22.0 * ui, theme::ACCENT, Align::Center),
            );
        }
        if let Some(p) = &req.practice {
            line(d, &p.label(), h * 0.22, 22.0, theme::ACCENT);
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
            TextStyle::new(16.0 * ui, theme::TEXT_DIM, Align::Center),
        );
    }
}

type Media = (Vec<Option<Picture>>, Vec<Option<PathBuf>>);

/// A chart's `#STAGEFILE`, `#BACKBMP` (`name`, in `dir`).
fn chart_image(dir: &Path, name: &str) -> Option<Picture> {
    crate::platform::vfs::inside(dir, name)
        .filter(|_| !name.is_empty())
        .and_then(|p| crate::scene::select::resolve_image(&p))
        .and_then(|p| crate::gfx::load_picture(&p, &|_| {}))
}

/// A chart's images (`files`), each an image (animated GIF, WebP and APNG
/// with their frames) or a video (shown when ffmpeg is installed).
fn load_images(index: &crate::platform::vfs::DirIndex, files: &[String]) -> Media {
    use crate::platform::vfs::IMAGE_EXTS;
    use crate::video::{is_video, VIDEO_EXTS};
    files
        .iter()
        .map(|f| match index.find(f, IMAGE_EXTS) {
            Some(p) if is_video(&p) => (None, Some(p)),
            Some(p) => (crate::gfx::load_picture(&p, &crate::gfx::key_black), None),
            None => (None, index.find(f, VIDEO_EXTS)),
        })
        .unzip()
}

/// A NONSTOP course: its stages (`paths`) joined into one chart, each
/// stage's sounds and images from its own folder, a tempo-matched stage's
/// sounds played at its speed (and pitch).
fn load_nonstop(
    paths: &[PathBuf],
    joins: &[vibebms_core::course::Join],
    play: &PlayConfig,
    seed: i64,
    rate: u32,
    progress: &AtomicUsize,
    total: &AtomicUsize,
) -> Result<Box<Loaded>, String> {
    let bytes = paths
        .iter()
        .map(|p| crate::platform::vfs::read(p).map_err(|e| format!("{}: {e}", p.display())))
        .collect::<Result<Vec<_>, _>>()?;
    let stages: Vec<(&Path, &[u8])> = paths
        .iter()
        .zip(&bytes)
        .map(|(p, b)| (p.as_path(), b.as_slice()))
        .collect();
    let (pc, ns) = build_nonstop_chart(&stages, joins, &play.rules, seed)?;
    let chart = pc.chart;
    total.store(chart.sound_files.len(), Ordering::Relaxed);
    let mut bank = SoundBank::default();
    let (mut images, mut videos) = (Vec::new(), Vec::new());
    for (span, path) in ns.stages.iter().zip(paths) {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let index = crate::platform::vfs::DirIndex::new(&dir);
        let resolve = |name: &str| index.find(name, crate::platform::vfs::AUDIO_EXTS);
        let read = |p: &Path| crate::platform::vfs::read(p).ok();
        let files = &chart.sound_files[span.files.start as usize..span.files.end as usize];
        let mut own = SoundBank::load_with(files, rate, progress, &resolve, &read);
        if span.ratio != 1.0 {
            resample(&mut own, span.ratio);
        }
        bank.samples.extend(own.samples);
        if play.bga {
            let files = &chart.images[span.images.start as usize..span.images.end as usize];
            let (i, v) = load_images(&index, files);
            images.extend(i);
            videos.extend(v);
        }
    }
    let first = paths
        .first()
        .and_then(|p| p.parent())
        .unwrap_or(Path::new(""));
    let stagefile = chart_image(first, &chart.meta.stagefile);
    let backbmp = chart_image(first, &chart.meta.backbmp);
    Ok(Box::new(Loaded {
        chart,
        bank,
        images,
        videos,
        stagefile,
        backbmp,
        seed,
        facts: pc.facts,
        ln_profile: pc.ln_profile,
        nonstop: Some(ns),
        section: None,
    }))
}

/// Every sound of `bank` played `speed` times as fast (and as high).
fn resample(bank: &mut SoundBank, speed: f64) {
    for s in bank.samples.iter_mut().flatten() {
        *s = Arc::new(s.resampled(speed));
    }
}
