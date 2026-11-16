//! Records out (JSON Lines or text) and in (JSON Lines, a JSON array or
//! object, from a file or stdin).

use serde_json::{Map, Value};
use std::io::Write;
use std::path::Path;

pub struct Out {
    pub json: bool,
}

impl Out {
    /// One record: a JSON line with `--json`, else `text`.
    pub fn record(&self, v: Value, text: impl FnOnce() -> String) {
        let line = if self.json { v.to_string() } else { text() };
        let mut o = std::io::stdout().lock();
        let _ = writeln!(o, "{line}");
        let _ = o.flush();
    }

    /// A record that is always JSON (an event's works and checks).
    pub fn json(&self, v: &Value) {
        let mut o = std::io::stdout().lock();
        let _ = writeln!(o, "{v}");
        let _ = o.flush();
    }

    /// A note for a person (stderr, never in the records).
    pub fn note(&self, text: impl AsRef<str>) {
        eprintln!("{}", text.as_ref());
    }
}

/// `path`'s text, or stdin's for `-`.
pub fn read_text(path: &Path) -> Result<String, String> {
    if path == Path::new("-") {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| e.to_string())?;
        Ok(s)
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Records from `text`: JSON Lines, a JSON array of records, or a JSON
/// object of records by `key` (`{"42": {...}}`: each gets `key` = its
/// name, as a number if it is one).
pub fn records(text: &str, key: &str) -> Result<Vec<Map<String, Value>>, String> {
    let t = text.trim_start_matches('\u{feff}').trim();
    if t.is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return match v {
            Value::Array(a) => Ok(a
                .into_iter()
                .filter_map(|r| match r {
                    Value::Object(o) => Some(o),
                    _ => None,
                })
                .collect()),
            Value::Object(o) if o.contains_key(key) => Ok(vec![o]),
            Value::Object(o) => Ok(o
                .into_iter()
                .filter_map(|(k, r)| match r {
                    Value::Object(r) => {
                        // the key first, as a record of its own would have it
                        let id = k.parse::<u64>().map(Value::from).unwrap_or(Value::from(k));
                        let mut rec = Map::new();
                        rec.insert(key.into(), id);
                        rec.extend(r);
                        Some(rec)
                    }
                    _ => None,
                })
                .collect()),
            _ => Err("not records".into()),
        };
    }
    t.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| match serde_json::from_str::<Value>(l) {
            Ok(Value::Object(o)) => Ok(o),
            Ok(_) => Err(format!("line {}: not a record", i + 1)),
            Err(e) => Err(format!("line {}: {e}", i + 1)),
        })
        .collect()
}

/// A record's string field.
pub fn s(r: &Map<String, Value>, k: &str) -> String {
    r.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_records() {
        let lines = "{\"num\":1}\n\n{\"num\":2}\n";
        assert_eq!(records(lines, "num").unwrap().len(), 2);
        assert_eq!(
            records("[{\"num\":1},{\"num\":2}]", "num").unwrap().len(),
            2
        );
        let by_key = records(r#"{"15":{"note":"x"},"16":{}}"#, "num").unwrap();
        assert_eq!(by_key[0]["num"], 15);
        let by_url = records(r#"{"https://a/b.zip":{"state":"gone"}}"#, "url").unwrap();
        assert_eq!(by_url[0]["url"], "https://a/b.zip");
        assert_eq!(records(r#"{"num":3,"title":"t"}"#, "num").unwrap().len(), 1);
        assert!(records("{\"num\":1}\nnot json\n", "num").is_err());
    }
}
