//! A difficulty table's song that is not in the library, downloaded in the
//! app (`bmseeker::install`) in the background: natively on a thread; on the
//! web through the vibeBMS proxy, unpacked / placed in memory. A link that
//! leads to a page instead opens it in the browser.

use crate::library::Song;
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
    /// The library's charts as last read (where each is, its hashes, title
    /// and artist), for placing what comes.
    songs: Arc<Vec<Song>>,
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

/// What a link brought (`bytes`; a chart's own link: its file name
/// `file`) into the library, as natively (`bmseeker::install::take`): in
/// memory, unpacked, its packages each into a folder of its own, into
/// their songs' folders (the library as the player read it), or waiting;
/// a song added, the charts that waited for it into its folder.
/// `check`: found by name, only if it holds the chart `q.md5`.
#[cfg(any(target_arch = "wasm32", test))]
fn take(
    root: &Path,
    songs: &[Song],
    bytes: Vec<u8>,
    q: &bmseeker::sources::Query,
    check: bool,
    file: Option<String>,
) -> Vec<Done> {
    let req = bmseeker::install::request(q, check, file, &crate::hosts::context().config);
    let lib = &mut crate::placement::library(root, songs);
    bmseeker::install::take(
        &crate::placement::Vfs,
        bmseeker::unpack::Blob::Bytes(bytes),
        &req,
        &root.join("downloads"),
        lib,
    )
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

    /// The library's charts, as read (for placing what comes).
    pub fn set_library(&mut self, songs: Arc<Vec<Song>>) {
        self.songs = songs;
    }

    /// Download `title` from `links`: the song's, then its separate
    /// chart's (each its own first, then the others known for the chart, tried
    /// in turn until one downloads; for the song, then the links BMS SEARCH
    /// knows for the chart `md5`). With the song in the library already
    /// (`have_song`), only the chart: when none of its links works, it ends
    /// in [`Failure::NoChartLink`], and the song is not downloaded again. False when
    /// there is nothing to try. `org_md5`: the song's own charts a table
    /// names (where its separate chart goes).
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        root: &Path,
        title: &str,
        artist: &str,
        md5: &str,
        org_md5: Vec<String>,
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
        let songs = self.songs.clone();
        #[cfg(not(target_arch = "wasm32"))]
        crate::platform::spawn("song", move || {
            progress.set_stage(Stage::Finding);
            // the library as the player has it, for the song and its chart
            let mut lib = crate::placement::library(&root, &songs);
            // the settings and hosts, reporting to (and cancelled by) this
            // song's progress
            let cx = crate::hosts::context().with_progress(progress);
            let mut done = Vec::new();
            for (ls, diff) in &links {
                // the song brought the chart: no separate chart to get
                if *diff && has_chart(&done, md5.as_deref()) {
                    break;
                }
                // the lists' links are in `ls` already: BMS SEARCH after
                let q = bmseeker::sources::Query {
                    md5: md5.clone().unwrap_or_default(),
                    want: want.clone(),
                    diff: *diff,
                    org_md5: org_md5.clone(),
                };
                let sources = bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
                done.extend(bmseeker::install::links_in(
                    &root.join("downloads"),
                    &mut lib,
                    &q,
                    ls,
                    &sources,
                    &cx,
                ));
            }
            let done = no_chart_link(done, have_song);
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
                    org_md5: org_md5.clone(),
                })
                .collect();
            web(
                root,
                songs,
                want,
                progress,
                groups,
                Vec::new(),
                move |done| {
                    let _ = tx.send((id, no_chart_link(done, have_song)));
                },
            );
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
    /// The song's own charts a table names (where a separate chart goes).
    org_md5: Vec<String>,
}

/// The web build: the links one after another through the vibeBMS proxy,
/// unpacked / placed in memory; a file's next link tried when one does not
/// download.
#[cfg(target_arch = "wasm32")]
fn web(
    root: PathBuf,
    songs: Arc<Vec<Song>>,
    want: crate::hosts::Want,
    progress: Arc<Progress>,
    mut groups: Vec<Group>,
    mut done: Vec<Done>,
    finish: impl FnOnce(Vec<Done>) + Send + 'static,
) {
    use crate::archive::{web_fetch, Fail};
    use crate::hosts::via_proxy;
    if groups.is_empty() {
        return finish(done);
    }
    if progress.cancelled() {
        done.push(Done::Failed(bmseeker::net::NetError::Cancelled.into()));
        return finish(done);
    }
    let mut g = groups.remove(0);
    // the song brought the chart: no separate chart to get
    if g.diff && g.tried.is_empty() && has_chart(&done, g.chart.as_deref()) {
        return web(root, songs, want, progress, groups, done, finish);
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
                    org_md5: g.org_md5.clone(),
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
                    web(root, songs, want, progress, groups, done, finish)
                });
            }
            None => {
                done.extend(g.tries.end());
                web(root, songs, want, progress, groups, done, finish)
            }
        }
        return;
    }
    let link = g.links.remove(0);
    g.tried.push(link.clone());
    // found by name: the chart it brings must be the one
    let check = g.checked.contains(&link) && g.want.is_some();
    let q = bmseeker::sources::Query {
        md5: g.want.clone().unwrap_or_default(),
        want: want.clone(),
        diff: g.diff,
        org_md5: g.org_md5.clone(),
    };
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
                // (a page where a chart was expected)
                Ok(b) if !b.trim_ascii_start().starts_with(b"<") => {
                    take(&root, &songs, b, &q, check, Some(name))
                }
                _ => vec![Done::Page(link)],
            };
            let (groups, done) = next(got, groups, done);
            web(root, songs, want, progress, groups, done, finish)
        });
        return;
    }
    let want_song = (!q.diff).then(|| want.clone());
    web_fetch(link, want_song, Some(progress.clone()), move |r| {
        progress.set_stage(Stage::Unpacking);
        let got = match r {
            Ok(bytes) => take(&root, &songs, bytes, &q, check, None),
            Err(Fail::Page(url)) => vec![Done::Page(url)],
            Err(Fail::Msg(e)) => vec![Done::Failed(e.into())],
        };
        let (groups, done) = next(got, groups, done);
        web(root, songs, want, progress, groups, done, finish)
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
    const SONG: &str = "#TITLE Song\n#ARTIST alpha\n#WAV01 01.wav\n";

    /// A zip (stored) of `files` (name, contents), as a host would serve it.
    fn zip(files: &[(&str, &str)]) -> Vec<u8> {
        fn crc32(b: &[u8]) -> u32 {
            let mut c = !0u32;
            for &x in b {
                c ^= x as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        (c >> 1) ^ 0xEDB8_8320
                    } else {
                        c >> 1
                    };
                }
            }
            !c
        }
        let (mut out, mut dir) = (Vec::new(), Vec::new());
        for (name, body) in files {
            let (n, b) = (name.as_bytes(), body.as_bytes());
            let at = out.len() as u32;
            // version, flags, stored, time, date (1980-01-01), crc, sizes
            let mut head = vec![20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0];
            head.extend(crc32(b).to_le_bytes());
            head.extend((b.len() as u32).to_le_bytes());
            head.extend((b.len() as u32).to_le_bytes());
            head.extend((n.len() as u16).to_le_bytes());
            head.extend([0, 0]);
            out.extend(b"PK\x03\x04");
            out.extend(&head);
            out.extend(n);
            out.extend(b);
            dir.extend(b"PK\x01\x02\x14\x00");
            dir.extend(&head);
            // comment, disk, attributes, offset
            dir.extend([0u8; 10]);
            dir.extend(at.to_le_bytes());
            dir.extend(n);
        }
        let start = out.len() as u32;
        out.extend(&dir);
        out.extend(b"PK\x05\x06\x00\x00\x00\x00");
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((dir.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    fn query(md5: &str, diff: bool) -> bmseeker::sources::Query {
        bmseeker::sources::Query {
            md5: md5.into(),
            want: crate::hosts::Want {
                title: "Song".into(),
                artist: "alpha".into(),
            },
            diff,
            ..Default::default()
        }
    }

    /// Nothing left of what passed through the downloads.
    fn passed(r: &Path) -> bool {
        std::fs::read_dir(r.join("downloads"))
            .into_iter()
            .flatten()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().starts_with(".take-"))
    }

    /// The web build's separate chart archive: the chart wanted waits (no
    /// other), what it was unpacked into goes; a pack without it is no
    /// success.
    #[test]
    fn a_separate_charts_archive_on_the_web() {
        let r = root("diff");
        let pack = zip(&[
            ("Song/ours.bms", OURS),
            ("Song/other.bms", "#TITLE Other\n#ARTIST g\n#WAV01 o.wav\n"),
        ]);
        let missing = bmseeker::install::md5_hex(b"#TITLE Missing");
        let done = take(&r, &[], pack.clone(), &query(&missing, true), false, None);
        assert!(matches!(&done[..], [Done::Failed(Failure::NotTheChart)]));
        assert!(!r.join("downloads/ours.bms").exists() && passed(&r));
        // found by name and not holding it: not even looked at further
        let done = take(&r, &[], pack.clone(), &query(&missing, true), true, None);
        assert!(matches!(&done[..], [Done::Failed(Failure::NotTheChart)]));
        let md5 = bmseeker::install::md5_hex(OURS.as_bytes());
        let done = take(&r, &[], pack, &query(&md5, true), false, None);
        let waiting = r.join("downloads/ours.bms");
        assert!(
            matches!(&done[..], [Done::Waiting(p)] if *p == waiting),
            "{done:?}"
        );
        assert!(waiting.is_file() && !r.join("downloads/other.bms").exists());
        assert!(passed(&r));
        let _ = std::fs::remove_dir_all(&r);
    }

    /// A song's archive: a folder of its own named after the song, the
    /// chart that waited for it placed there.
    #[test]
    fn a_waiting_chart_finds_its_song() {
        let r = root("waiting");
        put(&r.join("downloads/ours.bms"), OURS);
        let pack = zip(&[("song.bms", SONG), ("01.ogg", "x")]);
        let done = take(&r, &[], pack, &query("", false), false, None);
        let song = r.join("bms/Song");
        assert!(
            matches!(&done[..], [Done::Added(d, 2), Done::Placed(p)]
                if *d == song && *p == song.join("ours.bms")),
            "{done:?}"
        );
        assert!(song.join("01.ogg").is_file() && passed(&r));
        let _ = std::fs::remove_dir_all(&r);
    }

    /// The library as the player read it stands for its charts (in a web
    /// page they cannot be read when a download comes): a chart's own link
    /// goes into its song's folder by it, and one the library has is
    /// already installed.
    #[test]
    fn placed_by_the_library_as_read() {
        let r = root("catalog");
        let song = r.join("bms/Song");
        // (what bmseeker would read of it says nothing of the song)
        put(&song.join("song.bms"), "#WAV01 01.wav\n");
        put(&song.join("01.ogg"), "x");
        let chart = |songs: &[Song]| {
            let q = query("", true);
            take(&r, songs, OURS.into(), &q, false, Some("ours.bms".into()))
        };
        let done = chart(&[]);
        let waiting = r.join("downloads/ours.bms");
        assert!(
            matches!(&done[..], [Done::Waiting(p)] if *p == waiting),
            "{done:?}"
        );
        let _ = std::fs::remove_file(&waiting);
        let mut read = vec![Song {
            path: song.join("song.bms").to_string_lossy().into_owned(),
            md5: "0123456789abcdef0123456789abcdef".into(),
            title: "Song".into(),
            artist: "alpha".into(),
            ..Default::default()
        }];
        let done = chart(&read);
        assert!(
            matches!(&done[..], [Done::Placed(p)] if *p == song.join("ours.bms")),
            "{done:?}"
        );
        read.push(Song {
            path: song.join("ours.bms").to_string_lossy().into_owned(),
            md5: bmseeker::install::md5_hex(OURS.as_bytes()),
            ..Default::default()
        });
        let done = chart(&read);
        assert!(
            matches!(&done[..], [Done::Owned(d)] if *d == song),
            "{done:?}"
        );
        assert!(!song.join("ours_.bms").exists() && passed(&r));
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

    /// Another song of the title has the folder: this one gets its own;
    /// the same song goes into its folder.
    #[test]
    fn same_title_other_song_on_the_web() {
        let r = root("same-title");
        put(
            &r.join("bms/Song/a.bms"),
            "#TITLE Song [A]\n#ARTIST alpha\n",
        );
        // by the files, or by the library as read (its chart's title and
        // artist from its records)
        let read = [Song {
            path: r.join("bms/Song/a.bms").to_string_lossy().into_owned(),
            title: "Song [A]".into(),
            artist: "alpha".into(),
            ..Default::default()
        }];
        let pack = |artist: &str| {
            let chart = format!("#TITLE Song [B]\n#ARTIST {artist}\n#WAV01 01.wav\n");
            zip(&[("b.bms", &chart), ("01.ogg", "x")])
        };
        let mut q = query("", false);
        for songs in [&[][..], &read] {
            q.want.artist = "beta".into();
            let done = take(&r, songs, pack("beta"), &q, false, None);
            let other = r.join("bms/Song (2)");
            assert!(
                matches!(&done[..], [Done::Added(d, 2)] if *d == other),
                "{done:?}"
            );
            let _ = std::fs::remove_dir_all(&other);
            q.want.artist = "alpha / obj: x".into();
            let done = take(&r, songs, pack("alpha"), &q, false, None);
            assert!(
                matches!(&done[..], [Done::Added(d, 2)] if *d == r.join("bms/Song")),
                "{done:?}"
            );
        }
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
