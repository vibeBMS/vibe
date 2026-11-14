//! Browser integration: pickers for a BMS folder, loose files or song pack
//! zips -> virtual file system.

use crate::placement::{self, ChartInfo, FolderInfo, Placement};
use crate::platform::vfs;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

thread_local! {
    /// Data root the pickers mount under (`<root>/bms/`).
    static ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Hook up the pickers of `web/index.html`:
/// `<input id="bms-input" webkitdirectory>` (a folder) and
/// `<input id="bms-files" multiple>` (files and `.zip` song packs; iPad
/// Safari cannot pick folders).
pub fn install_file_input(root: PathBuf) {
    ROOT.with(|r| *r.borrow_mut() = Some(root.clone()));
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    for id in ["bms-input", "bms-files"] {
        let Some(input) = doc
            .get_element_by_id(id)
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        else {
            log::warn!("no #{id} element");
            continue;
        };
        let input2 = input.clone();
        let root = root.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            if let Some(files) = input2.files() {
                log::info!("picked {} files", files.length());
                crate::platform::vfs::add_files(&root, &files, "picked");
                for i in 0..files.length() {
                    if let Some(f) = files.get(i) {
                        if f.name().to_lowercase().ends_with(".zip") {
                            mount_zip_file(f);
                        }
                    }
                }
            }
        });
        input.set_onchange(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
}

/// Read a picked zip and unpack it.
fn mount_zip_file(f: web_sys::File) {
    wasm_bindgen_futures::spawn_local(async move {
        let name = f.name();
        match wasm_bindgen_futures::JsFuture::from(f.array_buffer()).await {
            Ok(buf) => {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                mount_zip(&name, &bytes);
            }
            Err(e) => log::warn!("{name}: {e:?}"),
        }
    });
}

/// Unpack a song pack zip into the library (the page's demo button calls
/// this with a downloaded zip). Returns the number of files added.
#[wasm_bindgen(js_name = mountZip)]
pub fn mount_zip(name: &str, data: &[u8]) -> usize {
    let Some(root) = ROOT.with(|r| r.borrow().clone()) else {
        return 0;
    };
    let n = crate::platform::vfs::add_zip(&root, name, data.to_vec());
    log::info!("{name}: {n} files");
    n
}

thread_local! {
    static BUTTONS_SHOWN: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Show or hide the page's picker buttons (`#bar`; hidden while playing, so
/// they do not cover the touch keys). A bar the user closed stays closed.
pub fn show_page_buttons(show: bool) {
    if BUTTONS_SHOWN.with(|b| b.replace(Some(show))) == Some(show) {
        return;
    }
    let Some(bar) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("bar"))
    else {
        return;
    };
    if show {
        let _ = bar.remove_attribute("data-playing");
    } else {
        let _ = bar.set_attribute("data-playing", "");
    }
}

/// A chart the user is asked to place: its path and the candidate folders
/// with a label each.
struct Ask {
    chart: PathBuf,
    what: String,
    folders: Vec<(PathBuf, String)>,
}

thread_local! {
    /// Charts already placed by the user (or left where they are).
    static ASKED: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
    static QUEUE: RefCell<VecDeque<Ask>> = const { RefCell::new(VecDeque::new()) };
    static DIALOG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

const CHART_EXTS: [&str; 5] = ["bms", "bme", "bml", "pms", "bmson"];

fn is_chart(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CHART_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every mounted folder below `dir` with its file names.
fn folders(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<String>>) {
    for e in vfs::read_dir(dir).unwrap_or_default() {
        if e.is_dir {
            folders(&e.path, out);
        } else {
            out.entry(dir.to_path_buf())
                .or_default()
                .push(name_of(&e.path));
        }
    }
}

fn chart_info(path: &Path) -> Option<ChartInfo> {
    let bytes = vfs::read(path).ok()?;
    let c = vibebms_core::load_chart_bytes(path, &bytes, &Default::default()).ok()?;
    Some(ChartInfo {
        title: c.meta.title,
        artist: c.meta.artist,
        sounds: c.sound_files,
    })
}

/// Charts picked as loose files (`<root>/bms/picked/`) whose sounds are
/// elsewhere: move each into its song's folder when exactly one folder has
/// every sound and the same title and artist, else ask (see
/// `placement`). Returns messages for the user.
pub fn place_picked_charts(root: &Path) -> Vec<String> {
    let bms = root.join("bms");
    let picked = bms.join("picked");
    let mut all = BTreeMap::new();
    folders(&bms, &mut all);
    let Some(own_files) = all.get(&picked).cloned() else {
        return Vec::new();
    };
    let own = FolderInfo {
        dir: picked.clone(),
        files: own_files.clone(),
        charts: Vec::new(),
    };
    let mut out = Vec::new();
    for name in own_files {
        let path = picked.join(&name);
        if !is_chart(&path) || ASKED.with(|a| a.borrow().contains(&path)) {
            continue;
        }
        let Some(info) = chart_info(&path) else {
            continue;
        };
        // only folders with every sound are candidates; read their charts
        let candidates: Vec<FolderInfo> = all
            .iter()
            .filter(|(d, _)| **d != picked)
            .map(|(d, files)| FolderInfo {
                dir: d.clone(),
                files: files.clone(),
                charts: Vec::new(),
            })
            .filter(|f| placement::covers(&f.files, &info.sounds))
            .map(|mut f| {
                f.charts = f
                    .files
                    .iter()
                    .map(|n| f.dir.join(n))
                    .filter(|p| is_chart(p))
                    .filter_map(|p| chart_info(&p))
                    .collect();
                f
            })
            .collect();
        match placement::place(&info, &own, &candidates) {
            Placement::Move(dir) => {
                if vfs::rename(&path, &dir.join(&name)) {
                    out.push(format!("{name} → {}", name_of(&dir)));
                }
            }
            Placement::Ask(dirs) => {
                ASKED.with(|a| a.borrow_mut().insert(path.clone()));
                let folders = dirs
                    .into_iter()
                    .map(|d| {
                        let song = candidates
                            .iter()
                            .find(|f| f.dir == d)
                            .and_then(|f| f.charts.first())
                            .map(|c| format!("{} — {}", c.title, c.artist))
                            .unwrap_or_default();
                        let label = format!("{}  ({})", song, name_of(&d));
                        (d, label)
                    })
                    .collect();
                QUEUE.with(|q| {
                    q.borrow_mut().push_back(Ask {
                        what: format!("{name}: {} — {}", info.title, info.artist),
                        chart: path,
                        folders,
                    })
                });
            }
            // its sounds are nowhere yet
            Placement::Stay if !placement::covers(&own.files, &info.sounds) => {
                ASKED.with(|a| a.borrow_mut().insert(path));
                out.push(format!("{name}: add its song first, then this chart again"));
            }
            Placement::Stay => {}
        }
    }
    show_next_dialog();
    out
}

/// Ask about the next queued chart: a list of song folders and "keep it
/// separate", over the page.
fn show_next_dialog() {
    if DIALOG.with(|d| d.get()) {
        return;
    }
    let Some(ask) = QUEUE.with(|q| q.borrow_mut().pop_front()) else {
        return;
    };
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(body) = doc.body() else { return };
    let el = |tag: &str, css: &str, text: &str| -> Option<web_sys::HtmlElement> {
        let e = doc
            .create_element(tag)
            .ok()?
            .dyn_into::<web_sys::HtmlElement>()
            .ok()?;
        let _ = e.set_attribute("style", css);
        e.set_text_content(Some(text));
        Some(e)
    };
    let Some(shade) = el(
        "div",
        "position:fixed;inset:0;z-index:20;background:#000a;display:flex;\
         align-items:center;justify-content:center;font:14px Inter,sans-serif",
        "",
    ) else {
        return;
    };
    let Some(boxed) = el(
        "div",
        "background:#141824;color:#e2e8f5;border:1px solid #343e5c;border-radius:10px;\
         padding:16px;max-width:min(560px,90vw);max-height:80vh;overflow:auto",
        "",
    ) else {
        return;
    };
    for (css, text) in [
        (
            "font-weight:700;margin-bottom:6px",
            "Which song is this chart for?".to_string(),
        ),
        (
            "color:#808caa;margin-bottom:12px;word-break:break-all",
            ask.what.clone(),
        ),
    ] {
        if let Some(p) = el("div", css, &text) {
            let _ = boxed.append_child(&p);
        }
    }
    let button_css = "display:block;width:100%;text-align:left;margin:4px 0;padding:8px 10px;\
                      background:#1e2434;color:#e2e8f5;border:1px solid #343e5c;\
                      border-radius:6px;font:inherit;cursor:pointer";
    let choices = ask
        .folders
        .iter()
        .map(|(d, l)| (Some(d.clone()), l.clone()))
        .chain(std::iter::once((None, "Keep it separate".to_string())));
    for (dir, label) in choices {
        let Some(b) = el("button", button_css, &label) else {
            continue;
        };
        let chart = ask.chart.clone();
        let shade2 = shade.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            if let Some(dir) = &dir {
                let to = dir.join(chart.file_name().unwrap_or_default());
                if vfs::rename(&chart, &to) {
                    vfs::mark_added();
                }
            }
            shade2.remove();
            DIALOG.with(|d| d.set(false));
            show_next_dialog();
        });
        b.set_onclick(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
        let _ = boxed.append_child(&b);
    }
    let _ = shade.append_child(&boxed);
    let _ = body.append_child(&shade);
    DIALOG.with(|d| d.set(true));
}
