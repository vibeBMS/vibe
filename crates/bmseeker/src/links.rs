//! Links for a table's songs whose own are missing or dead, by the chart's
//! MD5, from lists people keep (as BeMusicSeeker's unofficial fork fills its
//! playlists' URL1 / URL2):
//!
//! - [`MD5_URL_TSV`]: `md5 <TAB> url_diff <TAB> url` rows (the fork's map,
//!   with mirrors of the dead 差分 uploaders);
//! - [`STELLA_UPLOADS`]: the charts sent to the Stella uploader, with their
//!   song's and chart's links.
//!
//! A link is filled only where the table has none, or one on a host known to
//! be gone ([`is_dead`]); the first list to name an MD5 wins.

use crate::table::TableEntry;
use std::collections::HashMap;

/// The fork's MD5 → links map.
pub const MD5_URL_TSV: &str =
    "https://raw.githubusercontent.com/Neeted/bemusicseeker-unofficial-fork/main/bms-md5-url-map.tsv";

/// The Stella uploader's charts, with their links.
pub const STELLA_UPLOADS: &str = "https://stellabms.xyz/score_upload_full.json";

/// Hosts that are gone (their links are filled as if empty).
const DEAD: &[&str] = &["gnqg.rosx.net", "absolute.pv.land.to"];

/// A link to a host known to be gone (also through web.archive.org).
pub fn is_dead(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    DEAD.iter().any(|d| u.contains(d))
}

fn usable(url: &str) -> bool {
    let u = url.trim();
    (u.starts_with("https://") || u.starts_with("http://")) && !is_dead(u)
}

/// A 32-digit hex MD5, in lower case.
fn md5_key(s: &str) -> Option<String> {
    let s = s.trim();
    (s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_ascii_lowercase())
}

/// MD5 → (the song's link, the chart's link).
#[derive(Clone, Debug, Default)]
pub struct Links {
    map: HashMap<String, (String, String)>,
}

impl Links {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn add(&mut self, md5: &str, url: &str, url_diff: &str) {
        let Some(k) = md5_key(md5) else { return };
        let keep = |u: &str| {
            if usable(u) {
                u.trim().to_string()
            } else {
                String::new()
            }
        };
        let (url, url_diff) = (keep(url), keep(url_diff));
        if url.is_empty() && url_diff.is_empty() {
            return;
        }
        self.map.entry(k).or_insert((url, url_diff));
    }

    /// [`MD5_URL_TSV`]'s format (a header row, then `md5`, `url_diff`, `url`).
    pub fn from_tsv(text: &str) -> Links {
        let mut out = Links::default();
        for line in text.lines().skip(1) {
            let mut col = line.split('\t');
            let md5 = col.next().unwrap_or("");
            let url_diff = col.next().unwrap_or("");
            let url = col.next().unwrap_or("");
            out.add(md5, url, url_diff);
        }
        out
    }

    /// [`STELLA_UPLOADS`]'s format (an array of `{md5, url, url_diff, ...}`).
    pub fn from_stella(json: &[u8]) -> Links {
        let mut out = Links::default();
        let rows: Vec<serde_json::Value> = serde_json::from_slice(json).unwrap_or_default();
        let s = |r: &serde_json::Value, k: &str| {
            r.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
        };
        for r in &rows {
            out.add(&s(r, "md5"), &s(r, "url"), &s(r, "url_diff"));
        }
        out
    }

    /// Both, `self` first where both name an MD5.
    pub fn and(mut self, other: Links) -> Links {
        for (k, v) in other.map {
            self.map.entry(k).or_insert(v);
        }
        self
    }

    /// `e`'s links, filled where missing or dead: (url, url_diff).
    pub fn of(&self, e: &TableEntry) -> (String, String) {
        let found = md5_key(&e.md5).and_then(|k| self.map.get(&k));
        let pick = |own: &str, other: Option<&String>| match other {
            Some(o) if !o.is_empty() && !usable(own) => o.clone(),
            _ => own.to_string(),
        };
        (
            pick(&e.url, found.map(|f| &f.0)),
            pick(&e.url_diff, found.map(|f| &f.1)),
        )
    }

    /// Fill `entries`' missing or dead links. Returns how many changed.
    pub fn fill(&self, entries: &mut [TableEntry]) -> usize {
        let mut n = 0;
        for e in entries {
            let (url, url_diff) = self.of(e);
            if url != e.url || url_diff != e.url_diff {
                e.url = url;
                e.url_diff = url_diff;
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_missing_and_dead_links() {
        let tsv = "md5\turl_diff\turl\n\
            75DC1FF763E736BA307FBC39EC9A386B\thttps://m.example/00001.zip\t\n\
            997c5d2e1756d22793b287fe72a9b184\thttps://m.example/00003.zip\thttp://manbow.nothing.sh/x\n\
            bad\thttps://x\thttps://y\n";
        let stella = br#"[{"md5":"997c5d2e1756d22793b287fe72a9b184","url":"https://other/","url_diff":""},
            {"md5":"0123456789abcdef0123456789abcdef","url":"https://s.example/song.zip","url_diff":"https://stellabms.xyz/upload/1"}]"#;
        let links = Links::from_tsv(tsv).and(Links::from_stella(stella));
        assert_eq!(links.len(), 3);
        let entry = |md5: &str, url: &str, url_diff: &str| TableEntry {
            md5: md5.into(),
            url: url.into(),
            url_diff: url_diff.into(),
            ..Default::default()
        };
        // a dead 差分 link, no song link
        let e = entry(
            "75dc1ff763e736ba307fbc39ec9a386b",
            "",
            "http://gnqg.rosx.net/upload/up/1.zip",
        );
        assert_eq!(
            links.of(&e),
            (String::new(), "https://m.example/00001.zip".into())
        );
        // the table's own links stay; the TSV wins over Stella
        let e = entry("997c5d2e1756d22793b287fe72a9b184", "https://own/", "");
        assert_eq!(
            links.of(&e),
            ("https://own/".into(), "https://m.example/00003.zip".into())
        );
        let mut es = vec![
            entry("0123456789ABCDEF0123456789ABCDEF", "", ""),
            entry("ffffffffffffffffffffffffffffffff", "", ""),
        ];
        assert_eq!(links.fill(&mut es), 1);
        assert_eq!(es[0].url, "https://s.example/song.zip");
        assert!(is_dead(
            "https://web.archive.org/web/2019/http://absolute.pv.land.to/x"
        ));
    }
}
