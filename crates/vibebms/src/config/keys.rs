//! Key and controller bindings.

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

/// One lane binding: up to a few keyboard keys and gamepad buttons.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LaneKeys {
    pub keys: Vec<KeyCode>,
    /// gilrs raw button codes, prefixed with the pad index: `pad * 1_000_000 + code`.
    pub pad: Vec<u64>,
}

impl LaneKeys {
    fn k(keys: &[KeyCode]) -> Self {
        LaneKeys {
            keys: keys.to_vec(),
            pad: Vec::new(),
        }
    }
}

/// Bindings for one player side: 9 key slots (7K uses 0..7, PMS uses 0..9)
/// plus two scratch directions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SideKeys {
    pub keys: Vec<LaneKeys>,
    pub scratch_up: LaneKeys,
    pub scratch_down: LaneKeys,
    /// gilrs axis codes used as turntable (`pad * 1_000_000 + code`).
    pub scratch_axis: Vec<u64>,
}

impl Default for SideKeys {
    fn default() -> Self {
        SideKeys {
            keys: vec![LaneKeys::default(); 9],
            scratch_up: LaneKeys::default(),
            scratch_down: LaneKeys::default(),
            scratch_axis: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct KeyConfig {
    pub p1: SideKeys,
    pub p2: SideKeys,
    /// pop'n 9 button layout.
    pub pms: Vec<LaneKeys>,
    pub start: LaneKeys,
    pub select: LaneKeys,
    /// The second player's START / SELECT (VS).
    pub p2_start: LaneKeys,
    pub p2_select: LaneKeys,
    /// Any gamepad axis movement counts as scratch when no axis is bound.
    pub any_axis_is_scratch: bool,
}

impl Default for KeyConfig {
    fn default() -> Self {
        use KeyCode::*;
        let side = |keys: [KeyCode; 7], up: KeyCode, down: KeyCode| {
            let mut s = SideKeys::default();
            for (i, k) in keys.iter().enumerate() {
                s.keys[i] = LaneKeys::k(&[*k]);
            }
            s.scratch_up = LaneKeys::k(&[up]);
            s.scratch_down = LaneKeys::k(&[down]);
            s
        };
        KeyConfig {
            p1: side(
                [KeyZ, KeyS, KeyX, KeyD, KeyC, KeyF, KeyV],
                ShiftLeft,
                ControlLeft,
            ),
            p2: side(
                [KeyM, KeyK, Comma, KeyL, Period, Semicolon, Slash],
                ShiftRight,
                ControlRight,
            ),
            pms: [KeyA, KeyS, KeyD, KeyF, KeyG, KeyH, KeyJ, KeyK, KeyL]
                .iter()
                .map(|k| LaneKeys::k(&[*k]))
                .collect(),
            start: LaneKeys::k(&[KeyQ]),
            select: LaneKeys::k(&[KeyW]),
            p2_start: LaneKeys::k(&[KeyP]),
            p2_select: LaneKeys::k(&[KeyO]),
            any_axis_is_scratch: true,
        }
    }
}
