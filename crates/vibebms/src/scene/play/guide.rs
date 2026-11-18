//! GUIDE SE: a short sound for each judgement as it is made. From the
//! sound set chosen, by beatoraja's file names (`guide-pg`, `guide-gr`,
//! `guide-gd`, `guide-bd`, `guide-pr` for an empty POOR and `guide-ms` for
//! a missed note; any audio format); a judgement the set has none for (an
//! LR2 sound set, the built-in set) gets a built-in tone.

use crate::platform::{vfs, Job};
use crate::sysound::tone;
use std::path::Path;
use std::sync::Arc;
use vibebms_audio::{Bus, Mixer, PlayOptions, Sample};
use vibebms_core::judge::Judge;

/// The sound set's file name of a judgement's guide sound.
pub(crate) fn file(j: Judge) -> &'static str {
    match j {
        Judge::PGreat => "guide-pg.wav",
        Judge::Great => "guide-gr.wav",
        Judge::Good => "guide-gd.wav",
        Judge::Bad => "guide-bd.wav",
        // (vibeBMS's POOR is a missed note, its MISS an empty POOR)
        Judge::Poor => "guide-ms.wav",
        Judge::Miss => "guide-pr.wav",
    }
}

/// The built-in guide sound of a judgement: higher and brighter the
/// better, a low buzz for a miss.
fn built_in(j: Judge, rate: u32) -> Sample {
    match j {
        Judge::PGreat => tone(rate, 45.0, 1760.0, 1760.0, 0.18),
        Judge::Great => tone(rate, 45.0, 1320.0, 1320.0, 0.18),
        Judge::Good => tone(rate, 50.0, 990.0, 990.0, 0.18),
        Judge::Bad => tone(rate, 70.0, 520.0, 440.0, 0.2),
        Judge::Poor => tone(rate, 110.0, 220.0, 180.0, 0.22),
        Judge::Miss => tone(rate, 40.0, 330.0, 330.0, 0.15),
    }
}

/// Mixer voice of a judgement's guide sound (each cuts its own).
fn key(j: Judge) -> u64 {
    u64::MAX - 200 + j as u64
}

/// The guide sounds of a set, decoded off the main thread.
#[derive(Default)]
pub(crate) struct GuideSounds {
    samples: [Option<Arc<Sample>>; 6],
    job: Option<Job<Vec<(Judge, Sample)>>>,
}

impl GuideSounds {
    /// The guide sounds of the set `value` names (a path relative to
    /// `root`; empty: the built-in set): the built-in ones at once, the
    /// set's own when decoded.
    pub fn load(value: &str, root: &Path, rate: u32) -> GuideSounds {
        let samples = Judge::ALL.map(|j| Some(Arc::new(built_in(j, rate))));
        let dir = root.join(value);
        let job = (!value.is_empty() && vfs::is_dir(&dir)).then(|| {
            Job::spawn("guide SE", move |_| {
                let mut out = Vec::new();
                for j in Judge::ALL {
                    let Some(p) = vfs::find_by_stem(&dir, file(j), vfs::AUDIO_EXTS) else {
                        continue;
                    };
                    let ext = p
                        .extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    match vfs::read(&p)
                        .map_err(|e| e.to_string())
                        .and_then(|b| vibebms_audio::decode_bytes(b, &ext, rate))
                    {
                        Ok(x) => out.push((j, x)),
                        Err(e) => log::warn!("guide SE: {}: {e}", p.display()),
                    }
                }
                out
            })
        });
        GuideSounds { samples, job }
    }

    /// Take the set's own sounds once they are decoded.
    pub fn poll(&mut self) {
        let Some(v) = self.job.as_mut().and_then(Job::poll) else {
            return;
        };
        self.job = None;
        for (j, x) in v {
            self.samples[j as usize] = Some(Arc::new(x));
        }
    }

    pub fn play(&self, m: &Mixer, j: Judge) {
        if let Some(x) = &self.samples[j as usize] {
            m.play(
                x,
                PlayOptions {
                    key: key(j),
                    bus: Bus::System,
                    ..Default::default()
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_s_own_and_the_built_in_ones() {
        let root = std::env::temp_dir().join(format!("vibebms-guide-{}", std::process::id()));
        let set = root.join("sounds/raja");
        std::fs::create_dir_all(&set).unwrap();
        std::fs::write(set.join("GUIDE-PG.ogg"), b"x").unwrap();
        assert!(vfs::find_by_stem(&set, file(Judge::PGreat), vfs::AUDIO_EXTS).is_some());
        assert!(vfs::find_by_stem(&set, file(Judge::Great), vfs::AUDIO_EXTS).is_none());
        std::fs::remove_dir_all(&root).ok();
        // none in the set: every judgement has a built-in sound, each its own
        let g = GuideSounds::load("", Path::new("/nonexistent"), 44100);
        assert!(g.job.is_none() && g.samples.iter().all(Option::is_some));
        let lens: Vec<usize> = Judge::ALL
            .iter()
            .map(|j| built_in(*j, 44100).frames)
            .collect();
        assert!(lens.iter().all(|n| *n > 0));
        assert_eq!(file(Judge::Miss), "guide-pr.wav");
    }
}
