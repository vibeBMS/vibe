//! Downloading an archive from where BMS authors link to it, without a
//! browser: Google
//! Drive files and folders, Dropbox files and folders, MediaFire, MEGA, and
//! any server that answers with the archive itself. What needs a person on
//! a page (OneDrive, uploaders, a quota page) is [`Fetch::Page`].
//!
//! [`Resolver`] knows the hosts: from a link it asks for the requests to
//! make, one [`Step`] at a time. The app makes them itself ([`fetch`],
//! streaming the archive to disk); a web page makes them through a CORS
//! proxy (no host here sends CORS headers) and keeps the archive in memory.

#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// Why a link gave no archive.
#[derive(Debug)]
pub enum Fetch {
    /// The link is a page to use in a browser.
    Page,
    /// The download failed.
    Failed(String),
}

impl From<String> for Fetch {
    fn from(e: String) -> Fetch {
        Fetch::Failed(e)
    }
}

/// The next request a download needs.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Read this page and [`Resolver::feed`] it back.
    Get(String),
    /// POST this JSON (the MEGA API) and feed back the answer.
    Post(String, Vec<u8>),
    /// The archive: download it, decrypting with `mega_key` (a MEGA file).
    /// What arrives must be an archive; anything else means the link is a
    /// page ([`Fetch::Page`]).
    File {
        url: String,
        mega_key: Option<Vec<u8>>,
    },
    /// No archive from this link.
    Fail(Fetch),
}

impl PartialEq for Fetch {
    fn eq(&self, o: &Fetch) -> bool {
        matches!(
            (self, o),
            (Fetch::Page, Fetch::Page) | (Fetch::Failed(_), Fetch::Failed(_))
        )
    }
}

/// The way from an author's link to its archive.
pub struct Resolver {
    link: String,
    /// The song looked for, in a folder of many songs' archives.
    want: Option<Want>,
    wait: Wait,
    /// Pages followed to another link (an event venue's work page).
    hops: u8,
    /// The file's name, where a folder listing or MEGA tells it.
    name: Option<String>,
}

/// What the last [`Step::Get`] / [`Step::Post`] was for.
enum Wait {
    Nothing,
    /// A manbow event venue's work page: its download links.
    Venue,
    /// A BMS SEARCH venue work's manifest (JSON): its download links.
    Manifest,
    DriveFolder,
    MediaFire,
    Mega(Vec<u8>),
    /// A MEGA folder's nodes: (handle, key, the node the link names).
    MegaFolder(String, Vec<u8>, Option<String>),
}

/// A song looked for by its title and artist, as a difficulty table names
/// it: a link may lead to a folder of many songs' archives (a MEGA pack of
/// a table's songs, `[artist] title.zip` each).
#[derive(Clone, Debug, Default)]
pub struct Want {
    pub title: String,
    pub artist: String,
}

/// For a browser showing a [`Fetch::Page`]: on a OneDrive share (a file,
/// or a folder, which comes as a zip) it presses the page's Download button
/// once, as a person would. OneDrive has no download URL to fetch without a
/// browser any more (its API wants an app's token; its pages are built by
/// script). Run it on every page load (wry: `with_initialization_script`;
/// Android: `evaluateJavascript` when the page has finished).
pub const PRESS_DOWNLOAD_JS: &str = r#"(function () {
  if (window.top !== window || window.__bmseekerPressed) return;
  if (!/(^|\.)(onedrive\.live\.com|1drv\.ms|sharepoint\.com)$/i.test(location.hostname)) return;
  var name = /^\s*(download|ダウンロード|다운로드|下载|下載)\s*$/i;
  var tries = 0;
  var timer = setInterval(function () {
    if (++tries > 120) return clearInterval(timer);
    var all = document.querySelectorAll('button, [role="button"], [role="menuitem"]');
    for (var i = 0; i < all.length; i++) {
      var b = all[i];
      var label = b.getAttribute("aria-label") || b.textContent || "";
      if (name.test(label) && !b.disabled && b.offsetParent !== null) {
        window.__bmseekerPressed = true;
        clearInterval(timer);
        b.click();
        return;
      }
    }
  }, 500);
})();"#;

/// Hosts whose links always need a browser.
const PAGES: &[&str] = &[
    "1drv.ms",
    "onedrive",
    "sharepoint.com",
    "baidu",
    "lanzou",
    "lanzn",
    "gigafile",
    "box.com",
];

impl Resolver {
    /// Start from `link`: the first request.
    pub fn new(link: &str) -> (Resolver, Step) {
        Resolver::find(link, None)
    }

    /// Start from `link`, for the song `want` (a folder link: its archive).
    pub fn find(link: &str, want: Option<Want>) -> (Resolver, Step) {
        let mut r = Resolver {
            link: link.trim().to_string(),
            want,
            wait: Wait::Nothing,
            hops: 0,
            name: None,
        };
        let step = r.first();
        (r, step)
    }

    fn first(&mut self) -> Step {
        let u = self.link.clone();
        let lower = u.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Step::Fail(Fetch::Page);
        }
        let host = lower.split('/').nth(2).unwrap_or("").to_string();
        let file = |url: String| Step::File {
            url,
            mega_key: None,
        };
        if host.ends_with("drive.google.com") || host == "docs.google.com" {
            let Some(id) = drive_id(&u) else {
                return Step::Fail(Fetch::Page);
            };
            if lower.contains("/folders/") {
                self.wait = Wait::DriveFolder;
                return Step::Get(format!(
                    "https://drive.google.com/embeddedfolderview?id={id}"
                ));
            }
            return file(drive_file_url(&id));
        }
        if host.ends_with("dropbox.com") {
            return file(dropbox_url(&u));
        }
        // a difficulty table's link to the work's page on an event venue
        if host == "manbow.nothing.sh" && lower.contains("action=more_def") && self.hops < 2 {
            self.wait = Wait::Venue;
            return Step::Get(u);
        }
        // a work on a BMS SEARCH event venue: its manifest lists the links
        if host == "venue.bmssearch.net" && self.hops < 2 {
            let path = u
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .trim_end_matches('/');
            // https://venue.bmssearch.net/<event>/<work>
            if path.split('/').filter(|p| !p.is_empty()).count() >= 4 {
                self.wait = Wait::Manifest;
                return Step::Get(format!("{path}/manifest"));
            }
        }
        if host.ends_with("mediafire.com") {
            if lower.contains("/folder/") {
                return Step::Fail(Fetch::Page);
            }
            self.wait = Wait::MediaFire;
            return Step::Get(u);
        }
        if host == "mega.nz" || host == "mega.co.nz" || host.ends_with(".mega.nz") {
            if let Some((handle, key, node)) = mega_folder(&u) {
                let step = Step::Post(
                    format!("{MEGA_API}&n={handle}"),
                    br#"[{"a":"f","c":1,"r":1,"ca":1}]"#.to_vec(),
                );
                self.wait = Wait::MegaFolder(handle, key, node);
                return step;
            }
            let Some((handle, key)) = mega_link(&u) else {
                return Step::Fail(Fetch::Page);
            };
            self.wait = Wait::Mega(key);
            return Step::Post(MEGA_API.into(), mega_request(&handle));
        }
        if PAGES.iter().any(|p| host.contains(p)) {
            return Step::Fail(Fetch::Page);
        }
        file(u)
    }

    /// The answer to the last [`Step::Get`] / [`Step::Post`]: the next step.
    pub fn feed(&mut self, answer: Result<Vec<u8>, String>) -> Step {
        let body = match answer {
            Ok(b) => b,
            Err(e) => return Step::Fail(Fetch::Failed(e)),
        };
        match std::mem::replace(&mut self.wait, Wait::Nothing) {
            Wait::Nothing => Step::Fail(Fetch::Failed("nothing asked".into())),
            // on to the work's own download link
            Wait::Venue => {
                let page = vibebms_core::encoding::decode_text(&body);
                match pick_link(&venue_links(&page)) {
                    Some(next) => {
                        self.link = next;
                        self.hops += 1;
                        self.first()
                    }
                    None => Step::Fail(Fetch::Page),
                }
            }
            Wait::Manifest => {
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
                let s = |r: &serde_json::Value, k: &str| {
                    r.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
                };
                let links: Vec<(String, String)> = v
                    .get("resources")
                    .and_then(|r| r.as_array())
                    .into_iter()
                    .flatten()
                    .filter(|r| s(r, "type") != "additional")
                    .map(|r| (s(r, "name"), s(r, "url")))
                    .filter(|(_, u)| u.starts_with("http"))
                    .collect();
                match pick_link(&links) {
                    Some(next) => {
                        self.link = next;
                        self.hops += 1;
                        self.first()
                    }
                    None => Step::Fail(Fetch::Page),
                }
            }
            Wait::DriveFolder => {
                let page = String::from_utf8_lossy(&body);
                let files = folder_entries(&page);
                match pick_file(
                    files
                        .iter()
                        .filter(|(name, _)| archive_name(name))
                        .map(|(n, i)| (n.as_str(), (n, i))),
                ) {
                    Some((name, id)) => {
                        self.name = Some(name.clone());
                        Step::File {
                            url: drive_file_url(id),
                            mega_key: None,
                        }
                    }
                    None => Step::Fail(Fetch::Page),
                }
            }
            // the download button's link
            Wait::MediaFire => {
                let page = String::from_utf8_lossy(&body);
                match page
                    .find("href=\"https://download")
                    .and_then(|at| between(&page[at..], "href=\"", "\""))
                {
                    Some(link) => Step::File {
                        url: unescape(link),
                        mega_key: None,
                    },
                    None => Step::Fail(Fetch::Page),
                }
            }
            Wait::Mega(key) => match mega_answer(&body) {
                Ok(url) => {
                    if self.name.is_none() {
                        self.name = mega_file_name(&body, &key);
                    }
                    Step::File {
                        url,
                        mega_key: Some(key),
                    }
                }
                Err(e) => Step::Fail(Fetch::Failed(e)),
            },
            // the folder's archive, then its download URL
            Wait::MegaFolder(handle, key, node) => {
                let files = match mega_nodes(&body, &key) {
                    Ok(f) => f,
                    Err(e) => return Step::Fail(Fetch::Failed(e)),
                };
                let pick = pick_mega(&files, node.as_deref(), self.want.as_ref());
                let Some(f) = pick else {
                    return Step::Fail(Fetch::Page);
                };
                self.wait = Wait::Mega(f.key.clone());
                self.name = Some(f.name.clone());
                Step::Post(
                    format!("{MEGA_API}&n={handle}"),
                    format!(r#"[{{"a":"g","g":1,"ssl":1,"n":"{}"}}]"#, f.handle).into_bytes(),
                )
            }
        }
    }
}

/// (label, URL) of the download links on a manbow work page (its
/// "Download" box): a link's label is the text before it.
fn venue_links(page: &str) -> Vec<(String, String)> {
    let Some(at) = page.find("<h3>Download</h3>") else {
        return Vec::new();
    };
    let block = &page[at + "<h3>Download</h3>".len()..];
    let block = &block[..block.find("</blockquote>").unwrap_or(block.len())];
    let mut out = Vec::new();
    let mut label = String::new();
    let mut rest = block;
    while let Some(i) = rest.find("href=\"") {
        // the last line of text before the link (lines end at <br>)
        let before = &rest[..i];
        let lower = before.to_ascii_lowercase();
        let mut cuts: Vec<usize> = lower.match_indices("<br").map(|(k, _)| k).collect();
        cuts.insert(0, 0);
        cuts.push(before.len());
        let text = cuts
            .windows(2)
            .rev()
            .map(|w| {
                strip_tags(&before[w[0]..w[1]])
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .find(|t| !t.is_empty())
            .unwrap_or_default();
        if !text.is_empty() {
            label = text;
        }
        let Some(url) = between(&rest[i..], "href=\"", "\"") else {
            break;
        };
        let url = unescape(url);
        if url.starts_with("http") {
            out.push((label.clone(), url));
        }
        rest = &rest[i + 6..];
        rest = &rest[rest
            .find("</a>")
            .or_else(|| rest.find("</A>"))
            .map_or(0, |k| k + 4)..];
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    unescape(&out)
}

/// The main archive among a work's links (as the mirror's
/// `scrape_manbow.py` picks): high quality over low, ogg over wav, full
/// packages over BGA-only archives and patches, the first of equals.
fn pick_link(links: &[(String, String)]) -> Option<String> {
    let score = |label: &str, url: &str| {
        let t = format!("{label} {url}").to_lowercase();
        let has = |ws: &[&str]| ws.iter().any(|w| t.contains(w));
        let mut s = 0;
        if has(&["hq", "高画質", "高品質", "high quality"]) {
            s += 6;
        }
        if has(&["lq", "低画質", "低品質", "low quality"]) {
            s -= 6;
        }
        if has(&["ogg"]) {
            s += 4;
        }
        if has(&["wav", "flac"]) {
            s -= 4;
        }
        if has(&[
            "bga only",
            "only bga",
            "bga単体",
            "bgaのみ",
            "差分",
            "patch",
            "追加",
        ]) {
            s -= 8;
        }
        if has(&["mirror", "ミラー", "予備", "backup"]) {
            s -= 1;
        }
        if has(&["/folders/", "/scl/fo/", "/folder/"]) {
            s -= 3;
        }
        s
    };
    let mut best: Option<(i32, &String)> = None;
    for (label, url) in links {
        let s = score(label, url);
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, url));
        }
    }
    best.map(|(_, u)| u.to_string())
}

/// The file or folder id of a Google Drive link (`/file/d/ID`,
/// `/folders/ID`, `?id=ID`).
pub fn drive_id(u: &str) -> Option<String> {
    let rest = ["/d/", "/folders/", "id="]
        .iter()
        .find_map(|k| u.find(k).map(|i| &u[i + k.len()..]))?;
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!id.is_empty()).then_some(id)
}

fn drive_file_url(id: &str) -> String {
    format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t")
}

/// (name, file id) of the files in an `embeddedfolderview` page.
fn folder_entries(page: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in page.split("class=\"flip-entry\"").skip(1) {
        let Some(id) = between(part, "/file/d/", "/") else {
            continue; // a sub-folder
        };
        if let Some(name) = between(part, "flip-entry-title\">", "<") {
            out.push((unescape(name), id.to_string()));
        }
    }
    out
}

fn archive_name(n: &str) -> bool {
    let n = n.to_ascii_lowercase();
    [".zip", ".rar", ".7z"].iter().any(|e| n.ends_with(e))
}

/// The best of (name, id) in a Drive folder (as `fetch.py` `pick_file`):
/// ogg over wav, full over BGA-only and patches, the first of equals.
fn pick_file<'a, T>(files: impl Iterator<Item = (&'a str, T)>) -> Option<T> {
    let score = |n: &str| {
        let n = n.to_lowercase();
        let has = |ws: &[&str]| ws.iter().any(|w| n.contains(w));
        let mut s = 0;
        if has(&["ogg"]) {
            s += 4;
        }
        if has(&["wav", "flac"]) {
            s -= 4;
        }
        if has(&["lq", "low", "lr2", "wmv", "mpg", "lite", "compact"]) {
            s -= 2;
        }
        if has(&[
            "bga only",
            "bga_only",
            "only bga",
            "bga単体",
            "patch",
            "差分",
            "追加",
        ]) {
            s -= 8;
        }
        s
    };
    let mut best: Option<(i32, T)> = None;
    for (name, id) in files {
        let s = score(name);
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Dropbox files and folders download with `dl=1` (a folder as a zip).
fn dropbox_url(u: &str) -> String {
    let base = u.replace("dl=0", "dl=1");
    if base.contains("dl=1") {
        base
    } else if base.contains('?') {
        format!("{base}&dl=1")
    } else {
        format!("{base}?dl=1")
    }
}

/// MEGA's AES-128-CTR with the 32-byte key of a file link.
pub struct MegaCipher {
    aes: aes::Aes128,
    ctr: u128,
}

impl MegaCipher {
    pub fn new(k: &[u8]) -> MegaCipher {
        use aes::cipher::KeyInit;
        let mut key = [0u8; 16];
        for i in 0..16 {
            key[i] = k[i] ^ k[i + 16];
        }
        // the counter starts at the key's nonce (bytes 16..24), then 0
        let mut nonce = [0u8; 16];
        nonce[..8].copy_from_slice(&k[16..24]);
        MegaCipher {
            aes: aes::Aes128::new(&key.into()),
            ctr: u128::from_be_bytes(nonce),
        }
    }

    /// Decrypt the next part of the file (whole 16-byte blocks but at the
    /// end).
    pub fn apply(&mut self, data: &mut [u8]) {
        use aes::cipher::BlockCipherEncrypt;
        for chunk in data.chunks_mut(16) {
            let mut block = self.ctr.to_be_bytes().into();
            self.aes.encrypt_block(&mut block);
            for (b, s) in chunk.iter_mut().zip(block.iter()) {
                *b ^= s;
            }
            self.ctr = self.ctr.wrapping_add(1);
        }
    }
}

/// A MEGA file link (`mega.nz/file/H#K`, `mega.nz/#!H!K`): (handle,
/// 32-byte key).
pub fn mega_link(u: &str) -> Option<(String, Vec<u8>)> {
    if u.contains("/folder/") || u.contains("#F!") {
        return None;
    }
    let (h, k) = if let Some(i) = u.find("/file/") {
        u[i + 6..].split_once('#')?
    } else {
        u[u.find("#!")? + 2..].split_once('!')?
    };
    let key = b64url(k).filter(|k| k.len() == 32)?;
    Some((h.to_string(), key))
}

/// A MEGA folder link (`mega.nz/folder/H#K`, with `/file/N` or
/// `/folder/N` for a node in it, also written `!N` or `?N`;
/// `mega.nz/#F!H!K!N`): (handle, 16-byte key, the node).
fn mega_folder(u: &str) -> Option<(String, Vec<u8>, Option<String>)> {
    let (h, rest) = if let Some(i) = u.find("/folder/") {
        u[i + 8..].split_once('#')?
    } else {
        u[u.find("#F!")? + 3..].split_once('!')?
    };
    let mut parts = rest.split(['/', '!', '?']);
    let key = b64url(parts.next()?).filter(|k| k.len() == 16)?;
    let node = parts
        .rfind(|p| !p.is_empty() && *p != "file" && *p != "folder")
        .map(str::to_string);
    Some((h.to_string(), key, node))
}

/// A file in a MEGA folder.
#[derive(Clone)]
struct MegaFile {
    handle: String,
    /// The folders it is in, innermost first.
    under: Vec<String>,
    name: String,
    /// Its 32-byte key (as a file link's).
    key: Vec<u8>,
}

/// The files of a MEGA folder (the API's `"a":"f"` answer), their keys
/// and names decrypted with the folder's `key`.
fn mega_nodes(body: &[u8], key: &[u8]) -> Result<Vec<MegaFile>, String> {
    use aes::cipher::{BlockCipherDecrypt, KeyInit};
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("MEGA: {e}"))?;
    let Some(nodes) = v.get(0).and_then(|f| f.get("f")).and_then(|f| f.as_array()) else {
        return Err(format!(
            "MEGA refused ({})",
            v.to_string().chars().take(80).collect::<String>()
        ));
    };
    let ecb = |k: &[u8], data: &mut [u8]| {
        let Ok(aes) = aes::Aes128::new_from_slice(k) else {
            return;
        };
        for b in data.chunks_exact_mut(16) {
            let mut block = <[u8; 16]>::try_from(&*b).expect("16").into();
            aes.decrypt_block(&mut block);
            b.copy_from_slice(&block);
        }
    };
    let field = |n: &serde_json::Value, f: &str| {
        n.get(f).and_then(|x| x.as_str()).unwrap_or("").to_string()
    };
    let parent: std::collections::HashMap<String, String> = nodes
        .iter()
        .map(|n| (field(n, "h"), field(n, "p")))
        .collect();
    let mut out = Vec::new();
    for n in nodes {
        let s = |f: &str| n.get(f).and_then(|x| x.as_str()).unwrap_or("");
        if n.get("t").and_then(|t| t.as_u64()) != Some(0) {
            continue; // a folder
        }
        let Some(attr) = b64url(s("a")) else { continue };
        // "owner:key" for each who shares it; the folder's is the one that
        // opens the attributes
        let found = s("k").split('/').find_map(|part| {
            let mut k = b64url(part.split_once(':')?.1).filter(|k| k.len() == 32)?;
            ecb(key, &mut k);
            let name = mega_attr_name(&attr, &k)?;
            Some((k, name))
        });
        if let Some((key, name)) = found {
            let mut under = Vec::new();
            let mut at = s("p");
            while let Some(p) = parent.get(at).filter(|_| under.len() < 64) {
                under.push(at.to_string());
                at = p;
            }
            out.push(MegaFile {
                handle: s("h").to_string(),
                under,
                name,
                key,
            });
        }
    }
    Ok(out)
}

/// The file a MEGA folder link names, or the song `want` in the folder it
/// names (or in the whole folder).
fn pick_mega<'a>(
    files: &'a [MegaFile],
    node: Option<&str>,
    want: Option<&Want>,
) -> Option<&'a MegaFile> {
    if let Some(n) = node {
        if let Some(f) = files.iter().find(|f| f.handle == n) {
            return Some(f);
        }
        let inside: Vec<MegaFile> = files
            .iter()
            .filter(|f| f.under.iter().any(|u| u == n))
            .cloned()
            .collect();
        if let Some(p) = pick_in_pack(&inside, want) {
            return files.iter().find(|f| f.handle == p.handle);
        }
    }
    pick_in_pack(files, want)
}

/// Full-width ASCII as ASCII: a pack's file names hold "＊", "：", "＂"
/// for what a file name may not.
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        })
        .collect()
}

/// A pack file's artist (`by`) is the table's `artist`: one holds the other,
/// without the chart maker's part ("artist / obj: someone") and spaces.
pub(crate) fn pack_artist(by: &str, artist: &str) -> bool {
    let squash = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    let lower = artist.to_lowercase();
    let cut = ["/", "obj", "差分", "譜面"]
        .iter()
        .filter_map(|m| lower.find(m))
        .min()
        .unwrap_or(lower.len());
    let (a, b) = (squash(by), squash(&lower[..cut]));
    a.chars().count() >= 2 && b.chars().count() >= 2 && (a.contains(&b) || b.contains(&a))
}

/// The archive in a folder for `want`: its name is `[artist] title.zip`
/// (or just the title). Without a match, a folder of one song's archives
/// gives its best; one of many songs gives none.
fn pick_in_pack<'a>(files: &'a [MegaFile], want: Option<&Want>) -> Option<&'a MegaFile> {
    let archives: Vec<&MegaFile> = files.iter().filter(|f| archive_name(&f.name)).collect();
    let best = |c: &[&'a MegaFile]| {
        let i = pick_file(c.iter().enumerate().map(|(i, f)| (f.name.as_str(), i)))?;
        Some(c[i])
    };
    if let Some(w) = want {
        let (title, artist) = (fold(&w.title), fold(&w.artist));
        // (the title exactly, the artist, the file)
        let mut found: Vec<(bool, bool, &MegaFile)> = Vec::new();
        for f in &archives {
            let name = fold(&f.name);
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
            let (by, named) = match stem.strip_prefix('[').and_then(|s| s.split_once(']')) {
                Some((a, t)) => (a, t.trim()),
                None => ("", stem),
            };
            let exact = crate::placement::same_name(named, &title);
            if exact || crate::placement::same_title(named, &title) {
                found.push((exact, pack_artist(by, &artist), f));
            }
        }
        let with = |exact: bool, by: bool| -> Vec<&'a MegaFile> {
            found
                .iter()
                .filter(|(e, a, _)| (*e || !exact) && (*a || !by))
                .map(|(_, _, f)| *f)
                .collect()
        };
        // by its artist; else a title no other song in the pack has
        if let Some(f) = best(&with(true, true)).or_else(|| best(&with(false, true))) {
            return Some(f);
        }
        let only = |c: Vec<&'a MegaFile>| (c.len() == 1).then(|| c[0]);
        let long = title.chars().count() >= 6;
        if let Some(f) =
            only(with(true, false)).or_else(|| only(with(false, false)).filter(|_| long))
        {
            return Some(f);
        }
    }
    // one song's versions (ogg / wav, BGA), but not a pack's other songs
    // (`[artist] title`) for the one it lacks
    let versions: Vec<&MegaFile> = archives
        .into_iter()
        .filter(|f| want.is_none() || !f.name.starts_with('['))
        .collect();
    (versions.len() <= 6).then(|| best(&versions)).flatten()
}

/// The MEGA API.
pub const MEGA_API: &str = "https://g.api.mega.co.nz/cs?id=0";

/// The API request for a file's download URL.
pub fn mega_request(handle: &str) -> Vec<u8> {
    // ssl: an https URL (a page served over https may not read http)
    format!(r#"[{{"a":"g","g":1,"ssl":1,"p":"{handle}"}}]"#).into_bytes()
}

/// The download URL in the MEGA API's answer.
pub fn mega_answer(body: &[u8]) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("MEGA: {e}"))?;
    v.get(0)
        .and_then(|f| f.get("g"))
        .and_then(|g| g.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("MEGA refused ({v})"))
}

/// The name in a MEGA node's attributes (`attr`, encrypted with the
/// node's 32-byte key `k`).
fn mega_attr_name(attr: &[u8], k: &[u8]) -> Option<String> {
    use aes::cipher::{BlockCipherDecrypt, KeyInit};
    let ak: Vec<u8> = (0..16).map(|i| k[i] ^ k[i + 16]).collect();
    let aes = aes::Aes128::new_from_slice(&ak).ok()?;
    // CBC with a zero IV
    let mut a = attr.to_vec();
    let mut prev = [0u8; 16];
    for b in a.chunks_exact_mut(16) {
        let c: [u8; 16] = (&*b).try_into().ok()?;
        let mut block = c.into();
        aes.decrypt_block(&mut block);
        for ((x, d), p) in b.iter_mut().zip(block.iter()).zip(prev) {
            *x = d ^ p;
        }
        prev = c;
    }
    let json = a.strip_prefix(b"MEGA")?;
    let end = json.iter().rposition(|&b| b == b'}')? + 1;
    let v: serde_json::Value = serde_json::from_slice(&json[..end]).ok()?;
    Some(v.get("n")?.as_str()?.to_string())
}

/// The file's name in a MEGA `"a":"g"` answer (its `at` attributes).
fn mega_file_name(body: &[u8], key: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let at = b64url(v.get(0)?.get("at")?.as_str()?)?;
    (key.len() == 32).then(|| mega_attr_name(&at, key))?
}

/// The archive format of a file's first bytes.
pub fn sniff_bytes(h: &[u8]) -> Option<&'static str> {
    if h.starts_with(b"PK\x03\x04") || h.starts_with(b"PK\x05\x06") {
        Some("zip")
    } else if h.starts_with(b"Rar!\x1a\x07") {
        Some("rar")
    } else if h.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Some("7z")
    } else {
        None
    }
}

fn b64url(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.trim_end_matches('=').bytes() {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// `s` percent-encoded for a URL's query.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ---- the app: requests made directly, the archive streamed to disk ----

#[cfg(not(target_arch = "wasm32"))]
const UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36";

/// Download the archive `link` leads to into `out`.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch(link: &str, out: &Path) -> Result<(), Fetch> {
    fetch_for(link, None, out)
}

/// [`fetch`] for the song `want` (a folder link: its archive).
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch_for(link: &str, want: Option<Want>, out: &Path) -> Result<(), Fetch> {
    fetch_named(link, want, out, false).map(|_| ())
}

/// [`fetch_for`], and the file's name where it came from (the folder's
/// listing, MEGA, the server, the link). With `any`, a file that is not an
/// archive is kept too if it is not a page: a chart or a movie posted on
/// its own (an event's add-on).
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch_named(link: &str, want: Option<Want>, out: &Path, any: bool) -> Result<String, Fetch> {
    let (mut r, mut step) = Resolver::find(link, want);
    crate::progress::stage(crate::progress::Stage::Finding);
    loop {
        step = match step {
            Step::Get(url) => {
                let mut req = ehttp::Request::get(&url);
                req.headers.insert("User-Agent", UA);
                r.feed(answer(ehttp::fetch_blocking(&req)))
            }
            Step::Post(url, body) => {
                let mut req = ehttp::Request::post(&url, body);
                req.headers.insert("Content-Type", "application/json");
                r.feed(answer(ehttp::fetch_blocking(&req)))
            }
            Step::File { url, mega_key } => {
                let served = match mega_key {
                    None => download(&url, out)?,
                    Some(key) => {
                        let enc = out.with_extension("mega");
                        download(&url, &enc)?;
                        let d = mega_decrypt(&enc, out, &key);
                        let _ = std::fs::remove_file(&enc);
                        d?;
                        None
                    }
                };
                let name = r
                    .name
                    .take()
                    .or(served)
                    .or_else(|| url_file_name(&url))
                    .unwrap_or_else(|| "download".into());
                let page = || {
                    let mut head = [0u8; 512];
                    let n = std::fs::File::open(out)
                        .and_then(|mut f| f.read(&mut head))
                        .unwrap_or(0);
                    n == 0 || head[..n].trim_ascii_start().starts_with(b"<")
                };
                if sniff(out).is_none() && !(any && !page()) {
                    let _ = std::fs::remove_file(out);
                    return Err(Fetch::Page);
                }
                return Ok(name);
            }
            Step::Fail(f) => return Err(f),
        }
    }
}

/// The file name in a `Content-Disposition` header (`filename*=UTF-8''…`
/// or `filename="…"`; UTF-8 sent as Latin-1 read back as UTF-8).
#[cfg(not(target_arch = "wasm32"))]
fn disposition_name(h: &str) -> Option<String> {
    let lower = h.to_ascii_lowercase();
    let value = |key: &str| {
        let at = lower.find(key)? + key.len();
        let v = h[at..].split(';').next()?.trim().trim_matches('"');
        (!v.is_empty()).then(|| v.to_string())
    };
    let name = if let Some(v) = value("filename*=") {
        let v = v
            .split_once("''")
            .map_or(v.as_str(), |(_, n)| n)
            .to_string();
        percent_decode(&v)
    } else {
        let v = value("filename=")?;
        let latin1: Option<Vec<u8>> = v.chars().map(|c| u8::try_from(c as u32).ok()).collect();
        latin1.and_then(|b| String::from_utf8(b).ok()).unwrap_or(v)
    };
    safe_file_name(&name)
}

/// The last part of a URL's path, decoded.
#[cfg(not(target_arch = "wasm32"))]
fn url_file_name(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?.trim_end_matches('/');
    let last = path.rsplit('/').next()?;
    if path.matches('/').count() < 3 {
        return None; // just the host
    }
    safe_file_name(&percent_decode(last))
}

#[cfg(not(target_arch = "wasm32"))]
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

/// A name safe for a file: no path or reserved characters.
#[cfg(not(target_arch = "wasm32"))]
fn safe_file_name(n: &str) -> Option<String> {
    let n: String = n
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let n = n.trim().trim_matches('.').to_string();
    (!n.is_empty()).then_some(n)
}

/// A page or file, in memory (a table, a list): a blocking GET.
#[cfg(not(target_arch = "wasm32"))]
pub fn get(url: &str) -> Result<Vec<u8>, String> {
    answer(ehttp::fetch_blocking(&ehttp::Request::get(url)))
}

#[cfg(not(target_arch = "wasm32"))]
fn answer(r: Result<ehttp::Response, String>) -> Result<Vec<u8>, String> {
    match r {
        Ok(r) if r.ok => Ok(r.bytes),
        Ok(r) => Err(format!("HTTP {}", r.status)),
        Err(e) => Err(e),
    }
}

/// The archive format of a file from its first bytes.
#[cfg(not(target_arch = "wasm32"))]
pub fn sniff(p: &Path) -> Option<&'static str> {
    let mut head = [0u8; 8];
    let n = std::fs::File::open(p).ok()?.read(&mut head).ok()?;
    sniff_bytes(&head[..n])
}

#[cfg(not(target_arch = "wasm32"))]
fn mega_decrypt(from: &Path, to: &Path, k: &[u8]) -> Result<(), String> {
    let mut c = MegaCipher::new(k);
    let mut src = std::io::BufReader::new(std::fs::File::open(from).map_err(|e| e.to_string())?);
    let mut dst = std::io::BufWriter::new(std::fs::File::create(to).map_err(|e| e.to_string())?);
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = read_full(&mut src, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        c.apply(&mut buf[..n]);
        dst.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    dst.flush().map_err(|e| e.to_string())
}

/// Fill `buf` (short only at the end), so chunks stay on 16-byte blocks.
#[cfg(not(target_arch = "wasm32"))]
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// How long a download may go without data before it is given up (ms).
pub const STALL_MS: u64 = 60_000;

#[cfg(not(target_arch = "wasm32"))]
const STALL: std::time::Duration = std::time::Duration::from_millis(STALL_MS);

/// Stream `url` into `out`. No limit on the whole (an archive from a slow
/// host can take an hour), but it ends when no data comes for [`STALL`],
/// or when the thread's progress is cancelled.
#[cfg(not(target_arch = "wasm32"))]
pub fn download_to(url: &str, out: &Path) -> Result<(), String> {
    download(url, out).map(|_| ())
}

/// [`download_to`], and the file name the server gives
/// (`Content-Disposition`), if any.
#[cfg(not(target_arch = "wasm32"))]
fn download(url: &str, out: &Path) -> Result<Option<String>, String> {
    use ehttp::streaming::Part;
    use std::ops::ControlFlow;
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::{Duration, Instant};
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut w = std::io::BufWriter::new(file);
    let progress = crate::progress::current();
    let mut req = ehttp::Request::get(url);
    req.timeout = None;
    // Dropbox answers a browser with its page instead of the file
    if !url.contains("dropbox") {
        req.headers.insert("User-Agent", UA);
    }
    // the request on its own thread; its parts come here. Once this returns
    // the channel is closed and it stops at its next part.
    let (tx, rx) = channel::<Result<Part, String>>();
    ehttp::streaming::fetch(
        req,
        Box::new(move |part| {
            let end = match &part {
                Err(_) => true,
                Ok(Part::Chunk(c)) => c.is_empty(),
                Ok(Part::Response(_)) => false,
            };
            if tx.send(part).is_err() || end {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }),
    );
    let mut last = Instant::now();
    let mut name = None;
    loop {
        if progress.as_ref().is_some_and(|p| p.cancelled()) {
            return Err(crate::progress::CANCELLED.into());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Err(e)) => return Err(e),
            Ok(Ok(Part::Response(r))) if !r.ok => return Err(format!("HTTP {}", r.status)),
            Ok(Ok(Part::Response(r))) => {
                name = r
                    .headers
                    .get("content-disposition")
                    .and_then(disposition_name);
                if let Some(p) = &progress {
                    p.start_file(r.headers.get("content-length").and_then(|l| l.parse().ok()));
                }
                last = Instant::now();
            }
            Ok(Ok(Part::Chunk(c))) if c.is_empty() => {
                return w.flush().map(|_| name).map_err(|e| e.to_string());
            }
            Ok(Ok(Part::Chunk(c))) => {
                if let Some(p) = &progress {
                    p.add(c.len() as u64);
                }
                w.write_all(&c).map_err(|e| e.to_string())?;
                last = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) if last.elapsed() > STALL => {
                return Err(format!("no data for {} s", STALL.as_secs()));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err("the download ended early".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(
            disposition_name("attachment; filename=\"a b.zip\"").as_deref(),
            Some("a b.zip")
        );
        assert_eq!(
            disposition_name("attachment; filename=\"x.zip\"; filename*=UTF-8''%E6%9B%B2.zip")
                .as_deref(),
            Some("曲.zip")
        );
        // UTF-8 bytes sent as Latin-1
        let latin1: String = "曲.rar".bytes().map(|b| b as char).collect();
        assert_eq!(
            disposition_name(&format!("attachment; filename=\"{latin1}\"")).as_deref(),
            Some("曲.rar")
        );
        assert_eq!(
            url_file_name("https://h.example/d/%E6%9B%B2%20x.7z?dl=1").as_deref(),
            Some("曲 x.7z")
        );
        assert_eq!(url_file_name("https://h.example/"), None);
        assert_eq!(safe_file_name("../a/b.zip").as_deref(), Some("_a_b.zip"));
    }

    #[test]
    fn links() {
        assert_eq!(
            drive_id("https://drive.google.com/file/d/1AbC-d_9/view?usp=sharing").as_deref(),
            Some("1AbC-d_9")
        );
        assert_eq!(
            drive_id("https://drive.google.com/drive/folders/1UX?usp=x").as_deref(),
            Some("1UX")
        );
        assert_eq!(
            drive_id("https://drive.google.com/open?id=XYZ").as_deref(),
            Some("XYZ")
        );
        assert_eq!(
            dropbox_url("https://www.dropbox.com/scl/fo/a/b?rlkey=k&dl=0"),
            "https://www.dropbox.com/scl/fo/a/b?rlkey=k&dl=1"
        );
        assert_eq!(b64url("AQID").unwrap(), [1, 2, 3]);
        assert_eq!(b64url("-_8").unwrap(), [0xfb, 0xff]);
        assert_eq!(
            url_encode("https://a.b/c?d=1&e"),
            "https%3A%2F%2Fa.b%2Fc%3Fd%3D1%26e"
        );
    }

    #[test]
    fn steps() {
        let file = |s: Step| match s {
            Step::File { url, .. } => url,
            s => panic!("{s:?}"),
        };
        assert!(file(Resolver::new("https://drive.google.com/file/d/X/view").1).contains("id=X"));
        assert!(file(Resolver::new("http://example.com/a.zip").1).ends_with("a.zip"));
        assert_eq!(
            Resolver::new("https://1drv.ms/u/c/x").1,
            Step::Fail(Fetch::Page)
        );
        assert_eq!(Resolver::new("not a link").1, Step::Fail(Fetch::Page));
        // a Drive folder: its page, then its best archive
        let (mut r, s) = Resolver::new("https://drive.google.com/drive/folders/F");
        assert!(matches!(s, Step::Get(u) if u.ends_with("id=F")));
        let page = r#"<div class="flip-entry" id="entry-B"><a href="https://drive.google.com/file/d/B/view"><div class="flip-entry-title">s_ogg.zip</div></a></div>"#;
        assert!(file(r.feed(Ok(page.into()))).contains("id=B"));
        // MediaFire: the download button
        let (mut r, s) = Resolver::new("https://www.mediafire.com/file/x/a.rar/file");
        assert!(matches!(s, Step::Get(_)));
        let page = r#"<a class="input" href="https://download1.mediafire.com/x/a.rar">"#;
        assert_eq!(
            file(r.feed(Ok(page.into()))),
            "https://download1.mediafire.com/x/a.rar"
        );
        // MEGA: the API, then the file and its key
        let (mut r, s) = Resolver::new(
            "https://mega.nz/file/bzQy3bYS#Og0LBoWkfbKwxUceGvzLHDl2exyI0l1DiQ3XNw79zMw",
        );
        assert!(matches!(s, Step::Post(..)));
        match r.feed(Ok(br#"[{"s":3,"g":"https://gfs.example/dl/x"}]"#.to_vec())) {
            Step::File { url, mega_key } => {
                assert_eq!(url, "https://gfs.example/dl/x");
                assert_eq!(mega_key.map(|k| k.len()), Some(32));
            }
            s => panic!("{s:?}"),
        }
    }

    /// Which archive of a MEGA pack each song gets: `VIBEBMS_PACKS=FILE`
    /// of `title<TAB>artist<TAB>link` lines.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "network"]
    fn mega_packs() {
        let list = std::fs::read_to_string(std::env::var("VIBEBMS_PACKS").unwrap()).unwrap();
        let mut seen = std::collections::HashMap::new();
        for line in list.lines() {
            let [title, artist, link] = line.splitn(3, '\t').collect::<Vec<_>>()[..] else {
                continue;
            };
            let (h, key, node) = mega_folder(link.trim()).unwrap();
            let files = seen.entry(h.clone()).or_insert_with(|| {
                let req = ehttp::Request::post(
                    format!("{MEGA_API}&n={h}"),
                    br#"[{"a":"f","c":1,"r":1,"ca":1}]"#.to_vec(),
                );
                let got = answer(ehttp::fetch_blocking(&req)).and_then(|b| mega_nodes(&b, &key));
                got.unwrap_or_else(|e| {
                    println!("{h}: {e}");
                    Vec::new()
                })
            });
            let want = Want {
                title: title.into(),
                artist: artist.into(),
            };
            let pick = pick_mega(files, node.as_deref(), Some(&want));
            println!("{title} | {artist} -> {:?}", pick.map(|f| &f.name));
        }
    }

    #[test]
    fn mega_folders() {
        let key = "wfv-iI4UuSIaGpP0oFZdtQ";
        let parse = |u: &str| mega_folder(u).map(|(h, k, n)| (h, k.len(), n));
        let at = |n: &str| Some(("FNFDCQaT".to_string(), 16, Some(n.to_string())));
        assert_eq!(
            parse(&format!("https://mega.nz/folder/FNFDCQaT#{key}")),
            Some(("FNFDCQaT".to_string(), 16, None))
        );
        assert_eq!(
            parse(&format!("https://mega.nz/folder/FNFDCQaT#{key}/file/N1")),
            at("N1")
        );
        assert_eq!(
            parse(&format!("https://mega.nz/folder/FNFDCQaT#{key}/folder/S")),
            at("S")
        );
        assert_eq!(
            parse(&format!("https://mega.nz/folder/FNFDCQaT#{key}?N1")),
            at("N1")
        );
        assert_eq!(
            parse(&format!("https://mega.nz/folder/FNFDCQaT#{key}!N1")),
            at("N1")
        );
        assert_eq!(
            parse(&format!("https://mega.nz/#F!FNFDCQaT!{key}!N1")),
            at("N1")
        );
        assert_eq!(parse("https://mega.nz/file/bzQy3bYS#Og0LBoWk"), None);

        // a node as the API sends it: its key under the folder's, its
        // attributes under its own
        use aes::cipher::{BlockCipherEncrypt, KeyInit};
        let enc = |k: &[u8], data: &mut [u8], cbc: bool| {
            let aes = aes::Aes128::new_from_slice(k).unwrap();
            let mut prev = [0u8; 16];
            for b in data.chunks_exact_mut(16) {
                if cbc {
                    b.iter_mut().zip(prev).for_each(|(x, p)| *x ^= p);
                }
                let mut block = <[u8; 16]>::try_from(&*b).unwrap().into();
                aes.encrypt_block(&mut block);
                b.copy_from_slice(&block);
                prev.copy_from_slice(b);
            }
        };
        let b64 = |b: &[u8]| {
            const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut out = String::new();
            for c in b.chunks(3) {
                let n = c.iter().fold(0u32, |n, &x| n << 8 | x as u32) << (8 * (3 - c.len()));
                for i in 0..=c.len() {
                    out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
                }
            }
            out
        };
        let folder = [7u8; 16];
        let node = |h: &str, name: &str, fk: u8| {
            let mut k: Vec<u8> = (0..32).map(|i| i as u8 ^ fk).collect();
            let ak: Vec<u8> = (0..16).map(|i| k[i] ^ k[i + 16]).collect();
            let mut a = format!(r#"MEGA{{"n":"{name}"}}"#).into_bytes();
            a.resize(a.len().div_ceil(16) * 16, 0);
            enc(&ak, &mut a, true);
            enc(&folder, &mut k, false);
            format!(
                r#"{{"h":"{h}","p":"D","t":0,"a":"{}","k":"R:{}"}}"#,
                b64(&a),
                b64(&k)
            )
        };
        let body = format!(
            r#"[{{"f":[{{"h":"R","p":"","t":1}},{{"h":"D","p":"R","t":1}},{},{},{}]}}]"#,
            node("A", "[xi] Beatrice.zip", 1),
            node("B", "[Beatrice de Fermat] Lieselotte.zip", 2),
            node("C", "[Team：SASAKURATION] AVALON.zip", 3),
        );
        let files = mega_nodes(body.as_bytes(), &folder).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].name, "[xi] Beatrice.zip");
        assert_eq!(files[0].under, ["D", "R"]);
        assert_eq!(
            files[0].key,
            (0..32).map(|i| i as u8 ^ 1).collect::<Vec<_>>()
        );

        let pick = |title: &str, artist: &str| {
            let want = Want {
                title: title.into(),
                artist: artist.into(),
            };
            pick_in_pack(&files, Some(&want)).map(|f| f.handle.as_str())
        };
        assert_eq!(pick("Beatrice [ANOTHER]", "xi"), Some("A"));
        assert_eq!(
            pick("AVALON -Genocide-", "Team:SASAKURATION / selene."),
            Some("C")
        );
        // a title no other song has, whoever the table says made it
        assert_eq!(pick("AVALON", "someone"), Some("C"));
        assert_eq!(pick("Elina", "Squar"), None);
        // a pack of a few songs without a match: its best
        assert!(pick_in_pack(&files, None).is_some());
        assert_eq!(
            pick_mega(&files, Some("B"), None).map(|f| f.handle.as_str()),
            Some("B")
        );
    }

    #[test]
    fn bmssearch_venue() {
        let (mut r, s) = Resolver::new("https://venue.bmssearch.net/bmsshuin3/75");
        assert_eq!(
            s,
            Step::Get("https://venue.bmssearch.net/bmsshuin3/75/manifest".into())
        );
        let manifest = r#"{"version":1,"resources":[
            {"url":"https://drive.google.com/file/d/W/view","type":"core","name":"wav版"},
            {"url":"https://drive.google.com/file/d/O/view","type":"core","name":"ogg版"}]}"#;
        match r.feed(Ok(manifest.as_bytes().to_vec())) {
            Step::File { url, .. } => assert!(url.contains("id=O"), "{url}"),
            s => panic!("{s:?}"),
        }
        // the venue's list of works is a page
        assert_eq!(
            Resolver::new("https://venue.bmssearch.net/bmsshuin3").1,
            Step::File {
                url: "https://venue.bmssearch.net/bmsshuin3".into(),
                mega_key: None
            }
        );
    }

    #[test]
    fn venue_pages() {
        let page = r#"<h3>Download</h3></div><blockquote><p style="x">wav<br><a href="https://drive.google.com/file/d/W/view">https://drive.google.com/file/d/W/view</A><br>ogg<br><a href="https://drive.google.com/file/d/O/view">https://drive.google.com/file/d/O/view</A></p><footer>Total</footer></blockquote>"#;
        let links = venue_links(page);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].0, "wav");
        assert_eq!(links[1].0, "ogg");
        assert_eq!(
            pick_link(&links).as_deref(),
            Some("https://drive.google.com/file/d/O/view")
        );
        // the venue page, then the ogg archive on Drive
        let (mut r, s) = Resolver::new(
            "https://manbow.nothing.sh/event/event.cgi?action=More_def&num=94&event=146",
        );
        assert!(matches!(s, Step::Get(_)));
        match r.feed(Ok(page.as_bytes().to_vec())) {
            Step::File { url, .. } => assert!(url.contains("id=O")),
            s => panic!("{s:?}"),
        }
    }

    #[test]
    fn drive_folders() {
        let page = r#"<div class="flip-entry" id="entry-A"><a href="https://drive.google.com/file/d/A/view?usp=drive_web"><div class="flip-entry-title">song.zip</div></a></div>
            <div class="flip-entry" id="entry-S"><a href="https://drive.google.com/drive/folders/S"><div class="flip-entry-title">sub</div></a></div>
            <div class="flip-entry" id="entry-B"><a href="https://drive.google.com/file/d/B/view?usp=drive_web"><div class="flip-entry-title">song_ogg.zip</div></a></div>
            <div class="flip-entry" id="entry-C"><a href="https://drive.google.com/file/d/C/view?usp=drive_web"><div class="flip-entry-title">song_wav.zip</div></a></div>"#;
        let e = folder_entries(page);
        assert_eq!(e.len(), 3);
        let best = pick_file(e.iter().map(|(n, i)| (n.as_str(), i.as_str())));
        assert_eq!(best, Some("B"));
    }

    /// `VIBEBMS_FETCH=URL cargo test -p vibebms fetch_url -- --ignored`
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "network"]
    fn fetch_url() {
        let url = std::env::var("VIBEBMS_FETCH").expect("VIBEBMS_FETCH");
        let out = std::env::temp_dir().join("vibebms-fetch");
        match fetch(&url, &out) {
            Ok(()) => println!(
                "OK {:?} {} bytes",
                sniff(&out),
                std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0)
            ),
            Err(e) => println!("ERR {e:?}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn mega_ctr() {
        // AES-128 of a zero key and zero counter block, xored with zeros
        let dir = std::env::temp_dir().join(format!("vibebms-mega-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::write(&a, [0u8; 20]).unwrap();
        mega_decrypt(&a, &b, &[0u8; 32]).unwrap();
        let out = std::fs::read(&b).unwrap();
        assert_eq!(out.len(), 20);
        assert_eq!(&out[..4], &[0x66, 0xe9, 0x4b, 0xd4]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A server that sends a little and then nothing: cancelling ends it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_stalled_download_cancels() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\nPK");
            std::thread::sleep(std::time::Duration::from_secs(30));
        });
        let p = crate::progress::Progress::new();
        let p2 = p.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            p2.cancel();
        });
        let out = std::env::temp_dir().join(format!("bmseeker-stall-{}", std::process::id()));
        let t = std::time::Instant::now();
        let r = crate::progress::scope(Some(p.clone()), || {
            download_to(&format!("http://{addr}/a.zip"), &out)
        });
        assert_eq!(r, Err(crate::progress::CANCELLED.to_string()));
        assert!(t.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(p.bytes(), (2, Some(1_000_000)));
        let _ = std::fs::remove_file(out);
    }
}
