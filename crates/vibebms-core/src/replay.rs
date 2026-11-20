//! Replays: what drove the judge in a play (its inputs and the times it was
//! advanced to, [`Op`]), with what made the chart and the rules (the seed,
//! the play options), so the play can be judged again exactly. Its
//! judgements in order are the play's ghost: a later play's pacemaker at
//! the same note. Kept as gzipped JSON.

use crate::course::CourseDef;
use crate::engine::Op;
use crate::judge::Judge;
use crate::play::RuleOptions;
use crate::score::ClearType;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// The replay format: bumped when a replay of the one before may judge
/// otherwise on the judge of now (each change: `docs/RULES.md`, "Replay
/// formats").
pub const VERSION: u32 = 7;

/// The oldest format still read: its ghost still paces a play, but
/// watching it or playing it again (G-BATTLE) runs it on this judge.
const OLDEST: u32 = 1;

/// The largest replay kept or read (its JSON, bytes): hours of play at
/// a thousand updates a second. A file that unpacks to more is not one.
pub const MAX_JSON: usize = 128 << 20;

/// A play's replay. `P`: its play options, the rules and whatever else the
/// player kept with them (the app's view options).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replay<P = RuleOptions> {
    pub version: u32,
    pub sha256: String,
    pub md5: String,
    pub title: String,
    /// The LN mode played (as `ln` of the scores).
    pub ln: i32,
    /// When it was played (Unix seconds).
    pub date: i64,
    /// The chart's `#RANDOM` and the lane options' seed.
    pub seed: i64,
    /// The play options that made the chart and the rules.
    pub play: P,
    pub ex_score: u32,
    #[serde(with = "crate::by_label")]
    pub clear: ClearType,
    /// The judgements in the order the notes were passed (kept as their
    /// numbers, PG 0 .. MISS 5).
    pub ghost: Vec<Judge>,
    pub ops: Vec<Op>,
    /// A course's play: what makes its chart and gauge besides the chart
    /// of `sha256` (missing in single plays and older replays).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub course: Option<CourseReplay>,
}

/// What a course's replay needs: a NONSTOP play's chart is its stages
/// joined (`play::build_nonstop_chart`; `sha256` and `md5` are then the
/// course's own, its stages here); an expert or class course's stage is
/// its chart's, with the gauge it was given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CourseReplay {
    pub course: CourseDef,
    /// The stages' SHA-256, in order (to find their files).
    pub sha256: Vec<String>,
    /// The stage played (0 for NONSTOP).
    pub stage: u32,
    /// The gauges' values the stage started from (the stages before).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gauge: Option<Vec<f32>>,
}

impl<P> Replay<P> {
    /// Recorded with an older judge: played again, it may judge otherwise
    /// than it did (its ghost is what it did).
    pub fn older_engine(&self) -> bool {
        self.version < VERSION
    }

    /// The EX score of the first `n` judgements.
    pub fn ex_at(&self, n: usize) -> u32 {
        self.ghost[..n.min(self.ghost.len())]
            .iter()
            .map(|j| j.ex())
            .sum()
    }
}

impl<P: AsRef<RuleOptions>> Replay<P> {
    /// Whether it may be kept as a chart's latest or best replay (the
    /// material of ghosts and G-BATTLE opponents): not a BATTLE play (a
    /// single play chart on both sides, judged as one), nor one at a FREQ
    /// below 0 (such a play is not saved at all).
    pub fn keepable(&self) -> bool {
        let p = self.play.as_ref();
        !p.battle && p.freq >= 0
    }
}

impl<P: Serialize> Replay<P> {
    /// The replay file's bytes.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if json.len() > MAX_JSON {
            return Err("the replay is too long to keep".into());
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&json).map_err(|e| e.to_string())?;
        gz.finish().map_err(|e| e.to_string())
    }
}

impl<P: DeserializeOwned> Replay<P> {
    /// The replay in a file's bytes, if it is one of a format still read.
    pub fn decode(bytes: &[u8]) -> Option<Replay<P>> {
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_JSON as u64 + 1)
            .read_to_end(&mut json)
            .ok()?;
        if json.len() > MAX_JSON {
            return None;
        }
        let r: Replay<P> = serde_json::from_slice(&json).ok()?;
        (OLDEST..=VERSION).contains(&r.version).then_some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(play: RuleOptions) -> Replay {
        Replay {
            version: VERSION,
            sha256: String::new(),
            md5: String::new(),
            title: String::new(),
            ln: 0,
            date: 0,
            seed: 1,
            play,
            ex_score: 0,
            clear: ClearType::Failed,
            ghost: Vec::new(),
            ops: Vec::new(),
            course: None,
        }
    }

    /// A file that unpacks to more than a replay's JSON may be is none
    /// (and is not unpacked further).
    #[test]
    fn a_replay_that_unpacks_too_large_is_not_read() {
        let json = serde_json::to_vec(&replay(RuleOptions::default())).unwrap();
        let packed = |pad: usize| {
            let level = flate2::Compression::fast();
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), level);
            gz.write_all(&json).unwrap();
            gz.write_all(&vec![b' '; pad]).unwrap();
            gz.finish().unwrap()
        };
        assert!(Replay::<RuleOptions>::decode(&packed(MAX_JSON - json.len())).is_some());
        assert!(Replay::<RuleOptions>::decode(&packed(MAX_JSON + 1 - json.len())).is_none());
    }

    #[test]
    fn plays_below_freq_0_and_battles_are_not_kept() {
        let at = |freq| {
            replay(RuleOptions {
                freq,
                ..Default::default()
            })
        };
        assert!(at(0).keepable() && at(12).keepable());
        assert!(!at(-1).keepable());
        let battle = replay(RuleOptions {
            battle: true,
            ..Default::default()
        });
        assert!(!battle.keepable());
        // the FREQ played comes back; a replay of before has 0
        let r = Replay::<RuleOptions>::decode(&at(3).encode().unwrap()).unwrap();
        assert_eq!(r.play.freq, 3);
        let mut json = serde_json::to_value(at(0)).unwrap();
        json["play"].as_object_mut().unwrap().remove("freq");
        let old: Replay = serde_json::from_value(json).unwrap();
        assert_eq!(old.play.freq, 0);
    }
}
