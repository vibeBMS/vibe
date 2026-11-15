//! A difficulty table's song that is not in the library, downloaded in the
//! app (`bmseeker::install`) in the background: natively on a thread; on the
//! web through the vibeBMS proxy, unpacked / placed in memory. A link that
//! leads to a page instead opens it in the browser.

use std::path::Path;
#[cfg(target_arch = "wasm32")]
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

pub use bmseeker::install::Done;
#[cfg(target_arch = "wasm32")]
use bmseeker::install::{chart_link, chart_name, place_charts};

#[derive(Default)]
pub struct SongFetch {
    tx: Option<Sender<(String, Vec<Done>)>>,
    rx: Option<Receiver<(String, Vec<Done>)>>,
    busy: Vec<String>,
}

fn is_link(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://")
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
                .flat_map(|(l, diff)| {
                    bmseeker::install::link(
                        &root.join("downloads"),
                        &root.join("bms"),
                        &want,
                        l,
                        *diff,
                    )
                })
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
                done.extend(place_charts(
                    &crate::placement::Vfs,
                    &tmp.join(&stem),
                    &root.join("bms"),
                ));
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
