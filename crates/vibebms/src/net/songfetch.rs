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
    /// (a table may give it alone)
    pub sha256: String,
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
    /// What the downloads share ([`Run`]), while one runs or the library
    /// has not been read again for what they brought.
    run: Option<Run>,
    /// The run's downloads brought what the library is to be read again
    /// for (a [`Polled::Rescan`] since the last [`SongFetch::scanned`]).
    owed: bool,
    /// The charts wanted ([`SongFetch::want`]) while no run is: told to the
    /// next.
    wanted: Vec<String>,
}

/// The downloads into one library that share it: the library as bmseeker
/// sees it (built from the player's songs when the first started, what
/// came in since added) and each file once (`bmseeker::install::Session`).
/// A download started while the run lasts sees what the others brought,
/// even before the library is read again; the next run starts from the
/// library as then read, which has it.
struct Run {
    root: std::path::PathBuf,
    session: bmseeker::install::Session,
    /// The charts told to the session as wanted (MD5s, lower case).
    wanted: std::collections::HashSet<String>,
    /// A download of it stopped unexpectedly (it may have lost the
    /// library): no download joins it; the next starts once it has ended
    /// (its downloads, and the library read again for what they brought).
    lost: bool,
}

impl Run {
    /// The charts `md5s` wanted too (those told before passed by).
    fn want(&mut self, md5s: impl IntoIterator<Item = String>) {
        let new: Vec<String> = (md5s.into_iter())
            .map(|m| m.to_ascii_lowercase())
            .filter(|m| !m.is_empty() && self.wanted.insert(m.clone()))
            .collect();
        if !new.is_empty() {
            self.session.want(new);
        }
    }
}

/// What a download whose work ended without its outcome (it panicked)
/// came to.
const STOPPED: &str = "the download stopped unexpectedly";

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
        sha256: w.sha256.clone(),
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

    /// The library has been read again: the songs added are in it (the
    /// next run starts from it).
    pub fn scanned(&mut self) {
        self.adding.clear();
        self.owed = false;
        self.settle();
    }

    /// Whether the library is owed a reading for what the run brought.
    #[cfg(test)]
    pub(crate) fn owed(&mut self) -> &mut bool {
        &mut self.owed
    }

    /// The run ended when no download runs and none brought what the
    /// library has not been read again for.
    fn settle(&mut self) {
        if self.jobs.is_empty() && !self.owed {
            self.run = None;
        }
    }

    /// The run's session for the library of `songs` below `root`: the
    /// one running, else one made now (its sources from `cx`'s settings;
    /// the charts wanted meanwhile told to it).
    fn session(
        &mut self,
        root: &Path,
        songs: &[Song],
        cx: &crate::net::hosts::Context,
    ) -> &mut Run {
        if self.run.as_ref().is_none_or(|r| r.root != root) {
            let session = bmseeker::install::Session::new(
                Arc::new(crate::platform::Vfs),
                root.join("downloads"),
                crate::net::placement::library(root, songs),
                bmseeker::sources::Sources::from_config(&cx.config, Vec::new()),
            );
            let mut run = Run {
                root: root.to_path_buf(),
                session,
                wanted: Default::default(),
                lost: false,
            };
            run.want(std::mem::take(&mut self.wanted));
            self.run = Some(run);
        }
        self.run.as_mut().expect("made above")
    }

    /// The charts a bulk download wants (MD5s; those told before passed
    /// by), for the library below `root`: a separate charts' archive keeps
    /// those it holds waiting for their songs. Told to the run, or to the
    /// next one (none is made for it: a library built is a frame of the
    /// screen's).
    pub fn want(&mut self, root: &Path, md5s: impl IntoIterator<Item = String>) {
        match self.run.as_mut().filter(|r| r.root == root) {
            Some(run) => run.want(md5s),
            None => self.wanted.extend(md5s),
        }
    }

    /// The player asks again for what failed: the downloads started from
    /// now on try again the files that failed in the run (a download, a
    /// page, one that brought nothing); what came in stays known.
    pub fn again(&mut self) {
        if let Some(run) = &self.run {
            run.session.again();
        }
    }

    /// A download stopped unexpectedly and the downloads it ran with have
    /// not all ended: none starts until they have (and the library is
    /// read again for what they brought).
    pub fn held(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.lost)
    }

    /// Download the table's entry `e` (a chart, its song, their links:
    /// each its own first, then the others known for the chart), then the
    /// add-ons `addons`, into the library as the player read it (`songs`)
    /// and what the run's other downloads brought ([`Run`]):
    /// `bmseeker::install::Session::job` (natively on a thread; on the web
    /// through the vibeBMS proxy, unpacked / placed in memory; its steps
    /// made there, from the library as it is then). Its chart is wanted
    /// with the run's others. False when there is nothing to try (no run
    /// made for it), or while [`Self::held`]. `quiet` (a bulk download): no
    /// messages and no pages opened, its outcome kept for
    /// [`Self::take_ended`]; else the player chose it: what failed in the
    /// run is tried again for it ([`Self::again`]).
    pub fn start(
        &mut self,
        root: &Path,
        songs: &Arc<Vec<Song>>,
        cx: &crate::net::hosts::Context,
        e: bmseeker::install::Entry,
        addons: Vec<String>,
        quiet: bool,
    ) -> bool {
        if self.is_busy(&e.want.title, &e.want.artist) || self.held() {
            return false;
        }
        // (asked of the entry alone, with no library built)
        let alone = bmseeker::install::EntryJob::new(
            Arc::new(crate::platform::Vfs),
            root.join("downloads"),
            bmseeker::library::Library::new(&root.join("bms")),
            e.clone(),
            Default::default(),
            cx,
        );
        if alone.addons(addons.clone()).is_empty() {
            return false;
        }
        let id = SongId::new(&e.want.title, &e.want.artist);
        if !quiet {
            self.again();
        }
        let run = self.session(root, songs, cx);
        run.want([e.md5.clone()]);
        let session = run.session.clone();
        let job = Job::start(|tx, progress| {
            // the settings and hosts, reporting to (and cancelled by) this
            // song's progress
            let cx = cx.with_progress(progress.clone());
            let job = session.job(e, &cx).addons(addons);
            progress.set_stage(Stage::Finding);
            crate::platform::run_job(job, &cx, move |done: Vec<Done>| {
                let _ = tx.send(done);
            });
        });
        self.jobs.push((id, job, quiet));
        true
    }

    /// The quiet downloads that ended since the last call: song title,
    /// artist, outcome.
    pub fn take_ended(&mut self) -> Vec<(String, String, Outcome)> {
        std::mem::take(&mut self.ended)
    }

    /// What the songs that finished since the last call brought.
    pub fn poll(&mut self) -> Vec<Polled> {
        let (mut done, mut stopped) = (Vec::new(), false);
        self.jobs.retain_mut(|(id, job, quiet)| {
            let d = match job.poll() {
                Some(d) => d,
                None if job.is_over() => {
                    stopped = true;
                    vec![Done::Failed(Failure::Other(STOPPED.into()))]
                }
                None => return true,
            };
            done.push((std::mem::take(id), job.progress().clone(), d, *quiet));
            false
        });
        let mut out = Vec::new();
        // (a download that panicked may have lost the run's library: the
        // run's other downloads end on it, then the next starts a run of
        // its own; what the stopped one brought before is read in)
        if stopped {
            if let Some(run) = &mut self.run {
                run.lost = true;
            }
            out.push(Polled::Rescan);
        }
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
        self.owed |= out.contains(&Polled::Rescan);
        self.settle();
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
            sha256: "c".repeat(64),
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
        assert_eq!(e.sha256, w.sha256);
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

    /// A context whose downloads may reach this machine's test servers.
    fn local() -> crate::net::hosts::Context {
        crate::net::hosts::Context::new(bmseeker::config::Config {
            local_hosts: true,
            ..Default::default()
        })
    }

    /// [`local`], no source asked (a song not found asks none).
    fn no_sources() -> crate::net::hosts::Context {
        crate::net::hosts::Context::new(bmseeker::config::Config {
            local_hosts: true,
            sources: Vec::new(),
            ..Default::default()
        })
    }

    fn pack_chart(n: u8) -> String {
        format!("#TITLE Pack [{n}]\n#ARTIST p\n#WAV01 01.wav\n")
    }

    /// One song's archive holding two table entries' charts.
    fn pack() -> Vec<u8> {
        let (one, two) = (pack_chart(1), pack_chart(2));
        zip(&[
            ("Pack/1.bms", one.as_str()),
            ("Pack/2.bms", two.as_str()),
            ("Pack/01.wav", "RIFF"),
        ])
    }

    /// The entry of the pack's chart `n`, its song at `url`.
    fn pack_entry(n: u8, url: &str) -> bmseeker::install::Entry {
        bmseeker::install::Entry {
            md5: bmseeker::install::md5_hex(pack_chart(n).as_bytes()),
            want: bmseeker::hosts::Want {
                title: format!("Pack [{n}]"),
                artist: "p".into(),
            },
            song: vec![url.into()],
            ..Default::default()
        }
    }

    /// The quiet downloads' outcomes, once none runs.
    fn ended(f: &mut SongFetch) -> Vec<(String, Outcome)> {
        while !f.jobs.is_empty() {
            f.poll();
            std::thread::yield_now();
        }
        let mut out: Vec<_> = (f.take_ended().into_iter())
            .map(|(title, _, o)| (title, o))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn owns(f: &SongFetch, md5: &str) -> bool {
        let run = f.run.as_ref().expect("a run");
        run.session.with_library(|l| l.owns(md5)) == Some(true)
    }

    /// Two queued charts in one pack, downloading at once: the pack is
    /// downloaded once, and both are in the library.
    #[test]
    fn a_shared_pack_downloaded_once() {
        let server =
            bmseeker::testkit::serve(vec![("/pack.zip", bmseeker::testkit::Reply::ok(pack()))]);
        let url = format!("{}/pack.zip", server.url);
        let r = root("pack");
        let (cx, songs) = (local(), Arc::new(Vec::new()));
        let mut f = SongFetch::default();
        let md5s = [1, 2].map(|n| pack_entry(n, &url).md5);
        f.want(&r, md5s.clone());
        for n in [1, 2] {
            assert!(f.start(&r, &songs, &cx, pack_entry(n, &url), Vec::new(), true));
        }
        let out = ended(&mut f);
        assert!(
            matches!(&out[..], [(_, Outcome::Got(_)), (_, Outcome::Got(_))]),
            "{out:?}"
        );
        assert_eq!(server.asked(), ["/pack.zip"]);
        assert!(md5s.iter().all(|m| owns(&f, m)));
        let _ = std::fs::remove_dir_all(&r);
    }

    /// The run lasts while a download runs (a scan meanwhile does not end
    /// it) and until the library is read again for what came: a download
    /// started then sees it (the pack not downloaded again). The next run
    /// starts from the library as read then.
    #[test]
    fn a_run_lasts_until_the_library_is_read_again() {
        let server =
            bmseeker::testkit::serve(vec![("/pack.zip", bmseeker::testkit::Reply::ok(pack()))]);
        let url = format!("{}/pack.zip", server.url);
        let r = root("run");
        let (cx, songs) = (local(), Arc::new(Vec::new()));
        let mut f = SongFetch::default();
        assert!(f.start(&r, &songs, &cx, pack_entry(1, &url), Vec::new(), true));
        // (a scan that had begun before it ended)
        f.scanned();
        assert!(f.run.is_some());
        assert!(matches!(&ended(&mut f)[..], [(_, Outcome::Got(_))]));
        assert!(f.run.is_some(), "what came is not read yet");
        let two = pack_entry(2, &url);
        assert!(f.start(&r, &songs, &cx, two.clone(), Vec::new(), true));
        let out = ended(&mut f);
        assert!(
            matches!(&out[..], [(_, Outcome::Got(how))] if how.starts_with("already in")),
            "{out:?}"
        );
        assert_eq!(server.asked(), ["/pack.zip"]);
        f.scanned();
        assert!(f.run.is_none());
        // the next run: from the library as read (which has the chart)
        let chart = walk(&r.join("bms"))
            .into_iter()
            .find(|p| p.ends_with("2.bms"));
        let read = Arc::new(vec![Song {
            path: chart
                .expect("the chart on disk")
                .to_string_lossy()
                .into_owned(),
            md5: two.md5.clone(),
            title: "Pack [2]".into(),
            artist: "p".into(),
            ..Default::default()
        }]);
        assert!(f.start(&r, &read, &cx, two, Vec::new(), true));
        let out = ended(&mut f);
        assert!(
            matches!(&out[..], [(_, Outcome::Got(how))] if how.starts_with("already in")),
            "{out:?}"
        );
        assert_eq!(server.asked(), ["/pack.zip"]);
        let _ = std::fs::remove_dir_all(&r);
    }

    /// The files below `dir`.
    fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            match e.path() {
                p if p.is_dir() => out.extend(walk(&p)),
                p => out.push(p),
            }
        }
        out
    }

    /// A server of `body` that answers each request only once `release`
    /// is set, telling `connected` of each.
    fn held(
        body: Vec<u8>,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicBool>,
        std::sync::mpsc::Receiver<()>,
    ) {
        use std::io::{Read, Write};
        use std::sync::atomic::Ordering;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, connected) = std::sync::mpsc::channel();
        let r = release.clone();
        std::thread::spawn(move || {
            for mut c in listener.incoming().flatten() {
                let (r, tx, body) = (r.clone(), tx.clone(), body.clone());
                std::thread::spawn(move || {
                    let mut asked = Vec::new();
                    let mut b = [0; 1024];
                    while !asked.windows(4).any(|w| w == b"\r\n\r\n") {
                        match c.read(&mut b) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => asked.extend_from_slice(&b[..n]),
                        }
                    }
                    let _ = tx.send(());
                    while !r.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = c.write_all(head.as_bytes());
                    let _ = c.write_all(&body);
                });
            }
        });
        (url, release, connected)
    }

    /// One of two downloads sharing a pack cancelled while the pack comes
    /// (whichever of them is downloading it): the other gets it.
    #[test]
    fn one_of_two_sharing_a_pack_cancelled() {
        let (base, release, connected) = held(pack());
        let url = format!("{base}/pack.zip");
        let r = root("cancel");
        let (cx, songs) = (local(), Arc::new(Vec::new()));
        let mut f = SongFetch::default();
        for n in [1, 2] {
            assert!(f.start(&r, &songs, &cx, pack_entry(n, &url), Vec::new(), true));
        }
        connected
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
        assert!(f.cancel("Pack [1]", "p"));
        // (the cancelled one has ended, or ends on the answer)
        release.store(true, std::sync::atomic::Ordering::Relaxed);
        let out = ended(&mut f);
        assert!(
            matches!(&out[..], [(_, Outcome::Cancelled), (_, Outcome::Got(_))]),
            "{out:?}"
        );
        assert!(owns(&f, &pack_entry(2, &url).md5));
        let _ = std::fs::remove_dir_all(&r);
    }

    /// The downloads (none quiet) running, polled until only `left` run;
    /// what they told.
    fn until(f: &mut SongFetch, left: usize) -> Vec<Polled> {
        let mut out = Vec::new();
        while f.jobs.len() > left {
            out.extend(f.poll());
            std::thread::yield_now();
        }
        out
    }

    /// The first quiet download to end, the others running.
    fn next_ended(f: &mut SongFetch) -> Outcome {
        loop {
            f.poll();
            if let Some((_, _, o)) = f.take_ended().pop() {
                return o;
            }
            std::thread::yield_now();
        }
    }

    /// A download whose work panicked (it may have lost the run's
    /// library): it ends failed, is not left downloading, and the library
    /// is read again. The run's other downloads end on it; no download
    /// starts (none joins it, none beside it) until they have and the
    /// library has been read again; the next starts a run of its own.
    #[test]
    fn a_download_that_panicked_ends() {
        let (base, release, connected) = held(pack());
        let url = format!("{base}/pack.zip");
        let r = root("panicked");
        let (cx, songs) = (local(), Arc::new(Vec::new()));
        let mut f = SongFetch::default();
        assert!(f.start(&r, &songs, &cx, pack_entry(1, &url), Vec::new(), true));
        connected
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
        let job = Job::spawn("test", |_| -> Vec<Done> { panic!("a step that panics") });
        f.jobs.push((SongId::new("Aurora", "alpha"), job, true));
        let mut told = Vec::new();
        while f.jobs.len() > 1 {
            told.extend(f.poll());
            std::thread::yield_now();
        }
        assert!(told.contains(&Polled::Rescan), "{told:?}");
        assert!(!f.is_busy("Aurora", "alpha"));
        assert_eq!(
            f.take_ended(),
            [(
                "Aurora".to_string(),
                "alpha".to_string(),
                Outcome::Failed(STOPPED.into())
            )]
        );
        assert!(f.held());
        assert!(!f.start(&r, &songs, &cx, pack_entry(2, &url), Vec::new(), true));
        release.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(matches!(&ended(&mut f)[..], [(_, Outcome::Got(_))]));
        assert!(f.held(), "until the library is read again");
        f.scanned();
        assert!(!f.held() && f.run.is_none());
        assert!(f.start(&r, &songs, &cx, pack_entry(2, &url), Vec::new(), true));
        assert!(f.run.as_ref().is_some_and(|r| !r.lost));
        let _ = ended(&mut f);
        let _ = std::fs::remove_dir_all(&r);
    }

    /// Within a run (another download keeps it going), a file that failed
    /// answers the next download of it at once; after [`SongFetch::again`]
    /// (the player retried), or for a chart the player chose, it is asked
    /// for again: a 404 the server now serves, a page still a page.
    #[test]
    fn a_retry_in_the_run_tries_again() {
        let (base, release, connected) = held(pack());
        let block = pack_entry(1, &format!("{base}/pack.zip"));
        let chart = |n: u8| format!("#TITLE Song {n}\n#ARTIST o\n#WAV01 01.wav\n");
        let song = |n: u8| {
            zip(&[
                (&*format!("{n}/{n}.bms"), &*chart(n)),
                (&*format!("{n}/01.wav"), "RIFF"),
            ])
        };
        use bmseeker::testkit::Reply;
        let server = bmseeker::testkit::serve(vec![
            ("/a.zip", Reply::status(404, "")),
            ("/a.zip", Reply::ok(song(1))),
            ("/b.zip", Reply::status(404, "")),
            ("/b.zip", Reply::ok(song(2))),
            ("/page", Reply::ok("<html>the event</html>")),
        ]);
        let e = |n: u8, path: &str| bmseeker::install::Entry {
            md5: bmseeker::install::md5_hex(chart(n).as_bytes()),
            want: bmseeker::hosts::Want {
                title: format!("Song {n}"),
                artist: "o".into(),
            },
            song: vec![format!("{}{path}", server.url)],
            ..Default::default()
        };
        let r = root("again");
        let (cx, songs) = (no_sources(), Arc::new(Vec::new()));
        let mut f = SongFetch::default();
        assert!(f.start(&r, &songs, &cx, block, Vec::new(), true));
        connected
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
        let run = |f: &mut SongFetch, e: bmseeker::install::Entry| {
            assert!(f.start(&r, &songs, &cx, e, Vec::new(), true));
            next_ended(f)
        };
        let failed = |o: &Outcome| matches!(o, Outcome::Failed(_));
        assert!(failed(&run(&mut f, e(1, "/a.zip"))));
        assert!(failed(&run(&mut f, e(1, "/a.zip"))));
        assert_eq!(server.asked(), ["/a.zip"]);
        f.again();
        assert!(matches!(run(&mut f, e(1, "/a.zip")), Outcome::Got(_)));
        assert_eq!(server.asked().len(), 2);
        // a page: a page again, asked again
        let page = |o: Outcome| matches!(o, Outcome::Page(u) if u.ends_with("/page"));
        assert!(page(run(&mut f, e(3, "/page"))));
        f.again();
        assert!(page(run(&mut f, e(3, "/page"))));
        assert_eq!(server.asked().iter().filter(|p| *p == "/page").count(), 2);
        // chosen by the player: tried again, and wanted with the run's
        assert!(failed(&run(&mut f, e(2, "/b.zip"))));
        let two = e(2, "/b.zip");
        assert!(f.start(&r, &songs, &cx, two.clone(), Vec::new(), false));
        assert!(f.run.as_ref().unwrap().wanted.contains(&two.md5));
        assert!(until(&mut f, 1).contains(&Polled::Rescan));
        assert_eq!(server.asked().iter().filter(|p| *p == "/b.zip").count(), 2);
        release.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = ended(&mut f);
        let _ = std::fs::remove_dir_all(&r);
    }

    /// An entry with nothing to try makes no run (no library built for it);
    /// the charts wanted with no run are told to the run the next download
    /// makes, before it starts.
    #[test]
    fn nothing_to_try_builds_no_library() {
        let server = bmseeker::testkit::serve(Vec::new());
        let (cx, songs) = (no_sources(), Arc::new(Vec::new()));
        let r = root("no-run");
        let mut f = SongFetch::default();
        let md5s = [1, 2].map(|n| pack_entry(n, "").md5);
        f.want(&r, md5s.clone());
        assert!(f.run.is_none());
        let nothing = bmseeker::install::Entry {
            song: Vec::new(),
            md5: String::new(),
            ..pack_entry(1, "")
        };
        assert!(!f.start(&r, &songs, &cx, nothing, Vec::new(), true));
        assert!(f.run.is_none());
        let e = pack_entry(1, &format!("{}/pack.zip", server.url));
        assert!(f.start(&r, &songs, &cx, e, Vec::new(), true));
        let run = f.run.as_ref().expect("a run");
        assert!(md5s.iter().all(|m| run.wanted.contains(m)));
        let _ = ended(&mut f);
        let _ = std::fs::remove_dir_all(&r);
    }

    /// A download waiting for another's file says so in its progress.
    #[test]
    fn a_download_waiting_for_another_says_so() {
        let (base, release, connected) = held(pack());
        let url = format!("{base}/pack.zip");
        let r = root("waiting");
        let (cx, songs) = (local(), Arc::new(Vec::new()));
        let mut f = SongFetch::default();
        for n in [1, 2] {
            assert!(f.start(&r, &songs, &cx, pack_entry(n, &url), Vec::new(), true));
        }
        connected
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
        let stage = |f: &SongFetch, t: &str| f.progress(t, "p").map(|p| p.stage());
        let waits =
            |f: &SongFetch| ["Pack [1]", "Pack [2]"].map(|t| stage(f, t) == Some(Stage::Waiting));
        while waits(&f) == [false, false] {
            std::thread::yield_now();
        }
        assert_ne!(waits(&f), [true, true]);
        release.store(true, std::sync::atomic::Ordering::Relaxed);
        let out = ended(&mut f);
        assert!(
            matches!(&out[..], [(_, Outcome::Got(_)), (_, Outcome::Got(_))]),
            "{out:?}"
        );
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
