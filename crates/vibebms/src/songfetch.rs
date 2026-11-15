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
    pub fn start(&mut self, root: &Path, title: &str, url: &str, url_diff: &str) -> bool {
        let links: Vec<String> = [url, url_diff]
            .into_iter()
            .filter(|u| is_link(u))
            .map(str::to_string)
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
        let (root, title) = (root.to_path_buf(), title.to_string());
        #[cfg(not(target_arch = "wasm32"))]
        crate::platform::spawn("song", move || {
            let done: Vec<Done> = links
                .iter()
                .flat_map(|l| native(&root, &title, l))
                .collect();
            let _ = tx.send((title, done));
        });
        #[cfg(target_arch = "wasm32")]
        web(root, title.clone(), links, Vec::new(), move |done| {
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

/// The app: a link into `downloads`, then `import` (unpack an archive, or
/// put a chart into its song's folder).
#[cfg(not(target_arch = "wasm32"))]
fn native(root: &Path, title: &str, link: &str) -> Vec<Done> {
    use crate::import::{self, Imported};
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
        match crate::hosts::fetch(link, &tmp) {
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
    title: String,
    mut links: Vec<String>,
    mut done: Vec<Done>,
    finish: impl FnOnce(Vec<Done>) + Send + 'static,
) {
    use crate::archive::{via_proxy, web_step, Fail};
    use crate::platform::vfs;
    if links.is_empty() {
        return finish(done);
    }
    let link = links.remove(0);
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
            web(root, title, links, done, finish)
        });
        return;
    }
    let (r, step) = crate::hosts::Resolver::new(&link);
    web_step(r, step, link, move |r| {
        done.push(match r {
            Ok(bytes) => {
                let ext = crate::hosts::sniff_bytes(&bytes).unwrap_or("zip");
                let stem = crate::archive::safe_name(&title);
                match vfs::add_zip(&root, &format!("{stem}.{ext}"), bytes) {
                    0 => Done::Failed("the archive holds nothing".into()),
                    n => Done::Added(root.join("bms").join(&stem), n),
                }
            }
            Err(Fail::Page(url)) => Done::Page(url),
            Err(Fail::Msg(e)) => Done::Failed(e),
        });
        web(root, title, links, done, finish)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `VIBEBMS_SONG=URL VIBEBMS_DIFF=URL cargo test -p vibebms song_links -- --ignored`
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "network"]
    fn song_links() {
        let root = std::env::temp_dir().join("vibebms-songfetch");
        let _ = std::fs::remove_dir_all(&root);
        for var in ["VIBEBMS_SONG", "VIBEBMS_DIFF"] {
            if let Ok(link) = std::env::var(var) {
                println!("{var}: {:?}", native(&root, "Test Song", &link));
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
