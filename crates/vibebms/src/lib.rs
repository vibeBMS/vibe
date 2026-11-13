//! vibeBMS application: window, renderer, input, scenes.

pub mod app;
pub mod config;
pub mod gfx;
pub mod input;
pub mod library;
pub mod paths;
pub mod platform;
pub mod scene;
pub mod skin;
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
