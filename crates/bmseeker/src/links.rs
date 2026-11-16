//! Other links for a table's songs, by the chart's MD5: where a table's
//! own link is missing, dead, or does not download (a page, a host that
//! needs a browser), the same chart may have a link elsewhere: in another
//! table the player reads (発狂BMS and Satellite list some of the same
//! charts), or in the Stella uploader's list ([`Config::stella_list`]).
//!
//! [`Config::stella_list`]: crate::config::Config::stella_list
//!
//! [`Links::fill`] fills a link only where the table has none, or one on a
//! host known to be gone ([`is_dead`]); [`Links::song_links`] /
//! [`Links::chart_links`] give every link known for a chart, the table's own
//! first, to try one after another.

use crate::table::{DifficultyTable, TableEntry};
use std::collections::HashMap;

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

/// MD5 → the (song's link, chart's link) pairs known for it, in the order
/// the sources were given.
#[derive(Clone, Debug, Default)]
pub struct Links {
    map: HashMap<String, Vec<(String, String)>>,
}

impl Links {
    /// How many charts have links.
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
        let pair = (keep(url), keep(url_diff));
        if pair.0.is_empty() && pair.1.is_empty() {
            return;
        }
        let known = self.map.entry(k).or_default();
        if !known.contains(&pair) {
            known.push(pair);
        }
    }

    /// The links the tables give their charts.
    pub fn from_tables<'a>(tables: impl IntoIterator<Item = &'a DifficultyTable>) -> Links {
        let mut out = Links::default();
        for t in tables {
            for e in &t.entries {
                out.add(&e.md5, &e.url, &e.url_diff);
            }
        }
        out
    }

    /// A list of `md5 <TAB> url_diff <TAB> url` rows after a header row
    /// (one a player keeps, or trusts).
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

    /// The Stella uploader's list's format (an array of `{md5, url, url_diff, ...}`).
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

    /// Both, `self`'s links first.
    pub fn and(mut self, other: Links) -> Links {
        for (k, v) in other.map {
            let known = self.map.entry(k).or_default();
            for pair in v {
                if !known.contains(&pair) {
                    known.push(pair);
                }
            }
        }
        self
    }

    fn known(&self, md5: &str) -> &[(String, String)] {
        md5_key(md5)
            .and_then(|k| self.map.get(&k))
            .map_or(&[], |v| v.as_slice())
    }

    /// `own` (if usable), then the other links in `pick` of the chart's pairs.
    fn all(&self, own: &str, md5: &str, pick: fn(&(String, String)) -> &String) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let own = own.trim();
        if usable(own) {
            out.push(own.to_string());
        }
        for p in self.known(md5) {
            let u = pick(p);
            if !u.is_empty() && !out.contains(u) {
                out.push(u.clone());
            }
        }
        out
    }

    /// Every link known for `e`'s song, its own first.
    pub fn song_links(&self, e: &TableEntry) -> Vec<String> {
        self.all(&e.url, &e.md5, |p| &p.0)
    }

    /// Every link known for `e`'s separate chart, its own first.
    pub fn chart_links(&self, e: &TableEntry) -> Vec<String> {
        self.all(&e.url_diff, &e.md5, |p| &p.1)
    }

    /// `e`'s links, filled where missing or dead: (url, url_diff).
    pub fn of(&self, e: &TableEntry) -> (String, String) {
        let pick = |own: &str, other: Vec<String>| {
            if usable(own) {
                own.to_string()
            } else {
                other.into_iter().next().unwrap_or_else(|| own.to_string())
            }
        };
        (
            pick(&e.url, self.song_links(e)),
            pick(&e.url_diff, self.chart_links(e)),
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
        assert_eq!(links.known("997C5D2E1756D22793B287FE72A9B184").len(), 2);
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
