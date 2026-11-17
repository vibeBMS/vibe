//! Touch controls and the touch perspective play skin.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TouchMode {
    /// On for phones/tablets, and on desktop as soon as a touch is seen.
    #[default]
    Auto,
    On,
    Off,
}

vibebms_core::choices! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum TouchStyle {
        /// The BM preset: turntable + 7 buttons in two rows like a beatmania
        /// controller.
        #[default]
        Iidx => "BM",
        /// A row of equally sized keys (keyboard-like).
        Keyboard => "KEYBOARD",
        /// The player's own layouts (`TouchConfig::layouts`).
        Custom => "CUSTOM",
    }
}

vibebms_core::choices! {
    /// Where the START / SELECT touch buttons go.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum TouchSide {
        /// Beside the play screen on the turntable's side: a custom layout's
        /// turntables when they are all on one half of the screen, else left
        /// for 1P and double play, right for 2P.
        #[default]
        Auto => "AUTO",
        Left => "LEFT",
        Right => "RIGHT",
    }
}

/// A touch button: its lane, and its rectangle as fractions of the
/// controls area (the strip below the play screen; may reach past it).
/// A round button is the circle inside the rectangle.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct TouchButton {
    pub lane: u8,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub round: bool,
}

/// The buttons of one key mode and orientation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct TouchLayout {
    pub buttons: Vec<TouchButton>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TouchConfig {
    pub mode: TouchMode,
    pub style: TouchStyle,
    /// Fraction of the screen height used by the controls, in landscape
    /// (the default skin's touch layout fills the rest on a 4:3 tablet) and
    /// in portrait.
    pub landscape_height: f32,
    pub portrait_height: f32,
    /// Opacity of the controls.
    pub opacity: f32,
    /// The screen's diagonal in inches, for controls at their real size
    /// (0: guessed from the kind of device).
    pub screen_inches: f32,
    /// Play double charts in landscape with two control sets.
    pub allow_dp: bool,
    /// Scratch sensitivity: degrees of rotation needed to register a direction change.
    pub scratch_threshold_deg: f32,
    /// The side of the screen START / SELECT go on.
    pub start_select_side: TouchSide,
    /// The preset a custom layout starts from, and what Custom shows for
    /// modes not edited.
    pub custom_from: TouchStyle,
    /// Edited layouts by key mode and orientation ("7KEYS landscape").
    pub layouts: std::collections::BTreeMap<String, TouchLayout>,
}

impl Default for TouchConfig {
    fn default() -> Self {
        TouchConfig {
            mode: TouchMode::Auto,
            style: TouchStyle::Iidx,
            landscape_height: 0.45,
            portrait_height: 0.32,
            opacity: 0.85,
            screen_inches: 0.0,
            allow_dp: true,
            scratch_threshold_deg: 6.0,
            start_select_side: TouchSide::Auto,
            custom_from: TouchStyle::Iidx,
            layouts: Default::default(),
        }
    }
}

/// The touch perspective play skin: lanes narrow toward the far end.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PerspectiveConfig {
    /// How much narrower the far end is than the judge line (0: flat, the
    /// far end as wide; 0.9: a tenth as wide).
    pub strength: f32,
    /// How far up the lanes reach: share of the room above the judge line.
    pub length: f32,
    /// Touch area: share of the screen height from the bottom in which the
    /// lanes are played (with touch controls).
    pub touch_height: f32,
}

/// Limits of `PerspectiveConfig`.
pub const PERSPECTIVE_MAX: f32 = 0.9;
pub const LENGTH_RANGE: (f32, f32) = (0.4, 1.0);
pub const TOUCH_HEIGHT_RANGE: (f32, f32) = (0.1, 0.6);

impl Default for PerspectiveConfig {
    fn default() -> Self {
        PerspectiveConfig {
            strength: 0.65,
            length: 1.0,
            touch_height: 0.3,
        }
    }
}
