//! The web build's song library kept in the browser (its Origin Private
//! File System): what goes into `<root>/bms/` is written there, and is
//! mounted again (lazily, as files read when needed) the next time the
//! page opens.
//!
//! The writing is done by a worker (`createSyncAccessHandle`: the way to
//! write OPFS that every browser with it has, Safari included); it also
//! lists and removes. Its requests run one after another.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// The worker: `[id, op, path, data]` in, `[id, ok, value]` out. `list`:
/// every file below `path` as `[relative path, File]`; `write`: `data` (an
/// ArrayBuffer or a Blob) into `path`, answering with its File; `remove`:
/// `path` and what is below it.
const WORKER: &str = r#"
let root = null;
async function dir(parts, create) {
  let d = root || (root = await navigator.storage.getDirectory());
  for (const n of parts) d = await d.getDirectoryHandle(n, { create });
  return d;
}
async function list(d, prefix, out) {
  for await (const [name, h] of d.entries()) {
    if (h.kind === "directory") await list(h, prefix + name + "/", out);
    else out.push([prefix + name, await h.getFile()]);
  }
}
async function run(id, op, path, data) {
  const parts = path.split("/").filter((p) => p);
  if (op === "list") {
    const out = [];
    let d = null;
    try { d = await dir(parts, false); } catch (e) { return out; }
    await list(d, "", out);
    return out;
  }
  if (op === "write") {
    const name = parts.pop();
    const fh = await (await dir(parts, true)).getFileHandle(name, { create: true });
    const buf = data instanceof Blob ? await data.arrayBuffer() : data;
    const h = await fh.createSyncAccessHandle();
    try {
      h.truncate(0);
      h.write(new Uint8Array(buf), { at: 0 });
      h.flush();
    } finally {
      h.close();
    }
    return await fh.getFile();
  }
  if (op === "remove") {
    const name = parts.pop();
    try {
      await (await dir(parts, false)).removeEntry(name, { recursive: true });
    } catch (e) {
      if (e.name !== "NotFoundError") throw e;
    }
    return null;
  }
  throw new Error("unknown op " + op);
}
let queue = Promise.resolve();
onmessage = (e) => {
  const [id, op, path, data] = e.data;
  queue = queue.then(() => run(id, op, path, data)).then(
    (v) => postMessage([id, true, v]),
    (err) => postMessage([id, false, (err && err.name ? err.name + ": " : "") + (err && err.message || err)])
  );
};
"#;

type Done = Box<dyn FnOnce(Result<JsValue, String>)>;

/// What the browser says of the page's storage.
#[derive(Clone, Copy, Debug, Default)]
pub struct Status {
    /// OPFS works in this browser.
    pub available: bool,
    /// Bytes used and the most the page may use (the browser's estimate).
    pub usage: f64,
    pub quota: f64,
    /// The browser keeps it even when short of space.
    pub persisted: bool,
    /// Files waiting to be written.
    pub pending: usize,
}

thread_local! {
    static WORKER_H: RefCell<Option<web_sys::Worker>> = const { RefCell::new(None) };
    static NEXT: Cell<u32> = const { Cell::new(1) };
    static WAITING: RefCell<HashMap<u32, Done>> = RefCell::new(HashMap::new());
    /// The folders kept, and their names in the storage: `<root>/bms`
    /// (songs) and `<root>/scores/replays`.
    static KEPT: RefCell<Vec<(PathBuf, &'static str)>> = const { RefCell::new(Vec::new()) };
    static STATUS: Cell<Status> = Cell::new(Status::default());
    /// Messages for the player (a write failed: the storage is full, ...).
    static NOTES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn send(op: &str, rel: &str, data: JsValue, transfer: Option<&JsValue>, done: Done) -> bool {
    WORKER_H.with(|w| {
        let w = w.borrow();
        let Some(w) = w.as_ref() else {
            return false;
        };
        let id = NEXT.with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1));
            id
        });
        WAITING.with(|m| m.borrow_mut().insert(id, done));
        let msg = js_sys::Array::of4(&id.into(), &op.into(), &rel.into(), &data);
        let sent = match transfer {
            Some(t) => w.post_message_with_transfer(&msg, &js_sys::Array::of1(t)),
            None => w.post_message(&msg),
        };
        if sent.is_err() {
            WAITING.with(|m| m.borrow_mut().remove(&id));
            return false;
        }
        bump(1);
        true
    })
}

fn bump(d: isize) {
    STATUS.with(|s| {
        let mut v = s.get();
        v.pending = (v.pending as isize + d).max(0) as usize;
        s.set(v);
    });
}

fn note(m: String) {
    NOTES.with(|n| n.borrow_mut().push(m));
}

/// Messages since the last call (failed writes).
pub fn take_notes() -> Vec<String> {
    NOTES.with(|n| std::mem::take(&mut *n.borrow_mut()))
}

/// `p`'s path in the storage, if it is kept.
fn kept(p: &Path) -> Option<String> {
    KEPT.with(|k| {
        k.borrow().iter().find_map(|(dir, name)| {
            let rel = p.strip_prefix(dir).ok()?;
            let s = rel.to_string_lossy().replace('\\', "/");
            Some(if s.is_empty() {
                name.to_string()
            } else {
                format!("{name}/{s}")
            })
        })
    })
}

/// Start: the worker, then the kept files mounted below `<root>/bms`
/// (`mount` for each), then `done`.
pub fn start(
    root: &Path,
    mount: impl FnMut(PathBuf, web_sys::File) + 'static,
    done: impl FnOnce(usize) + 'static,
) {
    let kept_dirs = vec![
        (root.join("bms"), "bms"),
        (crate::replay::dir(root), "replays"),
    ];
    KEPT.with(|k| *k.borrow_mut() = kept_dirs.clone());
    let has_opfs = web_sys::window()
        .map(|w| w.navigator())
        .and_then(|n| js_sys::Reflect::get(&n, &"storage".into()).ok())
        .filter(|s| !s.is_undefined())
        .and_then(|s| js_sys::Reflect::get(&s, &"getDirectory".into()).ok())
        .is_some_and(|f| f.is_function());
    if !has_opfs {
        log::info!("library: this browser has no OPFS; songs last until the page closes");
        return done(0);
    }
    let blob = {
        let parts = js_sys::Array::of1(&WORKER.into());
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type("text/javascript");
        web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts)
    };
    let worker = blob
        .ok()
        .and_then(|b| web_sys::Url::create_object_url_with_blob(&b).ok())
        .and_then(|u| web_sys::Worker::new(&u).ok());
    let Some(worker) = worker else {
        log::warn!("library: no worker for OPFS");
        return done(0);
    };
    let on_msg = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(|e: web_sys::MessageEvent| {
        let a: js_sys::Array = e.data().unchecked_into();
        let id = a.get(0).as_f64().unwrap_or(0.0) as u32;
        let ok = a.get(1).as_bool().unwrap_or(false);
        let v = a.get(2);
        bump(-1);
        if let Some(f) = WAITING.with(|m| m.borrow_mut().remove(&id)) {
            f(if ok {
                Ok(v)
            } else {
                Err(v.as_string().unwrap_or_default())
            });
        }
    });
    worker.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
    on_msg.forget();
    WORKER_H.with(|w| *w.borrow_mut() = Some(worker));
    // each kept folder listed, one after another; then `done`
    let mount = std::rc::Rc::new(RefCell::new(mount));
    list_all(kept_dirs, mount, 0, Box::new(done));
}

fn list_all(
    mut dirs: Vec<(PathBuf, &'static str)>,
    mount: std::rc::Rc<RefCell<dyn FnMut(PathBuf, web_sys::File)>>,
    n: usize,
    done: Box<dyn FnOnce(usize)>,
) {
    if dirs.is_empty() {
        refresh_status();
        return done(n);
    }
    let (dir, name) = dirs.remove(0);
    send(
        "list",
        name,
        JsValue::NULL,
        None,
        Box::new(move |r| {
            let mut n = n;
            match r {
                Ok(v) => {
                    STATUS.with(|s| {
                        let mut st = s.get();
                        st.available = true;
                        s.set(st);
                    });
                    for item in js_sys::Array::from(&v).iter() {
                        let item: js_sys::Array = item.unchecked_into();
                        let (Some(rel), Ok(file)) = (
                            item.get(0).as_string(),
                            item.get(1).dyn_into::<web_sys::File>(),
                        ) else {
                            continue;
                        };
                        (mount.borrow_mut())(dir.join(rel), file);
                        n += 1;
                    }
                }
                Err(e) => log::warn!("library: {e}"),
            }
            list_all(dirs, mount, n, done)
        }),
    );
}

/// Keep `p` (below `<root>/bms`): its bytes written; `written` gets the
/// stored file (the bytes in memory can go then).
pub fn save(p: &Path, data: &[u8], written: impl FnOnce(web_sys::File) + 'static) {
    let Some(rel) = kept(p) else { return };
    let arr = js_sys::Uint8Array::from(data);
    let buf: JsValue = arr.buffer().into();
    write(rel, buf.clone(), Some(buf), Box::new(written));
}

/// Keep `p` from a file the player picked (read by the worker).
pub fn save_file(p: &Path, file: &web_sys::File, written: impl FnOnce(web_sys::File) + 'static) {
    let Some(rel) = kept(p) else { return };
    write(rel, file.clone().into(), None, Box::new(written));
}

fn write(
    rel: String,
    data: JsValue,
    transfer: Option<JsValue>,
    written: Box<dyn FnOnce(web_sys::File)>,
) {
    let what = rel.clone();
    send(
        "write",
        &rel,
        data,
        transfer.as_ref(),
        Box::new(move |r| match r {
            Ok(f) => {
                if let Ok(f) = f.dyn_into::<web_sys::File>() {
                    written(f);
                }
            }
            Err(e) => {
                log::warn!("library: {what}: {e}");
                if e.starts_with("QuotaExceededError") {
                    note("No room to keep the song: free some in Settings › LIBRARY".into());
                } else {
                    note(format!("Could not keep {what}: {e}"));
                }
            }
        }),
    );
}

/// Forget `p` (a file or a folder below `<root>/bms`).
pub fn remove(p: &Path) {
    let Some(rel) = kept(p) else { return };
    let what = rel.clone();
    send(
        "remove",
        &rel,
        JsValue::NULL,
        None,
        Box::new(move |r| {
            if let Err(e) = r {
                log::warn!("library: remove {what}: {e}");
            }
            refresh_status();
        }),
    );
}

thread_local! {
    static ASKED: Cell<f64> = const { Cell::new(0.0) };
}

/// The storage's state, as last asked (asked again every few seconds
/// while it is looked at).
pub fn status() -> Status {
    let now = js_sys::Date::now();
    if now - ASKED.with(|a| a.get()) > 3000.0 {
        ASKED.with(|a| a.set(now));
        refresh_status();
    }
    STATUS.with(|s| s.get())
}

fn storage_manager() -> Option<JsValue> {
    let n = web_sys::window()?.navigator();
    js_sys::Reflect::get(&n, &"storage".into())
        .ok()
        .filter(|s| !s.is_undefined())
}

fn call(target: &JsValue, name: &str) -> Option<js_sys::Promise> {
    let f: js_sys::Function = js_sys::Reflect::get(target, &name.into())
        .ok()?
        .dyn_into()
        .ok()?;
    f.call0(target).ok()?.dyn_into().ok()
}

/// Ask the browser how much is used, and whether it is kept.
pub fn refresh_status() {
    let Some(sm) = storage_manager() else { return };
    let (est, per) = (call(&sm, "estimate"), call(&sm, "persisted"));
    wasm_bindgen_futures::spawn_local(async move {
        if let Some(p) = est {
            if let Ok(v) = wasm_bindgen_futures::JsFuture::from(p).await {
                let num = |k: &str| {
                    js_sys::Reflect::get(&v, &k.into())
                        .ok()
                        .and_then(|x| x.as_f64())
                        .unwrap_or(0.0)
                };
                STATUS.with(|s| {
                    let mut st = s.get();
                    st.usage = num("usage");
                    st.quota = num("quota");
                    s.set(st);
                });
            }
        }
        if let Some(p) = per {
            if let Ok(v) = wasm_bindgen_futures::JsFuture::from(p).await {
                STATUS.with(|s| {
                    let mut st = s.get();
                    st.persisted = v.as_bool().unwrap_or(false);
                    s.set(st);
                });
            }
        }
    });
}

/// Ask the browser to keep the library even when it is short of space
/// (granted more readily to a page added to the home screen).
pub fn request_persist() {
    let Some(sm) = storage_manager() else { return };
    let Some(p) = call(&sm, "persist") else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        let granted = wasm_bindgen_futures::JsFuture::from(p)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !granted {
            note("The browser did not promise to keep the songs (adding the page to the home screen helps)".into());
        }
        refresh_status();
    });
}
