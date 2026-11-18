//! Practice mode's settings (`[practice]`), apart from the play's: the
//! speed, lead-in, gauge, `#RANK`, TOTAL, loop, and each chart's last
//! section.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use vibebms_core::gauge::GaugeType;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PracticeConfig {
    /// Speed in percent; the pitch follows it.
    pub speed: u32,
    /// Measures played before the section.
    pub lead_in: usize,
    pub gauge: GaugeType,
    /// The gauge's value at the start (%); `None`: the gauge's own.
    pub gauge_start: Option<u32>,
    /// `#RANK` 0..=4 in place of the chart's.
    pub rank: Option<i32>,
    /// TOTAL in place of the chart's.
    pub total: Option<u32>,
    /// The section again at once when it ends or the gauge empties.
    #[serde(rename = "loop")]
    pub repeat: bool,
    /// Each chart's last section (its SHA-256: first and last measure).
    pub sections: BTreeMap<String, [usize; 2]>,
}

impl Default for PracticeConfig {
    fn default() -> Self {
        PracticeConfig {
            speed: 100,
            lead_in: 1,
            gauge: GaugeType::Normal,
            gauge_start: None,
            rank: None,
            total: None,
            repeat: false,
            sections: BTreeMap::new(),
        }
    }
}

impl PracticeConfig {
    pub fn is_default(&self) -> bool {
        *self == PracticeConfig::default()
    }
}
