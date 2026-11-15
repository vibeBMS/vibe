//! Links for a chart from BMS SEARCH (bmssearch.net), its public API: the
//! chart (by MD5) → the BMS work it belongs to → the work's download links
//! as its author gave them (and its page on an event venue). Where the
//! chart is not tied to a work (most 差分 charts), the work is searched by
//! title and artist.
//!
//! [`Lookup`] is sans-IO like [`hosts::Resolver`](crate::hosts::Resolver):
//! it asks for GETs (`api.bmssearch.net` sends CORS headers, so a web page
//! makes them itself) and ends with the links; [`lookup`] makes them
//! natively.

use crate::hosts::Want;
use serde_json::Value;

pub const API: &str = "https://api.bmssearch.net/v1";

#[derive(Debug, PartialEq)]
pub enum Step {
    /// Read this and [`Lookup::feed`] the answer back.
    Get(String),
    /// The links found (maybe none).
    Done(Vec<String>),
}

pub struct Lookup {
    want: Want,
    wait: Wait,
}

enum Wait {
    Pattern,
    Bms,
    /// (title, artist) searched for
    Search(String, String),
}

/// Event venues whose work pages lead to the download (the resolver reads
/// them); other related links (a video, an author's profile) are left out.
const VENUES: &[&str] = &["manbow.nothing.sh", "venue.bmssearch.net"];

/// A work's links: its downloads, then its venue pages.
fn work_links(bms: &Value) -> Vec<String> {
    let urls = |k: &str| -> Vec<String> {
        bms.get(k)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|d| d.get("url").and_then(|u| u.as_str()))
            .map(|u| u.trim().to_string())
            .filter(|u| u.starts_with("http"))
            .collect()
    };
    let mut out = urls("downloads");
    for u in urls("relatedLinks") {
        let host = u.split('/').nth(2).unwrap_or("").to_ascii_lowercase();
        if VENUES.contains(&host.as_str()) && !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

/// The title to search for: the table's without its chart's name
/// (`Song [ANOTHER]`, `Song -Remix-`, `Song (7KEYS)`).
fn base_title(t: &str) -> String {
    let cut = t
        .char_indices()
        .find(|&(i, c)| {
            i > 0
                && (matches!(c, '[' | '(' | '（' | '【' | '<' | '～' | '〜')
                    || t[i..].starts_with(" -"))
        })
        .map_or(t.len(), |(i, _)| i);
    t[..cut].trim().to_string()
}

fn url_encode(s: &str) -> String {
    crate::hosts::url_encode(s)
}

impl Lookup {
    /// Start from the chart `md5` (of the song `want`).
    pub fn new(md5: &str, want: &Want) -> (Lookup, Step) {
        let md5 = md5.trim().to_ascii_lowercase();
        let mut l = Lookup {
            want: want.clone(),
            wait: Wait::Pattern,
        };
        let step = if md5.len() == 32 && md5.bytes().all(|b| b.is_ascii_hexdigit()) {
            Step::Get(format!("{API}/patterns/{md5}"))
        } else {
            let (t, a) = (want.title.clone(), want.artist.clone());
            l.search(&t, &a)
        };
        (l, step)
    }

    fn search(&mut self, title: &str, artist: &str) -> Step {
        let t = base_title(title);
        if t.chars().count() < 2 {
            return Step::Done(Vec::new());
        }
        let step = Step::Get(format!("{API}/bmses/search?title={}", url_encode(&t)));
        self.wait = Wait::Search(t, artist.to_string());
        step
    }

    /// The answer to the last [`Step::Get`].
    pub fn feed(&mut self, answer: Result<Vec<u8>, String>) -> Step {
        let v: Option<Value> = answer.ok().and_then(|b| serde_json::from_slice(&b).ok());
        match std::mem::replace(&mut self.wait, Wait::Bms) {
            Wait::Pattern => {
                let id = v
                    .as_ref()
                    .and_then(|p| p.get("bms"))
                    .and_then(|b| b.get("id"))
                    .and_then(|i| i.as_str());
                if let Some(id) = id {
                    self.wait = Wait::Bms;
                    return Step::Get(format!("{API}/bmses/{}", url_encode(id)));
                }
                // not tied to a work: search by its title (BMS SEARCH's, as
                // the chart has it, else the table's)
                let s = |k: &str| {
                    v.as_ref()
                        .and_then(|p| p.get(k))
                        .and_then(|x| x.as_str())
                        .filter(|x| !x.is_empty())
                        .map(str::to_string)
                };
                let title = s("title").unwrap_or_else(|| self.want.title.clone());
                let artist = s("artist").unwrap_or_else(|| self.want.artist.clone());
                self.search(&title, &artist)
            }
            Wait::Bms => Step::Done(v.as_ref().map(work_links).unwrap_or_default()),
            Wait::Search(title, artist) => {
                let works: Vec<Value> = match v {
                    Some(Value::Array(a)) => a,
                    _ => Vec::new(),
                };
                let s = |w: &Value, k: &str| {
                    w.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
                };
                let same: Vec<&Value> = works
                    .iter()
                    .filter(|w| crate::placement::same_title(&s(w, "title"), &title))
                    .collect();
                let by: Vec<&Value> = same
                    .iter()
                    .copied()
                    .filter(|w| crate::hosts::pack_artist(&s(w, "artist"), &artist))
                    .collect();
                // by its artist; else the one work of that title
                let pick: Vec<&Value> = if !by.is_empty() {
                    by
                } else if same.len() == 1 && title.chars().count() >= 6 {
                    same
                } else {
                    Vec::new()
                };
                let mut out: Vec<String> = Vec::new();
                for w in pick {
                    for u in work_links(w) {
                        if !out.contains(&u) {
                            out.push(u);
                        }
                    }
                }
                Step::Done(out)
            }
        }
    }
}

/// [`Lookup`] made natively (blocking).
#[cfg(not(target_arch = "wasm32"))]
pub fn lookup(md5: &str, want: &Want) -> Vec<String> {
    let (mut l, mut step) = Lookup::new(md5, want);
    loop {
        match step {
            Step::Get(url) => step = l.feed(crate::hosts::get(&url)),
            Step::Done(links) => return links,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn want(t: &str, a: &str) -> Want {
        Want {
            title: t.into(),
            artist: a.into(),
        }
    }

    #[test]
    fn titles() {
        assert_eq!(base_title("Beatrice [ANOTHER]"), "Beatrice");
        assert_eq!(base_title("AVALON -Genocide-"), "AVALON");
        assert_eq!(
            base_title("Tell The Truth (7KEYS ANOTHER)"),
            "Tell The Truth"
        );
        assert_eq!(base_title("[ASIA]"), "[ASIA]");
    }

    #[test]
    fn a_tied_chart() {
        let (mut l, s) = Lookup::new("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", &want("Song [X]", "a"));
        assert_eq!(
            s,
            Step::Get(format!("{API}/patterns/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))
        );
        let s = l.feed(Ok(
            br#"{"bms":{"id":"W1","title":"Song"},"title":"Song"}"#.to_vec()
        ));
        assert_eq!(s, Step::Get(format!("{API}/bmses/W1")));
        let bms = br#"{"downloads":[{"url":"https://dl.example/song.zip"}],
            "relatedLinks":[{"url":"https://twitter.com/x"},{"url":"http://manbow.nothing.sh/event/event.cgi?action=More_def&num=1&event=1"}]}"#;
        assert_eq!(
            l.feed(Ok(bms.to_vec())),
            Step::Done(vec![
                "https://dl.example/song.zip".into(),
                "http://manbow.nothing.sh/event/event.cgi?action=More_def&num=1&event=1".into()
            ])
        );
    }

    #[test]
    fn an_untied_chart_by_its_title() {
        let (mut l, _) = Lookup::new("0123456789abcdef0123456789abcdef", &want("x", "y"));
        let s = l.feed(Ok(br#"{"title":"$trange Attraktor","artist":"schwarzekugel / You might not need Scratch"}"#.to_vec()));
        assert_eq!(
            s,
            Step::Get(format!("{API}/bmses/search?title=%24trange%20Attraktor"))
        );
        let found = br#"[{"title":"$trange Attraktor","artist":"schwarzekugel","downloads":[{"url":"https://d/s.zip"}]},
            {"title":"$trange Attraktor II","artist":"other","downloads":[{"url":"https://d/2.zip"}]}]"#;
        assert_eq!(
            l.feed(Ok(found.to_vec())),
            Step::Done(vec!["https://d/s.zip".into()])
        );
        // unknown chart: the table's title
        let (mut l, _) = Lookup::new(
            "0123456789abcdef0123456789abcdef",
            &want("Beatrice [ANOTHER]", "xi"),
        );
        assert_eq!(
            l.feed(Err("HTTP 404".into())),
            Step::Get(format!("{API}/bmses/search?title=Beatrice"))
        );
        assert_eq!(l.feed(Ok(b"[]".to_vec())), Step::Done(Vec::new()));
    }

    /// `BMSEEKER_MD5=… BMSEEKER_TITLE=… cargo test -p bmseeker bmssearch_live -- --ignored`
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "network"]
    fn bmssearch_live() {
        let md5 = std::env::var("BMSEEKER_MD5").unwrap_or_default();
        let w = want(
            &std::env::var("BMSEEKER_TITLE").unwrap_or_default(),
            &std::env::var("BMSEEKER_ARTIST").unwrap_or_default(),
        );
        println!("{:?}", lookup(&md5, &w));
    }
}
