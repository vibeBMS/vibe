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
    /// Original download pages (works the mirror could not copy).
    pub links: Vec<String>,
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

enum Msg {
    Catalog(String, Result<Vec<CatalogEntry>, String>),
    Index(String, Result<Vec<Work>, String>),
    Status(String),
    /// A work finished: (event, number, its folder relative to the data root).
    Done(String, u32, Result<(PathBuf, usize), String>),
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
            .filter(|c| force || now.saturating_sub(c.fetched_at) >= REFRESH_S)
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

    /// Download work `num` of event `ev` and unpack it into the library.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download(&mut self, root: &Path, ev: usize, num: u32) -> Result<String, String> {
        let event = self.events.get(ev).ok_or("no such event")?;
        let work = event
            .works
            .iter()
            .find(|w| w.num == num)
            .ok_or("no such work")?;
        if work.files.is_empty() {
            return Err("not mirrored: download it from its page".into());
        }
        let k = key(&event.name, num);
        if self.busy.contains(&k) {
            return Err("already downloading".into());
        }
        self.busy.push(k);
        let (name, files, title) = (event.name.clone(), work.files.clone(), work.title.clone());
        let size: u64 = files.iter().map(|f| f.size).sum();
        let tx = self.channel();
        let root = root.to_path_buf();
        crate::platform::spawn("archive", move || {
            let r = fetch_work(&root, &name, &files, &tx);
            let _ = tx.send(Msg::Done(name, num, r));
        });
        Ok(format!(
            "Downloading {title} ({} MB)…",
            (size + (1 << 19)) >> 20
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub fn download(&mut self, _root: &Path, _ev: usize, _num: u32) -> Result<String, String> {
        Err("downloads need the desktop or mobile app".into())
    }

    /// Messages since the last frame: (toasts, the library changed, the
    /// indexes changed).
    pub fn poll(&mut self, paths: &crate::paths::Paths) -> (Vec<String>, bool, bool) {
        let msgs: Vec<Msg> = self
            .rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        let (mut toasts, mut rescan, mut changed) = (Vec::new(), false, false);
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
                        Err(e) => toasts.push(format!("Download failed: {e}")),
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
        (toasts, rescan, changed)
    }
}

/// `name` made safe as a single file name.
fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "download".into()
    } else {
        s
    }
}

/// Stream `url` into `out`.
#[cfg(not(target_arch = "wasm32"))]
fn download_to(url: &str, out: &Path) -> Result<(), String> {
    use std::io::Write;
    use std::ops::ControlFlow;
    use std::sync::{Arc, Mutex};
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    // the file being written, then how the download ended
    type State = (
        Option<std::io::BufWriter<std::fs::File>>,
        Option<Result<(), String>>,
    );
    let state: Arc<Mutex<State>> =
        Arc::new(Mutex::new((Some(std::io::BufWriter::new(file)), None)));
    let st = state.clone();
    ehttp::streaming::fetch_streaming_blocking(
        ehttp::Request::get(url),
        Box::new(move |part| {
            let mut g = st.lock().expect("download state");
            match part {
                Err(e) => {
                    g.1 = Some(Err(e));
                    ControlFlow::Break(())
                }
                Ok(ehttp::streaming::Part::Response(r)) if !r.ok => {
                    g.1 = Some(Err(format!("HTTP {}", r.status)));
                    ControlFlow::Break(())
                }
                Ok(ehttp::streaming::Part::Response(_)) => ControlFlow::Continue(()),
                Ok(ehttp::streaming::Part::Chunk(c)) if c.is_empty() => {
                    let r = g.0.take().map(|mut w| w.flush()).transpose();
                    g.1 = Some(r.map(|_| ()).map_err(|e| e.to_string()));
                    ControlFlow::Break(())
                }
                Ok(ehttp::streaming::Part::Chunk(c)) => {
                    if let Some(Err(e)) = g.0.as_mut().map(|w| w.write_all(&c)) {
                        g.1 = Some(Err(e.to_string()));
                        return ControlFlow::Break(());
                    }
                    ControlFlow::Continue(())
                }
            }
        }),
    );
    let r = state.lock().expect("download state").1.take();
    r.unwrap_or_else(|| Err("the download ended early".into()))
}

/// Download a work's files and unpack them into `bms/<event>/<archive>`
/// (add-ons into the same folder). Returns that folder (relative to the
/// data root) and the number of files written.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_work(
    root: &Path,
    event: &str,
    files: &[File],
    tx: &Sender<Msg>,
) -> Result<(PathBuf, usize), String> {
    let downloads = root.join("downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let main = files.first().ok_or("no files")?;
    let original = if main.original.is_empty() {
        &main.name
    } else {
        &main.original
    };
    let stem = Path::new(&safe_name(original))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "work".into());
    let rel = PathBuf::from("bms").join(event).join(&stem);
    let dest = root.join(&rel);
    let mut n = 0;
    for (i, f) in files.iter().enumerate() {
        let name = safe_name(if f.original.is_empty() {
            &f.name
        } else {
            &f.original
        });
        let tmp = downloads.join(format!(".{event}-{}", safe_name(&f.name)));
        if i > 0 {
            let _ = tx.send(Msg::Status(format!("Downloading {name}…")));
        }
        download_to(&f.url, &tmp)?;
        n += crate::import::unpack_into(&tmp, &name, &dest)?;
        let _ = std::fs::remove_file(&tmp);
    }
    Ok((rel, n))
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
