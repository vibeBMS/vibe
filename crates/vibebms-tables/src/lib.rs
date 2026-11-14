//! Difficulty tables in the common "bmstable" format.
//!
//! This is the format used by the LR2-era tables (通常/発狂BMS難易度表,
//! overjoy, LN tables, ...) and by beatoraja-era tables such as Satellite
//! and Stella:
//!
//! 1. an HTML page containing `<meta name="bmstable" content="header.json">`
//!    (or the header JSON URL directly),
//! 2. a header JSON with `name`, `symbol`, `data_url`, optional
//!    `level_order` and `course`,
//! 3. a data JSON: an array of entries with `md5` / `sha256` and `level`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CourseTrophy {
    pub name: String,
    pub missrate: f64,
    pub scorerate: f64,
}

/// A course (段位 / dan) defined by a table.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Course {
    pub name: String,
    pub md5: Vec<String>,
    pub sha256: Vec<String>,
    /// e.g. "grade_mirror", "gauge_lr2", "ln", "cn", "hcn", "no_speed"...
    pub constraint: Vec<String>,
    pub trophy: Vec<CourseTrophy>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TableHeader {
    pub name: String,
    pub symbol: String,
    pub tag: String,
    pub data_url: String,
    pub level_order: Vec<String>,
    pub courses: Vec<Course>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TableEntry {
    pub md5: String,
    pub sha256: String,
    pub level: String,
    pub title: String,
    pub artist: String,
    pub url: String,
    pub url_diff: String,
    pub comment: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct DifficultyTable {
    /// URL the user registered.
    pub source_url: String,
    pub header: TableHeader,
    pub entries: Vec<TableEntry>,
    /// Unix seconds of the last successful fetch.
    pub fetched_at: u64,
}

#[derive(Debug)]
pub enum TableError {
    Http(String),
    Format(String),
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TableError::Http(e) => write!(f, "network error: {e}"),
            TableError::Format(e) => write!(f, "invalid table: {e}"),
        }
    }
}
impl std::error::Error for TableError {}

/// Well-known tables offered by default.
pub const DEFAULT_TABLES: &[(&str, &str)] = &[
    (
        "発狂BMS難易度表 (★)",
        "http://www.ribbit.xyz/bms/tables/insane.html",
    ),
    (
        "通常難易度表 (☆)",
        "http://www.ribbit.xyz/bms/tables/normal.html",
    ),
    ("Satellite (sl)", "https://stellabms.xyz/sl/table.html"),
    ("Stella (st)", "https://stellabms.xyz/st/table.html"),
];

fn s(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn str_list(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().map(s).filter(|x| !x.is_empty()).collect(),
        Value::Null => Vec::new(),
        other => vec![s(other)],
    }
}

/// Find `<meta name="bmstable" content="...">` in an HTML page.
pub fn find_bmstable_meta(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<meta") {
        let start = from + i;
        let end = lower[start..]
            .find('>')
            .map(|e| start + e)
            .unwrap_or(lower.len());
        let tag = &html[start..end];
        let ltag = &lower[start..end];
        if ltag.contains("bmstable") {
            if let Some(c) = attr(tag, ltag, "content") {
                return Some(c);
            }
        }
        from = end;
    }
    None
}

fn attr(tag: &str, ltag: &str, name: &str) -> Option<String> {
    // `ltag` is the ASCII-lowercased `tag`, so byte offsets are shared.
    let b = ltag.as_bytes();
    let mut idx = 0;
    while let Some(i) = ltag[idx..].find(name) {
        let mut p = idx + i + name.len();
        while p < b.len() && b[p].is_ascii_whitespace() {
            p += 1;
        }
        if p < b.len() && b[p] == b'=' {
            p += 1;
            while p < b.len() && b[p].is_ascii_whitespace() {
                p += 1;
            }
            let q = *b.get(p)?;
            if q == b'"' || q == b'\'' {
                let rest = &tag[p + 1..];
                let e = rest.find(q as char)?;
                return Some(rest[..e].trim().to_string());
            }
            let rest = &tag[p..];
            let e = rest.find(char::is_whitespace).unwrap_or(rest.len());
            return Some(rest[..e].trim_end_matches('/').to_string());
        }
        idx = p;
    }
    None
}

pub fn parse_header(v: &Value) -> Result<TableHeader, TableError> {
    let o = v
        .as_object()
        .ok_or_else(|| TableError::Format("header is not an object".into()))?;
    let get = |k: &str| o.get(k).map(s).unwrap_or_default();
    let mut courses = Vec::new();
    if let Some(c) = o.get("course") {
        let mut flat = Vec::new();
        match c {
            Value::Array(a) => {
                for x in a {
                    match x {
                        Value::Array(inner) => flat.extend(inner.iter().cloned()),
                        other => flat.push(other.clone()),
                    }
                }
            }
            Value::Object(_) => flat.push(c.clone()),
            _ => {}
        }
        for c in flat {
            let Some(co) = c.as_object() else { continue };
            courses.push(Course {
                name: co.get("name").map(s).unwrap_or_default(),
                md5: co.get("md5").map(str_list).unwrap_or_default(),
                sha256: co.get("sha256").map(str_list).unwrap_or_default(),
                constraint: co.get("constraint").map(str_list).unwrap_or_default(),
                trophy: co
                    .get("trophy")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_object)
                            .map(|t| CourseTrophy {
                                name: t.get("name").map(s).unwrap_or_default(),
                                missrate: t.get("missrate").and_then(Value::as_f64).unwrap_or(0.0),
                                scorerate: t
                                    .get("scorerate")
                                    .and_then(Value::as_f64)
                                    .unwrap_or(0.0),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }
    let header = TableHeader {
        name: get("name"),
        symbol: get("symbol"),
        tag: get("tag"),
        data_url: get("data_url"),
        level_order: o.get("level_order").map(str_list).unwrap_or_default(),
        courses,
    };
    if header.data_url.is_empty() {
        return Err(TableError::Format("header has no data_url".into()));
    }
    Ok(header)
}

pub fn parse_data(v: &Value) -> Result<Vec<TableEntry>, TableError> {
    let a = v
        .as_array()
        .ok_or_else(|| TableError::Format("data is not an array".into()))?;
    Ok(a.iter()
        .filter_map(Value::as_object)
        .map(|o| {
            let get = |k: &str| o.get(k).map(s).unwrap_or_default();
            TableEntry {
                md5: get("md5").to_ascii_lowercase(),
                sha256: get("sha256").to_ascii_lowercase(),
                level: get("level"),
                title: get("title"),
                artist: get("artist"),
                url: get("url"),
                url_diff: get("url_diff"),
                comment: get("comment"),
            }
        })
        .filter(|e| !e.md5.is_empty() || !e.sha256.is_empty())
        .collect())
}

fn join(base: &str, rel: &str) -> Result<String, TableError> {
    let rel = rel.trim();
    if let Ok(u) = Url::parse(rel) {
        return Ok(u.to_string());
    }
    let b = Url::parse(base).map_err(|e| TableError::Format(format!("bad url {base}: {e}")))?;
    b.join(rel)
        .map(|u| u.to_string())
        .map_err(|e| TableError::Format(e.to_string()))
}

/// Decode a response body (UTF-8 with BOM, or Shift_JIS for old tables).
pub fn decode_body(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned(),
    }
}

/// Known mirrors used when the primary host is unreachable
/// (ribbit.xyz is frequently down).
pub const MIRRORS: &[(&str, &str)] = &[
    (
        "http://www.ribbit.xyz/bms/tables/insane.html",
        "https://darksabun.club/table/archive/insane1/",
    ),
    (
        "http://www.ribbit.xyz/bms/tables/normal.html",
        "https://darksabun.club/table/archive/normal1/",
    ),
];

/// Next action of a [`TableLoader`].
#[derive(Debug)]
pub enum Step {
    /// Fetch this URL and pass the body to [`TableLoader::feed`].
    Get(String),
    Done(Box<DifficultyTable>),
}

#[derive(Debug)]
enum Stage {
    Page(String),
    Header(String),
    Data(TableHeader),
}

/// HTTP-free table loading state machine (page → header.json → data.json),
/// so any HTTP client (native or browser) can drive it.
#[derive(Debug)]
pub struct TableLoader {
    source: String,
    stage: Stage,
    mirror_used: bool,
}

impl TableLoader {
    /// Returns the loader and the first URL to fetch.
    pub fn new(url: &str) -> (TableLoader, String) {
        (
            TableLoader {
                source: url.to_string(),
                stage: Stage::Page(url.to_string()),
                mirror_used: false,
            },
            url.to_string(),
        )
    }

    /// A loader for the table at `source` that reads a copy of it at
    /// `first` (its page or header; relative links resolve against it).
    pub fn starting_at(source: &str, first: &str) -> TableLoader {
        TableLoader {
            source: source.to_string(),
            stage: Stage::Page(first.to_string()),
            // the copy is the fallback already
            mirror_used: true,
        }
    }

    pub fn feed(&mut self, body: Result<Vec<u8>, String>) -> Result<Step, TableError> {
        match self.advance(body) {
            Ok(s) => Ok(s),
            Err(e) => match MIRRORS
                .iter()
                .find(|(u, _)| *u == self.source)
                .filter(|_| !self.mirror_used)
            {
                Some((_, mirror)) => {
                    log::info!("{} failed ({e}); trying mirror {mirror}", self.source);
                    self.mirror_used = true;
                    self.stage = Stage::Page(mirror.to_string());
                    Ok(Step::Get(mirror.to_string()))
                }
                None => Err(e),
            },
        }
    }

    fn advance(&mut self, body: Result<Vec<u8>, String>) -> Result<Step, TableError> {
        let text = decode_body(&body.map_err(TableError::Http)?);
        let json = |t: &str| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}'));
        match std::mem::replace(&mut self.stage, Stage::Page(String::new())) {
            Stage::Page(url) => {
                if text.trim_start().starts_with('{') {
                    self.stage = Stage::Header(url);
                    return self.advance(Ok(text.into_bytes()));
                }
                let meta = find_bmstable_meta(&text).ok_or_else(|| {
                    TableError::Format("no <meta name=\"bmstable\"> found".into())
                })?;
                let hu = join(&url, &meta)?;
                self.stage = Stage::Header(hu.clone());
                Ok(Step::Get(hu))
            }
            Stage::Header(url) => {
                let header = parse_header(
                    &json(&text).map_err(|e| TableError::Format(format!("header: {e}")))?,
                )?;
                let du = join(&url, &header.data_url)?;
                self.stage = Stage::Data(header);
                Ok(Step::Get(du))
            }
            Stage::Data(header) => {
                let entries = parse_data(
                    &json(&text).map_err(|e| TableError::Format(format!("data: {e}")))?,
                )?;
                let fetched_at = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                Ok(Step::Done(Box::new(DifficultyTable {
                    source_url: self.source.clone(),
                    header,
                    entries,
                    fetched_at,
                })))
            }
        }
    }
}

/// Blocking HTTP GET, for [`fetch_table`].
pub trait Fetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// Load a table synchronously with a blocking fetcher (tests, tools).
pub fn fetch_table(fetcher: &dyn Fetcher, url: &str) -> Result<DifficultyTable, TableError> {
    let (mut loader, mut next) = TableLoader::new(url);
    loop {
        match loader.feed(fetcher.get(&next))? {
            Step::Get(u) => next = u,
            Step::Done(t) => return Ok(*t),
        }
    }
}

/// Compare levels naturally ("2" < "10", "10" < "10+", numbers before text).
fn level_key(l: &str) -> (u8, f64, String) {
    let num: String = l
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    match num.parse::<f64>() {
        Ok(n) => (0, n, l[num.len()..].to_string()),
        Err(_) => (1, 0.0, l.to_string()),
    }
}

impl DifficultyTable {
    /// Levels in display order (`level_order` if given, else natural order).
    pub fn levels(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for l in &self.header.level_order {
            if !seen.contains(l) {
                seen.push(l.clone());
            }
        }
        let mut rest: Vec<String> = self
            .entries
            .iter()
            .map(|e| e.level.clone())
            .filter(|l| !seen.contains(l))
            .collect();
        rest.sort_by(|a, b| {
            level_key(a)
                .partial_cmp(&level_key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        rest.dedup();
        seen.extend(rest);
        seen
    }

    /// Display label e.g. "★12".
    pub fn label(&self, level: &str) -> String {
        format!("{}{}", self.header.symbol, level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Mock(HashMap<String, String>);
    impl Fetcher for Mock {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            self.0
                .get(url)
                .map(|s| s.clone().into_bytes())
                .ok_or_else(|| format!("404 {url}"))
        }
    }

    #[test]
    fn meta_parsing() {
        assert_eq!(
            find_bmstable_meta(
                r#"<html><head><META NAME="bmstable" CONTENT="header.json"></head>"#
            )
            .as_deref(),
            Some("header.json")
        );
        assert_eq!(
            find_bmstable_meta("<meta name='bmstable' content='./h.json' />").as_deref(),
            Some("./h.json")
        );
        assert_eq!(find_bmstable_meta("<meta charset=utf-8>"), None);
    }

    #[test]
    fn full_fetch_with_relative_urls() {
        let mut m = HashMap::new();
        m.insert(
            "https://ex.com/t/table.html".into(),
            r#"<meta name="bmstable" content="header.json">"#.into(),
        );
        m.insert(
            "https://ex.com/t/header.json".into(),
            r#"{"name":"Test","symbol":"sl","data_url":"data/score.json","level_order":["0","1",2],
                "course":[[{"name":"sl1","md5":["a","b"],"constraint":["grade_mirror","gauge_lr2"]}]]}"#
                .into(),
        );
        m.insert(
            "https://ex.com/t/data/score.json".into(),
            r#"[{"md5":"ABC","level":1,"title":"x"},{"sha256":"def","level":"10"},{"md5":"q","level":"2"},{"title":"no hash"}]"#.into(),
        );
        let t = fetch_table(&Mock(m), "https://ex.com/t/table.html").unwrap();
        assert_eq!(t.header.symbol, "sl");
        assert_eq!(t.entries.len(), 3);
        assert_eq!(t.entries[0].md5, "abc");
        assert_eq!(t.levels(), vec!["0", "1", "2", "10"]);
        assert_eq!(t.header.courses.len(), 1);
        assert_eq!(t.header.courses[0].md5.len(), 2);
        assert_eq!(t.label("12"), "sl12");
    }
}
