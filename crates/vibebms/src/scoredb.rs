//! Scores from another player's database, filed as vibeBMS's records:
//!
//! - LR2's (`LR2files/Database/Score/<player>.db`): its `score` table, a
//!   row per chart MD5, under the LR2 rule mode, every long note as LN
//!   (LR2 has no other kind).
//! - beatoraja's and LR2oraja's (`player/<name>/score.db`): their `score`
//!   table, a row per chart SHA-256 and LN mode (`mode`: 0 LN, 1 CN,
//!   2 HCN), under the beatoraja rule mode, or LR2's for LR2oraja.
//!
//! A row is a best: its lamp, its best EX score's counts, its max combo and
//! min BP (each from whichever play was best at it) and its play and clear
//! counts. Rows of charts not in the library, and of courses, are counted,
//! not kept. Read with `lr2db`'s SQLite reader; the tables' layout is
//! learnt from the files and public descriptions of them.

use crate::library::{BestScore, RecordKey, Song};
use crate::lr2db::{get, Db, Row, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use vibebms_core::judge::JudgePreset;
use vibebms_core::score::ClearType;
use vibebms_core::LnType;

/// Whose database it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Lr2,
    Beatoraja,
    Lr2oraja,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Lr2 => "LR2",
            Source::Beatoraja => "beatoraja",
            Source::Lr2oraja => "LR2oraja",
        }
    }

    /// The rule mode its plays are filed under.
    fn rule(self) -> JudgePreset {
        match self {
            Source::Beatoraja => JudgePreset::Beatoraja,
            Source::Lr2 | Source::Lr2oraja => JudgePreset::Lr2,
        }
    }
}

/// A chart's row: its MD5 (LR2) or SHA-256, the LN mode it was played in
/// (beatoraja's `mode`; LR2: LN) and its best.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartRow {
    pub hash: String,
    pub ln: LnType,
    pub best: BestScore,
}

/// What a database holds.
#[derive(Debug, PartialEq)]
pub struct Rows {
    pub source: Source,
    pub charts: Vec<ChartRow>,
    /// Rows left out: courses, rows of no play, rows that do not add up.
    pub skipped: usize,
}

fn int(r: &Row, c: &str) -> i64 {
    get(r, c).and_then(Value::int).unwrap_or(0)
}

/// A count: negative ones (unknown) as 0.
fn count(r: &Row, c: &str) -> u32 {
    int(r, c).clamp(0, u32::MAX as i64) as u32
}

/// A min BP: LR2 keeps -1, beatoraja `i32::MAX` before one is known.
fn min_bp(r: &Row) -> u32 {
    match int(r, "minbp") {
        v if (0..i32::MAX as i64).contains(&v) => v as u32,
        _ => u32::MAX,
    }
}

/// LR2's lamp ids: NO PLAY, FAILED, EASY, NORMAL, HARD, FULL COMBO.
fn lr2_clear(id: i64) -> Option<ClearType> {
    use ClearType::*;
    [NoPlay, Failed, Easy, Normal, Hard, FullCombo]
        .get(usize::try_from(id).ok()?)
        .copied()
}

/// `b` with what follows from it: its EX score, a CB of 0 for a lamp of
/// FULL COMBO or more, when its lamp and score were made (`date`, Unix
/// s; 0: unknown); `None` for a row that holds no play or does not add up.
fn finish(mut b: BestScore, date: i64) -> Option<BestScore> {
    let [pg, gr, ..] = b.judges;
    b.ex_score = pg.saturating_mul(2).saturating_add(gr);
    if b.clear >= ClearType::FullCombo {
        b.min_cb = 0;
    }
    b.last_played = date;
    if b.clear > ClearType::Failed {
        b.lamp_updated = date;
    }
    if b.ex_score > 0 {
        b.score_updated = date;
    }
    let played = b.play_count > 0 || b.clear > ClearType::NoPlay || b.ex_score > 0;
    let sane = b.ex_score <= b.notes.saturating_mul(2);
    (played && sane).then_some(b)
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// An LR2 row: `hash` the chart's MD5 (a course's packed string is
/// longer), `clear` LR2's lamp, `perfect`..`poor` the counts of its best
/// EX score (`poor` takes empty POORs too, which LR2 does not keep apart),
/// `totalnotes`, `maxcombo`, `minbp`, `playcount`, `clearcount`.
fn lr2_row(r: &Row) -> Option<ChartRow> {
    let hash = get(r, "hash")?.text()?.to_ascii_lowercase();
    if !is_hex(&hash, 32) {
        return None;
    }
    let c = |name| count(r, name);
    let best = BestScore {
        clear: lr2_clear(int(r, "clear"))?,
        notes: c("totalnotes"),
        judges: [c("perfect"), c("great"), c("good"), c("bad"), c("poor"), 0],
        max_combo: count(r, "maxcombo"),
        min_bp: min_bp(r),
        min_cb: u32::MAX,
        play_count: count(r, "playcount"),
        clear_count: count(r, "clearcount"),
        ..Default::default()
    };
    Some(ChartRow {
        hash,
        ln: LnType::Ln,
        best: finish(best, 0)?,
    })
}

/// A beatoraja row: `sha256` the chart's (a course's: its charts'
/// joined), `mode` the LN mode, `clear` the lamp (vibeBMS's ids are
/// beatoraja's), `epg`/`lpg` .. `ems`/`lms` the early and late counts of
/// the best EX score, `notes`, `combo`, `minbp`, `playcount`,
/// `clearcount`, `date` (Unix s).
fn beatoraja_row(r: &Row) -> Option<ChartRow> {
    let hash = get(r, "sha256")?.text()?.to_ascii_lowercase();
    if !is_hex(&hash, 64) {
        return None;
    }
    let ln = *[LnType::Ln, LnType::Cn, LnType::Hcn].get(usize::try_from(int(r, "mode")).ok()?)?;
    let clear = int(r, "clear");
    if !(0..ClearType::ALL.len() as i64).contains(&clear) {
        return None;
    }
    let judges = ["pg", "gr", "gd", "bd", "pr", "ms"]
        .map(|j| count(r, &format!("e{j}")).saturating_add(count(r, &format!("l{j}"))));
    let best = BestScore {
        clear: ClearType::from_id(clear),
        notes: count(r, "notes"),
        judges,
        max_combo: count(r, "combo"),
        min_bp: min_bp(r),
        min_cb: u32::MAX,
        play_count: count(r, "playcount"),
        clear_count: count(r, "clearcount"),
        ..Default::default()
    };
    Some(ChartRow {
        hash,
        ln,
        best: finish(best, int(r, "date").max(0))?,
    })
}

/// The rows of a score database: LR2's (its `score` table has `hash`) or
/// beatoraja's (it has `sha256`), LR2oraja's when `lr2oraja`.
pub fn read(bytes: &[u8], lr2oraja: bool) -> Result<Rows, String> {
    let db = Db::open(bytes)?;
    let rows = db.table("score").ok_or("no score table in it")?;
    let has = |c: &str| rows.first().is_some_and(|r| get(r, c).is_some());
    let (source, row): (_, fn(&Row) -> Option<ChartRow>) = if has("sha256") {
        let s = if lr2oraja {
            Source::Lr2oraja
        } else {
            Source::Beatoraja
        };
        (s, beatoraja_row)
    } else if has("hash") || rows.is_empty() {
        (Source::Lr2, lr2_row)
    } else {
        return Err("not an LR2 or beatoraja score table".into());
    };
    let charts: Vec<ChartRow> = rows.iter().filter_map(row).collect();
    Ok(Rows {
        source,
        skipped: rows.len() - charts.len(),
        charts,
    })
}

/// The records `rows` make for the charts of `songs` (by MD5 for LR2,
/// else by SHA-256), each under the LN mode the chart's plays are filed
/// under (`Song::score_ln`) and the source's rule mode; rows that fall on
/// one record are taken together. Also: how many rows were of charts not
/// in the library.
pub fn records(rows: &Rows, songs: &[Song]) -> (Vec<(RecordKey, BestScore)>, usize) {
    let by: HashMap<&str, &Song> = songs
        .iter()
        .map(|s| {
            let h = match rows.source {
                Source::Lr2 => &s.md5,
                _ => &s.sha256,
            };
            (h.as_str(), s)
        })
        .collect();
    let rule = rows.source.rule();
    let mut out: Vec<(RecordKey, BestScore)> = Vec::new();
    let mut at: HashMap<RecordKey, usize> = HashMap::new();
    let mut unknown = 0;
    for r in &rows.charts {
        let Some(song) = by.get(r.hash.as_str()) else {
            unknown += 1;
            continue;
        };
        let key = RecordKey::new(&song.sha256, song.score_ln(r.ln) as i32, rule);
        match at.get(&key) {
            Some(&i) => out[i].1 = out[i].1.absorb(&r.best),
            None => {
                at.insert(key.clone(), out.len());
                out.push((key, r.best.clone()));
            }
        }
    }
    (out, unknown)
}

/// The score database at `path`: the file itself, or in that folder a
/// beatoraja player's `score.db`, the `score.db` of a beatoraja
/// install's only player, or an LR2 install's only player database
/// (`LR2files/Database/Score/*.db`). With several players, the error
/// names them.
pub fn find(path: &Path) -> Result<PathBuf, String> {
    use crate::platform::vfs;
    if vfs::is_file(path) {
        return Ok(path.to_path_buf());
    }
    if vfs::is_file(&path.join("score.db")) {
        return Ok(path.join("score.db"));
    }
    let dbs_in = |dir: PathBuf| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = vfs::read_dir(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| {
                !e.is_dir
                    && e.path
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("db"))
            })
            .map(|e| e.path)
            .collect();
        v.sort();
        v
    };
    let mut players: Vec<PathBuf> = vfs::read_dir(&path.join("player"))
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.path.join("score.db"))
        .filter(|p| vfs::is_file(p))
        .collect();
    players.sort();
    let lr2 = [
        path.join("LR2files").join("Database").join("Score"),
        path.join("Database").join("Score"),
        path.join("Score"),
        path.to_path_buf(),
    ];
    let mut found = match players.is_empty() {
        true => lr2
            .into_iter()
            .map(dbs_in)
            .find(|v| !v.is_empty())
            .unwrap_or_default(),
        false => players,
    };
    match found.len() {
        0 => Err(format!("no score database at {}", path.display())),
        1 => Ok(found.remove(0)),
        _ => {
            let names: Vec<String> = found
                .iter()
                .map(|p| {
                    let p = match p.file_name().is_some_and(|n| n == "score.db") {
                        true => p.parent().unwrap_or(p),
                        false => p.as_path(),
                    };
                    p.file_stem().unwrap_or_default().to_string_lossy().into()
                })
                .collect();
            Err(format!(
                "several players there ({}): pick one",
                names.join(", ")
            ))
        }
    }
}

/// `db` (a `player/<name>/score.db`) is LR2oraja's: its install (the
/// folder of `player/`) has an LR2oraja program (`lr2oraja*.jar`, ...).
/// The database itself does not tell them apart.
pub fn is_lr2oraja(db: &Path) -> bool {
    let Some(root) = db.parent().and_then(Path::parent).and_then(Path::parent) else {
        return false;
    };
    crate::platform::vfs::read_dir(root)
        .unwrap_or_default()
        .iter()
        .filter(|e| !e.is_dir)
        .filter_map(|e| e.path.file_name())
        .any(|n| {
            n.to_string_lossy()
                .to_ascii_lowercase()
                .contains("lr2oraja")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lr2db::write::{db, V};

    const LR2_SQL: &str = "CREATE TABLE score(hash TEXT primary key, clear INTEGER, \
        perfect INTEGER, great INTEGER, good INTEGER, bad INTEGER, poor INTEGER, \
        totalnotes INTEGER, maxcombo INTEGER, minbp INTEGER, playcount INTEGER, \
        clearcount INTEGER, failcount INTEGER, rank INTEGER, rate INTEGER, \
        clear_db INTEGER, op_history INTEGER, scorehash TEXT, ghost TEXT, \
        clear_sd INTEGER, clear_ex INTEGER, op_best INTEGER, rseed INTEGER, \
        complete INTEGER)";

    const ORAJA_SQL: &str = "CREATE TABLE [score] ([sha256] TEXT NOT NULL, \
        [mode] INTEGER, [clear] INTEGER, [epg] INTEGER, [lpg] INTEGER, \
        [egr] INTEGER, [lgr] INTEGER, [egd] INTEGER, [lgd] INTEGER, \
        [ebd] INTEGER, [lbd] INTEGER, [epr] INTEGER, [lpr] INTEGER, \
        [ems] INTEGER, [lms] INTEGER, [notes] INTEGER, [combo] INTEGER, \
        [minbp] INTEGER, [playcount] INTEGER, [clearcount] INTEGER, \
        [trophy] TEXT, [ghost] TEXT, [option] INTEGER, [seed] INTEGER, \
        [random] INTEGER, [date] INTEGER, [state] INTEGER, [scorehash] TEXT, \
        PRIMARY KEY(sha256, mode))";

    fn h(c: char, n: usize) -> String {
        std::iter::repeat_n(c, n).collect()
    }

    /// An LR2 row: hash, clear, [PG, GR, GD, BD, PR], notes, combo, min
    /// BP, plays, clears.
    #[allow(clippy::too_many_arguments)]
    fn lr2(
        hash: &str,
        clear: i64,
        j: [i64; 5],
        notes: i64,
        combo: i64,
        bp: i64,
        plays: i64,
        clears: i64,
    ) -> Vec<V<'_>> {
        let mut v = vec![V::Text(hash.as_bytes()), V::Int(clear)];
        v.extend(j.map(V::Int));
        v.extend([notes, combo, bp, plays, clears].map(V::Int));
        // failcount .. complete
        v.extend([0, 0, 0, 0, 0].map(V::Int));
        v.extend([V::Text(b"x"), V::Text(b"ghost")]);
        v.extend([0, 0, 0, 0, 1].map(V::Int));
        v
    }

    fn lr2_db() -> Vec<u8> {
        let a = h('a', 32);
        // (hex digits in either case)
        let b = h('B', 32);
        let (unknown, unplayed) = (h('c', 32), h('d', 32));
        let course = h('0', 32) + &a + &b;
        let rows = vec![
            lr2(&a, 4, [500, 300, 20, 5, 10], 835, 400, 15, 12, 3),
            lr2(&b, 5, [900, 100, 0, 0, 0], 1000, 1000, 0, 2, 2),
            lr2(&unknown, 1, [1, 1, 0, 0, 0], 10, 2, -1, 1, 0),
            lr2(&course, 2, [1, 0, 0, 0, 0], 2, 1, 1, 1, 1),
            // opened, never played
            lr2(&unplayed, 0, [0; 5], 0, 0, -1, 0, 0),
        ];
        db(&[(
            "score",
            LR2_SQL,
            rows.into_iter()
                .enumerate()
                .map(|(i, v)| (i as i64 + 1, v))
                .collect(),
        )])
    }

    /// A beatoraja row: sha256, mode, clear, [PG..MS] early and late,
    /// notes, combo, min BP, plays, clears, date.
    #[allow(clippy::too_many_arguments)]
    fn oraja(
        sha: &str,
        mode: i64,
        clear: i64,
        el: [i64; 12],
        notes: i64,
        combo: i64,
        bp: i64,
        plays: i64,
        clears: i64,
        date: i64,
    ) -> Vec<V<'_>> {
        let mut v = vec![V::Text(sha.as_bytes()), V::Int(mode), V::Int(clear)];
        v.extend(el.map(V::Int));
        v.extend([notes, combo, bp, plays, clears].map(V::Int));
        v.extend([V::Text(b""), V::Text(b"ghost")]);
        v.extend([0, 0, 0, date, 0].map(V::Int));
        v.push(V::Null);
        v
    }

    fn oraja_db() -> Vec<u8> {
        let a = h('a', 64);
        let b = h('b', 64);
        let course = a.clone() + &b;
        let e = h('e', 64);
        let el = [400, 100, 50, 50, 5, 5, 1, 1, 2, 2, 3, 4];
        let rows = vec![
            oraja(&a, 0, 7, el, 700, 300, 13, 40, 20, 1_700_000_000),
            oraja(&a, 1, 4, el, 800, 200, 20, 3, 1, 1_600_000_000),
            oraja(
                &b,
                2,
                10,
                [5, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                10,
                10,
                0,
                1,
                1,
                0,
            ),
            // never got a min BP
            oraja(&e, 0, 1, [0; 12], 100, 0, i32::MAX as i64, 1, 0, 0),
            oraja(&course, 0, 5, el, 1500, 300, 13, 1, 1, 0),
            // a mode it does not know
            oraja(&b, 9, 5, el, 700, 300, 13, 1, 1, 0),
        ];
        db(&[(
            "score",
            ORAJA_SQL,
            rows.into_iter()
                .enumerate()
                .map(|(i, v)| (i as i64 + 1, v))
                .collect(),
        )])
    }

    fn song(md5: char, sha: char, ln: Option<u8>) -> Song {
        Song {
            md5: h(md5, 32),
            sha256: h(sha, 64),
            has_ln: ln.is_some_and(|b| b != 0),
            ln_profile: Some(ln.unwrap_or(0)),
            ..Default::default()
        }
    }

    #[test]
    fn lr2_rows_are_read_as_lr2_keeps_them() {
        let rows = read(&lr2_db(), false).unwrap();
        assert_eq!(rows.source, Source::Lr2);
        // the course and the row of no play left out
        assert_eq!((rows.charts.len(), rows.skipped), (3, 2));
        let a = &rows.charts[0];
        assert_eq!((a.hash.as_str(), a.ln), (h('a', 32).as_str(), LnType::Ln));
        let b = &a.best;
        assert_eq!(b.clear, ClearType::Hard);
        assert_eq!(
            (b.ex_score, b.notes, b.judges),
            (1300, 835, [500, 300, 20, 5, 10, 0])
        );
        assert_eq!((b.max_combo, b.min_bp, b.min_cb), (400, 15, u32::MAX));
        assert_eq!((b.play_count, b.clear_count), (12, 3));
        let fc = &rows.charts[1];
        assert_eq!(fc.hash, h('b', 32));
        assert_eq!((fc.best.clear, fc.best.min_cb), (ClearType::FullCombo, 0));
        // -1: no min BP known
        assert_eq!(rows.charts[2].best.min_bp, u32::MAX);
    }

    #[test]
    fn beatoraja_rows_keep_their_ln_mode() {
        let rows = read(&oraja_db(), false).unwrap();
        assert_eq!(rows.source, Source::Beatoraja);
        assert_eq!((rows.charts.len(), rows.skipped), (4, 2));
        let a = &rows.charts[0];
        assert_eq!((a.ln, a.best.clear), (LnType::Ln, ClearType::ExHard));
        assert_eq!(a.best.judges, [500, 100, 10, 2, 4, 7]);
        assert_eq!((a.best.ex_score, a.best.notes), (1100, 700));
        assert_eq!(
            (a.best.last_played, a.best.lamp_updated),
            (1_700_000_000, 1_700_000_000)
        );
        assert_eq!(rows.charts[1].ln, LnType::Cn);
        let b = &rows.charts[2];
        assert_eq!(
            (b.ln, b.best.clear, b.best.min_cb),
            (LnType::Hcn, ClearType::Max, 0)
        );
        // i32::MAX: no min BP known
        assert_eq!(rows.charts[3].best.min_bp, u32::MAX);
        assert_eq!(read(&oraja_db(), true).unwrap().source, Source::Lr2oraja);
    }

    #[test]
    fn what_is_not_a_score_database_says_so() {
        assert!(read(b"nope", false).is_err());
        let other = db(&[("song", "CREATE TABLE song (hash TEXT)", Vec::new())]);
        assert!(read(&other, false).is_err());
        let odd = db(&[(
            "score",
            "CREATE TABLE score (id INTEGER)",
            vec![(1, vec![V::Int(1)])],
        )]);
        assert!(read(&odd, false).is_err());
    }

    #[test]
    fn rows_are_filed_as_vibes_records() {
        // a: long notes of no type (filed under the mode played);
        // b: CN typed by the chart (filed under CN whatever the mode)
        let undefined = vibebms_core::LnProfile {
            undefined: true,
            typed: [false; 3],
        }
        .bits();
        let songs = [song('a', 'a', Some(undefined)), song('b', 'b', Some(0b100))];
        let (recs, unknown) = records(&read(&lr2_db(), false).unwrap(), &songs);
        assert_eq!(unknown, 1);
        let keys: Vec<(char, i32, JudgePreset)> = recs
            .iter()
            .map(|(k, _)| (k.id.chars().next().unwrap(), k.ln, k.rule))
            .collect();
        assert_eq!(
            keys,
            [('a', 0, JudgePreset::Lr2), ('b', 1, JudgePreset::Lr2)]
        );

        let rows = read(&oraja_db(), false).unwrap();
        let (recs, unknown) = records(&rows, &songs);
        // the chart of `e` is not in the library
        assert_eq!(unknown, 1);
        let keys: Vec<(char, i32, JudgePreset)> = recs
            .iter()
            .map(|(k, _)| (k.id.chars().next().unwrap(), k.ln, k.rule))
            .collect();
        // b's HCN row is filed under the chart's CN
        let rule = JudgePreset::Beatoraja;
        assert_eq!(keys, [('a', 0, rule), ('a', 1, rule), ('b', 1, rule)]);
        let rows = Rows {
            source: Source::Lr2oraja,
            ..rows
        };
        assert!(records(&rows, &songs)
            .0
            .iter()
            .all(|(k, _)| k.rule == JudgePreset::Lr2));
    }

    #[test]
    fn rows_on_one_record_are_taken_together() {
        let fixed = song('a', 'a', Some(0b100));
        let rows = read(&oraja_db(), false).unwrap();
        let (recs, _) = records(&rows, &[fixed]);
        assert_eq!(recs.len(), 1);
        let b = &recs[0].1;
        assert_eq!(
            (b.clear, b.play_count, b.min_bp),
            (ClearType::ExHard, 40, 13)
        );
    }

    #[test]
    fn a_folder_finds_its_one_database() {
        let dir = std::env::temp_dir().join(format!("vibe-scoredb-{}", std::process::id()));
        let p = dir.join("player").join("me");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("score.db"), b"x").unwrap();
        std::fs::write(dir.join("lr2oraja-endlessdream.jar"), b"").unwrap();
        assert_eq!(find(&dir).unwrap(), p.join("score.db"));
        assert_eq!(find(&p).unwrap(), p.join("score.db"));
        assert!(is_lr2oraja(&p.join("score.db")));
        std::fs::remove_file(dir.join("lr2oraja-endlessdream.jar")).unwrap();
        std::fs::write(dir.join("beatoraja.jar"), b"").unwrap();
        assert!(!is_lr2oraja(&p.join("score.db")));
        std::fs::create_dir_all(dir.join("player").join("you")).unwrap();
        std::fs::write(dir.join("player").join("you").join("score.db"), b"x").unwrap();
        let e = find(&dir).unwrap_err();
        assert!(e.contains("me, you"), "{e}");

        let lr2 = dir.join("lr2");
        let s = lr2.join("LR2files").join("Database").join("Score");
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("PLAYER.db"), b"x").unwrap();
        assert_eq!(find(&lr2).unwrap(), s.join("PLAYER.db"));
        assert!(find(&dir.join("none")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
