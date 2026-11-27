//! Device input -> lanes.

pub mod gamepad;

use crate::config::{KeyConfig, LaneKeys, SideKeys};
use crate::platform::{Duration, Instant};
use std::collections::HashMap;
use vibebms_core::PlayMode;
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

#[derive(Clone, Debug)]
pub enum InputEv {
    Key {
        code: KeyCode,
        pressed: bool,
        repeat: bool,
        time: Instant,
        text: Option<String>,
    },
    /// Gamepad button (`pad * 1_000_000 + code`).
    Pad {
        id: u64,
        pressed: bool,
        time: Instant,
    },
    Axis {
        id: u64,
        value: f32,
        time: Instant,
    },
    Pointer {
        id: u64,
        phase: Phase,
        x: f32,
        y: f32,
        time: Instant,
        touch: bool,
    },
    Wheel {
        dy: f32,
    },
    Dropped(std::path::PathBuf),
    /// Text typed without a key of its own: an on-screen keyboard (iOS
    /// sends its characters with no key code) or an input method's
    /// composed text. A "\n" is its return key.
    Text(String),
    /// Keys may have been let go unseen: the window lost the focus (all
    /// keys), or no finger is on the screen any more (`touch_only`).
    Released {
        time: Instant,
        touch_only: bool,
    },
}

/// A lane key transition produced by any device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneEv {
    pub lane: u8,
    pub sub: u8,
    pub pressed: bool,
    pub time: Instant,
}

/// Pseudo lanes of the START and SELECT buttons (the hi-speed panel), and
/// the second player's in VS.
pub const START: u8 = 254;
pub const SELECT: u8 = 255;
pub const START2: u8 = 252;
pub const SELECT2: u8 = 253;
/// The first pseudo lane: lanes from here on are buttons, not keys.
pub const SYSTEM: u8 = START2;

struct AxisState {
    last: Option<f32>,
    dir: i8,
    last_move: Instant,
    lane: u8,
}

pub struct LaneMapper {
    keys: HashMap<KeyCode, (u8, u8)>,
    pads: HashMap<u64, (u8, u8)>,
    axes: HashMap<u64, u8>,
    any_axis: Vec<u8>,
    axis: HashMap<u64, AxisState>,
    /// An axis movement smaller than this is not a turn.
    dead_band: f32,
    /// A turntable that stops this long is let go.
    stop: Duration,
    /// The chatter filter (`InputTuning::chatter_ms`; zero: off), the
    /// last press of each lane key it let through and the releases still
    /// to drop.
    chatter: Duration,
    last_press: HashMap<(u8, u8), Instant>,
    dropped: HashMap<(u8, u8), u32>,
    pub mode: PlayMode,
    /// The first two controllers swapped (VS: the 1P controller is the one
    /// connected second).
    pub swap_pads: bool,
}

/// A controller code with the first two controllers swapped.
fn swapped(id: u64) -> u64 {
    match id / 1_000_000 {
        0 => id + 1_000_000,
        1 => id - 1_000_000,
        _ => id,
    }
}

impl LaneMapper {
    pub fn new(cfg: &KeyConfig, mode: PlayMode) -> LaneMapper {
        let mut m = LaneMapper {
            keys: HashMap::new(),
            pads: HashMap::new(),
            axes: HashMap::new(),
            any_axis: Vec::new(),
            axis: HashMap::new(),
            dead_band: cfg.tuning.turntable_dead_band as f32 / 1000.0,
            stop: Duration::from_millis(cfg.tuning.turntable_stop_ms as u64),
            chatter: Duration::from_millis(cfg.tuning.chatter_ms as u64),
            last_press: HashMap::new(),
            dropped: HashMap::new(),
            mode,
            swap_pads: false,
        };
        let add = |lk: &LaneKeys, lane: usize, sub: u8, m: &mut LaneMapper| {
            for k in &lk.keys {
                m.keys.entry(*k).or_insert((lane as u8, sub));
            }
            for p in &lk.pad {
                m.pads.entry(*p).or_insert((lane as u8, sub));
            }
        };
        // START / SELECT: the hi-speed panel (pseudo lanes; a key bound to
        // a lane too stays the lane's)
        let controls = |m: &mut LaneMapper| {
            add(&cfg.start, START as usize, 0, m);
            add(&cfg.select, SELECT as usize, 0, m);
            add(&cfg.p2_start, START2 as usize, 0, m);
            add(&cfg.p2_select, SELECT2 as usize, 0, m);
        };
        if let Some(own) = cfg.few(mode) {
            for (i, lk) in own.iter().take(mode.lane_count()).enumerate() {
                add(lk, i, 0, &mut m);
            }
            // controllers: their 7 key and turntable bindings, at the lanes
            // where the chart has them in 7 key channels (8K's first key is
            // the turntable)
            let seven = mode.seven_key_lanes().unwrap_or_default();
            let pads = |lk: &LaneKeys| LaneKeys {
                keys: Vec::new(),
                pad: lk.pad.clone(),
            };
            for side in [&cfg.p1, &cfg.p2] {
                for (lane, s) in seven.iter().enumerate() {
                    match side.keys.get(*s as usize) {
                        Some(lk) if *s < 7 => add(&pads(lk), lane, 0, &mut m),
                        _ => {
                            add(&pads(&side.scratch_up), lane, 0, &mut m);
                            add(&pads(&side.scratch_down), lane, 1, &mut m);
                            for a in &side.scratch_axis {
                                m.axes.insert(*a, lane as u8);
                            }
                            if cfg.any_axis_is_scratch {
                                m.any_axis = vec![lane as u8];
                            }
                        }
                    }
                }
            }
            controls(&mut m);
            return m;
        }
        if mode == PlayMode::Popn9K {
            for (i, lk) in cfg.pms.iter().take(9).enumerate() {
                add(lk, i, 0, &mut m);
            }
            // also allow the 7K layout + scratch keys on PMS
            for (i, lk) in cfg.p1.keys.iter().take(7).enumerate() {
                add(lk, i, 0, &mut m);
            }
            controls(&mut m);
            return m;
        }
        let k = mode.keys_per_player();
        let lpp = mode.lanes_per_player();
        let sides: [&SideKeys; 2] = [&cfg.p1, &cfg.p2];
        for (si, side) in sides.iter().enumerate() {
            // single play: both sides' bindings drive the one player
            let player = if mode.is_double() { si } else { 0 };
            let base = player * lpp;
            for (i, lk) in side.keys.iter().take(k).enumerate() {
                add(lk, base + i, 0, &mut m);
            }
            add(&side.scratch_up, base + k, 0, &mut m);
            add(&side.scratch_down, base + k, 1, &mut m);
            for a in &side.scratch_axis {
                m.axes.insert(*a, (base + k) as u8);
            }
        }
        if cfg.any_axis_is_scratch {
            m.any_axis = (0..mode.players()).map(|p| (p * lpp + k) as u8).collect();
        }
        controls(&mut m);
        m
    }

    pub fn key(&self, code: KeyCode) -> Option<(u8, u8)> {
        self.keys.get(&code).copied()
    }

    pub fn pad(&self, id: u64) -> Option<(u8, u8)> {
        let id = if self.swap_pads { swapped(id) } else { id };
        self.pads.get(&id).copied()
    }

    /// Turntable emulation from an analog axis: movement in one direction
    /// holds sub-key 0/1, reversing switches, stopping releases.
    pub fn axis(&mut self, id: u64, value: f32, time: Instant, out: &mut Vec<LaneEv>) {
        let id = if self.swap_pads { swapped(id) } else { id };
        let lane = match self.axes.get(&id) {
            Some(l) => *l,
            None => {
                if self.any_axis.is_empty() {
                    return;
                }
                let pad = (id / 1_000_000) as usize;
                self.any_axis[pad.min(self.any_axis.len() - 1)]
            }
        };
        let st = self.axis.entry(id).or_insert(AxisState {
            last: None,
            dir: 0,
            last_move: time,
            lane,
        });
        let Some(last) = st.last.replace(value) else {
            return;
        };
        let mut d = value - last;
        if d > 1.0 {
            d -= 2.0;
        } else if d < -1.0 {
            d += 2.0;
        }
        if d.abs() < self.dead_band {
            return;
        }
        let dir = if d > 0.0 { 1 } else { -1 };
        st.last_move = time;
        if dir != st.dir {
            if st.dir != 0 {
                out.push(LaneEv {
                    lane: st.lane,
                    sub: if st.dir > 0 { 0 } else { 1 },
                    pressed: false,
                    time,
                });
            }
            out.push(LaneEv {
                lane: st.lane,
                sub: if dir > 0 { 0 } else { 1 },
                pressed: true,
                time,
            });
            st.dir = dir;
        }
    }

    /// Release turntables that stopped moving.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<LaneEv>) {
        for st in self.axis.values_mut() {
            if st.dir != 0 && now.duration_since(st.last_move) > self.stop {
                out.push(LaneEv {
                    lane: st.lane,
                    sub: if st.dir > 0 { 0 } else { 1 },
                    pressed: false,
                    time: st.last_move + self.stop,
                });
                st.dir = 0;
            }
        }
    }

    /// The chatter filter on a key's or button's event: `false` for a
    /// press that came sooner than the minimum after the last press of
    /// its lane key let through (by the events' own times), and for the
    /// release that ends such a press, so presses and releases still pair.
    pub fn passes(&mut self, e: &LaneEv) -> bool {
        if self.chatter.is_zero() {
            return true;
        }
        let k = (e.lane, e.sub);
        if !e.pressed {
            return match self.dropped.get_mut(&k) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    false
                }
                _ => true,
            };
        }
        let soon = self
            .last_press
            .get(&k)
            .is_some_and(|t| e.time.saturating_duration_since(*t) < self.chatter);
        if soon {
            *self.dropped.entry(k).or_default() += 1;
        } else {
            self.last_press.insert(k, e.time);
        }
        !soon
    }

    /// Everything was let go (the window lost the focus): no release is
    /// still to drop.
    pub fn released_all(&mut self) {
        self.dropped.clear();
    }
}

/// Menu navigation derived from keys and the 7K bindings (so a controller
/// can drive the whole UI like in beatoraja).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Decide,
    Back,
    /// Close the folder (LR2: the blue keys, ←): unlike `Back`, nothing at
    /// the top.
    Close,
    Settings,
    Option,
}

pub fn nav_from_key(code: KeyCode, cfg: &KeyConfig) -> Option<Nav> {
    use KeyCode::*;
    match code {
        ArrowUp => return Some(Nav::Up),
        ArrowDown => return Some(Nav::Down),
        // LR2: ← closes the folder, → is Enter
        ArrowLeft => return Some(Nav::Close),
        ArrowRight => return Some(Nav::Decide),
        Enter | NumpadEnter => return Some(Nav::Decide),
        Escape | Backspace => return Some(Nav::Back),
        F1 | Tab => return Some(Nav::Settings),
        _ => {}
    }
    let in_lk = |lk: &LaneKeys| lk.keys.contains(&code);
    let p = &cfg.p1;
    if in_lk(&p.keys[0]) || in_lk(&p.keys[2]) || in_lk(&p.keys[4]) || in_lk(&p.keys[6]) {
        return Some(Nav::Decide);
    }
    // LR2: the white keys decide, the blue ones close the folder
    if in_lk(&p.keys[1]) || in_lk(&p.keys[3]) || in_lk(&p.keys[5]) {
        return Some(Nav::Close);
    }
    if in_lk(&p.scratch_up) {
        return Some(Nav::Up);
    }
    if in_lk(&p.scratch_down) {
        return Some(Nav::Down);
    }
    if in_lk(&cfg.start) {
        return Some(Nav::Option);
    }
    if in_lk(&cfg.select) {
        return Some(Nav::Settings);
    }
    None
}

/// As [`nav_from_pad`], with buttons bound to the turntable moving up /
/// down (the screens whose rows a controller walks).
pub fn nav_from_pad_rows(id: u64, cfg: &KeyConfig) -> Option<Nav> {
    let p = &cfg.p1;
    if p.scratch_up.pad.contains(&id) {
        return Some(Nav::Up);
    }
    if p.scratch_down.pad.contains(&id) {
        return Some(Nav::Down);
    }
    nav_from_pad(id, cfg)
}

pub fn nav_from_pad(id: u64, cfg: &KeyConfig) -> Option<Nav> {
    let in_lk = |lk: &LaneKeys| lk.pad.contains(&id);
    let p = &cfg.p1;
    if in_lk(&p.keys[0]) || in_lk(&p.keys[2]) || in_lk(&p.keys[4]) || in_lk(&p.keys[6]) {
        return Some(Nav::Decide);
    }
    if in_lk(&p.keys[1]) || in_lk(&p.keys[3]) || in_lk(&p.keys[5]) {
        return Some(Nav::Close);
    }
    if in_lk(&cfg.start) {
        return Some(Nav::Option);
    }
    if in_lk(&cfg.select) {
        return Some(Nav::Settings);
    }
    None
}

impl InputEv {
    /// The menu move this event makes: a key pressed (its repeats too) or
    /// a controller button pressed, by [`nav_from_key`] / [`nav_from_pad`];
    /// with `axis`, a turntable turned by a step is `Up` / `Down`.
    pub fn nav(&self, keys: &KeyConfig, axis: Option<&mut AxisStepper>) -> Option<Nav> {
        self.nav_by(keys, nav_from_pad, axis)
    }

    /// [`InputEv::nav`], with the buttons bound to the turntable moving up
    /// and down ([`nav_from_pad_rows`]: the screens a controller walks).
    pub fn nav_rows(&self, keys: &KeyConfig, axis: Option<&mut AxisStepper>) -> Option<Nav> {
        self.nav_by(keys, nav_from_pad_rows, axis)
    }

    fn nav_by(
        &self,
        keys: &KeyConfig,
        pad: fn(u64, &KeyConfig) -> Option<Nav>,
        axis: Option<&mut AxisStepper>,
    ) -> Option<Nav> {
        match self {
            InputEv::Key {
                code,
                pressed: true,
                ..
            } => nav_from_key(*code, keys),
            InputEv::Pad {
                id, pressed: true, ..
            } => pad(*id, keys),
            InputEv::Axis { id, value, .. } => match axis?.step(*id, *value) {
                0 => None,
                d if d > 0 => Some(Nav::Down),
                _ => Some(Nav::Up),
            },
            _ => None,
        }
    }
}

/// A turntable axis as list steps: one per `AXIS_STEP` turned (the axis
/// wraps around at ±1).
#[derive(Debug, Default)]
pub struct AxisStepper {
    last: HashMap<u64, f32>,
}

pub const AXIS_STEP: f32 = 0.06;

impl AxisStepper {
    /// Axis `id` is at `value` now: -1, 0 or 1 steps.
    pub fn step(&mut self, id: u64, value: f32) -> i32 {
        let last = self.last.entry(id).or_insert(value);
        let mut d = value - *last;
        if d > 1.0 {
            d -= 2.0
        } else if d < -1.0 {
            d += 2.0
        }
        if d.abs() < AXIS_STEP {
            return 0;
        }
        *last = value;
        if d > 0.0 {
            1
        } else {
            -1
        }
    }
}

/// A held key that acts again and again: first after a delay, then each
/// time `due` is asked past the interval it gives.
#[derive(Clone, Copy, Debug)]
pub struct Repeat<K> {
    pub key: K,
    since: Instant,
    next: Instant,
    /// Repeats so far.
    pub count: u32,
}

impl<K> Repeat<K> {
    pub fn new(key: K, now: Instant, delay: Duration) -> Repeat<K> {
        Repeat {
            key,
            since: now,
            next: now + delay,
            count: 0,
        }
    }

    /// How long the key has been held at `now`.
    pub fn held(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.since)
    }

    /// Whether it acts again at `now`; the next time `every` later.
    pub fn due(&mut self, now: Instant, every: Duration) -> bool {
        if now < self.next {
            return false;
        }
        self.next = now + every;
        self.count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `InputEv::nav` makes of each event what the screens made of it
    /// before it: a pressed key (repeats too) through `nav_from_key`, a
    /// pressed button through `nav_from_pad` (`_rows`), an axis through an
    /// `AxisStepper` (a step up `Up`); nothing of releases, of the other
    /// events, or of an axis without a stepper.
    #[test]
    fn nav_is_what_the_screens_made_of_an_event() {
        let mut cfg = KeyConfig::default();
        // buttons on every lane, the turntable, START and SELECT
        let mut id = 1_000_000;
        let mut bind = |l: &mut LaneKeys| {
            id += 1;
            l.pad.push(id);
        };
        cfg.p1.keys.iter_mut().for_each(&mut bind);
        bind(&mut cfg.p1.scratch_up);
        bind(&mut cfg.p1.scratch_down);
        bind(&mut cfg.start);
        bind(&mut cfg.select);
        let t = Instant::now();
        let mut codes: Vec<KeyCode> = (cfg.p1.keys.iter().chain([
            &cfg.p1.scratch_up,
            &cfg.p1.scratch_down,
            &cfg.start,
            &cfg.select,
        ]))
        .flat_map(|l| l.keys.clone())
        .collect();
        codes.extend([
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::Enter,
            KeyCode::NumpadEnter,
            KeyCode::Escape,
            KeyCode::Backspace,
            KeyCode::F1,
            KeyCode::Tab,
            KeyCode::KeyQ,
            KeyCode::F12,
        ]);
        for code in codes {
            for (pressed, repeat) in [(true, false), (true, true), (false, false)] {
                let ev = InputEv::Key {
                    code,
                    pressed,
                    repeat,
                    time: t,
                    text: None,
                };
                let want = nav_from_key(code, &cfg).filter(|_| pressed);
                assert_eq!(ev.nav(&cfg, None), want, "{code:?}");
                assert_eq!(ev.nav_rows(&cfg, None), want, "{code:?}");
            }
        }
        for id in 1_000_000..=id + 1 {
            for pressed in [true, false] {
                let ev = InputEv::Pad {
                    id,
                    pressed,
                    time: t,
                };
                let plain = nav_from_pad(id, &cfg).filter(|_| pressed);
                let rows = nav_from_pad_rows(id, &cfg).filter(|_| pressed);
                assert_eq!(ev.nav(&cfg, None), plain, "{id}");
                assert_eq!(ev.nav_rows(&cfg, None), rows, "{id}");
            }
        }
        // the turntable: the stepper's steps, the same stepper state
        let (mut a, mut b) = (AxisStepper::default(), AxisStepper::default());
        for v in [0.0, 0.03, 0.07, 0.2, 0.19, -0.5, 0.99, -0.98, -0.9] {
            let ev = InputEv::Axis {
                id: 3,
                value: v,
                time: t,
            };
            let want = match b.step(3, v) {
                0 => None,
                d if d > 0 => Some(Nav::Down),
                _ => Some(Nav::Up),
            };
            assert_eq!(ev.nav(&cfg, Some(&mut a)), want, "{v}");
            assert_eq!(ev.nav(&cfg, None), None);
        }
        assert_eq!(InputEv::Wheel { dy: 1.0 }.nav(&cfg, Some(&mut a)), None);
    }

    #[test]
    fn default_bindings() {
        let cfg = KeyConfig::default();
        let m = LaneMapper::new(&cfg, PlayMode::Beat7K);
        assert_eq!(m.key(KeyCode::KeyZ), Some((0, 0)));
        assert_eq!(m.key(KeyCode::ShiftLeft), Some((7, 0)));
        assert_eq!(m.key(KeyCode::ControlLeft), Some((7, 1)));
        // 2P keys also drive SP
        assert_eq!(m.key(KeyCode::KeyM), Some((0, 0)));
        let d = LaneMapper::new(&cfg, PlayMode::Beat14K);
        assert_eq!(d.key(KeyCode::KeyM), Some((8, 0)));
        assert_eq!(d.key(KeyCode::ShiftRight), Some((15, 0)));
        let f = LaneMapper::new(&cfg, PlayMode::Beat5K);
        assert_eq!(f.key(KeyCode::ShiftLeft), Some((5, 0)));
        assert_eq!(f.key(KeyCode::KeyF), None);
    }

    #[test]
    fn four_six_eight_keys() {
        let mut cfg = KeyConfig::default();
        let m = LaneMapper::new(&cfg, PlayMode::Key4);
        let keys = [KeyCode::KeyD, KeyCode::KeyF, KeyCode::KeyJ, KeyCode::KeyK];
        for (i, k) in keys.into_iter().enumerate() {
            assert_eq!(m.key(k), Some((i as u8, 0)));
        }
        // the 7 key keyboard layout is not theirs (its keys would clash)
        assert_eq!(m.key(KeyCode::KeyZ), None);
        assert_eq!(m.key(KeyCode::ShiftLeft), None);
        let six = LaneMapper::new(&cfg, PlayMode::Key6);
        assert_eq!(six.key(KeyCode::KeyS), Some((0, 0)));
        assert_eq!(six.key(KeyCode::KeyL), Some((5, 0)));
        let eight = LaneMapper::new(&cfg, PlayMode::Key8);
        assert_eq!(eight.key(KeyCode::KeyA), Some((0, 0)));
        assert_eq!(eight.key(KeyCode::Semicolon), Some((7, 0)));
        // a controller's 7 key buttons and turntable play them where the
        // chart's channels are: 4K keys 1 2 4 5, 8K the turntable first
        for (i, k) in cfg.p1.keys.iter_mut().enumerate() {
            k.pad = vec![i as u64];
        }
        cfg.p1.scratch_up.pad = vec![50];
        cfg.p1.scratch_down.pad = vec![51];
        cfg.p1.scratch_axis = vec![60];
        let m = LaneMapper::new(&cfg, PlayMode::Key4);
        let lanes: Vec<_> = (0..7).map(|b| m.pad(b).map(|l| l.0)).collect();
        assert_eq!(
            lanes,
            [Some(0), Some(1), None, Some(2), Some(3), None, None]
        );
        assert_eq!(m.pad(50), None);
        let eight = LaneMapper::new(&cfg, PlayMode::Key8);
        assert_eq!(eight.pad(50), Some((0, 0)));
        assert_eq!(eight.pad(51), Some((0, 1)));
        assert_eq!(eight.pad(0), Some((1, 0)));
        assert_eq!(eight.pad(6), Some((7, 0)));
        assert_eq!(eight.axes.get(&60), Some(&0));
    }

    #[test]
    fn turntable_axis() {
        let mut m = LaneMapper::new(&KeyConfig::default(), PlayMode::Beat7K);
        let t = Instant::now();
        let mut out = Vec::new();
        m.axis(5, 0.0, t, &mut out);
        m.axis(5, 0.05, t, &mut out);
        assert_eq!(
            out,
            vec![LaneEv {
                lane: 7,
                sub: 0,
                pressed: true,
                time: t
            }]
        );
        out.clear();
        m.axis(5, 0.01, t, &mut out);
        assert_eq!(out.len(), 2); // release up, press down
        out.clear();
        m.tick(t + Duration::from_millis(200), &mut out);
        assert_eq!(out.len(), 1);
        assert!(!out[0].pressed);
    }

    /// The turntable's dead band and stop time are settings.
    #[test]
    fn turntable_tuning() {
        let mut cfg = KeyConfig::default();
        cfg.tuning.turntable_dead_band = 20;
        cfg.tuning.turntable_stop_ms = 300;
        let mut m = LaneMapper::new(&cfg, PlayMode::Beat7K);
        let t = Instant::now();
        let mut out = Vec::new();
        m.axis(5, 0.0, t, &mut out);
        m.axis(5, 0.015, t, &mut out);
        assert!(out.is_empty());
        m.axis(5, 0.04, t, &mut out);
        assert_eq!(out.len(), 1);
        out.clear();
        m.tick(t + Duration::from_millis(200), &mut out);
        assert!(out.is_empty());
        m.tick(t + Duration::from_millis(301), &mut out);
        assert_eq!(out[0].time, t + Duration::from_millis(300));
    }

    /// A press sooner than the chatter time after the last press of its
    /// lane key let through is dropped with its release (by the events'
    /// times, whenever they arrive); other lane keys are apart.
    #[test]
    fn chatter_filter() {
        let mut cfg = KeyConfig::default();
        let t = Instant::now();
        let ev = |lane, pressed, ms| LaneEv {
            lane,
            sub: 0,
            pressed,
            time: t + Duration::from_millis(ms),
        };
        let mut off = LaneMapper::new(&cfg, PlayMode::Beat7K);
        assert!(off.passes(&ev(0, true, 0)) && off.passes(&ev(0, true, 1)));
        cfg.tuning.chatter_ms = 10;
        let mut m = LaneMapper::new(&cfg, PlayMode::Beat7K);
        let seq = [
            (0, true, 0),
            (0, false, 3),
            // a bounce: dropped, and its release
            (0, true, 6),
            (1, true, 7),
            (0, false, 8),
            // 10 ms after the last press let through: a press
            (0, true, 10),
            (0, false, 60),
        ];
        let passed: Vec<bool> = seq
            .iter()
            .map(|&(l, p, ms)| m.passes(&ev(l, p, ms)))
            .collect();
        assert_eq!(passed, [true, true, false, true, false, true, true]);
        // a dropped press whose release never came (the focus left)
        assert!(m.passes(&ev(2, true, 1000)));
        assert!(!m.passes(&ev(2, true, 1001)));
        m.released_all();
        assert!(m.passes(&ev(2, false, 1100)));
    }

    #[test]
    fn repeats() {
        let t = Instant::now();
        let ms = Duration::from_millis;
        let mut r = Repeat::new(1, t, ms(300));
        assert!(!r.due(t + ms(299), ms(50)));
        assert!(r.due(t + ms(300), ms(50)));
        assert!(!r.due(t + ms(340), ms(50)));
        assert!(r.due(t + ms(350), ms(50)));
        assert_eq!(r.count, 2);
        assert_eq!(r.held(t + ms(400)), ms(400));
    }

    #[test]
    fn turntable_steps() {
        let mut a = AxisStepper::default();
        // the first value seen is where it starts
        assert_eq!(a.step(1, 0.5), 0);
        assert_eq!(a.step(1, 0.53), 0);
        assert_eq!(a.step(1, 0.57), 1);
        assert_eq!(a.step(1, 0.5), -1);
        // across the wrap at ±1: the short way
        assert_eq!(a.step(1, 0.98), 1);
        assert_eq!(a.step(1, -0.9), 1);
        assert_eq!(a.step(1, 0.98), -1);
        // each axis on its own
        assert_eq!(a.step(2, 0.0), 0);
        assert_eq!(a.step(2, -0.1), -1);
    }
}
