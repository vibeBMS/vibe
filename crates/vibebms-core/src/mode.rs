//! Play modes and lane layouts.

use serde::{Deserialize, Serialize};

crate::choices! {
    /// Key mode of a chart.
    ///
    /// Lanes are numbered per player as `[key1 .. keyN, scratch]`; for double play
    /// the second player's lanes follow the first player's.
    #[derive(
        Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord,
    )]
    pub enum PlayMode {
        Beat5K => "5KEYS",
        #[default]
        Beat7K => "7KEYS",
        Beat10K => "10KEYS",
        Beat14K => "14KEYS",
        /// pop'n music style 9 buttons, no scratch.
        Popn9K => "9KEYS",
        /// Qwilight-style 4 keys, no scratch (`#4K`: BMS keys 1, 2, 4, 5).
        Key4 => "4KEYS",
        /// Qwilight-style 6 keys, no scratch (`#6K`: BMS keys 1-3, 5-7).
        Key6 => "6KEYS",
        /// Qwilight-style 8 keys, no scratch (`#8K`: the BMS scratch lane
        /// as the leftmost key, then keys 1-7).
        Key8 => "8KEYS",
    }
}

/// Rule family used to pick judge/gauge constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuleFamily {
    FiveKeys,
    SevenKeys,
    Pms,
}

impl PlayMode {
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
            PlayMode::Key4 => 4,
            PlayMode::Key6 => 6,
            PlayMode::Key8 => 8,
        }
    }

    pub fn has_scratch(self) -> bool {
        !matches!(
            self,
            PlayMode::Popn9K | PlayMode::Key4 | PlayMode::Key6 | PlayMode::Key8
        )
    }

    /// One of the Qwilight-style 4 / 6 / 8 key modes (no standard skin
    /// type, no IR ranks them).
    pub fn is_qwilight(self) -> bool {
        matches!(self, PlayMode::Key4 | PlayMode::Key6 | PlayMode::Key8)
    }

    /// Where a 4 / 6 / 8 key mode's lanes are among 7 keys' lanes
    /// (`[k1..k7, S]`): the 7 key channels its charts write them in (4K
    /// keys 1, 2, 4, 5; 6K keys 1-3, 5-7; 8K the scratch, then keys 1-7).
    /// `None` for the other modes.
    pub fn seven_key_lanes(self) -> Option<&'static [u8]> {
        Some(match self {
            PlayMode::Key4 => &[0, 1, 3, 4],
            PlayMode::Key6 => &[0, 1, 2, 4, 5, 6],
            PlayMode::Key8 => &[7, 0, 1, 2, 3, 4, 5, 6],
            _ => return None,
        })
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
            // (4K / 6K / 8K: no rule of their own; 7 key windows and gauges)
            PlayMode::Beat7K | PlayMode::Beat14K => RuleFamily::SevenKeys,
            PlayMode::Key4 | PlayMode::Key6 | PlayMode::Key8 => RuleFamily::SevenKeys,
            PlayMode::Popn9K => RuleFamily::Pms,
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
        for (m, n) in [
            (PlayMode::Key4, 4),
            (PlayMode::Key6, 6),
            (PlayMode::Key8, 8),
        ] {
            assert_eq!(m.lane_count(), n);
            assert!(!m.has_scratch() && !m.is_double() && m.is_qwilight());
            assert_eq!(m.display_order(0, false), (0..n).collect::<Vec<_>>());
            assert_eq!(m.family(), RuleFamily::SevenKeys);
        }
    }
}
