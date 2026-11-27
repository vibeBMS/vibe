//! The replays kept (`vibebms_core::replay`): each chart (by SHA-256, LN
//! mode and rule mode, as its record) keeps its latest play's replay and
//! its best EX score's, and up to four the player saved (slots 1-4, from
//! the results), in `<root>/scores/replays/`. A course's stages and a
//! NONSTOP course's play are kept the same way under the course
//! (`PlayRequest::replay_key`).

use crate::config::PlayConfig;
use std::path::{Path, PathBuf};
use vibebms_core::judge::JudgePreset;
use vibebms_core::replay::Slot;
pub use vibebms_core::replay::VERSION;

/// A replay with the play options it was played with (rules and view).
pub type Replay = vibebms_core::replay::Replay<PlayConfig>;

/// The folder replays are kept in.
pub fn dir(root: &Path) -> PathBuf {
    root.join("scores").join("replays")
}

/// The file of a replay: `<sha256>_<ln>_<rule>_<slot>`; replays from
/// before rule modes have no `<rule>` (`rule: None`).
fn file(root: &Path, sha256: &str, ln: i32, rule: Option<JudgePreset>, slot: Slot) -> PathBuf {
    let rule = match rule {
        Some(JudgePreset::Beatoraja) => "beatoraja_",
        Some(JudgePreset::Lr2) => "lr2_",
        Some(JudgePreset::Iidx) => "iidx_",
        None => "",
    };
    dir(root).join(format!("{sha256}_{ln}_{rule}{}.json.gz", slot.name()))
}

/// Keep `r` as `slot` of its chart (under the rule mode it was played
/// in).
pub fn save(r: &Replay, root: &Path, slot: Slot) -> Result<(), String> {
    let bytes = r.encode()?;
    let path = file(root, &r.sha256, r.ln, Some(r.play.judge_preset), slot);
    if !crate::platform::IS_WEB {
        crate::platform::vfs::create_dir_all(&dir(root)).map_err(|e| e.to_string())?;
    }
    write(&path, bytes)
}

fn read(path: &Path) -> Option<Replay> {
    use crate::platform::vfs;
    // (a file larger than a replay unpacks to is none: not read at all)
    if vfs::stamp(path).1 > vibebms_core::replay::MAX_JSON as i64 {
        return None;
    }
    Replay::decode(&vfs::read(path).ok()?)
}

/// The chart's replay `slot` under rule mode `rule`, if there is one: a
/// replay from before rule modes counts when it was played in `rule`.
pub fn load(root: &Path, sha256: &str, ln: i32, rule: JudgePreset, slot: Slot) -> Option<Replay> {
    read(&file(root, sha256, ln, Some(rule), slot)).or_else(|| {
        read(&file(root, sha256, ln, None, slot)).filter(|r| r.play.judge_preset == rule)
    })
}

/// The rule mode of a chart's replay `slot` from before rule modes
/// (`Library::open` files the record of before under its best play's).
pub fn rule_of_old(root: &Path, sha256: &str, ln: i32, slot: Slot) -> Option<JudgePreset> {
    read(&file(root, sha256, ln, None, slot)).map(|r| r.play.judge_preset)
}

/// The replay `slot` filed under LN mode `ln` and rule mode `rule`.
/// `fixed`: the chart's long notes all have their type, and replays from
/// before vibe filed them under the chart's mode may be under any (the
/// best EX score's, or the latest, of those).
pub fn load_filed(
    root: &Path,
    sha256: &str,
    ln: i32,
    rule: JudgePreset,
    fixed: bool,
    slot: Slot,
) -> Option<Replay> {
    if let Some(r) = load(root, sha256, ln, rule, slot) {
        return Some(r);
    }
    if !fixed {
        return None;
    }
    (0..3)
        .filter(|k| *k != ln)
        .filter_map(|k| load(root, sha256, k, rule, slot))
        .max_by_key(|r| match slot {
            Slot::Best => (r.ex_score as i64, r.date),
            Slot::Last | Slot::Saved(_) => (r.date, r.ex_score as i64),
        })
}

/// Whether `load_filed` finds a replay of the chart in any slot (the
/// select screen dims its REPLAY button without one).
pub fn exists(root: &Path, sha256: &str, ln: i32, rule: JudgePreset, fixed: bool) -> bool {
    let lns = if fixed { 0..3 } else { ln..ln + 1 };
    let is = |p: PathBuf| crate::platform::vfs::is_file(&p);
    lns.into_iter().any(|ln| {
        Slot::ALL.into_iter().any(|slot| {
            is(file(root, sha256, ln, Some(rule), slot))
                || is(file(root, sha256, ln, None, slot))
                    && load(root, sha256, ln, rule, slot).is_some()
        })
    })
}

/// Why `r` cannot be a G-BATTLE opponent on `song` for a player in LN
/// mode `ln` (`None`: it can). The opponent plays its inputs again on the
/// second side, so the chart and the judge must come out as they did.
pub fn ghost_refusal(
    r: &Replay,
    song: &crate::library::Song,
    ln: impl Into<vibebms_core::LnPref>,
) -> Option<&'static str> {
    let ln = ln.into();
    use vibebms_core::PlayMode;
    let five_or_seven = |m| matches!(m, PlayMode::Beat7K | PlayMode::Beat5K);
    // (a KEY CONVERT play's chart is in the mode it converted to)
    let played = vibebms_core::convert::target(song.mode, r.play.key_convert);
    Some(if !five_or_seven(song.mode) || !five_or_seven(played) {
        tr!("G-BATTLE needs a 5 or 7 key chart")
    } else if r.play.battle {
        tr!("the replay is a BATTLE play")
    } else if r.ops.is_empty() {
        tr!("the replay has no inputs")
    } else if r.play.legacy_note {
        tr!("the replay is a LEGACY NOTE play")
    } else if !song.plays_ln_alike(r.play.ln_pref(), ln) {
        tr!("the replay was played in another LN mode")
    } else {
        return None;
    })
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
            course: None,
        };
        let load = |slot| load(&root, "ab", 0, JudgePreset::Beatoraja, slot);
        save(&r, &root, Slot::Best).unwrap();
        let back = load(Slot::Best).unwrap();
        assert_eq!(back.ops, r.ops);
        assert_eq!(back.seed, 42);
        assert_eq!(back.ex_at(2), 3);
        assert!(load(Slot::Last).is_none());
        assert!(!back.older_engine());
        // an older judge's replay still loads (its ghost paces), marked
        let old = Replay {
            version: 1,
            ..r.clone()
        };
        save(&old, &root, Slot::Last).unwrap();
        let back = load(Slot::Last).unwrap();
        assert!(back.older_engine());
        assert_eq!(back.ghost, r.ghost);
        // a format not known: not loaded
        let future = Replay {
            version: VERSION + 1,
            ..r.clone()
        };
        save(&future, &root, Slot::Last).unwrap();
        assert!(load(Slot::Last).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// A file larger than any replay is not read (here: a replay with
    /// nothing after it to past the limit, which reads when it is read).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_file_too_large_is_not_read() {
        let root = std::env::temp_dir().join(format!("vibebms-replay-big-{}", std::process::id()));
        let r = Replay {
            version: VERSION,
            sha256: "ab".into(),
            md5: String::new(),
            title: String::new(),
            ln: 0,
            date: 1,
            seed: 1,
            play: PlayConfig::default(),
            ex_score: 0,
            clear: ClearType::Failed,
            ghost: Vec::new(),
            ops: vec![Op::Update(5)],
            course: None,
        };
        save(&r, &root, Slot::Last).unwrap();
        let rule = JudgePreset::Beatoraja;
        assert!(load(&root, "ab", 0, rule, Slot::Last).is_some());
        let path = file(&root, "ab", 0, Some(rule), Slot::Last);
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_len(vibebms_core::replay::MAX_JSON as u64 + 1)
            .unwrap();
        drop(f);
        assert!(load(&root, "ab", 0, rule, Slot::Last).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// The four saved slots are kept apart from the latest and the best,
    /// per chart, LN mode and rule mode; one alone makes the chart's
    /// replays exist.
    #[test]
    fn saved_slots() {
        let root = std::env::temp_dir().join(format!("vibebms-replay-slot-{}", std::process::id()));
        let r = |ex: u32| Replay {
            version: VERSION,
            sha256: "ab".into(),
            md5: String::new(),
            title: "Song".into(),
            ln: 1,
            date: ex as i64,
            seed: 1,
            play: PlayConfig::default(),
            ex_score: ex,
            clear: ClearType::Easy,
            ghost: Vec::new(),
            ops: vec![Op::Update(5)],
            course: None,
        };
        let rule = JudgePreset::Beatoraja;
        assert!(!exists(&root, "ab", 1, rule, false));
        save(&r(7), &root, Slot::Saved(3)).unwrap();
        assert!(exists(&root, "ab", 1, rule, false));
        assert!(!exists(&root, "ab", 0, rule, false));
        assert!(!exists(&root, "ab", 1, JudgePreset::Lr2, false));
        let get = |slot| load(&root, "ab", 1, rule, slot).map(|r| r.ex_score);
        assert_eq!(get(Slot::Saved(3)), Some(7));
        assert_eq!(
            (get(Slot::Saved(1)), get(Slot::Last), get(Slot::Best)),
            (None, None, None)
        );
        // a slot saved again holds the new replay
        save(&r(9), &root, Slot::Saved(3)).unwrap();
        save(&r(4), &root, Slot::Last).unwrap();
        assert_eq!((get(Slot::Saved(3)), get(Slot::Last)), (Some(9), Some(4)));
        // a chart whose long notes all have their type: found under any
        assert_eq!(
            load_filed(&root, "ab", 0, rule, true, Slot::Saved(3)).map(|r| r.ex_score),
            Some(9)
        );
        let labels: Vec<String> = Slot::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(
            labels,
            ["MY BEST", "LAST", "SLOT 1", "SLOT 2", "SLOT 3", "SLOT 4"]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// Each rule mode keeps its own replays; one from before rule modes
    /// counts for the rule it was played in.
    #[test]
    fn replays_per_rule_mode() {
        let root = std::env::temp_dir().join(format!("vibebms-replay-rule-{}", std::process::id()));
        let mut r = Replay {
            version: VERSION,
            sha256: "ab".into(),
            md5: String::new(),
            title: String::new(),
            ln: 0,
            date: 1,
            seed: 1,
            play: PlayConfig::default(),
            ex_score: 3,
            clear: ClearType::Easy,
            ghost: Vec::new(),
            ops: Vec::new(),
            course: None,
        };
        r.play.judge_preset = JudgePreset::Lr2;
        save(&r, &root, Slot::Best).unwrap();
        let get = |rule| load(&root, "ab", 0, rule, Slot::Best).map(|r| r.ex_score);
        assert_eq!(get(JudgePreset::Lr2), Some(3));
        assert_eq!(get(JudgePreset::Beatoraja), None);
        // an IIDX play of before, in the file of before
        let old = Replay {
            ex_score: 9,
            play: PlayConfig {
                rules: vibebms_core::play::RuleOptions {
                    judge_preset: JudgePreset::Iidx,
                    ..Default::default()
                },
                ..PlayConfig::default()
            },
            ..r.clone()
        };
        let path = file(&root, "ab", 0, None, Slot::Best);
        std::fs::write(&path, old.encode().unwrap()).unwrap();
        assert_eq!(get(JudgePreset::Iidx), Some(9));
        assert_eq!(get(JudgePreset::Beatoraja), None);
        assert_eq!(get(JudgePreset::Lr2), Some(3));
        assert_eq!(
            rule_of_old(&root, "ab", 0, Slot::Best),
            Some(JudgePreset::Iidx)
        );
        // a newer IIDX play replaces it
        let new = Replay {
            ex_score: 10,
            ..old
        };
        save(&new, &root, Slot::Best).unwrap();
        assert_eq!(get(JudgePreset::Iidx), Some(10));
        let _ = std::fs::remove_dir_all(root);
    }

    /// A replay written before the play options were split keeps all of
    /// them, and is written again with the same JSON.
    #[test]
    fn a_replay_of_before_reads_whole() {
        let bytes = include_bytes!("../../../vibebms-core/tests/data/replay/v2.json.gz");
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
