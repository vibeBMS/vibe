//! Rule profiles: the rule sets a ranking can trust. A profile fixes every
//! rule that decides a score at once (judge windows, gauge set and the
//! gauges of it, LN modes, how untyped long notes play, the note choice,
//! the judge rank forms, GAUGE AUTO SHIFT); combo and empty POOR rules come with the judge
//! preset and the key mode (`judge::judge_rules`), so a judge left as its
//! preset has them. A play whose rules match one exactly is that profile's;
//! any other mix of options is local.
//!
//! The profiles are vibeBMS's rules. How far each is the game it is named
//! after is the equivalence list below: a play an open gap reaches is not
//! sent to a third-party IR.

use serde::{Deserialize, Serialize};
use vibebms_core::chart::{JudgeRank, LnType};
use vibebms_core::engine::JudgeAlgorithm;
use vibebms_core::gauge::{lr2_hard_damage_unconfirmed, GaugeAutoShift, GaugePreset, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::mode::RuleFamily;
use vibebms_core::record::{ChartFacts, PlayRecord, RuleFacts};
use vibebms_core::PlayMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Profile {
    #[serde(rename = "vibe-lr2")]
    VibeLr2,
    #[serde(rename = "vibe-beatoraja")]
    VibeBeatoraja,
}

/// The exact rules of a profile. The judge windows are always the
/// preset's own for the chart (no EXPAND JUDGE).
#[derive(Debug)]
pub struct ProfileRules {
    pub judge: JudgePreset,
    pub gauge_set: GaugePreset,
    /// The gauges of the set a play may start on.
    pub gauges: &'static [GaugeType],
    /// The LN mode untyped long notes are played as (a chart without them
    /// plays the same in every mode).
    pub ln_modes: &'static [LnType],
    /// Charts that type their long notes CN or HCN (`#LNMODE`, bmson `t`)
    /// are played so.
    pub typed_charge_notes: bool,
    /// The judge rank may be `#DEFEXRANK` or bmson's (else `#RANK` only).
    pub ex_rank: bool,
    pub algorithms: &'static [JudgeAlgorithm],
    pub gauge_auto_shift: &'static [GaugeAutoShift],
    pub modes: &'static [PlayMode],
}

const BMS_MODES: &[PlayMode] = &[
    PlayMode::Beat5K,
    PlayMode::Beat7K,
    PlayMode::Beat10K,
    PlayMode::Beat14K,
];

const ALGORITHMS: &[JudgeAlgorithm] = &[
    JudgeAlgorithm::Combo,
    JudgeAlgorithm::Duration,
    JudgeAlgorithm::Lowest,
];

/// LR2: its judge by `#RANK` and its gauges, P-ATTACK and G-ATTACK and the
/// class course gauges among them (the set's ASSIST EASY, EX-HARD and
/// EXHARD-CLASS are not LR2's); every long note an LN (LR2
/// has no CN or HCN, nor `#DEFEXRANK`); any note choice option, which the
/// LR2 judge does not take (it has LR2's own); no GAUGE AUTO SHIFT (LR2
/// has none); no PMS (left out: LR2 plays it by the same rules, S-OLR2 8,
/// but no IR here takes it).
const VIBE_LR2: ProfileRules = ProfileRules {
    judge: JudgePreset::Lr2,
    gauge_set: GaugePreset::Lr2,
    gauges: &[
        GaugeType::Easy,
        GaugeType::Normal,
        GaugeType::Hard,
        GaugeType::Hazard,
        GaugeType::PAttack,
        GaugeType::GAttack,
        GaugeType::Class,
        GaugeType::ExClass,
    ],
    ln_modes: &[LnType::Ln],
    typed_charge_notes: false,
    ex_rank: false,
    algorithms: ALGORITHMS,
    gauge_auto_shift: &[GaugeAutoShift::None],
    modes: BMS_MODES,
};

/// beatoraja: its windows and gauges per key mode, every LN mode, typed
/// long notes as typed, every judge rank form, note choice and GAUGE AUTO
/// SHIFT (player options in beatoraja too), every key mode.
const VIBE_BEATORAJA: ProfileRules = ProfileRules {
    judge: JudgePreset::Beatoraja,
    gauge_set: GaugePreset::Beatoraja,
    gauges: &GaugeType::STANDARD,
    ln_modes: &[LnType::Ln, LnType::Cn, LnType::Hcn],
    typed_charge_notes: true,
    ex_rank: true,
    algorithms: ALGORITHMS,
    gauge_auto_shift: &GaugeAutoShift::ALL,
    modes: &PlayMode::ALL,
};

impl Profile {
    pub const ALL: [Profile; 2] = [Profile::VibeLr2, Profile::VibeBeatoraja];

    /// Shown to the player.
    pub fn label(self) -> &'static str {
        match self {
            Profile::VibeLr2 => "vibe-LR2",
            Profile::VibeBeatoraja => "vibe-beatoraja",
        }
    }

    /// Sent to an IR as the rule kind.
    pub fn wire(self) -> &'static str {
        match self {
            Profile::VibeLr2 => "vibe-lr2",
            Profile::VibeBeatoraja => "vibe-beatoraja",
        }
    }

    pub fn rules(self) -> &'static ProfileRules {
        match self {
            Profile::VibeLr2 => &VIBE_LR2,
            Profile::VibeBeatoraja => &VIBE_BEATORAJA,
        }
    }

    /// The profile's gaps of the equivalence list still open.
    pub fn open_gaps(self) -> impl Iterator<Item = &'static Gap> {
        EQUIVALENCE
            .iter()
            .filter(move |g| g.profile == self && !g.closed)
    }
}

impl ProfileRules {
    /// The rules `r` on chart `c` are exactly these.
    pub fn matches(&self, r: &RuleFacts, c: &ChartFacts) -> bool {
        let typed_charge = c.ln.typed[LnType::Cn as usize] || c.ln.typed[LnType::Hcn as usize];
        r.judge == self.judge
            && r.judge_rates.is_none()
            && r.gauge_set == self.gauge_set
            && self.gauges.contains(&r.gauge)
            && (!c.ln.undefined || self.ln_modes.contains(&r.ln_mode))
            && (self.typed_charge_notes || !typed_charge)
            && (self.ex_rank || matches!(c.judge_rank, JudgeRank::Rank(_) | JudgeRank::NoNumber))
            && self.algorithms.contains(&r.algorithm)
            && self.gauge_auto_shift.contains(&r.gas)
            && self.modes.contains(&c.mode)
    }
}

/// The profile a play's rules are, `None`: local rules.
pub fn profile_of(rec: &PlayRecord) -> Option<Profile> {
    Profile::ALL
        .into_iter()
        .find(|p| p.rules().matches(&rec.rules, &rec.chart))
}

/// A known or possible difference between a profile and the game it is
/// named after.
#[derive(Debug)]
pub struct Gap {
    pub id: &'static str,
    pub profile: Profile,
    pub what: &'static str,
    /// Shown equivalent (golden vectors with their origin recorded).
    pub closed: bool,
    pub reaches: Reach,
    /// Why a play it reaches is not sent, for the player.
    pub reason: &'static str,
}

/// The gap of the equivalence list with id `id`.
pub fn gap(id: &str) -> Option<&'static Gap> {
    EQUIVALENCE.iter().find(|g| g.id == id)
}

/// The plays a gap can change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    Every,
    /// HARD where the note-count factor of its damage is unchecked and
    /// the larger (`lr2_hard_damage_unconfirmed`).
    HardFewNotes,
    /// 5/10 keys where the EX-HARD gauge can decide the lamp.
    FiveKeyExHard,
    /// A back-spin scratch played as CN or HCN.
    ChargeBss,
}

impl Reach {
    pub fn hits(self, rec: &PlayRecord) -> bool {
        let r = &rec.rules;
        match self {
            Reach::Every => true,
            Reach::HardFewNotes => {
                r.gauge == GaugeType::Hard
                    && lr2_hard_damage_unconfirmed(rec.chart.total, rec.result.score.total_notes)
            }
            Reach::FiveKeyExHard => {
                rec.chart.mode.family() == RuleFamily::FiveKeys
                    && match r.gas {
                        GaugeAutoShift::BestClear => true,
                        GaugeAutoShift::SelectToUnder => r.gauge >= GaugeType::ExHard,
                        _ => r.gauge == GaugeType::ExHard,
                    }
            }
            Reach::ChargeBss => r.charge_bss,
        }
    }
}

/// The open gap that reaches `rec` under `p`, if any.
pub fn open_gap(p: Profile, rec: &PlayRecord) -> Option<&'static Gap> {
    p.open_gaps().find(|g| g.reaches.hits(rec))
}

/// Each difference the IR design found (its section 2.4), by profile. A gap
/// closes (`closed: true`) when a source establishes the rule
/// and the engine follows it, or the profile leaves the case out; the
/// sources and each gap's state are in `docs/RULES.md`.
pub const EQUIVALENCE: &[Gap] = &[
    Gap {
        id: "A1",
        profile: Profile::VibeLr2,
        what: "judge and gauge presets were separate settings; the profile now requires both",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A2",
        profile: Profile::VibeLr2,
        what: "empty POOR window (up to 1000 ms early, never late) and its combo and note rules",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A3",
        profile: Profile::VibeLr2,
        what: "#RANK 4, out-of-range and non-numeric #RANK: LR2 reads the number's digits, 0 \
               when there are none, and plays all but 0, 1, 3 NORMAL (S-OLR2); #DEFEXRANK and \
               bmson are left out by the profile",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A4",
        profile: Profile::VibeLr2,
        what: "HARD damage note-count factor on 21 to 29 notes: S-OLR2 marks its segment \
               unchecked (tools/lr2-measure's 29- and 30-note charts settle it)",
        closed: false,
        reaches: Reach::HardFewNotes,
        reason: "LR2's HARD damage for a chart with this few notes isn't confirmed",
    },
    Gap {
        id: "A5",
        profile: Profile::VibeLr2,
        what: "LN mode and typed CN/HCN (the profile takes LN and charts without typed CN/HCN)",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A6",
        profile: Profile::VibeLr2,
        what: "combo, note consumption and one-POOR rules: LR2's in every key mode",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A7",
        profile: Profile::VibeLr2,
        what: "#TOTAL as an integer; missing or below 1, floor(160 + 0.16 (n + clamp(n - 400, \
               0, 200))) (S-OLR2)",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A8",
        profile: Profile::VibeLr2,
        what: "mines: the value as percent on every gauge, held as it passes or pressed up to \
               GREAT early, one POOR (S-OLR2, S14); scratch LN ends are as keys'",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A9",
        profile: Profile::VibeLr2,
        what: "origin of the LR2 judge and gauge golden vectors not recorded",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A10",
        profile: Profile::VibeLr2,
        what: "the LR2 gauge set offers ASSIST EASY, EX-HARD and EXHARD-CLASS, which LR2 does \
               not have: the profile takes none of them",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "A11",
        profile: Profile::VibeLr2,
        what: "the note a press takes: the lane's earliest unresolved object, then the BAD \
               chain (S-OLR2; tools/lr2-measure checks it)",
        closed: true,
        reaches: Reach::Every,
        reason: "",
    },
    Gap {
        id: "B1",
        profile: Profile::VibeBeatoraja,
        what: "5-key EX-HARD damage factor keeps only the positive part of the note-count term",
        closed: false,
        reaches: Reach::FiveKeyExHard,
        reason: "beatoraja's 5-key EX-HARD damage isn't confirmed",
    },
    Gap {
        id: "B3",
        profile: Profile::VibeBeatoraja,
        what: "a key release ends a back-spin scratch CN/HCN (vibeBMS's keyboard rule)",
        closed: false,
        reaches: Reach::ChargeBss,
        reason: "how beatoraja ends a back-spin scratch played on keys isn't confirmed",
    },
];

/// The rules line of a play: its profile, else "local", and the rules.
pub fn describe(rec: &PlayRecord) -> String {
    let r = &rec.rules;
    let profile = profile_of(rec).map_or("local rules", Profile::label);
    let ln = match r.ln_mode {
        LnType::Ln => "LN",
        LnType::Cn => "CN",
        LnType::Hcn => "HCN",
    };
    let mut s = format!(
        "{profile}: judge {} / gauge {} / {ln}",
        r.judge.label(),
        r.gauge_set.label()
    );
    if let Some([pg, gr, gd]) = r.judge_rates {
        s += &format!(" / windows {pg}-{gr}-{gd} %");
    }
    s
}
