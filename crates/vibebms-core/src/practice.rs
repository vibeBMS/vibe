//! Practice: a section of a chart played alone, slower or faster, on a
//! gauge, `#RANK` and TOTAL of the player's choosing. A practice play is
//! never recorded (`record::PlayFlags::practice`).
//!
//! The section is measures `from..=to`. The play starts `lead_in`
//! measures before it, where only the music sounds: every note before the
//! section is a background sound there, so whatever was sounding as the
//! play starts can sound on (`sounding_at`). A long note that starts
//! before the section is not played (its head's sound is a background
//! sound); one that starts in it is played whole, even past its end.
//! The gauge moves by the whole chart's TOTAL and notes, as it would at
//! that point of a full play.
//!
//! At another speed the chart's times are scaled, so the judge windows
//! stay in real time (a slower play is easier) and the sounds are
//! resampled to match, their pitch with them (as a NONSTOP course's
//! tempo-matched stage).

use crate::chart::{BgmEvent, Chart, JudgeRank, Note, NoteKind, SoundId, NO_SOUND};
use crate::engine::Engine;
use crate::play::{build_engine_with, RuleOptions};
use std::collections::HashMap;

/// Speed (%): its range and step.
pub const SPEED_MIN: u32 = 50;
pub const SPEED_MAX: u32 = 200;
pub const SPEED_STEP: u32 = 5;
/// The most lead-in measures.
pub const LEAD_IN_MAX: usize = 4;

/// What a practice play is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct Practice {
    /// The section: its first and last measure.
    pub from: usize,
    pub to: usize,
    /// Measures played before the section (the music only).
    pub lead_in: usize,
    /// Speed in percent (`SPEED_MIN..=SPEED_MAX`).
    pub speed: u32,
    /// `#RANK` 0..=4 in place of the chart's.
    pub rank: Option<i32>,
    /// TOTAL in place of the chart's.
    pub total: Option<u32>,
}

impl Practice {
    pub fn rate(&self) -> f64 {
        self.speed.clamp(SPEED_MIN, SPEED_MAX) as f64 / 100.0
    }
}

/// When each measure of `chart` starts (µs, from 0), and when its last
/// ends: its bar lines, then 4 beats each up to its last note or sound.
pub fn measures(chart: &Chart) -> Vec<i64> {
    let mut bars: Vec<i64> = chart.bar_lines.iter().map(|b| b.time.max(0)).collect();
    bars.push(0);
    bars.sort_unstable();
    bars.dedup();
    let end = chart.length();
    while bars.len() < 2 || bars[bars.len() - 1] < end {
        let last = bars[bars.len() - 1];
        let beat = chart.timing.beat_at_time(last as f64) + 4.0;
        let next = chart.timing.time_at_beat(beat);
        if next <= last {
            bars.push(last + 2_000_000);
        } else {
            bars.push(next);
        }
    }
    bars
}

/// The playable notes of each measure (`bars`: `measures`).
pub fn density(chart: &Chart, bars: &[i64]) -> Vec<u32> {
    let mut n = vec![0u32; bars.len().saturating_sub(1)];
    for note in chart.notes.iter().filter(|n| n.is_playable()) {
        let m = bars.partition_point(|b| *b <= note.time).saturating_sub(1);
        if let Some(c) = n.get_mut(m) {
            *c += 1;
        }
    }
    n
}

/// A section ready to play: its chart, time 0 where the lead-in starts.
pub struct PracticeChart {
    pub chart: Chart,
    pub section: Section,
}

/// Where a section is in its chart and what moves its gauge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    /// Its start and end (µs).
    pub start: i64,
    pub end: i64,
    /// The whole chart's TOTAL and judgements.
    pub total: f64,
    pub judgements: usize,
}

/// The section `p` of `full` (read and arranged by the play's options),
/// with `rules`' LN mode and gauge set.
pub fn prepare(mut full: Chart, p: &Practice, rules: &RuleOptions) -> PracticeChart {
    if let Some(r) = p.rank {
        full.meta.judge_rank = JudgeRank::Rank(r.clamp(0, 4));
    }
    if let Some(t) = p.total {
        full.meta.total = Some(t as f64);
        full.meta.total_rate = None;
    }
    let total = full.total_of(rules.ln_mode, rules.gauge_preset, None);
    let judgements = full.total_judgements_of(rules.ln_mode, None);
    let bars = measures(&full);
    let to = p.to.min(bars.len() - 2);
    let from = p.from.min(to);
    let (start, end) = (bars[from], bars[to + 1]);
    let seek = bars[from.saturating_sub(p.lead_in)];
    let speed = p.rate();
    let at = |t: i64| ((t - seek) as f64 / speed).round() as i64;
    PracticeChart {
        chart: retime(cut(full, start, end), seek, speed),
        section: Section {
            start: at(start),
            end: at(end),
            total,
            judgements,
        },
    }
}

/// The notes of `chart` from `start` to before `end` (µs). The playable
/// notes before `start` become background sounds, its other notes are
/// dropped; a long note that starts inside is kept whole; no background
/// sound starts after the section (or its last long note) ends.
pub fn cut(mut chart: Chart, start: i64, end: i64) -> Chart {
    let mut bgm = std::mem::take(&mut chart.bgm);
    let mut kept = Vec::new();
    for n in std::mem::take(&mut chart.notes) {
        if n.time >= end {
            continue;
        }
        if n.time >= start {
            kept.push(n);
        } else if n.is_playable() && n.sound != NO_SOUND {
            bgm.push(BgmEvent {
                time: n.time,
                sound: n.sound,
            });
        }
    }
    let last = kept.iter().map(Note::end_time).fold(end, i64::max);
    bgm.retain(|b| b.time < last);
    bgm.sort_by_key(|b| b.time);
    chart.notes = kept;
    chart.bgm = bgm;
    chart
}

/// `chart` played from `from` (its time 0 there), `speed` times as fast:
/// every time `t` moved to `(t - from) / speed`, its BPMs × `speed`, its
/// stops and sound slices ÷ `speed` (its sounds must be resampled to
/// match); beats and scroll positions stay. Its first BPM is the one
/// at `from`.
pub fn retime(mut chart: Chart, from: i64, speed: f64) -> Chart {
    let at = |t: i64| ((t - from) as f64 / speed).round() as i64;
    let len = |us: i64| (us as f64 / speed).round() as i64;
    for n in &mut chart.notes {
        n.time = at(n.time);
        if let NoteKind::Long { end_time, .. } = &mut n.kind {
            *end_time = at(*end_time);
        }
    }
    for b in &mut chart.bgm {
        b.time = at(b.time);
    }
    for b in &mut chart.bga {
        b.time = at(b.time);
    }
    for b in &mut chart.bar_lines {
        b.time = at(b.time);
    }
    for s in &mut chart.sounds {
        s.start = len(s.start);
        s.end = s.end.map(len);
    }
    chart.timing = chart.timing.retimed(from, speed);
    let m = &mut chart.meta;
    m.initial_bpm = chart.timing.bpm_at_time(0.0);
    m.min_bpm *= speed;
    m.max_bpm *= speed;
    m.main_bpm *= speed;
    chart
}

/// The judge of section `s` (its `chart`): the gauge moved by the whole
/// chart's TOTAL and notes, started at `gauge_start` (`None`: the
/// gauge's own start).
pub fn engine(chart: &Chart, s: &Section, rules: &RuleOptions, gauge_start: Option<f32>) -> Engine {
    let mut e = build_engine_with(chart, rules, None, false, s.total, s.judgements);
    if let Some(v) = gauge_start {
        let current = e.gauge.current_type();
        let mut values = e.gauge.values();
        if let Some(i) = e.gauge.gauges.iter().position(|g| g.ty == current) {
            values[i] = v;
            e.gauge.set_values(&values);
        }
    }
    e
}

/// The background sounds of `bgm` (in time order) still sounding at `t`:
/// each one and how far into it `t` is (µs). A sound started again cuts
/// itself off (one voice each); `len` is a sound's length (µs).
pub fn sounding_at(bgm: &[BgmEvent], t: i64, len: impl Fn(SoundId) -> i64) -> Vec<(SoundId, i64)> {
    let mut last: HashMap<SoundId, i64> = HashMap::new();
    for b in bgm.iter().take_while(|b| b.time < t) {
        last.insert(b.sound, b.time);
    }
    let mut on: Vec<(SoundId, i64)> = last
        .into_iter()
        .map(|(s, at)| (s, t - at))
        .filter(|(s, into)| *into < len(*s))
        .collect();
    on.sort_unstable();
    on
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bms::parse_bms;
    use crate::gauge::{GaugePreset, GaugeType};
    use crate::judge::JudgePreset;

    const SEC: i64 = 1_000_000;

    /// 120 BPM (a measure 2 s), `#TOTAL 300`: key 1 at the start of
    /// measures 1-5, key 2 half way through measure 2, long notes on key 3
    /// from measure 1.5 to 2 and from 3.5 to 4.5, a mine on key 2 at 3,
    /// BGM at 0 and 3.5, 240 BPM from measure 4 (a measure 1 s).
    const TEXT: &str = "#BPM 120\n#TOTAL 300\n#LNTYPE 1\n#WAV01 a.wav\n#WAV02 b.wav\n\
                        #WAV03 c.wav\n#WAV04 bgm.wav\n#BPM01 240\n\
                        #00001:04\n#00301:0004\n#00408:01\n\
                        #00111:01\n#00211:01\n#00212:0002\n#00311:01\n#00411:01\n#00511:01\n\
                        #00153:0003\n#00253:0300\n#00353:0003\n#00453:0003\n#003D2:01\n";

    fn chart() -> Chart {
        parse_bms(TEXT.as_bytes(), "bme", &Default::default()).unwrap()
    }

    fn practice(from: usize, to: usize, lead_in: usize, speed: u32) -> Practice {
        Practice {
            from,
            to,
            lead_in,
            speed,
            rank: None,
            total: None,
        }
    }

    fn playable(c: &Chart) -> Vec<(u8, i64)> {
        c.notes
            .iter()
            .filter(|n| n.is_playable())
            .map(|n| (n.lane, n.time))
            .collect()
    }

    #[test]
    fn measures_follow_the_bar_lines_to_the_end() {
        let c = chart();
        let bars = measures(&c);
        // measures 0-3 at 2 s, measure 4 at 240 BPM (1 s), measure 5
        assert_eq!(&bars[..6], [0, 2 * SEC, 4 * SEC, 6 * SEC, 8 * SEC, 9 * SEC]);
        assert!(*bars.last().unwrap() >= c.length());
        assert_eq!(&density(&c, &bars)[..6], [0, 2, 2, 2, 1, 1]);
        // no bar lines: 4 beats each, past the last note
        let mut c = chart();
        c.bar_lines.clear();
        let bars = measures(&c);
        assert_eq!(&bars[..3], [0, 2 * SEC, 4 * SEC]);
        assert!(*bars.last().unwrap() >= c.length());
    }

    #[test]
    fn the_section_keeps_its_notes_and_starts_at_the_lead_in() {
        let rules = RuleOptions::default();
        // measures 2-3, one measure of lead-in: time 0 at measure 1 (2 s)
        let pc = prepare(chart(), &practice(2, 3, 1, 100), &rules);
        assert_eq!((pc.section.start, pc.section.end), (2 * SEC, 6 * SEC));
        // key 1 at 2 and 3, key 2 at 2.5; the long note from 3.5 (kept
        // whole past the end, to 4.5); the one from 1.5 is skipped
        let c = &pc.chart;
        assert_eq!(
            playable(c),
            [(0, 2 * SEC), (1, 3 * SEC), (0, 4 * SEC), (2, 5 * SEC)]
        );
        let ln = c.notes.iter().find(|n| n.is_long()).unwrap();
        assert_eq!(ln.end_time(), 6_500_000);
        // the mine at 3 is in the section
        assert!(c
            .notes
            .iter()
            .any(|n| matches!(n.kind, NoteKind::Mine { .. })));
        // background sounds: the BGM at 0 (-2 s), the notes before the
        // section (key 1 at 1, the skipped long note's head at 1.5) and
        // the BGM at 3.5; none after the section's last long note
        let bgm: Vec<i64> = c.bgm.iter().map(|b| b.time).collect();
        assert_eq!(bgm, [-2 * SEC, 0, SEC, 5 * SEC]);
        // beats and scroll stay; the timing follows the notes
        for n in &c.notes {
            assert!((c.timing.time_at_beat(n.beat) - n.time).abs() <= 1);
        }
        // the gauge goes by the whole chart: 6 notes and 2 long notes
        assert_eq!(pc.section.judgements, 8);
        assert_eq!(pc.section.total, 300.0);
    }

    #[test]
    fn the_first_measures_and_the_last() {
        let rules = RuleOptions::default();
        // from measure 0 with a lead-in: no measure before, time 0 at 0
        let pc = prepare(chart(), &practice(0, 1, 2, 100), &rules);
        assert_eq!((pc.section.start, pc.section.end), (0, 4 * SEC));
        assert_eq!(playable(&pc.chart), [(0, 2 * SEC), (2, 3 * SEC)]);
        // a section past the chart's end: its last measure
        let pc = prepare(chart(), &practice(90, 99, 0, 100), &rules);
        assert_eq!(playable(&pc.chart).len(), 1);
        assert_eq!(pc.section.start, 0);
    }

    #[test]
    fn speed_scales_the_timeline_and_the_sounds() {
        let rules = RuleOptions::default();
        let full = chart();
        let normal = prepare(full.clone(), &practice(3, 4, 1, 100), &rules);
        let mut sliced = full.clone();
        sliced.sounds[0].start = 100_000;
        sliced.sounds[0].end = Some(300_000);
        for speed in [50, 75, 150, 200] {
            let k = speed as f64 / 100.0;
            let pc = prepare(sliced.clone(), &practice(3, 4, 1, speed), &rules);
            let scaled = |t: i64| (t as f64 / k).round() as i64;
            assert_eq!(
                (pc.section.start, pc.section.end),
                (scaled(normal.section.start), scaled(normal.section.end))
            );
            let times = |c: &Chart| c.notes.iter().map(|n| n.time).collect::<Vec<_>>();
            let want: Vec<i64> = times(&normal.chart).into_iter().map(scaled).collect();
            assert_eq!(times(&pc.chart), want);
            // BPMs × speed: 120 then 240 at measure 4
            let c = &pc.chart;
            assert_eq!(c.meta.initial_bpm, 120.0 * k);
            assert_eq!(c.timing.bpm_at_time(pc.section.end as f64), 240.0 * k);
            assert_eq!(c.meta.max_bpm, 240.0 * k);
            // a sound's slice in the resampled sound
            assert_eq!(c.sounds[0].start, scaled(100_000));
            assert_eq!(c.sounds[0].end, Some(scaled(300_000)));
            // the scroll does not change
            let n = c.notes.iter().find(|n| n.is_playable()).unwrap();
            assert!((c.timing.pos_at_time(n.time as f64) - n.pos).abs() < 1e-4);
        }
    }

    #[test]
    fn rank_total_and_the_gauge_start() {
        let rules = RuleOptions {
            gauge: GaugeType::Hard,
            judge_preset: JudgePreset::Lr2,
            gauge_preset: GaugePreset::Lr2,
            ..Default::default()
        };
        let p = Practice {
            rank: Some(0),
            total: Some(500),
            ..practice(1, 2, 0, 50)
        };
        let pc = prepare(chart(), &p, &rules);
        assert_eq!(pc.chart.meta.judge_rank, JudgeRank::Rank(0));
        assert_eq!(pc.section.total, 500.0);
        let e = engine(&pc.chart, &pc.section, &rules, Some(40.0));
        assert_eq!(
            (e.gauge.current_type(), e.gauge.value()),
            (GaugeType::Hard, 40.0)
        );
        // VERY HARD under LR2: PGREAT ±8 ms in real time, whatever the speed
        assert_eq!(e.windows().note[0], (-8_000, 8_000));
        // the section's notes are judged, the gauge's start is the gauge's
        assert_eq!(e.score.total_notes, 4);
        let e = engine(&pc.chart, &pc.section, &rules, None);
        assert_eq!(e.gauge.value(), 100.0);
    }

    #[test]
    fn sounds_still_sounding_at_the_start() {
        let bgm = |v: &[(i64, SoundId)]| -> Vec<BgmEvent> {
            v.iter()
                .map(|&(time, sound)| BgmEvent { time, sound })
                .collect()
        };
        // sound 1 is 3 s long, sound 2 0.5 s; sound 1 again at -1 s cuts
        // the one at -4 s
        let b = bgm(&[
            (-4 * SEC, 1),
            (-2 * SEC, 2),
            (-SEC, 1),
            (-200_000, 2),
            (0, 2),
        ]);
        let len = |s: SoundId| if s == 1 { 3 * SEC } else { 500_000 };
        assert_eq!(sounding_at(&b, 0, len), [(1, SEC), (2, 200_000)]);
        assert!(sounding_at(&b, 2_500_000, len).is_empty());
        assert!(sounding_at(&b, -5 * SEC, len).is_empty());
    }
}
