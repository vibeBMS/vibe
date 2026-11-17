//! The play's sounds: BGM and auto lanes' keysounds scheduled ahead, the
//! players' keysounds as they press.

use super::session::{Out, Session};
use vibebms_audio::{Bus, Mixer, PlayOptions, SoundBank};
use vibebms_core::chart::SoundId;

/// A sound's mixer voice: VS keeps each side's keysounds apart (a side's
/// keysound, or its HCN muting, never cuts the other's).
pub(super) fn voice(sound: SoundId, side: usize, vs: bool) -> u64 {
    sound as u64 + 1 + if vs { (side as u64) << 32 } else { 0 }
}

/// The mixer group of a NONSTOP course's stage `k`.
fn group(k: usize) -> u8 {
    (k as u8 + 1).min(vibebms_audio::GROUPS)
}

#[derive(Default)]
pub(super) struct Audio {
    bank: SoundBank,
    /// The next BGM event and auto lane keysound to schedule.
    bgm_i: usize,
    auto_i: usize,
    /// Keysounds of auto lanes: (time, sound, side).
    auto_notes: Vec<(i64, SoundId, usize)>,
    /// A NONSTOP course's stages' volumes as last set (each stage a
    /// mixer group).
    stage_volume: Vec<f32>,
}

impl Audio {
    pub fn new(bank: SoundBank, auto_notes: Vec<(i64, SoundId, usize)>) -> Audio {
        Audio {
            bank,
            bgm_i: 0,
            auto_i: 0,
            auto_notes,
            stage_volume: Vec::new(),
        }
    }

    /// A NONSTOP course's stages faded in and out at chart time `t`
    /// (S-OLR2b 3.1): each stage's sounds are a mixer group.
    pub fn fade(&mut self, m: &Mixer, s: &Session, t: i64) {
        let Some(ns) = &s.nonstop else {
            return;
        };
        self.stage_volume.resize(ns.stages.len(), -1.0);
        for (k, v) in self.stage_volume.iter_mut().enumerate() {
            let now = ns.volume(k, t);
            if (now - *v).abs() > 0.002 || (now != *v && (now == 0.0 || now == 1.0)) {
                *v = now;
                m.set_group_volume(group(k), now);
            }
        }
    }

    /// `sound` of `side` at chart time `at` (`None`: now).
    fn play(&self, m: &Mixer, s: &Session, sound: SoundId, at: Option<i64>, bus: Bus, side: usize) {
        let Some(slice) = s.chart.sounds.get(sound as usize) else {
            return;
        };
        let Some(sample) = self.bank.get(slice.file) else {
            return;
        };
        m.play(
            sample,
            PlayOptions {
                key: voice(sound, side, s.vs),
                volume: 1.0,
                bus,
                at: at.map(|t| s.instant_at(t)),
                offset: m.us_to_frames(slice.start).max(0) as usize,
                end: slice.end.map(|e| m.us_to_frames(e).max(0) as usize),
                looping: false,
                group: s
                    .nonstop
                    .as_ref()
                    .map_or(0, |n| group(n.stage_of_sound(sound))),
            },
        );
    }

    /// The BGM and auto lanes' keysounds of the next 200 ms after chart
    /// time `t`, to the mixer for exact timing (those over 100 ms late are
    /// left out).
    pub fn schedule(&mut self, m: &Mixer, s: &Session, t: i64) {
        let horizon = t + 200_000;
        while let Some(b) = s.chart.bgm.get(self.bgm_i) {
            if b.time > horizon {
                break;
            }
            if b.time >= t - 100_000 {
                self.play(m, s, b.sound, Some(b.time), Bus::Bgm, 0);
            }
            self.bgm_i += 1;
        }
        while let Some(&(time, sound, side)) = self.auto_notes.get(self.auto_i) {
            if time > horizon {
                break;
            }
            if time >= t - 100_000 {
                self.play(m, s, sound, Some(time), Bus::Key, side);
            }
            self.auto_i += 1;
        }
    }

    /// A step's sound.
    pub fn out(&self, m: &Mixer, s: &Session, o: Out) {
        match o {
            Out::Key { side, sound } => self.play(m, s, sound, None, Bus::Key, side),
            Out::Volume {
                side,
                sound,
                volume,
            } => m.set_volume(voice(sound, side, s.vs), volume),
            Out::Adjust(_) => {}
        }
    }
}
