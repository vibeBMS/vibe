//! The replays kept (`vibebms_core::replay`): each chart (by SHA-256 and LN
//! mode) keeps its latest play's replay and its best EX score's, in
//! `<root>/scores/replays/`.

use crate::config::PlayConfig;
use std::path::{Path, PathBuf};
pub use vibebms_core::replay::VERSION;

/// A replay with the play options it was played with (rules and view).
pub type Replay = vibebms_core::replay::Replay<PlayConfig>;

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

/// Keep `r` as `slot` of its chart.
pub fn save(r: &Replay, root: &Path, slot: Slot) -> Result<(), String> {
    let bytes = r.encode()?;
    let path = file(root, &r.sha256, r.ln, slot);
    if !crate::platform::IS_WEB {
        crate::platform::vfs::create_dir_all(&dir(root)).map_err(|e| e.to_string())?;
    }
    write(&path, bytes)
}

/// The chart's replay `slot`, if there is one.
pub fn load(root: &Path, sha256: &str, ln: i32, slot: Slot) -> Option<Replay> {
    Replay::decode(&crate::platform::vfs::read(&file(root, sha256, ln, slot)).ok()?)
}

/// The replay `slot` filed under LN mode `ln`. `fixed`: the chart's long
/// notes all have their type, and replays from before vibe filed them
/// under the chart's mode may be under any (the best EX score's, or the
/// latest, of those).
pub fn load_filed(root: &Path, sha256: &str, ln: i32, fixed: bool, slot: Slot) -> Option<Replay> {
    if let Some(r) = load(root, sha256, ln, slot) {
        return Some(r);
    }
    if !fixed {
        return None;
    }
    (0..3)
        .filter(|k| *k != ln)
        .filter_map(|k| load(root, sha256, k, slot))
        .max_by_key(|r| match slot {
            Slot::Best => (r.ex_score as i64, r.date),
            Slot::Last => (r.date, r.ex_score as i64),
        })
}

/// Why `r` cannot be a G-BATTLE opponent on `song` for a player in LN
/// mode `ln` (`None`: it can). The opponent plays its inputs again on the
/// second side, so the chart and the judge must come out as they did.
pub fn ghost_refusal(
    r: &Replay,
    song: &crate::library::Song,
    ln: vibebms_core::LnType,
) -> Option<&'static str> {
    use vibebms_core::PlayMode;
    Some(
        if !matches!(song.mode, PlayMode::Beat7K | PlayMode::Beat5K) {
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
        },
    )
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
    use vibebms_core::engine::Op;
    use vibebms_core::judge::Judge;
    use vibebms_core::score::ClearType;

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
            clear: ClearType::Easy,
            ghost: vec![Judge::PGreat, Judge::Great, Judge::Poor],
            ops: vec![Op::Update(5), Op::Input(10, 1, 0, true)],
        };
        save(&r, &root, Slot::Best).unwrap();
        let back = load(&root, "ab", 0, Slot::Best).unwrap();
        assert_eq!(back.ops, r.ops);
        assert_eq!(back.seed, 42);
        assert_eq!(back.ex_at(2), 3);
        assert!(load(&root, "ab", 0, Slot::Last).is_none());
        assert!(!back.older_engine());
        // an older judge's replay still loads (its ghost paces), marked
        let old = Replay {
            version: 1,
            ..r.clone()
        };
        save(&old, &root, Slot::Last).unwrap();
        let back = load(&root, "ab", 0, Slot::Last).unwrap();
        assert!(back.older_engine());
        assert_eq!(back.ghost, r.ghost);
        // a format not known: not loaded
        let future = Replay {
            version: VERSION + 1,
            ..r.clone()
        };
        save(&future, &root, Slot::Last).unwrap();
        assert!(load(&root, "ab", 0, Slot::Last).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// A replay written before the play options were split keeps all of
    /// them, and is written again with the same JSON.
    #[test]
    fn a_replay_of_before_reads_whole() {
        let bytes = include_bytes!("../../vibebms-core/tests/data/replay/v2.json.gz");
        let r = Replay::decode(bytes).unwrap();
        assert_eq!((r.play.hispeed, r.play.sudden), (3.25, 0.2));
        assert_eq!(r.clear, ClearType::Hard);
        let json = |r: &Replay| -> serde_json::Value {
            serde_json::from_slice(&serde_json::to_vec(r).unwrap()).unwrap()
        };
        let again = Replay::decode(&r.encode().unwrap()).unwrap();
        assert_eq!(json(&again), json(&r));
        let mut original = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut original)
            .unwrap();
        let original: serde_json::Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(json(&r), original);
    }
}
