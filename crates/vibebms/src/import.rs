//! Songs the in-app browser downloaded: archives (.zip, .7z, .rar) are unpacked
//! into the library, a chart on its own goes into its song's folder (see
//! `placement`).

use crate::placement::{self, Located};
use crate::platform::vfs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// What became of a downloaded file.
#[derive(Debug, PartialEq)]
pub enum Imported {
    /// Unpacked into this folder (files written).
    Unpacked(PathBuf, usize),
    /// A chart moved into its song's folder.
    Placed(PathBuf),
    /// A chart whose song is not in the library yet (it waits in the
    /// downloads folder and is placed when the song arrives).
    Waiting(PathBuf),
    /// A chart with several possible songs: the user picks one.
    Ask(PathBuf, Vec<(PathBuf, String)>),
    /// Not a song (kept in the downloads folder).
    Other(PathBuf),
}

/// What a pack may unpack to (a zip can claim, or expand to, anything).
const BUDGET: u64 = 4 << 30;

fn ext(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// `name` from an archive as a relative path that cannot leave its folder.
fn safe_rel(name: &str) -> Option<PathBuf> {
    let rel: PathBuf = name
        .split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .filter(|c| !c.contains(':'))
        .collect();
    (!rel.as_os_str().is_empty()).then_some(rel)
}

fn write_entry(dest: &Path, rel: &Path, r: &mut dyn Read, budget: &mut u64) -> Result<(), String> {
    let path = dest.join(rel);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut f = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let n = std::io::copy(&mut r.take(*budget + 1), &mut f).map_err(|e| e.to_string())?;
    if n > *budget {
        drop(f);
        let _ = std::fs::remove_file(&path);
        return Err(format!("larger than {} MB unpacked", BUDGET >> 20));
    }
    *budget -= n;
    Ok(())
}

fn unzip(file: &Path, dest: &Path) -> Result<usize, String> {
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut budget = BUDGET;
    let mut n = 0;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        // names in Shift_JIS / EUC-KR as BMS packs often have
        let Some(rel) = safe_rel(&vfs::decode_name(entry.name_raw())) else {
            continue;
        };
        write_entry(dest, &rel, &mut entry, &mut budget)?;
        n += 1;
    }
    Ok(n)
}

fn un7z(file: &Path, dest: &Path) -> Result<usize, String> {
    let mut a = sevenz_rust2::ArchiveReader::open(file, sevenz_rust2::Password::new(""))
        .map_err(|e| e.to_string())?;
    let mut budget = BUDGET;
    let mut n = 0;
    let mut err = None;
    a.for_each_entries(|entry, r| {
        if entry.is_directory() {
            return Ok(true);
        }
        let Some(rel) = safe_rel(entry.name()) else {
            return Ok(true);
        };
        match write_entry(dest, &rel, r, &mut budget) {
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
    err.map_or(Ok(n), Err)
}

/// A writer that fails past `left` bytes (an archive member larger than it
/// claims).
struct Capped<W> {
    w: W,
    left: u64,
}

impl<W: std::io::Write> std::io::Write for Capped<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() as u64 > self.left {
            return Err(std::io::Error::other(format!(
                "larger than {} MB unpacked",
                BUDGET >> 20
            )));
        }
        let n = self.w.write(buf)?;
        self.left -= n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}

fn unrar(file: &Path, dest: &Path) -> Result<usize, String> {
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut a = unrar_rs::RarArchive::open(f).map_err(|e| e.to_string())?;
    let mut budget = BUDGET;
    let mut n = 0;
    for i in 0..a.len() {
        let Some(info) = a.member_info(i) else {
            continue;
        };
        if info.is_directory {
            continue;
        }
        // the name as stored (made safe here, as for the other formats)
        let name = info.raw_name.clone();
        let Some(rel) = safe_rel(&name) else { continue };
        let path = dest.join(&rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let out = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut w = Capped {
            w: std::io::BufWriter::new(out),
            left: budget,
        };
        let r = a
            .by_index(i)
            .and_then(|e| e.copy_to(&mut w))
            .map_err(|e| format!("{name}: {e}"));
        match r {
            Ok(k) => budget -= k,
            Err(e) => {
                drop(w);
                let _ = std::fs::remove_file(&path);
                return Err(e);
            }
        }
        n += 1;
    }
    Ok(n)
}

/// Bring a downloaded `file` into `library` (`<root>/bms`). Archives are
/// unpacked into a folder named after them and removed.
pub fn import(file: &Path, library: &Path) -> Result<Imported, String> {
    let e = ext(file);
    if e == "zip" || e == "7z" || e == "rar" {
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".into());
        let dest = library.join(stem);
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        let n = match e.as_str() {
            "zip" => unzip(file, &dest)?,
            "7z" => un7z(file, &dest)?,
            _ => unrar(file, &dest)?,
        };
        let _ = std::fs::remove_file(file);
        return Ok(Imported::Unpacked(dest, n));
    }
    if !placement::is_chart(file) {
        return Ok(Imported::Other(file.to_path_buf()));
    }
    Ok(match placement::locate(file, library) {
        Some(Located::Move(dir)) => {
            let to = dir.join(file.file_name().unwrap_or_default());
            if vfs::rename(file, &to) {
                Imported::Placed(to)
            } else {
                Imported::Waiting(file.to_path_buf())
            }
        }
        Some(Located::Ask(c)) => Imported::Ask(file.to_path_buf(), c),
        _ => Imported::Waiting(file.to_path_buf()),
    })
}

pub fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What a finished import means for the browser.
pub struct Report {
    /// The library changed.
    pub rescan: bool,
    /// A song arrived (time for the separate chart's page).
    pub song_arrived: bool,
    /// A chart that could belong to several songs, and their folders.
    pub ask: Option<(PathBuf, Vec<(PathBuf, String)>)>,
}

/// Tell the user how an import went; charts waiting for this song go into
/// its folder.
pub fn report(ctx: &mut crate::app::Ctx, r: Result<Imported, String>) -> Report {
    let mut out = Report {
        rescan: true,
        song_arrived: false,
        ask: None,
    };
    match r {
        Err(e) => {
            ctx.toast(format!("Could not unpack the download: {e}"));
            out.rescan = false;
        }
        Ok(Imported::Unpacked(dir, n)) => {
            ctx.toast(format!("Added {} ({n} files)", file_name(&dir)));
            let root = &ctx.paths.root;
            for p in place_waiting(&root.join("downloads"), &root.join("bms")) {
                ctx.toast(format!("{} → its song", file_name(&p)));
            }
            out.song_arrived = true;
        }
        Ok(Imported::Placed(p)) => {
            let dir = p.parent().map(file_name).unwrap_or_default();
            ctx.toast(format!("{} → {dir}", file_name(&p)));
        }
        Ok(Imported::Waiting(p)) => {
            ctx.toast(format!("{}: download its song too", file_name(&p)));
            out.rescan = false;
        }
        Ok(Imported::Ask(chart, folders)) => {
            out.rescan = false;
            out.ask = Some((chart, folders));
        }
        Ok(Imported::Other(p)) => {
            ctx.toast(format!(
                "{} is not a song (kept in downloads)",
                file_name(&p)
            ));
            out.rescan = false;
        }
    }
    out
}

/// Charts waiting in `downloads` (their song came later): place those that
/// now have a folder.
pub fn place_waiting(downloads: &Path, library: &Path) -> Vec<PathBuf> {
    vfs::read_dir(downloads)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir && placement::is_chart(&e.path))
        .filter_map(|e| match placement::locate(&e.path, library) {
            Some(Located::Move(dir)) => {
                let to = dir.join(e.path.file_name().unwrap_or_default());
                vfs::rename(&e.path, &to).then_some(to)
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vibebms-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const CHART: &str = "#TITLE Song\n#ARTIST someone\n#BPM 120\n#WAV01 01.wav\n#00111:01\n";

    #[test]
    fn unpacks_a_zip_and_places_its_chart_later() {
        let root = tmp("zip");
        let (library, downloads) = (root.join("bms"), root.join("downloads"));
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&downloads).unwrap();
        // a 差分 chart that comes before its song waits
        let diff = downloads.join("song_insane.bme");
        std::fs::write(&diff, CHART.replace("#TITLE Song", "#TITLE Song [INSANE]")).unwrap();
        assert_eq!(
            import(&diff, &library).unwrap(),
            Imported::Waiting(diff.clone())
        );
        // the song, zipped with a folder inside and an escaping name
        let pack = downloads.join("Song.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&pack).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            ("Song/song.bms", CHART.as_bytes()),
            ("Song/01.ogg", b"ogg".as_slice()),
            ("../evil.txt", b"x".as_slice()),
        ] {
            z.start_file(name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        let dest = library.join("Song");
        assert_eq!(
            import(&pack, &library).unwrap(),
            Imported::Unpacked(dest.clone(), 3)
        );
        assert!(dest.join("Song/01.ogg").is_file() && dest.join("evil.txt").is_file());
        assert!(!pack.exists() && !root.join("evil.txt").exists());
        // now the waiting chart finds its song
        assert_eq!(
            place_waiting(&downloads, &library),
            vec![dest.join("Song/song_insane.bme")]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
