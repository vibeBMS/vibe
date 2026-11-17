//! Key bindings as the settings' CONTROLS and the key config screen edit
//! them: which binding a row is, its label, and learning a key into it.

use crate::config::{KeyConfig, LaneKeys, SideKeys};
use crate::input::{InputEv, Phase};
use std::collections::HashMap;
use winit::keyboard::KeyCode;

/// Which binding a key-config row edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Key {
        side: usize,
        idx: usize,
    },
    ScratchUp(usize),
    ScratchDown(usize),
    ScratchAxis(usize),
    Pms(usize),
    Start,
    Select,
    /// The second player's START / SELECT (VS).
    Start2,
    Select2,
}

impl Slot {
    /// A short name (beatoraja's key config rows).
    pub fn label(self) -> String {
        match self {
            Slot::Key { side, idx } => format!("{}P-{} KEY", side + 1, idx + 1),
            Slot::ScratchUp(s) => format!("{}P-F-SCR", s + 1),
            Slot::ScratchDown(s) => format!("{}P-R-SCR", s + 1),
            Slot::ScratchAxis(s) => format!("{}P-SCR AXIS", s + 1),
            Slot::Pms(i) => format!("{} KEY", i + 1),
            Slot::Start => "START".into(),
            Slot::Select => "SELECT".into(),
            Slot::Start2 => "2P START".into(),
            Slot::Select2 => "2P SELECT".into(),
        }
    }

    /// A turntable binding (it takes an axis too).
    pub fn side_of_scratch(self) -> Option<usize> {
        match self {
            Slot::ScratchUp(s) | Slot::ScratchDown(s) | Slot::ScratchAxis(s) => Some(s),
            _ => None,
        }
    }
}

fn side(k: &KeyConfig, i: usize) -> &SideKeys {
    if i == 0 {
        &k.p1
    } else {
        &k.p2
    }
}

fn side_mut(k: &mut KeyConfig, i: usize) -> &mut SideKeys {
    if i == 0 {
        &mut k.p1
    } else {
        &mut k.p2
    }
}

pub fn slot_keys(k: &mut KeyConfig, s: Slot) -> Option<&mut LaneKeys> {
    Some(match s {
        Slot::Key { side: sd, idx } => side_mut(k, sd).keys.get_mut(idx)?,
        Slot::ScratchUp(sd) => &mut side_mut(k, sd).scratch_up,
        Slot::ScratchDown(sd) => &mut side_mut(k, sd).scratch_down,
        Slot::Pms(i) => k.pms.get_mut(i)?,
        Slot::Start => &mut k.start,
        Slot::Select => &mut k.select,
        Slot::Start2 => &mut k.p2_start,
        Slot::Select2 => &mut k.p2_select,
        Slot::ScratchAxis(_) => return None,
    })
}

pub fn slot_keys_ref(k: &KeyConfig, s: Slot) -> Option<&LaneKeys> {
    Some(match s {
        Slot::Key { side: sd, idx } => side(k, sd).keys.get(idx)?,
        Slot::ScratchUp(sd) => &side(k, sd).scratch_up,
        Slot::ScratchDown(sd) => &side(k, sd).scratch_down,
        Slot::Pms(i) => k.pms.get(i)?,
        Slot::Start => &k.start,
        Slot::Select => &k.select,
        Slot::Start2 => &k.p2_start,
        Slot::Select2 => &k.p2_select,
        Slot::ScratchAxis(_) => return None,
    })
}

/// A gamepad button or axis (`pad * 1_000_000 + code`).
pub fn pad_label(p: u64) -> String {
    format!("Pad{}#{}", p / 1_000_000, p % 1_000_000)
}

/// A key as people name it (`KeyZ` -> `Z`, `Digit1` -> `1`).
pub fn key_label(k: KeyCode) -> String {
    let s = format!("{k:?}");
    for p in ["Key", "Digit"] {
        if let Some(r) = s.strip_prefix(p).filter(|r| !r.is_empty()) {
            return r.to_string();
        }
    }
    s
}

/// Every binding of a row (keys first), `—` when none.
pub fn keys_label(lk: &LaneKeys) -> String {
    let mut v: Vec<String> = lk.keys.iter().map(|k| format!("{k:?}")).collect();
    v.extend(lk.pad.iter().map(|p| pad_label(*p)));
    if v.is_empty() {
        "—".into()
    } else {
        v.join(", ")
    }
}

/// The bindings of one kind of a row: 0 keyboard, 1 controller buttons,
/// 2 turntable axes (`None` for none).
pub fn device_label(k: &KeyConfig, s: Slot, device: usize) -> Option<String> {
    let v: Vec<String> = match (device, slot_keys_ref(k, s)) {
        (0, Some(lk)) => lk.keys.iter().map(|k| key_label(*k)).collect(),
        (1, Some(lk)) => lk.pad.iter().map(|p| pad_label(*p)).collect(),
        (2, _) => match s.side_of_scratch() {
            Some(sd) => side(k, sd)
                .scratch_axis
                .iter()
                .map(|a| pad_label(*a))
                .collect(),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    (!v.is_empty()).then(|| v.join(", "))
}

/// What beatoraja shows for a key: its keyboard key, else its controller
/// button, else `---`.
pub fn first_label(k: &KeyConfig, s: Slot) -> String {
    (0..3)
        .find_map(|d| device_label(k, s, d))
        .unwrap_or_else(|| "---".into())
}

/// Bind a pressed key, controller button or moved axis to a row: it
/// replaces the row's bindings of that kind. Whether anything was bound.
pub fn assign(
    k: &mut KeyConfig,
    s: Slot,
    key: Option<KeyCode>,
    pad: Option<u64>,
    axis: Option<u64>,
) -> bool {
    if let Some(a) = axis {
        return match s.side_of_scratch() {
            Some(sd) => {
                side_mut(k, sd).scratch_axis = vec![a];
                true
            }
            None => false,
        };
    }
    let Some(lk) = slot_keys(k, s) else {
        return false;
    };
    if let Some(c) = key {
        lk.keys = vec![c];
    }
    if let Some(p) = pad {
        lk.pad = vec![p];
    }
    key.is_some() || pad.is_some()
}

/// Remove a row's bindings: of one kind (0 keys, 1 buttons, 2 axes), or
/// all of them (`None`).
pub fn clear(k: &mut KeyConfig, s: Slot, device: Option<usize>) {
    if let Some(lk) = slot_keys(k, s) {
        match device {
            Some(0) => lk.keys.clear(),
            Some(1) => lk.pad.clear(),
            Some(_) => {}
            None => *lk = LaneKeys::default(),
        }
    }
    let axes = match device {
        Some(d) => d == 2,
        None => matches!(s, Slot::ScratchAxis(_)),
    };
    if axes {
        if let Some(sd) = s.side_of_scratch() {
            side_mut(k, sd).scratch_axis.clear();
        }
    }
}

/// Waiting for what to bind to a row: a key, a controller button, or an
/// axis turned far enough from where it was first seen.
#[derive(Debug, Default)]
pub struct Learn {
    pub slot: Option<Slot>,
    axes: HashMap<u64, f32>,
}

/// What an input does while a row waits ([`Learn::input`]).
#[derive(Debug, PartialEq)]
pub enum Learned {
    Waiting,
    /// Escape, or a touch: it stopped waiting.
    Cancel,
    /// Delete: clear the row.
    Clear,
    /// Bind this to the row (`assign`'s arguments).
    Bind(Option<KeyCode>, Option<u64>, Option<u64>),
}

impl Learn {
    pub fn start(&mut self, s: Slot) {
        self.slot = Some(s);
        self.axes.clear();
    }

    pub fn stop(&mut self) {
        self.slot = None;
    }

    pub fn on(&self) -> bool {
        self.slot.is_some()
    }

    pub fn input(&mut self, ev: &InputEv) -> Learned {
        match ev {
            InputEv::Key {
                code: KeyCode::Escape,
                pressed: true,
                ..
            }
            | InputEv::Pointer {
                phase: Phase::Down, ..
            } => {
                self.stop();
                Learned::Cancel
            }
            InputEv::Key {
                code: KeyCode::Delete,
                pressed: true,
                ..
            } => Learned::Clear,
            InputEv::Key {
                code,
                pressed: true,
                repeat: false,
                ..
            } => Learned::Bind(Some(*code), None, None),
            InputEv::Pad {
                id, pressed: true, ..
            } => Learned::Bind(None, Some(*id), None),
            InputEv::Axis { id, value, .. } => {
                let first = *self.axes.entry(*id).or_insert(*value);
                if (value - first).abs() > 0.3 {
                    self.axes.clear();
                    Learned::Bind(None, None, Some(*id))
                } else {
                    Learned::Waiting
                }
            }
            _ => Learned::Waiting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assign_and_clear() {
        let mut k = KeyConfig::default();
        let s = Slot::Key { side: 0, idx: 2 };
        assert_eq!(first_label(&k, s), "X");
        assert!(assign(&mut k, s, Some(KeyCode::KeyA), None, None));
        assert!(assign(&mut k, s, None, Some(3_000_007), None));
        assert_eq!(device_label(&k, s, 0).as_deref(), Some("A"));
        assert_eq!(device_label(&k, s, 1).as_deref(), Some("Pad3#7"));
        // an axis only binds to a turntable
        assert!(!assign(&mut k, s, None, None, Some(5)));
        assert!(assign(&mut k, Slot::ScratchUp(1), None, None, Some(5)));
        assert_eq!(k.p2.scratch_axis, vec![5]);
        clear(&mut k, s, Some(0));
        assert_eq!(first_label(&k, s), "Pad3#7");
        clear(&mut k, s, None);
        assert_eq!(first_label(&k, s), "---");
        clear(&mut k, Slot::ScratchUp(1), Some(2));
        assert!(k.p2.scratch_axis.is_empty());
        assert!(!k.p2.scratch_up.keys.is_empty());
    }

    #[test]
    fn learning() {
        use crate::platform::Instant;
        let mut l = Learn::default();
        l.start(Slot::Start);
        let axis = |value| InputEv::Axis {
            id: 4,
            value,
            time: Instant::now(),
        };
        // an axis binds once it has turned far enough from where it was
        assert_eq!(l.input(&axis(0.5)), Learned::Waiting);
        assert_eq!(l.input(&axis(0.7)), Learned::Waiting);
        assert_eq!(l.input(&axis(0.9)), Learned::Bind(None, None, Some(4)));
        let key = |code, repeat| InputEv::Key {
            code,
            pressed: true,
            repeat,
            time: Instant::now(),
            text: None,
        };
        assert_eq!(l.input(&key(KeyCode::KeyQ, true)), Learned::Waiting);
        assert_eq!(
            l.input(&key(KeyCode::KeyQ, false)),
            Learned::Bind(Some(KeyCode::KeyQ), None, None)
        );
        assert_eq!(l.input(&key(KeyCode::Delete, false)), Learned::Clear);
        assert!(l.on());
        assert_eq!(l.input(&key(KeyCode::Escape, false)), Learned::Cancel);
        assert!(!l.on());
    }
}
