//! The first stage: a chart text's headers, definitions and channel lines,
//! with its control flow applied.

use super::flow::Flow;
use super::{parse_f64, parse_i32, parse_id, ParseOptions};
use crate::chart::{ChartMeta, JudgeRank, LnType};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// A channel line `#mmmcc:data`.
pub(super) struct Line<'a> {
    pub measure: u32,
    /// The channel, in upper case.
    pub ch: [u8; 2],
    /// The objects, without whitespace.
    pub data: Cow<'a, str>,
}

/// What the text says, as read in order.
pub(super) struct Source<'a> {
    /// The headers' values, and the `#RANDOM` outcomes.
    pub meta: ChartMeta,
    pub initial_bpm: f64,
    /// `#BASE 62`: object ids in base 62.
    pub b62: bool,
    pub wav: HashMap<u32, &'a str>,
    pub bmp: HashMap<u32, &'a str>,
    pub bpm: HashMap<u32, f64>,
    pub stop: HashMap<u32, f64>,
    pub scroll: HashMap<u32, f64>,
    pub lnobj: HashSet<u32>,
    /// Measure lengths other than 1 (channel 02).
    pub measure_len: HashMap<u32, f64>,
    pub lines: Vec<Line<'a>>,
    /// A `#RANK` / `#BPM` line was read (a NONSTOP stage's).
    pub rank_given: bool,
    pub bpm_given: bool,
}

/// The longest header name the parser knows (`DIFFICULTY`), with room.
const KEY_MAX: usize = 12;

/// `rng`: the `#RANDOM` sequence to go on with (a NONSTOP course's), in
/// place of one seeded by `opts`.
pub(super) fn read<'a>(
    text: &'a str,
    opts: &ParseOptions,
    mut rng: Option<&mut crate::rng::JavaRandom>,
) -> Source<'a> {
    let mut flow = Flow::new(opts);
    if let Some(r) = rng.as_deref_mut() {
        std::mem::swap(&mut flow.rng, r);
    }
    let mut src = Source {
        meta: ChartMeta::default(),
        initial_bpm: 130.0,
        b62: false,
        wav: HashMap::new(),
        bmp: HashMap::new(),
        bpm: HashMap::new(),
        stop: HashMap::new(),
        scroll: HashMap::new(),
        lnobj: HashSet::new(),
        measure_len: HashMap::new(),
        lines: Vec::new(),
        rank_given: false,
        bpm_given: false,
    };
    let (mut rank_given, mut bpm_given) = (false, false);
    let meta = &mut src.meta;
    let mut lnobj: Vec<&str> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        let Some(body) = line.strip_prefix('#') else {
            continue;
        };
        let bb = body.as_bytes();
        // channel line: #mmmcc:data
        if bb.len() >= 6 && bb[..3].iter().all(u8::is_ascii_digit) && bb[5] == b':' {
            if !flow.active() {
                continue;
            }
            let measure: u32 = body[..3].parse().unwrap_or(0);
            let ch = [bb[3].to_ascii_uppercase(), bb[4].to_ascii_uppercase()];
            let data = &body[6..];
            let data = if data.contains(char::is_whitespace) {
                Cow::Owned(data.chars().filter(|c| !c.is_whitespace()).collect())
            } else {
                Cow::Borrowed(data)
            };
            if &ch == b"02" {
                if let Some(v) = parse_f64(&data) {
                    if v > 0.0 {
                        src.measure_len.insert(measure, v);
                    }
                }
            } else {
                src.lines.push(Line { measure, ch, data });
            }
            continue;
        }
        let (key, arg) = match body.find(|c: char| c.is_whitespace()) {
            Some(i) => (&body[..i], body[i..].trim()),
            None => (body, ""),
        };
        // (a longer name is none the parser knows)
        if key.len() > KEY_MAX {
            continue;
        }
        let mut upper = [0u8; KEY_MAX];
        upper[..key.len()].copy_from_slice(key.as_bytes());
        upper.make_ascii_uppercase();
        let ukey = std::str::from_utf8(&upper[..key.len()]).unwrap_or_default();
        if flow.handle(ukey, arg) || !flow.active() {
            continue;
        }
        let idx = |prefix: &str| -> Option<u32> {
            ukey.strip_prefix(prefix)
                .filter(|r| r.len() == 2)
                .and_then(|_| parse_id(&key.as_bytes()[prefix.len()..], src.b62))
        };
        match ukey {
            "TITLE" => meta.title = arg.to_owned(),
            "SUBTITLE" => meta.subtitle = arg.to_owned(),
            "ARTIST" => meta.artist = arg.to_owned(),
            "SUBARTIST" => {
                if !meta.subartist.is_empty() {
                    meta.subartist.push(' ');
                }
                meta.subartist.push_str(arg)
            }
            "GENRE" => meta.genre = arg.to_owned(),
            "BPM" => {
                bpm_given = true;
                src.initial_bpm = parse_f64(arg).filter(|v| *v > 0.0).unwrap_or(130.0)
            }
            "PLAYLEVEL" => meta.level = arg.to_owned(),
            "DIFFICULTY" => meta.difficulty = parse_i32(arg).unwrap_or(0),
            "RANK" => {
                rank_given = true;
                meta.judge_rank = parse_i32(arg).map_or(JudgeRank::NoNumber, JudgeRank::Rank)
            }
            "DEFEXRANK" => {
                if let Some(v) = parse_f64(arg) {
                    meta.judge_rank = JudgeRank::DefExRank(v)
                }
            }
            "TOTAL" => meta.total = parse_f64(arg).filter(|v| *v > 0.0),
            "STAGEFILE" => meta.stagefile = arg.to_owned(),
            "BANNER" => meta.banner = arg.to_owned(),
            "BACKBMP" => meta.backbmp = arg.to_owned(),
            "PREVIEW" => meta.preview = arg.to_owned(),
            "LNOBJ" => lnobj.push(arg),
            "LNMODE" => {
                meta.ln_type = match parse_i32(arg) {
                    Some(1) => Some(LnType::Ln),
                    Some(2) => Some(LnType::Cn),
                    Some(3) => Some(LnType::Hcn),
                    _ => None,
                }
            }
            "BASE" => src.b62 = parse_i32(arg) == Some(62),
            _ => {
                if let Some(id) = idx("WAV") {
                    src.wav.insert(id, arg);
                } else if let Some(id) = idx("BMP") {
                    src.bmp.insert(id, arg);
                } else if let Some(id) = idx("EXBPM").or_else(|| idx("BPM")) {
                    if let Some(v) = parse_f64(arg) {
                        src.bpm.insert(id, v);
                    }
                } else if let Some(id) = idx("STOP") {
                    if let Some(v) = parse_f64(arg) {
                        src.stop.insert(id, v);
                    }
                } else if let Some(id) = idx("SCROLL") {
                    if let Some(v) = parse_f64(arg) {
                        src.scroll.insert(id, v);
                    }
                }
            }
        }
    }
    meta.has_random = flow.used;
    meta.random_selections = std::mem::take(&mut flow.selections);
    if let Some(r) = rng {
        std::mem::swap(&mut flow.rng, r);
    }
    (src.rank_given, src.bpm_given) = (rank_given, bpm_given);
    // (in the base of the whole file: #BASE may come after #LNOBJ)
    src.lnobj = lnobj
        .iter()
        .filter_map(|s| parse_id(s.trim().as_bytes(), src.b62))
        .collect();
    src
}
