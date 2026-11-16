//! Google Drive: a file downloads from its id (`confirm=t` skips the virus
//! scan page of big files); a folder's listing (`embeddedfolderview`)
//! names its archives, the best of which is taken.

use super::{
    archive_name, between, get_then, pick_file, unescape, Host, Link, Resolved, Target, Want,
};
use crate::config::Config;
use crate::net::{BoxTask, Ready};

pub struct Drive;

impl Host for Drive {
    fn name(&self) -> &'static str {
        "drive"
    }

    fn accepts(&self, link: &Link) -> bool {
        link.on("drive.google.com") || link.host == "docs.google.com"
    }

    fn open(&self, link: &Link, _: Option<&Want>, _: &Config) -> BoxTask<Resolved> {
        let Some(id) = drive_id(&link.url) else {
            return Ready::boxed(Resolved::Page);
        };
        if !link.lower.contains("/folders/") {
            return Ready::boxed(Resolved::File(Target::url(file_url(&id))));
        }
        get_then(
            format!("https://drive.google.com/embeddedfolderview?id={id}"),
            |body| {
                let page = String::from_utf8_lossy(&body);
                let files = folder_entries(&page);
                let best = pick_file(
                    files
                        .iter()
                        .filter(|(name, _)| archive_name(name))
                        .map(|(n, i)| (n.as_str(), (n, i))),
                );
                match best {
                    Some((name, id)) => {
                        Resolved::File(Target::named(file_url(id), Some(name.clone())))
                    }
                    None => Resolved::Page,
                }
            },
        )
    }
}

/// The file or folder id of a Google Drive link (`/file/d/ID`,
/// `/folders/ID`, `?id=ID`).
pub fn drive_id(u: &str) -> Option<String> {
    let rest = ["/d/", "/folders/", "id="]
        .iter()
        .find_map(|k| u.find(k).map(|i| &u[i + k.len()..]))?;
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!id.is_empty()).then_some(id)
}

fn file_url(id: &str) -> String {
    format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t")
}

/// (name, file id) of the files in an `embeddedfolderview` page.
fn folder_entries(page: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in page.split("class=\"flip-entry\"").skip(1) {
        let Some(id) = between(part, "/file/d/", "/") else {
            continue; // a sub-folder
        };
        if let Some(name) = between(part, "flip-entry-title\">", "<") {
            out.push((unescape(name), id.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::tests::resolve;

    #[test]
    fn ids() {
        assert_eq!(
            drive_id("https://drive.google.com/file/d/1AbC-d_9/view?usp=sharing").as_deref(),
            Some("1AbC-d_9")
        );
        assert_eq!(
            drive_id("https://drive.google.com/drive/folders/1UX?usp=x").as_deref(),
            Some("1UX")
        );
        assert_eq!(
            drive_id("https://drive.google.com/open?id=XYZ").as_deref(),
            Some("XYZ")
        );
    }

    #[test]
    fn files_and_folders() {
        let t = resolve("https://drive.google.com/file/d/X/view", &[]).unwrap();
        assert!(t.url.contains("id=X"));
        let page = br#"<div class="flip-entry" id="entry-A"><a href="https://drive.google.com/file/d/A/view?usp=drive_web"><div class="flip-entry-title">song.zip</div></a></div>
            <div class="flip-entry" id="entry-S"><a href="https://drive.google.com/drive/folders/S"><div class="flip-entry-title">sub</div></a></div>
            <div class="flip-entry" id="entry-B"><a href="https://drive.google.com/file/d/B/view?usp=drive_web"><div class="flip-entry-title">song_ogg.zip</div></a></div>
            <div class="flip-entry" id="entry-C"><a href="https://drive.google.com/file/d/C/view?usp=drive_web"><div class="flip-entry-title">song_wav.zip</div></a></div>"#;
        assert_eq!(folder_entries(&String::from_utf8_lossy(page)).len(), 3);
        let t = resolve(
            "https://drive.google.com/drive/folders/F",
            &[("https://drive.google.com/embeddedfolderview?id=F", page)],
        )
        .unwrap();
        assert!(t.url.contains("id=B"));
        assert_eq!(t.name.as_deref(), Some("song_ogg.zip"));
    }
}
