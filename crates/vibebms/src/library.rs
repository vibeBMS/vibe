//! Song library and score store.
//!
//! Stored as JSON in `cache/songs.json` (rebuildable scan cache),
//! `scores.json` (your play records) and `player.json` (totals over all
//! plays) so it works identically on every platform, including the web
//! build. Scanning runs in the background.

use crate::platform::{arcfs, vfs};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use vibebms_core::judge::{Judge, JudgePreset};
use vibebms_core::score::{ClearType, Score};
use vibebms_core::{LnPref, LnProfile, LnType, PlayMode};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Song {
    pub path: String,
    /// Group folder shown in song select (root/<first sub directory>).
    pub folder: String,
    pub md5: String,
    pub sha256: String,
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub subartist: String,
    pub genre: String,
    pub level: String,
    pub difficulty: i32,
    #[serde(with = "vibebms_core::by_label")]
    pub mode: PlayMode,
    pub min_bpm: f64,
    pub max_bpm: f64,
    pub main_bpm: f64,
    /// 0 in libraries scanned before it was stored (then the main BPM).
    pub start_bpm: f64,
    pub notes: i64,
    pub length_ms: i64,
    pub total: f64,
    pub has_ln: bool,
    pub has_mine: bool,
    pub has_random: bool,
    pub has_bga: bool,
    pub has_stop: bool,
    pub stagefile: String,
    pub banner: String,
    pub preview: String,
    /// Notes by kind: normal, long, scratch, long on the scratch, mines
    /// (zeros in libraries scanned before they were stored).
    pub note_kinds: [i64; 5],
    /// Judge rank as beatoraja keeps it: `#RANK` 0..4, else a percentage.
    pub judge: Option<i64>,
    /// Notes per second: average, peak and near the end, as beatoraja
    /// measures them.
    pub density: [f64; 3],
    /// The long notes by type (`LnProfile::bits`); `None` in libraries
    /// scanned before it was stored.
    pub ln_profile: Option<u8>,
    /// When its file was last changed (s; the scan's stamp): when it came,
    /// for the NEW sort.
    #[serde(skip)]
    pub added: i64,
}

impl Song {
    pub fn level_num(&self) -> i64 {
        self.level
            .trim()
            .parse::<f64>()
            .map(|v| v as i64)
            .unwrap_or(0)
    }
    /// The LN mode its scores, replays and IR entries are filed under
    /// with the player's mode `pref` (`LnProfile::score_ln`).
    pub fn score_ln(&self, pref: impl Into<LnPref>) -> LnType {
        let pref = pref.into();
        match self.ln_profile {
            Some(b) => LnProfile::from_bits(b).score_ln(pref),
            None if self.has_ln => pref.ty,
            None => LnType::Ln,
        }
    }

    /// Its long notes play the same under LN modes `a` and `b` (not known:
    /// they are filed alike and AUTO or FORCE alike).
    pub fn plays_ln_alike(&self, a: LnPref, b: LnPref) -> bool {
        match self.ln_profile {
            Some(bits) => LnProfile::from_bits(bits).plays_alike(a, b),
            None => self.score_ln(a) == self.score_ln(b) && (a.force == b.force || !self.has_ln),
        }
    }

    /// Its long notes all have their type: one LN mode whatever the
    /// player's (records from before were filed under the player's).
    pub fn fixed_ln(&self) -> bool {
        self.has_ln
            && self
                .ln_profile
                .is_some_and(|b| !LnProfile::from_bits(b).undefined)
    }

    pub fn full_title(&self) -> String {
        if self.subtitle.is_empty() {
            self.title.clone()
        } else {
            format!("{} {}", self.title, self.subtitle)
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BestScore {
    pub clear: ClearType,
    pub ex_score: u32,
    pub notes: u32,
    /// [PG, GR, GD, BD, PR, MS] of the best EX play.
    pub judges: [u32; 6],
    pub max_combo: u32,
    pub min_bp: u32,
    /// The fewest combo breaks of a play (the notes a failed play never
    /// reached count). `u32::MAX`: none known (no play yet, or records
    /// from before it was kept).
    #[serde(default = "no_cb")]
    pub min_cb: u32,
    pub play_count: u32,
    pub clear_count: u32,
    pub last_played: i64,
    /// When a play last raised the lamp, and the best EX score (Unix
    /// seconds; 0: not since these were kept): the LAMP UPDATE and SCORE
    /// UPDATE folders.
    #[serde(skip_serializing_if = "is_zero")]
    pub lamp_updated: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub score_updated: i64,
}

fn no_cb() -> u32 {
    u32::MAX
}

fn is_zero(t: &i64) -> bool {
    *t == 0
}

impl BestScore {
    /// No play yet.
    fn none() -> BestScore {
        BestScore {
            min_bp: u32::MAX,
            min_cb: u32::MAX,
            ..Default::default()
        }
    }

    /// Two records of the same chart, LN mode and rule mode as one.
    pub fn merge(&self, o: &BestScore) -> BestScore {
        let (hi, lo) = if o.ex_score > self.ex_score {
            (o, self)
        } else {
            (self, o)
        };
        BestScore {
            clear: hi.clear.max(lo.clear),
            ex_score: hi.ex_score,
            notes: hi.notes,
            judges: hi.judges,
            max_combo: hi.max_combo.max(lo.max_combo),
            min_bp: hi.min_bp.min(lo.min_bp),
            min_cb: hi.min_cb.min(lo.min_cb),
            play_count: hi.play_count + lo.play_count,
            clear_count: hi.clear_count + lo.clear_count,
            last_played: hi.last_played.max(lo.last_played),
            lamp_updated: hi.lamp_updated.max(lo.lamp_updated),
            score_updated: hi.score_updated.max(lo.score_updated),
        }
    }

    /// `o`, a record read from another player's database, taken into this
    /// one: the better lamp, EX score (with its counts) and combo, the lower
    /// BP and CB, and the play and clear counts as the higher of the two,
    /// not their sum (the same database imported again changes nothing).
    pub fn absorb(&self, o: &BestScore) -> BestScore {
        let mut b = self.clone();
        if o.clear > b.clear {
            b.clear = o.clear;
            b.lamp_updated = b.lamp_updated.max(o.lamp_updated);
        }
        // (a record of assisted plays only has no counts yet)
        if o.ex_score > b.ex_score || (b.notes == 0 && o.ex_score == b.ex_score) {
            b.ex_score = o.ex_score;
            b.notes = o.notes;
            b.judges = o.judges;
            b.score_updated = b.score_updated.max(o.score_updated);
        }
        b.max_combo = b.max_combo.max(o.max_combo);
        b.min_bp = b.min_bp.min(o.min_bp);
        b.min_cb = b.min_cb.min(o.min_cb);
        b.play_count = b.play_count.max(o.play_count);
        b.clear_count = b.clear_count.max(o.clear_count);
        b.last_played = b.last_played.max(o.last_played);
        b
    }

    /// The lamp to `clear` if that is higher, noting when it rose to a
    /// clear (a first play FAILED is no update).
    fn raise_lamp(&mut self, clear: ClearType, now: i64) {
        if clear > self.clear {
            self.clear = clear;
            if clear > ClearType::Failed {
                self.lamp_updated = now;
            }
        }
    }

    pub fn rate(&self) -> f64 {
        if self.notes == 0 {
            0.0
        } else {
            self.ex_score as f64 / (self.notes as f64 * 2.0)
        }
    }
}

/// The player's totals over every recorded play (as beatoraja's
/// `PlayerData`; skin numbers 17-19, 33-37 and 333). Plays and clears are
/// the sums of the charts' counts instead.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Totals {
    /// [PG, GR, GD, BD, PR, MS] of every play. Missing in files from
    /// before vibe kept them: then seeded once from the best plays, so
    /// plays from before count with their best play only.
    pub judges: Option<[u64; 6]>,
    /// Time played (ms): up to the last judgement of each play, counted
    /// since vibe keeps it.
    pub play_ms: u64,
}

impl Totals {
    pub fn judge(&self, j: usize) -> u64 {
        self.judges.map_or(0, |v| v[j])
    }

    /// Notes hit (PG to BD), beatoraja's "total notes played".
    pub fn notes(&self) -> u64 {
        (0..4).map(|j| self.judge(j)).sum()
    }
}

/// The player's favourites: charts by SHA-256, songs by their folder
/// (every chart in it, those added later too).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Favorites {
    pub charts: std::collections::BTreeSet<String>,
    pub songs: std::collections::BTreeSet<String>,
}

impl Favorites {
    /// The folder that makes a chart's song.
    fn song_of(s: &Song) -> String {
        Path::new(&s.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn chart(&self, s: &Song) -> bool {
        self.charts.contains(&s.sha256)
    }

    pub fn song(&self, s: &Song) -> bool {
        self.songs.contains(&Self::song_of(s))
    }

    /// A favourite as a chart or as its song's.
    pub fn has(&self, s: &Song) -> bool {
        self.chart(s) || self.song(s)
    }
}

#[derive(Serialize, Deserialize)]
struct SongRecord {
    #[serde(flatten)]
    song: Song,
    mtime: i64,
    size: i64,
    /// Not a playable chart (it does not parse, or has no notes) to this
    /// version of the game: kept so that it is read again only when it
    /// changes or another version scans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skipped_by: Option<String>,
}

/// What a record is filed under: a chart (its SHA-256) or a course (its
/// identity), the LN mode it was played in (`Song::score_ln`) and the rule
/// mode, the judge preset it was played with (vibe-LR2, vibe-beatoraja and
/// IIDX plays each have their own bests, lamps and replays).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RecordKey {
    pub id: String,
    pub ln: i32,
    pub rule: JudgePreset,
}

impl RecordKey {
    pub fn new(id: &str, ln: i32, rule: JudgePreset) -> RecordKey {
        RecordKey {
            id: id.to_string(),
            ln,
            rule,
        }
    }
}

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

fn save_json(path: &Path, v: &impl Serialize, what: &str) {
    match serde_json::to_vec_pretty(v) {
        Ok(j) => {
            if let Err(e) = vfs::write(path, &j) {
                log::error!("saving {what}: {e}");
            }
        }
        Err(e) => log::error!("saving {what}: {e}"),
    }
}

/// The charts of the song cache at `file`, as the last scan left it (the
/// command line's `songs list` too). One in an archive that is also a
/// file of its own (the archive unpacked beside it) is left out: the file
/// is the one listed.
pub fn cached_songs(file: &Path) -> Vec<Song> {
    let songs: Vec<Song> = load_songs(file)
        .into_iter()
        .filter(|r| r.skipped_by.is_none())
        .map(|r| Song {
            added: r.mtime,
            ..r.song
        })
        .collect();
    let packed = |s: &Song| arcfs::split(Path::new(&s.path)).is_some();
    if !songs.iter().any(packed) {
        return songs;
    }
    let loose: HashSet<String> = songs
        .iter()
        .filter(|s| !packed(s))
        .map(|s| s.sha256.clone())
        .collect();
    songs
        .into_iter()
        .filter(|s| !packed(s) || !loose.contains(&s.sha256))
        .collect()
}

fn load_songs(file: &Path) -> Vec<SongRecord> {
    vfs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
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

#[derive(Default)]
pub struct ScanProgress {
    pub found: AtomicUsize,
    pub parsed: AtomicUsize,
    pub to_parse: AtomicUsize,
    pub done: AtomicBool,
    pub changed: AtomicBool,
    pub current: Mutex<String>,
    /// Charts the scan could not read (on the web: not in memory, as when
    /// the browser's storage swapped a stored file in after it was read).
    /// They got no record, so they are read again; the app reads them in
    /// and scans once more.
    pub unread: Mutex<Vec<PathBuf>>,
}

pub(crate) fn is_chart(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            vibebms_core::CHART_EXTENSIONS
                .iter()
                .any(|x| x.eq_ignore_ascii_case(e))
        })
        .unwrap_or(false)
}

/// What the scan finds below a folder.
#[derive(Default)]
struct Found {
    charts: Vec<PathBuf>,
    /// Archives (read as folders: `platform::arcfs`).
    archives: Vec<PathBuf>,
    /// Folders that cannot be read (a drive not mounted, no permission).
    unreadable: Vec<PathBuf>,
}

/// Chart files and archives under `dir` into `out`.
fn walk(dir: &Path, depth: usize, skip: &HashSet<PathBuf>, out: &mut Found) {
    if depth > 16 {
        return;
    }
    let Ok(entries) = vfs::read_dir(dir) else {
        out.unreadable.push(dir.to_path_buf());
        return;
    };
    for e in entries {
        let name = e
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if e.is_dir {
            if !skip.contains(&e.path) {
                walk(&e.path, depth + 1, skip, out);
            }
        } else if is_chart(&e.path) {
            out.charts.push(e.path);
        } else if arcfs::is_archive(&e.path) {
            out.archives.push(e.path);
        }
    }
}

/// Chart files under `roots` (used by the web build to prefetch them; the
/// archives that changed are read in when the scan asks: `ScanProgress::unread`).
pub fn find_charts(roots: &[PathBuf], skip: &HashSet<PathBuf>) -> Vec<PathBuf> {
    let mut found = Found::default();
    for r in roots {
        walk(r, 0, skip, &mut found);
    }
    found.charts
}

/// The library folder a chart (or an archive) below `root` is listed under:
/// `root`'s sub-folder it is in, else `root`.
fn folder_of(root: &Path, f: &Path) -> String {
    let first = f
        .strip_prefix(root)
        .ok()
        .and_then(|r| r.components().next())
        .map(|c| root.join(c.as_os_str()));
    match first {
        Some(p) if p != f => p.to_string_lossy().into_owned(),
        _ => root.to_string_lossy().into_owned(),
    }
}

/// The archives found that changed since the scan before: listed, and
/// their charts read (several at once natively). Their indexes are updated;
/// the charts' bytes by path. Archives that cannot be read now go into
/// `unreadable` (web: not in memory yet, also into `unread`).
fn read_archives(
    archives: &[(PathBuf, (i64, i64))],
    progress: &ScanProgress,
    unreadable: &mut Vec<PathBuf>,
) -> (HashMap<PathBuf, Vec<u8>>, bool) {
    let changed: Vec<usize> = (0..archives.len())
        .filter(|&i| {
            let (a, stamp) = &archives[i];
            arcfs::index_of(a).map(|ix| ix.stamp) != Some(*stamp)
        })
        .collect();
    let read: Mutex<Vec<(usize, std::io::Result<arcfs::Unpacked>)>> = Mutex::default();
    crate::platform::parallel_for(changed.len(), |k| {
        let i = changed[k];
        let a = &archives[i].0;
        *progress.current.lock().unwrap() = a.display().to_string();
        let r = arcfs::read_charts(a, None);
        read.lock().unwrap().push((i, r));
    });
    let mut read = read.into_inner().unwrap();
    read.sort_by_key(|(i, _)| *i);
    let mut bytes = HashMap::new();
    let mut indexed = false;
    for (i, r) in read {
        let (a, stamp) = &archives[i];
        match r {
            Ok(u) => {
                let files = u.listing.unwrap_or_default();
                log::info!(
                    "scan: {} ({} files, {} charts)",
                    a.display(),
                    files.len(),
                    u.files.len()
                );
                arcfs::set_index(a, arcfs::Index::new(*stamp, &u.format, files));
                for (rel, b) in u.files {
                    bytes.insert(arcfs::join(a, &rel), b);
                }
                indexed = true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                log::warn!("scan: {}: {e}", a.display());
                arcfs::set_index(a, arcfs::Index::failed(*stamp, e.to_string()));
                indexed = true;
            }
            Err(e) => {
                log::debug!("scan: {} not read: {e}", a.display());
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    progress.unread.lock().unwrap().push(a.clone());
                }
                unreadable.push(arcfs::join(a, Path::new("")));
            }
        }
    }
    (bytes, indexed)
}

/// Notes by kind and the densities of a chart (see `Song`).
///
/// Kinds: normal notes and long notes off the scratch lanes, the same on
/// them, mines. Densities (notes per second): the average over the
/// seconds that are not much quieter than the chart's mean, the busiest
/// second, and the busiest stretch of up to 5 s from the point where the
/// notes left could still fill the gauge from empty. A second's count
/// includes the long notes held through it. Charts over an hour get no
/// densities.
///
/// Written from a behaviour specification, not from another program's
/// code.
pub fn chart_info(c: &vibebms_core::Chart) -> ([i64; 5], [f64; 3]) {
    use vibebms_core::chart::{LnType, NoteKind};
    const SEC: i64 = 1_000_000;

    let mode = c.mode();
    let lanes = mode.lane_count();
    let is_scratch = |lane: u8| (lane as usize) < lanes && mode.is_scratch(lane as usize);
    let mut kinds = [0i64; 5];
    for n in &c.notes {
        let slot = match n.kind {
            NoteKind::Normal => 0,
            NoteKind::Long { .. } => 1,
            NoteKind::Mine { .. } => {
                kinds[4] += 1;
                continue;
            }
            NoteKind::Invisible => continue,
        };
        kinds[slot + if is_scratch(n.lane) { 2 } else { 0 }] += 1;
    }

    let mut playable: Vec<&vibebms_core::Note> =
        c.notes.iter().filter(|n| n.is_playable()).collect();
    playable.sort_by_key(|n| (n.time, n.lane));
    let n_notes = playable.len() as i64;
    let last = playable
        .iter()
        .map(|n| n.end_time())
        .max()
        .unwrap_or(0)
        .max(0);
    if n_notes == 0 || last > 3_600 * SEC {
        return (kinds, [0.0; 3]);
    }

    let second = |t: i64| t.max(0) / SEC;
    let buckets_len = second(last) + 2;
    let mut buckets = vec![0i64; buckets_len as usize];
    for n in &playable {
        let from = second(n.time);
        let to = second(n.end_time()).min(buckets_len - 1).max(from);
        for b in &mut buckets[from as usize..=to as usize] {
            *b += 1;
        }
    }

    let threshold = n_notes / buckets_len / 4;
    let busy: Vec<i64> = buckets
        .iter()
        .copied()
        .filter(|b| *b >= threshold)
        .collect();
    let average = if busy.is_empty() {
        0.0
    } else {
        busy.iter().sum::<i64>() as f64 / busy.len() as f64
    };
    let peak = buckets.iter().copied().max().unwrap_or(0) as f64;

    let total = c.total(LnType::Ln);
    let k = if total > 0.0 {
        (n_notes as f64 * (1.0 - 100.0 / total)) as i64
    } else {
        0
    };
    let fill = if k >= 1 {
        second(playable[(k.min(n_notes) - 1) as usize].time)
    } else {
        0
    };
    let width = 5.min(buckets_len - 1 - fill);
    let end = if width > 0 {
        (fill..=buckets_len - 1 - width)
            .map(|s| {
                buckets[s as usize..(s + width) as usize]
                    .iter()
                    .sum::<i64>()
            })
            .max()
            .map_or(0.0, |sum| sum as f64 / width as f64)
    } else {
        0.0
    };
    (kinds, [average, peak, end])
}

pub fn song_from_chart(path: &Path, folder: String, c: &vibebms_core::Chart) -> Song {
    let m = &c.meta;
    let (note_kinds, density) = chart_info(c);
    Song {
        note_kinds,
        density,
        judge: Some(crate::scene::skinvals::judge_rank_number(m)),
        ln_profile: Some(c.ln_profile().bits()),
        path: path.to_string_lossy().into_owned(),
        folder,
        md5: m.md5.clone(),
        sha256: m.sha256.clone(),
        title: m.title.clone(),
        subtitle: m.subtitle.clone(),
        artist: m.artist.clone(),
        subartist: m.subartist.clone(),
        genre: m.genre.clone(),
        level: m.level.clone(),
        difficulty: m.difficulty,
        mode: c.mode(),
        min_bpm: m.min_bpm,
        max_bpm: m.max_bpm,
        main_bpm: m.main_bpm,
        start_bpm: m.initial_bpm,
        notes: c.note_count() as i64,
        length_ms: c.length() / 1000,
        total: c.total(vibebms_core::LnType::Ln),
        has_ln: m.has_long,
        has_mine: m.has_mine,
        has_random: m.has_random,
        has_bga: m.has_bga,
        has_stop: m.has_stop,
        stagefile: m.stagefile.clone(),
        banner: m.banner.clone(),
        preview: m.preview.clone(),
        added: 0,
    }
}

/// The game's version, which a chart it could not read is filed under.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Scan `roots` and update the song cache. Runs on a background thread.
pub fn scan(
    songs_file: PathBuf,
    roots: Vec<PathBuf>,
    skip: HashSet<PathBuf>,
    progress: Arc<ScanProgress>,
) {
    let known_records = load_songs(&songs_file);
    // charts read before the library kept their judge rank and note
    // kinds, or the types of their long notes, are read again (once)
    let known: HashMap<String, (i64, i64)> = known_records
        .iter()
        .filter(|r| match &r.skipped_by {
            Some(v) => v == VERSION,
            None => r.song.judge.is_some() && (!r.song.has_ln || r.song.ln_profile.is_some()),
        })
        .map(|r| (r.song.path.clone(), (r.mtime, r.size)))
        .collect();
    let index_file = songs_file.with_file_name("archives.json");
    arcfs::load_index(&index_file);
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    // (each with the root it is below)
    let mut archives: Vec<(PathBuf, &Path)> = Vec::new();
    let mut unreadable: Vec<PathBuf> = Vec::new();
    for root in &roots {
        *progress.current.lock().unwrap() = root.display().to_string();
        let mut found = Found::default();
        walk(root, 0, &skip, &mut found);
        for f in found.charts {
            let folder = folder_of(root, &f);
            files.push((f, folder));
        }
        archives.extend(found.archives.into_iter().map(|a| (a, root.as_path())));
        unreadable.extend(found.unreadable);
        progress.found.store(files.len(), Ordering::Relaxed);
    }
    // archives: those that changed read (their charts in `pre`), every
    // one's charts from its index; a chart in one has the archive's stamp
    let stamped: Vec<(PathBuf, (i64, i64))> = archives
        .iter()
        .map(|(a, _)| (a.clone(), vfs::stamp(a)))
        .collect();
    let (mut pre, indexed) = read_archives(&stamped, &progress, &mut unreadable);
    let mut archive_stamp: HashMap<PathBuf, (i64, i64)> = HashMap::new();
    for ((a, root), (_, stamp)) in archives.iter().zip(&stamped) {
        let Some(ix) = arcfs::index_of(a).filter(|ix| ix.stamp == *stamp) else {
            continue;
        };
        for rel in ix.charts() {
            let p = arcfs::join(a, rel);
            archive_stamp.insert(p.clone(), *stamp);
            // (an archive right in the root is a folder of its own)
            let folder = folder_of(root, &p);
            files.push((p, folder));
        }
    }
    progress.found.store(files.len(), Ordering::Relaxed);
    let mut seen: HashSet<String> = files
        .iter()
        .map(|(f, _)| f.to_string_lossy().into_owned())
        .collect();
    // charts in a folder that cannot be read now (a drive not mounted) are
    // kept, not removed with their records
    for r in &known_records {
        if unreadable
            .iter()
            .any(|d| Path::new(&r.song.path).starts_with(d))
        {
            seen.insert(r.song.path.clone());
        }
    }
    if !unreadable.is_empty() {
        log::warn!("scan: cannot read {}", unreadable[0].display());
    }
    // (each file's stamp: on a slow drive, several at once)
    let stamps: Vec<(i64, i64)> = {
        let out: Vec<Mutex<(i64, i64)>> = files.iter().map(|_| Mutex::default()).collect();
        crate::platform::parallel_for(files.len(), |i| {
            let f = &files[i].0;
            let stamp = archive_stamp
                .get(f)
                .copied()
                .unwrap_or_else(|| vfs::stamp(f));
            *out[i].lock().unwrap() = stamp;
        });
        out.into_iter().map(|m| m.into_inner().unwrap()).collect()
    };
    let todo: Vec<(PathBuf, String, (i64, i64))> = files
        .into_iter()
        .zip(stamps)
        .filter_map(|((f, folder), stamp)| {
            let key = f.to_string_lossy().into_owned();
            (known.get(&key) != Some(&stamp)).then_some((f, folder, stamp))
        })
        .collect();
    progress.to_parse.store(todo.len(), Ordering::Relaxed);
    // charts in archives that did not change but are read again (records
    // of an older version): each archive read once for them
    let mut again: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
    for (f, _, _) in &todo {
        if let Some((a, rel)) = arcfs::split(f).filter(|_| !pre.contains_key(f)) {
            again.entry(a).or_default().insert(rel);
        }
    }
    let mut again: Vec<(PathBuf, HashSet<PathBuf>)> = again.into_iter().collect();
    again.sort_by(|a, b| a.0.cmp(&b.0));
    let reread: Mutex<Vec<(PathBuf, Vec<u8>)>> = Mutex::default();
    crate::platform::parallel_for(again.len(), |i| {
        let (a, want) = &again[i];
        match arcfs::read_charts(a, Some(want)) {
            Ok(u) => reread.lock().unwrap().extend(
                u.files
                    .into_iter()
                    .map(|(rel, b)| (arcfs::join(a, &rel), b)),
            ),
            Err(e) => log::debug!("scan: {} not read: {e}", a.display()),
        }
    });
    pre.extend(reread.into_inner().unwrap());
    // (taken as each is parsed)
    let pre: Vec<Mutex<Option<Vec<u8>>>> = todo
        .iter()
        .map(|(f, _, _)| Mutex::new(pre.remove(f)))
        .collect();

    let results: Mutex<Vec<SongRecord>> = Mutex::new(Vec::new());
    crate::platform::parallel_for(todo.len(), |i| {
        let (f, folder, stamp) = &todo[i];
        let opts = vibebms_core::bms::ParseOptions::default();
        // (a file that cannot be read now is tried again next time)
        let bytes = match pre[i].lock().unwrap().take() {
            Some(b) => Ok(b),
            None if arcfs::split(f).is_some() => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "not read from its archive",
            )),
            None => vfs::read(f),
        };
        let parsed = bytes.map(|b| {
            // a parser bug on one file skips that file
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                vibebms_core::load_chart_bytes(f, &b, &opts).map_err(|e| e.to_string())
            }))
            .unwrap_or_else(|_| Err("the parser panicked".into()))
        });
        let entry = |song, skipped_by| SongRecord {
            song,
            mtime: stamp.0,
            size: stamp.1,
            skipped_by,
        };
        let record = match parsed {
            Ok(Ok(c)) if c.note_count() > 0 => {
                Some(entry(song_from_chart(f, folder.clone(), &c), None))
            }
            Ok(r) => {
                if let Err(e) = r {
                    log::debug!("skip {}: {e}", f.display());
                }
                let song = Song {
                    path: f.to_string_lossy().into_owned(),
                    folder: folder.clone(),
                    ..Song::default()
                };
                Some(entry(song, Some(VERSION.to_string())))
            }
            Err(e) => {
                log::debug!("not read {}: {e}", f.display());
                progress.unread.lock().unwrap().push(f.clone());
                None
            }
        };
        if let Some(r) = record {
            results.lock().unwrap().push(r);
        }
        progress.parsed.fetch_add(1, Ordering::Relaxed);
    });
    let new = results.into_inner().unwrap();
    // the archives' indexes: those found (and those in folders that cannot
    // be read now, or not below these roots) kept
    let found_archives: HashSet<&Path> = archives.iter().map(|(a, _)| a.as_path()).collect();
    let keep_archive = |a: &Path| {
        found_archives.contains(a)
            || !roots.iter().any(|r| a.starts_with(r))
            || unreadable
                .iter()
                .any(|d| a.starts_with(d) || arcfs::join(a, Path::new("")) == *d)
    };
    let gone = arcfs::indexed().iter().any(|a| !keep_archive(a));
    if indexed || gone || (!archives.is_empty() && !vfs::is_file(&index_file)) {
        arcfs::save_index(&index_file, &keep_archive);
    }
    // (records read again count too: their file may be gone)
    let removed = known_records
        .iter()
        .filter(|r| !seen.contains(&r.song.path))
        .count();
    let changed = !new.is_empty() || removed > 0;
    if changed {
        let updated: HashSet<String> = new.iter().map(|r| r.song.path.clone()).collect();
        let mut all: Vec<SongRecord> = known_records
            .into_iter()
            .filter(|r| seen.contains(&r.song.path) && !updated.contains(&r.song.path))
            .collect();
        all.extend(new);
        all.sort_by(|a, b| a.song.path.cmp(&b.song.path));
        match serde_json::to_vec(&all) {
            Ok(j) => {
                if let Err(e) = vfs::write(&songs_file, &j) {
                    log::error!("saving song cache: {e}");
                }
            }
            Err(e) => log::error!("saving song cache: {e}"),
        }
        log::info!(
            "scan finished: {} charts ({} updated, {removed} removed)",
            all.iter().filter(|r| r.skipped_by.is_none()).count(),
            updated.len()
        );
    } else {
        log::info!("scan finished: {} charts, no changes", seen.len());
    }
    progress.changed.store(changed, Ordering::Release);
    progress.done.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {

    #[test]
    fn chart_info_counts() {
        // 120 BPM: a measure is 2 s. Scratch note, two keys, a 4 s LN.
        let bms = "#PLAYER 1\n#BPM 120\n#TOTAL 200\n#WAV01 a.wav\n\
            #00116:01\n#00111:0001\n#00112:01\n#00153:01\n#00353:01\n";
        let c = vibebms_core::bms::parse_bms(bms.as_bytes(), "bms", &Default::default()).unwrap();
        let (k, d) = chart_info(&c);
        assert_eq!(k, [2, 1, 1, 0, 0]);
        // the LN holds through seconds 2..=6; second 2 has two more notes
        assert_eq!(d[1], 3.0);
        // buckets [0, 0, 3, 2, 1, 1, 1, 0]: all count (threshold 0); the
        // gauge could fill after the 2nd note (second 2): 8/5
        assert_eq!(d, [1.0, 3.0, 1.6]);
    }

    use super::*;
    use vibebms_core::chart::{Chart, ChartMeta, LnType, Note, NoteKind};

    /// The library at `dir` (its records from before rule modes under the
    /// default).
    fn open(dir: &Path) -> Library {
        Library::open(dir, dir, |_, _| None)
    }

    fn key(id: &str, ln: i32) -> RecordKey {
        RecordKey::new(id, ln, JudgePreset::Beatoraja)
    }
    use vibebms_core::PlayMode;

    fn info_note(lane: u8, time: i64, kind: NoteKind) -> Note {
        Note {
            lane,
            time,
            beat: 0.0,
            pos: 0.0,
            sound: 0,
            kind,
        }
    }

    fn info_long(lane: u8, time: i64, end_time: i64, ln_type: Option<LnType>) -> Note {
        info_note(
            lane,
            time,
            NoteKind::Long {
                end_time,
                end_beat: 0.0,
                end_pos: 0.0,
                end_sound: 0,
                ln_type,
            },
        )
    }

    fn info_chart(mode: PlayMode, total: Option<f64>, mut notes: Vec<Note>) -> Chart {
        notes.sort_by_key(|n| (n.time, n.lane));
        Chart {
            meta: ChartMeta {
                mode: Some(mode),
                total,
                ..Default::default()
            },
            notes,
            ..Default::default()
        }
    }

    /// The spec's larger example: 48 notes (two long notes adding a count
    /// to the next second each), buckets [0, 2, 6, 8, 7, 1, 0, 3, 9, 10,
    /// 4, 0], TOTAL 175 (the 20th note, in second 4, is where the gauge
    /// could fill).
    #[test]
    fn chart_info_synthetic() {
        let mut notes = Vec::new();
        let normals = [
            (1, 2),
            (2, 6),
            (3, 8),
            (4, 7),
            (5, 1),
            (7, 3),
            (8, 8),
            (9, 8),
            (10, 3),
        ];
        for (sec, n) in normals {
            for k in 0..n {
                notes.push(info_note(
                    (k % 7) as u8,
                    sec * 1_000_000 + k * 100_000,
                    NoteKind::Normal,
                ));
            }
        }
        notes.push(info_long(0, 8_950_000, 9_500_000, None));
        notes.push(info_long(1, 9_950_000, 10_500_000, None));
        notes.push(info_note(2, 6_500_000, NoteKind::Mine { damage: 1.0 }));
        let c = info_chart(PlayMode::Beat7K, Some(175.0), notes);
        let (k, d) = chart_info(&c);
        assert_eq!(k, [46, 2, 0, 0, 1]);
        assert!((d[0] - 50.0 / 9.0).abs() < 1e-9, "{d:?}");
        assert_eq!(d[1], 10.0);
        assert!((d[2] - 5.2).abs() < 1e-9, "{d:?}");
        // TOTAL 100 or less: from second 0, every 5-second window counts
        let mut low = c.clone();
        low.meta.total = Some(100.0);
        assert!((chart_info(&low).1[2] - 5.2).abs() < 1e-9);
        // an unset TOTAL is beatoraja's default for the judgement count
        // (here 260: k = trunc(48 × (1 − 100/260)) = 29; the 29th note is in
        // second 8; three buckets are left before the last: 23 / 3)
        let mut def = c.clone();
        def.meta.total = None;
        assert!((chart_info(&def).1[2] - 23.0 / 3.0).abs() < 1e-9);
    }

    /// The one-hour guard limits only the densities: all five kinds are
    /// counted.
    #[test]
    fn chart_info_counts_kinds_past_an_hour() {
        let c = info_chart(
            PlayMode::Beat14K,
            None,
            vec![
                info_note(7, 1_000_000, NoteKind::Normal),
                info_long(15, 2_000_000, 3_000_000, Some(LnType::Cn)),
                info_long(8, 3_000_000, 4_000_000, None),
                info_note(4, 5_000_000, NoteKind::Mine { damage: 5.0 }),
                info_note(0, 3_600_000_001, NoteKind::Normal),
            ],
        );
        assert_eq!(chart_info(&c), ([1, 1, 1, 1, 1], [0.0; 3]));
    }

    /// Kinds by scratch lane (any player's), mines; negative times in
    /// second 0; over an hour no densities; no notes nothing.
    #[test]
    fn chart_info_edges() {
        let c = info_chart(
            PlayMode::Beat14K,
            Some(300.0),
            vec![
                info_note(7, -2_000_000, NoteKind::Normal),
                info_note(15, 500_000, NoteKind::Normal),
                info_long(15, 600_000, 2_400_000, Some(LnType::Cn)),
                info_long(8, 700_000, 900_000, None),
                info_note(3, 800_000, NoteKind::Invisible),
                info_note(4, 800_000, NoteKind::Mine { damage: 5.0 }),
            ],
        );
        let (k, d) = chart_info(&c);
        assert_eq!(k, [0, 1, 2, 1, 1]);
        // buckets [4, 1, 1, 0]: threshold ⌊⌊4/4⌋/4⌋ = 0
        assert_eq!(d[0], 6.0 / 4.0);
        assert_eq!(d[1], 4.0);
        let mut long = c.clone();
        long.notes
            .push(info_note(0, 3_600_000_001, NoteKind::Normal));
        // over an hour: no densities (the arrays per second would be too
        // long), but the kinds are counted
        assert_eq!(chart_info(&long).1, [0.0; 3]);
        let mut hour = c.clone();
        hour.notes
            .push(info_note(0, 3_600_000_000, NoteKind::Normal));
        assert!(chart_info(&hour).1[1] > 0.0);
        let empty = info_chart(
            PlayMode::Beat7K,
            None,
            vec![info_note(1, 1_000_000, NoteKind::Mine { damage: 1.0 })],
        );
        assert_eq!(chart_info(&empty), ([0, 0, 0, 0, 1], [0.0; 3]));
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

    #[test]
    fn scan_and_scores() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-{}", std::process::id()));
        let songs = dir.join("pack").join("song");
        std::fs::create_dir_all(&songs).unwrap();
        std::fs::write(
            songs.join("a.bme"),
            "#TITLE A\n#PLAYLEVEL 5\n#BPM 150\n#00111:01\n#00118:01\n",
        )
        .unwrap();
        std::fs::write(songs.join("readme.txt"), "x").unwrap();
        // no notes: not a song, and not read again while it stays so
        std::fs::write(songs.join("empty.bme"), "#TITLE E\n").unwrap();
        let cache = dir.join("cache");
        let p = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.clone()],
            [cache.clone()].into_iter().collect(),
            p.clone(),
        );
        assert!(p.done.load(Ordering::Acquire));
        let mut lib = open(&cache);
        let s = lib.songs();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "A");
        assert_eq!(s[0].mode, PlayMode::Beat7K);
        assert!(s[0].folder.ends_with("pack"));
        // rescan is incremental
        let p2 = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.clone()],
            [cache.clone()].into_iter().collect(),
            p2.clone(),
        );
        assert_eq!(p2.to_parse.load(Ordering::Relaxed), 0);
        assert!(!p2.changed.load(Ordering::Relaxed));

        let mut sc = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        sc.add(Judge::PGreat, 1);
        sc.add(Judge::Great, 1);
        let b = lib.record_play(&key(&s[0].sha256, 0), &sc, ClearType::Hard, 1500);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 1));
        let worse = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        let b = lib.record_play(&key(&s[0].sha256, 0), &worse, ClearType::Failed, 0);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 2));
        // an assisted play: lamp and counts only, never the best score
        let mut great = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        great.add(Judge::PGreat, 1);
        great.add(Judge::PGreat, 1);
        let b = lib.record_assisted(&key(&s[0].sha256, 0), &great, ClearType::AssistEasy, 0);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 3));
        let b = lib.record_assisted(&key("other", 0), &great, ClearType::LightAssistEasy, 0);
        assert_eq!((b.ex_score, b.min_bp, b.max_combo), (0, u32::MAX, 0));
        assert_eq!((b.clear, b.clear_count), (ClearType::LightAssistEasy, 1));
        // persisted
        let lib2 = open(&cache);
        assert_eq!(lib2.scores().len(), 2);
        // the library folder cannot be read (here: gone): its charts stay
        let moved = dir.with_extension("away");
        std::fs::rename(&dir, &moved).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::copy(moved.join("cache/songs.json"), cache.join("songs.json")).unwrap();
        let p3 = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.join("pack")],
            [cache.clone()].into_iter().collect(),
            p3.clone(),
        );
        assert!(!p3.changed.load(Ordering::Relaxed));
        assert_eq!(open(&cache).songs().len(), 1);
        // and go once the folder is there without them
        std::fs::create_dir_all(dir.join("pack")).unwrap();
        let p4 = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.join("pack")],
            [cache.clone()].into_iter().collect(),
            p4.clone(),
        );
        assert_eq!(open(&cache).songs().len(), 0);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&moved).ok();
    }

    /// A chart that cannot be read when the scan comes to it (on the web:
    /// its bytes not in memory) gets no record, not one as a skipped
    /// chart: it is listed as unread and read by the next scan.
    #[cfg(unix)]
    #[test]
    fn a_chart_not_read_yet_is_read_by_the_next_scan() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-unread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let songs = dir.join("pack");
        std::fs::create_dir_all(&songs).unwrap();
        let cache = dir.join("cache");
        // (a chart whose file cannot be read: a link to nothing yet)
        let chart = songs.join("a.bme");
        let target = dir.join("a.bme.data");
        std::os::unix::fs::symlink(&target, &chart).unwrap();
        let run = || {
            let p = Arc::new(ScanProgress::default());
            scan(
                cache.join("songs.json"),
                vec![songs.clone()],
                [cache.clone()].into_iter().collect(),
                p.clone(),
            );
            p
        };
        let p = run();
        assert_eq!(*p.unread.lock().unwrap(), vec![chart.clone()]);
        assert!(!p.changed.load(Ordering::Relaxed));
        assert!(load_songs(&cache.join("songs.json")).is_empty());
        std::fs::write(&target, "#TITLE A\n#BPM 150\n#00111:01\n").unwrap();
        let p = run();
        assert!(p.unread.lock().unwrap().is_empty());
        assert!(p.changed.load(Ordering::Relaxed));
        let s = Library::open(&cache, &cache, |_, _| None).songs();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "A");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn old_records_of_deleted_charts_go() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-old-{}", std::process::id()));
        let songs = dir.join("pack");
        std::fs::create_dir_all(&songs).unwrap();
        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        // a record from before judge ranks were kept (read again when
        // seen), its chart deleted since
        let gone = songs.join("gone.bme").to_string_lossy().into_owned();
        let old = serde_json::json!([{ "path": gone, "title": "Gone", "mtime": 1, "size": 1 }]);
        std::fs::write(cache.join("songs.json"), old.to_string()).unwrap();
        assert_eq!(open(&cache).songs().len(), 1);
        let p = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![songs],
            [cache.clone()].into_iter().collect(),
            p.clone(),
        );
        assert!(p.changed.load(Ordering::Relaxed));
        assert!(open(&cache).songs().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A small WAV (16-bit mono, 22050 Hz).
    #[cfg(not(target_arch = "wasm32"))]
    fn wav(frames: u32) -> Vec<u8> {
        let data = frames * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&22050u32.to_le_bytes());
        b.extend_from_slice(&44100u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        b.extend((0..frames).flat_map(|i| ((i as i16).wrapping_mul(300) as u16).to_le_bytes()));
        b
    }

    /// A solid 7z (LZMA2) of `files`.
    #[cfg(not(target_arch = "wasm32"))]
    fn seven(files: &[(&str, &[u8])]) -> Vec<u8> {
        use sevenz_rust2::{ArchiveEntry, ArchiveWriter, SourceReader};
        let mut w = ArchiveWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        let entries = files
            .iter()
            .map(|(n, _)| ArchiveEntry::new_file(n))
            .collect();
        let readers = files.iter().map(|(_, b)| SourceReader::new(*b)).collect();
        w.push_archive_entries(entries, readers).unwrap();
        w.finish().unwrap().into_inner()
    }

    /// Charts in a zip, a solid 7z and a RAR: scanned as folders (an
    /// unchanged archive not opened again, a changed one read again), read
    /// and played through the file system as files on a disk are.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn charts_in_archives() {
        use crate::platform::arcfs;
        let dir = std::env::temp_dir().join(format!("vibebms-lib-arc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let packs = dir.join("packs");
        std::fs::create_dir_all(&packs).unwrap();
        let cache = dir.join("cache");
        let chart = |title: &str| {
            format!("#TITLE {title}\n#BPM 150\n#WAV01 kick.wav\n#00111:01\n#00112:0101\n")
        };
        let kick = wav(2205);
        let zip_of = |extra: bool| {
            let mut files: Vec<(&str, Vec<u8>)> = vec![
                ("Song/a.bme", chart("A").into_bytes()),
                ("Song/kick.wav", kick.clone()),
                ("Song/bga/stage.png", b"png".to_vec()),
                // a song in a folder of the song's (its own folder)
                ("Song/Another/b.bms", chart("B").into_bytes()),
                ("readme.txt", b"hi".to_vec()),
            ];
            if extra {
                files.push(("New/d.bme", chart("D").into_bytes()));
            }
            bmseeker::testkit::zip(&files)
        };
        let zip = packs.join("pack.zip");
        std::fs::write(&zip, zip_of(false)).unwrap();
        let c = chart("C");
        let seven_bytes = seven(&[
            ("notes/readme.txt", b"read me"),
            ("Other/c.bme", c.as_bytes()),
            ("Other/kick.wav", &kick),
            ("Other/big.bin", &[7u8; 50_000]),
        ]);
        let sz = dir.join("solid.7z");
        std::fs::write(&sz, seven_bytes).unwrap();
        // (a chart with no notes: listed, no song)
        std::fs::write(dir.join("song.rar"), bmseeker::testkit::SONG_RAR).unwrap();
        let run = || {
            let p = Arc::new(ScanProgress::default());
            scan(
                cache.join("songs.json"),
                vec![dir.clone()],
                [cache.clone()].into_iter().collect(),
                p.clone(),
            );
            p
        };
        let p = run();
        assert!(p.changed.load(Ordering::Relaxed));
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        let mut titles: Vec<(&str, &str)> = songs
            .iter()
            .map(|s| (s.title.as_str(), s.path.as_str()))
            .collect();
        titles.sort();
        let a = arcfs::join(&zip, Path::new("Song/a.bme"));
        let c_path = arcfs::join(&sz, Path::new("Other/c.bme"));
        assert_eq!(
            titles,
            [
                ("A", a.to_str().unwrap()),
                (
                    "B",
                    arcfs::join(&zip, Path::new("Song/Another/b.bms"))
                        .to_str()
                        .unwrap()
                ),
                ("C", c_path.to_str().unwrap()),
            ]
        );
        let song_a = songs.iter().find(|s| s.title == "A").unwrap();
        assert_eq!(song_a.notes, 3);
        assert_eq!(song_a.folder, packs.to_string_lossy());
        let song_c = songs.iter().find(|s| s.title == "C").unwrap();
        assert_eq!(
            song_c.folder,
            arcfs::join(&sz, Path::new("")).to_string_lossy()
        );
        assert_eq!(arcfs::shown_name("solid.7z!"), "solid.7z");
        assert!(arcfs::index_of(&dir.join("song.rar")).is_some_and(|ix| ix.files.len() == 2));

        // the folders and files in the archive, as the loaders see them
        let song = a.parent().unwrap();
        assert!(vfs::is_dir(song) && vfs::is_dir(&arcfs::join(&zip, Path::new(""))));
        assert!(vfs::is_file(&a) && !vfs::is_dir(&a));
        assert!(!vfs::is_file(&song.join("none.wav")));
        let mut names: Vec<String> = vfs::read_dir(song)
            .unwrap()
            .into_iter()
            .map(|e| {
                format!(
                    "{}{}",
                    e.path.file_name().unwrap().to_string_lossy(),
                    if e.is_dir { "/" } else { "" }
                )
            })
            .collect();
        names.sort();
        assert_eq!(names, ["Another/", "a.bme", "bga/", "kick.wav"]);
        assert_eq!(vfs::read(&a).unwrap(), chart("A").into_bytes());
        assert_eq!(vfs::stamp(&song.join("kick.wav")).1, kick.len() as i64);
        // a keysound found (`#WAV01 kick.wav`) and decoded as in a play
        let index = vfs::DirIndex::new(song);
        let k = index.find("KICK.ogg", vfs::AUDIO_EXTS).unwrap();
        assert_eq!(k, song.join("kick.wav"));
        let bytes = vfs::read(&k).unwrap();
        assert!(vibebms_audio::decode_bytes(bytes, "wav", 44100).is_ok());
        // nothing is written into an archive
        assert!(vfs::write(&song.join("x.txt"), b"x").is_err());
        assert!(!dir.join("packs/pack.zip!").exists());

        // a play: its song unpacked once, kept until another is chosen
        let opened = arcfs::times_opened(&zip);
        vfs::prepare_songs(std::slice::from_ref(&a)).unwrap();
        assert_eq!(arcfs::times_opened(&zip), opened + 1);
        assert!(arcfs::is_ready(&zip, Path::new("Song/bga/stage.png")));
        assert_eq!(vfs::read(&song.join("bga/stage.png")).unwrap(), b"png");
        vfs::prepare_songs(std::slice::from_ref(&a)).unwrap();
        vfs::keep_song(Some(&a));
        assert_eq!(arcfs::times_opened(&zip), opened + 1);
        assert_eq!(vfs::read(&k).unwrap(), kick);
        assert_eq!(arcfs::times_opened(&zip), opened + 1);
        vfs::keep_song(Some(&c_path));
        assert!(!arcfs::is_ready(&zip, Path::new("Song/a.bme")));

        // a 7z's file: its song unpacked (read from the start), then read
        // from memory
        let other = c_path.parent().unwrap();
        let opened = arcfs::times_opened(&sz);
        // (a file of no song: read alone, the rest not decoded)
        let notes = arcfs::join(&sz, Path::new("notes/readme.txt"));
        assert_eq!(vfs::read(&notes).unwrap(), b"read me");
        assert_eq!(vfs::read(&notes).unwrap(), b"read me");
        assert_eq!(arcfs::times_opened(&sz), opened + 1);
        let opened = opened + 1;
        assert_eq!(vfs::read(&other.join("kick.wav")).unwrap(), kick);
        assert_eq!(
            vfs::read(&other.join("big.bin")).unwrap(),
            vec![7u8; 50_000]
        );
        assert_eq!(vfs::read(&c_path).unwrap(), c.as_bytes());
        assert_eq!(arcfs::times_opened(&sz), opened + 1);
        // a large song goes into a folder: a video's file is there
        arcfs::set_memory_limit(Some(1000));
        vfs::prepare_songs(std::slice::from_ref(&a)).unwrap();
        arcfs::set_memory_limit(None);
        assert!(arcfs::in_folder(&zip, Path::new("Song/kick.wav")));
        assert_eq!(vfs::read(&k).unwrap(), kick);
        let on_disk = vfs::native_path(&k).unwrap();
        // (in the first cache's `unpacked/` of the run)
        assert!(on_disk.is_file() && arcfs::split(&on_disk).is_none());
        assert!(on_disk.parent().unwrap().ends_with("Song"));
        assert_eq!(std::fs::read(&on_disk).unwrap(), kick);
        vfs::keep_song(None);

        // unchanged: no archive opened, nothing read
        let opens = || [&zip, &sz, &dir.join("song.rar")].map(|a| arcfs::times_opened(a));
        let before = opens();
        let p = run();
        assert_eq!(opens(), before);
        assert_eq!(p.to_parse.load(Ordering::Relaxed), 0);
        assert!(!p.changed.load(Ordering::Relaxed));
        // (nor from the index kept on disk)
        let kept = std::fs::read_to_string(cache.join("archives.json")).unwrap();
        assert!(kept.contains("Song/kick.wav"), "{kept}");

        // the zip changed: read again (the 7z not)
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&zip, zip_of(true)).unwrap();
        let before = opens();
        let p = run();
        assert!(p.changed.load(Ordering::Relaxed));
        assert_eq!(opens(), [before[0] + 1, before[1], before[2]]);
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        assert_eq!(songs.len(), 4);
        assert!(songs.iter().any(|s| s.title == "D"
            && s.path == arcfs::join(&zip, Path::new("New/d.bme")).to_string_lossy()));

        // the zip unpacked beside it: its charts listed once, as files
        let copy = packs.join("pack/Song");
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join("a.bme"), chart("A")).unwrap();
        run();
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        let a_paths: Vec<&str> = songs
            .iter()
            .filter(|s| s.title == "A")
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(a_paths, [copy.join("a.bme").to_str().unwrap()]);
        std::fs::remove_dir_all(packs.join("pack")).unwrap();

        // the archive gone: its charts go
        std::fs::remove_file(&sz).unwrap();
        run();
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        assert_eq!(songs.len(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }
}
