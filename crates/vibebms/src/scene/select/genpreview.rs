//! Previews made from a chart with no preview of its own (no `#PREVIEW`, no
//! `preview*` sound): its BGM and keysounds mixed over a short stretch from
//! its first dense part, off the main thread natively, kept in a small cache.
//!
//! The chart file is read first; the stretch is chosen from its notes; only
//! the sound files that play in it are read and decoded (on the web each
//! read waits for the browser's storage). Shorter on the web and mobile,
//! where memory is scarcer, and fewer kept.

use crate::platform::vfs::{self, Prefetch};
use crate::platform::Job;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use vibebms_audio::{Sample, SoundBank};
use vibebms_core::chart::{Chart, NO_SOUND};

const SMALL: bool = cfg!(any(
    target_arch = "wasm32",
    target_os = "android",
    target_os = "ios"
));
/// How long a made preview lasts (µs), and how many are kept.
pub(super) const LEN_US: i64 = if SMALL { 10_000_000 } else { 15_000_000 };
const KEPT: usize = if SMALL { 3 } else { 8 };
/// A sound started this long before the stretch may still be heard in it.
const LOOK_BACK_US: i64 = 30_000_000;
const FADE_IN_US: i64 = 300_000;
const FADE_OUT_US: i64 = 2_000_000;

/// What a preview is made of: the chart, where the stretch starts, and
/// the files of the sounds that play in it (by sound file index).
pub(super) struct Plan {
    chart: Chart,
    start: i64,
    files: Vec<Option<PathBuf>>,
}

enum Stage {
    /// The chart file being read (the web).
    Chart(Prefetch),
    Plan(Job<Option<Plan>>),
    /// The sound files being read (the web).
    Files(Prefetch, Box<Plan>),
    Mix(Job<Option<Preview>>),
}

/// A preview's sound and its loudness.
pub(super) type Preview = (Arc<Sample>, crate::loudness::Loudness);

/// The previews made, newest last, and the one being made.
#[derive(Default)]
pub(super) struct Generated {
    cache: VecDeque<(String, Preview)>,
    making: Option<(String, Stage)>,
}

impl Generated {
    /// The preview made for the chart at `path`, if it is kept.
    pub fn cached(&mut self, path: &str) -> Option<Preview> {
        let i = self.cache.iter().position(|(p, _)| p == path)?;
        let hit = self.cache.remove(i)?;
        let s = hit.1.clone();
        self.cache.push_back(hit);
        Some(s)
    }

    /// Whether a preview is being made (one at a time).
    pub fn busy(&self) -> bool {
        self.making.is_some()
    }

    /// Make the preview of the chart at `path`.
    pub fn start(&mut self, path: &str) {
        let stage = Stage::Chart(Prefetch::files(vec![PathBuf::from(path)]));
        self.making = Some((path.to_string(), stage));
    }

    /// Take the next step; the chart and its preview once made (`None`
    /// for the sample when it could not be).
    pub fn poll(&mut self, rate: u32) -> Option<(String, Option<Preview>)> {
        let (path, stage) = self.making.take()?;
        let next = match stage {
            Stage::Chart(f) if f.finished() => {
                let p = PathBuf::from(&path);
                Stage::Plan(Job::spawn("preview plan", move |_| plan(&p)))
            }
            Stage::Plan(mut job) => match job.poll() {
                Some(Some(plan)) => {
                    let want = plan.files.iter().flatten().cloned().collect();
                    Stage::Files(Prefetch::files(want), Box::new(plan))
                }
                Some(None) => return Some((path, None)),
                None => Stage::Plan(job),
            },
            Stage::Files(f, plan) if f.finished() => {
                Stage::Mix(Job::spawn("preview mix", move |_| {
                    let s = mix_plan(&plan, rate);
                    let l = crate::loudness::measure_sound(&s, rate);
                    Some((Arc::new(s), l))
                }))
            }
            Stage::Mix(mut job) => match job.poll() {
                Some(s) => {
                    if let Some(s) = &s {
                        if self.cache.len() >= KEPT {
                            self.cache.pop_front();
                        }
                        self.cache.push_back((path.clone(), s.clone()));
                    }
                    return Some((path, s));
                }
                None => Stage::Mix(job),
            },
            waiting => waiting,
        };
        self.making = Some((path, next));
        None
    }
}

/// The chart at `path` read, its stretch chosen, the files it needs found.
fn plan(path: &Path) -> Option<Plan> {
    // a song in an archive: unpacked (its sounds are read one by one; kept
    // for a play of it while it is chosen)
    if crate::platform::arcfs::split(path).is_some() {
        if let Err(e) = vfs::prepare_songs(&[path.to_path_buf()]) {
            log::warn!("preview: {e}");
            return None;
        }
    }
    let bytes = vfs::read(path).ok()?;
    let chart = vibebms_core::load_chart_bytes(path, &bytes, &Default::default()).ok()?;
    let start = start_of(&chart, LEN_US);
    let index = vfs::DirIndex::new(path.parent()?);
    let mut files: Vec<Option<PathBuf>> = vec![None; chart.sound_files.len()];
    for (_, sound) in events(&chart, start, LEN_US) {
        let Some(slice) = chart.sounds.get(sound as usize) else {
            continue;
        };
        let i = slice.file as usize;
        if let (Some(slot @ None), Some(name)) = (files.get_mut(i), chart.sound_files.get(i)) {
            *slot = index.find(name, vfs::AUDIO_EXTS);
        }
    }
    Some(Plan {
        chart,
        start,
        files,
    })
}

/// The plan's sounds decoded and mixed.
fn mix_plan(p: &Plan, rate: u32) -> Sample {
    let names: Vec<String> = (p.files.iter())
        .map(|f| {
            f.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        })
        .collect();
    let bank = SoundBank::load_with(
        &names,
        rate,
        &AtomicUsize::new(0),
        &|n| (!n.is_empty()).then(|| PathBuf::from(n)),
        &|p| vfs::read_shared(p).ok(),
    );
    mix(&p.chart, &bank, p.start, LEN_US, rate)
}

/// Where the stretch of `len` µs starts: the first one, from a fifth of
/// the chart on, with at least 85 % of the notes of the densest (a chart
/// without notes: a third in); on the bar line at or before.
pub(super) fn start_of(chart: &Chart, len: i64) -> i64 {
    let times: Vec<i64> = (chart.notes.iter())
        .filter(|n| n.is_playable())
        .map(|n| n.time)
        .collect();
    let end = times.last().copied().unwrap_or(0);
    let count = |t: i64| {
        let a = times.partition_point(|x| *x < t);
        times.partition_point(|x| *x < t + len) - a
    };
    let starts: Vec<i64> = (end / 5..=(end - len).max(end / 5))
        .step_by(500_000)
        .collect();
    let densest = starts.iter().map(|t| count(*t)).max().unwrap_or(0);
    let t = (starts.iter())
        .find(|t| densest > 0 && count(**t) * 100 >= densest * 85)
        .copied()
        .unwrap_or(end / 3);
    (chart.bar_lines.iter())
        .map(|b| b.time)
        .filter(|b| *b <= t)
        .max()
        .unwrap_or(t)
        .max(0)
}

/// The sounds (time, sound) that may be heard from `start` for `len`: the
/// BGM and the notes' keysounds, a while before it included.
fn events(chart: &Chart, start: i64, len: i64) -> Vec<(i64, u32)> {
    let within = |t: i64| t >= start - LOOK_BACK_US && t < start + len;
    let mut v: Vec<(i64, u32)> = (chart.bgm.iter())
        .map(|b| (b.time, b.sound))
        .chain(
            (chart.notes.iter())
                .filter(|n| n.is_playable())
                .map(|n| (n.time, n.sound)),
        )
        .filter(|(t, s)| within(*t) && *s != NO_SOUND)
        .collect();
    v.sort_unstable();
    v
}

/// The stretch from `start`, `len` µs, as the chart sounds autoplayed:
/// a sound played again cuts itself off (as in play); faded in and out,
/// never louder than full scale. Stereo at `rate`.
pub(super) fn mix(chart: &Chart, bank: &SoundBank, start: i64, len: i64, rate: u32) -> Sample {
    let frames = |us: i64| (us as i128 * rate as i128 / 1_000_000) as i64;
    let n = frames(len).max(0) as usize;
    let mut out = vec![0f32; n * 2];
    let ev = events(chart, start, len);
    // when each sound is played next (from the last event back)
    let mut next_of: std::collections::HashMap<u32, i64> = Default::default();
    let mut nexts: Vec<Option<i64>> = (ev.iter().rev())
        .map(|&(time, sound)| next_of.insert(sound, time))
        .collect();
    nexts.reverse();
    for (&(time, sound), next) in ev.iter().zip(nexts) {
        let Some(slice) = chart.sounds.get(sound as usize) else {
            continue;
        };
        let Some(s) = bank.get(slice.file) else {
            continue;
        };
        let from = frames(slice.start).clamp(0, s.frames as i64);
        let mut to = slice
            .end
            .map_or(s.frames as i64, frames)
            .min(s.frames as i64);
        if let Some(next) = next {
            to = to.min(from + frames(next - time));
        }
        let at = frames(time - start);
        let src = from + (-at).max(0);
        let dst = at.max(0);
        let count = (to - src).min(n as i64 - dst);
        if count <= 0 {
            continue;
        }
        let (dst, src, count) = (dst as usize, src as usize, count as usize);
        s.mix_into(src, &mut out[dst * 2..(dst + count) * 2], 1.0);
    }
    let (fade_in, fade_out) = (frames(FADE_IN_US).max(1), frames(FADE_OUT_US).max(1));
    let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
    let gain = if peak > 0.9 { 0.9 / peak } else { 1.0 };
    let data = (out.chunks(2).enumerate())
        .flat_map(|(i, f)| {
            let i = i as i64;
            let g = gain
                * (i as f32 / fade_in as f32).min(1.0)
                * ((n as i64 - i) as f32 / fade_out as f32).min(1.0);
            f.iter().map(move |v| (v * g * 32767.0).round() as i16)
        })
        .collect();
    Sample {
        channels: 2,
        frames: n,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibebms_core::chart::{BarLine, BgmEvent, Note, NoteKind, SoundSlice};

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

    /// A chart in a folder: read, its stretch's sounds decoded and mixed
    /// (only the file that plays in it is decoded), then kept.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn made_from_a_chart_file() {
        let dir = std::env::temp_dir().join(format!("vibebms-genprev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 1 s of a 16-bit mono tone at 8 kHz
        let frames = 8000u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + frames * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(frames * 2).to_le_bytes());
        for i in 0..frames {
            let v = if i % 20 < 10 { 8000i16 } else { -8000 };
            wav.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(dir.join("a.wav"), &wav).unwrap();
        // 120 BPM: a measure is 2 s; a note on every beat for 20 measures
        let mut bms = String::from("#BPM 120\n#WAV01 a.wav\n#WAV02 missing.wav\n");
        for m in 1..=20 {
            bms += &format!("#{m:03}11:01010101\n");
        }
        bms += "#00101:02\n";
        std::fs::write(dir.join("c.bms"), bms).unwrap();
        let path = dir.join("c.bms").display().to_string();
        let mut g = Generated::default();
        g.start(&path);
        let mut made = None;
        for _ in 0..2000 {
            if let Some(m) = g.poll(8000) {
                made = Some(m);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let (p, s) = made.expect("made");
        let s = s.expect("a preview");
        let (s, l) = (s.0, s.1);
        assert_eq!((p.as_str(), s.channels), (path.as_str(), 2));
        assert_eq!(s.frames as i64, LEN_US * 8000 / 1_000_000);
        assert!(s.data.iter().any(|v| *v != 0));
        // measured as it was made
        assert!(l.lufs.is_some() && l.peak > 0.0);
        assert!(!g.busy() && g.cached(&path).is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The densest stretch from a fifth on, on its bar line; a short chart
    /// a third in.
    #[test]
    fn starts_at_the_first_dense_part() {
        let sec = 1_000_000;
        // sparse 0..40 s, dense 40..60 s, sparse to 100 s
        let c = Chart {
            notes: (0..100)
                .flat_map(|s| {
                    let k = if (40..60).contains(&s) { 8 } else { 1 };
                    (0..k).map(move |i| note(s * sec + i * sec / 8, 0))
                })
                .collect(),
            bar_lines: (0..50)
                .map(|b| BarLine {
                    time: b * 2 * sec + sec / 2,
                    pos: 0.0,
                })
                .collect(),
            ..Default::default()
        };
        // the first stretch with 68 of the densest's 80 notes starts at
        // 38.3 s (from 19.8 s, every 0.5 s): its bar line is at 36.5 s
        let t = start_of(&c, 10 * sec);
        assert_eq!(t, 36 * sec + sec / 2, "{t}");
        // a chart shorter than the stretch: from a fifth in
        let short = Chart {
            notes: vec![note(0, 0), note(9 * sec, 0)],
            ..Default::default()
        };
        assert_eq!(start_of(&short, 10 * sec), 1_800_000);
        // no notes: 0
        assert_eq!(start_of(&Chart::default(), 10 * sec), 0);
    }

    /// Many sounds in the stretch, none played twice: mixed without a
    /// search through the rest for each one's next time.
    #[test]
    fn mixes_many_sounds() {
        let n = 200_000;
        let bank = SoundBank {
            samples: vec![Some(Arc::new(Sample {
                channels: 1,
                frames: 1,
                data: vec![1000],
            }))],
        };
        let c = Chart {
            sounds: (0..n)
                .map(|_| SoundSlice {
                    file: 0,
                    start: 0,
                    end: None,
                })
                .collect(),
            notes: (0..n)
                .map(|i| note(1_000_000 + (i / 2) as i64, i))
                .collect(),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let s = mix(&c, &bank, 0, 2_000_000, 1000);
        assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
        // a thousand in each frame from 1 s: 0.9 of full scale, half way
        // through the fade out
        assert_eq!(s.data[1000 * 2], (0.9f32 * 0.5 * 32767.0).round() as i16);
    }

    /// Sounds from before the stretch are heard from where they are; a
    /// sound played again cuts itself; the end fades out.
    #[test]
    fn mixes_the_stretch() {
        let rate = 1000;
        let ms = 1000;
        let tone = |v: i16, frames: usize| {
            Some(Arc::new(Sample {
                channels: 1,
                frames,
                data: vec![v; frames],
            }))
        };
        let bank = SoundBank {
            samples: vec![tone(1000, 5000), tone(2000, 100)],
        };
        let slice = |file| SoundSlice {
            file,
            start: 0,
            end: None,
        };
        // a long BGM from 1 s before; a keysound at 100 ms and again 20 ms
        // later (the first cut there)
        let c = Chart {
            sounds: vec![slice(0), slice(1)],
            bgm: vec![BgmEvent {
                time: 9_000 * ms,
                sound: 0,
            }],
            notes: vec![note(10_100 * ms, 1), note(10_120 * ms, 1)],
            ..Default::default()
        };
        let s = mix(&c, &bank, 10_000 * ms, 10_000 * ms, rate);
        assert_eq!((s.channels, s.frames), (2, 10_000));
        let at = |f: usize| s.data[f * 2];
        // fading in over 300 frames
        assert!(at(0) == 0 && at(50) > 0 && at(50) < at(400));
        // the BGM alone; with the keysound; its second time
        assert_eq!(at(500), 1000);
        assert_eq!(at(110), (3000.0 * 110.0 / 300.0f32).round() as i16);
        assert_eq!(at(1000), 1000);
        assert_eq!(at(3999), 1000);
        // the BGM ends 4 s in
        assert_eq!(at(4000), 0);
        let mut c2 = c.clone();
        c2.bgm.clear();
        let k = mix(&c2, &bank, 10_000 * ms, 10_000 * ms, rate);
        let at = |f: usize| k.data[f * 2];
        // one keysound at a time: 2000 x 150 / 300, not twice that
        assert_eq!((at(99), at(150), at(220)), (0, 1000, 0));
        assert!(at(219) > 0);
    }
}
