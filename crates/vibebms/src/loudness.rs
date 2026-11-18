//! Loudness normalisation: a chart's sounds, the song select's preview and
//! the sound set's music played at a common loudness
//! (`SETTINGS › DISPLAY & AUDIO`, `[audio] normalize*`).
//!
//! A chart is measured as its BGM and every note's keysound autoplayed
//! (a sound played again cuts itself off, as in play), mixed a tenth of a
//! second at a time into a BS.1770 meter (`vibebms_audio::loudness`): its
//! integrated loudness (LUFS) and sample peak. That is done when it is
//! first played, with its sounds just decoded, and kept by the chart's
//! SHA-256 in `cache/loudness.txt`. Previews and the sound set's select
//! music and decide jingle are measured as they are decoded.
//!
//! The gain brings each to the target: down as far as needed (30 dB at
//! most), up at most `MAX_BOOST_DB` and no further than the peak reaching
//! `CEILING`, where the output's soft limiter starts (so normalising adds
//! no distortion).

use crate::config::AudioSettings;
use crate::platform::{store, Job};
use std::collections::HashMap;
use std::path::PathBuf;
use vibebms_audio::loudness::{measure_mix, normal_gain, Placed};
pub use vibebms_audio::loudness::{Loudness, Meter};
use vibebms_audio::{Bus, Mixer, Sample, SoundBank};
use vibebms_core::chart::{Chart, NO_SOUND};

/// The most a quiet chart is raised (dB).
pub const MAX_BOOST_DB: f32 = 12.0;
/// The highest peak a raised chart may reach (the output's soft limiter
/// starts there).
pub const CEILING: f32 = 0.8;
/// The target range offered (LUFS).
pub const TARGET_RANGE: (f32, f32) = (-24.0, -6.0);

/// The web and mobile measure a chart from every second frame (half the
/// work, a little off for very bright music); elsewhere every frame.
const STEP: usize = if cfg!(any(
    target_arch = "wasm32",
    target_os = "android",
    target_os = "ios"
)) {
    2
} else {
    1
};

/// The loudness a chart's sounds and previews are played at, if they are
/// normalised.
pub fn chart_target(a: &AudioSettings) -> Option<f32> {
    a.normalize.then_some(a.normalize_target)
}

/// The loudness the sound set's music is played at, if it is normalised.
pub fn system_target(a: &AudioSettings) -> Option<f32> {
    a.normalize_system.then_some(a.normalize_target)
}

/// The gain that plays `l` at `target` LUFS.
pub fn gain(l: &Loudness, target: f32) -> f32 {
    normal_gain(l, target, MAX_BOOST_DB, CEILING)
}

/// A play starts: the chart's sounds (keysounds and BGM) at its gain; a
/// loudness just measured is kept.
pub fn start_play(ctx: &mut crate::app::Ctx, sha256: &str, l: Option<(Loudness, bool)>) {
    if let Some((l, true)) = l {
        ctx.audio.loudness.put(sha256, l);
    }
    let g = match (chart_target(&ctx.config.audio), l) {
        (Some(t), Some((l, _))) => gain(&l, t),
        _ => 1.0,
    };
    log::debug!("loudness: {l:?}, gain {g:.3}");
    for bus in [Bus::Key, Bus::Bgm] {
        ctx.audio.mixer.set_bus_gain(bus, g);
    }
}

/// A play ends: the buses as they were.
pub fn end_play(mixer: &Mixer) {
    for bus in [Bus::Key, Bus::Bgm] {
        mixer.set_bus_gain(bus, 1.0);
    }
}

/// A sound (a preview, the select music) at `rate`, measured.
pub fn measure_sound(s: &Sample, rate: u32) -> Loudness {
    let mut m = Meter::new(rate);
    m.push_sample(s);
    m.finish()
}

/// The chart's sounds (decoded in `bank` at `rate`) autoplayed, measured.
pub fn measure_chart(chart: &Chart, bank: &SoundBank, rate: u32) -> Loudness {
    let frames = |us: i64| (us.max(0) as i128 * rate as i128 / 1_000_000) as usize;
    let mut ev: Vec<(i64, u32)> = (chart.bgm.iter())
        .map(|b| (b.time, b.sound))
        .chain(
            (chart.notes.iter())
                .filter(|n| n.is_playable())
                .map(|n| (n.time, n.sound)),
        )
        .filter(|(_, s)| *s != NO_SOUND)
        .collect();
    ev.sort_unstable();
    // when each sound is played next (it cuts itself there)
    let mut next_of: HashMap<u32, i64> = HashMap::new();
    let mut placed = Vec::with_capacity(ev.len());
    for &(time, sound) in ev.iter().rev() {
        let next = next_of.insert(sound, time);
        let Some(slice) = chart.sounds.get(sound as usize) else {
            continue;
        };
        let Some(s) = bank.get(slice.file) else {
            continue;
        };
        let from = frames(slice.start);
        let mut to = slice.end.map_or(s.frames, frames).min(s.frames);
        if let Some(n) = next {
            to = to.min(from + frames(n - time));
        }
        placed.push(Placed {
            sample: s,
            at: frames(time),
            from,
            to,
        });
    }
    measure_mix(&mut placed, rate, STEP)
}

/// The charts measured, by SHA-256, in `cache/loudness.txt` (a line per
/// chart: `sha256 lufs peak`, `-` for a silent chart; the last line of a
/// chart counts). Read in the background when the app starts.
pub struct LoudnessCache {
    map: HashMap<String, Loudness>,
    file: PathBuf,
    reading: Option<Job<String>>,
}

impl LoudnessCache {
    pub fn new(cache_dir: &std::path::Path) -> LoudnessCache {
        let file = cache_dir.join("loudness.txt");
        let at = file.clone();
        let reading = Some(Job::start(move |tx, _| {
            store::load(&at, move |r| {
                let _ = tx.send(r.unwrap_or_default());
            })
        }));
        LoudnessCache {
            map: HashMap::new(),
            file,
            reading,
        }
    }

    /// Take the file once it is read.
    pub fn poll(&mut self) {
        let Some(text) = self.reading.as_mut().and_then(Job::poll) else {
            return;
        };
        self.reading = None;
        // (charts measured meanwhile are newer)
        let mut read = parse(&text);
        read.extend(self.map.drain());
        self.map = read;
    }

    pub fn get(&self, sha256: &str) -> Option<Loudness> {
        self.map.get(sha256).copied()
    }

    /// Keep a chart's loudness (and write it down).
    pub fn put(&mut self, sha256: &str, l: Loudness) {
        if sha256.is_empty() || self.map.get(sha256) == Some(&l) {
            return;
        }
        self.map.insert(sha256.to_string(), l);
        if let Err(e) = store::append(&self.file, &line(sha256, &l)) {
            log::warn!("loudness cache: {e}");
        }
    }
}

fn line(sha256: &str, l: &Loudness) -> String {
    match l.lufs {
        Some(v) => format!("{sha256} {v:.2} {:.4}", l.peak),
        None => format!("{sha256} - {:.4}", l.peak),
    }
}

fn parse(text: &str) -> HashMap<String, Loudness> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (sha, lufs, peak) = (f.next()?, f.next()?, f.next()?);
            let lufs = match lufs {
                "-" => None,
                v => Some(v.parse::<f32>().ok().filter(|v| v.is_finite())?),
            };
            let peak = peak.parse::<f32>().ok().filter(|v| v.is_finite())?;
            Some((sha.to_string(), Loudness { lufs, peak }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use vibebms_core::chart::{BgmEvent, Note, NoteKind, SoundSlice};

    /// `secs` of a 1 kHz sine at `amp`, mono (whole periods in 0.5 s, so
    /// sounds started on the half second are in phase).
    fn tone(rate: u32, amp: f64, secs: f64) -> Option<Arc<Sample>> {
        let n = (rate as f64 * secs) as usize;
        let data = (0..n)
            .map(|i| {
                let v = amp * (std::f64::consts::TAU * 1000.0 * i as f64 / rate as f64).sin();
                (v * 32767.0).round() as i16
            })
            .collect();
        Some(Arc::new(Sample {
            channels: 1,
            frames: n,
            data,
        }))
    }

    fn note(time: i64, sound: u32) -> Note {
        Note {
            lane: 1,
            time,
            beat: 0.0,
            pos: 0.0,
            sound,
            kind: NoteKind::Normal,
        }
    }

    #[test]
    fn a_chart_is_measured_as_autoplayed() {
        let rate = 48_000;
        let sec = 1_000_000;
        let slice = |file| SoundSlice {
            file,
            start: 0,
            end: None,
        };
        let bank = SoundBank {
            samples: vec![tone(rate, 0.1, 10.0), tone(rate, 0.1, 1.0)],
        };
        // a 10 s BGM at -20 LUFS
        let bgm = Chart {
            sounds: vec![slice(0), slice(1)],
            bgm: vec![BgmEvent { time: 0, sound: 0 }],
            ..Default::default()
        };
        let l = measure_chart(&bgm, &bank, rate);
        assert!((l.lufs.unwrap() + 20.0).abs() < 0.2, "{l:?}");
        // a keysound every 0.5 s for 10 s: each cuts the one before, so the
        // tone is heard once at a time (-20 LUFS), not doubled
        let keys = Chart {
            sounds: vec![slice(0), slice(1)],
            notes: (0..20).map(|i| note(i * sec / 2, 1)).collect(),
            ..Default::default()
        };
        let l = measure_chart(&keys, &bank, rate);
        assert!((l.lufs.unwrap() + 20.0).abs() < 0.5, "{l:?}");
        // both together, in phase: 6 dB up
        let both = Chart {
            notes: keys.notes.clone(),
            ..bgm.clone()
        };
        let l = measure_chart(&both, &bank, rate);
        assert!((l.lufs.unwrap() + 14.0).abs() < 0.5, "{l:?}");
        assert!((l.peak - 0.2).abs() < 0.01);
        // nothing to hear
        assert_eq!(measure_chart(&Chart::default(), &bank, rate).lufs, None);
    }

    #[test]
    fn gains_cap_the_boost_at_the_peak() {
        let l = |lufs, peak| Loudness {
            lufs: Some(lufs),
            peak,
        };
        // a loud chart down to the target
        assert!((gain(&l(-6.0, 1.0), -12.0) - 0.5012).abs() < 1e-3);
        // a quiet one up, until its peak is at the ceiling
        assert!((gain(&l(-24.0, 0.25), -12.0) - 3.2).abs() < 1e-3);
        assert_eq!(gain(&l(-24.0, 0.9), -12.0), 1.0);
    }

    #[test]
    fn the_cache_file() {
        let a = Loudness {
            lufs: Some(-9.5),
            peak: 0.98,
        };
        let b = Loudness {
            lufs: None,
            peak: 0.0,
        };
        let text = format!(
            "{}\n{}\nbad line\n{}\n",
            line(
                "aa",
                &Loudness {
                    lufs: Some(-3.0),
                    peak: 1.0
                }
            ),
            line("bb", &b),
            line("aa", &a)
        );
        let m = parse(&text);
        assert_eq!(m.len(), 2);
        assert_eq!(m["aa"], a);
        assert_eq!(m["bb"], b);
    }
}
