//! What the IRs say about charts and players: lamps, leaderboards, chart
//! facts, and each service's lamp names.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use vibebms_core::score::ClearType;

/// A player's lamp on one chart, as an IR has it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLamp {
    pub clear: ClearType,
    pub ex: Option<u32>,
    pub bp: Option<u32>,
    /// When it was achieved (Unix ms).
    pub at_ms: Option<i64>,
    /// The service's own id of the chart (Tachi's `chartID`).
    pub remote: Option<String>,
    /// The best EX score's PGREAT, GREAT, GOOD, BAD and POOR, the best
    /// combo and the chart's notes, where the service gives them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judges: Option<[u32; 5]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_combo: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<u32>,
}

/// One of a player's best scores as an IR keeps them (a vibeBMS IR's
/// `/players/{id}/scores`): its chart, the LN mode and rule profile it
/// was played in where the service says, and the score.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteScore {
    pub sha256: String,
    pub md5: String,
    pub ln: Option<vibebms_core::chart::LnType>,
    pub profile: Option<crate::rule::Profile>,
    pub score: RemoteLamp,
}

/// One player's lamps, by chart hash (lower-case hexadecimal: an MD5 is 32
/// digits, a SHA-256 64, so both kinds share the map).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LampSet {
    pub player_name: String,
    pub by_chart: HashMap<String, RemoteLamp>,
    /// For the next request's `If-None-Match`.
    pub etag: Option<String>,
}

impl LampSet {
    /// The lamp of the chart with these hashes, by either.
    pub fn get(&self, md5: &str, sha256: &str) -> Option<&RemoteLamp> {
        [sha256, md5]
            .into_iter()
            .filter(|h| !h.is_empty())
            .find_map(|h| self.by_chart.get(&h.to_ascii_lowercase()))
    }

    pub(crate) fn insert(&mut self, hash: &str, lamp: RemoteLamp) {
        self.by_chart.insert(hash.to_ascii_lowercase(), lamp);
    }
}

/// A chart's leaderboard, best first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RankView {
    /// Players with a score on the chart (in the view asked).
    pub players: u32,
    pub entries: Vec<RankEntry>,
    pub etag: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RankEntry {
    pub rank: u32,
    pub player: String,
    pub player_id: String,
    pub clear: ClearType,
    pub ex: u32,
    pub bp: Option<u32>,
    pub max_combo: Option<u32>,
    pub notes: Option<u32>,
    /// The program the score came from, as the service names it.
    pub client: Option<String>,
    pub at_ms: Option<i64>,
}

/// What an IR knows of a chart.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChartInfo {
    pub md5: String,
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    /// As the chart writes it.
    pub level: String,
    pub notes: u32,
    pub players: u32,
}

/// BMS-IR's lamp names (its read API), best first: one per lamp.
pub const BMSIR_LAMPS: [(&str, ClearType); 11] = [
    ("MAX", ClearType::Max),
    ("P-A", ClearType::Perfect),
    ("FULL COMBO", ClearType::FullCombo),
    ("EX HARD", ClearType::ExHard),
    ("HARD", ClearType::Hard),
    ("CLEAR", ClearType::Normal),
    ("EASY", ClearType::Easy),
    ("LIGHT ASSIST", ClearType::LightAssistEasy),
    ("ASSIST EASY", ClearType::AssistEasy),
    ("FAILED", ClearType::Failed),
    ("NO PLAY", ClearType::NoPlay),
];

pub fn bmsir_lamp(name: &str) -> Option<ClearType> {
    BMSIR_LAMPS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, c)| c)
}

/// Tachi's BMS and PMS lamps (its game configs), best first.
pub const TACHI_LAMPS: [(&str, ClearType); 8] = [
    ("FULL COMBO", ClearType::FullCombo),
    ("EX HARD CLEAR", ClearType::ExHard),
    ("HARD CLEAR", ClearType::Hard),
    ("CLEAR", ClearType::Normal),
    ("EASY CLEAR", ClearType::Easy),
    ("ASSIST CLEAR", ClearType::LightAssistEasy),
    ("FAILED", ClearType::Failed),
    ("NO PLAY", ClearType::NoPlay),
];

pub fn tachi_lamp(name: &str) -> Option<ClearType> {
    TACHI_LAMPS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, c)| c)
}

/// The Tachi lamp a play is sent with: PERFECT and MAX are a FULL COMBO
/// there; an assisted lamp (ASSIST EASY's options) and NO PLAY are never
/// sent.
pub fn to_tachi_lamp(c: ClearType) -> Option<&'static str> {
    let c = match c {
        ClearType::Perfect | ClearType::Max => ClearType::FullCombo,
        ClearType::AssistEasy | ClearType::NoPlay => return None,
        c => c,
    };
    TACHI_LAMPS.iter().find(|(_, l)| *l == c).map(|&(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmsir_lamps_are_ours_one_to_one() {
        let mut ours: Vec<ClearType> = BMSIR_LAMPS.iter().map(|&(_, c)| c).collect();
        ours.sort();
        assert_eq!(ours, ClearType::ALL);
        assert_eq!(bmsir_lamp("P-A"), Some(ClearType::Perfect));
        assert_eq!(bmsir_lamp("EX-HARD"), None);
    }

    #[test]
    fn tachi_lamps_go_both_ways() {
        for (name, c) in TACHI_LAMPS {
            assert_eq!(tachi_lamp(name), Some(c));
            if c != ClearType::NoPlay {
                assert_eq!(to_tachi_lamp(c), Some(name));
            }
        }
        assert_eq!(to_tachi_lamp(ClearType::Max), Some("FULL COMBO"));
        assert_eq!(to_tachi_lamp(ClearType::AssistEasy), None);
        assert_eq!(to_tachi_lamp(ClearType::NoPlay), None);
    }

    #[test]
    fn a_lamp_is_found_by_either_hash() {
        let mut s = LampSet::default();
        let lamp = RemoteLamp {
            clear: ClearType::Hard,
            ..Default::default()
        };
        s.insert("ABCDEF0123456789ABCDEF0123456789", lamp.clone());
        assert_eq!(s.get("abcdef0123456789abcdef0123456789", ""), Some(&lamp));
        assert_eq!(s.get("", "ff"), None);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<LampSet>(&json).unwrap(), s);
    }
}
