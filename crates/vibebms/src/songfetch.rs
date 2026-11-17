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
use bmseeker::install::{chart_link, chart_name, Batch, Entry, Step, Tries};
pub use bmseeker::install::{Done, Failure};
use bmseeker::install::{Role, When};

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
/// `file`) into the library `lib` (the library as the player read it, and
/// what came in since), as natively (`bmseeker::install::take`): in
/// memory, unpacked, its packages each into a folder of its own, into
/// their songs' folders, or waiting; a song added, the charts that waited
/// for it into its folder. `check`: found by name, only if it holds the
/// chart `q.md5`.
#[cfg(any(target_arch = "wasm32", test))]
fn take_in(
    root: &Path,
    lib: &mut bmseeker::library::Library,
    bytes: Vec<u8>,
    q: &bmseeker::sources::Query,
    check: bool,
    file: Option<String>,
) -> Vec<Done> {
    let req = bmseeker::install::request(q, check, file, &crate::hosts::install_context().config);
    bmseeker::install::take(
        &crate::placement::Vfs,
        bmseeker::unpack::Blob::Bytes(bytes),
        &req,
        &root.join("downloads"),
        lib,
    )
}

/// [`take_in`] the library as the player read it (`songs`).
#[cfg(test)]
fn take(
    root: &Path,
    songs: &[Song],
    bytes: Vec<u8>,
    q: &bmseeker::sources::Query,
    check: bool,
    file: Option<String>,
) -> Vec<Done> {
    let lib = &mut crate::placement::library(root, songs);
    take_in(root, lib, bytes, q, check, file)
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

    /// Download the table's entry `e` (a chart, its song, their links:
    /// each its own first, then the others known for the chart), in the
    /// order `bmseeker::install::entry_steps` gives: with its song in the
    /// library, the chart first (none of its links working ends in
    /// [`Failure::NoChartLink`]), the song only if the chart is left
    /// waiting; else the song, then the chart unless the song brought it;
    /// then the add-ons `addons` (each on its own; not when the song
    /// brought the chart). With none of a download's own links, those BMS
    /// SEARCH knows for the chart's MD5. A file comes once (by its
    /// `DownloadKey`). False when there is nothing to try.
    pub fn start(
        &mut self,
        root: &Path,
        mut e: bmseeker::install::Entry,
        addons: Vec<String>,
    ) -> bool {
        use bmseeker::install::{entry_steps, Step};
        let md5 = e.md5.len() == 32;
        e.song.retain(|u| is_link(u));
        e.chart.retain(|u| is_link(u));
        let lib = crate::placement::library(root, &self.songs);
        let have_song = bmseeker::install::entry::has_song(&e, &lib);
        let mut steps = entry_steps(&e, &lib);
        steps.extend(addons.into_iter().filter(|u| is_link(u)).map(|a| Step {
            links: vec![a],
            role: Role::Diff,
            when: When::Missing,
        }));
        // none of its own: BMS SEARCH's (the chart's only with the song in)
        steps.retain(|s| !s.links.is_empty() || md5);
        if steps.is_empty() || self.is_busy(&e.want.title, &e.want.artist) {
            return false;
        }
        if self.tx.is_none() {
            let (tx, rx) = channel();
            self.tx = Some(tx);
            self.rx = Some(rx);
        }
        let tx = self.tx.clone().expect("channel");
        let progress = Progress::new();
        let id = SongId::new(&e.want.title, &e.want.artist);
        self.busy.push((id.clone(), progress.clone()));
        let root = root.to_path_buf();
        #[cfg(not(target_arch = "wasm32"))]
        crate::platform::spawn("song", move || {
            progress.set_stage(Stage::Finding);
            // the library as the player has it, for the song and its chart
            let mut lib = lib;
            // the settings and hosts, reporting to (and cancelled by) this
            // song's progress
            let cx = crate::hosts::install_context().with_progress(progress);
            let sources = bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
            let mut batch = bmseeker::install::Batch::new([e.md5.clone()]);
            let (mut done, mut last) = (Vec::new(), Vec::new());
            for step in &steps {
                if !step.due(&e, &lib, &last) {
                    continue;
                }
                // the lists' links are in the step's already: BMS SEARCH after
                last = bmseeker::install::links_seen(
                    &root.join("downloads"),
                    &mut lib,
                    &e.query(step.role),
                    &step.links,
                    &sources,
                    &mut batch,
                    &cx,
                );
                done.extend(last.iter().cloned());
            }
            let done = no_chart_link(done, have_song);
            let _ = tx.send((id, done));
        });
        #[cfg(target_arch = "wasm32")]
        {
            let groups = steps
                .into_iter()
                .map(|step| Group {
                    // BMS SEARCH: the song's, the chart's with the song in
                    ask: md5 && (step.role == Role::Song || have_song),
                    step,
                    tries: Default::default(),
                    tried: Vec::new(),
                    checked: Vec::new(),
                })
                .collect();
            let w = Web {
                root,
                batch: Batch::new([e.md5.clone()]),
                e,
                lib,
                progress,
                last: Vec::new(),
            };
            web(w, groups, Vec::new(), move |done| {
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

/// The web build's download for one step: its links to try in turn
/// (those tried, what they came to), whether BMS SEARCH is still to be
/// asked once they are tried, and the links it found by name (what they
/// bring is checked to hold the chart).
#[cfg(target_arch = "wasm32")]
struct Group {
    step: Step,
    tries: Tries,
    tried: Vec<String>,
    ask: bool,
    checked: Vec<String>,
}

/// What the web build's steps share: the entry, the library (as the player
/// read it, and what came in since), the files had, the song's progress
/// and what the last step came to.
#[cfg(target_arch = "wasm32")]
struct Web {
    root: PathBuf,
    e: Entry,
    lib: bmseeker::library::Library,
    batch: Batch,
    progress: Arc<Progress>,
    last: Vec<Done>,
}

/// The web build: the links one after another through the vibeBMS proxy,
/// unpacked / placed in memory; a step's next link tried when one does not
/// download.
#[cfg(target_arch = "wasm32")]
fn web(
    mut w: Web,
    mut groups: Vec<Group>,
    mut done: Vec<Done>,
    finish: impl FnOnce(Vec<Done>) + Send + 'static,
) {
    use crate::archive::{web_fetch, Fail};
    use crate::hosts::via_proxy;
    if groups.is_empty() {
        return finish(done);
    }
    if w.progress.cancelled() {
        done.push(Done::Failed(bmseeker::net::NetError::Cancelled.into()));
        return finish(done);
    }
    let mut g = groups.remove(0);
    // not needed now (the song brought the chart; the chart did not wait)
    if g.tried.is_empty() && !g.step.due(&w.e, &w.lib, &w.last) {
        return web(w, groups, done, finish);
    }
    let q = w.e.query(g.step.role);
    // its links tried: BMS SEARCH's next, else the end
    if g.step.links.is_empty() {
        if std::mem::take(&mut g.ask) {
            // the lists' links are in the step already: BMS SEARCH's
            let cx = crate::hosts::context();
            let sources = bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
            w.progress.set_stage(Stage::Finding);
            // through the proxy (but BMS SEARCH, which sends CORS headers)
            crate::platform::run_task(sources.find_all(&q), true, move |found| {
                for (source, l) in found {
                    if g.tried.contains(&l) || g.step.links.contains(&l) {
                        continue;
                    }
                    // found by name: what it brings is checked
                    if sources.checked(source) {
                        g.checked.push(l.clone());
                    }
                    g.step.links.push(l);
                }
                groups.insert(0, g);
                web(w, groups, done, finish)
            });
        } else {
            w.last = g.tries.end();
            done.extend(w.last.iter().cloned());
            web(w, groups, done, finish)
        }
        return;
    }
    let link = g.step.links.remove(0);
    g.tried.push(link.clone());
    // found by name: the chart it brings must be the one
    let check = g.checked.contains(&link) && w.e.md5.len() == 32;
    // a file this song's downloads have had already
    let key = Batch::key(&link, &q);
    if w.batch.has(&key) {
        let had = vec![Done::Failed(bmseeker::install::DOWNLOADED.into())];
        g.tries.record(had);
        groups.insert(0, g);
        return web(w, groups, done, finish);
    }
    // this link's outcome: the file in, or the step again (its next link)
    let next = move |got: Vec<Done>, mut w: Web, mut groups: Vec<Group>, mut done: Vec<Done>| {
        w.batch.tried(key, &got);
        match g.tries.record(got) {
            Some(got) => {
                done.extend(got.iter().cloned());
                w.last = got;
            }
            None => groups.insert(0, g),
        }
        web(w, groups, done, finish)
    };
    if chart_link(&link) {
        let name = chart_name(&link);
        w.progress.set_stage(Stage::Finding);
        let p = Some(w.progress.clone());
        crate::platform::http_get_progress(&via_proxy(&link), p, move |r| {
            let got = match r {
                // (a page where a chart was expected)
                Ok(b) if !b.trim_ascii_start().starts_with(b"<") => {
                    take_in(&w.root, &mut w.lib, b, &q, check, Some(name))
                }
                _ => vec![Done::Page(link)],
            };
            next(got, w, groups, done)
        });
        return;
    }
    let want_song = (!q.diff).then(|| q.want.clone());
    web_fetch(link, want_song, Some(w.progress.clone()), move |r| {
        w.progress.set_stage(Stage::Unpacking);
        let got = match r {
            Ok(bytes) => take_in(&w.root, &mut w.lib, bytes, &q, check, None),
            Err(Fail::Page(url)) => vec![Done::Page(url)],
            Err(Fail::Msg(e)) => vec![Done::Failed(e.into())],
        };
        next(got, w, groups, done)
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
