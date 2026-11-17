//! Groove gauges.
//!
//! All gauge types are simulated in parallel (like beatoraja) so that
//! GAUGE AUTO SHIFT can switch between them and the best clear can be
//! determined after play.
//!
//! Gauge change tables are ordered PG, GR, GD, BD, POOR, empty POOR.

use crate::mode::RuleFamily;
use crate::score::ClearType;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord, Default,
)]
pub enum GaugeType {
    AssistEasy = 0,
    Easy = 1,
    #[default]
    Normal = 2,
    Hard = 3,
    ExHard = 4,
    Hazard = 5,
    /// Dan (段位) gauges.
    Class = 6,
    ExClass = 7,
    ExHardClass = 8,
}

impl GaugeType {
    pub const ALL: [GaugeType; 9] = [
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
    /// Gauges selectable for normal play.
    pub const SELECTABLE: [GaugeType; 6] = [
        GaugeType::AssistEasy,
        GaugeType::Easy,
        GaugeType::Normal,
        GaugeType::Hard,
        GaugeType::ExHard,
        GaugeType::Hazard,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GaugeType::AssistEasy => "ASSIST EASY",
            GaugeType::Easy => "EASY",
            GaugeType::Normal => "NORMAL",
            GaugeType::Hard => "HARD",
            GaugeType::ExHard => "EX-HARD",
            GaugeType::Hazard => "HAZARD",
            GaugeType::Class => "CLASS",
            GaugeType::ExClass => "EX-CLASS",
            GaugeType::ExHardClass => "EXHARD-CLASS",
        }
    }

    pub fn is_survival(self) -> bool {
        !matches!(
            self,
            GaugeType::AssistEasy | GaugeType::Easy | GaugeType::Normal
        )
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
            GaugeType::Hard | GaugeType::ExClass => ClearType::Hard,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GaugePreset {
    /// beatoraja gauges for the chart's key mode.
    #[default]
    Beatoraja,
    /// beatoraja's LR2-compatible gauge set.
    Lr2,
    /// IIDX-like gauges (approximation).
    Iidx,
}

impl GaugePreset {
    pub const ALL: [GaugePreset; 3] = [GaugePreset::Beatoraja, GaugePreset::Lr2, GaugePreset::Iidx];
    pub fn label(self) -> &'static str {
        match self {
            GaugePreset::Beatoraja => "beatoraja",
            GaugePreset::Lr2 => "LR2",
            GaugePreset::Iidx => "IIDX",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GaugeAutoShift {
    #[default]
    None,
    /// Keep playing after the gauge empties (result is FAILED).
    Continue,
    /// HARD/EX-HARD drop to NORMAL when emptied.
    SurvivalToGroove,
    /// Start at EX-HARD and step down, keeping the best clear.
    BestClear,
    /// Start at the selected gauge and step down.
    SelectToUnder,
}

impl GaugeAutoShift {
    pub const ALL: [GaugeAutoShift; 5] = [
        GaugeAutoShift::None,
        GaugeAutoShift::Continue,
        GaugeAutoShift::SurvivalToGroove,
        GaugeAutoShift::BestClear,
        GaugeAutoShift::SelectToUnder,
    ];
    pub fn label(self) -> &'static str {
        match self {
            GaugeAutoShift::None => "OFF",
            GaugeAutoShift::Continue => "CONTINUE",
            GaugeAutoShift::SurvivalToGroove => "HARD→GROOVE",
            GaugeAutoShift::BestClear => "BEST CLEAR",
            GaugeAutoShift::SelectToUnder => "SELECT TO UNDER",
        }
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
    /// LR2's damage increase (as LR2oraja): by TOTAL below 240 and by note
    /// count below 1000, the larger.
    Lr2Damage,
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
    }
}

/// An LR2 survival gauge: it empties below 2 %.
const fn lr2(g: GaugeElement) -> GaugeElement {
    GaugeElement { death: 2.0, ..g }
}

use Modifier::*;
const HARD_GUTS: &[(f32, f32)] = &[
    (10.0, 0.4),
    (20.0, 0.5),
    (30.0, 0.6),
    (40.0, 0.7),
    (50.0, 0.8),
];
const CLASS_GUTS: &[(f32, f32)] = &[
    (5.0, 0.4),
    (10.0, 0.5),
    (15.0, 0.6),
    (20.0, 0.7),
    (25.0, 0.8),
];
const LR2_GUTS: &[(f32, f32)] = &[(32.0, 0.6)];
const IIDX_GUTS: &[(f32, f32)] = &[(30.0, 0.5)];

// beatoraja GaugeProperty values
const FIVE: [GaugeElement; 9] = [
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        50.0,
        [1.0, 1.0, 0.5, -1.5, -3.0, -0.5],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        75.0,
        [1.0, 1.0, 0.5, -1.5, -4.5, -1.0],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        75.0,
        [1.0, 1.0, 0.5, -3.0, -6.0, -2.0],
        &[],
    ),
    e(
        Some(LimitIncrement),
        0.0,
        100.0,
        100.0,
        0.0,
        [0.0, 0.0, 0.0, -5.0, -10.0, -5.0],
        &[],
    ),
    e(
        Some(ModifyDamage),
        0.0,
        100.0,
        100.0,
        0.0,
        [0.0, 0.0, 0.0, -10.0, -20.0, -10.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.0, 0.0, 0.0, -100.0, -100.0, -100.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.01, 0.01, 0.0, -0.5, -1.0, -0.5],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.01, 0.01, 0.0, -1.0, -2.0, -1.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.01, 0.01, 0.0, -2.5, -5.0, -2.5],
        &[],
    ),
];
const SEVEN: [GaugeElement; 9] = [
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        60.0,
        [1.0, 1.0, 0.5, -1.5, -3.0, -0.5],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        80.0,
        [1.0, 1.0, 0.5, -1.5, -4.5, -1.0],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        80.0,
        [1.0, 1.0, 0.5, -3.0, -6.0, -2.0],
        &[],
    ),
    e(
        Some(LimitIncrement),
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.12, 0.03, -5.0, -10.0, -5.0],
        HARD_GUTS,
    ),
    e(
        Some(LimitIncrement),
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.06, 0.0, -8.0, -16.0, -8.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.06, 0.0, -100.0, -100.0, -10.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.12, 0.06, -1.5, -3.0, -1.5],
        CLASS_GUTS,
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.12, 0.03, -3.0, -6.0, -3.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.06, 0.0, -5.0, -10.0, -5.0],
        &[],
    ),
];
const PMS: [GaugeElement; 9] = [
    e(
        Some(Total),
        2.0,
        120.0,
        30.0,
        65.0,
        [1.0, 1.0, 0.5, -1.0, -2.0, -2.0],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        120.0,
        30.0,
        85.0,
        [1.0, 1.0, 0.5, -1.0, -3.0, -3.0],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        120.0,
        30.0,
        85.0,
        [1.0, 1.0, 0.5, -2.0, -6.0, -6.0],
        &[],
    ),
    e(
        Some(LimitIncrement),
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.12, 0.03, -5.0, -10.0, -10.0],
        HARD_GUTS,
    ),
    e(
        Some(LimitIncrement),
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.06, 0.0, -10.0, -15.0, -15.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.06, 0.0, -100.0, -100.0, -100.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.12, 0.06, -1.5, -3.0, -3.0],
        CLASS_GUTS,
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.12, 0.03, -3.0, -6.0, -6.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.06, 0.0, -5.0, -10.0, -10.0],
        &[],
    ),
];
const LR2: [GaugeElement; 9] = [
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        60.0,
        [1.2, 1.2, 0.6, -3.2, -4.8, -1.6],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        80.0,
        [1.2, 1.2, 0.6, -3.2, -4.8, -1.6],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        100.0,
        20.0,
        80.0,
        [1.0, 1.0, 0.5, -4.0, -6.0, -2.0],
        &[],
    ),
    lr2(e(
        Some(Lr2Damage),
        0.0,
        100.0,
        100.0,
        0.0,
        [0.1, 0.1, 0.05, -6.0, -10.0, -2.0],
        LR2_GUTS,
    )),
    lr2(e(
        Some(Lr2Damage),
        0.0,
        100.0,
        100.0,
        0.0,
        [0.1, 0.1, 0.05, -12.0, -20.0, -2.0],
        &[],
    )),
    lr2(e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.15, 0.06, 0.0, -100.0, -100.0, -10.0],
        &[],
    )),
    lr2(e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.10, 0.10, 0.05, -2.0, -3.0, -2.0],
        LR2_GUTS,
    )),
    lr2(e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.10, 0.10, 0.05, -6.0, -10.0, -2.0],
        LR2_GUTS,
    )),
    lr2(e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.10, 0.10, 0.05, -12.0, -20.0, -2.0],
        &[],
    )),
];
// IIDX-like approximation (groove gauges start at 22%, HARD damage halves below 30%)
const IIDX: [GaugeElement; 9] = [
    e(
        Some(Total),
        2.0,
        100.0,
        22.0,
        60.0,
        [1.0, 1.0, 0.5, -1.6, -4.8, -1.6],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        100.0,
        22.0,
        80.0,
        [1.0, 1.0, 0.5, -1.6, -4.8, -1.6],
        &[],
    ),
    e(
        Some(Total),
        2.0,
        100.0,
        22.0,
        80.0,
        [1.0, 1.0, 0.5, -2.0, -6.0, -2.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.16, 0.16, 0.0, -5.0, -9.0, -5.0],
        IIDX_GUTS,
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.16, 0.16, 0.0, -10.0, -18.0, -10.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.16, 0.16, 0.0, -100.0, -100.0, -10.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.16, 0.16, 0.04, -1.5, -2.5, -1.5],
        IIDX_GUTS,
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.16, 0.16, 0.0, -3.0, -6.0, -3.0],
        &[],
    ),
    e(
        None,
        0.0,
        100.0,
        100.0,
        0.0,
        [0.16, 0.16, 0.0, -5.0, -10.0, -5.0],
        &[],
    ),
];

pub fn gauge_elements(preset: GaugePreset, fam: RuleFamily) -> &'static [GaugeElement; 9] {
    match (preset, fam) {
        (GaugePreset::Lr2, _) => &LR2,
        (GaugePreset::Iidx, _) => &IIDX,
        (_, RuleFamily::FiveKeys) => &FIVE,
        (_, RuleFamily::SevenKeys) => &SEVEN,
        (_, RuleFamily::Pms) => &PMS,
    }
}

fn modify(m: Modifier, f: f32, total: f64, notes: usize) -> f32 {
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

/// Damage multiplier of the beatoraja-preset five-key EX-HARD gauge: the
/// larger of a TOTAL step factor and a note-count factor (single precision).
fn five_key_damage_factor(total: f64, notes: usize) -> f32 {
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
    let by_total = if total.is_nan() {
        1.0
    } else {
        TOTAL_STEPS
            .iter()
            .find(|(t, _)| total >= *t)
            .map_or(10.0, |(_, f)| *f)
    };

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
    by_total.max(by_notes)
}

/// Damage multiplier of the LR2-preset HARD and EX-HARD gauges: the larger
/// of a TOTAL factor and a note-count factor, computed in double precision
/// and rounded to single precision.
fn lr2_damage_factor(total: f64, n: f64) -> f32 {
    let by_total = if total.is_nan() {
        1.0
    } else {
        let k = ((total / 16.0).floor() - 5.0).clamp(1.0, 10.0);
        10.0 / k
    };
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
    f64::max(by_total, by_notes) as f32
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
            for v in values.iter_mut() {
                *v = modify(m, *v, total, notes);
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

    pub fn update(&mut self, judge: usize, rate: f32) {
        let mut inc = self.values[judge] * rate;
        if inc < 0.0 {
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

    pub fn change_for(&self, judge: usize) -> f32 {
        self.values[judge]
    }
}

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
        let elems = gauge_elements(preset, fam);
        let gauges = GaugeType::ALL
            .iter()
            .zip(elems.iter())
            .map(|(t, e)| Gauge::new(*t, e, total, notes))
            .collect();
        let current = match gas {
            GaugeAutoShift::BestClear if !selected.is_class() => GaugeType::ExHard,
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

    pub fn current(&self) -> &Gauge {
        &self.gauges[self.current as usize]
    }

    pub fn get(&self, t: GaugeType) -> &Gauge {
        &self.gauges[t as usize]
    }

    pub fn value(&self) -> f32 {
        self.current().value()
    }

    pub fn update(&mut self, judge: usize, rate: f32) {
        for g in &mut self.gauges {
            g.update(judge, rate);
        }
        self.shift();
    }

    pub fn add_value(&mut self, v: f32) {
        for g in &mut self.gauges {
            let nv = g.value() + v;
            g.set_value(nv);
        }
        self.shift();
    }

    /// Restore values (e.g. carrying gauges across a course).
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
                let order = [
                    GaugeType::Hazard,
                    GaugeType::ExHard,
                    GaugeType::Hard,
                    GaugeType::Normal,
                    GaugeType::Easy,
                    GaugeType::AssistEasy,
                ];
                if let Some(pos) = order.iter().position(|t| *t == self.current) {
                    if let Some(t) = order[pos + 1..].iter().find(|t| !self.get(**t).is_dead()) {
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
                let order = [
                    GaugeType::Hazard,
                    GaugeType::ExHard,
                    GaugeType::Hard,
                    GaugeType::Normal,
                    GaugeType::Easy,
                    GaugeType::AssistEasy,
                ];
                let pos = order.iter().position(|t| *t == start).unwrap_or(0);
                order[pos..]
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
    use crate::judge::*;

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
        assert!((g.get(GaugeType::Normal).change_for(PGREAT) - 0.3).abs() < 1e-6);
        assert_eq!(g.get(GaugeType::Normal).change_for(POOR), -6.0);
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
            g.update(POOR, 1.0);
        }
        assert!(g.current().is_dead());
        assert!(g.is_failed_now());
        g.update(PGREAT, 1.0);
        assert_eq!(g.value(), 0.0);
    }

    #[test]
    fn hard_guts_reduce_damage() {
        let mut g = Gauge::new(GaugeType::Hard, &SEVEN[3], 300.0, 1000);
        g.set_value(25.0);
        g.update(POOR, 1.0);
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
            g.update(POOR, 1.0);
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
            s.update(POOR, 1.0);
        }
        assert_eq!(s.current_type(), GaugeType::Normal);
        assert!(!s.is_failed_now());
    }

    #[test]
    fn lr2_modify_damage_small_chart() {
        let g = Gauge::new(GaugeType::Hard, &LR2[3], 300.0, 250);
        // 250 notes: damage x3 (LR2's note count correction)
        assert!((g.change_for(POOR) + 30.0).abs() < 1e-4);
        let big = Gauge::new(GaugeType::Hard, &LR2[3], 300.0, 2000);
        assert_eq!(big.change_for(POOR), -10.0);
        // TOTAL 200: floor(200 / 16) - 5 = 7 -> x10/7
        let low = Gauge::new(GaugeType::Hard, &LR2[3], 200.0, 2000);
        assert!((low.change_for(POOR) + 100.0 / 7.0).abs() < 1e-3);
        assert!(
            (Gauge::new(GaugeType::Hard, &LR2[3], 100.0, 2000).change_for(POOR) + 100.0).abs()
                < 1e-3
        );
    }

    #[test]
    fn lr2_guts_and_death() {
        let mut g = Gauge::new(GaugeType::Hard, &LR2[3], 300.0, 2000);
        g.set_value(31.0);
        g.update(POOR, 1.0);
        // below 32 %: damage x0.6
        assert!((g.value() - (31.0 - 6.0)).abs() < 1e-4);
        // below 2 % it empties
        g.set_value(3.0);
        assert!(!g.is_dead());
        g.update(GOOD, 1.0);
        g.update(MISS, 1.0);
        assert!(g.is_dead());
        // groove gauges do not
        let mut n = Gauge::new(GaugeType::Normal, &LR2[2], 300.0, 2000);
        n.set_value(3.0);
        n.update(BAD, 1.0);
        assert!(!n.is_dead());
        // beatoraja's HARD has no such floor
        let mut b = Gauge::new(GaugeType::Hard, &SEVEN[3], 300.0, 2000);
        b.set_value(1.0);
        assert!(!b.is_dead());
    }
}
