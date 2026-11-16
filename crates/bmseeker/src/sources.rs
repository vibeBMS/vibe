//! Where else a song's (or its separate chart's) links are looked for when
//! its own do not bring it: each place is a [`Source`] (link lists by MD5:
//! difficulty tables, the Stella uploader's list, TSV lists; BMS SEARCH by
//! MD5, else by title and artist), and [`Sources`] asks them in the order
//! [`Config::sources`] gives. A source answers with a [`Task`], so one that
//! reads the network runs anywhere a task does.
//!
//! [`Config::sources`]: crate::config::Config::sources

use crate::config::Config;
use crate::hosts::Want;
use crate::links::Links;
use crate::net::{Answer, BoxTask, Next, Ready, Task};

/// What is looked for: a chart (by its MD5, if known) of the song `want`;
/// `diff`: the chart's own link (a 差分 posted apart from its song), else
/// the song's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub md5: String,
    pub want: Want,
    pub diff: bool,
}

/// A place a song's links are looked for.
pub trait Source: Send + Sync {
    /// Its name in [`Config::sources`] ("table", "stella", "tsv",
    /// "bmssearch").
    fn name(&self) -> &'static str;
    /// The links it knows for `q`, in its order.
    fn find(&self, q: &Query) -> BoxTask<Vec<String>>;
}

/// A list of links by MD5 ([`Links`]): difficulty tables, the Stella
/// uploader's list, a TSV list.
pub struct List {
    name: &'static str,
    links: Links,
}

impl List {
    pub fn new(name: &'static str, links: Links) -> List {
        List { name, links }
    }
}

impl Source for List {
    fn name(&self) -> &'static str {
        self.name
    }

    fn find(&self, q: &Query) -> BoxTask<Vec<String>> {
        let e = crate::table::TableEntry {
            md5: q.md5.clone(),
            ..Default::default()
        };
        Ready::boxed(if q.diff {
            self.links.chart_links(&e)
        } else {
            self.links.song_links(&e)
        })
    }
}

/// BMS SEARCH: the work a chart belongs to and its download links (a song
/// only: BMS SEARCH lists works, not separate charts).
pub struct BmsSearch {
    api: String,
}

impl BmsSearch {
    pub fn new(config: &Config) -> BmsSearch {
        BmsSearch {
            api: config.bmssearch_api.clone(),
        }
    }
}

impl Source for BmsSearch {
    fn name(&self) -> &'static str {
        "bmssearch"
    }

    fn find(&self, q: &Query) -> BoxTask<Vec<String>> {
        if q.diff {
            return Ready::boxed(Vec::new());
        }
        Box::new(crate::bmssearch::Lookup::new(&q.md5, &q.want, &self.api))
    }
}

/// The sources, asked in order.
#[derive(Default)]
pub struct Sources(Vec<Box<dyn Source>>);

impl Sources {
    /// The sources [`Config::sources`] names, in its order: the link lists
    /// from `lists` (by name, those the caller has: "table", "stella",
    /// "tsv"), BMS SEARCH from the settings.
    pub fn from_config(config: &Config, mut lists: Vec<(&'static str, Links)>) -> Sources {
        let mut out = Sources::default();
        for name in &config.sources {
            if name == "bmssearch" {
                out.push(Box::new(BmsSearch::new(config)));
            } else if let Some(i) = lists.iter().position(|(n, _)| n == name) {
                let (n, links) = lists.remove(i);
                out.push(Box::new(List::new(n, links)));
            }
        }
        out
    }

    pub fn push(&mut self, source: Box<dyn Source>) {
        self.0.push(source);
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|s| s.name()).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Source> {
        self.0.iter().map(|s| s.as_ref())
    }

    /// Every source's links for `q`, in order, each once.
    pub fn find_all(&self, q: &Query) -> BoxTask<Vec<(&'static str, String)>> {
        Box::new(All {
            tasks: self.0.iter().map(|s| (s.name(), s.find(q))).collect(),
            at: 0,
            out: Vec::new(),
        })
    }
}

/// The sources' tasks one after another.
struct All {
    tasks: Vec<(&'static str, BoxTask<Vec<String>>)>,
    at: usize,
    out: Vec<(&'static str, String)>,
}

impl All {
    fn take(&mut self, name: &'static str, links: Vec<String>) {
        for l in links {
            if !self.out.iter().any(|(_, u)| *u == l) {
                self.out.push((name, l));
            }
        }
    }

    /// Start the sources from `at` until one asks.
    fn next(&mut self) -> Next<Vec<(&'static str, String)>> {
        while self.at < self.tasks.len() {
            let (name, t) = &mut self.tasks[self.at];
            let name = *name;
            match t.start() {
                Next::Ask(r) => return Next::Ask(r),
                Next::Done(links) => {
                    self.take(name, links);
                    self.at += 1;
                }
            }
        }
        Next::Done(std::mem::take(&mut self.out))
    }
}

impl Task for All {
    type Output = Vec<(&'static str, String)>;

    fn start(&mut self) -> Next<Self::Output> {
        self.at = 0;
        self.out.clear();
        self.next()
    }

    fn feed(&mut self, answer: Answer) -> Next<Self::Output> {
        let Some((name, t)) = self.tasks.get_mut(self.at) else {
            return Next::Done(std::mem::take(&mut self.out));
        };
        let name = *name;
        match t.feed(answer) {
            Next::Ask(r) => Next::Ask(r),
            Next::Done(links) => {
                self.take(name, links);
                self.at += 1;
                self.next()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{run, Canned};

    #[test]
    fn in_the_order_set() {
        let md5 = "0123456789abcdef0123456789abcdef";
        let stella = format!(
            r#"[{{"md5":"{md5}","url":"https://s.example/song.zip","url_diff":"https://s.example/diff.zip"}}]"#
        );
        let tsv = format!("md5\turl_diff\turl\n{md5}\t\thttps://t.example/song.zip\n");
        let config = Config {
            sources: ["tsv", "bmssearch", "stella"].map(String::from).to_vec(),
            ..Config::default()
        };
        let sources = Sources::from_config(
            &config,
            vec![
                ("stella", Links::from_stella(stella.as_bytes())),
                ("tsv", Links::from_tsv(&tsv)),
                ("table", Links::default()),
            ],
        );
        assert_eq!(sources.names(), ["tsv", "bmssearch", "stella"]);
        let q = Query {
            md5: md5.into(),
            want: Want::default(),
            diff: false,
        };
        let api = &config.bmssearch_api;
        let http = Canned(vec![
            (format!("{api}/patterns/{md5}"), Ok(br#"{"bms":{"id":"W"}}"#.to_vec())),
            (
                format!("{api}/bmses/W"),
                Ok(br#"{"downloads":[{"url":"https://b.example/song.zip"},{"url":"https://t.example/song.zip"}]}"#.to_vec()),
            ),
        ]);
        let found = run(&mut sources.find_all(&q), &http);
        assert_eq!(
            found,
            [
                ("tsv", "https://t.example/song.zip".to_string()),
                ("bmssearch", "https://b.example/song.zip".to_string()),
                ("stella", "https://s.example/song.zip".to_string()),
            ]
        );
        // a separate chart: the lists' chart links, none from BMS SEARCH
        let q = Query { diff: true, ..q };
        assert_eq!(
            run(&mut sources.find_all(&q), &http),
            [("stella", "https://s.example/diff.zip".to_string())]
        );
    }
}
