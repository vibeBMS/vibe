//! Other links for difficulty-table songs, by the chart's MD5
//! (`bmseeker::links`): the tables the player reads, and the Stella
//! uploader's list (loaded in the background once per start; natively from a
//! cache a day old at most). They fill missing or dead links, and are tried
//! in turn when a song's own link does not download.

use crate::platform::Job;
use bmseeker::links::Links;
use bmseeker::table::{DifficultyTable, TableEntry};
use std::path::{Path, PathBuf};

/// How long the cached list is used before it is fetched again.
#[cfg(not(target_arch = "wasm32"))]
const FRESH_S: i64 = 86_400;

/// Its cache file.
const STELLA_FILE: &str = "links-stella.json";

/// The tables' links and the list's, each held once: a lookup asks the
/// tables first.
#[derive(Default)]
pub struct LinkLists {
    tables: Links,
    stella: Links,
    job: Option<Job<Links>>,
}

impl LinkLists {
    /// Load the Stella uploader's list in the background (its cache in
    /// `cache`).
    pub fn start(&mut self, cache: &Path) {
        if self.job.is_some() {
            return;
        }
        let file = cache.join(STELLA_FILE);
        self.job = Some(Job::start(|tx, _| {
            crate::platform::spawn("links", move || {
                load(file, move |l| {
                    let _ = tx.send(l);
                })
            })
        }));
    }

    /// True when the list has just loaded: `tables` are filled from it.
    pub fn poll(&mut self, tables: &mut [DifficultyTable]) -> bool {
        let Some(links) = self.job.as_mut().and_then(Job::poll) else {
            return false;
        };
        log::info!("Stella uploader: links for {} charts", links.len());
        self.stella = links;
        self.job = None;
        self.fill_from_list(tables);
        true
    }

    /// `links` as the tables' own; `tables` (filled from them) filled from
    /// the list too.
    pub fn set_tables(&mut self, links: Links, tables: &mut [DifficultyTable]) {
        self.tables = links;
        self.fill_from_list(tables);
    }

    fn fill_from_list(&self, tables: &mut [DifficultyTable]) {
        if !self.stella.is_empty() {
            for t in tables {
                self.stella.fill(&mut t.entries);
            }
        }
    }

    /// Every link known for `e`'s song, its own first.
    pub fn song_links(&self, e: &TableEntry) -> Vec<String> {
        self.of_chart(&e.md5).song_links(e)
    }

    /// Every link known for `e`'s separate chart, its own first.
    pub fn chart_links(&self, e: &TableEntry) -> Vec<String> {
        self.of_chart(&e.md5).chart_links(e)
    }

    /// The links both know for the chart `md5`, as one [`Links`] (the
    /// tables' first, as [`Links::and`] keeps them).
    fn of_chart(&self, md5: &str) -> Links {
        let bare = TableEntry {
            md5: md5.to_string(),
            ..Default::default()
        };
        let rows = |l: &Links| {
            let row = |url, url_diff| TableEntry {
                url,
                url_diff,
                ..bare.clone()
            };
            let songs = l
                .song_links(&bare)
                .into_iter()
                .map(|u| row(u, String::new()));
            let charts = l
                .chart_links(&bare)
                .into_iter()
                .map(|u| row(String::new(), u));
            DifficultyTable {
                entries: songs.chain(charts).collect(),
                ..Default::default()
            }
        };
        Links::from_tables([&rows(&self.tables), &rows(&self.stella)])
    }
}

/// The list: from its fresh cache, else fetched (and cached; the stale cache
/// when the fetch fails).
fn load(file: PathBuf, deliver: impl FnOnce(Links) + Send + 'static) {
    use crate::platform::vfs;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let now = crate::platform::unix_now() as i64;
        let (mtime, size) = vfs::stamp(&file);
        if size > 0 && now - mtime < FRESH_S {
            if let Ok(b) = vfs::read(&file) {
                return deliver(Links::from_stella(&b));
            }
        }
    }
    let list = bmseeker::config::Config::default().stella_list;
    // web: stellabms.xyz sends no CORS headers
    #[cfg(target_arch = "wasm32")]
    let url = bmseeker::web::proxied(bmseeker::web::PROXY, &list);
    #[cfg(not(target_arch = "wasm32"))]
    let url = list.clone();
    crate::platform::http_get(&url, move |r| match r {
        Ok(b) => {
            #[cfg(not(target_arch = "wasm32"))]
            let _ = vfs::write(&file, &b);
            deliver(Links::from_stella(&b))
        }
        Err(e) => {
            log::warn!("{list}: {e}");
            deliver(
                vfs::read(&file)
                    .map(|b| Links::from_stella(&b))
                    .unwrap_or_default(),
            )
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Looked up in the two lists, the links are those of one list made
    /// of both ([`Links::and`]), the tables' first.
    #[test]
    fn both_lists_as_one() {
        let (a, b, c) = ("a".repeat(32), "b".repeat(32), "c".repeat(32));
        let e = |md5: &str, url: &str, url_diff: &str| TableEntry {
            md5: md5.into(),
            url: url.into(),
            url_diff: url_diff.into(),
            ..Default::default()
        };
        let table = DifficultyTable {
            entries: vec![
                e(&a, "https://t/a1.zip", ""),
                e(&a, "https://t/a2.zip", "https://t/a2.bms"),
                e(&b, "", "https://t/b.bms"),
            ],
            ..Default::default()
        };
        let stella = Links::from_stella(
            format!(
                r#"[{{"md5":"{a}","url":"https://s/a.zip","url_diff":""}},
                    {{"md5":"{a}","url":"https://t/a1.zip","url_diff":"https://s/a.bms"}},
                    {{"md5":"{c}","url":"https://s/c.zip","url_diff":"https://s/c.bms"}}]"#
            )
            .as_bytes(),
        );
        let one = Links::from_tables([&table]).and(stella.clone());
        let mut two = LinkLists {
            stella,
            ..Default::default()
        };
        two.set_tables(Links::from_tables([&table]), &mut []);
        let dead = "http://gnqg.rosx.net/upload/upload.cgi?get=1";
        let mut entries = Vec::new();
        for md5 in [&a, &b, &c, &"d".repeat(32)] {
            for own in ["", "https://own/x.zip", dead, "not a link"] {
                entries.push(e(md5, own, ""));
                entries.push(e(md5, "", own));
            }
        }
        for x in &entries {
            assert_eq!(two.song_links(x), one.song_links(x), "{x:?}");
            assert_eq!(two.chart_links(x), one.chart_links(x), "{x:?}");
        }
        // filled from the tables' links, then the list's
        let mut filled = entries.clone();
        one.fill(&mut filled);
        two.tables.fill(&mut entries);
        let mut tables = [DifficultyTable {
            entries,
            ..Default::default()
        }];
        two.set_tables(two.tables.clone(), &mut tables);
        assert_eq!(tables[0].entries, filled);
    }
}
