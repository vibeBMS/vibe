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

/// Android entry point (build with `cargo apk`/`xbuild`, see docs/BUILDING.md).
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    std::env::set_var("VIBEBMS_ROOT", app.internal_data_path().unwrap_or_default());
    let event_loop = winit::event_loop::EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("event loop");
    let mut a = app::App::new(paths::Paths::detect());
    let _ = event_loop.run_app(&mut a);
}
