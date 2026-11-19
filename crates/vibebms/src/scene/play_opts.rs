//! The play options the song select changes, each once: its name (as
//! skins and toasts show it), its value and its step. The START / SELECT
//! panels (`option_panel`), the select's option list (`SelectOption`), its
//! option buttons and PLAY OPTION's key option texts use them.

use crate::config::{BgaMode, BgaSize, Bpms, Ghost, HiSpeedMode, HsFix, PlayConfig, Side, Versus};
use crate::scene::{cycle, onoff};
use std::borrow::Cow;
use vibebms_core::convert::KeyConvert;
use vibebms_core::gauge::GaugeAutoShift;
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::{DoubleOption, LaneOption, LnAdd, MineAdd};
use vibebms_core::play::{freq_label, FREQ_MAX, FREQ_MIN};
use vibebms_core::LnPref;

/// A play option.
pub struct PlayOpt {
    pub name: &'static str,
    /// Its value (`Bpms`: the chart's, for the green number).
    pub label: for<'p> fn(&'p PlayConfig, &Bpms) -> Cow<'p, str>,
    /// Moved `d` places (-1: back; a toggle flips either way).
    pub step: fn(&mut PlayConfig, i32, &Bpms),
}

impl PlayOpt {
    /// Its name and value, as a toast says them ("GAUGE: HARD").
    pub fn said(&self, p: &PlayConfig, b: &Bpms) -> String {
        format!("{}: {}", self.name, (self.label)(p, b))
    }
}

/// An option of a list of values (their `label`s).
macro_rules! choice {
    ($name:literal, $f:ident, $all:expr) => {
        PlayOpt {
            name: $name,
            label: |p, _| p.$f.label().into(),
            step: |p, d, _| p.$f = cycle(&$all, p.$f, d),
        }
    };
}

/// An option that is on or off.
macro_rules! toggle {
    ($name:literal, $f:ident) => {
        PlayOpt {
            name: $name,
            label: |p, _| onoff(p.$f).into(),
            step: |p, _, _| p.$f ^= true,
        }
    };
}

/// A lane cover / lift share in 1 % steps, up to `max`.
fn cover(v: &mut f32, d: i32, max: f32) {
    *v = (*v + d as f32 * 0.01).clamp(0.0, max);
}

/// A judge or visual offset: 1 ms steps within ±500.
pub fn add_offset(v: &mut i32, d: i32) {
    *v = (*v + d).clamp(-500, 500);
}

fn ms(v: i32) -> Cow<'static, str> {
    format!("{v:+} ms").into()
}

fn pct(v: f32) -> Cow<'static, str> {
    format!("{:.0}%", v * 100.0).into()
}

pub const RANDOM: PlayOpt = choice!("RANDOM", random_1p, LaneOption::ALL);
pub const RANDOM_2P: PlayOpt = choice!("RANDOM 2P", random_2p, LaneOption::ALL);
pub const GAUGE: PlayOpt = PlayOpt {
    name: "GAUGE",
    label: |p, _| p.gauge.label().into(),
    step: |p, d, _| p.gauge = cycle(p.gauge_preset.selectable(), p.gauge, d),
};
/// DP OPTION with the battles (`BATTLE_LABELS`), as PLAY OPTION's key 4.
pub const DP_OPTION: PlayOpt = PlayOpt {
    name: "DP OPTION",
    label: |p, _| BATTLE_LABELS[battle_index(p) as usize].into(),
    step: |p, d, _| step_battle(p, &DP_CHOICES, d),
};
/// The battles alone (DP OPTION without FLIP).
pub const BATTLE: PlayOpt = PlayOpt {
    name: "BATTLE",
    step: |p, d, _| step_battle(p, &BATTLE_CHOICES, d),
    ..DP_OPTION
};
pub const HS_FIX: PlayOpt = PlayOpt {
    name: "HS-FIX",
    label: |p, _| p.hs_fix.label().into(),
    step: |p, d, b| p.set_hs_fix(cycle(&HsFix::ALL, p.hs_fix, d), b),
};
pub const AUTO_SHIFT: PlayOpt = choice!("AUTO SHIFT", gauge_auto_shift, GaugeAutoShift::ALL);
pub const LN_MODE: PlayOpt = PlayOpt {
    name: "LN MODE",
    label: |p, _| p.ln_pref().label().into(),
    step: |p, d, _| {
        let ln = cycle(&LnPref::ALL, p.ln_pref(), d);
        p.set_ln_pref(ln)
    },
};
pub const SIDE: PlayOpt = PlayOpt {
    name: "SIDE",
    label: |p, _| format!("{:?}", p.side).into(),
    step: |p, d, _| p.side = cycle(&[Side::P1, Side::P2], p.side, d),
};
pub const JUDGE: PlayOpt = choice!("JUDGE", judge_preset, JudgePreset::ALL);
pub const AUTOPLAY: PlayOpt = toggle!("AUTOPLAY", autoplay);
pub const LANE_COVER: PlayOpt = PlayOpt {
    name: "LANE COVER",
    label: |p, _| if p.lane_cover { "SUDDEN+" } else { "OFF" }.into(),
    step: |p, _, _| p.lane_cover ^= true,
};
pub const SUDDEN: PlayOpt = PlayOpt {
    name: "SUDDEN+",
    label: |p, _| pct(p.sudden),
    step: |p, d, _| cover(&mut p.sudden, d, 0.9),
};
pub const HIDDEN: PlayOpt = PlayOpt {
    name: "HIDDEN+",
    label: |p, _| pct(p.hidden),
    step: |p, d, _| cover(&mut p.hidden, d, 0.9),
};
pub const LIFT: PlayOpt = PlayOpt {
    name: "LIFT",
    label: |p, _| pct(p.lift),
    step: |p, d, _| cover(&mut p.lift, d, 0.6),
};
pub const JUDGE_TIMING: PlayOpt = PlayOpt {
    name: "JUDGE TIMING",
    label: |p, _| ms(p.judge_offset_ms),
    step: |p, d, _| add_offset(&mut p.judge_offset_ms, d),
};
pub const VISUAL_OFFSET: PlayOpt = PlayOpt {
    name: "VISUAL OFFSET",
    label: |p, _| ms(p.visual_offset_ms),
    step: |p, d, _| add_offset(&mut p.visual_offset_ms, d),
};
/// In 1 ms steps.
pub const GREEN_NUMBER: PlayOpt = PlayOpt {
    name: "GREEN NUMBER",
    label: |p, b| p.green(b).to_string().into(),
    step: |p, d, b| p.add_duration(d as f64, b),
};
pub const CONSTANT: PlayOpt = PlayOpt {
    name: "CONSTANT",
    label: |p, _| onoff(p.hispeed_mode == HiSpeedMode::Constant).into(),
    step: |p, _, _| {
        p.hispeed_mode = match p.hispeed_mode {
            HiSpeedMode::Floating => HiSpeedMode::Constant,
            HiSpeedMode::Constant => HiSpeedMode::Floating,
        }
    },
};
pub const EXPAND_JUDGE: PlayOpt = toggle!("EXPAND JUDGE", expand_judge);
pub const JUDGE_AREA: PlayOpt = toggle!("JUDGE AREA", judge_area);
pub const LEGACY_NOTE: PlayOpt = toggle!("LEGACY NOTE", legacy_note);
pub const MARK_NOTE: PlayOpt = toggle!("MARK NOTE", mark_notes);
pub const BPM_GUIDE: PlayOpt = toggle!("BPM GUIDE", bpm_guide);
pub const NO_MINE: PlayOpt = toggle!("NO MINE", no_mine);
pub const AUTO_SCRATCH: PlayOpt = toggle!("AUTO SCRATCH", auto_scratch);
pub const SCROLL_REMOVE: PlayOpt = toggle!("SCROLL REMOVE", scroll_remove);
pub const TIMING_AUTO_ADJUST: PlayOpt = toggle!("JUDGE AUTO ADJUST", timing_auto_adjust);
pub const FAST_SLOW: PlayOpt = toggle!("FAST / SLOW", show_fast_slow);
pub const SCORE_GRAPH: PlayOpt = toggle!("SCORE GRAPH", score_graph);
pub const BGA: PlayOpt = choice!("BGA", bga, BgaMode::ALL);
pub const BGA_SIZE: PlayOpt = choice!("BGA SIZE", bga_size, BgaSize::ALL);
pub const GHOST: PlayOpt = choice!("GHOST POSITION", ghost, Ghost::ALL);
pub const MINE_ADD: PlayOpt = choice!("MINE ADD", mine_add, MineAdd::ALL);
pub const LN_ADD: PlayOpt = choice!("LONGNOTE ADD", ln_add, LnAdd::ALL);
pub const KEY_CONVERT: PlayOpt = choice!("KEY CONVERT", key_convert, KeyConvert::ALL);
/// A semitone a step.
pub const FREQ: PlayOpt = PlayOpt {
    name: "FREQ",
    label: |p, _| freq_label(p.freq).into(),
    step: |p, d, _| p.freq = (p.freq + d).clamp(FREQ_MIN, FREQ_MAX),
};
/// A lane on (or back): the order turned.
pub const TRAINER_ORDER: PlayOpt = PlayOpt {
    name: "TRAINER ORDER",
    label: |p, _| p.trainer_order.as_str().into(),
    step: |p, d, _| p.trainer_order = vibebms_core::lane_option::turn_order(&p.trainer_order, d),
};

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
fn set_dp_index(p: &mut PlayConfig, i: i64) {
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
/// (chosen in the order of `DP_CHOICES`).
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

/// DP OPTION's values in their order: the saved slots after LAST, VS last
/// (it needs a local second player's controls).
const DP_CHOICES: [i64; 11] = [0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 6];
const BATTLE_CHOICES: [i64; 10] = [0, 2, 3, 4, 5, 7, 8, 9, 10, 6];

pub fn battle_index(p: &PlayConfig) -> i64 {
    match p.versus {
        Versus::Off => dp_index(p),
        Versus::GhostBest => 4,
        Versus::GhostLast => 5,
        Versus::Local => 6,
        Versus::GhostSlot(n) => 6 + n.clamp(1, 4) as i64,
    }
}

fn set_battle_index(p: &mut PlayConfig, i: i64) {
    set_dp_index(p, if i <= 3 { i } else { 0 });
    p.versus = match i {
        4 => Versus::GhostBest,
        5 => Versus::GhostLast,
        6 => Versus::Local,
        7..=10 => Versus::GhostSlot((i - 6) as u8),
        _ => Versus::Off,
    };
}

/// The battle value `d` places after the current one along `choices`.
fn step_battle(p: &mut PlayConfig, choices: &[i64], d: i32) {
    let cur = battle_index(p);
    let n = choices.len() as i32;
    let i = choices.iter().position(|c| *c == cur).unwrap_or(0) as i32;
    set_battle_index(p, choices[(i + d).rem_euclid(n) as usize]);
}
