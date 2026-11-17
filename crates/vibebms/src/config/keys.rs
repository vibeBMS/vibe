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
    /// The analog turntable and the chatter filter (written once changed).
    #[serde(skip_serializing_if = "InputTuning::is_default")]
    pub tuning: InputTuning,
}

/// How input from devices is taken.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InputTuning {
    /// An analog turntable's movement between two reports smaller than
    /// this (thousandths of the axis, which goes from −1 to 1) is not a
    /// turn: higher is less sensitive.
    pub turntable_dead_band: u32,
    /// An analog turntable that has not moved for this long (ms) is let go.
    pub turntable_stop_ms: u32,
    /// A key or button press sooner than this (ms) after the last press
    /// of its lane let through is chatter, dropped with its release (0:
    /// off). Keyboard and controller buttons only.
    pub chatter_ms: u32,
}

impl Default for InputTuning {
    fn default() -> Self {
        InputTuning {
            turntable_dead_band: 4,
            turntable_stop_ms: 100,
            chatter_ms: 0,
        }
    }
}

impl InputTuning {
    pub fn is_default(&self) -> bool {
        *self == InputTuning::default()
    }
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
            tuning: InputTuning::default(),
        }
    }
}
