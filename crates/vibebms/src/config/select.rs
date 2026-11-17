//! Song select: the list's order and filters, kept between runs.

use serde::{Deserialize, Serialize};
use vibebms_core::score::ClearType;
use vibebms_core::PlayMode;

vibebms_core::choices! {
    /// The order of a folder's charts (skin button 12 shows its index).
    /// Ties go by title, then difficulty and level.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum SortMode {
        #[default]
        Title => "TITLE",
        Level => "LEVEL",
        /// Lowest lamp first (not played first).
        Clear => "CLEAR",
        /// Lowest EX score rate first.
        Score => "SCORE",
        /// Fewest misses (min BP) first; not played last.
        MissCount => "MISS COUNT",
        /// Played most recently first; not played last.
        LastPlayed => "LAST PLAYED",
        /// The newest chart file first.
        Added => "NEW",
        Bpm => "BPM",
        Artist => "ARTIST",
        Notes => "NOTES",
    }
}

vibebms_core::choices! {
    /// The key modes shown (skin button 11), in beatoraja's order.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum KeyFilter {
        #[default]
        All => "ALL",
        K7 => "7KEYS",
        K14 => "14KEYS",
        K9 => "9KEYS",
        K5 => "5KEYS",
        K10 => "10KEYS",
    }
}

impl KeyFilter {
    pub fn keeps(self, mode: PlayMode) -> bool {
        match self {
            KeyFilter::All => true,
            KeyFilter::K7 => mode == PlayMode::Beat7K,
            KeyFilter::K14 => mode == PlayMode::Beat14K,
            KeyFilter::K9 => mode == PlayMode::Popn9K,
            KeyFilter::K5 => mode == PlayMode::Beat5K,
            KeyFilter::K10 => mode == PlayMode::Beat10K,
        }
    }
}

vibebms_core::choices! {
    /// The difficulty shown (skin button 10): `#DIFFICULTY` 1..5.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum DifficultyFilter {
        #[default]
        All = 0 => "ALL",
        Beginner = 1 => "BEGINNER",
        Normal = 2 => "NORMAL",
        Hyper = 3 => "HYPER",
        Another = 4 => "ANOTHER",
        Insane = 5 => "INSANE",
    }
}

impl DifficultyFilter {
    pub fn keeps(self, difficulty: i32) -> bool {
        self == DifficultyFilter::All || self as i32 == difficulty
    }
}

vibebms_core::choices! {
    /// Which charts RANDOM SELECT draws from (the open folder's, as
    /// filtered), by the lamp under the rule mode chosen.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum RandomSelect {
        #[default]
        Any => "ANY",
        /// Below CLEAR (not played, FAILED, ASSIST, EASY).
        NotCleared => "NOT CLEAR",
        /// Below HARD CLEAR.
        NotHard => "NOT HARD",
        /// Below FULL COMBO.
        NotFc => "NOT FC",
    }
}

impl RandomSelect {
    /// A chart with lamp `clear` may be drawn.
    pub fn keeps(self, clear: ClearType) -> bool {
        match self {
            RandomSelect::Any => true,
            RandomSelect::NotCleared => clear < ClearType::Normal,
            RandomSelect::NotHard => clear < ClearType::Hard,
            RandomSelect::NotFc => clear < ClearType::FullCombo,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SelectConfig {
    pub sort: SortMode,
    pub key_filter: KeyFilter,
    pub difficulty_filter: DifficultyFilter,
    pub random: RandomSelect,
}

impl SelectConfig {
    pub fn is_default(&self) -> bool {
        *self == SelectConfig::default()
    }
}
