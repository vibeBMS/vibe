//! Replays: what drove the judge in a play (its inputs and the times it was
//! advanced to, `vibebms_core::engine::Op`), with what made the chart and
//! the rules (the seed, the play options), so the play can be watched
//! again exactly. Its judgements in order are the play's ghost: a later
//! play's pacemaker at the same note.
//!
//! Each chart (by SHA-256 and LN mode) keeps its latest play's replay and
//! its best EX score's, gzipped JSON in `<root>/scores/replays/`.

use crate::config::PlayConfig;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use vibebms_core::engine::Op;

/// The replay format. 2: the judge handles every input at its own time
/// (a replay of 1 may judge a little otherwise when played again).
pub const VERSION: u32 = 2;

/// The oldest format still loaded: its ghost still paces a play, but
/// watching it or playing it again (G-BATTLE) runs it on this judge.
const OLDEST: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replay {
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
    pub play: PlayConfig,
    pub ex_score: u32,
    pub clear: String,
    /// The judgements in the order the notes were passed (PG 0 .. MISS 5).
    pub ghost: Vec<u8>,
    pub ops: Vec<Op>,
}

/// Which of a chart's replays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The latest play.
    Last,
    /// The best EX score.
    Best,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Slot::Last => "last",
            Slot::Best => "best",
        }
    }
}

/// The folder replays are kept in.
pub fn dir(root: &Path) -> PathBuf {
    root.join("scores").join("replays")
}

fn file(root: &Path, sha256: &str, ln: i32, slot: Slot) -> PathBuf {
    dir(root).join(format!("{sha256}_{ln}_{}.json.gz", slot.name()))
}

impl Replay {
    /// Whether it may be kept as a chart's latest or best replay (the
    /// material of ghosts and G-BATTLE opponents): not a BATTLE play (a
    /// single play chart on both sides, judged as one).
    pub fn keepable(&self) -> bool {
        !self.play.battle
    }

    /// Recorded with an older judge: played again, it may judge otherwise
    /// than it did (its ghost is what it did).
    pub fn older_engine(&self) -> bool {
        self.version < VERSION
    }

    /// The EX score of the first `n` judgements.
    pub fn ex_at(&self, n: usize) -> u32 {
        ex_of(&self.ghost[..n.min(self.ghost.len())])
    }

    /// Keep it as `slot` of its chart.
    pub fn save(&self, root: &Path, slot: Slot) -> Result<(), String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&json).map_err(|e| e.to_string())?;
        let bytes = gz.finish().map_err(|e| e.to_string())?;
        let path = file(root, &self.sha256, self.ln, slot);
        if !crate::platform::IS_WEB {
            crate::platform::vfs::create_dir_all(&dir(root)).map_err(|e| e.to_string())?;
        }
        write(&path, bytes)
    }

    /// The chart's replay `slot`, if there is one.
    pub fn load(root: &Path, sha256: &str, ln: i32, slot: Slot) -> Option<Replay> {
        let bytes = crate::platform::vfs::read(&file(root, sha256, ln, slot)).ok()?;
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_end(&mut json)
            .ok()?;
        let r: Replay = serde_json::from_slice(&json).ok()?;
        (OLDEST..=VERSION).contains(&r.version).then_some(r)
    }

    /// The replay `slot` filed under LN mode `ln`. `fixed`: the chart's
    /// long notes all have their type, and replays from before vibe filed
    /// them under the chart's mode may be under any (the best EX score's,
    /// or the latest, of those).
    pub fn load_filed(
        root: &Path,
        sha256: &str,
        ln: i32,
        fixed: bool,
        slot: Slot,
    ) -> Option<Replay> {
        if let Some(r) = Replay::load(root, sha256, ln, slot) {
            return Some(r);
        }
        if !fixed {
            return None;
        }
        (0..3)
            .filter(|k| *k != ln)
            .filter_map(|k| Replay::load(root, sha256, k, slot))
            .max_by_key(|r| match slot {
                Slot::Best => (r.ex_score as i64, r.date),
                Slot::Last => (r.date, r.ex_score as i64),
            })
    }
}

/// Why `r` cannot be a G-BATTLE opponent on `song` for a player in LN
/// mode `ln` (`None`: it can). The opponent plays its inputs again on the
/// second side, so the chart and the judge must come out as they did.
pub fn ghost_refusal(
    r: &Replay,
    song: &crate::library::Song,
    ln: vibebms_core::LnType,
) -> Option<&'static str> {
    Some(if !matches!(song.mode.as_str(), "7KEYS" | "5KEYS") {
        "G-BATTLE needs a 5 or 7 key chart"
    } else if r.play.battle {
        "the replay is a BATTLE play"
    } else if r.ops.is_empty() {
        "the replay has no inputs"
    } else if r.play.legacy_note {
        "the replay is a LEGACY NOTE play"
    } else if song.score_ln(r.play.ln_mode) != song.score_ln(ln) {
        "the replay was played in another LN mode"
    } else {
        return None;
    })
}

/// EX score of judgements (PG 2, GR 1).
pub fn ex_of(judges: &[u8]) -> u32 {
    judges
        .iter()
        .map(|j| match j {
            0 => 2,
            1 => 1,
            _ => 0,
        })
        .sum()
}

#[cfg(not(target_arch = "wasm32"))]
fn write(path: &Path, bytes: Vec<u8>) -> Result<(), String> {
    crate::platform::vfs::write(path, &bytes).map_err(|e| e.to_string())
}

/// The web build: held in memory and kept in the browser's storage.
#[cfg(target_arch = "wasm32")]
fn write(path: &Path, bytes: Vec<u8>) -> Result<(), String> {
    crate::platform::vfs::store(path, bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_and_loaded() {
        let root = std::env::temp_dir().join(format!("vibebms-replay-{}", std::process::id()));
        let r = Replay {
            version: VERSION,
            sha256: "ab".into(),
            md5: "cd".into(),
            title: "Song".into(),
            ln: 0,
            date: 1,
            seed: 42,
            play: PlayConfig::default(),
            ex_score: 3,
            clear: "EASY".into(),
            ghost: vec![0, 1, 4],
            ops: vec![Op::Update(5), Op::Input(10, 1, 0, true)],
        };
        r.save(&root, Slot::Best).unwrap();
        let back = Replay::load(&root, "ab", 0, Slot::Best).unwrap();
        assert_eq!(back.ops, r.ops);
        assert_eq!(back.seed, 42);
        assert_eq!(back.ex_at(2), 3);
        assert!(Replay::load(&root, "ab", 0, Slot::Last).is_none());
        assert!(!back.older_engine());
        // an older judge's replay still loads (its ghost paces), marked
        let old = Replay {
            version: 1,
            ..r.clone()
        };
        old.save(&root, Slot::Last).unwrap();
        let back = Replay::load(&root, "ab", 0, Slot::Last).unwrap();
        assert!(back.older_engine());
        assert_eq!(back.ghost, r.ghost);
        // a format not known: not loaded
        let future = Replay {
            version: VERSION + 1,
            ..r.clone()
        };
        future.save(&root, Slot::Last).unwrap();
        assert!(Replay::load(&root, "ab", 0, Slot::Last).is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
