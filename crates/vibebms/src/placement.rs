//! Which song's folder a chart of its own belongs in: `bmseeker::placement`
//! on the player's files (`platform::vfs`), which are also where archives
//! are unpacked (`bmseeker::fs::Store`).

pub use bmseeker::placement::*;
use std::io::Write;
use std::path::Path;

/// The player's files, for `bmseeker`.
pub struct Vfs;

impl bmseeker::fs::Fs for Vfs {
    fn read(&self, p: &Path) -> std::io::Result<Vec<u8>> {
        crate::platform::vfs::read(p)
    }
    fn read_dir(&self, p: &Path) -> std::io::Result<Vec<bmseeker::fs::Entry>> {
        Ok(crate::platform::vfs::read_dir(p)?
            .into_iter()
            .map(|e| bmseeker::fs::Entry {
                path: e.path,
                is_dir: e.is_dir,
            })
            .collect())
    }
    fn rename(&self, from: &Path, to: &Path) -> bool {
        crate::platform::vfs::rename(from, to)
    }
    fn stamp(&self, p: &Path) -> Option<(i64, u64)> {
        use crate::platform::vfs;
        vfs::is_file(p).then(|| {
            let (t, n) = vfs::stamp(p);
            (t, n as u64)
        })
    }
    fn remove(&self, p: &Path) -> bool {
        crate::platform::vfs::remove_file(p).is_ok()
    }
    /// A web page's files take the time they are stored at, not the
    /// archive's.
    fn keeps_times(&self) -> bool {
        !cfg!(target_arch = "wasm32")
    }
}

impl bmseeker::fs::Store for Vfs {
    #[cfg(not(target_arch = "wasm32"))]
    fn create(&self, p: &Path, mtime: Option<i64>) -> std::io::Result<Box<dyn Write + '_>> {
        bmseeker::fs::Store::create(&bmseeker::fs::Disk, p, mtime)
    }
    /// Held in memory (and kept in the browser's storage) once written.
    #[cfg(target_arch = "wasm32")]
    fn create(&self, p: &Path, _mtime: Option<i64>) -> std::io::Result<Box<dyn Write + '_>> {
        Ok(Box::new(WebFile {
            p: p.to_path_buf(),
            buf: Vec::new(),
        }))
    }
    fn remove_dir(&self, p: &Path) {
        crate::platform::vfs::remove_dir(p)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn open(&self, p: &Path) -> std::io::Result<Box<dyn bmseeker::fs::ReadSeek + Send>> {
        bmseeker::fs::Store::open(&bmseeker::fs::Disk, p)
    }
    fn create_dir_all(&self, p: &Path) {
        let _ = crate::platform::vfs::create_dir_all(p);
    }
}

/// A file being written in a web page.
#[cfg(target_arch = "wasm32")]
struct WebFile {
    p: std::path::PathBuf,
    buf: Vec<u8>,
}

#[cfg(target_arch = "wasm32")]
impl Write for WebFile {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for WebFile {
    fn drop(&mut self) {
        crate::platform::vfs::put_bytes(&self.p, std::mem::take(&mut self.buf));
    }
}

pub fn chart_info(path: &Path) -> Option<ChartInfo> {
    bmseeker::placement::chart_info(&Vfs, path)
}

pub fn locate(chart: &Path, library: &Path) -> Option<Located> {
    bmseeker::placement::locate(&Vfs, chart, library)
}

/// A chart that came on its own (the files beside it are not its).
pub fn locate_loose(chart: &Path, library: &Path) -> Option<Located> {
    bmseeker::placement::locate_loose(&Vfs, chart, library)
}

/// The library below `<root>/bms` as bmseeker sees it, its charts as the
/// player's library has them (their hashes, titles and artists: a web
/// page cannot read its library's charts when it likes).
pub fn library(root: &Path, songs: &[crate::library::Song]) -> bmseeker::library::Library {
    let owned = songs.iter().map(|s| bmseeker::library::Owned {
        path: s.path.clone().into(),
        md5: s.md5.clone(),
        sha256: s.sha256.clone(),
        title: s.title.clone(),
        artist: s.artist.clone(),
    });
    bmseeker::library::Library::from_catalog(&root.join("bms"), owned)
}

/// [`locate_loose`] in `lib`.
pub fn locate_loose_in(chart: &Path, lib: &bmseeker::library::Library) -> Option<Located> {
    bmseeker::placement::locate_in(&Vfs, chart, lib, false, Rules::default()).map(|(l, _)| l)
}

/// Move `file` into `dir` (smart overwrite: `bmseeker::placement::bring`).
pub fn bring(file: &Path, dir: &Path) -> Option<std::path::PathBuf> {
    bmseeker::placement::bring(&Vfs, file, dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmseeker::fs::Fs;

    /// An archive unpacked into the player's files keeps its times.
    #[test]
    fn unpacked_with_the_archive_times() {
        use bmseeker::unpack::{unpack, Blob, BUDGET};
        let d = std::env::temp_dir().join(format!("vibebms-vfs-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        // a RAR5 (stored) of song/a.bms and song/01.ogg ("ogg"), both of
        // 2020-01-02 03:04:05 UTC
        let rar: [u8; 117] = [
            82, 97, 114, 33, 26, 7, 1, 0, 51, 146, 181, 229, 10, 1, 5, 6, 0, 5, 1, 1, 128, 128, 0,
            66, 98, 211, 59, 32, 2, 2, 136, 0, 6, 136, 0, 164, 131, 2, 165, 93, 13, 94, 221, 184,
            73, 19, 128, 0, 1, 10, 115, 111, 110, 103, 47, 97, 46, 98, 109, 115, 35, 84, 73, 84,
            76, 69, 32, 65, 227, 107, 162, 118, 33, 2, 2, 131, 0, 6, 131, 0, 164, 131, 2, 165, 93,
            13, 94, 148, 92, 160, 69, 128, 0, 1, 11, 115, 111, 110, 103, 47, 48, 49, 46, 111, 103,
            103, 111, 103, 103, 29, 119, 86, 81, 3, 5, 4, 0,
        ];
        let got = unpack(Blob::Bytes(rar.to_vec()), &Vfs, &d, BUDGET).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(Vfs.stamp(&d.join("song/01.ogg")), Some((1_577_934_245, 3)));
        assert!(Vfs.keeps_times());
        bmseeker::fs::Store::remove_dir(&Vfs, &d);
        assert!(!d.exists());
    }
}
