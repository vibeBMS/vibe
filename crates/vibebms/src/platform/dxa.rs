//! DxLib archives (`.dxa`, versions 1-5, default key): LR2 skins ship
//! folders packed as `name.dxa`, read as if the folder `name/` existed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// DxLib's key when an archive has no password.
const KEY: [u8; 12] = [
    0x55, 0xaa, 0x20, 0x55, 0x55, 0x06, 0x55, 0xaa, 0x55, 0xd5, 0x7c, 0x66,
];

struct Archive {
    /// Decrypted archive bytes.
    data: Vec<u8>,
    /// Upper-case path inside the archive -> (offset, size, packed size).
    files: HashMap<String, (usize, usize, Option<usize>)>,
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
}

impl Archive {
    fn parse(mut data: Vec<u8>) -> Option<Archive> {
        for (i, b) in data.iter_mut().enumerate() {
            *b ^= KEY[i % 12];
        }
        if data.get(0..2)? != b"DX" {
            return None;
        }
        let ver = u16_at(&data, 2)?;
        if !(1..=5).contains(&ver) {
            log::debug!("dxa: version {ver} not supported");
            return None;
        }
        let data_start = u32_at(&data, 8)?;
        let names = u32_at(&data, 12)?;
        let file_tab = names + u32_at(&data, 16)?;
        let dir_tab = names + u32_at(&data, 20)?;
        let head = if ver >= 2 { 44 } else { 40 };
        let name = |o: usize| -> Option<String> {
            let n = u16_at(&data, names + o)? as usize * 4;
            // the upper-case form comes first
            let raw = data.get(names + o + 4..names + o + 4 + n)?;
            let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
            let (s, _, _) = encoding_rs::SHIFT_JIS.decode(&raw[..end]);
            Some(s.into_owned())
        };
        let mut files = HashMap::new();
        // walk directories from the root (directory table offset 0)
        let mut stack = vec![(0usize, String::new())];
        let mut guard = 0;
        while let Some((dir, prefix)) = stack.pop() {
            guard += 1;
            if guard > 100_000 {
                return None;
            }
            let count = u32_at(&data, dir_tab + dir + 8)?;
            let first = u32_at(&data, dir_tab + dir + 12)?;
            for k in 0..count {
                let fh = file_tab + first + k * head;
                let nm = name(u32_at(&data, fh)?)?;
                let attr = u32_at(&data, fh + 4)?;
                let addr = u32_at(&data, fh + 32)?;
                let size = u32_at(&data, fh + 36)?;
                let packed = if ver >= 2 {
                    Some(u32_at(&data, fh + 40)?).filter(|p| *p != 0xffff_ffff)
                } else {
                    None
                };
                let path = format!("{prefix}{nm}");
                if attr & 0x10 != 0 {
                    stack.push((addr, format!("{path}/")));
                } else {
                    files.insert(path, (data_start + addr, size, packed));
                }
            }
        }
        Some(Archive { data, files })
    }

    fn get(&self, rel: &str) -> Option<Vec<u8>> {
        let &(off, size, packed) = self.files.get(&rel.to_uppercase())?;
        match packed {
            None => self.data.get(off..off + size).map(<[u8]>::to_vec),
            Some(p) => decode(self.data.get(off..off + p)?).filter(|d| d.len() == size),
        }
    }
}

/// DxLib's LZ decompression (`DXA_Decode`).
fn decode(src: &[u8]) -> Option<Vec<u8>> {
    let dest = u32_at(src, 0)?;
    let mut left = u32_at(src, 4)?.checked_sub(9)?;
    let key = *src.get(8)?;
    let mut out = Vec::with_capacity(dest);
    let mut sp = 9;
    let byte = |i: usize| src.get(i).copied();
    while left > 0 {
        let b = byte(sp)?;
        if b != key {
            out.push(b);
            sp += 1;
            left -= 1;
            continue;
        }
        let mut code = byte(sp + 1)?;
        if code == key {
            out.push(key);
            sp += 2;
            left = left.checked_sub(2)?;
            continue;
        }
        if code > key {
            code -= 1;
        }
        sp += 2;
        left = left.checked_sub(2)?;
        let mut len = (code >> 3) as usize;
        if code & 4 != 0 {
            len |= (byte(sp)? as usize) << 5;
            sp += 1;
            left = left.checked_sub(1)?;
        }
        len += 4;
        let n = (code & 3) as usize + 1;
        let mut index = 0usize;
        for i in 0..n.min(3) {
            index |= (byte(sp + i)? as usize) << (8 * i);
        }
        sp += n.min(3);
        left = left.checked_sub(n.min(3))?;
        index += 1;
        let start = out.len().checked_sub(index)?;
        for i in 0..len {
            let b = out[start + i];
            out.push(b);
        }
        if out.len() > dest {
            return None;
        }
    }
    Some(out)
}

type Cache = Mutex<HashMap<PathBuf, Option<Arc<Archive>>>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Read `p` from an archive standing in for one of its folders
/// (`a/b.dxa` for `a/b/c.png`).
pub fn read(p: &Path, read_file: &dyn Fn(&Path) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    for dir in p.ancestors().skip(1) {
        let Some(name) = dir.file_name() else {
            continue;
        };
        let arc_path = dir.with_file_name(format!("{}.dxa", name.to_string_lossy()));
        let arc = {
            let mut c = cache().lock().ok()?;
            c.entry(arc_path.clone())
                .or_insert_with(|| read_file(&arc_path).and_then(Archive::parse).map(Arc::new))
                .clone()
        };
        if let Some(a) = arc {
            let rel = p
                .strip_prefix(dir)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            return a.get(&rel);
        }
    }
    None
}

/// Forget opened archives (after a skin has loaded its files).
pub fn clear_cache() {
    if let Ok(mut c) = cache().lock() {
        c.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a v3 archive: one stored file in the root and one compressed
    /// file in a sub folder.
    fn archive() -> Vec<u8> {
        let mut names = Vec::new();
        let mut name = |s: &str| {
            let off = names.len();
            let n = s.len().div_ceil(4).max(1);
            names.extend_from_slice(&(n as u16).to_le_bytes());
            names.extend_from_slice(&[0, 0]);
            let mut up = s.to_uppercase().into_bytes();
            up.resize(n * 4, 0);
            let mut orig = s.as_bytes().to_vec();
            orig.resize(n * 4, 0);
            names.extend(up);
            names.extend(orig);
            off
        };
        let root = name("");
        let _ = root;
        let a = name("a.txt");
        let sub = name("sub");
        let b = name("b.txt");
        // data: "hello", then "abababab" packed as literal "ab" + copy
        let mut data = b"hello".to_vec();
        let key = 0xffu8;
        let mut packed = Vec::new();
        packed.extend_from_slice(&8u32.to_le_bytes());
        // copy 6 bytes from 2 back: code = (6 - 4) << 3, 1-byte index 2 - 1
        let body = [b'a', b'b', key, 2u8 << 3, 1u8];
        packed.extend_from_slice(&((9 + body.len()) as u32).to_le_bytes());
        packed.push(key);
        packed.extend_from_slice(&body);
        let b_off = data.len();
        data.extend_from_slice(&packed);
        let fh = |nm: usize, attr: u32, addr: usize, size: usize, press: u32| {
            let mut v = Vec::new();
            v.extend_from_slice(&(nm as u32).to_le_bytes());
            v.extend_from_slice(&attr.to_le_bytes());
            v.extend_from_slice(&[0u8; 24]);
            v.extend_from_slice(&(addr as u32).to_le_bytes());
            v.extend_from_slice(&(size as u32).to_le_bytes());
            v.extend_from_slice(&press.to_le_bytes());
            v
        };
        let mut files = Vec::new();
        files.extend(fh(0, 0x10, 0, 0, 0xffff_ffff)); // root itself
        files.extend(fh(a, 0x20, 0, 5, 0xffff_ffff));
        files.extend(fh(sub, 0x10, 16, 0, 0xffff_ffff));
        files.extend(fh(b, 0x20, b_off, 8, packed.len() as u32));
        let dirs = [[0u32, 0xffff_ffff, 2, 44], [88, 0, 1, 132]]
            .iter()
            .flat_map(|d| d.iter().flat_map(|x| x.to_le_bytes()))
            .collect::<Vec<u8>>();
        let block_start = 24 + data.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"DX");
        out.extend_from_slice(&3u16.to_le_bytes());
        let head_size = names.len() + files.len() + dirs.len();
        for v in [
            head_size,
            24,
            block_start,
            names.len(),
            names.len() + files.len(),
        ] {
            out.extend_from_slice(&(v as u32).to_le_bytes());
        }
        out.extend(data);
        out.extend(names);
        out.extend(files);
        out.extend(dirs);
        for (i, b) in out.iter_mut().enumerate() {
            *b ^= KEY[i % 12];
        }
        out
    }

    #[test]
    fn reads_archive_as_folder() {
        let bytes = archive();
        let rf = |p: &Path| (p == Path::new("/x/pack.dxa")).then(|| bytes.clone());
        assert_eq!(read(Path::new("/x/pack/A.TXT"), &rf).unwrap(), b"hello");
        assert_eq!(
            read(Path::new("/x/pack/sub/b.txt"), &rf).unwrap(),
            b"abababab"
        );
        assert!(read(Path::new("/x/pack/none.txt"), &rf).is_none());
        clear_cache();
    }
}
