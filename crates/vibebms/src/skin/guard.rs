//! Which skins were loading when the game last stopped.
//!
//! A skin that takes the game down as it loads or is first drawn (a
//! crash, a hang the player ends) would do so at every start once it is
//! chosen for the song select. So a skin file is noted in a marker file
//! from the moment it loads until a frame has been drawn with it; a
//! marker found at the start names the skins the last run did not get
//! past, which the start then sets aside (`scene::skinned::recover`).
//! The skins compiled into the game are not noted.

use crate::platform::vfs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The marker file and the skins noted in it.
struct Guard {
    file: PathBuf,
    loading: Vec<PathBuf>,
}

impl Guard {
    /// A guard writing `file`, and the skins a run before left in it.
    fn open(file: PathBuf) -> (Guard, Vec<PathBuf>) {
        let left = vfs::read_to_string(&file)
            .map(|t| t.lines().map(PathBuf::from).collect())
            .unwrap_or_default();
        let _ = vfs::remove_file(&file);
        let loading = Vec::new();
        (Guard { file, loading }, left)
    }

    fn loading(&mut self, skin: &Path) {
        if skin.starts_with(vfs::BUILTIN) || self.loading.iter().any(|p| p == skin) {
            return;
        }
        self.loading.push(skin.to_path_buf());
        let lines: Vec<_> = self.loading.iter().map(|p| p.to_string_lossy()).collect();
        let _ = vfs::write(&self.file, lines.join("\n").as_bytes());
    }

    fn frame_drawn(&mut self) {
        if !self.loading.is_empty() {
            self.loading.clear();
            let _ = vfs::remove_file(&self.file);
        }
    }
}

/// The running game's guard (none in tests: nothing is written).
static GUARD: Mutex<Option<Guard>> = Mutex::new(None);

/// Note skin loads in `file` from here on. Returns the skins the last
/// run was loading when it stopped.
pub fn arm(file: PathBuf) -> Vec<PathBuf> {
    let (guard, left) = Guard::open(file);
    if let Ok(mut g) = GUARD.lock() {
        *g = Some(guard);
    }
    left
}

/// `skin` is being loaded.
pub(super) fn loading(skin: &Path) {
    if let Ok(mut g) = GUARD.lock() {
        if let Some(g) = g.as_mut() {
            g.loading(skin);
        }
    }
}

/// A frame was drawn: the skins loaded are past their first.
pub fn frame_drawn() {
    if let Ok(mut g) = GUARD.lock() {
        if let Some(g) = g.as_mut() {
            g.frame_drawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skin_is_noted_until_a_frame_is_drawn() {
        let dir = std::env::temp_dir().join(format!("vibebms-guard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("skins-loading");
        let (a, b) = (dir.join("skins/a/play.json"), dir.join("skins/b/s.lr2skin"));
        let (mut g, left) = Guard::open(file.clone());
        assert!(left.is_empty());
        // nothing loaded, or a skin of the game's own: no file
        g.frame_drawn();
        g.loading(&Path::new(vfs::BUILTIN).join("skins/default/play7.json"));
        assert!(!file.exists());
        g.loading(&a);
        g.loading(&b);
        g.loading(&a);
        // the game stops here: the next run finds both
        let (mut g, left) = Guard::open(file.clone());
        assert_eq!(left, [a.clone(), b]);
        assert!(!file.exists());
        // a frame drawn: nothing is left for the run after
        g.loading(&a);
        assert!(file.exists());
        g.frame_drawn();
        assert!(Guard::open(file).1.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
