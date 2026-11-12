//! vibeBMS application: window, renderer, input, scenes.

pub mod app;
pub mod config;
pub mod gfx;
pub mod input;
pub mod library;
pub mod paths;
pub mod scene;
pub mod theme;
pub mod touch;
pub mod ui;

/// Entry point for desktop builds.
pub fn run() {
    app::run();
}
