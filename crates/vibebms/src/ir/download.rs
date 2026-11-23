//! The player's own scores read from an IR, filed as vibeBMS's records
//! the way a score database import is (`scoredb`, `Library::import`):
//! the better lamp, EX score, combo and BP kept, never a worse value over
//! a better one, the same scores read again changing nothing.
//!
//! Each score goes under the rule mode and LN mode it was played in, as
//! far as the service says:
//! - a vibeBMS IR (`/players/{id}/scores`): each score's rule profile and
//!   LN mode; a score without a profile is left out.
//! - Bokutachi (the player's PB lists): BMS 7K / 14K are LR2's and
//!   LR2oraja's plays, filed under the LR2 rule mode as LN; PMS
//!   (Keyboard, Controller) are beatoraja's, LN mode not said.
//! - BMS-IR (the player's lamps on the tables followed, lamps only): by
//!   whose scores count (`client_view`): LR2 and OpenLR2 under the LR2
//!   rule mode as LN; LR2oraja and its kin under LR2's, beatoraja under
//!   beatoraja's, LN mode not said; a mix of clients is left out.
//!
//! A score whose LN mode is not said is filed only on a chart whose plays
//! are filed the same in every LN mode (no long notes, or long notes
//! that all have their type); elsewhere it is left out.

use crate::library::{BestScore, RecordKey, Song};
use std::collections::HashMap;
use vibebms_core::judge::JudgePreset;
use vibebms_core::score::ClearType;
use vibebms_core::LnType;
use vibebms_ir::lamps::{LampSet, RemoteLamp, RemoteScore};
use vibebms_ir::rule::Profile;
use vibebms_ir::ProviderId;

/// One score read: its chart (an MD5 or a SHA-256, lower case), the rule
/// mode and LN mode it is filed under (`None`: not said), the best it
/// makes.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoreRow {
    pub hash: String,
    pub rule: JudgePreset,
    pub ln: Option<LnType>,
    pub best: BestScore,
}

/// What one download brought.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Download {
    /// The account, for people.
    pub title: String,
    pub rows: Vec<ScoreRow>,
    /// Scores left out before filing (no rule mode), and why.
    pub left_out: usize,
    pub why: Vec<String>,
}

/// The rule mode a profile's plays are filed under.
fn rule_of_profile(p: Profile) -> JudgePreset {
    p.rules().judge
}

/// The rule mode and LN mode of the scores of a lamp set: provider `p`'s,
/// in `scope` (a Tachi playtype, a BMS-IR table) and `view` (BMS-IR's
/// `client_view`). `None`: they cannot be filed.
pub fn rule_of_set(
    p: ProviderId,
    scope: &str,
    view: &str,
) -> Option<(JudgePreset, Option<LnType>)> {
    match p {
        ProviderId::Bokutachi => match scope {
            "7K" | "14K" => Some((JudgePreset::Lr2, Some(LnType::Ln))),
            "Keyboard" | "Controller" => Some((JudgePreset::Beatoraja, None)),
            _ => None,
        },
        ProviderId::BmsIr => match view {
            // (no view: BMS-IR counts LR2's)
            "" | "lr2" | "openlr2" => Some((JudgePreset::Lr2, Some(LnType::Ln))),
            "all" | "lr2oraja" | "lr2oraja-ed" | "bmz-lr2" => Some((JudgePreset::Lr2, None)),
            "beatoraja" | "bmz-oraja" => Some((JudgePreset::Beatoraja, None)),
            _ => None,
        },
        ProviderId::VibeJson(_) | ProviderId::RianIr => None,
    }
}

/// The best a lamp makes: its lamp, EX score with its counts (when the
/// service gives them), combo and BP, when it was made (Unix ms).
pub fn best_of(l: &RemoteLamp) -> BestScore {
    let at = l.at_ms.map_or(0, |ms| ms.div_euclid(1000));
    let [pg, gr, gd, bd, pr] = l.judges.unwrap_or_default();
    let ex = l.ex.unwrap_or(0);
    let mut b = BestScore {
        clear: l.clear,
        ex_score: ex,
        notes: l.notes.unwrap_or(0),
        judges: [pg, gr, gd, bd, pr, 0],
        max_combo: l.max_combo.unwrap_or(0),
        min_bp: l.bp.unwrap_or(u32::MAX),
        min_cb: if l.clear >= ClearType::FullCombo {
            0
        } else {
            u32::MAX
        },
        play_count: 0,
        clear_count: 0,
        last_played: at,
        lamp_updated: 0,
        score_updated: 0,
    };
    if l.clear > ClearType::Failed {
        b.lamp_updated = at;
    }
    if ex > 0 {
        b.score_updated = at;
    }
    b
}

/// The rows of a lamp set (`rule_of_set`), or why it has none.
pub fn rows_of_set(
    p: ProviderId,
    scope: &str,
    view: &str,
    set: &LampSet,
) -> Result<Vec<ScoreRow>, String> {
    let (rule, ln) = rule_of_set(p, scope, view)
        .ok_or_else(|| format!("the scores of {view:?} mix clients: not filed"))?;
    Ok(set
        .by_chart
        .iter()
        .filter(|(_, l)| l.clear > ClearType::NoPlay || l.ex.is_some_and(|e| e > 0))
        .map(|(hash, l)| ScoreRow {
            hash: hash.to_ascii_lowercase(),
            rule,
            ln,
            best: best_of(l),
        })
        .collect())
}

/// The rows of a vibeBMS IR's scores; how many had no rule profile.
pub fn rows_of_scores(scores: &[RemoteScore]) -> (Vec<ScoreRow>, usize) {
    let mut rows = Vec::new();
    let mut left = 0;
    for s in scores {
        let hash = if s.sha256.is_empty() {
            &s.md5
        } else {
            &s.sha256
        };
        match s.profile.filter(|_| !hash.is_empty()) {
            Some(p) => rows.push(ScoreRow {
                hash: hash.to_ascii_lowercase(),
                rule: rule_of_profile(p),
                ln: s.ln,
                best: best_of(&s.score),
            }),
            None => left += 1,
        }
    }
    (rows, left)
}

/// What filing the rows does.
#[derive(Debug, Default, PartialEq)]
pub struct Filed {
    pub records: Vec<(RecordKey, BestScore)>,
    /// Rows of charts not in the library.
    pub unknown: usize,
    /// Rows whose LN mode is not said, on charts where it matters.
    pub ln_unknown: usize,
}

/// The records `rows` make for the charts of `songs` (by SHA-256 or MD5),
/// each under the LN mode the chart's plays of its LN mode are filed
/// under; rows that fall on one record are taken together.
pub fn records(rows: &[ScoreRow], songs: &[Song]) -> Filed {
    let charts = crate::charts::Charts::new(songs);
    let mut f = Filed::default();
    let mut at: HashMap<RecordKey, usize> = HashMap::new();
    for r in rows {
        let found = (charts.by_sha(&r.hash)).or_else(|| charts.by_md5(songs, &r.hash, "", ""));
        let Some(song) = found.map(|i| &songs[i]) else {
            f.unknown += 1;
            continue;
        };
        let ln = match r.ln {
            Some(ln) => song.score_ln(ln),
            None => {
                let [a, b, c] = LnType::ALL.map(|l| song.score_ln(l));
                if a != b || a != c {
                    f.ln_unknown += 1;
                    continue;
                }
                a
            }
        };
        let mut best = r.best.clone();
        if best.notes == 0 && best.ex_score > 0 {
            best.notes = song.notes.max(0) as u32;
        }
        let key = RecordKey::new(&song.sha256, ln as i32, r.rule);
        match at.get(&key) {
            Some(&i) => f.records[i].1 = f.records[i].1.absorb(&best),
            None => {
                at.insert(key.clone(), f.records.len());
                f.records.push((key, best));
            }
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibebms_ir::lamps::RemoteLamp;

    fn song(sha: char, md5: char, ln_profile: Option<u8>, has_ln: bool) -> Song {
        Song {
            sha256: sha.to_string().repeat(64),
            md5: md5.to_string().repeat(32),
            notes: 1000,
            has_ln,
            ln_profile,
            ..Default::default()
        }
    }

    fn lamp(clear: ClearType, ex: Option<u32>) -> RemoteLamp {
        RemoteLamp {
            clear,
            ex,
            bp: Some(7),
            at_ms: Some(1_767_225_600_123),
            ..Default::default()
        }
    }

    #[test]
    fn sets_are_filed_by_service_scope_and_view() {
        use JudgePreset::*;
        let ln = Some(LnType::Ln);
        assert_eq!(
            rule_of_set(ProviderId::Bokutachi, "14K", ""),
            Some((Lr2, ln))
        );
        assert_eq!(
            rule_of_set(ProviderId::Bokutachi, "Controller", "Controller"),
            Some((Beatoraja, None))
        );
        assert_eq!(
            rule_of_set(ProviderId::BmsIr, "official", ""),
            Some((Lr2, ln))
        );
        assert_eq!(
            rule_of_set(ProviderId::BmsIr, "x", "lr2oraja"),
            Some((Lr2, None))
        );
        assert_eq!(
            rule_of_set(ProviderId::BmsIr, "x", "beatoraja"),
            Some((Beatoraja, None))
        );
        assert_eq!(rule_of_set(ProviderId::BmsIr, "x", "all_clients"), None);
        assert_eq!(rule_of_set(ProviderId::BmsIr, "x", "bmz-player"), None);
        assert_eq!(rule_of_set(ProviderId::VibeJson(0), "", ""), None);
    }

    #[test]
    fn a_lamp_makes_a_best() {
        let mut l = lamp(ClearType::FullCombo, Some(1500));
        l.judges = Some([700, 100, 3, 0, 0]);
        l.max_combo = Some(803);
        let b = best_of(&l);
        assert_eq!(
            (b.clear, b.ex_score, b.min_bp, b.min_cb),
            (ClearType::FullCombo, 1500, 7, 0)
        );
        assert_eq!((b.judges, b.max_combo), ([700, 100, 3, 0, 0, 0], 803));
        assert_eq!(
            (b.last_played, b.lamp_updated, b.score_updated),
            (1_767_225_600, 1_767_225_600, 1_767_225_600)
        );
        assert_eq!((b.play_count, b.clear_count), (0, 0));
        // a lamp alone (BMS-IR)
        let b = best_of(&RemoteLamp {
            clear: ClearType::Hard,
            ..Default::default()
        });
        assert_eq!(
            (b.ex_score, b.min_bp, b.min_cb, b.last_played),
            (0, u32::MAX, u32::MAX, 0)
        );
    }

    /// Rows go to their charts by either hash, under the LN mode their
    /// plays are filed under; a row whose LN mode is not said only where
    /// it does not matter.
    #[test]
    fn rows_are_filed_on_the_library() {
        // a chart without long notes, one with untyped long notes
        let songs = vec![song('a', 'b', None, false), song('c', 'd', None, true)];
        let rows = |hash: String, ln| ScoreRow {
            hash,
            rule: JudgePreset::Beatoraja,
            ln,
            best: best_of(&lamp(ClearType::Hard, Some(900))),
        };
        let f = records(
            &[
                rows("b".repeat(32), None),
                rows("a".repeat(64), Some(LnType::Cn)),
                rows("c".repeat(64), None),
                rows("c".repeat(64), Some(LnType::Hcn)),
                rows("e".repeat(32), Some(LnType::Ln)),
            ],
            &songs,
        );
        assert_eq!((f.unknown, f.ln_unknown), (1, 1));
        let keys: Vec<(String, i32)> = f
            .records
            .iter()
            .map(|(k, _)| (k.id[..1].to_string(), k.ln))
            .collect();
        // the chart without long notes: both rows on its one record
        assert_eq!(keys, [("a".into(), 0), ("c".into(), LnType::Hcn as i32)]);
        // its notes from the library where the service gave none
        assert_eq!(f.records[0].1.notes, 1000);
    }

    /// A row by an MD5 two charts share goes to the first of them, as
    /// everywhere a chart is found by its hash.
    #[test]
    fn an_md5_of_two_charts_is_the_first() {
        let songs = vec![song('a', 'b', None, false), song('c', 'b', None, false)];
        let row = ScoreRow {
            hash: "b".repeat(32),
            rule: JudgePreset::Beatoraja,
            ln: None,
            best: best_of(&lamp(ClearType::Hard, Some(900))),
        };
        let f = records(&[row], &songs);
        assert_eq!(f.records[0].0.id, "a".repeat(64));
    }

    #[test]
    fn vibe_scores_need_a_rule_profile() {
        let s = |profile, ln| RemoteScore {
            sha256: "A".repeat(64),
            md5: String::new(),
            ln,
            profile,
            score: lamp(ClearType::Easy, Some(10)),
        };
        let (rows, left) = rows_of_scores(&[
            s(Some(Profile::VibeLr2), Some(LnType::Ln)),
            s(Some(Profile::VibeBeatoraja), Some(LnType::Cn)),
            s(None, Some(LnType::Ln)),
        ]);
        assert_eq!(left, 1);
        assert_eq!(rows[0].rule, JudgePreset::Lr2);
        assert_eq!(
            (rows[1].rule, rows[1].ln),
            (JudgePreset::Beatoraja, Some(LnType::Cn))
        );
        assert_eq!(rows[0].hash, "a".repeat(64));
    }

    /// Taken in as a database's records are: better values only, and
    /// again changes nothing.
    #[test]
    fn a_download_keeps_better_local_records_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!("vibebms-ir-download-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let mut lib = crate::library::Library::open(&root, &root, |_, _| None);
        let songs = vec![song('a', 'b', None, false)];
        let key = RecordKey::new(&"a".repeat(64), 0, JudgePreset::Lr2);
        let local = BestScore {
            clear: ClearType::ExHard,
            ex_score: 1200,
            notes: 1000,
            judges: [500, 200, 0, 0, 0, 0],
            max_combo: 300,
            min_bp: 20,
            min_cb: 9,
            play_count: 4,
            clear_count: 3,
            ..Default::default()
        };
        lib.import(&[(key.clone(), local.clone())]);
        let mut remote = lamp(ClearType::Hard, Some(1500));
        remote.bp = Some(25);
        let set = LampSet {
            by_chart: [("b".repeat(32), remote)].into_iter().collect(),
            ..Default::default()
        };
        let rows = rows_of_set(ProviderId::BmsIr, "official", "lr2", &set).unwrap();
        let f = records(&rows, &songs);
        assert_eq!(lib.import(&f.records), (0, 1));
        let b = lib.scores()[&key].clone();
        // the better EX score taken, the better lamp and BP kept
        assert_eq!(
            (b.clear, b.ex_score, b.min_bp, b.min_cb),
            (ClearType::ExHard, 1500, 20, 9)
        );
        assert_eq!((b.play_count, b.clear_count, b.max_combo), (4, 3, 300));
        // read again: nothing changes
        assert_eq!(lib.import(&records(&rows, &songs).records), (0, 0));
        let _ = std::fs::remove_dir_all(root);
    }
}
