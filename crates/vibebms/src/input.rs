//! Device input -> lanes.

use crate::config::{KeyConfig, LaneKeys, SideKeys};
use std::collections::HashMap;
use std::time::{Duration, Instant};
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
}

/// A lane key transition produced by any device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneEv {
    pub lane: u8,
    pub sub: u8,
    pub pressed: bool,
    pub time: Instant,
}

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
    pub mode: PlayMode,
}

const AXIS_RELEASE: Duration = Duration::from_millis(100);

impl LaneMapper {
    pub fn new(cfg: &KeyConfig, mode: PlayMode) -> LaneMapper {
        let mut m = LaneMapper {
            keys: HashMap::new(),
            pads: HashMap::new(),
            axes: HashMap::new(),
            any_axis: Vec::new(),
            axis: HashMap::new(),
            mode,
        };
        let add = |lk: &LaneKeys, lane: usize, sub: u8, m: &mut LaneMapper| {
            for k in &lk.keys {
                m.keys.entry(*k).or_insert((lane as u8, sub));
            }
            for p in &lk.pad {
                m.pads.entry(*p).or_insert((lane as u8, sub));
            }
        };
        if mode == PlayMode::Popn9K {
            for (i, lk) in cfg.pms.iter().take(9).enumerate() {
                add(lk, i, 0, &mut m);
            }
            // also allow the 7K layout + scratch keys on PMS
            for (i, lk) in cfg.p1.keys.iter().take(7).enumerate() {
                add(lk, i, 0, &mut m);
            }
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
        m
    }

    pub fn key(&self, code: KeyCode) -> Option<(u8, u8)> {
        self.keys.get(&code).copied()
    }

    pub fn pad(&self, id: u64) -> Option<(u8, u8)> {
        self.pads.get(&id).copied()
    }

    /// Turntable emulation from an analog axis: movement in one direction
    /// holds sub-key 0/1, reversing switches, stopping releases.
    pub fn axis(&mut self, id: u64, value: f32, time: Instant, out: &mut Vec<LaneEv>) {
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
        if d.abs() < 0.004 {
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
            if st.dir != 0 && now.duration_since(st.last_move) > AXIS_RELEASE {
                out.push(LaneEv {
                    lane: st.lane,
                    sub: if st.dir > 0 { 0 } else { 1 },
                    pressed: false,
                    time: st.last_move + AXIS_RELEASE,
                });
                st.dir = 0;
            }
        }
    }
}

/// Menu navigation derived from keys and the 7K bindings (so a controller
/// can drive the whole UI like in beatoraja).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Decide,
    Back,
    Settings,
    Option,
}

pub fn nav_from_key(code: KeyCode, cfg: &KeyConfig) -> Option<Nav> {
    use KeyCode::*;
    match code {
        ArrowUp => return Some(Nav::Up),
        ArrowDown => return Some(Nav::Down),
        ArrowLeft => return Some(Nav::Left),
        ArrowRight => return Some(Nav::Right),
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
    if in_lk(&p.keys[1]) {
        return Some(Nav::Back);
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

pub fn nav_from_pad(id: u64, cfg: &KeyConfig) -> Option<Nav> {
    let in_lk = |lk: &LaneKeys| lk.pad.contains(&id);
    let p = &cfg.p1;
    if in_lk(&p.keys[0]) || in_lk(&p.keys[2]) || in_lk(&p.keys[4]) || in_lk(&p.keys[6]) {
        return Some(Nav::Decide);
    }
    if in_lk(&p.keys[1]) {
        return Some(Nav::Back);
    }
    if in_lk(&p.keys[3]) {
        return Some(Nav::Settings);
    }
    if in_lk(&cfg.start) {
        return Some(Nav::Option);
    }
    if in_lk(&cfg.select) {
        return Some(Nav::Settings);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
