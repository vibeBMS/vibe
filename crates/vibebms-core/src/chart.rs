//! Format-independent chart model shared by the BMS and BMSON decoders.

use crate::mode::PlayMode;
use crate::timing::TimingMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Long note behaviour. `#LNMODE` 1/2/3 in BMS, `t` in BMSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LnType {
    /// Only the start is judged; releasing early is a BAD (LR2 / beatoraja "LN").
    #[default]
    Ln,
    /// Charge note: start and end are judged separately (IIDX style).
    Cn,
    /// Hell charge note: CN plus gauge drain/gain while holding the body.
    Hcn,
}

/// Index into [`Chart::sounds`]. `NO_SOUND` means silence.
pub type SoundId = u32;
pub const NO_SOUND: SoundId = u32::MAX;

#[derive(Clone, Debug, PartialEq)]
pub enum NoteKind {
    Normal,
    Long {
        end_time: i64,
        end_beat: f64,
        end_pos: f64,
        end_sound: SoundId,
        /// `None` = decided by the player's LN mode preference.
        ln_type: Option<LnType>,
    },
    /// Land mine; damage in gauge percent.
    Mine {
        damage: f32,
    },
    /// Invisible note (BMS channels 3x/4x): only provides keysounds.
    Invisible,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub lane: u8,
    /// Time in microseconds from chart start.
    pub time: i64,
    pub beat: f64,
    /// Scroll position (beats integrated over `#SCROLL`) used for rendering.
    pub pos: f64,
    pub sound: SoundId,
    pub kind: NoteKind,
}

impl Note {
    pub fn is_long(&self) -> bool {
        matches!(self.kind, NoteKind::Long { .. })
    }
    pub fn is_playable(&self) -> bool {
        matches!(self.kind, NoteKind::Normal | NoteKind::Long { .. })
    }
    pub fn end_time(&self) -> i64 {
        match self.kind {
            NoteKind::Long { end_time, .. } => end_time,
            _ => self.time,
        }
    }
}

/// A playable slice of an audio file (BMS uses whole files, BMSON slices).
#[derive(Clone, Debug, PartialEq)]
pub struct SoundSlice {
    pub file: u32,
    /// Start offset inside the file in microseconds.
    pub start: i64,
    /// End offset (exclusive); `None` = until the end of the file.
    pub end: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BgmEvent {
    pub time: i64,
    pub sound: SoundId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BgaLayer {
    Base,
    Layer,
    Layer2,
    Poor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BgaEvent {
    pub time: i64,
    pub layer: BgaLayer,
    /// Index into [`Chart::images`], or `None` to clear the layer.
    pub image: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BarLine {
    pub time: i64,
    pub pos: f64,
}

/// How the judge rank was specified, needed to convert to a window scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum JudgeRank {
    /// `#RANK 0..=4` (VERY HARD .. VERY EASY)
    Rank(i32),
    /// `#DEFEXRANK n` (percent of NORMAL)
    DefExRank(f64),
    /// bmson `judge_rank` (percent, 100 = EASY in beatoraja terms)
    Bmson(f64),
}

impl Default for JudgeRank {
    fn default() -> Self {
        JudgeRank::Rank(2)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChartMeta {
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub subartist: String,
    pub genre: String,
    pub level: String,
    /// `#DIFFICULTY` 1..5 (BEGINNER..INSANE), 0 = undefined.
    pub difficulty: i32,
    pub mode: Option<PlayMode>,
    pub initial_bpm: f64,
    pub min_bpm: f64,
    pub max_bpm: f64,
    /// BPM that covers the longest play time (used as hi-speed base).
    pub main_bpm: f64,
    pub judge_rank: JudgeRank,
    /// `#TOTAL`; `None` = use the default formula.
    pub total: Option<f64>,
    pub ln_type: Option<LnType>,
    pub stagefile: String,
    pub banner: String,
    pub backbmp: String,
    pub preview: String,
    pub md5: String,
    pub sha256: String,
    pub path: Option<PathBuf>,
    /// Selections made for `#RANDOM` blocks, so the same pattern can be replayed.
    pub random_selections: Vec<u32>,
    pub has_random: bool,
    pub has_bga: bool,
    pub has_stop: bool,
    pub has_mine: bool,
    pub has_long: bool,
    pub has_invisible: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Chart {
    pub meta: ChartMeta,
    /// Audio file names relative to the chart directory.
    pub sound_files: Vec<String>,
    pub sounds: Vec<SoundSlice>,
    /// Image / video file names relative to the chart directory.
    pub images: Vec<String>,
    /// All notes (playable, invisible, mines) sorted by time then lane.
    pub notes: Vec<Note>,
    pub bgm: Vec<BgmEvent>,
    pub bga: Vec<BgaEvent>,
    pub bar_lines: Vec<BarLine>,
    pub timing: TimingMap,
}

impl Chart {
    pub fn mode(&self) -> PlayMode {
        self.meta.mode.unwrap_or(PlayMode::Beat7K)
    }

    /// Number of judgements a full play produces (LN counts once, CN/HCN twice).
    pub fn total_judgements(&self, default_ln: LnType) -> usize {
        self.notes
            .iter()
            .map(|n| match &n.kind {
                NoteKind::Normal => 1,
                NoteKind::Long { ln_type, .. } => match ln_type.unwrap_or(default_ln) {
                    LnType::Ln => 1,
                    LnType::Cn | LnType::Hcn => 2,
                },
                _ => 0,
            })
            .sum()
    }

    /// Number of playable notes (LN counted once), as shown in song select.
    pub fn note_count(&self) -> usize {
        self.notes.iter().filter(|n| n.is_playable()).count()
    }

    /// Chart length in microseconds (last note, BGM or BGA event).
    pub fn length(&self) -> i64 {
        let n = self.notes.iter().map(|n| n.end_time()).max().unwrap_or(0);
        let b = self.bgm.last().map(|b| b.time).unwrap_or(0);
        n.max(b)
    }

    /// `#TOTAL` or beatoraja's default: `max(260, 7.605 n / (0.01 n + 6.5))`.
    pub fn total(&self, default_ln: LnType) -> f64 {
        let n = self.total_judgements(default_ln).max(1) as f64;
        match self.meta.total {
            Some(t) if t > 0.0 => t,
            _ => (7.605 * n / (0.01 * n + 6.5)).max(260.0),
        }
    }

    /// Recompute min/max/main BPM from the timing map.
    pub fn compute_bpm_stats(&mut self) {
        let end = self.length();
        let (min, max, main) = self.timing.bpm_stats(end);
        self.meta.min_bpm = min;
        self.meta.max_bpm = max;
        self.meta.main_bpm = main;
    }
}
