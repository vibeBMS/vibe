//! The difficulty tables: read from their caches, fetched in the background,
//! and their songs' links filled from the other links known.

use super::AppEvent;
use crate::config::TableConfig;
use crate::paths::Paths;
use crate::platform::Job;
use bmseeker::links::Links;
use bmseeker::table::DifficultyTable;
use std::path::PathBuf;

enum TableMsg {
    Loaded(Box<DifficultyTable>),
    Failed(String, String),
}

/// The difficulty tables read, and the links that fill theirs.
#[derive(Default)]
pub struct Tables {
    /// In the configuration's order.
    pub list: Vec<DifficultyTable>,
    /// Tables being fetched.
    job: Option<Job<TableMsg>>,
    /// Links for table songs whose own are missing or dead.
    pub links: crate::links::LinkLists,
}

fn cache_file(paths: &Paths, url: &str) -> PathBuf {
    let h = vibebms_core::bms::md5_hex(url.as_bytes());
    paths.tables.join(format!("{h}.json"))
}

/// The enabled tables of `config` from their caches, in its order, their
/// missing or dead links filled from each other's; and their links.
pub fn read_cached(paths: &Paths, config: &[TableConfig]) -> (Vec<DifficultyTable>, Links) {
    let mut list: Vec<DifficultyTable> = (config.iter().filter(|t| t.enabled))
        .filter_map(|t| {
            let b = crate::platform::vfs::read(&cache_file(paths, &t.url)).ok()?;
            serde_json::from_slice(&b).ok()
        })
        .collect();
    let links = own_links(&mut list);
    (list, links)
}

/// The links `list` gives, its missing or dead ones filled from them.
fn own_links(list: &mut [DifficultyTable]) -> Links {
    let links = Links::from_tables(&*list);
    for t in list {
        links.fill(&mut t.entries);
    }
    links
}

impl Tables {
    /// The enabled tables of `config` from their caches.
    pub fn load_cached(&mut self, paths: &Paths, config: &[TableConfig]) {
        let (list, links) = read_cached(paths, config);
        self.set(list, links);
    }

    /// `list` as the tables, `links` theirs ([`read_cached`]).
    pub fn set(&mut self, list: Vec<DifficultyTable>, links: Links) {
        self.list = list;
        self.links.set_tables(links, &mut self.list);
    }

    /// Fetch the enabled tables of `config` in the background. `force`
    /// refetches even fresh caches.
    pub fn refresh(&mut self, config: &[TableConfig], force: bool) {
        if self.job.is_some() {
            return;
        }
        let now = crate::platform::unix_now();
        let todo: Vec<(String, String)> = config
            .iter()
            .filter(|t| t.enabled)
            .filter(|t| {
                force
                    || !self.list.iter().any(|dt| {
                        dt.source_url == t.url && now.saturating_sub(dt.fetched_at) < 86_400
                    })
            })
            .map(|t| (t.name.clone(), t.url.clone()))
            .collect();
        if todo.is_empty() {
            return;
        }
        self.job = Some(Job::start(|tx, _| {
            for (name, url) in todo {
                let tx = tx.clone();
                crate::platform::load_table(&url, move |r| {
                    let _ = tx.send(match r {
                        Ok(t) => TableMsg::Loaded(Box::new(t)),
                        Err(e) => TableMsg::Failed(name, e),
                    });
                });
            }
        }));
    }

    /// The tables' missing or dead links filled from the other links known
    /// for their charts (the other tables', the Stella uploader's).
    fn fill_links(&mut self) {
        let links = own_links(&mut self.list);
        self.links.set_tables(links, &mut self.list);
    }

    /// The links list and the tables that came since the last frame (cached,
    /// in `config`'s order).
    pub fn poll(&mut self, paths: &Paths, config: &[TableConfig]) -> Vec<AppEvent> {
        let mut out = Vec::new();
        if self.links.poll(&mut self.list) {
            out.push(AppEvent::LibraryChanged);
        }
        let mut loaded = false;
        for m in Job::drain_slot(&mut self.job) {
            let t = match m {
                TableMsg::Loaded(t) => *t,
                TableMsg::Failed(n, e) => {
                    out.push(AppEvent::Toast(tr!("Table {name}: {e}", name = n, e = e)));
                    continue;
                }
            };
            loaded = true;
            // (cached as the table has them; filled below)
            if let Ok(j) = serde_json::to_vec(&t) {
                let _ = crate::platform::vfs::write(&cache_file(paths, &t.source_url), &j);
            }
            out.push(AppEvent::Toast(tr!(
                "Table updated: {name} ({n} charts)",
                name = t.header.name,
                n = t.entries.len()
            )));
            match self.list.iter_mut().find(|o| o.source_url == t.source_url) {
                Some(old) => *old = t,
                None => self.list.push(t),
            }
            self.list.sort_by_key(|t| {
                config
                    .iter()
                    .position(|c| c.url == t.source_url)
                    .unwrap_or(usize::MAX)
            });
        }
        if loaded {
            self.fill_links();
            out.push(AppEvent::LibraryChanged);
        }
        out
    }
}
