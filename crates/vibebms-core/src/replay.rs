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

/// The replay format. 2: the judge handles every input at its own time
/// (a replay of 1 may judge a little otherwise when played again).
/// 3: the LR2 preset keeps LR2's combo and note rules in every key mode
/// and its HAZARD gauge ignores an empty POOR (a LR2-preset replay of 2
/// in 5/10 keys or PMS, or on HAZARD, may judge otherwise).
/// 4: the LR2 preset's HARD and EX-HARD damage by TOTAL steps at the
/// multiples of 10 (a LR2-preset replay of 3 below TOTAL 240 may end
/// otherwise on those gauges).
/// 5: the LR2 preset as `docs/LR2_BEHAVIOUR.md` has it: the note choice
/// and the BAD chain, whole milliseconds, its long notes and mines,
/// `#RANK` and `#TOTAL` as LR2 reads them, its HARD damage by TOTAL, its
/// HAZARD and dan gauges (a LR2-preset replay of 4 may judge otherwise).
/// 6: the IIDX preset as DX MODE: its windows (BAD ±200 ms, 9 keys
/// pop'n's), combo and note rules, gauges and 2 % rule (an IIDX-preset
/// replay of 5 may judge otherwise).
pub const VERSION: u32 = 6;

/// The oldest format still read: its ghost still paces a play, but
/// watching it or playing it again (G-BATTLE) runs it on this judge.
const OLDEST: u32 = 1;

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
    /// single play chart on both sides, judged as one).
    pub fn keepable(&self) -> bool {
        !self.play.as_ref().battle
    }
}

impl<P: Serialize> Replay<P> {
    /// The replay file's bytes.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
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
            .read_to_end(&mut json)
            .ok()?;
        let r: Replay<P> = serde_json::from_slice(&json).ok()?;
        (OLDEST..=VERSION).contains(&r.version).then_some(r)
    }
}
