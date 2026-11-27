//! The in-app browser: a chart's download page in the system web view.
//! What the page downloads goes to `<root>/downloads/` and is brought into
//! the library (`import`): one file at a time, each once the player has
//! said yes to it, none larger than [`MAX_DOWNLOAD`]. Builds without one
//! (the web, Linux without WebKitGTK) get a `Downloads` that does nothing:
//! their pages open in the system browser instead (`IN_APP_BROWSER`).

#[cfg(target_os = "android")]
mod android;
#[cfg(all(in_app_browser, not(target_os = "android")))]
mod gate;
#[cfg(in_app_browser)]
mod imports;
#[cfg(all(in_app_browser, not(target_os = "android")))]
mod webview;

#[cfg(target_os = "android")]
pub use android::Downloads;
#[cfg(all(in_app_browser, not(target_os = "android")))]
pub use webview::Downloads;

/// Songs are downloaded in an in-app browser (desktop, Android, iOS).
pub const IN_APP_BROWSER: bool = cfg!(in_app_browser);

/// The largest file the in-app browser keeps (bytes): song packs run to
/// several GB, and a page must not be able to fill the disk.
#[cfg(in_app_browser)]
const MAX_DOWNLOAD: u64 = 16 << 30;

/// No in-app browser: nothing is ever shown in it.
#[cfg(not(in_app_browser))]
#[derive(Default)]
pub struct Downloads;

#[cfg(not(in_app_browser))]
impl Downloads {
    pub fn browse(
        &mut self,
        _el: &winit::event_loop::ActiveEventLoop,
        _game: &winit::window::Window,
        _ctx: &mut crate::app::Ctx,
        _pages: Vec<String>,
    ) {
    }

    pub fn window_event(
        &mut self,
        _id: winit::window::WindowId,
        _event: &winit::event::WindowEvent,
    ) -> bool {
        false
    }

    pub fn is_open(&self) -> bool {
        false
    }

    pub fn input(&mut self, _ctx: &mut crate::app::Ctx, _ev: &crate::input::InputEv) -> bool {
        false
    }

    pub fn draw(&mut self, _ctx: &crate::app::Ctx, _d: &mut crate::gfx::Draw) {}

    pub fn poll(&mut self, _ctx: &mut crate::app::Ctx) {}
}
