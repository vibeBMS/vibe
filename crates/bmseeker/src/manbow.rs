//! manbow (manbow.nothing.sh), where BMS events are held: an event's works
//! and which of each work's download links to take.
//!
//! An event's URL list has every work with its links; a link's label is
//! the text on its line or the headings above it. [`pick`] guesses which
//! link is the main archive (a high quality one over a low quality one,
//! then ogg over wav, a file over a folder, the latest full package over
//! patches for earlier downloads), which are separate add-on archives
//! (added charts, a BGA on its own), and which are mirrors of the same
//! archive (kept as fallbacks).

use regex::Regex;
use std::sync::LazyLock;

pub const BASE: &str = "https://manbow.nothing.sh/event/event.cgi";

fn re(p: &str) -> Regex {
    Regex::new(&format!("(?i){p}")).expect("a valid pattern")
}

macro_rules! pattern {
    ($name:ident, $p:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| re($p));
    };
}

pattern!(MIRROR, r"mirror|ミラー|予備|backup|alt\b|サブ|sub\b");
pattern!(
    EXTRA,
    r"追加|差分|add|append|extra|patch|修正|update|fix|bonus|dp譜面|dp chart|別譜面|譜面のみ|charts? only|new \w* ?charts?"
);
// a full package, though its label speaks of a fix or an update
pattern!(
    FULL,
    r"new ?(ver|link|package|upload|download)|^\W*new\W*$|最新|latest|fixed ?ver|complete|^\W*本体|recommend|推奨|以降はこちら|updated|更新版"
);
// a package with its fixes and additions in ("本体(差分適用済)")
pattern!(APPLIED, r"適用済|applied|includ|同梱|込み|全部入り");
// for those who downloaded an earlier version: a patch
pattern!(
    EARLIER,
    r"以前に.{0,24}(dl|ダウンロード)|before .{0,24}download|for package before"
);
pattern!(AGAIN, r"再度|もう一度|re-?download");
// what only adds to a package ("差分のみ", "BGA only")
pattern!(ADDON_ONLY, r"のみ|だけ|only|単体|差分|patch|sabun");
// a separate BGA archive ("BGA only", "HQ BGA", "高画質BGA")
pattern!(
    BGA_ONLY,
    r"bga ?(only|単体|のみ)|only ?bga|^\W*(hq ?|sd画質|高画質|仮の|updated |new |fixed )?bga\b"
);
// "beatoraja用高画質BGA (HQ BGA for beatoraja)": a high quality BGA alone,
// unless the label speaks of a package ("ogg HQ BGA", "HQBGA版")
pattern!(HQ_BGA, r"(高画質|hq|hd) ?bga");
pattern!(
    PACKAGE,
    r"本体|bms|ogg|wav|package|パッケージ|full|同梱|込|入り|with|版|ver"
);
pattern!(
    HOSTS,
    r"g-?drive|google ?drive|drive|dropbox|onedrive|one ?drive|mega|mediafire|url ?\d*|link|dl|download|ダウンロード|[\[\]()（）:：・\-]"
);
pattern!(OGG_WAV, r"ogg|wav");
pattern!(BGA, r"bga");

/// Weights of what a label says. HQ / LQ marks outweigh the audio format:
/// an HQ wav beats an LQ ogg.
static GOOD: LazyLock<Vec<(Regex, i32)>> = LazyLock::new(|| {
    vec![
        // the newer of an old and a new version
        (
            re(
                r"new ?(ver|link|package|upload|download)|^\W*new\W*$|最新|latest|fixed|修正版|recommend|推奨|以降はこちら|updated",
            ),
            3,
        ),
        (re(r"hq|高画質|高品質|high ?quality"), 6),
        (re(r"ogg"), 4),
        (re(r"high|beatoraja|mp4|full|完全"), 2),
    ]
});
static BAD: LazyLock<Vec<(Regex, i32)>> = LazyLock::new(|| {
    vec![
        (re(r"lq|低画質|低品質|low ?quality"), -6),
        (re(r"wav|flac"), -4),
        (
            re(r"low|lr2|mpg|wmv|compact|lite|軽量|no ?bga|bga ?(なし|無し)|without"),
            -2,
        ),
        (re(r"\bold\b|old ?ver|旧|not fixed"), -3),
        // a team's songs in one archive
        (
            re(r"チーム曲|チーム.{0,12}(全部入り|パッケージ)|team\b.{0,40}pack|all (team )?songs"),
            -8,
        ),
    ]
});

fn hq() -> &'static Regex {
    &GOOD[1].0
}
fn lq() -> &'static Regex {
    &BAD[0].0
}
fn old() -> &'static Regex {
    &BAD[3].0
}

fn bga_only(label: &str) -> bool {
    BGA_ONLY.is_match(label) || HQ_BGA.is_match(label) && !PACKAGE.is_match(label)
}

fn is_folder(url: &str) -> bool {
    url.contains("/folders/") || url.contains("/scl/fo/") || url.contains("/drive/u/")
}

/// The label without host names and mirror words: mirrors of one archive
/// share it.
fn variant(label: &str) -> String {
    let v = MIRROR.replace_all(label, "");
    let v = HOSTS.replace_all(&v, " ");
    v.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// HTML 4's named character references (sorted by name).
const ENTITIES: &[(&str, u32)] = &[
    ("AElig", 198),
    ("Aacute", 193),
    ("Acirc", 194),
    ("Agrave", 192),
    ("Alpha", 913),
    ("Aring", 197),
    ("Atilde", 195),
    ("Auml", 196),
    ("Beta", 914),
    ("Ccedil", 199),
    ("Chi", 935),
    ("Dagger", 8225),
    ("Delta", 916),
    ("ETH", 208),
    ("Eacute", 201),
    ("Ecirc", 202),
    ("Egrave", 200),
    ("Epsilon", 917),
    ("Eta", 919),
    ("Euml", 203),
    ("Gamma", 915),
    ("Iacute", 205),
    ("Icirc", 206),
    ("Igrave", 204),
    ("Iota", 921),
    ("Iuml", 207),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Ntilde", 209),
    ("Nu", 925),
    ("OElig", 338),
    ("Oacute", 211),
    ("Ocirc", 212),
    ("Ograve", 210),
    ("Omega", 937),
    ("Omicron", 927),
    ("Oslash", 216),
    ("Otilde", 213),
    ("Ouml", 214),
    ("Phi", 934),
    ("Pi", 928),
    ("Prime", 8243),
    ("Psi", 936),
    ("Rho", 929),
    ("Scaron", 352),
    ("Sigma", 931),
    ("THORN", 222),
    ("Tau", 932),
    ("Theta", 920),
    ("Uacute", 218),
    ("Ucirc", 219),
    ("Ugrave", 217),
    ("Upsilon", 933),
    ("Uuml", 220),
    ("Xi", 926),
    ("Yacute", 221),
    ("Yuml", 376),
    ("Zeta", 918),
    ("aacute", 225),
    ("acirc", 226),
    ("acute", 180),
    ("aelig", 230),
    ("agrave", 224),
    ("alefsym", 8501),
    ("alpha", 945),
    ("amp", 38),
    ("and", 8743),
    ("ang", 8736),
    ("aring", 229),
    ("asymp", 8776),
    ("atilde", 227),
    ("auml", 228),
    ("bdquo", 8222),
    ("beta", 946),
    ("brvbar", 166),
    ("bull", 8226),
    ("cap", 8745),
    ("ccedil", 231),
    ("cedil", 184),
    ("cent", 162),
    ("chi", 967),
    ("circ", 710),
    ("clubs", 9827),
    ("cong", 8773),
    ("copy", 169),
    ("crarr", 8629),
    ("cup", 8746),
    ("curren", 164),
    ("dArr", 8659),
    ("dagger", 8224),
    ("darr", 8595),
    ("deg", 176),
    ("delta", 948),
    ("diams", 9830),
    ("divide", 247),
    ("eacute", 233),
    ("ecirc", 234),
    ("egrave", 232),
    ("empty", 8709),
    ("emsp", 8195),
    ("ensp", 8194),
    ("epsilon", 949),
    ("equiv", 8801),
    ("eta", 951),
    ("eth", 240),
    ("euml", 235),
    ("euro", 8364),
    ("exist", 8707),
    ("fnof", 402),
    ("forall", 8704),
    ("frac12", 189),
    ("frac14", 188),
    ("frac34", 190),
    ("frasl", 8260),
    ("gamma", 947),
    ("ge", 8805),
    ("gt", 62),
    ("hArr", 8660),
    ("harr", 8596),
    ("hearts", 9829),
    ("hellip", 8230),
    ("iacute", 237),
    ("icirc", 238),
    ("iexcl", 161),
    ("igrave", 236),
    ("image", 8465),
    ("infin", 8734),
    ("int", 8747),
    ("iota", 953),
    ("iquest", 191),
    ("isin", 8712),
    ("iuml", 239),
    ("kappa", 954),
    ("lArr", 8656),
    ("lambda", 955),
    ("lang", 9001),
    ("laquo", 171),
    ("larr", 8592),
    ("lceil", 8968),
    ("ldquo", 8220),
    ("le", 8804),
    ("lfloor", 8970),
    ("lowast", 8727),
    ("loz", 9674),
    ("lrm", 8206),
    ("lsaquo", 8249),
    ("lsquo", 8216),
    ("lt", 60),
    ("macr", 175),
    ("mdash", 8212),
    ("micro", 181),
    ("middot", 183),
    ("minus", 8722),
    ("mu", 956),
    ("nabla", 8711),
    ("nbsp", 160),
    ("ndash", 8211),
    ("ne", 8800),
    ("ni", 8715),
    ("not", 172),
    ("notin", 8713),
    ("nsub", 8836),
    ("ntilde", 241),
    ("nu", 957),
    ("oacute", 243),
    ("ocirc", 244),
    ("oelig", 339),
    ("ograve", 242),
    ("oline", 8254),
    ("omega", 969),
    ("omicron", 959),
    ("oplus", 8853),
    ("or", 8744),
    ("ordf", 170),
    ("ordm", 186),
    ("oslash", 248),
    ("otilde", 245),
    ("otimes", 8855),
    ("ouml", 246),
    ("para", 182),
    ("part", 8706),
    ("permil", 8240),
    ("perp", 8869),
    ("phi", 966),
    ("pi", 960),
    ("piv", 982),
    ("plusmn", 177),
    ("pound", 163),
    ("prime", 8242),
    ("prod", 8719),
    ("prop", 8733),
    ("psi", 968),
    ("quot", 34),
    ("rArr", 8658),
    ("radic", 8730),
    ("rang", 9002),
    ("raquo", 187),
    ("rarr", 8594),
    ("rceil", 8969),
    ("rdquo", 8221),
    ("real", 8476),
    ("reg", 174),
    ("rfloor", 8971),
    ("rho", 961),
    ("rlm", 8207),
    ("rsaquo", 8250),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("scaron", 353),
    ("sdot", 8901),
    ("sect", 167),
    ("shy", 173),
    ("sigma", 963),
    ("sigmaf", 962),
    ("sim", 8764),
    ("spades", 9824),
    ("sub", 8834),
    ("sube", 8838),
    ("sum", 8721),
    ("sup", 8835),
    ("sup1", 185),
    ("sup2", 178),
    ("sup3", 179),
    ("supe", 8839),
    ("szlig", 223),
    ("tau", 964),
    ("there4", 8756),
    ("theta", 952),
    ("thetasym", 977),
    ("thinsp", 8201),
    ("thorn", 254),
    ("tilde", 732),
    ("times", 215),
    ("trade", 8482),
    ("uArr", 8657),
    ("uacute", 250),
    ("uarr", 8593),
    ("ucirc", 251),
    ("ugrave", 249),
    ("uml", 168),
    ("upsih", 978),
    ("upsilon", 965),
    ("uuml", 252),
    ("weierp", 8472),
    ("xi", 958),
    ("yacute", 253),
    ("yen", 165),
    ("yuml", 255),
    ("zeta", 950),
    ("zwj", 8205),
    ("zwnj", 8204),
];

/// HTML's character references (`&amp;`, `&#12354;`, `&#x3042;`...).
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|e| *e <= 10);
        let decoded = end.and_then(|e| {
            let name = &rest[1..e];
            let c = if let Some(n) = name.strip_prefix('#') {
                let v = match n.strip_prefix(['x', 'X']) {
                    Some(h) => u32::from_str_radix(h, 16).ok()?,
                    None => n.parse().ok()?,
                };
                char::from_u32(v)?
            } else {
                if name == "apos" {
                    '\''
                } else {
                    let i = ENTITIES.binary_search_by(|(k, _)| (*k).cmp(name)).ok()?;
                    char::from_u32(ENTITIES[i].1)?
                }
            };
            Some((c, e + 1))
        });
        // as HTML5 reads them: a few old names need no ';' ("&lt", "&eacute")
        let decoded = decoded.or_else(|| {
            (2..=6).rev().find_map(|n| {
                let name = rest.get(1..1 + n)?;
                let i = ENTITIES.binary_search_by(|(k, _)| (*k).cmp(name)).ok()?;
                let cp = ENTITIES[i].1;
                if cp <= 0xff {
                    Some((char::from_u32(cp)?, 1 + n))
                } else {
                    None
                }
            })
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A cell's text: tags dropped, references decoded, trimmed.
fn text(s: &str) -> String {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("pattern"));
    unescape(&TAG.replace_all(s, "")).trim().to_string()
}

/// `[(label, url)]` of a URL list's address cell: a line's own text, or the
/// last heading lines above it.
pub fn links_of(cell: &str) -> Vec<(String, String)> {
    static BR: LazyLock<Regex> = LazyLock::new(|| re(r"<br\s*/?>"));
    static HREF: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"href="(https?://[^"]+)""#).expect("pattern"));
    let mut out: Vec<(String, String)> = Vec::new();
    let mut heading: Vec<String> = Vec::new();
    for raw in BR.split(cell) {
        let urls: Vec<&str> = HREF
            .captures_iter(raw)
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .collect();
        let mut t = text(raw);
        for u in &urls {
            t = t.replace(&unescape(u), "");
        }
        let t = t.trim_matches([' ', ':', '：', '-']).to_string();
        if urls.is_empty() {
            // headings stack ("[ogg]" then "Google Drive:") until a gap
            if t.is_empty() {
                heading.clear();
            } else {
                heading.push(t);
            }
            continue;
        }
        for u in urls {
            let u = unescape(u);
            if out.iter().all(|(_, x)| *x != u) {
                let mut parts: Vec<&str> = heading
                    .iter()
                    .skip(heading.len().saturating_sub(2))
                    .map(String::as_str)
                    .collect();
                parts.push(&t);
                out.push((parts.join(" ").trim().to_string(), u));
            }
        }
        // a host heading ("Google Drive:") names just the link below it
        if heading.last().is_some_and(|h| variant(h).is_empty()) {
            heading.pop();
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The file name in a URL, if it shows one (`.../song_ogg.zip`).
fn file_part(url: &str) -> String {
    let after = url.split_once("://").map_or(url, |(_, r)| r);
    let path = after.find('/').map_or("", |i| &after[i..]);
    let path = path.split(['?', '#']).next().unwrap_or("");
    let path = percent_decode(&percent_decode(path));
    path.split('/')
        .rfind(|p| p.contains('.'))
        .unwrap_or("")
        .to_string()
}

/// Whether the links are one package with a low and a high quality BGA
/// ("(高画質BGA)" and "(低画質BGA)"), not an archive and its BGA.
fn bga_versions(links: &[(String, String)]) -> bool {
    let only =
        |q: &Regex, other: &Regex, l: &str| q.is_match(l) && !other.is_match(l) && BGA.is_match(l);
    links.iter().any(|(l, _)| only(lq(), hq(), l)) && links.iter().any(|(l, _)| only(hq(), lq(), l))
}

fn score(label: &str, url: &str, versions: bool) -> i32 {
    let label = format!("{label} {}", file_part(url));
    let mut s: i32 = GOOD
        .iter()
        .chain(BAD.iter())
        .filter(|(r, _)| r.is_match(&label))
        .map(|(_, w)| w)
        .sum();
    if bga_only(&label) && !versions {
        s -= 14;
    }
    if MIRROR.is_match(&label) {
        s -= 1;
    }
    if is_folder(url) {
        s -= 3;
    }
    if ["pan.baidu", "lanzou", "lanzn", "lanzoub"]
        .iter()
        .any(|h| url.contains(h))
    {
        s -= 5;
    }
    s
}

/// What [`pick`] makes of a work's links.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pick {
    /// The main archive's links, in the order to try (mirrors after).
    pub main: Vec<String>,
    /// Add-on archives, each with its mirrors.
    pub extra: Vec<Vec<String>>,
    /// Why a person should look at the guess ("" if it is clear).
    pub review: String,
}

/// The first of `items` with the highest key (as Python's `max`).
fn first_max<T>(items: impl IntoIterator<Item = T>, key: impl Fn(&T) -> i32) -> Option<T> {
    let mut best: Option<(i32, T)> = None;
    for x in items {
        let k = key(&x);
        if best.as_ref().is_none_or(|(b, _)| k > *b) {
            best = Some((k, x));
        }
    }
    best.map(|(_, x)| x)
}

/// Which of a work's `(label, URL)` links is its main archive, which are
/// add-ons, and which mirror them.
pub fn pick(links: &[(String, String)]) -> Pick {
    if links.is_empty() {
        return Pick {
            review: "no link".into(),
            ..Pick::default()
        };
    }
    let versions = bga_versions(links);
    let addon = |l: &str| {
        if bga_only(l) && !versions || EARLIER.is_match(l) && !AGAIN.is_match(l) {
            return true;
        }
        if FULL.is_match(l) && !ADDON_ONLY.is_match(l) || old().is_match(l) || APPLIED.is_match(l) {
            return false;
        }
        EXTRA.is_match(l) && !OGG_WAV.is_match(l)
    };
    type Link<'a> = &'a (String, String);
    let mut extra_links: Vec<Link> = links.iter().filter(|(l, _)| addon(l)).collect();
    let mut rest: Vec<Link> = links.iter().filter(|x| !extra_links.contains(x)).collect();
    if rest.is_empty() {
        // every link adds to a package: the best one that is not a BGA
        // alone is the package, the others stay add-ons
        let mut all: Vec<Link> = links.iter().filter(|(l, _)| !bga_only(l)).collect();
        if all.is_empty() {
            all = links.iter().collect();
        }
        let top = first_max(all.iter().copied(), |x| score(&x.0, &x.1, versions)).expect("links");
        let key = variant(&top.0);
        rest = all.into_iter().filter(|x| variant(&x.0) == key).collect();
    }
    extra_links.retain(|x| !rest.contains(x));
    let sc = |u: &str| {
        // as a dict by URL: the last link with that URL counts
        rest.iter()
            .rev()
            .find(|(_, x)| x == u)
            .map_or(0, |(l, x)| score(l, x, versions))
    };
    let best = first_max(rest.iter().copied(), |x| sc(&x.1)).expect("rest");
    let best_sc = sc(&best.1);
    let key = variant(&best.0);
    // a mirror of the main archive is no add-on
    extra_links.retain(|x| variant(&x.0) != key);
    let mut main = vec![best.1.clone()];
    main.extend(
        rest.iter()
            .filter(|(l, u)| {
                *u != best.1 && variant(l) == key && !is_folder(u) && sc(u) >= best_sc - 1
            })
            .map(|(_, u)| u.clone()),
    );
    let mut extra: Vec<(String, Vec<String>)> = Vec::new();
    for (l, u) in extra_links {
        let k = variant(l);
        match extra.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.1.push(u.clone()),
            None => extra.push((k, vec![u.clone()])),
        }
    }
    let mut why = Vec::new();
    let top = rest
        .iter()
        .any(|(l, u)| sc(u) == best_sc && variant(l) != key);
    if top {
        why.push("several links score the same");
    }
    if !extra.is_empty() {
        why.push("add-on archives guessed");
    }
    if is_folder(&best.1) {
        why.push("main link is a folder");
    }
    let marked = GOOD
        .iter()
        .chain(BAD.iter())
        .any(|(r, _)| r.is_match(&best.0));
    if rest.len() > 1 && !marked && !top {
        why.push("labels say little");
    }
    Pick {
        main,
        extra: extra.into_iter().map(|g| g.1).collect(),
        review: why.join("; "),
    }
}

/// A work of an event.
#[derive(Clone, Debug, PartialEq)]
pub struct Work {
    pub num: u32,
    pub title: String,
    pub artist: String,
    pub team: String,
    /// "Original", "Arrange", "Disq" (disqualified)... from the work list.
    pub kind: String,
    pub size_kb: Option<u64>,
    /// The work's page on the venue.
    pub page: String,
    pub links: Vec<(String, String)>,
    pub pick: Pick,
}

/// The event's URL list (every work and its links).
pub fn url_list_url(event: u32) -> String {
    format!("{BASE}?action=URLList&event={event}&start=0&end=9999")
}

/// The event's work list (each work's kind).
pub fn work_list_url(event: u32) -> String {
    format!("{BASE}?action=List_def&event={event}")
}

/// A manbow page's text (Shift_JIS).
pub fn decode(page: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode(page).0.into_owned()
}

/// The works of event `event` from its URL list and work list.
pub fn works(event: u32, url_list: &str, work_list: &str) -> Vec<Work> {
    static KIND: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"No\.(\d+) / (\w+)").expect("pattern"));
    static ROW: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<tr\s*>(.*?)</tr>").expect("pattern"));
    static TD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<td>(.*?)</td>").expect("pattern"));
    static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)\s*KB").expect("pattern"));
    let kinds: std::collections::HashMap<u32, String> = KIND
        .captures_iter(work_list)
        .filter_map(|c| Some((c[1].parse().ok()?, c[2].to_string())))
        .collect();
    let mut out = Vec::new();
    for row in ROW.captures_iter(url_list) {
        let mut tds: Vec<&str> = TD
            .captures_iter(row.get(1).map_or("", |m| m.as_str()))
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .collect();
        if tds.len() == 5 {
            // events up to 2005 have no team column
            tds.insert(2, "");
        }
        if tds.len() < 6 {
            continue;
        }
        let Ok(num) = text(tds[0]).parse::<u32>() else {
            continue;
        };
        let links = links_of(tds[5]);
        out.push(Work {
            num,
            title: text(tds[3]),
            artist: text(tds[1]),
            team: text(tds[2]),
            kind: kinds.get(&num).cloned().unwrap_or_default(),
            size_kb: SIZE.captures(&text(tds[4])).and_then(|c| c[1].parse().ok()),
            page: format!("{BASE}?action=More_def&num={num}&event={event}"),
            pick: pick(&links),
            links,
        });
    }
    out.sort_by_key(|w| w.num);
    out
}

/// A work's page on the venue: (event, work number).
pub fn work_page(url: &str) -> Option<(u32, u32)> {
    if !url.contains("manbow.nothing.sh/event/event.cgi") {
        return None;
    }
    let q = url.split_once('?')?.1;
    let arg = |k: &str| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='))
            .and_then(|v| v.parse().ok())
    };
    q.split('&')
        .any(|kv| kv == "action=More_def")
        .then(|| Some((arg("event")?, arg("num")?)))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn picks() {
        // ogg over wav, mirrors after
        let p = pick(&l(&[
            ("wav", "https://a/w.zip"),
            ("ogg", "https://a/o.zip"),
            ("ogg mirror", "https://b/o.zip"),
        ]));
        assert_eq!(p.main, ["https://a/o.zip", "https://b/o.zip"]);
        assert!(p.extra.is_empty());
        // an added chart is an add-on
        let p = pick(&l(&[
            ("本体", "https://a/s.zip"),
            ("追加譜面", "https://a/x.zip"),
        ]));
        assert_eq!(p.main, ["https://a/s.zip"]);
        assert_eq!(p.extra, [vec!["https://a/x.zip".to_string()]]);
        // HQ beats LQ whatever the audio
        let p = pick(&l(&[
            ("LQ ogg", "https://a/l.zip"),
            ("HQ wav", "https://a/h.zip"),
        ]));
        assert_eq!(p.main[0], "https://a/h.zip");
        assert_eq!(pick(&[]).review, "no link");
    }

    #[test]
    fn cells() {
        let cell = "[ogg]<br>Google Drive: <a href=\"https://drive.google.com/x\">https://drive.google.com/x</a><br>Dropbox: <a href=\"https://dropbox.com/y?a=1&amp;b=2\">link</a>";
        assert_eq!(
            links_of(cell),
            l(&[
                ("[ogg] Google Drive", "https://drive.google.com/x"),
                ("[ogg] Dropbox: link", "https://dropbox.com/y?a=1&b=2"),
            ])
        );
        assert_eq!(unescape("a&amp;b&#12354;&#x3042;&zz;"), "a&bああ&zz;");
        assert_eq!(unescape("T&eacute; &apos;x&apos;"), "Té 'x'");
        assert_eq!(unescape("a&ltbr&gt c&amp"), "a<br> c&");
        assert_eq!(file_part("https://h/d/%255Bx%255D.zip?dl=1"), "[x].zip");
        assert_eq!(
            work_page("https://manbow.nothing.sh/event/event.cgi?action=More_def&num=33&event=133"),
            Some((133, 33))
        );
        assert_eq!(
            work_page("https://manbow.nothing.sh/event/event.cgi?action=List_def&event=1"),
            None
        );
    }
}
