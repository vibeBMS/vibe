//! The MD5 and SHA-256 of every chart in the library, read once per file
//! (by its size and modification time, kept in `scan.json`).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
struct Known {
    mtime: u64,
    size: u64,
    md5: String,
    sha256: String,
}

fn cache_file() -> PathBuf {
    crate::config::dir().join("scan.json")
}

/// The hashes of the charts below `library` (not those waiting in its
/// `.bmseeker`), and how many charts.
pub fn scan(library: &Path) -> (HashSet<String>, usize) {
    use md5::Digest;
    let old: HashMap<PathBuf, Known> = std::fs::read(cache_file())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut new: HashMap<PathBuf, Known> = HashMap::new();
    let mut todo = vec![library.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if !p.ends_with(crate::work::DOWNLOADS) {
                    todo.push(p);
                }
                continue;
            }
            if !bmseeker::placement::is_chart(&p) {
                continue;
            }
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let size = m.len();
            let known = match old.get(&p) {
                Some(k) if k.mtime == mtime && k.size == size => k.clone(),
                _ => {
                    let Ok(b) = std::fs::read(&p) else { continue };
                    let hex = |h: &[u8]| h.iter().map(|x| format!("{x:02x}")).collect::<String>();
                    Known {
                        mtime,
                        size,
                        md5: hex(&md5::Md5::digest(&b)),
                        sha256: hex(&sha2::Sha256::digest(&b)),
                    }
                }
            };
            new.insert(p, known);
        }
    }
    if let Ok(j) = serde_json::to_vec(&new) {
        let _ = std::fs::create_dir_all(crate::config::dir());
        let _ = std::fs::write(cache_file(), j);
    }
    let n = new.len();
    let owned = new.into_values().flat_map(|k| [k.md5, k.sha256]).collect();
    (owned, n)
}
