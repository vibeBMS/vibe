//! The in-app browser: a chart's download page in the system web view.
//! What the page downloads goes to `<root>/downloads/` without asking and is
//! brought into the library (`import`). Builds without one (the web, Linux
//! without WebKitGTK) get a `Downloads` that does nothing: their pages open
//! in the system browser instead (`IN_APP_BROWSER`).

#[cfg(target_os = "android")]
mod android;
#[cfg(in_app_browser)]
mod imports;
#[cfg(all(in_app_browser, not(target_os = "android")))]
mod webview;

#[cfg(target_os = "android")]
pub use android::Downloads;
#[cfg(all(in_app_browser, not(target_os = "android")))]
pub use webview::Downloads;

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

    pub fn poll(&mut self, _ctx: &mut crate::app::Ctx) {}
}
