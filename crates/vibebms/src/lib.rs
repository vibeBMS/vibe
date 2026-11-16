//! vibeBMS application: window, renderer, input, scenes.

pub mod app;
pub mod archive;
#[cfg(all(
    feature = "browser",
    any(windows, target_os = "macos", target_os = "linux", target_os = "ios")
))]
pub mod browser;
#[cfg(target_os = "android")]
#[path = "browser_android.rs"]
pub mod browser;
pub mod config;
pub mod gfx;
pub mod hosts;
#[cfg(not(target_arch = "wasm32"))]
pub mod import;
pub mod input;
pub mod library;
pub mod links;
pub mod paths;
pub mod placement;
pub mod platform;
pub mod replay;
pub mod scene;
pub mod skin;
pub mod songfetch;
pub mod theme;
pub mod touch;
pub mod ui;
pub mod video;
#[cfg(target_arch = "wasm32")]
pub mod web;

/// Entry point for desktop builds.
pub fn run() {
    app::run();
}
