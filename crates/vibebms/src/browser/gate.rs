//! What a page in the in-app browser may save. Any page shown there can
//! start a download by itself (a link from a table, an uploader's
//! advertisement, a redirect), and what the browser saved used to be
//! unpacked into the library at once.
//!
//! A download goes into a slot: a file in the downloads folder under a
//! name of the app's own, one at a time. When it has arrived whole and
//! within the size limit the player is asked about it, by its name, the
//! host it came from and its size: a yes gives it its name (it is then
//! brought into the library), a no deletes it. Nothing is remembered: the
//! next download is asked about again. A slot is deleted when its download
//! fails, when the browser closes, and at the next start if it was left.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long after a refusal its end may be told (a web view tells it at
/// once, where it tells it at all).
const REFUSAL_TOLD: Duration = Duration::from_secs(10);

/// What a slot's file name starts and ends with.
const SLOT: (&str, &str) = (".browser-", ".part");

/// A download in its slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    /// Where it comes from ([`host`]).
    pub host: String,
    /// Its file name (a safe one), which it gets on a yes.
    pub name: String,
    /// The slot.
    pub path: PathBuf,
    /// Its size once it has arrived (bytes).
    pub size: u64,
}

/// What became of the download that ended.
#[derive(Debug, PartialEq)]
pub enum Arrival {
    /// It is in its slot: the player is asked ([`Gate::asked`]).
    Ask,
    /// Larger than a download may be: deleted (its name).
    TooLarge(String),
    /// It failed: its slot is deleted.
    Failed,
    /// A download that was refused, or none of ours.
    Other,
}

#[derive(Default)]
pub struct Gate {
    /// The download being saved, and its address.
    saving: Option<(String, Saved)>,
    /// The one that arrived, until the player answers.
    asked: Option<Saved>,
    /// Downloads refused whose end the web view has not told yet.
    refused: Vec<(String, Instant)>,
    /// Slots made (their names' number).
    slots: u32,
    /// The folder the slots are made in.
    dir: Option<PathBuf>,
}

/// The host a download at `url` comes from (a `blob:` address: its
/// page's), for the player to judge it by; an address without one: its
/// scheme.
pub fn host(url: &str) -> String {
    let inner = url.strip_prefix("blob:").unwrap_or(url);
    let rest = match inner.split_once("://") {
        Some((scheme, rest)) if ["http", "https"].contains(&scheme) => rest,
        _ => return format!("{}:", url.split(':').next().unwrap_or("")),
    };
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = match host.strip_prefix('[').and_then(|h| h.split_once(']')) {
        Some((v6, _)) => v6,
        None => host.split(':').next().unwrap_or(""),
    };
    host.to_lowercase()
}

/// `dir/name`, or `dir/name (2)` etc. when that exists.
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|k| dir.join(format!("{stem} ({k}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(p)
}

/// Delete the slots left in `dir` (by a run that ended while a download
/// was under way or waited for its answer).
pub fn clear_slots(dir: &Path) {
    remove_slots(dir, SLOT.0);
}

/// Delete the slots in `dir` whose names start with `prefix`.
fn remove_slots(dir: &Path, prefix: &str) {
    let Ok(files) = std::fs::read_dir(dir) else {
        return;
    };
    for f in files.flatten() {
        let name = f.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && name.ends_with(SLOT.1) {
            let _ = std::fs::remove_file(f.path());
        }
    }
}

impl Gate {
    /// A page starts a download of `name` (a safe name) from `url`: the
    /// slot in `dir` to save it in, or None when it is refused (another is
    /// being saved or waits for its answer).
    pub fn started(&mut self, url: &str, name: &str, dir: &Path, now: Instant) -> Option<PathBuf> {
        if self.saving.is_some() || self.asked.is_some() {
            self.refused.push((url.to_string(), now));
            return None;
        }
        self.slots += 1;
        let slot = format!("{}{}{}", self.prefix(), self.slots, SLOT.1);
        self.dir = Some(dir.to_path_buf());
        let saved = Saved {
            host: host(url),
            name: name.to_string(),
            path: dir.join(slot),
            size: 0,
        };
        self.refused.retain(|(u, _)| u != url);
        self.saving = Some((url.to_string(), saved.clone()));
        Some(saved.path)
    }

    /// The web view says the download from `url` ended (`ok`: whole). The
    /// one being saved is kept for the player's answer when it is whole
    /// and no larger than `limit` bytes, else deleted.
    pub fn ended(&mut self, url: &str, ok: bool, limit: u64, now: Instant) -> Arrival {
        self.refused.retain(|(_, at)| now < *at + REFUSAL_TOLD);
        // A refused download never ends whole: a whole end for the address
        // being saved is the saved one's, even when that address was also
        // refused (pressed twice) and that refusal's end is not told.
        let saving = self.saving.as_ref().is_some_and(|(u, _)| u == url);
        let refused = self.refused.iter().position(|(u, _)| u == url);
        if let Some(i) = refused.filter(|_| !(ok && saving)) {
            self.refused.swap_remove(i);
            return Arrival::Other;
        }
        let Some((_, mut saved)) = self.saving.take_if(|(u, _)| u == url) else {
            return Arrival::Other;
        };
        let size = std::fs::metadata(&saved.path).map(|m| m.len());
        match size {
            Ok(size) if ok && size <= limit => {
                saved.size = size;
                self.asked = Some(saved);
                Arrival::Ask
            }
            size => {
                let _ = std::fs::remove_file(&saved.path);
                match size {
                    Ok(_) if ok => Arrival::TooLarge(saved.name),
                    _ => Arrival::Failed,
                }
            }
        }
    }

    /// What the names of this run's slots start with.
    fn prefix(&self) -> String {
        format!("{}{}-", SLOT.0, std::process::id())
    }

    /// The download the player is asked about.
    pub fn asked(&self) -> Option<&Saved> {
        self.asked.as_ref()
    }

    /// The player's answer. A yes: the file under its name in `dir`, for
    /// the library; a no: it is deleted.
    pub fn answer(&mut self, yes: bool, dir: &Path) -> Option<PathBuf> {
        let saved = self.asked.take()?;
        let to = free_path(dir, &saved.name);
        if yes && std::fs::rename(&saved.path, &to).is_ok() {
            return Some(to);
        }
        let _ = std::fs::remove_file(&saved.path);
        None
    }
}

/// The browser is gone (closed, or the game ends): so are the download
/// under way, the one not answered, and any slot a web view wrote after
/// its download was told ended.
impl Drop for Gate {
    fn drop(&mut self) {
        if let Some(dir) = &self.dir {
            remove_slots(dir, &self.prefix());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://files.example/get/Song.zip?t=1";
    const LIMIT: u64 = 100;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibebms-gate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The names of the files in `dir`.
    fn files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = (std::fs::read_dir(dir).unwrap().flatten())
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    /// A download a page starts, as the web view saves it: `bytes` in the
    /// slot it was given.
    fn save(g: &mut Gate, dir: &Path, url: &str, name: &str, bytes: usize) -> PathBuf {
        let slot = g.started(url, name, dir, Instant::now()).unwrap();
        std::fs::write(&slot, vec![0u8; bytes]).unwrap();
        slot
    }

    /// A download is saved under a name of the app's own and gets the
    /// page's only on a yes: nothing is there for the library before.
    #[test]
    fn nothing_is_kept_without_a_yes() {
        let dir = temp("yes");
        let t = Instant::now();
        let mut g = Gate::default();
        let slot = save(&mut g, &dir, URL, "Song.zip", 7);
        assert_eq!(slot.parent(), Some(dir.as_path()));
        let slot_name = slot.file_name().unwrap().to_string_lossy().into_owned();
        assert!(slot_name.starts_with(".browser-") && !slot_name.contains("Song"));
        assert_eq!(g.asked(), None);
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Ask);
        let asked = g.asked().unwrap().clone();
        assert_eq!(
            (asked.host.as_str(), asked.name.as_str(), asked.size),
            ("files.example", "Song.zip", 7)
        );
        assert_eq!(files(&dir), [slot_name]);
        // no: deleted
        assert_eq!(g.answer(false, &dir), None);
        assert!(files(&dir).is_empty());
        // the same again is asked about again; yes: its name
        save(&mut g, &dir, URL, "Song.zip", 7);
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Ask);
        assert_eq!(g.answer(true, &dir), Some(dir.join("Song.zip")));
        assert_eq!(files(&dir), ["Song.zip"]);
        // a file of that name is not written over
        save(&mut g, &dir, URL, "Song.zip", 7);
        g.ended(URL, true, LIMIT, t);
        assert_eq!(g.answer(true, &dir), Some(dir.join("Song (2).zip")));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// One at a time: while one is being saved or waits for its answer, a
    /// page's other downloads are refused, and their ends do not end it.
    #[test]
    fn one_download_at_a_time() {
        let dir = temp("one");
        let t = Instant::now();
        let ad = "https://ad.example/a.exe";
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Song.zip", 7);
        assert_eq!(g.started(ad, "a.exe", &dir, t), None);
        // pressed twice: the second is refused, the first goes on
        assert_eq!(g.started(URL, "Song.zip", &dir, t), None);
        assert_eq!(g.ended(ad, false, LIMIT, t), Arrival::Other);
        assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Other);
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Ask);
        // waiting for the answer
        assert_eq!(g.started(ad, "a.exe", &dir, t), None);
        assert_eq!(g.ended(ad, false, LIMIT, t), Arrival::Other);
        assert_eq!(g.asked().unwrap().name, "Song.zip");
        g.answer(false, &dir);
        assert!(g.started(ad, "a.exe", &dir, t).is_some());
        // a web view that never told the refusals' ends: the one saved
        // still ends
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Song.zip", 7);
        g.started(URL, "Song.zip", &dir, t);
        assert_eq!(g.ended(URL, true, LIMIT, t + REFUSAL_TOLD), Arrival::Ask);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A download over the limit, or one that failed, is deleted and not
    /// asked about.
    #[test]
    fn too_large_or_failed_is_deleted() {
        let dir = temp("large");
        let t = Instant::now();
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Pack.zip", LIMIT as usize + 1);
        assert_eq!(
            g.ended(URL, true, LIMIT, t),
            Arrival::TooLarge("Pack.zip".into())
        );
        save(&mut g, &dir, URL, "Song.zip", 7);
        assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Failed);
        // (a web view that made no file)
        g.started(URL, "Song.zip", &dir, t).unwrap();
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Failed);
        assert_eq!(g.asked(), None);
        assert_eq!(g.answer(true, &dir), None);
        assert!(files(&dir).is_empty());
        // at the limit: asked about
        save(&mut g, &dir, URL, "Song.zip", LIMIT as usize);
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Ask);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The download under way and the one not answered go with the
    /// browser; slots a run left are deleted, other files are not.
    #[test]
    fn slots_do_not_outlive_the_browser() {
        let dir = temp("slots");
        let t = Instant::now();
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Song.zip", 7);
        drop(g);
        assert!(files(&dir).is_empty());
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Song.zip", 7);
        g.ended(URL, true, LIMIT, t);
        drop(g);
        assert!(files(&dir).is_empty());
        std::fs::write(dir.join(".browser-1-1.part"), "x").unwrap();
        std::fs::write(dir.join("Song.zip"), "x").unwrap();
        clear_slots(&dir);
        assert_eq!(files(&dir), ["Song.zip"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// After an order of events: the question (if any) is answered no,
    /// the next download is taken and arrives, and closing the browser
    /// leaves no slot.
    fn takes_the_next(mut g: Gate, dir: &Path, t: Instant) {
        g.answer(false, dir);
        let next = "https://next.example/n.zip";
        let slot = g.started(next, "n.zip", dir, t).expect("the next is taken");
        std::fs::write(&slot, "x").unwrap();
        assert_eq!(g.ended(next, true, LIMIT, t), Arrival::Ask);
        drop(g);
        assert!(files(dir).is_empty());
    }

    /// The orders of events a web view can deliver. WebKitGTK tells a
    /// refused download's end (not whole) at once; WKWebView does not tell
    /// it by wry's code as read; WebView2 may or may not. A refused
    /// download never ends whole on any of them.
    #[test]
    fn every_order_leaves_the_gate_free() {
        let dir = temp("orders");
        let t = Instant::now();
        let b = "https://other.example/b.zip";

        // pressed twice, the refusal's end not told: the saved one's end
        // is its own
        let mut g = Gate::default();
        let slot = save(&mut g, &dir, URL, "Song.zip", 7);
        assert_eq!(g.started(URL, "Song.zip", &dir, t), None);
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Ask);
        assert_eq!(g.asked().unwrap().path, slot);
        takes_the_next(g, &dir, t);

        // pressed twice, the refusal's end told before or after it
        for refusal_first in [true, false] {
            let mut g = Gate::default();
            save(&mut g, &dir, URL, "Song.zip", 7);
            g.started(URL, "Song.zip", &dir, t);
            if refusal_first {
                assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Other);
            }
            assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Ask);
            if !refusal_first {
                assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Other);
            }
            assert_eq!(g.asked().unwrap().name, "Song.zip");
            takes_the_next(g, &dir, t);
        }

        // another address refused while one is saved: its end told before,
        // after, or not at all
        for told in [Some(true), Some(false), None] {
            let mut g = Gate::default();
            save(&mut g, &dir, URL, "Song.zip", 7);
            assert_eq!(g.started(b, "b.zip", &dir, t), None);
            if told == Some(true) {
                assert_eq!(g.ended(b, false, LIMIT, t), Arrival::Other);
            }
            assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Ask);
            if told == Some(false) {
                assert_eq!(g.ended(b, false, LIMIT, t), Arrival::Other);
            }
            takes_the_next(g, &dir, t);
        }

        // an end without a start
        let mut g = Gate::default();
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Other);
        assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Other);
        takes_the_next(g, &dir, t);

        // two ends: the second is nobody's
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Song.zip", 7);
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Ask);
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Other);
        assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Other);
        assert_eq!(g.asked().unwrap().size, 7);
        takes_the_next(g, &dir, t);

        // stopped or failed (wry lets nobody but the page or the network
        // stop one): deleted
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Song.zip", 7);
        assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Failed);
        assert!(files(&dir).is_empty());
        takes_the_next(g, &dir, t);

        // a web view that writes the slot again after its end was told
        let mut g = Gate::default();
        let slot = save(&mut g, &dir, URL, "Song.zip", 7);
        assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Failed);
        std::fs::write(&slot, "x").unwrap();
        takes_the_next(g, &dir, t);

        // an end after the browser was closed (a new gate): nobody's
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Song.zip", 7);
        g.started(URL, "Song.zip", &dir, t);
        drop(g);
        assert!(files(&dir).is_empty());
        let mut g = Gate::default();
        assert_eq!(g.ended(URL, true, LIMIT, t), Arrival::Other);
        takes_the_next(g, &dir, t);

        // pressed twice and the saved one fails at once, the refusal's end
        // not told: its failure is taken for the refusal's and the gate
        // waits until the browser is closed, which deletes the slot
        let mut g = Gate::default();
        save(&mut g, &dir, URL, "Song.zip", 7);
        g.started(URL, "Song.zip", &dir, t);
        assert_eq!(g.ended(URL, false, LIMIT, t), Arrival::Other);
        drop(g);
        assert!(files(&dir).is_empty());
        takes_the_next(Gate::default(), &dir, t);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hosts() {
        assert_eq!(host("https://Files.Example:8443/a/b.zip"), "files.example");
        assert_eq!(host("blob:https://mega.nz/1b2c"), "mega.nz");
        // who a download is from is the host, whatever is put before it
        assert_eq!(host("https://good.example@evil.example/x"), "evil.example");
        assert_eq!(
            host("https://evil.example\\@good.example/x"),
            "evil.example"
        );
        assert_eq!(host("https://evil.example#@good.example"), "evil.example");
        assert_eq!(host("http://[::1]:8080/x"), "::1");
        assert_eq!(host("data:application/zip;base64,AAAA"), "data:");
        assert_eq!(host("data:text/plain,https://good.example/x"), "data:");
    }
}
