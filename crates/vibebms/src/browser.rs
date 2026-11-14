//! The in-app browser (desktop, iOS): the system web view for a chart's
//! download page, in a window of its own (on iOS over the game, with Back
//! and Close buttons the page gets). What the page downloads goes to
//! `<root>/downloads/` without asking and is brought into the library
//! (`import`): archives are unpacked, a 差分 chart goes into its song's
//! folder. After a song arrives the browser moves on to the next page given
//! (the separate chart's).

use crate::app::Ctx;
use crate::import::{self, Imported};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;
use wry::{WebView, WebViewBuilder};

enum Event {
    Started(String),
    Finished(PathBuf),
    Failed(String),
    /// The page's Close button (iOS).
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    Closed,
}

struct Window {
    #[cfg(not(any(target_os = "linux", target_os = "ios")))]
    window: winit::window::Window,
    #[cfg(target_os = "linux")]
    window: gtk::Window,
    #[cfg(target_os = "linux")]
    closed: Rc<std::cell::Cell<bool>>,
    webview: WebView,
    rx: Receiver<Event>,
}

type ImportResult = Result<Imported, String>;

/// The browser window, the pages still to show and the imports running.
#[derive(Default)]
pub struct Downloads {
    window: Option<Window>,
    next: VecDeque<String>,
    imports: Option<(Sender<ImportResult>, Receiver<ImportResult>)>,
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

/// The file name a download gets: the one the page suggests, else the last
/// part of the URL.
fn download_name(url: &str, suggested: &Path) -> String {
    suggested
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .or_else(|| {
            url.split(['?', '#'])
                .next()
                .and_then(|u| u.rsplit('/').next())
                .filter(|n| !n.is_empty())
                .map(String::from)
        })
        .unwrap_or_else(|| "download".into())
        .replace(['/', '\\', ':'], "_")
}

/// iOS: the web view covers the game and has no window frame, so every page
/// gets Back and Close buttons along the bottom.
#[cfg(target_os = "ios")]
const BUTTONS_JS: &str = r#"(function () {
  if (window.top !== window) return;
  function add() {
    if (document.getElementById("__vibebms_bar")) return;
    var bar = document.createElement("div");
    bar.id = "__vibebms_bar";
    bar.style.cssText = "position:fixed;left:0;right:0;bottom:0;z-index:2147483647;display:flex;" +
      "gap:8px;justify-content:flex-end;padding:8px calc(8px + env(safe-area-inset-right)) " +
      "calc(8px + env(safe-area-inset-bottom)) 8px;background:rgba(20,24,36,.92);" +
      "font:600 15px -apple-system,sans-serif";
    function button(text, f) {
      var b = document.createElement("button");
      b.textContent = text;
      b.style.cssText = "color:#e2e8f5;background:#2a3044;border:1px solid #343e5c;" +
        "border-radius:8px;padding:8px 14px;font:inherit";
      b.onclick = f;
      bar.appendChild(b);
    }
    button("◀ Back", function () { history.back(); });
    button("Close", function () { window.ipc.postMessage("close"); });
    document.documentElement.appendChild(bar);
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", add);
  else add();
})();"#;

fn builder<'a>(url: &str, downloads: PathBuf, tx: Sender<Event>) -> WebViewBuilder<'a> {
    // where each download went (macOS does not report it when done)
    let paths: Rc<RefCell<HashMap<String, PathBuf>>> = Rc::default();
    let (p1, p2) = (paths.clone(), paths);
    let tx2 = tx.clone();
    #[cfg(target_os = "ios")]
    let tx3 = tx.clone();
    let b = WebViewBuilder::new()
        .with_url(url)
        .with_download_started_handler(move |url, path| {
            let name = download_name(&url, path);
            let _ = std::fs::create_dir_all(&downloads);
            *path = free_path(&downloads, &name);
            p1.borrow_mut().insert(url, path.clone());
            let _ = tx.send(Event::Started(name));
            true
        })
        .with_download_completed_handler(move |url, path, ok| {
            let path = path
                .filter(|p| !p.as_os_str().is_empty())
                .or_else(|| p2.borrow_mut().remove(&url));
            let _ = tx2.send(match (ok, path) {
                (true, Some(p)) => Event::Finished(p),
                _ => Event::Failed(url),
            });
        });
    #[cfg(target_os = "ios")]
    let b = b
        .with_initialization_script(BUTTONS_JS)
        .with_ipc_handler(move |req| {
            if req.body() == "close" {
                let _ = tx3.send(Event::Closed);
            }
        });
    b
}

impl Window {
    /// iOS: over the game window (UIKit has no second window to give it).
    #[cfg(target_os = "ios")]
    fn open(
        _el: &ActiveEventLoop,
        game: &winit::window::Window,
        url: &str,
        downloads: PathBuf,
    ) -> Result<Window, String> {
        let (tx, rx) = channel();
        let webview = builder(url, downloads, tx)
            .build(game)
            .map_err(|e| e.to_string())?;
        Ok(Window { webview, rx })
    }

    #[cfg(not(any(target_os = "linux", target_os = "ios")))]
    fn open(
        el: &ActiveEventLoop,
        _game: &winit::window::Window,
        url: &str,
        downloads: PathBuf,
    ) -> Result<Window, String> {
        let attrs = winit::window::Window::default_attributes()
            .with_title("vibeBMS — download")
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 800.0));
        let window = el.create_window(attrs).map_err(|e| e.to_string())?;
        let (tx, rx) = channel();
        let webview = builder(url, downloads, tx)
            .build(&window)
            .map_err(|e| e.to_string())?;
        Ok(Window {
            window,
            webview,
            rx,
        })
    }

    /// Linux: a GTK window (the web view is WebKitGTK; this works on X11
    /// and Wayland alike), its events run from `pump`.
    #[cfg(target_os = "linux")]
    fn open(
        _el: &ActiveEventLoop,
        _game: &winit::window::Window,
        url: &str,
        downloads: PathBuf,
    ) -> Result<Window, String> {
        use gtk::prelude::*;
        use wry::WebViewBuilderExtUnix;
        if !gtk::is_initialized_main_thread() {
            gtk::init().map_err(|e| e.to_string())?;
        }
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("vibeBMS — download");
        window.set_default_size(1100, 800);
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&vbox);
        let (tx, rx) = channel();
        let webview = builder(url, downloads, tx)
            .build_gtk(&vbox)
            .map_err(|e| e.to_string())?;
        let closed = Rc::new(std::cell::Cell::new(false));
        let c = closed.clone();
        window.connect_delete_event(move |_, _| {
            c.set(true);
            gtk::glib::Propagation::Proceed
        });
        window.show_all();
        Ok(Window {
            window,
            closed,
            webview,
            rx,
        })
    }

    fn id(&self) -> Option<WindowId> {
        #[cfg(not(any(target_os = "linux", target_os = "ios")))]
        return Some(self.window.id());
        #[cfg(any(target_os = "linux", target_os = "ios"))]
        None
    }

    fn raise(&self) {
        #[cfg(not(any(target_os = "linux", target_os = "ios")))]
        self.window.focus_window();
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::*;
            self.window.present();
        }
    }

    /// Run the window's own events (Linux); false once it was closed.
    fn pump(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            !self.closed.get()
        }
        #[cfg(not(target_os = "linux"))]
        true
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::*;
            self.window.close();
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        }
    }
}

impl Downloads {
    /// Show `pages` one after another: the first now, the next after a
    /// download from it has arrived.
    pub fn browse(
        &mut self,
        el: &ActiveEventLoop,
        game: &winit::window::Window,
        ctx: &mut Ctx,
        pages: Vec<String>,
    ) {
        let mut pages: VecDeque<String> = pages.into();
        let Some(first) = pages.pop_front() else {
            return;
        };
        self.next = pages;
        if let Some(w) = &self.window {
            let _ = w.webview.load_url(&first);
            w.raise();
            return;
        }
        match Window::open(el, game, &first, ctx.paths.root.join("downloads")) {
            Ok(w) => self.window = Some(w),
            Err(e) => {
                // no web view (e.g. WebKitGTK missing): the system browser
                log::warn!("in-app browser: {e}");
                let opened = std::iter::once(first)
                    .chain(self.next.drain(..))
                    .filter(|u| crate::platform::open_url(u))
                    .count();
                if opened == 0 {
                    ctx.toast("Could not open a browser");
                }
            }
        }
    }

    /// Events of the browser window; true when `id` is that window.
    pub fn window_event(&mut self, id: WindowId, event: &WindowEvent) -> bool {
        let Some(w) = &self.window else { return false };
        if w.id() != Some(id) {
            return false;
        }
        match event {
            WindowEvent::CloseRequested => self.window = None,
            WindowEvent::Resized(s) => {
                let _ = w.webview.set_bounds(wry::Rect {
                    position: winit::dpi::PhysicalPosition::new(0, 0).into(),
                    size: (*s).into(),
                });
            }
            _ => {}
        }
        true
    }

    /// Once per frame: the browser's events, downloads that finished,
    /// imports that are done.
    pub fn poll(&mut self, ctx: &mut Ctx) {
        let mut finished = Vec::new();
        let mut closed = false;
        if let Some(w) = &self.window {
            if !w.pump() {
                self.window = None;
            }
        }
        if let Some(w) = &self.window {
            for ev in w.rx.try_iter() {
                match ev {
                    Event::Started(name) => ctx.toast(format!("Downloading {name}…")),
                    Event::Failed(url) => ctx.toast(format!("Download failed: {url}")),
                    Event::Finished(p) => finished.push(p),
                    Event::Closed => closed = true,
                }
            }
        }
        if closed {
            self.window = None;
        }
        let library = ctx.paths.root.join("bms");
        for file in finished {
            let (tx, _) = self.imports.get_or_insert_with(channel);
            let tx = tx.clone();
            let lib = library.clone();
            crate::platform::spawn("import", move || {
                let _ = tx.send(import::import(&file, &lib));
            });
        }
        let done: Vec<_> = self
            .imports
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default();
        for r in done {
            self.imported(ctx, r);
        }
    }

    fn imported(&mut self, ctx: &mut Ctx, r: ImportResult) {
        let done = import::report(ctx, r);
        // on to the separate chart's page
        if done.song_arrived {
            if let (Some(url), Some(w)) = (self.next.pop_front(), &self.window) {
                let _ = w.webview.load_url(&url);
                ctx.toast("Now the chart: download it from this page");
            }
        }
        let mut rescan = done.rescan;
        #[cfg(target_os = "ios")]
        if let Some((chart, _)) = done.ask {
            ctx.toast(format!(
                "{}: several songs could be it; it stays in downloads",
                import::file_name(&chart)
            ));
        }
        #[cfg(not(target_os = "ios"))]
        if let Some((chart, folders)) = done.ask {
            // several songs could be it: let the user pick the folder
            let name = import::file_name(&chart);
            let mut dialog =
                rfd::FileDialog::new().set_title(format!("Which song folder is {name} for?"));
            if let Some((d, _)) = folders.first() {
                dialog = dialog.set_directory(d);
            }
            if let Some(dir) = dialog.pick_folder() {
                if crate::platform::vfs::rename(&chart, &dir.join(&name)) {
                    ctx.toast(format!("{name} → {}", import::file_name(&dir)));
                    rescan = true;
                }
            }
        }
        if rescan {
            ctx.ensure_scanned("bms");
            ctx.start_scan();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_names() {
        assert_eq!(
            download_name("https://x.org/a/song.zip?dl=1", Path::new("")),
            "song.zip"
        );
        assert_eq!(
            download_name("https://x.org/f", Path::new("/tmp/Song.7z")),
            "Song.7z"
        );
        assert_eq!(download_name("https://x.org/", Path::new("")), "download");
    }
}
