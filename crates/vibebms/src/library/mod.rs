//! Song library and score store.
//!
//! Stored as JSON in `cache/songs.json` (rebuildable scan cache: `scan`),
//! `scores.json` (your play records) and `player.json` (totals over all
//! plays: `store`) so it works identically on every platform, including
//! the web build. Scanning runs in the background. Beside them, the
//! player's other data: courses, replays, scores imported from other
//! players' databases, and one chart held in several folders.

pub mod charts;
pub mod courses;
pub mod lr2db;
pub mod replay;
mod scan;
pub mod scoredb;
pub mod sqlite;
mod store;

use crate::platform::arcfs;
use serde::{Deserialize, Serialize};
use std::path::Path;
use vibebms_core::judge::JudgePreset;
use vibebms_core::score::ClearType;
use vibebms_core::{LnPref, LnProfile, LnType, PlayMode};

pub use scan::{
    cached_songs, chart_info, find_charts, going_on, scan, song_from_chart, stopping, ScanProgress,
};
pub use store::{read_records, Library};

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
    /// No sound file is in its folder (a chart alone, as in a downloads or
    /// backup folder), as the last scan found it: of a chart's copies, one
    /// with sounds is taken first (`charts`). Never for one in an archive.
    pub bare: bool,
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

    /// Whether its file is inside an archive (`platform::arcfs`).
    pub fn in_archive(&self) -> bool {
        arcfs::split(Path::new(&self.path)).is_some()
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

/// The image a chart names (`#STAGEFILE`, `#BANNER`, `#BACKBMP`) in its
/// song's folder `dir`, trying common extension substitutions. The name
/// is the chart author's text: one that leaves the folder finds nothing
/// ([`crate::platform::vfs::inside`], which the lookup starts with).
pub fn chart_image(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    use crate::platform::vfs;
    vfs::find_by_stem(dir, name, vfs::IMAGE_EXTS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `#STAGEFILE` / `#BANNER` are looked for in the song's folder only,
    /// whatever screen asks.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_chart_image_is_in_its_songs_folder() {
        let root = std::env::temp_dir().join(format!("vibebms-chartimg-{}", std::process::id()));
        let dir = root.join("bms/Song");
        std::fs::create_dir_all(dir.join("img")).unwrap();
        for f in ["secret.png", "bms/other.png", "bms/Song/bg.png"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        std::fs::write(dir.join("img/Title.JPG"), b"x").unwrap();
        let found = |name: &str| chart_image(&dir, name);
        assert_eq!(found("bg.png"), Some(dir.join("bg.png")));
        assert_eq!(found("BG.bmp"), Some(dir.join("bg.png")));
        assert_eq!(found("img\\title.bmp"), Some(dir.join("img/Title.JPG")));
        let outside = root.join("secret.png");
        for bad in [
            "../other.png",
            "../../secret.png",
            "img/../../other.png",
            outside.to_str().unwrap(),
            "\\\\host\\share\\a.png",
            "//host/share/a.png",
            "",
        ] {
            assert_eq!(found(bad), None, "{bad}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// A song in an archive: its images are the archive's files.
    #[test]
    fn a_chart_image_in_an_archive() {
        use crate::platform::arcfs;
        let pack = Path::new("/vibebms-chartimg/pack.zip");
        let files = ["Song/a.bms", "Song/bg.png", "top.png"]
            .iter()
            .map(|p| (PathBuf::from(p), 1))
            .collect();
        arcfs::set_index(pack, arcfs::Index::new((0, 0), "zip", files));
        let dir = arcfs::join(pack, Path::new("Song"));
        assert_eq!(chart_image(&dir, "bg.bmp"), Some(dir.join("bg.png")));
        assert_eq!(chart_image(&dir, "../top.png"), None);
    }
}
