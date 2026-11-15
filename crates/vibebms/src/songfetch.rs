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

    /// Download `title` from `links`: the song's, then its separate
    /// chart's (each its own first, then the others known for the chart, tried
    /// in turn until one downloads). False when there is no link.
    pub fn start(
        &mut self,
        root: &Path,
        title: &str,
        artist: &str,
        links: Vec<(Vec<String>, bool)>,
    ) -> bool {
        let links: Vec<(Vec<String>, bool)> = links
            .into_iter()
            .map(|(ls, d)| (ls.into_iter().filter(|u| is_link(u)).collect::<Vec<_>>(), d))
            .filter(|(ls, _)| !ls.is_empty())
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
                .flat_map(|(ls, diff)| {
                    bmseeker::install::links(
                        &root.join("downloads"),
                        &root.join("bms"),
                        &want,
                        ls,
                        *diff,
                    )
                })
                .collect();
            let _ = tx.send((title, done));
        });
        #[cfg(target_arch = "wasm32")]
        {
            let groups = links
                .into_iter()
                .map(|(ls, diff)| (ls, diff, Default::default()))
                .collect();
            web(root, want, groups, Vec::new(), move |done| {
                let _ = tx.send((title, done));
            });
        }
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

/// The web build's links: (the same file's links to try in turn, they are
/// the separate chart's, what the tried ones came to).
#[cfg(target_arch = "wasm32")]
type Group = (Vec<String>, bool, bmseeker::install::Tries);

/// The web build: the links one after another through the vibeBMS proxy,
/// unpacked / placed in memory; a file's next link tried when one does not
/// download.
#[cfg(target_arch = "wasm32")]
fn web(
    root: PathBuf,
    want: crate::hosts::Want,
    mut groups: Vec<Group>,
    done: Vec<Done>,
    finish: impl FnOnce(Vec<Done>) + Send + 'static,
) {
    use crate::archive::{web_step, Fail};
    use crate::hosts::via_proxy;
    use crate::platform::vfs;
    if groups.is_empty() {
        return finish(done);
    }
    let title = want.title.clone();
    let (mut links, diff, tries) = groups.remove(0);
    let link = links.remove(0);
    // this link's outcome: the file in, or the next link, or the end
    let next = move |got: Vec<Done>,
                     mut tries: bmseeker::install::Tries,
                     mut groups: Vec<Group>,
                     mut done: Vec<Done>| {
        match tries.record(got) {
            Some(got) => done.extend(got),
            None if !links.is_empty() => groups.insert(0, (links, diff, tries)),
            None => done.extend(tries.end()),
        }
        (groups, done)
    };
    if chart_link(&link) {
        let name = chart_name(&link);
        crate::platform::http_get(&via_proxy(&link), move |r| {
            let got = match r {
                Ok(b) if !b.trim_ascii_start().starts_with(b"<") => {
                    let at = root.join("downloads").join(&name);
                    vfs::put_bytes(&at, b);
                    match crate::placement::locate_loose(&at, &root.join("bms")) {
                        Some(crate::placement::Located::Move(dir)) => {
                            match crate::placement::bring(&at, &dir) {
                                Some(to) => Done::Placed(to),
                                None => Done::Waiting(at),
                            }
                        }
                        _ => Done::Waiting(at),
                    }
                }
                _ => Done::Page(link),
            };
            let (groups, done) = next(vec![got], tries, groups, done);
            web(root, want, groups, done, finish)
        });
        return;
    }
    let (r, step) = crate::hosts::Resolver::find(&link, (!diff).then(|| want.clone()));
    web_step(r, step, link, move |r| {
        let got = match r {
            // a separate chart's archive: its charts into their songs' folders
            Ok(bytes) if diff => {
                let ext = crate::hosts::sniff_bytes(&bytes).unwrap_or("zip");
                let stem = crate::archive::safe_name(&title);
                let tmp = root.join("downloads").join(".diff");
                vfs::add_archive_at(&tmp, &format!("{stem}.{ext}"), bytes);
                place_charts(&crate::placement::Vfs, &tmp.join(&stem), &root.join("bms"))
            }
            Ok(bytes) => {
                let ext = crate::hosts::sniff_bytes(&bytes).unwrap_or("zip");
                let stem = crate::archive::safe_name(&title);
                vec![match vfs::add_zip(&root, &format!("{stem}.{ext}"), bytes) {
                    0 => Done::Failed("the archive holds nothing".into()),
                    n => Done::Added(root.join("bms").join(&stem), n),
                }]
            }
            Err(Fail::Page(url)) => vec![Done::Page(url)],
            Err(Fail::Msg(e)) => vec![Done::Failed(e)],
        };
        let (groups, done) = next(got, tries, groups, done);
        web(root, want, groups, done, finish)
    });
}
