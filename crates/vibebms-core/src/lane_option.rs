//! Lane options (RANDOM family) and chart-transforming assist options.

use crate::chart::{Chart, Note, NoteKind};
use crate::mode::PlayMode;
use crate::rng::JavaRandom;
use serde::{Deserialize, Serialize};

/// Arrangement option per player side (same order as beatoraja).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LaneOption {
    #[default]
    Off,
    Mirror,
    Random,
    RRandom,
    SRandom,
    Spiral,
    HRandom,
    AllScratch,
    /// RANDOM including the scratch lane.
    RandomEx,
    /// S-RANDOM including the scratch lane.
    SRandomEx,
}

impl LaneOption {
    pub const ALL: [LaneOption; 10] = [
        LaneOption::Off,
        LaneOption::Mirror,
        LaneOption::Random,
        LaneOption::RRandom,
        LaneOption::SRandom,
        LaneOption::Spiral,
        LaneOption::HRandom,
        LaneOption::AllScratch,
        LaneOption::RandomEx,
        LaneOption::SRandomEx,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LaneOption::Off => "OFF",
            LaneOption::Mirror => "MIRROR",
            LaneOption::Random => "RANDOM",
            LaneOption::RRandom => "R-RANDOM",
            LaneOption::SRandom => "S-RANDOM",
            LaneOption::Spiral => "SPIRAL",
            LaneOption::HRandom => "H-RANDOM",
            LaneOption::AllScratch => "ALL-SCR",
            LaneOption::RandomEx => "RANDOM+",
            LaneOption::SRandomEx => "S-RANDOM+",
        }
    }
}

/// Double-play only options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DoubleOption {
    #[default]
    Off,
    /// Swap 1P and 2P sides.
    Flip,
}

/// Swap 1P/2P lanes of a double play chart.
pub fn flip(chart: &mut Chart) {
    let mode = chart.mode();
    if !mode.is_double() {
        return;
    }
    let lpp = mode.lanes_per_player() as u8;
    for n in &mut chart.notes {
        n.lane = (n.lane + lpp) % (lpp * 2);
    }
}

/// Convert a single play chart into a double play "battle" chart: both sides
/// get the same notes.
pub fn battle(chart: &mut Chart) {
    let mode = chart.mode();
    let dp = match mode {
        PlayMode::Beat5K => PlayMode::Beat10K,
        PlayMode::Beat7K => PlayMode::Beat14K,
        _ => return,
    };
    let lpp = mode.lanes_per_player() as u8;
    let copy: Vec<Note> = chart
        .notes
        .iter()
        .map(|n| Note {
            lane: n.lane + lpp,
            ..n.clone()
        })
        .collect();
    chart.notes.extend(copy);
    chart
        .notes
        .sort_by(|a, b| a.time.cmp(&b.time).then(a.lane.cmp(&b.lane)));
    chart.meta.mode = Some(dp);
}

/// LEGACY NOTE: turn long notes into normal notes (assist).
pub fn legacy_note(chart: &mut Chart) {
    for n in &mut chart.notes {
        if n.is_long() {
            n.kind = NoteKind::Normal;
        }
    }
}

pub fn remove_mines(chart: &mut Chart) {
    chart
        .notes
        .retain(|n| !matches!(n.kind, NoteKind::Mine { .. }));
}

/// Apply a lane option to one player side.
///
/// `hran_threshold` is the minimum interval (µs) H-RANDOM / ALL-SCR try to keep
/// between notes on the same lane.
pub fn apply(chart: &mut Chart, player: usize, opt: LaneOption, seed: i64, hran_threshold: i64) {
    let mode = chart.mode();
    if player >= mode.players() || opt == LaneOption::Off {
        return;
    }
    let mut rng = JavaRandom::new(seed);
    let base = player * mode.lanes_per_player();
    let keys: Vec<usize> = (0..mode.keys_per_player()).map(|k| base + k).collect();
    let scratch = mode.scratch_lane(player);
    let with_scratch: Vec<usize> = keys.iter().copied().chain(scratch).collect();

    let lane_map = |chart: &mut Chart, from: &[usize], to: &[usize]| {
        let mut map: Vec<u8> = (0..mode.lane_count() as u8).collect();
        for (f, t) in from.iter().zip(to) {
            map[*f] = *t as u8;
        }
        for n in &mut chart.notes {
            n.lane = map[n.lane as usize];
        }
    };

    match opt {
        LaneOption::Off => {}
        LaneOption::Mirror => {
            let rev: Vec<usize> = keys.iter().rev().copied().collect();
            lane_map(chart, &keys, &rev);
        }
        LaneOption::Random | LaneOption::RandomEx => {
            let src = if opt == LaneOption::Random {
                &keys
            } else {
                &with_scratch
            };
            let mut dst = src.clone();
            rng.shuffle(&mut dst);
            lane_map(chart, src, &dst);
        }
        LaneOption::RRandom => {
            let k = keys.len();
            let shift = 1 + rng.next_int_bounded((k - 1) as i32) as usize;
            let mirror = rng.next_int_bounded(2) == 1;
            let dst: Vec<usize> = (0..k)
                .map(|i| {
                    let j = (i + shift) % k;
                    keys[if mirror { k - 1 - j } else { j }]
                })
                .collect();
            lane_map(chart, &keys, &dst);
        }
        LaneOption::SRandom
        | LaneOption::SRandomEx
        | LaneOption::HRandom
        | LaneOption::Spiral
        | LaneOption::AllScratch => {
            per_note(chart, mode, &keys, scratch, opt, &mut rng, hran_threshold);
        }
    }
}

fn per_note(
    chart: &mut Chart,
    mode: PlayMode,
    keys: &[usize],
    scratch: Option<usize>,
    opt: LaneOption,
    rng: &mut JavaRandom,
    threshold: i64,
) {
    let pool: Vec<usize> = if opt == LaneOption::SRandomEx {
        keys.iter().copied().chain(scratch).collect()
    } else {
        keys.to_vec()
    };
    let movable = |lane: usize| -> bool {
        pool.contains(&lane) || (opt == LaneOption::AllScratch && scratch == Some(lane))
    };
    let lanes = mode.lane_count();
    let mut busy_until = vec![i64::MIN; lanes];
    let mut last_time = vec![i64::MIN / 2; lanes];
    let mut spiral_offset = 0usize;
    let spiral_dir = if rng.next_int_bounded(2) == 0 {
        1
    } else {
        keys.len() - 1
    };

    let mut i = 0;
    let notes = &mut chart.notes;
    while i < notes.len() {
        let t = notes[i].time;
        let mut j = i;
        while j < notes.len() && notes[j].time == t {
            j += 1;
        }
        // playable notes of this side at this timestamp
        let idx: Vec<usize> = (i..j)
            .filter(|&k| notes[k].is_playable() && movable(notes[k].lane as usize))
            .collect();
        if !idx.is_empty() {
            match opt {
                LaneOption::Spiral => {
                    spiral_offset = (spiral_offset + spiral_dir) % keys.len();
                    let mut used: Vec<usize> = Vec::new();
                    for &k in &idx {
                        if let Some(p) = keys.iter().position(|l| *l == notes[k].lane as usize) {
                            let mut target = keys[(p + spiral_offset) % keys.len()];
                            if busy_until[target] >= t || used.contains(&target) {
                                // keep the pattern playable: fall back to any free lane
                                if let Some(f) = keys
                                    .iter()
                                    .copied()
                                    .find(|l| busy_until[*l] < t && !used.contains(l))
                                {
                                    target = f;
                                }
                            }
                            used.push(target);
                            notes[k].lane = target as u8;
                        }
                    }
                }
                LaneOption::AllScratch => {
                    if let Some(sc) = scratch {
                        let has_sc = idx.iter().any(|&k| notes[k].lane as usize == sc);
                        if !has_sc && busy_until[sc] < t && t - last_time[sc] >= threshold {
                            let pick = idx[rng.next_int_bounded(idx.len() as i32) as usize];
                            notes[pick].lane = sc as u8;
                        }
                    }
                }
                _ => {
                    let mut free: Vec<usize> = pool
                        .iter()
                        .copied()
                        .filter(|l| busy_until[*l] < t)
                        .collect();
                    rng.shuffle(&mut free);
                    if opt == LaneOption::HRandom {
                        // lanes that would not create a fast jack come first
                        free.sort_by_key(|l| t - last_time[*l] < threshold);
                    }
                    if free.len() >= idx.len() {
                        for (n, &k) in idx.iter().enumerate() {
                            notes[k].lane = free[n] as u8;
                        }
                    }
                }
            }
        }
        for n in &notes[i..j] {
            if n.is_playable() {
                let l = n.lane as usize;
                last_time[l] = n.end_time();
                if let NoteKind::Long { end_time, .. } = n.kind {
                    busy_until[l] = end_time;
                }
            }
        }
        i = j;
    }
    notes.sort_by(|a, b| a.time.cmp(&b.time).then(a.lane.cmp(&b.lane)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bms::{parse_bms, ParseOptions};

    fn chart() -> Chart {
        let src = "#00111:01010101\n#00112:00010001\n#00113:01\n#00114:01\n#00115:01\n#00118:01\n#00119:01\n#00116:01\n#00151:0100000001\n";
        parse_bms(src.as_bytes(), "bms", &ParseOptions::default()).unwrap()
    }

    fn lanes(c: &Chart) -> Vec<(i64, u8)> {
        c.notes.iter().map(|n| (n.time, n.lane)).collect()
    }

    #[test]
    fn mirror_keeps_scratch() {
        let mut c = chart();
        apply(&mut c, 0, LaneOption::Mirror, 1, 0);
        assert!(c.notes.iter().any(|n| n.lane == 7));
        assert!(c.notes.iter().any(|n| n.lane == 6)); // key1 -> key7
    }

    #[test]
    fn per_note_randoms_never_overlap() {
        for opt in [
            LaneOption::SRandom,
            LaneOption::HRandom,
            LaneOption::Spiral,
            LaneOption::SRandomEx,
            LaneOption::AllScratch,
            LaneOption::Random,
            LaneOption::RRandom,
        ] {
            for seed in 0..20 {
                let mut c = chart();
                let count = c.notes.len();
                apply(&mut c, 0, opt, seed, 125_000);
                assert_eq!(c.notes.len(), count);
                let mut l = lanes(&c);
                l.sort();
                l.dedup();
                assert_eq!(l.len(), count, "{opt:?} produced overlapping notes");
                // no note inside a long note's body on the same lane
                for ln in c.notes.iter().filter(|n| n.is_long()) {
                    assert!(!c
                        .notes
                        .iter()
                        .any(|n| n.lane == ln.lane && n.time > ln.time && n.time <= ln.end_time()));
                }
            }
        }
    }

    #[test]
    fn battle_and_flip() {
        let mut c = chart();
        let n = c.notes.len();
        battle(&mut c);
        assert_eq!(c.mode(), PlayMode::Beat14K);
        assert_eq!(c.notes.len(), n * 2);
        flip(&mut c);
        assert_eq!(c.notes.len(), n * 2);
    }
}
