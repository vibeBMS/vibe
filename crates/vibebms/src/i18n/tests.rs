use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The Rust string literal at the start of `s` (after `tr!(` and blanks):
/// its text, or `None` when there is none.
fn literal(s: &str) -> Option<String> {
    let s = s.trim_start();
    if let Some(raw) = s.strip_prefix('r') {
        let hashes = raw.chars().take_while(|&c| c == '#').count();
        let body = raw[hashes..].strip_prefix('"')?;
        let end = format!("\"{}", "#".repeat(hashes));
        return Some(body[..body.find(&end)?].to_string());
    }
    let mut it = s.strip_prefix('"')?.chars().peekable();
    let mut out = String::new();
    while let Some(c) = it.next() {
        match c {
            '"' => return Some(out),
            '\\' => match it.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '0' => out.push('\0'),
                '\n' => {
                    while it.peek().is_some_and(|c| c.is_whitespace()) {
                        it.next();
                    }
                }
                'u' => {
                    it.next(); // {
                    let hex: String = it.by_ref().take_while(|&c| c != '}').collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// Where the next `tr!(` or `tr_key!(` in `s` is, and its length.
fn next_call(s: &str) -> Option<(usize, usize)> {
    let a = s.find("tr!(").map(|i| (i, 4));
    let b = s.find("tr_key!(").map(|i| (i, 8));
    match (a, b) {
        (Some(a), Some(b)) => Some(if a.0 < b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// Every key the code passes to `tr!`, with the files it is in (by path
/// under `src`), in the order first found.
fn code_keys() -> Vec<(String, String)> {
    let src = crate_dir().join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let mut seen = BTreeSet::new();
    let mut keys = Vec::new();
    for f in files {
        let rel = f.strip_prefix(&src).unwrap().display().to_string();
        if rel == "i18n.rs" || rel.starts_with("i18n/") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        let mut rest = text.as_str();
        while let Some((i, len)) = next_call(rest) {
            let word = rest[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            rest = &rest[i + len..];
            if word {
                continue;
            }
            let key = literal(rest)
                .unwrap_or_else(|| panic!("{rel}: tr! without a string literal: {:.40}", rest));
            if seen.insert(key.clone()) {
                keys.push((rel.clone(), key));
            }
        }
    }
    keys
}

/// A TOML basic string for `s`.
fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// en.toml as it should be: each key with itself, grouped by file.
fn english_toml(keys: &[(String, String)]) -> String {
    let mut out = String::from(
        "# Every text vibeBMS translates, in English (the key the other tables use).\n\
         # Generated from the code's tr!(...) calls: I18N_BLESS=1 cargo test -p vibebms i18n\n",
    );
    let mut file = "";
    for (f, k) in keys {
        if f != file {
            out.push_str(&format!("\n# {f}\n"));
            file = f;
        }
        let q = toml_str(k);
        out.push_str(&format!("{q} = {q}\n"));
    }
    out
}

#[test]
fn english_table_lists_the_codes_keys() {
    let want = english_toml(&code_keys());
    let path = crate_dir().join("assets/i18n/en.toml");
    if std::env::var_os("I18N_BLESS").is_some() {
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "assets/i18n/en.toml is not the code's keys: I18N_BLESS=1 cargo test -p vibebms i18n"
    );
}

/// The `{name}` placeholders of `s`.
fn placeholders(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j)
                if after[..j]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_') =>
            {
                out.insert(after[..j].to_string());
                rest = &after[j + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

#[test]
fn every_language_has_every_key() {
    let keys: BTreeSet<String> = code_keys().into_iter().map(|(_, k)| k).collect();
    let mut problems = Vec::new();
    for lang in Lang::ALL {
        let Some(src) = lang.source() else { continue };
        let tag = lang.tag();
        let v: toml::Table = toml::from_str(src).unwrap_or_else(|e| panic!("i18n/{tag}.toml: {e}"));
        let untranslated: BTreeSet<String> = match v.get(UNTRANSLATED) {
            Some(toml::Value::Array(a)) => a
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => BTreeSet::new(),
        };
        let mut have = BTreeMap::new();
        for (k, val) in &v {
            if k == UNTRANSLATED {
                continue;
            }
            match val.as_str() {
                Some(s) => {
                    have.insert(k.clone(), s.to_string());
                }
                None => problems.push(format!("{tag}: {k:?} is not a string")),
            }
        }
        for k in &keys {
            match have.get(k) {
                Some(t) if placeholders(t) != placeholders(k) => {
                    problems.push(format!("{tag}: placeholders differ: {k:?} -> {t:?}"))
                }
                Some(_) => {}
                None if untranslated.contains(k) => {}
                None => problems.push(format!("{tag}: missing {}", toml_str(k))),
            }
        }
        for k in have.keys().chain(&untranslated) {
            if !keys.contains(k) {
                problems.push(format!("{tag}: not in the code: {}", toml_str(k)));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn tables_parse() {
    for lang in Lang::ALL {
        if let Some(src) = lang.source() {
            assert!(!parse(src, lang.tag()).is_empty() || src.trim().is_empty());
        }
    }
}

#[test]
fn locales() {
    for (tag, want) in [
        ("en-US", Lang::En),
        ("C", Lang::En),
        ("ko-KR", Lang::Ko),
        ("ko_KR.UTF-8", Lang::Ko),
        ("ja", Lang::Ja),
        ("ja-JP", Lang::Ja),
        ("zh-CN", Lang::ZhHans),
        ("zh_SG", Lang::ZhHans),
        ("zh-Hans-HK", Lang::ZhHans),
        ("zh-TW", Lang::ZhHant),
        ("zh_HK.UTF-8", Lang::ZhHant),
        ("zh-Hant", Lang::ZhHant),
        ("zh", Lang::ZhHans),
        ("fr-FR", Lang::En),
    ] {
        assert_eq!(Lang::from_locale(tag), want, "{tag}");
    }
}

#[test]
fn fills_placeholders() {
    assert_eq!(fill("{a} and {b}", &[("a", &1), ("b", &"x")]), "1 and x");
    assert_eq!(fill("{a} {missing} {", &[("a", &"y")]), "y {missing} {");
    assert_eq!(fill("{ not one }", &[]), "{ not one }");
}

#[test]
fn falls_back_to_english() {
    set(Lang::Ko);
    assert_eq!(get("a text no table has"), "a text no table has");
    set(Lang::En);
}

#[test]
fn literals() {
    assert_eq!(literal(r#""a \"b\" c", x)"#).as_deref(), Some("a \"b\" c"));
    assert_eq!(literal("\"one \\\n    two\")").as_deref(), Some("one two"));
    assert_eq!(literal(r##"r#"raw "x""#)"##).as_deref(), Some("raw \"x\""));
    assert_eq!(literal("\"\\u{2192}\"").as_deref(), Some("→"));
}
