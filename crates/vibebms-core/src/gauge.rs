//! Groove gauges.
//!
//! All gauge types are simulated in parallel (like beatoraja) so that
//! GAUGE AUTO SHIFT can switch between them and the best clear can be
//! determined after play.
//!
//! Gauge change tables are ordered PG, GR, GD, BD, POOR, empty POOR.

use crate::course::CourseKind;
use crate::judge::Judge;
use crate::mode::RuleFamily;
use crate::score::ClearType;
use serde::{Deserialize, Serialize};

crate::choices! {
    #[derive(
        Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord, Default,
    )]
    pub enum GaugeType {
        AssistEasy = 0 => "ASSIST EASY",
        Easy = 1 => "EASY",
        #[default]
        Normal = 2 => "NORMAL",
        Hard = 3 => "HARD",
        ExHard = 4 => "EX-HARD",
        Hazard = 5 => "HAZARD",
        /// Dan (段位) gauges.
        Class = 6 => "CLASS",
        ExClass = 7 => "EX-CLASS",
        ExHardClass = 8 => "EXHARD-CLASS",
        /// LR2's attack gauges (the LR2 set only): P-ATTACK takes only
        /// PGREAT and GREAT, G-ATTACK heals only on GOOD (S-OLR2b 1).
        PAttack = 9 => "P-ATTACK",
        GAttack = 10 => "G-ATTACK",
    }
}

impl GaugeType {
    /// Gauges selectable for normal play.
    pub const SELECTABLE: [GaugeType; 6] = [
        GaugeType::AssistEasy,
        GaugeType::Easy,
        GaugeType::Normal,
        GaugeType::Hard,
        GaugeType::ExHard,
        GaugeType::Hazard,
    ];

    pub fn is_survival(self) -> bool {
        !matches!(
            self,
            GaugeType::AssistEasy | GaugeType::Easy | GaugeType::Normal
        )
    }

    /// The gauges every set has (all but the attack gauges).
    pub const STANDARD: [GaugeType; 9] = [
        GaugeType::AssistEasy,
        GaugeType::Easy,
        GaugeType::Normal,
        GaugeType::Hard,
        GaugeType::ExHard,
        GaugeType::Hazard,
        GaugeType::Class,
        GaugeType::ExClass,
        GaugeType::ExHardClass,
    ];

    /// The LR2 set's gauge option, in LR2's order (S-OLR2b 1.1), then the
    /// set's two gauges LR2 does not have.
    pub const LR2_SELECTABLE: [GaugeType; 8] = [
        GaugeType::Normal,
        GaugeType::Hard,
        GaugeType::Hazard,
        GaugeType::Easy,
        GaugeType::PAttack,
        GaugeType::GAttack,
        GaugeType::AssistEasy,
        GaugeType::ExHard,
    ];

    pub fn is_attack(self) -> bool {
        matches!(self, GaugeType::PAttack | GaugeType::GAttack)
    }

    pub fn is_class(self) -> bool {
        matches!(
            self,
            GaugeType::Class | GaugeType::ExClass | GaugeType::ExHardClass
        )
    }

    /// Clear lamp awarded when this gauge survives / is qualified.
    pub fn clear_type(self) -> ClearType {
        match self {
            GaugeType::AssistEasy => ClearType::LightAssistEasy,
            GaugeType::Easy => ClearType::Easy,
            GaugeType::Normal | GaugeType::Class => ClearType::Normal,
            GaugeType::Hard | GaugeType::ExClass | GaugeType::PAttack | GaugeType::GAttack => {
                ClearType::Hard
            }
            GaugeType::ExHard | GaugeType::ExHardClass => ClearType::ExHard,
            GaugeType::Hazard => ClearType::FullCombo,
        }
    }

    /// The dan gauge corresponding to a normal gauge selection.
    pub fn to_class(self) -> GaugeType {
        match self {
            GaugeType::AssistEasy | GaugeType::Easy | GaugeType::Normal => GaugeType::Class,
            GaugeType::Hard => GaugeType::ExClass,
            _ => GaugeType::ExHardClass,
        }
    }

    /// The gauge an LR2 class course plays a selection on (S-OLR2b 5.1):
    /// EASY and G-ATTACK become GROOVE (its class gauge, CLASS), HARD its
    /// class HARD (EX-CLASS); HAZARD and P-ATTACK are kept. (ASSIST EASY
    /// and EX-HARD, not LR2's, as `to_class`.)
    pub fn to_lr2_class(self) -> GaugeType {
        match self {
            GaugeType::Hazard
            | GaugeType::PAttack
            | GaugeType::Class
            | GaugeType::ExClass
            | GaugeType::ExHardClass => self,
            GaugeType::GAttack => GaugeType::Class,
            g => g.to_class(),
        }
    }
}

impl GaugePreset {
    /// The gauge option's values under this set, in its order.
    pub fn selectable(self) -> &'static [GaugeType] {
        match self {
            GaugePreset::Lr2 => &GaugeType::LR2_SELECTABLE,
            _ => &GaugeType::SELECTABLE,
        }
    }

    /// Whether the set has gauge `t` (the attack gauges are LR2's only).
    pub fn has(self, t: GaugeType) -> bool {
        self == GaugePreset::Lr2 || !t.is_attack()
    }
}

crate::choices! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum GaugePreset {
        /// beatoraja gauges for the chart's key mode.
        #[default]
        Beatoraja => "beatoraja",
        /// beatoraja's LR2-compatible gauge set.
        Lr2 => "LR2",
        /// IIDX-like gauges (approximation).
        Iidx => "IIDX",
    }
}

crate::choices! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum GaugeAutoShift {
        #[default]
        None => "OFF",
        /// Keep playing after the gauge empties (result is FAILED).
        Continue => "CONTINUE",
        /// HARD/EX-HARD drop to NORMAL when emptied.
        SurvivalToGroove => "HARD→GROOVE",
        /// Start at EX-HARD and step down, keeping the best clear.
        BestClear => "BEST CLEAR",
        /// Start at the selected gauge and step down.
        SelectToUnder => "SELECT TO UNDER",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Modifier {
    /// Positive values scaled by TOTAL / notes.
    Total,
    /// Positive values limited for low-TOTAL charts.
    LimitIncrement,
    /// Damage increased for low TOTAL / few notes (beatoraja's version).
    ModifyDamage,
    /// LR2's HARD damage increase: by TOTAL below 256 and by note count
    /// below 1000, the larger (S-OLR2 3.4).
    Lr2Damage,
    /// G-ATTACK's: that factor on its PGREAT, POOR and empty POOR only
    /// (S-OLR2b 1.2).
    Lr2Attack,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GaugeElement {
    modifier: Option<Modifier>,
    pub min: f32,
    pub max: f32,
    pub init: f32,
    pub border: f32,
    pub values: [f32; 6],
    /// `(threshold, multiplier)`: damage is multiplied when below the threshold.
    pub guts: &'static [(f32, f32)],
    /// The gauge empties when it falls below this (LR2: 2 %).
    pub death: f32,
    /// A mine takes the gauge down to this at most (LR2's groove gauges
    /// in a course: 2 %, though judgements may empty them).
    pub mine_floor: f32,
}

const fn e(
    modifier: Option<Modifier>,
    min: f32,
    max: f32,
    init: f32,
    border: f32,
    values: [f32; 6],
    guts: &'static [(f32, f32)],
) -> GaugeElement {
    GaugeElement {
        modifier,
        min,
        max,
        init,
        border,
        values,
        guts,
        death: 0.0,
        mine_floor: 0.0,
    }
}

/// An LR2 survival gauge: it empties below 2 %.
const fn lr2(g: GaugeElement) -> GaugeElement {
    GaugeElement { death: 2.0, ..g }
}

/// A gauge of LR2's in a course (S-OLR2 3.6; S-OLR2b 3.6, 4.1, 5.2): it
/// starts full, has no 2 % floor for judgements (it empties below 2 %,
/// though a mine still stops the groove gauges at 2 %), takes the × 0.6
/// rule and clears at any value it lives at. In expert and NONSTOP
/// courses GROOVE and EASY have their own values; a class course plays
/// its own gauges (`GaugeType::to_lr2_class`).
fn lr2_course(g: &GaugeElement, ty: GaugeType, kind: CourseKind) -> GaugeElement {
    use GaugeType::*;
    let groove = matches!(ty, AssistEasy | Easy | Normal | Class);
    let mut e = GaugeElement {
        init: 100.0,
        min: 0.0,
        border: 0.0,
        death: 2.0,
        guts: LR2_GUTS,
        mine_floor: if groove { 2.0 } else { 0.0 },
        ..g.clone()
    };
    let own = match (kind, ty) {
        (CourseKind::Class, _) => None,
        (_, Normal) => Some([0.1, 0.1, 0.04, -1.5, -2.0, -1.5]),
        (_, AssistEasy | Easy) => Some([0.12, 0.12, 0.048, -1.2, -1.6, -1.2]),
        _ => None,
    };
    if let Some(v) = own {
        e.values = v;
        e.modifier = None;
    }
    e
}

use Modifier::*;
#[rustfmt::skip]
const HARD_GUTS: &[(f32, f32)] = &[(10.0, 0.4), (20.0, 0.5), (30.0, 0.6), (40.0, 0.7), (50.0, 0.8)];
#[rustfmt::skip]
const CLASS_GUTS: &[(f32, f32)] = &[(5.0, 0.4), (10.0, 0.5), (15.0, 0.6), (20.0, 0.7), (25.0, 0.8)];
const LR2_GUTS: &[(f32, f32)] = &[(32.0, 0.6)];
const IIDX_GUTS: &[(f32, f32)] = &[(30.0, 0.5)];

// One row per GaugeType, in its order: e(modifier, min, max, init, border,
// [PG, GR, GD, BD, POOR, empty POOR], guts).

// beatoraja GaugeProperty values
#[rustfmt::skip]
const FIVE: [GaugeElement; 9] = [
    e(Some(Total),          2.0, 100.0, 20.0,  50.0, [ 1.0,  1.0, 0.5,   -1.5,   -3.0,   -0.5], &[]),
    e(Some(Total),          2.0, 100.0, 20.0,  75.0, [ 1.0,  1.0, 0.5,   -1.5,   -4.5,   -1.0], &[]),
    e(Some(Total),          2.0, 100.0, 20.0,  75.0, [ 1.0,  1.0, 0.5,   -3.0,   -6.0,   -2.0], &[]),
    e(Some(LimitIncrement), 0.0, 100.0, 100.0, 0.0,  [ 0.0,  0.0, 0.0,   -5.0,  -10.0,   -5.0], &[]),
    e(Some(ModifyDamage),   0.0, 100.0, 100.0, 0.0,  [ 0.0,  0.0, 0.0,  -10.0,  -20.0,  -10.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [ 0.0,  0.0, 0.0, -100.0, -100.0, -100.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.01, 0.01, 0.0,   -0.5,   -1.0,   -0.5], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.01, 0.01, 0.0,   -1.0,   -2.0,   -1.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.01, 0.01, 0.0,   -2.5,   -5.0,   -2.5], &[]),
];
#[rustfmt::skip]
const SEVEN: [GaugeElement; 9] = [
    e(Some(Total),          2.0, 100.0, 20.0,  60.0, [ 1.0,  1.0,  0.5,   -1.5,   -3.0,  -0.5], &[]),
    e(Some(Total),          2.0, 100.0, 20.0,  80.0, [ 1.0,  1.0,  0.5,   -1.5,   -4.5,  -1.0], &[]),
    e(Some(Total),          2.0, 100.0, 20.0,  80.0, [ 1.0,  1.0,  0.5,   -3.0,   -6.0,  -2.0], &[]),
    e(Some(LimitIncrement), 0.0, 100.0, 100.0, 0.0,  [0.15, 0.12, 0.03,   -5.0,  -10.0,  -5.0], HARD_GUTS),
    e(Some(LimitIncrement), 0.0, 100.0, 100.0, 0.0,  [0.15, 0.06,  0.0,   -8.0,  -16.0,  -8.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.15, 0.06,  0.0, -100.0, -100.0, -10.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.15, 0.12, 0.06,   -1.5,   -3.0,  -1.5], CLASS_GUTS),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.15, 0.12, 0.03,   -3.0,   -6.0,  -3.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.15, 0.06,  0.0,   -5.0,  -10.0,  -5.0], &[]),
];
#[rustfmt::skip]
const PMS: [GaugeElement; 9] = [
    e(Some(Total),          2.0, 120.0, 30.0,  65.0, [ 1.0,  1.0,  0.5,   -1.0,   -2.0,   -2.0], &[]),
    e(Some(Total),          2.0, 120.0, 30.0,  85.0, [ 1.0,  1.0,  0.5,   -1.0,   -3.0,   -3.0], &[]),
    e(Some(Total),          2.0, 120.0, 30.0,  85.0, [ 1.0,  1.0,  0.5,   -2.0,   -6.0,   -6.0], &[]),
    e(Some(LimitIncrement), 0.0, 100.0, 100.0, 0.0,  [0.15, 0.12, 0.03,   -5.0,  -10.0,  -10.0], HARD_GUTS),
    e(Some(LimitIncrement), 0.0, 100.0, 100.0, 0.0,  [0.15, 0.06,  0.0,  -10.0,  -15.0,  -15.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.15, 0.06,  0.0, -100.0, -100.0, -100.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.15, 0.12, 0.06,   -1.5,   -3.0,   -3.0], CLASS_GUTS),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.15, 0.12, 0.03,   -3.0,   -6.0,   -6.0], &[]),
    e(None,                 0.0, 100.0, 100.0, 0.0,  [0.15, 0.06,  0.0,   -5.0,  -10.0,  -10.0], &[]),
];
// LR2's (S-OLR2 3): HAZARD (DEATH) gains nothing, a BAD or POOR ends the
// play, an empty POOR costs nothing, it clears at 80 % as GROOVE does;
// the dan gauges (GROOVE or EASY: CLASS; HARD: EX-CLASS, with HARD's
// damage) gain 0.04 for a GOOD. ASSIST EASY, EX-HARD and EXHARD-CLASS are
// not LR2's.
#[rustfmt::skip]
const LR2: [GaugeElement; 11] = [
    e(    Some(Total),     2.0, 100.0, 20.0,  60.0, [ 1.2,  1.2,  0.6,   -3.2,   -4.8,  -1.6], &[]),
    e(    Some(Total),     2.0, 100.0, 20.0,  80.0, [ 1.2,  1.2,  0.6,   -3.2,   -4.8,  -1.6], &[]),
    e(    Some(Total),     2.0, 100.0, 20.0,  80.0, [ 1.0,  1.0,  0.5,   -4.0,   -6.0,  -2.0], &[]),
    lr2(e(Some(Lr2Damage), 0.0, 100.0, 100.0, 0.0,  [ 0.1,  0.1, 0.05,   -6.0,  -10.0,  -2.0], LR2_GUTS)),
    lr2(e(Some(Lr2Damage), 0.0, 100.0, 100.0, 0.0,  [ 0.1,  0.1, 0.05,  -12.0,  -20.0,  -2.0], &[])),
    lr2(e(None,            0.0, 100.0, 100.0, 80.0, [ 0.0,  0.0,  0.0, -100.0, -100.0,   0.0], LR2_GUTS)),
    lr2(e(None,            0.0, 100.0, 100.0, 0.0,  [0.10, 0.10, 0.04,   -2.0,   -3.0,  -2.0], LR2_GUTS)),
    lr2(e(Some(Lr2Damage), 0.0, 100.0, 100.0, 0.0,  [0.10, 0.10, 0.04,   -6.0,  -10.0,  -2.0], LR2_GUTS)),
    lr2(e(None,            0.0, 100.0, 100.0, 0.0,  [0.10, 0.10, 0.05,  -12.0,  -20.0,  -2.0], &[])),
    // P-ATTACK: a GOOD or worse, an empty POOR, ends the play; G-ATTACK:
    // its PGREAT costs as much as a POOR (S-OLR2b 1.2)
    lr2(e(None,            0.0, 100.0, 100.0, 0.0,  [ 0.1, -1.0, -100.0, -100.0, -100.0, -100.0], LR2_GUTS)),
    lr2(e(Some(Lr2Attack), 0.0, 100.0, 100.0, 0.0,  [-10.0, -1.0,  0.1,   -6.0,  -10.0,  -2.0], LR2_GUTS)),
];
// IIDX-like approximation (groove gauges start at 22%, HARD damage halves below 30%)
#[rustfmt::skip]
const IIDX: [GaugeElement; 9] = [
    e(Some(Total), 2.0, 100.0, 22.0,  60.0, [ 1.0,  1.0,  0.5,   -1.6,   -4.8,  -1.6], &[]),
    e(Some(Total), 2.0, 100.0, 22.0,  80.0, [ 1.0,  1.0,  0.5,   -1.6,   -4.8,  -1.6], &[]),
    e(Some(Total), 2.0, 100.0, 22.0,  80.0, [ 1.0,  1.0,  0.5,   -2.0,   -6.0,  -2.0], &[]),
    e(None,        0.0, 100.0, 100.0, 0.0,  [0.16, 0.16,  0.0,   -5.0,   -9.0,  -5.0], IIDX_GUTS),
    e(None,        0.0, 100.0, 100.0, 0.0,  [0.16, 0.16,  0.0,  -10.0,  -18.0, -10.0], &[]),
    e(None,        0.0, 100.0, 100.0, 0.0,  [0.16, 0.16,  0.0, -100.0, -100.0, -10.0], &[]),
    e(None,        0.0, 100.0, 100.0, 0.0,  [0.16, 0.16, 0.04,   -1.5,   -2.5,  -1.5], IIDX_GUTS),
    e(None,        0.0, 100.0, 100.0, 0.0,  [0.16, 0.16,  0.0,   -3.0,   -6.0,  -3.0], &[]),
    e(None,        0.0, 100.0, 100.0, 0.0,  [0.16, 0.16,  0.0,   -5.0,  -10.0,  -5.0], &[]),
];

/// The set's gauges, by `GaugeType` (the attack gauges: the LR2 set's).
pub fn gauge_elements(preset: GaugePreset, fam: RuleFamily) -> &'static [GaugeElement] {
    match (preset, fam) {
        (GaugePreset::Lr2, _) => &LR2,
        (GaugePreset::Iidx, _) => &IIDX,
        (_, RuleFamily::FiveKeys) => &FIVE,
        (_, RuleFamily::SevenKeys) => &SEVEN,
        (_, RuleFamily::Pms) => &PMS,
    }
}

/// Change `i` (PG .. empty POOR), `f`, made for `total` and `notes`.
fn modify(m: Modifier, i: usize, f: f32, total: f64, notes: usize) -> f32 {
    let notes_f = notes.max(1) as f64;
    match m {
        Total => {
            if f > 0.0 {
                (f as f64 * total / notes_f) as f32
            } else {
                f
            }
        }
        LimitIncrement => {
            let pg = ((2.0 * total - 320.0) / notes_f).clamp(0.0, 0.15) as f32;
            if f > 0.0 {
                f * pg / 0.15
            } else {
                f
            }
        }
        Lr2Damage => {
            if f < 0.0 {
                f * lr2_damage_factor(total, notes as f64)
            } else {
                f
            }
        }
        Lr2Attack => {
            if matches!(i, 0 | 4 | 5) {
                f * lr2_damage_factor(total, notes as f64)
            } else {
                f
            }
        }
        ModifyDamage => {
            if f < 0.0 {
                f * five_key_damage_factor(total, notes)
            } else {
                f
            }
        }
    }
}

// The two damage multipliers below were written from a behaviour
// specification (TOTAL steps and note-count breakpoints), not from another
// program's code.

/// The TOTAL factor of the five-key EX-HARD gauge: steps at multiples of
/// 10, below 120 10 (the table S2 and S4 give as beatoraja's).
const TOTAL_STEPS: [(f64, f32); 9] = [
    (240.0, 1.0),
    (230.0, 1.11),
    (210.0, 1.25),
    (200.0, 1.5),
    (180.0, 1.666),
    (160.0, 2.0),
    (150.0, 2.5),
    (130.0, 3.333),
    (120.0, 5.0),
];

/// Damage multiplier of the beatoraja-preset five-key EX-HARD gauge: the
/// larger of the TOTAL factor and a note-count factor (single precision).
fn five_key_damage_factor(total: f64, notes: usize) -> f32 {
    // breakpoints 1000, 500, 250, ... (halved), slopes 0.002, 0.004, ...
    // (doubled) per note fewer
    let by_notes = if notes >= 1000 {
        1.0
    } else {
        // below 1 the factor stays at its value for 1
        let n = notes.max(1);
        let (mut hi, mut factor, mut slope) = (1000usize, 1.0f32, 0.002f32);
        loop {
            let lo = hi / 2;
            if n >= lo {
                break factor + slope * (hi - n) as f32;
            }
            factor += slope * (hi - lo) as f32;
            slope *= 2.0;
            hi = lo;
        }
    };
    let by_total = if total.is_nan() {
        1.0
    } else {
        TOTAL_STEPS
            .iter()
            .find(|(t, _)| total >= *t)
            .map_or(10.0, |(_, f)| *f)
    };
    by_total.max(by_notes)
}

/// LR2's HARD damage factor by TOTAL: 10 / floor((TOTAL - 80) / 16), the
/// step at least 1 (S-OLR2 3.4).
fn lr2_total_factor(total: f64) -> f64 {
    if total.is_nan() {
        return 1.0;
    }
    10.0 / ((total - 80.0) / 16.0).trunc().max(1.0)
}

/// Damage multiplier of the LR2-preset HARD (and EX-HARD, EX-CLASS)
/// gauges: the larger of the TOTAL factor and the note-count factor
/// (S-OLR2 3.4), computed in double precision and rounded to single
/// precision.
fn lr2_damage_factor(total: f64, n: f64) -> f32 {
    let by_notes = if n >= 1000.0 {
        1.0
    } else if n >= 500.0 {
        1.0 + 0.002 * (1000.0 - n)
    } else if n >= 250.0 {
        2.0 + 0.004 * (500.0 - n)
    } else if n >= 125.0 {
        3.0 + 0.008 * (250.0 - n)
    } else if n >= 60.0 {
        4.0 + (125.0 - n) / 65.0
    } else if n >= 30.0 {
        5.0 + (60.0 - n) / 15.0
    } else if n > 20.0 {
        8.0 + 0.2 * (30.0 - n)
    } else {
        10.0
    };
    f64::max(lr2_total_factor(total), by_notes) as f32
}

/// The LR2-preset HARD damage of a chart with `notes` notes and `total`
/// may not be LR2's: from 21 to 29 notes S-OLR2 marks the note-count
/// factor's segment unchecked (10 − 0.2 per note above 20, then 7 at 30;
/// a continuous 10 − 0.3 per note would differ), and it decides there
/// unless the TOTAL factor is at least as large (docs/RULES.md, gap A4).
pub fn lr2_hard_damage_unconfirmed(total: f64, notes: u32) -> bool {
    (21..30).contains(&notes) && lr2_total_factor(total) < 10.0 - 0.2 * (notes - 20) as f64
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gauge {
    pub ty: GaugeType,
    pub elem: GaugeElement,
    values: [f32; 6],
    value: f32,
}

impl Gauge {
    pub fn new(ty: GaugeType, elem: &GaugeElement, total: f64, notes: usize) -> Self {
        let mut values = elem.values;
        if let Some(m) = elem.modifier {
            for (i, v) in values.iter_mut().enumerate() {
                *v = modify(m, i, *v, total, notes);
            }
        }
        Gauge {
            ty,
            elem: elem.clone(),
            values,
            value: elem.init,
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Once a survival gauge hits 0 it stays dead.
    pub fn set_value(&mut self, v: f32) {
        if self.value > 0.0 {
            self.value = v.clamp(self.elem.min, self.elem.max);
            if self.value < self.elem.death {
                self.value = 0.0;
            }
        }
    }

    pub fn update(&mut self, judge: Judge, rate: f32) {
        let mut inc = self.values[judge as usize] * rate;
        // (only BAD .. empty POOR: G-ATTACK's PGREAT and the attack
        // gauges' GREAT are never reduced)
        if inc < 0.0 && judge >= Judge::Bad {
            if let Some((_, m)) = self.elem.guts.iter().find(|(t, _)| self.value < *t) {
                inc *= m;
            }
        }
        self.set_value(self.value + inc);
    }

    pub fn is_dead(&self) -> bool {
        self.value <= 0.0
    }

    pub fn is_qualified(&self) -> bool {
        self.value > 0.0 && self.value >= self.elem.border
    }

    pub fn is_max(&self) -> bool {
        self.value >= self.elem.max
    }

    pub fn change_for(&self, judge: Judge) -> f32 {
        self.values[judge as usize]
    }
}

/// The gauges GAUGE AUTO SHIFT steps down through, strongest first.
const SHIFT_ORDER: [GaugeType; 6] = [
    GaugeType::Hazard,
    GaugeType::ExHard,
    GaugeType::Hard,
    GaugeType::Normal,
    GaugeType::Easy,
    GaugeType::AssistEasy,
];

#[derive(Clone, Debug, PartialEq)]
pub struct GrooveGauge {
    pub gauges: Vec<Gauge>,
    current: GaugeType,
    selected: GaugeType,
    pub auto_shift: GaugeAutoShift,
}

impl GrooveGauge {
    pub fn new(
        preset: GaugePreset,
        fam: RuleFamily,
        selected: GaugeType,
        gas: GaugeAutoShift,
        total: f64,
        notes: usize,
    ) -> Self {
        Self::new_in(preset, fam, selected, gas, total, notes, None)
    }

    /// The gauges of a play, in a course of `course`'s kind (`None`: a
    /// single play): every gauge of the set, the attack gauge played
    /// with them (an attack gauge the set has not: HARD). A course
    /// changes only the LR2 set's gauges (`lr2_course`).
    pub fn new_in(
        preset: GaugePreset,
        fam: RuleFamily,
        selected: GaugeType,
        gas: GaugeAutoShift,
        total: f64,
        notes: usize,
        course: Option<CourseKind>,
    ) -> Self {
        let selected = if preset.has(selected) {
            selected
        } else {
            GaugeType::Hard
        };
        let elems = gauge_elements(preset, fam);
        let attack = selected.is_attack().then_some(selected);
        let gauges = GaugeType::STANDARD
            .into_iter()
            .chain(attack)
            .map(|t| {
                let e = &elems[t as usize];
                match course.filter(|_| preset == GaugePreset::Lr2) {
                    Some(k) => Gauge::new(t, &lr2_course(e, t, k), total, notes),
                    None => Gauge::new(t, e, total, notes),
                }
            })
            .collect();
        let current = match gas {
            GaugeAutoShift::BestClear if SHIFT_ORDER.contains(&selected) => GaugeType::ExHard,
            _ => selected,
        };
        GrooveGauge {
            gauges,
            current,
            selected,
            auto_shift: gas,
        }
    }

    pub fn current_type(&self) -> GaugeType {
        self.current
    }

    /// The gauge played from (an attack gauge the set has not: HARD).
    pub fn selected_type(&self) -> GaugeType {
        self.selected
    }

    pub fn current(&self) -> &Gauge {
        self.get(self.current)
    }

    /// Gauge `t`: one of the set's, or the attack gauge played.
    pub fn get(&self, t: GaugeType) -> &Gauge {
        let i = t as usize;
        match self.gauges.get(i).filter(|g| g.ty == t) {
            Some(g) => g,
            None => self
                .gauges
                .iter()
                .find(|g| g.ty == t)
                .unwrap_or(&self.gauges[0]),
        }
    }

    pub fn value(&self) -> f32 {
        self.current().value()
    }

    pub fn update(&mut self, judge: Judge, rate: f32) {
        for g in &mut self.gauges {
            g.update(judge, rate);
        }
        self.shift();
    }

    /// A mine's damage `v` on every gauge (no guts; LR2's groove gauges
    /// in a course stop at their mine floor).
    pub fn add_value(&mut self, v: f32) {
        for g in &mut self.gauges {
            let nv = (g.value() + v).max(g.elem.mine_floor);
            g.set_value(nv);
        }
        self.shift();
    }

    /// Restore values (e.g. carrying gauges across a course), in the
    /// order of `values`.
    pub fn set_values(&mut self, values: &[f32]) {
        for (g, v) in self.gauges.iter_mut().zip(values) {
            g.value = v.clamp(g.elem.min, g.elem.max);
            if *v <= 0.0 {
                g.value = 0.0;
            }
        }
    }

    pub fn values(&self) -> Vec<f32> {
        self.gauges.iter().map(|g| g.value()).collect()
    }

    fn shift(&mut self) {
        if !self.current().is_dead() {
            return;
        }
        match self.auto_shift {
            GaugeAutoShift::SurvivalToGroove
                if self.current.is_survival() && !self.current.is_class() =>
            {
                self.current = GaugeType::Normal;
            }
            GaugeAutoShift::BestClear | GaugeAutoShift::SelectToUnder => {
                if let Some(pos) = SHIFT_ORDER.iter().position(|t| *t == self.current) {
                    if let Some(t) = SHIFT_ORDER[pos + 1..]
                        .iter()
                        .find(|t| !self.get(**t).is_dead())
                    {
                        self.current = *t;
                    }
                }
            }
            _ => {}
        }
    }

    /// True when play must stop (survival gauge emptied without a fallback).
    pub fn is_failed_now(&self) -> bool {
        self.current().is_dead() && self.auto_shift != GaugeAutoShift::Continue
    }

    /// Best gauge that is qualified at the end of play (respecting GAS).
    pub fn final_clear_gauge(&self) -> Option<GaugeType> {
        match self.auto_shift {
            GaugeAutoShift::BestClear | GaugeAutoShift::SelectToUnder => {
                let start = if self.auto_shift == GaugeAutoShift::BestClear {
                    GaugeType::ExHard
                } else {
                    self.selected
                };
                let pos = SHIFT_ORDER.iter().position(|t| *t == start).unwrap_or(0);
                SHIFT_ORDER[pos..]
                    .iter()
                    .copied()
                    .find(|t| self.get(*t).is_qualified())
            }
            _ => self.current().is_qualified().then_some(self.current),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groove_gauge_total_scaling() {
        let g = GrooveGauge::new(
            GaugePreset::Beatoraja,
            RuleFamily::SevenKeys,
            GaugeType::Normal,
            GaugeAutoShift::None,
            300.0,
            1000,
        );
        assert!((g.get(GaugeType::Normal).change_for(Judge::PGreat) - 0.3).abs() < 1e-6);
        assert_eq!(g.get(GaugeType::Normal).change_for(Judge::Poor), -6.0);
        assert_eq!(g.value(), 20.0);
    }

    #[test]
    fn hard_gauge_dies_and_stays_dead() {
        let mut g = GrooveGauge::new(
            GaugePreset::Beatoraja,
            RuleFamily::SevenKeys,
            GaugeType::ExHard,
            GaugeAutoShift::None,
            300.0,
            1000,
        );
        for _ in 0..10 {
            g.update(Judge::Poor, 1.0);
        }
        assert!(g.current().is_dead());
        assert!(g.is_failed_now());
        g.update(Judge::PGreat, 1.0);
        assert_eq!(g.value(), 0.0);
    }

    #[test]
    fn hard_guts_reduce_damage() {
        let mut g = Gauge::new(GaugeType::Hard, &SEVEN[3], 300.0, 1000);
        g.set_value(25.0);
        g.update(Judge::Poor, 1.0);
        assert!((g.value() - (25.0 - 10.0 * 0.6)).abs() < 1e-4);
    }

    #[test]
    fn gauge_auto_shift() {
        let mut g = GrooveGauge::new(
            GaugePreset::Beatoraja,
            RuleFamily::SevenKeys,
            GaugeType::Hard,
            GaugeAutoShift::BestClear,
            300.0,
            1000,
        );
        assert_eq!(g.current_type(), GaugeType::ExHard);
        for _ in 0..7 {
            g.update(Judge::Poor, 1.0);
        }
        assert_eq!(g.current_type(), GaugeType::Hard);
        let mut s = GrooveGauge::new(
            GaugePreset::Beatoraja,
            RuleFamily::SevenKeys,
            GaugeType::Hard,
            GaugeAutoShift::SurvivalToGroove,
            300.0,
            1000,
        );
        for _ in 0..20 {
            s.update(Judge::Poor, 1.0);
        }
        assert_eq!(s.current_type(), GaugeType::Normal);
        assert!(!s.is_failed_now());
    }

    #[test]
    fn lr2_modify_damage_small_chart() {
        let g = Gauge::new(GaugeType::Hard, &LR2[3], 300.0, 250);
        // 250 notes: damage x3 (LR2's note count correction)
        assert!((g.change_for(Judge::Poor) + 30.0).abs() < 1e-4);
        let big = Gauge::new(GaugeType::Hard, &LR2[3], 300.0, 2000);
        assert_eq!(big.change_for(Judge::Poor), -10.0);
        // TOTAL 192 to 207: 10 / 7
        for t in [192.0, 200.0, 207.0] {
            let low = Gauge::new(GaugeType::Hard, &LR2[3], t, 2000);
            assert!((low.change_for(Judge::Poor) + 100.0 / 7.0).abs() < 1e-3);
        }
        assert!(
            (Gauge::new(GaugeType::Hard, &LR2[3], 100.0, 2000).change_for(Judge::Poor) + 100.0)
                .abs()
                < 1e-3
        );
    }

    #[test]
    fn lr2_hazard_is_lr2s_death_gauge() {
        let mut g = Gauge::new(GaugeType::Hazard, &LR2[5], 300.0, 1000);
        for _ in 0..50 {
            g.update(Judge::Miss, 1.0);
        }
        assert_eq!(g.value(), 100.0);
        // no gains; it clears at 80 %, as GROOVE
        g.set_value(50.0);
        g.update(Judge::PGreat, 1.0);
        assert_eq!((g.value(), g.is_qualified()), (50.0, false));
        g.update(Judge::Bad, 1.0);
        assert!(g.is_dead());
        // beatoraja's HAZARD still pays for an empty POOR
        let mut b = Gauge::new(GaugeType::Hazard, &SEVEN[5], 300.0, 1000);
        b.update(Judge::Miss, 1.0);
        assert_eq!(b.value(), 90.0);
    }

    #[test]
    fn lr2_guts_and_death() {
        let mut g = Gauge::new(GaugeType::Hard, &LR2[3], 300.0, 2000);
        g.set_value(31.0);
        g.update(Judge::Poor, 1.0);
        // below 32 %: damage x0.6
        assert!((g.value() - (31.0 - 6.0)).abs() < 1e-4);
        // below 2 % it empties
        g.set_value(3.0);
        assert!(!g.is_dead());
        g.update(Judge::Good, 1.0);
        g.update(Judge::Miss, 1.0);
        assert!(g.is_dead());
        // groove gauges do not
        let mut n = Gauge::new(GaugeType::Normal, &LR2[2], 300.0, 2000);
        n.set_value(3.0);
        n.update(Judge::Bad, 1.0);
        assert!(!n.is_dead());
        // beatoraja's HARD has no such floor
        let mut b = Gauge::new(GaugeType::Hard, &SEVEN[3], 300.0, 2000);
        b.set_value(1.0);
        assert!(!b.is_dead());
    }

    fn lr2(ty: GaugeType, total: f64, notes: usize) -> GrooveGauge {
        GrooveGauge::new(
            GaugePreset::Lr2,
            RuleFamily::SevenKeys,
            ty,
            GaugeAutoShift::None,
            total,
            notes,
        )
    }

    fn changes(g: &GrooveGauge) -> [f32; 6] {
        Judge::ALL.map(|j| g.current().change_for(j))
    }

    fn close(a: [f32; 6], b: [f32; 6]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    /// S-OLR2b 1.2's table: P-ATTACK +0.1, -1, then -100 for the rest;
    /// G-ATTACK -10f, -1, +0.1, -6, -10f, -2f; both from 100.
    #[test]
    fn attack_gauges_as_specified() {
        let p = lr2(GaugeType::PAttack, 300.0, 1000);
        assert_eq!(changes(&p), [0.1, -1.0, -100.0, -100.0, -100.0, -100.0]);
        assert_eq!((p.value(), p.current_type()), (100.0, GaugeType::PAttack));
        // f = 1 (TOTAL 300, 1000 notes); f = 10/7 (TOTAL 200)
        let g = lr2(GaugeType::GAttack, 300.0, 1000);
        assert_eq!(changes(&g), [-10.0, -1.0, 0.1, -6.0, -10.0, -2.0]);
        let f = 10.0 / 7.0;
        let g = lr2(GaugeType::GAttack, 200.0, 1000);
        assert!(close(
            changes(&g),
            [-10.0 * f, -1.0, 0.1, -6.0, -10.0 * f, -2.0 * f]
        ));
        // 250 notes: g(n) = 3
        let g = lr2(GaugeType::GAttack, 300.0, 250);
        assert!(close(changes(&g), [-30.0, -1.0, 0.1, -6.0, -30.0, -6.0]));
        // only the gauge played is added to the set's
        assert_eq!(g.gauges.len(), 10);
        assert_eq!(lr2(GaugeType::Hard, 300.0, 1000).gauges.len(), 9);
    }

    #[test]
    fn p_attack_dies_on_good_or_worse_and_on_empty_poor() {
        for j in [Judge::Good, Judge::Bad, Judge::Poor, Judge::Miss] {
            let mut g = lr2(GaugeType::PAttack, 300.0, 1000);
            g.update(j, 1.0);
            assert!(g.is_failed_now(), "{j:?}");
            // also at a low gauge (x 0.6: -60)
            let mut g = lr2(GaugeType::PAttack, 300.0, 1000);
            g.set_values(&[100.0; 9].into_iter().chain([30.0]).collect::<Vec<_>>());
            g.update(j, 1.0);
            assert!(g.is_failed_now(), "{j:?} at 30");
        }
        // PGREAT and GREAT live; a mine takes its value
        let mut g = lr2(GaugeType::PAttack, 300.0, 1000);
        for _ in 0..50 {
            g.update(Judge::Great, 1.0);
        }
        assert_eq!(g.value(), 50.0);
        g.update(Judge::PGreat, 1.0);
        g.add_value(-40.0);
        assert!((g.value() - 10.1).abs() < 1e-4);
        // a clear is HARD CLEAR
        assert_eq!(
            g.final_clear_gauge().map(|t| t.clear_type()),
            Some(ClearType::Hard)
        );
    }

    #[test]
    fn attack_gauges_low_gauge_rule_and_death() {
        let mut g = lr2(GaugeType::GAttack, 300.0, 1000);
        let set = |g: &mut GrooveGauge, v: f32| {
            let mut vals = vec![100.0; 9];
            vals.push(v);
            g.set_values(&vals);
        };
        // at 30 shown (below 32): BAD, POOR, empty POOR x 0.6; PGREAT and
        // GREAT never
        for (j, after) in [
            (Judge::Bad, 30.0 - 3.6),
            (Judge::Poor, 30.0 - 6.0),
            (Judge::Miss, 30.0 - 1.2),
            (Judge::PGreat, 20.0),
            (Judge::Great, 29.0),
            (Judge::Good, 30.1),
        ] {
            set(&mut g, 30.0);
            g.update(j, 1.0);
            assert!((g.value() - after).abs() < 1e-4, "{j:?}: {}", g.value());
        }
        // empties below 2; exactly 2 lives
        set(&mut g, 3.0);
        g.update(Judge::Great, 1.0);
        assert!(!g.is_failed_now());
        assert_eq!(g.value(), 2.0);
        g.update(Judge::Great, 1.0);
        assert!(g.is_failed_now());
        // no 80 % border: any value it lives at clears
        assert_eq!(g.current().elem.border, 0.0);
    }

    #[test]
    fn attack_gauges_are_the_lr2_sets() {
        assert_eq!(
            GaugePreset::Lr2.selectable()[..6],
            [
                GaugeType::Normal,
                GaugeType::Hard,
                GaugeType::Hazard,
                GaugeType::Easy,
                GaugeType::PAttack,
                GaugeType::GAttack
            ]
        );
        assert!(!GaugePreset::Beatoraja
            .selectable()
            .contains(&GaugeType::PAttack));
        // another set plays an attack gauge as its HARD
        let g = GrooveGauge::new(
            GaugePreset::Beatoraja,
            RuleFamily::SevenKeys,
            GaugeType::GAttack,
            GaugeAutoShift::None,
            300.0,
            1000,
        );
        assert_eq!((g.current_type(), g.gauges.len()), (GaugeType::Hard, 9));
    }

    /// LR2's course gauges (S-OLR2 3.6, S-OLR2b 3.6, 4.1, 5.2).
    #[test]
    fn lr2_course_gauges() {
        let course = |ty, kind| {
            GrooveGauge::new_in(
                GaugePreset::Lr2,
                RuleFamily::SevenKeys,
                ty,
                GaugeAutoShift::None,
                300.0,
                1000,
                Some(kind),
            )
        };
        for kind in [CourseKind::Expert, CourseKind::Nonstop] {
            let g = course(GaugeType::Normal, kind);
            assert_eq!(changes(&g), [0.1, 0.1, 0.04, -1.5, -2.0, -1.5]);
            assert_eq!(g.value(), 100.0);
            let e = course(GaugeType::Easy, kind);
            assert!(close(changes(&e), [0.12, 0.12, 0.048, -1.2, -1.6, -1.2]));
            let h = course(GaugeType::Hard, kind);
            assert_eq!(changes(&h), [0.1, 0.1, 0.05, -6.0, -10.0, -2.0]);
            let p = course(GaugeType::PAttack, kind);
            assert_eq!(changes(&p)[2], -100.0);
        }
        // class: its own gauges, HAZARD as single play
        let c = course(GaugeType::Class, CourseKind::Class);
        assert_eq!(changes(&c), [0.1, 0.1, 0.04, -2.0, -3.0, -2.0]);
        let z = course(GaugeType::Hazard, CourseKind::Class);
        assert_eq!(changes(&z), [0.0, 0.0, 0.0, -100.0, -100.0, 0.0]);
        // every gauge: x 0.6 at 30, no 2 % floor, clears at any value
        let mut g = course(GaugeType::Normal, CourseKind::Expert);
        let mut v = g.values();
        v[GaugeType::Normal as usize] = 30.0;
        g.set_values(&v);
        g.update(Judge::Poor, 1.0);
        assert!((g.value() - 28.8).abs() < 1e-4);
        assert!(g.current().is_qualified());
        v[GaugeType::Normal as usize] = 3.0;
        g.set_values(&v);
        g.update(Judge::Poor, 1.0);
        assert!(g.is_failed_now());
        // a mine still stops GROOVE at 2 %, but empties HARD
        let mut g = course(GaugeType::Normal, CourseKind::Nonstop);
        g.add_value(-1295.0);
        assert_eq!(g.value(), 2.0);
        assert!(g.get(GaugeType::Hard).is_dead());
        let mut c = course(GaugeType::Class, CourseKind::Class);
        c.add_value(-1295.0);
        assert_eq!(c.value(), 2.0);
        // the other sets: as without a course
        let b = GrooveGauge::new_in(
            GaugePreset::Beatoraja,
            RuleFamily::SevenKeys,
            GaugeType::Class,
            GaugeAutoShift::None,
            300.0,
            1000,
            Some(CourseKind::Expert),
        );
        assert_eq!(
            b,
            GrooveGauge::new(
                GaugePreset::Beatoraja,
                RuleFamily::SevenKeys,
                GaugeType::Class,
                GaugeAutoShift::None,
                300.0,
                1000,
            )
        );
    }
}
