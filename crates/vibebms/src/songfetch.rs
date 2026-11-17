//! A difficulty table's song that is not in the library, downloaded in the
//! app (`bmseeker::install`) in the background: natively on a thread; on the
//! web through the vibeBMS proxy, unpacked / placed in memory. A link that
//! leads to a page instead opens it in the browser.

use crate::app::AppEvent;
use crate::library::Song;
use bmseeker::progress::{Progress, Stage};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

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
    /// each its own first, then the others known for the chart), then the
    /// add-ons `addons`: `bmseeker::install::EntryJob` (natively on a
    /// thread; on the web through the vibeBMS proxy, unpacked / placed in
    /// memory). False when there is nothing to try.
    pub fn start(
        &mut self,
        root: &Path,
        mut e: bmseeker::install::Entry,
        addons: Vec<String>,
    ) -> bool {
        use bmseeker::install::job::Installed;
        if self.is_busy(&e.want.title, &e.want.artist) {
            return false;
        }
        e.song.retain(|u| is_link(u));
        e.chart.retain(|u| is_link(u));
        let id = SongId::new(&e.want.title, &e.want.artist);
        let progress = Progress::new();
        // the settings and hosts, reporting to (and cancelled by) this
        // song's progress
        let cx = crate::hosts::install_context().with_progress(progress.clone());
        let sources = bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
        // the library as the player has it, for the song and its chart
        let lib = crate::placement::library(root, &self.songs);
        let store = Arc::new(crate::placement::Vfs);
        let job =
            bmseeker::install::EntryJob::new(store, root.join("downloads"), lib, e, sources, &cx)
                .addons(addons.into_iter().filter(|u| is_link(u)));
        if job.is_empty() {
            return false;
        }
        let tx = self
            .tx
            .get_or_insert_with(|| {
                let (tx, rx) = channel();
                self.rx = Some(rx);
                tx
            })
            .clone();
        self.busy.push((id.clone(), progress.clone()));
        progress.set_stage(Stage::Finding);
        let finish = move |out: Installed| {
            let _ = tx.send((id, out.done));
        };
        #[cfg(not(target_arch = "wasm32"))]
        crate::platform::spawn("song", move || {
            let mut job = job;
            finish(bmseeker::install::job::run_job(
                &mut job,
                &bmseeker::native::Native::new(&cx),
            ))
        });
        #[cfg(target_arch = "wasm32")]
        crate::platform::run_job(job, Some(progress), finish);
        true
    }

    /// What the songs that finished since the last call brought.
    pub fn poll(&mut self) -> Vec<AppEvent> {
        let done: Vec<(SongId, Vec<Done>)> = self
            .rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        let mut out = Vec::new();
        for (id, d) in done {
            if let Some(i) = self.busy.iter().position(|(b, _)| *b == id) {
                let (id, p) = self.busy.remove(i);
                // in the library: until it is read again
                if bmseeker::install::got(&d) {
                    p.set_stage(Stage::Adding);
                    self.adding.push((id, p));
                }
            }
            out.extend(d.into_iter().flat_map(|d| events(&id.title, d)));
        }
        out
    }
}

/// What to tell of a link of the song `title`, and do.
fn events(title: &str, d: Done) -> Vec<AppEvent> {
    let name = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let toast = |s: String| vec![AppEvent::Toast(s)];
    match d {
        Done::Added(dir, n) => vec![
            AppEvent::Toast(format!("Added {} ({n} files)", name(&dir))),
            AppEvent::Rescan,
        ],
        // (files its folder lacked may have come in)
        Done::Owned(dir) => vec![
            AppEvent::Toast(format!("{title}: already installed ({})", name(&dir))),
            AppEvent::Rescan,
        ],
        Done::Placed(p) => {
            let dir = p.parent().map(name).unwrap_or_default();
            vec![
                AppEvent::Toast(format!("{} → {dir}", name(&p))),
                AppEvent::Rescan,
            ]
        }
        Done::Waiting(p) => toast(format!("{}: download its song too", name(&p))),
        Done::Page(url) => vec![
            AppEvent::Toast(format!("{title}: no direct download, opening its page")),
            AppEvent::Browse(url),
        ],
        // (cancelled: said when it was)
        Done::Failed(e) if e.cancelled() => Vec::new(),
        Done::Failed(Failure::NoChartLink) => toast(format!(
            "{title}: the song is in the library, but none of this chart's links works"
        )),
        Done::Failed(e) => toast(format!("{title}: download failed ({e})")),
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
}
