//! A difficulty table's song that is not in the library, downloaded in the
//! app (`bmseeker::install`) in the background: natively on a thread; on the
//! web through the vibeBMS proxy, unpacked / placed in memory. A link that
//! leads to a page instead opens it in the browser.

use crate::library::Song;
use crate::platform::Job;
use bmseeker::progress::{Progress, Stage};
use std::path::Path;
use std::sync::Arc;

pub use bmseeker::install::{Done, Failure};

/// A chart a table lists, as its entry has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wanted {
    pub title: String,
    pub artist: String,
    pub md5: String,
    pub org_md5: Vec<String>,
    /// The table's level label ("★12").
    pub level: String,
    pub url: String,
    pub url_diff: String,
}

/// A song as a table names it: its title and artist (two songs of one
/// title are two downloads).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

/// What [`SongFetch::poll`] reports, in order.
#[derive(Debug, PartialEq)]
pub enum Polled {
    Toast(String),
    /// Files came into the library: scan it again.
    Rescan,
    /// A song's download page, which needs a person.
    Browse(String),
}

#[derive(Default)]
pub struct SongFetch {
    /// The songs downloading (one cancelled until it has stopped), and
    /// what became of their links; quiet: a bulk download's (no messages,
    /// no pages opened: its outcome in `ended`).
    jobs: Vec<(SongId, Job<Vec<Done>>, bool)>,
    /// Songs in the library, until the library has been read again.
    adding: Vec<(SongId, Arc<Progress>)>,
    /// The quiet downloads that ended, until taken.
    ended: Vec<(String, String, Outcome)>,
    /// The library as bmseeker sees it, built from the player's songs (of
    /// that list, below that folder) for the downloads running: each takes
    /// a clone, which shares its records until something comes in.
    lib: Option<(
        std::sync::Weak<Vec<Song>>,
        std::path::PathBuf,
        bmseeker::library::Library,
    )>,
}

/// What a song's download came to, in a word.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// In the library (how).
    Got(String),
    /// Only a page to download it from.
    Page(String),
    Failed(String),
    Cancelled,
}

impl Outcome {
    fn of(d: &[Done], cancelled: bool) -> Outcome {
        use bmseeker::fs::file_name as name;
        if cancelled {
            return Outcome::Cancelled;
        }
        for x in d {
            match x {
                Done::Added(dir, _) => return Outcome::Got(format!("added {}", name(dir))),
                Done::Owned(dir) => return Outcome::Got(format!("already in {}", name(dir))),
                Done::Placed(p) => {
                    let dir = p.parent().map(name).unwrap_or_default();
                    return Outcome::Got(format!("put into {dir}"));
                }
                _ => {}
            }
        }
        if d.iter().any(|x| matches!(x, Done::Waiting(_))) {
            return Outcome::Failed("the chart came, its song is not in the library".into());
        }
        if let Some(url) = d.iter().find_map(|x| match x {
            Done::Page(u) => Some(u.clone()),
            _ => None,
        }) {
            return Outcome::Page(url);
        }
        match d.iter().rev().find_map(|x| match x {
            Done::Failed(e) => Some(e),
            _ => None,
        }) {
            Some(e) if e.cancelled() => Outcome::Cancelled,
            Some(e) => Outcome::Failed(e.to_string()),
            None => Outcome::Failed("nothing came".into()),
        }
    }
}

/// What to download for a table's chart `w`: its links, then the others
/// known for the chart (`known`), then `more`.
pub fn entry(
    known: &crate::net::links::LinkLists,
    w: &Wanted,
    more: Vec<String>,
) -> bmseeker::install::Entry {
    let e = bmseeker::table::TableEntry {
        md5: w.md5.clone(),
        title: w.title.clone(),
        artist: w.artist.clone(),
        org_md5: w.org_md5.clone(),
        url: w.url.clone(),
        url_diff: w.url_diff.clone(),
        ..Default::default()
    };
    let mut entry = bmseeker::install::Entry::from_table_in(&e, &known.lists());
    for u in more {
        if !entry.song.contains(&u) {
            entry.song.push(u);
        }
    }
    entry
}

/// What a link brought (`bytes`; a chart's own link: its file name
/// `file`) into the library `lib` (the library as the player read it, and
/// what came in since), as natively (`bmseeker::install::take`): in
/// memory, unpacked, its packages each into a folder of its own, into
/// their songs' folders, or waiting; a song added, the charts that waited
/// for it into its folder. `check`: found by name, only if it holds the
/// chart `q.md5`.
#[cfg(test)]
fn take_in(
    root: &Path,
    lib: &mut bmseeker::library::Library,
    bytes: Vec<u8>,
    q: &bmseeker::sources::Query,
    check: bool,
    file: Option<String>,
) -> Vec<Done> {
    let req = bmseeker::install::request(q, check, file, &Default::default());
    bmseeker::install::take(
        &crate::platform::Vfs,
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
    let lib = &mut crate::net::placement::library(root, songs);
    take_in(root, lib, bytes, q, check, file)
}

impl SongFetch {
    /// Downloading, or in the library but not read yet.
    pub fn is_busy(&self, title: &str, artist: &str) -> bool {
        self.progress(title, artist).is_some()
    }

    /// How far the song's download has got (while [`Self::is_busy`]).
    pub fn progress(&self, title: &str, artist: &str) -> Option<&Arc<Progress>> {
        let id = SongId::new(title, artist);
        self.jobs
            .iter()
            .map(|(s, job, _)| (s, job.progress()))
            .filter(|(_, p)| !p.cancelled())
            .chain(self.adding.iter().map(|(s, p)| (s, p)))
            .find(|(s, _)| **s == id)
            .map(|(_, p)| p)
    }

    /// Stop the song's download (true if it was downloading). Its outcome
    /// still comes (a failure, cancelled), when it has stopped.
    pub fn cancel(&mut self, title: &str, artist: &str) -> bool {
        let id = SongId::new(title, artist);
        match self
            .jobs
            .iter()
            .find(|(s, job, _)| *s == id && !job.progress().cancelled())
        {
            Some((_, job, _)) => {
                job.cancel();
                true
            }
            None => false,
        }
    }

    /// The library has been read again: the songs added are in it.
    pub fn scanned(&mut self) {
        self.adding.clear();
    }

    /// Download the table's entry `e` (a chart, its song, their links:
    /// each its own first, then the others known for the chart), then the
    /// add-ons `addons`, into the library as the player read it (`songs`):
    /// `bmseeker::install::EntryJob` (natively on a thread; on the web
    /// through the vibeBMS proxy, unpacked / placed in memory). False when
    /// there is nothing to try. `quiet` (a bulk download): no messages and
    /// no pages opened, its outcome kept for [`Self::take_ended`].
    pub fn start(
        &mut self,
        root: &Path,
        songs: &Arc<Vec<Song>>,
        cx: &crate::net::hosts::Context,
        e: bmseeker::install::Entry,
        addons: Vec<String>,
        quiet: bool,
    ) -> bool {
        use bmseeker::install::job::Installed;
        if self.is_busy(&e.want.title, &e.want.artist) {
            return false;
        }
        let id = SongId::new(&e.want.title, &e.want.artist);
        let lib = self.library(root, songs);
        let mut started = false;
        let job = Job::start(|tx, progress| {
            // the settings and hosts, reporting to (and cancelled by) this
            // song's progress
            let cx = cx.with_progress(progress.clone());
            let sources = bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
            let store = Arc::new(crate::platform::Vfs);
            let job = bmseeker::install::EntryJob::new(
                store,
                root.join("downloads"),
                lib,
                e,
                sources,
                &cx,
            )
            .addons(addons);
            if job.is_empty() {
                return;
            }
            started = true;
            progress.set_stage(Stage::Finding);
            crate::platform::run_job(job, &cx, move |out: Installed| {
                let _ = tx.send(out.done);
            });
        });
        if started {
            self.jobs.push((id, job, quiet));
        }
        started
    }

    /// The library of `songs` below `root`, as bmseeker sees it: the one
    /// built for them, else built now.
    fn library(&mut self, root: &Path, songs: &Arc<Vec<Song>>) -> bmseeker::library::Library {
        let built = Arc::downgrade(songs);
        match &self.lib {
            Some((s, r, lib)) if s.ptr_eq(&built) && r == root => lib.clone(),
            _ => {
                let lib = crate::net::placement::library(root, songs);
                self.lib = Some((built, root.to_path_buf(), lib.clone()));
                lib
            }
        }
    }

    /// The quiet downloads that ended since the last call: song title,
    /// artist, outcome.
    pub fn take_ended(&mut self) -> Vec<(String, String, Outcome)> {
        std::mem::take(&mut self.ended)
    }

    /// What the songs that finished since the last call brought.
    pub fn poll(&mut self) -> Vec<Polled> {
        let mut done = Vec::new();
        self.jobs.retain_mut(|(id, job, quiet)| match job.poll() {
            Some(d) => {
                done.push((std::mem::take(id), job.progress().clone(), d, *quiet));
                false
            }
            None => true,
        });
        if self.jobs.is_empty() {
            self.lib = None;
        }
        let mut out = Vec::new();
        for (id, p, d, quiet) in done {
            // in the library: until it is read again
            if !p.cancelled() && bmseeker::install::got(&d) {
                p.set_stage(Stage::Adding);
                self.adding.push((id.clone(), p.clone()));
            }
            if quiet {
                // (the library read again for what came)
                if bmseeker::install::got(&d) {
                    out.push(Polled::Rescan);
                }
                let o = Outcome::of(&d, p.cancelled());
                self.ended.push((id.title, id.artist, o));
                continue;
            }
            out.extend(d.into_iter().flat_map(|d| events(&id.title, d)));
        }
        out
    }
}

/// What to tell of a link of the song `title`, and do.
fn events(title: &str, d: Done) -> Vec<Polled> {
    use bmseeker::fs::file_name as name;
    let toast = |s: String| vec![Polled::Toast(s)];
    match d {
        Done::Added(dir, n) => vec![
            Polled::Toast(tr!("Added {name} ({n} files)", name = name(&dir), n = n)),
            Polled::Rescan,
        ],
        // (files its folder lacked may have come in)
        Done::Owned(dir) => vec![
            Polled::Toast(tr!(
                "{title}: already installed ({folder})",
                title = title,
                folder = name(&dir)
            )),
            Polled::Rescan,
        ],
        Done::Placed(p) => {
            let dir = p.parent().map(name).unwrap_or_default();
            vec![
                Polled::Toast(format!("{} → {dir}", name(&p))),
                Polled::Rescan,
            ]
        }
        Done::Waiting(p) => toast(tr!("{name}: download its song too", name = name(&p))),
        Done::Page(url) => vec![
            Polled::Toast(tr!(
                "{title}: no direct download, opening its page",
                title = title
            )),
            Polled::Browse(url),
        ],
        // (cancelled: said when it was)
        Done::Failed(e) if e.cancelled() => Vec::new(),
        Done::Failed(Failure::NoChartLink) => toast(tr!(
            "{title}: the song is in the library, but none of this chart's links works",
            title = title
        )),
        Done::Failed(e) => toast(tr!("{title}: download failed ({e})", title = title, e = e)),
        other => {
            log::warn!("{title}: {other:?}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmseeker::testkit::zip;

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

    fn query(md5: &str, diff: bool) -> bmseeker::sources::Query {
        bmseeker::sources::Query {
            md5: md5.into(),
            want: bmseeker::hosts::Want {
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

    /// What a song's download came to, in a word, for the bulk queue.
    #[test]
    fn outcomes() {
        use std::path::PathBuf;
        let added = Done::Added(PathBuf::from("/bms/Song"), 3);
        let page = Done::Page("https://example.org/dl".into());
        let failed = Done::Failed(Failure::NotTheChart);
        assert_eq!(
            Outcome::of(&[failed.clone(), added], false),
            Outcome::Got("added Song".into())
        );
        assert_eq!(
            Outcome::of(&[page, failed.clone()], false),
            Outcome::Page("https://example.org/dl".into())
        );
        assert!(matches!(
            Outcome::of(std::slice::from_ref(&failed), false),
            Outcome::Failed(_)
        ));
        assert_eq!(Outcome::of(&[failed], true), Outcome::Cancelled);
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

    /// A table's chart: its own link, then those the tables and the list
    /// know for its MD5, then `more` (each link once); its separate
    /// chart's the same way.
    #[test]
    fn entry_links_in_order() {
        let a = "a".repeat(32);
        let table = bmseeker::table::DifficultyTable {
            entries: vec![bmseeker::table::TableEntry {
                md5: a.clone(),
                url: "https://t/a.zip".into(),
                url_diff: "https://t/a.bms".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut known = crate::net::links::LinkLists::default();
        known.set_tables(bmseeker::links::Links::from_tables([&table]), &mut []);
        let w = Wanted {
            title: "Song".into(),
            artist: "alpha".into(),
            md5: a.clone(),
            org_md5: vec!["b".repeat(32)],
            url: "https://own/a.zip".into(),
            ..Default::default()
        };
        let more = ["https://t/a.zip", "https://page/a"]
            .map(String::from)
            .to_vec();
        let e = entry(&known, &w, more);
        assert_eq!(
            e.song,
            ["https://own/a.zip", "https://t/a.zip", "https://page/a"]
        );
        assert_eq!(e.chart, ["https://t/a.bms"]);
        assert_eq!((e.md5, e.org_md5), (a, w.org_md5.clone()));
        assert_eq!(
            (e.want.title.as_str(), e.want.artist.as_str()),
            ("Song", "alpha")
        );
    }

    /// What a download could not try is not tried: links not plainly
    /// written (a user name, a backslash, a control character) leave
    /// nothing to try, as no links do; a link in capitals or with spaces
    /// around it is tried.
    #[test]
    fn only_web_links_are_tried() {
        let cx = crate::net::hosts::context(&Default::default());
        let songs = Arc::new(Vec::new());
        let r = root("web-links");
        let mut f = SongFetch::default();
        let e = |song: &[&str]| bmseeker::install::Entry {
            want: bmseeker::hosts::Want {
                title: "Song".into(),
                artist: "alpha".into(),
            },
            song: song.iter().map(|u| u.to_string()).collect(),
            ..Default::default()
        };
        let none: &[&str] = &[];
        let unplain = [
            "https://user@127.0.0.1/a.zip",
            "https://127.0.0.1\\a.zip",
            "https://127.0.0.1/\u{7}a.zip",
        ];
        for links in [
            none,
            &["", "not a link", "file:///a.zip", "javascript:x"],
            &unplain,
        ] {
            assert!(
                !f.start(&r, &songs, &cx, e(links), Vec::new(), true),
                "{links:?}"
            );
            assert!(!f.start(
                &r,
                &songs,
                &cx,
                e(none),
                links.iter().map(|u| u.to_string()).collect(),
                true
            ));
        }
        // (a host of this machine: refused before any request)
        for link in [
            "HTTPS://127.0.0.1/a.zip",
            "Http://127.0.0.1/a.zip",
            " https://127.0.0.1/a.zip ",
        ] {
            for (song, addons) in [(vec![link], vec![]), (vec![], vec![link.to_string()])] {
                assert!(f.start(&r, &songs, &cx, e(&song), addons, true), "{link:?}");
                while !f.jobs.is_empty() {
                    f.poll();
                    std::thread::yield_now();
                }
            }
        }
        let _ = std::fs::remove_dir_all(&r);
    }

    /// Two songs of one title are two downloads.
    #[test]
    fn downloads_by_title_and_artist() {
        let mut f = SongFetch::default();
        let job = Job::spawn("test", |p| {
            while !p.cancelled() {
                std::thread::yield_now();
            }
            vec![Done::Failed(Failure::Net(
                bmseeker::net::NetError::Cancelled,
            ))]
        });
        f.jobs.push((SongId::new("Aurora", "alpha"), job, false));
        assert!(f.is_busy("Aurora", "alpha") && !f.is_busy("Aurora", "beta"));
        assert!(f.progress("Aurora", "beta").is_none());
        assert!(!f.cancel("Aurora", "beta"));
        assert!(f.cancel("Aurora", "alpha"));
        assert!(!f.is_busy("Aurora", "alpha") && !f.cancel("Aurora", "alpha"));
        // its outcome comes once it has stopped, and says nothing more
        while !f.jobs.is_empty() {
            assert_eq!(f.poll(), []);
            std::thread::yield_now();
        }
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
            zip(&[("b.bms", chart.as_str()), ("01.ogg", "x")])
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

    /// `SongFetch::start` over a library of 20,000 charts, 50 times: what
    /// it costs on the screen's thread before a download's own (an entry
    /// with nothing to try, so none starts). `cargo test -p vibebms
    /// start_over_a_large_library -- --ignored --nocapture`
    #[test]
    #[ignore = "a measurement"]
    fn start_over_a_large_library() {
        let songs: Arc<Vec<Song>> = Arc::new(
            (0..20_000)
                .map(|i| Song {
                    path: format!("/r/bms/Song {i}/{i}.bms"),
                    md5: format!("{i:032x}"),
                    title: format!("ソング {i} [ANOTHER]"),
                    artist: format!("Artist {} / obj: x", i % 500),
                    ..Default::default()
                })
                .collect(),
        );
        let cx = crate::net::hosts::context(&Default::default());
        let mut f = SongFetch::default();
        let started = std::time::Instant::now();
        for i in 0..50 {
            let e = bmseeker::install::Entry {
                want: bmseeker::hosts::Want {
                    title: format!("ソング {i}"),
                    artist: format!("Artist {i}"),
                },
                ..Default::default()
            };
            assert!(!f.start(Path::new("/r"), &songs, &cx, e, Vec::new(), true));
        }
        println!("50 starts over 20,000 charts: {:?}", started.elapsed());
    }
}
