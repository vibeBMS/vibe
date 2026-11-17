// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    // ureq's trace level logs requests as sent, credentials too
    .filter_module("ureq", log::LevelFilter::Debug)
    .filter_module("ureq_proto", log::LevelFilter::Debug)
    .init();
    vibebms_app::run();
}
