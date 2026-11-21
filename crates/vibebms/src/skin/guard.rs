//! Which skins were loading when the game last stopped.
//!
//! A skin that takes the game down as it loads or is first drawn (a
//! crash, a hang the player ends) would do so at every start once it is
//! chosen for the song select. So a skin file is noted in a marker file
//! from the moment it loads until a frame has been drawn with it ready
//! (its images in: the frames before draw nothing of it); a marker found
//! at the start names the skins the last run did not get past, which the
//! start then sets aside (`scene::skinned::recover`). A load that fails,
//! or a skin let go before it was drawn (a trial load, a preview), is
//! past too. The skins compiled into the game are not noted; a sound set
//! is noted while it is decoded.

use crate::platform::vfs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The marker file and the skins noted in it.
struct Guard {
    file: PathBuf,
    /// Each skin noted and how many of its loads are not past yet.
    loading: Vec<(PathBuf, usize)>,
    /// Skins drawn ready in the frame being made.
    drawn: Vec<PathBuf>,
}

impl Guard {
    /// A guard writing `file`, and the skins a run before left in it.
    fn open(file: PathBuf) -> (Guard, Vec<PathBuf>) {
        let left = vfs::read_to_string(&file)
            .map(|t| t.lines().map(PathBuf::from).collect())
            .unwrap_or_default();
        let _ = vfs::remove_file(&file);
        let guard = Guard {
            file,
            loading: Vec::new(),
            drawn: Vec::new(),
        };
        (guard, left)
    }

    fn write(&self) {
        if self.loading.is_empty() {
            let _ = vfs::remove_file(&self.file);
            return;
        }
        let lines: Vec<_> = self
            .loading
            .iter()
            .map(|(p, _)| p.to_string_lossy())
            .collect();
        let _ = vfs::write(&self.file, lines.join("\n").as_bytes());
    }

    fn loading(&mut self, skin: &Path) {
        match self.loading.iter_mut().find(|(p, _)| p == skin) {
            Some((_, n)) => *n += 1,
            None => {
                self.loading.push((skin.to_path_buf(), 1));
                self.write();
            }
        }
    }

    /// One load of `skin` is past.
    fn past(&mut self, skin: &Path) {
        let Some(i) = self.loading.iter().position(|(p, _)| p == skin) else {
            return;
        };
        self.loading[i].1 -= 1;
        if self.loading[i].1 == 0 {
            self.loading.remove(i);
            self.write();
        }
    }

    fn frame_drawn(&mut self) {
        for p in std::mem::take(&mut self.drawn) {
            self.past(&p);
        }
    }
}

/// The running game's guard (none in tests: nothing is written).
static GUARD: Mutex<Option<Guard>> = Mutex::new(None);

fn with(f: impl FnOnce(&mut Guard)) {
    if let Ok(mut g) = GUARD.lock() {
        if let Some(g) = g.as_mut() {
            f(g);
        }
    }
}

/// Note skin loads in `file` from here on. Returns the skins the last
/// run was loading when it stopped.
pub fn arm(file: PathBuf) -> Vec<PathBuf> {
    let (guard, left) = Guard::open(file);
    if let Ok(mut g) = GUARD.lock() {
        *g = Some(guard);
    }
    left
}

/// A load of a skin file, noted until it is past: a frame drawn with the
/// skin ready ([`Note::drawn`]), or this dropped before (the load failed,
/// what it made was let go). Dropped as a panic unwinds, it stays.
#[derive(Debug)]
pub struct Note {
    path: PathBuf,
    drawn: AtomicBool,
}

impl Note {
    /// The skin was drawn ready: past once the frame is.
    pub fn drawn(&self) {
        if !self.drawn.swap(true, Ordering::Relaxed) {
            with(|g| g.drawn.push(self.path.clone()));
        }
    }
}

impl Drop for Note {
    fn drop(&mut self) {
        if !self.drawn.load(Ordering::Relaxed) && !std::thread::panicking() {
            with(|g| g.past(&self.path));
        }
    }
}

/// `skin` is being loaded (none for a skin of the game's own).
pub fn loading(skin: &Path) -> Option<Note> {
    if skin.starts_with(vfs::BUILTIN) {
        return None;
    }
    with(|g| g.loading(skin));
    Some(Note {
        path: skin.to_path_buf(),
        drawn: AtomicBool::new(false),
    })
}

/// A frame was drawn: the skins drawn ready in it are past their first.
pub fn frame_drawn() {
    with(Guard::frame_drawn);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skin_is_noted_until_a_frame_is_drawn_with_it() {
        let dir = std::env::temp_dir().join(format!("vibebms-guard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("skins-loading");
        let (a, b) = (dir.join("skins/a/play.json"), dir.join("skins/b/s.lr2skin"));
        let (mut g, left) = Guard::open(file.clone());
        assert!(left.is_empty());
        // nothing loaded: no file
        g.frame_drawn();
        assert!(!file.exists());
        g.loading(&a);
        g.loading(&b);
        g.loading(&a);
        // frames drawn before the skins are: they stay noted
        g.frame_drawn();
        // the game stops here: the next run finds both
        let (mut g, left) = Guard::open(file.clone());
        assert_eq!(left, [a.clone(), b.clone()]);
        assert!(!file.exists());
        // a drawn, b let go undrawn: nothing is left for the run after
        g.loading(&a);
        g.loading(&b);
        assert!(file.exists());
        g.past(&b);
        g.drawn.push(a.clone());
        assert!(Guard::open(file.clone()).1 == [a.clone()]);
        let (mut g, _) = Guard::open(file.clone());
        g.loading(&a);
        g.loading(&a);
        g.past(&a);
        g.drawn.push(a.clone());
        g.frame_drawn();
        assert!(Guard::open(file).1.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
