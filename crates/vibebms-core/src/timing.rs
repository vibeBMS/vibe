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

/// Piecewise-linear mapping between beats, real time and scroll position.
#[derive(Clone, Debug, PartialEq)]
pub struct TimingMap {
    segs: Vec<TimeSeg>,
    scroll: Vec<ScrollSeg>,
    pub stops: Vec<StopEvent>,
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
}

impl TimingMap {
    pub fn new(initial_bpm: f64) -> Self {
        let bpm = if initial_bpm > 0.0 { initial_bpm } else { 130.0 };
        Self {
            segs: vec![TimeSeg { beat: 0.0, time: 0.0, bpm }],
            scroll: vec![ScrollSeg { beat: 0.0, pos: 0.0, scroll: 1.0 }],
            stops: Vec::new(),
        }
    }

    /// Build from `(beat, event)` pairs. Events need not be sorted.
    /// At equal beats BPM changes are applied before stops.
    pub fn build(initial_bpm: f64, mut events: Vec<(f64, TimingEvent)>) -> Self {
        let mut map = TimingMap::new(initial_bpm);
        events.sort_by(|a, b| {
            a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then_with(|| {
                let rank = |e: &TimingEvent| match e {
                    TimingEvent::Bpm(_) => 0,
                    TimingEvent::Scroll(_) => 1,
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
            while j < events.len() && events[j].0.max(0.0) == beat {
                match events[j].1 {
                    TimingEvent::Bpm(b) if b > 0.0 => new_bpm = Some(b),
                    TimingEvent::Bpm(_) => {}
                    TimingEvent::StopBeats(s) if s > 0.0 => stop_beats += s,
                    TimingEvent::StopMicros(s) if s > 0.0 => stop_us += s,
                    TimingEvent::Scroll(s) => new_scroll = Some(s),
                    _ => {}
                }
                j += 1;
            }
            let t = map.time_at_beat_f(beat);
            let last = map.segs.last().unwrap();
            let bpm = new_bpm.unwrap_or(last.bpm);
            let stop = stop_us + stop_beats * 60_000_000.0 / bpm;
            if new_bpm.is_some() || stop > 0.0 {
                if beat == 0.0 && map.segs.len() == 1 {
                    map.segs[0].bpm = bpm;
                    map.segs[0].time = stop;
                    if stop > 0.0 {
                        // keep a zero-length segment so notes at beat 0 happen before the stop
                        map.segs.insert(0, TimeSeg { beat: 0.0, time: 0.0, bpm });
                    }
                } else {
                    map.segs.push(TimeSeg { beat, time: t + stop, bpm });
                }
                if stop > 0.0 {
                    map.stops.push(StopEvent { beat, time: t.round() as i64, duration: stop.round() as i64 });
                }
            }
            if let Some(s) = new_scroll {
                let pos = map.pos_at_beat(beat);
                let last = map.scroll.last_mut().unwrap();
                if last.beat == beat {
                    last.scroll = s;
                } else {
                    map.scroll.push(ScrollSeg { beat, pos, scroll: s });
                }
            }
            i = j;
        }
        map
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

    pub fn is_stopped_at(&self, t: i64) -> bool {
        self.stops.iter().any(|s| t >= s.time && t < s.time + s.duration)
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
                v.push(BpmChange { time: s.time.round() as i64, bpm: s.bpm });
            }
        }
        v
    }

    /// (min, max, main) BPM over `[0, end]`. "main" is the BPM played longest.
    pub fn bpm_stats(&self, end: i64) -> (f64, f64, f64) {
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        let mut durations: Vec<(f64, f64)> = Vec::new();
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
            if let Some(e) = durations.iter_mut().find(|e| (e.0 - s.bpm).abs() < 1e-9) {
                e.1 += d;
            } else {
                durations.push((s.bpm, d));
            }
        }
        let main = durations
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|e| e.0)
            .unwrap_or(self.segs[0].bpm);
        (min, max, main)
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
            vec![(4.0, TimingEvent::StopBeats(1.0)), (4.0, TimingEvent::Bpm(240.0))],
        );
        assert_eq!(m.time_at_beat(4.0), 2_000_000); // note on the stop: before it
        assert_eq!(m.time_at_beat(5.0), 2_000_000 + 250_000 + 250_000);
        assert!((m.beat_at_time(2_100_000.0) - 4.0).abs() < 1e-9); // stopped
        assert!((m.beat_at_time(2_500_000.0) - 5.0).abs() < 1e-9);
        assert_eq!(m.stops.len(), 1);
        let (min, max, _) = m.bpm_stats(3_000_000);
        assert_eq!((min, max), (120.0, 240.0));
    }

    #[test]
    fn stop_at_zero() {
        let m = TimingMap::build(60.0, vec![(0.0, TimingEvent::StopBeats(1.0))]);
        assert_eq!(m.time_at_beat(0.0), 0);
        assert_eq!(m.time_at_beat(1.0), 2_000_000);
    }

    #[test]
    fn scroll() {
        let m = TimingMap::build(120.0, vec![(2.0, TimingEvent::Scroll(0.5))]);
        assert!((m.pos_at_beat(4.0) - 3.0).abs() < 1e-9);
    }
}
