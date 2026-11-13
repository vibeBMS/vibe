//! Browser integration: pickers for a BMS folder, loose files or song pack
//! zips -> virtual file system.

use std::cell::RefCell;
use std::path::PathBuf;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

thread_local! {
    /// Data root the pickers mount under (`<root>/bms/`).
    static ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Hook up the pickers of `web/index.html`:
/// `<input id="bms-input" webkitdirectory>` (a folder) and
/// `<input id="bms-files" multiple>` (files and `.zip` song packs; iPad
/// Safari cannot pick folders).
pub fn install_file_input(root: PathBuf) {
    ROOT.with(|r| *r.borrow_mut() = Some(root.clone()));
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    for id in ["bms-input", "bms-files"] {
        let Some(input) = doc
            .get_element_by_id(id)
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        else {
            log::warn!("no #{id} element");
            continue;
        };
        let input2 = input.clone();
        let root = root.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            if let Some(files) = input2.files() {
                log::info!("picked {} files", files.length());
                crate::platform::vfs::add_files(&root, &files, "picked");
                for i in 0..files.length() {
                    if let Some(f) = files.get(i) {
                        if f.name().to_lowercase().ends_with(".zip") {
                            mount_zip_file(f);
                        }
                    }
                }
            }
        });
        input.set_onchange(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
}

/// Read a picked zip and unpack it.
fn mount_zip_file(f: web_sys::File) {
    wasm_bindgen_futures::spawn_local(async move {
        let name = f.name();
        match wasm_bindgen_futures::JsFuture::from(f.array_buffer()).await {
            Ok(buf) => {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                mount_zip(&name, &bytes);
            }
            Err(e) => log::warn!("{name}: {e:?}"),
        }
    });
}

/// Unpack a song pack zip into the library (the page's demo button calls
/// this with a downloaded zip). Returns the number of files added.
#[wasm_bindgen(js_name = mountZip)]
pub fn mount_zip(name: &str, data: &[u8]) -> usize {
    let Some(root) = ROOT.with(|r| r.borrow().clone()) else {
        return 0;
    };
    let n = crate::platform::vfs::add_zip(&root, name, data.to_vec());
    log::info!("{name}: {n} files");
    n
}

thread_local! {
    static BUTTONS_SHOWN: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Show or hide the page's picker buttons (`#bar`; hidden while playing, so
/// they do not cover the touch keys). A bar the user closed stays closed.
pub fn show_page_buttons(show: bool) {
    if BUTTONS_SHOWN.with(|b| b.replace(Some(show))) == Some(show) {
        return;
    }
    let Some(bar) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("bar"))
    else {
        return;
    };
    if show {
        let _ = bar.remove_attribute("data-playing");
    } else {
        let _ = bar.set_attribute("data-playing", "");
    }
}
