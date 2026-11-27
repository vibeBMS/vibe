//! Screenshots (Print Screen, or song select's SCREENSHOT button): the
//! frame as a PNG named for when it was taken and what it shows,
//! `YYYYMMDD-HHMMSS_<title>_<difficulty>_<lamp>_<EX>.png`. Desktop and
//! mobile keep it in `<root>/screenshots/` (the app's own storage on
//! Android and iOS: there is no share sheet yet); the web build offers it
//! as a download.

use crate::platform::Job;
use std::path::{Path, PathBuf};

/// The folder screenshots go to.
pub fn dir(root: &Path) -> PathBuf {
    root.join("screenshots")
}

/// A part of a file name: letters and digits of any script, spaces, `-`,
/// `.`, `(`, `)`, `[`, `]` kept, anything else (path separators, `:`,
/// `*`, `?`, quotes, control characters) as `_`; at most 60 characters,
/// no space or dot at either end.
fn sanitize(s: &str) -> String {
    let kept = |c: char| c.is_alphanumeric() || " -.()[]".contains(c);
    let s: String = s
        .chars()
        .map(|c| if kept(c) { c } else { '_' })
        .take(60)
        .collect();
    s.trim_matches([' ', '.']).to_string()
}

/// The file name of a screenshot taken at `stamp` (`YYYYMMDD-HHMMSS`)
/// showing `parts` (the scene's: title, difficulty, lamp, EX score; empty
/// ones left out).
pub fn file_name(stamp: &str, parts: &[String]) -> String {
    let mut name = stamp.to_string();
    for p in parts.iter().map(|p| sanitize(p)).filter(|p| !p.is_empty()) {
        name.push('_');
        name.push_str(&p);
    }
    name + ".png"
}

/// The local time now as a screenshot's stamp.
pub fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// A chart's `#DIFFICULTY` as a name (empty when it has none).
pub fn difficulty(d: i32) -> String {
    crate::config::DifficultyFilter::ALL
        .into_iter()
        .find(|f| *f as i32 == d && d > 0)
        .map_or(String::new(), |f| f.label().to_string())
}

/// Encode `img` and keep it as `name` (in the background): what the job
/// sends is where it went, or why it could not.
pub fn save(img: image::RgbaImage, name: String, root: &Path) -> Job<Result<String, String>> {
    let dir = dir(root);
    Job::spawn("screenshot", move |_| {
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        keep(&dir, &name, png)
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn keep(dir: &Path, name: &str, png: Vec<u8>) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// The web build: a download of it.
#[cfg(target_arch = "wasm32")]
fn keep(_dir: &Path, name: &str, png: Vec<u8>) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    let fail = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(png.as_slice()));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob =
        web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(fail)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(fail)?;
    let doc = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let a = doc.create_element("a").map_err(fail)?;
    a.set_attribute("href", &url).map_err(fail)?;
    a.set_attribute("download", name).map_err(fail)?;
    a.dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| "no link element".to_string())?
        .click();
    // (after the download has taken it)
    crate::platform::after(crate::platform::Duration::from_secs(10), move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    Ok(format!("{name} (downloaded)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let parts = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            file_name(
                "20261117-153000",
                &parts(&["Song: A/B *mix*", "ANOTHER", "HARD", "1500"])
            ),
            "20261117-153000_Song_ A_B _mix__ANOTHER_HARD_1500.png"
        );
        // other scripts stay; empty parts and dots at the ends go
        assert_eq!(
            file_name("20261117-153000", &parts(&["..星の夢..", "", "\u{7}"])),
            "20261117-153000_星の夢__.png"
        );
        assert_eq!(file_name("x", &[]), "x.png");
        assert_eq!(sanitize(&"a".repeat(100)).len(), 60);
        assert_eq!(difficulty(4), "ANOTHER");
        assert_eq!(difficulty(0), "");
    }
}
