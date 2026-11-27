//! Browser integration: pickers for a BMS folder, loose files or song pack
//! zips -> virtual file system.

use crate::placement::{self, Located};
use crate::platform::vfs;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

thread_local! {
    /// Data root the pickers mount under (`<root>/bms/`).
    static ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Hook up the pickers of `web/index.html`:
/// `<input id="bms-input" webkitdirectory>` (a folder) and
/// `<input id="bms-files" multiple>` (files and .zip / .7z / .rar / .lzh / .tar / .tar.gz
/// song packs; iPad Safari cannot pick folders).
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
                        if bmseeker::unpack::is_archive(Path::new(&f.name())) {
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

/// Read a picked song pack (.zip / .7z / .rar / .lzh / .tar / .tar.gz) and
/// unpack it.
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

/// Unpack a song pack (.zip / .7z / .rar / .lzh / .tar / .tar.gz) into the
/// library. Returns the number of files added.
fn mount_zip(name: &str, data: &[u8]) -> usize {
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

/// Charts picked as loose files (`<root>/bms/picked/`) whose sounds are
/// elsewhere: move each into its song's folder, or ask (see
/// `placement`). Returns messages for the user.
pub fn place_picked_charts(root: &Path, songs: &[crate::library::Song]) -> Vec<String> {
    let bms = root.join("bms");
    let picked = bms.join("picked");
    // the library's charts as read (those picked cannot be read yet)
    let lib = placement::library(root, songs);
    let mut out = Vec::new();
    for e in vfs::read_dir(&picked).unwrap_or_default() {
        let path = e.path;
        if e.is_dir || !placement::is_chart(&path) || ASKED.with(|a| a.borrow().contains(&path)) {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match placement::locate_loose(&path, &lib) {
            Some(Located::Move(dir)) => {
                if placement::bring(&path, &dir).is_some() {
                    out.push(format!(
                        "{name} → {}",
                        dir.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
            }
            Some(Located::Ask(folders)) => {
                ASKED.with(|a| a.borrow_mut().insert(path.clone()));
                let what = placement::chart_info(&path)
                    .map(|i| format!("{name}: {} — {}", i.title, i.artist))
                    .unwrap_or_else(|| name.clone());
                QUEUE.with(|q| {
                    q.borrow_mut().push_back(Ask {
                        chart: path,
                        what,
                        folders,
                    })
                });
            }
            Some(Located::NoSong) => {
                ASKED.with(|a| a.borrow_mut().insert(path));
                out.push(tr!(
                    "{name}: add its song first, then this chart again",
                    name = name
                ));
            }
            Some(Located::Here) | None => {}
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
            tr!("Which song is this chart for?").to_string(),
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
        .chain(std::iter::once((None, tr!("Keep it separate").to_string())));
    for (dir, label) in choices {
        let Some(b) = el("button", button_css, &label) else {
            continue;
        };
        let chart = ask.chart.clone();
        let shade2 = shade.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            if let Some(dir) = &dir {
                if crate::placement::bring(&chart, dir).is_some() {
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

thread_local! {
    /// The browser's time of the input event being handled (ms, as
    /// `performance.now()`), set before the app sees it.
    static INPUT_STAMP: std::cell::Cell<f64> = const { std::cell::Cell::new(-1.0) };
}

/// Note the browser's own time of each input event (its `timeStamp`), in a
/// capture listener that runs before the app's: the app takes it as the
/// event's time ([`input_time`]), not when its handler ran, which on a busy
/// page (an iPad drawing a chart) is a frame or more later.
pub fn install_input_stamps() {
    let Some(w) = web_sys::window() else { return };
    let note = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        INPUT_STAMP.with(|s| s.set(e.time_stamp()));
    });
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    opts.set_passive(true);
    for kind in [
        "keydown",
        "keyup",
        "pointerdown",
        "pointerup",
        "pointermove",
        "touchstart",
        "touchend",
        "touchmove",
        "mousedown",
        "mouseup",
    ] {
        let _ = w.add_event_listener_with_callback_and_add_event_listener_options(
            kind,
            note.as_ref().unchecked_ref(),
            &opts,
        );
    }
    note.forget();
}

/// When the input event being handled happened: its browser time, if it
/// was noted just now; else now.
pub fn input_time() -> crate::platform::Instant {
    stamp_time(INPUT_STAMP.with(|s| s.replace(-1.0)))
}

/// An event's browser time (`timeStamp`, ms; < 0: none) as an `Instant`.
fn stamp_time(stamp: f64) -> crate::platform::Instant {
    let now = crate::platform::Instant::now();
    let perf = web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now());
    match perf {
        Some(p) if stamp >= 0.0 => crate::platform::event_instant(now, (p - stamp) / 1000.0),
        _ => now,
    }
}

/// Tell `changed` when the page gets or loses the focus (another window
/// or app is used): winit's `Focused` is the canvas', which a click on
/// the page's own buttons takes too.
pub fn install_page_focus(changed: impl Fn(bool) + 'static) {
    let Some(w) = web_sys::window() else { return };
    let changed = std::rc::Rc::new(changed);
    for (kind, focused) in [("focus", true), ("blur", false)] {
        let changed = changed.clone();
        let f = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            // the window's own, not an element's (they do not bubble, but
            // a capture listener would see them)
            let own = e
                .target()
                .is_some_and(|t| t.dyn_ref::<web_sys::Window>().is_some());
            if own {
                changed(focused);
            }
        });
        let _ = w.add_event_listener_with_callback(kind, f.as_ref().unchecked_ref());
        f.forget();
    }
}

thread_local! {
    /// When the last finger left the screen, until the app takes it.
    static TOUCHES_GONE: std::cell::Cell<Option<crate::platform::Instant>> =
        const { std::cell::Cell::new(None) };
}

/// Watch for the moment no finger is on the screen (`touchend` /
/// `touchcancel` with no touches left): a key whose release the page never
/// got (the browser took the gesture, an iPad's home bar) is let go then.
pub fn install_touch_watch() {
    let Some(w) = web_sys::window() else { return };
    let watch = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        if let Some(t) = e.dyn_ref::<web_sys::TouchEvent>() {
            if t.touches().length() == 0 {
                let at = stamp_time(e.time_stamp());
                TOUCHES_GONE.with(|g| g.set(Some(at)));
            }
        }
    });
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    opts.set_passive(true);
    for kind in ["touchend", "touchcancel"] {
        let _ = w.add_event_listener_with_callback_and_add_event_listener_options(
            kind,
            watch.as_ref().unchecked_ref(),
            &opts,
        );
    }
    watch.forget();
}

/// When no finger was left on the screen, once.
pub fn take_touches_gone() -> Option<crate::platform::Instant> {
    TOUCHES_GONE.with(|g| g.take())
}

/// A touch on the page's canvas, from the browser's touch events.
pub enum TouchIn {
    /// A finger came down, moved or went up (`id` is the touch's
    /// identifier + 1; `x`, `y` in device pixels on the canvas).
    Point {
        id: u64,
        phase: crate::input::Phase,
        x: f32,
        y: f32,
        time: crate::platform::Instant,
    },
    /// The fingers still on the screen after one went up: any other the app
    /// holds lost its release.
    Left(Vec<u64>),
}

thread_local! {
    static TOUCH_IN: RefCell<Vec<TouchIn>> = const { RefCell::new(Vec::new()) };
}

/// Read touches from the browser's touch events rather than winit's pointer
/// events: a finger's `pointerup` can go missing (a key then stays down
/// while other fingers hold the screen), and each `touchend` lists the
/// fingers still down, so the ones that are not are let go at once.
pub fn install_touch_input() {
    use crate::input::Phase;
    let Some(w) = web_sys::window() else { return };
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    opts.set_passive(true);
    for (kind, phase) in [
        ("touchstart", Phase::Down),
        ("touchmove", Phase::Move),
        ("touchend", Phase::Up),
        ("touchcancel", Phase::Up),
    ] {
        let f = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let Some(t) = e.dyn_ref::<web_sys::TouchEvent>() else {
                return;
            };
            // only the canvas's (a touch's events go where it began)
            let Some(canvas) = e
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                return;
            };
            let r = canvas.get_bounding_client_rect();
            let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
            let time = stamp_time(e.time_stamp());
            let changed = t.changed_touches();
            TOUCH_IN.with(|q| {
                let mut q = q.borrow_mut();
                for i in 0..changed.length() {
                    let Some(c) = changed.get(i) else { continue };
                    q.push(TouchIn::Point {
                        id: c.identifier() as u64 + 1,
                        phase,
                        x: ((c.client_x() as f64 - r.left()) * dpr) as f32,
                        y: ((c.client_y() as f64 - r.top()) * dpr) as f32,
                        time,
                    });
                }
                if phase == Phase::Up {
                    let left = t.touches();
                    q.push(TouchIn::Left(
                        (0..left.length())
                            .filter_map(|i| left.get(i))
                            .map(|c| c.identifier() as u64 + 1)
                            .collect(),
                    ));
                }
            });
        });
        let _ = w.add_event_listener_with_callback_and_add_event_listener_options(
            kind,
            f.as_ref().unchecked_ref(),
            &opts,
        );
        f.forget();
    }
}

/// The touches since the last call.
pub fn take_touches() -> Vec<TouchIn> {
    TOUCH_IN.with(|q| std::mem::take(&mut *q.borrow_mut()))
}
