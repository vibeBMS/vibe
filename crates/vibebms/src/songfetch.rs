//! A difficulty table's song that is not in the library, downloaded in the
//! app: its link (and its separate chart's, 差分) through the same hosts as
//! the event mirror (`hosts`). The song's archive is unpacked into the
//! library; the chart goes into its song's folder (`placement`). A link that
//! leads to a page instead opens it in the browser.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

/// What became of one link.
#[derive(Debug)]
pub enum Done {
    /// Unpacked into this folder (files written).
    Added(PathBuf, usize),
    /// A chart put into its song's folder.
    Placed(PathBuf),
    /// A chart whose song is not in the library yet.
    Waiting(PathBuf),
    /// A page to open in the browser.
    Page(String),
    Failed(String),
}

#[derive(Default)]
pub struct SongFetch {
    tx: Option<Sender<(String, Vec<Done>)>>,
    rx: Option<Receiver<(String, Vec<Done>)>>,
    busy: Vec<String>,
}

fn is_link(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://")
}

/// A link to a chart file of its own (`….bms` and the like).
fn chart_link(u: &str) -> bool {
    let path = u.split(['?', '#']).next().unwrap_or("");
    crate::placement::is_chart(Path::new(path))
}

/// The chart's file name in its link.
fn chart_name(u: &str) -> String {
    let path = u.split(['?', '#']).next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or("chart.bms");
    crate::archive::safe_name(&percent_decode(name))
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

impl SongFetch {
    pub fn is_busy(&self, title: &str) -> bool {
        self.busy.iter().any(|t| t == title)
    }

    /// Download `title` from `url` (the song) and `url_diff` (its separate
    /// chart). False when neither is a link.
    pub fn start(
        &mut self,
        root: &Path,
        title: &str,
        artist: &str,
        url: &str,
        url_diff: &str,
    ) -> bool {
        // (link, it is the separate chart's)
        let links: Vec<(String, bool)> = [(url, false), (url_diff, true)]
            .into_iter()
            .map(|(u, d)| (u.trim(), d))
            .filter(|(u, _)| is_link(u))
            .map(|(u, d)| (u.to_string(), d))
            .collect();
        if links.is_empty() || self.is_busy(title) {
            return false;
        }
        if self.tx.is_none() {
            let (tx, rx) = channel();
            self.tx = Some(tx);
            self.rx = Some(rx);
        }
        let tx = self.tx.clone().expect("channel");
        self.busy.push(title.to_string());
        let want = crate::hosts::Want {
            title: title.to_string(),
            artist: artist.to_string(),
        };
        let (root, title) = (root.to_path_buf(), title.to_string());
        #[cfg(not(target_arch = "wasm32"))]
        crate::platform::spawn("song", move || {
            let done: Vec<Done> = links
                .iter()
                .flat_map(|(l, diff)| native(&root, &want, l, *diff))
                .collect();
            let _ = tx.send((title, done));
        });
        #[cfg(target_arch = "wasm32")]
        web(root, want, links, Vec::new(), move |done| {
            let _ = tx.send((title, done));
        });
        true
    }

    /// The songs that finished since the last call, with what became of
    /// their links.
    pub fn poll(&mut self) -> Vec<(String, Vec<Done>)> {
        let done: Vec<(String, Vec<Done>)> = self
            .rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for (t, _) in &done {
            self.busy.retain(|b| b != t);
        }
        done
    }
}

/// The charts in a separate chart's unpacked archive `dir` into their
/// songs' folders, each with the files beside it there (extra sounds, a
/// BGA) unless the song has them already.
fn place_charts(dir: &Path, library: &Path) -> Vec<Done> {
    use crate::platform::vfs;
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let entries = vfs::read_dir(&d).unwrap_or_default();
        todo.extend(entries.iter().filter(|e| e.is_dir).map(|e| e.path.clone()));
        let files: Vec<&PathBuf> = entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| &e.path)
            .collect();
        let mut song_dir = None;
        for chart in files.iter().filter(|p| crate::placement::is_chart(p)) {
            match crate::placement::locate(chart, library) {
                Some(crate::placement::Located::Move(to)) => {
                    let at = to.join(chart.file_name().unwrap_or_default());
                    if vfs::rename(chart, &at) {
                        out.push(Done::Placed(at));
                        song_dir = Some(to);
                    }
                }
                _ => out.push(Done::Waiting((*chart).clone())),
            }
        }
        if let Some(to) = song_dir {
            for f in files.iter().filter(|p| !crate::placement::is_chart(p)) {
                let _ = vfs::rename(f, &to.join(f.file_name().unwrap_or_default()));
            }
        }
    }
    out
}

/// The app: a link into `downloads`, then `import` (unpack an archive, or
/// put a chart into its song's folder; a separate chart's archive goes to
/// its song's folder too).
#[cfg(not(target_arch = "wasm32"))]
fn native(root: &Path, want: &crate::hosts::Want, link: &str, diff: bool) -> Vec<Done> {
    use crate::import::{self, Imported};
    let title = want.title.as_str();
    let downloads = root.join("downloads");
    let library = root.join("bms");
    if let Err(e) = std::fs::create_dir_all(&downloads) {
        return vec![Done::Failed(e.to_string())];
    }
    let file = if chart_link(link) {
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
        let tmp = downloads.join(format!(".song-{}", crate::archive::safe_name(title)));
        // the song's own link may be a pack of many songs: its archive
        let want = (!diff).then(|| want.clone());
        match crate::hosts::fetch_for(link, want, &tmp) {
            Ok(()) => {
                let ext = crate::hosts::sniff(&tmp).unwrap_or("zip");
                let file = downloads.join(format!("{}.{ext}", crate::archive::safe_name(title)));
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
    if diff && !chart_link(link) {
        let dir = downloads.join(format!(".diff-{}", crate::archive::safe_name(title)));
        let name = crate::import::file_name(&file);
        let r = import::unpack_into(&file, &name, &dir);
        let _ = std::fs::remove_file(&file);
        let out = match r {
            Ok(_) => place_charts(&dir, &library),
            Err(e) => vec![Done::Failed(e)],
        };
        let _ = std::fs::remove_dir_all(&dir);
        return out;
    }
    let mut out = vec![match import::import(&file, &library) {
        Ok(Imported::Unpacked(dir, n)) => Done::Added(dir, n),
        Ok(Imported::Placed(p)) => Done::Placed(p),
        Ok(Imported::Waiting(p)) | Ok(Imported::Ask(p, _)) => Done::Waiting(p),
        Ok(Imported::Other(p)) => Done::Failed(format!("{} is not a song", p.display())),
        Err(e) => Done::Failed(e),
    }];
    // charts that waited for this song
    if matches!(out[0], Done::Added(..)) {
        out.extend(
            import::place_waiting(&downloads, &library)
                .into_iter()
                .map(Done::Placed),
        );
    }
    out
}

/// The web build: the links one after another through the vibeBMS proxy,
/// unpacked / placed in memory.
#[cfg(target_arch = "wasm32")]
fn web(
    root: PathBuf,
    want: crate::hosts::Want,
    mut links: Vec<(String, bool)>,
    mut done: Vec<Done>,
    finish: impl FnOnce(Vec<Done>) + Send + 'static,
) {
    use crate::archive::{web_step, Fail};
    use crate::hosts::via_proxy;
    use crate::platform::vfs;
    if links.is_empty() {
        return finish(done);
    }
    let title = want.title.clone();
    let (link, diff) = links.remove(0);
    if chart_link(&link) {
        let name = chart_name(&link);
        crate::platform::http_get(&via_proxy(&link), move |r| {
            done.push(match r {
                Ok(b) if !b.trim_ascii_start().starts_with(b"<") => {
                    let at = root.join("downloads").join(&name);
                    vfs::put_bytes(&at, b);
                    match crate::placement::locate(&at, &root.join("bms")) {
                        Some(crate::placement::Located::Move(dir)) => {
                            let to = dir.join(&name);
                            if vfs::rename(&at, &to) {
                                Done::Placed(to)
                            } else {
                                Done::Waiting(at)
                            }
                        }
                        _ => Done::Waiting(at),
                    }
                }
                _ => Done::Page(link),
            });
            web(root, want, links, done, finish)
        });
        return;
    }
    let (r, step) = crate::hosts::Resolver::find(&link, (!diff).then(|| want.clone()));
    web_step(r, step, link, move |r| {
        match r {
            // a separate chart's archive: its charts into their songs' folders
            Ok(bytes) if diff => {
                let ext = crate::hosts::sniff_bytes(&bytes).unwrap_or("zip");
                let stem = crate::archive::safe_name(&title);
                let tmp = root.join("downloads").join(".diff");
                vfs::add_archive_at(&tmp, &format!("{stem}.{ext}"), bytes);
                done.extend(place_charts(&tmp.join(&stem), &root.join("bms")));
            }
            Ok(bytes) => {
                let ext = crate::hosts::sniff_bytes(&bytes).unwrap_or("zip");
                let stem = crate::archive::safe_name(&title);
                done.push(match vfs::add_zip(&root, &format!("{stem}.{ext}"), bytes) {
                    0 => Done::Failed("the archive holds nothing".into()),
                    n => Done::Added(root.join("bms").join(&stem), n),
                });
            }
            Err(Fail::Page(url)) => done.push(Done::Page(url)),
            Err(Fail::Msg(e)) => done.push(Done::Failed(e)),
        }
        web(root, want, links, done, finish)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `VIBEBMS_SONG=URL VIBEBMS_DIFF=URL [VIBEBMS_TITLE=… VIBEBMS_ARTIST=…] cargo test -p vibebms song_links -- --ignored`
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "network"]
    fn song_links() {
        let root = std::env::temp_dir().join("vibebms-songfetch");
        let _ = std::fs::remove_dir_all(&root);
        for var in ["VIBEBMS_SONG", "VIBEBMS_DIFF"] {
            if let Ok(link) = std::env::var(var) {
                println!(
                    "{var}: {:?}",
                    native(
                        &root,
                        &crate::hosts::Want {
                            title: std::env::var("VIBEBMS_TITLE").unwrap_or("Test Song".into()),
                            artist: std::env::var("VIBEBMS_ARTIST").unwrap_or_default(),
                        },
                        &link,
                        var == "VIBEBMS_DIFF"
                    )
                );
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
    }
}
