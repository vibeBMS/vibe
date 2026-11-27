//! The in-app browser on the desktop and iOS (wry): the page in a window of
//! its own (on iOS over the game, with Back and Close buttons the page
//! gets). Archives it downloads are unpacked, a 差分 chart goes into its
//! song's folder. After a song arrives the browser moves on to the next page
//! given (the separate chart's).
//!
//! A page's download comes into the library once the player has said yes
//! to it (`gate`): it is saved aside first, and asked about when it has
//! arrived. The question is the game's, in the game's window, which the
//! browser's window is in front of (on iOS the web view covers it): the
//! browser is out of the way while it is asked. The web view says neither
//! how large a download is nor lets one be stopped, so the size limit
//! holds when the file has arrived: one over it is deleted.

use super::gate::{Arrival, Gate};
use super::imports::Imports;
use super::MAX_DOWNLOAD;
use crate::ctx::Ctx;
use crate::gfx::Draw;
use crate::input::InputEv;
use crate::ui::Notice;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;
use wry::{WebView, WebViewBuilder};

enum Event {
    /// A download is being saved (its name).
    Started(String),
    /// One was refused, another being under way (its name).
    Busy(String),
    /// The download from this address ended, whole or not.
    Ended(String, bool),
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

/// The browser window, the pages still to show and the imports running.
#[derive(Default)]
pub struct Downloads {
    window: Option<Window>,
    next: VecDeque<String>,
    imports: Imports,
    /// What the pages may save (the web view's handlers ask it).
    gate: Rc<RefCell<Gate>>,
    /// The question about the download asked about, while it is up.
    notice: Option<Notice>,
    /// "One at a time" was said since a download was last begun.
    told_busy: bool,
    /// The slots an earlier run left were looked for.
    cleared: bool,
}

/// The longest file name a download gets (bytes: file systems take 255,
/// and ` (2)` may follow).
const NAME_MAX: usize = 200;

/// The file name a download gets once the player has said yes to it: the
/// one the page suggests, else the last part of the URL; as one name a
/// file system takes (`safe_name`), of at most `NAME_MAX` bytes with its
/// extension kept. The server chooses it.
fn download_name(url: &str, suggested: &Path) -> String {
    let name = suggested
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .or_else(|| {
            url.split(['?', '#'])
                .next()
                .and_then(|u| u.rsplit('/').next())
                .map(String::from)
        })
        .unwrap_or_default();
    let name = crate::archive::safe_name(&name);
    if name.len() <= NAME_MAX {
        return name;
    }
    let ext = match name.rfind('.') {
        Some(i) if name.len() - i <= 16 => &name[i..],
        _ => "",
    };
    let mut end = NAME_MAX - ext.len();
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ext}", name[..end].trim_end_matches(['.', ' ']))
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

fn builder<'a>(
    url: &str,
    downloads: PathBuf,
    tx: Sender<Event>,
    gate: Rc<RefCell<Gate>>,
) -> WebViewBuilder<'a> {
    let tx2 = tx.clone();
    #[cfg(target_os = "ios")]
    let tx3 = tx.clone();
    let b = WebViewBuilder::new()
        .with_url(url)
        .with_download_started_handler(move |url, path| {
            let name = download_name(&url, path);
            let slot = (gate.borrow_mut()).started(&url, &name, &downloads, Instant::now());
            let taken = slot.is_some();
            let _ = tx.send(match slot {
                Some(slot) => {
                    let _ = std::fs::create_dir_all(&downloads);
                    *path = slot;
                    Event::Started(name)
                }
                None => Event::Busy(name),
            });
            taken
        })
        .with_download_completed_handler(move |url, _, ok| {
            let _ = tx2.send(Event::Ended(url, ok));
        });
    #[cfg(target_os = "ios")]
    let b = b
        .with_initialization_script(BUTTONS_JS)
        // a OneDrive share downloads without a press
        .with_initialization_script(bmseeker::hosts::PRESS_DOWNLOAD_JS)
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
        gate: Rc<RefCell<Gate>>,
    ) -> Result<Window, String> {
        let (tx, rx) = channel();
        let webview = builder(url, downloads, tx, gate)
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
        gate: Rc<RefCell<Gate>>,
    ) -> Result<Window, String> {
        let attrs = winit::window::Window::default_attributes()
            .with_title(format!("vibeBMS — {}", tr!("download")))
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 800.0));
        let window = el.create_window(attrs).map_err(|e| e.to_string())?;
        let (tx, rx) = channel();
        let webview = builder(url, downloads, tx, gate)
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
        gate: Rc<RefCell<Gate>>,
    ) -> Result<Window, String> {
        use gtk::prelude::*;
        use wry::WebViewBuilderExtUnix;
        if !gtk::is_initialized_main_thread() {
            gtk::init().map_err(|e| e.to_string())?;
        }
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title(&format!("vibeBMS — {}", tr!("download")));
        window.set_default_size(1100, 800);
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&vbox);
        let (tx, rx) = channel();
        let webview = builder(url, downloads, tx, gate)
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

    /// Out of the way while the game asks about a download (its window is
    /// behind this one; on iOS the web view covers it), then back.
    fn show(&self, on: bool) {
        #[cfg(not(any(target_os = "linux", target_os = "ios")))]
        self.window.set_visible(on);
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::*;
            self.window.set_visible(on);
        }
        #[cfg(target_os = "ios")]
        let _ = self.webview.set_visible(on);
        if on {
            self.raise();
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
        let downloads = ctx.paths.root.join("downloads");
        match Window::open(el, game, &first, downloads, self.gate.clone()) {
            Ok(w) => self.window = Some(w),
            Err(e) => {
                // no web view (e.g. WebKitGTK missing): the system browser
                log::warn!("in-app browser: {e}");
                let opened = std::iter::once(first)
                    .chain(self.next.drain(..))
                    .filter(|u| crate::platform::open_url(u))
                    .count();
                if opened == 0 {
                    ctx.toast(tr!("Could not open a browser"));
                }
            }
        }
    }

    /// The browser is up (its window may have the focus).
    pub fn is_open(&self) -> bool {
        self.window.is_some()
    }

    /// The window is gone: what its pages were saving or asking goes with
    /// it.
    fn close(&mut self) {
        self.window = None;
        self.notice = None;
        *self.gate.borrow_mut() = Gate::default();
    }

    /// Events of the browser window; true when `id` is that window.
    pub fn window_event(&mut self, id: WindowId, event: &WindowEvent) -> bool {
        let Some(w) = &self.window else { return false };
        if w.id() != Some(id) {
            return false;
        }
        match event {
            WindowEvent::CloseRequested => self.close(),
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

    /// While the player is asked about a download, `ev` answers the
    /// question or is kept from the scene (true). A yes brings the file
    /// into the library, a no deletes it.
    pub fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> bool {
        let Some(n) = self.notice.as_mut() else {
            return false;
        };
        // (keys let go unseen still reach the scene)
        if matches!(ev, InputEv::Released { .. }) {
            return false;
        }
        if let Some(yes) = n.input(ev, &ctx.config.keys) {
            self.notice = None;
            let downloads = ctx.paths.root.join("downloads");
            if let Some(file) = self.gate.borrow_mut().answer(yes, &downloads) {
                let songs = ctx.lib.songs.clone();
                self.imports.start(file, ctx.paths.root.clone(), songs);
            }
            if let Some(w) = &self.window {
                w.show(true);
            }
        }
        true
    }

    /// The question about the download that arrived, over the game.
    pub fn draw(&mut self, ctx: &Ctx, d: &mut Draw) {
        let gate = self.gate.borrow();
        let (Some(n), Some(f)) = (self.notice.as_mut(), gate.asked()) else {
            return;
        };
        let text = format!(
            "{}\n{}\n{:.1} MB\n\n{}",
            f.name,
            tr!("from {host}", host = f.host),
            f.size as f64 / 1_048_576.0,
            tr!("OK brings it into your library. Otherwise it is deleted.")
        );
        n.draw(ctx, d, tr!("Keep this download?"), &text, tr!("Delete"));
    }

    /// Once per frame: the browser's events, downloads that finished,
    /// imports that are done.
    pub fn poll(&mut self, ctx: &mut Ctx) {
        if !std::mem::replace(&mut self.cleared, true) {
            super::gate::clear_slots(&ctx.paths.root.join("downloads"));
        }
        let mut closed = false;
        if let Some(w) = &self.window {
            closed = !w.pump();
            for ev in w.rx.try_iter() {
                match ev {
                    Event::Started(name) => {
                        self.told_busy = false;
                        ctx.toast(tr!("Downloading {name}…", name = name));
                    }
                    Event::Busy(name) if !self.told_busy => {
                        self.told_busy = true;
                        ctx.toast(tr!(
                            "One download at a time: {name} was not started",
                            name = name
                        ));
                    }
                    Event::Busy(_) => {}
                    Event::Ended(url, ok) => {
                        let now = Instant::now();
                        let arrival = (self.gate.borrow_mut()).ended(&url, ok, MAX_DOWNLOAD, now);
                        match arrival {
                            Arrival::Ask => {
                                w.show(false);
                                self.notice = Some(Notice::default());
                            }
                            Arrival::TooLarge(name) => ctx.toast(tr!(
                                "{name} is larger than {gb} GB: not kept",
                                name = name,
                                gb = MAX_DOWNLOAD >> 30
                            )),
                            Arrival::Failed => ctx.toast(tr!("Download failed: {url}", url = url)),
                            Arrival::Other => {}
                        }
                    }
                    Event::Closed => closed = true,
                }
            }
        }
        if closed {
            self.close();
        }
        let (next, window) = (&mut self.next, &self.window);
        self.imports.poll(ctx, |ctx| {
            if let (Some(url), Some(w)) = (next.pop_front(), window) {
                let _ = w.webview.load_url(&url);
                ctx.toast(tr!("Now the chart: download it from this page"));
            }
        });
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

    /// The name is the server's to choose: it stays one name inside the
    /// downloads folder, with nothing a file system refuses, of a length
    /// one takes.
    #[test]
    fn a_download_name_is_one_safe_name() {
        let dir = Path::new("/root/downloads");
        for (url, suggested) in [
            ("https://x.org/a/..", ""),
            ("https://x.org/a/%2e%2e", ".."),
            ("https://x.org/", "..\\..\\evil.bat"),
            ("https://x.org/", "a\u{0}b\n<c>|d?.zip"),
            ("https://x.org/C:evil.zip", ""),
            ("https://x.org/...", ""),
        ] {
            let name = download_name(url, Path::new(suggested));
            let bad = |c: char| c.is_control() || "/\\:*?\"<>|".contains(c);
            assert!(!name.is_empty() && !name.contains(bad), "{name:?}");
            assert!(!name.starts_with('.') && !name.ends_with('.'), "{name:?}");
            assert_eq!(dir.join(&name).parent(), Some(dir), "{name:?}");
        }
        let long = format!("{}.tar.gz", "あ".repeat(100_000));
        let name = download_name("https://x.org/", Path::new(&long));
        assert!(name.len() <= NAME_MAX && name.ends_with(".gz"), "{name}");
    }
}
