//! The play option panels of the music select, opened while START and / or
//! SELECT is held (IIDX, LR2 and beatoraja). The keys follow beatoraja's
//! 7-key layout, so skins drawn for beatoraja label them right:
//!
//! | panel        | 1            | 2           | 3              | 4           | 5         | 6          | 7           | turntable |
//! |--------------|--------------|-------------|----------------|-------------|-----------|------------|-------------|-----------|
//! | 1 START      | RANDOM ▼     | RANDOM ▲    | GAUGE          | DP OPTION   | HS-FIX    | RANDOM 2P ▲| RANDOM 2P ▼ | TARGET    |
//! | 2 SELECT     | EXPAND JUDGE | CONSTANT    | JUDGE AREA     | LEGACY NOTE | MARK NOTE | BPM GUIDE  | NO MINE     |           |
//! | 3 both       | BGA          | GAUGE SHIFT | TIMING AUTO    | GREEN −1 ms | TIMING −1 | GREEN +1 ms| TIMING +1   |           |
//!
//! Panel 3's green number keys repeat while held (`repeats`), by 10 ms
//! after 50 steps. Number key 3 changes the LN mode, as in beatoraja.

use crate::config::{Bpms, HiSpeedMode, HsFix, PlayConfig, Target};
use vibebms_core::gauge::{GaugeAutoShift, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::{DoubleOption, LaneOption};
use vibebms_core::LnType;

/// The panel START / SELECT open: 1, 2, 3 (both), or 0.
pub fn panel_of(start: bool, select: bool) -> usize {
    match (start, select) {
        (true, false) => 1,
        (false, true) => 2,
        (true, true) => 3,
        _ => 0,
    }
}

/// The current value of option group `g` for skin conditions (the default
/// skin lights value `v` of group `g` with option 1500 + 16 g + v).
pub fn value(p: &PlayConfig, g: i32) -> Option<i64> {
    let b = |x: bool| x as i64;
    Some(match g {
        0 => index_of(&LaneOption::ALL, p.random_1p),
        1 => match p.gauge {
            GaugeType::Class => 3,
            GaugeType::ExClass | GaugeType::ExHardClass => 4,
            g => index_of(&GaugeType::SELECTABLE, g),
        },
        2 => dp_index(p),
        3 => index_of(&HsFix::ALL, p.hs_fix),
        4 => index_of(&LaneOption::ALL, p.random_2p),
        5 => index_of(&Target::ALL, p.target),
        6 => index_of(&[LnType::Ln, LnType::Cn, LnType::Hcn], p.ln_mode),
        7 => index_of(&JudgePreset::ALL, p.judge_preset),
        8 => index_of(&GaugeAutoShift::ALL, p.gauge_auto_shift),
        9 => b(p.auto_scratch),
        10 => b(p.hispeed_mode == HiSpeedMode::Constant),
        11 => b(p.legacy_note),
        12 => b(p.lane_cover),
        13 => b(p.no_mine),
        14 => b(p.bga),
        15 => b(p.expand_judge),
        16 => b(p.judge_area),
        17 => b(p.mark_notes),
        18 => b(p.bpm_guide),
        19 => b(p.timing_auto_adjust),
        _ => return None,
    })
}

/// Option groups `value` knows.
pub const GROUPS: i32 = 20;

/// Skin option 1500 + 16 g + v: group `g` has value `v`.
pub fn option(p: &PlayConfig, id: i32) -> Option<bool> {
    if !(1500..1500 + 16 * GROUPS).contains(&id) {
        return None;
    }
    let (g, v) = ((id - 1500) / 16, (id - 1500) % 16);
    Some(value(p, g)? == v as i64)
}

/// Position of `v` in `all` (an image set index).
pub fn index_of<T: PartialEq>(all: &[T], v: T) -> i64 {
    all.iter().position(|x| *x == v).unwrap_or(0) as i64
}

fn step<T: Copy + PartialEq>(all: &[T], v: T, d: i32) -> T {
    let n = all.len() as i32;
    let i = all.iter().position(|x| *x == v).unwrap_or(0) as i32;
    all[(i + d).rem_euclid(n) as usize]
}

fn onoff(b: bool) -> &'static str {
    if b {
        "ON"
    } else {
        "OFF"
    }
}

/// DP OPTION as beatoraja lists it: OFF, FLIP, BATTLE.
pub fn dp_index(p: &PlayConfig) -> i64 {
    if p.battle {
        2
    } else {
        p.dp_option as i64
    }
}

const DP_LABELS: [&str; 3] = ["OFF", "FLIP", "BATTLE"];

/// Keys that repeat while held: panel 3's green number (keys 4 and 6).
pub fn repeats(panel: usize, key: usize) -> bool {
    panel == 3 && matches!(key, 3 | 5)
}

/// A held repeating key: change the green number by `step` ms.
pub fn repeat(p: &mut PlayConfig, key: usize, step: f64, b: &Bpms) -> String {
    p.add_duration(if key == 5 { step } else { -step }, b);
    hispeed_label(p, b)
}

/// Key `key` (0 = key 1) pressed with panel `panel` open: change the option
/// and name it with its new value.
pub fn key(p: &mut PlayConfig, panel: usize, key: usize, b: &Bpms) -> Option<String> {
    Some(match (panel, key) {
        (1, 0 | 1) => {
            p.random_1p = step(&LaneOption::ALL, p.random_1p, if key == 0 { 1 } else { -1 });
            format!("RANDOM: {}", p.random_1p.label())
        }
        (1, 2) => {
            p.gauge = step(&GaugeType::SELECTABLE, p.gauge, 1);
            format!("GAUGE: {}", p.gauge.label())
        }
        (1, 3) => {
            let i = (dp_index(p) + 1) % 3;
            p.battle = i == 2;
            p.dp_option = if i == 1 {
                DoubleOption::Flip
            } else {
                DoubleOption::Off
            };
            format!("DP OPTION: {}", DP_LABELS[i as usize])
        }
        (1, 4) => {
            let f = step(&HsFix::ALL, p.hs_fix, 1);
            p.set_hs_fix(f, b);
            format!("HS-FIX: {}", f.label())
        }
        (1, 5 | 6) => {
            p.random_2p = step(&LaneOption::ALL, p.random_2p, if key == 6 { 1 } else { -1 });
            format!("RANDOM 2P: {}", p.random_2p.label())
        }
        (2, 0) => {
            p.expand_judge = !p.expand_judge;
            format!("EXPAND JUDGE: {}", onoff(p.expand_judge))
        }
        (2, 1) => {
            p.hispeed_mode = match p.hispeed_mode {
                HiSpeedMode::Floating => HiSpeedMode::Constant,
                HiSpeedMode::Constant => HiSpeedMode::Floating,
            };
            format!(
                "CONSTANT: {}",
                onoff(p.hispeed_mode == HiSpeedMode::Constant)
            )
        }
        (2, 2) => {
            p.judge_area = !p.judge_area;
            format!("JUDGE AREA: {}", onoff(p.judge_area))
        }
        (2, 3) => {
            p.legacy_note = !p.legacy_note;
            format!("LEGACY NOTE: {}", onoff(p.legacy_note))
        }
        (2, 4) => {
            p.mark_notes = !p.mark_notes;
            format!("MARK NOTE: {}", onoff(p.mark_notes))
        }
        (2, 5) => {
            p.bpm_guide = !p.bpm_guide;
            format!("BPM GUIDE: {}", onoff(p.bpm_guide))
        }
        (2, 6) => {
            p.no_mine = !p.no_mine;
            format!("NO MINE: {}", onoff(p.no_mine))
        }
        (3, 0) => {
            p.bga = !p.bga;
            format!("BGA: {}", onoff(p.bga))
        }
        (3, 1) => {
            p.gauge_auto_shift = step(&GaugeAutoShift::ALL, p.gauge_auto_shift, 1);
            format!("GAUGE AUTO SHIFT: {}", p.gauge_auto_shift.label())
        }
        (3, 2) => {
            p.timing_auto_adjust = !p.timing_auto_adjust;
            format!("TIMING AUTO ADJUST: {}", onoff(p.timing_auto_adjust))
        }
        (3, 3 | 5) => {
            p.add_duration(if key == 5 { 1.0 } else { -1.0 }, b);
            hispeed_label(p, b)
        }
        (3, 4 | 6) => {
            p.judge_offset_ms =
                (p.judge_offset_ms + if key == 6 { 1 } else { -1 }).clamp(-500, 500);
            format!("JUDGE TIMING: {:+} ms", p.judge_offset_ms)
        }
        _ => return None,
    })
}

/// The turntable turned `dir` (+1 up) with panel `panel` open.
pub fn turn(p: &mut PlayConfig, panel: usize, dir: i32, _b: &Bpms) -> Option<String> {
    Some(match panel {
        1 => {
            p.target = step(&Target::ALL, p.target, -dir);
            format!("TARGET: {}", p.target.label())
        }
        _ => return None,
    })
}

fn hispeed_label(p: &PlayConfig, b: &Bpms) -> String {
    format!("HI-SPEED {:.2} (GN {})", p.hispeed_at(b), p.green(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_beatoraja() {
        let mut p = PlayConfig::default();
        let b = Bpms::flat(150.0);
        assert_eq!(panel_of(true, false), 1);
        assert_eq!(panel_of(true, true), 3);
        key(&mut p, 1, 0, &b);
        assert_eq!(p.random_1p, LaneOption::Mirror);
        key(&mut p, 1, 1, &b);
        assert_eq!(p.random_1p, LaneOption::Off);
        // DP OPTION: OFF -> FLIP -> BATTLE -> OFF
        key(&mut p, 1, 3, &b);
        assert_eq!((p.dp_option, p.battle), (DoubleOption::Flip, false));
        key(&mut p, 1, 3, &b);
        assert_eq!((p.dp_option, p.battle), (DoubleOption::Off, true));
        key(&mut p, 1, 3, &b);
        assert_eq!(dp_index(&p), 0);
        key(&mut p, 1, 4, &b);
        assert_eq!(p.hs_fix, HsFix::Start);
        let d = p.duration_ms(&b);
        key(&mut p, 3, 5, &b);
        assert!((p.duration_ms(&b) - d - 1.0).abs() < 1e-3);
        key(&mut p, 2, 0, &b);
        assert!(p.expand_judge);
        assert!(key(&mut p, 2, 7, &b).is_none());
    }
}
