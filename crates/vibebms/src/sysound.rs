//! System sounds: the music select BGM, the decide jingle, clicks for
//! folders and options, the result fanfares. They come from a sound set,
//! chosen like a skin (`config.skins["SOUND_SET"]`):
//!
//! - a folder of beatoraja's sound files (`select.wav`, `decide.wav`,
//!   `f-open.wav`, ...; any audio format), as beatoraja's sound / BGM sets;
//! - an LR2 sound set file (`.lr2ss`, `#INFORMATION,10,...` then
//!   `#SELECT,LR2files\Bgm\...`);
//! - none: the built-in set, a few short generated clicks;
//! - RANDOM ([`RANDOM_SET`]): one of the sets drawn when the app starts and
//!   each time the song select comes back.
//!
//! Sets are looked for in `<root>/sounds/` and `<root>/skins/`: the first
//! folder down holding sound files is a set. Its subfolders holding sound
//! files are its variants: one is drawn with the set, and a sound it lacks
//! is the set's own (a variant has only what it changes).
//!
//! A folder set follows bmz-player's sound set conventions (its
//! documentation): the result music by outcome (`fail`; `aaa`, `aa` or `a`
//! for a clear at that rank, else `clear`), `<name>.loop.<ext>` playing
//! over and over until the screen ends (`select.loop`, `clear.loop`, ...),
//! `<name>.enter.<ext>` once as the screen comes (`select.enter`,
//! `clear.enter`, ...).
//!
//! A screen's music is decoded each time it starts and let go when it
//! stops (minutes of music are tens of MB decoded); the short sounds stay
//! decoded.

use crate::loudness::Loudness;
use crate::platform::{vfs, Job};
use crate::skin::guard;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vibebms_audio::{Budget, Bus, Mixer, PlayOptions, Sample};

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
    /// A clear at rank A, AA, AAA (bmz-player's sound sets).
    ResultA,
    ResultAa,
    ResultAaa,
}

/// How a sound set's file of a sound plays: as it is, `<name>.loop` (over
/// and over until the screen ends) or `<name>.enter` (once as the screen
/// comes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Form {
    Plain,
    Loop,
    Enter,
}

impl Form {
    const ALL: [Form; 3] = [Form::Plain, Form::Loop, Form::Enter];
}

/// The value of `config.skins["SOUND_SET"]` that draws a set at random.
pub const RANDOM_SET: &str = "*random*";

impl Sound {
    pub const ALL: [Sound; 19] = [
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
        Sound::ResultA,
        Sound::ResultAa,
        Sound::ResultAaa,
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
            Sound::ResultA => "a.wav",
            Sound::ResultAa => "aa.wav",
            Sound::ResultAaa => "aaa.wav",
        }
    }

    /// The file name of its `form` (`clear.loop.wav`), if it has one.
    fn file_of(self, form: Form) -> Option<String> {
        let stem = self.file().trim_end_matches(".wav");
        match form {
            Form::Plain => Some(self.file().to_string()),
            Form::Loop | Form::Enter if !self.scene_music() => None,
            Form::Loop => Some(format!("{stem}.loop.wav")),
            Form::Enter => Some(format!("{stem}.enter.wav")),
        }
    }

    /// A screen's music, which may have `.loop` and `.enter` files.
    fn scene_music(self) -> bool {
        matches!(
            self,
            Sound::Select
                | Sound::ResultClear
                | Sound::ResultFail
                | Sound::ResultA
                | Sound::ResultAa
                | Sound::ResultAaa
                | Sound::CourseClear
                | Sound::CourseFail
        )
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
            Sound::PlayReady
            | Sound::ResultClose
            | Sound::CourseClose
            | Sound::ResultA
            | Sound::ResultAa
            | Sound::ResultAaa => return None,
        })
    }

    /// Plays until stopped: `.loop` files, and the select music in either
    /// form.
    fn looping(self, form: Form) -> bool {
        form == Form::Loop || (self == Sound::Select && form == Form::Plain)
    }

    /// Music, brought to a common loudness with `[audio] normalize_system`:
    /// what loops and the decide jingle.
    fn music(self, form: Form) -> bool {
        self.looping(form) || (self == Sound::Decide && form == Form::Plain)
    }

    /// A screen's music (minutes of it, tens of MB decoded): decoded when
    /// it plays and not kept.
    fn long(self, form: Form) -> bool {
        self.scene_music() && form != Form::Enter
    }

    /// Mixer voice key (one voice per sound and form).
    fn key(self, form: Form) -> u64 {
        u64::MAX - 100 + self as u64 * 3 + form as u64
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
    let b = crate::skin::read_text(path).ok()?;
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

/// The sound files in `dir`, by sound and form.
fn folder_files(dir: &Path) -> HashMap<(Sound, Form), PathBuf> {
    let index = vfs::DirIndex::new(dir);
    let mut out = HashMap::new();
    for s in Sound::ALL {
        for f in Form::ALL {
            if let Some(p) = s.file_of(f).and_then(|n| index.find(&n, vfs::AUDIO_EXTS)) {
                out.insert((s, f), p);
            }
        }
    }
    out
}

/// A folder holding beatoraja's (or bmz-player's) sound files.
fn is_sound_folder(dir: &Path) -> bool {
    !folder_files(dir).is_empty()
}

/// A folder set's variants: its subfolders holding sound files.
pub fn variants(set: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vfs::read_dir(set)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.is_dir && is_sound_folder(&e.path))
        .map(|e| e.path)
        .collect();
    v.sort();
    v
}

/// The folders of the data root that sound sets are kept in.
const SET_FOLDERS: [&str; 2] = ["sounds", "skins"];

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
                    // (its subfolders are its variants)
                    continue;
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
    for d in SET_FOLDERS {
        walk(&root.join(d), 0, &mut out);
    }
    out
}

/// A set's name and author (the folder's name for a folder).
pub fn set_info(path: &Path) -> Option<SetInfo> {
    if path.ends_with(RANDOM_SET) {
        return Some(SetInfo {
            path: path.to_path_buf(),
            name: "RANDOM".into(),
            author: String::new(),
        });
    }
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
/// `<root>/sounds`, or beside the set file. A set's sounds are in the
/// folders sets are kept in (`<root>/sounds`, `<root>/skins`): a path
/// that leads out of them finds nothing.
fn lr2_path(set: &Path, root: &Path, raw: &str) -> Option<PathBuf> {
    let s = raw.trim().replace('\\', "/");
    let rest = match s.to_ascii_lowercase().find("lr2files/") {
        Some(i) => s[i + "lr2files/".len()..].to_string(),
        None => s.clone(),
    };
    let dir = set.parent()?;
    let tail = rest.split_once('/').map(|(_, t)| t.to_string());
    let homes = SET_FOLDERS.map(|d| root.join(d));
    let bases = [dir.parent(), Some(&homes[0]), Some(dir)];
    for base in bases.into_iter().flatten() {
        for name in std::iter::once(&rest).chain(tail.as_ref()) {
            let Some(p) = homes.iter().find_map(|h| vfs::within(h, base, name)) else {
                continue;
            };
            let p = vfs::resolve_case(&p);
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

/// The files of a set (a folder or an `.lr2ss`), by sound and form; a
/// folder's with those of its `variant` (a subfolder) first. A sound is
/// taken from the variant if it has the sound in any form (its `clear`
/// before the set's `clear.loop`), else from the set; its `.enter` file
/// on its own.
pub fn set_files(
    path: &Path,
    variant: Option<&Path>,
    root: &Path,
) -> HashMap<(Sound, Form), PathBuf> {
    if vfs::is_dir(path) {
        let own = folder_files(path);
        let Some(var) = variant.map(folder_files) else {
            return own;
        };
        let mut out = HashMap::new();
        for s in Sound::ALL {
            let has = |f| var.contains_key(&(s, f));
            let from = match has(Form::Plain) || has(Form::Loop) {
                true => &var,
                false => &own,
            };
            for f in [Form::Plain, Form::Loop] {
                if let Some(p) = from.get(&(s, f)) {
                    out.insert((s, f), p.clone());
                }
            }
            if let Some(p) = var.get(&(s, Form::Enter)).or(own.get(&(s, Form::Enter))) {
                out.insert((s, Form::Enter), p.clone());
            }
        }
        return out;
    }
    let mut out = HashMap::new();
    let Ok(b) = crate::skin::read_text(path) else {
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
        if out.contains_key(&(s, Form::Plain)) {
            continue;
        }
        if let Some(p) = f.get(1).and_then(|raw| lr2_path(path, root, raw)) {
            out.insert((s, Form::Plain), p);
        }
    }
    out
}

/// A short tone gliding from `f0` to `f1` Hz with a quick decay.
pub(crate) fn tone(rate: u32, ms: f32, f0: f32, f1: f32, amp: f32) -> Sample {
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

/// A sound and the form of its file.
type Key = (Sound, Form);

/// A sound decoded, with its loudness if it is music.
type Decoded<S> = (S, Option<Loudness>);

/// A set read: its short sounds decoded (the music's loudness with them),
/// its long ones' files.
#[derive(Default)]
struct Loaded {
    sounds: Vec<(Key, Decoded<Arc<Sample>>)>,
    long: HashMap<Key, PathBuf>,
    /// Drawing again could give other sounds.
    random: bool,
    folders: Vec<PathBuf>,
}

/// The PCM a set's short sounds take together, and a screen's music each
/// time it is decoded to play: ten minutes of stereo at the mixer's `rate`
/// (110 MiB at 48 kHz). The short sounds are seconds long, a few minutes
/// all told; a screen's music a few minutes. However long its files say
/// they are, no more comes out of them.
fn set_budget(rate: u32) -> Budget {
    let bytes = (rate as usize).saturating_mul(4 * 10 * 60);
    Budget::new(bytes, vibebms_audio::decode::MAX_SOUND_SECS)
}

/// A sound file decoded at the mixer's `rate`, within `budget`.
fn decode(p: &Path, rate: u32, budget: &Budget) -> Result<Sample, String> {
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let b = crate::skin::read_media(p).map_err(|e| e.to_string())?;
    vibebms_audio::decode_bytes_within(b, &ext, rate, budget)
}

/// The chosen set's sounds, decoded off the main thread.
#[derive(Default)]
pub struct SystemSounds {
    /// The `config.skins` value loaded (`None`: nothing yet).
    value: Option<String>,
    samples: HashMap<(Sound, Form), Arc<Sample>>,
    /// The long sounds' files (`Sound::long`), and those being decoded to
    /// play.
    long: HashMap<Key, PathBuf>,
    decoding: Vec<(Key, Job<Option<Decoded<Sample>>>)>,
    rate: u32,
    /// The music's loudness (what loops, the decide jingle).
    loudness: HashMap<(Sound, Form), Loudness>,
    job: Option<Job<Loaded>>,
    /// The set was drawn at random, or with one of its variants: drawn
    /// again when the song select comes back.
    random: bool,
    /// A looping sound and a screen's entry sound asked for while the set
    /// was loading.
    want_loop: Option<Sound>,
    want_enter: Option<Sound>,
    /// The result's sounds playing.
    result: Vec<(Sound, Form)>,
    /// The folders of the set drawn (see `folders`).
    folders: Vec<PathBuf>,
}

/// One of `v`, at random.
fn draw<T: Clone>(v: &[T]) -> Option<T> {
    let n = i32::try_from(v.len()).ok().filter(|n| *n > 0)?;
    let k = vibebms_core::rng::JavaRandom::new(crate::platform::unix_ms()).next_int_bounded(n);
    v.get(k as usize).cloned()
}

impl SystemSounds {
    /// Load the set `value` names (a path relative to `root`; empty: the
    /// built-in one; [`RANDOM_SET`]) unless it is the one loaded.
    pub fn sync(&mut self, value: &str, root: &Path, rate: u32) {
        if self.value.as_deref() == Some(value) {
            return;
        }
        self.value = Some(value.to_string());
        self.load(root, rate);
    }

    /// The song select comes back: a set drawn at random, or one with
    /// variants, is drawn again.
    pub fn redraw(&mut self, root: &Path, rate: u32) {
        if self.random && self.job.is_none() {
            self.load(root, rate);
        }
    }

    fn load(&mut self, root: &Path, rate: u32) {
        let value = self.value.clone().unwrap_or_default();
        self.samples.clear();
        self.long.clear();
        self.decoding.clear();
        self.rate = rate;
        self.loudness.clear();
        self.random = false;
        self.folders.clear();
        if value.is_empty() {
            for s in Sound::ALL {
                if let Some(x) = built_in(s, rate) {
                    self.samples.insert((s, Form::Plain), Arc::new(x));
                }
            }
            self.job = None;
            return;
        }
        let root = root.to_path_buf();
        self.job = Some(Job::spawn("sound set", move |_| {
            let (path, many) = if value == RANDOM_SET {
                let sets = find_sets(&root);
                let Some(s) = draw(&sets) else {
                    return Loaded::default();
                };
                (s.path, sets.len() > 1)
            } else {
                (root.join(&value), false)
            };
            let vars = match vfs::is_dir(&path) {
                true => variants(&path),
                false => Vec::new(),
            };
            let variant = draw(&vars);
            // a set that takes the game down as it is decoded is set aside
            // at the next start, as a skin is (RANDOM too when it drew it)
            let mut noted = vec![guard::loading(&path)];
            if value == RANDOM_SET {
                noted.push(guard::loading(&root.join(RANDOM_SET)));
            }
            let budget = set_budget(rate);
            log::info!(
                "sound set {}{}",
                path.display(),
                variant
                    .as_ref()
                    .map_or(String::new(), |v| format!(" ({})", v.display()))
            );
            let mut sounds = Vec::new();
            let mut long = HashMap::new();
            for (k, p) in set_files(&path, variant.as_deref(), &root) {
                if k.0.long(k.1) {
                    long.insert(k, p);
                    continue;
                }
                match decode(&p, rate, &budget) {
                    Ok(x) => {
                        let music = k.0.music(k.1);
                        let l = music.then(|| crate::loudness::measure_sound(&x, rate));
                        sounds.push((k, (Arc::new(x), l)))
                    }
                    Err(e) => log::warn!("sound set: {}: {e}", p.display()),
                }
            }
            let folders = match vfs::is_dir(&path) {
                true => variant.into_iter().chain([path]).collect(),
                false => Vec::new(),
            };
            Loaded {
                sounds,
                long,
                random: many || vars.len() > 1,
                folders,
            }
        }));
    }

    /// Take the decoded sounds once they are ready (`norm`: the loudness
    /// the music is played at, if it is normalised).
    pub fn poll(&mut self, mixer: &Mixer, norm: Option<f32>) {
        self.poll_long(mixer, norm);
        let Some(set) = self.job.as_mut().and_then(Job::poll) else {
            return;
        };
        self.job = None;
        self.random = set.random;
        self.folders = set.folders;
        self.long = set.long;
        for (k, (x, l)) in set.sounds {
            self.samples.insert(k, x);
            if let Some(l) = l {
                self.loudness.insert(k, l);
            }
        }
        if let Some(s) = self.want_enter.take() {
            self.play_enter(mixer, s, norm);
        }
        if let Some(s) = self.want_loop.take() {
            self.play(mixer, s, norm);
        }
    }

    /// The volume `s` plays at: its music brought to `norm` LUFS.
    fn volume(&self, k: (Sound, Form), norm: Option<f32>) -> f32 {
        match (norm, self.loudness.get(&k)) {
            (Some(t), Some(l)) => crate::loudness::gain(l, t),
            _ => 1.0,
        }
    }

    /// Normalising changed: the music playing follows.
    pub fn renormalize(&self, mixer: &Mixer, norm: Option<f32>) {
        for &(s, f) in self.loudness.keys() {
            mixer.set_volume(s.key(f), self.volume((s, f), norm));
        }
    }

    /// The folders of the folder set drawn, to look for its other files
    /// in (GUIDE SE): its variant's, then its own (none for an LR2 or the
    /// built-in set, or while it loads).
    pub fn folders(&self) -> Vec<PathBuf> {
        self.folders.clone()
    }

    pub fn has(&self, s: Sound) -> bool {
        self.found((s, Form::Plain))
    }

    fn found(&self, k: Key) -> bool {
        self.samples.contains_key(&k) || self.long.contains_key(&k)
    }

    /// Play `k`; a long sound once it is decoded. False: the set has no
    /// such file.
    fn start(&mut self, mixer: &Mixer, k: Key, norm: Option<f32>) -> bool {
        if let Some(x) = self.samples.get(&k) {
            self.play_sample(mixer, k, x, norm);
            return true;
        }
        let Some(p) = self.long.get(&k).cloned() else {
            return false;
        };
        self.decoding.retain(|(d, _)| *d != k);
        // (music is measured once)
        let measure = k.0.music(k.1) && !self.loudness.contains_key(&k);
        let rate = self.rate;
        let job = Job::spawn("sound set music", move |_| {
            match decode(&p, rate, &set_budget(rate)) {
                Ok(x) => {
                    let l = measure.then(|| crate::loudness::measure_sound(&x, rate));
                    Some((x, l))
                }
                Err(e) => {
                    log::warn!("sound set: {}: {e}", p.display());
                    None
                }
            }
        });
        self.decoding.push((k, job));
        true
    }

    fn play_sample(&self, mixer: &Mixer, k: Key, x: &Arc<Sample>, norm: Option<f32>) {
        let o = PlayOptions {
            key: k.0.key(k.1),
            bus: Bus::System,
            volume: self.volume(k, norm),
            looping: k.0.looping(k.1),
            ..Default::default()
        };
        mixer.play(x, o);
    }

    /// The long sounds decoded since the last call start; only the mixer
    /// holds them (they are freed once they stop).
    fn poll_long(&mut self, mixer: &Mixer, norm: Option<f32>) {
        let mut done = Vec::new();
        self.decoding.retain_mut(|(k, job)| match job.poll() {
            Some(x) => {
                done.extend(x.map(|x| (*k, x)));
                false
            }
            None => true,
        });
        for (k, (x, l)) in done {
            if let Some(l) = l {
                self.loudness.insert(k, l);
            }
            self.play_sample(mixer, k, &Arc::new(x), norm);
        }
    }

    /// Stop `k`, playing or still being decoded.
    fn stop_key(&mut self, mixer: &Mixer, k: Key) {
        self.decoding.retain(|(d, _)| *d != k);
        mixer.stop(k.0.key(k.1));
    }

    /// Play `s`: the select music as `select.loop`, else `select` (both
    /// over and over); any other sound once.
    pub fn play(&mut self, mixer: &Mixer, s: Sound, norm: Option<f32>) {
        let started = (s == Sound::Select && self.start(mixer, (s, Form::Loop), norm))
            || self.start(mixer, (s, Form::Plain), norm);
        if !started && s.looping(Form::Plain) && self.job.is_some() {
            self.want_loop = Some(s);
        }
    }

    /// A screen comes: its `<name>.enter` sound, once.
    pub fn play_enter(&mut self, mixer: &Mixer, s: Sound, norm: Option<f32>) {
        if !self.start(mixer, (s, Form::Enter), norm) && self.job.is_some() {
            self.want_enter = Some(s);
        }
    }

    /// Stop `s` (its plain and `.loop` files).
    pub fn stop(&mut self, mixer: &Mixer, s: Sound) {
        if self.want_loop == Some(s) {
            self.want_loop = None;
        }
        // (a screen's entry sound plays out)
        for f in [Form::Plain, Form::Loop] {
            self.stop_key(mixer, (s, f));
        }
    }

    /// A result comes: of `sounds` (the outcome's, then those it falls back
    /// on: `aaa`, `clear`), the first's `.enter` sound found, and the music:
    /// the first's `.loop` (over and over) or plain file (once) found. With
    /// an `.enter` sound, only a `.loop` is music (a plain one is not played
    /// over it).
    pub fn play_result(&mut self, mixer: &Mixer, sounds: &[Sound], norm: Option<f32>) {
        self.stop_result(mixer);
        let found = |f: Form| sounds.iter().map(|&s| (s, f)).find(|&k| self.found(k));
        let enter = found(Form::Enter);
        let music = sounds.iter().find_map(|&s| {
            let forms: &[Form] = match enter {
                Some(_) => &[Form::Loop],
                None => &[Form::Loop, Form::Plain],
            };
            forms.iter().map(|&f| (s, f)).find(|&k| self.found(k))
        });
        for k in enter.into_iter().chain(music) {
            self.start(mixer, k, norm);
            self.result.push(k);
        }
    }

    /// The result ends: its sounds stop.
    pub fn stop_result(&mut self, mixer: &Mixer) {
        for k in std::mem::take(&mut self.result) {
            self.stop_key(mixer, k);
        }
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
        // (files outside the folders sets are kept in are not a set's)
        std::fs::write(root.join("secret.wav"), b"x").unwrap();
        let secret = root.join("secret.wav");
        std::fs::write(
            snd.join("LR2/Sound/e2e.lr2ss"),
            format!(
                "#INFORMATION,10,E2E,me,\n#SELECT,LR2files\\Bgm\\e2e\\select.wav,\n\
                 #SELECT,LR2files\\Bgm\\other.wav,\n#DECIDE,LR2files\\Bgm\\e2e\\missing.wav,\n\
                 #DECIDE,..\\..\\secret.wav,\n#CLEAR,LR2files\\..\\..\\..\\secret.wav,\n\
                 #FAIL,{},\n#STOP,\\\\host\\share\\a.wav,\n",
                secret.display()
            ),
        )
        .unwrap();
        let sets = find_sets(&root);
        let names: Vec<&str> = sets.iter().map(|s| s.name.as_str()).collect();
        // LR2's BGM folder holds beatoraja's select.wav: a set too
        assert_eq!(names, vec!["e2e", "E2E", "raja"]);
        assert_eq!(sets[1].author, "me");
        let raja = set_files(&snd.join("raja"), None, &root);
        assert_eq!(raja.len(), 2);
        assert!(raja[&(Sound::Decide, Form::Plain)].ends_with("decide.ogg"));
        assert!(raja[&(Sound::FolderOpen, Form::Plain)].ends_with("F-OPEN.wav"));
        let lr2 = set_files(&snd.join("LR2/Sound/e2e.lr2ss"), None, &root);
        // chosen like a skin of type 10
        let h = crate::skin::header(&snd.join("LR2/Sound/e2e.lr2ss"), &root.join("skins"));
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(lr2.len(), 1);
        assert!(lr2[&(Sound::Select, Form::Plain)].ends_with("Bgm/e2e/select.wav"));
        assert_eq!(h, Some((crate::skin::SkinType::SoundSet, "E2E".into())));
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    /// bmz-player's conventions: `.loop` and `.enter` files, the rank
    /// sounds, variants in subfolders taking what they lack from the set.
    #[test]
    fn loops_entries_and_variants() {
        let root = std::env::temp_dir().join(format!("vibebms-ssetv-{}", std::process::id()));
        let set = root.join("sounds/bmz");
        for f in [
            "select.ogg",
            "select.enter.wav",
            "decide.ogg",
            "clear.loop.ogg",
            "fail.wav",
            "AAA.WAV",
            "style-a/select.ogg",
            "style-a/clear.ogg",
            "style-b/aaa.loop.wav",
            "style-b/clear.enter.ogg",
            "notes/readme.txt",
        ] {
            touch(&set.join(f));
        }
        // the variants are the set's, not sets of their own
        let names: Vec<String> = find_sets(&root).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["bmz"]);
        assert_eq!(variants(&set), [set.join("style-a"), set.join("style-b")]);
        let name = |m: &HashMap<(Sound, Form), PathBuf>, s, f| {
            m.get(&(s, f))
                .map(|p| p.strip_prefix(&set).unwrap().display().to_string())
        };
        use Form::*;
        use Sound::*;
        let own = set_files(&set, None, &root);
        assert_eq!(
            name(&own, Select, Enter).as_deref(),
            Some("select.enter.wav")
        );
        assert_eq!(
            name(&own, ResultClear, Loop).as_deref(),
            Some("clear.loop.ogg")
        );
        assert_eq!(name(&own, ResultClear, Plain), None);
        assert_eq!(name(&own, ResultAaa, Plain).as_deref(), Some("AAA.WAV"));
        assert_eq!(own.len(), 6);
        // style-a: its select and its clear (before the set's clear.loop);
        // the rest the set's
        let a = set_files(&set, Some(&set.join("style-a")), &root);
        assert_eq!(
            name(&a, Select, Plain).as_deref(),
            Some("style-a/select.ogg")
        );
        assert_eq!(name(&a, Select, Enter).as_deref(), Some("select.enter.wav"));
        assert_eq!(
            name(&a, ResultClear, Plain).as_deref(),
            Some("style-a/clear.ogg")
        );
        assert_eq!(name(&a, ResultClear, Loop), None);
        assert_eq!(name(&a, Decide, Plain).as_deref(), Some("decide.ogg"));
        // style-b: its aaa.loop (the set's aaa not with it), its clear.enter
        let b = set_files(&set, Some(&set.join("style-b")), &root);
        assert_eq!(
            name(&b, ResultAaa, Loop).as_deref(),
            Some("style-b/aaa.loop.wav")
        );
        assert_eq!(name(&b, ResultAaa, Plain), None);
        assert_eq!(
            name(&b, ResultClear, Loop).as_deref(),
            Some("clear.loop.ogg")
        );
        assert_eq!(
            name(&b, ResultClear, Enter).as_deref(),
            Some("style-b/clear.enter.ogg")
        );
        // other sounds have no .loop or .enter files
        assert_eq!(Sound::Decide.file_of(Form::Loop), None);
        std::fs::remove_dir_all(&root).ok();
    }

    fn mixer() -> Mixer {
        Mixer::new(&vibebms_audio::AudioConfig {
            buffer_frames: 0,
            sample_rate: 44100,
            master: 0.0,
            key_volume: 0.0,
            bgm_volume: 0.0,
            system_volume: 0.0,
            max_voices: 16,
            #[cfg(windows)]
            exclusive: false,
        })
    }

    /// The result's music: the outcome's `.loop`, else its plain file,
    /// then clear's; with an `.enter` sound only a `.loop` is music.
    #[test]
    fn result_music() {
        use Form::*;
        use Sound::*;
        let m = mixer();
        let with = |keys: &[(Sound, Form)]| {
            let mut s = SystemSounds::default();
            for k in keys {
                s.samples.insert(*k, Arc::new(Sample::default()));
            }
            s
        };
        let aaa = [ResultAaa, ResultClear];
        let mut s = with(&[(ResultAaa, Plain), (ResultClear, Loop)]);
        s.play_result(&m, &aaa, None);
        assert_eq!(s.result, [(ResultAaa, Plain)]);
        let mut s = with(&[(ResultAaa, Plain), (ResultAaa, Loop), (ResultClear, Loop)]);
        s.play_result(&m, &aaa, None);
        assert_eq!(s.result, [(ResultAaa, Loop)]);
        // no aaa: clear (not aa)
        let mut s = with(&[(ResultAa, Plain), (ResultClear, Plain)]);
        s.play_result(&m, &aaa, None);
        assert_eq!(s.result, [(ResultClear, Plain)]);
        // an entry sound, with a .loop but not over a plain file
        let mut s = with(&[
            (ResultClear, Enter),
            (ResultAaa, Plain),
            (ResultClear, Loop),
        ]);
        s.play_result(&m, &aaa, None);
        assert_eq!(s.result, [(ResultClear, Enter), (ResultClear, Loop)]);
        let mut s = with(&[(ResultAaa, Enter), (ResultAaa, Plain)]);
        s.play_result(&m, &aaa, None);
        assert_eq!(s.result, [(ResultAaa, Enter)]);
        s.stop_result(&m);
        assert!(s.result.is_empty());
        // looping: the select music in any form, the .loop files
        assert!(Select.looping(Plain) && ResultClear.looping(Loop) && !ResultClear.looping(Plain));
        assert!(Decide.music(Plain) && !ResultClear.music(Plain) && !Select.music(Enter));
        // one voice key each
        let keys: std::collections::HashSet<u64> = Sound::ALL
            .iter()
            .flat_map(|s| Form::ALL.map(|f| s.key(f)))
            .collect();
        assert_eq!(keys.len(), Sound::ALL.len() * 3);
        assert!(keys.iter().all(|k| *k < u64::MAX - 2));
    }

    /// RANDOM: a set drawn among those found, again when asked.
    #[test]
    fn a_set_drawn_at_random() {
        let root = std::env::temp_dir().join(format!("vibebms-ssetr-{}", std::process::id()));
        touch(&root.join("sounds/one/decide.wav"));
        touch(&root.join("sounds/two/decide.wav"));
        let mut s = SystemSounds::default();
        let m = mixer();
        s.sync(RANDOM_SET, &root, 44100);
        for _ in 0..500 {
            s.poll(&m, None);
            if s.job.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // (the files do not decode: nothing is loaded, but two sets
        // can be drawn from)
        assert!(s.job.is_none() && s.random);
        // its folder, for GUIDE SE
        let f = s.folders();
        assert!(
            f.len() == 1 && (f[0].ends_with("one") || f[0].ends_with("two")),
            "{f:?}"
        );
        s.redraw(&root, 44100);
        assert!(s.job.is_some());
        let info = set_info(&root.join(RANDOM_SET)).unwrap();
        assert_eq!(info.name, "RANDOM");
        std::fs::remove_dir_all(&root).ok();
    }

    /// A set's short sounds take one budget together, however long their
    /// files say they are.
    #[test]
    fn a_sets_sounds_share_a_budget() {
        let root = std::env::temp_dir().join(format!("vibebms-ssetb-{}", std::process::id()));
        // a WAV that states 1 Hz: its 4000 frames are an hour
        let mut wav = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\
            \x01\0\0\0\x02\0\0\0\x02\0\x10\0data\x40\x1f\0\0"
            .to_vec();
        wav.resize(wav.len() + 8000, 1);
        let set = root.join("sounds/set");
        std::fs::create_dir_all(&set).unwrap();
        for f in ["o-change.wav", "o-open.wav", "o-close.wav", "f-open.wav"] {
            std::fs::write(set.join(f), &wav).unwrap();
        }
        let mut s = SystemSounds::default();
        let m = mixer();
        let rate = 100;
        s.sync("sounds/set", &root, rate);
        for _ in 0..2000 {
            s.poll(&m, None);
            if s.job.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        std::fs::remove_dir_all(&root).ok();
        let bytes: usize = s.samples.values().map(|x| x.data.len() * 2).sum();
        assert_eq!(s.samples.len(), 4);
        assert!(bytes <= rate as usize * 4 * 10 * 60, "{bytes}");
    }

    /// A screen's music is decoded when it plays, not kept; the short
    /// sounds are.
    #[test]
    fn music_is_decoded_to_play() {
        let root = std::env::temp_dir().join(format!("vibebms-ssetm-{}", std::process::id()));
        // a WAV of no frames, 16-bit mono at 44100 Hz
        let wav = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\
            \x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data\0\0\0\0";
        let set = root.join("sounds/set");
        std::fs::create_dir_all(&set).unwrap();
        for f in ["select.wav", "decide.wav", "clear.loop.wav"] {
            std::fs::write(set.join(f), wav).unwrap();
        }
        let mut s = SystemSounds::default();
        let m = mixer();
        let wait = |s: &mut SystemSounds| {
            for _ in 0..500 {
                s.poll(&m, None);
                if s.job.is_none() && s.decoding.is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        };
        s.sync("sounds/set", &root, 44100);
        wait(&mut s);
        assert!(s.has(Sound::Decide) && s.has(Sound::Select));
        assert!(s.samples.contains_key(&(Sound::Decide, Form::Plain)));
        assert_eq!(s.samples.len(), 1);
        s.play(&m, Sound::Select, None);
        assert_eq!(s.decoding.len(), 1);
        wait(&mut s);
        assert!(s.decoding.is_empty() && s.samples.len() == 1);
        // stopped before it is decoded: not played
        s.play_result(&m, &[Sound::ResultClear], None);
        assert_eq!(s.decoding.len(), 1);
        s.stop_result(&m);
        assert!(s.decoding.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn built_in_set() {
        let mut s = SystemSounds::default();
        s.sync("", Path::new("/nonexistent"), 44100);
        assert!(s.has(Sound::Decide) && s.has(Sound::OptionChange));
        assert!(!s.has(Sound::Select));
    }
}
