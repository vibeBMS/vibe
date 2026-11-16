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

#[cfg(target_arch = "wasm32")]
use bmseeker::install::{chart_link, chart_name};
pub use bmseeker::install::{Done, Failure};

/// A song as a table names it: its title and artist (two songs of one
/// title are two downloads).
#[derive(Clone, Debug, PartialEq, Eq)]
struct SongId {
    title: String,
    artist: String,
}

impl SongId {
    fn new(title: &str, artist: &str) -> SongId {
        SongId {
            title: title.to_string(),
            artist: artist.to_string(),
        }
    }
}

#[derive(Default)]
pub struct SongFetch {
    tx: Option<Sender<(SongId, Vec<Done>)>>,
    rx: Option<Receiver<(SongId, Vec<Done>)>>,
    /// The songs downloading, and how far each has got.
    busy: Vec<(SongId, Arc<Progress>)>,
    /// Songs in the library, until the library has been read again.
    adding: Vec<(SongId, Arc<Progress>)>,
}

/// Whether a song just added (`done`) holds the chart `md5`.
fn has_chart(done: &[Done], md5: Option<&str>) -> bool {
    md5.is_some_and(|m| {
        bmseeker::install::added(done)
            .any(|dir| bmseeker::install::holds_chart(&crate::placement::Vfs, dir, m))
    })
}

/// A chart's download whose song is in the library already, and none of
/// the chart's links brought it (nor led to a page): [`Failure::NoChartLink`].
fn no_chart_link(done: Vec<Done>, have_song: bool) -> Vec<Done> {
    let page = done.iter().any(|d| matches!(d, Done::Page(_)));
    let cancelled = done
        .iter()
        .any(|d| matches!(d, Done::Failed(e) if e.cancelled()));
    if have_song && !page && !cancelled && !bmseeker::install::got(&done) {
        vec![Done::Failed(Failure::NoChartLink)]
    } else {
        done
    }
}

/// A song just added (`done`, from one link): the charts that waited in
/// the downloads for it go into its folder (natively `install::link` does
/// this).
#[cfg(any(target_arch = "wasm32", test))]
fn place_waiting(root: &Path, mut done: Vec<Done>) -> Vec<Done> {
    if matches!(done.first(), Some(Done::Added(..))) {
        let (downloads, library) = (root.join("downloads"), root.join("bms"));
        let placed = bmseeker::install::place_waiting(&crate::placement::Vfs, &downloads, &library);
        done.extend(placed.into_iter().map(Done::Placed));
    }
    done
}

/// A separate chart's archive unpacked into `tmp` (below the downloads):
/// its charts into their songs' folders, the chart `md5` kept waiting (no
/// other: a pack without it fails, and the next link is tried); `tmp` goes.
#[cfg(any(target_arch = "wasm32", test))]
fn place_diff(root: &Path, tmp: &Path, md5: &str) -> Vec<Done> {
    let (downloads, library) = (root.join("downloads"), root.join("bms"));
    let done =
        bmseeker::install::place_diff(&crate::placement::Vfs, tmp, &downloads, &library, md5, None);
    crate::platform::vfs::remove_dir(tmp);
    done
}

/// The library folder (below `<root>/bms`) for the song `want`'s archive
/// named `stem`: that name, unless a folder of it holds another song of
/// that title (then `stem (2)`...).
#[cfg(any(target_arch = "wasm32", test))]
fn song_dir(root: &Path, stem: &str, want: &crate::hosts::Want) -> std::path::PathBuf {
    let chart = crate::placement::ChartInfo {
        title: want.title.clone(),
        artist: want.artist.clone(),
        sounds: Vec::new(),
    };
    bmseeker::placement::song_folder(&crate::placement::Vfs, &root.join("bms"), stem, &[chart])
}

fn is_link(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://")
}

impl SongFetch {
    /// Downloading, or in the library but not read yet.
    pub fn is_busy(&self, title: &str, artist: &str) -> bool {
        self.progress(title, artist).is_some()
    }

    /// How far the song's download has got (while [`Self::is_busy`]).
    pub fn progress(&self, title: &str, artist: &str) -> Option<&Arc<Progress>> {
        let id = SongId::new(title, artist);
        self.busy
            .iter()
            .chain(&self.adding)
            .find(|(s, _)| *s == id)
            .map(|(_, p)| p)
    }

    /// Stop the song's download (true if it was downloading). Its outcome
    /// still comes (a failure, cancelled), when it has stopped.
    pub fn cancel(&mut self, title: &str, artist: &str) -> bool {
        let id = SongId::new(title, artist);
        let Some(i) = self.busy.iter().position(|(s, _)| *s == id) else {
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
    /// in [`Failure::NoChartLink`], and the song is not downloaded again. False when
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
        if links.is_empty() || self.is_busy(title, artist) {
            return false;
        }
        if self.tx.is_none() {
            let (tx, rx) = channel();
            self.tx = Some(tx);
            self.rx = Some(rx);
        }
        let tx = self.tx.clone().expect("channel");
        let progress = Progress::new();
        let id = SongId::new(title, artist);
        self.busy.push((id.clone(), progress.clone()));
        let want = crate::hosts::Want {
            title: title.to_string(),
            artist: artist.to_string(),
        };
        let root = root.to_path_buf();
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
            let _ = tx.send((id, done));
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
                    want: md5.clone(),
                })
                .collect();
            web(root, want, progress, groups, Vec::new(), move |done| {
                let _ = tx.send((id, no_chart_link(done, have_song)));
            });
        }
        true
    }

    /// The songs that finished since the last call (their titles), with
    /// what became of their links.
    pub fn poll(&mut self) -> Vec<(String, Vec<Done>)> {
        let done: Vec<(SongId, Vec<Done>)> = self
            .rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for (id, d) in &done {
            if let Some(i) = self.busy.iter().position(|(b, _)| b == id) {
                let (id, p) = self.busy.remove(i);
                // in the library: until it is read again
                if bmseeker::install::got(d) {
                    p.set_stage(Stage::Adding);
                    self.adding.push((id, p));
                }
            }
        }
        done.into_iter().map(|(id, d)| (id.title, d)).collect()
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
    /// Links a source found by name: what they bring is checked to hold
    /// the chart `want`.
    checked: Vec<String>,
    want: Option<String>,
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
        done.push(Done::Failed(bmseeker::net::NetError::Cancelled.into()));
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
                // through the proxy (but BMS SEARCH, which sends CORS headers)
                crate::platform::run_task(sources.find_all(&q), true, move |found| {
                    for (source, l) in found {
                        if g.tried.contains(&l) || g.links.contains(&l) {
                            continue;
                        }
                        // found by name: what it brings is checked
                        if sources.checked(source) {
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
    let check = g.checked.contains(&link).then(|| g.want.clone()).flatten();
    let md5 = g.want.clone().unwrap_or_default();
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
                    Done::Failed(Failure::NotTheChart)
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
    let song = want_song.clone();
    web_fetch(link, want_song, Some(progress.clone()), move |r| {
        progress.set_stage(Stage::Unpacking);
        let got = match r {
            // found by name: only if it holds the chart (a song: or its
            // charts are that song's, its chart a separate one)
            Ok(bytes)
                if check.as_ref().is_some_and(|m| {
                    !bmseeker::install::archive_holds_chart(bytes.clone(), m, song.as_ref())
                }) =>
            {
                vec![Done::Failed(Failure::NotTheChart)]
            }
            // a separate chart's archive: its charts into their songs' folders
            Ok(bytes) if diff => {
                let ext = crate::hosts::sniff_bytes(&bytes).unwrap_or("zip");
                let stem = crate::archive::safe_name(&title);
                // (what is left of it goes with the folder)
                let tmp = root.join("downloads").join(".diff");
                match vfs::add_archive_at(&tmp, &format!("{stem}.{ext}"), bytes) {
                    0 => {
                        vfs::remove_dir(&tmp);
                        vec![Done::Failed("the archive holds nothing".into())]
                    }
                    _ => place_diff(&root, &tmp, &md5),
                }
            }
            Ok(bytes) => {
                let ext = crate::hosts::sniff_bytes(&bytes).unwrap_or("zip");
                let dir = song_dir(&root, &crate::archive::safe_name(&title), &want);
                let stem = dir.file_name().unwrap_or_default().to_string_lossy();
                // and the charts that waited for it
                place_waiting(
                    &root,
                    vec![match vfs::add_zip(&root, &format!("{stem}.{ext}"), bytes) {
                        0 => Done::Failed("the archive holds nothing".into()),
                        n => Done::Added(dir.clone(), n),
                    }],
                )
            }
            Err(Fail::Page(url)) => vec![Done::Page(url)],
            Err(Fail::Msg(e)) => vec![Done::Failed(e.into())],
        };
        let (groups, done) = next(got, groups, done);
        web(root, want, progress, groups, done, finish)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("vibebms-songfetch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn put(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    const OURS: &str = "#TITLE Song [X]\n#ARTIST alpha / obj: b\n#WAV01 01.wav\n";

    /// The web build's separate chart archive: the chart wanted waits (no
    /// other), the unpacked folder goes; a pack without it is no success.
    #[test]
    fn a_separate_charts_archive_on_the_web() {
        let r = root("diff");
        let tmp = r.join("downloads/.diff");
        put(&tmp.join("Song/ours.bms"), OURS);
        put(
            &tmp.join("Song/other.bms"),
            "#TITLE Other\n#ARTIST g\n#WAV01 o.wav\n",
        );
        let missing = bmseeker::install::md5_hex(b"#TITLE Missing");
        let done = place_diff(&r, &tmp, &missing);
        assert!(!bmseeker::install::got(&done), "{done:?}");
        assert!(matches!(&done[..], [Done::Failed(Failure::NotTheChart)]));
        assert!(!tmp.exists() && !r.join("downloads/ours.bms").exists());
        put(&tmp.join("Song/ours.bms"), OURS);
        put(
            &tmp.join("Song/other.bms"),
            "#TITLE Other\n#ARTIST g\n#WAV01 o.wav\n",
        );
        let md5 = bmseeker::install::md5_hex(OURS.as_bytes());
        let done = place_diff(&r, &tmp, &md5);
        let waiting = r.join("downloads/ours.bms");
        assert!(
            matches!(&done[..], [Done::Waiting(p)] if *p == waiting),
            "{done:?}"
        );
        assert!(waiting.is_file() && !r.join("downloads/other.bms").exists());
        assert!(!tmp.exists());
        let _ = std::fs::remove_dir_all(&r);
    }

    /// A chart that came before its song goes into the song's folder once
    /// the song is added.
    #[test]
    fn a_waiting_chart_finds_its_song() {
        let r = root("waiting");
        put(&r.join("downloads/ours.bms"), OURS);
        let song = r.join("bms/Song");
        put(
            &song.join("song.bms"),
            "#TITLE Song\n#ARTIST alpha\n#WAV01 01.wav\n",
        );
        put(&song.join("01.ogg"), "x");
        // a failure places nothing
        let done = place_waiting(&r, vec![Done::Failed("x".into())]);
        assert_eq!(done.len(), 1);
        let done = place_waiting(&r, vec![Done::Added(song.clone(), 2)]);
        assert!(
            matches!(&done[..], [Done::Added(..), Done::Placed(p)] if *p == song.join("ours.bms")),
            "{done:?}"
        );
        let _ = std::fs::remove_dir_all(&r);
    }

    /// Two songs of one title are two downloads.
    #[test]
    fn downloads_by_title_and_artist() {
        let mut f = SongFetch::default();
        f.busy
            .push((SongId::new("Aurora", "alpha"), Progress::new()));
        assert!(f.is_busy("Aurora", "alpha") && !f.is_busy("Aurora", "beta"));
        assert!(f.progress("Aurora", "beta").is_none());
        assert!(!f.cancel("Aurora", "beta"));
        assert!(f.cancel("Aurora", "alpha"));
        assert!(!f.is_busy("Aurora", "alpha"));
    }

    /// Another song of the title has the folder: this one gets its own.
    #[test]
    fn same_title_other_song_on_the_web() {
        let r = root("same-title");
        put(
            &r.join("bms/Aurora/a.bms"),
            "#TITLE Aurora [A]\n#ARTIST alpha\n",
        );
        let want = |artist: &str| crate::hosts::Want {
            title: "Aurora [HYPER]".into(),
            artist: artist.into(),
        };
        assert_eq!(
            song_dir(&r, "Aurora", &want("alpha / obj: x")),
            r.join("bms/Aurora")
        );
        assert_eq!(
            song_dir(&r, "Aurora", &want("beta")),
            r.join("bms/Aurora (2)")
        );
        assert_eq!(song_dir(&r, "Other", &want("beta")), r.join("bms/Other"));
        let _ = std::fs::remove_dir_all(&r);
    }

    /// The chart alone, its song in the library: no link working is a
    /// failure of its own; a page or a cancel stays what it was.
    #[test]
    fn no_chart_link_with_the_song_in() {
        let failed = || vec![Done::Failed(Failure::NotTheChart)];
        assert!(matches!(
            &no_chart_link(failed(), true)[..],
            [Done::Failed(Failure::NoChartLink)]
        ));
        assert!(matches!(
            &no_chart_link(failed(), false)[..],
            [Done::Failed(Failure::NotTheChart)]
        ));
        let cancelled = vec![Done::Failed(bmseeker::net::NetError::Cancelled.into())];
        assert!(matches!(&no_chart_link(cancelled, true)[..], [Done::Failed(e)] if e.cancelled()));
        let page = vec![Done::Page("https://x/".into())];
        assert!(matches!(&no_chart_link(page, true)[..], [Done::Page(_)]));
    }
}
