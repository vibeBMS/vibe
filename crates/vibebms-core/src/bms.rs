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

use crate::chart::*;
use crate::encoding::decode_text;
use crate::mode::PlayMode;
use crate::rng::JavaRandom;
use crate::timing::{TimingEvent, TimingMap};
use crate::ChartError;
use md5::{Digest, Md5};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub struct ParseOptions {
    /// Seed for `#RANDOM`; `None` = derived from the system clock.
    pub random_seed: Option<i64>,
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
    s[..end].parse().ok()
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

#[derive(Debug)]
enum Frame {
    Random {
        value: u32,
    },
    If {
        active: bool,
        matched: bool,
        parent: bool,
        depth: usize,
    },
    Switch {
        value: u32,
        active: bool,
        matched: bool,
        skipping: bool,
        parent: bool,
    },
}

struct Flow {
    stack: Vec<Frame>,
    rng: JavaRandom,
    forced: Option<Vec<u32>>,
    forced_idx: usize,
    selections: Vec<u32>,
    used: bool,
}

impl Flow {
    fn active(&self) -> bool {
        for f in self.stack.iter().rev() {
            match f {
                Frame::If { active, .. } | Frame::Switch { active, .. } => return *active,
                Frame::Random { .. } => continue,
            }
        }
        true
    }

    fn random_depth(&self) -> usize {
        self.stack
            .iter()
            .filter(|f| matches!(f, Frame::Random { .. }))
            .count()
    }

    fn current_random(&self) -> u32 {
        self.stack
            .iter()
            .rev()
            .find_map(|f| match f {
                Frame::Random { value } => Some(*value),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn roll(&mut self, max: u32) -> u32 {
        self.used = true;
        let v = if let Some(f) = self.forced.as_ref().and_then(|f| f.get(self.forced_idx)) {
            *f
        } else {
            self.rng.next_int_bounded(max.max(1) as i32) as u32 + 1
        };
        self.forced_idx += 1;
        self.selections.push(v);
        v
    }

    /// Returns true when the line was a control-flow directive.
    fn handle(&mut self, key: &str, arg: &str) -> bool {
        let n = || parse_i32(arg).unwrap_or(0).max(0) as u32;
        match key {
            "RANDOM" | "SETRANDOM" => {
                let value = if !self.active() {
                    0
                } else if key == "RANDOM" {
                    self.roll(n())
                } else {
                    self.used = true;
                    n()
                };
                self.stack.push(Frame::Random { value });
            }
            "ENDRANDOM" => {
                while let Some(f) = self.stack.pop() {
                    if matches!(f, Frame::Random { .. }) {
                        break;
                    }
                }
            }
            "IF" => {
                let depth = self.random_depth();
                if let Some(Frame::If { depth: d, .. }) = self.stack.last() {
                    if *d == depth {
                        self.stack.pop(); // implicit #ENDIF
                    }
                }
                let parent = self.active();
                let hit = self.current_random() == n();
                self.stack.push(Frame::If {
                    active: parent && hit,
                    matched: hit,
                    parent,
                    depth,
                });
            }
            "ELSEIF" => {
                let r = self.current_random();
                if let Some(Frame::If {
                    active,
                    matched,
                    parent,
                    ..
                }) = self.stack.last_mut()
                {
                    let hit = !*matched && r == n();
                    *active = *parent && hit;
                    *matched |= hit;
                }
            }
            "ELSE" => {
                if let Some(Frame::If {
                    active,
                    matched,
                    parent,
                    ..
                }) = self.stack.last_mut()
                {
                    *active = *parent && !*matched;
                    *matched = true;
                }
            }
            "ENDIF" => {
                if matches!(self.stack.last(), Some(Frame::If { .. })) {
                    self.stack.pop();
                }
            }
            "SWITCH" | "SETSWITCH" => {
                let parent = self.active();
                let value = if !parent {
                    0
                } else if key == "SWITCH" {
                    self.roll(n())
                } else {
                    self.used = true;
                    n()
                };
                self.stack.push(Frame::Switch {
                    value,
                    active: false,
                    matched: false,
                    skipping: false,
                    parent,
                });
            }
            "CASE" => {
                let v = n();
                if let Some(Frame::Switch {
                    value,
                    active,
                    matched,
                    skipping,
                    parent,
                }) = self.stack.last_mut()
                {
                    if !*skipping && !*active && *value == v {
                        *active = *parent;
                        *matched = true;
                    }
                }
            }
            "SKIP" => {
                if let Some(Frame::Switch {
                    active, skipping, ..
                }) = self.stack.last_mut()
                {
                    if *active {
                        *active = false;
                        *skipping = true;
                    }
                }
            }
            "DEF" => {
                if let Some(Frame::Switch {
                    active,
                    matched,
                    skipping,
                    parent,
                    ..
                }) = self.stack.last_mut()
                {
                    if !*skipping && !*matched {
                        *active = *parent;
                        *matched = true;
                    }
                }
            }
            "ENDSW" => {
                while let Some(f) = self.stack.pop() {
                    if matches!(f, Frame::Switch { .. }) {
                        break;
                    }
                }
            }
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjKind {
    Visible,
    Invisible,
    Long,
    Mine,
}

#[derive(Clone, Debug)]
struct RawObj {
    player: usize,
    /// 0..=8 key index in BMS channel space (see `map_key`), scratch = 100
    key: usize,
    beat: f64,
    pos_key: (u32, u64, u64),
    id: u32,
    kind: ObjKind,
}

const SCRATCH: usize = 100;

/// Map the second channel character to a key index.
fn map_key(c: u8, pms: bool, player: usize) -> Option<usize> {
    if pms {
        return match (player, c) {
            (0, b'1'..=b'5') => Some((c - b'1') as usize),
            (1, b'2'..=b'5') => Some((c - b'2') as usize + 5),
            _ => None,
        };
    }
    match c {
        b'1'..=b'5' => Some((c - b'1') as usize),
        b'6' => Some(SCRATCH),
        b'8' => Some(5),
        b'9' => Some(6),
        _ => None,
    }
}

pub fn parse_bms(bytes: &[u8], ext: &str, opts: &ParseOptions) -> Result<Chart, ChartError> {
    let text = decode_text(bytes);
    let pms = ext.eq_ignore_ascii_case("pms");

    let seed = opts.random_seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0)
    });
    let mut flow = Flow {
        stack: Vec::new(),
        rng: JavaRandom::new(seed),
        forced: opts.random_selections.clone(),
        forced_idx: 0,
        selections: Vec::new(),
        used: false,
    };

    let mut meta = ChartMeta {
        md5: md5_hex(bytes),
        sha256: sha256_hex(bytes),
        ..Default::default()
    };
    let mut initial_bpm = 130.0;
    let mut b62 = false;
    let mut wav_defs: HashMap<u32, String> = HashMap::new();
    let mut bmp_defs: HashMap<u32, String> = HashMap::new();
    let mut bpm_defs: HashMap<u32, f64> = HashMap::new();
    let mut stop_defs: HashMap<u32, f64> = HashMap::new();
    let mut scroll_defs: HashMap<u32, f64> = HashMap::new();
    let mut lnobj_raw: Vec<String> = Vec::new();
    let mut measure_len: HashMap<u32, f64> = HashMap::new();
    // (measure, channel, data)
    let mut lines: Vec<(u32, [u8; 2], String)> = Vec::new();

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
            let data: String = body[6..].chars().filter(|c| !c.is_whitespace()).collect();
            if &ch == b"02" {
                if let Some(v) = parse_f64(&data) {
                    if v > 0.0 {
                        measure_len.insert(measure, v);
                    }
                }
            } else {
                lines.push((measure, ch, data));
            }
            continue;
        }
        let (key, arg) = match body.find(|c: char| c.is_whitespace()) {
            Some(i) => (&body[..i], body[i..].trim()),
            None => (body, ""),
        };
        let ukey = key.to_ascii_uppercase();
        if flow.handle(&ukey, arg) || !flow.active() {
            continue;
        }
        let idx = |prefix: &str| -> Option<u32> {
            ukey.strip_prefix(prefix)
                .filter(|r| r.len() == 2)
                .and_then(|_| parse_id(&key.as_bytes()[prefix.len()..], b62))
        };
        match ukey.as_str() {
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
            "BPM" => initial_bpm = parse_f64(arg).filter(|v| *v > 0.0).unwrap_or(130.0),
            "PLAYLEVEL" => meta.level = arg.to_owned(),
            "DIFFICULTY" => meta.difficulty = parse_i32(arg).unwrap_or(0),
            "RANK" => meta.judge_rank = JudgeRank::Rank(parse_i32(arg).unwrap_or(2)),
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
            "LNOBJ" => lnobj_raw.push(arg.to_owned()),
            "LNMODE" => {
                meta.ln_type = match parse_i32(arg) {
                    Some(1) => Some(LnType::Ln),
                    Some(2) => Some(LnType::Cn),
                    Some(3) => Some(LnType::Hcn),
                    _ => None,
                }
            }
            "BASE" => b62 = parse_i32(arg) == Some(62),
            _ => {
                if let Some(id) = idx("WAV") {
                    wav_defs.insert(id, arg.to_owned());
                } else if let Some(id) = idx("BMP") {
                    bmp_defs.insert(id, arg.to_owned());
                } else if let Some(id) = idx("EXBPM").or_else(|| idx("BPM")) {
                    if let Some(v) = parse_f64(arg) {
                        bpm_defs.insert(id, v);
                    }
                } else if let Some(id) = idx("STOP") {
                    if let Some(v) = parse_f64(arg) {
                        stop_defs.insert(id, v);
                    }
                } else if let Some(id) = idx("SCROLL") {
                    if let Some(v) = parse_f64(arg) {
                        scroll_defs.insert(id, v);
                    }
                }
            }
        }
    }
    meta.has_random = flow.used;
    meta.random_selections = flow.selections;
    let lnobj: HashSet<u32> = lnobj_raw
        .iter()
        .filter_map(|s| parse_id(s.trim().as_bytes(), b62))
        .collect();

    // measure start beats
    let max_measure = lines.iter().map(|l| l.0).max().unwrap_or(0);
    let mut measure_start = Vec::with_capacity(max_measure as usize + 2);
    let mut acc = 0.0;
    for m in 0..=max_measure + 1 {
        measure_start.push(acc);
        acc += 4.0 * measure_len.get(&m).copied().unwrap_or(1.0);
    }
    let mlen = |m: u32| measure_len.get(&m).copied().unwrap_or(1.0);

    // sounds / images
    let mut chart = Chart::default();
    let mut wav_map: HashMap<u32, SoundId> = HashMap::new();
    let mut wav_ids: Vec<_> = wav_defs.keys().copied().collect();
    wav_ids.sort_unstable();
    for id in wav_ids {
        let file = chart.sound_files.len() as u32;
        chart.sound_files.push(wav_defs[&id].clone());
        wav_map.insert(id, chart.sounds.len() as SoundId);
        chart.sounds.push(SoundSlice {
            file,
            start: 0,
            end: None,
        });
    }
    let snd = |id: u32| wav_map.get(&id).copied().unwrap_or(NO_SOUND);
    let mut bmp_map: HashMap<u32, u32> = HashMap::new();
    let mut bmp_ids: Vec<_> = bmp_defs.keys().copied().collect();
    bmp_ids.sort_unstable();
    for id in bmp_ids {
        bmp_map.insert(id, chart.images.len() as u32);
        chart.images.push(bmp_defs[&id].clone());
    }

    let mut timing_events: Vec<(f64, TimingEvent)> = Vec::new();
    let mut bgm_raw: Vec<(f64, u32)> = Vec::new();
    let mut bga_raw: Vec<(f64, BgaLayer, u32)> = Vec::new();
    let mut objs: Vec<RawObj> = Vec::new();

    for (measure, ch, data) in &lines {
        let db = data.as_bytes();
        let n = db.len() / 2;
        if n == 0 {
            continue;
        }
        for i in 0..n {
            let pair = &db[i * 2..i * 2 + 2];
            if pair == b"00" {
                continue;
            }
            let beat =
                measure_start[*measure as usize] + (i as f64 / n as f64) * 4.0 * mlen(*measure);
            let g = gcd(i as u64, n as u64).max(1);
            let pos_key = (*measure, i as u64 / g, n as u64 / g);
            let id = || parse_id(pair, b62);
            match ch {
                b"01" => {
                    if let Some(id) = id() {
                        bgm_raw.push((beat, id));
                    }
                }
                b"03" => {
                    if let Ok(v) = u32::from_str_radix(std::str::from_utf8(pair).unwrap_or("0"), 16)
                    {
                        timing_events.push((beat, TimingEvent::Bpm(v as f64)));
                    }
                }
                b"08" => {
                    if let Some(v) = id().and_then(|id| bpm_defs.get(&id)) {
                        timing_events.push((beat, TimingEvent::Bpm(*v)));
                    }
                }
                b"09" => {
                    if let Some(v) = id().and_then(|id| stop_defs.get(&id)) {
                        timing_events.push((beat, TimingEvent::StopBeats(*v / 48.0)));
                        meta.has_stop = true;
                    }
                }
                b"SC" => {
                    if let Some(v) = id().and_then(|id| scroll_defs.get(&id)) {
                        timing_events.push((beat, TimingEvent::Scroll(*v)));
                    }
                }
                b"04" | b"06" | b"07" | b"0A" => {
                    let layer = match ch {
                        b"04" => BgaLayer::Base,
                        b"06" => BgaLayer::Poor,
                        b"07" => BgaLayer::Layer,
                        _ => BgaLayer::Layer2,
                    };
                    if let Some(id) = id() {
                        bga_raw.push((beat, layer, id));
                    }
                }
                _ => {
                    let (player, kind) = match ch[0] {
                        b'1' => (0, ObjKind::Visible),
                        b'2' => (1, ObjKind::Visible),
                        b'3' => (0, ObjKind::Invisible),
                        b'4' => (1, ObjKind::Invisible),
                        b'5' => (0, ObjKind::Long),
                        b'6' => (1, ObjKind::Long),
                        b'D' => (0, ObjKind::Mine),
                        b'E' => (1, ObjKind::Mine),
                        _ => continue,
                    };
                    let Some(id) = id() else { continue };
                    match map_key(ch[1], pms, player) {
                        Some(key) => objs.push(RawObj {
                            player: if pms { 0 } else { player },
                            key,
                            beat,
                            pos_key,
                            id,
                            kind,
                        }),
                        None => {
                            // free zone (x7) and unknown lanes: keep the sound as BGM
                            if kind == ObjKind::Visible {
                                bgm_raw.push((beat, id));
                            }
                        }
                    }
                }
            }
        }
    }

    // mode detection
    let mode = if pms {
        PlayMode::Popn9K
    } else {
        let double = objs
            .iter()
            .any(|o| o.player == 1 && o.kind != ObjKind::Invisible);
        let seven = objs
            .iter()
            .any(|o| (o.key == 5 || o.key == 6) && o.kind != ObjKind::Invisible);
        match (double, seven) {
            (false, false) => PlayMode::Beat5K,
            (false, true) => PlayMode::Beat7K,
            (true, false) => PlayMode::Beat10K,
            (true, true) => PlayMode::Beat14K,
        }
    };
    meta.mode = Some(mode);
    let lane_of = |o: &RawObj| -> Option<u8> {
        let key = if o.key == SCRATCH {
            mode.keys_per_player()
        } else {
            o.key
        };
        if key >= mode.lanes_per_player() || o.player >= mode.players() {
            return None;
        }
        Some((o.player * mode.lanes_per_player() + key) as u8)
    };

    let timing = TimingMap::build(initial_bpm, timing_events);
    meta.initial_bpm = initial_bpm;
    let t_of = |beat: f64| timing.time_at_beat(beat);
    let p_of = |beat: f64| timing.pos_at_beat(beat);

    // Visible notes: later definitions override earlier ones at the same position.
    let lanes = mode.lane_count();
    let mut visible: Vec<HashMap<(u32, u64, u64), RawObj>> = vec![HashMap::new(); lanes];
    let mut longs: Vec<HashMap<(u32, u64, u64), RawObj>> = vec![HashMap::new(); lanes];
    let mut notes: Vec<Note> = Vec::new();
    for o in &objs {
        let Some(lane) = lane_of(o) else { continue };
        match o.kind {
            ObjKind::Visible => {
                visible[lane as usize].insert(o.pos_key, o.clone());
            }
            ObjKind::Long => {
                longs[lane as usize].insert(o.pos_key, o.clone());
            }
            ObjKind::Invisible => {
                meta.has_invisible = true;
                notes.push(Note {
                    lane,
                    time: t_of(o.beat),
                    beat: o.beat,
                    pos: p_of(o.beat),
                    sound: snd(o.id),
                    kind: NoteKind::Invisible,
                });
            }
            ObjKind::Mine => {
                meta.has_mine = true;
                notes.push(Note {
                    lane,
                    time: t_of(o.beat),
                    beat: o.beat,
                    pos: p_of(o.beat),
                    sound: snd(0),
                    kind: NoteKind::Mine {
                        damage: o.id as f32,
                    },
                });
            }
        }
    }

    let make_long = |s: &RawObj, e_beat: f64, end_sound: SoundId, lane: u8| Note {
        lane,
        time: t_of(s.beat),
        beat: s.beat,
        pos: p_of(s.beat),
        sound: snd(s.id),
        kind: NoteKind::Long {
            end_time: t_of(e_beat),
            end_beat: e_beat,
            end_pos: p_of(e_beat),
            end_sound,
            ln_type: meta.ln_type,
        },
    };

    for lane in 0..lanes {
        let lane_u8 = lane as u8;
        // LNTYPE 1 channel long notes: pair consecutive objects
        let mut ln: Vec<RawObj> = longs[lane].values().cloned().collect();
        ln.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
        let mut ranges: Vec<(f64, f64)> = Vec::new();
        let mut it = ln.chunks(2);
        for pair in &mut it {
            if pair.len() == 2 {
                let end_sound = snd(pair[1].id);
                let end_sound = if end_sound == snd(pair[0].id) {
                    NO_SOUND
                } else {
                    end_sound
                };
                notes.push(make_long(&pair[0], pair[1].beat, end_sound, lane_u8));
                ranges.push((pair[0].beat, pair[1].beat));
                meta.has_long = true;
            } else {
                // dangling start: treat as a normal note
                let s = &pair[0];
                notes.push(Note {
                    lane: lane_u8,
                    time: t_of(s.beat),
                    beat: s.beat,
                    pos: p_of(s.beat),
                    sound: snd(s.id),
                    kind: NoteKind::Normal,
                });
            }
        }
        // normal channel notes, with LNOBJ terminators
        let mut vis: Vec<RawObj> = visible[lane].values().cloned().collect();
        vis.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
        let mut pending: Option<RawObj> = None;
        let flush = |p: Option<RawObj>, notes: &mut Vec<Note>| {
            if let Some(s) = p {
                notes.push(Note {
                    lane: lane_u8,
                    time: t_of(s.beat),
                    beat: s.beat,
                    pos: p_of(s.beat),
                    sound: snd(s.id),
                    kind: NoteKind::Normal,
                });
            }
        };
        for o in vis {
            if ranges.iter().any(|(s, e)| o.beat >= *s && o.beat <= *e) {
                continue; // overlaps a channel LN
            }
            if lnobj.contains(&o.id) {
                if let Some(s) = pending.take() {
                    let end_sound = snd(o.id);
                    let end_sound = if end_sound == snd(s.id) {
                        NO_SOUND
                    } else {
                        end_sound
                    };
                    notes.push(make_long(&s, o.beat, end_sound, lane_u8));
                    meta.has_long = true;
                }
                continue;
            }
            flush(pending.replace(o), &mut notes);
        }
        flush(pending, &mut notes);
    }

    notes.sort_by(|a, b| a.time.cmp(&b.time).then(a.lane.cmp(&b.lane)));
    chart.notes = notes;

    bgm_raw.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    chart.bgm = bgm_raw
        .into_iter()
        .filter_map(|(beat, id)| {
            let sound = snd(id);
            (sound != NO_SOUND).then(|| BgmEvent {
                time: t_of(beat),
                sound,
            })
        })
        .collect();

    bga_raw.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    chart.bga = bga_raw
        .into_iter()
        .map(|(beat, layer, id)| BgaEvent {
            time: t_of(beat),
            layer,
            image: bmp_map.get(&id).copied(),
        })
        .collect();
    meta.has_bga = !chart.bga.is_empty();

    chart.bar_lines = measure_start
        .iter()
        .map(|b| BarLine {
            time: t_of(*b),
            pos: p_of(*b),
        })
        .collect();

    chart.timing = timing;
    chart.meta = meta;
    chart.compute_bpm_stats();
    Ok(chart)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Chart {
        parse_bms(
            s.as_bytes(),
            "bms",
            &ParseOptions {
                random_seed: Some(1),
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
    fn five_key_and_double_detection() {
        assert_eq!(parse("#00111:01\n#00116:01").mode(), PlayMode::Beat5K);
        assert_eq!(parse("#00111:01\n#00121:01").mode(), PlayMode::Beat10K);
        assert_eq!(parse("#00118:01\n#00121:01").mode(), PlayMode::Beat14K);
        let p = parse_bms(b"#00111:01\n#00125:01", "pms", &ParseOptions::default()).unwrap();
        assert_eq!(p.mode(), PlayMode::Popn9K);
        assert!(p.notes.iter().any(|n| n.lane == 8));
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
        assert_eq!(c.total_judgements(LnType::Ln), 3);
        assert_eq!(c.total_judgements(LnType::Cn), 5);
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
