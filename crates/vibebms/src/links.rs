//! Other links for difficulty-table songs, by the chart's MD5
//! (`bmseeker::links`): the tables the player reads, and the Stella
//! uploader's list (loaded in the background once per start; natively from a
//! cache a day old at most). They fill missing or dead links, and are tried
//! in turn when a song's own link does not download.

use bmseeker::links::{Links, STELLA_UPLOADS};
use bmseeker::table::DifficultyTable;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

/// How long the cached list is used before it is fetched again.
#[cfg(not(target_arch = "wasm32"))]
const FRESH_S: i64 = 86_400;

/// Its cache file.
const STELLA_FILE: &str = "links-stella.json";

#[derive(Default)]
pub struct LinkLists {
    /// Everything known: the tables' links, then the list's.
    pub links: Links,
    stella: Links,
    rx: Option<Receiver<Links>>,
}

impl LinkLists {
    /// Load the Stella uploader's list in the background (its cache in
    /// `cache`).
    pub fn start(&mut self, cache: &Path) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.rx = Some(rx);
        load(cache.join(STELLA_FILE), move |l| {
            let _ = tx.send(l);
        });
    }

    /// True when the list has just loaded (call [`Self::rebuild`]).
    pub fn poll(&mut self) -> bool {
        let Some(links) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        log::info!("Stella uploader: links for {} charts", links.len());
        self.stella = links;
        self.rx = None;
        true
    }

    /// Everything known again, from `tables` and the list.
    pub fn rebuild(&mut self, tables: &[DifficultyTable]) {
        self.links = Links::from_tables(tables).and(self.stella.clone());
    }
}

/// The list: from its fresh cache, else fetched (and cached; the stale cache
/// when the fetch fails).
fn load(file: PathBuf, deliver: impl FnOnce(Links) + Send + 'static) {
    use crate::platform::vfs;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let now = crate::platform::SystemTime::now()
            .duration_since(crate::platform::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (mtime, size) = vfs::stamp(&file);
        if size > 0 && now - mtime < FRESH_S {
            if let Ok(b) = vfs::read(&file) {
                return deliver(Links::from_stella(&b));
            }
        }
    }
    // web: stellabms.xyz sends no CORS headers
    #[cfg(target_arch = "wasm32")]
    let url = crate::hosts::via_proxy(STELLA_UPLOADS);
    #[cfg(not(target_arch = "wasm32"))]
    let url = STELLA_UPLOADS.to_string();
    crate::platform::http_get(&url, move |r| match r {
        Ok(b) => {
            #[cfg(not(target_arch = "wasm32"))]
            let _ = vfs::write(&file, &b);
            deliver(Links::from_stella(&b))
        }
        Err(e) => {
            log::warn!("{STELLA_UPLOADS}: {e}");
            deliver(
                vfs::read(&file)
                    .map(|b| Links::from_stella(&b))
                    .unwrap_or_default(),
            )
        }
    });
}
