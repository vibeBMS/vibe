//! Discord Rich Presence and OBS WebSocket (`integrations`; desktop).

use serde::{Deserialize, Serialize};

/// Discord Rich Presence: the screen and the chart in the player's Discord
/// status, through the Discord app's local IPC socket.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DiscordConfig {
    pub enabled: bool,
    /// The id of a Discord application (made at Discord's developer
    /// portal; its name is what the status says is being played). None is
    /// built in: empty, the feature waits for one.
    pub application_id: String,
}

impl DiscordConfig {
    pub fn is_default(&self) -> bool {
        *self == DiscordConfig::default()
    }
}

/// The OBS scenes switched to on each screen (empty: no switch).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ObsScenes {
    pub select: String,
    pub decide: String,
    pub play: String,
    pub result: String,
}

impl ObsScenes {
    /// Song select, decide, play, result.
    pub const LABELS: [&'static str; 4] = ["Song select", "Decide", "Play", "Result"];

    pub fn get(&self, i: usize) -> &str {
        match i {
            0 => &self.select,
            1 => &self.decide,
            2 => &self.play,
            _ => &self.result,
        }
    }

    pub fn get_mut(&mut self, i: usize) -> &mut String {
        match i {
            0 => &mut self.select,
            1 => &mut self.decide,
            2 => &mut self.play,
            _ => &mut self.result,
        }
    }
}

/// OBS through obs-websocket 5 (OBS 28 and later: Tools › WebSocket Server
/// Settings).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ObsConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    /// The password a configuration of before kept here (`migrate`), on
    /// its way to the credentials store as the app starts (`ir::creds`):
    /// neither read from `[obs]` nor written.
    #[serde(skip)]
    pub old_password: super::OldToken,
    pub scenes: ObsScenes,
    /// Start recording when a play starts and stop when its result is left.
    pub record_play: bool,
}

impl Default for ObsConfig {
    fn default() -> Self {
        ObsConfig {
            enabled: false,
            host: "127.0.0.1".into(),
            port: 4455,
            old_password: Default::default(),
            scenes: ObsScenes::default(),
            record_play: false,
        }
    }
}

impl ObsConfig {
    pub fn is_default(&self) -> bool {
        *self == ObsConfig::default()
    }
}
