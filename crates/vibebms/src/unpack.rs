//! Unpacking an archive held in memory (.zip, .7z, .rar), for the web build,
//! whose downloads never touch a disk: each member is handed over as a
//! relative path (made safe) and its bytes.

use std::io::{Cursor, Read};
use std::path::PathBuf;

/// What an archive may unpack to (it can claim, or expand to, anything).
const BUDGET: u64 = 2 << 30;

/// `name` from an archive as a relative path that cannot leave its folder.
pub fn safe_rel(name: &str) -> Option<PathBuf> {
    let rel: PathBuf = name
        .split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .filter(|c| !c.contains(':'))
        .collect();
    (!rel.as_os_str().is_empty()).then_some(rel)
}

/// Unpack `bytes` (its format from its first bytes), giving `put` each file.
/// Returns the number of files.
pub fn unpack_bytes(
    bytes: Vec<u8>,
    mut put: impl FnMut(PathBuf, Vec<u8>),
) -> Result<usize, String> {
    let mut budget = BUDGET;
    let mut take = |rel: PathBuf, r: &mut dyn Read, budget: &mut u64| -> Result<(), String> {
        let mut buf = Vec::new();
        r.take(*budget + 1)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        if buf.len() as u64 > *budget {
            return Err(format!("larger than {} MB unpacked", BUDGET >> 20));
        }
        *budget -= buf.len() as u64;
        put(rel, buf);
        Ok(())
    };
    let mut n = 0;
    match crate::hosts::sniff_bytes(&bytes) {
        Some("zip") => {
            let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
            for i in 0..z.len() {
                let mut e = z.by_index(i).map_err(|e| e.to_string())?;
                if e.is_dir() {
                    continue;
                }
                // names in Shift_JIS / EUC-KR as BMS packs often have
                let name = crate::platform::vfs::decode_name(e.name_raw());
                let Some(rel) = safe_rel(&name) else { continue };
                take(rel, &mut e, &mut budget)?;
                n += 1;
            }
        }
        Some("7z") => {
            let mut a = sevenz_rust2::ArchiveReader::new(Cursor::new(bytes), "".into())
                .map_err(|e| e.to_string())?;
            let mut err = None;
            a.for_each_entries(|entry, r| {
                if entry.is_directory() {
                    return Ok(true);
                }
                let Some(rel) = safe_rel(entry.name()) else {
                    return Ok(true);
                };
                match take(rel, r, &mut budget) {
                    Ok(()) => {
                        n += 1;
                        Ok(true)
                    }
                    Err(e) => {
                        err = Some(e);
                        Ok(false)
                    }
                }
            })
            .map_err(|e| e.to_string())?;
            if let Some(e) = err {
                return Err(e);
            }
        }
        Some("rar") => {
            let mut a =
                unrar_rs::RarArchive::open(Cursor::new(bytes)).map_err(|e| e.to_string())?;
            for i in 0..a.len() {
                let Some(info) = a.member_info(i) else {
                    continue;
                };
                if info.is_directory {
                    continue;
                }
                let name = info.raw_name.clone();
                let Some(rel) = safe_rel(&name) else { continue };
                let mut buf = Vec::new();
                a.by_index(i)
                    .and_then(|e| e.copy_to(&mut buf))
                    .map_err(|e| format!("{name}: {e}"))?;
                take(rel, &mut buf.as_slice(), &mut budget)?;
                n += 1;
            }
        }
        _ => return Err("not an archive".into()),
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `VIBEBMS_UNPACK=FILE cargo test -p vibebms unpack_file -- --ignored`
    #[test]
    #[ignore = "needs an archive"]
    fn unpack_file() {
        let f = std::env::var("VIBEBMS_UNPACK").expect("VIBEBMS_UNPACK");
        let mut size = 0;
        let n = unpack_bytes(std::fs::read(f).unwrap(), |_, b| size += b.len()).unwrap();
        println!("UNPACKED {n} files, {size} bytes");
    }

    #[test]
    fn zip_in_memory() {
        let mut out = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut out);
            let o = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            z.start_file("song/a.bms", o).unwrap();
            std::io::Write::write_all(&mut z, b"#TITLE A").unwrap();
            z.start_file("../evil.txt", o).unwrap();
            std::io::Write::write_all(&mut z, b"x").unwrap();
            z.finish().unwrap();
        }
        let mut got = Vec::new();
        let n = unpack_bytes(out.into_inner(), |p, b| got.push((p, b))).unwrap();
        assert_eq!(n, 2);
        assert_eq!(got[0].0, PathBuf::from("song/a.bms"));
        assert_eq!(got[0].1, b"#TITLE A");
        // no way out of the folder
        assert_eq!(got[1].0, PathBuf::from("evil.txt"));
        assert!(unpack_bytes(b"<!DOCTYPE html>".to_vec(), |_, _| {}).is_err());
    }
}
