//! Image files decoded away from the frame.

use super::Picture;
use std::path::PathBuf;

/// An image file's pixels (an animation's frames); `key` turns that colour
/// transparent if the image has no alpha of its own (LR2 `#TRANSCOLOR`).
fn decode(path: &std::path::Path, key: Option<[u8; 3]>) -> Option<Picture> {
    let keyed = |px: &mut [u8]| {
        if let Some(k) = key {
            if px.as_chunks::<4>().0.iter().all(|p| p[3] == 255) {
                for p in px.as_chunks_mut::<4>().0 {
                    if p[..3] == k {
                        p[3] = 0;
                    }
                }
            }
        }
    };
    super::load_picture(path, &keyed)
}

/// Image files decoded off the main thread, one after another on a worker
/// thread (so no more than a couple are held at once); on the web, which
/// has no threads, as many as fit in a few milliseconds per `poll`.
pub struct ImageJobs {
    total: usize,
    left: usize,
    source: Source,
}

enum Source {
    Worker(std::sync::mpsc::Receiver<Option<Picture>>),
    Here(Vec<PathBuf>, Option<[u8; 3]>),
}

impl ImageJobs {
    pub fn start(paths: Vec<PathBuf>, key: Option<[u8; 3]>) -> ImageJobs {
        let total = paths.len();
        let source = if crate::platform::IS_WEB {
            Source::Here(paths, key)
        } else {
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            crate::platform::spawn("image decode", move || {
                for p in &paths {
                    // the receiver is gone: the images are not wanted
                    if tx.send(decode(p, key)).is_err() {
                        break;
                    }
                }
            });
            Source::Worker(rx)
        };
        ImageJobs {
            total,
            left: total,
            source,
        }
    }

    pub fn done(&self) -> bool {
        self.left == 0
    }

    /// The images decoded since the last call, by index in the paths
    /// given, made textures by `upload` (`None`: not an image); `wait`
    /// waits for all.
    pub fn poll<T>(
        &mut self,
        wait: bool,
        mut upload: impl FnMut(Picture) -> T,
    ) -> Vec<(usize, Option<T>)> {
        use std::sync::mpsc::TryRecvError;
        let mut out = Vec::new();
        let t0 = crate::platform::Instant::now();
        while self.left > 0 {
            let i = self.total - self.left;
            let img = match &self.source {
                Source::Worker(rx) if wait => rx.recv().ok().flatten(),
                Source::Worker(rx) => match rx.try_recv() {
                    Ok(img) => img,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => None,
                },
                Source::Here(..) if !wait && !out.is_empty() && t0.elapsed().as_millis() >= 8 => {
                    break
                }
                Source::Here(paths, key) => decode(&paths[i], *key),
            };
            self.left -= 1;
            out.push((i, img.map(&mut upload)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_decode_off_the_main_thread_in_order() {
        let dir = std::env::temp_dir().join(format!("vibebms-imgjobs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut img = image::RgbaImage::new(3, 2);
        img.put_pixel(0, 0, image::Rgba([1, 2, 3, 255]));
        img.save(dir.join("a.png")).unwrap();
        image::RgbImage::new(5, 4).save(dir.join("b.png")).unwrap();
        let paths = ["a.png", "none.png", "b.png"].map(|f| dir.join(f)).to_vec();
        let size = |p: Picture| (p.w, p.h);
        let mut jobs = ImageJobs::start(paths.clone(), None);
        let mut got = Vec::new();
        while !jobs.done() {
            got.extend(jobs.poll(false, size));
        }
        assert_eq!(got, vec![(0, Some((3, 2))), (1, None), (2, Some((5, 4)))]);
        // the colour key (black): in images without alpha only
        let mut jobs = ImageJobs::start(paths, Some([0, 0, 0]));
        let px = jobs.poll(true, |p| p.px[3]);
        assert_eq!(
            px.iter().map(|p| p.1).collect::<Vec<_>>(),
            [Some(255), None, Some(0)]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
