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

#[derive(Default)]
pub(super) struct Audio {
    bank: SoundBank,
    /// The next BGM event and auto lane keysound to schedule.
    bgm_i: usize,
    auto_i: usize,
    /// Keysounds of auto lanes: (time, sound, side).
    auto_notes: Vec<(i64, SoundId, usize)>,
}

impl Audio {
    pub fn new(bank: SoundBank, auto_notes: Vec<(i64, SoundId, usize)>) -> Audio {
        Audio {
            bank,
            bgm_i: 0,
            auto_i: 0,
            auto_notes,
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
