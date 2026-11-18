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
        /// (then bmz-player's: Qwilight-style 4 / 6 / 8 keys)
        K4 => "4KEYS",
        K6 => "6KEYS",
        K8 => "8KEYS",
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
            KeyFilter::K4 => mode == PlayMode::Key4,
            KeyFilter::K6 => mode == PlayMode::Key6,
            KeyFilter::K8 => mode == PlayMode::Key8,
        }
    }

    /// Skin button 11 (beatoraja's and LR2's six images): 4 / 6 / 8 keys
    /// show as ALL there, as bmz-player has it; button 1407 has them all.
    pub fn skin_index(self) -> i64 {
        match self {
            KeyFilter::K4 | KeyFilter::K6 | KeyFilter::K8 => 0,
            f => KeyFilter::ALL.iter().position(|k| *k == f).unwrap_or(0) as i64,
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

vibebms_core::choices! {
    /// Which of a chart's replays REPLAY (P) watches.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum ReplayPick {
        /// The best EX score's, else the latest.
        #[default]
        Auto => "BEST, ELSE LAST",
        Best => "MY BEST",
        Last => "LAST",
        Slot1 => "SLOT 1",
        Slot2 => "SLOT 2",
        Slot3 => "SLOT 3",
        Slot4 => "SLOT 4",
    }
}

impl ReplayPick {
    /// The replays it looks for, in order.
    pub fn slots(self) -> Vec<crate::replay::Slot> {
        use crate::replay::Slot;
        match self {
            ReplayPick::Auto => vec![Slot::Best, Slot::Last],
            ReplayPick::Best => vec![Slot::Best],
            ReplayPick::Last => vec![Slot::Last],
            ReplayPick::Slot1 => vec![Slot::Saved(1)],
            ReplayPick::Slot2 => vec![Slot::Saved(2)],
            ReplayPick::Slot3 => vec![Slot::Saved(3)],
            ReplayPick::Slot4 => vec![Slot::Saved(4)],
        }
    }
}

/// A replay is watched at 25 % to 300 % of its speed, in steps of 25.
pub const WATCH_SPEED_MIN: u32 = 25;
pub const WATCH_SPEED_MAX: u32 = 300;
pub const WATCH_SPEED_STEP: u32 = 25;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SelectConfig {
    pub sort: SortMode,
    pub key_filter: KeyFilter,
    pub difficulty_filter: DifficultyFilter,
    pub random: RandomSelect,
    /// The replay REPLAY (P) watches.
    pub replay: ReplayPick,
    /// The speed replays are watched at (%): the chart's presentation and
    /// sounds, never its judgements.
    pub replay_speed: u32,
}

impl Default for SelectConfig {
    fn default() -> SelectConfig {
        SelectConfig {
            sort: SortMode::default(),
            key_filter: KeyFilter::default(),
            difficulty_filter: DifficultyFilter::default(),
            random: RandomSelect::default(),
            replay: ReplayPick::default(),
            replay_speed: 100,
        }
    }
}

impl SelectConfig {
    pub fn is_default(&self) -> bool {
        *self == SelectConfig::default()
    }

    /// The watch speed `d` steps on, within its range.
    pub fn step_replay_speed(&mut self, d: i32) {
        let v = self.replay_speed as i32 + d * WATCH_SPEED_STEP as i32;
        self.replay_speed = v.clamp(WATCH_SPEED_MIN as i32, WATCH_SPEED_MAX as i32) as u32;
    }

    /// The watch speed as a rate (1.0: as played), within its range.
    pub fn watch_rate(&self) -> f64 {
        self.replay_speed.clamp(WATCH_SPEED_MIN, WATCH_SPEED_MAX) as f64 / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::Slot;

    #[test]
    fn replay_pick_and_watch_speed() {
        assert_eq!(ReplayPick::Auto.slots(), [Slot::Best, Slot::Last]);
        assert_eq!(ReplayPick::Slot2.slots(), [Slot::Saved(2)]);
        let mut c = SelectConfig::default();
        assert_eq!((c.replay_speed, c.watch_rate()), (100, 1.0));
        c.step_replay_speed(-3);
        assert_eq!(c.replay_speed, 25);
        c.step_replay_speed(-1);
        assert_eq!(c.replay_speed, 25);
        c.step_replay_speed(20);
        assert_eq!((c.replay_speed, c.watch_rate()), (300, 3.0));
        c.step_replay_speed(-5);
        assert_eq!(c.replay_speed, 175);
        // a speed out of range in the file: kept within it
        c.replay_speed = 1000;
        assert_eq!(c.watch_rate(), 3.0);
        // a config of before: the defaults
        let old: SelectConfig = toml::from_str("sort = \"Level\"").unwrap();
        assert_eq!((old.replay, old.replay_speed), (ReplayPick::Auto, 100));
    }
}
