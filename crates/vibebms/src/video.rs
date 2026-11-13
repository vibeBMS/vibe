//! Video playback for BGA and skin movies, decoded by an installed `ffmpeg`
//! (desktop builds). Frames come at a fixed rate as raw RGBA through a
//! pipe; a reader thread keeps a few ready, and each frame the newest one
//! due is uploaded into the video's texture. Without ffmpeg, videos are not
//! shown.

use crate::gfx::{Gfx, TexId};
use std::path::Path;

/// Video file extensions (charts' `#BMP`, LR2 skin `#IMAGE`).
pub const VIDEO_EXTS: &[&str] = &[
    "mpg", "mpeg", "m1v", "m2v", "avi", "wmv", "mp4", "m4v", "mkv", "webm", "mov", "flv", "ogv",
];

pub fn is_video(p: &Path) -> bool {
    p.extension()
        .map(|e| {
            let e = e.to_string_lossy().to_lowercase();
            VIDEO_EXTS.contains(&e.as_str())
        })
        .unwrap_or(false)
}

/// Decoded frame rate.
pub const FPS: u32 = 30;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{size, Video};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::io::Read;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{sync_channel, Receiver};
    use std::sync::OnceLock;

    /// The ffmpeg to run (`VIBEBMS_FFMPEG`, else `ffmpeg` on the PATH), if
    /// it runs.
    fn ffmpeg() -> Option<&'static str> {
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
            .args(["-hide_banner", "-i"])
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

    pub struct Video {
        path: PathBuf,
        looping: bool,
        size: (u32, u32),
        child: Option<Child>,
        rx: Option<Receiver<Vec<u8>>>,
        tex: Option<TexId>,
        /// Index of the frame on the texture (-1 = none yet).
        shown: i64,
        /// Frames of earlier loops (a looping video's position is the
        /// time since it started, minus these).
        base: i64,
        ended: bool,
    }

    impl Video {
        /// Prepare a video (`None` without ffmpeg or a readable video
        /// stream). Videos above 1920x1080 are scaled down to fit.
        pub fn open(path: &Path, looping: bool) -> Option<Video> {
            let exe = ffmpeg()?;
            let (w, h) = probe(exe, path)?;
            let k = (1920.0 / w as f32).min(1080.0 / h as f32).min(1.0);
            let even = |v: f32| ((v as u32).max(2) / 2) * 2;
            let mut v = Video {
                path: path.to_path_buf(),
                looping,
                size: (even(w as f32 * k), even(h as f32 * k)),
                child: None,
                rx: None,
                tex: None,
                shown: -1,
                base: 0,
                ended: false,
            };
            log::debug!("video {}: {w}x{h} -> {:?}", path.display(), v.size);
            v.start();
            Some(v)
        }

        fn start(&mut self) {
            self.stop();
            let Some(exe) = ffmpeg() else { return };
            let (w, h) = self.size;
            let child = Command::new(exe)
                .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
                .arg(&self.path)
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
                .stderr(Stdio::null())
                .spawn();
            let mut child = match child {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("video {}: {e}", self.path.display());
                    return;
                }
            };
            let Some(mut out) = child.stdout.take() else {
                return;
            };
            // a few frames ahead; the pipe paces the decoder
            let (tx, rx) = sync_channel::<Vec<u8>>(3);
            let n = (w * h * 4) as usize;
            crate::platform::spawn("video", move || loop {
                let mut buf = vec![0u8; n];
                if out.read_exact(&mut buf).is_err() || tx.send(buf).is_err() {
                    break;
                }
            });
            self.child = Some(child);
            self.rx = Some(rx);
            self.shown = -1;
            self.ended = false;
        }

        fn stop(&mut self) {
            self.rx = None;
            if let Some(mut c) = self.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }

        /// Show the frame due `t_ms` after the video started (negative =
        /// not yet). Restarts from the beginning when `t_ms` goes back.
        pub fn update(&mut self, gfx: &mut Gfx, t_ms: i64) {
            if let Some(f) = self.advance(t_ms) {
                let (w, h) = self.size;
                match self.tex {
                    Some(t) => gfx.update_texture(t, 0, 0, w, h, &f),
                    None => self.tex = Some(gfx.create_texture(w, h, &f)),
                }
            }
        }

        /// Step to the frame due at `t_ms`: the newest frame reached, if
        /// any (frames not decoded yet are caught up on later calls).
        fn advance(&mut self, t_ms: i64) -> Option<Vec<u8>> {
            if t_ms < 0 {
                return None;
            }
            let mut due = t_ms * FPS as i64 / 1000 - self.base;
            if due < self.shown {
                // time went back: from the beginning
                self.base = 0;
                due = t_ms * FPS as i64 / 1000;
                self.start();
            }
            let mut latest = None;
            while self.shown < due && !self.ended {
                match self.rx.as_ref().map(|r| r.try_recv()) {
                    Some(Ok(f)) => {
                        if self.shown < 0 {
                            log::debug!("video {}: first frame", self.path.display());
                        }
                        self.shown += 1;
                        latest = Some(f);
                    }
                    Some(Err(std::sync::mpsc::TryRecvError::Empty)) => break,
                    _ => {
                        // finished: a looping video starts over
                        if self.looping && self.shown > 0 {
                            self.base += self.shown + 1;
                            self.start();
                        } else {
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
            assert_eq!(v.size, (32, 32));
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
            let (r, b) = at(&mut v, 1500).unwrap();
            assert!(r < 60 && b > 200, "blue at 1.5 s: {r} {b}");
            // past the end it loops: red again
            let _ = at(&mut v, 2100);
            let (r, b) = at(&mut v, 2500).unwrap();
            assert!(r > 200 && b < 60, "red again at 2.5 s: {r} {b}");
            drop(v);
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
    pub fn update(&mut self, _: &mut Gfx, _: i64) {}
    pub fn texture(&self) -> Option<TexId> {
        None
    }
    pub fn free(&mut self, _: &mut Gfx) {}
}
