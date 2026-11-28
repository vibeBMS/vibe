//! Bulk downloads: every chart of a difficulty table or one of its levels
//! that the library lacks, queued and downloaded one or two at a time
//! through the single-song path (`songfetch`, bmseeker's install), with
//! what became of each (queued, downloading, done, failed), retry and
//! cancel. Politeness: at most `AT_ONCE` downloads at a time, and a new one
//! started `SPACING` after the last. The downloads share the library and
//! each file (`songfetch`'s run): a pack holding several queued charts is
//! downloaded once, the queue's charts wanted by all of them. Each time
//! the player asks for downloads (a table or level queued, RETRY FAILED, a
//! chart queued again) the next download starts a new round of the run:
//! what failed in it is tried again.

use crate::net::songfetch::{Outcome, SongFetch, Wanted};
use crate::platform::{Duration, Instant};

/// Downloads at once.
pub const AT_ONCE: usize = 2;
/// The least time between two downloads' starts.
pub const SPACING: Duration = Duration::from_millis(2000);

/// What a queued chart is now.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Queued,
    Downloading,
    /// In the library (how it got there).
    Done(String),
    /// Nothing came (why).
    Failed(String),
    /// Only a page to download it from (deciding on it opens the page).
    Page(String),
    Cancelled,
}

impl State {
    /// Waiting or downloading.
    pub fn active(&self) -> bool {
        matches!(self, State::Queued | State::Downloading)
    }

    /// Ended without the chart: retried by RETRY.
    pub fn retryable(&self) -> bool {
        matches!(self, State::Failed(_) | State::Page(_) | State::Cancelled)
    }

    pub fn label(&self) -> &'static str {
        match self {
            State::Queued => "QUEUED",
            State::Downloading => "DOWNLOADING",
            State::Done(_) => "DONE",
            State::Failed(_) => "FAILED",
            State::Page(_) => "PAGE ONLY",
            State::Cancelled => "CANCELLED",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub want: Wanted,
    pub state: State,
}

/// The queue (`ctx.net.bulk`).
#[derive(Default)]
pub struct Bulk {
    pub items: Vec<Item>,
    last_start: Option<Instant>,
    /// The player asked for downloads since the last start: the next
    /// starts a new round ([`Downloader::again`]).
    again: bool,
}

/// What starts the downloads.
pub trait Downloader {
    /// The chart's song downloads already (another of its charts), or no
    /// download may start now.
    fn busy(&self, w: &Wanted) -> bool;
    /// What failed before is tried again by the downloads started next.
    fn again(&mut self);
    /// Start its download (`queue`: the list, whose charts queued or
    /// downloading are wanted by the downloads together): false when it
    /// has no link to try.
    fn start(&mut self, w: &Wanted, queue: &[Item]) -> bool;
}

/// The counts of the queue: queued, downloading, done, failed (failed,
/// page only, cancelled).
pub type Counts = [usize; 4];

impl Bulk {
    /// `wanted` queued (a chart queued or downloading already is left as
    /// it is; one that ended is queued again). How many were queued.
    pub fn queue(&mut self, wanted: impl IntoIterator<Item = Wanted>) -> usize {
        self.again = true;
        let mut n = 0;
        for w in wanted {
            let same = |i: &Item| match (w.md5.is_empty(), w.sha256.is_empty()) {
                (false, _) => i.want.md5 == w.md5,
                (true, false) => i.want.sha256 == w.sha256,
                (true, true) => i.want.title == w.title && i.want.artist == w.artist,
            };
            match self.items.iter_mut().find(|i| same(i)) {
                Some(i) if i.state.active() => {}
                Some(i) => {
                    i.state = State::Queued;
                    n += 1;
                }
                None => {
                    self.items.push(Item {
                        want: w,
                        state: State::Queued,
                    });
                    n += 1;
                }
            }
        }
        n
    }

    pub fn counts(&self) -> Counts {
        let mut c = [0; 4];
        for i in &self.items {
            c[match i.state {
                State::Queued => 0,
                State::Downloading => 1,
                State::Done(_) => 2,
                _ => 3,
            }] += 1;
        }
        c
    }

    /// The charts that failed (or were cancelled) queued again.
    pub fn retry_failed(&mut self) -> usize {
        let mut n = 0;
        for i in self.items.iter_mut().filter(|i| i.state.retryable()) {
            i.state = State::Queued;
            n += 1;
        }
        self.again = true;
        n
    }

    /// Item `k` again: queued (it ended without the chart) or cancelled
    /// (it is queued or downloading). False when it is done.
    pub fn toggle(&mut self, k: usize, fetch: &mut SongFetch) -> bool {
        let Some(i) = self.items.get_mut(k) else {
            return false;
        };
        match i.state {
            State::Queued => i.state = State::Cancelled,
            // (its outcome comes, cancelled, once it has stopped)
            State::Downloading => {
                fetch.cancel(&i.want.title, &i.want.artist);
            }
            State::Done(_) => return false,
            _ => {
                i.state = State::Queued;
                self.again = true;
            }
        }
        true
    }

    /// Everything queued or downloading cancelled. How many.
    pub fn cancel_all(&mut self, fetch: &mut SongFetch) -> usize {
        let mut n = 0;
        for i in self.items.iter_mut() {
            match i.state {
                State::Queued => i.state = State::Cancelled,
                State::Downloading => {
                    fetch.cancel(&i.want.title, &i.want.artist);
                }
                _ => continue,
            }
            n += 1;
        }
        n
    }

    /// The charts done, failed or cancelled taken off the list.
    pub fn clear_finished(&mut self) {
        self.items.retain(|i| i.state.active());
    }

    /// One step, each frame: what the downloads that ended brought
    /// (`ended`: song title, artist, outcome); then, while fewer than
    /// `AT_ONCE` download and `SPACING` has passed, the next queued chart
    /// started (none to try: failed), a chart the library has now (`have`)
    /// done at once, one whose song downloads already left waiting. True
    /// when the list changed.
    pub fn tick(
        &mut self,
        now: Instant,
        ended: Vec<(String, String, Outcome)>,
        have: impl Fn(&Wanted) -> bool,
        dl: &mut dyn Downloader,
    ) -> bool {
        let mut changed = false;
        for (title, artist, outcome) in ended {
            let Some(i) = self.items.iter_mut().find(|i| {
                i.state == State::Downloading && i.want.title == title && i.want.artist == artist
            }) else {
                continue;
            };
            i.state = match outcome {
                Outcome::Got(how) => State::Done(how),
                Outcome::Page(url) => State::Page(url),
                Outcome::Failed(why) => State::Failed(why),
                Outcome::Cancelled => State::Cancelled,
            };
            changed = true;
        }
        let mut downloading = (self.items.iter())
            .filter(|i| i.state == State::Downloading)
            .count();
        let spaced = |last: Option<Instant>| {
            last.is_none_or(|t| now.saturating_duration_since(t) >= SPACING)
        };
        for k in 0..self.items.len() {
            if downloading >= AT_ONCE || !spaced(self.last_start) {
                break;
            }
            let i = &mut self.items[k];
            if i.state != State::Queued {
                continue;
            }
            if have(&i.want) {
                i.state = State::Done("in the library".into());
                changed = true;
                continue;
            }
            // its song downloads (another of its charts): after it
            if dl.busy(&i.want) {
                continue;
            }
            if std::mem::take(&mut self.again) {
                dl.again();
            }
            let started = dl.start(&self.items[k].want, &self.items);
            let i = &mut self.items[k];
            i.state = if started {
                downloading += 1;
                self.last_start = Some(now);
                State::Downloading
            } else {
                State::Failed("no download link".into())
            };
            changed = true;
        }
        changed
    }
}

/// The single-song path, quietly (the states here, not messages).
struct Fetch<'a> {
    fetch: &'a mut SongFetch,
    root: &'a std::path::Path,
    songs: &'a std::sync::Arc<Vec<crate::library::Song>>,
    hosts: &'a crate::net::hosts::Context,
    known: &'a crate::net::links::LinkLists,
}

impl Downloader for Fetch<'_> {
    fn busy(&self, w: &Wanted) -> bool {
        self.fetch.held() || self.fetch.is_busy(&w.title, &w.artist)
    }

    fn again(&mut self) {
        self.fetch.again();
    }

    fn start(&mut self, w: &Wanted, queue: &[Item]) -> bool {
        // (all the queue's: a separate charts' archive keeps them waiting)
        let wanted = queue.iter().filter(|i| i.state.active());
        (self.fetch).want(self.root, wanted.map(|i| i.want.md5.clone()));
        let entry = crate::net::songfetch::entry(self.known, w, Vec::new());
        (self.fetch).start(self.root, self.songs, self.hosts, entry, Vec::new(), true)
    }
}

/// One step of the queue `bulk` (each frame), its songs downloaded by
/// `fetch` into the library below `root`, whose charts are `songs`, with
/// `hosts` and the links `known`. True when what the music select lists
/// changed.
pub fn tick(
    bulk: &mut Bulk,
    fetch: &mut SongFetch,
    root: &std::path::Path,
    songs: &std::sync::Arc<Vec<crate::library::Song>>,
    hosts: &crate::net::hosts::Context,
    known: &crate::net::links::LinkLists,
    now: Instant,
) -> bool {
    if bulk.items.is_empty() {
        return false;
    }
    let ended = fetch.take_ended();
    if ended.is_empty() && !bulk.items.iter().any(|i| i.state == State::Queued) {
        return false;
    }
    let same = |a: &str, b: &str| !a.is_empty() && a.eq_ignore_ascii_case(b);
    let have =
        |w: &Wanted| (songs.iter()).any(|s| same(&w.md5, &s.md5) || same(&w.sha256, &s.sha256));
    let mut dl = Fetch {
        fetch,
        root,
        songs,
        hosts,
        known,
    };
    bulk.tick(now, ended, have, &mut dl)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Starts what it is asked to, `busy` for the titles given.
    #[derive(Default)]
    struct Mock {
        busy: Vec<&'static str>,
        /// What was started, a new round marked `again`.
        started: Vec<String>,
        links: bool,
    }

    impl Downloader for Mock {
        fn busy(&self, w: &Wanted) -> bool {
            self.busy.contains(&w.title.as_str())
        }

        fn again(&mut self) {
            self.started.push("again".into());
        }

        fn start(&mut self, w: &Wanted, _: &[Item]) -> bool {
            self.started.push(w.title.clone());
            self.links
        }
    }

    fn want(title: &str) -> Wanted {
        Wanted {
            title: title.into(),
            md5: format!("{title:0>32}"),
            ..Default::default()
        }
    }

    /// Two at a time, spaced; the library's charts done at once, a chart
    /// whose song downloads waits; outcomes, retry and cancel.
    #[test]
    fn the_queue_downloads_two_at_a_time() {
        let mut b = Bulk::default();
        assert_eq!(b.queue(["a", "b", "c", "d"].map(want)), 4);
        // queued again while queued: no change
        assert_eq!(b.queue([want("a")]), 0);
        let t0 = Instant::now();
        let mut dl = Mock {
            links: true,
            ..Default::default()
        };
        let have = |w: &Wanted| w.title == "b";
        assert!(b.tick(t0, Vec::new(), have, &mut dl));
        // a started (a new round: the player asked), then the spacing
        dl.started.retain(|s| s != "again");
        assert_eq!(dl.started, ["a"]);
        assert_eq!(b.items[1].state, State::Queued);
        let t1 = t0 + SPACING;
        dl.busy = vec!["c"];
        b.tick(t1, Vec::new(), have, &mut dl);
        // b is in the library; c's song is busy: d instead; two
        // downloading now
        assert_eq!(b.items[1].state, State::Done("in the library".into()));
        assert_eq!(dl.started, ["a", "d"]);
        assert_eq!(b.counts(), [1, 2, 1, 0]);
        dl.busy.clear();
        b.tick(t1 + SPACING * 2, Vec::new(), have, &mut dl);
        assert_eq!(dl.started.len(), 2, "two at once");
        // a fails, d is done: c starts
        let ended = vec![
            ("a".into(), String::new(), Outcome::Failed("404".into())),
            ("d".into(), String::new(), Outcome::Got("added".into())),
        ];
        b.tick(t1 + SPACING * 3, ended, have, &mut dl);
        assert_eq!(dl.started, ["a", "d", "c"]);
        assert_eq!(b.counts(), [0, 1, 2, 1]);
        assert_eq!(b.retry_failed(), 1);
        assert_eq!(b.items[0].state, State::Queued);
        let mut fetch = SongFetch::default();
        assert_eq!(b.cancel_all(&mut fetch), 2);
        assert_eq!(b.items[0].state, State::Cancelled);
        b.clear_finished();
        // c is still downloading (its cancel comes as its outcome)
        assert_eq!(b.items.len(), 1);
        // a chart with no link fails at once
        b.queue([want("e")]);
        dl.links = false;
        b.tick(t1 + SPACING * 9, Vec::new(), have, &mut dl);
        assert_eq!(b.items[1].state, State::Failed("no download link".into()));
    }

    /// Each time the player asks for downloads (a table queued, RETRY
    /// FAILED, a chart queued again), the next download starts a new
    /// round; the queue going on by itself does not.
    #[test]
    fn the_player_asking_starts_a_new_round() {
        let mut b = Bulk::default();
        let mut dl = Mock {
            links: true,
            ..Default::default()
        };
        let never = |_: &Wanted| false;
        let mut t = Instant::now();
        let mut step = |b: &mut Bulk, dl: &mut Mock, ended| {
            t += SPACING;
            b.tick(t, ended, never, dl);
        };
        b.queue(["a", "b", "c"].map(want));
        for _ in 0..3 {
            step(&mut b, &mut dl, Vec::new());
        }
        assert_eq!(dl.started, ["again", "a", "b"]);
        let ended = |t: &str| vec![(t.to_string(), String::new(), Outcome::Failed("404".into()))];
        step(&mut b, &mut dl, ended("a"));
        assert_eq!(dl.started, ["again", "a", "b", "c"]);
        step(&mut b, &mut dl, ended("b"));
        step(&mut b, &mut dl, ended("c"));
        // RETRY FAILED
        assert_eq!(b.retry_failed(), 3);
        dl.started.clear();
        step(&mut b, &mut dl, Vec::new());
        step(&mut b, &mut dl, Vec::new());
        assert_eq!(dl.started, ["again", "a", "b"]);
        step(&mut b, &mut dl, ended("a"));
        step(&mut b, &mut dl, ended("b"));
        step(&mut b, &mut dl, ended("c"));
        // one queued again, then a table whose rows failed queued again
        let mut fetch = SongFetch::default();
        assert!(b.toggle(0, &mut fetch));
        dl.started.clear();
        step(&mut b, &mut dl, Vec::new());
        assert_eq!(dl.started, ["again", "a"]);
        assert_eq!(b.queue(["b", "c"].map(want)), 2);
        step(&mut b, &mut dl, Vec::new());
        assert_eq!(dl.started, ["again", "a", "again", "b"]);
    }

    /// A chart the library holds by its SHA-256 (a table that gives it
    /// alone) is done at once.
    #[test]
    fn a_chart_held_by_its_sha256() {
        let sha = "ab".repeat(32);
        let songs = std::sync::Arc::new(vec![crate::library::Song {
            sha256: sha.to_uppercase(),
            ..Default::default()
        }]);
        let mut b = Bulk::default();
        b.queue([Wanted {
            title: "held".into(),
            sha256: sha,
            ..Default::default()
        }]);
        let (mut fetch, cx) = (
            SongFetch::default(),
            crate::net::hosts::context(&Default::default()),
        );
        let known = crate::net::links::LinkLists::default();
        let root = std::path::Path::new("/nowhere");
        assert!(tick(
            &mut b,
            &mut fetch,
            root,
            &songs,
            &cx,
            &known,
            Instant::now()
        ));
        assert_eq!(b.items[0].state, State::Done("in the library".into()));
    }
}
