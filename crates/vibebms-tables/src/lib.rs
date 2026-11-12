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
use std::collections::BTreeMap;
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

/// Fetch a table, falling back to a known mirror on failure.
pub fn fetch_table_with_fallback(
    fetcher: &dyn Fetcher,
    url: &str,
) -> Result<DifficultyTable, TableError> {
    match fetch_table(fetcher, url) {
        Ok(t) => Ok(t),
        Err(e) => match MIRRORS.iter().find(|(u, _)| *u == url) {
            Some((_, mirror)) => {
                log::info!("{url} failed ({e}); trying mirror {mirror}");
                let mut t = fetch_table(fetcher, mirror)?;
                t.source_url = url.to_string();
                Ok(t)
            }
            None => Err(e),
        },
    }
}

/// Abstraction over HTTP so the logic can be unit-tested.
pub trait Fetcher {
    fn get(&self, url: &str) -> Result<String, TableError>;
}

#[cfg(not(target_arch = "wasm32"))]
pub struct UreqFetcher {
    agent: ureq::Agent,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for UreqFetcher {
    fn default() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("vibeBMS/", env!("CARGO_PKG_VERSION")))
            .build();
        UreqFetcher { agent }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Fetcher for UreqFetcher {
    fn get(&self, url: &str) -> Result<String, TableError> {
        let resp = self
            .agent
            .get(url)
            .call()
            .map_err(|e| TableError::Http(e.to_string()))?;
        let mut buf = Vec::new();
        use std::io::Read;
        resp.into_reader()
            .take(64 << 20)
            .read_to_end(&mut buf)
            .map_err(|e| TableError::Http(e.to_string()))?;
        Ok(decode_body(&buf))
    }
}

/// Fetch and parse a table from its page or header URL.
pub fn fetch_table(fetcher: &dyn Fetcher, url: &str) -> Result<DifficultyTable, TableError> {
    let first = fetcher.get(url)?;
    let trimmed = first.trim_start();
    let (header_url, header_text) = if trimmed.starts_with('{') {
        (url.to_string(), first.clone())
    } else {
        let meta = find_bmstable_meta(&first)
            .ok_or_else(|| TableError::Format("no <meta name=\"bmstable\"> found".into()))?;
        let hu = join(url, &meta)?;
        let text = fetcher.get(&hu)?;
        (hu, text)
    };
    let hv: Value = serde_json::from_str(header_text.trim_start_matches('\u{feff}'))
        .map_err(|e| TableError::Format(format!("header: {e}")))?;
    let header = parse_header(&hv)?;
    let data_url = join(&header_url, &header.data_url)?;
    let dt = fetcher.get(&data_url)?;
    let dv: Value = serde_json::from_str(dt.trim_start_matches('\u{feff}'))
        .map_err(|e| TableError::Format(format!("data: {e}")))?;
    let entries = parse_data(&dv)?;
    let fetched_at = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(DifficultyTable {
        source_url: url.to_string(),
        header,
        entries,
        fetched_at,
    })
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

    pub fn by_level(&self) -> BTreeMap<usize, (String, Vec<&TableEntry>)> {
        let levels = self.levels();
        let mut m = BTreeMap::new();
        for (i, l) in levels.iter().enumerate() {
            m.insert(
                i,
                (
                    l.clone(),
                    self.entries.iter().filter(|e| &e.level == l).collect(),
                ),
            );
        }
        m
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
        fn get(&self, url: &str) -> Result<String, TableError> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| TableError::Http(format!("404 {url}")))
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
