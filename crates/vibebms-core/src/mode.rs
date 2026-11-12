//! Play modes and lane layouts.

use serde::{Deserialize, Serialize};

/// Key mode of a chart.
///
/// Lanes are numbered per player as `[key1 .. keyN, scratch]`; for double play
/// the second player's lanes follow the first player's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum PlayMode {
    Beat5K,
    Beat7K,
    Beat10K,
    Beat14K,
    /// pop'n music style 9 buttons, no scratch.
    Popn9K,
}

/// Rule family used to pick judge/gauge constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuleFamily {
    FiveKeys,
    SevenKeys,
    Pms,
}

impl PlayMode {
    pub const ALL: [PlayMode; 5] = [
        PlayMode::Beat5K,
        PlayMode::Beat7K,
        PlayMode::Beat10K,
        PlayMode::Beat14K,
        PlayMode::Popn9K,
    ];

    pub fn players(self) -> usize {
        match self {
            PlayMode::Beat10K | PlayMode::Beat14K => 2,
            _ => 1,
        }
    }

    /// Number of (non-scratch) keys per player.
    pub fn keys_per_player(self) -> usize {
        match self {
            PlayMode::Beat5K | PlayMode::Beat10K => 5,
            PlayMode::Beat7K | PlayMode::Beat14K => 7,
            PlayMode::Popn9K => 9,
        }
    }

    pub fn has_scratch(self) -> bool {
        !matches!(self, PlayMode::Popn9K)
    }

    pub fn lanes_per_player(self) -> usize {
        self.keys_per_player() + usize::from(self.has_scratch())
    }

    pub fn lane_count(self) -> usize {
        self.lanes_per_player() * self.players()
    }

    pub fn is_scratch(self, lane: usize) -> bool {
        self.has_scratch() && lane % self.lanes_per_player() == self.keys_per_player()
    }

    pub fn player_of_lane(self, lane: usize) -> usize {
        lane / self.lanes_per_player()
    }

    pub fn scratch_lane(self, player: usize) -> Option<usize> {
        self.has_scratch()
            .then(|| player * self.lanes_per_player() + self.keys_per_player())
    }

    pub fn is_double(self) -> bool {
        self.players() == 2
    }

    pub fn family(self) -> RuleFamily {
        match self {
            PlayMode::Beat5K | PlayMode::Beat10K => RuleFamily::FiveKeys,
            PlayMode::Beat7K | PlayMode::Beat14K => RuleFamily::SevenKeys,
            PlayMode::Popn9K => RuleFamily::Pms,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Beat5K => "5KEYS",
            PlayMode::Beat7K => "7KEYS",
            PlayMode::Beat10K => "10KEYS",
            PlayMode::Beat14K => "14KEYS",
            PlayMode::Popn9K => "9KEYS",
        }
    }

    /// Display order of lanes from left to right for a player side.
    /// `scratch_right` places the turntable on the right (2P side style).
    pub fn display_order(self, player: usize, scratch_right: bool) -> Vec<usize> {
        let base = player * self.lanes_per_player();
        let keys: Vec<usize> = (0..self.keys_per_player()).map(|k| base + k).collect();
        match self.scratch_lane(player) {
            None => keys,
            Some(sc) => {
                let mut v = Vec::with_capacity(keys.len() + 1);
                if !scratch_right {
                    v.push(sc);
                }
                v.extend(keys);
                if scratch_right {
                    v.push(sc);
                }
                v
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_layout() {
        assert_eq!(PlayMode::Beat7K.lane_count(), 8);
        assert_eq!(PlayMode::Beat14K.lane_count(), 16);
        assert!(PlayMode::Beat14K.is_scratch(7));
        assert!(PlayMode::Beat14K.is_scratch(15));
        assert!(!PlayMode::Popn9K.is_scratch(8));
        assert_eq!(
            PlayMode::Beat7K.display_order(0, false),
            vec![7, 0, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            PlayMode::Beat14K.display_order(1, true),
            vec![8, 9, 10, 11, 12, 13, 14, 15]
        );
    }
}
