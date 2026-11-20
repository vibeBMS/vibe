//! Video playback for BGA and skin movies, decoded by an installed `ffmpeg`
//! (desktop builds). Frames come at a fixed rate as raw RGBA through a
//! pipe; a reader thread keeps a few ready, and each frame the newest one
//! due is uploaded into the video's texture. Without ffmpeg, videos are not
//! shown. A video costs nothing until it is about to be shown (its file is
//! looked into and decoded from then, off the main thread), and only a few
//! are decoded at once.

use crate::gfx::{Gfx, TexId};
use std::path::Path;

/// Video file extensions (charts' `#BMP`, LR2 skin `#IMAGE`).
pub const VIDEO_EXTS: &[&str] = &[
    "mpg", "mpeg", "m1v", "m2v", "avi", "wmv", "mp4", "m4v", "mkv", "webm", "mov", "flv", "ogv",
];

pub fn is_video(p: &Path) -> bool {
    VIDEO_EXTS.contains(&crate::platform::vfs::ext_lower(p).as_str())
}

/// Decoded frame rate.
pub const FPS: u32 = 30;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{ffmpeg, set_offline, size, Video};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::io::Read;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
    use std::sync::{Arc, Mutex, OnceLock};

    /// Frames are drawn offline (a video export): each waits for its
    /// video frames, so they show at their times however slowly they
    /// decode.
    static OFFLINE: AtomicBool = AtomicBool::new(false);

    /// Videos wait for their frames from now on ([`OFFLINE`]).
    pub fn set_offline(on: bool) {
        OFFLINE.store(on, Ordering::Relaxed);
    }

    /// How long an offline frame waits for a video's frame at most.
    const OFFLINE_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

    /// The decoders that run at once at most, each an ffmpeg and a few
    /// frames in memory: the four BGA layers', and a skin's. A video past
    /// them starts when one of them has ended.
    const MAX_DECODERS: usize = 6;

    static DECODERS: AtomicUsize = AtomicUsize::new(0);

    /// A place among the decoders that run, given back when dropped.
    struct Place;

    impl Place {
        fn take() -> Option<Place> {
            let mut n = DECODERS.load(Ordering::Relaxed);
            while n < MAX_DECODERS {
                let set =
                    DECODERS.compare_exchange_weak(n, n + 1, Ordering::Relaxed, Ordering::Relaxed);
                match set {
                    Ok(_) => return Some(Place),
                    Err(now) => n = now,
                }
            }
            None
        }
    }

    impl Drop for Place {
        fn drop(&mut self) {
            DECODERS.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// The ffmpeg to run (`VIBEBMS_FFMPEG`, else `ffmpeg` on the PATH), if
    /// it runs.
    pub fn ffmpeg() -> Option<&'static str> {
        static FF: OnceLock<Option<String>> = OnceLock::new();
        FF.get_or_init(|| {
            let exe = std::env::var("VIBEBMS_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
            let ok = Command::new(&exe)
                .arg("-version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                log::info!("video: ffmpeg not found, videos are not shown");
            }
            ok.then_some(exe)
        })
        .as_deref()
    }

    /// Width and height of the first video stream (from ffmpeg's report).
    fn probe(exe: &str, path: &Path) -> Option<(u32, u32)> {
        let out = Command::new(exe)
            .args(["-hide_banner", "-protocol_whitelist", "file", "-i"])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stderr);
        let line = text.lines().find(|l| l.contains("Video:"))?;
        // "..., yuv420p, 640x480 [SAR 1:1 DAR 4:3], ..."
        line.split([',', ' '])
            .filter_map(|tok| {
                let (w, h) = tok.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            })
            .find(|(w, h): &(u32, u32)| *w > 0 && *h > 0)
    }

    /// Frame size of a video (`None` without ffmpeg).
    pub fn size(path: &Path) -> Option<(u32, u32)> {
        probe(ffmpeg()?, path)
    }

    /// A decoder: a thread that runs ffmpeg and reads its frames.
    struct Run {
        rx: Receiver<Vec<u8>>,
        proc: Arc<Mutex<Proc>>,
    }

    /// A decoder's ffmpeg once it runs, and whether it is to stop.
    #[derive(Default)]
    struct Proc {
        child: Option<Child>,
        stopped: bool,
    }

    /// A decoder's thread: the frames of `path` from frame `skip` on to
    /// `tx`, until the video ends or the decoder is stopped. A video's
    /// first decoder finds the frames' `size`: above 1920x1080 they are
    /// scaled down to fit.
    fn decode(
        exe: &str,
        path: &Path,
        skip: i64,
        size: &OnceLock<(u32, u32)>,
        proc: &Mutex<Proc>,
        tx: &SyncSender<Vec<u8>>,
    ) {
        let (w, h) = match size.get() {
            Some(s) => *s,
            None => {
                let Some((w, h)) = probe(exe, path) else {
                    return;
                };
                let k = (1920.0 / w as f32).min(1080.0 / h as f32).min(1.0);
                let even = |v: f32| ((v as u32).max(2) / 2) * 2;
                let fit = (even(w as f32 * k), even(h as f32 * k));
                log::debug!("video {}: {w}x{h} -> {fit:?}", path.display());
                *size.get_or_init(|| fit)
            }
        };
        let mut cmd = Command::new(exe);
        cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if skip > 0 {
            cmd.args(["-ss", &format!("{:.3}", skip as f64 / FPS as f64)]);
        }
        // local files only: a crafted "video" (an HLS playlist, say)
        // cannot make ffmpeg open anything else
        cmd.args(["-protocol_whitelist", "file", "-i"])
            .arg(path)
            .args([
                "-an",
                "-sn",
                "-vf",
                &format!("fps={FPS},scale={w}:{h}"),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("video {}: {e}", path.display());
                return;
            }
        };
        let out = child.stdout.take();
        {
            let mut p = proc.lock().unwrap();
            if p.stopped {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            p.child = Some(child);
        }
        let Some(mut out) = out else { return };
        let n = (w * h * 4) as usize;
        loop {
            let mut buf = vec![0u8; n];
            if out.read_exact(&mut buf).is_err() || tx.send(buf).is_err() {
                break;
            }
        }
    }

    pub struct Video {
        path: PathBuf,
        looping: bool,
        /// The frames' size, known once a decoder has looked into the file.
        size: Arc<OnceLock<(u32, u32)>>,
        /// The decoder that runs.
        run: Option<Run>,
        tex: Option<TexId>,
        /// Index of the frame on the texture (-1 = none yet).
        shown: i64,
        /// Frames before the decoder's first: those of earlier loops (a
        /// looping video's position is the time since it started, minus
        /// these), and those a late start left out.
        base: i64,
        ended: bool,
    }

    impl Video {
        /// A video to play (`None` without ffmpeg). Nothing is read or
        /// run before its first frame is due or [`Video::prepare`]: what
        /// is no video shows nothing.
        pub fn open(path: &Path, looping: bool) -> Option<Video> {
            ffmpeg()?;
            Some(Video {
                path: path.to_path_buf(),
                looping,
                size: Arc::default(),
                run: None,
                tex: None,
                shown: -1,
                base: 0,
                ended: false,
            })
        }

        /// Start decoding now, so that the first frames are there when
        /// they are due.
        pub fn prepare(&mut self) {
            if self.run.is_none() && !self.ended {
                self.start(0);
            }
        }

        /// A decoder from frame `skip` of the video on, if one more may run
        /// ([`MAX_DECODERS`]: else the next frame due asks again).
        fn start(&mut self, skip: i64) {
            self.stop();
            self.shown = -1;
            self.ended = false;
            let (Some(exe), Some(place)) = (ffmpeg(), Place::take()) else {
                return;
            };
            // a few frames ahead; the pipe paces the decoder
            let (tx, rx) = sync_channel::<Vec<u8>>(3);
            let proc = Arc::new(Mutex::new(Proc::default()));
            let (path, size, shared) = (self.path.clone(), self.size.clone(), proc.clone());
            crate::platform::spawn("video", move || {
                decode(exe, &path, skip, &size, &shared, &tx);
                // (before `tx`: the place is free when the end is seen)
                drop(place);
            });
            self.run = Some(Run { rx, proc });
            self.base += skip;
        }

        fn stop(&mut self) {
            if let Some(run) = self.run.take() {
                let mut p = run.proc.lock().unwrap();
                p.stopped = true;
                if let Some(mut c) = p.child.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
        }

        /// Show the frame due `t_ms` after the video started (negative =
        /// not yet). Restarts from the beginning when `t_ms` goes back.
        pub fn update(&mut self, gfx: &mut Gfx, t_ms: i64) {
            let Some(f) = self.advance(t_ms) else { return };
            // (a frame came: its size is known)
            let Some(&(w, h)) = self.size.get() else {
                return;
            };
            match self.tex {
                Some(t) => gfx.update_texture(t, 0, 0, w, h, &f),
                None => self.tex = Some(gfx.create_texture(w, h, &f)),
            }
        }

        /// Step to the frame due at `t_ms`: the newest frame reached, if
        /// any (frames not decoded yet are caught up on later calls).
        fn advance(&mut self, t_ms: i64) -> Option<Vec<u8>> {
            if t_ms < 0 {
                return None;
            }
            let frame = t_ms * FPS as i64 / 1000;
            if frame - self.base < self.shown {
                // time went back: from the beginning
                self.stop();
                (self.base, self.shown, self.ended) = (0, -1, false);
            }
            if self.run.is_none() && !self.ended {
                // first needed now, or no decoder could run before: a
                // video that plays once starts at the frame due
                self.start(if self.looping { 0 } else { frame - self.base });
            }
            let due = frame - self.base;
            let mut latest = None;
            let offline = OFFLINE.load(Ordering::Relaxed);
            while self.shown < due && !self.ended {
                let Some(run) = &self.run else { break };
                let next = match offline {
                    true => run.rx.recv_timeout(OFFLINE_WAIT).map_err(|e| match e {
                        RecvTimeoutError::Timeout => TryRecvError::Empty,
                        RecvTimeoutError::Disconnected => TryRecvError::Disconnected,
                    }),
                    false => run.rx.try_recv(),
                };
                match next {
                    Ok(f) => {
                        if self.shown < 0 {
                            log::debug!("video {}: first frame", self.path.display());
                        }
                        self.shown += 1;
                        latest = Some(f);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        // finished: a looping video starts over
                        if self.looping && self.shown > 0 {
                            self.base += self.shown + 1;
                            self.start(0);
                        } else {
                            self.stop();
                            self.ended = true;
                        }
                        break;
                    }
                }
            }
            latest
        }

        pub fn texture(&self) -> Option<TexId> {
            self.tex
        }

        pub fn free(&mut self, gfx: &mut Gfx) {
            self.stop();
            if let Some(t) = self.tex.take() {
                gfx.free_texture(t);
            }
        }
    }

    impl Drop for Video {
        fn drop(&mut self) {
            self.stop();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A 2 s test video; frames are numbered by their colour.
        #[test]
        fn frames_follow_time_and_loop() {
            let Some(exe) = ffmpeg() else {
                eprintln!("ffmpeg not installed: skipped");
                return;
            };
            let dir = std::env::temp_dir().join(format!("vibebms-video-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let p = dir.join("v.mp4");
            // red for 1 s, then blue for 1 s
            let ok = Command::new(exe)
                .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i"])
                .arg("color=c=red:s=32x32:r=30:d=1,format=yuv444p[a];color=c=blue:s=32x32:r=30:d=1,format=yuv444p[b];[a][b]concat")
                .args(["-c:v", "libx264", "-pix_fmt", "yuv444p", "-qp", "0"])
                .arg(&p)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                // no lavfi graphs / x264 here: try a plain mpeg1 test pattern
                eprintln!("could not make a test video: skipped");
                std::fs::remove_dir_all(&dir).ok();
                return;
            }
            let mut v = Video::open(&p, true).expect("opens");
            // nothing is read or run before a frame is due
            assert!(v.run.is_none() && v.size.get().is_none());
            assert!(v.advance(-1).is_none() && v.run.is_none());
            let at = |v: &mut Video, ms: i64| {
                let mut last = None;
                for _ in 0..400 {
                    if let Some(f) = v.advance(ms) {
                        last = Some(f);
                    }
                    if v.shown == ms * FPS as i64 / 1000 - v.base {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                last.map(|f| (f[0], f[2]))
            };
            let (r, b) = at(&mut v, 500).unwrap();
            assert!(r > 200 && b < 60, "red at 0.5 s: {r} {b}");
            assert_eq!(v.size.get(), Some(&(32, 32)));
            let (r, b) = at(&mut v, 1500).unwrap();
            assert!(r < 60 && b > 200, "blue at 1.5 s: {r} {b}");
            // past the end it loops: red again
            let _ = at(&mut v, 2100);
            let (r, b) = at(&mut v, 2500).unwrap();
            assert!(r > 200 && b < 60, "red again at 2.5 s: {r} {b}");
            drop(v);
            // a video that plays once, first asked for late: from there
            let mut once = Video::open(&p, false).unwrap();
            let (r, b) = at(&mut once, 1500).unwrap();
            assert!(r < 60 && b > 200, "blue from 1.5 s: {r} {b}");
            assert_eq!(once.base, 45);
            drop(once);
            // no more decoders at once than MAX_DECODERS: one more waits
            // for a place
            let free = |n: usize| {
                for _ in 0..400 {
                    if DECODERS.load(Ordering::Relaxed) <= n {
                        return true;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                false
            };
            assert!(free(0));
            let mut all: Vec<Video> = (0..=MAX_DECODERS)
                .map(|_| Video::open(&p, false).unwrap())
                .collect();
            all.iter_mut().for_each(Video::prepare);
            let running = |all: &[Video]| all.iter().filter(|v| v.run.is_some()).count();
            assert_eq!(running(&all), MAX_DECODERS);
            assert!(all[MAX_DECODERS].run.is_none());
            all.remove(0);
            assert!(free(MAX_DECODERS - 1));
            all[MAX_DECODERS - 1].prepare();
            assert_eq!(running(&all), MAX_DECODERS);
            drop(all);
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}

/// No ffmpeg in the web build: videos are not shown.
#[cfg(target_arch = "wasm32")]
pub struct Video;

#[cfg(target_arch = "wasm32")]
pub fn size(_: &Path) -> Option<(u32, u32)> {
    None
}

#[cfg(target_arch = "wasm32")]
impl Video {
    pub fn open(_: &Path, _: bool) -> Option<Video> {
        None
    }
    pub fn prepare(&mut self) {}
    pub fn update(&mut self, _: &mut Gfx, _: i64) {}
    pub fn texture(&self) -> Option<TexId> {
        None
    }
    pub fn free(&mut self, _: &mut Gfx) {}
}
