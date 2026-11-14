//! Where a chart file added on its own belongs (a 差分 chart picked or
//! downloaded without its song's folder). A chart goes to a song folder only
//! when that folder holds every sound the chart uses *and* one of its
//! charts has the same title and artist: numbered sound names (`01.wav`,
//! `02.wav`, ...) are common, so the file names alone prove nothing. When
//! that leaves no single folder the user is asked.

use std::collections::HashSet;
use std::path::PathBuf;

/// What placement needs from a chart.
#[derive(Clone, Debug, Default)]
pub struct ChartInfo {
    pub title: String,
    pub artist: String,
    /// Sound files it uses (names as written in the chart).
    pub sounds: Vec<String>,
}

/// A folder of the library: its files and the charts in it.
#[derive(Clone, Debug, Default)]
pub struct FolderInfo {
    pub dir: PathBuf,
    /// File names in the folder.
    pub files: Vec<String>,
    pub charts: Vec<ChartInfo>,
}

#[derive(Debug, PartialEq)]
pub enum Placement {
    /// Where it is (its sounds are there, or no folder has them all).
    Stay,
    /// Into this folder.
    Move(PathBuf),
    /// One of these folders, or none: the user decides.
    Ask(Vec<PathBuf>),
}

/// File name without extension, lower case (BMS often names `.wav` files
/// that are shipped as `.ogg`).
fn stem(name: &str) -> String {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let s = match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    };
    s.to_lowercase()
}

/// `name` without bracketed parts (`[ANOTHER]`, `(obj: ...)`, `【...】`),
/// lower case, spaces collapsed.
fn strip_brackets(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '[' | '(' | '【' | '（' | '［' | '<' | '〔' => depth += 1,
            ']' | ')' | '】' | '）' | '］' | '>' | '〕' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.extend(c.to_lowercase()),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Titles match when one starts with the other once tags are gone
/// ("Arcadia -★1-" and "Arcadia"); the shorter needs three characters.
fn same_title(a: &str, b: &str) -> bool {
    let (a, b) = (strip_brackets(a), strip_brackets(b));
    let (short, long) = if a.chars().count() <= b.chars().count() {
        (a, b)
    } else {
        (b, a)
    };
    short.chars().count() >= 3 && long.starts_with(&short)
}

/// Artists match without the chart maker's part ("artist / obj: someone").
fn same_artist(a: &str, b: &str) -> bool {
    let clean = |s: &str| {
        let s = s.to_lowercase();
        let cut = ["/", "obj", "差分", "譜面"]
            .iter()
            .filter_map(|m| s.find(m))
            .min()
            .unwrap_or(s.len());
        strip_brackets(&s[..cut])
    };
    let (a, b) = (clean(a), clean(b));
    if a.is_empty() || b.is_empty() {
        return a == b;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    long.starts_with(&short)
}

/// Whether `files` hold every one of `sounds` (by name, any extension).
pub fn covers(files: &[String], sounds: &[String]) -> bool {
    let have: HashSet<String> = files.iter().map(|f| stem(f)).collect();
    sounds.iter().all(|s| have.contains(&stem(s)))
}

fn has_all(folder: &FolderInfo, sounds: &[String]) -> bool {
    covers(&folder.files, sounds)
}

/// Where `chart`, now in `own`, belongs among `folders`.
pub fn place(chart: &ChartInfo, own: &FolderInfo, folders: &[FolderInfo]) -> Placement {
    if chart.sounds.is_empty() || has_all(own, &chart.sounds) {
        return Placement::Stay;
    }
    let complete: Vec<&FolderInfo> = folders
        .iter()
        .filter(|f| f.dir != own.dir && has_all(f, &chart.sounds))
        .collect();
    let same: Vec<&FolderInfo> = complete
        .iter()
        .copied()
        .filter(|f| {
            f.charts.iter().any(|c| {
                same_title(&c.title, &chart.title) && same_artist(&c.artist, &chart.artist)
            })
        })
        .collect();
    match (same.len(), complete.len()) {
        (1, _) => Placement::Move(same[0].dir.clone()),
        (_, 0) => Placement::Stay,
        // the same song in several folders: ask among those; else among
        // every folder that has the sounds
        (n, _) if n > 1 => Placement::Ask(same.iter().map(|f| f.dir.clone()).collect()),
        _ => Placement::Ask(complete.iter().map(|f| f.dir.clone()).collect()),
    }
}

/// Where a chart file in the library belongs, with labels for asking.
#[derive(Debug, PartialEq)]
pub enum Located {
    /// Its sounds are beside it.
    Here,
    /// No folder has its sounds (yet).
    NoSong,
    Move(PathBuf),
    /// Candidate folders, each with a label naming its song.
    Ask(Vec<(PathBuf, String)>),
}

const CHART_EXTS: [&str; 5] = ["bms", "bme", "bml", "pms", "bmson"];

pub fn is_chart(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CHART_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every folder below `dir` with its file names.
pub fn folders(dir: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<String>> {
    fn walk(
        dir: &std::path::Path,
        depth: usize,
        out: &mut std::collections::BTreeMap<PathBuf, Vec<String>>,
    ) {
        for e in crate::platform::vfs::read_dir(dir).unwrap_or_default() {
            if e.is_dir {
                if depth < 8 {
                    walk(&e.path, depth + 1, out);
                }
            } else {
                out.entry(dir.to_path_buf())
                    .or_default()
                    .push(file_name(&e.path));
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, 0, &mut out);
    out
}

/// The title, artist and sounds of a chart file.
pub fn chart_info(path: &std::path::Path) -> Option<ChartInfo> {
    let bytes = crate::platform::vfs::read(path).ok()?;
    let c = vibebms_core::load_chart_bytes(path, &bytes, &Default::default()).ok()?;
    Some(ChartInfo {
        title: c.meta.title,
        artist: c.meta.artist,
        sounds: c.sound_files,
    })
}

/// Where `chart` belongs among the folders below `library` (see [`place`]).
pub fn locate(chart: &std::path::Path, library: &std::path::Path) -> Option<Located> {
    let info = chart_info(chart)?;
    let here = chart.parent()?.to_path_buf();
    let all = folders(library);
    let own = FolderInfo {
        dir: here.clone(),
        files: crate::platform::vfs::read_dir(&here)
            .unwrap_or_default()
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| file_name(&e.path))
            .collect(),
        charts: Vec::new(),
    };
    if covers(&own.files, &info.sounds) {
        return Some(Located::Here);
    }
    // only folders with every sound are candidates: read their charts
    let candidates: Vec<FolderInfo> = all
        .into_iter()
        .filter(|(d, files)| *d != here && covers(files, &info.sounds))
        .map(|(dir, files)| FolderInfo {
            charts: files
                .iter()
                .map(|n| dir.join(n))
                .filter(|p| is_chart(p))
                .filter_map(|p| chart_info(&p))
                .collect(),
            dir,
            files,
        })
        .collect();
    Some(match place(&info, &own, &candidates) {
        Placement::Move(d) => Located::Move(d),
        Placement::Stay => Located::NoSong,
        Placement::Ask(dirs) => Located::Ask(
            dirs.into_iter()
                .map(|d| {
                    let song = candidates
                        .iter()
                        .find(|f| f.dir == d)
                        .and_then(|f| f.charts.first())
                        .map(|c| format!("{} — {}", c.title, c.artist))
                        .unwrap_or_default();
                    let label = format!("{song}  ({})", file_name(&d));
                    (d, label)
                })
                .collect(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(title: &str, artist: &str, sounds: &[&str]) -> ChartInfo {
        ChartInfo {
            title: title.into(),
            artist: artist.into(),
            sounds: sounds.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn folder(dir: &str, files: &[&str], charts: Vec<ChartInfo>) -> FolderInfo {
        FolderInfo {
            dir: dir.into(),
            files: files.iter().map(|s| s.to_string()).collect(),
            charts,
        }
    }

    #[test]
    fn titles_and_artists() {
        assert!(same_title("Arcadia -★1-", "Arcadia"));
        assert!(same_title("Mirage Garden [ANOTHER]", "Mirage Garden"));
        assert!(!same_title("X (虎のANOTHER)", "X [7KEYS]")); // too short to tell
        assert!(!same_title("Love Story", "Lovely Day"));
        assert!(same_artist(
            "ねこみみ魔法使い / obj:秘密",
            "ねこみみ魔法使い"
        ));
        assert!(same_artist("Squar / 差分 赤い人", "Squar"));
        assert!(!same_artist("xi", "Grand Thaw"));
    }

    #[test]
    fn numbered_sounds_need_the_same_song() {
        let diff = chart("Song B [INSANE]", "beta / obj: x", &["01.wav", "02.wav"]);
        let picked = folder("picked", &["b_insane.bme"], vec![diff.clone()]);
        let a = folder(
            "A",
            &["01.ogg", "02.ogg", "a.bms"],
            vec![chart("Song A", "alpha", &[])],
        );
        let b = folder(
            "B",
            &["01.ogg", "02.ogg", "b.bms"],
            vec![chart("Song B", "beta", &[])],
        );
        // both folders have 01 / 02; only B is the same song
        assert_eq!(
            place(&diff, &picked, &[a.clone(), b.clone()]),
            Placement::Move("B".into())
        );
        // the same song nowhere: ask among the folders that have the sounds
        let other = chart("Song C", "gamma", &["01.wav", "02.wav"]);
        assert_eq!(
            place(&other, &picked, &[a.clone(), b]),
            Placement::Ask(vec!["A".into(), "B".into()])
        );
        // no folder has every sound: it stays
        let more = chart("Song B", "beta", &["01.wav", "03.wav"]);
        assert_eq!(place(&more, &picked, &[a]), Placement::Stay);
    }

    #[test]
    fn a_chart_with_its_sounds_stays() {
        let c = chart("Song", "artist", &["kick.wav"]);
        let own = folder("picked", &["kick.wav", "song.bme"], vec![c.clone()]);
        let other = folder("S", &["kick.wav"], vec![chart("Song", "artist", &[])]);
        assert_eq!(place(&c, &own, &[other]), Placement::Stay);
    }
}
