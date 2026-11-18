//! A play's judging, free of the window, the GPU and the mixer: the chart,
//! each side's judge, what the skin shows of them, the BGA layers.

use crate::input::LaneEv;
use crate::platform::{Duration, Instant};
use crate::replay::Replay;
use std::sync::Arc;
use vibebms_core::chart::{BgaLayer, Chart, SoundId};
use vibebms_core::engine::{Engine, EngineEvent, InputEvent, Op, PlayNote};
use vibebms_core::judge::Judge;
use vibebms_core::nonstop::Nonstop;
use vibebms_core::record::{DeviceFacts, InputSource};
use vibebms_core::PlayMode;

/// Who plays a side.
#[derive(Clone, Debug)]
pub(crate) enum Role {
    /// A person (or autoplay): the keys reach its judge.
    Player,
    /// A replay being watched.
    Replay(Feed),
    /// G-BATTLE's opponent: a replay of the chart played again, silent.
    /// `desync`: its judgements went other than the replay's (another
    /// version judged it): its replay's EX score is shown instead.
    Ghost { feed: Feed, desync: bool },
}

/// Replay ops going into a judge as the chart plays.
#[derive(Clone, Debug)]
pub(crate) struct Feed {
    pub replay: Arc<Replay>,
    /// The next op.
    pub at: usize,
    /// Lanes its inputs move by (a single play's replay on the 2P side).
    pub shift: u8,
}

impl Feed {
    pub fn new(replay: Arc<Replay>, shift: u8) -> Feed {
        Feed {
            replay,
            at: 0,
            shift,
        }
    }

    /// The ops up to chart time `t` into `e` (an input waits in the judge
    /// for the update after it); `lit` sees each input (lane, pressed).
    pub fn run(&mut self, e: &mut Engine, t: i64, mut lit: impl FnMut(u8, bool)) {
        while let Some(op) = self.replay.ops.get(self.at) {
            let op = op.shifted(self.shift);
            match op {
                Op::Update(u) if u > t => break,
                Op::Input(_, lane, _, pressed) => lit(lane, pressed),
                Op::Update(_) => {}
            }
            e.apply(op);
            self.at += 1;
        }
    }
}

/// One side of the play: its judge and what is kept of it.
pub(crate) struct SideState {
    pub engine: Engine,
    pub role: Role,
    /// The gauge every 500 ms (the result's graph).
    pub gauge_log: Vec<f32>,
    /// When its gauge last rose (skin timers 42 / 43), and its value then.
    pub gauge_up: Option<Instant>,
    pub last_gauge: f32,
    /// When its last note was judged (timers 143 / 144).
    pub finished: Option<Instant>,
    /// VS: when the side failed (it plays no more, the other goes on).
    pub failed: Option<Instant>,
}

impl SideState {
    pub fn new(engine: Engine, role: Role) -> SideState {
        let last_gauge = engine.gauge.value();
        SideState {
            engine,
            role,
            gauge_log: Vec::new(),
            gauge_up: None,
            last_gauge,
            finished: None,
            failed: None,
        }
    }

    pub fn player(&self) -> bool {
        matches!(self.role, Role::Player)
    }

    pub fn ghost(&self) -> bool {
        matches!(self.role, Role::Ghost { .. })
    }
}

/// A lane: its notes in its judge and what the skin shows of it.
#[derive(Clone, Default)]
pub(crate) struct Lane {
    /// Its notes (indices in its judge's notes), from `cursor` on still
    /// drawn.
    pub notes: Vec<usize>,
    pub cursor: usize,
    /// When its key was last pressed and let go (key beams, timers
    /// 100-139).
    pub pressed: Option<Instant>,
    pub released: Option<Instant>,
    /// When its bomb was last lit (timers 50-69).
    pub bomb: Option<Instant>,
    /// Its last judgement (skin values 500-519).
    pub judge: Option<Judge>,
}

/// A judge region's last judgement.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LastJudge {
    pub judge: Judge,
    /// µs; > 0: early.
    pub diff: i64,
    pub at: Instant,
    pub combo: u32,
}

/// What the scene does with a step's events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Out {
    /// A player's keysound, now (auto lanes' are scheduled ahead).
    Key { side: usize, sound: SoundId },
    /// A side's keysound's volume (HCN).
    Volume {
        side: usize,
        sound: SoundId,
        volume: f32,
    },
    /// The first side judged a key early (> 0) or late: the judge offset
    /// goes this many ms the other way (timing auto adjust).
    Adjust(i32),
    /// A judgement's guide sound (GUIDE SE).
    Guide(Judge),
}

/// The play options that change what a session does with its judges'
/// events (none changes how it judges).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Fx {
    /// GUIDE SE: each judgement's guide sound.
    pub guide_se: bool,
    /// RESULT WAIT: the play goes on until 5 s after the chart's last
    /// note, BGM or BGA layer (not the POOR layer).
    pub result_wait: bool,
    /// A PGREAT, GREAT or GOOD clears the POOR BGA at once.
    pub good_clears_poor: bool,
}

/// What the skin decides of a judgement's effects.
#[derive(Clone, Copy)]
pub(crate) struct Show {
    /// Judgements up to this one light the bomb (`#JUDGETIMER`).
    pub bomb_upto: Judge,
    /// beatoraja judge regions: three for a PMS skin with a third judge
    /// display (keys 1-3, 4-6 and 7-9), else one per player.
    pub three_regions: bool,
}

pub(crate) struct Session {
    pub chart: Chart,
    /// The chart's mode (its lanes).
    pub mode: PlayMode,
    /// VS: two players here, one per side.
    pub vs: bool,
    /// The judges: one per side (a single play, BATTLE: one for both).
    pub sides: Vec<SideState>,
    /// Chart time 0.
    pub start: Instant,
    pub ready_at: Option<Instant>,
    pub lanes: Vec<Lane>,
    /// The last judgement of each judge region.
    pub last_judge: [Option<LastJudge>; 3],
    pub poor_until: Option<Instant>,
    /// The image of each BGA layer (base, layer, layer 2, POOR) and the
    /// chart time each image was first shown (a video plays from then).
    pub bga: [Option<u32>; 4],
    pub bga_start: Vec<Option<i64>>,
    bga_i: usize,
    /// A NONSTOP course's stages, and each stage's BGA layers (its BGA
    /// cross-fades with the next's).
    pub nonstop: Option<Arc<Nonstop>>,
    pub stage_bga: Vec<[Option<u32>; 4]>,
    next_sample: i64,
    /// When every side's notes were judged (chart time, instant).
    pub finished_at: Option<i64>,
    pub finished_instant: Option<Instant>,
    /// The chart's BPM changes: (time, BPM).
    pub bpm_log: Vec<(i64, f64)>,
    /// The first side's presses that reached its judge, by source.
    pub device: DeviceFacts,
    pub fx: Fx,
    /// This step's events: (side, event).
    events: Vec<(usize, EngineEvent)>,
}

impl Session {
    pub fn new(chart: Chart, sides: Vec<SideState>, vs: bool, start: Instant) -> Session {
        let mode = chart.mode();
        let mut lanes = vec![Lane::default(); mode.lane_count()];
        for side in &sides {
            for (i, n) in side.engine.notes().iter().enumerate() {
                lanes[n.lane as usize].notes.push(i);
            }
        }
        let bpm_log = chart
            .timing
            .bpm_changes()
            .into_iter()
            .map(|c| (c.time, c.bpm))
            .collect();
        Session {
            mode,
            vs,
            sides,
            start,
            ready_at: None,
            lanes,
            last_judge: [None; 3],
            poor_until: None,
            bga: [None; 4],
            bga_start: vec![None; chart.images.len()],
            bga_i: 0,
            next_sample: 0,
            finished_at: None,
            finished_instant: None,
            bpm_log,
            device: DeviceFacts::default(),
            fx: Fx::default(),
            events: Vec::new(),
            nonstop: None,
            stage_bga: Vec::new(),
            chart,
        }
    }

    pub fn set_nonstop(&mut self, ns: Option<Arc<Nonstop>>) {
        self.stage_bga = vec![[None; 4]; ns.as_ref().map_or(0, |n| n.stages.len())];
        self.nonstop = ns;
    }

    /// The course stage playing at chart time `t` (NONSTOP).
    pub fn stage_at(&self, t: i64) -> Option<usize> {
        self.nonstop.as_ref().map(|n| n.stage_at(t))
    }

    /// Chart time (µs) at `t`.
    pub fn chart_us(&self, t: Instant) -> i64 {
        if t >= self.start {
            t.duration_since(self.start).as_micros() as i64
        } else {
            -(self.start.duration_since(t).as_micros() as i64)
        }
    }

    /// The moment of chart time `t` (µs).
    pub fn instant_at(&self, t: i64) -> Instant {
        if t >= 0 {
            self.start + Duration::from_micros(t as u64)
        } else {
            self.start - Duration::from_micros(t.unsigned_abs())
        }
    }

    /// The first side's judge (1P: the scores the skin shows).
    pub fn engine(&self) -> Option<&Engine> {
        self.sides.first().map(|s| &s.engine)
    }

    /// The judge of a lane.
    pub fn engine_for(&self, lane: usize) -> Option<&Engine> {
        self.sides
            .iter()
            .map(|s| &s.engine)
            .find(|e| e.judges(lane as u8))
    }

    /// A battle's second side (G-BATTLE's opponent, VS's 2P).
    pub fn opponent(&self) -> Option<&SideState> {
        self.sides.get(1)
    }

    /// A side for skin values: 2P is the opponent, else the one judge.
    pub fn side(&self, side: usize) -> Option<&SideState> {
        self.sides.get(side).or(self.sides.first())
    }

    /// The opponent's EX score now: its judge's, or once a G-BATTLE
    /// replay went out of step, its replay's at the player's note.
    pub fn opponent_ex(&self) -> Option<i64> {
        let side = self.sides.get(1)?;
        Some(match &side.role {
            Role::Ghost { feed, desync: true } => {
                let passed = self.engine().map_or(0, |e| e.score.passed as usize);
                feed.replay.ex_at(passed) as i64
            }
            _ => side.engine.score.ex_score() as i64,
        })
    }

    /// G-BATTLE's opponent went out of step with its replay.
    pub fn ghost_desync(&self) -> bool {
        self.sides
            .iter()
            .any(|s| matches!(s.role, Role::Ghost { desync: true, .. }))
    }

    /// A replay played again here (watched, or G-BATTLE's opponent) was
    /// recorded with an older judge.
    pub fn older_replay(&self) -> bool {
        self.sides.iter().any(|s| match &s.role {
            Role::Replay(f) | Role::Ghost { feed: f, .. } => f.replay.older_engine(),
            Role::Player => false,
        })
    }

    /// A player's key into the judge of its lane (`offsets`: each side's
    /// judge offset, µs), lit, a press counted by its `src`. A side that
    /// failed plays no more.
    pub fn key(&mut self, ev: LaneEv, offsets: [i64; 2], src: Option<InputSource>) {
        let Some(k) = self
            .sides
            .iter()
            .position(|s| s.player() && s.engine.judges(ev.lane))
        else {
            return;
        };
        if self.sides[k].failed.is_some() {
            return;
        }
        let time = self.chart_us(ev.time) - offsets[k];
        if let Some(src) = src.filter(|_| ev.pressed && k == 0) {
            if !self.sides[0].engine.is_auto(ev.lane as usize) {
                self.device.count(src);
            }
        }
        self.sides[k].engine.input(InputEvent {
            time,
            lane: ev.lane,
            key: ev.sub,
            pressed: ev.pressed,
        });
        if let Some(l) = self.lanes.get_mut(ev.lane as usize) {
            match ev.pressed {
                true => l.pressed = Some(ev.time),
                false => l.released = Some(ev.time),
            }
        }
    }

    /// Judge the players' keys at chart time `t` now rather than at the
    /// next frame: their sounds into `out`; the other events wait for the
    /// frame.
    pub fn judge_keys(&mut self, t: i64, offsets: [i64; 2], out: &mut Vec<Out>) {
        for (k, side) in self.sides.iter_mut().enumerate() {
            if !side.player() || side.failed.is_some() {
                continue;
            }
            side.engine.update(t - offsets[k]);
            for ev in side.engine.take_sounds() {
                out.extend(sound(k, &side.engine, ev));
            }
        }
    }

    /// The play at chart time `t` (`now`): the replays' ops in, each side's
    /// judge on (`offsets`: each side's judge offset, µs), what they judged
    /// shown, the BGA on. `out` gets what the scene does with it. `true`:
    /// the play failed.
    pub fn step(
        &mut self,
        t: i64,
        now: Instant,
        offsets: [i64; 2],
        show: Show,
        out: &mut Vec<Out>,
    ) -> bool {
        self.run_feeds(t, now);
        let sample = t >= self.next_sample;
        let mut finished = true;
        for (k, side) in self.sides.iter_mut().enumerate() {
            let (player, ghost) = (side.player(), side.ghost());
            let gone = side.failed.is_some();
            let engine = &mut side.engine;
            // (a side that failed in VS plays no more)
            if player && !gone {
                engine.update(t - offsets[k]);
            }
            self.events.extend(engine.drain_events().map(|e| (k, e)));
            if sample {
                side.gauge_log.push(engine.gauge.value());
            }
            let g = engine.gauge.value();
            if g > side.last_gauge {
                side.gauge_up = Some(now);
            }
            side.last_gauge = g;
            let done = engine.is_finished(t - offsets[k]);
            if done && side.finished.is_none() {
                side.finished = Some(now);
            }
            finished &= ghost || gone || done;
        }
        if sample {
            // every 500 ms of play time, like beatoraja
            self.next_sample = t + 500_000;
        }
        if !self.sides.is_empty() && finished && self.finished_at.is_none() {
            self.finished_at = Some(t);
            self.finished_instant = Some(now);
        }
        let mut events = std::mem::take(&mut self.events);
        let mut failed = false;
        for (k, ev) in events.drain(..) {
            failed |= self.event(k, ev, now, show, out);
        }
        self.events = events;
        self.advance_bga(t);
        failed
    }

    /// A side's judge event: `true` when it ends the play (failed).
    fn event(
        &mut self,
        k: usize,
        ev: EngineEvent,
        now: Instant,
        show: Show,
        out: &mut Vec<Out>,
    ) -> bool {
        let side = &mut self.sides[k];
        let ghost = side.ghost();
        match ev {
            // the opponent is silent, its failing ends nothing
            EngineEvent::Sound { .. } | EngineEvent::Volume { .. } | EngineEvent::Failed
                if ghost => {}
            EngineEvent::Sound { .. } | EngineEvent::Volume { .. } => {
                out.extend(sound(k, &side.engine, ev))
            }
            // VS: a player who fails stops; the play ends when both have
            EngineEvent::Failed if self.vs => {
                side.failed.get_or_insert(now);
                return self
                    .sides
                    .iter()
                    .filter(|s| s.player())
                    .all(|s| s.failed.is_some());
            }
            EngineEvent::Judge {
                lane,
                judge,
                diff,
                combo,
                ..
            } => {
                let lane = lane as usize;
                if self.fx.guide_se && !ghost {
                    out.push(Out::Guide(judge));
                }
                // timing auto adjust (beatoraja): PG..GOOD within 150 ms
                // move the timing a ms per 30 ms off
                if k == 0
                    && !side.engine.is_auto(lane)
                    && judge <= Judge::Good
                    && diff.abs() <= 150_000
                {
                    let d = (diff + if diff >= 0 { 15_000 } else { -15_000 }) / 30_000;
                    out.push(Out::Adjust(d as i32));
                }
                let region = if show.three_regions {
                    (lane / 3).min(2)
                } else {
                    self.mode.player_of_lane(lane).min(1)
                };
                self.last_judge[region] = Some(LastJudge {
                    judge,
                    diff,
                    at: now,
                    combo,
                });
                if let Some(l) = self.lanes.get_mut(lane) {
                    l.judge = Some(judge);
                    if judge <= show.bomb_upto {
                        l.bomb = Some(now);
                    }
                }
                // the POOR BGA: a judgement that leaves no combo
                // (beatoraja; an empty POOR in 7KEYS keeps the combo)
                if combo == 0 && !ghost {
                    self.poor_until = Some(now + Duration::from_millis(500));
                } else if self.fx.good_clears_poor && judge <= Judge::Good && !ghost {
                    self.poor_until = None;
                }
            }
            EngineEvent::Mine { .. } => {}
            EngineEvent::Failed => return true,
        }
        false
    }

    /// The sides that replays drive: their ops up to chart time `t`,
    /// the keys they press lit.
    fn run_feeds(&mut self, t: i64, now: Instant) {
        for side in &mut self.sides {
            let (f, desync) = match &mut side.role {
                Role::Player => continue,
                Role::Replay(f) => (f, None),
                Role::Ghost { feed, desync } => (feed, Some(desync)),
            };
            let lanes = &mut self.lanes;
            f.run(&mut side.engine, t, |lane, pressed| {
                if let Some(l) = lanes.get_mut(lane as usize) {
                    match pressed {
                        true => l.pressed = Some(now),
                        false => l.released = Some(now),
                    }
                }
            });
            // an opponent that judges other than its replay did
            if let Some(d) = desync.filter(|d| !**d) {
                let (e, r) = (&side.engine, &f.replay);
                let done = f.at >= r.ops.len();
                *d = !r.ghost.starts_with(&e.ghost) || (done && e.score.ex_score() != r.ex_score);
                if *d {
                    log::warn!("G-BATTLE: the opponent's replay went out of step");
                }
            }
        }
    }

    /// The BGA events up to chart time `t`.
    fn advance_bga(&mut self, t: i64) {
        while let Some(b) = self.chart.bga.get(self.bga_i) {
            if b.time > t {
                break;
            }
            let i = match b.layer {
                BgaLayer::Base => 0,
                BgaLayer::Layer => 1,
                BgaLayer::Layer2 => 2,
                BgaLayer::Poor => 3,
            };
            self.bga[i] = b.image;
            if let Some(k) = self.nonstop.as_ref().map(|n| n.bga_stage[self.bga_i]) {
                self.stage_bga[k][i] = b.image;
            }
            if let Some(s) = b.image.and_then(|m| self.bga_start.get_mut(m as usize)) {
                s.get_or_insert(b.time);
            }
            self.bga_i += 1;
        }
    }

    /// Every side's notes judged a second ago and the chart's sounds over
    /// (chart time `t`): the play is done. RESULT WAIT: every note judged
    /// and 5 s past the chart's last note, BGM and BGA layer (the POOR
    /// layer left out; a BGM's sound is not followed to its end).
    pub fn over(&self, t: i64) -> bool {
        let Some(ft) = self.finished_at else {
            return false;
        };
        if self.fx.result_wait {
            let bga = self.chart.bga.iter().filter(|b| b.layer != BgaLayer::Poor);
            let last = bga
                .map(|b| b.time)
                .max()
                .unwrap_or(0)
                .max(self.chart.length());
            return t > last + 5_000_000;
        }
        let tail = self
            .chart
            .bgm
            .last()
            .map(|b| b.time)
            .unwrap_or(0)
            .max(self.chart.length());
        t > ft + 1_000_000 && t > tail + 500_000
    }

    /// The lanes' first notes still drawn at chart time `t`: notes done a
    /// second ago are not.
    pub fn advance_cursors(&mut self, t: i64) {
        for (lane, l) in self.lanes.iter_mut().enumerate() {
            let judge = self.sides.iter().find(|s| s.engine.judges(lane as u8));
            let Some(notes) = judge.map(|s| s.engine.notes()) else {
                continue;
            };
            while let Some(&i) = l.notes.get(l.cursor) {
                let n = &notes[i];
                if n.end_time < t - 1_000_000 && (n.is_done() || n.is_mine()) {
                    l.cursor += 1;
                } else {
                    break;
                }
            }
        }
    }
}

/// LR2's way of drawing notes (the display option): a note (a long note's
/// head) whose time `t` has passed while it is still unjudged waits at the
/// judge line until it is hit or missed. Not a mine (never judged).
pub(crate) fn held_at_line(n: &PlayNote, t: i64) -> bool {
    n.time < t && n.state.is_none() && !n.is_mine()
}

/// What the scene does with a side's sound event: a keysound of a lane the
/// player plays (an auto lane's is scheduled ahead), a volume.
fn sound(side: usize, e: &Engine, ev: EngineEvent) -> Option<Out> {
    match ev {
        EngineEvent::Sound { sound, lane } if !e.is_auto(lane as usize) => {
            Some(Out::Key { side, sound })
        }
        EngineEvent::Volume { sound, volume } => Some(Out::Volume {
            side,
            sound,
            volume,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use vibebms_core::bot;
    use vibebms_core::chart::NoteKind;
    use vibebms_core::gauge::GaugeType;
    use vibebms_core::play::{build_chart, build_engine, RuleOptions};

    const SHOW: Show = Show {
        bomb_upto: Judge::Great,
        three_regions: false,
    };

    fn chart(play: &RuleOptions) -> Chart {
        let text = bot::chart_text(7, 5);
        let path = Path::new("song/chart.bme");
        build_chart(path, text.as_bytes(), play, 1, None)
            .unwrap()
            .chart
    }

    fn side(chart: &Chart, r: &RuleOptions, player: Option<usize>, auto: bool) -> SideState {
        SideState::new(build_engine(chart, r, player, auto).0, Role::Player)
    }

    /// Frames every 1/60 s as the scene steps them, `keys` before each,
    /// until the play fails (`true`) or is over; the chart time then.
    fn run(
        s: &mut Session,
        out: &mut Vec<Out>,
        mut keys: impl FnMut(&mut Session, i64),
    ) -> (bool, i64) {
        let mut t = -1_500_000;
        loop {
            keys(s, t);
            if s.step(t, s.instant_at(t), [0, 0], SHOW, out) {
                return (true, t);
            }
            if s.over(t) {
                return (false, t);
            }
            t += 16_667;
        }
    }

    #[test]
    fn autoplay_steps_through_the_chart() {
        let play = RuleOptions::default();
        let c = chart(&play);
        let sides = vec![side(&c, &play, None, true)];
        let mut s = Session::new(c, sides, false, Instant::now());
        let mut out = Vec::new();
        let (failed, t) = run(&mut s, &mut out, |_, _| {});
        assert!(!failed);
        let e = s.engine().unwrap();
        assert!(e.score.total_notes > 0 && e.ghost.iter().all(|j| *j == Judge::PGreat));
        assert!(s.finished_at.is_some_and(|ft| ft < t - 1_000_000));
        // auto lanes' keysounds are scheduled ahead, they adjust nothing
        assert_eq!(out, []);
        assert_eq!(s.last_judge[0].map(|j| j.judge), Some(Judge::PGreat));
        // the gauge every 500 ms from the chart's start (frames 1/60 s apart)
        let samples = t / 500_000 + 1;
        assert!((s.sides[0].gauge_log.len() as i64 - samples).abs() <= 1);
        s.advance_cursors(t);
        let e = s.engine().unwrap();
        for l in s.lanes.iter().filter(|l| !l.notes.is_empty()) {
            assert_eq!(l.cursor, l.notes.len());
            assert!(l.judge.is_some() || l.notes.iter().all(|&i| e.notes()[i].is_mine()));
        }
    }

    #[test]
    fn guide_se_sounds_each_judgement() {
        let play = RuleOptions::default();
        let c = chart(&play);
        let sides = vec![side(&c, &play, None, true)];
        let mut s = Session::new(c, sides, false, Instant::now());
        s.fx.guide_se = true;
        let mut out = Vec::new();
        run(&mut s, &mut out, |_, _| {});
        let e = s.engine().unwrap();
        assert!(!e.judge_log.is_empty());
        let guides: Vec<Out> = e.judge_log.iter().map(|j| Out::Guide(j.1)).collect();
        assert_eq!(out, guides);
    }

    #[test]
    fn result_wait_plays_on_to_the_last_bga() {
        let play = RuleOptions::default();
        let mut ends = Vec::new();
        for wait in [false, true] {
            let mut c = chart(&play);
            // a BGA layer 3 s after the chart's last sound, and a POOR
            // image later still (not waited for)
            let last = c.length() + 3_000_000;
            for (time, layer) in [(last, BgaLayer::Base), (last + 9_000_000, BgaLayer::Poor)] {
                c.bga.push(vibebms_core::chart::BgaEvent {
                    time,
                    layer,
                    image: None,
                });
            }
            let sides = vec![side(&c, &play, None, true)];
            let mut s = Session::new(c, sides, false, Instant::now());
            s.fx.result_wait = wait;
            let (failed, t) = run(&mut s, &mut Vec::new(), |_, _| {});
            assert!(!failed);
            ends.push((t, last));
        }
        let ((plain, last), (waited, _)) = (ends[0], ends[1]);
        assert!(plain < last, "{plain} {last}");
        assert!(waited > last + 5_000_000 && waited < last + 5_100_000);
    }

    #[test]
    fn a_good_clears_the_poor_bga() {
        let play = RuleOptions::default();
        let c = chart(&play);
        let judge = |judge, combo| EngineEvent::Judge {
            lane: 0,
            judge,
            diff: 0,
            combo,
            tail: false,
        };
        for clear in [false, true] {
            let sides = vec![side(&c, &play, None, false)];
            let mut s = Session::new(c.clone(), sides, false, Instant::now());
            s.fx.good_clears_poor = clear;
            let (now, mut out) = (Instant::now(), Vec::new());
            s.event(0, judge(Judge::Poor, 0), now, SHOW, &mut out);
            assert!(s.poor_until.is_some());
            s.event(0, judge(Judge::Good, 1), now, SHOW, &mut out);
            assert_eq!(s.poor_until.is_none(), clear);
            // the next BAD shows it again
            s.event(0, judge(Judge::Bad, 0), now, SHOW, &mut out);
            assert!(s.poor_until.is_some());
        }
    }

    #[test]
    fn notes_wait_at_the_line_until_judged() {
        let play = RuleOptions::default();
        let c = chart(&play);
        let sides = vec![side(&c, &play, None, false)];
        let mut s = Session::new(c, sides, false, Instant::now());
        let first = |s: &Session| {
            let e = s.engine().unwrap();
            let i = e.notes().iter().position(|n| !n.is_mine()).unwrap();
            (i, e.notes()[i].time)
        };
        let (i, time) = first(&s);
        let held = |s: &Session, t| held_at_line(&s.engine().unwrap().notes()[i], t);
        let mut out = Vec::new();
        s.step(
            time - 10_000,
            s.instant_at(time - 10_000),
            [0, 0],
            SHOW,
            &mut out,
        );
        assert!(!held(&s, time - 10_000), "not yet at the line");
        // past its time, unjudged: at the line, until the miss
        s.step(
            time + 100_000,
            s.instant_at(time + 100_000),
            [0, 0],
            SHOW,
            &mut out,
        );
        assert!(held(&s, time + 100_000));
        s.step(
            time + 400_000,
            s.instant_at(time + 400_000),
            [0, 0],
            SHOW,
            &mut out,
        );
        assert!(!held(&s, time + 400_000), "missed: scrolls on");
        // a hit one never waits
        let sides = vec![side(&s.chart, &play, None, true)];
        let mut s = Session::new(s.chart.clone(), sides, false, Instant::now());
        s.step(
            time + 50_000,
            s.instant_at(time + 50_000),
            [0, 0],
            SHOW,
            &mut out,
        );
        assert!(!held(&s, time + 50_000));
    }

    #[test]
    fn keys_early_judge_and_move_the_offset() {
        let play = RuleOptions::default();
        let c = chart(&play);
        // every note pressed 45 ms early (> 0: early), long notes left alone
        let mut keys: Vec<(i64, u8, bool)> = c
            .notes
            .iter()
            .filter(|n| n.is_playable() && matches!(n.kind, NoteKind::Normal))
            .flat_map(|n| [(n.time - 45_000, n.lane, true), (n.time, n.lane, false)])
            .collect();
        keys.sort_by_key(|k| k.0);
        let sides = vec![side(&c, &play, None, false)];
        let mut s = Session::new(c, sides, false, Instant::now());
        let mut out = Vec::new();
        let mut next = 0;
        run(&mut s, &mut out, |s, t| {
            while let Some(&(time, lane, pressed)) = keys.get(next).filter(|k| k.0 <= t) {
                let ev = LaneEv {
                    lane,
                    sub: 0,
                    pressed,
                    time: s.instant_at(time),
                };
                // presses from a keyboard and a controller by turns
                let src = match next % 4 {
                    0 => InputSource::Keyboard,
                    _ => InputSource::Gamepad,
                };
                s.key(ev, [0, 0], Some(src));
                next += 1;
            }
        });
        let e = s.engine().unwrap();
        let hits = e.ghost.iter().filter(|j| **j == Judge::Great).count();
        assert_eq!(hits, keys.len() / 2);
        // every press counted once, by its source; releases are not
        let kb = (0..keys.len()).filter(|i| keys[*i].2 && i % 4 == 0).count();
        let d = s.device;
        assert_eq!(
            (d.keyboard, d.gamepad, d.touch),
            (kb as u32, (hits - kb) as u32, 0)
        );
        let adjust: Vec<i32> = out
            .iter()
            .filter_map(|o| match o {
                Out::Adjust(d) => Some(*d),
                _ => None,
            })
            .collect();
        assert_eq!(adjust, vec![2; hits]);
        // the keys' sounds, the key beams
        assert!(out.iter().any(|o| matches!(o, Out::Key { side: 0, .. })));
        assert!(s
            .lanes
            .iter()
            .any(|l| l.pressed.is_some() && l.released.is_some()));
    }

    #[test]
    fn a_vs_player_who_fails_stops_and_the_other_plays_on() {
        let play = RuleOptions {
            battle: true,
            ..RuleOptions::default()
        };
        let c = chart(&play);
        let hard = RuleOptions {
            gauge: GaugeType::Hard,
            ..play.clone()
        };
        let sides = vec![
            side(&c, &hard, Some(0), false),
            side(&c, &play, Some(1), true),
        ];
        let mut s = Session::new(c, sides, true, Instant::now());
        let mut out = Vec::new();
        let (failed, _) = run(&mut s, &mut out, |_, _| {});
        assert!(!failed);
        let (one, two) = (&s.sides[0], &s.sides[1]);
        assert!(one.failed.is_some() && two.failed.is_none());
        assert!(one.engine.score.passed < one.engine.score.total_notes);
        assert!(two.engine.ghost.iter().all(|j| *j == Judge::PGreat));
        assert!(s.finished_at.is_some());
    }
}
