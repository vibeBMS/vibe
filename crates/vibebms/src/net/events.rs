//! Mirrored BMS event entries (github.com/vibeBMS/bms): each event's release
//! has an `index.json` listing its works and the archives that hold them
//! (or, for an event that is not mirrored, the authors' download links).
//! `events.json` in that repository lists the events. The music select
//! shows an event as a folder; a work not in the library downloads from the
//! mirror and unpacks into `bms/<EVENT>/` on choosing it, or opens its
//! author's download page.

use crate::net::hosts::Context;
use crate::platform::{Job, Launch};
use bmseeker::fs::{Fs, Store};
use bmseeker::install::job::{Effect, Fed, Files, Got, Missed, Step};
use bmseeker::manbow::mirror::{self, File, Work};
use bmseeker::net::{Answer, Kind, Next, Request};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Mutex;

/// Re-read an index after this long (s).
const REFRESH_S: u64 = 86_400;

/// Re-read a list of events after this long (s): it is small, and the
/// URLs of the indexes in it may change.
const CATALOG_REFRESH_S: u64 = 600;

/// The most events a list may name, and works an index may have (the
/// published mirror: 32 events, a few hundred works an index): each event
/// is a request at every start and a folder in the music select, and each
/// work is matched against the library when its folder opens. A list or an
/// index with more is not read.
const MAX_EVENTS: usize = 2_000;
const MAX_WORKS: usize = 20_000;

/// An event as the music select lists it, with its index's works (and as
/// it is cached).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    pub url: String,
    /// "BOF22", from the release tag in the URL: the folder it installs to.
    pub name: String,
    /// The name to show ("BOF:TT"), else `name`.
    pub label: String,
    /// "THE BMS OF FIGHTERS : TT - …"
    pub title: String,
    /// The archives are mirrored (else the works link to their authors).
    pub mirrored: bool,
    pub works: Vec<Work>,
    pub fetched_at: u64,
    /// Named by a list (not by the player): its index is asked for as a
    /// download's links are ([`fetch_listed`]).
    #[serde(skip)]
    pub listed: bool,
}

impl Event {
    pub fn label(&self) -> &str {
        if self.label.is_empty() {
            &self.name
        } else {
            &self.label
        }
    }
}

/// A list of events (`events.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub url: String,
    pub events: Vec<mirror::Event>,
    pub fetched_at: u64,
}

/// Whether a configured URL is a list of events.
pub fn is_catalog(url: &str) -> bool {
    url.trim_end_matches('/').ends_with("events.json")
}

/// The folder under `bms/` of the event a list or an index URL calls
/// `tag`: in upper case. `None` unless that is one plain folder name (the
/// tag is another party's text, and a path in it would be followed).
fn folder_name(tag: &str) -> Option<String> {
    let name = tag.to_uppercase();
    (safe_name(&name) == name).then_some(name)
}

/// The event name of an index URL (`.../download/bof22/index.json`).
pub fn event_name(url: &str) -> Option<String> {
    let tag = url.trim_end_matches('/').rsplit('/').nth(1);
    folder_name(tag.unwrap_or("archive"))
}

/// Whether an index a list names is asked for: an https address, written
/// plainly (no user name in it, and no white space or control character
/// anywhere, so that the address checked is the one asked). Where it is and
/// where it leads are kept to as a download's links are ([`fetch_listed`]).
fn listed_index(index: &str) -> bool {
    !index.contains(|c: char| c.is_whitespace() || c.is_control())
        && crate::platform::http::origin(index).is_some_and(|o| o.starts_with("https://"))
}

/// Fetch a list or an index the player gave (wherever it is: their own
/// choice): at most what [`mirror`] reads of one.
fn fetch(url: &str, map: impl FnOnce(Result<Vec<u8>, String>) -> Msg + Send + 'static) -> Job<Msg> {
    Job::start(|tx, _| {
        crate::platform::http::http_get_within(url, mirror::MAX_BYTES as u64, move |r| {
            let _ = tx.send(map(r));
        })
    })
}

/// An index a list names, read.
struct ListedIndex(String);

impl bmseeker::net::Task for ListedIndex {
    type Output = Result<Vec<Work>, String>;
    fn start(&mut self) -> Next<Self::Output> {
        // (`Kind::Api`'s limit is what the mirror's reader reads)
        Next::Ask(Request::Get(self.0.clone(), Kind::Api))
    }
    fn feed(&mut self, answer: Answer) -> Next<Self::Output> {
        Next::Done((answer.map_err(|e| e.to_string())).and_then(|b| mirror::parse_index(&b)))
    }
}

/// Fetch and read an index a list names ([`listed_index`]). A list is
/// another party's text, and the app fetches what it names by itself: so
/// as the links of a download are ([`crate::platform::run_job`]), kept to
/// `cx`'s refusals, natively asking no host of this machine or its
/// networks, at its address or where a redirect leads, and following no
/// redirect from https to http.
fn fetch_listed(url: &str, cx: &Context) -> Job<Msg> {
    let url = url.to_string();
    Job::start(|tx, _| {
        let job = bmseeker::install::job::TaskJob::new(ListedIndex(url.clone()));
        crate::platform::run_job(job, cx, move |r| {
            let _ = tx.send(Msg::Index(url, r));
        })
    })
}

/// Why a work did not arrive.
pub(crate) enum Fail {
    /// Its author's link is a page to use in a browser.
    Page(String),
    Msg(String),
}

impl From<String> for Fail {
    fn from(e: String) -> Fail {
        Fail::Msg(e)
    }
}

impl From<bmseeker::net::NetError> for Fail {
    fn from(e: bmseeker::net::NetError) -> Fail {
        Fail::Msg(e.to_string())
    }
}

impl From<&str> for Fail {
    fn from(e: &str) -> Fail {
        Fail::Msg(e.into())
    }
}

enum Msg {
    Catalog(String, Result<Vec<mirror::Event>, String>),
    Index(String, Result<Vec<Work>, String>),
    Status(String),
    /// A work finished: (event, number, its folder relative to the data root).
    Done(String, u32, Result<(PathBuf, usize), Fail>),
}

/// What [`Archives::poll`] reports, in order.
#[derive(Debug, PartialEq)]
pub enum Polled {
    Toast(String),
    /// A work came into the library: scan it again.
    Rescan,
    /// A work's download page, which needs a person.
    Browse(String),
    /// What the music select lists of the events changed (their works,
    /// one installed).
    LibraryChanged,
}

#[derive(Default)]
pub struct Archives {
    /// The configured URLs, in order.
    sources: Vec<String>,
    catalogs: Vec<Catalog>,
    pub events: Vec<Event>,
    /// "EVENT/num" -> the work's folder, relative to the data root.
    pub installed: HashMap<String, PathBuf>,
    /// Works downloading now ("EVENT/num").
    pub busy: Vec<String>,
    /// Lists, indexes and works being fetched.
    jobs: Vec<Job<Msg>>,
}

fn key(event: &str, num: u32) -> String {
    format!("{event}/{num}")
}

impl Archives {
    fn dir(paths: &crate::paths::Paths) -> PathBuf {
        paths.cache.join("archives")
    }

    fn cache_file(paths: &crate::paths::Paths, url: &str) -> PathBuf {
        let h = vibebms_core::bms::md5_hex(url.as_bytes());
        Self::dir(paths).join(format!("{h}.json"))
    }

    /// The cached indexes of `urls` and the record of installed works.
    pub fn load(paths: &crate::paths::Paths, urls: &[String]) -> Archives {
        let mut a = Archives {
            sources: urls.to_vec(),
            ..Default::default()
        };
        for u in urls.iter().filter(|u| is_catalog(u)) {
            let c = crate::platform::vfs::read(&Self::cache_file(paths, u))
                .ok()
                .and_then(|b| serde_json::from_slice::<Catalog>(&b).ok())
                .filter(|c| c.events.len() <= MAX_EVENTS)
                .unwrap_or_else(|| Catalog {
                    url: u.clone(),
                    ..Default::default()
                });
            a.catalogs.push(c);
        }
        a.rebuild(paths);
        a.installed = crate::platform::vfs::read(&Self::dir(paths).join("installed.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        a
    }

    /// The events of the configured URLs in order: each catalog's, then
    /// single indexes (their works from the cache). Returns each list that
    /// had entries left out (a name or an index it may not have), and how
    /// many.
    fn rebuild(&mut self, paths: &crate::paths::Paths) -> Vec<(String, usize)> {
        let mut old: HashMap<String, Event> =
            self.events.drain(..).map(|e| (e.url.clone(), e)).collect();
        let mut wanted: Vec<Event> = Vec::new();
        let mut left_out = Vec::new();
        for u in &self.sources {
            if is_catalog(u) {
                let Some(c) = self.catalogs.iter().find(|c| &c.url == u) else {
                    continue;
                };
                let mut n = 0;
                for e in c.events.iter().filter(|e| !e.index.is_empty()) {
                    let Some(name) = folder_name(&e.tag) else {
                        log::warn!("archive events {u}: {:?} is no folder name", e.tag);
                        n += 1;
                        continue;
                    };
                    if !listed_index(&e.index) {
                        log::warn!("archive events {u}: {:?} is not a plain https URL", e.index);
                        n += 1;
                        continue;
                    }
                    wanted.push(Event {
                        url: e.index.clone(),
                        name,
                        label: e.name.clone(),
                        title: e.title.clone(),
                        mirrored: e.mirror,
                        listed: true,
                        ..Default::default()
                    });
                }
                if n > 0 {
                    left_out.push((u.clone(), n));
                }
            } else if let Some(name) = event_name(u) {
                wanted.push(Event {
                    url: u.clone(),
                    name,
                    mirrored: true,
                    ..Default::default()
                });
            } else {
                log::warn!("archive index {u}: its URL names no event");
            }
        }
        let mut seen = HashSet::new();
        for w in wanted {
            if !seen.insert(w.url.clone()) {
                continue;
            }
            // the works: from the last run, else from the cache
            let cached = old.remove(&w.url).or_else(|| {
                crate::platform::vfs::read(&Self::cache_file(paths, &w.url))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Event>(&b).ok())
                    .filter(|c| c.works.len() <= MAX_WORKS)
            });
            let (works, fetched_at) = cached.map(|c| (c.works, c.fetched_at)).unwrap_or_default();
            self.events.push(Event {
                works,
                fetched_at,
                ..w
            });
        }
        left_out
    }

    /// Fetch the lists of events and the indexes older than a day (all
    /// with `force`); an index a list names kept to `cx`
    /// ([`fetch_listed`]).
    pub fn refresh(&mut self, cx: &Context, force: bool) {
        let now = crate::platform::unix_now();
        let lists: Vec<String> = self
            .catalogs
            .iter()
            .filter(|c| force || now.saturating_sub(c.fetched_at) >= CATALOG_REFRESH_S)
            .map(|c| c.url.clone())
            .collect();
        for url in lists {
            let u = url.clone();
            self.jobs.push(fetch(&url, move |r| {
                Msg::Catalog(u, r.and_then(|b| mirror::parse_catalog(&b)))
            }));
        }
        let todo: Vec<(String, bool)> = self
            .events
            .iter()
            .filter(|e| force || now.saturating_sub(e.fetched_at) >= REFRESH_S)
            .map(|e| (e.url.clone(), e.listed))
            .collect();
        for (url, listed) in todo {
            let u = url.clone();
            self.jobs.push(match listed {
                true => fetch_listed(&url, cx),
                false => fetch(&url, move |r| {
                    Msg::Index(u, r.and_then(|b| mirror::parse_index(&b)))
                }),
            });
        }
    }

    /// The installed folder of a work (recorded when it was downloaded).
    pub fn installed_dir(&self, event: &str, num: u32) -> Option<&PathBuf> {
        self.installed.get(&key(event, num))
    }

    pub fn is_busy(&self, event: &str, num: u32) -> bool {
        self.busy.contains(&key(event, num))
    }

    /// A work's mirrored files (a work that is not mirrored is downloaded
    /// from its author's links as a table's song is: `songfetch`).
    fn files_of(&self, _event: &Event, work: &Work) -> Option<Vec<File>> {
        (!work.files.is_empty()).then(|| work.files.clone())
    }

    /// Whether choosing work `num` of event `ev` downloads it in the app
    /// (else its page opens).
    pub fn can_download(&self, ev: usize, num: u32) -> bool {
        self.events.get(ev).is_some_and(|e| {
            e.works
                .iter()
                .find(|w| w.num == num)
                .is_some_and(|w| self.files_of(e, w).is_some())
        })
    }

    /// Download work `num` of event `ev` and unpack it into the library
    /// ([`fetch_work`]).
    pub fn download(
        &mut self,
        root: &Path,
        cx: &crate::net::hosts::Context,
        ev: usize,
        num: u32,
    ) -> Result<String, String> {
        let event = self.events.get(ev).ok_or("no such event")?;
        let work = event
            .works
            .iter()
            .find(|w| w.num == num)
            .ok_or("no such work")?;
        let files = self
            .files_of(event, work)
            .ok_or("not mirrored: download it from its page")?;
        let k = key(&event.name, num);
        if self.busy.contains(&k) {
            return Err("already downloading".into());
        }
        self.busy.push(k);
        let (name, title) = (event.name.clone(), work.title.clone());
        let size: u64 = files.iter().map(|f| f.size).sum();
        let event = name.clone();
        self.jobs.push(Job::start(|tx, _| {
            let done = tx.clone();
            let launch = crate::platform::run_job;
            fetch_work(root, cx, files, &event, tx, launch, move |r| {
                let _ = done.send(Msg::Done(name, num, r));
            })
        }));
        let name = match size {
            0 => title,
            _ => format!("{title} ({} MB)", (size + (1 << 19)) >> 20),
        };
        Ok(tr!("Downloading {name}…", name = name))
    }

    /// What happened since the last frame; the events new in a list fetched
    /// ([`Archives::refresh`], kept to `cx`).
    pub fn poll(&mut self, paths: &crate::paths::Paths, cx: &Context) -> Vec<Polled> {
        let msgs = Job::drain_all(&mut self.jobs);
        let mut out = Vec::new();
        let (mut changed, mut fetched) = (false, Vec::new());
        for m in msgs {
            match m {
                Msg::Catalog(url, Ok(entries)) if entries.len() > MAX_EVENTS => {
                    log::warn!(
                        "archive events {url}: {} events, more than {MAX_EVENTS}: not read",
                        entries.len()
                    );
                    out.push(Polled::Toast(tr!(
                        "Event list {list}: more than {n} events, not read",
                        list = url,
                        n = MAX_EVENTS
                    )));
                }
                Msg::Catalog(url, Ok(entries)) => {
                    if let Some(c) = self.catalogs.iter_mut().find(|c| c.url == url) {
                        c.events = entries;
                        c.fetched_at = crate::platform::unix_now();
                        if let Ok(j) = serde_json::to_vec(c) {
                            let _ = crate::platform::vfs::create_dir_all(&Self::dir(paths));
                            let _ = crate::platform::vfs::write(&Self::cache_file(paths, &url), &j);
                        }
                        fetched.push(url);
                    }
                }
                Msg::Catalog(url, Err(e)) => log::warn!("archive events {url}: {e}"),
                Msg::Index(url, Ok(works)) if works.len() > MAX_WORKS => {
                    log::warn!(
                        "archive index {url}: {} works, more than {MAX_WORKS}: not read",
                        works.len()
                    );
                    out.push(Polled::Toast(tr!(
                        "Event index {index}: more than {n} works, not read",
                        index = url,
                        n = MAX_WORKS
                    )));
                }
                Msg::Index(url, Ok(works)) => {
                    if let Some(ev) = self.events.iter_mut().find(|e| e.url == url) {
                        ev.works = works;
                        ev.fetched_at = crate::platform::unix_now();
                        if let Ok(j) = serde_json::to_vec(ev) {
                            let _ = crate::platform::vfs::create_dir_all(&Self::dir(paths));
                            let _ = crate::platform::vfs::write(&Self::cache_file(paths, &url), &j);
                        }
                        changed = true;
                    }
                }
                Msg::Index(url, Err(e)) => log::warn!("archive index {url}: {e}"),
                Msg::Status(s) => out.push(Polled::Toast(s)),
                Msg::Done(event, num, r) => {
                    let k = key(&event, num);
                    self.busy.retain(|b| *b != k);
                    match r {
                        Ok((dir, n)) => {
                            out.push(Polled::Toast(tr!(
                                "Added {name} ({n} files)",
                                name = dir.display(),
                                n = n
                            )));
                            self.installed.insert(k, dir);
                            if let Ok(j) = serde_json::to_vec_pretty(&self.installed) {
                                let _ = crate::platform::vfs::create_dir_all(&Self::dir(paths));
                                let _ = crate::platform::vfs::write(
                                    &Self::dir(paths).join("installed.json"),
                                    &j,
                                );
                            }
                            out.push(Polled::Rescan);
                            changed = true;
                        }
                        Err(Fail::Page(url)) => {
                            out.push(Polled::Toast(
                                tr!("No direct download: opening its page").into(),
                            ));
                            out.push(Polled::Browse(url));
                        }
                        Err(Fail::Msg(e)) => {
                            out.push(Polled::Toast(tr!("Download failed: {url}", url = e)))
                        }
                    }
                }
            }
        }
        if !fetched.is_empty() {
            for (list, n) in self.rebuild(paths) {
                if fetched.contains(&list) {
                    log::warn!("archive events {list}: {n} events left out");
                    out.push(Polled::Toast(tr!(
                        "Event list {list}: {n} events left out",
                        list = list,
                        n = n
                    )));
                }
            }
            // the events new in the list
            self.refresh(cx, false);
            changed = true;
        }
        if changed {
            out.push(Polled::LibraryChanged);
        }
        out
    }
}

pub(crate) use bmseeker::install::safe_name;

/// The folder a work unpacks into: its main archive's name.
fn work_stem(files: &[File]) -> String {
    let main = files.first().map(|f| {
        if f.original.is_empty() {
            &f.name
        } else {
            &f.original
        }
    });
    // (`Song.tar.gz`: `Song`)
    bmseeker::unpack::archive_folder(main.map(String::as_str).unwrap_or(""))
}

/// Where a work unpacks, from the data root: `bms/<event>/<stem>`. `None`
/// unless both are single names and the folder is below `bms` (no `..`,
/// no root or drive): archives are written there.
fn work_dir(event: &str, stem: &str) -> Option<PathBuf> {
    let name = format!("{event}/{stem}");
    (name.matches(['/', '\\']).count() == 1)
        .then(|| crate::platform::vfs::inside(Path::new("bms"), &name))
        .flatten()
}

/// A work is not downloaded when [`work_dir`] has no folder for it.
const NO_FOLDER: &str = "its event's name is no folder of the library";

/// A work's files as bmseeker's `Files` takes them: a mirror's file as it
/// is, an author's link through the hosts.
fn sources(files: &[File]) -> Vec<bmseeker::install::job::FileLink> {
    files
        .iter()
        .map(|f| bmseeker::install::job::FileLink {
            url: f.url.clone(),
            plain: !f.original.is_empty(),
        })
        .collect()
}

/// What a work's file that did not come is: an author's link that is a
/// page, or the first mirror file (the proxy down: the browser downloads
/// it itself), opens in the browser.
fn missed(files: &[File], m: bmseeker::install::job::Missed) -> Fail {
    use crate::net::hosts::Fetch;
    let mirrored = files.get(m.index).is_some_and(|f| !f.original.is_empty());
    match m.fetch {
        Fetch::Page if mirrored && m.index > 0 => Fail::Msg(format!("{}: did not download", m.url)),
        Fetch::Page => Fail::Page(m.url),
        Fetch::Failed(e) => e.into(),
    }
}

/// The file name a work's file is unpacked as: the one its author
/// published (a mirror's file), else the mirror's.
fn file_name(f: &File) -> String {
    safe_name(if f.original.is_empty() {
        &f.name
    } else {
        &f.original
    })
}

/// A work's files, telling which comes after the first (`Msg::Status`).
struct Telling {
    files: Files,
    tx: Sender<Msg>,
    /// Each file's slot, and its name.
    slots: Vec<(PathBuf, String)>,
}

impl Telling {
    fn tell(&self, step: &Step<Result<Vec<Got>, Missed>>) {
        if let Step::Do(Effect::Download { slot, .. }) = step {
            if let Some((_, n)) = self.slots.iter().skip(1).find(|(s, _)| s == slot) {
                let _ = self
                    .tx
                    .send(Msg::Status(tr!("Downloading {name}…", name = n)));
            }
        }
    }
}

impl bmseeker::install::job::Job for Telling {
    type Output = Result<Vec<Got>, Missed>;
    fn start(&mut self) -> Step<Self::Output> {
        let step = self.files.start();
        self.tell(&step);
        step
    }
    fn feed(&mut self, fed: Fed) -> Step<Self::Output> {
        let step = self.files.feed(fed);
        self.tell(&step);
        step
    }
}

/// The player's files ([`crate::platform::Vfs`]) as a work is unpacked
/// into them, each file it writes noted: what a work that fails takes back
/// ([`take_back`]).
#[derive(Default)]
struct Noting(Mutex<Vec<PathBuf>>);

const VFS: &crate::platform::Vfs = &crate::platform::Vfs;

impl Noting {
    fn note<T>(&self, p: &Path, r: std::io::Result<T>) -> std::io::Result<T> {
        if r.is_ok() {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(p.to_path_buf());
        }
        r
    }
}

impl Fs for Noting {
    fn read(&self, p: &Path) -> std::io::Result<Vec<u8>> {
        VFS.read(p)
    }
    fn read_dir(&self, p: &Path) -> std::io::Result<Vec<bmseeker::fs::Entry>> {
        VFS.read_dir(p)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.note(to, VFS.rename(from, to))
    }
    fn stamp(&self, p: &Path) -> Option<(i64, u64)> {
        VFS.stamp(p)
    }
    fn remove(&self, p: &Path) -> std::io::Result<()> {
        VFS.remove(p)
    }
    fn keeps_times(&self) -> bool {
        VFS.keeps_times()
    }
}

impl Store for Noting {
    fn create(
        &self,
        p: &Path,
        mtime: Option<i64>,
    ) -> std::io::Result<Box<dyn std::io::Write + '_>> {
        self.note(p, VFS.create(p, mtime))
    }
    fn remove_dir(&self, p: &Path) {
        VFS.remove_dir(p)
    }
    fn open(&self, p: &Path) -> std::io::Result<Box<dyn bmseeker::fs::ReadSeek + Send>> {
        VFS.open(p)
    }
    fn create_dir_all(&self, p: &Path) {
        VFS.create_dir_all(p)
    }
    fn create_new_dir(&self, p: &Path) -> std::io::Result<()> {
        VFS.create_new_dir(p)
    }
    fn copy(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.note(to, VFS.copy(from, to))
    }
}

/// What a work that failed wrote into its folder `dest` taken back, so
/// that no part of a song is listed and trying again starts clean: the
/// folder, when it was not there before (`was`), else the files it wrote
/// (`noted`), never one that was there.
fn take_back(dest: &Path, was: bool, noted: Noting) {
    if !was {
        if crate::platform::vfs::is_dir(dest) {
            crate::platform::vfs::remove_dir(dest);
        }
        return;
    }
    for f in noted.0.into_inner().unwrap_or_else(|e| e.into_inner()) {
        let _ = crate::platform::vfs::remove_file(&f);
    }
}

/// Download a work's files (bmseeker's `Files`, a status for each after
/// the first: `tx`) and unpack them into `bms/<event>/<archive>` (add-ons
/// into the same folder); then `done` with that folder (relative to the
/// data root) and the number of files written. The files come into their
/// slots below `downloads/` natively, into memory on the web (`launch`:
/// [`crate::platform::run_job`]); either is unpacked into the player's
/// files ([`crate::platform::Vfs`]), and what a work that fails wrote there
/// is taken back ([`take_back`]).
fn fetch_work(
    root: &Path,
    cx: &Context,
    files: Vec<File>,
    event: &str,
    tx: Sender<Msg>,
    launch: Launch<Telling>,
    done: impl FnOnce(Result<(PathBuf, usize), Fail>) + Send + 'static,
) {
    let stem = work_stem(&files);
    let Some(rel) = work_dir(event, &stem) else {
        return done(Err(NO_FOLDER.into()));
    };
    let dest = root.join(&rel);
    let downloads = root.join("downloads");
    if let Err(e) = crate::platform::vfs::create_dir_all(&downloads) {
        return done(Err(e.to_string().into()));
    }
    let slots = downloads.join(format!(".{event}-{}", safe_name(&stem)));
    let job = Telling {
        files: Files::new(sources(&files), slots.clone(), cx),
        tx,
        slots: (files.iter().enumerate())
            .map(|(i, f)| (Files::slot(&slots, i), file_name(f)))
            .collect(),
    };
    let tmps: Vec<PathBuf> = job.slots.iter().map(|(s, _)| s.clone()).collect();
    launch(
        job,
        cx,
        Box::new(move |got| {
            let (was, noted) = (crate::platform::vfs::is_dir(&dest), Noting::default());
            let r = (|| {
                let got = got.map_err(|m| missed(&files, m))?;
                let mut n = 0;
                for (f, g) in files.iter().zip(got) {
                    let name = if f.original.is_empty() {
                        // an author's link: an archive (its own bytes tell
                        // which), named as one so that a file that is none is
                        // refused
                        format!("{}.zip", file_name(f))
                    } else {
                        file_name(f)
                    };
                    n += bmseeker::import::unpack_into(&noted, g.blob, &name, &dest)?;
                }
                Ok((rel, n))
            })();
            if r.is_err() {
                take_back(&dest, was, noted);
            }
            for t in &tmps {
                let _ = crate::platform::vfs::remove_file(t);
            }
            done(r)
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CATALOG, OLD_DEFAULT};

    /// Where the mirror's list is, and so its indexes.
    const MIRROR: &str = "https://raw.githubusercontent.com/vibeBMS/BMS/main";

    #[test]
    fn names() {
        assert_eq!(
            event_name("https://github.com/vibeBMS/BMS/releases/download/bof22/index.json"),
            Some("BOF22".into())
        );
        assert_eq!(safe_name("a/b:c.zip"), "a_b_c.zip");
        assert_eq!(safe_name(".."), "download");
        let file = |name: &str| File {
            name: name.into(),
            url: String::new(),
            size: 0,
            original: String::new(),
        };
        assert_eq!(work_stem(&[file("Song.tar.gz")]), "Song");
        assert_eq!(work_stem(&[file("Song_ogg.zip")]), "Song_ogg");
        assert_eq!(work_stem(&[file("notes.txt")]), "notes");
        // nothing left of the name but what Windows drops: a neutral one
        assert_eq!(work_stem(&[file(". .zip")]), "archive");
        assert_eq!(work_stem(&[file(" . . .7z")]), "archive");
        assert_eq!(work_stem(&[file("...zip")]), "archive");
    }

    #[test]
    fn reads_the_events() {
        let j = r#"[{"tag": "boftt", "name": "BOF:TT", "title": "T", "venue": 146,
            "mirror": true,
            "index": "https://raw.githubusercontent.com/vibeBMS/BMS/main/boftt/index.json"},
            {"tag": "bofnt", "name": "BOF:NT", "title": "N", "venue": 142,
            "mirror": false,
            "index": "https://raw.githubusercontent.com/vibeBMS/BMS/main/bofnt/index.json"}]"#;
        let mut a = Archives {
            sources: vec![CATALOG.into(), "https://y/download/bof22/index.json".into()],
            catalogs: vec![Catalog {
                url: CATALOG.into(),
                events: serde_json::from_str(j).unwrap(),
                fetched_at: 1,
            }],
            ..Default::default()
        };
        let paths = crate::paths::Paths::at(std::env::temp_dir().join("vibebms-archive-test"));
        a.rebuild(&paths);
        let names: Vec<(&str, &str, bool)> = a
            .events
            .iter()
            .map(|e| (e.name.as_str(), e.label(), e.mirrored))
            .collect();
        assert_eq!(
            names,
            [
                ("BOFTT", "BOF:TT", true),
                ("BOFNT", "BOF:NT", false),
                ("BOF22", "BOF22", true)
            ]
        );
        assert!(is_catalog(CATALOG) && !is_catalog(OLD_DEFAULT));
    }

    #[test]
    fn reads_other_events() {
        // events not of BOF: names in Japanese, no venue or a null one, links
        // only; an entry that does not read is left out, the rest kept
        let j = r#"[{"tag": "mumei2026", "name": "無名戦&A-1 2026",
            "title": "自称無名BMS作家が物申す！ & A-1 ClimaX 2026", "venue": 151,
            "date": "2026-05-01", "mirror": false,
            "index": "https://raw.githubusercontent.com/vibeBMS/BMS/main/mumei2026/index.json"},
            {"tag": "dpbms2014", "name": "DPBMS差分大会", "title": "合作！DPBMS差分大会",
            "venue": null, "mirror": true,
            "index": "https://raw.githubusercontent.com/vibeBMS/BMS/main/dpbms2014/index.json"},
            {"tag": "pabat", "title": "P",
            "index": "https://raw.githubusercontent.com/vibeBMS/BMS/main/pabat/index.json"},
            {"tag": "bad", "name": null,
            "index": "https://raw.githubusercontent.com/vibeBMS/BMS/main/bad/index.json"},
            {"tag": "noindex", "name": "N"}]"#;
        let events = mirror::parse_catalog(j.as_bytes()).unwrap();
        assert_eq!(events.len(), 4);
        let mut a = Archives {
            sources: vec![CATALOG.into()],
            catalogs: vec![Catalog {
                url: CATALOG.into(),
                events,
                fetched_at: 1,
            }],
            ..Default::default()
        };
        let paths = crate::paths::Paths::at(std::env::temp_dir().join("vibebms-archive-test-2"));
        a.rebuild(&paths);
        let names: Vec<(&str, &str, &str, bool)> = a
            .events
            .iter()
            .map(|e| (e.name.as_str(), e.label(), e.title.as_str(), e.mirrored))
            .collect();
        assert_eq!(
            names,
            [
                (
                    "MUMEI2026",
                    "無名戦&A-1 2026",
                    "自称無名BMS作家が物申す！ & A-1 ClimaX 2026",
                    false
                ),
                ("DPBMS2014", "DPBMS差分大会", "合作！DPBMS差分大会", true),
                ("PABAT", "PABAT", "P", false)
            ]
        );
        assert!(mirror::parse_catalog(b"{}").is_err());
        // a links-only work: no files, so its page opens rather than a download
        a.events[0].works = serde_json::from_str(
            r#"[{"num": 3, "title": "T", "artist": "A", "team": "", "kind": "",
            "page": "p", "files": [], "links": ["https://drive.google.com/x"]}]"#,
        )
        .unwrap();
        assert!(!a.can_download(0, 3));
    }

    /// An event's name becomes a folder: a list or a URL that gives a
    /// path for it has no event, and a work is not written outside `bms`.
    #[test]
    fn an_event_is_one_folder_below_bms() {
        let entry = |(i, tag): (usize, &str)| mirror::Event {
            tag: tag.into(),
            index: format!("{MIRROR}/{i}/index.json"),
            ..Default::default()
        };
        let tags = [
            "../../..",
            "..",
            ".",
            "",
            "/tmp",
            "a/b",
            "..\\..\\Start Menu",
            "C:x",
            "bof\n",
            "ok1",
        ];
        let mut a = Archives {
            sources: vec![
                CATALOG.into(),
                "https://y/download/../index.json".into(),
                "https://y//index.json".into(),
                "https://y/download/ok2/index.json".into(),
            ],
            catalogs: vec![Catalog {
                url: CATALOG.into(),
                events: tags.into_iter().enumerate().map(entry).collect(),
                fetched_at: 1,
            }],
            ..Default::default()
        };
        let paths = crate::paths::Paths::at(std::env::temp_dir().join("vibebms-archive-test-3"));
        a.rebuild(&paths);
        let names: Vec<&str> = a.events.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["OK1", "OK2"]);

        assert_eq!(work_dir("BOF22", "Song"), Some("bms/BOF22/Song".into()));
        for (event, stem) in [
            ("..", "Song"),
            ("../..", "Song"),
            ("A/..", "Song"),
            ("A\\B", "Song"),
            ("/tmp", "Song"),
            ("", "Song"),
            ("BOF22", ".."),
            ("BOF22", "a/b"),
        ] {
            assert_eq!(work_dir(event, stem), None, "{event} {stem}");
        }
    }

    /// The download itself refuses such a name, before anything is fetched
    /// or made on disk.
    #[test]
    fn a_work_is_not_fetched_outside_bms() {
        let root = std::env::temp_dir().join(format!("vibebms-archive-out-{}", std::process::id()));
        let cx = crate::net::hosts::context(&crate::config::Config::default());
        let (tx, _rx) = std::sync::mpsc::channel();
        let files = vec![File {
            name: "001.zip".into(),
            original: "Song.zip".into(),
            ..Default::default()
        }];
        let (done, r) = std::sync::mpsc::channel();
        let launch = crate::platform::run_job;
        fetch_work(
            &root.join("data"),
            &cx,
            files,
            "../..",
            tx,
            launch,
            move |r| {
                let _ = done.send(r);
            },
        );
        let r = r.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(matches!(r, Err(Fail::Msg(e)) if e == NO_FOLDER));
        assert!(!root.exists());
    }

    /// An answer of `body`, whole.
    fn answer(body: &[u8]) -> &'static str {
        let body = std::str::from_utf8(body).unwrap();
        let head = "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length";
        Box::leak(format!("{head}: {}\r\n\r\n{body}", body.len()).into_boxed_str())
    }

    /// A server of `body`, once: its URL.
    fn served(body: &[u8]) -> String {
        let (url, _) = crate::platform::http::tests::serve(vec![answer(body)]);
        format!("{url}/f")
    }

    /// What asks for a work's files: natively's own, and the web's driver
    /// (which every target builds in tests: the files come into memory).
    fn launchers() -> [(&'static str, Launch<Telling>); 2] {
        [
            ("thread", crate::platform::run_job),
            ("page", crate::platform::in_page),
        ]
    }

    /// Work `files` of event `EV` fetched into `root` through `launch`
    /// (from this machine's test servers): how it ended, and what was told.
    fn fetch_into(
        root: &Path,
        files: Vec<File>,
        launch: Launch<Telling>,
    ) -> (Result<(PathBuf, usize), Fail>, Vec<String>) {
        let cx = Context::new(bmseeker::config::Config {
            local_hosts: true,
            ..Default::default()
        });
        let (tx, told) = std::sync::mpsc::channel();
        let (done, r) = std::sync::mpsc::channel();
        fetch_work(root, &cx, files, "EV", tx, launch, move |r| {
            let _ = done.send(r);
        });
        let r = r.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let told = (told.try_iter())
            .filter_map(|m| match m {
                Msg::Status(s) => Some(s),
                _ => None,
            })
            .collect();
        (r, told)
    }

    /// A song's archive, its readme, and (`addon`) an add-on archive that
    /// is none, each from a test server.
    fn song(addon: bool) -> Vec<File> {
        let tar = bmseeker::testkit::tar(&[("a.bms", "#TITLE A"), ("01.ogg", "o")]);
        let file = |n: &str, body: &[u8], original: &str| File {
            name: n.into(),
            url: served(body),
            original: original.into(),
            ..Default::default()
        };
        let mut files = vec![
            file("001.tar", &tar, "Song.tar"),
            file("002.txt", b"read me", "readme.txt"),
        ];
        if addon {
            files.push(file("003.zip", b"<html>", "addon.zip"));
        }
        files
    }

    /// A work's mirrored files come one after another, each after the
    /// first told, and are unpacked into its folder (an archive's files,
    /// an add-on as it is); nothing stays in the downloads. So as files
    /// natively, and as bytes in memory as on the web.
    #[test]
    fn a_work_is_fetched_and_unpacked() {
        for (how, launch) in launchers() {
            let root = std::env::temp_dir()
                .join(format!("vibebms-archive-got-{how}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let (r, told) = fetch_into(&root, song(false), launch);
            let dir = root.join("bms/EV/Song");
            let (rel, n) = match r {
                Ok(got) => got,
                Err(Fail::Msg(e) | Fail::Page(e)) => panic!("{how}: {e}"),
            };
            assert_eq!((root.join(rel), n), (dir.clone(), 3), "{how}");
            assert_eq!(std::fs::read(dir.join("a.bms")).unwrap(), b"#TITLE A");
            assert_eq!(std::fs::read(dir.join("readme.txt")).unwrap(), b"read me");
            assert_eq!(told, ["Downloading readme.txt…"], "{how}");
            assert_eq!(
                std::fs::read_dir(root.join("downloads")).unwrap().count(),
                0,
                "{how}"
            );
            std::fs::remove_dir_all(&root).ok();
        }
    }

    /// A work that fails takes back what it wrote: its folder when it made
    /// it, else the files it added, the player's own kept; trying again
    /// then works (a file of the failed try is not in the way).
    #[test]
    fn a_failed_work_is_taken_back() {
        for (how, launch) in launchers() {
            let root = std::env::temp_dir().join(format!(
                "vibebms-archive-failed-{how}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            let dir = root.join("bms/EV/Song");
            let failed = |r: Result<(PathBuf, usize), Fail>| match r {
                Err(Fail::Msg(e)) => e.contains("not an archive"),
                _ => false,
            };
            // a folder of its own
            assert!(failed(fetch_into(&root, song(true), launch).0), "{how}");
            assert!(!dir.exists(), "{how}");
            assert!(fetch_into(&root, song(false), launch).0.is_ok(), "{how}");
            assert!(dir.join("a.bms").is_file());
            // a folder that was there
            std::fs::remove_dir_all(&dir).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("mine.txt"), "mine").unwrap();
            assert!(failed(fetch_into(&root, song(true), launch).0), "{how}");
            let left: Vec<_> = (std::fs::read_dir(&dir).unwrap().flatten())
                .map(|e| e.file_name())
                .collect();
            assert_eq!(left, ["mine.txt"], "{how}");
            assert!(fetch_into(&root, song(false), launch).0.is_ok(), "{how}");
            assert!(dir.join("readme.txt").is_file() && dir.join("mine.txt").is_file());
            std::fs::remove_dir_all(&root).ok();
        }
    }

    #[test]
    fn reads_an_index() {
        let j = r#"[{"num": 42, "title": "T", "artist": "A", "team": "X", "kind": "Original",
            "page": "p", "files": [{"name": "042.zip", "url": "u", "size": 3, "original": "t.zip"}]},
            {"num": 21, "title": "U", "artist": "B", "team": "Y", "kind": "Original",
            "page": "q", "files": [], "links": ["https://1drv.ms/x"]}]"#;
        let w = mirror::parse_index(j.as_bytes()).unwrap();
        assert_eq!(w[0].files[0].original, "t.zip");
        assert_eq!(w[1].links.len(), 1);
    }

    /// What the mirror's reader refuses: an index with a work that has no
    /// number (it was work 0), or any larger than [`mirror::MAX_BYTES`];
    /// in a list, an entry given as an array or with a key twice (they
    /// were read), and one whose venue is no number (it was passed by).
    #[test]
    fn reads_only_what_the_schema_says() {
        assert!(mirror::parse_index(br#"[{"title": "T", "files": []}]"#).is_err());
        let big = format!("[{}{{\"num\": 1}}]", " ".repeat(mirror::MAX_BYTES));
        assert!(mirror::parse_index(big.as_bytes()).is_err());
        let j = r#"[["array", "A", "T", null, true, "https://i/a/index.json"],
            {"tag": "twice", "tag": "again", "index": "https://i/t/index.json"},
            {"tag": "venue", "venue": "151", "index": "https://i/v/index.json"},
            {"tag": "minus", "venue": -1, "index": "https://i/m/index.json"},
            {"tag": "kept", "venue": null, "index": "https://i/k/index.json"},
            {"tag": "numbered", "venue": 146, "index": "https://i/n/index.json"}]"#;
        let tags: Vec<String> = (mirror::parse_catalog(j.as_bytes()).unwrap())
            .into_iter()
            .map(|e| e.tag)
            .collect();
        assert_eq!(tags, ["kept", "numbered"]);
    }

    /// A list's index is asked for at a plain https address only, on any
    /// host (a copy of a list elsewhere names the mirror's indexes): not
    /// over http, with a user name, or with white space or a control
    /// character in or around it (the address checked is the one asked).
    /// The list says how many it left out.
    #[test]
    fn a_lists_indexes_are_plain_https() {
        let good = [
            format!("{MIRROR}/bof22/index.json"),
            "https://RAW.githubusercontent.com:443/vibeBMS/BMS/main/x/index.json".into(),
            "https://github.com/vibeBMS/BMS/releases/download/bof22/index.json".into(),
            "https://example.com/x/index.json".into(),
        ];
        let bad = [
            "http://raw.githubusercontent.com/vibeBMS/BMS/main/x/index.json",
            "http://192.168.1.1/x/index.json",
            "http://127.0.0.1/x/index.json",
            "https://user@example.com/x/index.json",
            "https://raw.githubusercontent.com@example.com/x/index.json",
            "https://example.com\\@b.example/x/index.json",
            " https://example.com/x/index.json",
            "https://example.com/x/index.json\n",
            "\u{3000}https://example.com/x/index.json",
            "https://example.com/x/index\u{a0}.json",
            "https://example.com/x/\tindex.json",
            "https://example.com/x/\u{0}index.json",
            "file:///etc/x/index.json",
            "/x/index.json",
        ];
        for u in &good {
            assert!(listed_index(u), "{u}");
        }
        for u in bad {
            assert!(!listed_index(u), "{u:?}");
        }

        let list = "https://lists.example/events.json";
        let entries = [good[2].as_str(), good[3].as_str(), bad[0], bad[6]];
        let entries = entries.map(|u| mirror::Event {
            tag: format!("e{}", u.len()),
            index: u.to_string(),
            ..Default::default()
        });
        let mut a = Archives {
            sources: vec![list.into()],
            catalogs: vec![Catalog {
                url: list.into(),
                events: entries.into(),
                fetched_at: 1,
            }],
            ..Default::default()
        };
        let paths = crate::paths::Paths::at(std::env::temp_dir().join("vibebms-archive-test-4"));
        assert_eq!(a.rebuild(&paths), [(list.to_string(), 2)]);
        let urls: Vec<(&str, bool)> = a
            .events
            .iter()
            .map(|e| (e.url.as_str(), e.listed))
            .collect();
        assert_eq!(urls, [(good[2].as_str(), true), (good[3].as_str(), true)]);
    }

    /// What `job` sends first.
    fn first(job: Job<Msg>) -> Msg {
        let mut jobs = vec![job];
        let start = std::time::Instant::now();
        loop {
            if let Some(m) = Job::drain_all(&mut jobs).into_iter().next() {
                return m;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(20));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Index `url` as fetched for a list (`local`: hosts of this machine
    /// allowed, as for the test servers).
    fn listed(url: &str, local: bool) -> Result<Vec<Work>, String> {
        let cx = Context::new(bmseeker::config::Config {
            local_hosts: local,
            ..Default::default()
        });
        match first(fetch_listed(url, &cx)) {
            Msg::Index(u, r) if u == url => r,
            _ => panic!("not the index's answer"),
        }
    }

    /// An index a list names is asked for as a download's links are: no
    /// address of this machine or its networks, and no redirect to an
    /// address that is no plain web link.
    #[test]
    fn a_listed_index_is_kept_to_the_downloads_rules() {
        let index = br#"[{"num": 1, "title": "W", "artist": "A", "team": "", "kind": "",
            "page": "p", "files": []}]"#;
        assert_eq!(listed(&served(index), true).unwrap()[0].title, "W");
        let refused = |r: Result<Vec<Work>, String>| r.is_err_and(|e| e.contains("local_hosts"));
        assert!(refused(listed(&served(index), false)));
        for u in [
            "https://127.0.0.1:9/x/index.json",
            "https://localhost/x/index.json",
            "https://192.168.1.1/x/index.json",
            "https://10.0.0.1/x/index.json",
            "https://169.254.169.254/x/index.json",
            "https://[::1]/x/index.json",
        ] {
            assert!(refused(listed(u, false)), "{u}");
        }
        // a redirect is kept to the same rules as the address asked
        let to = served(index).replacen("http://", "http://user@", 1);
        let moved = format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n");
        let (from, _) =
            crate::platform::http::tests::serve(vec![Box::leak(moved.into_boxed_str())]);
        let r = listed(&format!("{from}/f"), true);
        assert!(r.is_err_and(|e| e.contains("redirect")));
    }

    /// A list or an index of more than [`MAX_EVENTS`] events or
    /// [`MAX_WORKS`] works is not read, and the player is told; one cached
    /// is as none.
    #[test]
    fn a_list_or_an_index_too_long_is_not_read() {
        let root =
            std::env::temp_dir().join(format!("vibebms-archive-long-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = crate::paths::Paths::at(root.clone());
        let ev = format!("{MIRROR}/ev/index.json");
        let entry = mirror::Event {
            tag: "ev".into(),
            index: ev.clone(),
            ..Default::default()
        };
        let mut a = Archives {
            sources: vec![CATALOG.into()],
            catalogs: vec![Catalog {
                url: CATALOG.into(),
                events: vec![entry.clone()],
                fetched_at: 1,
            }],
            ..Default::default()
        };
        a.rebuild(&paths);
        let (events, works) = (
            vec![entry; MAX_EVENTS + 1],
            vec![Work::default(); MAX_WORKS + 1],
        );
        let sent = [
            Msg::Catalog(CATALOG.into(), Ok(events.clone())),
            Msg::Index(ev.clone(), Ok(works)),
        ];
        a.jobs.push(Job::start(|tx, _| {
            for m in sent {
                let _ = tx.send(m);
            }
        }));
        let cx = crate::net::hosts::context(&Default::default());
        let out = a.poll(&paths, &cx);
        assert_eq!(
            out,
            [
                Polled::Toast(format!(
                    "Event list {CATALOG}: more than 2000 events, not read"
                )),
                Polled::Toast(format!("Event index {ev}: more than 20000 works, not read")),
            ]
        );
        assert_eq!(
            (a.catalogs[0].events.len(), a.events[0].works.len()),
            (1, 0)
        );
        std::fs::create_dir_all(Archives::dir(&paths)).unwrap();
        let cached = Catalog {
            url: CATALOG.into(),
            events,
            fetched_at: 5,
        };
        let file = Archives::cache_file(&paths, CATALOG);
        std::fs::write(file, serde_json::to_vec(&cached).unwrap()).unwrap();
        let a = Archives::load(&paths, &[CATALOG.into()]);
        assert_eq!((a.catalogs[0].fetched_at, a.events.len()), (0, 0));
        std::fs::remove_dir_all(&root).ok();
    }

    /// A list fetched with entries left out says so, once for that list.
    #[test]
    fn a_list_tells_what_it_left_out() {
        let root =
            std::env::temp_dir().join(format!("vibebms-archive-told-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = crate::paths::Paths::at(root.clone());
        let (other, single) = (
            "https://lists.example/events.json".to_string(),
            "https://y/download/ok/index.json".to_string(),
        );
        let entry = |tag: &str, index: &str| mirror::Event {
            tag: tag.into(),
            index: index.into(),
            ..Default::default()
        };
        let mut a = Archives {
            sources: vec![CATALOG.into(), other.clone(), single],
            catalogs: [CATALOG, &other]
                .map(|u| Catalog {
                    url: u.into(),
                    ..Default::default()
                })
                .into(),
            ..Default::default()
        };
        let sent = [
            (
                CATALOG.to_string(),
                vec![
                    entry("ok", "https://a.invalid/ok/index.json"),
                    entry("plain", "http://a.invalid/p/index.json"),
                    entry("../up", "https://a.invalid/u/index.json"),
                ],
            ),
            (
                other.clone(),
                vec![entry("spaced", " https://a.invalid/s/index.json")],
            ),
        ];
        a.jobs.push(Job::start(|tx, _| {
            for (u, l) in sent {
                let _ = tx.send(Msg::Catalog(u, Ok(l)));
            }
        }));
        let cx = crate::net::hosts::context(&Default::default());
        let told: Vec<Polled> = (a.poll(&paths, &cx).into_iter())
            .filter(|p| matches!(p, Polled::Toast(_)))
            .collect();
        assert_eq!(
            told,
            [
                Polled::Toast(format!("Event list {CATALOG}: 2 events left out")),
                Polled::Toast(format!("Event list {other}: 1 events left out")),
            ]
        );
        let names: Vec<&str> = a.events.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["OK", "OK"]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// The list and indexes cached by an earlier version (its own types)
    /// read back; a cache that does not read is as none: fetched again.
    #[test]
    fn earlier_caches_load() {
        let root =
            std::env::temp_dir().join(format!("vibebms-archive-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = crate::paths::Paths::at(root.clone());
        std::fs::create_dir_all(Archives::dir(&paths)).unwrap();
        let (tt, nt) = (
            format!("{MIRROR}/boftt/index.json"),
            format!("{MIRROR}/bofnt/index.json"),
        );
        // (an index on another host than the list, as the mirror once was)
        let r22 = "https://github.com/vibeBMS/BMS/releases/download/bof22/index.json";
        let list = format!(
            r#"{{"url":"{CATALOG}","events":[{{"tag":"boftt","name":"BOF:TT","title":"T",
            "mirror":true,"index":"{tt}"}},{{"tag":"bofnt","name":"BOF:NT","title":"N",
            "mirror":false,"index":"{nt}"}},{{"tag":"bof22","name":"BOF22","title":"B",
            "mirror":true,"index":"{r22}"}}],"fetched_at":5}}"#
        );
        let index = format!(
            r#"{{"url":"{tt}","name":"BOFTT","label":"BOF:TT","title":"T","mirrored":true,
            "works":[{{"num":42,"title":"W","artist":"A","team":"X","kind":"Original","page":"p",
            "files":[{{"name":"042.zip","url":"u","size":3,"original":"w.zip"}}],"links":[],
            "addons":[]}}],"fetched_at":7}}"#
        );
        // (not written by any version: a work without its number)
        let broken = format!(r#"{{"url":"{nt}","works":[{{"title":"W"}}],"fetched_at":7}}"#);
        for (u, j) in [
            (CATALOG, &list),
            (tt.as_str(), &index),
            (nt.as_str(), &broken),
        ] {
            std::fs::write(Archives::cache_file(&paths, u), j).unwrap();
        }
        let a = Archives::load(&paths, &[CATALOG.into()]);
        assert_eq!(a.catalogs[0].fetched_at, 5);
        let got: Vec<(&str, &str, usize, u64)> = a
            .events
            .iter()
            .map(|e| (e.name.as_str(), e.label(), e.works.len(), e.fetched_at))
            .collect();
        assert_eq!(
            got,
            [
                ("BOFTT", "BOF:TT", 1, 7),
                ("BOFNT", "BOF:NT", 0, 0),
                ("BOF22", "BOF22", 0, 0)
            ]
        );
        assert_eq!(a.events[0].works[0].files[0].original, "w.zip");
        assert!(a.can_download(0, 42));
        std::fs::remove_dir_all(&root).ok();
    }
}
