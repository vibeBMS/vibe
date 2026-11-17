//! Hi-speed and the green number: how long notes are visible, kept with
//! HS-FIX through a chart's BPMs.

use super::{HsFix, PlayConfig};

/// A chart's BPMs, for hi-speed and green numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bpms {
    pub start: f64,
    pub min: f64,
    pub max: f64,
    pub main: f64,
}

impl Bpms {
    /// One BPM for all (a chart without changes, or a reference).
    pub fn flat(bpm: f64) -> Bpms {
        Bpms {
            start: bpm,
            min: bpm,
            max: bpm,
            main: bpm,
        }
    }

    pub fn of_meta(m: &vibebms_core::ChartMeta) -> Bpms {
        Bpms {
            start: m.initial_bpm,
            min: m.min_bpm,
            max: m.max_bpm,
            main: m.main_bpm,
        }
    }

    pub fn of_song(s: &crate::library::Song) -> Bpms {
        Bpms {
            start: if s.start_bpm > 0.0 {
                s.start_bpm
            } else {
                s.main_bpm
            },
            min: s.min_bpm,
            max: s.max_bpm,
            main: s.main_bpm,
        }
    }
}

fn valid(bpm: f64) -> f64 {
    if bpm > 0.0 && bpm.is_finite() {
        bpm
    } else {
        130.0
    }
}

/// Hi-speed limits and steps (as in LR2 / beatoraja).
pub const HISPEED_MIN: f32 = 0.25;
pub const HISPEED_MAX: f32 = 20.0;
pub const HISPEED_STEP: f32 = 0.25;

impl PlayConfig {
    /// Share of the lane notes are visible in (below the lane cover, above
    /// the LIFT).
    pub fn visible(&self, with_cover: bool) -> f32 {
        let cover = if with_cover && (self.lane_cover || self.sudden > 0.0) {
            self.sudden
        } else {
            0.0
        };
        (1.0 - cover - self.lift).max(0.05)
    }

    /// The BPM the green number is shown (and with HS-FIX kept) at.
    pub fn basis(&self, b: &Bpms) -> f64 {
        valid(match self.hs_fix {
            HsFix::Off | HsFix::Main => b.main,
            HsFix::Start => b.start,
            HsFix::Max => b.max,
            HsFix::Min => b.min,
        })
    }

    /// The hi-speed a chart plays at: the setting, or with HS-FIX what
    /// keeps the green number.
    pub fn hispeed_at(&self, b: &Bpms) -> f32 {
        if self.hs_fix == HsFix::Off {
            return self.hispeed;
        }
        (240_000.0 * self.visible(true) as f64
            / (self.basis(b) * self.fixed_duration_ms.max(1.0) as f64)) as f32
    }

    /// How long (ms) a note is visible at `bpm` with the chart's hi-speed:
    /// the lane holds 4 / hi-speed beats.
    pub fn duration_at(&self, b: &Bpms, bpm: f64, with_cover: bool) -> f64 {
        240_000.0 * self.visible(with_cover) as f64
            / (valid(bpm) * self.hispeed_at(b).max(0.01) as f64)
    }

    /// The duration at the basis BPM (the one the green number is about).
    pub fn duration_ms(&self, b: &Bpms) -> f64 {
        self.duration_at(b, self.basis(b), true)
    }

    pub fn duration_ms_with(&self, b: &Bpms, with_cover: bool) -> f64 {
        self.duration_at(b, self.basis(b), with_cover)
    }

    /// The green number (beatoraja: the duration x 0.6).
    pub fn green(&self, b: &Bpms) -> i64 {
        (self.duration_ms(b) * 0.6).round() as i64
    }

    /// Change the hi-speed by `d` for a chart; with HS-FIX this sets the new
    /// green number.
    pub fn add_hispeed(&mut self, d: f32, b: &Bpms) {
        let hs =
            (((self.hispeed_at(b) + d) * 100.0).round() / 100.0).clamp(HISPEED_MIN, HISPEED_MAX);
        self.hispeed = hs;
        if self.hs_fix != HsFix::Off {
            self.fixed_duration_ms =
                (240_000.0 * self.visible(true) as f64 / (self.basis(b) * hs as f64)) as f32;
        }
    }

    /// Change the green number (the duration at the basis BPM) by `d` ms:
    /// beatoraja's duration keys. With HS-FIX this is the kept duration,
    /// otherwise the hi-speed that gives it.
    pub fn add_duration(&mut self, d: f64, b: &Bpms) {
        let vis = self.visible(true) as f64;
        let min = 240_000.0 * vis / (self.basis(b) * HISPEED_MAX as f64);
        let max = 240_000.0 * vis / (self.basis(b) * HISPEED_MIN as f64);
        let dur = (self.duration_ms(b) + d).clamp(min, max).max(1.0);
        if self.hs_fix != HsFix::Off {
            self.fixed_duration_ms = dur as f32;
        }
        self.hispeed = (240_000.0 * vis / (self.basis(b) * dur)) as f32;
    }

    /// Change HS-FIX, keeping the speed the chart `b` plays at.
    pub fn set_hs_fix(&mut self, fix: HsFix, b: &Bpms) {
        let hs = self.hispeed_at(b);
        if fix == HsFix::Off {
            if self.hs_fix != HsFix::Off {
                self.hispeed = (hs / HISPEED_STEP).round().max(1.0) * HISPEED_STEP;
            }
            self.hs_fix = fix;
        } else {
            self.hs_fix = fix;
            self.fixed_duration_ms =
                (240_000.0 * self.visible(true) as f64 / (self.basis(b) * hs as f64)) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_green_number() {
        let mut p = PlayConfig {
            hispeed: 2.0,
            ..Default::default()
        };
        let at150 = Bpms::flat(150.0);
        // 240000 / (150 x 2) = 800 ms at 150 BPM
        p.set_hs_fix(HsFix::Main, &at150);
        assert!((p.fixed_duration_ms - 800.0).abs() < 0.5);
        // a 300 BPM chart plays at hi-speed 1.0, same green number
        let at300 = Bpms::flat(300.0);
        assert!((p.hispeed_at(&at300) - 1.0).abs() < 1e-4);
        assert_eq!(p.green(&at300), p.green(&at150));
        // SUDDEN+ keeps the green number: the hi-speed drops
        p.lane_cover = true;
        p.sudden = 0.5;
        assert_eq!(p.green(&at150), 480);
        assert!((p.hispeed_at(&at150) - 1.0).abs() < 1e-4);
        // hi-speed +0.25 at 150 BPM sets a new green number
        p.add_hispeed(0.25, &at150);
        assert!((p.hispeed_at(&at150) - 1.25).abs() < 1e-3);
        assert!((p.duration_ms(&at150) - 640.0).abs() < 0.5);
        // MAX BPM: a 100..200 chart keeps the green number at 200
        let b = Bpms {
            start: 100.0,
            min: 100.0,
            max: 200.0,
            main: 100.0,
        };
        p.hs_fix = HsFix::Max;
        assert!((p.hispeed_at(&b) - 1.25 * 150.0 / 200.0).abs() < 1e-3);
        // back to OFF keeps the speed, on the 0.25 grid
        p.set_hs_fix(HsFix::Off, &at150);
        assert_eq!(p.hispeed, 1.25);
    }
}
