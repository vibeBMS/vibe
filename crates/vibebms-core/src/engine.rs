//! The real-time judge engine: normal notes, long notes (LN / CN / HCN),
//! mines, back-spin scratch ends, autoplay lanes and battle sides.
//!
//! This file was written from a behaviour specification of vibe's judge
//! rules (note selection, release and miss handling, HCN body ticks, event
//! order), not from another program's code.
//!
//! Time is in microseconds of chart time. A timing difference ("diff") is
//! always `note time - input time`: positive is early (FAST).
//!
//! The engine is driven with [`Engine::input`] (queued) and
//! [`Engine::update`] (which handles the queued inputs, each at its own
//! timestamp in time order, then advances to `now`). Its output is a list
//! of [`EngineEvent`]s plus the score, logs, ghost and gauge.

use crate::chart::{Chart, LnType, NoteKind, SoundId, NO_SOUND};
use crate::gauge::GrooveGauge;
use crate::judge::{JudgeRules, JudgeWindows, NoteType, BAD, GOOD, GREAT, MISS, PGREAT, POOR};
use crate::mode::PlayMode;
use crate::score::Score;
use serde::{Deserialize, Serialize};

/// One recorded call: `update(now)` or `input(..)`. Saved replays are a
/// list of these (a bare number, or `[time, lane, key, pressed]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Op {
    Update(i64),
    Input(i64, u8, u8, bool),
}

impl Op {
    /// The op with an input's lane moved by `by` (wrapping); updates are
    /// unchanged. Moves a single play's replay onto a battle chart's 2P side.
    pub fn shifted(self, by: u8) -> Op {
        match self {
            Op::Input(time, lane, key, pressed) => {
                Op::Input(time, lane.wrapping_add(by), key, pressed)
            }
            op => op,
        }
    }
}

/// A key going down or up. `key` is the physical key within the lane
/// (scratch: 0 = up, 1 = down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub time: i64,
    pub lane: u8,
    pub key: u8,
    pub pressed: bool,
}

/// How a press picks between several notes in reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum JudgeAlgorithm {
    /// Move on to the next note once the current one is beyond GOOD (late)
    /// and the next one is within GOOD (early).
    #[default]
    Combo,
    /// The closest unjudged note.
    Duration,
    /// Always the earliest note in reach.
    Lowest,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    Judge {
        lane: u8,
        judge: usize,
        diff: i64,
        combo: u32,
        tail: bool,
    },
    Sound {
        sound: SoundId,
        lane: u8,
    },
    Volume {
        sound: SoundId,
        volume: f32,
    },
    Mine {
        lane: u8,
        damage: f32,
    },
    Failed,
}

/// What a play note is (long notes with their effective LN type).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Normal,
    Long(LnType),
    Mine(f32),
}

/// A note as the engine plays it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayNote {
    pub lane: u8,
    pub time: i64,
    pub end_time: i64,
    pub sound: SoundId,
    pub end_sound: SoundId,
    kind: Kind,
    /// 0 = head not judged, else judgement + 1.
    pub state: u8,
    /// The same for the tail.
    pub end_state: u8,
    /// The head's last recorded diff.
    pub play_time: Option<i64>,
    /// The tail's last recorded diff.
    pub end_play_time: Option<i64>,
    /// Index into the chart's notes.
    pub chart_index: usize,
}

impl PlayNote {
    pub fn is_hcn(&self) -> bool {
        self.kind == Kind::Long(LnType::Hcn)
    }

    pub fn is_long(&self) -> bool {
        matches!(self.kind, Kind::Long(_))
    }

    pub fn is_mine(&self) -> bool {
        matches!(self.kind, Kind::Mine(_))
    }

    /// The effective LN type of a long note (the chart's, else the
    /// configured LN mode); `None` for other notes.
    pub fn ln_type(&self) -> Option<LnType> {
        match self.kind {
            Kind::Long(t) => Some(t),
            _ => None,
        }
    }

    /// Fully judged: a long note's tail, a normal note's head; a mine never.
    pub fn is_done(&self) -> bool {
        match self.kind {
            Kind::Normal => self.state != 0,
            Kind::Long(_) => self.end_state != 0,
            Kind::Mine(_) => false,
        }
    }

    /// A CN or HCN (head and tail judged separately).
    fn is_charge(&self) -> bool {
        matches!(self.kind, Kind::Long(LnType::Cn | LnType::Hcn))
    }

    fn head_judged(&self) -> bool {
        self.state != 0
    }
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub rules: JudgeRules,
    /// LN mode for long notes the chart leaves untyped.
    pub default_ln: LnType,
    pub algorithm: JudgeAlgorithm,
    /// Lanes played by the engine (AUTO SCRATCH, autoplay).
    pub auto_lanes: Vec<bool>,
    /// Releasing every key of a scratch lane ends a long note there (the
    /// keyboard's way); `false` is the turntable's way, where only the
    /// key that started the hold counts.
    pub scratch_release_ends_bss: bool,
    /// Battle: the player whose lanes this engine judges (`None`: all).
    pub player: Option<usize>,
}

/// HCN body: one gauge tick for every full 200 ms (strictly more) held or
/// let go.
const HCN_TICK: i64 = 200_000;

/// Index of the empty POOR window in a note or scratch table.
const EMPTY_POOR: usize = 4;

/// Which judgement slot of a note.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Head,
    Tail,
}

/// Per-lane state.
#[derive(Clone, Debug, Default)]
struct Lane {
    scratch: bool,
    auto: bool,
    /// The lane's notes (indices into `Engine::notes`), in time order.
    notes: Vec<usize>,
    /// Invisible notes (time, sound), for empty-press keysounds only.
    hidden: Vec<(i64, SoundId)>,
    /// How many of `notes` the advance phase has passed (time <= P).
    passed: usize,
    /// How many of `notes` the miss sweep need never look at again.
    swept: usize,
    /// The long note being held.
    held: Option<usize>,
    /// An early release waiting to be judged: (input time, judgement).
    pending: Option<(i64, usize)>,
    /// LN mode: the held note's head judgement and diff.
    head: (usize, i64),
    /// Scratch: the key that started the hold.
    hold_key: Option<u8>,
    /// The HCN whose body is at the judge line.
    body: Option<usize>,
    /// HCN body time banked (µs, signed).
    counter: i64,
    rising: bool,
    /// The body is let go at the step time (keys after the inputs being
    /// handled): it is damaged.
    let_go: bool,
    /// The last body `Volume` sent for the body note was 0.
    body_quiet: bool,
    keys: [bool; 2],
    /// The keys once the inputs being handled have acted.
    after: [bool; 2],
}

impl Lane {
    fn pressed(&self) -> bool {
        self.keys[0] || self.keys[1]
    }
}

pub struct Engine {
    pub gauge: GrooveGauge,
    pub score: Score,
    /// (note time, diff) of every PGREAT..BAD.
    pub timing_log: Vec<(i64, i64)>,
    /// (note time, judgement, sign of diff) of every judgement.
    pub judge_log: Vec<(i64, u8, i8)>,
    /// Every consuming judgement, in order.
    pub ghost: Vec<u8>,
    cfg: EngineConfig,
    mode: PlayMode,
    notes: Vec<PlayNote>,
    lanes: Vec<Lane>,
    damaged: Vec<bool>,
    last_time: i64,
    /// Searched range of diffs: earliest late bound, latest early bound.
    reach: (i64, i64),
    /// The previous step's time (P); `None` before the first update.
    prev: Option<i64>,
    queue: Vec<InputEvent>,
    combo: u32,
    failed: bool,
    recorded: Option<Vec<Op>>,
    events: Vec<EngineEvent>,
}

impl Engine {
    pub fn new(chart: &Chart, cfg: EngineConfig, gauge: GrooveGauge) -> Self {
        let mode = chart.mode();
        let lane_count = mode.lane_count();
        let mut lanes: Vec<Lane> = (0..lane_count)
            .map(|l| Lane {
                scratch: mode.is_scratch(l),
                auto: cfg.auto_lanes.get(l).copied().unwrap_or(false),
                ..Lane::default()
            })
            .collect();

        let takes = |lane: usize| {
            lane < lane_count && cfg.player.is_none_or(|p| mode.player_of_lane(lane) == p)
        };
        let mut order: Vec<usize> = (0..chart.notes.len()).collect();
        order.sort_by_key(|&i| (chart.notes[i].time, chart.notes[i].lane));

        let mut notes = Vec::new();
        let mut total = 0u32;
        for i in order {
            let n = &chart.notes[i];
            let lane = n.lane as usize;
            if !takes(lane) {
                continue;
            }
            let (kind, end_time, end_sound) = match &n.kind {
                NoteKind::Normal => (Kind::Normal, n.time, NO_SOUND),
                NoteKind::Long {
                    end_time,
                    end_sound,
                    ln_type,
                    ..
                } => (
                    Kind::Long(ln_type.unwrap_or(cfg.default_ln)),
                    *end_time,
                    *end_sound,
                ),
                NoteKind::Mine { damage } => (Kind::Mine(*damage), n.time, NO_SOUND),
                NoteKind::Invisible => {
                    lanes[lane].hidden.push((n.time, n.sound));
                    continue;
                }
            };
            total += match kind {
                Kind::Normal | Kind::Long(LnType::Ln) => 1,
                Kind::Long(_) => 2,
                Kind::Mine(_) => 0,
            };
            lanes[lane].notes.push(notes.len());
            notes.push(PlayNote {
                lane: n.lane,
                time: n.time,
                end_time,
                sound: n.sound,
                end_sound,
                kind,
                state: 0,
                end_state: 0,
                play_time: None,
                end_play_time: None,
                chart_index: i,
            });
        }
        let last_time = notes.iter().map(|n| n.end_time).max().unwrap_or(0);
        let reach = cfg.rules.windows.extent();
        let score = Score {
            total_notes: total,
            ..Score::default()
        };
        Engine {
            gauge,
            score,
            timing_log: Vec::new(),
            judge_log: Vec::new(),
            ghost: Vec::new(),
            damaged: vec![false; notes.len()],
            cfg,
            mode,
            notes,
            lanes,
            last_time,
            reach,
            prev: None,
            queue: Vec::new(),
            combo: 0,
            failed: false,
            recorded: None,
            events: Vec::new(),
        }
    }

    // ----- accessors ------------------------------------------------------

    pub fn notes(&self) -> &[PlayNote] {
        &self.notes
    }

    pub fn combo(&self) -> u32 {
        self.combo
    }

    pub fn is_failed(&self) -> bool {
        self.failed
    }

    pub fn rules(&self) -> &JudgeRules {
        &self.cfg.rules
    }

    pub fn windows(&self) -> &JudgeWindows {
        &self.cfg.rules.windows
    }

    /// The HCN whose body is at the line on `lane`, and whether the lane
    /// is held (any key down, or an auto lane).
    pub fn hcn_passing(&self, lane: usize) -> Option<(usize, bool)> {
        let l = self.lanes.get(lane)?;
        l.body.map(|b| (b, l.auto || l.pressed()))
    }

    /// Note `i` is a HCN that was let go while its body was at the line.
    pub fn hcn_damaged(&self, i: usize) -> bool {
        self.damaged.get(i).copied().unwrap_or(false)
    }

    /// The long note held on `lane`.
    pub fn holding(&self, lane: usize) -> Option<usize> {
        self.lanes.get(lane).and_then(|l| l.held)
    }

    /// Whether this engine judges `lane` (its player's lanes in battle).
    pub fn judges(&self, lane: u8) -> bool {
        self.cfg
            .player
            .is_none_or(|p| self.mode.player_of_lane(lane as usize) == p)
    }

    pub fn player(&self) -> Option<usize> {
        self.cfg.player
    }

    pub fn is_auto(&self, lane: usize) -> bool {
        self.lanes.get(lane).is_some_and(|l| l.auto)
    }

    /// Past the last note's end and every judgement made.
    pub fn is_finished(&self, now: i64) -> bool {
        now > self.last_time && self.score.passed >= self.score.total_notes
    }

    // ----- recording ------------------------------------------------------

    /// Start keeping the ops given to this engine (idempotent).
    pub fn record(&mut self) {
        self.recorded.get_or_insert_with(Vec::new);
    }

    pub fn recorded(&self) -> &[Op] {
        self.recorded.as_deref().unwrap_or(&[])
    }

    pub fn apply(&mut self, op: Op) {
        match op {
            Op::Update(now) => self.update(now),
            Op::Input(time, lane, key, pressed) => self.input(InputEvent {
                time,
                lane,
                key,
                pressed,
            }),
        }
    }

    // ----- input and output ---------------------------------------------

    /// Queue a key event; it takes effect at the next `update`.
    pub fn input(&mut self, ev: InputEvent) {
        if let Some(r) = &mut self.recorded {
            r.push(Op::Input(ev.time, ev.lane, ev.key, ev.pressed));
        }
        let lane = ev.lane as usize;
        if lane < self.lanes.len() && self.judges(ev.lane) && !self.lanes[lane].auto {
            self.queue.push(ev);
        }
    }

    /// The (lane, key) pairs down, the queued inputs included.
    pub fn held(&self) -> Vec<(u8, u8)> {
        let mut out: Vec<(u8, u8)> = Vec::new();
        for (lane, l) in self.lanes.iter().enumerate() {
            for (key, down) in l.keys.iter().enumerate() {
                if *down {
                    out.push((lane as u8, key as u8));
                }
            }
        }
        for e in &self.queue {
            let pair = (e.lane, e.key & 1);
            out.retain(|p| *p != pair);
            if e.pressed {
                out.push(pair);
            }
        }
        out
    }

    pub fn drain_events(&mut self) -> std::vec::Drain<'_, EngineEvent> {
        self.events.drain(..)
    }

    /// Take the `Sound` and `Volume` events; the rest stay for
    /// `drain_events`.
    pub fn take_sounds(&mut self) -> Vec<EngineEvent> {
        let mut sounds = Vec::new();
        self.events.retain(|e| {
            let sound = matches!(e, EngineEvent::Sound { .. } | EngineEvent::Volume { .. });
            if sound {
                sounds.push(e.clone());
            }
            !sound
        });
        sounds
    }

    // ----- time ------------------------------------------------------------

    /// Handle the queued inputs, each at its own time (in time order), then
    /// advance to `now`.
    pub fn update(&mut self, now: i64) {
        if let Some(r) = &mut self.recorded {
            r.push(Op::Update(now));
        }
        if self.prev.is_none() {
            let p = now.min(0).saturating_sub(1);
            self.prev = Some(p);
            for l in &mut self.lanes {
                l.passed = l.notes.partition_point(|&i| self.notes[i].time <= p);
            }
        }
        // taken while handled, then given back to keep its buffer
        let mut queue = std::mem::take(&mut self.queue);
        queue.sort_by_key(|e| e.time);
        for group in queue.chunk_by(|a, b| a.time == b.time) {
            let t = group[0].time;
            let m = now.min(self.prev().max(t));
            for l in &mut self.lanes {
                l.after = l.keys;
            }
            for e in group {
                self.lanes[e.lane as usize].after[(e.key & 1) as usize] = e.pressed;
            }
            self.advance(m, Some(t));
            for l in &mut self.lanes {
                l.keys = l.after;
            }
            for e in group {
                self.handle(e);
            }
            self.settle(m);
        }
        queue.clear();
        self.queue = queue;
        self.advance(now, None);
        self.settle(now);
    }

    fn prev(&self) -> i64 {
        self.prev.unwrap_or(i64::MIN)
    }

    /// Advance phase over (P, t]. `group`: the time of the inputs about to
    /// be handled. Notes exactly at that time, and a HCN body let go at
    /// `t`, see the keys after them (`Lane::after`); everything before
    /// sees the keys as they were.
    fn advance(&mut self, t: i64, group: Option<i64>) {
        let p = self.prev();
        if t < p {
            return;
        }
        for lane in 0..self.lanes.len() {
            let pressed_before = self.lanes[lane].pressed();
            let pressed_after = self.lanes[lane].after.contains(&true);
            let auto = self.lanes[lane].auto;
            while let Some(&i) = self.lanes[lane].notes.get(self.lanes[lane].passed) {
                if self.notes[i].time > t {
                    break;
                }
                self.lanes[lane].passed += 1;
                let time = self.notes[i].time;
                let pressed = if group == Some(time) {
                    pressed_after
                } else {
                    pressed_before
                };
                self.pass_note(lane, i, pressed && !auto);
            }
            if self.lanes[lane]
                .body
                .is_some_and(|b| self.notes[b].end_time <= t)
            {
                let l = &mut self.lanes[lane];
                l.body = None;
                l.counter = 0;
            }
            if auto {
                if let Some(h) = self.lanes[lane].held {
                    let n = &self.notes[h];
                    if n.is_charge() && n.end_time <= t && n.end_state == 0 {
                        self.judge(h, Slot::Tail, PGREAT, 0, true);
                        self.sound(lane, self.notes[h].end_sound);
                        self.release_hold(lane);
                    }
                }
            }
            // ticks over (P, t] follow the keys as they were; a lane let go
            // at t (by the inputs about to be handled) damages the body at
            // once
            let pressed_at_t = if group.is_some() {
                pressed_after
            } else {
                pressed_before
            };
            let l = &self.lanes[lane];
            let (rising, let_go) = l.body.map_or((false, false), |b| {
                let es = self.notes[b].end_state;
                let good_tail = (1..=GOOD as u8 + 1).contains(&es);
                (
                    pressed_before || good_tail || auto,
                    !(pressed_at_t || good_tail || auto),
                )
            });
            let l = &mut self.lanes[lane];
            l.rising = rising;
            l.let_go = let_go;
        }
        self.body_pass(t.saturating_sub(p));
        self.prev = Some(t);
    }

    /// A note's time is reached in the advance phase.
    fn pass_note(&mut self, lane: usize, i: usize, pressed: bool) {
        let kind = self.notes[i].kind;
        if kind == Kind::Long(LnType::Hcn) {
            let l = &mut self.lanes[lane];
            l.body = Some(i);
            l.body_quiet = false;
        }
        if let Kind::Mine(damage) = kind {
            if pressed {
                self.gauge.add_value(-damage);
                self.events.push(EngineEvent::Mine {
                    lane: lane as u8,
                    damage,
                });
                self.sound(lane, self.notes[i].sound);
                self.check_failed();
            }
            return;
        }
        if !self.lanes[lane].auto || self.notes[i].head_judged() {
            return;
        }
        match kind {
            Kind::Normal => {
                self.sound(lane, self.notes[i].sound);
                self.judge(i, Slot::Head, PGREAT, 0, true);
            }
            Kind::Long(ty) => {
                // a long note still held ends where the next one starts
                if let Some(h) = self.lanes[lane].held {
                    if self.notes[h].is_charge() {
                        self.judge(h, Slot::Tail, PGREAT, 0, true);
                    } else {
                        let (j, d) = self.lanes[lane].head;
                        self.ln_final(h, j, d);
                    }
                    self.sound(lane, self.notes[h].end_sound);
                    self.release_hold(lane);
                }
                self.sound(lane, self.notes[i].sound);
                if ty == LnType::Ln {
                    self.lanes[lane].head = (PGREAT, 0);
                } else {
                    self.judge(i, Slot::Head, PGREAT, 0, true);
                }
                self.lanes[lane].held = Some(i);
            }
            Kind::Mine(_) => {}
        }
    }

    /// HCN bodies at the line: gauge ticks, the body's volume.
    fn body_pass(&mut self, dt: i64) {
        for lane in 0..self.lanes.len() {
            let Some(b) = self.lanes[lane].body else {
                continue;
            };
            if !self.notes[b].head_judged() {
                continue;
            }
            let rising = self.lanes[lane].rising;
            if !rising || self.lanes[lane].let_go {
                self.damaged[b] = true;
            }
            if rising {
                let l = &mut self.lanes[lane];
                l.counter = l.counter.saturating_add(dt);
                while self.lanes[lane].counter > HCN_TICK {
                    self.gauge.update(GREAT, 0.5);
                    self.lanes[lane].counter -= HCN_TICK;
                }
            } else {
                let l = &mut self.lanes[lane];
                l.counter = l.counter.saturating_sub(dt);
                while self.lanes[lane].counter < -HCN_TICK {
                    self.gauge.update(BAD, 0.5);
                    self.lanes[lane].counter += HCN_TICK;
                }
            }
            let es = self.notes[b].end_state;
            if es == BAD as u8 + 1 || es == POOR as u8 + 1 {
                let quiet = !rising;
                if quiet != self.lanes[lane].body_quiet {
                    self.lanes[lane].body_quiet = quiet;
                    self.events.push(EngineEvent::Volume {
                        sound: self.notes[b].sound,
                        volume: if quiet { 0.0 } else { 1.0 },
                    });
                }
            }
            self.check_failed();
        }
    }

    /// Settle phase at `m`: pending releases and ends of held notes, the
    /// miss sweep, charge notes held too long.
    fn settle(&mut self, m: i64) {
        let w = &self.cfg.rules.windows;
        let (ln_margin, scratch_margin) = (w.ln_margin, w.scratch_ln_margin);
        for lane in 0..self.lanes.len() {
            let scratch = self.lanes[lane].scratch;
            let margin = if scratch { scratch_margin } else { ln_margin };
            if let Some(h) = self.lanes[lane].held {
                let end = self.notes[h].end_time;
                let pending = self.lanes[lane].pending;
                if !self.notes[h].is_charge() {
                    match pending {
                        Some((rt, BAD)) if rt.saturating_add(margin) <= m => {
                            self.mute(h);
                            self.ln_final(h, BAD, end.saturating_sub(rt));
                            self.sound(lane, self.notes[h].end_sound);
                            self.release_hold(lane);
                        }
                        _ if end < m => {
                            let (j, d) = match pending {
                                Some((rt, j)) => (j, end.saturating_sub(rt)),
                                None => self.lanes[lane].head,
                            };
                            self.ln_final(h, j, d);
                            self.sound(lane, self.notes[h].end_sound);
                            self.release_hold(lane);
                        }
                        _ => {}
                    }
                } else if let Some((rt, j)) = pending {
                    if rt.saturating_add(margin) <= m {
                        self.mute(h);
                        self.judge(h, Slot::Tail, j, end.saturating_sub(rt), true);
                        self.sound(lane, self.notes[h].end_sound);
                        self.release_hold(lane);
                    }
                }
            }

            let bad_late = self.lane_table(lane)[BAD].0;
            let limit = m.saturating_add(bad_late);
            let mut k = self.lanes[lane].swept;
            while let Some(&i) = self.lanes[lane].notes.get(k) {
                let n = &self.notes[i];
                if n.time >= limit {
                    break;
                }
                k += 1;
                if n.head_judged() {
                    continue;
                }
                let diff = n.time.saturating_sub(m);
                let kind = n.kind;
                match kind {
                    Kind::Normal => self.judge(i, Slot::Head, POOR, diff, true),
                    Kind::Long(LnType::Ln) => {
                        if self.lanes[lane].held != Some(i) {
                            self.ln_final(i, POOR, diff);
                        }
                    }
                    Kind::Long(_) => {
                        self.judge(i, Slot::Head, POOR, diff, true);
                        self.judge(i, Slot::Tail, POOR, diff, true);
                    }
                    Kind::Mine(_) => {}
                }
            }
            // move the sweep's start past what it need never see again
            let l = &self.lanes[lane];
            let mut s = l.swept;
            while let Some(&i) = l.notes.get(s) {
                let n = &self.notes[i];
                if n.is_mine() || n.head_judged() {
                    s += 1;
                } else {
                    break;
                }
            }
            self.lanes[lane].swept = s;

            if let Some(h) = self.lanes[lane].held {
                let n = &self.notes[h];
                if n.is_charge() && n.end_state == 0 && n.end_time < limit {
                    let diff = n.end_time.saturating_sub(m);
                    self.judge(h, Slot::Tail, POOR, diff, true);
                    self.release_hold(lane);
                }
            }
        }
    }

    // ----- inputs ----------------------------------------------------------

    fn handle(&mut self, e: &InputEvent) {
        let lane = e.lane as usize;
        let key = e.key & 1;
        if e.pressed {
            self.press(lane, key, e.time);
        } else {
            self.release(lane, key, e.time);
        }
    }

    fn press(&mut self, lane: usize, key: u8, t: i64) {
        if let Some(h) = self.lanes[lane].held {
            let l = &mut self.lanes[lane];
            l.pending = None;
            if self.notes[h].is_charge() && l.scratch && l.hold_key.is_some_and(|k| k != key) {
                self.back_spin(lane, h, t);
            }
            return;
        }
        let Some((i, j)) = self.select(lane, t) else {
            self.empty_press_sound(lane, t);
            return;
        };
        let d = self.quantize(self.notes[i].time.saturating_sub(t));
        self.sound(lane, self.notes[i].sound);
        let consume = self.cfg.rules.vanish[j];
        match self.notes[i].kind {
            Kind::Long(LnType::Ln) => {
                if consume {
                    self.start_hold(lane, i, key);
                    self.lanes[lane].head = (j, d);
                } else {
                    self.judge(i, Slot::Head, j, d, false);
                }
            }
            Kind::Long(_) => {
                if consume {
                    self.start_hold(lane, i, key);
                }
                self.judge(i, Slot::Head, j, d, consume);
            }
            Kind::Normal => self.judge(i, Slot::Head, j, d, consume),
            Kind::Mine(_) => {}
        }
    }

    fn start_hold(&mut self, lane: usize, i: usize, key: u8) {
        let l = &mut self.lanes[lane];
        l.held = Some(i);
        l.pending = None;
        l.hold_key = l.scratch.then_some(key);
    }

    fn release_hold(&mut self, lane: usize) {
        let l = &mut self.lanes[lane];
        l.held = None;
        l.pending = None;
        l.hold_key = None;
    }

    /// The note a press at `t` on `lane` judges, and how.
    fn select(&self, lane: usize, t: i64) -> Option<(usize, usize)> {
        let w = &self.cfg.rules.windows;
        let ty = self.note_type(lane);
        let table = w.table(ty);
        let (late, early) = self.reach;
        let good = table.get(GOOD).copied().unwrap_or((0, 0));
        let empty = table.get(EMPTY_POOR).copied();
        let notes = &self.lanes[lane].notes;
        // a quantised diff is within half a frame of the raw one
        let from = late.saturating_sub(20_000);
        let start = notes.partition_point(|&i| self.notes[i].time.saturating_sub(t) < from);
        let mut choice: Option<(usize, usize)> = None;
        for &i in &notes[start..] {
            let n = &self.notes[i];
            let raw = n.time.saturating_sub(t);
            let d = w.quantize(raw);
            if d > early {
                break;
            }
            if d < late || n.is_mine() {
                continue;
            }
            let judged = n.head_judged();
            if let Some((c, _)) = choice {
                let cur = &self.notes[c];
                let cur_raw = cur.time.saturating_sub(t);
                let competes = cur.head_judged()
                    || match self.cfg.algorithm {
                        JudgeAlgorithm::Combo => !judged && cur_raw < good.0 && raw <= good.1,
                        JudgeAlgorithm::Duration => !judged && raw.abs() < cur_raw.abs(),
                        JudgeAlgorithm::Lowest => false,
                    };
                if !competes {
                    continue;
                }
            }
            if self.cfg.rules.miss_one
                && (judged || (n.play_time.is_some() && !(good.0 <= d && d <= good.1)))
            {
                continue;
            }
            let j = if judged {
                empty.filter(|(l, e)| *l <= d && d <= *e).map(|_| MISS)
            } else {
                match w.judge(ty, d) {
                    k if k <= BAD => Some(k),
                    EMPTY_POOR if table.len() > EMPTY_POOR => Some(MISS),
                    _ => None,
                }
            };
            match j {
                None => {}
                Some(MISS) => {
                    let closer = choice.is_none_or(|(c, _)| {
                        raw.abs() < self.notes[c].time.saturating_sub(t).abs()
                    });
                    if closer {
                        choice = Some((i, MISS));
                    }
                }
                Some(k) => choice = Some((i, k)),
            }
        }
        choice
    }

    fn release(&mut self, lane: usize, key: u8, t: i64) {
        let Some(h) = self.lanes[lane].held else {
            return;
        };
        let scratch = self.lanes[lane].scratch;
        let end = self.notes[h].end_time;
        let d = self.quantize(end.saturating_sub(t));
        let r = self.windows().judge(self.end_type(lane), d).min(POOR);
        let charge = self.notes[h].is_charge();
        let keyboard = self.cfg.scratch_release_ends_bss || !scratch;
        let counts = if keyboard {
            !self.lanes[lane].pressed()
        } else {
            self.lanes[lane].hold_key == Some(key) && (!charge || r == POOR)
        };
        if !counts {
            return;
        }
        if !keyboard {
            self.lanes[lane].hold_key = None;
        }
        if charge {
            if r >= BAD && d > 0 {
                self.lanes[lane].pending = Some((t, r));
            } else {
                self.judge(h, Slot::Tail, r, d, true);
                self.sound(lane, self.notes[h].end_sound);
                self.release_hold(lane);
            }
        } else {
            let (hj, hd) = self.lanes[lane].head;
            let j = r.max(hj);
            let dd = if hd.abs() > d.abs() { hd } else { d };
            if dd > 0 {
                self.lanes[lane].pending = Some((t, j.min(BAD)));
            } else {
                self.ln_final(h, j, dd);
                self.sound(lane, self.notes[h].end_sound);
                self.release_hold(lane);
            }
        }
    }

    /// The other scratch key pressed while holding a charge note ends it.
    fn back_spin(&mut self, lane: usize, h: usize, t: i64) {
        let d = self.quantize(self.notes[h].end_time.saturating_sub(t));
        let r = self.windows().judge(NoteType::LongScratchEnd, d).min(POOR);
        self.sound(lane, self.notes[h].end_sound);
        self.judge(h, Slot::Tail, r, d, true);
        self.release_hold(lane);
    }

    /// The keysound of a press that judges nothing.
    fn empty_press_sound(&mut self, lane: usize, t: i64) {
        let l = &self.lanes[lane];
        if l.body.is_some() {
            return;
        }
        let before = l.notes.partition_point(|&i| self.notes[i].time < t);
        let visible = l.notes[..before].iter().rev().copied().find(|&i| {
            let n = &self.notes[i];
            !(n.is_long() && n.head_judged())
        });
        let hidden = l.hidden[..l.hidden.partition_point(|h| h.0 < t)].last();
        let sound = match (visible, hidden) {
            (Some(v), Some(&(ht, hs))) => {
                if self.notes[v].time >= ht {
                    self.visible_sound(v)
                } else {
                    Some(hs)
                }
            }
            (Some(v), None) => self.visible_sound(v),
            (None, Some(&(_, hs))) => Some(hs),
            (None, None) => l.notes.first().and_then(|&v| self.visible_sound(v)),
        };
        if let Some(s) = sound {
            self.sound(lane, s);
        }
    }

    fn visible_sound(&self, i: usize) -> Option<SoundId> {
        let n = &self.notes[i];
        (!n.is_mine()).then_some(n.sound)
    }

    // ----- judgements --------------------------------------------------------

    /// Record judgement `j` with `diff` on a note's head or tail.
    fn judge(&mut self, i: usize, slot: Slot, j: usize, diff: i64, consume: bool) {
        let n = &mut self.notes[i];
        if consume {
            match slot {
                Slot::Head => n.state = j as u8 + 1,
                Slot::Tail => n.end_state = j as u8 + 1,
            }
            self.score.passed += 1;
            self.ghost.push(j as u8);
        }
        let timing = match slot {
            Slot::Head => &mut n.play_time,
            Slot::Tail => &mut n.end_play_time,
        };
        // PMS: a note gives at most one POOR
        if self.cfg.rules.miss_one && j == POOR && timing.is_some() {
            return;
        }
        *timing = Some(diff);
        let (lane, time) = match slot {
            Slot::Head => (n.lane, n.time),
            Slot::Tail => (n.lane, n.end_time),
        };
        self.score.add(j, diff);
        if j <= BAD {
            self.timing_log.push((time, diff));
        }
        self.judge_log.push((time, j as u8, diff.signum() as i8));
        if self.cfg.rules.combo[j] {
            if j != MISS {
                self.combo += 1;
                self.score.max_combo = self.score.max_combo.max(self.combo);
            }
        } else {
            self.combo = 0;
            self.score.combo_breaks += 1;
        }
        self.gauge.update(j, 1.0);
        self.events.push(EngineEvent::Judge {
            lane,
            judge: j,
            diff,
            combo: self.combo,
            tail: slot == Slot::Tail,
        });
        self.check_failed();
    }

    /// An LN-mode note's one judgement: on the head, the tail done with it.
    fn ln_final(&mut self, i: usize, j: usize, diff: i64) {
        self.judge(i, Slot::Head, j, diff, true);
        self.notes[i].end_state = self.notes[i].state;
    }

    fn check_failed(&mut self) {
        if !self.failed && self.gauge.is_failed_now() {
            self.failed = true;
            self.events.push(EngineEvent::Failed);
        }
    }

    /// Play a keysound (nothing for a note without one).
    fn sound(&mut self, lane: usize, sound: SoundId) {
        if sound != NO_SOUND {
            self.events.push(EngineEvent::Sound {
                sound,
                lane: lane as u8,
            });
        }
    }

    /// Mute a long note's head keysound.
    fn mute(&mut self, i: usize) {
        self.events.push(EngineEvent::Volume {
            sound: self.notes[i].sound,
            volume: 0.0,
        });
    }

    // ----- tables ------------------------------------------------------------

    fn quantize(&self, diff: i64) -> i64 {
        self.cfg.rules.windows.quantize(diff)
    }

    fn note_type(&self, lane: usize) -> NoteType {
        if self.lanes[lane].scratch {
            NoteType::Scratch
        } else {
            NoteType::Note
        }
    }

    fn end_type(&self, lane: usize) -> NoteType {
        if self.lanes[lane].scratch {
            NoteType::LongScratchEnd
        } else {
            NoteType::LongEnd
        }
    }

    fn lane_table(&self, lane: usize) -> &[(i64, i64)] {
        self.cfg.rules.windows.table(self.note_type(lane))
    }
}
