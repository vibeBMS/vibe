//! Android entry point. Charts, config and themes live in the app's
//! external files directory (`Android/data/io.github.vibebms/files`), which
//! is reachable over USB / file managers, falling back to internal storage.

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("vibeBMS"),
    );
    let root = app
        .external_data_path()
        .or_else(|| app.internal_data_path())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let paths = vibebms_app::paths::Paths::at(root);
    let event_loop = winit::event_loop::EventLoop::<vibebms_app::app::UserEvent>::with_user_event()
        .with_android_app(app)
        .build()
        .expect("event loop");
    let mut a = vibebms_app::app::App::with_proxy(paths, event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut a) {
        log::error!("event loop: {e}");
    }
}
