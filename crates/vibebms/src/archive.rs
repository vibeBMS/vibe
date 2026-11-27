//! Mirrored BMS event entries (github.com/vibeBMS/bms): each event's release
//! has an `index.json` listing its works and the archives that hold them
//! (or, for an event that is not mirrored, the authors' download links).
//! `events.json` in that repository lists the events. The music select
//! shows an event as a folder; a work not in the library downloads from the
//! mirror and unpacks into `bms/<EVENT>/` on choosing it, or opens its
//! author's download page.

use crate::platform::Job;
use bmseeker::install::job::{Effect, Fed, Files, Got, Missed, Step};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Re-read an index after this long (s).
const REFRESH_S: u64 = 86_400;

/// Re-read a list of events after this long (s): it is small, and the
/// URLs of the indexes in it may change.
const CATALOG_REFRESH_S: u64 = 600;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct File {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// The file name the author published.
    pub original: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Work {
    pub num: u32,
    pub title: String,
    pub artist: String,
    pub team: String,
    pub kind: String,
    pub page: String,
    /// The main archive first, then add-ons.
    pub files: Vec<File>,
    /// The authors' download links (works the mirror could not copy), in
    /// the order to try them (the index puts those found to bring the song
    /// first).
    pub links: Vec<String>,
    /// Links found to hold add-ons for the song (charts without their
    /// sounds: 差分), to put into its folder.
    pub addons: Vec<String>,
}

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

/// An event in `events.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogEntry {
    pub tag: String,
    pub name: String,
    pub title: String,
    pub mirror: bool,
    pub index: String,
}

/// A list of events (`events.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub url: String,
    pub events: Vec<CatalogEntry>,
    pub fetched_at: u64,
}

/// The events of a list (`events.json`). An entry that does not read (a
/// field of another type, a null) is left out, not the whole list; one
/// without an index is kept and skipped later. Fields the app does not
/// use (`venue`, `date`) may be missing or null.
pub fn parse_catalog(bytes: &[u8]) -> Result<Vec<CatalogEntry>, String> {
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    Ok(entries
        .into_iter()
        .filter_map(|e| serde_json::from_value(e).ok())
        .collect())
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
    Catalog(String, Result<Vec<CatalogEntry>, String>),
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
    /// single indexes (their works from the cache).
    fn rebuild(&mut self, paths: &crate::paths::Paths) {
        let mut old = std::mem::take(&mut self.events);
        let mut wanted: Vec<Event> = Vec::new();
        for u in &self.sources {
            if is_catalog(u) {
                let Some(c) = self.catalogs.iter().find(|c| &c.url == u) else {
                    continue;
                };
                for e in c.events.iter().filter(|e| !e.index.is_empty()) {
                    let Some(name) = folder_name(&e.tag) else {
                        log::warn!("archive events {u}: {:?} is no folder name", e.tag);
                        continue;
                    };
                    wanted.push(Event {
                        url: e.index.clone(),
                        name,
                        label: e.name.clone(),
                        title: e.title.clone(),
                        mirrored: e.mirror,
                        ..Default::default()
                    });
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
        for w in wanted {
            if self.events.iter().any(|e| e.url == w.url) {
                continue;
            }
            // the works: from the last run, else from the cache
            let cached = match old.iter().position(|e| e.url == w.url) {
                Some(i) => Some(old.swap_remove(i)),
                None => crate::platform::vfs::read(&Self::cache_file(paths, &w.url))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Event>(&b).ok()),
            };
            let (works, fetched_at) = cached.map(|c| (c.works, c.fetched_at)).unwrap_or_default();
            self.events.push(Event {
                works,
                fetched_at,
                ..w
            });
        }
    }

    /// Fetch the lists of events and the indexes older than a day (all
    /// with `force`).
    pub fn refresh(&mut self, force: bool) {
        let now = crate::platform::unix_now();
        let lists: Vec<String> = self
            .catalogs
            .iter()
            .filter(|c| force || now.saturating_sub(c.fetched_at) >= CATALOG_REFRESH_S)
            .map(|c| c.url.clone())
            .collect();
        for url in lists {
            let u = url.clone();
            self.jobs.push(Job::get(&url, move |r| {
                Msg::Catalog(u, r.and_then(|b| parse_catalog(&b)))
            }));
        }
        let todo: Vec<String> = self
            .events
            .iter()
            .filter(|e| force || now.saturating_sub(e.fetched_at) >= REFRESH_S)
            .map(|e| e.url.clone())
            .collect();
        for url in todo {
            let u = url.clone();
            self.jobs.push(Job::get(&url, move |r| {
                let r = r.and_then(|b| {
                    serde_json::from_slice::<Vec<Work>>(&b).map_err(|e| e.to_string())
                });
                Msg::Index(u, r)
            }));
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
        cx: &crate::hosts::Context,
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
            fetch_work(root, cx, files, &event, tx, move |r| {
                let _ = done.send(Msg::Done(name, num, r));
            })
        }));
        Ok(if size > 0 {
            format!("Downloading {title} ({} MB)…", (size + (1 << 19)) >> 20)
        } else {
            format!("Downloading {title}…")
        })
    }

    /// What happened since the last frame.
    pub fn poll(&mut self, paths: &crate::paths::Paths) -> Vec<Polled> {
        let msgs = Job::drain_all(&mut self.jobs);
        let mut out = Vec::new();
        let (mut changed, mut new_events) = (false, false);
        for m in msgs {
            match m {
                Msg::Catalog(url, Ok(entries)) => {
                    if let Some(c) = self.catalogs.iter_mut().find(|c| c.url == url) {
                        c.events = entries;
                        c.fetched_at = crate::platform::unix_now();
                        if let Ok(j) = serde_json::to_vec(c) {
                            let _ = crate::platform::vfs::create_dir_all(&Self::dir(paths));
                            let _ = crate::platform::vfs::write(&Self::cache_file(paths, &url), &j);
                        }
                        new_events = true;
                    }
                }
                Msg::Catalog(url, Err(e)) => log::warn!("archive events {url}: {e}"),
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
        if new_events {
            self.rebuild(paths);
            // the events new in the list
            self.refresh(false);
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
    use crate::hosts::Fetch;
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
                let _ = self.tx.send(Msg::Status(format!("Downloading {n}…")));
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

/// Download a work's files (bmseeker's `Files`, a status for each after
/// the first: `tx`) and unpack them into `bms/<event>/<archive>` (add-ons
/// into the same folder); then `done` with that folder (relative to the
/// data root) and the number of files written. The files come into their
/// slots below `downloads/` natively, into memory on the web
/// ([`crate::platform::run_job`]); either is unpacked into the player's
/// files ([`crate::platform::Vfs`]).
fn fetch_work(
    root: &Path,
    cx: &crate::hosts::Context,
    files: Vec<File>,
    event: &str,
    tx: Sender<Msg>,
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
    crate::platform::run_job(job, cx, move |got| {
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
                n += bmseeker::import::unpack_into(&crate::platform::Vfs, g.blob, &name, &dest)?;
            }
            Ok((rel, n))
        })();
        for t in &tmps {
            let _ = crate::platform::vfs::remove_file(t);
        }
        done(r)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CATALOG, OLD_DEFAULT};

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
            "mirror": true, "index": "https://x/boftt/index.json"},
            {"tag": "bofnt", "name": "BOF:NT", "title": "N", "venue": 142,
            "mirror": false, "index": "https://x/bofnt/index.json"}]"#;
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
            "date": "2026-05-01", "mirror": false, "index": "https://x/mumei2026/index.json"},
            {"tag": "dpbms2014", "name": "DPBMS差分大会", "title": "合作！DPBMS差分大会",
            "venue": null, "mirror": true, "index": "https://x/dpbms2014/index.json"},
            {"tag": "pabat", "title": "P", "index": "https://x/pabat/index.json"},
            {"tag": "bad", "name": null, "index": "https://x/bad/index.json"},
            {"tag": "noindex", "name": "N"}]"#;
        let events = parse_catalog(j.as_bytes()).unwrap();
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
        assert!(parse_catalog(b"{}").is_err());
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
        let entry = |(i, tag): (usize, &str)| CatalogEntry {
            tag: tag.into(),
            index: format!("https://x/{i}/index.json"),
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
        let cx = crate::hosts::context(&crate::config::Config::default());
        let (tx, _rx) = std::sync::mpsc::channel();
        let files = vec![File {
            name: "001.zip".into(),
            original: "Song.zip".into(),
            ..Default::default()
        }];
        let (done, r) = std::sync::mpsc::channel();
        fetch_work(&root.join("data"), &cx, files, "../..", tx, move |r| {
            let _ = done.send(r);
        });
        let r = r.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(matches!(r, Err(Fail::Msg(e)) if e == NO_FOLDER));
        assert!(!root.exists());
    }

    /// A work's mirrored files come one after another, each after the
    /// first told, and are unpacked into its folder (an archive's files,
    /// an add-on as it is); nothing stays in the downloads.
    #[test]
    fn a_work_is_fetched_and_unpacked() {
        let root = std::env::temp_dir().join(format!("vibebms-archive-got-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let tar = bmseeker::testkit::tar(&[("a.bms", "#TITLE A"), ("01.ogg", "o")]);
        let answer = |body: &[u8]| -> &'static str {
            let body = std::str::from_utf8(body).unwrap();
            let head = "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length";
            Box::leak(format!("{head}: {}\r\n\r\n{body}", body.len()).into_boxed_str())
        };
        let serve = |body: &[u8]| {
            let (url, _) = crate::platform::http::tests::serve(vec![answer(body)]);
            format!("{url}/f")
        };
        let files = vec![
            File {
                name: "001.tar".into(),
                url: serve(&tar),
                original: "Song.tar".into(),
                ..Default::default()
            },
            File {
                name: "002.txt".into(),
                url: serve(b"read me"),
                original: "readme.txt".into(),
                ..Default::default()
            },
        ];
        // (the test's server is on this machine)
        let cx = crate::hosts::Context::new(bmseeker::config::Config {
            local_hosts: true,
            ..Default::default()
        });
        let (tx, told) = std::sync::mpsc::channel();
        let (done, r) = std::sync::mpsc::channel();
        fetch_work(&root, &cx, files, "EV", tx, move |r| {
            let _ = done.send(r);
        });
        let r = r.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let dir = root.join("bms/EV/Song");
        let (rel, n) = match r {
            Ok(got) => got,
            Err(Fail::Msg(e) | Fail::Page(e)) => panic!("{e}"),
        };
        assert_eq!((root.join(rel), n), (dir.clone(), 3));
        assert_eq!(std::fs::read(dir.join("a.bms")).unwrap(), b"#TITLE A");
        assert_eq!(std::fs::read(dir.join("readme.txt")).unwrap(), b"read me");
        let told: Vec<String> = told
            .try_iter()
            .filter_map(|m| match m {
                Msg::Status(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(told, ["Downloading readme.txt…"]);
        assert_eq!(
            std::fs::read_dir(root.join("downloads")).unwrap().count(),
            0
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reads_an_index() {
        let j = r#"[{"num": 42, "title": "T", "artist": "A", "team": "X", "kind": "Original",
            "page": "p", "files": [{"name": "042.zip", "url": "u", "size": 3, "original": "t.zip"}]},
            {"num": 21, "title": "U", "artist": "B", "team": "Y", "kind": "Original",
            "page": "q", "files": [], "links": ["https://1drv.ms/x"]}]"#;
        let w: Vec<Work> = serde_json::from_str(j).unwrap();
        assert_eq!(w[0].files[0].original, "t.zip");
        assert_eq!(w[1].links.len(), 1);
    }
}
