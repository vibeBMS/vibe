//! A difficulty table's song that is not in the library, downloaded in the
//! app (`bmseeker::install`) in the background: natively on a thread; on the
//! web through the vibeBMS proxy, unpacked / placed in memory. A link that
//! leads to a page instead opens it in the browser.

use bmseeker::progress::{Progress, Stage};
use std::path::Path;
#[cfg(target_arch = "wasm32")]
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

pub use bmseeker::install::Done;
#[cfg(target_arch = "wasm32")]
use bmseeker::install::{chart_link, chart_name, place_charts};

#[derive(Default)]
pub struct SongFetch {
    tx: Option<Sender<(String, Vec<Done>)>>,
    rx: Option<Receiver<(String, Vec<Done>)>>,
    /// The songs downloading, and how far each has got.
    busy: Vec<(String, Arc<Progress>)>,
    /// Songs in the library, until the library has been read again.
    adding: Vec<(String, Arc<Progress>)>,
}

/// Whether a song just added (`done`) holds the chart `md5`.
fn has_chart(done: &[Done], md5: Option<&str>) -> bool {
    md5.is_some_and(|m| {
        bmseeker::install::added(done)
            .any(|dir| bmseeker::install::holds_chart(&crate::placement::Vfs, dir, m))
    })
}

/// What a chart's download came to when its song is in the library already
/// and none of the chart's links brought it (nor led to a page).
pub const NO_CHART_LINK: &str = "the song is in the library; no link for this chart works";

fn no_chart_link(done: Vec<Done>, have_song: bool) -> Vec<Done> {
    let page = done.iter().any(|d| matches!(d, Done::Page(_)));
    let cancelled = done
        .iter()
        .any(|d| matches!(d, Done::Failed(e) if e == bmseeker::progress::CANCELLED));
    if have_song && !page && !cancelled && !bmseeker::install::got(&done) {
        vec![Done::Failed(NO_CHART_LINK.into())]
    } else {
        done
    }
}

fn is_link(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://")
}

impl SongFetch {
    /// Downloading, or in the library but not read yet.
    pub fn is_busy(&self, title: &str) -> bool {
        self.progress(title).is_some()
    }

    /// How far `title`'s download has got (while [`Self::is_busy`]).
    pub fn progress(&self, title: &str) -> Option<&Arc<Progress>> {
        self.busy
            .iter()
            .chain(&self.adding)
            .find(|(t, _)| t == title)
            .map(|(_, p)| p)
    }

    /// Stop `title`'s download (true if it was downloading). Its outcome
    /// still comes (a failure, cancelled), when it has stopped.
    pub fn cancel(&mut self, title: &str) -> bool {
        let Some(i) = self.busy.iter().position(|(t, _)| t == title) else {
            return false;
        };
        let (_, p) = self.busy.remove(i);
        p.cancel();
        true
    }

    /// The library has been read again: the songs added are in it.
    pub fn scanned(&mut self) {
        self.adding.clear();
    }

    /// Download `title` from `links`: the song's, then its separate
    /// chart's (each its own first, then the others known for the chart, tried
    /// in turn until one downloads; for the song, then the links BMS SEARCH
    /// knows for the chart `md5`). With the song in the library already
    /// (`have_song`), only the chart: when none of its links works, it ends
    /// in [`NO_CHART_LINK`], and the song is not downloaded again. False when
    /// there is nothing to try.
    pub fn start(
        &mut self,
        root: &Path,
        title: &str,
        artist: &str,
        md5: &str,
        links: Vec<(Vec<String>, bool)>,
        have_song: bool,
    ) -> bool {
        let md5 = (md5.len() == 32).then(|| md5.to_string());
        let links: Vec<(Vec<String>, bool)> = links
            .into_iter()
            .filter(|(_, d)| *d || !have_song)
            .map(|(ls, d)| (ls.into_iter().filter(|u| is_link(u)).collect::<Vec<_>>(), d))
            // none of its own: BMS SEARCH's (the chart's only with the song in)
            .filter(|(ls, d)| !ls.is_empty() || ((!d || have_song) && md5.is_some()))
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
        let progress = Progress::new();
        self.busy.push((title.to_string(), progress.clone()));
        let want = crate::hosts::Want {
            title: title.to_string(),
            artist: artist.to_string(),
        };
        let (root, title) = (root.to_path_buf(), title.to_string());
        #[cfg(not(target_arch = "wasm32"))]
        crate::platform::spawn("song", move || {
            progress.set_stage(Stage::Finding);
            let done: Vec<Done> = bmseeker::progress::scope(Some(progress), || {
                let mut done = Vec::new();
                for (ls, diff) in &links {
                    // the song brought the chart: no separate chart to get
                    if *diff && has_chart(&done, md5.as_deref()) {
                        break;
                    }
                    // the lists' links are in `ls` already: BMS SEARCH after
                    let cx = crate::hosts::context();
                    let q = bmseeker::sources::Query {
                        md5: md5.clone().unwrap_or_default(),
                        want: want.clone(),
                        diff: *diff,
                    };
                    let sources = bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
                    done.extend(bmseeker::install::links(
                        &root.join("downloads"),
                        &root.join("bms"),
                        &q,
                        ls,
                        &sources,
                        cx,
                    ));
                }
                no_chart_link(done, have_song)
            });
            let _ = tx.send((title, done));
        });
        #[cfg(target_arch = "wasm32")]
        {
            let groups = links
                .into_iter()
                .map(|(links, diff)| Group {
                    links,
                    diff,
                    tries: Default::default(),
                    tried: Vec::new(),
                    md5: md5.clone().filter(|_| !diff || have_song),
                    chart: md5.clone().filter(|_| diff),
                    checked: Vec::new(),
                })
                .collect();
            web(root, want, progress, groups, Vec::new(), move |done| {
                let _ = tx.send((title, no_chart_link(done, have_song)));
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
        for (t, d) in &done {
            if let Some(i) = self.busy.iter().position(|(b, _)| b == t) {
                let (title, p) = self.busy.remove(i);
                // in the library: until it is read again
                if bmseeker::install::got(d) {
                    p.set_stage(Stage::Adding);
                    self.adding.push((title, p));
                }
            }
        }
        done
    }
}

/// The web build's links for one file: those to try in turn, it is the
/// separate chart, what the tried ones came to (and which they were), and
/// the chart's MD5 to ask BMS SEARCH with once they are tried.
#[cfg(target_arch = "wasm32")]
struct Group {
    links: Vec<String>,
    diff: bool,
    tries: bmseeker::install::Tries,
    tried: Vec<String>,
    md5: Option<String>,
    /// A separate chart's: its MD5 (not needed when the song holds it).
    chart: Option<String>,
    /// Links a source found by name: the chart they bring is checked.
    checked: Vec<String>,
}

/// The web build: the links one after another through the vibeBMS proxy,
/// unpacked / placed in memory; a file's next link tried when one does not
/// download.
#[cfg(target_arch = "wasm32")]
fn web(
    root: PathBuf,
    want: crate::hosts::Want,
    progress: Arc<Progress>,
    mut groups: Vec<Group>,
    mut done: Vec<Done>,
    finish: impl FnOnce(Vec<Done>) + Send + 'static,
) {
    use crate::archive::{web_fetch, Fail};
    use crate::hosts::via_proxy;
    use crate::platform::vfs;
    if groups.is_empty() {
        return finish(done);
    }
    let title = want.title.clone();
    if progress.cancelled() {
        done.push(Done::Failed(bmseeker::progress::CANCELLED.into()));
        return finish(done);
    }
    let mut g = groups.remove(0);
    // the song brought the chart: no separate chart to get
    if g.diff && g.tried.is_empty() && has_chart(&done, g.chart.as_deref()) {
        return web(root, want, progress, groups, done, finish);
    }
    // its links tried: BMS SEARCH's next, else the end
    if g.links.is_empty() {
        match g.md5.take() {
            Some(md5) => {
                // the lists' links are in the group already: BMS SEARCH's
                let cx = crate::hosts::context();
                let q = bmseeker::sources::Query {
                    md5,
                    want: want.clone(),
                    diff: g.diff,
                };
                let sources = bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
                progress.set_stage(Stage::Finding);
                // BMS SEARCH's API sends CORS headers: no proxy
                crate::platform::run_task(sources.find_all(&q), false, move |found| {
                    for (source, l) in found {
                        if g.tried.contains(&l) || g.links.contains(&l) {
                            continue;
                        }
                        // found by name: a chart is checked here; an
                        // archive would be looked into, which this build
                        // does not
                        if sources.checked(source) {
                            if !chart_link(&l) {
                                continue;
                            }
                            g.checked.push(l.clone());
                        }
                        g.links.push(l);
                    }
                    groups.insert(0, g);
                    web(root, want, progress, groups, done, finish)
                });
            }
            None => {
                done.extend(g.tries.end());
                web(root, want, progress, groups, done, finish)
            }
        }
        return;
    }
    let link = g.links.remove(0);
    g.tried.push(link.clone());
    let diff = g.diff;
    // found by name: the chart it brings must be the one
    let check = g.checked.contains(&link).then(|| g.chart.clone()).flatten();
    // this link's outcome: the file in, or the group again (its next link)
    let next = move |got: Vec<Done>, mut groups: Vec<Group>, mut done: Vec<Done>| {
        match g.tries.record(got) {
            Some(got) => done.extend(got),
            None => groups.insert(0, g),
        }
        (groups, done)
    };
    if chart_link(&link) {
        let name = chart_name(&link);
        progress.set_stage(Stage::Finding);
        let p = Some(progress.clone());
        crate::platform::http_get_progress(&via_proxy(&link), p, move |r| {
            let got = match r {
                Ok(b)
                    if check.as_ref().is_some_and(|m| {
                        !bmseeker::install::md5_hex(&b).eq_ignore_ascii_case(m)
                    }) =>
                {
                    Done::Failed(bmseeker::install::NOT_THE_CHART.into())
                }
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
            let (groups, done) = next(vec![got], groups, done);
            web(root, want, progress, groups, done, finish)
        });
        return;
    }
    let want_song = (!diff).then(|| want.clone());
    web_fetch(link, want_song, Some(progress.clone()), move |r| {
        progress.set_stage(Stage::Unpacking);
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
        let (groups, done) = next(got, groups, done);
        web(root, want, progress, groups, done, finish)
    });
}
