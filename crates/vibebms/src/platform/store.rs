//! Small files that must outlast the program: the IR history (lines added
//! one at a time) and the IR outbox (rewritten whole).
//!
//! Natively they are files under the data root. On the web they are kept
//! in OPFS by its worker ([`super::opfs`]), as `store/<path>`; a browser
//! without OPFS keeps them in `localStorage`, a history there only its
//! newest [`FALLBACK_LINES`] lines ([`limited`] says so).
//!
//! Every line ends with a line break. A line cut short (the program or the
//! page ended while writing it) is ended by the next [`append`], so it
//! stays one line that does not parse and the next is whole.

use std::io;
use std::path::Path;

/// The lines a history keeps in `localStorage` (about 5 MB for the whole
/// page).
pub const FALLBACK_LINES: usize = 200;

/// Add `line` (one line: no line break) at the end of the file `p`.
/// On the web this queues the write: a failure is logged and told to the
/// player ([`super::opfs::take_notes`]).
pub fn append(p: &Path, line: &str) -> io::Result<()> {
    if line.contains('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a store line has no line break",
        ));
    }
    imp::append(p, line)
}

/// Put `text` in the file `p`'s place: a reader finds the old text or the
/// new, never part of one (on the web, the new one is written in place
/// where the browser cannot move a file over another).
pub fn rewrite(p: &Path, text: &str) -> io::Result<()> {
    imp::rewrite(p, text)
}

/// The text of the file `p`, "" when there is none (bytes that are not
/// UTF-8 replaced); `on_done` is called at once natively, when the worker
/// answers on the web.
pub fn load(p: &Path, on_done: impl FnOnce(io::Result<String>) + 'static) {
    imp::load(p, on_done)
}

/// Whether the files are kept with less room than asked: the web without
/// OPFS.
pub fn limited() -> bool {
    imp::limited()
}

/// Whether this program sends the IR outbox and writes its file: `None`
/// while that is being asked. Natively always. On the web the first tab
/// that asks holds the lock `name` (`navigator.locks`, with `ifAvailable`;
/// one per profile, [`outbox_lock_name`]) until it closes or asks for
/// another; another tab only reads, and asks again every
/// [`LOCK_RETRY_MS`]. A browser without the Locks API has its tabs send
/// each on their own.
pub fn outbox_lock(name: &str) -> Option<bool> {
    imp::outbox_lock(name)
}

/// The outbox lock of profile `profile` ("": the default's, the name it
/// had before there were profiles).
pub fn outbox_lock_name(profile: &str) -> String {
    match profile {
        "" => "vibebms-outbox".into(),
        p => format!("vibebms-outbox:{p}"),
    }
}

/// Forget the files below `dir` (a profile deleted): on the web those kept
/// in OPFS or `localStorage`; natively they are files there, removed with
/// the folder.
pub fn remove_below(dir: &Path) {
    imp::remove_below(dir)
}

/// How often a tab without the outbox lock asks for it again.
pub const LOCK_RETRY_MS: f64 = 10_000.0;

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::{Read, Seek, SeekFrom, Write};

    pub fn append(p: &Path, line: &str) -> io::Result<()> {
        if let Some(d) = p.parent() {
            fs::create_dir_all(d)?;
        }
        let mut f = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(p)?;
        let mut add = Vec::with_capacity(line.len() + 2);
        if f.seek(SeekFrom::End(0))? > 0 {
            let mut last = [0];
            f.seek(SeekFrom::End(-1))?;
            f.read_exact(&mut last)?;
            if last[0] != b'\n' {
                add.push(b'\n');
            }
        }
        add.extend_from_slice(line.as_bytes());
        add.push(b'\n');
        f.write_all(&add)?;
        f.sync_data()
    }

    pub fn rewrite(p: &Path, text: &str) -> io::Result<()> {
        super::super::vfs::write_atomic(p, text.as_bytes(), true)
    }

    pub fn load(p: &Path, on_done: impl FnOnce(io::Result<String>)) {
        on_done(match fs::read(p) {
            Ok(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e),
        })
    }

    pub fn limited() -> bool {
        false
    }

    pub fn outbox_lock(_name: &str) -> Option<bool> {
        Some(true)
    }

    pub fn remove_below(_dir: &Path) {}
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::super::opfs;
    use super::*;

    /// `p` in OPFS.
    fn name(p: &Path) -> String {
        let p = p.to_string_lossy().replace('\\', "/");
        format!("store/{}", p.trim_start_matches('/'))
    }

    pub fn append(p: &Path, line: &str) -> io::Result<()> {
        if opfs::has_worker() {
            opfs::write_text(&name(p), &format!("{line}\n"), false);
            return Ok(());
        }
        let mut text = local::get(p)?;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(line);
        text.push('\n');
        let lines = text.matches('\n').count();
        if lines > FALLBACK_LINES {
            let n = lines - FALLBACK_LINES;
            let cut = text
                .match_indices('\n')
                .nth(n - 1)
                .map_or(0, |(i, _)| i + 1);
            text.drain(..cut);
        }
        local::set(p, &text)
    }

    pub fn rewrite(p: &Path, text: &str) -> io::Result<()> {
        if opfs::has_worker() {
            opfs::write_text(&name(p), text, true);
            return Ok(());
        }
        local::set(p, text)
    }

    pub fn load(p: &Path, on_done: impl FnOnce(io::Result<String>) + 'static) {
        if opfs::has_worker() {
            opfs::read_text(&name(p), move |r| on_done(r.map_err(io::Error::other)));
        } else {
            on_done(local::get(p));
        }
    }

    pub fn limited() -> bool {
        !opfs::has_worker()
    }

    pub fn outbox_lock(name: &str) -> Option<bool> {
        lock::held(name)
    }

    pub fn remove_below(dir: &Path) {
        if opfs::has_worker() {
            opfs::remove_name(&name(dir));
        }
        local::remove_below(dir);
    }

    mod lock {
        use super::super::LOCK_RETRY_MS;
        use std::cell::{Cell, RefCell};
        use wasm_bindgen::closure::Closure;
        use wasm_bindgen::{JsCast, JsValue};

        #[derive(Clone, Copy, PartialEq)]
        enum State {
            Unasked,
            Asking,
            Held,
            /// Another tab holds it: asked again from this time (ms).
            Other(f64),
        }

        thread_local! {
            static STATE: Cell<State> = const { Cell::new(State::Unasked) };
            /// The lock asked for (the profile's).
            static NAME: RefCell<String> = const { RefCell::new(String::new()) };
            /// Lets the lock held go (the promise it waits for, resolved).
            static RELEASE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
        }

        fn asked(name: &str) -> bool {
            NAME.with(|n| *n.borrow() == name)
        }

        pub fn held(name: &str) -> Option<bool> {
            if !asked(name) {
                // another profile's: that one goes, this one is asked
                if let Some(f) = RELEASE.with(|r| r.borrow_mut().take()) {
                    let _ = f.call0(&JsValue::UNDEFINED);
                }
                NAME.with(|n| *n.borrow_mut() = name.to_string());
                STATE.set(State::Unasked);
            }
            match STATE.get() {
                State::Held => return Some(true),
                State::Asking => return None,
                State::Other(at) if js_sys::Date::now() < at => return Some(false),
                State::Unasked | State::Other(_) => {}
            }
            if ask(name).is_some() {
                STATE.set(State::Asking);
                None
            } else {
                // no Locks API: this tab is on its own
                STATE.set(State::Held);
                Some(true)
            }
        }

        /// `navigator.locks.request(name, {ifAvailable: true}, cb)`: `cb`
        /// gets the lock, or `null` when another tab has it; the promise it
        /// returns for the lock settles only when another is asked for, so
        /// the lock is held while the page lives on this profile.
        fn ask(name: &str) -> Option<()> {
            let nav = web_sys::window()?.navigator();
            let locks = js_sys::Reflect::get(&nav, &"locks".into())
                .ok()
                .filter(|l| !l.is_undefined() && !l.is_null())?;
            let request: js_sys::Function = js_sys::Reflect::get(&locks, &"request".into())
                .ok()?
                .dyn_into()
                .ok()?;
            let opts = js_sys::Object::new();
            js_sys::Reflect::set(&opts, &"ifAvailable".into(), &true.into()).ok()?;
            let mine = name.to_string();
            let cb = Closure::once_into_js(move |lock: JsValue| -> JsValue {
                // (asked for another meanwhile: this one goes at once)
                if !asked(&mine) {
                    return JsValue::UNDEFINED;
                }
                if lock.is_null() {
                    STATE.set(State::Other(js_sys::Date::now() + LOCK_RETRY_MS));
                    JsValue::UNDEFINED
                } else {
                    STATE.set(State::Held);
                    js_sys::Promise::new(&mut |resolve, _| {
                        RELEASE.with(|r| *r.borrow_mut() = Some(resolve));
                    })
                    .into()
                }
            });
            request.call3(&locks, &name.into(), &opts, &cb).ok()?;
            Some(())
        }
    }

    /// The fallback: `localStorage`, one value per file.
    mod local {
        use super::*;

        fn storage() -> io::Result<web_sys::Storage> {
            web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "no storage"))
        }

        fn key(p: &Path) -> String {
            format!("vibebms-store:{}", p.display())
        }

        pub fn get(p: &Path) -> io::Result<String> {
            Ok(storage()?
                .get_item(&key(p))
                .ok()
                .flatten()
                .unwrap_or_default())
        }

        pub fn set(p: &Path, text: &str) -> io::Result<()> {
            storage()?
                .set_item(&key(p), text)
                .map_err(|_| io::Error::other("the browser's storage is full"))
        }

        /// Every value of a file below `dir` removed.
        pub fn remove_below(dir: &Path) {
            let Ok(s) = storage() else { return };
            let prefix = format!("{}/", key(dir));
            let keys: Vec<String> = (0..s.length().unwrap_or(0))
                .filter_map(|i| s.key(i).ok().flatten())
                .filter(|k| k.starts_with(&prefix))
                .collect();
            for k in keys {
                let _ = s.remove_item(&k);
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn loaded(p: &Path) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        load(p, move |r| tx.send(r.unwrap()).unwrap());
        rx.try_recv().unwrap()
    }

    #[test]
    fn lines_outlast_a_cut_short_one() {
        let dir = std::env::temp_dir().join(format!("vibebms-store-{}", std::process::id()));
        let p = dir.join("history/plays.jsonl");
        assert_eq!(loaded(&p), "");
        append(&p, r#"{"n":1}"#).unwrap();
        append(&p, r#"{"n":2}"#).unwrap();
        assert_eq!(loaded(&p), "{\"n\":1}\n{\"n\":2}\n");
        assert!(append(&p, "a\nb").is_err());
        // the program ended halfway through a line
        std::fs::write(&p, "{\"n\":1}\n{\"n\":2}\n{\"n\":").unwrap();
        append(&p, r#"{"n":4}"#).unwrap();
        let text = loaded(&p);
        let read: Vec<Result<serde_json::Value, _>> =
            text.lines().map(serde_json::from_str).collect();
        assert_eq!(read.len(), 4);
        assert!(read[2].is_err());
        assert_eq!(read[3].as_ref().unwrap()["n"], 4);
        // rewritten whole, nothing left beside it
        rewrite(&p, "{\"n\":5}\n").unwrap();
        assert_eq!(loaded(&p), "{\"n\":5}\n");
        let left: Vec<_> = std::fs::read_dir(p.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
