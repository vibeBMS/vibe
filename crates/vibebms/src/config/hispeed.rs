//! Hi-speed and the green number: how long notes are visible, kept with
//! HS-FIX through a chart's BPMs; the hi-speed types (`HsType`).

use super::{HsFix, HsType, PlayConfig, NORMAL_GREENS};

/// A chart's BPMs, for hi-speed and green numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bpms {
    pub start: f64,
    pub min: f64,
    pub max: f64,
    pub main: f64,
    /// The BPM at the moment, while the chart plays: FLOATING keeps the
    /// green number at it.
    pub now: Option<f64>,
}

impl Bpms {
    /// One BPM for all (a chart without changes, or a reference).
    pub fn flat(bpm: f64) -> Bpms {
        Bpms {
            start: bpm,
            min: bpm,
            max: bpm,
            main: bpm,
            now: None,
        }
    }

    /// The same at a moment of the play, at `bpm`.
    pub fn at(self, bpm: f64) -> Bpms {
        Bpms {
            now: Some(bpm),
            ..self
        }
    }

    pub fn of_meta(m: &vibebms_core::ChartMeta) -> Bpms {
        Bpms {
            start: m.initial_bpm,
            min: m.min_bpm,
            max: m.max_bpm,
            main: m.main_bpm,
            now: None,
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
            now: None,
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

/// The hi-speed that shows notes for `ms` at `bpm` over the share `vis` of
/// the lane.
fn hs_for(ms: f64, vis: f64, bpm: f64) -> f32 {
    (240_000.0 * vis / (valid(bpm) * ms.max(1.0))) as f32
}

/// The BPM LR2's HS-FIX hi-speed is a multiplier at.
const LR2_FIX_BPM: f64 = 150.0;

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

    /// The BPM the green number is kept at: FLOATING's while the chart
    /// plays is the BPM of the moment.
    fn kept_at(&self, b: &Bpms) -> f64 {
        match (self.hs_type, b.now) {
            (HsType::Floating, Some(now)) => valid(now),
            _ => self.basis(b),
        }
    }

    /// The duration (ms) of NORMAL's level over the whole lane.
    fn level_ms(&self) -> f64 {
        let i = (self.hs_level.clamp(1, 20) - 1) as usize;
        NORMAL_GREENS[i] as f64 / 0.6
    }

    /// The hi-speed a chart plays at: the setting, or what keeps the green
    /// number (HS-FIX, NORMAL's level, FLOATING), or LR2's HS-FIX.
    pub fn hispeed_at(&self, b: &Bpms) -> f32 {
        let vis = self.visible(true) as f64;
        match self.hs_type {
            HsType::Normal => hs_for(self.level_ms(), 1.0, self.basis(b)),
            HsType::Floating => hs_for(self.fixed_duration_ms as f64, vis, self.kept_at(b)),
            _ if self.hs_fix == HsFix::Off => self.hispeed,
            HsType::Lr2 => (self.hispeed as f64 * LR2_FIX_BPM / self.basis(b)) as f32,
            HsType::Classic => hs_for(self.fixed_duration_ms as f64, vis, self.basis(b)),
        }
    }

    /// The hi-speed as the player sets and sees it: LR2's the setting, the
    /// others what the chart plays at.
    pub fn hispeed_shown(&self, b: &Bpms) -> f32 {
        match self.hs_type {
            HsType::Lr2 => self.hispeed,
            _ => self.hispeed_at(b),
        }
    }

    /// How long (ms) a note is visible at `bpm` at hi-speed `hs`.
    pub fn duration_with(&self, hs: f32, bpm: f64, with_cover: bool) -> f64 {
        240_000.0 * self.visible(with_cover) as f64 / (valid(bpm) * hs.max(0.01) as f64)
    }

    /// How long (ms) a note is visible at `bpm` with the chart's hi-speed:
    /// the lane holds 4 / hi-speed beats.
    pub fn duration_at(&self, b: &Bpms, bpm: f64, with_cover: bool) -> f64 {
        self.duration_with(self.hispeed_at(b), bpm, with_cover)
    }

    /// The duration at the basis BPM (the one the green number is about;
    /// FLOATING's while playing: the BPM of the moment).
    pub fn duration_ms(&self, b: &Bpms) -> f64 {
        self.duration_at(b, self.kept_at(b), true)
    }

    pub fn duration_ms_with(&self, b: &Bpms, with_cover: bool) -> f64 {
        self.duration_at(b, self.kept_at(b), with_cover)
    }

    /// The green number (beatoraja: the duration x 0.6).
    pub fn green(&self, b: &Bpms) -> i64 {
        (self.duration_ms(b) * 0.6).round() as i64
    }

    /// Keep hi-speed `hs` for the chart `b` as the setting of the type.
    fn set_hispeed(&mut self, hs: f32, b: &Bpms) {
        let hs = ((hs * 100.0).round() / 100.0).clamp(HISPEED_MIN, HISPEED_MAX);
        let vis = self.visible(true) as f64;
        match self.hs_type {
            HsType::Normal => {
                // the nearest level, the faster on a tie
                let gn = self.duration_with(hs, self.basis(b), false) * 0.6;
                let i = (0..NORMAL_GREENS.len())
                    .rev()
                    .min_by(|a, c| {
                        let d = |i: &usize| (NORMAL_GREENS[*i] as f64 - gn).abs();
                        d(a).total_cmp(&d(c))
                    })
                    .unwrap_or(9);
                self.hs_level = i as u8 + 1;
            }
            HsType::Floating => {
                self.fixed_duration_ms = (240_000.0 * vis / (self.kept_at(b) * hs as f64)) as f32
            }
            HsType::Lr2 if self.hs_fix != HsFix::Off => {
                self.hispeed =
                    (((hs as f64 * self.basis(b) / LR2_FIX_BPM) * 100.0).round() / 100.0) as f32
            }
            HsType::Lr2 => self.hispeed = hs,
            HsType::Classic => {
                self.hispeed = hs;
                if self.hs_fix != HsFix::Off {
                    self.fixed_duration_ms = (240_000.0 * vis / (self.basis(b) * hs as f64)) as f32;
                }
            }
        }
    }

    /// Change the hi-speed by `d` for a chart (NORMAL: a level per step,
    /// faster for `d` > 0); with HS-FIX or FLOATING this sets the new green
    /// number, LR2's HS-FIX steps the setting.
    pub fn add_hispeed(&mut self, d: f32, b: &Bpms) {
        match self.hs_type {
            HsType::Normal => {
                let l = self.hs_level.clamp(1, 20) as i32 + d.signum() as i32;
                self.hs_level = l.clamp(1, 20) as u8;
            }
            HsType::Lr2 => {
                self.hispeed =
                    (((self.hispeed + d) * 100.0).round() / 100.0).clamp(HISPEED_MIN, HISPEED_MAX)
            }
            _ => self.set_hispeed(self.hispeed_at(b) + d, b),
        }
    }

    /// Change the green number (the duration at the basis BPM) by `d` ms:
    /// beatoraja's duration keys (NORMAL: a level the other way).
    pub fn add_duration(&mut self, d: f64, b: &Bpms) {
        if self.hs_type == HsType::Normal {
            return self.add_hispeed(-d.signum() as f32, b);
        }
        let vis = self.visible(true) as f64;
        let at = self.kept_at(b);
        let min = 240_000.0 * vis / (at * HISPEED_MAX as f64);
        let max = 240_000.0 * vis / (at * HISPEED_MIN as f64);
        let dur = (self.duration_ms(b) + d).clamp(min, max).max(1.0);
        let hs = hs_for(dur, vis, at);
        let fix = self.hs_fix != HsFix::Off;
        match self.hs_type {
            HsType::Floating => self.fixed_duration_ms = dur as f32,
            HsType::Lr2 if fix => self.hispeed = (hs as f64 * at / LR2_FIX_BPM) as f32,
            _ => {
                self.hispeed = hs;
                if fix {
                    self.fixed_duration_ms = dur as f32;
                }
            }
        }
    }

    /// Change HS-FIX, keeping the speed the chart `b` plays at (LR2: the
    /// setting; NORMAL and FLOATING keep their level or green number).
    pub fn set_hs_fix(&mut self, fix: HsFix, b: &Bpms) {
        let hs = self.hispeed_at(b);
        let was = self.hs_fix;
        self.hs_fix = fix;
        if self.hs_type != HsType::Classic {
            return;
        }
        if fix == HsFix::Off {
            if was != HsFix::Off {
                self.hispeed = (hs / HISPEED_STEP).round().max(1.0) * HISPEED_STEP;
            }
        } else {
            self.fixed_duration_ms =
                (240_000.0 * self.visible(true) as f64 / (self.basis(b) * hs as f64)) as f32;
        }
    }

    /// Change the hi-speed type, keeping the speed the chart `b` plays at
    /// (LR2's HS-FIX: as near as its setting has it).
    pub fn set_hs_type(&mut self, t: HsType, b: &Bpms) {
        let hs = self.hispeed_at(b);
        self.hs_type = t;
        self.set_hispeed(hs, b);
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
            now: None,
        };
        p.hs_fix = HsFix::Max;
        assert!((p.hispeed_at(&b) - 1.25 * 150.0 / 200.0).abs() < 1e-3);
        // back to OFF keeps the speed, on the 0.25 grid
        p.set_hs_fix(HsFix::Off, &at150);
        assert_eq!(p.hispeed, 1.25);
    }

    /// NORMAL: levels of fixed green numbers over the whole lane, at the
    /// HS-FIX BPM; the cover makes the green number shown smaller.
    #[test]
    fn normal_levels() {
        let mut p = PlayConfig {
            hs_type: HsType::Normal,
            hs_level: 18,
            ..Default::default()
        };
        let at150 = Bpms::flat(150.0);
        assert_eq!(p.green(&at150), 300);
        assert_eq!(p.green(&Bpms::flat(300.0)), 300);
        p.add_hispeed(0.25, &at150);
        assert_eq!((p.hs_level, p.green(&at150)), (19, 280));
        p.add_hispeed(10.0, &at150);
        p.add_hispeed(0.01, &at150);
        assert_eq!(p.hs_level, 20);
        p.add_duration(1.0, &at150);
        assert_eq!(p.hs_level, 19);
        p.lane_cover = true;
        p.sudden = 0.5;
        assert_eq!(p.green(&at150), 140);
        // from a hi-speed: the nearest level (GN 560 lies between 550
        // and 600: 550)
        let mut c = PlayConfig {
            hispeed: (240_000.0 / (150.0 * 560.0 / 0.6)) as f32,
            ..Default::default()
        };
        c.set_hs_type(HsType::Normal, &at150);
        assert_eq!(c.hs_level, 7);
    }

    /// FLOATING keeps the green number through the cover and, while the
    /// chart plays, through its BPM changes.
    #[test]
    fn floating_keeps_the_green_number() {
        let mut p = PlayConfig {
            hispeed: 2.0,
            ..Default::default()
        };
        let b = Bpms {
            start: 150.0,
            min: 75.0,
            max: 300.0,
            main: 150.0,
            now: None,
        };
        p.set_hs_type(HsType::Floating, &b);
        assert_eq!(p.green(&b), 480);
        assert_eq!(p.green(&b.at(300.0)), 480);
        assert!((p.hispeed_at(&b.at(300.0)) - 1.0).abs() < 1e-4);
        assert!((p.hispeed_at(&b.at(75.0)) - 4.0).abs() < 1e-4);
        p.lane_cover = true;
        p.sudden = 0.25;
        assert_eq!(p.green(&b.at(75.0)), 480);
        p.add_hispeed(1.0, &b.at(75.0));
        assert!((p.hispeed_at(&b.at(75.0)) - 4.0).abs() < 1e-2);
        assert_eq!(p.green(&b.at(300.0)), p.green(&b.at(75.0)));
    }

    /// LR2's HS-FIX: the setting is the hi-speed at 150 BPM; the cover does
    /// not change it, and it is what the player sees.
    #[test]
    fn lr2_hs_fix() {
        let mut p = PlayConfig {
            hispeed: 3.0,
            hs_type: HsType::Lr2,
            hs_fix: HsFix::Max,
            ..Default::default()
        };
        let b = Bpms {
            start: 150.0,
            min: 150.0,
            max: 300.0,
            main: 150.0,
            now: None,
        };
        assert!((p.hispeed_at(&b) - 1.5).abs() < 1e-6);
        assert_eq!(p.hispeed_shown(&b), 3.0);
        // the green number at MAX BPM a 150 BPM chart's at the same setting
        assert_eq!(p.green(&b), p.green(&Bpms::flat(150.0)));
        p.lane_cover = true;
        p.sudden = 0.5;
        assert!((p.hispeed_at(&b) - 1.5).abs() < 1e-6);
        p.add_hispeed(0.25, &b);
        assert_eq!(p.hispeed, 3.25);
        // without HS-FIX it is a plain multiplier
        p.set_hs_fix(HsFix::Off, &b);
        assert_eq!(p.hispeed_at(&b), 3.25);
        // to CLASSIC: the speed it played at
        p.hs_fix = HsFix::Max;
        p.set_hs_type(HsType::Classic, &b);
        assert!((p.hispeed_at(&b) - 1.625).abs() < 1e-2);
    }
}
