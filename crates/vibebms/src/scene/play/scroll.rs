//! Where a note or bar line is along a player's lane: one rule for the
//! flat and the perspective note fields, which only map it to a y or a
//! depth.

use super::session::{held_at_line, Lane};
use crate::config::{HiSpeedMode, PlayConfig};
use vibebms_core::chart::{BarLine, Chart};
use vibebms_core::engine::PlayNote;

/// A player's lane at one moment of the chart.
#[derive(Clone, Copy)]
pub(super) struct Scroll<'a> {
    chart: &'a Chart,
    mode: HiSpeedMode,
    /// The chart time drawn (µs).
    pub t_vis: i64,
    /// The judge's clock then (past a negative BPM's fold point the lane
    /// runs back, and this clock with it).
    pub t_judge: i64,
    /// The scroll position the lane is drawn from.
    cur_pos: f64,
    /// `#SPEED` then: the whole lane faster or slower (not under CONSTANT).
    speed: f64,
    /// How long a note takes from the top of the lane to the line (µs).
    dur_us: f64,
    /// The scroll positions the lane shows at the main BPM.
    beats: f64,
}

impl<'a> Scroll<'a> {
    /// The lane of a player with options `p` and hi-speed `hs` (as eased
    /// to a change) at chart time `t_vis`.
    pub fn new(chart: &'a Chart, p: &PlayConfig, hs: f32, t_vis: i64) -> Scroll<'a> {
        let main_bpm = match chart.meta.main_bpm {
            b if b > 0.0 => b,
            _ => 130.0,
        };
        let dur_us = p.duration_with(hs, main_bpm, true) * 1000.0;
        let timing = &chart.timing;
        Scroll {
            chart,
            mode: p.hispeed_mode,
            t_vis,
            t_judge: timing.judge_time(t_vis),
            cur_pos: timing.drawn_pos_at_time(t_vis as f64),
            speed: timing.speed_at_time(t_vis as f64),
            dur_us,
            beats: dur_us / 1_000_000.0 * main_bpm / 60.0,
        }
    }

    /// How far up the lane something at scroll position `pos` and time
    /// `time` is: 0 at the judge line, 1 at the top (the cover's edge).
    /// FLOATING goes by the position (and `#SPEED`), CONSTANT by the time
    /// against the judge's clock.
    pub fn ahead(&self, pos: f64, time: i64) -> f64 {
        match self.mode {
            HiSpeedMode::Floating => (pos - self.cur_pos) * self.speed / self.beats,
            HiSpeedMode::Constant => (time - self.t_judge) as f64 / self.dur_us,
        }
    }

    /// [`Scroll::ahead`] of what is at chart time `t`.
    pub fn ahead_at(&self, t: i64) -> f64 {
        self.ahead(self.chart.timing.pos_at_time(t as f64), t)
    }

    /// A lane's notes still drawn (indices in `notes`, its judge's) with
    /// their [`Scroll::ahead`], until they are past the top for sure. With
    /// `hold` (LR2's way), an unjudged note whose time passed waits at the
    /// line.
    pub fn notes(
        self,
        lane: &'a Lane,
        notes: &'a [PlayNote],
        hold: bool,
    ) -> impl Iterator<Item = (usize, f64)> + 'a {
        let list = &lane.notes[lane.cursor.min(lane.notes.len())..];
        list.iter()
            .map(move |&ni| {
                let n = &notes[ni];
                let cn = &self.chart.notes[n.chart_index];
                let a = self.ahead(cn.pos, n.time);
                match hold && held_at_line(n, self.t_judge) {
                    true => (ni, a.max(0.0)),
                    false => (ni, a),
                }
            })
            .take_while(move |&(ni, a)| notes[ni].time <= self.t_vis + 2_000_000 || a <= 2.0)
    }

    /// The [`Scroll::ahead`] of each bar line between the line and the top,
    /// nearest first.
    pub fn bar_lines(self) -> impl Iterator<Item = f64> + 'a {
        let lines = &self.chart.bar_lines;
        // (they go up the lane in chart order, but under a `#SPEED` below 0)
        let rising = self.mode == HiSpeedMode::Constant || self.speed >= 0.0;
        let ahead = move |bl: &BarLine| self.ahead(bl.pos, bl.time);
        let first = match rising {
            true => lines.partition_point(|bl| ahead(bl) < 0.0),
            false => 0,
        };
        lines[first..]
            .iter()
            .map(ahead)
            .take_while(move |&a| !rising || a <= 1.0)
            .filter(|a| (0.0..=1.0).contains(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use vibebms_core::play::{build_chart, RuleOptions};

    /// 150 BPM; `#SPEED` 0.5 from measure 2; a negative BPM's fold point
    /// at measure 4; a note on every bar line.
    const CHART: &str = "#BPM 150\n#BPM01 -150\n#SPEED01 0.5\n\
        #002SP:01\n#00408:01\n\
        #00111:01\n#00211:01\n#00311:01\n#00411:01\n#00511:01\n#00611:01\n\
        #00711:01\n#00811:01\n";

    fn chart() -> Chart {
        let play = RuleOptions::default();
        build_chart(Path::new("s.bms"), CHART.as_bytes(), &play, 1, None)
            .unwrap()
            .chart
    }

    /// Both note fields place bar lines and notes by `ahead` alone.
    #[test]
    fn bar_lines_go_with_the_notes_on_them() {
        let c = chart();
        let fold = c.timing.folds()[0];
        let measure = 1_600_000;
        let mut checked = 0;
        for mode in [HiSpeedMode::Floating, HiSpeedMode::Constant] {
            let p = PlayConfig {
                hispeed_mode: mode,
                hispeed: 0.5,
                ..Default::default()
            };
            // in measure 1, with `#SPEED` 0.5 on, and past the fold point
            for t in [measure + 300_000, 3 * measure + 300_000, fold + 500_000] {
                let sc = Scroll::new(&c, &p, 0.5, t);
                let bars: Vec<f64> = sc.bar_lines().collect();
                assert!(!bars.is_empty(), "{mode:?} {t}");
                for &a in &bars {
                    // a note sits on it
                    let n = c
                        .notes
                        .iter()
                        .find(|n| (sc.ahead(n.pos, n.time) - a).abs() < 1e-9);
                    assert!(n.is_some(), "{mode:?} {t}: a bar line at {a} has no note");
                    checked += 1;
                }
            }
        }
        assert!(checked >= 6);
    }

    #[test]
    fn speed_and_fold_move_the_lane() {
        let c = chart();
        let p = PlayConfig {
            hispeed: 0.5,
            ..Default::default()
        };
        let measure = 1_600_000;
        let bar = |t: i64, k: usize| {
            let sc = Scroll::new(&c, &p, 0.5, t);
            let bl = &c.bar_lines[k];
            sc.ahead(bl.pos, bl.time)
        };
        // FLOATING: a measure ahead is half as far up under `#SPEED` 0.5
        let plain = bar(0, 1);
        let slow = bar(3 * measure, 4);
        assert!((slow * 2.0 - plain).abs() < 1e-9, "{plain} {slow}");
        // past the fold point the lane runs back: a bar line just after it
        // moves up, away from the line
        let fold = c.timing.folds()[0];
        let after = c.bar_lines.iter().position(|b| b.time > fold).unwrap();
        assert!(bar(fold + 400_000, after) > bar(fold + 200_000, after));
        // CONSTANT: by time against the judge's clock, `#SPEED` aside
        let p = PlayConfig {
            hispeed_mode: HiSpeedMode::Constant,
            ..p
        };
        let sc = Scroll::new(&c, &p, 0.5, 3 * measure);
        let bl = &c.bar_lines[4];
        let half = sc.ahead(bl.pos, bl.time);
        assert!((sc.ahead_at(bl.time) - half).abs() < 1e-9);
        let sc0 = Scroll::new(&c, &p, 0.5, 0);
        let first = &c.bar_lines[1];
        assert!((sc0.ahead(first.pos, first.time) - half).abs() < 1e-9);
    }
}
