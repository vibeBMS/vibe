//! Links for difficulty-table songs whose own are missing or dead
//! (`bmseeker::links`): the lists load in the background once per start
//! (natively from a cache a day old at most), then fill the tables' links.

use bmseeker::links::{Links, MD5_URL_TSV, STELLA_UPLOADS};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

/// How long a cached list is used before it is fetched again.
#[cfg(not(target_arch = "wasm32"))]
const FRESH_S: i64 = 86_400;

#[derive(Default)]
pub struct LinkLists {
    pub links: Links,
    rx: Option<Receiver<Links>>,
}

/// One list: where it is, its cache file, and its format.
struct List {
    url: &'static str,
    file: &'static str,
    parse: fn(&[u8]) -> Links,
}

const LISTS: [List; 2] = [
    List {
        url: MD5_URL_TSV,
        file: "links-md5.tsv",
        parse: |b| Links::from_tsv(&String::from_utf8_lossy(b)),
    },
    List {
        url: STELLA_UPLOADS,
        file: "links-stella.json",
        parse: Links::from_stella,
    },
];

impl LinkLists {
    /// Load the lists in the background (cache files in `cache`).
    pub fn start(&mut self, cache: &Path) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.rx = Some(rx);
        // each list's links, in LISTS order; sent merged once all are in
        let got: Arc<Mutex<Vec<Option<Links>>>> = Arc::new(Mutex::new(vec![None; LISTS.len()]));
        for (i, list) in LISTS.iter().enumerate() {
            let (got, tx) = (got.clone(), tx.clone());
            let deliver = move |links: Links| {
                let mut g = got.lock().unwrap_or_else(|e| e.into_inner());
                g[i] = Some(links);
                if g.iter().all(Option::is_some) {
                    let all = g
                        .iter_mut()
                        .filter_map(Option::take)
                        .reduce(Links::and)
                        .unwrap_or_default();
                    let _ = tx.send(all);
                }
            };
            load(list, cache.join(list.file), deliver);
        }
    }

    /// True when the lists have just loaded (into `links`).
    pub fn poll(&mut self) -> bool {
        let Some(links) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        log::info!("links for {} charts by MD5", links.len());
        self.links = links;
        self.rx = None;
        true
    }
}

/// One list: from its fresh cache, else fetched (and cached; the stale
/// cache when the fetch fails).
fn load(list: &'static List, file: PathBuf, deliver: impl FnOnce(Links) + Send + 'static) {
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
                return deliver((list.parse)(&b));
            }
        }
    }
    // web: raw.githubusercontent.com sends CORS headers, stellabms.xyz none
    #[cfg(target_arch = "wasm32")]
    let url = if list.url.starts_with("https://raw.githubusercontent.com/") {
        list.url.to_string()
    } else {
        crate::hosts::via_proxy(list.url)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let url = list.url.to_string();
    crate::platform::http_get(&url, move |r| match r {
        Ok(b) => {
            #[cfg(not(target_arch = "wasm32"))]
            let _ = vfs::write(&file, &b);
            deliver((list.parse)(&b))
        }
        Err(e) => {
            log::warn!("{}: {e}", list.url);
            deliver(
                vfs::read(&file)
                    .map(|b| (list.parse)(&b))
                    .unwrap_or_default(),
            )
        }
    });
}
