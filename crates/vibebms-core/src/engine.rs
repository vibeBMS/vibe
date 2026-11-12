//! Real-time judge engine.
//!
//! The behaviour follows beatoraja's judge manager closely (note selection
//! algorithms, LN/CN/HCN handling, back-spin scratch, mines, empty POOR,
//! PMS "miss once" rule) but is an independent implementation.
//!
//! Usage: push [`InputEvent`]s with their precise timestamps (µs, chart time,
//! offset-corrected) via [`Engine::input`], then call [`Engine::update`] every
//! frame with the current chart time and drain [`Engine::drain_events`].

use crate::chart::{Chart, LnType, NoteKind, SoundId, NO_SOUND};
use crate::gauge::GrooveGauge;
use crate::judge::*;
use crate::score::Score;
use serde::{Deserialize, Serialize};

/// HCN gauge tick interval (µs).
const HCN_TICK: i64 = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    /// Chart time in µs.
    pub time: i64,
    pub lane: u8,
    /// Physical key index within the lane (scratch: 0 = up, 1 = down).
    pub key: u8,
    pub pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum JudgeAlgorithm {
    /// Prefer the next note if the earlier one can no longer get GOOD.
    #[default]
    Combo,
    /// Prefer the note closest in time.
    Duration,
    /// Always the earliest note.
    Lowest,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    Judge { lane: u8, judge: usize, diff: i64, combo: u32, tail: bool },
    Sound { sound: SoundId, lane: u8 },
    /// Mute / unmute a keysound (HCN body, early LN release).
    Volume { sound: SoundId, volume: f32 },
    Mine { lane: u8, damage: f32 },
    /// Survival gauge emptied (play should stop unless GAS=Continue).
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PKind {
    Normal,
    Long(LnType),
    Mine(f32),
}

/// Runtime state of a note.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayNote {
    pub lane: u8,
    pub time: i64,
    pub end_time: i64,
    pub sound: SoundId,
    pub end_sound: SoundId,
    kind: PKind,
    /// 0 = unjudged, otherwise judge + 1.
    pub state: u8,
    pub end_state: u8,
    pub play_time: Option<i64>,
    pub end_play_time: Option<i64>,
    /// Index into `Chart::notes`.
    pub chart_index: usize,
}

impl PlayNote {
    pub fn is_long(&self) -> bool {
        matches!(self.kind, PKind::Long(_))
    }
    pub fn is_mine(&self) -> bool {
        matches!(self.kind, PKind::Mine(_))
    }
    pub fn ln_type(&self) -> Option<LnType> {
        match self.kind {
            PKind::Long(t) => Some(t),
            _ => None,
        }
    }
    /// Note is fully resolved (for rendering: hide it).
    pub fn is_done(&self) -> bool {
        match self.kind {
            PKind::Long(_) => self.end_state != 0,
            PKind::Mine(_) => false,
            PKind::Normal => self.state != 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct LaneState {
    cursor: usize,
    processing: Option<usize>,
    passing: Option<usize>,
    increase: bool,
    passing_count: i64,
    release_time: Option<i64>,
    ln_end_judge: Option<usize>,
    ln_start_judge: usize,
    ln_start_dur: i64,
    sc_key: Option<u8>,
    keys: [bool; 2],
    auto_hold: bool,
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub rules: JudgeRules,
    /// LN behaviour for charts that do not specify one.
    pub default_ln: LnType,
    pub algorithm: JudgeAlgorithm,
    /// Lanes played automatically (autoplay, AUTO-SCRATCH).
    pub auto_lanes: Vec<bool>,
    /// If true, releasing the scratch ends a back-spin scratch like a normal
    /// key (keyboard / touch). If false the opposite direction must be
    /// spun (turntable controllers, beatoraja behaviour).
    pub scratch_release_ends_bss: bool,
}

pub struct Engine {
    notes: Vec<PlayNote>,
    lane_notes: Vec<Vec<usize>>,
    lane_hidden: Vec<Vec<(i64, SoundId)>>,
    lanes: Vec<LaneState>,
    scratch: Vec<bool>,
    cfg: EngineConfig,
    judge_start: i64,
    judge_end: i64,
    prev_time: i64,
    pub gauge: GrooveGauge,
    pub score: Score,
    combo: u32,
    events: Vec<EngineEvent>,
    queue: Vec<InputEvent>,
    failed: bool,
    /// (chart time, diff) of every PG..BAD judgement, for timing graphs.
    pub timing_log: Vec<(i64, i64)>,
    /// Judgement in order of resolution (ghost data).
    pub ghost: Vec<u8>,
    last_time: i64,
}

impl Engine {
    pub fn new(chart: &Chart, cfg: EngineConfig, gauge: GrooveGauge) -> Self {
        let mode = chart.mode();
        let lanes = mode.lane_count();
        let mut notes = Vec::new();
        let mut lane_notes = vec![Vec::new(); lanes];
        let mut lane_hidden = vec![Vec::new(); lanes];
        for (ci, n) in chart.notes.iter().enumerate() {
            let lane = n.lane as usize;
            if lane >= lanes {
                continue;
            }
            let (kind, end_time, end_sound) = match &n.kind {
                NoteKind::Normal => (PKind::Normal, n.time, NO_SOUND),
                NoteKind::Long { end_time, end_sound, ln_type, .. } => {
                    (PKind::Long(ln_type.unwrap_or(cfg.default_ln)), *end_time, *end_sound)
                }
                NoteKind::Mine { damage } => (PKind::Mine(*damage), n.time, NO_SOUND),
                NoteKind::Invisible => {
                    lane_hidden[lane].push((n.time, n.sound));
                    continue;
                }
            };
            lane_notes[lane].push(notes.len());
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
                chart_index: ci,
            });
        }
        let (judge_start, judge_end) = cfg.rules.windows.extent();
        let total = chart.total_judgements(cfg.default_ln) as u32;
        let mut auto = cfg.auto_lanes.clone();
        auto.resize(lanes, false);
        let cfg = EngineConfig { auto_lanes: auto, ..cfg };
        let last_time = notes.iter().map(|n| n.end_time).max().unwrap_or(0);
        Engine {
            notes,
            lane_notes,
            lane_hidden,
            lanes: vec![LaneState::default(); lanes],
            scratch: (0..lanes).map(|l| mode.is_scratch(l)).collect(),
            cfg,
            judge_start,
            judge_end,
            prev_time: i64::MIN / 4,
            gauge,
            score: Score { total_notes: total, ..Default::default() },
            combo: 0,
            events: Vec::new(),
            queue: Vec::new(),
            failed: false,
            timing_log: Vec::new(),
            ghost: Vec::new(),
            last_time,
        }
    }

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

    /// Index (into [`Engine::notes`]) of the long note currently held on a lane.
    pub fn holding(&self, lane: usize) -> Option<usize> {
        self.lanes.get(lane).and_then(|l| l.processing)
    }

    /// HCN currently gaining gauge on this lane.
    pub fn hcn_active(&self, lane: usize) -> bool {
        self.lanes.get(lane).map(|l| l.passing.is_some() && l.increase).unwrap_or(false)
    }

    pub fn is_auto(&self, lane: usize) -> bool {
        self.cfg.auto_lanes.get(lane).copied().unwrap_or(false)
    }

    /// All notes have been judged and `now` is past the last note.
    pub fn is_finished(&self, now: i64) -> bool {
        now > self.last_time && self.score.passed >= self.score.total_notes
    }

    pub fn input(&mut self, ev: InputEvent) {
        if (ev.lane as usize) < self.lanes.len() && !self.is_auto(ev.lane as usize) {
            self.queue.push(ev);
        }
    }

    pub fn drain_events(&mut self) -> std::vec::Drain<'_, EngineEvent> {
        self.events.drain(..)
    }

    /// Advance the engine to chart time `now` (µs).
    pub fn update(&mut self, now: i64) {
        if self.prev_time == i64::MIN / 4 {
            self.prev_time = now.min(0) - 1;
        }
        let mut q = std::mem::take(&mut self.queue);
        q.sort_by_key(|e| e.time);
        let mut from = 0;
        while from < q.len() {
            let mut to = from + 1;
            while to < q.len() && q[to].time == q[from].time {
                to += 1;
            }
            for e in &q[from..to] {
                self.lanes[e.lane as usize].keys[(e.key & 1) as usize] = e.pressed;
            }
            let checkpoint = now.min(self.prev_time.max(q[from].time));
            self.advance_passing(checkpoint);
            for e in &q[from..to] {
                self.process_key(*e, checkpoint);
            }
            self.finish_time(checkpoint);
            from = to;
        }
        self.advance_passing(now);
        self.finish_time(now);
    }

    fn windows(&self) -> &JudgeWindows {
        &self.cfg.rules.windows
    }

    fn note_type(&self, lane: usize) -> NoteType {
        if self.scratch[lane] {
            NoteType::Scratch
        } else {
            NoteType::Note
        }
    }

    fn sound(&mut self, sound: SoundId, lane: usize) {
        if sound != NO_SOUND {
            self.events.push(EngineEvent::Sound { sound, lane: lane as u8 });
        }
    }

    fn reset_release(&mut self, lane: usize) {
        let l = &mut self.lanes[lane];
        l.release_time = None;
        l.ln_end_judge = None;
    }

    fn advance_passing(&mut self, t: i64) {
        let prev = self.prev_time;
        if t < prev {
            return;
        }
        for lane in 0..self.lanes.len() {
            let auto = self.is_auto(lane);
            let pressed = self.lanes[lane].keys.iter().any(|k| *k);
            let cursor = self.lanes[lane].cursor;
            let count = self.lane_notes[lane].len();
            for k in cursor..count {
                let i = self.lane_notes[lane][k];
                let n = &self.notes[i];
                if n.time > t {
                    break;
                }
                if n.time <= prev {
                    continue;
                }
                match n.kind {
                    PKind::Long(LnType::Hcn) => self.lanes[lane].passing = Some(i),
                    PKind::Mine(damage) if pressed => {
                        self.gauge.add_value(-damage);
                        let s = n.sound;
                        self.events.push(EngineEvent::Mine { lane: lane as u8, damage });
                        self.sound(s, lane);
                        self.check_failed();
                    }
                    _ => {}
                }
                if auto {
                    let n = &self.notes[i];
                    match n.kind {
                        PKind::Normal if n.state == 0 => {
                            let s = n.sound;
                            self.sound(s, lane);
                            self.judge_note(i, false, PGREAT, 0, true);
                        }
                        PKind::Long(lt) if n.state == 0 && self.lanes[lane].processing.is_none() => {
                            let s = n.sound;
                            self.sound(s, lane);
                            if lt == LnType::Ln {
                                self.lanes[lane].ln_start_judge = PGREAT;
                                self.lanes[lane].ln_start_dur = 0;
                            } else {
                                self.judge_note(i, false, PGREAT, 0, true);
                            }
                            self.lanes[lane].processing = Some(i);
                            self.lanes[lane].auto_hold = true;
                        }
                        _ => {}
                    }
                }
            }
            if let Some(p) = self.lanes[lane].passing {
                if self.notes[p].end_time <= t {
                    self.lanes[lane].passing = None;
                    self.lanes[lane].passing_count = 0;
                }
            }
            if auto {
                if let Some(p) = self.lanes[lane].processing {
                    let n = &self.notes[p];
                    if n.ln_type() != Some(LnType::Ln) && n.end_time <= t && n.end_state == 0 {
                        let s = n.end_sound;
                        self.judge_note(p, true, PGREAT, 0, true);
                        self.sound(s, lane);
                        self.lanes[lane].processing = None;
                        self.lanes[lane].auto_hold = false;
                    }
                }
            }
            let l = &self.lanes[lane];
            let next_increase = match l.passing {
                Some(p) => pressed || (1..=3).contains(&self.notes[p].end_state) || auto,
                None => false,
            };
            self.lanes[lane].increase = next_increase;
        }
        // HCN gauge ticks
        let dt = t - prev;
        for lane in 0..self.lanes.len() {
            let Some(p) = self.lanes[lane].passing else { continue };
            if self.notes[p].state == 0 {
                continue;
            }
            let snd = self.notes[p].sound;
            if self.lanes[lane].increase {
                self.lanes[lane].passing_count += dt;
                while self.lanes[lane].passing_count > HCN_TICK {
                    self.gauge.update(GREAT, 0.5);
                    self.lanes[lane].passing_count -= HCN_TICK;
                }
                if self.notes[p].end_state > 3 {
                    self.events.push(EngineEvent::Volume { sound: snd, volume: 1.0 });
                }
            } else {
                self.lanes[lane].passing_count -= dt;
                while self.lanes[lane].passing_count < -HCN_TICK {
                    self.gauge.update(BAD, 0.5);
                    self.lanes[lane].passing_count += HCN_TICK;
                }
                if self.notes[p].end_state > 3 {
                    self.events.push(EngineEvent::Volume { sound: snd, volume: 0.0 });
                }
            }
            self.check_failed();
        }
        self.prev_time = t;
    }

    fn compare(&self, t1: usize, t2: usize, ptime: i64, nt: NoteType) -> bool {
        let a = &self.notes[t1];
        let b = &self.notes[t2];
        let w = self.windows();
        match self.cfg.algorithm {
            JudgeAlgorithm::Combo => b.state == 0 && a.time < ptime + w.time(nt, GOOD, false) && b.time <= ptime + w.time(nt, GOOD, true),
            JudgeAlgorithm::Duration => (a.time - ptime).abs() > (b.time - ptime).abs() && b.state == 0,
            JudgeAlgorithm::Lowest => false,
        }
    }

    fn process_key(&mut self, ev: InputEvent, mtime: i64) {
        let lane = ev.lane as usize;
        let ptime = ev.time;
        let sc = self.scratch[lane];
        if ev.pressed {
            if let Some(p) = self.lanes[lane].processing {
                let lt = self.notes[p].ln_type().unwrap_or(LnType::Ln);
                let other_dir = self.lanes[lane].sc_key.map(|k| k != ev.key).unwrap_or(false);
                if lt != LnType::Ln && sc && other_dir {
                    // back-spin scratch: spinning the other way ends the note
                    let dm = self.notes[p].end_time - ptime;
                    let j = self.windows().judge(NoteType::LongScratchEnd, dm);
                    let s = self.notes[p].end_sound;
                    self.sound(s, lane);
                    self.judge_note(p, true, j.min(POOR), dm, true);
                    self.lanes[lane].processing = None;
                    self.lanes[lane].sc_key = None;
                }
                self.reset_release(lane);
                return;
            }
            let nt = self.note_type(lane);
            let w = self.windows().clone();
            let mut best: Option<(usize, usize)> = None;
            let cursor = self.lanes[lane].cursor;
            for k in cursor..self.lane_notes[lane].len() {
                let i = self.lane_notes[lane][k];
                let n = &self.notes[i];
                let dm = n.time - ptime;
                if dm >= self.judge_end {
                    break;
                }
                if dm < self.judge_start || n.is_mine() {
                    continue;
                }
                let replace = match best {
                    None => true,
                    Some((b, _)) => self.notes[b].state != 0 || self.compare(b, i, ptime, nt),
                };
                if !replace {
                    continue;
                }
                let n = &self.notes[i];
                if self.cfg.rules.miss_one
                    && (n.state != 0
                        || (n.play_time.is_some() && (dm > w.time(nt, GOOD, true) || dm < w.time(nt, GOOD, false))))
                {
                    continue;
                }
                let judge = if n.state != 0 {
                    if dm >= w.time(nt, 4, false) && dm <= w.time(nt, 4, true) {
                        MISS
                    } else {
                        6
                    }
                } else {
                    let j = w.judge(nt, dm);
                    if j >= 4 {
                        j + 1
                    } else {
                        j
                    }
                };
                if judge < 6 {
                    let closer = best.map(|(b, _)| (self.notes[b].time - ptime).abs() > (n.time - ptime).abs()).unwrap_or(true);
                    if judge < 4 || closer {
                        best = Some((i, judge));
                    }
                } else {
                    best = None;
                }
            }
            match best {
                Some((i, judge)) => {
                    let dm = self.notes[i].time - ptime;
                    let s = self.notes[i].sound;
                    self.sound(s, lane);
                    let vanish = self.cfg.rules.vanish[judge];
                    match self.notes[i].kind {
                        PKind::Long(LnType::Ln) => {
                            if vanish {
                                let l = &mut self.lanes[lane];
                                l.ln_start_judge = judge;
                                l.ln_start_dur = dm;
                                l.processing = Some(i);
                                l.release_time = None;
                                l.ln_end_judge = None;
                                if sc {
                                    l.sc_key = Some(ev.key);
                                }
                            } else {
                                self.judge_note(i, false, judge, dm, false);
                            }
                        }
                        PKind::Long(_) => {
                            if vanish {
                                let l = &mut self.lanes[lane];
                                l.processing = Some(i);
                                l.release_time = None;
                                l.ln_end_judge = None;
                                if sc {
                                    l.sc_key = Some(ev.key);
                                }
                            }
                            self.judge_note(i, false, judge, dm, vanish);
                        }
                        _ => self.judge_note(i, false, judge, dm, vanish),
                    }
                }
                None => self.empty_press_sound(lane, ptime),
            }
        } else {
            let Some(p) = self.lanes[lane].processing else { return };
            let mut dm = self.notes[p].end_time - ptime;
            let mut judge = self.windows().judge(if sc { NoteType::LongScratchEnd } else { NoteType::LongEnd }, dm);
            let lt = self.notes[p].ln_type().unwrap_or(LnType::Ln);
            let any_held = self.lanes[lane].keys.iter().any(|k| *k);
            if lt != LnType::Ln {
                let release = if sc && !self.cfg.scratch_release_ends_bss {
                    if judge != 4 || Some(ev.key) != self.lanes[lane].sc_key {
                        false
                    } else {
                        self.lanes[lane].sc_key = None;
                        true
                    }
                } else {
                    !any_held
                };
                if release {
                    if judge >= 3 && dm > 0 {
                        self.lanes[lane].release_time = Some(mtime);
                        self.lanes[lane].ln_end_judge = Some(judge);
                    } else {
                        let s = self.notes[p].end_sound;
                        self.judge_note(p, true, judge, dm, true);
                        self.sound(s, lane);
                        self.lanes[lane].processing = None;
                        self.reset_release(lane);
                    }
                }
            } else {
                let release = if sc && !self.cfg.scratch_release_ends_bss {
                    if Some(ev.key) != self.lanes[lane].sc_key {
                        false
                    } else {
                        self.lanes[lane].sc_key = None;
                        true
                    }
                } else {
                    !any_held
                };
                if release {
                    judge = judge.max(self.lanes[lane].ln_start_judge);
                    if self.lanes[lane].ln_start_dur.abs() > dm.abs() {
                        dm = self.lanes[lane].ln_start_dur;
                    }
                    if dm > 0 {
                        self.lanes[lane].release_time = Some(ptime);
                        self.lanes[lane].ln_end_judge = Some(judge.min(3));
                    } else {
                        let s = self.notes[p].end_sound;
                        self.judge_ln_mode(p, judge, dm);
                        self.sound(s, lane);
                        self.lanes[lane].processing = None;
                        self.reset_release(lane);
                    }
                }
            }
        }
    }

    /// Keysound for a press that hit no note: the latest note before the
    /// press (visible or invisible), or the lane's first note.
    fn empty_press_sound(&mut self, lane: usize, ptime: i64) {
        if self.lanes[lane].passing.is_some() {
            return;
        }
        let ln = &self.lane_notes[lane];
        let mut cand: Option<(i64, SoundId, bool)> = ln.first().map(|&i| (self.notes[i].time, self.notes[i].sound, self.notes[i].is_mine()));
        let hidden = &self.lane_hidden[lane];
        let h = hidden.partition_point(|(t, _)| *t < ptime);
        if h > 0 {
            cand = Some((hidden[h - 1].0, hidden[h - 1].1, false));
        }
        let v = ln.partition_point(|&i| self.notes[i].time < ptime);
        for k in (0..v).rev() {
            let n = &self.notes[ln[k]];
            if n.is_long() && n.state != 0 {
                continue;
            }
            if cand.map(|c| c.0 <= n.time).unwrap_or(true) {
                cand = Some((n.time, n.sound, n.is_mine()));
            }
            break;
        }
        if let Some((_, s, mine)) = cand {
            if !mine {
                self.sound(s, lane);
            }
        }
    }

    fn judge_ln_mode(&mut self, i: usize, judge: usize, dm: i64) {
        self.judge_note(i, false, judge, dm, true);
        self.notes[i].end_state = self.notes[i].state;
    }

    fn finish_time(&mut self, mtime: i64) {
        for lane in 0..self.lanes.len() {
            let sc = self.scratch[lane];
            let nt = self.note_type(lane);
            let margin = if sc { self.windows().scratch_ln_margin } else { self.windows().ln_margin };
            if let Some(p) = self.lanes[lane].processing {
                let lt = self.notes[p].ln_type().unwrap_or(LnType::Ln);
                let end = self.notes[p].end_time;
                let rt = self.lanes[lane].release_time;
                let ej = self.lanes[lane].ln_end_judge;
                if lt == LnType::Ln {
                    if let (Some(rt), Some(ej)) = (rt, ej) {
                        if ej >= 3 && rt + margin <= mtime {
                            let (s, es) = (self.notes[p].sound, self.notes[p].end_sound);
                            self.events.push(EngineEvent::Volume { sound: s, volume: 0.0 });
                            self.judge_ln_mode(p, ej, end - rt);
                            self.sound(es, lane);
                            self.lanes[lane].processing = None;
                            self.reset_release(lane);
                            continue_after_processing(self, lane, mtime, nt);
                            continue;
                        }
                    }
                    if end < mtime {
                        let (judge, dur) = match (ej, rt) {
                            (Some(j), Some(r)) => (j, end - r),
                            _ => (self.lanes[lane].ln_start_judge, self.lanes[lane].ln_start_dur),
                        };
                        let es = self.notes[p].end_sound;
                        self.judge_ln_mode(p, judge, dur);
                        self.sound(es, lane);
                        self.lanes[lane].processing = None;
                        self.lanes[lane].auto_hold = false;
                        self.reset_release(lane);
                    }
                } else if let (Some(rt), Some(ej)) = (rt, ej) {
                    if rt + margin <= mtime {
                        if ej >= 3 {
                            let s = self.notes[p].sound;
                            self.events.push(EngineEvent::Volume { sound: s, volume: 0.0 });
                        }
                        let es = self.notes[p].end_sound;
                        self.judge_note(p, true, ej, end - rt, true);
                        self.sound(es, lane);
                        self.lanes[lane].processing = None;
                        self.reset_release(lane);
                    }
                }
            }
            continue_after_processing(self, lane, mtime, nt);
        }

        /// Miss sweep and cursor advance for one lane.
        fn continue_after_processing(e: &mut Engine, lane: usize, mtime: i64, nt: NoteType) {
            let bad_late = e.windows().time(nt, BAD, false);
            let cursor = e.lanes[lane].cursor;
            for k in cursor..e.lane_notes[lane].len() {
                let i = e.lane_notes[lane][k];
                let n = &e.notes[i];
                if n.time >= mtime + bad_late {
                    break;
                }
                let dm = n.time - mtime;
                match n.kind {
                    PKind::Normal if n.state == 0 => e.judge_note(i, false, POOR, dm, true),
                    PKind::Long(lt) if n.state == 0 => {
                        if lt != LnType::Ln {
                            e.judge_note(i, false, POOR, dm, true);
                            e.judge_note(i, true, POOR, dm, true);
                        } else if e.lanes[lane].processing != Some(i) {
                            e.judge_ln_mode(i, POOR, dm);
                        }
                    }
                    _ => {}
                }
            }
            // charge note held too long past its end
            if let Some(p) = e.lanes[lane].processing {
                let n = &e.notes[p];
                if n.ln_type() != Some(LnType::Ln) && n.end_state == 0 && n.end_time < mtime + bad_late {
                    let dm = n.end_time - mtime;
                    e.judge_note(p, true, POOR, dm, true);
                    e.lanes[lane].processing = None;
                    e.lanes[lane].sc_key = None;
                    e.lanes[lane].auto_hold = false;
                    e.reset_release(lane);
                }
            }
            // advance cursor past resolved notes
            let limit = mtime + e.judge_start - 100_000;
            let ln = &e.lane_notes[lane];
            let mut c = e.lanes[lane].cursor;
            while c < ln.len() {
                let n = &e.notes[ln[c]];
                let resolved = n.is_mine() || n.is_done();
                if n.end_time < limit && resolved {
                    c += 1;
                } else {
                    break;
                }
            }
            e.lanes[lane].cursor = c;
        }
    }

    fn judge_note(&mut self, i: usize, tail: bool, judge: usize, diff: i64, vanish: bool) {
        if vanish {
            if tail {
                self.notes[i].end_state = judge as u8 + 1;
            } else {
                self.notes[i].state = judge as u8 + 1;
            }
            self.score.passed += 1;
            self.ghost.push(judge as u8);
        }
        let n = &mut self.notes[i];
        let pt = if tail { &mut n.end_play_time } else { &mut n.play_time };
        if self.cfg.rules.miss_one && judge == POOR && pt.is_some() {
            return;
        }
        *pt = Some(diff);
        let lane = n.lane;
        let time = if tail { n.end_time } else { n.time };
        self.score.add(judge, diff >= 0);
        if judge < POOR {
            self.timing_log.push((time, diff));
        }
        let keep = self.cfg.rules.combo[judge];
        if keep && judge < MISS {
            self.combo += 1;
            self.score.max_combo = self.score.max_combo.max(self.combo);
        }
        if !keep {
            self.combo = 0;
        }
        self.gauge.update(judge, 1.0);
        self.events.push(EngineEvent::Judge { lane, judge, diff, combo: self.combo, tail });
        self.check_failed();
    }

    fn check_failed(&mut self) {
        if !self.failed && self.gauge.is_failed_now() {
            self.failed = true;
            self.events.push(EngineEvent::Failed);
        }
    }

    /// Fraction of lane key state, for key beams.
    pub fn key_down(&self, lane: usize) -> bool {
        self.lanes.get(lane).map(|l| l.keys.iter().any(|k| *k) || l.auto_hold).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bms::{parse_bms, ParseOptions};
    use crate::chart::JudgeRank;
    use crate::gauge::{GaugeAutoShift, GaugePreset, GaugeType};
    use crate::mode::RuleFamily;

    fn engine(src: &str, ln: LnType, auto: bool) -> (Chart, Engine) {
        let chart = parse_bms(src.as_bytes(), "bme", &ParseOptions::default()).unwrap();
        let rules = judge_rules(JudgePreset::Beatoraja, RuleFamily::SevenKeys, JudgeRank::Rank(3), None);
        let total = chart.total(ln);
        let gauge = GrooveGauge::new(GaugePreset::Beatoraja, RuleFamily::SevenKeys, GaugeType::Normal, GaugeAutoShift::None, total, chart.total_judgements(ln));
        let cfg = EngineConfig {
            rules,
            default_ln: ln,
            algorithm: JudgeAlgorithm::Combo,
            auto_lanes: vec![auto; 8],
            scratch_release_ends_bss: true,
        };
        let e = Engine::new(&chart, cfg, gauge);
        (chart, e)
    }

    fn press(e: &mut Engine, lane: u8, t: i64) {
        e.input(InputEvent { time: t, lane, key: 0, pressed: true });
        e.update(t);
    }
    fn release(e: &mut Engine, lane: u8, t: i64) {
        e.input(InputEvent { time: t, lane, key: 0, pressed: false });
        e.update(t);
    }

    fn judges(e: &mut Engine) -> Vec<usize> {
        e.drain_events()
            .filter_map(|ev| match ev {
                EngineEvent::Judge { judge, .. } => Some(judge),
                _ => None,
            })
            .collect()
    }

    // 120 BPM: measure 1 starts at 2 s; notes every beat on key 1
    const FOUR: &str = "#BPM 120\n#WAV01 a.wav\n#00111:01010101\n";

    #[test]
    fn perfect_and_timing() {
        let (_, mut e) = engine(FOUR, LnType::Ln, false);
        e.update(1_000_000);
        press(&mut e, 0, 2_000_000);
        release(&mut e, 0, 2_050_000);
        press(&mut e, 0, 2_500_000 + 40_000); // GREAT (late)
        press(&mut e, 0, 3_000_000 - 100_000); // GOOD (early)
        e.update(5_000_000); // last note missed
        assert_eq!(judges(&mut e), vec![PGREAT, GREAT, GOOD, POOR]);
        assert_eq!(e.score.ex_score(), 3);
        assert_eq!(e.combo(), 0);
        assert_eq!(e.score.max_combo, 3);
        assert!(e.is_finished(5_000_000));
        assert_eq!(e.score.count(1), 1);
        assert_eq!(e.score.counts[1][1], 1); // late
    }

    #[test]
    fn empty_poor_does_not_consume_note() {
        let (_, mut e) = engine(FOUR, LnType::Ln, false);
        e.update(1_000_000);
        press(&mut e, 0, 2_000_000 - 400_000); // way early -> empty poor
        release(&mut e, 0, 2_000_000 - 390_000);
        press(&mut e, 0, 2_000_000);
        assert_eq!(judges(&mut e), vec![MISS, PGREAT]);
        assert_eq!(e.combo(), 1); // 7K: empty poor keeps combo
    }

    #[test]
    fn autoplay_all_pgreat() {
        let (_, mut e) = engine(FOUR, LnType::Ln, true);
        let mut t = 0;
        while t < 4_000_000 {
            e.update(t);
            t += 16_667;
        }
        assert_eq!(judges(&mut e), vec![PGREAT; 4]);
        assert_eq!(e.score.ex_score(), 8);
    }

    // LN on key 1 from 2.0 s to 3.0 s
    const LN: &str = "#BPM 120\n#WAV01 a.wav\n#00151:0101\n";
    const HCN: &str = "#BPM 120\n#TOTAL 1\n#WAV01 a.wav\n#00151:0101\n";

    #[test]
    fn ln_mode_hold_and_early_release() {
        let (_, mut e) = engine(LN, LnType::Ln, false);
        e.update(1_000_000);
        press(&mut e, 0, 2_010_000);
        e.update(3_100_000);
        assert_eq!(judges(&mut e), vec![PGREAT]);
        assert_eq!(e.score.total_notes, 1);

        let (_, mut e) = engine(LN, LnType::Ln, false);
        e.update(1_000_000);
        press(&mut e, 0, 2_000_000);
        release(&mut e, 0, 2_500_000); // 500 ms early
        e.update(2_600_000);
        assert_eq!(judges(&mut e), vec![BAD]);
    }

    #[test]
    fn charge_note_head_and_tail() {
        let (_, mut e) = engine(LN, LnType::Cn, false);
        assert_eq!(e.score.total_notes, 2);
        e.update(1_000_000);
        press(&mut e, 0, 2_000_000);
        release(&mut e, 0, 3_150_000);
        assert_eq!(judges(&mut e), vec![PGREAT, GREAT]);

        // held too long -> tail POOR
        let (_, mut e) = engine(LN, LnType::Cn, false);
        e.update(1_000_000);
        press(&mut e, 0, 2_000_000);
        e.update(3_400_000);
        assert_eq!(judges(&mut e), vec![PGREAT, POOR]);

        // never pressed -> two POORs
        let (_, mut e) = engine(LN, LnType::Cn, false);
        e.update(4_000_000);
        assert_eq!(judges(&mut e), vec![POOR, POOR]);
    }

    #[test]
    fn hell_charge_gauge_ticks() {
        let (_, mut e) = engine(HCN, LnType::Hcn, false);
        e.update(1_000_000);
        press(&mut e, 0, 2_000_000);
        let before = e.gauge.value();
        let mut t = 2_000_000;
        while t < 2_900_000 {
            t += 16_667;
            e.update(t);
        }
        assert!(e.gauge.value() > before, "holding HCN must raise the gauge");
        release(&mut e, 0, 2_900_000);
        let mid = e.gauge.value();
        let _ = judges(&mut e);
        // let go of the body: gauge drains until the end
        let (_, mut e2) = engine(HCN, LnType::Hcn, false);
        e2.update(1_000_000);
        press(&mut e2, 0, 2_000_000);
        release(&mut e2, 0, 2_050_000);
        let start = e2.gauge.value();
        let mut t = 2_050_000;
        while t < 2_900_000 {
            t += 16_667;
            e2.update(t);
        }
        assert!(e2.gauge.value() < start);
        assert!(mid > 0.0);
    }

    #[test]
    fn mines_damage_when_held() {
        let (_, mut e) = engine("#BPM 120\n#WAV00 boom.wav\n#001D1:0A\n#00112:01\n", LnType::Ln, false);
        e.update(1_000_000);
        press(&mut e, 0, 1_900_000);
        let g = e.gauge.value();
        e.update(2_100_000);
        let mines: Vec<_> = e.drain_events().filter(|ev| matches!(ev, EngineEvent::Mine { .. })).collect();
        assert_eq!(mines.len(), 1);
        assert!(e.gauge.value() < g);
    }

    #[test]
    fn combo_algorithm_picks_next_note_for_dense_jacks() {
        // notes 50 ms apart (16ths at 300 BPM); pressing late for the first
        // note should not steal the second.
        let (_, mut e) = engine("#BPM 300\n#00111:0101010101010101\n", LnType::Ln, false);
        e.update(0);
        let first = 800_000;
        press(&mut e, 0, first + 10_000);
        release(&mut e, 0, first + 20_000);
        press(&mut e, 0, first + 100_000 + 5_000);
        assert_eq!(judges(&mut e), vec![PGREAT, PGREAT]);
    }
}
