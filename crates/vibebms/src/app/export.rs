//! `vibebms export video` (desktop): an autoplay or a replay of a chart
//! rendered offline to a video file. Frames are drawn at a fixed step of
//! the play's own clock (not the wall clock) into a texture with no window
//! and piped to an installed `ffmpeg` (the one video BGA uses); the chart's
//! sounds are mixed offline at the chart's sample rate into a WAV file,
//! which a second `ffmpeg` run puts beside the frames.

use crate::ctx::Ctx;
use crate::gfx::text::Text;
use crate::gfx::{Batch, Draw, Offscreen};
use crate::paths::Paths;
use crate::platform::{Duration, Instant};
use crate::scene::play::{PlayRequest, PlayScene};
use crate::scene::{Next, Scene};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use vibebms_audio::Mixer;
use vibebms_core::replay::Slot;

pub const USAGE: &str = "\
usage: vibebms export video <chart> [--replay <best|last|1-4>] [--out <file.mp4>]
                            [--size <W>x<H>] [--fps <fps>] [--duration <seconds>]

  <chart>                     the chart (autoplayed unless --replay is given)
  --replay <slot>             its replay instead: best, last or a saved slot 1-4
  --out <file>                the video file (default: <chart name>.mp4 here);
                              its extension is the container (mp4, mkv, mov, webm)
  --size <W>x<H>              the frame size, even numbers (default 1280x720)
  --fps <fps>                 frames a second (default 60)
  --duration <seconds>        at most this long (default: to the play's end)";

/// What `export video` is asked to make.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoExport {
    pub chart: PathBuf,
    /// A replay's slot (else an autoplay).
    pub replay: Option<Slot>,
    pub out: Option<PathBuf>,
    pub size: (u32, u32),
    pub fps: u32,
    /// Seconds at most.
    pub duration: Option<f64>,
}

impl VideoExport {
    /// The arguments after `export video`.
    pub fn parse(args: &[String]) -> Result<VideoExport, String> {
        let mut chart = None;
        let mut e = VideoExport {
            chart: PathBuf::new(),
            replay: None,
            out: None,
            size: (1280, 720),
            fps: 60,
            duration: None,
        };
        let mut args = super::cli::Args::new(args);
        while let Some(name) = args.next() {
            let mut value = || args.value(name);
            match name {
                "--replay" => {
                    let v = value()?;
                    e.replay = Some(slot(&v).ok_or_else(|| {
                        format!(
                            "--replay {v:?}: best, last or a slot 1-{}",
                            vibebms_core::replay::SAVED
                        )
                    })?);
                }
                "--out" | "-o" => e.out = Some(PathBuf::from(value()?)),
                "--size" => {
                    let v = value()?;
                    let size = v.split_once(['x', 'X']).and_then(|(w, h)| {
                        Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
                    });
                    e.size = match size {
                        Some((w, h)) if w >= 2 && h >= 2 && w % 2 == 0 && h % 2 == 0 => (w, h),
                        Some(_) => return Err(format!("--size {v}: even numbers, 2 or more")),
                        None => return Err(format!("--size {v}: <width>x<height>")),
                    };
                }
                "--fps" => {
                    let v = value()?;
                    e.fps = match v.parse::<u32>() {
                        Ok(f) if (1..=240).contains(&f) => f,
                        _ => return Err(format!("--fps {v}: 1 to 240")),
                    };
                }
                "--duration" => {
                    let v = value()?;
                    e.duration = match v.parse::<f64>() {
                        Ok(s) if s > 0.0 && s.is_finite() => Some(s),
                        _ => return Err(format!("--duration {v}: seconds, more than 0")),
                    };
                }
                n if n.starts_with('-') && n.len() > 1 => {
                    return Err(format!("export video: unknown option {n}"))
                }
                a if chart.is_none() => chart = Some(PathBuf::from(a)),
                a => return Err(format!("export video: one chart only ({a:?})")),
            }
        }
        e.chart = chart.ok_or("export video: which chart?")?;
        Ok(e)
    }

    /// The file it writes: `--out`, else the chart's name with `.mp4`.
    pub fn out_path(&self) -> PathBuf {
        self.out.clone().unwrap_or_else(|| {
            let stem = self.chart.file_stem().unwrap_or_default();
            PathBuf::from(stem).with_extension("mp4")
        })
    }
}

/// `best`, `last`, `1`..`4` (`slot1`..).
fn slot(s: &str) -> Option<Slot> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "best" => Some(Slot::Best),
        "last" => Some(Slot::Last),
        n => {
            let n: u8 = n.strip_prefix("slot").unwrap_or(n).parse().ok()?;
            (1..=vibebms_core::replay::SAVED)
                .contains(&n)
                .then_some(Slot::Saved(n))
        }
    }
}

/// What was made.
#[derive(Clone, Debug)]
pub struct Exported {
    pub out: PathBuf,
    pub frames: u64,
    pub seconds: f64,
    pub sample_rate: u32,
}

/// Set by Ctrl-C: the export stops and leaves nothing behind.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Ctrl-C (the console's break on Windows) sets [`CANCEL`].
fn on_ctrl_c() {
    #[cfg(unix)]
    {
        extern "C" fn handler(_: i32) {
            CANCEL.store(true, Ordering::Relaxed);
        }
        extern "C" {
            fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        const SIGINT: i32 = 2;
        // SAFETY: the handler only stores to an atomic
        unsafe {
            signal(SIGINT, handler);
        }
    }
    #[cfg(windows)]
    {
        unsafe extern "system" fn handler(_: u32) -> i32 {
            CANCEL.store(true, Ordering::Relaxed);
            1
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleCtrlHandler(
                handler: Option<unsafe extern "system" fn(u32) -> i32>,
                add: i32,
            ) -> i32;
        }
        // SAFETY: the handler only stores to an atomic
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}

/// `vibebms export ...` from the command line: the exit code.
pub fn run(paths: &Paths, args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("video") => {}
        Some("-h" | "--help") | None => {
            println!("{USAGE}");
            return if args.is_empty() { 2 } else { 0 };
        }
        Some(o) => {
            eprintln!("vibebms: export: unknown kind {o:?} (video)\n\n{USAGE}");
            return 2;
        }
    }
    if args[1..].iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let opts = match VideoExport::parse(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("vibebms: {e}\n\n{USAGE}");
            return 2;
        }
    };
    on_ctrl_c();
    let mut last = Instant::now();
    let mut progress = |done: u64, total: Option<u64>, fps: u32| {
        let now = Instant::now();
        if done > 1 && total != Some(done) && now.duration_since(last) < Duration::from_millis(250)
        {
            return;
        }
        last = now;
        let secs = done as f64 / fps as f64;
        let line = match total {
            Some(t) => format!(
                "\rframe {done}/{t} ({:.0} %), {secs:.1} s",
                done as f64 * 100.0 / t.max(1) as f64
            ),
            None => format!("\rframe {done}, {secs:.1} s"),
        };
        print!("{line}   ");
        let _ = std::io::stdout().flush();
    };
    match export_video(paths, &opts, &CANCEL, &mut progress) {
        Ok(e) => {
            println!(
                "\n{}: {} frames, {:.2} s, {}x{} at {} fps, sound at {} Hz",
                e.out.display(),
                e.frames,
                e.seconds,
                opts.size.0,
                opts.size.1,
                opts.fps,
                e.sample_rate
            );
            0
        }
        Err(e) if CANCEL.load(Ordering::Relaxed) => {
            println!("\ncancelled ({e})");
            130
        }
        Err(e) => {
            eprintln!("\nvibebms: export video: {e}");
            1
        }
    }
}

/// The sample rate the chart's sounds are recorded at (the most common of
/// its first ones), else 44.1 kHz.
fn chart_rate(path: &Path, chart: &vibebms_core::Chart) -> u32 {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let index = crate::platform::vfs::DirIndex::new(&dir);
    let mut rates: Vec<(u32, usize)> = Vec::new();
    for name in chart.sound_files.iter().take(32) {
        let Some(p) = index.find(name, crate::platform::vfs::AUDIO_EXTS) else {
            continue;
        };
        let Ok(bytes) = crate::platform::vfs::read(&p) else {
            continue;
        };
        let ext = crate::platform::vfs::ext_lower(&p);
        if let Some(r) = vibebms_audio::decode::native_rate(bytes, &ext) {
            match rates.iter_mut().find(|(x, _)| *x == r) {
                Some((_, n)) => *n += 1,
                None => rates.push((r, 1)),
            }
        }
    }
    rates
        .iter()
        .max_by_key(|(_, n)| *n)
        .map_or(44_100, |(r, _)| (*r).clamp(8_000, 192_000))
}

/// A 16-bit stereo WAV file written as the frames come; its sizes are
/// filled in at the end.
struct Wav {
    file: std::io::BufWriter<std::fs::File>,
    frames: u64,
}

impl Wav {
    fn create(path: &Path, rate: u32) -> std::io::Result<Wav> {
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&2u16.to_le_bytes());
        h.extend_from_slice(&rate.to_le_bytes());
        h.extend_from_slice(&(rate * 4).to_le_bytes());
        h.extend_from_slice(&4u16.to_le_bytes());
        h.extend_from_slice(&16u16.to_le_bytes());
        h.extend_from_slice(b"data\0\0\0\0");
        file.write_all(&h)?;
        Ok(Wav { file, frames: 0 })
    }

    fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        let mut b = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        self.frames += samples.len() as u64 / 2;
        self.file.write_all(&b)
    }

    fn finish(mut self) -> std::io::Result<()> {
        let data = (self.frames * 4).min(u32::MAX as u64 - 36) as u32;
        self.file.seek(SeekFrom::Start(4))?;
        self.file.write_all(&(36 + data).to_le_bytes())?;
        self.file.seek(SeekFrom::Start(40))?;
        self.file.write_all(&data.to_le_bytes())?;
        self.file.flush()
    }
}

/// Files made on the way, removed however the export ends (the video
/// too unless it was finished).
struct Cleanup {
    files: Vec<PathBuf>,
    child: Option<Child>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        for f in &self.files {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// Videos wait for their frames while it is held (`gfx::video::set_offline`).
struct OfflineVideos;

impl OfflineVideos {
    fn on() -> OfflineVideos {
        crate::gfx::video::set_offline(true);
        OfflineVideos
    }
}

impl Drop for OfflineVideos {
    fn drop(&mut self) {
        crate::gfx::video::set_offline(false);
    }
}

/// `path` with `.<what>` before its extension.
fn beside(path: &Path, what: &str, ext: &str) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{stem}.{what}.{ext}"))
}

/// Render the play `opts` asks for into its video file; `progress` is
/// told the frames made (and how many there will be, when known).
/// `cancel` stops it; nothing is left behind then, nor on an error.
pub fn export_video(
    paths: &Paths,
    opts: &VideoExport,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, Option<u64>, u32),
) -> Result<Exported, String> {
    let ffmpeg = crate::gfx::video::ffmpeg()
        .ok_or("ffmpeg was not found: install it, or set VIBEBMS_FFMPEG to its path")?;
    let (w, h) = opts.size;
    let fps = opts.fps.max(1);
    let out = opts.out_path();
    let ext = out
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "mp4".into());
    let path = std::path::absolute(&opts.chart).unwrap_or_else(|_| opts.chart.clone());
    let (song, chart) = super::cli::song_at(&path)?;
    let rate = chart_rate(&path, &chart);

    let mut config = crate::profiles::load_config(paths);
    // as watched, the whole screen the play's, at the speed it was played
    config.touch.mode = crate::config::TouchMode::Off;
    config.select.replay_speed = 100;
    let mut off = Offscreen::new(w, h)
        .map_err(|e| format!("drawing without a window is not available here: {e}"))?;
    let offline = crate::config::AudioSettings {
        buffer_frames: 0,
        sample_rate: rate,
        exclusive: false,
        ..config.audio.clone()
    };
    let mixer = Mixer::offline(&offline.mixer_config(512), rate);
    let screen = crate::platform::screen::Screen {
        size_px: (w as f32, h as f32),
        points_per_inch: 96.0,
    };
    let size = winit::dpi::PhysicalSize::new(w, h);
    let view = crate::ctx::View {
        fps: fps as f32,
        ..crate::ctx::View::new(size, 1.0, screen, &off.gfx.adapter_name)
    };
    let profiles = crate::profiles::Profiles::open(paths);
    let mut ctx = Ctx::new(
        paths.clone(),
        profiles,
        config,
        Default::default(),
        mixer,
        view,
    );
    ctx.open_records();
    ctx.reload_library();
    let replay = match opts.replay {
        Some(slot) => {
            let p = &ctx.config.play;
            let ln = song.score_ln(p.ln_pref()) as i32;
            let fixed = song.fixed_ln() && !p.ln_force;
            let player = &ctx.paths.player;
            let r = crate::library::replay::load_filed(
                player,
                &song.sha256,
                ln,
                p.judge_preset,
                fixed,
                slot,
            )
            .ok_or_else(|| format!("no {} replay of {}", slot.label(), song.title))?;
            Some(std::sync::Arc::new(r))
        }
        None => None,
    };
    let req = PlayRequest {
        song,
        autoplay: replay.is_none(),
        course: None,
        replay,
        vs: None,
        practice: None,
    };
    let mut text = Text::new(&[ctx.paths.fonts.as_path()]);
    let mut batch = Batch::default();
    let _videos = OfflineVideos::on();
    let mut scene = PlayScene::new(&mut ctx, req);

    // loading, by the wall clock
    let mut now = Instant::now();
    let began = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("while loading".into());
        }
        now = now.max(Instant::now());
        let norm = crate::audio::loudness::system_target(&ctx.config.audio);
        ctx.audio.sounds.poll(&ctx.audio.mixer, norm);
        ctx.audio.loudness.poll();
        if !matches!(scene.update(&mut ctx, &mut off.gfx, now), Next::Stay) {
            let why = ctx.toasts.last().map(|t| t.0.clone());
            return Err(why.unwrap_or_else(|| "the chart could not be played".into()));
        }
        if let Some(b) = scene.began() {
            break b;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    // from here the play's own clock: frame k at `base` + k / fps
    let base = began.max(now);
    let mut sound = Vec::new();
    ctx.audio.mixer.render_until(base, &mut sound);
    sound.clear();

    let video_tmp = beside(&out, "video", &ext);
    let wav_tmp = beside(&out, "sound", "wav");
    let mut cleanup = Cleanup {
        files: vec![video_tmp.clone(), wav_tmp.clone()],
        child: None,
    };
    let mut wav = Wav::create(&wav_tmp, rate).map_err(|e| format!("{}: {e}", wav_tmp.display()))?;
    let mut child = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba"])
        .args(["-s", &format!("{w}x{h}"), "-r", &fps.to_string(), "-i", "-"])
        .args(["-pix_fmt", "yuv420p", "-an"])
        .arg(&video_tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{ffmpeg}: {e}"))?;
    let mut pipe = child.stdin.take().ok_or("ffmpeg's input")?;
    cleanup.child = Some(child);

    let limit = opts
        .duration
        .map(|s| (s * fps as f64).round().max(1.0) as u64);
    // (a play that never ends: the chart's length and a minute more)
    let longest = ((chart.length().max(0) as f64 / 1e6 + 60.0) * fps as f64) as u64;
    let total = limit.map(|l| l.min(longest));
    let at = |k: u64| base + Duration::from_nanos(k * 1_000_000_000 / fps as u64);
    let mut frames = 0u64;
    while frames < total.unwrap_or(longest) {
        if cancel.load(Ordering::Relaxed) {
            return Err(format!("at frame {frames}"));
        }
        let t = at(frames);
        if scene.shown_to_end(t) {
            break;
        }
        ctx.audio.mixer.render_until(t, &mut sound);
        wav.write(&sound).map_err(|e| e.to_string())?;
        sound.clear();
        let _ = ctx.audio.mixer.keep_alive();
        if !matches!(scene.update(&mut ctx, &mut off.gfx, t), Next::Stay) {
            break;
        }
        batch.clear();
        let mut d = Draw {
            gfx: &off.gfx,
            text: &mut text,
            b: &mut batch,
            pointer: None,
        };
        scene.draw(&mut ctx, &mut d, t);
        let px = off.frame(&batch, crate::gfx::theme::BACKGROUND)?;
        if let Err(e) = pipe.write_all(&px) {
            if cancel.load(Ordering::Relaxed) {
                return Err(format!("at frame {frames}"));
            }
            let err = ffmpeg_error(&mut cleanup);
            return Err(format!("ffmpeg stopped taking frames ({e}){err}"));
        }
        frames += 1;
        progress(frames, total, fps);
    }
    progress(frames, Some(frames), fps);
    // the sound to the end of the last frame
    ctx.audio.mixer.render_until(at(frames), &mut sound);
    wav.write(&sound).map_err(|e| e.to_string())?;
    wav.finish().map_err(|e| e.to_string())?;
    drop(pipe);
    let mut child = cleanup.child.take().ok_or("ffmpeg")?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        cleanup.child = Some(child);
        return Err(format!(
            "ffmpeg could not make the video{}",
            ffmpeg_error(&mut cleanup)
        ));
    }
    if cancel.load(Ordering::Relaxed) || frames == 0 {
        return Err(format!("at frame {frames}"));
    }
    // the frames and the sound in one file
    let mux = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&video_tmp)
        .arg("-i")
        .arg(&wav_tmp)
        .args(["-map", "0:v", "-map", "1:a", "-c:v", "copy"])
        .arg(&out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{ffmpeg}: {e}"))?;
    if !mux.status.success() || cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&out);
        let err = String::from_utf8_lossy(&mux.stderr);
        return Err(format!("ffmpeg could not add the sound: {}", err.trim()));
    }
    Ok(Exported {
        out,
        frames,
        seconds: frames as f64 / fps as f64,
        sample_rate: rate,
    })
}

/// What a failed ffmpeg said (its error output), after it ended.
fn ffmpeg_error(c: &mut Cleanup) -> String {
    let Some(mut child) = c.child.take() else {
        return String::new();
    };
    let _ = child.kill();
    let out = child.wait_with_output().ok();
    let err = out.map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string());
    match err {
        Some(e) if !e.is_empty() => format!(": {e}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<VideoExport, String> {
        let v: Vec<String> = s.split_whitespace().map(String::from).collect();
        VideoExport::parse(&v)
    }

    #[test]
    fn parses_its_options() {
        let e = parse("a.bms --replay best --out x.mkv --size 640x360 --fps 30").unwrap();
        assert_eq!(e.chart, PathBuf::from("a.bms"));
        assert_eq!(e.replay, Some(Slot::Best));
        assert_eq!(e.out_path(), PathBuf::from("x.mkv"));
        assert_eq!((e.size, e.fps), ((640, 360), 30));
        let e = parse("dir/song.bme --replay=3 --duration 2").unwrap();
        assert_eq!(e.replay, Some(Slot::Saved(3)));
        assert_eq!(e.duration, Some(2.0));
        assert_eq!(e.out_path(), PathBuf::from("song.mp4"));
        assert_eq!(
            parse("a.bms --replay last").unwrap().replay,
            Some(Slot::Last)
        );
        assert_eq!(parse("a.bms").unwrap().size, (1280, 720));
        let e = parse("--size=640x360 a.bms --fps=30 -o out.webm").unwrap();
        assert_eq!(
            (e.size, e.fps, e.out_path()),
            ((640, 360), 30, "out.webm".into())
        );
        assert_eq!(parse("a.bms --out").unwrap_err(), "--out needs a value");
        for bad in [
            "",
            "a.bms --replay 9",
            "a.bms --size 641x360",
            "a.bms --size big",
            "a.bms --fps 0",
            "a.bms b.bms",
            "a.bms --loud",
            "a.bms --duration -1",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    /// The sound file's sizes are filled in at the end.
    #[test]
    fn wav_sizes() {
        let dir = std::env::temp_dir().join(format!("vibebms-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        let mut w = Wav::create(&p, 48_000).unwrap();
        w.write(&[0.5, -0.5, 1.5, 0.0]).unwrap();
        w.finish().unwrap();
        let b = std::fs::read(&p).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(b.len(), 44 + 8);
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(u32::from_le_bytes(b[40..44].try_into().unwrap()), 8);
        assert_eq!(native_rate(&b), Some(48_000));
        assert_eq!(i16::from_le_bytes([b[48], b[49]]), 32767);
    }

    fn native_rate(b: &[u8]) -> Option<u32> {
        vibebms_audio::decode::native_rate(b.to_vec(), "wav")
    }

    /// A 22.05 kHz mono WAV: a short beep.
    fn beep() -> Vec<u8> {
        let rate = 22_050u32;
        let n = rate as usize / 10;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + 2 * n as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        for v in [16u32, 1 | 1 << 16, rate, rate * 2, 2 | 16 << 16] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(2 * n as u32).to_le_bytes());
        for i in 0..n {
            let v = (i as f32 * 0.2).sin() * 12_000.0;
            b.extend_from_slice(&(v as i16).to_le_bytes());
        }
        b
    }

    /// (frames, seconds, audio streams' sample rate) of a video file as
    /// ffprobe counts them.
    fn probe(ffmpeg: &str, path: &Path) -> (u64, f64, Option<u32>) {
        let probe = Path::new(ffmpeg).with_file_name("ffprobe");
        let exe = match probe.is_absolute() && probe.exists() {
            true => probe.to_string_lossy().into_owned(),
            false => "ffprobe".into(),
        };
        let out = Command::new(exe)
            .args(["-v", "error", "-count_frames", "-show_entries"])
            .args(["stream=codec_type,nb_read_frames,sample_rate:format=duration"])
            .args(["-of", "default=noprint_wrappers=1"])
            .arg(path)
            .output()
            .expect("ffprobe runs");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let field = |k: &str| {
            text.lines()
                .filter_map(|l| l.strip_prefix(k)?.strip_prefix('='))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let frames = field("nb_read_frames")[0].parse().unwrap();
        let secs = field("duration")[0].parse().unwrap();
        let rate = field("sample_rate").iter().find_map(|r| r.parse().ok());
        (frames, secs, rate)
    }

    /// Two seconds of a tiny chart's autoplay at 64x36, then a second of
    /// a replay of it: frame-exact (as many frames as the time at the rate
    /// asked), with the chart's sound at its rate; nothing else left.
    /// Skipped without ffmpeg or a GPU (or software one) to draw with.
    #[test]
    fn exports_an_autoplay_and_a_replay() {
        let Some(ffmpeg) = crate::gfx::video::ffmpeg() else {
            eprintln!("ffmpeg not installed: skipped");
            return;
        };
        if let Err(e) = Offscreen::new(64, 36) {
            eprintln!("no headless drawing ({e}): skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!("vibebms-export-{}", std::process::id()));
        let songs = root.join("songs/t");
        std::fs::create_dir_all(&songs).unwrap();
        std::fs::write(songs.join("k.wav"), beep()).unwrap();
        let text = "#PLAYER 1\n#TITLE Tiny\n#BPM 150\n#WAV01 k.wav\n\
#00111:01010101\n#00112:0001\n#00101:01\n#00211:0101\n#00213:01\n";
        let chart_path = songs.join("tiny.bms");
        std::fs::write(&chart_path, text).unwrap();
        let paths = Paths::at(root.clone());
        let cancel = AtomicBool::new(false);
        let mut seen = 0;
        let out = root.join("auto.mp4");
        let opts = VideoExport {
            chart: chart_path.clone(),
            replay: None,
            out: Some(out.clone()),
            size: (64, 36),
            fps: 60,
            duration: Some(2.0),
        };
        let e = export_video(&paths, &opts, &cancel, &mut |n, total, _| {
            assert_eq!(total, Some(120));
            seen = n;
        })
        .unwrap();
        assert_eq!((e.frames, seen, e.sample_rate), (120, 120, 22_050));
        let (frames, secs, rate) = probe(ffmpeg, &out);
        assert_eq!(frames, 120);
        assert!((secs - 2.0).abs() < 0.05, "{secs} s");
        assert_eq!(rate, Some(22_050));
        // (the parts it was made of are gone)
        let left: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".wav") || n.contains(".video."))
            .collect();
        assert!(left.is_empty(), "{left:?}");

        // a replay of it: the bot's play, saved as the best one
        let (song, _) = crate::app::cli::song_at(&chart_path).unwrap();
        let play = crate::config::PlayConfig::default();
        let built =
            vibebms_core::play::build_chart(&chart_path, text.as_bytes(), &play, 7, None).unwrap();
        let (mut eng, _) = vibebms_core::play::build_engine(&built.chart, &play, None, false);
        eng.record();
        let inputs = vibebms_core::bot::inputs(&built.chart, None, 3);
        vibebms_core::bot::drive(&mut eng, &inputs, built.chart.length() + 2_000_000);
        let r = crate::library::replay::Replay {
            version: crate::library::replay::VERSION,
            sha256: song.sha256.clone(),
            md5: song.md5.clone(),
            title: song.title.clone(),
            ln: song.score_ln(play.ln_pref()) as i32,
            date: 0,
            seed: 7,
            play: play.clone(),
            ex_score: eng.score.ex_score(),
            clear: Default::default(),
            ghost: eng.ghost.clone(),
            ops: eng.recorded().to_vec(),
            course: None,
        };
        crate::library::replay::save(&r, &root, Slot::Best).unwrap();
        let out = root.join("replay.mkv");
        let opts = VideoExport {
            replay: Some(Slot::Best),
            out: Some(out.clone()),
            fps: 30,
            duration: Some(1.0),
            ..opts
        };
        let e = export_video(&paths, &opts, &cancel, &mut |_, _, _| {}).unwrap();
        assert_eq!(e.frames, 30);
        assert_eq!(probe(ffmpeg, &out).0, 30);
        // no replay in a slot: said
        let opts = VideoExport {
            replay: Some(Slot::Saved(2)),
            ..opts
        };
        let err = export_video(&paths, &opts, &cancel, &mut |_, _, _| {}).unwrap_err();
        assert!(err.contains("SLOT 2"), "{err}");
        // cancelled: nothing left behind
        let out = root.join("cancelled.mp4");
        let opts = VideoExport {
            replay: None,
            out: Some(out.clone()),
            ..opts
        };
        let cancel = AtomicBool::new(false);
        let err = export_video(&paths, &opts, &cancel, &mut |n, _, _| {
            if n == 5 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap_err();
        assert!(err.contains("frame 5"), "{err}");
        assert!(!out.exists() && !beside(&out, "sound", "wav").exists());
        assert!(!beside(&out, "video", "mp4").exists());
        std::fs::remove_dir_all(&root).ok();
    }
}
