//! The UI's languages: English (the text in the code), Korean, Japanese,
//! Simplified and Traditional Chinese.
//!
//! The English text is the key (as gettext's msgid): `tr!("Player name")`
//! is that text in the language in use, from `assets/i18n/<lang>.toml`
//! (embedded), or the English when the table lacks it. Placeholders are
//! named, `{name}`: `tr!("Profile {name} deleted", name = p.name)`.
//! `assets/i18n/en.toml` lists every key the code has (a test keeps it so,
//! `I18N_BLESS=1` rewrites it), for translators; the tests check that each
//! language has every key (or lists it as untranslated) with the same
//! placeholders.
//!
//! The language is the player's choice (SETTINGS › SYSTEM › LANGUAGE),
//! AUTO being the system's ([`system_locale`]). Log messages and the
//! command line stay English, as do the terms players use in English
//! (EX SCORE, PGREAT, gauge names, RANDOM, HI-SPEED, ...). Values skins
//! read (their text ids) are never translated.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// The language setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    /// The system's (English when it is none of the others).
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "ko")]
    Ko,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "zh-Hans")]
    ZhHans,
    #[serde(rename = "zh-Hant")]
    ZhHant,
}

impl Language {
    /// In the order the setting steps through them.
    pub const ALL: [Language; 6] = [
        Language::Auto,
        Language::En,
        Language::Ko,
        Language::Ja,
        Language::ZhHans,
        Language::ZhHant,
    ];

    pub fn is_auto(&self) -> bool {
        *self == Language::Auto
    }

    /// The language text is shown in: the system's for AUTO.
    pub fn resolve(self) -> Lang {
        match self {
            Language::Auto => system_lang(),
            Language::En => Lang::En,
            Language::Ko => Lang::Ko,
            Language::Ja => Lang::Ja,
            Language::ZhHans => Lang::ZhHans,
            Language::ZhHant => Lang::ZhHant,
        }
    }
}

/// A language the UI is shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
    Ja,
    ZhHans,
    ZhHant,
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::En, Lang::Ko, Lang::Ja, Lang::ZhHans, Lang::ZhHant];

    /// Its name in itself (never translated).
    pub fn native(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ko => "한국어",
            Lang::Ja => "日本語",
            Lang::ZhHans => "简体中文",
            Lang::ZhHant => "繁體中文",
        }
    }

    /// The BCP 47 tag (and the table's file name).
    pub fn tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ko => "ko",
            Lang::Ja => "ja",
            Lang::ZhHans => "zh-Hans",
            Lang::ZhHant => "zh-Hant",
        }
    }

    /// The language of locale `tag` (`ko-KR`, `ja_JP.UTF-8`, `zh-Hant-TW`,
    /// `zh_TW`, ...): English for any other.
    pub fn from_locale(tag: &str) -> Lang {
        let t = tag.to_ascii_lowercase().replace('_', "-");
        let mut parts = t.split(['-', '.', '@']);
        match parts.next().unwrap_or("") {
            "ko" => Lang::Ko,
            "ja" => Lang::Ja,
            "zh" => {
                let rest: Vec<&str> = parts.collect();
                if rest.contains(&"hant") {
                    Lang::ZhHant
                } else if rest.contains(&"hans") {
                    Lang::ZhHans
                } else if rest.iter().any(|r| matches!(*r, "tw" | "hk" | "mo")) {
                    Lang::ZhHant
                } else {
                    Lang::ZhHans
                }
            }
            _ => Lang::En,
        }
    }

    fn index(self) -> u8 {
        self as u8
    }

    fn from_index(i: u8) -> Lang {
        Lang::ALL.get(i as usize).copied().unwrap_or(Lang::En)
    }

    /// Its table file (none for English: the code's text).
    fn source(self) -> Option<&'static str> {
        match self {
            Lang::En => None,
            Lang::Ko => Some(include_str!("../assets/i18n/ko.toml")),
            Lang::Ja => Some(include_str!("../assets/i18n/ja.toml")),
            Lang::ZhHans => Some(include_str!("../assets/i18n/zh-Hans.toml")),
            Lang::ZhHant => Some(include_str!("../assets/i18n/zh-Hant.toml")),
        }
    }
}

/// The system's locale, as a tag (`None` when it has none).
pub fn system_locale() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.navigator().language())
    }
    // Android: the Java locale (the system properties sys-locale reads
    // are not always there)
    #[cfg(target_os = "android")]
    if let Some(l) = crate::platform::android::locale() {
        return Some(l);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys_locale::get_locale()
    }
}

/// The system's language (read once).
fn system_lang() -> Lang {
    static SYS: OnceLock<Lang> = OnceLock::new();
    *SYS.get_or_init(|| {
        let loc = system_locale();
        let lang = loc.as_deref().map_or(Lang::En, Lang::from_locale);
        log::info!("system locale: {loc:?} (UI: {})", lang.tag());
        lang
    })
}

// The language in use. Tests run each on its own thread: there it is the
// thread's, so a test that switches it does not change the others'.
#[cfg(not(test))]
static CURRENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
#[cfg(test)]
thread_local! {
    static CURRENT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// The language text is shown in now.
pub fn current() -> Lang {
    #[cfg(not(test))]
    let i = CURRENT.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    let i = CURRENT.with(|c| c.get());
    Lang::from_index(i)
}

/// Show text in `lang` from now on.
pub fn set(lang: Lang) {
    #[cfg(not(test))]
    CURRENT.store(lang.index(), std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    CURRENT.with(|c| c.set(lang.index()));
}

/// Apply the setting (at start and when it changes).
pub fn apply(setting: Language) {
    set(setting.resolve());
}

/// (A fast hash: every `tr!` call hashes its English text.)
type Table = rustc_hash::FxHashMap<&'static str, &'static str>;

/// The key of a table that lists keys left in English on purpose.
#[cfg(test)]
const UNTRANSLATED: &str = "_untranslated";

/// `lang`'s table: its file read once.
fn table(lang: Lang) -> Option<&'static Table> {
    static TABLES: [OnceLock<Table>; 5] = [const { OnceLock::new() }; 5];
    let src = lang.source()?;
    Some(TABLES[lang.index() as usize].get_or_init(|| parse(src)))
}

/// A table's `"key" = "value"` lines, the TOML the tables are written in
/// (its comments, blank lines and `_untranslated` list are passed over; a
/// test keeps the two readings the same). The text is the embedded
/// file's own where it has no escapes.
fn parse(src: &'static str) -> Table {
    let mut t = Table::default();
    for line in src.lines() {
        let entry = || {
            let (k, rest) = basic_string(line)?;
            let (v, rest) = basic_string(rest.strip_prefix(" = ")?)?;
            rest.is_empty().then_some((k, v))
        };
        if let Some((k, v)) = entry() {
            t.insert(k, v);
        }
    }
    t
}

/// The TOML basic string `s` starts with, and what follows it.
fn basic_string(s: &'static str) -> Option<(&'static str, &'static str)> {
    let body = s.strip_prefix('"')?;
    let (mut i, mut escaped) = (0, false);
    let end = loop {
        match body.as_bytes().get(i)? {
            b'"' => break i,
            b'\\' => (i, escaped) = (i + 2, true),
            _ => i += 1,
        }
    };
    let text = match escaped {
        true => Box::leak(unescape(&body[..end])?.into_boxed_str()),
        false => &body[..end],
    };
    Some((text, &body[end + 1..]))
}

/// `raw` with its escapes (those the tables use) made the characters.
fn unescape(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        out.push(match c {
            '\\' => match it.next()? {
                'n' => '\n',
                '"' => '"',
                '\\' => '\\',
                _ => return None,
            },
            c => c,
        });
    }
    Some(out)
}

/// `key` (English) in the language in use.
pub fn get(key: &'static str) -> &'static str {
    lookup(current(), key)
}

/// `key` in `lang`.
pub fn lookup(lang: Lang, key: &'static str) -> &'static str {
    match table(lang) {
        Some(t) => t.get(key).copied().unwrap_or(key),
        None => key,
    }
}

/// `template` with each `{name}` replaced by its value in `args` (a name
/// it lacks stays as it is).
pub fn fill(template: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j)
                if after[..j]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_') =>
            {
                let name = &after[..j];
                match args.iter().find(|(n, _)| *n == name) {
                    Some((_, v)) => {
                        let _ = write!(out, "{v}");
                    }
                    None => out.push_str(&rest[i..i + j + 2]),
                }
                rest = &after[j + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text `key` (an English string literal) in the language in use: a
/// `&'static str`, or with `name = value` pairs a `String` with each
/// `{name}` filled in.
macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::get($key)
    };
    ($key:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::get($key),
            &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),+],
        )
    };
}

/// `key` as it is, marked as a text to translate (for tables in
/// constants): `i18n::get(key)` shows it.
macro_rules! tr_key {
    ($key:literal) => {
        $key
    };
}

#[cfg(test)]
mod tests;
