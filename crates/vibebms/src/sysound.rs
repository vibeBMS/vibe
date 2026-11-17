//! System sounds: the music select BGM, the decide jingle, clicks for
//! folders and options, the result fanfares. They come from a sound set,
//! chosen like a skin (`config.skins["SOUND_SET"]`):
//!
//! - a folder of beatoraja's sound files (`select.wav`, `decide.wav`,
//!   `f-open.wav`, ...; any audio format), as beatoraja's sound / BGM sets;
//! - an LR2 sound set file (`.lr2ss`, `#INFORMATION,10,...` then
//!   `#SELECT,LR2files\Bgm\...`);
//! - none: the built-in set, a few short generated clicks.
//!
//! Sets are looked for in `<root>/sounds/` and `<root>/skins/`.

use crate::platform::{vfs, Job};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vibebms_audio::{Bus, Mixer, PlayOptions, Sample};

/// A system sound (beatoraja's `SoundType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    Select,
    Decide,
    Scratch,
    FolderOpen,
    FolderClose,
    OptionChange,
    OptionOpen,
    OptionClose,
    PlayReady,
    PlayStop,
    ResultClear,
    ResultFail,
    ResultClose,
    CourseClear,
    CourseFail,
    CourseClose,
}

impl Sound {
    pub const ALL: [Sound; 16] = [
        Sound::Select,
        Sound::Decide,
        Sound::Scratch,
        Sound::FolderOpen,
        Sound::FolderClose,
        Sound::OptionChange,
        Sound::OptionOpen,
        Sound::OptionClose,
        Sound::PlayReady,
        Sound::PlayStop,
        Sound::ResultClear,
        Sound::ResultFail,
        Sound::ResultClose,
        Sound::CourseClear,
        Sound::CourseFail,
        Sound::CourseClose,
    ];

    /// beatoraja's file name.
    pub fn file(self) -> &'static str {
        match self {
            Sound::Select => "select.wav",
            Sound::Decide => "decide.wav",
            Sound::Scratch => "scratch.wav",
            Sound::FolderOpen => "f-open.wav",
            Sound::FolderClose => "f-close.wav",
            Sound::OptionChange => "o-change.wav",
            Sound::OptionOpen => "o-open.wav",
            Sound::OptionClose => "o-close.wav",
            Sound::PlayReady => "playready.wav",
            Sound::PlayStop => "playstop.wav",
            Sound::ResultClear => "clear.wav",
            Sound::ResultFail => "fail.wav",
            Sound::ResultClose => "resultclose.wav",
            Sound::CourseClear => "course_clear.wav",
            Sound::CourseFail => "course_fail.wav",
            Sound::CourseClose => "course_close.wav",
        }
    }

    /// The LR2 sound set command.
    fn lr2(self) -> Option<&'static str> {
        Some(match self {
            Sound::Select => "#SELECT",
            Sound::Decide => "#DECIDE",
            Sound::Scratch => "#SCRATCH",
            Sound::FolderOpen => "#FOLDER_OPEN",
            Sound::FolderClose => "#FOLDER_CLOSE",
            Sound::OptionChange => "#OPTION_CHANGE",
            Sound::OptionOpen => "#PANEL_OPEN",
            Sound::OptionClose => "#PANEL_CLOSE",
            Sound::PlayStop => "#STOP",
            Sound::ResultClear => "#CLEAR",
            Sound::ResultFail => "#FAIL",
            Sound::CourseClear => "#COURSECLEAR",
            Sound::CourseFail => "#COURSEFAIL",
            Sound::PlayReady | Sound::ResultClose | Sound::CourseClose => return None,
        })
    }

    /// Plays until stopped.
    fn looping(self) -> bool {
        self == Sound::Select
    }

    /// Mixer voice key (one voice per sound).
    fn key(self) -> u64 {
        u64::MAX - 100 + self as u64
    }
}

/// A sound set found on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct SetInfo {
    pub path: PathBuf,
    pub name: String,
    pub author: String,
}

/// (name, author) of an LR2 sound set file (`#INFORMATION,10,...`).
pub fn lr2ss_header(path: &Path) -> Option<(String, String)> {
    let b = vfs::read(path).ok()?;
    let text = vibebms_core::encoding::decode_text(&b);
    text.lines().find_map(|l| {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        (f[0].eq_ignore_ascii_case("#INFORMATION") && f.get(1) == Some(&"10")).then(|| {
            (
                f.get(2).unwrap_or(&"").to_string(),
                f.get(3).unwrap_or(&"").to_string(),
            )
        })
    })
}

/// A folder holding beatoraja's sound files.
fn is_sound_folder(dir: &Path) -> bool {
    Sound::ALL
        .iter()
        .any(|s| vfs::find_by_stem(dir, s.file(), vfs::AUDIO_EXTS).is_some())
}

/// The sound sets in `<root>/sounds` and `<root>/skins`, by path.
pub fn find_sets(root: &Path) -> Vec<SetInfo> {
    fn walk(d: &Path, depth: usize, out: &mut Vec<SetInfo>) {
        if depth > 4 {
            return;
        }
        let mut entries = vfs::read_dir(d).unwrap_or_default();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        for e in entries {
            if e.is_dir {
                if is_sound_folder(&e.path) {
                    let name = e
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    out.push(SetInfo {
                        path: e.path.clone(),
                        name,
                        author: String::new(),
                    });
                }
                walk(&e.path, depth + 1, out);
            } else if e
                .path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("lr2ss"))
            {
                if let Some((name, author)) = lr2ss_header(&e.path) {
                    out.push(SetInfo {
                        path: e.path,
                        name,
                        author,
                    });
                }
            }
        }
    }
    let mut out = Vec::new();
    for d in ["sounds", "skins"] {
        walk(&root.join(d), 0, &mut out);
    }
    out
}

/// A set's name and author (the folder's name for a folder).
pub fn set_info(path: &Path) -> Option<SetInfo> {
    let (name, author) = if vfs::is_dir(path) {
        let n = path.file_name()?.to_string_lossy().into_owned();
        (n, String::new())
    } else {
        lr2ss_header(path)?
    };
    Some(SetInfo {
        path: path.to_path_buf(),
        name,
        author,
    })
}

/// Where an LR2 sound set's `LR2files\...` path is: next to the folder
/// holding the set (LR2's own layout, `Sound\x.lr2ss` beside `Bgm\`), in
/// `<root>/sounds`, or beside the set file.
fn lr2_path(set: &Path, root: &Path, raw: &str) -> Option<PathBuf> {
    let s = raw.trim().replace('\\', "/");
    let rest = match s.to_ascii_lowercase().find("lr2files/") {
        Some(i) => s[i + "lr2files/".len()..].to_string(),
        None => s.clone(),
    };
    let dir = set.parent()?;
    let tail = rest.split_once('/').map(|(_, t)| t.to_string());
    let bases = [dir.parent(), Some(&root.join("sounds")), Some(dir)];
    for base in bases.into_iter().flatten() {
        for name in std::iter::once(&rest).chain(tail.as_ref()) {
            let p = vfs::resolve_case(&base.join(name));
            let (Some(d), Some(f)) = (p.parent(), p.file_name()) else {
                continue;
            };
            if let Some(found) = vfs::find_by_stem(d, &f.to_string_lossy(), vfs::AUDIO_EXTS) {
                return Some(found);
            }
        }
    }
    None
}

/// The files of a set (a folder or an `.lr2ss`), by sound.
pub fn set_files(path: &Path, root: &Path) -> HashMap<Sound, PathBuf> {
    let mut out = HashMap::new();
    if vfs::is_dir(path) {
        for s in Sound::ALL {
            if let Some(p) = vfs::find_by_stem(path, s.file(), vfs::AUDIO_EXTS) {
                out.insert(s, p);
            }
        }
        return out;
    }
    let Ok(b) = vfs::read(path) else {
        return out;
    };
    let text = vibebms_core::encoding::decode_text(&b);
    for l in text.lines() {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        let Some(s) = Sound::ALL
            .into_iter()
            .find(|s| s.lr2().is_some_and(|k| k.eq_ignore_ascii_case(f[0])))
        else {
            continue;
        };
        // the first line of a sound counts
        if out.contains_key(&s) {
            continue;
        }
        if let Some(p) = f.get(1).and_then(|raw| lr2_path(path, root, raw)) {
            out.insert(s, p);
        }
    }
    out
}

/// A short tone gliding from `f0` to `f1` Hz with a quick decay.
fn tone(rate: u32, ms: f32, f0: f32, f1: f32, amp: f32) -> Sample {
    let n = (rate as f32 * ms / 1000.0) as usize;
    let mut phase = 0.0f32;
    let data = (0..n)
        .map(|i| {
            let t = i as f32 / n.max(1) as f32;
            phase += (f0 + (f1 - f0) * t) / rate as f32;
            let env = (1.0 - t).powi(2) * (t * 40.0).min(1.0);
            ((phase * std::f32::consts::TAU).sin() * env * amp * 32767.0) as i16
        })
        .collect();
    Sample {
        channels: 1,
        frames: n,
        data,
    }
}

/// The built-in set: soft clicks for the select screen and a short decide
/// chime (no music).
fn built_in(s: Sound, rate: u32) -> Option<Sample> {
    Some(match s {
        Sound::Scratch => tone(rate, 18.0, 1800.0, 1500.0, 0.12),
        Sound::OptionChange => tone(rate, 40.0, 1000.0, 1000.0, 0.2),
        Sound::OptionOpen => tone(rate, 70.0, 600.0, 900.0, 0.2),
        Sound::OptionClose => tone(rate, 70.0, 900.0, 600.0, 0.2),
        Sound::FolderOpen => tone(rate, 60.0, 500.0, 800.0, 0.22),
        Sound::FolderClose => tone(rate, 60.0, 800.0, 500.0, 0.22),
        Sound::Decide => {
            let (a, b) = (
                tone(rate, 120.0, 660.0, 660.0, 0.25),
                tone(rate, 260.0, 990.0, 990.0, 0.25),
            );
            let mut data = a.data;
            data.extend(b.data);
            Sample {
                channels: 1,
                frames: data.len(),
                data,
            }
        }
        _ => return None,
    })
}

type Loaded = Vec<(Sound, Arc<Sample>)>;

/// The chosen set's sounds, decoded off the main thread.
#[derive(Default)]
pub struct SystemSounds {
    /// The `config.skins` value loaded (`None`: nothing yet).
    value: Option<String>,
    samples: HashMap<Sound, Arc<Sample>>,
    job: Option<Job<Loaded>>,
    /// A looping sound asked for while the set was loading.
    want_loop: Option<Sound>,
}

impl SystemSounds {
    /// Load the set `value` names (a path relative to `root`; empty: the
    /// built-in one) unless it is the one loaded.
    pub fn sync(&mut self, value: &str, root: &Path, rate: u32) {
        if self.value.as_deref() == Some(value) {
            return;
        }
        self.value = Some(value.to_string());
        self.samples.clear();
        if value.is_empty() {
            for s in Sound::ALL {
                if let Some(x) = built_in(s, rate) {
                    self.samples.insert(s, Arc::new(x));
                }
            }
            self.job = None;
            return;
        }
        let path = root.join(value);
        let root = root.to_path_buf();
        self.job = Some(Job::spawn("sound set", move |_| {
            let mut out = Vec::new();
            for (s, p) in set_files(&path, &root) {
                let ext = p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                match vfs::read(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|b| vibebms_audio::decode_bytes(b, &ext, rate))
                {
                    Ok(x) => out.push((s, Arc::new(x))),
                    Err(e) => log::warn!("sound set: {}: {e}", p.display()),
                }
            }
            out
        }));
    }

    /// Take the decoded sounds once they are ready.
    pub fn poll(&mut self, mixer: &Mixer) {
        let Some(v) = self.job.as_mut().and_then(Job::poll) else {
            return;
        };
        self.job = None;
        self.samples.extend(v);
        if let Some(s) = self.want_loop.take() {
            self.play(mixer, s);
        }
    }

    pub fn has(&self, s: Sound) -> bool {
        self.samples.contains_key(&s)
    }

    pub fn play(&mut self, mixer: &Mixer, s: Sound) {
        match self.samples.get(&s) {
            Some(x) => mixer.play(
                x,
                PlayOptions {
                    key: s.key(),
                    bus: Bus::System,
                    looping: s.looping(),
                    ..Default::default()
                },
            ),
            None if s.looping() && self.job.is_some() => self.want_loop = Some(s),
            None => {}
        }
    }

    pub fn stop(&mut self, mixer: &Mixer, s: Sound) {
        if self.want_loop == Some(s) {
            self.want_loop = None;
        }
        mixer.stop(s.key());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_sets() {
        let root = std::env::temp_dir().join(format!("vibebms-sset-{}", std::process::id()));
        let snd = root.join("sounds");
        // a beatoraja set (a folder) and an LR2 set beside LR2's folders
        std::fs::create_dir_all(snd.join("raja")).unwrap();
        std::fs::write(snd.join("raja/decide.ogg"), b"x").unwrap();
        std::fs::write(snd.join("raja/F-OPEN.wav"), b"x").unwrap();
        std::fs::create_dir_all(snd.join("LR2/Sound")).unwrap();
        std::fs::create_dir_all(snd.join("LR2/Bgm/e2e")).unwrap();
        std::fs::write(snd.join("LR2/Bgm/e2e/select.wav"), b"x").unwrap();
        std::fs::write(
            snd.join("LR2/Sound/e2e.lr2ss"),
            "#INFORMATION,10,E2E,me,\n#SELECT,LR2files\\Bgm\\e2e\\select.wav,\n\
             #SELECT,LR2files\\Bgm\\other.wav,\n#DECIDE,LR2files\\Bgm\\e2e\\missing.wav,\n",
        )
        .unwrap();
        let sets = find_sets(&root);
        let names: Vec<&str> = sets.iter().map(|s| s.name.as_str()).collect();
        // LR2's BGM folder holds beatoraja's select.wav: a set too
        assert_eq!(names, vec!["e2e", "E2E", "raja"]);
        assert_eq!(sets[1].author, "me");
        let raja = set_files(&snd.join("raja"), &root);
        assert_eq!(raja.len(), 2);
        assert!(raja[&Sound::Decide].ends_with("decide.ogg"));
        assert!(raja[&Sound::FolderOpen].ends_with("F-OPEN.wav"));
        let lr2 = set_files(&snd.join("LR2/Sound/e2e.lr2ss"), &root);
        // chosen like a skin of type 10
        let h = crate::skin::header(&snd.join("LR2/Sound/e2e.lr2ss"));
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(lr2.len(), 1);
        assert!(lr2[&Sound::Select].ends_with("Bgm/e2e/select.wav"));
        assert_eq!(h, Some((crate::skin::SkinType::SoundSet, "E2E".into())));
    }

    #[test]
    fn built_in_set() {
        let mut s = SystemSounds::default();
        s.sync("", Path::new("/nonexistent"), 44100);
        assert!(s.has(Sound::Decide) && s.has(Sound::OptionChange));
        assert!(!s.has(Sound::Select));
    }
}
