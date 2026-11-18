//! The play option panels of the music select, opened while START and / or
//! SELECT is held (IIDX, LR2 and beatoraja). The keys follow beatoraja's
//! 7-key layout, so skins drawn for beatoraja label them right:
//!
//! | panel        | 1            | 2           | 3              | 4           | 5         | 6          | 7           | turntable |
//! |--------------|--------------|-------------|----------------|-------------|-----------|------------|-------------|-----------|
//! | 1 START      | RANDOM ▼     | RANDOM ▲    | GAUGE          | DP OPTION   | HS-FIX    | RANDOM 2P ▲| RANDOM 2P ▼ | TARGET    |
//! | 2 SELECT     | EXPAND JUDGE | CONSTANT    | JUDGE AREA     | LEGACY NOTE | MARK NOTE | BPM GUIDE  | NO MINE     | FREQ      |
//! | 3 both       | BGA          | GAUGE SHIFT | TIMING AUTO    | GREEN −1 ms | TIMING −1 | GREEN +1 ms| TIMING +1   |           |
//!
//! Panel 3's green number keys repeat while held (`repeats`), by 10 ms
//! after 50 steps. Number key 3 changes the LN mode, as in beatoraja.

use crate::config::{BgaMode, Bpms, HiSpeedMode, HsFix, PlayConfig, Target, Versus};
use crate::scene::target::{self, Rival};
use vibebms_core::gauge::{GaugeAutoShift, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::{DoubleOption, LaneOption};
use vibebms_core::play::{freq_label, FREQ_MAX, FREQ_MIN};
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
            g => index_of(p.gauge_preset.selectable(), g),
        },
        2 => battle_index(p),
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
        14 => b(p.bga != BgaMode::Off),
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

/// `v` moved `d` places along `all`, around the end.
pub fn cycle<T: Copy + PartialEq>(all: &[T], v: T, d: i32) -> T {
    let n = all.len() as i32;
    let i = all.iter().position(|x| *x == v).unwrap_or(0) as i32;
    all[(i + d).rem_euclid(n) as usize]
}

pub fn onoff(b: bool) -> &'static str {
    if b {
        "ON"
    } else {
        "OFF"
    }
}

/// DP OPTION as beatoraja lists it (skin button 54): OFF, FLIP, BATTLE,
/// BATTLE AS (BATTLE with AUTO SCRATCH on both sides).
pub fn dp_index(p: &PlayConfig) -> i64 {
    match (p.battle, p.auto_scratch) {
        (true, true) => 3,
        (true, false) => 2,
        _ => p.dp_option as i64,
    }
}

/// Set DP OPTION to `i` (as `dp_index`): BATTLE AS turns AUTO SCRATCH on,
/// leaving it turns it off again.
pub fn set_dp_index(p: &mut PlayConfig, i: i64) {
    if dp_index(p) == 3 && i != 3 {
        p.auto_scratch = false;
    }
    p.battle = i >= 2;
    if i == 3 {
        p.auto_scratch = true;
    }
    p.dp_option = if i == 1 {
        DoubleOption::Flip
    } else {
        DoubleOption::Off
    };
}

/// DP OPTION with the battles that are not play options: BATTLE AS,
/// then G-BATTLE (MY BEST, LAST), VS and G-BATTLE against a saved slot
/// (the select screen's panel and BATTLE row; chosen in the order of
/// `battle_choices`).
pub const BATTLE_LABELS: [&str; 11] = [
    "OFF",
    "FLIP",
    "BATTLE",
    "BATTLE AS",
    "G-BATTLE MY BEST",
    "G-BATTLE LAST",
    "VS",
    "G-BATTLE SLOT 1",
    "G-BATTLE SLOT 2",
    "G-BATTLE SLOT 3",
    "G-BATTLE SLOT 4",
];

pub fn battle_index(p: &PlayConfig) -> i64 {
    match p.versus {
        Versus::Off => dp_index(p),
        Versus::GhostBest => 4,
        Versus::GhostLast => 5,
        Versus::Local => 6,
        Versus::GhostSlot(n) => 6 + n.clamp(1, 4) as i64,
    }
}

pub fn set_battle_index(p: &mut PlayConfig, i: i64) {
    set_dp_index(p, if i <= 3 { i } else { 0 });
    p.versus = match i {
        4 => Versus::GhostBest,
        5 => Versus::GhostLast,
        6 => Versus::Local,
        7..=10 => Versus::GhostSlot((i - 6) as u8),
        _ => Versus::Off,
    };
}

/// The battle values that can be chosen now, in their order: the saved
/// slots after LAST, VS last (it needs a local second player's
/// controls).
pub fn battle_choices(vs: bool) -> &'static [i64] {
    if vs {
        &[0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 6]
    } else {
        &[0, 1, 2, 3, 4, 5, 7, 8, 9, 10]
    }
}

/// The next of `choices` after the current battle value (`d` = ±1).
pub fn step_battle(p: &mut PlayConfig, choices: &[i64], d: i32) -> &'static str {
    let cur = battle_index(p);
    let n = choices.len() as i32;
    let i = choices.iter().position(|c| *c == cur).unwrap_or(0) as i32;
    let next = choices[(i + d).rem_euclid(n) as usize];
    set_battle_index(p, next);
    BATTLE_LABELS[next as usize]
}

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
/// (the other way for `d` = -1: a tap on a value's left half) and name it
/// with its new value.
pub fn key(p: &mut PlayConfig, panel: usize, key: usize, b: &Bpms, d: i32) -> Option<String> {
    Some(match (panel, key) {
        (1, 0 | 1) => {
            let d = if key == 0 { d } else { -d };
            p.random_1p = cycle(&LaneOption::ALL, p.random_1p, d);
            format!("RANDOM: {}", p.random_1p.label())
        }
        (1, 2) => {
            p.gauge = cycle(p.gauge_preset.selectable(), p.gauge, d);
            format!("GAUGE: {}", p.gauge.label())
        }
        (1, 3) => {
            let label = step_battle(p, battle_choices(true), d);
            format!("DP OPTION: {label}")
        }
        (1, 4) => {
            let f = cycle(&HsFix::ALL, p.hs_fix, d);
            p.set_hs_fix(f, b);
            format!("HS-FIX: {}", f.label())
        }
        (1, 5 | 6) => {
            let d = if key == 6 { d } else { -d };
            p.random_2p = cycle(&LaneOption::ALL, p.random_2p, d);
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
            p.bga = cycle(&BgaMode::ALL, p.bga, d);
            format!("BGA: {}", p.bga.label())
        }
        (3, 1) => {
            p.gauge_auto_shift = cycle(&GaugeAutoShift::ALL, p.gauge_auto_shift, d);
            format!("GAUGE AUTO SHIFT: {}", p.gauge_auto_shift.label())
        }
        (3, 2) => {
            p.timing_auto_adjust = !p.timing_auto_adjust;
            format!("TIMING AUTO ADJUST: {}", onoff(p.timing_auto_adjust))
        }
        (3, 3 | 5) => {
            p.add_duration(if key == 5 { d } else { -d } as f64, b);
            hispeed_label(p, b)
        }
        (3, 4 | 6) => {
            p.judge_offset_ms =
                (p.judge_offset_ms + if key == 6 { d } else { -d }).clamp(-500, 500);
            format!("JUDGE TIMING: {:+} ms", p.judge_offset_ms)
        }
        _ => return None,
    })
}

/// The turntable turned `dir` (+1 up) with panel `panel` open (TARGET's
/// IR rivals: `rivals`).
pub fn turn(p: &mut PlayConfig, panel: usize, dir: i32, rivals: &[Rival]) -> Option<String> {
    Some(match panel {
        1 => format!("TARGET: {}", target::step(p, rivals, -dir)),
        2 => step_freq(p, dir),
        _ => return None,
    })
}

/// FREQ one semitone up (`d` = 1) or down, within its range.
pub fn step_freq(p: &mut PlayConfig, d: i32) -> String {
    p.freq = (p.freq + d).clamp(FREQ_MIN, FREQ_MAX);
    format!("FREQ: {}", freq_label(p.freq))
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
        key(&mut p, 1, 0, &b, 1);
        assert_eq!(p.random_1p, LaneOption::Mirror);
        key(&mut p, 1, 1, &b, 1);
        assert_eq!(p.random_1p, LaneOption::Off);
        // DP OPTION: OFF -> FLIP -> BATTLE -> BATTLE AS -> G-BATTLE (MY
        // BEST, LAST, SLOT 1-4) -> VS -> OFF
        key(&mut p, 1, 3, &b, 1);
        assert_eq!((p.dp_option, p.battle), (DoubleOption::Flip, false));
        key(&mut p, 1, 3, &b, 1);
        assert_eq!((p.dp_option, p.battle), (DoubleOption::Off, true));
        key(&mut p, 1, 3, &b, 1);
        assert_eq!((dp_index(&p), p.auto_scratch), (3, true));
        key(&mut p, 1, 3, &b, 1);
        assert_eq!((dp_index(&p), p.auto_scratch, p.battle), (0, false, false));
        assert_eq!(p.versus, Versus::GhostBest);
        key(&mut p, 1, 3, &b, 1);
        assert_eq!(p.versus, Versus::GhostLast);
        for n in 1..=4 {
            key(&mut p, 1, 3, &b, 1);
            assert_eq!(p.versus, Versus::GhostSlot(n));
            assert_eq!(
                BATTLE_LABELS[battle_index(&p) as usize],
                format!("G-BATTLE SLOT {n}")
            );
        }
        key(&mut p, 1, 3, &b, 1);
        assert_eq!(p.versus, Versus::Local);
        key(&mut p, 1, 3, &b, 1);
        assert_eq!((battle_index(&p), p.versus), (0, Versus::Off));
        key(&mut p, 1, 4, &b, 1);
        assert_eq!(p.hs_fix, HsFix::Start);
        let d = p.duration_ms(&b);
        key(&mut p, 3, 5, &b, 1);
        assert!((p.duration_ms(&b) - d - 1.0).abs() < 1e-3);
        key(&mut p, 2, 0, &b, 1);
        assert!(p.expand_judge);
        assert!(key(&mut p, 2, 7, &b, 1).is_none());
        // ASSIST's turntable: FREQ, a semitone a step, -12 ..= +12
        assert_eq!(turn(&mut p, 2, 1, &[]).unwrap(), "FREQ: +1 (106%)");
        for _ in 0..30 {
            turn(&mut p, 2, -1, &[]);
        }
        assert_eq!(p.freq, -12);
    }
}
