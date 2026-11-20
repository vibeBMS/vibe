//! Beat / time / scroll-position conversion.
//!
//! Times are microseconds (`f64` internally, `i64` at the API boundary).

#[derive(Clone, Debug, PartialEq)]
struct TimeSeg {
    beat: f64,
    /// Time at which this segment starts (after any STOP at `beat`).
    time: f64,
    bpm: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct ScrollSeg {
    beat: f64,
    pos: f64,
    scroll: f64,
}

/// A `#SPEED` value from a time on (µs).
#[derive(Clone, Copy, Debug, PartialEq)]
struct SpeedPoint {
    time: f64,
    speed: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StopEvent {
    pub beat: f64,
    pub time: i64,
    pub duration: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BpmChange {
    pub time: i64,
    pub bpm: f64,
}

/// Piecewise-linear mapping between beats, real time and scroll position,
/// and the `#SPEED` multiplier of the whole lane over time.
#[derive(Clone, PartialEq)]
pub struct TimingMap {
    segs: Vec<TimeSeg>,
    scroll: Vec<ScrollSeg>,
    pub stops: Vec<StopEvent>,
    /// `#SPEED` points in time order (none: 1 throughout).
    speeds: Vec<SpeedPoint>,
    /// Fold points (µs, in time order): where a BMS chart's negative BPM
    /// takes effect (LR2's backward scroll). None in most charts.
    folds: Vec<i64>,
}

/// (Without `#SPEED` points, as it was before them: the parser corpus's
/// digests of charts without them stay.)
impl std::fmt::Debug for TimingMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("TimingMap");
        d.field("segs", &self.segs)
            .field("scroll", &self.scroll)
            .field("stops", &self.stops);
        if !self.speeds.is_empty() {
            d.field("speeds", &self.speeds);
        }
        if !self.folds.is_empty() {
            d.field("folds", &self.folds);
        }
        d.finish()
    }
}

impl Default for TimingMap {
    fn default() -> Self {
        TimingMap::new(130.0)
    }
}

/// A raw timing event used while building a [`TimingMap`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimingEvent {
    Bpm(f64),
    /// Stop length in beats (BMS: value / 48).
    StopBeats(f64),
    /// Stop length in microseconds (independent of BPM).
    StopMicros(f64),
    Scroll(f64),
    /// beatoraja's `#SPEED` (channel SP): the scroll speed of the whole
    /// lane, reached at this point, interpolated in time from the last.
    Speed(f64),
    /// A fold point: the lane and the judge's clock turn back from the
    /// time this beat is reached (a BMS negative BPM, played at its
    /// absolute value as a `Bpm` at the same beat). None at time 0.
    Fold,
}

impl TimingMap {
    pub fn new(initial_bpm: f64) -> Self {
        let bpm = if initial_bpm > 0.0 {
            initial_bpm
        } else {
            130.0
        };
        Self {
            segs: vec![TimeSeg {
                beat: 0.0,
                time: 0.0,
                bpm,
            }],
            scroll: vec![ScrollSeg {
                beat: 0.0,
                pos: 0.0,
                scroll: 1.0,
            }],
            stops: Vec::new(),
            speeds: Vec::new(),
            folds: Vec::new(),
        }
    }

    /// Build from `(beat, event)` pairs. Events need not be sorted.
    /// At equal beats BPM changes are applied before stops.
    pub fn build(initial_bpm: f64, mut events: Vec<(f64, TimingEvent)>) -> Self {
        let mut map = TimingMap::new(initial_bpm);
        events.sort_by(|a, b| {
            a.0.total_cmp(&b.0).then_with(|| {
                let rank = |e: &TimingEvent| match e {
                    TimingEvent::Bpm(_) => 0,
                    TimingEvent::Scroll(_) | TimingEvent::Speed(_) => 1,
                    _ => 2,
                };
                rank(&a.1).cmp(&rank(&b.1))
            })
        });
        let mut i = 0;
        while i < events.len() {
            let beat = events[i].0.max(0.0);
            let mut j = i;
            let mut new_bpm = None;
            let mut stop_beats = 0.0;
            let mut stop_us = 0.0;
            let mut new_scroll = None;
            let mut new_speed = None;
            let mut fold = false;
            while j < events.len() && events[j].0.max(0.0) == beat {
                match events[j].1 {
                    TimingEvent::Bpm(b) if b > 0.0 => new_bpm = Some(b),
                    TimingEvent::Bpm(_) => {}
                    TimingEvent::StopBeats(s) if s > 0.0 => stop_beats += s,
                    TimingEvent::StopMicros(s) if s > 0.0 => stop_us += s,
                    TimingEvent::Scroll(s) => new_scroll = Some(s),
                    TimingEvent::Speed(s) if s.is_finite() => new_speed = Some(s),
                    TimingEvent::Fold => fold = true,
                    _ => {}
                }
                j += 1;
            }
            let t = map.time_at_beat_f(beat);
            // (at the time the beat is reached, before a stop there; an
            // event at time 0 only sets the tempo)
            let at = t.round() as i64;
            if fold && at > 0 && map.folds.last() != Some(&at) {
                map.folds.push(at);
            }
            if let Some(speed) = new_speed {
                // (at the time the beat is reached, before a stop there)
                map.speeds.push(SpeedPoint { time: t, speed });
            }
            let last = map.segs.last().unwrap();
            let bpm = new_bpm.unwrap_or(last.bpm);
            let stop = stop_us + stop_beats * 60_000_000.0 / bpm;
            if new_bpm.is_some() || stop > 0.0 {
                if beat == 0.0 && map.segs.len() == 1 {
                    map.segs[0].bpm = bpm;
                    map.segs[0].time = stop;
                    if stop > 0.0 {
                        // keep a zero-length segment so notes at beat 0 happen before the stop
                        map.segs.insert(
                            0,
                            TimeSeg {
                                beat: 0.0,
                                time: 0.0,
                                bpm,
                            },
                        );
                    }
                } else {
                    map.segs.push(TimeSeg {
                        beat,
                        time: t + stop,
                        bpm,
                    });
                }
                if stop > 0.0 {
                    map.stops.push(StopEvent {
                        beat,
                        time: t.round() as i64,
                        duration: stop.round() as i64,
                    });
                }
            }
            if let Some(s) = new_scroll {
                let pos = map.pos_at_beat(beat);
                let last = map.scroll.last_mut().unwrap();
                if last.beat == beat {
                    last.scroll = s;
                } else {
                    map.scroll.push(ScrollSeg {
                        beat,
                        pos,
                        scroll: s,
                    });
                }
            }
            i = j;
        }
        map
    }

    /// A map made in time (a NONSTOP course's): BPM `changes` (time,
    /// BPM; the first at 0) and `stops` (time, length), in µs and time
    /// order. Its scroll moves with time only, as a chart of `scroll_bpm`
    /// would; a stop still holds it (S-OLR2b 3.3).
    pub fn of_times(changes: &[(i64, f64)], stops: &[(i64, i64)], scroll_bpm: f64) -> TimingMap {
        let first = changes.first().map_or(130.0, |c| c.1);
        let mut map = TimingMap::new(first);
        map.scroll[0].scroll = scroll_bpm / map.segs[0].bpm;
        // changes before stops at the same time
        let mut events: Vec<(i64, u8, f64)> = changes
            .iter()
            .skip(1)
            .map(|&(t, b)| (t, 0, b))
            .chain(stops.iter().map(|&(t, d)| (t, 1, d as f64)))
            .filter(|e| e.0 >= 0 && e.2 > 0.0)
            .collect();
        events.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        for (t, kind, v) in events {
            let beat = map.beat_at_time(t as f64);
            let bpm = map.segs.last().map_or(first, |s| s.bpm);
            if kind == 0 {
                map.segs.push(TimeSeg {
                    beat,
                    time: t as f64,
                    bpm: v,
                });
                let pos = map.pos_at_beat(beat);
                map.scroll.push(ScrollSeg {
                    beat,
                    pos,
                    scroll: scroll_bpm / v,
                });
            } else {
                // (a time inside an earlier stop starts after it)
                let start = map.time_at_beat_f(beat).max(t as f64);
                map.segs.push(TimeSeg {
                    beat,
                    time: start + v,
                    bpm,
                });
                map.stops.push(StopEvent {
                    beat,
                    time: start.round() as i64,
                    duration: v as i64,
                });
            }
        }
        map
    }

    /// The map with time `t` at `(t - from) / speed`: played from `from`
    /// on, `speed` times as fast (the BPMs × `speed`, the stops ÷ it);
    /// beats and scroll positions stay.
    pub fn retimed(&self, from: i64, speed: f64) -> TimingMap {
        let at = |t: f64| (t - from as f64) / speed;
        TimingMap {
            segs: self
                .segs
                .iter()
                .map(|s| TimeSeg {
                    beat: s.beat,
                    time: at(s.time),
                    bpm: s.bpm * speed,
                })
                .collect(),
            scroll: self.scroll.clone(),
            stops: self
                .stops
                .iter()
                .map(|s| StopEvent {
                    beat: s.beat,
                    time: at(s.time as f64).round() as i64,
                    duration: (s.duration as f64 / speed).round() as i64,
                })
                .collect(),
            speeds: (self.speeds.iter())
                .map(|p| SpeedPoint {
                    time: at(p.time),
                    speed: p.speed,
                })
                .collect(),
            folds: (self.folds.iter())
                .map(|&f| at(f as f64).round() as i64)
                .collect(),
        }
    }

    /// The fold points (µs, in time order): from each, the judge's clock
    /// runs back from it ([`fold_time`]) and the lane scrolls back
    /// ([`TimingMap::drawn_pos_at_time`]). Empty for a chart without a
    /// negative BPM.
    pub fn folds(&self) -> &[i64] {
        &self.folds
    }

    /// The map with these fold points (µs, in time order), as a NONSTOP
    /// course's joined map takes on its stages'.
    pub fn with_folds(mut self, folds: Vec<i64>) -> TimingMap {
        self.folds = folds;
        self
    }

    /// The judge's clock at chart time `t`: [`fold_time`] of the folds.
    pub fn judge_time(&self, t: i64) -> i64 {
        fold_time(&self.folds, t)
    }

    /// The scroll position the lane is drawn from at time `t`: the
    /// position at `t` ([`TimingMap::pos_at_time`]), mirrored about the
    /// position of the last fold point before `t`, so that the lane runs
    /// back from there at the scroll speed of the time map.
    pub fn drawn_pos_at_time(&self, t: f64) -> f64 {
        let i = self.folds.partition_point(|&f| (f as f64) < t);
        match i {
            0 => self.pos_at_time(t),
            i => 2.0 * self.pos_at_time(self.folds[i - 1] as f64) - self.pos_at_time(t),
        }
    }

    /// The map without scroll gimmicks (SCROLL REMOVE): the scroll
    /// position is the beat, `#SPEED` is 1 throughout.
    pub fn without_scroll(&self) -> TimingMap {
        TimingMap {
            scroll: vec![ScrollSeg {
                beat: 0.0,
                pos: 0.0,
                scroll: 1.0,
            }],
            speeds: Vec::new(),
            ..self.clone()
        }
    }

    /// The scroll changes anywhere (`#SCROLL`, `#SPEED`, a NONSTOP
    /// course's time-based scroll).
    pub fn has_scroll_changes(&self) -> bool {
        !self.speeds.is_empty() || self.scroll.iter().any(|s| s.scroll != 1.0)
    }

    /// The `#SPEED` multiplier at time `t` (µs): linear in time between
    /// its points (beatoraja's, as bmz-player documents it), from 1 at 0
    /// to the first point, the last point's value after it.
    pub fn speed_at_time(&self, t: f64) -> f64 {
        let p = &self.speeds;
        let i = p.partition_point(|s| s.time <= t);
        let start = SpeedPoint {
            time: 0.0,
            speed: 1.0,
        };
        let (a, b) = match i {
            _ if p.is_empty() => return 1.0,
            0 => (start, p[0]),
            i if i == p.len() => return p[i - 1].speed,
            i => (p[i - 1], p[i]),
        };
        if t <= a.time || b.time <= a.time {
            return if t <= a.time { a.speed } else { b.speed };
        }
        a.speed + (b.speed - a.speed) * (t - a.time) / (b.time - a.time)
    }

    fn seg_before_beat(&self, beat: f64) -> &TimeSeg {
        // last segment whose beat is strictly less than `beat`, else the first one
        let idx = self.segs.partition_point(|s| s.beat < beat);
        if idx == 0 {
            &self.segs[0]
        } else {
            &self.segs[idx - 1]
        }
    }

    fn time_at_beat_f(&self, beat: f64) -> f64 {
        let s = self.seg_before_beat(beat);
        s.time + (beat - s.beat) * 60_000_000.0 / s.bpm
    }

    /// Time (µs) at which `beat` is reached. Notes on a STOP happen before it.
    pub fn time_at_beat(&self, beat: f64) -> i64 {
        self.time_at_beat_f(beat).round() as i64
    }

    /// Beat position at time `t` (µs). Constant during STOPs.
    pub fn beat_at_time(&self, t: f64) -> f64 {
        let idx = self.segs.partition_point(|s| s.time <= t);
        if idx == 0 {
            return self.segs[0].beat + (t - self.segs[0].time) * self.segs[0].bpm / 60_000_000.0;
        }
        let s = &self.segs[idx - 1];
        let b = s.beat + (t - s.time) * s.bpm / 60_000_000.0;
        match self.segs.get(idx) {
            Some(next) => b.min(next.beat),
            None => b,
        }
    }

    pub fn bpm_at_time(&self, t: f64) -> f64 {
        let idx = self.segs.partition_point(|s| s.time <= t);
        self.segs[idx.saturating_sub(1)].bpm
    }

    /// Scroll position for a beat (integral of the SCROLL factor).
    pub fn pos_at_beat(&self, beat: f64) -> f64 {
        let idx = self.scroll.partition_point(|s| s.beat <= beat);
        let s = &self.scroll[idx.saturating_sub(1)];
        s.pos + (beat - s.beat) * s.scroll
    }

    /// Scroll position at time `t` – what the renderer uses as "now".
    pub fn pos_at_time(&self, t: f64) -> f64 {
        self.pos_at_beat(self.beat_at_time(t))
    }

    pub fn bpm_changes(&self) -> Vec<BpmChange> {
        let mut v: Vec<BpmChange> = Vec::new();
        for s in &self.segs {
            if v.last().map(|l| l.bpm != s.bpm).unwrap_or(true) {
                v.push(BpmChange {
                    time: s.time.round() as i64,
                    bpm: s.bpm,
                });
            }
        }
        v
    }

    /// (min, max, main) BPM over `[0, end]`. "main" is the BPM played longest.
    pub fn bpm_stats(&self, end: i64) -> (f64, f64, f64) {
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        let mut durations: Vec<(f64, f64)> = Vec::new();
        // the BPMs met (all above 0, so their bits order as they do), each
        // with its place in `durations`
        let mut met: std::collections::BTreeMap<u64, usize> = Default::default();
        for (i, s) in self.segs.iter().enumerate() {
            let next_t = self.segs.get(i + 1).map(|n| {
                // time when the next segment's beat is reached (excludes stop time)
                s.time + (n.beat - s.beat) * 60_000_000.0 / s.bpm
            });
            let until = next_t.unwrap_or(end as f64).min(end as f64);
            if s.time > end as f64 && i > 0 {
                break;
            }
            min = min.min(s.bpm);
            max = max.max(s.bpm);
            let d = (until - s.time).max(0.0);
            // a BPM within 1e-9 of one met counts as it (the first met)
            let near = (s.bpm - 1e-9).max(0.0).to_bits()..=(s.bpm + 1e-9).to_bits();
            let same = met
                .range(near)
                .filter(|(_, at)| (durations[**at].0 - s.bpm).abs() < 1e-9)
                .map(|(_, at)| *at)
                .min();
            match same {
                Some(at) => durations[at].1 += d,
                None => {
                    met.insert(s.bpm.to_bits(), durations.len());
                    durations.push((s.bpm, d));
                }
            }
        }
        let main = durations
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|e| e.0)
            .unwrap_or(self.segs[0].bpm);
        (min, max, main)
    }
}

/// The judge's clock at chart time `t` with fold points `folds` (µs, in
/// time order): `t` until the first, then `2 T - t` from the last fold
/// point `T` before `t` (bmz-player's LR2 negative BPM, its
/// `docs/gameplay-runtime.md`): it runs back from each fold point, and
/// jumps to the next one when it is reached.
pub fn fold_time(folds: &[i64], t: i64) -> i64 {
    match folds.partition_point(|&f| f < t) {
        0 => t,
        i => folds[i - 1].saturating_mul(2).saturating_sub(t),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_bpm() {
        let m = TimingMap::build(120.0, vec![]);
        assert_eq!(m.time_at_beat(4.0), 2_000_000);
        assert!((m.beat_at_time(1_000_000.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn bpm_change_and_stop() {
        // 120 BPM, at beat 4 change to 240 and stop for 1 beat (at 240 = 0.25 s)
        let m = TimingMap::build(
            120.0,
            vec![
                (4.0, TimingEvent::StopBeats(1.0)),
                (4.0, TimingEvent::Bpm(240.0)),
            ],
        );
        assert_eq!(m.time_at_beat(4.0), 2_000_000); // note on the stop: before it
        assert_eq!(m.time_at_beat(5.0), 2_000_000 + 250_000 + 250_000);
        assert!((m.beat_at_time(2_100_000.0) - 4.0).abs() < 1e-9); // stopped
        assert!((m.beat_at_time(2_500_000.0) - 5.0).abs() < 1e-9);
        assert_eq!(m.stops.len(), 1);
        let (min, max, _) = m.bpm_stats(3_000_000);
        assert_eq!((min, max), (120.0, 240.0));
    }

    /// The main BPM: a BPM within 1e-9 of one met before counts as it;
    /// thousands of BPMs are told apart without each being compared with
    /// every other.
    #[test]
    fn main_bpm_of_near_and_of_many_bpms() {
        // 120 for a beat (0.5 s), 150 for 1.5 (0.6 s), 120 and a bit for
        // one more: 120 is played longest
        let near = TimingMap::build(
            120.0,
            vec![
                (1.0, TimingEvent::Bpm(150.0)),
                (2.5, TimingEvent::Bpm(120.0 + 4e-10)),
            ],
        );
        assert_eq!(near.bpm_stats(1_600_000), (120.0, 150.0, 120.0));
        let apart = TimingMap::build(
            120.0,
            vec![
                (1.0, TimingEvent::Bpm(150.0)),
                (2.5, TimingEvent::Bpm(120.0 + 4e-9)),
            ],
        );
        assert_eq!(apart.bpm_stats(1_600_000).2, 150.0);
        // 200,000 BPMs a beat each, the first one for two
        let n = 200_000;
        let mut events: Vec<(f64, TimingEvent)> = (1..n)
            .map(|i| (i as f64, TimingEvent::Bpm(100.0 + i as f64 * 0.001)))
            .collect();
        events.push((n as f64, TimingEvent::Bpm(100.0)));
        let many = TimingMap::build(100.0, events);
        let end = many.time_at_beat(n as f64 + 1.0);
        let t = std::time::Instant::now();
        let fastest = 100.0 + (n - 1) as f64 * 0.001;
        assert_eq!(many.bpm_stats(end), (100.0, fastest, 100.0));
        assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
    }

    #[test]
    fn stop_at_zero() {
        let m = TimingMap::build(60.0, vec![(0.0, TimingEvent::StopBeats(1.0))]);
        assert_eq!(m.time_at_beat(0.0), 0);
        assert_eq!(m.time_at_beat(1.0), 2_000_000);
    }

    #[test]
    fn made_in_time_scrolls_with_time() {
        // 120 BPM, 240 from 1 s, a 0.5 s stop at 2 s: the scroll moves
        // as 150 BPM would, and holds during the stop
        let m = TimingMap::of_times(
            &[(0, 120.0), (1_000_000, 240.0)],
            &[(2_000_000, 500_000)],
            150.0,
        );
        let pos = |t: f64| m.pos_at_time(t);
        assert!((pos(1_000_000.0) - 2.5).abs() < 1e-9);
        assert!((pos(2_000_000.0) - 5.0).abs() < 1e-9);
        assert!((pos(2_400_000.0) - 5.0).abs() < 1e-9);
        assert!((pos(3_500_000.0) - 7.5).abs() < 1e-9);
        assert_eq!(m.bpm_at_time(1_500_000.0), 240.0);
        assert_eq!(m.stops.len(), 1);
        assert!((m.beat_at_time(3_500_000.0) - (2.0 + 4.0 + 4.0)).abs() < 1e-9);
    }

    #[test]
    fn retimed_moves_and_scales_the_time() {
        // 120 BPM, 240 and a one-beat stop at beat 4: from 1 s on, at half
        // speed
        let m = TimingMap::build(
            120.0,
            vec![
                (4.0, TimingEvent::StopBeats(1.0)),
                (4.0, TimingEvent::Bpm(240.0)),
                (6.0, TimingEvent::Scroll(0.5)),
            ],
        );
        let r = m.retimed(1_000_000, 0.5);
        for beat in [0.0, 1.0, 4.0, 5.0, 7.5] {
            let t = (m.time_at_beat(beat) - 1_000_000) * 2;
            assert!((r.time_at_beat(beat) - t).abs() <= 1, "{beat}");
            assert!((r.beat_at_time(t as f64) - beat).abs() < 1e-9, "{beat}");
            assert_eq!(r.pos_at_beat(beat), m.pos_at_beat(beat));
        }
        assert_eq!(r.bpm_at_time(-1.0), 60.0);
        assert_eq!(r.bpm_at_time(4_000_000.0), 120.0);
        // the stop: twice as long, from the time it starts at
        assert_eq!((r.stops[0].time, r.stops[0].duration), (2_000_000, 500_000));
        assert!((r.beat_at_time(2_400_000.0) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn speed_is_interpolated_in_time() {
        // 120 BPM (a beat 0.5 s): SPEED 2 at beat 4 (2 s), 0.5 at beat 8
        // (4 s), with a one-beat stop at beat 6
        let m = TimingMap::build(
            120.0,
            vec![
                (4.0, TimingEvent::Speed(2.0)),
                (6.0, TimingEvent::StopBeats(1.0)),
                (9.0, TimingEvent::Speed(0.5)),
            ],
        );
        let at = |t: f64| m.speed_at_time(t);
        // from 1 at 0 to the first point
        assert_eq!(at(-1.0), 1.0);
        assert!((at(1_000_000.0) - 1.5).abs() < 1e-9);
        assert_eq!(at(2_000_000.0), 2.0);
        // beat 9 is at 5 s with the stop: half way at 3.5 s
        assert!((at(3_500_000.0) - 1.25).abs() < 1e-9);
        assert_eq!(at(9_000_000.0), 0.5);
        assert!(m.has_scroll_changes());
        // a retimed map keeps it in its time; SCROLL REMOVE drops it
        let r = m.retimed(0, 2.0);
        assert!((r.speed_at_time(500_000.0) - 1.5).abs() < 1e-9);
        let plain = m.without_scroll();
        assert_eq!(plain.speed_at_time(3_500_000.0), 1.0);
        assert!(!plain.has_scroll_changes());
        assert!(!TimingMap::build(120.0, vec![]).has_scroll_changes());
        let sc = TimingMap::build(120.0, vec![(2.0, TimingEvent::Scroll(0.5))]);
        assert!(sc.has_scroll_changes());
        assert_eq!(sc.without_scroll().pos_at_beat(4.0), 4.0);
    }

    #[test]
    fn the_judge_clock_runs_back_from_each_fold() {
        assert_eq!(fold_time(&[], 5), 5);
        let f = [1_000_000, 3_000_000];
        assert_eq!(fold_time(&f, 500_000), 500_000);
        assert_eq!(fold_time(&f, 1_000_000), 1_000_000);
        assert_eq!(fold_time(&f, 1_500_000), 500_000);
        assert_eq!(fold_time(&f, 2_900_000), -900_000);
        // the next fold point: from it, back from there
        assert_eq!(fold_time(&f, 3_000_000), -1_000_000);
        assert_eq!(fold_time(&f, 3_000_001), 2_999_999);
        assert_eq!(fold_time(&f, 4_000_000), 2_000_000);
        assert_eq!(fold_time(&[i64::MAX], i64::MIN), i64::MIN);
    }

    #[test]
    fn a_fold_turns_the_lane_back() {
        // 120 BPM (a beat 0.5 s), a fold at beat 4 (2 s) at 60 BPM (the
        // negative BPM's absolute value), SCROLL 2 from beat 6
        let m = TimingMap::build(
            120.0,
            vec![
                (4.0, TimingEvent::Bpm(60.0)),
                (4.0, TimingEvent::Fold),
                (6.0, TimingEvent::Scroll(2.0)),
            ],
        );
        assert_eq!(m.folds(), &[2_000_000]);
        assert_eq!(m.time_at_beat(6.0), 4_000_000);
        assert_eq!(m.judge_time(1_000_000), 1_000_000);
        assert_eq!(m.judge_time(3_000_000), 1_000_000);
        let drawn = |t: f64| m.drawn_pos_at_time(t);
        assert!((drawn(1_000_000.0) - 2.0).abs() < 1e-9);
        assert!((drawn(2_000_000.0) - 4.0).abs() < 1e-9);
        // a beat a second, back from beat 4; twice as fast under SCROLL 2
        assert!((drawn(3_000_000.0) - 3.0).abs() < 1e-9);
        assert!((drawn(5_000_000.0) - 0.0).abs() < 1e-9);
        // the time map goes on forward
        assert!((m.pos_at_time(5_000_000.0) - 8.0).abs() < 1e-9);
        assert_eq!(m.bpm_at_time(3_000_000.0), 60.0);
        assert_eq!(m.bpm_stats(6_000_000).0, 60.0);
        // retimed and without scroll, it keeps them (moved)
        assert_eq!(m.retimed(1_000_000, 2.0).folds(), &[500_000]);
        assert_eq!(m.without_scroll().folds(), &[2_000_000]);
        // shown in its Debug form only when there are folds
        assert!(format!("{m:?}").contains("folds"));
        assert!(!format!("{:?}", TimingMap::build(120.0, vec![])).contains("folds"));
    }

    #[test]
    fn a_fold_at_time_0_only_sets_the_tempo() {
        let m = TimingMap::build(
            120.0,
            vec![(0.0, TimingEvent::Bpm(60.0)), (0.0, TimingEvent::Fold)],
        );
        assert!(m.folds().is_empty());
        assert_eq!(m.time_at_beat(1.0), 1_000_000);
        // one fold point for two at a beat
        let m = TimingMap::build(
            120.0,
            vec![(2.0, TimingEvent::Fold), (2.0, TimingEvent::Fold)],
        );
        assert_eq!(m.folds(), &[1_000_000]);
    }

    #[test]
    fn scroll() {
        let m = TimingMap::build(120.0, vec![(2.0, TimingEvent::Scroll(0.5))]);
        assert!((m.pos_at_beat(4.0) - 3.0).abs() < 1e-9);
    }
}
