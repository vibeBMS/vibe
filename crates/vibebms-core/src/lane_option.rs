//! Lane options (RANDOM family) and chart-transforming assist options.

use crate::chart::{Chart, Note, NoteKind};
use crate::mode::PlayMode;
use crate::rng::JavaRandom;
use serde::{Deserialize, Serialize};

crate::choices! {
    /// Arrangement option per player side (same order as beatoraja).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum LaneOption {
        #[default]
        Off => "OFF",
        Mirror => "MIRROR",
        Random => "RANDOM",
        RRandom => "R-RANDOM",
        SRandom => "S-RANDOM",
        Spiral => "SPIRAL",
        HRandom => "H-RANDOM",
        AllScratch => "ALL-SCR",
        /// RANDOM including the scratch lane.
        RandomEx => "RANDOM+",
        /// S-RANDOM including the scratch lane.
        SRandomEx => "S-RANDOM+",
        /// HALF RANDOM: the keys left of the middle shuffled among
        /// themselves, those right of it too; the middle key (odd key
        /// counts) and the scratch stay.
        FRandom => "F-RANDOM",
        /// Mirrored HALF RANDOM: each half shuffled onto the other side.
        MfRandom => "MF-RANDOM",
    }
}

crate::choices! {
    /// How S-RANDOM (and S-RANDOM+) keeps a note off a lane whose last
    /// note was just before it (a jack). It is kept with the play.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum SRandomInterval {
        /// LR2's: no interval looked at; only notes at the same time and
        /// held long notes are avoided.
        #[default]
        Off => "OFF (LR2)",
        /// A lane whose last note ended less than 40 ms before is taken
        /// last (beatoraja's, as bmz-player documents it).
        Ms40 => "40 MS",
        /// The interval counted in whole 120 Hz frames: a lane less than 5
        /// frames since its last note is taken last, 1 frame more for each
        /// note of the side in a row closer than 8 frames to the one
        /// before, up to 8 frames.
        Hz120 => "120 HZ",
    }
}

/// What a lane option's draw goes by besides the seed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneRules {
    /// The minimum interval (µs) H-RANDOM / ALL-SCR try to keep between
    /// notes on the same lane.
    pub hran_threshold: i64,
    pub s_random: SRandomInterval,
}

/// A 120 Hz frame (µs).
const FRAME_120HZ: f64 = 1_000_000.0 / 120.0;

impl LaneOption {
    /// beatoraja's assist level of the option: the ones that move notes
    /// to or from the scratch, or that spread notes out, are Light Assist.
    pub fn assist(self) -> crate::score::Assist {
        use crate::score::Assist;
        match self {
            LaneOption::Spiral
            | LaneOption::HRandom
            | LaneOption::AllScratch
            | LaneOption::RandomEx
            | LaneOption::SRandomEx => Assist::Light,
            _ => Assist::None,
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

/// LEGACY NOTE (assist): turn the long notes of one player's lanes
/// (`None`: all) into normal notes.
pub fn legacy_note_of(chart: &mut Chart, player: Option<usize>) {
    let mode = chart.mode();
    for n in &mut chart.notes {
        if n.is_long() && player.is_none_or(|p| mode.player_of_lane(n.lane as usize) == p) {
            n.kind = NoteKind::Normal;
        }
    }
}

/// NO MINE: remove the mines of one player's lanes (`None`: all).
pub fn remove_mines_of(chart: &mut Chart, player: Option<usize>) {
    let mode = chart.mode();
    chart.notes.retain(|n| {
        !matches!(n.kind, NoteKind::Mine { .. })
            || player.is_some_and(|p| mode.player_of_lane(n.lane as usize) != p)
    });
}

/// Apply a lane option to one player side.
///
/// `hran_threshold` is the minimum interval (µs) H-RANDOM / ALL-SCR try to keep
/// between notes on the same lane.
pub fn apply(chart: &mut Chart, player: usize, opt: LaneOption, seed: i64, hran_threshold: i64) {
    let rules = LaneRules {
        hran_threshold,
        ..Default::default()
    };
    apply_with(chart, player, opt, seed, &rules)
}

/// Apply a lane option to one player side under `rules`.
pub fn apply_with(chart: &mut Chart, player: usize, opt: LaneOption, seed: i64, rules: &LaneRules) {
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
        LaneOption::FRandom | LaneOption::MfRandom => {
            let (k, h) = (keys.len(), keys.len() / 2);
            let mut left = keys[..h].to_vec();
            let mut right = keys[k - h..].to_vec();
            rng.shuffle(&mut left);
            rng.shuffle(&mut right);
            if opt == LaneOption::MfRandom {
                std::mem::swap(&mut left, &mut right);
            }
            let dst: Vec<usize> = (left.iter())
                .chain(&keys[h..k - h])
                .chain(&right)
                .copied()
                .collect();
            lane_map(chart, &keys, &dst);
        }
        LaneOption::SRandom
        | LaneOption::SRandomEx
        | LaneOption::HRandom
        | LaneOption::Spiral
        | LaneOption::AllScratch => {
            per_note(chart, mode, &keys, scratch, opt, &mut rng, rules);
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
    rules: &LaneRules,
) {
    let threshold = rules.hran_threshold;
    let s_random = matches!(opt, LaneOption::SRandom | LaneOption::SRandomEx);
    // (120 HZ) notes of the side in a row closer than 8 frames
    let mut run = 0i64;
    let mut last_group = i64::MIN / 2;
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
            let close = (t - last_group) < (8.0 * FRAME_120HZ) as i64;
            run = if close { run + 1 } else { 0 };
            last_group = t;
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
                    } else if s_random {
                        match rules.s_random {
                            SRandomInterval::Off => {}
                            SRandomInterval::Ms40 => {
                                free.sort_by_key(|l| t - last_time[*l] < 40_000)
                            }
                            SRandomInterval::Hz120 => {
                                let frames = |dt: i64| (dt as f64 / FRAME_120HZ).floor() as i64;
                                let min = 5 + run.min(3);
                                free.sort_by_key(|l| frames(t - last_time[*l]) < min)
                            }
                        }
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

    /// The lanes each key's notes went to (`keys` keys from lane 0).
    fn moved(before: &Chart, after: &Chart) -> Vec<(u8, u8)> {
        let mut v: Vec<(u8, u8)> = (before.notes.iter())
            .zip(&after.notes)
            .map(|(a, b)| (a.lane, b.lane))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// One note on each key at its own time, then all seven at once.
    fn keys_chart() -> Chart {
        let src = "#00111:01\n#00112:0001\n#00113:000001\n#00114:00000001\n#00115:0000000001\n\
                   #00118:000000000001\n#00119:00000000000001\n\
                   #00211:01\n#00212:01\n#00213:01\n#00214:01\n#00215:01\n#00218:01\n#00219:01\n";
        parse_bms(src.as_bytes(), "bme", &ParseOptions::default()).unwrap()
    }

    #[test]
    fn half_randoms_keep_or_swap_the_halves() {
        let plain = keys_chart();
        let mut seen = std::collections::HashSet::new();
        for seed in 0..40 {
            for opt in [LaneOption::FRandom, LaneOption::MfRandom] {
                let mut c = plain.clone();
                apply(&mut c, 0, opt, seed, 0);
                let map = moved(&plain, &c);
                // a lane each, the same lane for a key's every note
                assert_eq!(map.len(), 7);
                for (from, to) in map {
                    let side = |l: u8| (l > 3) as u8 + (l >= 3) as u8;
                    // the middle key stays; MF-RANDOM swaps the halves
                    match (from, opt) {
                        (3, _) => assert_eq!(to, 3),
                        (_, LaneOption::FRandom) => assert_eq!(side(from), side(to)),
                        _ => assert_eq!(side(from), 2 - side(to)),
                    }
                    if opt == LaneOption::FRandom {
                        seen.insert((from, to));
                    }
                }
            }
        }
        // every key of a half reaches every lane of it
        assert_eq!(seen.len(), 3 * 3 + 1 + 3 * 3);
        // 5 keys: halves of 2, the middle key stays
        let mut c = parse_bms(
            b"#00111:01\n#00113:0001\n#00115:000001\n",
            "bms",
            &Default::default(),
        )
        .unwrap();
        assert_eq!(c.mode(), PlayMode::Beat5K);
        apply(&mut c, 0, LaneOption::MfRandom, 3, 0);
        let lanes: Vec<u8> = c.notes.iter().map(|n| n.lane).collect();
        assert!(lanes[0] >= 3 && lanes[1] == 2 && lanes[2] <= 1, "{lanes:?}");
        // neither is an assist
        assert_eq!(LaneOption::FRandom.assist(), crate::score::Assist::None);
        assert_eq!(LaneOption::MfRandom.assist(), crate::score::Assist::None);
    }

    /// Jacks (a key's next note on the lane just played) under S-RANDOM
    /// with each interval rule, on streams of one note at a time.
    #[test]
    fn s_random_intervals_avoid_jacks() {
        // `div` notes a measure at 300 BPM (16: 50 ms apart, 32: 25 ms),
        // keys 1 to 7 in turn
        let stream = |div: usize| {
            let keys = ["11", "12", "13", "14", "15", "18", "19"];
            let mut src = String::from("#BPM 300\n");
            for m in 1..9 {
                for (i, k) in keys.iter().enumerate() {
                    let body: String = (0..div)
                        .map(|s| if s % 7 == (i + m) % 7 { "01" } else { "00" })
                        .collect();
                    src += &format!("#{m:03}{k}:{body}\n");
                }
            }
            parse_bms(src.as_bytes(), "bme", &ParseOptions::default()).unwrap()
        };
        // jacks closer than `limit` µs over 10 seeds, past the first 4 notes
        let jacks = |plain: &Chart, rule: SRandomInterval, limit: i64| -> usize {
            let rules = LaneRules {
                s_random: rule,
                ..Default::default()
            };
            (0..10)
                .map(|seed| {
                    let mut c = plain.clone();
                    apply_with(&mut c, 0, LaneOption::SRandom, seed, &rules);
                    let mut last = [i64::MIN / 2; 8];
                    let mut n = 0;
                    for (k, note) in c.notes.iter().enumerate() {
                        let l = note.lane as usize;
                        n += (k >= 4 && note.time - last[l] < limit) as usize;
                        last[l] = note.time;
                    }
                    n
                })
                .sum()
        };
        let (s16, s32) = (stream(16), stream(32));
        // OFF: the draw as before, jacks and all
        let mut off = s16.clone();
        let mut old = s16.clone();
        apply_with(&mut off, 0, LaneOption::SRandom, 4, &LaneRules::default());
        apply(&mut old, 0, LaneOption::SRandom, 4, 0);
        assert_eq!(lanes(&off), lanes(&old));
        assert!(jacks(&s16, SRandomInterval::Off, 60_000) > 0);
        assert!(jacks(&s32, SRandomInterval::Off, 40_000) > 0);
        // 40 MS: none closer than 40 ms; at 50 ms apart it does nothing
        assert_eq!(jacks(&s32, SRandomInterval::Ms40, 40_000), 0);
        assert_eq!(
            jacks(&s16, SRandomInterval::Ms40, 60_000),
            jacks(&s16, SRandomInterval::Off, 60_000)
        );
        // 120 HZ: on a stream it keeps 8 frames (66.7 ms) apart
        assert_eq!(jacks(&s16, SRandomInterval::Hz120, 60_000), 0);
        assert_eq!(SRandomInterval::default(), SRandomInterval::Off);
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
