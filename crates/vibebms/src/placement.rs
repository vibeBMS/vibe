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
    #[cfg(not(target_arch = "wasm32"))]
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        bmseeker::fs::Fs::rename(&bmseeker::fs::Disk, from, to)
    }
    #[cfg(target_arch = "wasm32")]
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        use crate::platform::vfs;
        match vfs::rename(from, to) {
            true => Ok(()),
            false if vfs::is_file(to) => Err(std::io::ErrorKind::AlreadyExists.into()),
            false => Err(std::io::ErrorKind::NotFound.into()),
        }
    }
    fn stamp(&self, p: &Path) -> Option<(i64, u64)> {
        use crate::platform::vfs;
        vfs::is_file(p).then(|| {
            let (t, n) = vfs::stamp(p);
            (t, n as u64)
        })
    }
    fn remove(&self, p: &Path) -> std::io::Result<()> {
        crate::platform::vfs::remove_file(p)
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
    #[cfg(not(target_arch = "wasm32"))]
    fn copy(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        bmseeker::fs::Store::copy(&bmseeker::fs::Disk, from, to)
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

/// Where in `lib` a chart that came on its own belongs (the files beside
/// it are not its).
pub fn locate_loose(chart: &Path, lib: &bmseeker::library::Library) -> Option<Located> {
    bmseeker::placement::locate(&Vfs, chart, lib, false, Rules::default()).map(|(l, _)| l)
}

/// Move `file` into `dir` (smart overwrite: `bmseeker::placement::bring`).
pub fn bring(file: &Path, dir: &Path) -> Option<std::path::PathBuf> {
    bmseeker::placement::bring(&Vfs, file, dir)
        .inspect_err(|e| log::warn!("{} into {}: {e}", file.display(), dir.display()))
        .ok()
        .flatten()
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
        // (both of 2020-01-02 03:04:05 UTC)
        let rar = bmseeker::testkit::SONG_RAR.to_vec();
        let got = unpack(Blob::Bytes(rar), &Vfs, &d, BUDGET).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(Vfs.stamp(&d.join("song/01.ogg")), Some((1_577_934_245, 3)));
        assert!(Vfs.keeps_times());
        bmseeker::fs::Store::remove_dir(&Vfs, &d);
        assert!(!d.exists());
    }
}
