//! Mirrored BMS event entries (github.com/vibeBMS/bms): each event's release
//! has an `index.json` listing its works and the archives that hold them
//! (or, for an event that is not mirrored, the authors' download links).
//! `events.json` in that repository lists the events. The music select
//! shows an event as a folder; a work not in the library downloads from the
//! mirror and unpacks into `bms/<EVENT>/` on choosing it, or opens its
//! author's download page.

use crate::app::AppEvent;
use crate::platform::Job;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::Sender;

/// The mirror's list of events.
pub const CATALOG: &str = "https://raw.githubusercontent.com/vibeBMS/BMS/main/events.json";

/// Mirrors read unless the configuration names others: a list of events
/// (`…/events.json`) or one event's `index.json`.
pub const DEFAULT_INDEXES: &[&str] = &[CATALOG];

/// The default before the list of events (BOF22 alone).
pub const OLD_DEFAULT: &str = "https://github.com/vibeBMS/BMS/releases/download/bof22/index.json";

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

// the web build tells no progress (Status)
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Msg {
    Catalog(String, Result<Vec<CatalogEntry>, String>),
    Index(String, Result<Vec<Work>, String>),
    Status(String),
    /// A work finished: (event, number, its folder relative to the data root).
    Done(String, u32, Result<(PathBuf, usize), Fail>),
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

    /// Download work `num` of event `ev` and unpack it into the library:
    /// the app on a thread, to disk; the web build through the vibeBMS
    /// proxy, in memory.
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
        let (root, cx) = (root.to_path_buf(), cx.clone());
        self.jobs.push(Job::start(|tx, _| {
            #[cfg(not(target_arch = "wasm32"))]
            crate::platform::spawn("archive", move || {
                let r = fetch_work(&root, &cx, &name, &files, &tx);
                let _ = tx.send(Msg::Done(name, num, r));
            });
            #[cfg(target_arch = "wasm32")]
            {
                let stem = work_stem(&files);
                let Some(rel) = work_dir(&name, &stem) else {
                    let _ = tx.send(Msg::Done(name, num, Err(NO_FOLDER.into())));
                    return;
                };
                let dir = root.join("bms").join(&name);
                web_fetch_files(files, dir, stem, &cx, move |r| {
                    let _ = tx.send(Msg::Done(name, num, r.map(|n| (rel, n))));
                });
            }
        }));
        Ok(if size > 0 {
            format!("Downloading {title} ({} MB)…", (size + (1 << 19)) >> 20)
        } else {
            format!("Downloading {title}…")
        })
    }

    /// What happened since the last frame.
    pub fn poll(&mut self, paths: &crate::paths::Paths) -> Vec<AppEvent> {
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
                Msg::Status(s) => out.push(AppEvent::Toast(s)),
                Msg::Done(event, num, r) => {
                    let k = key(&event, num);
                    self.busy.retain(|b| *b != k);
                    match r {
                        Ok((dir, n)) => {
                            out.push(AppEvent::Toast(tr!(
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
                            out.push(AppEvent::Rescan);
                            changed = true;
                        }
                        Err(Fail::Page(url)) => {
                            out.push(AppEvent::Toast(
                                tr!("No direct download: opening its page").into(),
                            ));
                            out.push(AppEvent::Browse(url));
                        }
                        Err(Fail::Msg(e)) => {
                            out.push(AppEvent::Toast(tr!("Download failed: {url}", url = e)))
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
            out.push(AppEvent::LibraryChanged);
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
    crate::platform::arcfs::archive_folder(main.map(String::as_str).unwrap_or(""))
        .unwrap_or_else(|| "work".into())
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

/// Download a work's files one after another (bmseeker's `Files` in the
/// page) and unpack them into `<dir>/<stem>/` (add-ons too); `done` gets
/// the number of files.
#[cfg(target_arch = "wasm32")]
fn web_fetch_files(
    files: Vec<File>,
    dir: PathBuf,
    stem: String,
    cx: &crate::hosts::Context,
    done: impl FnOnce(Result<usize, Fail>) + Send + 'static,
) {
    use bmseeker::install::job::Files;
    let job = Files::new(sources(&files), PathBuf::new(), cx);
    crate::platform::run_job(job, cx, move |r| {
        let got = match r {
            Ok(got) => got,
            Err(m) => return done(Err(missed(&files, m))),
        };
        let mut n = 0;
        for (f, g) in files.iter().zip(got) {
            let bmseeker::unpack::Blob::Bytes(bytes) = g.blob else {
                continue;
            };
            n += match bmseeker::unpack::sniff(&bytes) {
                // into the main archive's folder
                Some(format) => {
                    let name = format!("{stem}.{}", format.ext());
                    crate::platform::vfs::add_archive_at(&dir, &name, bytes)
                }
                // a file on its own (a BGA movie)
                None => {
                    let name = if f.original.is_empty() {
                        &f.name
                    } else {
                        &f.original
                    };
                    crate::platform::vfs::put_bytes(&dir.join(&stem).join(safe_name(name)), bytes);
                    1
                }
            };
        }
        done(if n > 0 {
            Ok(n)
        } else {
            Err(Fail::Msg("the archives hold nothing".into()))
        })
    });
}

/// Download a work's files (bmseeker's `Files`, a status for each after
/// the first) and unpack them into `bms/<event>/<archive>` (add-ons into
/// the same folder). Returns that folder (relative to the data root) and
/// the number of files written.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_work(
    root: &Path,
    cx: &crate::hosts::Context,
    event: &str,
    files: &[File],
    tx: &Sender<Msg>,
) -> Result<(PathBuf, usize), Fail> {
    use bmseeker::install::job::{run_job, Driver, Effect, Fed, Files};
    if files.is_empty() {
        return Err("no files".into());
    }
    let stem = work_stem(files);
    let rel = work_dir(event, &stem).ok_or(NO_FOLDER)?;
    let dest = root.join(&rel);
    let downloads = root.join("downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let name = |f: &File| {
        safe_name(if f.original.is_empty() {
            &f.name
        } else {
            &f.original
        })
    };
    let slots = downloads.join(format!(".{event}-{}", safe_name(&stem)));
    // the native transport, telling which file comes
    struct Telling<'a> {
        native: crate::hosts::Native,
        tx: &'a Sender<Msg>,
        slots: Vec<(PathBuf, String)>,
    }
    impl Driver for Telling<'_> {
        fn perform(&self, e: Effect) -> Fed {
            if let Effect::Download { slot, .. } = &e {
                if let Some((_, n)) = self.slots.iter().skip(1).find(|(s, _)| s == slot) {
                    let _ = self.tx.send(Msg::Status(format!("Downloading {n}…")));
                }
            }
            self.native.perform(e)
        }
    }
    let driver = Telling {
        native: crate::hosts::Native::new(cx),
        tx,
        slots: (0..files.len())
            .map(|i| (Files::slot(&slots, i), name(&files[i])))
            .collect(),
    };
    let tmps: Vec<PathBuf> = driver.slots.iter().map(|(s, _)| s.clone()).collect();
    let net = bmseeker::refusal::Guarded::new(&driver, cx);
    let got = run_job(&mut Files::new(sources(files), slots, cx), &net);
    let r = (|| {
        got.map_err(|m| missed(files, m))?;
        let mut n = 0;
        for (f, tmp) in files.iter().zip(&tmps) {
            let name = if f.original.is_empty() {
                // an author's link: an archive (its own bytes tell which), named
                // as one so that a file that is none is refused
                format!("{}.zip", name(f))
            } else {
                name(f)
            };
            n += crate::import::unpack_into(
                &bmseeker::fs::Disk,
                bmseeker::unpack::Blob::File(tmp.clone()),
                &name,
                &dest,
            )?;
        }
        Ok((rel, n))
    })();
    for t in &tmps {
        let _ = std::fs::remove_file(t);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(work_stem(&[file(". .zip")]), "work");
        assert_eq!(work_stem(&[file(" . . .7z")]), "work");
        assert_eq!(work_stem(&[file("...zip")]), "zip");
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
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_work_is_not_fetched_outside_bms() {
        let root = std::env::temp_dir().join(format!("vibebms-archive-out-{}", std::process::id()));
        let cx = crate::hosts::context(&crate::config::Config::default());
        let (tx, _rx) = std::sync::mpsc::channel();
        let files = [File {
            name: "001.zip".into(),
            original: "Song.zip".into(),
            ..Default::default()
        }];
        let r = fetch_work(&root.join("data"), &cx, "../..", &files, &tx);
        assert!(matches!(r, Err(Fail::Msg(e)) if e == NO_FOLDER));
        assert!(!root.exists());
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
