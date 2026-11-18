//! vibeBMS application: window, renderer, input, scenes.

#[macro_use]
pub mod i18n;
pub mod app;
pub mod archive;
pub mod browser;
pub mod bulk;
pub mod config;
pub mod courses;
pub mod formats;
pub mod gfx;
pub mod hosts;
#[cfg(not(target_arch = "wasm32"))]
pub mod import;
pub mod input;
pub mod integrations;
pub mod ir;
pub mod library;
pub mod links;
pub mod loudness;
pub mod lr2db;
pub mod paths;
pub mod perspective;
pub mod placement;
pub mod platform;
pub mod profiles;
pub mod replay;
pub mod scene;
pub mod scoredb;
pub mod screenshot;
pub mod skin;
pub mod skinstore;
pub mod songfetch;
pub mod sysound;
pub mod theme;
pub mod touch;
pub mod ui;
pub mod update;
pub mod video;
#[cfg(target_arch = "wasm32")]
pub mod web;

/// Entry point for desktop builds.
pub fn run() {
    app::run();
}
