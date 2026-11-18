//! Format-independent chart model shared by the BMS and BMSON decoders.

use crate::gauge::GaugePreset;
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

impl LnType {
    pub const ALL: [LnType; 3] = [LnType::Ln, LnType::Cn, LnType::Hcn];

    pub fn label(self) -> &'static str {
        match self {
            LnType::Ln => "LN",
            LnType::Cn => "CN",
            LnType::Hcn => "HCN",
        }
    }
}

/// The player's LN mode: the type long notes the chart leaves untyped play
/// as (AUTO), or, with `force`, every long note (FORCE: the chart's own
/// types overridden).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LnPref {
    pub ty: LnType,
    pub force: bool,
}

impl From<LnType> for LnPref {
    fn from(ty: LnType) -> LnPref {
        LnPref { ty, force: false }
    }
}

impl LnPref {
    /// AUTO LN, CN, HCN, then FORCE LN, CN, HCN (the order the LN MODE
    /// option steps through).
    pub const ALL: [LnPref; 6] = [
        LnPref::auto(LnType::Ln),
        LnPref::auto(LnType::Cn),
        LnPref::auto(LnType::Hcn),
        LnPref::forced(LnType::Ln),
        LnPref::forced(LnType::Cn),
        LnPref::forced(LnType::Hcn),
    ];

    pub const fn auto(ty: LnType) -> LnPref {
        LnPref { ty, force: false }
    }

    pub const fn forced(ty: LnType) -> LnPref {
        LnPref { ty, force: true }
    }

    /// "CN", "FORCE CN".
    pub fn label(self) -> String {
        match self.force {
            true => format!("FORCE {}", self.ty.label()),
            false => self.ty.label().into(),
        }
    }

    /// The type a long note the chart gives type `typed` (`None`: none)
    /// plays as.
    pub fn plays(self, typed: Option<LnType>) -> LnType {
        match typed {
            Some(t) if !self.force => t,
            _ => self.ty,
        }
    }
}

/// Which long notes a chart has: ones of no type (played as the player's
/// LN mode) and ones typed by the chart (`#LNMODE`, bmson `t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LnProfile {
    pub undefined: bool,
    /// Typed LN, CN, HCN.
    pub typed: [bool; 3],
}

impl LnProfile {
    pub fn has_long(self) -> bool {
        self.undefined || self.typed.iter().any(|t| *t)
    }

    /// The LN mode a play's score, replay and IR entry are filed under:
    /// without long notes LN (0); with FORCE the forced type; when every
    /// long note has its type the chart plays the same whatever the
    /// player's AUTO mode, so the strongest type the chart has (HCN > CN >
    /// LN; with one type, that type); else the player's mode.
    pub fn score_ln(self, pref: impl Into<LnPref>) -> LnType {
        let pref = pref.into();
        if !self.has_long() {
            LnType::Ln
        } else if self.undefined || pref.force {
            pref.ty
        } else if self.typed[2] {
            LnType::Hcn
        } else if self.typed[1] {
            LnType::Cn
        } else {
            LnType::Ln
        }
    }

    /// Whether the long notes play the same under `a` and `b`.
    pub fn plays_alike(self, a: LnPref, b: LnPref) -> bool {
        (!self.undefined || a.plays(None) == b.plays(None))
            && LnType::ALL
                .into_iter()
                .filter(|t| self.typed[*t as usize])
                .all(|t| a.plays(Some(t)) == b.plays(Some(t)))
    }

    /// Packed in a byte (for the song cache): bit 0 undefined, 1..3 typed.
    pub fn bits(self) -> u8 {
        let mut b = self.undefined as u8;
        for (i, t) in self.typed.iter().enumerate() {
            b |= (*t as u8) << (i + 1);
        }
        b
    }

    pub fn from_bits(b: u8) -> LnProfile {
        LnProfile {
            undefined: b & 1 != 0,
            typed: std::array::from_fn(|i| b & (2 << i) != 0),
        }
    }
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
    /// `#RANK` with a value that is not a number: NORMAL, as a missing
    /// one, except under the LR2 judge, which reads it as 0, VERY HARD.
    NoNumber,
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
    /// bmson `total`: a percentage of the default TOTAL (which depends on
    /// the gauge rules); `total` holds it resolved with beatoraja's.
    #[serde(default)]
    pub total_rate: Option<f64>,
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

    /// Number of judgements a full play produces (LN counts once, CN/HCN
    /// twice), of one player's lanes (`None`: every lane): a side of a
    /// battle chart counts as its single play chart. Notes on lanes the
    /// mode does not have are not played, so not counted.
    pub fn total_judgements_of(&self, default_ln: LnType, player: Option<usize>) -> usize {
        let mode = self.mode();
        let lanes = mode.lane_count();
        self.notes
            .iter()
            .filter(|n| (n.lane as usize) < lanes)
            .filter(|n| player.is_none_or(|p| mode.player_of_lane(n.lane as usize) == p))
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

    /// The long notes the chart has, by type.
    pub fn ln_profile(&self) -> LnProfile {
        let mut p = LnProfile::default();
        for n in &self.notes {
            if let NoteKind::Long { ln_type, .. } = n.kind {
                match ln_type {
                    None => p.undefined = true,
                    Some(t) => p.typed[t as usize] = true,
                }
            }
        }
        p
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
        self.total_of(default_ln, GaugePreset::Beatoraja, None)
    }

    /// TOTAL under a gauge preset for one player's lanes (as
    /// `total_judgements_of`): `#TOTAL`, else the preset's default (LR2:
    /// `floor(160 + (n + clamp(n - 400, 0, 200)) × 0.16)`, otherwise
    /// beatoraja's). LR2 reads `#TOTAL` as an integer, fraction dropped,
    /// and takes its default below 1 (S-OLR2 3.3). A bmson `total` is a
    /// percentage of that default.
    pub fn total_of(&self, default_ln: LnType, preset: GaugePreset, player: Option<usize>) -> f64 {
        let judgements = self.total_judgements_of(default_ln, player);
        let n = judgements.max(1) as f64;
        let lr2 = preset == GaugePreset::Lr2;
        let default = if lr2 {
            // in integers: 0.16 is not exact in binary
            let n = judgements as i64;
            (160 + 4 * (n + (n - 400).clamp(0, 200)) / 25) as f64
        } else {
            (7.605 * n / (0.01 * n + 6.5)).max(260.0)
        };
        if let Some(r) = self.meta.total_rate {
            return if r > 0.0 {
                r / 100.0 * default
            } else {
                default
            };
        }
        match self.meta.total {
            Some(t) if lr2 && t >= 1.0 => t.trunc(),
            Some(t) if !lr2 && t > 0.0 => t,
            _ => default,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_ln_follows_the_chart_when_it_types_its_long_notes() {
        let none = LnProfile::default();
        assert_eq!(none.score_ln(LnType::Hcn), LnType::Ln);
        let undefined = LnProfile {
            undefined: true,
            typed: [false, true, false],
        };
        assert_eq!(undefined.score_ln(LnType::Hcn), LnType::Hcn);
        let cn = LnProfile {
            undefined: false,
            typed: [false, true, false],
        };
        assert_eq!(cn.score_ln(LnType::Ln), LnType::Cn);
        assert_eq!(cn.score_ln(LnType::Hcn), LnType::Cn);
        let mixed = LnProfile {
            undefined: false,
            typed: [true, true, false],
        };
        assert_eq!(mixed.score_ln(LnType::Ln), LnType::Cn);
        for p in [none, undefined, cn, mixed] {
            assert_eq!(LnProfile::from_bits(p.bits()), p);
        }
        // FORCE: the forced type, but LN without long notes
        let force = LnPref::forced(LnType::Hcn);
        assert_eq!(
            [none, undefined, cn, mixed].map(|p| p.score_ln(force)),
            [LnType::Ln, LnType::Hcn, LnType::Hcn, LnType::Hcn]
        );
        assert_eq!(cn.score_ln(LnPref::forced(LnType::Ln)), LnType::Ln);
    }

    #[test]
    fn forced_long_notes_play_as_forced() {
        let (auto_cn, force_cn) = (LnPref::auto(LnType::Cn), LnPref::forced(LnType::Cn));
        assert_eq!(auto_cn.plays(Some(LnType::Hcn)), LnType::Hcn);
        assert_eq!(force_cn.plays(Some(LnType::Hcn)), LnType::Cn);
        assert_eq!(auto_cn.plays(None), LnType::Cn);
        assert_eq!(LnPref::ALL.map(|p| p.label())[3], "FORCE LN");
        // a chart typed CN plays alike under AUTO (any) and FORCE CN, not
        // under FORCE LN; untyped long notes go by the type alone
        let cn = LnProfile {
            undefined: false,
            typed: [false, true, false],
        };
        let auto_ln = LnPref::auto(LnType::Ln);
        assert!(cn.plays_alike(auto_ln, force_cn));
        assert!(!cn.plays_alike(auto_ln, LnPref::forced(LnType::Ln)));
        let both = LnProfile {
            undefined: true,
            ..cn
        };
        assert!(both.plays_alike(auto_cn, force_cn));
        assert!(!both.plays_alike(LnPref::auto(LnType::Hcn), LnPref::forced(LnType::Hcn)));
        assert!(LnProfile::default().plays_alike(auto_ln, force_cn));
    }

    #[test]
    fn default_total_by_gauge_preset() {
        // 1000 notes, no #TOTAL
        let src = "#BPM 120\n#WAV01 a.wav\n#00111:01\n";
        let mut c = crate::bms::parse_bms(src.as_bytes(), "bme", &Default::default()).unwrap();
        let n = c.notes[0].clone();
        c.notes = (0..1000).map(|_| n.clone()).collect();
        assert!((c.total_of(LnType::Ln, GaugePreset::Lr2, None) - 352.0).abs() < 1e-9);
        let b = c.total_of(LnType::Ln, GaugePreset::Beatoraja, None);
        assert!((b - 7605.0 / 16.5).abs() < 1e-9);
        // a written #TOTAL wins
        c.meta.total = Some(300.0);
        assert_eq!(c.total_of(LnType::Ln, GaugePreset::Lr2, None), 300.0);
        // bmson: a percentage of the preset's default
        c.meta.total_rate = Some(50.0);
        assert!((c.total_of(LnType::Ln, GaugePreset::Lr2, None) - 176.0).abs() < 1e-9);
        // 300 notes: 160 + 300 x 0.16
        c.meta.total_rate = None;
        c.meta.total = None;
        c.notes.truncate(300);
        assert!((c.total_of(LnType::Ln, GaugePreset::Lr2, None) - 208.0).abs() < 1e-9);
        // LR2: whole numbers (7 notes: 161.12), #TOTAL's fraction dropped,
        // below 1 the default (S-OLR2 3.3); beatoraja keeps them
        c.notes.truncate(7);
        let lr2 = |c: &Chart| c.total_of(LnType::Ln, GaugePreset::Lr2, None);
        assert_eq!(lr2(&c), 161.0);
        for (t, want) in [(250.9, 250.0), (1.0, 1.0), (0.5, 161.0)] {
            c.meta.total = Some(t);
            assert_eq!(lr2(&c), want);
            assert_eq!(c.total_of(LnType::Ln, GaugePreset::Beatoraja, None), t);
        }
    }

    #[test]
    fn lnmode_types_every_long_note() {
        let src = "#BPM 120\n#LNMODE 2\n#WAV01 a.wav\n#00151:0101\n#00111:01\n";
        let c = crate::bms::parse_bms(src.as_bytes(), "bme", &Default::default()).unwrap();
        let p = c.ln_profile();
        assert!(!p.undefined && p.typed == [false, true, false]);
        assert_eq!(p.score_ln(LnType::Ln), LnType::Cn);
        let src = "#BPM 120\n#WAV01 a.wav\n#00151:0101\n";
        let c = crate::bms::parse_bms(src.as_bytes(), "bme", &Default::default()).unwrap();
        assert_eq!(c.ln_profile().score_ln(LnType::Hcn), LnType::Hcn);
    }
}
