//! Score bookkeeping: EX score, combo, clear lamp, DJ level.

use crate::judge::Judge;
use serde::{Deserialize, Serialize};

/// Options that help the player (beatoraja assist levels): the lamp is
/// LIGHT ASSIST EASY or ASSIST EASY whatever the gauge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Assist {
    #[default]
    None,
    Light,
    Full,
}

/// What a play's options did, which decides its assist level (as
/// beatoraja's `BMSPlayer`): an option counts only when it took effect.
/// An assisted play keeps only its lamp (ASSIST / L-ASSIST) and its play
/// count: no best scores, no best replay, no IR.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AssistFacts {
    /// The lane options applied, per side.
    pub lane: Vec<crate::lane_option::LaneOption>,
    /// AUTO SCRATCH played scored scratch notes.
    pub auto_scratch: bool,
    /// LEGACY NOTE turned long notes into notes.
    pub legacy_note: bool,
    /// NO MINE removed mines.
    pub no_mine: bool,
    /// BATTLE (a single play chart on both sides).
    pub battle: bool,
    /// Judge windows wider than the rules' (EXPAND JUDGE above 100 %).
    pub wide_judge: bool,
    /// BPM guide on a chart that changes BPM.
    pub bpm_guide: bool,
    /// SCROLL REMOVE took `#SCROLL` / `#SPEED` off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scroll_remove: bool,
    /// MINE ADD added mines.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mine_add: bool,
    /// LONGNOTE ADD made long notes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ln_add: bool,
    /// KEY CONVERT played the chart in another key mode: the chart's own
    /// mode (such a play is not saved and is sent to no IR).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_convert: Option<crate::PlayMode>,
}

impl AssistFacts {
    pub fn level(&self) -> Assist {
        // (LONGNOTE ADD as LEGACY NOTE, MINE ADD as NO MINE, SCROLL
        // REMOVE as BPM GUIDE: the options they undo or stand beside)
        let full = self.auto_scratch || self.legacy_note || self.wide_judge || self.ln_add;
        let light = self.no_mine
            || self.mine_add
            || self.scroll_remove
            || self.battle
            || self.bpm_guide
            || self.lane.iter().any(|o| o.assist() != Assist::None);
        if full {
            Assist::Full
        } else if light {
            Assist::Light
        } else {
            Assist::None
        }
    }
}

crate::choices! {
    #[derive(
        Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord, Default,
    )]
    pub enum ClearType {
        #[default]
        NoPlay = 0 => "NO PLAY",
        Failed = 1 => "FAILED",
        AssistEasy = 2 => "ASSIST",
        LightAssistEasy = 3 => "L-ASSIST",
        Easy = 4 => "EASY",
        Normal = 5 => "CLEAR",
        Hard = 6 => "HARD",
        ExHard = 7 => "EX-HARD",
        FullCombo = 8 => "FULL COMBO",
        Perfect = 9 => "PERFECT",
        Max = 10 => "MAX",
    }
}

impl ClearType {
    /// The lamp of id `id` (its discriminant), clamped to the known ones.
    pub fn from_id(id: i64) -> ClearType {
        ClearType::ALL[id.clamp(0, ClearType::ALL.len() as i64 - 1) as usize]
    }
}

crate::choices! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
    pub enum DjLevel {
        F => "F",
        E => "E",
        D => "D",
        C => "C",
        B => "B",
        A => "A",
        AA => "AA",
        AAA => "AAA",
    }
}

impl DjLevel {
    pub fn from_rate(ex: u32, max: u32) -> DjLevel {
        if max == 0 {
            return DjLevel::F;
        }
        // thresholds at n/9 of the max EX score
        let r = ex as u64 * 9;
        let m = max as u64;
        match () {
            _ if r >= m * 8 => DjLevel::AAA,
            _ if r >= m * 7 => DjLevel::AA,
            _ if r >= m * 6 => DjLevel::A,
            _ if r >= m * 5 => DjLevel::B,
            _ if r >= m * 4 => DjLevel::C,
            _ if r >= m * 3 => DjLevel::D,
            _ if r >= m * 2 => DjLevel::E,
            _ => DjLevel::F,
        }
    }
}

/// Judgement counts and derived values for one play.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Score {
    /// `[judge][0 = early/FAST, 1 = late/SLOW]`
    pub counts: [[u32; 2]; 6],
    pub max_combo: u32,
    /// Total number of judgements in the chart.
    pub total_notes: u32,
    /// Judgements processed so far.
    pub passed: u32,
    /// Judgements that broke the combo (CB).
    #[serde(default)]
    pub combo_breaks: u32,
    /// Judgements hit with zero timing error (neither FAST nor SLOW).
    #[serde(default)]
    pub just: [u32; 6],
}

impl Score {
    /// Record a judgement; `diff` > 0 is early (FAST), < 0 late (SLOW) and
    /// exactly 0 counts as neither.
    pub fn add(&mut self, judge: Judge, diff: i64) {
        let j = judge as usize;
        match diff.signum() {
            1 => self.counts[j][0] += 1,
            -1 => self.counts[j][1] += 1,
            _ => self.just[j] += 1,
        }
    }

    pub fn count(&self, judge: Judge) -> u32 {
        let j = judge as usize;
        self.counts[j][0] + self.counts[j][1] + self.just[j]
    }

    /// GREAT to BAD hit early (a PGREAT is neither FAST nor SLOW).
    pub fn fast(&self) -> u32 {
        self.great_to_bad(0)
    }

    pub fn slow(&self) -> u32 {
        self.great_to_bad(1)
    }

    /// GREAT to BAD counted early (`side` 0) or late (1).
    fn great_to_bad(&self, side: usize) -> u32 {
        [Judge::Great, Judge::Good, Judge::Bad]
            .into_iter()
            .map(|j| self.counts[j as usize][side])
            .sum()
    }

    pub fn ex_score(&self) -> u32 {
        Judge::ALL.into_iter().map(|j| self.count(j) * j.ex()).sum()
    }

    pub fn max_ex(&self) -> u32 {
        self.total_notes * 2
    }

    pub fn rate(&self) -> f64 {
        if self.total_notes == 0 {
            0.0
        } else {
            self.ex_score() as f64 / self.max_ex() as f64
        }
    }

    /// EX rate over the notes processed so far (for the "current rate" display).
    pub fn current_rate(&self) -> f64 {
        if self.passed == 0 {
            0.0
        } else {
            self.ex_score() as f64 / (self.passed as f64 * 2.0)
        }
    }

    /// Miss count (BAD + POOR + empty POOR + notes never reached).
    pub fn miss_count(&self) -> u32 {
        self.count(Judge::Bad)
            + self.count(Judge::Poor)
            + self.count(Judge::Miss)
            + self.total_notes.saturating_sub(self.passed)
    }

    pub fn dj_level(&self) -> DjLevel {
        DjLevel::from_rate(self.ex_score(), self.max_ex())
    }

    /// Lamp given the gauge outcome. `gauge_clear` is the lamp of the
    /// qualified gauge, or `None` when failed.
    pub fn clear_type(&self, gauge_clear: Option<ClearType>, assist: Assist) -> ClearType {
        let Some(gc) = gauge_clear else {
            return ClearType::Failed;
        };
        match assist {
            Assist::Full => return ClearType::AssistEasy,
            Assist::Light => return ClearType::LightAssistEasy,
            Assist::None => {}
        }
        let all = self.passed >= self.total_notes && self.total_notes > 0;
        if all && self.count(Judge::Bad) + self.count(Judge::Poor) == 0 {
            if self.count(Judge::Good) == 0 {
                if self.count(Judge::Great) == 0 {
                    return ClearType::Max;
                }
                return ClearType::Perfect;
            }
            return gc.max(ClearType::FullCombo);
        }
        gc
    }

    /// LR2's lamp (`docs/LR2_BEHAVIOUR.md`, 7.2), one of its five: FAILED
    /// when the gauge emptied (`died`); else FULL COMBO when the max combo
    /// is the note count, whatever the gauge ends at (empty POORs and
    /// mines keep the combo); else the qualified gauge's (`gauge_clear`,
    /// HAZARD's a CLEAR), or FAILED. Any assist caps it at EASY.
    pub fn lr2_clear_type(
        &self,
        died: bool,
        gauge_clear: Option<ClearType>,
        assist: Assist,
    ) -> ClearType {
        let lamp = if died {
            ClearType::Failed
        } else if self.max_combo >= self.total_notes {
            ClearType::FullCombo
        } else {
            match gauge_clear {
                None => ClearType::Failed,
                Some(ClearType::FullCombo) => ClearType::Normal,
                Some(c) => c,
            }
        };
        if assist == Assist::None || lamp == ClearType::Failed {
            lamp
        } else {
            lamp.min(ClearType::Easy)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assist_levels() {
        use crate::lane_option::LaneOption;
        assert_eq!(AssistFacts::default().level(), Assist::None);
        let lane = |o| AssistFacts {
            lane: vec![LaneOption::Mirror, o],
            ..Default::default()
        };
        for o in [
            LaneOption::Off,
            LaneOption::Random,
            LaneOption::RRandom,
            LaneOption::SRandom,
        ] {
            assert_eq!(lane(o).level(), Assist::None, "{o:?}");
        }
        for o in [
            LaneOption::Spiral,
            LaneOption::HRandom,
            LaneOption::AllScratch,
            LaneOption::RandomEx,
            LaneOption::SRandomEx,
        ] {
            assert_eq!(lane(o).level(), Assist::Light, "{o:?}");
        }
        let f = |f: fn(&mut AssistFacts)| {
            let mut a = AssistFacts::default();
            f(&mut a);
            a.level()
        };
        assert_eq!(f(|a| a.auto_scratch = true), Assist::Full);
        assert_eq!(f(|a| a.legacy_note = true), Assist::Full);
        assert_eq!(f(|a| a.wide_judge = true), Assist::Full);
        assert_eq!(f(|a| a.bpm_guide = true), Assist::Light);
        assert_eq!(f(|a| a.battle = true), Assist::Light);
        assert_eq!(f(|a| a.no_mine = true), Assist::Light);
        // the stronger wins
        let both = AssistFacts {
            lane: vec![LaneOption::Spiral],
            auto_scratch: true,
            ..Default::default()
        };
        assert_eq!(both.level(), Assist::Full);
    }

    #[test]
    fn dj_level_thresholds() {
        assert_eq!(DjLevel::from_rate(1600, 1800), DjLevel::AAA);
        assert_eq!(DjLevel::from_rate(1599, 1800), DjLevel::AA);
        assert_eq!(DjLevel::from_rate(0, 1800), DjLevel::F);
        // LR2's (S-OLR2 7.1): k = floor(9 EX / 2n), at most 8, at least 1
        // when EX > 0; 8 AAA .. 2 E, 1 or 0 F
        for n in 1..60u32 {
            for ex in 0..=2 * n {
                let mut k = (9 * ex / (2 * n)).min(8);
                if ex > 0 {
                    k = k.max(1);
                }
                let lr2 = DjLevel::ALL[k.saturating_sub(1) as usize];
                assert_eq!(DjLevel::from_rate(ex, 2 * n), lr2, "{ex} of {n}");
            }
        }
    }

    #[test]
    fn lamps() {
        let mut s = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        s.add(Judge::PGreat, 1);
        s.add(Judge::Great, -1);
        assert_eq!(s.ex_score(), 3);
        assert_eq!(
            s.clear_type(Some(ClearType::Normal), Assist::None),
            ClearType::Perfect
        );
        s.counts[1] = [0, 0];
        s.add(Judge::Good, 1);
        s.add(Judge::PGreat, 0);
        assert_eq!((s.count(Judge::PGreat), s.fast() + s.slow()), (2, 1));
        assert_eq!(
            s.clear_type(Some(ClearType::Hard), Assist::None),
            ClearType::FullCombo
        );
        assert_eq!(s.clear_type(None, Assist::None), ClearType::Failed);
        assert_eq!(
            s.clear_type(Some(ClearType::Hard), Assist::Full),
            ClearType::AssistEasy
        );
        assert_eq!(
            s.clear_type(Some(ClearType::Hard), Assist::Light),
            ClearType::LightAssistEasy
        );
    }

    #[test]
    fn lr2_lamps() {
        let mut s = Score {
            total_notes: 2,
            passed: 2,
            max_combo: 2,
            ..Default::default()
        };
        // a GROOVE full combo that ends below 80 %
        s.add(Judge::Miss, 1);
        assert_eq!(
            s.lr2_clear_type(false, None, Assist::None),
            ClearType::FullCombo
        );
        assert_eq!(
            s.lr2_clear_type(true, None, Assist::None),
            ClearType::Failed
        );
        assert_eq!(
            s.lr2_clear_type(false, None, Assist::Light),
            ClearType::Easy
        );
        s.max_combo = 1;
        for (gauge, lamp) in [
            (None, ClearType::Failed),
            (Some(ClearType::Easy), ClearType::Easy),
            (Some(ClearType::Normal), ClearType::Normal),
            (Some(ClearType::Hard), ClearType::Hard),
            // HAZARD
            (Some(ClearType::FullCombo), ClearType::Normal),
        ] {
            assert_eq!(s.lr2_clear_type(false, gauge, Assist::None), lamp);
        }
        assert_eq!(
            s.lr2_clear_type(false, Some(ClearType::Hard), Assist::Full),
            ClearType::Easy
        );
        assert_eq!(
            s.lr2_clear_type(false, None, Assist::Full),
            ClearType::Failed
        );
    }
}
