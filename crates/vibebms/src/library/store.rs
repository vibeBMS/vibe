//! The player's records, totals and favourites (`Library`): `scores.json`,
//! `player.json` and `favorites.json` in the profile's data folder.

use super::scan::cached_songs;
use super::{BestScore, Favorites, RecordKey, Song, Totals};
use crate::platform::vfs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use vibebms_core::judge::{Judge, JudgePreset};
use vibebms_core::score::{ClearType, Score};
use vibebms_core::LnType;

#[derive(Serialize, Deserialize)]
struct ScoreRecord {
    sha256: String,
    ln: i32,
    /// `None` in files from before records were kept per rule mode.
    #[serde(default)]
    rule: Option<JudgePreset>,
    #[serde(flatten)]
    best: BestScore,
}

pub struct Library {
    songs_file: PathBuf,
    scores_file: PathBuf,
    scores: HashMap<RecordKey, BestScore>,
    totals_file: PathBuf,
    totals: Totals,
    favorites_file: PathBuf,
    favorites: Favorites,
}

pub(super) fn save_json(path: &Path, v: &impl Serialize, what: &str) {
    match serde_json::to_vec_pretty(v) {
        Ok(j) => {
            if let Err(e) = vfs::write(path, &j) {
                log::error!("saving {what}: {e}");
            }
        }
        Err(e) => log::error!("saving {what}: {e}"),
    }
}

/// The score records, one by one: a record that does not read is dropped,
/// and the file is copied to `scores.json.bak` first (the next save
/// rewrites it).
fn load_scores(file: &Path) -> Vec<ScoreRecord> {
    let Ok(b) = vfs::read(file) else {
        return Vec::new();
    };
    let (records, bad) = match serde_json::from_slice::<Vec<serde_json::Value>>(&b) {
        Ok(list) => {
            let n = list.len();
            let ok: Vec<ScoreRecord> = list
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect();
            let bad = n - ok.len();
            (ok, bad)
        }
        Err(_) => (Vec::new(), 1),
    };
    if bad > 0 {
        let bak = file.with_extension("json.bak");
        log::error!(
            "{}: {bad} unreadable record(s); the file is saved as {}",
            file.display(),
            bak.display()
        );
        let _ = vfs::write(&bak, &b);
    }
    records
}

/// The records in the score file of `data` (another profile's, as a local
/// rival), read only: a record that does not read is left out, one from
/// before records were kept per rule mode is the default rule's, and
/// nothing is written.
pub fn read_records(data: &Path) -> HashMap<RecordKey, BestScore> {
    let list: Vec<serde_json::Value> = vfs::read(&data.join("scores.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut scores: HashMap<RecordKey, BestScore> = HashMap::new();
    for r in list
        .into_iter()
        .filter_map(|v| serde_json::from_value::<ScoreRecord>(v).ok())
    {
        let key = RecordKey {
            id: r.sha256,
            ln: r.ln,
            rule: r.rule.unwrap_or_default(),
        };
        let best = match scores.remove(&key) {
            Some(b) => b.merge(&r.best),
            None => r.best,
        };
        scores.insert(key, best);
    }
    scores
}

impl Library {
    /// `cache` holds the scan cache, `data` the score file. A record from
    /// before records were kept per rule mode goes under `rule_of`'s rule
    /// for it (its best play's replay says what that play was played
    /// with), else under the default (beatoraja), merged with what is
    /// there; the file is written again at once.
    pub fn open(
        cache: &Path,
        data: &Path,
        rule_of: impl Fn(&str, i32) -> Option<JudgePreset>,
    ) -> Library {
        let scores_file = data.join("scores.json");
        let mut scores: HashMap<RecordKey, BestScore> = HashMap::new();
        let mut migrated = false;
        for r in load_scores(&scores_file) {
            let rule = r.rule.unwrap_or_else(|| {
                migrated = true;
                rule_of(&r.sha256, r.ln).unwrap_or_default()
            });
            let key = RecordKey {
                id: r.sha256,
                ln: r.ln,
                rule,
            };
            let best = match scores.remove(&key) {
                Some(b) => b.merge(&r.best),
                None => r.best,
            };
            scores.insert(key, best);
        }
        let totals_file = data.join("player.json");
        let mut totals: Totals = vfs::read(&totals_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if totals.judges.is_none() {
            let mut j = [0u64; 6];
            for b in scores.values() {
                for (t, n) in j.iter_mut().zip(b.judges) {
                    *t += n as u64;
                }
            }
            totals.judges = Some(j);
        }
        // the files of the archives in the library (`platform::arcfs`)
        crate::platform::arcfs::load_index(&cache.join("archives.json"));
        let favorites_file = data.join("favorites.json");
        let favorites = vfs::read(&favorites_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let lib = Library {
            songs_file: cache.join("songs.json"),
            scores_file,
            scores,
            totals_file,
            totals,
            favorites_file,
            favorites,
        };
        if migrated {
            lib.save_scores();
        }
        lib
    }

    pub fn totals(&self) -> &Totals {
        &self.totals
    }

    pub fn favorites(&self) -> &Favorites {
        &self.favorites
    }

    /// Make `s` (`song`: its song) a favourite, or one no more; whether it
    /// is now.
    pub fn toggle_favorite(&mut self, s: &Song, song: bool) -> bool {
        let (set, key) = match song {
            true => (&mut self.favorites.songs, Favorites::song_of(s)),
            false => (&mut self.favorites.charts, s.sha256.clone()),
        };
        let on = !set.remove(&key);
        if on {
            set.insert(key);
        }
        save_json(&self.favorites_file, &self.favorites, "favourites");
        on
    }

    pub fn songs_file(&self) -> &Path {
        &self.songs_file
    }

    /// The charts (`cached_songs`).
    pub fn songs(&self) -> Vec<Song> {
        cached_songs(&self.songs_file)
    }

    /// The records.
    pub fn scores(&self) -> HashMap<RecordKey, BestScore> {
        self.scores.clone()
    }

    fn save_scores(&self) {
        let mut v: Vec<ScoreRecord> = self
            .scores
            .iter()
            .map(|(k, b)| ScoreRecord {
                sha256: k.id.clone(),
                ln: k.ln,
                rule: Some(k.rule),
                best: b.clone(),
            })
            .collect();
        let order = |r: &ScoreRecord| (r.ln, r.rule.map(|p| p as u8));
        v.sort_by(|a, b| a.sha256.cmp(&b.sha256).then(order(a).cmp(&order(b))));
        save_json(&self.scores_file, &v, "scores");
    }

    /// Records of charts whose long notes all have their type, filed
    /// under the player's LN mode before vibe filed them under the
    /// chart's: merged into the chart's (they were played as it).
    pub fn merge_fixed_ln(&mut self, songs: &[Song]) {
        let mut changed = false;
        for song in songs.iter().filter(|s| s.fixed_ln()) {
            let ln = song.score_ln(LnType::Ln) as i32;
            for rule in JudgePreset::ALL {
                for other in (0..3).filter(|k| *k != ln) {
                    let Some(old) = self
                        .scores
                        .remove(&RecordKey::new(&song.sha256, other, rule))
                    else {
                        continue;
                    };
                    let k = RecordKey::new(&song.sha256, ln, rule);
                    let merged = match self.scores.get(&k) {
                        Some(b) => b.merge(&old),
                        None => old,
                    };
                    self.scores.insert(k, merged);
                    changed = true;
                }
            }
        }
        if changed {
            self.save_scores();
        }
    }

    /// Records read from another player's database (`scoredb`), each
    /// taken into the one of its key (`BestScore::absorb`): how many were
    /// new and how many changed.
    pub fn import(&mut self, records: &[(RecordKey, BestScore)]) -> (usize, usize) {
        let (mut new, mut changed) = (0, 0);
        for (key, o) in records {
            let b = match self.scores.get(key) {
                Some(b) => {
                    let m = b.absorb(o);
                    if m == *b {
                        continue;
                    }
                    changed += 1;
                    m
                }
                None => {
                    new += 1;
                    BestScore::none().absorb(o)
                }
            };
            self.scores.insert(key.clone(), b);
        }
        if new + changed > 0 {
            self.save_scores();
        }
        (new, changed)
    }

    /// An assisted play (`AssistFacts`): its lamp, the play and clear
    /// counts and the totals, but no best score, combo or BP.
    pub fn record_assisted(
        &mut self,
        key: &RecordKey,
        s: &Score,
        clear: ClearType,
        play_ms: u64,
    ) -> BestScore {
        let mut b = self
            .scores
            .get(key)
            .cloned()
            .unwrap_or_else(BestScore::none);
        let now = crate::platform::unix_now() as i64;
        b.raise_lamp(clear, now);
        b.play_count += 1;
        b.clear_count += (clear > ClearType::Failed) as u32;
        b.last_played = now;
        self.scores.insert(key.clone(), b.clone());
        self.save_scores();
        self.add_totals(s, play_ms);
        b
    }

    fn add_totals(&mut self, s: &Score, play_ms: u64) {
        let j = self.totals.judges.get_or_insert_default();
        for (t, k) in j.iter_mut().zip(Judge::ALL) {
            *t += s.count(k) as u64;
        }
        self.totals.play_ms += play_ms;
        save_json(&self.totals_file, &self.totals, "totals");
    }

    /// A stage of an expert or class course into its chart's record
    /// `key`, partly (S-OLR2b 4.4): the best EX score with its counts
    /// (strictly higher), max combo, min BP and min CB, the lamp only to
    /// FULL COMBO; no play count (the course's result counts the play).
    pub fn record_stage(&mut self, key: &RecordKey, s: &Score, clear: ClearType) -> BestScore {
        let mut b = self
            .scores
            .get(key)
            .cloned()
            .unwrap_or_else(BestScore::none);
        let now = crate::platform::unix_now() as i64;
        if s.ex_score() > b.ex_score {
            b.ex_score = s.ex_score();
            b.judges = Judge::ALL.map(|j| s.count(j));
            b.score_updated = now;
        }
        if clear >= ClearType::FullCombo {
            b.raise_lamp(ClearType::FullCombo, now);
        }
        b.notes = s.total_notes;
        b.max_combo = b.max_combo.max(s.max_combo);
        b.min_bp = b.min_bp.min(s.miss_count());
        b.min_cb = b.min_cb.min(combo_breaks(s));
        self.scores.insert(key.clone(), b.clone());
        self.save_scores();
        b
    }

    /// Merge a play into the record `key` and the totals (`play_ms`: how
    /// long it was played). Returns the updated record. A course's record
    /// is one too, under the course's identity.
    pub fn record_play(
        &mut self,
        key: &RecordKey,
        s: &Score,
        clear: ClearType,
        play_ms: u64,
    ) -> BestScore {
        let old = self.scores.get(key).cloned();
        let mut b = old.clone().unwrap_or_else(BestScore::none);
        let now = crate::platform::unix_now() as i64;
        if s.ex_score() > b.ex_score || old.is_none() {
            if s.ex_score() > b.ex_score {
                b.score_updated = now;
            }
            b.ex_score = s.ex_score();
            b.judges = Judge::ALL.map(|j| s.count(j));
        }
        b.raise_lamp(clear, now);
        b.notes = s.total_notes;
        b.max_combo = b.max_combo.max(s.max_combo);
        b.min_bp = b.min_bp.min(s.miss_count());
        b.min_cb = b.min_cb.min(combo_breaks(s));
        b.play_count += 1;
        b.clear_count += (clear > ClearType::Failed) as u32;
        b.last_played = now;
        self.scores.insert(key.clone(), b.clone());
        self.save_scores();
        self.add_totals(s, play_ms);
        b
    }
}

/// A play's combo breaks, the notes it never reached among them (as its
/// miss count has them).
fn combo_breaks(s: &Score) -> u32 {
    s.combo_breaks + s.total_notes.saturating_sub(s.passed)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use vibebms_core::LnProfile;

    /// The library at `dir` (its records from before rule modes under the
    /// default).
    pub(crate) fn open(dir: &Path) -> Library {
        Library::open(dir, dir, |_, _| None)
    }

    pub(crate) fn key(id: &str, ln: i32) -> RecordKey {
        RecordKey::new(id, ln, JudgePreset::Beatoraja)
    }

    #[test]
    fn totals_seeded_then_added() {
        let dir = std::env::temp_dir().join(format!("vibebms-totals-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // a library from before totals: two charts' best plays
        let rec = |sha: &str, j: [u32; 6]| serde_json::json!({"sha256": sha, "ln": 0, "judges": j, "play_count": 3});
        let old = serde_json::json!([rec("a", [10, 5, 2, 1, 3, 4]), rec("b", [1, 1, 1, 1, 1, 1])]);
        std::fs::write(dir.join("scores.json"), old.to_string()).unwrap();
        std::fs::write(dir.join("player.json"), r#"{"play_ms": 5000}"#).unwrap();
        let mut lib = open(&dir);
        assert_eq!(lib.totals().judges, Some([11, 6, 3, 2, 4, 5]));
        assert_eq!((lib.totals().notes(), lib.totals().play_ms), (22, 5000));
        let mut sc = Score {
            total_notes: 3,
            passed: 3,
            ..Default::default()
        };
        sc.add(Judge::PGreat, 0);
        sc.add(Judge::PGreat, 1);
        sc.add(Judge::Poor, 0);
        sc.add(Judge::Miss, 0);
        // a better play of "a": the best changes, the totals only add up
        lib.record_play(&key("a", 0), &sc, ClearType::Easy, 61_000);
        let want = Totals {
            judges: Some([13, 6, 3, 2, 5, 6]),
            play_ms: 66_000,
        };
        assert_eq!(*lib.totals(), want);
        // kept, not seeded again
        assert_eq!(*open(&dir).totals(), want);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn records_of_typed_long_notes_merge_into_the_charts_mode() {
        let dir = std::env::temp_dir().join(format!("vibebms-lnkey-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rec = |ln: i32, ex: u32, clear: &str, plays: u32| {
            serde_json::json!({"sha256": "c", "ln": ln, "ex_score": ex, "clear": clear,
                "play_count": plays, "min_bp": 9})
        };
        let old = serde_json::json!([rec(0, 100, "Hard", 2), rec(2, 120, "Easy", 1)]);
        std::fs::write(dir.join("scores.json"), old.to_string()).unwrap();
        let mut lib = open(&dir);
        // #LNMODE 2: every long note a CN
        let cn = Song {
            sha256: "c".into(),
            has_ln: true,
            ln_profile: Some(
                LnProfile {
                    undefined: false,
                    typed: [false, true, false],
                }
                .bits(),
            ),
            ..Default::default()
        };
        assert_eq!(cn.score_ln(LnType::Hcn), LnType::Cn);
        lib.merge_fixed_ln(std::slice::from_ref(&cn));
        let s = open(&dir).scores();
        assert_eq!(s.len(), 1);
        let b = &s[&key("c", 1)];
        assert_eq!(
            (b.ex_score, b.clear, b.play_count),
            (120, ClearType::Hard, 3)
        );
        // a chart whose long notes take the player's mode keeps them apart
        let free = Song {
            sha256: "c".into(),
            has_ln: true,
            ln_profile: Some(
                LnProfile {
                    undefined: true,
                    typed: [false; 3],
                }
                .bits(),
            ),
            ..Default::default()
        };
        assert!(!free.fixed_ln());
        assert_eq!(free.score_ln(LnType::Hcn), LnType::Hcn);
        // no long notes: LN
        assert_eq!(Song::default().score_ln(LnType::Cn), LnType::Ln);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Records from before rule modes: under the rule of their best
    /// play's replay where there is one, else the default; written with
    /// their rule at once, and read back the same.
    #[test]
    fn records_of_before_rule_modes_are_filed_by_rule() {
        let dir = std::env::temp_dir().join(format!("vibebms-rules-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rec = |sha: &str, ex: u32| {
            serde_json::json!({"sha256": sha, "ln": 0, "ex_score": ex,
            "clear": "Hard", "min_bp": 7, "play_count": 2})
        };
        let old = serde_json::json!([rec("lr2", 100), rec("plain", 50)]);
        std::fs::write(dir.join("scores.json"), old.to_string()).unwrap();
        let rule_of = |sha: &str, _ln: i32| (sha == "lr2").then_some(JudgePreset::Lr2);
        let lib = Library::open(&dir, &dir, rule_of);
        let s = lib.scores();
        let lr2 = RecordKey::new("lr2", 0, JudgePreset::Lr2);
        assert_eq!(s[&lr2].ex_score, 100);
        assert_eq!(s[&key("plain", 0)].ex_score, 50);
        assert_eq!(s.len(), 2);
        // a min CB was not kept then
        assert_eq!((s[&lr2].min_bp, s[&lr2].min_cb), (7, u32::MAX));
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("scores.json")).unwrap()).unwrap();
        let rules: Vec<&str> = written
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["rule"].as_str().unwrap())
            .collect();
        assert_eq!(rules, ["Lr2", "Beatoraja"]);
        // read again: the rule is the record's own now
        assert_eq!(open(&dir).scores(), s);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Plays under each rule mode have their own record; the min BP and
    /// min CB are bests of their own (not the best EX play's), a failed
    /// play's counting the notes it never reached, an assisted play's not
    /// at all.
    #[test]
    fn records_per_rule_mode_and_min_cb() {
        let dir = std::env::temp_dir().join(format!("vibebms-mincb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut lib = open(&dir);
        let play = |pg: u32, bad: u32, cb: u32, passed: u32| {
            let mut s = Score {
                total_notes: 10,
                passed,
                combo_breaks: cb,
                ..Default::default()
            };
            for _ in 0..pg {
                s.add(Judge::PGreat, 0);
            }
            for _ in 0..bad {
                s.add(Judge::Bad, 0);
            }
            s
        };
        let bea = key("c", 0);
        let lr2 = RecordKey::new("c", 0, JudgePreset::Lr2);
        let b = lib.record_play(&bea, &play(8, 2, 2, 10), ClearType::Easy, 0);
        assert_eq!((b.ex_score, b.min_bp, b.min_cb), (16, 2, 2));
        // LR2's record is apart
        let b = lib.record_play(&lr2, &play(9, 1, 1, 10), ClearType::Hard, 0);
        assert_eq!((b.ex_score, b.min_cb, b.play_count), (18, 1, 1));
        // a lower EX score with fewer combo breaks: the min CB only
        let b = lib.record_play(&bea, &play(4, 0, 0, 10), ClearType::Failed, 0);
        assert_eq!((b.ex_score, b.min_bp, b.min_cb), (16, 0, 0));
        // a failed play: the notes it never reached break the combo
        let b = lib.record_play(&key("d", 0), &play(3, 0, 0, 3), ClearType::Failed, 0);
        assert_eq!((b.min_bp, b.min_cb), (7, 7));
        // an assisted play keeps neither
        let b = lib.record_assisted(&key("d", 0), &play(10, 0, 0, 10), ClearType::AssistEasy, 0);
        assert_eq!(b.min_cb, 7);
        let s = open(&dir).scores();
        assert_eq!(
            (s[&bea].clear, s[&lr2].clear),
            (ClearType::Easy, ClearType::Hard)
        );
        assert_eq!(s[&RecordKey::new("c", 0, JudgePreset::Lr2)].min_cb, 1);
        assert!(!s.contains_key(&RecordKey::new("c", 0, JudgePreset::Iidx)));
        // two records as one: each best on its own
        let m = s[&bea].merge(&s[&lr2]);
        assert_eq!((m.ex_score, m.min_bp, m.min_cb), (18, 0, 0));
        // when the lamp rose to a clear, and the EX score (LAMP / SCORE
        // UPDATE): a first FAILED play its score, not its lamp
        let at = |k: &RecordKey| (s[k].lamp_updated > 0, s[k].score_updated > 0);
        assert_eq!(at(&bea), (true, true));
        let failed = lib.record_play(&key("e", 0), &play(3, 0, 0, 3), ClearType::Failed, 0);
        assert_eq!(
            (failed.lamp_updated > 0, failed.score_updated > 0),
            (false, true)
        );
        let mut back = s[&bea].clone();
        (back.lamp_updated, back.score_updated) = (0, 0);
        let json = serde_json::to_string(&back).unwrap();
        assert!(!json.contains("updated"), "{json}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Favourite charts and songs (a song: every chart in its folder),
    /// kept with the records.
    #[test]
    fn imported_records_keep_the_better_of_each_and_import_once() {
        let dir = std::env::temp_dir().join(format!("vibebms-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut lib = open(&dir);
        let mut s = Score {
            total_notes: 100,
            passed: 100,
            max_combo: 50,
            combo_breaks: 4,
            ..Default::default()
        };
        s.counts[0][0] = 80;
        s.counts[3][0] = 4;
        let k = key("a", 0);
        lib.record_play(&k, &s, ClearType::Hard, 0);
        lib.record_play(&k, &s, ClearType::Hard, 0);
        let mine = lib.scores()[&k].clone();
        let theirs = BestScore {
            clear: ClearType::Easy,
            ex_score: 170,
            notes: 100,
            judges: [80, 10, 0, 0, 10, 0],
            max_combo: 40,
            min_bp: 2,
            min_cb: u32::MAX,
            play_count: 7,
            clear_count: 1,
            last_played: 1,
            ..Default::default()
        };
        let new = key("b", 0);
        let recs = [(k.clone(), theirs.clone()), (new.clone(), theirs.clone())];
        assert_eq!(lib.import(&recs), (1, 1));
        let got = &lib.scores()[&k];
        assert_eq!(
            (got.clear, got.ex_score, got.judges),
            (ClearType::Hard, 170, theirs.judges)
        );
        assert_eq!((got.max_combo, got.min_bp, got.min_cb), (50, 2, 4));
        // the higher counts, not their sum
        assert_eq!((got.play_count, got.clear_count), (7, 2));
        assert_eq!(got.last_played, mine.last_played);
        assert_eq!(lib.scores()[&new], theirs);
        // again: nothing changes, on disk neither
        let before = std::fs::read(dir.join("scores.json")).unwrap();
        assert_eq!(lib.import(&recs), (0, 0));
        assert_eq!(open(&dir).scores(), lib.scores());
        assert_eq!(std::fs::read(dir.join("scores.json")).unwrap(), before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn favorites_are_kept() {
        let dir = std::env::temp_dir().join(format!("vibebms-fav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let chart = |path: &str, sha: &str| Song {
            path: path.into(),
            sha256: sha.into(),
            ..Default::default()
        };
        let (a, b, c) = (
            chart("/bms/x/a.bms", "a"),
            chart("/bms/x/b.bms", "b"),
            chart("/bms/y/c.bms", "c"),
        );
        let mut lib = open(&dir);
        assert!(lib.toggle_favorite(&a, false));
        assert!(lib.toggle_favorite(&b, true));
        let f = open(&dir).favorites().clone();
        assert!(f.chart(&a) && !f.chart(&b) && f.song(&a) && f.song(&b));
        assert!(f.has(&a) && f.has(&b) && !f.has(&c));
        // off again
        assert!(!lib.toggle_favorite(&a, true));
        assert!(!lib.toggle_favorite(&a, false));
        assert_eq!(*open(&dir).favorites(), Favorites::default());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unreadable_score_records_are_kept_aside() {
        let dir = std::env::temp_dir().join(format!("vibebms-scores-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("scores.json");
        std::fs::write(&file, r#"[{"sha256": "a", "ln": 0}, {"sha256": 5}]"#).unwrap();
        assert_eq!(load_scores(&file).len(), 1);
        assert!(dir.join("scores.json.bak").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
