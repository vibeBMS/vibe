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

use crate::config::{BgaMode, Bpms, HiSpeedMode, HsFix, PlayConfig, Target};
use crate::scene::index_of;
use crate::scene::play_opts::*;
use crate::scene::target::{self, Rival};
use vibebms_core::gauge::{GaugeAutoShift, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::LaneOption;
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

/// Keys that repeat while held: panel 3's green number (keys 4 and 6).
pub fn repeats(panel: usize, key: usize) -> bool {
    panel == 3 && matches!(key, 3 | 5)
}

/// A held repeating key: change the green number by `step` ms.
pub fn repeat(p: &mut PlayConfig, key: usize, step: f64, b: &Bpms) -> String {
    p.add_duration(if key == 5 { step } else { -step }, b);
    hispeed_label(p, b)
}

/// The option key `key` (0 = key 1) changes with panel `panel` open, and
/// which way.
fn panel_key(panel: usize, key: usize) -> Option<(&'static PlayOpt, i32)> {
    Some(match (panel, key) {
        (1, 0) => (&RANDOM, 1),
        (1, 1) => (&RANDOM, -1),
        (1, 2) => (&GAUGE, 1),
        (1, 3) => (&DP_OPTION, 1),
        (1, 4) => (&HS_FIX, 1),
        (1, 5) => (&RANDOM_2P, -1),
        (1, 6) => (&RANDOM_2P, 1),
        (2, 0) => (&EXPAND_JUDGE, 1),
        (2, 1) => (&CONSTANT, 1),
        (2, 2) => (&JUDGE_AREA, 1),
        (2, 3) => (&LEGACY_NOTE, 1),
        (2, 4) => (&MARK_NOTE, 1),
        (2, 5) => (&BPM_GUIDE, 1),
        (2, 6) => (&NO_MINE, 1),
        (3, 0) => (&BGA, 1),
        (3, 1) => (&AUTO_SHIFT, 1),
        (3, 2) => (&TIMING_AUTO_ADJUST, 1),
        (3, 3) => (&GREEN_NUMBER, -1),
        (3, 4) => (&JUDGE_TIMING, -1),
        (3, 5) => (&GREEN_NUMBER, 1),
        (3, 6) => (&JUDGE_TIMING, 1),
        _ => return None,
    })
}

/// Key `key` (0 = key 1) pressed with panel `panel` open: change the option
/// (the other way for `d` = -1: a tap on a value's left half) and name it
/// with its new value (the green number keys: the hi-speed with it).
pub fn key(p: &mut PlayConfig, panel: usize, key: usize, b: &Bpms, d: i32) -> Option<String> {
    let (o, sign) = panel_key(panel, key)?;
    (o.step)(p, sign * d, b);
    Some(match repeats(panel, key) {
        true => hispeed_label(p, b),
        false => o.said(p, b),
    })
}

/// The turntable turned `dir` (+1 up) with panel `panel` open (TARGET's
/// IR rivals: `rivals`).
pub fn turn(p: &mut PlayConfig, panel: usize, dir: i32, rivals: &[Rival]) -> Option<String> {
    Some(match panel {
        1 => format!("TARGET: {}", target::step(p, rivals, -dir)),
        2 => {
            (FREQ.step)(p, dir, &Bpms::REFERENCE);
            FREQ.said(p, &Bpms::REFERENCE)
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
    use crate::config::Versus;
    use vibebms_core::lane_option::DoubleOption;

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
