//! Skins downloaded in the app (vibeBMS bundles none of other people's).
//!
//! `skins/catalog.json` in the vibeBMS repository lists skins the authors
//! publish for download; the copy compiled in is used until that is read.
//! Choosing one downloads its archives as songs are downloaded
//! (`bmseeker::hosts`: Google Drive, Dropbox, MEGA, plain servers; the web
//! build through the vibeBMS proxy), unpacks them and places the skin below
//! `<root>/skins/`, where the settings find it.
//!
//! An LR2 skin names its files `LR2files\Theme\<folder>\...`, and those
//! paths are read from `skins/`: its files go to `skins/<folder>/` (an
//! archive of a whole `LR2files` keeps its themes' names). A beatoraja
//! skin's paths are its own: it goes to `skins/<folder>/` too.

use crate::archive::{safe_name, Fail};
use bmseeker::progress::Progress;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Where the catalog is read from.
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/vibeBMS/vibe/main/skins/catalog.json";

/// The catalog this build came with.
const BUILTIN: &str = include_str!("../../../skins/catalog.json");

/// Read the catalog again after this long (s).
const REFRESH_S: u64 = 86_400;

/// The largest skin the web build unpacks (it holds the archive and the
/// files in memory at once).
pub const WEB_MAX_BYTES: u64 = 400 << 20;

/// The record of installed skins, in the skins folder (no extension: not
/// taken for a skin).
const RECORD: &str = ".vibebms-installed";

/// A skin in the catalog.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinEntry {
    pub id: String,
    pub name: String,
    pub author: String,
    /// "lr2" or "beatoraja".
    pub kind: String,
    /// A beatoraja skin written in Lua (`.luaskin`): not in the web build.
    pub lua: bool,
    /// See [`SCENES`].
    pub scenes: Vec<String>,
    pub resolution: String,
    pub version: String,
    /// Bytes to download (0: not known).
    pub size: u64,
    /// Its archives, unpacked one after another into its folder.
    pub downloads: Vec<String>,
    pub homepage: String,
    /// The author's terms, as stated.
    pub license: String,
    /// Its folder below `skins/` (an LR2 skin: the theme name its files use).
    pub folder: String,
}

/// The scene names a catalog entry may list.
pub const SCENES: &[&str] = &[
    "select",
    "decide",
    "play5",
    "play7",
    "play9",
    "play10",
    "play14",
    "play24",
    "play48",
    "result",
    "course_result",
];

impl SkinEntry {
    /// Its folder's name below `skins/`.
    pub fn folder(&self) -> String {
        safe_name(if self.folder.is_empty() {
            &self.id
        } else {
            &self.folder
        })
    }

    /// "SELECT · DECIDE · 7/14K · RESULT"
    pub fn scenes_label(&self) -> String {
        let has = |s: &str| self.scenes.iter().any(|x| x == s);
        let mut out: Vec<String> = Vec::new();
        for (s, l) in [("select", "SELECT"), ("decide", "DECIDE")] {
            if has(s) {
                out.push(l.into());
            }
        }
        let keys: Vec<&str> = self
            .scenes
            .iter()
            .filter_map(|s| s.strip_prefix("play"))
            .collect();
        if !keys.is_empty() {
            out.push(format!("{}K", keys.join("/")));
        }
        for (s, l) in [("result", "RESULT"), ("course_result", "COURSE")] {
            if has(s) {
                out.push(l.into());
            }
        }
        out.join(" · ")
    }

    /// Why the web build cannot install it, if it cannot.
    pub fn web_refusal(&self) -> Option<&'static str> {
        if self.lua {
            Some("a Lua skin: it needs the desktop app")
        } else if self.size > WEB_MAX_BYTES {
            Some("too large for the browser: use the desktop app")
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub version: u32,
    pub skins: Vec<SkinEntry>,
}

impl Catalog {
    /// A catalog's JSON, with what is wrong in it left out (an entry with
    /// no id or no download).
    pub fn parse(bytes: &[u8]) -> Result<Catalog, String> {
        let mut c: Catalog = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let mut seen = std::collections::HashSet::new();
        c.skins.retain(|s| {
            !s.id.is_empty()
                && !s.downloads.is_empty()
                && matches!(s.kind.as_str(), "lr2" | "beatoraja")
                && seen.insert(s.id.clone())
        });
        Ok(c)
    }

    pub fn builtin() -> Catalog {
        Catalog::parse(BUILTIN.as_bytes()).unwrap_or_default()
    }
}

/// What an install left: its version and the folders below `skins/`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Installed {
    pub version: String,
    pub folders: Vec<String>,
}

/// A catalog entry's state.
pub enum State {
    Absent,
    Busy(Arc<Progress>),
    /// In `skins/`; `update`: the catalog has another version.
    Installed {
        update: bool,
    },
}

enum Msg {
    Catalog(Result<Catalog, String>),
    Done(String, Result<Vec<String>, Fail>),
}

#[derive(Default)]
pub struct SkinStore {
    pub catalog: Catalog,
    /// When the catalog was read from [`CATALOG_URL`] (s; 0: not yet).
    fetched_at: u64,
    fetching: bool,
    /// id -> its install.
    pub installed: BTreeMap<String, Installed>,
    busy: Vec<(String, Arc<Progress>)>,
    /// A row pressed once that a second press confirms: (id, delete).
    pub armed: Option<(String, bool)>,
    tx: Option<Sender<Msg>>,
    rx: Option<Receiver<Msg>>,
}

/// What [`SkinStore::poll`] reports.
#[derive(Default)]
pub struct Polled {
    pub toasts: Vec<String>,
    /// Skins were added or replaced.
    pub changed: bool,
    /// Pages to open (a link that is not a file).
    pub pages: Vec<String>,
}

fn cache_file(paths: &crate::paths::Paths) -> PathBuf {
    paths.cache.join("skin-catalog.json")
}

#[derive(Serialize, Deserialize)]
struct Cached {
    fetched_at: u64,
    catalog: Catalog,
}

impl SkinStore {
    /// The cached catalog (else the one compiled in) and the record of
    /// installed skins.
    pub fn load(paths: &crate::paths::Paths) -> SkinStore {
        let cached = crate::platform::vfs::read(&cache_file(paths))
            .ok()
            .and_then(|b| serde_json::from_slice::<Cached>(&b).ok())
            .filter(|c| !c.catalog.skins.is_empty());
        let (catalog, fetched_at) = match cached {
            Some(c) => (c.catalog, c.fetched_at),
            None => (Catalog::builtin(), 0),
        };
        let installed = crate::platform::vfs::read(&paths.skins.join(RECORD))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        SkinStore {
            catalog,
            fetched_at,
            installed,
            ..Default::default()
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

    /// Read the catalog again if it is a day old (now with `force`).
    pub fn refresh(&mut self, force: bool) {
        let now = crate::platform::unix_now();
        if self.fetching || (!force && now.saturating_sub(self.fetched_at) < REFRESH_S) {
            return;
        }
        self.fetching = true;
        let tx = self.channel();
        // (GitHub's raw files send CORS headers: no proxy on the web)
        crate::platform::http_get(CATALOG_URL, move |r| {
            let _ = tx.send(Msg::Catalog(r.and_then(|b| Catalog::parse(&b))));
        });
    }

    pub fn is_fetching(&self) -> bool {
        self.fetching
    }

    fn save_record(&self, paths: &crate::paths::Paths) {
        if let Ok(j) = serde_json::to_vec_pretty(&self.installed) {
            let _ = crate::platform::vfs::write(&paths.skins.join(RECORD), &j);
        }
    }

    pub fn state(&self, skins: &Path, e: &SkinEntry) -> State {
        if let Some((_, p)) = self.busy.iter().find(|(id, _)| *id == e.id) {
            return State::Busy(p.clone());
        }
        match self.installed.get(&e.id) {
            Some(i)
                if i.folders
                    .iter()
                    .any(|f| crate::platform::vfs::is_dir(&skins.join(f))) =>
            {
                State::Installed {
                    update: !e.version.is_empty() && i.version != e.version,
                }
            }
            // put there by hand (or the record lost): installing replaces it
            _ if crate::platform::vfs::is_dir(&skins.join(e.folder())) => {
                State::Installed { update: false }
            }
            _ => State::Absent,
        }
    }

    /// Stop `id`'s download (true if it was downloading).
    pub fn cancel(&mut self, id: &str) -> bool {
        match self.busy.iter().find(|(b, _)| b == id) {
            Some((_, p)) => {
                p.cancel();
                true
            }
            None => false,
        }
    }

    /// Download catalog entry `idx` and install it (replacing what it
    /// left before). Returns what to tell the player.
    pub fn install(&mut self, paths: &crate::paths::Paths, idx: usize) -> Result<String, String> {
        let e = self.catalog.skins.get(idx).cloned().ok_or("no such skin")?;
        if self.busy.iter().any(|(id, _)| *id == e.id) {
            return Err(format!("{} is downloading already", e.name));
        }
        if crate::platform::IS_WEB {
            if let Some(why) = e.web_refusal() {
                return Err(format!("{}: {why}", e.name));
            }
        }
        let old = self.old_folders(&e);
        let progress = Progress::new();
        self.busy.push((e.id.clone(), progress.clone()));
        let tx = self.channel();
        let size = if e.size > 0 {
            format!(" ({} MB)", (e.size + (1 << 19)) >> 20)
        } else {
            String::new()
        };
        let msg = format!("Downloading {}{size}…", e.name);
        let id = e.id.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (root, skins) = (paths.root.clone(), paths.skins.clone());
            crate::platform::spawn("skin", move || {
                let r = bmseeker::progress::scope(Some(progress), || {
                    fetch_and_install(&root, &skins, &e, &old)
                });
                let _ = tx.send(Msg::Done(id, r));
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let skins = paths.skins.clone();
            web_install(e, skins, old, progress, 0, Vec::new(), move |r| {
                let _ = tx.send(Msg::Done(id, r));
            });
        }
        Ok(msg)
    }

    /// The folders an entry's install replaces: those it left last time,
    /// and its own folder.
    fn old_folders(&self, e: &SkinEntry) -> Vec<String> {
        let mut v = self
            .installed
            .get(&e.id)
            .map(|i| i.folders.clone())
            .unwrap_or_default();
        if !v.contains(&e.folder()) {
            v.push(e.folder());
        }
        v
    }

    /// Delete entry `idx`'s folders. Returns them (below `skins/`).
    pub fn delete(
        &mut self,
        paths: &crate::paths::Paths,
        idx: usize,
    ) -> Result<Vec<String>, String> {
        let e = self.catalog.skins.get(idx).cloned().ok_or("no such skin")?;
        if self.busy.iter().any(|(id, _)| *id == e.id) {
            return Err(format!("{} is downloading", e.name));
        }
        let folders = self.old_folders(&e);
        for f in &folders {
            remove_folder(&paths.skins, f);
        }
        self.installed.remove(&e.id);
        self.save_record(paths);
        Ok(folders)
    }

    /// What happened since the last frame.
    pub fn poll(&mut self, paths: &crate::paths::Paths) -> Polled {
        let msgs: Vec<Msg> = self
            .rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        let mut out = Polled::default();
        for m in msgs {
            match m {
                Msg::Catalog(r) => {
                    self.fetching = false;
                    match r {
                        Ok(c) if !c.skins.is_empty() => {
                            self.fetched_at = crate::platform::unix_now();
                            let cached = Cached {
                                fetched_at: self.fetched_at,
                                catalog: c,
                            };
                            if let Ok(j) = serde_json::to_vec(&cached) {
                                let _ = crate::platform::vfs::write(&cache_file(paths), &j);
                            }
                            self.catalog = cached.catalog;
                        }
                        Ok(_) => log::warn!("skin catalog: empty"),
                        // the copy at hand stays (offline, or not published yet)
                        Err(e) => log::warn!("skin catalog: {e}"),
                    }
                }
                Msg::Done(id, r) => {
                    self.busy.retain(|(b, _)| *b != id);
                    let e = self.catalog.skins.iter().find(|s| s.id == id);
                    let name = e.map(|e| e.name.clone()).unwrap_or_else(|| id.clone());
                    match r {
                        Ok(folders) => {
                            out.toasts.push(format!(
                                "Installed {name} in skins/{}: choose it in DISPLAY",
                                folders.join(", skins/")
                            ));
                            let version = e.map(|e| e.version.clone()).unwrap_or_default();
                            self.installed.insert(id, Installed { version, folders });
                            self.save_record(paths);
                            out.changed = true;
                        }
                        Err(Fail::Page(url)) => {
                            out.toasts
                                .push(format!("{name}: no direct download, opening its page"));
                            out.pages.push(url);
                        }
                        Err(Fail::Msg(e)) if e == bmseeker::progress::CANCELLED => {
                            out.toasts.push(format!("{name}: cancelled"))
                        }
                        Err(Fail::Msg(e)) => {
                            out.toasts.push(format!("{name}: download failed ({e})"))
                        }
                    }
                }
            }
        }
        out
    }
}

/// Remove folder `name` below `skins` (a single name: never anything else).
fn remove_folder(skins: &Path, name: &str) {
    if name.is_empty() || safe_name(name) != name {
        return;
    }
    let dir = skins.join(name);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[cfg(target_arch = "wasm32")]
    if crate::platform::vfs::is_dir(&dir) {
        crate::platform::vfs::remove_dir(&dir);
    }
}

/// Files an archive may hold that are not the skin's.
fn junk(parts: &[String]) -> bool {
    parts.iter().any(|p| p == "__MACOSX")
        || parts.last().is_some_and(|n| {
            let n = n.to_ascii_lowercase();
            n == ".ds_store" || n == "thumbs.db" || n == "desktop.ini"
        })
}

fn is_skin_file(p: &Path) -> bool {
    p.extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        matches!(e.as_str(), "lr2skin" | "json" | "luaskin")
    })
}

/// Where each of an archive's files goes, below `skins/`: (path in the
/// archive, path placed). An `LR2files/Theme/<name>/` package gives its
/// themes (and drops the rest of `LR2files`: sounds, BGM); anything else
/// goes into the entry's folder, without the folders all its files share
/// (`skin-main/`, `skin/<name>/`; an LR2 skin: the top one only).
pub fn place(entry: &SkinEntry, files: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
    let parts: Vec<Vec<String>> = files
        .iter()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect()
        })
        .collect();
    let keep: Vec<usize> = (0..files.len())
        .filter(|&i| !parts[i].is_empty() && !junk(&parts[i]))
        .collect();
    let lr2 = entry.kind == "lr2";
    let theme_at = |c: &[String]| {
        c.windows(2)
            .position(|w| {
                w[0].eq_ignore_ascii_case("lr2files") && w[1].eq_ignore_ascii_case("theme")
            })
            .map(|i| i + 2)
            // (an LR2 skin's `Theme` folder alone)
            .or_else(|| (lr2 && c[0].eq_ignore_ascii_case("theme")).then_some(1))
            .filter(|&at| c.len() > at + 1)
    };
    let themed: Vec<(PathBuf, PathBuf)> = keep
        .iter()
        .filter_map(|&i| {
            let at = theme_at(&parts[i])?;
            let mut to = PathBuf::from(safe_name(&parts[i][at]));
            to.extend(&parts[i][at + 1..]);
            Some((files[i].clone(), to))
        })
        .collect();
    if !themed.is_empty() {
        return themed;
    }
    // the folders every file is in (an LR2 skin: only the archive's own
    // folder, as its files' paths start at its theme folder)
    let mut common = keep.iter().map(|&i| parts[i].len() - 1).min().unwrap_or(0);
    if lr2 {
        common = common.min(1);
    }
    if let Some(&first) = keep.first() {
        for &i in &keep {
            let same = parts[i]
                .iter()
                .zip(&parts[first])
                .take(common)
                .take_while(|(a, b)| a == b)
                .count();
            common = common.min(same);
        }
    }
    let folder = entry.folder();
    keep.iter()
        .map(|&i| {
            let mut to = PathBuf::from(&folder);
            to.extend(&parts[i][common..]);
            (files[i].clone(), to)
        })
        .collect()
}

/// The top folders of placed files, in order.
fn top_folders(placed: &[(PathBuf, PathBuf)]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (_, to) in placed {
        if let Some(Component::Normal(f)) = to.components().next() {
            let f = f.to_string_lossy().into_owned();
            if !v.contains(&f) {
                v.push(f);
            }
        }
    }
    v
}

/// Every file below `dir`, relative to it.
#[cfg(not(target_arch = "wasm32"))]
fn files_below(dir: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(base, &p, out),
                Ok(t) if t.is_file() => {
                    if let Ok(r) = p.strip_prefix(base) {
                        out.push(r.to_path_buf());
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Unpack `archives` (each into its own folder of `staging`) and put the
/// skin they hold below `skins`, the folders `old` removed first. Returns
/// the folders below `skins` it is in.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_files(
    archives: &[PathBuf],
    skins: &Path,
    staging: &Path,
    entry: &SkinEntry,
    old: &[String],
) -> Result<Vec<String>, String> {
    let _ = std::fs::remove_dir_all(staging);
    let r = install_staged(archives, skins, staging, entry, old);
    let _ = std::fs::remove_dir_all(staging);
    r
}

#[cfg(not(target_arch = "wasm32"))]
fn install_staged(
    archives: &[PathBuf],
    skins: &Path,
    staging: &Path,
    entry: &SkinEntry,
    old: &[String],
) -> Result<Vec<String>, String> {
    let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (i, a) in archives.iter().enumerate() {
        let ext = crate::hosts::sniff(a).ok_or("the download is not an archive")?;
        let dir = staging.join(i.to_string());
        crate::import::unpack_into(a, &format!("skin.{ext}"), &dir)?;
        let placed = place(entry, &files_below(&dir));
        moves.extend(placed.into_iter().map(|(from, to)| (dir.join(from), to)));
    }
    if !moves.iter().any(|(_, to)| is_skin_file(to)) {
        return Err("the archive holds no skin".into());
    }
    let folders = top_folders(&moves);
    for f in old.iter().chain(&folders) {
        remove_folder(skins, f);
    }
    for (from, to) in &moves {
        let to = skins.join(to);
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        if std::fs::rename(from, &to).is_err() {
            std::fs::copy(from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok(folders)
}

/// Download entry `e`'s archives (this thread's progress tells how far)
/// and install them.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_and_install(
    root: &Path,
    skins: &Path,
    e: &SkinEntry,
    old: &[String],
) -> Result<Vec<String>, Fail> {
    use crate::hosts::{context, fetch, Fetch};
    let downloads = root.join("downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let stem = format!(".skin-{}", safe_name(&e.id));
    let mut got = Vec::new();
    let r = (|| {
        for (i, url) in e.downloads.iter().enumerate() {
            let tmp = downloads.join(format!("{stem}-{i}.part"));
            got.push(tmp.clone());
            match fetch(url, None, &tmp, context()) {
                Ok(()) => {}
                Err(Fetch::Page) => return Err(Fail::Page(url.clone())),
                Err(Fetch::Failed(m)) => return Err(Fail::Msg(m)),
            }
        }
        bmseeker::progress::stage(bmseeker::progress::Stage::Unpacking);
        install_files(&got, skins, &downloads.join(&stem), e, old).map_err(Fail::Msg)
    })();
    for f in &got {
        let _ = std::fs::remove_file(f);
    }
    r
}

/// The web build: entry `e`'s archives one after another through the
/// vibeBMS proxy, unpacked in memory; then the old folders go and the
/// files are kept in the browser's storage.
#[cfg(target_arch = "wasm32")]
fn web_install(
    e: SkinEntry,
    skins: PathBuf,
    old: Vec<String>,
    progress: Arc<Progress>,
    i: usize,
    mut placed: Vec<(PathBuf, Vec<u8>)>,
    done: impl FnOnce(Result<Vec<String>, Fail>) + Send + 'static,
) {
    let Some(url) = e.downloads.get(i).cloned() else {
        if !placed.iter().any(|(to, _)| is_skin_file(to)) {
            return done(Err(Fail::Msg("the archive holds no skin".into())));
        }
        let pairs: Vec<(PathBuf, PathBuf)> = placed
            .iter()
            .map(|(to, _)| (PathBuf::new(), to.clone()))
            .collect();
        let folders = top_folders(&pairs);
        for f in old.iter().chain(&folders) {
            remove_folder(&skins, f);
        }
        for (to, data) in placed {
            crate::platform::vfs::store(&skins.join(to), data);
        }
        return done(Ok(folders));
    };
    let p = progress.clone();
    crate::archive::web_fetch(url, None, Some(progress.clone()), move |r| match r {
        Err(f) => done(Err(f)),
        Ok(bytes) => {
            p.set_stage(bmseeker::progress::Stage::Unpacking);
            let mut files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
            if let Err(m) =
                bmseeker::unpack::unpack_bytes(bytes, |rel, data| files.push((rel, data)))
            {
                return done(Err(Fail::Msg(m)));
            }
            let names: Vec<PathBuf> = files.iter().map(|(r, _)| r.clone()).collect();
            let mut by: std::collections::HashMap<PathBuf, Vec<u8>> = files.into_iter().collect();
            for (from, to) in place(&e, &names) {
                if let Some(data) = by.remove(&from) {
                    placed.push((to, data));
                }
            }
            web_install(e, skins, old, progress, i + 1, placed, done)
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, folder: &str) -> SkinEntry {
        SkinEntry {
            id: "t".into(),
            name: "T".into(),
            kind: kind.into(),
            folder: folder.into(),
            downloads: vec!["https://example.com/t.zip".into()],
            ..Default::default()
        }
    }

    #[test]
    fn the_catalog_compiled_in_is_whole() {
        let raw: serde_json::Value = serde_json::from_str(BUILTIN).unwrap();
        let n = raw["skins"].as_array().unwrap().len();
        let c = Catalog::builtin();
        // nothing left out as broken
        assert_eq!(c.skins.len(), n);
        assert!(n > 0);
        for s in &c.skins {
            assert!(!s.name.is_empty() && !s.author.is_empty(), "{}", s.id);
            assert!(
                s.downloads.iter().all(|u| u.starts_with("https://")),
                "{}",
                s.id
            );
            assert!(s.homepage.starts_with("https://"), "{}", s.id);
            assert!(
                s.scenes.iter().all(|x| SCENES.contains(&x.as_str())),
                "{}",
                s.id
            );
            assert_eq!(s.folder(), s.folder, "{}", s.id);
        }
    }

    #[test]
    fn reads_a_catalog() {
        let j = br#"{"version": 1, "skins": [
            {"id": "a", "name": "A", "author": "x", "kind": "beatoraja", "lua": true,
             "scenes": ["select", "play7", "play14", "result"], "size": 3,
             "downloads": ["https://h/a.zip"], "folder": "A dir", "future": 1},
            {"id": "b", "name": "B", "kind": "lr2", "downloads": []},
            {"id": "c", "name": "C", "kind": "other", "downloads": ["https://h/c.zip"]},
            {"id": "a", "name": "A again", "kind": "lr2", "downloads": ["https://h/a2.zip"]},
            {"id": "d", "name": "D", "kind": "lr2", "downloads": ["https://h/d.zip"]}
        ]}"#;
        let c = Catalog::parse(j).unwrap();
        let ids: Vec<&str> = c.skins.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a", "d"]);
        assert_eq!(c.skins[0].scenes_label(), "SELECT · 7/14K · RESULT");
        assert_eq!(c.skins[0].folder(), "A dir");
        assert_eq!(c.skins[1].folder(), "d");
        assert!(c.skins[0].web_refusal().is_some() && c.skins[1].web_refusal().is_none());
        assert!(Catalog::parse(b"<html>").is_err());
    }

    fn placed(e: &SkinEntry, files: &[&str]) -> Vec<(String, String)> {
        let files: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
        place(e, &files)
            .into_iter()
            .map(|(a, b)| {
                let s = |p: PathBuf| p.to_string_lossy().replace('\\', "/");
                (s(a), s(b))
            })
            .collect()
    }

    #[test]
    fn places_a_beatoraja_skin_without_its_shared_folders() {
        let e = entry("beatoraja", "Brook");
        let p = placed(
            &e,
            &[
                "skin/brook/play/play7.json",
                "skin/brook/select/select.json",
                "__MACOSX/skin/._play7.json",
                "skin/brook/Thumbs.db",
            ],
        );
        assert_eq!(
            p,
            [
                (
                    "skin/brook/play/play7.json".into(),
                    "Brook/play/play7.json".into()
                ),
                (
                    "skin/brook/select/select.json".into(),
                    "Brook/select/select.json".into()
                ),
            ]
        );
        // files at the top: nothing to drop
        let p = placed(&e, &["Select/s.json", "fonts/a.ttf", "readme.txt"]);
        assert_eq!(p[0].1, "Brook/Select/s.json");
        assert_eq!(p[2].1, "Brook/readme.txt");
    }

    #[test]
    fn places_an_lr2_skin_under_its_theme_name() {
        // a repository archive: its folder takes the theme's name
        let e = entry("lr2", "Hex_Simple_FHD");
        let p = placed(&e, &["lr2-simple-skin-master/Result/result.lr2skin"]);
        assert_eq!(p[0].1, "Hex_Simple_FHD/Result/result.lr2skin");
        // a whole LR2files: its themes, nothing else
        let p = placed(
            &entry("lr2", "EC"),
            &[
                "LR2files/Theme/EndlessCirculation/Play/play.lr2skin",
                "LR2files/Theme/Other/a.png",
                "LR2files/Sound/EC/a.wav",
                "readme.txt",
            ],
        );
        let to: Vec<&str> = p.iter().map(|(_, b)| b.as_str()).collect();
        assert_eq!(to, ["EndlessCirculation/Play/play.lr2skin", "Other/a.png"]);
    }

    /// A zip of `files` (stored, no compression).
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut dir) = (Vec::new(), Vec::new());
        let le16 = |v: &mut Vec<u8>, n: u16| v.extend_from_slice(&n.to_le_bytes());
        let le32 = |v: &mut Vec<u8>, n: u32| v.extend_from_slice(&n.to_le_bytes());
        for (name, data) in files {
            let mut crc = flate2::Crc::new();
            crc.update(data);
            let off = out.len() as u32;
            for (v, central) in [(&mut out, false), (&mut dir, true)] {
                le32(v, if central { 0x0201_4b50 } else { 0x0403_4b50 });
                if central {
                    le16(v, 20);
                }
                for n in [20u16, 0x800, 0, 0, 0] {
                    le16(v, n);
                }
                for n in [crc.sum(), data.len() as u32, data.len() as u32] {
                    le32(v, n);
                }
                le16(v, name.len() as u16);
                le16(v, 0);
                if central {
                    for n in [0u16, 0, 0] {
                        le16(v, n);
                    }
                    le32(v, 0);
                    le32(v, off);
                }
                v.extend_from_slice(name.as_bytes());
                if !central {
                    v.extend_from_slice(data);
                }
            }
        }
        let (at, size) = (out.len() as u32, dir.len() as u32);
        out.extend_from_slice(&dir);
        le32(&mut out, 0x0605_4b50);
        for n in [0u16, 0, files.len() as u16, files.len() as u16] {
            le16(&mut out, n);
        }
        le32(&mut out, size);
        le32(&mut out, at);
        le16(&mut out, 0);
        out
    }

    #[test]
    fn installs_an_archive_and_replaces_it() {
        let base = std::env::temp_dir().join(format!("vibebms-skinstore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (skins, staging) = (base.join("skins"), base.join("downloads/.skin-t"));
        std::fs::create_dir_all(&skins).unwrap();
        // a skin of the player's own stays
        std::fs::create_dir_all(skins.join("Mine")).unwrap();
        std::fs::write(skins.join("Mine/play.json"), "{}").unwrap();
        let a = base.join("a.zip");
        std::fs::write(
            &a,
            zip(&[
                ("repo-main/play/play7.json", b"{\"type\":0}"),
                ("repo-main/play/old.png", b"x"),
                ("repo-main/README.md", b"r"),
            ]),
        )
        .unwrap();
        let e = entry("beatoraja", "Repo");
        let got = install_files(std::slice::from_ref(&a), &skins, &staging, &e, &[]).unwrap();
        assert_eq!(got, ["Repo"]);
        assert_eq!(
            std::fs::read(skins.join("Repo/play/play7.json")).unwrap(),
            b"{\"type\":0}"
        );
        assert!(skins.join("Repo/README.md").is_file());
        assert!(!staging.exists());
        // the new version: what the old one had and this has not is gone
        std::fs::write(
            &a,
            zip(&[
                ("repo-main/play/play7.json", b"{\"type\":0,\"v\":2}"),
                ("repo-main/README.md", b"r"),
            ]),
        )
        .unwrap();
        install_files(std::slice::from_ref(&a), &skins, &staging, &e, &got).unwrap();
        assert!(!skins.join("Repo/play/old.png").exists());
        assert!(skins.join("Repo/play/play7.json").is_file());
        assert!(skins.join("Mine/play.json").is_file());
        // no skin in it: nothing changes
        std::fs::write(&a, zip(&[("x/readme.txt", b"r")])).unwrap();
        assert!(install_files(std::slice::from_ref(&a), &skins, &staging, &e, &got).is_err());
        assert!(skins.join("Repo/play/play7.json").is_file());
        // not an archive
        std::fs::write(&a, b"<html>").unwrap();
        assert!(install_files(&[a], &skins, &staging, &e, &got).is_err());
        std::fs::remove_dir_all(&base).ok();
    }

    /// Download and install a catalog entry for real:
    /// `VIBEBMS_SKIN=<id> cargo test -p vibebms skin_download -- --ignored --nocapture`
    #[test]
    #[ignore = "downloads a skin"]
    fn skin_download() {
        let id = std::env::var("VIBEBMS_SKIN").expect("VIBEBMS_SKIN");
        let root = std::env::var("VIBEBMS_SKIN_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("vibebms-skin-download"));
        let e = Catalog::builtin()
            .skins
            .into_iter()
            .find(|s| s.id == id)
            .expect("no such id");
        let skins = root.join("skins");
        std::fs::create_dir_all(&skins).unwrap();
        let p = Progress::new();
        let r = bmseeker::progress::scope(Some(p.clone()), || {
            fetch_and_install(&root, &skins, &e, &[e.folder()])
        });
        match r {
            Ok(folders) => {
                println!("INSTALLED {folders:?}");
                let found = crate::skin::lr2::find_skins(&skins);
                for (path, t, name) in &found {
                    println!("SKIN {t:?} {name} {}", path.display());
                }
                assert!(!found.is_empty(), "no skin found after install");
            }
            Err(Fail::Page(u)) => panic!("a page: {u}"),
            Err(Fail::Msg(m)) => panic!("failed: {m}"),
        }
    }
}
