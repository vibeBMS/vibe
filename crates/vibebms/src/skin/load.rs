//! What a skin load reads: the skin's folder, the files it names (within
//! what a load may take) and the sizes of its images.

use std::path::PathBuf;

/// Where a skin's files are, and how far it may read: the files it names
/// (images, fonts, includes, what its scripts open) are taken from `dir`
/// and never from outside `root`.
#[derive(Clone, Debug, Default)]
pub struct Home {
    /// The skins folder, for a skin anywhere below it (as the LR2 loader
    /// keeps to it: themes may share files); for a skin kept elsewhere
    /// (the built-in ones), its own folder.
    pub(super) root: PathBuf,
    /// The folder of the skin file.
    pub(super) dir: PathBuf,
}

impl Home {
    pub fn of(skin: &std::path::Path, skins_root: &std::path::Path) -> Home {
        let dir = skin.parent().unwrap_or(std::path::Path::new(""));
        let root = match skin.starts_with(skins_root) {
            true => skins_root,
            false => dir,
        };
        Home {
            root: root.to_path_buf(),
            dir: dir.to_path_buf(),
        }
    }

    /// The file the skin calls `name`, from the folder `dir`; `None` for
    /// one outside `root`, which is then not looked at (`vfs::within`).
    pub(super) fn file_from(&self, dir: &std::path::Path, name: &str) -> Option<PathBuf> {
        crate::platform::vfs::within(&self.root, dir, name)
    }

    /// [`Home::file_from`] the skin's own folder.
    pub(super) fn file(&self, name: &str) -> Option<PathBuf> {
        self.file_from(&self.dir, name)
    }
}

/// `read` (what reads the skin file `path`), a panic in it as an error:
/// what a skin file holds must not take the game down. What the readers
/// change while they run is their own (the model being built, a Lua
/// state) and is dropped as the panic unwinds; the caches they share are
/// written once a read is done, and never held across one. Where a panic
/// aborts, the web's build (wasm32-unknown-unknown has no unwinding), this
/// catches nothing and the page stops: there the skin guard is the cover,
/// its note kept to set the skin aside at the next start ([`super::guard`]).
pub(super) fn no_panic<T>(
    path: &std::path::Path,
    read: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).unwrap_or_else(|e| {
        let said = (e.downcast_ref::<&str>().copied())
            .or_else(|| e.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("a panic");
        Err(format!(
            "{}: the skin reader failed ({said})",
            path.display()
        ))
    })
}

/// Bytes of a file a skin is written in (its lines, a script, a font's
/// glyph list) and of what a script reads.
const TEXT_MAX: u64 = 16 << 20;
/// Bytes of an image or a sound a skin or a sound set names.
pub(super) const MEDIA_MAX: u64 = 256 << 20;

/// Read a file a skin is written in ([`TEXT_MAX`]).
pub(crate) fn read_text(p: &std::path::Path) -> std::io::Result<Vec<u8>> {
    crate::platform::vfs::read_capped(p, TEXT_MAX)
}

/// Read an image or a sound a skin names ([`MEDIA_MAX`]).
pub(crate) fn read_media(p: &std::path::Path) -> std::io::Result<Vec<u8>> {
    crate::platform::vfs::read_capped(p, MEDIA_MAX)
}

/// Bytes a skin load reads through `#INCLUDE` / `include` in all: the
/// nesting is limited, but a file may include another any number of
/// times at each level.
const INCLUDE_BYTES: usize = 16 << 20;
/// What an included file counts at least.
const INCLUDE_MIN: usize = 4 << 10;

/// What a skin load may still read through includes.
pub(super) struct Includes {
    left: usize,
    pub(super) over: bool,
}

impl Includes {
    pub(super) fn new() -> Includes {
        Includes {
            left: INCLUDE_BYTES,
            over: false,
        }
    }

    /// The bytes of included file `p`; none once the load has read more
    /// than it may.
    pub(super) fn read(&mut self, p: &std::path::Path) -> Option<Vec<u8>> {
        if self.over {
            return None;
        }
        let b = read_text(p).ok()?;
        match self.left.checked_sub(b.len().max(INCLUDE_MIN)) {
            Some(left) => self.left = left,
            None => self.over = true,
        }
        (!self.over).then_some(b)
    }
}

/// The files matching a `*` in the file name of `p` (sorted), else `p`.
pub fn glob_paths(p: &std::path::Path) -> Vec<PathBuf> {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some((pre, post)) = name.split_once('*') else {
        return vec![p.to_path_buf()];
    };
    let (pre, post) = (pre.to_lowercase(), post.to_lowercase());
    let dir = p.parent().unwrap_or(std::path::Path::new(""));
    let mut c: Vec<PathBuf> = crate::platform::vfs::read_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.path)
        .filter(|c| {
            let n = c
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            n.starts_with(&pre) && n.ends_with(&post) && n.len() >= pre.len() + post.len()
        })
        .collect();
    c.sort();
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin::image_size;

    /// A file a skin names is read when it is a file of a size a skin's
    /// files have: no device (which has no end), nothing far larger.
    #[cfg(unix)]
    #[test]
    fn devices_and_oversized_files_are_not_read() {
        let dir = std::env::temp_dir().join(format!("vibebms-readable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zero = std::path::Path::new("/dev/zero");
        assert!(read_text(zero).is_err() && read_media(zero).is_err());
        assert_eq!(image_size(zero), None);
        std::os::unix::fs::symlink(zero, dir.join("zero.png")).unwrap();
        assert_eq!(image_size(&dir.join("zero.png")), None);
        let big = dir.join("big.json");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(TEXT_MAX + 1).unwrap();
        assert!(read_text(&big).is_err());
        f.set_len(TEXT_MAX).unwrap();
        assert_eq!(read_text(&big).map(|b| b.len() as u64).ok(), Some(TEXT_MAX));
        std::fs::remove_dir_all(&dir).ok();
    }
}
