//! Browser integration: folder picker -> virtual file system.

use std::path::PathBuf;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Hook up `<input id="bms-input" type="file" webkitdirectory multiple>`
/// (see `web/index.html`). Picked files are mounted under `<root>/bms/`.
pub fn install_file_input(root: PathBuf) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(input) = doc
        .get_element_by_id("bms-input")
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        log::warn!("no #bms-input element; cannot open BMS folders");
        return;
    };
    let input2 = input.clone();
    let cb = Closure::<dyn FnMut()>::new(move || {
        if let Some(files) = input2.files() {
            log::info!("picked {} files", files.length());
            crate::platform::vfs::add_files(&root, &files);
        }
    });
    input.set_onchange(Some(cb.as_ref().unchecked_ref()));
    cb.forget();
}
