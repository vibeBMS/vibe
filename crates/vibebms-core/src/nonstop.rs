//! A NONSTOP course's chart: its stages joined into one (S-OLR2b 3).
//!
//! Each stage is read on its own (`bms::parse_stage`); stage N+1 is then
//! shifted so that one of its bar lines falls on the bar line of stage N
//! nearest stage N's last note (3.2), played faster or slower by a tempo
//! ratio (3.3), and its notes that would come too early become background
//! sounds. Unlike LR2, every stage is placed by the second stage's rule
//! (LR2 places the third and later ones a stage's trim earlier) and a
//! mine keeps its own value (LR2 adds 1296 per stage to it):
//! `docs/RULES.md`, "LR2 courses".

use crate::bms::StageFacts;
use crate::chart::*;
use crate::course::Join;
use crate::judge::{judge_rules, lr2_nonstop_rules, JudgePreset, JudgeRules};
use crate::mode::PlayMode;
use crate::timing::TimingMap;
use crate::ChartError;
use std::ops::Range;

/// A NONSTOP course scrolls by time, as a 150 BPM chart (S-OLR2b 3.3).
pub const SCROLL_BPM: f64 = 150.0;
/// Stage N+1's notes this close after stage N's last become background
/// sounds (3.2).
const OVERLAP: i64 = 100_000;
/// Gap 2 moves the join this far on at least (3.2).
const GAP2: i64 = 5_000_000;
/// The BGA of two stages cross-fades over this (3.1).
const BGA_FADE: i64 = 3_000_000;

/// Where a stage is in the joined chart.
#[derive(Clone, Debug, PartialEq)]
pub struct StageSpan {
    pub title: String,
    /// When its own chart starts (its measure 0), in the joined chart's
    /// time: its `#RANK` takes effect then (3.5).
    pub start: i64,
    /// The join into it (the first stage: 0).
    pub join: i64,
    /// Its speed (and its sounds' pitch): tempo matched, stage N's BPM at
    /// the join over its own `#BPM` (3.3).
    pub ratio: f64,
    /// Its sounds (`Chart::sounds`), sound files and images.
    pub sounds: Range<u32>,
    pub files: Range<u32>,
    pub images: Range<u32>,
    /// Its `#RANK`, when it has one.
    pub rank: Option<JudgeRank>,
    /// How long its sound fades in before its join, and out after the
    /// next (µs; 0: not faded).
    pub fade_in: i64,
    pub fade_out: i64,
}

/// The stages of a joined chart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nonstop {
    pub stages: Vec<StageSpan>,
    /// The stage of each BGA event (`Chart::bga`).
    pub bga_stage: Vec<usize>,
}

/// Measure and the part of it (0..1) at `beat`, of a stage whose measures
/// start at `starts` (4 beats each past its end).
fn measure_at(starts: &[f64], beat: f64) -> f64 {
    let last = starts.len().saturating_sub(1);
    let i = starts.partition_point(|s| *s <= beat).saturating_sub(1);
    if i >= last {
        let end = starts.get(last).copied().unwrap_or(0.0);
        return last as f64 + (beat - end) / 4.0;
    }
    let len = (starts[i + 1] - starts[i]).max(1e-9);
    i as f64 + (beat - starts[i]) / len
}

/// The beat measure `m` starts at.
fn measure_beat(starts: &[f64], m: usize) -> f64 {
    match starts.get(m) {
        Some(b) => *b,
        None => {
            let last = starts.len().saturating_sub(1);
            starts.get(last).copied().unwrap_or(0.0) + 4.0 * (m - last) as f64
        }
    }
}

/// A stage's lane `lane` in the joined chart's mode `to` (a 5-key stage
/// on a 7-key course: its scratch on the 7-key scratch).
fn lane_in(from: PlayMode, to: PlayMode, lane: u8) -> u8 {
    let (lpp, keys) = (from.lanes_per_player(), from.keys_per_player());
    let (p, k) = (lane as usize / lpp, lane as usize % lpp);
    let k = if from.has_scratch() && k == keys {
        to.keys_per_player()
    } else {
        k
    };
    (p * to.lanes_per_player() + k) as u8
}

/// The mode a course of these stages plays in: 5- and 7-key stages as
/// 7 keys (10 and 14 as 14); others must all be alike.
fn course_mode(modes: &[PlayMode]) -> Result<PlayMode, ChartError> {
    let first = modes[0];
    let fits = |m: PlayMode| {
        m == first || (m.players() == first.players() && m.has_scratch() && first.has_scratch())
    };
    if !modes.iter().all(|m| fits(*m)) {
        return Err(ChartError::Parse(
            "a NONSTOP course's stages must be of one key mode".into(),
        ));
    }
    Ok(*modes.iter().max_by_key(|m| m.keys_per_player()).unwrap_or(&first))
}

/// The playable notes' beats (an LN to its end).
fn note_beats(c: &Chart) -> impl Iterator<Item = (f64, f64)> + '_ {
    c.notes.iter().filter(|n| n.is_playable()).map(|n| match n.kind {
        NoteKind::Long { end_beat, .. } => (n.beat, end_beat),
        _ => (n.beat, n.beat),
    })
}

/// Joins `stages` (each read with `bms::parse_stage`, in order) with
/// `joins` (one fewer): the joined chart and where its stages are.
pub fn join(
    stages: Vec<(Chart, StageFacts)>,
    joins: &[Join],
) -> Result<(Chart, Nonstop), ChartError> {
    if stages.is_empty() {
        return Err(ChartError::Empty);
    }
    let modes: Vec<PlayMode> = stages.iter().map(|(c, _)| c.mode()).collect();
    let mode = course_mode(&modes)?;
    let mut out = Chart {
        meta: stages[0].0.meta.clone(),
        ..Chart::default()
    };
    out.meta.mode = Some(mode);
    let mut spans: Vec<StageSpan> = Vec::new();
    // each stage's bar lines (stage, index, time), BPM changes and stops
    // in joined time, and the first bar index it keeps
    let mut bars: Vec<(usize, i64)> = Vec::new();
    let mut tempo: Vec<(usize, i64, f64)> = Vec::new();
    let mut stops: Vec<(usize, i64, i64)> = Vec::new();
    let mut bga: Vec<(BgaEvent, usize)> = Vec::new();
    // the previous stage: its chart, facts, offset and ratio, and its
    // last note's (or LN head's, or mine's) time
    let mut prev: Option<(Chart, StageFacts, f64, f64, i64)> = None;
    for (k, (c, f)) in stages.into_iter().enumerate() {
        let join = joins.get(k.wrapping_sub(1)).copied().unwrap_or_default();
        let (offset, ratio, at) = match &prev {
            None => (0.0, 1.0, 0i64),
            Some((pc, pf, po, pr, _)) => {
                // 1, 2: the bar line nearest stage N's last note (a tie:
                // the earlier); gap 2 the first more than 5 s after it
                let last = note_beats(pc).map(|b| b.1).fold(0.0, f64::max);
                let pos = measure_at(&pf.measure_beats, last);
                let mut bar = pos.floor() as usize;
                if pos - bar as f64 > 0.5 + 1e-9 {
                    bar += 1;
                }
                let t_bar = |m| pc.timing.time_at_beat(measure_beat(&pf.measure_beats, m));
                let mut t_join = t_bar(bar);
                if join == Join::Gap2 {
                    let after = t_join + GAP2;
                    while t_join <= after {
                        bar += 1;
                        t_join = t_bar(bar);
                    }
                }
                let at = (po + t_join as f64 / pr).round() as i64;
                // 3.3: tempo matched to stage N's BPM at the join, as played
                let ratio = if join.tempo_match() && f.bpm_given && c.meta.initial_bpm > 0.0 {
                    pc.timing.bpm_at_time(t_join as f64) * pr / c.meta.initial_bpm
                } else {
                    1.0
                };
                // 3: stage N+1's bar line to put there, by its first note
                let first = note_beats(&c).map(|b| b.0).reduce(f64::min);
                let p = measure_at(&f.measure_beats, first.unwrap_or(0.0));
                let m = match join {
                    Join::Gap2 => 0.0,
                    Join::Gap1 => (p - 1.5).ceil(),
                    _ => (p - 0.5).ceil(),
                }
                .max(0.0) as usize;
                let own = c.timing.time_at_beat(measure_beat(&f.measure_beats, m));
                (at as f64 - own as f64 / ratio, ratio, at)
            }
        };
        let to = |t: i64| (offset + t as f64 / ratio).round() as i64;
        // 6: what comes at or before stage N's last note (+ 100 ms) is
        // background sound, but a note within 100 ms of it on a lane
        // stage N left free in its last 100 ms
        let (limit, busy): (i64, Vec<u8>) = match &prev {
            Some((pc, _, po, pr, last)) => {
                let busy = pc
                    .notes
                    .iter()
                    .filter(|n| n.is_playable() || matches!(n.kind, NoteKind::Mine { .. }))
                    .filter(|n| {
                        let t = (po + n.time as f64 / pr).round() as i64;
                        (last.saturating_sub(OVERLAP)..=*last).contains(&t)
                    })
                    .map(|n| lane_in(pc.mode(), mode, n.lane))
                    .collect();
                (last.saturating_add(OVERLAP), busy)
            }
            None => (i64::MIN, Vec::new()),
        };
        let last_before = prev.as_ref().map_or(i64::MIN, |p| p.4);
        // stage N's long notes still held (lane, end), where N+1 may not
        // start a note
        let held: Vec<(u8, i64)> = out
            .notes
            .iter()
            .filter(|n| n.end_time() > at)
            .filter_map(|n| match n.kind {
                NoteKind::Long { end_time, .. } => Some((n.lane, end_time)),
                _ => None,
            })
            .collect();
        let s0 = out.sounds.len() as u32;
        let f0 = out.sound_files.len() as u32;
        let i0 = out.images.len() as u32;
        let snd = |s: SoundId| if s == NO_SOUND { s } else { s + s0 };
        out.sound_files.extend(c.sound_files.iter().cloned());
        out.sounds.extend(c.sounds.iter().map(|s| SoundSlice {
            file: s.file + f0,
            ..s.clone()
        }));
        out.images.extend(c.images.iter().cloned());
        let mut last = i64::MIN;
        for n in &c.notes {
            let time = to(n.time);
            let lane = lane_in(c.mode(), mode, n.lane);
            let scored = n.is_playable() || matches!(n.kind, NoteKind::Mine { .. });
            let free = last_before != i64::MIN
                && (time - last_before).abs() < OVERLAP
                && !busy.contains(&lane);
            let inside = held.iter().any(|(l, e)| *l == lane && time <= *e);
            if scored && ((time <= limit && !free) || inside) {
                if n.is_playable() && n.sound != NO_SOUND {
                    out.bgm.push(BgmEvent {
                        time,
                        sound: snd(n.sound),
                    });
                }
                continue;
            }
            if scored {
                last = last.max(time);
            }
            let kind = match &n.kind {
                NoteKind::Long {
                    end_time,
                    end_sound,
                    ln_type,
                    ..
                } => NoteKind::Long {
                    end_time: to(*end_time),
                    end_beat: 0.0,
                    end_pos: 0.0,
                    end_sound: snd(*end_sound),
                    ln_type: *ln_type,
                },
                k => k.clone(),
            };
            out.notes.push(Note {
                lane,
                time,
                beat: 0.0,
                pos: 0.0,
                sound: snd(n.sound),
                kind,
            });
        }
        out.bgm.extend(c.bgm.iter().map(|b| BgmEvent {
            time: to(b.time),
            sound: snd(b.sound),
        }));
        bga.extend(c.bga.iter().map(|b| {
            let e = BgaEvent {
                time: to(b.time),
                layer: b.layer,
                image: b.image.map(|i| i + i0),
            };
            (e, k)
        }));
        // 4, 5: its bar lines from the one put on the join, its tempo from
        // the join (the previous stage's end at it)
        let first_bar = if k == 0 {
            0
        } else {
            c.bar_lines.iter().position(|b| to(b.time) >= at).unwrap_or(0)
        };
        bars.extend(c.bar_lines[first_bar..].iter().map(|b| (k, to(b.time))));
        let at_own = ((at as f64 - offset) * ratio).round();
        tempo.push((k, at, c.timing.bpm_at_time(at_own) * ratio));
        tempo.extend(
            c.timing
                .bpm_changes()
                .into_iter()
                .filter(|b| to(b.time) > at)
                .map(|b| (k, to(b.time), b.bpm * ratio)),
        );
        stops.extend(
            c.timing
                .stops
                .iter()
                .filter(|s| to(s.time) >= at)
                .map(|s| (k, to(s.time), (s.duration as f64 / ratio).round() as i64)),
        );
        let m = &mut out.meta;
        if c.meta.total.is_some() {
            m.total = c.meta.total;
        }
        m.has_bga |= c.meta.has_bga;
        m.has_stop |= c.meta.has_stop;
        m.has_mine |= c.meta.has_mine;
        m.has_long |= c.meta.has_long;
        m.has_invisible |= c.meta.has_invisible;
        m.has_random |= c.meta.has_random;
        if k > 0 {
            m.random_selections
                .extend_from_slice(&c.meta.random_selections);
        }
        if let Some(prev) = spans.last_mut() {
            prev.fade_out = join.fade_out();
        }
        spans.push(StageSpan {
            title: c.meta.title.clone(),
            start: to(0),
            join: at,
            ratio,
            sounds: s0..out.sounds.len() as u32,
            files: f0..out.sound_files.len() as u32,
            images: i0..out.images.len() as u32,
            rank: f.rank_given.then_some(c.meta.judge_rank),
            fade_in: if k == 0 { 0 } else { join.fade_in() },
            fade_out: 0,
        });
        prev = Some((c, f, offset, ratio, last));
    }
    // a stage's bar lines, tempo and stops up to the next join
    let ends: Vec<i64> = spans
        .iter()
        .skip(1)
        .map(|s| s.join)
        .chain([i64::MAX])
        .collect();
    let before_end = |k: usize, t: i64| t < ends[k];
    let changes: Vec<(i64, f64)> = tempo
        .into_iter()
        .filter(|(k, t, _)| before_end(*k, *t))
        .map(|(_, t, b)| (t, b))
        .collect();
    let stops: Vec<(i64, i64)> = stops
        .into_iter()
        .filter(|(k, t, _)| before_end(*k, *t))
        .map(|(_, t, d)| (t, d))
        .collect();
    out.timing = TimingMap::of_times(&changes, &stops, SCROLL_BPM);
    let tm = &out.timing;
    out.bar_lines = bars
        .into_iter()
        .filter(|(k, t)| before_end(*k, *t))
        .map(|(_, time)| BarLine {
            time,
            pos: tm.pos_at_time(time as f64),
        })
        .collect();
    for n in &mut out.notes {
        n.beat = tm.beat_at_time(n.time as f64);
        n.pos = tm.pos_at_beat(n.beat);
        if let NoteKind::Long {
            end_time,
            end_beat,
            end_pos,
            ..
        } = &mut n.kind
        {
            *end_beat = tm.beat_at_time(*end_time as f64);
            *end_pos = tm.pos_at_beat(*end_beat);
        }
    }
    out.notes.sort_by(|a, b| a.time.cmp(&b.time).then(a.lane.cmp(&b.lane)));
    out.bgm.sort_by_key(|b| b.time);
    bga.sort_by_key(|b| b.0.time);
    let (events, bga_stage) = bga.into_iter().unzip();
    out.bga = events;
    out.compute_bpm_stats();
    // the scroll's BPM, which hi-speed and the green number go by
    out.meta.main_bpm = SCROLL_BPM;
    out.meta.initial_bpm = changes.first().map_or(SCROLL_BPM, |c| c.1);
    Ok((
        out,
        Nonstop {
            stages: spans,
            bga_stage,
        },
    ))
}

impl Nonstop {
    /// The stage playing at `t`: it advances at every join (LR2 advances
    /// only at the gap joins, S-OLR2b 3.7).
    pub fn stage_at(&self, t: i64) -> usize {
        self.stages
            .iter()
            .rposition(|s| s.join <= t)
            .unwrap_or(0)
    }

    /// The stage of sound `sound`.
    pub fn stage_of_sound(&self, sound: SoundId) -> usize {
        self.stages
            .iter()
            .position(|s| s.sounds.contains(&sound))
            .unwrap_or(0)
    }


    /// Stage `k`'s sound volume at `t`: faded in before its join, out
    /// after the next, linearly in a value then squared (S-OLR2b 3.1).
    pub fn volume(&self, k: usize, t: i64) -> f32 {
        let s = &self.stages[k];
        let next = self.stages.get(k + 1).map(|n| n.join);
        let x = ramp(s.join, s.fade_in, t) * next.map_or(1.0, |j| fade(j, s.fade_out, t));
        x * x
    }

    /// Stage `k`'s BGA opacity at `t`: in over the 3 s before its join,
    /// out over the 3 s after the next (S-OLR2b 3.1).
    pub fn bga_alpha(&self, k: usize, t: i64) -> f32 {
        let s = &self.stages[k];
        let next = self.stages.get(k + 1).map(|n| n.join);
        let fade_in = if k == 0 { 0 } else { BGA_FADE };
        ramp(s.join, fade_in, t) * next.map_or(1.0, |j| fade(j, BGA_FADE, t))
    }

    /// The rules each later stage with a `#RANK` takes on, from its start
    /// (S-OLR2b 3.5): LR2's NONSTOP table under the LR2 judge, else the
    /// preset's own for that rank.
    pub fn rule_switches(
        &self,
        preset: JudgePreset,
        fam: crate::mode::RuleFamily,
    ) -> Vec<(i64, JudgeRules)> {
        self.stages
            .iter()
            .skip(1)
            .filter_map(|s| {
                let rank = s.rank?;
                let rules = match preset {
                    JudgePreset::Lr2 => lr2_nonstop_rules(fam, rank),
                    p => judge_rules(p, fam, rank, None),
                };
                Some((s.start, rules))
            })
            .collect()
    }
}

/// A fade in of `len` µs that ends at `end`: 0 before `end - len`,
/// rising to 1 at `end` (`len` 0: 1 throughout).
fn ramp(end: i64, len: i64, t: i64) -> f32 {
    if len <= 0 {
        return 1.0;
    }
    ((t - (end - len)) as f64 / len as f64).clamp(0.0, 1.0) as f32
}

/// A fade out of `len` µs from `start` (`len` 0: 1 throughout).
fn fade(start: i64, len: i64, t: i64) -> f32 {
    if len <= 0 {
        return 1.0;
    }
    1.0 - ramp(start + len, len, t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bms::{parse_stage, Shared};
    use crate::gauge::GaugePreset;
    use crate::judge::Judge;
    use crate::mode::RuleFamily;

    const SEC: i64 = 1_000_000;

    /// 120 BPM (a measure 2 s), `#TOTAL 300`: key 1 at 2 s, key 2 at
    /// measure 3.25 (6.5 s), its last note: the join is at bar 3, 6 s.
    const A: &str = "#BPM 120\n#TOTAL 300\n#WAV01 a.wav\n#00111:01\n#00312:00010000\n";
    /// 150 BPM (1.6 s), `#RANK 0`: key 1 at measure 2.5 and 4.
    const B: &str = "#BPM 150\n#RANK 0\n#WAV02 b.wav\n#00211:0002\n#00411:02\n";
    /// 120 BPM, `#TOTAL 250`: key 3 at measure 1.6, keys 2 and 4 at 2.25,
    /// key 1 at 2.3, key 5 at 2.375.
    const C: &str = "#BPM 120\n#TOTAL 250\n#WAV03 c.wav\n#00113:0000000300\n\
                     #00212:00030000\n#00214:00030000\n#00211:00000003000000000000\n\
                     #00215:0000000300000000\n";

    fn joined(texts: &[&str], joins: &[Join]) -> (Chart, Nonstop) {
        let mut shared = Shared::new(1);
        let stages = texts
            .iter()
            .map(|t| parse_stage(t.as_bytes(), "bme", &mut shared).unwrap())
            .collect();
        join(stages, joins).unwrap()
    }

    fn times(c: &Chart) -> Vec<(u8, i64)> {
        c.notes
            .iter()
            .filter(|n| n.is_playable())
            .map(|n| (n.lane, n.time))
            .collect()
    }

    #[test]
    fn the_join_bar_and_the_aligned_bar_per_join_type() {
        // fade, cut (and their tempo-matched forms below): B's first note
        // (measure 2.5) puts its bar 2 (3.2 s) on A's bar 3 (6 s)
        for j in [Join::Fade, Join::Cut] {
            let (c, ns) = joined(&[A, B], &[j]);
            assert_eq!(ns.stages[1].join, 6 * SEC);
            assert_eq!(ns.stages[1].start, 2_800_000);
            assert_eq!(times(&c), [(0, 2 * SEC), (1, 6_500_000), (0, 6_800_000), (0, 9_200_000)]);
        }
        // gap 1: the first note 0.5 to 1.5 measures after the join (bar 1)
        let (c, ns) = joined(&[A, B], &[Join::Gap1]);
        assert_eq!(ns.stages[1].start, 4_400_000);
        assert_eq!(times(&c)[2], (0, 8_400_000));
        // gap 2: the join on A's first bar more than 5 s after bar 3 (bar
        // 6, 12 s), B untrimmed
        let (c, ns) = joined(&[A, B], &[Join::Gap2]);
        assert_eq!((ns.stages[1].join, ns.stages[1].start), (12 * SEC, 12 * SEC));
        assert_eq!(times(&c)[2], (0, 16 * SEC));
        // a last note half way through a measure: the earlier bar line
        let tie = "#BPM 120\n#WAV01 a.wav\n#00312:0001\n";
        let (_, ns) = joined(&[tie, B], &[Join::Cut]);
        assert_eq!(ns.stages[1].join, 6 * SEC);
        // A's bar lines up to the join, B's from it
        let (c, _) = joined(&[A, B], &[Join::Cut]);
        let bars: Vec<i64> = c.bar_lines.iter().map(|b| b.time).collect();
        assert_eq!(&bars[..4], [0, 2 * SEC, 4 * SEC, 6 * SEC]);
        assert_eq!(bars[4], 7_600_000);
    }

    #[test]
    fn tempo_match_plays_the_next_stage_at_the_last_ones_bpm() {
        let (c, ns) = joined(&[A, B], &[Join::FadeMatch]);
        // 120 / 150: B slower, bar 2 at 4 s of its time
        assert!((ns.stages[1].ratio - 0.8).abs() < 1e-12);
        assert_eq!(ns.stages[1].start, 2 * SEC);
        assert_eq!(times(&c)[2], (0, 7 * SEC));
        assert!((c.timing.bpm_at_time(7.5e6) - 120.0).abs() < 1e-9);
        // no #BPM line: not matched
        let no_bpm = "#WAV02 b.wav\n#00211:0002\n";
        let (_, ns) = joined(&[A, no_bpm], &[Join::CutMatch]);
        assert_eq!(ns.stages[1].ratio, 1.0);
        // the scroll goes by time, as 150 BPM
        assert_eq!(c.meta.main_bpm, SCROLL_BPM);
        let n = &c.notes[2];
        assert!((n.pos - c.timing.pos_at_time(n.time as f64)).abs() < 1e-9);
        assert!((c.timing.pos_at_time(2e6) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn overlap_becomes_background_sound() {
        // C's bar 2 (4 s) on A's bar 3 (6 s): its notes come 2 s later.
        // A's last note: key 2 at 6.5 s
        let (c, _) = joined(&[A, C], &[Join::Cut]);
        // key 3 at 5.2 s, key 1 at 6.6 s (at the limit) and key 2 at 6.5 s
        // (A's last 100 ms used key 2) are background sounds; key 4 at
        // 6.5 s (a free lane, within 100 ms) and key 5 at 6.75 s stay
        assert_eq!(
            times(&c)[2..],
            [(3, 6_500_000), (4, 6_750_000)]
        );
        let bgm: Vec<i64> = c.bgm.iter().map(|b| b.time).collect();
        assert_eq!(bgm, [5_200_000, 6_500_000, 6_600_000]);
        assert_eq!(c.note_count(), 4);
    }

    #[test]
    fn total_and_factor_of_the_whole_course() {
        // the last #TOTAL read: C's 250 (B has none)
        let (c, _) = joined(&[A, B, C], &[Join::Cut, Join::Cut]);
        assert_eq!(c.meta.total, Some(250.0));
        assert_eq!(c.total_of(LnType::Ln, GaugePreset::Lr2, None), 250.0);
        // A then B: A's 300
        let (c, _) = joined(&[A, B], &[Join::Cut]);
        assert_eq!(c.total_of(LnType::Ln, GaugePreset::Lr2, None), 300.0);
        // none: LR2's default of every stage's notes together
        let plain = "#BPM 120\n#WAV01 a.wav\n#00111:01\n#00312:00010000\n";
        let (c, _) = joined(&[plain, B], &[Join::Cut]);
        assert_eq!(c.total_judgements_of(LnType::Ln, None), 4);
        assert_eq!(c.total_of(LnType::Ln, GaugePreset::Lr2, None), 160.0);
        // HARD's factor from those: 4 notes, TOTAL 160 -> 10
        let g = crate::gauge::GrooveGauge::new(
            GaugePreset::Lr2,
            RuleFamily::SevenKeys,
            crate::gauge::GaugeType::Hard,
            crate::gauge::GaugeAutoShift::None,
            160.0,
            4,
        );
        assert_eq!(g.current().change_for(Judge::Poor), -100.0);
    }

    #[test]
    fn windows_switch_at_a_ranked_stages_start() {
        let (c, ns) = joined(&[A, B], &[Join::Cut]);
        let sw = ns.rule_switches(JudgePreset::Lr2, RuleFamily::SevenKeys);
        assert_eq!(sw.len(), 1);
        assert_eq!(sw[0].0, 2_800_000);
        assert_eq!(sw[0].1.windows.note[0], (-12_000, 12_000));
        // the engine takes them on: 15 ms early is a PGREAT on A (NORMAL),
        // a GREAT on B (#RANK 0: NONSTOP's 12 ms)
        let rules = crate::play::RuleOptions {
            judge_preset: JudgePreset::Lr2,
            gauge_preset: GaugePreset::Lr2,
            ..Default::default()
        };
        let (mut e, _) = crate::play::build_engine(&c, &rules, None, false);
        e.switch_rules(sw);
        let mut press = |t: i64, lane: u8| {
            for down in [true, false] {
                e.input(crate::engine::InputEvent {
                    time: t,
                    lane,
                    key: 0,
                    pressed: down,
                });
                e.update(t);
            }
        };
        press(2 * SEC - 15_000, 0);
        press(6_800_000 - 15_000, 0);
        // (A's note at 6.5 s not played)
        assert_eq!(e.ghost, [Judge::PGreat, Judge::Poor, Judge::Great]);
        // a later stage without #RANK keeps the windows
        let (_, ns) = joined(&[B, A], &[Join::Cut]);
        assert!(ns.rule_switches(JudgePreset::Lr2, RuleFamily::SevenKeys).is_empty());
    }

    #[test]
    fn fades_and_the_current_stage() {
        let (_, ns) = joined(&[A, B], &[Join::Fade]);
        let j = 6 * SEC;
        // A: 10 s out after the join; B: 5 s in before it, squared
        assert_eq!(ns.volume(0, j), 1.0);
        assert!((ns.volume(0, j + 5 * SEC) - 0.25).abs() < 1e-6);
        assert_eq!(ns.volume(0, j + 10 * SEC), 0.0);
        assert_eq!(ns.volume(1, j - 5 * SEC), 0.0);
        assert!((ns.volume(1, j - 2_500_000) - 0.25).abs() < 1e-6);
        assert_eq!(ns.volume(1, j), 1.0);
        // gap 2: neither fades
        let (_, ns) = joined(&[A, B], &[Join::Gap2]);
        assert_eq!(ns.volume(0, 20 * SEC), 1.0);
        assert_eq!(ns.volume(1, 0), 1.0);
        // the BGA cross-fades over 3 s each side
        let (_, ns) = joined(&[A, B], &[Join::Cut]);
        assert!((ns.bga_alpha(0, j + 1_500_000) - 0.5).abs() < 1e-6);
        assert!((ns.bga_alpha(1, j - 1_500_000) - 0.5).abs() < 1e-6);
        // the current stage: at every join
        assert_eq!((ns.stage_at(j - 1), ns.stage_at(j)), (0, 1));
    }

    #[test]
    fn five_key_stages_on_a_seven_key_course() {
        // A is a 5-key chart: its scratch on the 7-key scratch
        let seven = "#BPM 120\n#WAV01 a.wav\n#00118:01\n";
        let five = "#BPM 120\n#WAV01 a.wav\n#00116:01\n#00111:00000001\n";
        let (c, _) = joined(&[seven, five], &[Join::Cut]);
        assert_eq!(c.mode(), PlayMode::Beat7K);
        assert!(c.notes.iter().any(|n| n.lane == 7));
        let mut shared = Shared::new(1);
        let pms = parse_stage(b"#WAV01 a.wav\n#00111:01\n", "pms", &mut shared).unwrap();
        let bme = parse_stage(A.as_bytes(), "bme", &mut shared).unwrap();
        assert!(join(vec![bme, pms], &[Join::Cut]).is_err());
    }
}
