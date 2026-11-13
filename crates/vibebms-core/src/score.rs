//! Score bookkeeping: EX score, combo, clear lamp, DJ level.

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord, Default,
)]
pub enum ClearType {
    #[default]
    NoPlay = 0,
    Failed = 1,
    AssistEasy = 2,
    LightAssistEasy = 3,
    Easy = 4,
    Normal = 5,
    Hard = 6,
    ExHard = 7,
    FullCombo = 8,
    Perfect = 9,
    Max = 10,
}

impl ClearType {
    pub fn label(self) -> &'static str {
        match self {
            ClearType::NoPlay => "NO PLAY",
            ClearType::Failed => "FAILED",
            ClearType::AssistEasy => "ASSIST",
            ClearType::LightAssistEasy => "L-ASSIST",
            ClearType::Easy => "EASY",
            ClearType::Normal => "CLEAR",
            ClearType::Hard => "HARD",
            ClearType::ExHard => "EX-HARD",
            ClearType::FullCombo => "FULL COMBO",
            ClearType::Perfect => "PERFECT",
            ClearType::Max => "MAX",
        }
    }

    pub fn from_id(id: i64) -> ClearType {
        use ClearType::*;
        [
            NoPlay,
            Failed,
            AssistEasy,
            LightAssistEasy,
            Easy,
            Normal,
            Hard,
            ExHard,
            FullCombo,
            Perfect,
            Max,
        ]
        .get(id.clamp(0, 10) as usize)
        .copied()
        .unwrap_or(NoPlay)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum DjLevel {
    F,
    E,
    D,
    C,
    B,
    A,
    AA,
    AAA,
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

    pub fn label(self) -> &'static str {
        match self {
            DjLevel::F => "F",
            DjLevel::E => "E",
            DjLevel::D => "D",
            DjLevel::C => "C",
            DjLevel::B => "B",
            DjLevel::A => "A",
            DjLevel::AA => "AA",
            DjLevel::AAA => "AAA",
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
}

impl Score {
    pub fn add(&mut self, judge: usize, early: bool) {
        self.counts[judge][usize::from(!early)] += 1;
    }

    pub fn count(&self, judge: usize) -> u32 {
        self.counts[judge][0] + self.counts[judge][1]
    }

    pub fn fast(&self) -> u32 {
        (1..4).map(|j| self.counts[j][0]).sum()
    }

    pub fn slow(&self) -> u32 {
        (1..4).map(|j| self.counts[j][1]).sum()
    }

    pub fn ex_score(&self) -> u32 {
        self.count(0) * 2 + self.count(1)
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
        self.count(3) + self.count(4) + self.count(5) + self.total_notes.saturating_sub(self.passed)
    }

    pub fn dj_level(&self) -> DjLevel {
        DjLevel::from_rate(self.ex_score(), self.max_ex())
    }

    /// Lamp given the gauge outcome. `gauge_clear` is the lamp of the
    /// qualified gauge, or `None` when failed.
    pub fn clear_type(&self, gauge_clear: Option<ClearType>, assist: bool) -> ClearType {
        let Some(gc) = gauge_clear else {
            return ClearType::Failed;
        };
        if assist {
            return ClearType::AssistEasy;
        }
        let all = self.passed >= self.total_notes && self.total_notes > 0;
        if all && self.count(3) + self.count(4) == 0 {
            if self.count(2) == 0 {
                if self.count(1) == 0 {
                    return ClearType::Max;
                }
                return ClearType::Perfect;
            }
            return gc.max(ClearType::FullCombo);
        }
        gc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dj_level_thresholds() {
        assert_eq!(DjLevel::from_rate(1600, 1800), DjLevel::AAA);
        assert_eq!(DjLevel::from_rate(1599, 1800), DjLevel::AA);
        assert_eq!(DjLevel::from_rate(0, 1800), DjLevel::F);
    }

    #[test]
    fn lamps() {
        let mut s = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        s.add(0, true);
        s.add(1, false);
        assert_eq!(s.ex_score(), 3);
        assert_eq!(
            s.clear_type(Some(ClearType::Normal), false),
            ClearType::Perfect
        );
        s.counts[1] = [0, 0];
        s.add(2, true);
        assert_eq!(
            s.clear_type(Some(ClearType::Hard), false),
            ClearType::FullCombo
        );
        assert_eq!(s.clear_type(None, false), ClearType::Failed);
        assert_eq!(
            s.clear_type(Some(ClearType::Hard), true),
            ClearType::AssistEasy
        );
    }
}
