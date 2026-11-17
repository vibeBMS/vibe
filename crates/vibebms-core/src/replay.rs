//! Replays: what drove the judge in a play (its inputs and the times it was
//! advanced to, [`Op`]), with what made the chart and the rules (the seed,
//! the play options), so the play can be judged again exactly. Its
//! judgements in order are the play's ghost: a later play's pacemaker at
//! the same note. Kept as gzipped JSON.

use crate::engine::Op;
use crate::judge::Judge;
use crate::play::RuleOptions;
use crate::score::ClearType;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// The replay format. 2: the judge handles every input at its own time
/// (a replay of 1 may judge a little otherwise when played again).
pub const VERSION: u32 = 2;

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
    /// The judgements in the order the notes were passed (PG 0 .. MISS 5).
    pub ghost: Vec<u8>,
    pub ops: Vec<Op>,
}

impl<P> Replay<P> {
    /// Recorded with an older judge: played again, it may judge otherwise
    /// than it did (its ghost is what it did).
    pub fn older_engine(&self) -> bool {
        self.version < VERSION
    }

    /// The EX score of the first `n` judgements.
    pub fn ex_at(&self, n: usize) -> u32 {
        ex_of(&self.ghost[..n.min(self.ghost.len())])
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

/// EX score of judgements (as kept: `Judge` indices).
pub fn ex_of(judges: &[u8]) -> u32 {
    judges
        .iter()
        .filter_map(|j| Judge::of(*j as usize))
        .map(Judge::ex)
        .sum()
}
