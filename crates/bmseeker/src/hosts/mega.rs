//! MEGA: a file link's download URL comes from the API, a folder link's
//! files (and their names) from its listing, decrypted with the link's key;
//! a folder of many songs' archives gives the one for the song looked for.
//! The bytes come encrypted ([`MegaCipher`]).

use super::{archive_name, pick_file, Host, Link, Resolved, Target, Want};
use crate::config::Config;
use crate::net::{Answer, BoxTask, Next, Ready, Request, Task};

pub struct Mega;

impl Host for Mega {
    fn name(&self) -> &'static str {
        "mega"
    }

    fn accepts(&self, link: &Link) -> bool {
        link.host == "mega.nz" || link.host == "mega.co.nz" || link.host.ends_with(".mega.nz")
    }

    fn open(&self, link: &Link, want: Option<&Want>, config: &Config) -> BoxTask<Resolved> {
        let api = config.mega_api.clone();
        if let Some((handle, key, node)) = mega_folder(&link.url) {
            return Box::new(MegaTask {
                api,
                state: State::Folder {
                    handle,
                    key,
                    node,
                    want: want.cloned(),
                },
            });
        }
        match mega_link(&link.url) {
            Some((handle, key)) => Box::new(MegaTask {
                api,
                state: State::File { handle, key },
            }),
            None => Ready::boxed(Resolved::Page),
        }
    }
}

enum State {
    /// A file link: its download URL.
    File {
        handle: String,
        key: Vec<u8>,
    },
    /// A folder link: its files.
    Folder {
        handle: String,
        key: Vec<u8>,
        node: Option<String>,
        want: Option<Want>,
    },
    /// The download URL of the file picked in a folder.
    Picked {
        key: Vec<u8>,
        name: String,
    },
    Done,
}

struct MegaTask {
    api: String,
    state: State,
}

impl Task for MegaTask {
    type Output = Resolved;

    fn start(&mut self) -> Next<Resolved> {
        match &self.state {
            State::File { handle, .. } => Next::Ask(Request::Post {
                url: self.api.clone(),
                body: mega_request(handle),
            }),
            State::Folder { handle, .. } => Next::Ask(Request::Post {
                url: format!("{}&n={handle}", self.api),
                body: br#"[{"a":"f","c":1,"r":1,"ca":1}]"#.to_vec(),
            }),
            _ => Next::Done(Resolved::Failed("MEGA: started twice".into())),
        }
    }

    fn feed(&mut self, answer: Answer) -> Next<Resolved> {
        let body = match answer {
            Ok(b) => b,
            Err(e) => return Next::Done(Resolved::Failed(e)),
        };
        match std::mem::replace(&mut self.state, State::Done) {
            State::File { key, .. } => Next::Done(match mega_answer(&body) {
                Ok(url) => Resolved::File(Target {
                    url,
                    name: mega_file_name(&body, &key),
                    mega_key: Some(key),
                }),
                Err(e) => Resolved::Failed(e),
            }),
            // the folder's archive, then its download URL
            State::Folder {
                handle,
                key,
                node,
                want,
            } => {
                let files = match mega_nodes(&body, &key) {
                    Ok(f) => f,
                    Err(e) => return Next::Done(Resolved::Failed(e)),
                };
                let Some(f) = pick_mega(&files, node.as_deref(), want.as_ref()) else {
                    return Next::Done(Resolved::Page);
                };
                self.state = State::Picked {
                    key: f.key.clone(),
                    name: f.name.clone(),
                };
                Next::Ask(Request::Post {
                    url: format!("{}&n={handle}", self.api),
                    body: format!(r#"[{{"a":"g","g":1,"ssl":1,"n":"{}"}}]"#, f.handle).into_bytes(),
                })
            }
            State::Picked { key, name } => Next::Done(match mega_answer(&body) {
                Ok(url) => Resolved::File(Target {
                    url,
                    name: Some(name),
                    mega_key: Some(key),
                }),
                Err(e) => Resolved::Failed(e),
            }),
            State::Done => Next::Done(Resolved::Failed("MEGA: nothing asked".into())),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::tests::resolve;

    #[test]
    fn file_links() {
        let link = "https://mega.nz/file/bzQy3bYS#Og0LBoWkfbKwxUceGvzLHDl2exyI0l1DiQ3XNw79zMw";
        let t = resolve(
            link,
            &[(
                "https://g.api.mega.co.nz/cs?id=0",
                br#"[{"s":3,"g":"https://gfs.example/dl/x"}]"#,
            )],
        )
        .unwrap();
        assert_eq!(t.url, "https://gfs.example/dl/x");
        assert_eq!(t.mega_key.map(|k| k.len()), Some(32));
        assert_eq!(b64url("AQID").unwrap(), [1, 2, 3]);
        assert_eq!(b64url("-_8").unwrap(), [0xfb, 0xff]);
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
                    format!("{}&n={h}", Config::default().mega_api),
                    br#"[{"a":"f","c":1,"r":1,"ca":1}]"#.to_vec(),
                );
                let got = crate::net::answer(ehttp::fetch_blocking(&req))
                    .and_then(|b| mega_nodes(&b, &key));
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
}
