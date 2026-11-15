//! A difficulty table for players that cannot read one, or not this one
//! (as BeMusicSeeker writes its playlists out):
//!
//! - LR2: a custom folder per level ([`lr2_folders`]): `.lr2folder` files
//!   whose SQL picks the level's charts from LR2's song database by MD5. Put
//!   their folder among LR2's song folders.
//! - beatoraja: the table's cache ([`beatoraja_bmt`], named
//!   [`bmt_file_name`]) in its table folder (`tablepath` in
//!   `config_sys.json`), as beatoraja writes one after reading a table: it
//!   shows the table without reading it (a table it cannot reach, or with
//!   [`links`](crate::links) filled).

use crate::table::DifficultyTable;
use serde_json::{json, Value};

/// `s` as an SQL string literal.
fn sql_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A 32-digit hex MD5 (LR2 knows charts by it), in lower case.
fn md5(s: &str) -> Option<String> {
    let s = s.trim();
    (s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_ascii_lowercase())
}

/// The table's LR2 custom folders, one per level in the table's order:
/// (file name, its Shift_JIS text). Levels without an MD5 are left out.
pub fn lr2_folders(t: &DifficultyTable) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for level in t.levels() {
        let hashes: Vec<String> = t
            .entries
            .iter()
            .filter(|e| e.level == level)
            .filter_map(|e| md5(&e.md5))
            .map(|h| sql_quote(&h))
            .collect();
        if hashes.is_empty() {
            continue;
        }
        let text = format!(
            "#COMMAND song.hash IN ({})\r\n#MAXTRACKS 0\r\n#CATEGORY {}\r\n#TITLE {}\r\n\
             #INFORMATION_A {}\r\n#INFORMATION_B {} charts\r\n\r\n",
            hashes.join(","),
            one_line(&t.header.name),
            one_line(&t.label(&level)),
            one_line(&t.header.name),
            hashes.len(),
        );
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode(&text);
        out.push((format!("{:04}.lr2folder", out.len()), sjis.into_owned()));
    }
    out
}

fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

/// A table's course constraint as beatoraja names it.
fn constraint_name(c: &str) -> Option<&'static str> {
    Some(match c.trim().to_ascii_lowercase().as_str() {
        "class" | "grade" => "CLASS",
        "mirror" | "grade_mirror" => "MIRROR",
        "random" | "grade_random" => "RANDOM",
        "no_speed" => "NO_SPEED",
        "no_good" => "NO_GOOD",
        "no_great" => "NO_GREAT",
        "gauge_lr2" => "GAUGE_LR2",
        "gauge_5k" | "gauge_5keys" => "GAUGE_5KEYS",
        "gauge_7k" | "gauge_7keys" => "GAUGE_7KEYS",
        "gauge_9k" | "gauge_9keys" => "GAUGE_9KEYS",
        "gauge_24k" | "gauge_24keys" => "GAUGE_24KEYS",
        "ln" => "LN",
        "cn" => "CN",
        "hcn" => "HCN",
        _ => return None,
    })
}

/// The file name beatoraja gives a table's cache: the SHA-256 (hex) of its
/// URL, `.bmt`.
pub fn bmt_file_name(url: &str) -> String {
    use sha2::Digest;
    let h = sha2::Sha256::digest(url.as_bytes());
    let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}.bmt")
}

/// The table as beatoraja caches it: gzip of its JSON (`url`, `name`,
/// `tag`, a `folder` per level with its `songs`, and the `course`s).
pub fn beatoraja_bmt(t: &DifficultyTable) -> Vec<u8> {
    use std::io::Write;
    let folders: Vec<Value> = t
        .levels()
        .into_iter()
        .map(|level| {
            let songs: Vec<Value> = t
                .entries
                .iter()
                .filter(|e| e.level == level)
                .filter(|e| !e.md5.is_empty() || !e.sha256.is_empty())
                .map(|e| {
                    let mut s = json!({ "title": e.title });
                    for (k, v) in [
                        ("artist", &e.artist),
                        ("md5", &e.md5.to_ascii_lowercase()),
                        ("sha256", &e.sha256.to_ascii_lowercase()),
                        ("url", &e.url),
                        ("appendurl", &e.url_diff),
                    ] {
                        if !v.is_empty() {
                            s[k] = json!(v);
                        }
                    }
                    s
                })
                .collect();
            json!({ "name": t.label(&level), "songs": songs })
        })
        .collect();
    let courses: Vec<Value> = t
        .header
        .courses
        .iter()
        .map(|c| {
            let hash: Vec<Value> = c
                .md5
                .iter()
                .map(|h| json!({ "md5": h.to_ascii_lowercase(), "title": "" }))
                .chain(c.sha256.iter().map(|h| json!({ "sha256": h.to_ascii_lowercase(), "title": "" })))
                .collect();
            let mut v = json!({ "name": c.name, "hash": hash });
            let mut constraint: Vec<&str> = Vec::new();
            for c in c.constraint.iter().filter_map(|c| constraint_name(c)) {
                if !constraint.contains(&c) {
                    constraint.push(c);
                }
            }
            if !constraint.is_empty() {
                v["constraint"] = json!(constraint);
            }
            if !c.trophy.is_empty() {
                v["trophy"] = json!(c
                    .trophy
                    .iter()
                    .map(|t| json!({ "name": t.name, "missrate": t.missrate, "scorerate": t.scorerate }))
                    .collect::<Vec<_>>());
            }
            v
        })
        .collect();
    let data = json!({
        "url": t.source_url,
        "name": t.header.name,
        "tag": t.header.symbol,
        "folder": folders,
        "course": courses,
    });
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = gz.write_all(data.to_string().as_bytes());
    gz.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{TableEntry, TableHeader};

    fn table() -> DifficultyTable {
        let e = |level: &str, md5: &str, title: &str| TableEntry {
            md5: md5.into(),
            level: level.into(),
            title: title.into(),
            ..Default::default()
        };
        DifficultyTable {
            source_url: "https://example.com/table.html".into(),
            header: TableHeader {
                name: "発狂BMS難易度表".into(),
                symbol: "★".into(),
                level_order: vec!["1".into(), "2".into(), "3".into()],
                ..Default::default()
            },
            entries: vec![
                e("2", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "B"),
                e("1", "0123456789abcdef0123456789abcdef", "A"),
                e("1", "not an md5", "C"),
                e("3", "", "D"),
            ],
            fetched_at: 0,
        }
    }

    #[test]
    fn lr2_custom_folders() {
        let f = lr2_folders(&table());
        assert_eq!(f.len(), 2); // level 3 has no MD5
        assert_eq!(f[0].0, "0000.lr2folder");
        let (text, _, _) = encoding_rs::SHIFT_JIS.decode(&f[0].1);
        assert!(text.starts_with("#COMMAND song.hash IN ('0123456789abcdef0123456789abcdef')\r\n"));
        assert!(text.contains("#CATEGORY 発狂BMS難易度表\r\n#TITLE ★1\r\n"));
        let (text, _, _) = encoding_rs::SHIFT_JIS.decode(&f[1].1);
        assert!(text.contains("'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'") && text.contains("#TITLE ★2"));
    }

    #[test]
    fn beatoraja_cache() {
        use std::io::Read;
        let mut json = String::new();
        flate2::read::GzDecoder::new(beatoraja_bmt(&table()).as_slice())
            .read_to_string(&mut json)
            .unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["url"], "https://example.com/table.html");
        assert_eq!(v["folder"][0]["name"], "★1");
        assert_eq!(v["folder"][0]["songs"].as_array().unwrap().len(), 2);
        assert_eq!(
            v["folder"][1]["songs"][0]["md5"],
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(v["folder"][2]["songs"].as_array().unwrap().len(), 0);
        assert_eq!(
            bmt_file_name("a"),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb.bmt"
        );
    }
}
