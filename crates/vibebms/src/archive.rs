//! Mirrored BMS event entries (github.com/vibeBMS/bms): each event's release
//! has an `index.json` listing its works and the archives that hold them
//! (or, for an event that is not mirrored, the authors' download links).
//! `events.json` in that repository lists the events. The music select
//! shows an event as a folder; a work not in the library downloads from the
//! mirror and unpacks into `bms/<EVENT>/` on choosing it, or opens its
//! author's download page.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

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

/// Whether a configured URL is a list of events.
pub fn is_catalog(url: &str) -> bool {
    url.trim_end_matches('/').ends_with("events.json")
}

/// The event name of an index URL (`.../download/bof22/index.json`).
pub fn event_name(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .nth(1)
        .unwrap_or("archive")
        .to_uppercase()
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
    tx: Option<Sender<Msg>>,
    rx: Option<Receiver<Msg>>,
}

fn key(event: &str, num: u32) -> String {
    format!("{event}/{num}")
}

fn now_s() -> u64 {
    crate::platform::SystemTime::now()
        .duration_since(crate::platform::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
                    wanted.push(Event {
                        url: e.index.clone(),
                        name: e.tag.to_uppercase(),
                        label: e.name.clone(),
                        title: e.title.clone(),
                        mirrored: e.mirror,
                        ..Default::default()
                    });
                }
            } else {
                wanted.push(Event {
                    url: u.clone(),
                    name: event_name(u),
                    mirrored: true,
                    ..Default::default()
                });
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

    fn channel(&mut self) -> Sender<Msg> {
        if self.tx.is_none() {
            let (tx, rx) = channel();
            self.tx = Some(tx);
            self.rx = Some(rx);
        }
        self.tx.clone().expect("channel")
    }

    /// Fetch the lists of events and the indexes older than a day (all
    /// with `force`).
    pub fn refresh(&mut self, force: bool) {
        let now = now_s();
        let lists: Vec<String> = self
            .catalogs
            .iter()
            .filter(|c| force || now.saturating_sub(c.fetched_at) >= CATALOG_REFRESH_S)
            .map(|c| c.url.clone())
            .collect();
        for url in lists {
            let tx = self.channel();
            let u = url.clone();
            crate::platform::http_get(&url, move |r| {
                let r = r.and_then(|b| {
                    serde_json::from_slice::<Vec<CatalogEntry>>(&b).map_err(|e| e.to_string())
                });
                let _ = tx.send(Msg::Catalog(u, r));
            });
        }
        let todo: Vec<String> = self
            .events
            .iter()
            .filter(|e| force || now.saturating_sub(e.fetched_at) >= REFRESH_S)
            .map(|e| e.url.clone())
            .collect();
        for url in todo {
            let tx = self.channel();
            let u = url.clone();
            crate::platform::http_get(&url, move |r| {
                let r = r.and_then(|b| {
                    serde_json::from_slice::<Vec<Work>>(&b).map_err(|e| e.to_string())
                });
                let _ = tx.send(Msg::Index(u, r));
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

    /// Download work `num` of event `ev` and unpack it into the library:
    /// the app on a thread, to disk; the web build through the vibeBMS
    /// proxy, in memory.
    pub fn download(&mut self, root: &Path, ev: usize, num: u32) -> Result<String, String> {
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
        let tx = self.channel();
        let root = root.to_path_buf();
        #[cfg(not(target_arch = "wasm32"))]
        crate::platform::spawn("archive", move || {
            let r = fetch_work(&root, &name, &files, &tx);
            let _ = tx.send(Msg::Done(name, num, r));
        });
        #[cfg(target_arch = "wasm32")]
        {
            let stem = work_stem(&files);
            let rel = PathBuf::from("bms").join(&name).join(&stem);
            let dir = root.join("bms").join(&name);
            web_fetch_files(files, dir, stem, move |r| {
                let _ = tx.send(Msg::Done(name, num, r.map(|n| (rel, n))));
            });
        }
        Ok(if size > 0 {
            format!("Downloading {title} ({} MB)…", (size + (1 << 19)) >> 20)
        } else {
            format!("Downloading {title}…")
        })
    }

    /// Messages since the last frame: (toasts, the library changed, the
    /// indexes changed, pages to open in the browser).
    #[allow(clippy::type_complexity)]
    pub fn poll(&mut self, paths: &crate::paths::Paths) -> (Vec<String>, bool, bool, Vec<String>) {
        let msgs: Vec<Msg> = self
            .rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        let (mut toasts, mut rescan, mut changed) = (Vec::new(), false, false);
        let mut pages = Vec::new();
        let mut new_events = false;
        for m in msgs {
            match m {
                Msg::Catalog(url, Ok(entries)) => {
                    if let Some(c) = self.catalogs.iter_mut().find(|c| c.url == url) {
                        c.events = entries;
                        c.fetched_at = now_s();
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
                        ev.fetched_at = now_s();
                        if let Ok(j) = serde_json::to_vec(ev) {
                            let _ = crate::platform::vfs::create_dir_all(&Self::dir(paths));
                            let _ = crate::platform::vfs::write(&Self::cache_file(paths, &url), &j);
                        }
                        changed = true;
                    }
                }
                Msg::Index(url, Err(e)) => log::warn!("archive index {url}: {e}"),
                Msg::Status(s) => toasts.push(s),
                Msg::Done(event, num, r) => {
                    let k = key(&event, num);
                    self.busy.retain(|b| *b != k);
                    match r {
                        Ok((dir, n)) => {
                            toasts.push(format!("Added {} ({n} files)", dir.display()));
                            self.installed.insert(k, dir);
                            if let Ok(j) = serde_json::to_vec_pretty(&self.installed) {
                                let _ = crate::platform::vfs::create_dir_all(&Self::dir(paths));
                                let _ = crate::platform::vfs::write(
                                    &Self::dir(paths).join("installed.json"),
                                    &j,
                                );
                            }
                            rescan = true;
                            changed = true;
                        }
                        Err(Fail::Page(url)) => {
                            toasts.push("No direct download: opening its page".into());
                            pages.push(url);
                        }
                        Err(Fail::Msg(e)) => toasts.push(format!("Download failed: {e}")),
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
        (toasts, rescan, changed, pages)
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
    Path::new(&safe_name(main.map(String::as_str).unwrap_or("")))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "work".into())
}

/// A work's files as bmseeker's `Files` takes them: a mirror's file as it
/// is, an author's link through the hosts.
fn sources(files: &[File]) -> Vec<bmseeker::install::job::Source> {
    files
        .iter()
        .map(|f| bmseeker::install::job::Source {
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
    done: impl FnOnce(Result<usize, Fail>) + Send + 'static,
) {
    use bmseeker::install::job::Files;
    let job =
        Files::new(sources(&files), PathBuf::new(), crate::hosts::context()).failures_are_pages();
    crate::platform::run_job(job, None, move |r| {
        let got = match r {
            Ok(got) => got,
            Err(m) => return done(Err(missed(&files, m))),
        };
        let mut n = 0;
        for (f, g) in files.iter().zip(got) {
            let bmseeker::unpack::Blob::Bytes(bytes) = g.blob else {
                continue;
            };
            n += match crate::hosts::sniff_bytes(&bytes) {
                // into the main archive's folder
                Some(ext) => {
                    crate::platform::vfs::add_archive_at(&dir, &format!("{stem}.{ext}"), bytes)
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

/// The archive `link` leads to (for the song `want`): bmseeker's
/// `Acquire` run in the page (`platform::run_job`: its requests and the
/// file itself through the proxy, reporting to `progress`, if any). A
/// file that does not come gives way to the next of the links its page
/// named; a server's passing failure is tried again; what the proxy
/// refuses (not an archive) is a page.
#[cfg(target_arch = "wasm32")]
pub(crate) fn web_fetch(
    link: String,
    want: Option<crate::hosts::Want>,
    progress: Option<std::sync::Arc<bmseeker::progress::Progress>>,
    done: impl FnOnce(Result<Vec<u8>, Fail>) + Send + 'static,
) {
    use crate::hosts::Fetch;
    use bmseeker::install::job::Acquire;
    let cx = match &progress {
        Some(p) => crate::hosts::context().with_progress(p.clone()),
        None => crate::hosts::context().clone(),
    };
    let job = Acquire::new(&link, want, false, PathBuf::new(), &cx).failures_are_pages();
    crate::platform::run_job(job, progress, move |r| {
        done(match r {
            Ok(got) => match got.blob {
                bmseeker::unpack::Blob::Bytes(b) => Ok(b),
                bmseeker::unpack::Blob::File(_) => Err(Fail::Msg("not in memory".into())),
            },
            Err(Fetch::Page) => Err(Fail::Page(link)),
            Err(Fetch::Failed(e)) => Err(e.into()),
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
    event: &str,
    files: &[File],
    tx: &Sender<Msg>,
) -> Result<(PathBuf, usize), Fail> {
    use bmseeker::install::job::{run_job, Driver, Effect, Fed, Files};
    let downloads = root.join("downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    if files.is_empty() {
        return Err("no files".into());
    }
    let stem = work_stem(files);
    let rel = PathBuf::from("bms").join(event).join(&stem);
    let dest = root.join(&rel);
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
    let cx = crate::hosts::context();
    let driver = Telling {
        native: crate::hosts::Native::new(cx),
        tx,
        slots: (0..files.len())
            .map(|i| (Files::slot(&slots, i), name(&files[i])))
            .collect(),
    };
    let tmps: Vec<PathBuf> = driver.slots.iter().map(|(s, _)| s.clone()).collect();
    let got = run_job(&mut Files::new(sources(files), slots, cx), &driver);
    let r = (|| {
        got.map_err(|m| missed(files, m))?;
        let mut n = 0;
        for (f, tmp) in files.iter().zip(&tmps) {
            let name = if f.original.is_empty() {
                // an author's link: its own bytes tell the format
                let ext = crate::hosts::sniff(tmp).unwrap_or("zip");
                format!("{}.{ext}", name(f))
            } else {
                name(f)
            };
            n += crate::import::unpack_into(tmp, &name, &dest)?;
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
            "BOF22"
        );
        assert_eq!(safe_name("a/b:c.zip"), "a_b_c.zip");
        assert_eq!(safe_name(".."), "download");
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
