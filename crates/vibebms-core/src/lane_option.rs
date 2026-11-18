//! Lane options (RANDOM family) and chart-transforming assist options.

use crate::chart::{Chart, LnType, Note, NoteKind, NO_SOUND};
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
        /// RANDOM TRAINER: the keys in one fixed order of the player's
        /// (`LaneRules::trainer`) instead of a draw.
        Trainer => "TRAINER",
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
    /// TRAINER's order (`trainer_order`).
    pub trainer: String,
}

/// A Random Trainer order: the keys from the left, each once, as digits
/// from 1 ("3142576": key 3's notes on the first lane, key 1's on the
/// second, ...). The keys counted from 0; `None`: not an order.
pub fn trainer_order(s: &str) -> Option<Vec<usize>> {
    let keys: Vec<usize> = (s.chars())
        .map(|c| c.to_digit(10).map(|d| d as usize))
        .collect::<Option<_>>()?;
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    (!keys.is_empty() && sorted.iter().enumerate().all(|(i, k)| *k == i + 1))
        .then(|| keys.iter().map(|k| k - 1).collect())
}

/// An order of `n` keys as written: "1234567", the keys as they are.
pub fn plain_order(n: usize) -> String {
    (1..=n.min(9)).map(|k| char::from(b'0' + k as u8)).collect()
}

/// `order` turned `d` lanes to the right (each key one lane on, the last
/// to the first), or reversed for `d` = 0 (its mirror).
pub fn turn_order(order: &str, d: i32) -> String {
    let mut c: Vec<char> = order.chars().collect();
    let n = c.len().max(1);
    match d {
        0 => c.reverse(),
        d if d > 0 => c.rotate_right(d as usize % n),
        d => c.rotate_left(d.unsigned_abs() as usize % n),
    }
    c.into_iter().collect()
}
crate::choices! {
    /// MINE ADD (bmz-player's ADD RANDOM / NEAR / BLANK; what each adds
    /// is vibeBMS's reading): mines at the times notes of a side are, on
    /// its keys that are free then (no note there, no long note held).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum MineAdd {
        #[default]
        Off => "OFF",
        /// One free key drawn at random.
        Random => "RANDOM",
        /// The free keys beside each note.
        Near => "NEAR",
        /// Every free key.
        Blank => "BLANK",
    }
}

crate::choices! {
    /// LONGNOTE ADD: normal notes made long notes of a type.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum LnAdd {
        #[default]
        Off => "OFF",
        Ln => "LN",
        Cn => "CN",
        Hcn => "HCN",
    }
}

impl LnAdd {
    pub fn ln_type(self) -> Option<LnType> {
        match self {
            LnAdd::Off => None,
            LnAdd::Ln => Some(LnType::Ln),
            LnAdd::Cn => Some(LnType::Cn),
            LnAdd::Hcn => Some(LnType::Hcn),
        }
    }
}

/// The damage of a mine MINE ADD adds (gauge %).
pub const ADDED_MINE_DAMAGE: f32 = 2.0;

/// SCROLL REMOVE: `#SCROLL` and `#SPEED` taken off (the notes, long note
/// ends and bar lines at their beats). Whether there were any.
pub fn remove_scroll(chart: &mut Chart) -> bool {
    if !chart.timing.has_scroll_changes() {
        return false;
    }
    let t = chart.timing.without_scroll();
    for n in &mut chart.notes {
        n.pos = t.pos_at_beat(n.beat);
        if let NoteKind::Long {
            end_beat, end_pos, ..
        } = &mut n.kind
        {
            *end_pos = t.pos_at_beat(*end_beat);
        }
    }
    for b in &mut chart.bar_lines {
        b.pos = t.pos_at_time(b.time as f64);
    }
    chart.timing = t;
    true
}

/// MINE ADD on every side's keys, drawn from `seed`: whether it added
/// any.
pub fn add_mines(chart: &mut Chart, how: MineAdd, seed: i64) -> bool {
    if how == MineAdd::Off {
        return false;
    }
    let mode = chart.mode();
    let mut rng = JavaRandom::new(seed ^ 0x4d49_4e45);
    let mut busy_until = vec![i64::MIN; mode.lane_count()];
    let mut added = Vec::new();
    let notes = &chart.notes;
    let mut i = 0;
    while i < notes.len() {
        let t = notes[i].time;
        let j = i + notes[i..].iter().take_while(|n| n.time == t).count();
        let here = &notes[i..j];
        let taken = |l: usize| busy_until[l] >= t || here.iter().any(|n| n.lane as usize == l);
        for p in 0..mode.players() {
            let base = p * mode.lanes_per_player();
            let keys: Vec<usize> = (base..base + mode.keys_per_player()).collect();
            let hit: Vec<usize> = (here.iter())
                .filter(|n| n.is_playable() && keys.contains(&(n.lane as usize)))
                .map(|n| n.lane as usize)
                .collect();
            if hit.is_empty() {
                continue;
            }
            let free: Vec<usize> = keys.iter().copied().filter(|l| !taken(*l)).collect();
            let mines: Vec<usize> = match how {
                MineAdd::Off => Vec::new(),
                MineAdd::Random if free.is_empty() => Vec::new(),
                MineAdd::Random => vec![free[rng.next_int_bounded(free.len() as i32) as usize]],
                MineAdd::Near => (free.iter().copied())
                    .filter(|l| hit.iter().any(|h| h.abs_diff(*l) == 1))
                    .collect(),
                MineAdd::Blank => free,
            };
            let at = &here[0];
            added.extend(mines.into_iter().map(|lane| Note {
                lane: lane as u8,
                sound: NO_SOUND,
                kind: NoteKind::Mine {
                    damage: ADDED_MINE_DAMAGE,
                },
                ..at.clone()
            }));
        }
        for n in here {
            if let NoteKind::Long { end_time, .. } = n.kind {
                busy_until[n.lane as usize] = end_time;
            }
        }
        i = j;
    }
    let any = !added.is_empty();
    chart.notes.extend(added);
    chart
        .notes
        .sort_by(|a, b| a.time.cmp(&b.time).then(a.lane.cmp(&b.lane)));
    chart.meta.has_mine |= any;
    any
}

/// LONGNOTE ADD: each normal note made a long note of `how`'s type, to
/// half way to the next object of its lane but at most a beat on (a beat
/// after the lane's last note); none shorter than a 32nd note. Whether
/// it made any.
pub fn add_long_notes(chart: &mut Chart, how: LnAdd) -> bool {
    let Some(ty) = how.ln_type() else {
        return false;
    };
    let timing = &chart.timing;
    let notes = &mut chart.notes;
    let mut any = false;
    for k in 0..notes.len() {
        if notes[k].kind != NoteKind::Normal {
            continue;
        }
        let (lane, beat) = (notes[k].lane, notes[k].beat);
        let next = (notes[k + 1..].iter())
            .find(|n| n.lane == lane && !matches!(n.kind, NoteKind::Invisible) && n.beat > beat)
            .map(|n| n.beat);
        let len = next.map_or(1.0, |b| ((b - beat) / 2.0).min(1.0));
        if len < 0.125 {
            continue;
        }
        let end_beat = beat + len;
        notes[k].kind = NoteKind::Long {
            end_time: timing.time_at_beat(end_beat),
            end_beat,
            end_pos: timing.pos_at_beat(end_beat),
            end_sound: NO_SOUND,
            ln_type: Some(ty),
        };
        any = true;
    }
    chart.meta.has_long |= any;
    any
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
            | LaneOption::SRandomEx
            // a lane order chosen, not drawn
            | LaneOption::Trainer => Assist::Light,
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
    apply_with(chart, player, opt, seed, &rules);
}

/// Apply a lane option to one player side under `rules`: the option
/// that took effect (TRAINER with no order of the side's keys: OFF).
pub fn apply_with(
    chart: &mut Chart,
    player: usize,
    opt: LaneOption,
    seed: i64,
    rules: &LaneRules,
) -> LaneOption {
    let mode = chart.mode();
    if player >= mode.players() || opt == LaneOption::Off {
        return opt;
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
        LaneOption::Trainer => {
            let order = trainer_order(&rules.trainer).filter(|o| o.len() == keys.len());
            let Some(order) = order else {
                return LaneOption::Off;
            };
            let from: Vec<usize> = order.iter().map(|k| keys[*k]).collect();
            lane_map(chart, &from, &keys);
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
    opt
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
    fn the_trainer_plays_its_order() {
        let plain = keys_chart();
        let rules = LaneRules {
            trainer: "3142576".into(),
            ..Default::default()
        };
        for seed in [1, 99] {
            let mut c = plain.clone();
            let took = apply_with(&mut c, 0, LaneOption::Trainer, seed, &rules);
            assert_eq!(took, LaneOption::Trainer);
            // key 3's notes on lane 1, key 1's on lane 2, ...
            let map = moved(&plain, &c);
            assert_eq!(
                map,
                [(0, 1), (1, 3), (2, 0), (3, 2), (4, 4), (5, 6), (6, 5)]
            );
        }
        // the same order mirrored is MIRROR's, turned is a shift
        assert_eq!(turn_order("1234567", 0), "7654321");
        assert_eq!(turn_order("1234567", 1), "7123456");
        assert_eq!(turn_order("1234567", -2), "3456712");
        assert_eq!(plain_order(7), "1234567");
        // not an order of the side's keys: nothing changes, OFF
        for bad in ["", "1234566", "12345", "12a4567", "01234567"] {
            let mut c = plain.clone();
            let rules = LaneRules {
                trainer: bad.into(),
                ..Default::default()
            };
            assert_eq!(
                apply_with(&mut c, 0, LaneOption::Trainer, 1, &rules),
                LaneOption::Off
            );
            assert_eq!(lanes(&c), lanes(&plain));
        }
        assert_eq!(trainer_order("21"), Some(vec![1, 0]));
        assert_eq!(LaneOption::Trainer.assist(), crate::score::Assist::Light);
    }

    #[test]
    fn added_mines_go_on_free_keys_only() {
        // a long note on key 1 over measure 1, chords and notes elsewhere,
        // a 14-key side too
        let src = "#00151:01\n#00251:01\n#00112:01010101\n#00113:0001\n#00119:01\n#00118:01\n\
                   #00221:01\n";
        let plain = parse_bms(src.as_bytes(), "bme", &ParseOptions::default()).unwrap();
        let count = |how: MineAdd, seed: i64| {
            let mut c = plain.clone();
            let any = add_mines(&mut c, how, seed);
            let mines: Vec<Note> = (c.notes.iter())
                .filter(|n| matches!(n.kind, NoteKind::Mine { .. }))
                .cloned()
                .collect();
            // never on a note's lane at its time, nor over a held long note,
            // nor on the scratch
            for m in &mines {
                assert!(!plain.notes.iter().any(|n| n.lane == m.lane
                    && (n.time == m.time || (n.time < m.time && m.time <= n.end_time()))));
                assert!(!c.mode().is_scratch(m.lane as usize));
                assert_eq!(
                    m.kind,
                    NoteKind::Mine {
                        damage: ADDED_MINE_DAMAGE
                    }
                );
            }
            assert_eq!(any, !mines.is_empty());
            (mines, c)
        };
        assert!(count(MineAdd::Off, 1).0.is_empty());
        // RANDOM: one a time a side has notes (4 such times on 1P, 1 on 2P)
        let (r1, _) = count(MineAdd::Random, 1);
        assert_eq!(r1.len(), 5);
        assert_eq!(r1, count(MineAdd::Random, 1).0);
        // BLANK: every free key; NEAR: the free keys beside a note
        let (blank, c) = count(MineAdd::Blank, 1);
        let (near, _) = count(MineAdd::Near, 1);
        assert!(near.len() < blank.len() && !near.is_empty());
        assert!(c.meta.has_mine);
        for m in &near {
            assert!(plain.notes.iter().any(|n| n.time == m.time
                && n.is_playable()
                && (n.lane as i32 - m.lane as i32).abs() == 1));
        }
    }

    #[test]
    fn added_long_notes_end_before_the_next_note() {
        // 120 BPM: key 1 at beats 4, 6 and 6.125, key 2 at 4 and 8
        let key1 = format!("01{}0101{}", "00".repeat(15), "00".repeat(14));
        let src = format!("#BPM 120\n#00111:{key1}\n#00112:01\n#00212:01\n");
        let mut c = parse_bms(src.as_bytes(), "bme", &ParseOptions::default()).unwrap();
        assert_eq!(c.notes.len(), 5);
        assert!(!add_long_notes(&mut c, LnAdd::Off));
        assert!(add_long_notes(&mut c, LnAdd::Hcn));
        let long: Vec<(u8, f64, f64)> = (c.notes.iter())
            .filter_map(|n| match n.kind {
                NoteKind::Long {
                    end_beat,
                    ln_type: Some(LnType::Hcn),
                    end_time,
                    ..
                } => {
                    assert_eq!(end_time, c.timing.time_at_beat(end_beat));
                    Some((n.lane, n.beat, end_beat))
                }
                _ => None,
            })
            .collect();
        // key 1 at 4: a beat (half way to 6 is more); at 6: too short (a
        // 32nd before the next); the last ones: a beat
        assert_eq!(
            long,
            [
                (0, 4.0, 5.0),
                (1, 4.0, 5.0),
                (0, 6.125, 7.125),
                (1, 8.0, 9.0)
            ]
        );
        assert!(c.meta.has_long);
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
