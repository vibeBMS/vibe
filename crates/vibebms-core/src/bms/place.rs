//! The second stage: each channel's objects at their beats.

use super::source::{Line, Source};
use super::{gcd, parse_id};
use crate::chart::BgaLayer;
use crate::timing::TimingEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObjKind {
    Visible,
    Invisible,
    Long,
    Mine,
}

/// An object of a note channel.
pub(super) struct RawObj {
    pub player: usize,
    /// 0..=8 key index in BMS channel space (see `map_key`), scratch = 100
    pub key: usize,
    pub beat: f64,
    /// Its place: measure and the reduced fraction of it.
    pub pos_key: (u32, u64, u64),
    pub id: u32,
    pub kind: ObjKind,
}

pub(super) const SCRATCH: usize = 100;

/// How a `.pms` file places its 9 buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PmsLayout {
    /// Buttons 1-5 on 11-15, 6-9 on 22-25.
    Standard,
    /// "BME-type": buttons 1-9 on 11, 12, 13, 14, 15, 18, 19, 16, 17 (LR2,
    /// pomu2, PMSee-V).
    Bme,
}

/// The layout a `.pms` file uses: BME-type when it has more objects on the
/// 1P channels x6-x9 than on the 2P channels x2-x5 (the other channels
/// are then dropped).
fn pms_layout(lines: &[Line]) -> PmsLayout {
    let (mut bme, mut std) = (0usize, 0usize);
    for Line { ch, data, .. } in lines {
        let objs = data
            .as_bytes()
            .chunks_exact(2)
            .filter(|p| *p != b"00")
            .count();
        match (ch[0], ch[1]) {
            (b'1' | b'3' | b'5' | b'D', b'6'..=b'9') => bme += objs,
            (b'2' | b'4' | b'6' | b'E', b'2'..=b'5') => std += objs,
            _ => {}
        }
    }
    if bme > std {
        PmsLayout::Bme
    } else {
        PmsLayout::Standard
    }
}

/// Map the second channel character to a key index.
fn map_key(c: u8, pms: Option<PmsLayout>, player: usize) -> Option<usize> {
    if let Some(layout) = pms {
        return match (layout, player, c) {
            (_, 0, b'1'..=b'5') => Some((c - b'1') as usize),
            (PmsLayout::Standard, 1, b'2'..=b'5') => Some((c - b'2') as usize + 5),
            (PmsLayout::Bme, 0, b'8' | b'9') => Some((c - b'8') as usize + 5),
            (PmsLayout::Bme, 0, b'6' | b'7') => Some((c - b'6') as usize + 7),
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

/// The objects of a chart's channels, at their beats.
pub(super) struct Placed {
    pub objs: Vec<RawObj>,
    /// Background sounds (and note channel objects on no lane): beat, id.
    pub bgm: Vec<(f64, u32)>,
    pub bga: Vec<(f64, BgaLayer, u32)>,
    pub timing: Vec<(f64, TimingEvent)>,
    /// The beat each measure starts at, and the one after the last.
    pub measure_start: Vec<f64>,
}

pub(super) fn place(src: &Source, pms: bool) -> Placed {
    let mlen = |m: u32| src.measure_len.get(&m).copied().unwrap_or(1.0);
    let max_measure = src.lines.iter().map(|l| l.measure).max().unwrap_or(0);
    let mut measure_start = Vec::with_capacity(max_measure as usize + 2);
    let mut acc = 0.0;
    for m in 0..=max_measure + 1 {
        measure_start.push(acc);
        acc += 4.0 * mlen(m);
    }
    let mut p = Placed {
        objs: Vec::new(),
        bgm: Vec::new(),
        bga: Vec::new(),
        timing: Vec::new(),
        measure_start,
    };
    let pms_layout = pms.then(|| pms_layout(&src.lines));
    for Line { measure, ch, data } in &src.lines {
        let db = data.as_bytes();
        let n = db.len() / 2;
        for i in 0..n {
            let pair = &db[i * 2..i * 2 + 2];
            if pair == b"00" {
                continue;
            }
            let beat =
                p.measure_start[*measure as usize] + (i as f64 / n as f64) * 4.0 * mlen(*measure);
            let id = || parse_id(pair, src.b62);
            match ch {
                b"01" => {
                    if let Some(id) = id() {
                        p.bgm.push((beat, id));
                    }
                }
                b"03" => {
                    if let Ok(v) = u32::from_str_radix(std::str::from_utf8(pair).unwrap_or("0"), 16)
                    {
                        p.timing.push((beat, TimingEvent::Bpm(v as f64)));
                    }
                }
                b"08" => {
                    if let Some(v) = id().and_then(|id| src.bpm.get(&id)) {
                        p.timing.push((beat, TimingEvent::Bpm(*v)));
                    }
                }
                b"09" => {
                    if let Some(v) = id().and_then(|id| src.stop.get(&id)) {
                        p.timing.push((beat, TimingEvent::StopBeats(*v / 48.0)));
                    }
                }
                b"SC" => {
                    if let Some(v) = id().and_then(|id| src.scroll.get(&id)) {
                        p.timing.push((beat, TimingEvent::Scroll(*v)));
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
                        p.bga.push((beat, layer, id));
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
                    // the other PMS layout's channels
                    let conflict = match pms_layout {
                        Some(PmsLayout::Bme) => player == 1 && (b'2'..=b'5').contains(&ch[1]),
                        Some(PmsLayout::Standard) => player == 0 && (b'6'..=b'9').contains(&ch[1]),
                        None => false,
                    };
                    if conflict {
                        continue;
                    }
                    match map_key(ch[1], pms_layout, player) {
                        Some(key) => {
                            let g = gcd(i as u64, n as u64).max(1);
                            p.objs.push(RawObj {
                                player: if pms { 0 } else { player },
                                key,
                                beat,
                                pos_key: (*measure, i as u64 / g, n as u64 / g),
                                id,
                                kind,
                            })
                        }
                        // free zone (x7) and unknown lanes: keep the sound as BGM
                        None if kind == ObjKind::Visible => p.bgm.push((beat, id)),
                        None => {}
                    }
                }
            }
        }
    }
    p
}
