//! Loading a play's chart, sounds and images off the main thread, and the
//! screen shown meanwhile.

use super::bga::Files;
use super::{PlayRequest, Versus};
use crate::audio::loudness::Loudness;
use crate::config::{BgaMode, PlayConfig};
use crate::ctx::Ctx;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::theme;
use crate::gfx::{with_alpha, Draw, Picture, Rect};
use crate::platform::vfs::{DirIndex, Prefetch};
use crate::platform::{Instant, Job};
use std::collections::HashMap;
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
    /// The chart's images that are pictures, and those that are videos.
    pub images: Files<Picture>,
    pub videos: Files<PathBuf>,
    pub stagefile: Option<Picture>,
    /// `#BACKBMP` (the play skin's background image 101).
    pub backbmp: Option<Picture>,
    /// The chart's loudness, and whether it was measured now (to be kept).
    pub loudness: Option<(Loudness, bool)>,
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
    /// `watch_rate`: a replay watched at another speed (its sounds are
    /// resampled to it; the chart and its judging stay as they were).
    pub fn start(ctx: &Ctx, req: &PlayRequest, watch_rate: f64) -> Loading {
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
        // the BGA as the player has it now (AUTO: when watching)
        let watching = req.autoplay || req.replay.is_some();
        play.bga = match ctx.config.play.bga.shown(watching) {
            true => BgaMode::On,
            false => BgaMode::Off,
        };
        if let (Some(c), None) = (&req.course, &req.replay) {
            play.rules = c.rules(&play.rules);
        }
        let practice = req.practice.clone();
        if let Some(run) = &practice {
            play.rules = run.rules(&play.rules);
        }
        let rate = ctx.audio.mixer.sample_rate;
        // the chart's loudness, measured once its sounds are decoded unless
        // known (or not wanted)
        let normalize = crate::audio::loudness::chart_target(&ctx.config.audio).is_some();
        let known = ctx.audio.loudness.get(&req.song.sha256);
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
                // songs in archives unpacked
                crate::platform::vfs::prepare_songs(&paths).map_err(|e| e.to_string())?;
                let mut l = load_nonstop(&paths, &joins, &play, seed, rate, &p2, &t2)?;
                if watch_rate != 1.0 {
                    l.bank.resample(0..l.bank.samples.len(), watch_rate);
                }
                // (measured each time: the stages joined are not one chart)
                if normalize {
                    let m = crate::audio::loudness::measure_chart(&l.chart, &l.bank, rate);
                    l.loudness = Some((m, false));
                }
                Ok(l)
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
            // a song in an archive unpacked (kept while it is chosen)
            crate::platform::vfs::prepare_songs(std::slice::from_ref(&path))
                .map_err(|e| e.to_string())?;
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
            let index = DirIndex::new(&dir);
            let resolve = |name: &str| index.find(name, crate::platform::vfs::AUDIO_EXTS);
            let read = |p: &Path| crate::platform::vfs::read_shared(p).ok();
            let mut bank = SoundBank::load_with(&chart.sound_files, rate, &p2, &resolve, &read);
            // practice at another speed, FREQ (G-BATTLE: the opponent's):
            // the sounds at it, their pitch too
            let freq = ghost.as_ref().map_or(play.freq, |g| g.play.freq);
            let speed = practice.as_ref().map_or(1.0, |p| p.practice.rate())
                * vibebms_core::play::freq_rate(freq);
            // kept only when measured on the whole chart as written (not a
            // practice section, nor a play at another speed)
            let loudness = match known {
                Some(l) => Some((l, false)),
                None if normalize => Some((
                    crate::audio::loudness::measure_chart(&chart, &bank, rate),
                    speed == 1.0 && practice.is_none(),
                )),
                None => None,
            };
            if speed * watch_rate != 1.0 {
                bank.resample(0..bank.samples.len(), speed * watch_rate);
            }
            let mut bga = BgaFiles::new();
            if play.bga == BgaMode::On {
                bga.add(&index, &chart.images);
            }
            let stagefile = chart_image(&dir, &chart.meta.stagefile);
            let backbmp = chart_image(&dir, &chart.meta.backbmp);
            Ok(Box::new(Loaded {
                chart,
                bank,
                images: bga.images,
                videos: bga.videos,
                stagefile,
                backbmp,
                loudness,
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
                tr!("  ·  press a button on the 1P controller")
            } else {
                ""
            };
            let text = format!("VS  ·  {}{pads}", tr!("1P left, 2P right"));
            line(d, &text, h * 0.22, 20.0, theme::ACCENT);
        }
        if let Some(c) = &req.course {
            let stage = match c.nonstop() {
                true => tr!("NONSTOP {n} STAGES", n = c.songs.len()),
                false => tr!("STAGE {n}/{of}", n = c.index + 1, of = c.songs.len()),
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
            &format!("{}{dots}  {done}/{total}", tr!("LOADING")),
            w * 0.5,
            bar.y + 16.0 * ui,
            TextStyle::new(16.0 * ui, theme::TEXT_DIM, Align::Center),
        );
    }
}

/// A chart's `#STAGEFILE`, `#BACKBMP` (`name`, in `dir`).
fn chart_image(dir: &Path, name: &str) -> Option<Picture> {
    crate::scene::select::resolve_image(dir, name)
        .and_then(|p| crate::gfx::load_picture(&p, &|_| {}))
}

/// The decoded pixels (bytes) the pictures of one play take together at
/// most: the pictures past it are left out. A BGA of pictures is hundreds
/// of them at 256×256 or 640×480, tens to hundreds of megabytes; this is
/// 4096 of the first or 870 of the second (a quarter of it where memory is
/// short).
const MAX_PICTURE_BYTES: usize = if cfg!(small_device) {
    256 << 20
} else {
    1 << 30
};

/// Where one of a play's BGA files is kept.
#[derive(Clone, Copy)]
enum Kept {
    Picture(usize),
    Video(usize),
}

/// A play's BGA files as they are read: each file once, whatever number of
/// the chart's images name it.
struct BgaFiles {
    images: Files<Picture>,
    videos: Files<PathBuf>,
    /// By path: where the file is kept (`None`: it is not).
    kept: HashMap<PathBuf, Option<Kept>>,
    /// What the pictures to come may take of [`MAX_PICTURE_BYTES`].
    room: usize,
}

impl BgaFiles {
    fn new() -> BgaFiles {
        BgaFiles {
            images: Files::default(),
            videos: Files::default(),
            kept: HashMap::new(),
            room: MAX_PICTURE_BYTES,
        }
    }

    /// A chart's images (`files`, in the folder of `index`), each an image
    /// (animated GIF, WebP and APNG with their frames) or a video (shown
    /// when ffmpeg is installed).
    fn add(&mut self, index: &DirIndex, files: &[String]) {
        for f in files {
            let kept = self.file(index, f);
            let (picture, video) = match kept {
                Some(Kept::Picture(k)) => (Some(k), None),
                Some(Kept::Video(k)) => (None, Some(k)),
                None => (None, None),
            };
            self.images.of.push(picture);
            self.videos.of.push(video);
        }
    }

    /// Where the file a chart names `name` is kept, read if it was not yet.
    fn file(&mut self, index: &DirIndex, name: &str) -> Option<Kept> {
        use crate::gfx::video::{is_video, VIDEO_EXTS};
        use crate::platform::vfs::IMAGE_EXTS;
        let (path, video) = match index.find(name, IMAGE_EXTS) {
            Some(p) => {
                let video = is_video(&p);
                (p, video)
            }
            None => (index.find(name, VIDEO_EXTS)?, true),
        };
        if let Some(kept) = self.kept.get(&path) {
            return *kept;
        }
        let kept = match video {
            true => self.video(&path),
            false => self.picture(&path),
        };
        self.kept.insert(path, kept);
        kept
    }

    fn video(&mut self, path: &Path) -> Option<Kept> {
        // (ffmpeg reads a file: one in an archive written out of its song)
        #[cfg(not(target_arch = "wasm32"))]
        let file = crate::platform::vfs::native_path(path)?;
        #[cfg(target_arch = "wasm32")]
        let file = path.to_path_buf();
        self.videos.files.push(file);
        Some(Kept::Video(self.videos.files.len() - 1))
    }

    fn picture(&mut self, path: &Path) -> Option<Kept> {
        let left_out = || {
            log::warn!(
                "BGA: {} left out: more than {} MiB of images",
                path.display(),
                MAX_PICTURE_BYTES >> 20
            )
        };
        if self.room == 0 {
            left_out();
            return None;
        }
        let pic = crate::gfx::load_picture(path, &crate::gfx::key_black)?;
        let Some(room) = self.room.checked_sub(pic.px.len()) else {
            left_out();
            self.room = 0;
            return None;
        };
        self.room = room;
        self.images.files.push(pic);
        Some(Kept::Picture(self.images.files.len() - 1))
    }
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
    // the stages' sounds decoded together (a file once, one budget), then
    // each stage's at its speed
    let mut sounds: Vec<Option<PathBuf>> = Vec::new();
    let mut speeds = Vec::new();
    let mut bga = BgaFiles::new();
    for (span, path) in ns.stages.iter().zip(paths) {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let index = DirIndex::new(&dir);
        let files = &chart.sound_files[span.files.start as usize..span.files.end as usize];
        let from = sounds.len();
        sounds.extend(
            files
                .iter()
                .map(|f| index.find(f, crate::platform::vfs::AUDIO_EXTS)),
        );
        speeds.push((from..sounds.len(), span.ratio));
        if play.bga == BgaMode::On {
            let files = &chart.images[span.images.start as usize..span.images.end as usize];
            bga.add(&index, files);
        }
    }
    let read = |p: &Path| crate::platform::vfs::read_shared(p).ok();
    let mut bank = SoundBank::load_paths(&sounds, rate, progress, &read);
    for (stage, ratio) in speeds {
        if ratio != 1.0 {
            bank.resample(stage, ratio);
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
        images: bga.images,
        videos: bga.videos,
        stagefile,
        backbmp,
        loudness: None,
        seed,
        facts: pc.facts,
        ln_profile: pc.ln_profile,
        nonstop: Some(ns),
        section: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bga_file_is_read_once_and_pictures_within_their_budget() {
        let dir = std::env::temp_dir().join(format!("vibebms-bga-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(4, 4)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        for name in ["a.png", "b.png", "c.png"] {
            std::fs::write(dir.join(name), png.get_ref()).unwrap();
        }
        std::fs::write(dir.join("v.mp4"), b"a video").unwrap();
        let index = DirIndex::new(&dir);
        let names = [
            "a.bmp", "v.mpg", "a.png", "b.png", "none.png", "v.mp4", "a.bmp",
        ];
        let mut bga = BgaFiles::new();
        bga.add(&index, &names.map(String::from));
        assert_eq!((bga.images.files.len(), bga.videos.files.len()), (2, 1));
        let a = [Some(0), None, Some(0), Some(1), None, None, Some(0)];
        assert_eq!(bga.images.of, a);
        let v = [None, Some(0), None, None, None, Some(0), None];
        assert_eq!(bga.videos.of, v);
        assert_eq!(bga.room, MAX_PICTURE_BYTES - 2 * 4 * 4 * 4);
        // room for one picture more: the third is left out, and what
        // follows it
        bga.room = 4 * 4 * 4 + 10;
        bga.add(&index, &["c.png", "c.png", "a.png"].map(String::from));
        assert_eq!(bga.images.of[7..], [Some(2), Some(2), Some(0)]);
        let mut bga = BgaFiles::new();
        bga.room = 4 * 4 * 4 + 10;
        bga.add(
            &index,
            &["a.png", "b.png", "c.png", "a.png"].map(String::from),
        );
        assert_eq!(bga.images.of, [Some(0), None, None, Some(0)]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
