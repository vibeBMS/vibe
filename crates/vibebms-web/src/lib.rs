//! Web entry point. Build with `tools/build_web.sh`, then serve `web/`.

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
    vibebms_app::run();
}
