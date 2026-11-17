//! BMS / BME / BML / PMS decoder.
//!
//! Supported:
//! * headers: TITLE, SUBTITLE, ARTIST, SUBARTIST, GENRE, BPM, PLAYLEVEL, RANK,
//!   DEFEXRANK, TOTAL, DIFFICULTY, STAGEFILE, BANNER, BACKBMP, PREVIEW,
//!   LNTYPE, LNOBJ, LNMODE, BASE, WAVxx, BMPxx, BPMxx/EXBPMxx, STOPxx, SCROLLxx
//! * control flow: RANDOM / SETRANDOM / IF / ELSEIF / ELSE / ENDIF / ENDRANDOM,
//!   SWITCH / SETSWITCH / CASE / SKIP / DEF / ENDSW (nested)
//! * channels: 01 BGM, 02 measure length, 03/08 BPM, 09 STOP, SC scroll,
//!   04/06/07/0A BGA, 1x/2x notes, 3x/4x invisible, 5x/6x long notes,
//!   Dx/Ex mines
//!
//! In three stages with plain data between them: [`source`] reads the
//! headers and channel lines (control flow applied, [`flow`]), [`place`]
//! puts each channel's objects at their beats, [`build`] makes the notes,
//! long notes and times of the chart.

mod build;
mod flow;
mod place;
mod source;

use crate::chart::Chart;
use crate::encoding::decode_text;
use crate::rng::JavaRandom;
use crate::ChartError;
use md5::{Digest, Md5};
use sha2::Sha256;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct ParseOptions {
    /// Seed for `#RANDOM`.
    pub random_seed: i64,
    /// Force the outcome of each `#RANDOM` in order (e.g. from a replay).
    pub random_selections: Option<Vec<u32>>,
}

pub fn md5_hex(bytes: &[u8]) -> String {
    hex(&Md5::digest(bytes))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        let _ = write!(s, "{x:02x}");
    }
    s
}

fn base36(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u32),
        b'A'..=b'Z' => Some((c - b'A') as u32 + 10),
        b'a'..=b'z' => Some((c - b'a') as u32 + 10),
        _ => None,
    }
}

fn base62(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u32),
        b'A'..=b'Z' => Some((c - b'A') as u32 + 10),
        b'a'..=b'z' => Some((c - b'a') as u32 + 36),
        _ => None,
    }
}

fn parse_id(s: &[u8], b62: bool) -> Option<u32> {
    if s.len() < 2 {
        return None;
    }
    let f = if b62 { base62 } else { base36 };
    let base = if b62 { 62 } else { 36 };
    Some(f(s[0])? * base + f(s[1])?)
}

fn parse_f64(s: &str) -> Option<f64> {
    let s = s.trim();
    // tolerate trailing junk such as "150.00 ;comment"
    let end = s
        .char_indices()
        .find(|(_, c)| {
            !(c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+' || *c == 'e' || *c == 'E')
        })
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    // finite and in a range the timing arithmetic survives
    s[..end]
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite() && v.abs() <= 1e9)
}

fn parse_i32(s: &str) -> Option<i32> {
    parse_f64(s).map(|v| v as i32)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

pub fn parse_bms(bytes: &[u8], ext: &str, opts: &ParseOptions) -> Result<Chart, ChartError> {
    let pms = ext.eq_ignore_ascii_case("pms");
    let text = decode_text(bytes);
    let mut src = source::read(&text, opts, None);
    src.meta.md5 = md5_hex(bytes);
    src.meta.sha256 = sha256_hex(bytes);
    let placed = place::place(&src, pms);
    Ok(build::build(src, placed, pms))
}

/// What the stages of a NONSTOP course share as each is read (S-OLR2b
/// 3.3, 3.7): the `#BPMxx` and `#STOPxx` definitions (a later stage's
/// replaces an earlier one's of the same number, the others stay) and
/// one `#RANDOM` sequence, seeded once. The open `#IF` blocks are each
/// stage's own.
pub struct Shared {
    bpm: HashMap<u32, f64>,
    stop: HashMap<u32, f64>,
    rng: JavaRandom,
}

impl Shared {
    pub fn new(seed: i64) -> Shared {
        Shared {
            bpm: HashMap::new(),
            stop: HashMap::new(),
            rng: JavaRandom::new(seed),
        }
    }
}

/// What a NONSTOP course needs of a stage beyond its chart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StageFacts {
    /// The beat each measure starts at, to the one after the last.
    pub measure_beats: Vec<f64>,
    /// It has a `#RANK` line (read) and a `#BPM` header line.
    pub rank_given: bool,
    pub bpm_given: bool,
}

/// A NONSTOP course's stage: its chart read on from `shared`.
pub fn parse_stage(
    bytes: &[u8],
    ext: &str,
    shared: &mut Shared,
) -> Result<(Chart, StageFacts), ChartError> {
    let pms = ext.eq_ignore_ascii_case("pms");
    let text = decode_text(bytes);
    let mut src = source::read(&text, &ParseOptions::default(), Some(&mut shared.rng));
    src.meta.md5 = md5_hex(bytes);
    src.meta.sha256 = sha256_hex(bytes);
    for (defs, own) in [(&shared.bpm, &mut src.bpm), (&shared.stop, &mut src.stop)] {
        for (k, v) in defs {
            own.entry(*k).or_insert(*v);
        }
    }
    shared.bpm.clone_from(&src.bpm);
    shared.stop.clone_from(&src.stop);
    let placed = place::place(&src, pms);
    let facts = StageFacts {
        measure_beats: placed.measure_start.clone(),
        rank_given: src.rank_given,
        bpm_given: src.bpm_given,
    };
    let chart = build::build(src, placed, pms);
    if chart.notes.is_empty() {
        return Err(ChartError::Empty);
    }
    Ok((chart, facts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::*;
    use crate::mode::PlayMode;

    fn parse(s: &str) -> Chart {
        parse_bms(
            s.as_bytes(),
            "bms",
            &ParseOptions {
                random_seed: 1,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn basic_7k() {
        let c = parse(
            "#TITLE test\n#BPM 120\n#WAV01 a.wav\n#WAV02 b.wav\n#00111:01000100\n#00116:02\n#00119:01\n#00101:0202\n",
        );
        assert_eq!(c.meta.title, "test");
        assert_eq!(c.mode(), PlayMode::Beat7K);
        let playable: Vec<_> = c.notes.iter().filter(|n| n.is_playable()).collect();
        assert_eq!(playable.len(), 4);
        // measure 1 at 120 BPM starts at 2 s
        assert_eq!(playable[0].time, 2_000_000);
        assert_eq!(playable.iter().filter(|n| n.lane == 7).count(), 1); // scratch
        assert_eq!(playable.iter().filter(|n| n.lane == 6).count(), 1); // key 7
        assert_eq!(c.bgm.len(), 2);
        assert_eq!(c.bgm[1].time, 3_000_000);
    }

    #[test]
    fn hostile_numbers_do_not_panic() {
        for head in [
            "#00102:1e999",
            "#00102:1e300",
            "#00102:0",
            "#00102:-1",
            "#BPM 0",
            "#BPM -120",
            "#BPM 1e300",
            "#BPM NaN",
            "#BPM01 0\n#00108:01",
            "#STOP01 1e300\n#00109:01",
            "#STOP01 -1e9\n#00109:01",
            "#SCROLL01 1e300\n#001SC:01",
            "#DEFEXRANK 1e300",
            "#TOTAL 1e300",
        ] {
            let src = format!("#BPM 120\n{head}\n#00111:0101\n#00211:01");
            let c = parse_bms(src.as_bytes(), "bms", &ParseOptions::default()).unwrap();
            let _ = (
                c.length(),
                c.timing.pos_at_time(1e6),
                c.timing.bpm_changes(),
            );
            let _ = c.timing.bpm_stats(c.length());
            for t in [0.0, 1e6, 1e12] {
                let _ = (c.timing.beat_at_time(t), c.timing.bpm_at_time(t));
            }
        }
    }

    #[test]
    fn five_key_and_double_detection() {
        assert_eq!(parse("#00111:01\n#00116:01").mode(), PlayMode::Beat5K);
        assert_eq!(parse("#00111:01\n#00121:01").mode(), PlayMode::Beat10K);
        assert_eq!(parse("#00118:01\n#00121:01").mode(), PlayMode::Beat14K);
        let p = parse_bms(b"#00111:01\n#00125:01", "pms", &ParseOptions::default()).unwrap();
        assert_eq!(p.mode(), PlayMode::Popn9K);
        assert!(p.notes.iter().any(|n| n.lane == 8));
    }

    #[test]
    fn pms_bme_type_layout() {
        let lanes = |src: &str| {
            let c = parse_bms(src.as_bytes(), "pms", &ParseOptions::default()).unwrap();
            let mut v: Vec<u8> = c.notes.iter().map(|n| n.lane).collect();
            v.sort_unstable();
            (v, c.bgm.len())
        };
        // buttons 1-9 on 11 12 13 14 15 18 19 16 17
        let bme = "#WAV01 a.wav\n#00111:01\n#00112:01\n#00113:01\n#00114:01\n#00115:01\n\
            #00118:01\n#00119:01\n#00116:01\n#00117:01\n";
        assert_eq!(lanes(bme), ((0..9).collect(), 0));
        let c = parse_bms(bme.as_bytes(), "pms", &ParseOptions::default()).unwrap();
        let lane_of = |ch: &str| {
            let src = format!("#WAV01 a.wav\n#001{ch}:01\n#00216:01\n#00217:01\n");
            let c = parse_bms(src.as_bytes(), "pms", &ParseOptions::default()).unwrap();
            c.notes.iter().find(|n| n.time < 3_000_000).map(|n| n.lane)
        };
        assert_eq!(c.mode(), PlayMode::Popn9K);
        assert_eq!(lane_of("18"), Some(5));
        assert_eq!(lane_of("19"), Some(6));
        assert_eq!(lane_of("16"), Some(7));
        assert_eq!(lane_of("17"), Some(8));
        // a stray 2P object in a BME-type file is dropped (not a note, no BGM)
        assert_eq!(lanes(&format!("{bme}#00122:01\n")), ((0..9).collect(), 0));
        // standard layout: 22-25 are buttons 6-9; a stray 16 is dropped
        let std = "#WAV01 a.wav\n#00111:01\n#00122:01\n#00125:01\n#00216:01\n";
        assert_eq!(lanes(std), (vec![0, 5, 8], 0));
    }

    #[test]
    fn long_notes_lntype1_and_lnobj() {
        let c = parse("#BPM 60\n#LNOBJ ZZ\n#00151:01000001\n#00112:0100ZZ00\n#00118:01\n");
        let lns: Vec<_> = c.notes.iter().filter(|n| n.is_long()).collect();
        assert_eq!(lns.len(), 2);
        let l0 = lns.iter().find(|n| n.lane == 0).unwrap();
        assert_eq!(l0.time, 4_000_000);
        assert_eq!(l0.end_time(), 4_000_000 + 3_000_000);
        let l1 = lns.iter().find(|n| n.lane == 1).unwrap();
        assert_eq!(l1.end_time() - l1.time, 2_000_000);
        assert_eq!(c.total_judgements_of(LnType::Ln, None), 3);
        assert_eq!(c.total_judgements_of(LnType::Cn, None), 5);
    }

    #[test]
    fn lnobj_marker_wav_is_not_the_end_sound() {
        let c = parse("#WAV01 a.wav\n#WAVZZ z.wav\n#WAV02 b.wav\n#LNOBJ ZZ\n#00112:0100ZZ00\n#00151:01000002\n");
        let end = |lane: u8| match c.notes.iter().find(|n| n.lane == lane).map(|n| &n.kind) {
            Some(NoteKind::Long { end_sound, .. }) => *end_sound,
            _ => panic!("no long note on {lane}"),
        };
        assert_eq!(end(1), NO_SOUND);
        // a channel LN's end with a sound of its own keeps it
        assert_ne!(end(0), NO_SOUND);
    }

    #[test]
    fn measure_length_bpm_stop() {
        // measure 0 is 3/4 long; BPM 120 -> 240 at measure 1; stop 48 (1 beat) at measure 1
        let c = parse(
            "#BPM 120\n#STOP01 48\n#00002:0.75\n#00103:F0\n#00109:01\n#00111:01\n#00211:01\n",
        );
        let n: Vec<_> = c.notes.iter().collect();
        assert_eq!(n[0].time, 1_500_000);
        // measure 1 is 4 beats at 240 = 1 s, plus 1 beat stop at 240 = 0.25 s
        assert_eq!(n[1].time, 1_500_000 + 250_000 + 1_000_000);
        assert_eq!(c.meta.max_bpm, 240.0);
        assert!(c.meta.has_stop);
    }

    #[test]
    fn random_blocks() {
        let src = "#RANDOM 2\n#IF 1\n#00111:01\n#ENDIF\n#IF 2\n#00112:01\n#ENDIF\n#ENDRANDOM\n";
        for sel in [1u32, 2] {
            let c = parse_bms(
                src.as_bytes(),
                "bms",
                &ParseOptions {
                    random_selections: Some(vec![sel]),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(c.notes.len(), 1);
            assert_eq!(c.notes[0].lane as u32, sel - 1);
            assert!(c.meta.has_random);
        }
        // nested + else
        let src = "#RANDOM 2\n#IF 1\n#RANDOM 2\n#IF 2\n#00113:01\n#ENDIF\n#ENDRANDOM\n#ELSE\n#00114:01\n#ENDIF\n#ENDRANDOM\n";
        let c = parse_bms(
            src.as_bytes(),
            "bms",
            &ParseOptions {
                random_selections: Some(vec![1, 2]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(c.notes[0].lane, 2);
        let c = parse_bms(
            src.as_bytes(),
            "bms",
            &ParseOptions {
                random_selections: Some(vec![2]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(c.notes[0].lane, 3);
    }

    #[test]
    fn switch_blocks() {
        let src = "#SETSWITCH 2\n#CASE 1\n#00111:01\n#SKIP\n#CASE 2\n#00112:01\n#CASE 3\n#00113:01\n#SKIP\n#DEF\n#00114:01\n#ENDSW\n";
        let c = parse(src);
        let lanes: Vec<u8> = c.notes.iter().map(|n| n.lane).collect();
        assert_eq!(lanes, vec![1, 2]); // case 2 falls through into case 3
    }

    #[test]
    fn duplicate_definitions_override() {
        let c = parse("#00111:01\n#00111:02\n#00111:0003\n");
        let n: Vec<_> = c.notes.iter().collect();
        assert_eq!(n.len(), 2);
    }

    #[test]
    fn mines_and_invisible() {
        let c = parse("#WAV00 boom.wav\n#WAV01 a.wav\n#001D1:0A\n#00131:01\n#00111:01\n");
        assert!(c
            .notes
            .iter()
            .any(|n| matches!(n.kind, NoteKind::Mine { damage } if damage == 10.0)));
        assert!(c
            .notes
            .iter()
            .any(|n| matches!(n.kind, NoteKind::Invisible)));
        assert_eq!(c.note_count(), 1);
    }

    #[test]
    fn hashes() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(sha256_hex(b"").len(), 64);
    }
}
