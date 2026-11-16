//! A difficulty table's song from its links into the library: the song's
//! archive (through [`hosts`](crate::hosts)) is unpacked there, its separate
//! chart (差分) goes into the song's folder ([`placement`](crate::placement)).
//! A link that leads to a page instead is [`Done::Page`], for a browser.

use crate::fs::Fs;
use std::path::{Path, PathBuf};

/// What became of one link.
#[derive(Debug)]
pub enum Done {
    /// Unpacked into this folder (files written).
    Added(PathBuf, usize),
    /// A chart put into its song's folder.
    Placed(PathBuf),
    /// A chart whose song is not in the library yet.
    Waiting(PathBuf),
    /// A page to open in a browser.
    Page(String),
    Failed(String),
}

/// A link to a chart file of its own (`….bms` and the like).
pub fn chart_link(u: &str) -> bool {
    let path = u.split(['?', '#']).next().unwrap_or("");
    crate::placement::is_chart(Path::new(path))
}

/// The chart's file name in its link.
pub fn chart_name(u: &str) -> String {
    let path = u.split(['?', '#']).next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or("chart.bms");
    safe_name(&percent_decode(name))
}

/// `name` as one file name (no path, nothing a file system refuses).
pub fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "download".into()
    } else {
        s
    }
}

/// `%28` and the like in a URL's file name.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The charts in a separate chart's unpacked archive `dir` into their
/// songs' folders, each with the files beside it there (extra sounds, a
/// BGA; one the song has already stays unless this one is newer).
pub fn place_charts(fs: &dyn Fs, dir: &Path, library: &Path) -> Vec<Done> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let entries = fs.read_dir(&d).unwrap_or_default();
        todo.extend(entries.iter().filter(|e| e.is_dir).map(|e| e.path.clone()));
        let files: Vec<&PathBuf> = entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| &e.path)
            .collect();
        let mut song_dir = None;
        for chart in files.iter().filter(|p| crate::placement::is_chart(p)) {
            match crate::placement::locate(fs, chart, library) {
                Some(crate::placement::Located::Move(to)) => {
                    if let Some(at) = crate::placement::bring(fs, chart, &to) {
                        out.push(Done::Placed(at));
                        song_dir = Some(to);
                    }
                }
                _ => out.push(Done::Waiting((*chart).clone())),
            }
        }
        // the files it came with (smart overwrite)
        if let Some(to) = song_dir {
            for f in files.iter().filter(|p| !crate::placement::is_chart(p)) {
                crate::placement::bring(fs, f, &to);
            }
        }
    }
    out
}

/// One of a song's links (`diff`: its separate chart's) on a disk: into
/// `downloads`, then into `library` (an archive unpacked, a chart into its
/// song's folder; a separate chart's archive into its song's folder too).
#[cfg(not(target_arch = "wasm32"))]
pub fn link(
    downloads: &Path,
    library: &Path,
    want: &crate::hosts::Want,
    link: &str,
    diff: bool,
    cx: &crate::hosts::Context,
) -> Vec<Done> {
    use crate::fs::Disk;
    use crate::import::{self, Imported};
    let title = want.title.as_str();
    if let Err(e) = std::fs::create_dir_all(downloads) {
        return vec![Done::Failed(e.to_string())];
    }
    let file = if chart_link(link) {
        crate::progress::stage(crate::progress::Stage::Finding);
        let file = downloads.join(chart_name(link));
        if let Err(e) = crate::hosts::download_to(link, &file) {
            return vec![Done::Failed(e)];
        }
        // a page where a chart was expected
        if std::fs::read(&file).is_ok_and(|b| b.trim_ascii_start().starts_with(b"<")) {
            let _ = std::fs::remove_file(&file);
            return vec![Done::Page(link.to_string())];
        }
        file
    } else {
        let tmp = downloads.join(format!(".song-{}", safe_name(title)));
        // the song's own link may be a pack of many songs: its archive
        let want = (!diff).then(|| want.clone());
        match crate::hosts::fetch(link, want, &tmp, cx) {
            Ok(()) => {
                let ext = crate::hosts::sniff(&tmp).unwrap_or("zip");
                let file = downloads.join(format!("{}.{ext}", safe_name(title)));
                if std::fs::rename(&tmp, &file).is_err() {
                    return vec![Done::Failed("could not keep the download".into())];
                }
                file
            }
            Err(crate::hosts::Fetch::Page) => return vec![Done::Page(link.to_string())],
            Err(crate::hosts::Fetch::Failed(e)) => {
                let _ = std::fs::remove_file(&tmp);
                return vec![Done::Failed(e)];
            }
        }
    };
    crate::progress::stage(crate::progress::Stage::Unpacking);
    if diff && !chart_link(link) {
        let dir = downloads.join(format!(".diff-{}", safe_name(title)));
        let name = import::file_name(&file);
        let r = import::unpack_into(&file, &name, &dir);
        let _ = std::fs::remove_file(&file);
        let out = match r {
            Ok(_) => place_charts(&Disk, &dir, library),
            Err(e) => vec![Done::Failed(e)],
        };
        let _ = std::fs::remove_dir_all(&dir);
        return out;
    }
    let mut out = vec![match import::import(&Disk, &file, library) {
        Ok(Imported::Unpacked(dir, n)) => Done::Added(dir, n),
        Ok(Imported::Placed(p)) => Done::Placed(p),
        Ok(Imported::Waiting(p)) | Ok(Imported::Ask(p, _)) => Done::Waiting(p),
        Ok(Imported::Other(p)) => Done::Failed(format!("{} is not a song", p.display())),
        Err(e) => Done::Failed(e),
    }];
    // charts that waited for this song
    if matches!(out[0], Done::Added(..)) {
        out.extend(
            import::place_waiting(&Disk, downloads, library)
                .into_iter()
                .map(Done::Placed),
        );
    }
    out
}

/// Whether the folder `dir` (a song just added) holds the chart `md5`
/// already: its separate chart's links need no trying then (a table may
/// give the song's page as one).
pub fn holds_chart(fs: &dyn Fs, dir: &Path, md5: &str) -> bool {
    let md5 = md5.trim().to_ascii_lowercase();
    if md5.len() != 32 {
        return false;
    }
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in fs.read_dir(&d).unwrap_or_default() {
            if e.is_dir {
                todo.push(e.path);
            } else if crate::placement::is_chart(&e.path)
                && fs
                    .read(&e.path)
                    .is_ok_and(|b| vibebms_core::bms::md5_hex(&b) == md5)
            {
                return true;
            }
        }
    }
    false
}

/// The folders a song's outcome added.
pub fn added(done: &[Done]) -> impl Iterator<Item = &Path> {
    done.iter().filter_map(|d| match d {
        Done::Added(dir, _) => Some(dir.as_path()),
        _ => None,
    })
}

/// Whether a link's outcome brought the song (or its chart) in.
pub fn got(done: &[Done]) -> bool {
    done.iter()
        .any(|d| matches!(d, Done::Added(..) | Done::Placed(_) | Done::Waiting(_)))
}

/// What trying one link after another came to: the first that brought it
/// in; else the first page among them (to open in a browser); else the last
/// failure.
#[derive(Default)]
pub struct Tries {
    page: Option<String>,
    failed: Option<String>,
}

impl Tries {
    /// A link's outcome: Some(it) once one brought the file in.
    pub fn record(&mut self, done: Vec<Done>) -> Option<Vec<Done>> {
        if got(&done) {
            return Some(done);
        }
        for d in done {
            match d {
                Done::Page(u) => {
                    self.page.get_or_insert(u);
                }
                Done::Failed(e) => self.failed = Some(e),
                _ => {}
            }
        }
        None
    }

    /// When no link brought it in.
    pub fn end(self) -> Vec<Done> {
        vec![match (self.page, self.failed) {
            (Some(p), _) => Done::Page(p),
            (None, Some(e)) => Done::Failed(e),
            (None, None) => Done::Failed("no link".into()),
        }]
    }
}

/// [`link`] for each of `links` (the same file's, the table's own first:
/// [`Links::song_links`](crate::links::Links::song_links)) until one brings
/// it in; then, for a song with the chart `md5`, the links BMS SEARCH knows
/// for it ([`bmssearch`](crate::bmssearch)).
#[cfg(not(target_arch = "wasm32"))]
pub fn links(
    downloads: &Path,
    library: &Path,
    want: &crate::hosts::Want,
    links: &[String],
    diff: bool,
    md5: Option<&str>,
    cx: &crate::hosts::Context,
) -> Vec<Done> {
    let cancelled = || crate::progress::current().is_some_and(|p| p.cancelled());
    let mut tries = Tries::default();
    for l in links {
        if cancelled() {
            return vec![Done::Failed(crate::progress::CANCELLED.into())];
        }
        if let Some(done) = tries.record(link(downloads, library, want, l, diff, cx)) {
            return done;
        }
    }
    if let Some(md5) = md5.filter(|_| !diff && !cancelled()) {
        for l in crate::bmssearch::lookup(md5, want, &cx.config) {
            if cancelled() {
                return vec![Done::Failed(crate::progress::CANCELLED.into())];
            }
            if links.contains(&l) {
                continue;
            }
            if let Some(done) = tries.record(link(downloads, library, want, &l, diff, cx)) {
                return done;
            }
        }
    }
    tries.end()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BMSEEKER_SONG=URL BMSEEKER_DIFF=URL [BMSEEKER_TITLE=… BMSEEKER_ARTIST=…]
    /// cargo test -p bmseeker song_links -- --ignored`
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "network"]
    fn song_links() {
        let root = std::env::temp_dir().join("bmseeker-install");
        let _ = std::fs::remove_dir_all(&root);
        let want = crate::hosts::Want {
            title: std::env::var("BMSEEKER_TITLE").unwrap_or("Test Song".into()),
            artist: std::env::var("BMSEEKER_ARTIST").unwrap_or_default(),
        };
        for var in ["BMSEEKER_SONG", "BMSEEKER_DIFF"] {
            if let Ok(l) = std::env::var(var) {
                let done = link(
                    &root.join("downloads"),
                    &root.join("bms"),
                    &want,
                    &l,
                    var == "BMSEEKER_DIFF",
                    &crate::hosts::Context::default(),
                );
                println!("{var}: {done:?}");
            }
        }
    }

    #[test]
    fn chart_links() {
        assert!(chart_link("http://x.org/up/song_7A.bme"));
        assert!(chart_link("https://x.org/a/b.bms?dl=1"));
        assert!(!chart_link("https://x.org/a/song.zip"));
        assert!(!chart_link("https://drive.google.com/file/d/X/view"));
        assert_eq!(
            chart_name("https://x.org/a/b%20c%28M%29.bms?dl=1"),
            "b c(M).bms"
        );
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(safe_name("a/b: c?"), "a_b_ c_");
    }

    #[test]
    fn tries_in_order() {
        let mut t = Tries::default();
        assert!(t.record(vec![Done::Failed("404".into())]).is_none());
        assert!(t
            .record(vec![Done::Page("https://1drv.ms/x".into())])
            .is_none());
        assert!(t
            .record(vec![Done::Page("https://later/".into())])
            .is_none());
        assert!(matches!(&t.end()[..], [Done::Page(p)] if p == "https://1drv.ms/x"));
        let mut t = Tries::default();
        assert!(t.record(vec![Done::Failed("404".into())]).is_none());
        assert!(t.record(vec![Done::Added("a".into(), 3)]).is_some());
    }
}
