//! Where a chart file added on its own belongs (a 差分 chart picked or
//! downloaded without its song's folder), as BeMusicSeeker estimates an
//! install destination. A folder is a candidate when, with the files the
//! chart came with, it holds the sounds the chart uses ([`Rules`]: 99% of
//! them); the chart goes there without asking only when one of its charts
//! is the same song (title and artist): numbered sound names (`01.wav`,
//! `02.wav`, ...) are common, so the file names alone prove nothing. Else
//! the user picks among the best candidates.

use crate::fs::Fs;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
pub fn same_title(a: &str, b: &str) -> bool {
    let (a, b) = (strip_brackets(a), strip_brackets(b));
    let (short, long) = if a.chars().count() <= b.chars().count() {
        (a, b)
    } else {
        (b, a)
    };
    short.chars().count() >= 3 && long.starts_with(&short)
}

/// The same title, but for its brackets ("Song [ANOTHER]" is "Song").
pub fn same_name(a: &str, b: &str) -> bool {
    let a = strip_brackets(a);
    !a.is_empty() && a == strip_brackets(b)
}

/// Artists match without the chart maker's part ("artist / obj: someone").
pub fn same_artist(a: &str, b: &str) -> bool {
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

/// How a chart's folder is chosen.
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    /// The share of a chart's sounds a folder must hold, with the files the
    /// chart came with, to be its song's folder.
    pub min_coverage: f32,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules { min_coverage: 0.99 }
    }
}

/// A folder a chart may belong in.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub dir: PathBuf,
    /// The share of the chart's sounds there, with the files it came with.
    pub coverage: f32,
    /// How many of them the folder holds itself.
    pub own: usize,
    /// One of the folder's charts has the same title and artist.
    pub same_song: bool,
}

/// The folders among `folders` for `chart`, which came with the files
/// `bundled`: those holding (with them) at least one of its sounds, the best
/// first (the same song, then more of its sounds, then more of them in the
/// folder itself).
pub fn rank(chart: &ChartInfo, bundled: &[String], folders: &[FolderInfo]) -> Vec<Candidate> {
    let sounds: HashSet<String> = chart.sounds.iter().map(|s| stem(s)).collect();
    if sounds.is_empty() {
        return Vec::new();
    }
    let brought: HashSet<String> = bundled.iter().map(|f| stem(f)).collect();
    let mut out: Vec<Candidate> = folders
        .iter()
        .filter_map(|f| {
            let have: HashSet<String> = f.files.iter().map(|n| stem(n)).collect();
            let own = sounds.iter().filter(|s| have.contains(*s)).count();
            if own == 0 {
                return None;
            }
            let all = sounds
                .iter()
                .filter(|s| have.contains(*s) || brought.contains(*s))
                .count();
            Some(Candidate {
                dir: f.dir.clone(),
                coverage: all as f32 / sounds.len() as f32,
                own,
                same_song: f.charts.iter().any(|c| {
                    same_title(&c.title, &chart.title) && same_artist(&c.artist, &chart.artist)
                }),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        (b.same_song, b.coverage, b.own)
            .partial_cmp(&(a.same_song, a.coverage, a.own))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// What [`rank`]'s candidates mean: the one folder that is the same song
/// and holds the chart's sounds; else the folders that hold them (the user
/// picks, at most 3); else none yet.
pub fn decide(candidates: &[Candidate], rules: Rules) -> Placement {
    let enough: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.coverage >= rules.min_coverage)
        .collect();
    let same: Vec<&Candidate> = enough.iter().copied().filter(|c| c.same_song).collect();
    let dirs = |c: &[&Candidate]| c.iter().take(3).map(|c| c.dir.clone()).collect();
    match (same.len(), enough.len()) {
        (1, _) => Placement::Move(same[0].dir.clone()),
        (_, 0) => Placement::Stay,
        (n, _) if n > 1 => Placement::Ask(dirs(&same)),
        _ => Placement::Ask(dirs(&enough)),
    }
}

/// Where `chart`, now in `own` (with the files it came with), belongs among
/// `folders` (by the default [`Rules`]).
pub fn place(chart: &ChartInfo, own: &FolderInfo, folders: &[FolderInfo]) -> Placement {
    if chart.sounds.is_empty() || covers(&own.files, &chart.sounds) {
        return Placement::Stay;
    }
    let others: Vec<FolderInfo> = folders
        .iter()
        .filter(|f| f.dir != own.dir)
        .cloned()
        .collect();
    decide(&rank(chart, &own.files, &others), Rules::default())
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
pub fn folders(
    fs: &dyn Fs,
    dir: &std::path::Path,
) -> std::collections::BTreeMap<PathBuf, Vec<String>> {
    fn walk(
        fs: &dyn Fs,
        dir: &std::path::Path,
        depth: usize,
        out: &mut std::collections::BTreeMap<PathBuf, Vec<String>>,
    ) {
        for e in fs.read_dir(dir).unwrap_or_default() {
            if e.is_dir {
                if depth < 8 {
                    walk(fs, &e.path, depth + 1, out);
                }
            } else {
                out.entry(dir.to_path_buf())
                    .or_default()
                    .push(file_name(&e.path));
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(fs, dir, 0, &mut out);
    out
}

/// The title, artist and sounds of a chart file.
pub fn chart_info(fs: &dyn Fs, path: &std::path::Path) -> Option<ChartInfo> {
    let bytes = fs.read(path).ok()?;
    let c = vibebms_core::load_chart_bytes(path, &bytes, &Default::default()).ok()?;
    Some(ChartInfo {
        title: c.meta.title,
        artist: c.meta.artist,
        sounds: c.sound_files,
    })
}

/// Where `chart` belongs among the folders below `library` (see [`place`]),
/// the files beside it having come with it.
pub fn locate(fs: &dyn Fs, chart: &Path, library: &Path) -> Option<Located> {
    locate_with(fs, chart, library, true, Rules::default()).map(|(l, _)| l)
}

/// [`locate`] for a chart that came on its own, into a folder of downloads
/// (the files beside it are not its).
pub fn locate_loose(fs: &dyn Fs, chart: &Path, library: &Path) -> Option<Located> {
    locate_with(fs, chart, library, false, Rules::default()).map(|(l, _)| l)
}

/// [`locate`] (`siblings`: the files beside the chart came with it) by
/// `rules`, with the candidates (for a person to look at).
pub fn locate_with(
    fs: &dyn Fs,
    chart: &Path,
    library: &Path,
    siblings: bool,
    rules: Rules,
) -> Option<(Located, Vec<(Candidate, String)>)> {
    let info = chart_info(fs, chart)?;
    let here = chart.parent()?.to_path_buf();
    let bundled: Vec<String> = if siblings {
        fs.read_dir(&here)
            .unwrap_or_default()
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| file_name(&e.path))
            .collect()
    } else {
        Vec::new()
    };
    if covers(&bundled, &info.sounds) {
        return Some((Located::Here, Vec::new()));
    }
    // the folders with any of its sounds; their charts read for the best
    let sounds: HashSet<String> = info.sounds.iter().map(|s| stem(s)).collect();
    let near: Vec<FolderInfo> = folders(fs, library)
        .into_iter()
        .filter(|(d, files)| *d != here && files.iter().any(|f| sounds.contains(&stem(f))))
        .map(|(dir, files)| FolderInfo {
            dir,
            files,
            charts: Vec::new(),
        })
        .collect();
    let rough = rank(&info, &bundled, &near);
    let keep: Vec<FolderInfo> = rough
        .iter()
        .enumerate()
        .filter(|(i, c)| c.coverage >= rules.min_coverage || *i < 3)
        .filter_map(|(_, c)| near.iter().find(|f| f.dir == c.dir))
        .map(|f| FolderInfo {
            charts: f
                .files
                .iter()
                .map(|n| f.dir.join(n))
                .filter(|p| is_chart(p))
                .filter_map(|p| chart_info(fs, &p))
                .collect(),
            ..f.clone()
        })
        .collect();
    let ranked = rank(&info, &bundled, &keep);
    let song = |d: &Path| {
        let s = keep
            .iter()
            .find(|f| f.dir == d)
            .and_then(|f| f.charts.first())
            .map(|c| format!("{} — {}", c.title, c.artist))
            .unwrap_or_default();
        format!("{s}  ({})", file_name(d))
    };
    let located = match decide(&ranked, rules) {
        Placement::Move(d) => Located::Move(d),
        Placement::Stay => Located::NoSong,
        Placement::Ask(dirs) => Located::Ask(
            dirs.into_iter()
                .map(|d| {
                    let label = song(&d);
                    (d, label)
                })
                .collect(),
        ),
    };
    let listed = ranked
        .into_iter()
        .map(|c| {
            let label = song(&c.dir);
            (c, label)
        })
        .collect();
    Some((located, listed))
}

/// Move `file` into the folder `dir` (a chart, or a file it came with), as
/// BeMusicSeeker's smart overwrite: a file of the same name there stays
/// unless `file` is newer (by more than 2 seconds), and `file` goes; a
/// chart of the same name never overwrites (it comes in as `name_.bms`;
/// the same chart again just goes). Returns where it is now (None: it was
/// not needed, or could not move).
pub fn bring(fs: &dyn Fs, file: &Path, dir: &Path) -> Option<PathBuf> {
    let name = file_name(file);
    let mut to = dir.join(&name);
    if is_chart(file) {
        let same = |a: &Path, b: &Path| fs.read(a).ok().is_some_and(|x| fs.read(b).ok() == Some(x));
        if fs.stamp(&to).is_some() && same(file, &to) {
            fs.remove(file);
            return Some(to);
        }
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (name.clone(), String::new()),
        };
        let mut n = 1;
        while fs.stamp(&to).is_some() {
            to = dir.join(format!("{stem}{}{ext}", "_".repeat(n)));
            n += 1;
        }
        return fs.rename(file, &to).then_some(to);
    }
    match (fs.stamp(file), fs.stamp(&to)) {
        (_, None) => fs.rename(file, &to).then_some(to),
        (Some((new, _)), Some((old, _))) if new > old + 2 => {
            (fs.remove(&to) && fs.rename(file, &to)).then_some(to)
        }
        _ => {
            fs.remove(file);
            None
        }
    }
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

    #[test]
    fn sounds_it_came_with_count() {
        // the 差分 brings its extra sound; the song folder has the rest
        let diff = chart(
            "Song B [EX]",
            "beta / obj: x",
            &["01.wav", "02.wav", "extra.wav"],
        );
        let own = folder("pkg", &["b_ex.bme", "extra.ogg"], vec![diff.clone()]);
        let b = folder(
            "B",
            &["01.ogg", "02.ogg", "b.bms"],
            vec![chart("Song B", "beta", &[])],
        );
        assert_eq!(
            place(&diff, &own, std::slice::from_ref(&b)),
            Placement::Move("B".into())
        );
        // without it, 2 of 3: not enough
        let bare = folder("pkg", &["b_ex.bme"], vec![diff.clone()]);
        assert_eq!(
            place(&diff, &bare, std::slice::from_ref(&b)),
            Placement::Stay
        );
        // a lower bar takes it
        let r = rank(&diff, &[], &[b]);
        assert_eq!(r[0].own, 2);
        assert!((r[0].coverage - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(
            decide(&r, Rules { min_coverage: 0.6 }),
            Placement::Move("B".into())
        );
    }

    #[test]
    fn smart_overwrite() {
        use crate::fs::Disk;
        let d = std::env::temp_dir().join(format!("bmseeker-bring-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (pkg, song) = (d.join("pkg"), d.join("song"));
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::create_dir_all(&song).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let put = |p: &Path, body: &str, older: bool| {
            std::fs::write(p, body).unwrap();
            if older {
                std::fs::File::options()
                    .write(true)
                    .open(p)
                    .unwrap()
                    .set_modified(old)
                    .unwrap();
            }
        };
        // newer: overwrites; older: the song's stays
        put(&song.join("a.ogg"), "old", true);
        put(&pkg.join("a.ogg"), "new", false);
        put(&song.join("b.ogg"), "keep", false);
        put(&pkg.join("b.ogg"), "stale", true);
        assert_eq!(
            bring(&Disk, &pkg.join("a.ogg"), &song),
            Some(song.join("a.ogg"))
        );
        assert_eq!(std::fs::read_to_string(song.join("a.ogg")).unwrap(), "new");
        assert_eq!(bring(&Disk, &pkg.join("b.ogg"), &song), None);
        assert_eq!(std::fs::read_to_string(song.join("b.ogg")).unwrap(), "keep");
        assert!(!pkg.join("b.ogg").exists());
        // a chart: never over another, the same one just goes
        put(&song.join("x.bms"), "#TITLE A", false);
        put(&pkg.join("x.bms"), "#TITLE B", false);
        assert_eq!(
            bring(&Disk, &pkg.join("x.bms"), &song),
            Some(song.join("x_.bms"))
        );
        put(&pkg.join("x.bms"), "#TITLE A", false);
        assert_eq!(
            bring(&Disk, &pkg.join("x.bms"), &song),
            Some(song.join("x.bms"))
        );
        assert!(!pkg.join("x.bms").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
