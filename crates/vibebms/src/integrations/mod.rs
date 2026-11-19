//! What the player is doing, told to other programs (desktop, each off by
//! default): Discord Rich Presence through the Discord app's local IPC
//! socket (`discord`), and OBS through obs-websocket 5 (`obs`). Each runs
//! on a thread of its own that the game only sends to, so neither can hold
//! up a frame; each reconnects with a growing wait when its program is not
//! there.

#[cfg(desktop)]
mod codec;
#[cfg(desktop)]
pub mod discord;
#[cfg(desktop)]
pub mod obs;
#[cfg(desktop)]
mod ws;

use crate::config::Config;

/// A chart as the integrations name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chart {
    pub title: String,
    /// BEGINNER … INSANE (empty when the chart has none).
    pub difficulty: String,
}

impl Chart {
    pub fn of(song: &crate::library::Song) -> Chart {
        Chart {
            title: song.title.clone(),
            difficulty: crate::screenshot::difficulty(song.difficulty),
        }
    }

    /// `title [DIFFICULTY]`.
    pub fn label(&self) -> String {
        if self.difficulty.is_empty() {
            self.title.clone()
        } else {
            format!("{} [{}]", self.title, self.difficulty)
        }
    }
}

/// How a chart is being played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayKind {
    Play,
    Autoplay,
    Replay,
    Practice,
}

/// The screen the player is on (`Scene::activity`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activity {
    Select,
    Decide(Chart),
    Play(Chart, PlayKind),
    /// The chart and its clear lamp.
    Result(Chart, String),
}

impl Activity {
    /// Song select, decide, play, result (`ObsScenes::get`).
    pub fn screen(&self) -> usize {
        match self {
            Activity::Select => 0,
            Activity::Decide(_) => 1,
            Activity::Play(..) => 2,
            Activity::Result(..) => 3,
        }
    }
}

/// A connection's state, for the settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Off,
    /// On, but a setting it needs is missing (said).
    Needs(&'static str),
    Connecting,
    Connected,
    /// Not reached (the reason); it tries again.
    Failed(String),
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Status::Off => tr!("OFF").into(),
            Status::Needs(what) => tr!("needs {what}", what = what),
            Status::Connecting => tr!("connecting…").into(),
            Status::Connected => tr!("connected").into(),
            Status::Failed(e) => tr!("not connected: {e}", e = e),
        }
    }
}

/// A worker's status as it and the settings share it.
pub type SharedStatus = std::sync::Arc<std::sync::Mutex<Status>>;

/// The integrations running (as the settings say) and the activity they
/// were last told.
#[derive(Default)]
pub struct Integrations {
    #[cfg(desktop)]
    discord: Option<discord::Discord>,
    #[cfg(desktop)]
    obs: Option<obs::Obs>,
    activity: Option<Activity>,
}

impl Integrations {
    /// Started, stopped or told the new settings as `config` says (cheap
    /// when nothing changed: called each frame).
    pub fn sync(&mut self, config: &Config) {
        #[cfg(desktop)]
        {
            let d = &config.discord;
            let want = d.enabled && !d.application_id.trim().is_empty();
            match &mut self.discord {
                Some(x) if !want || x.application_id() != d.application_id.trim() => {
                    self.discord = None
                }
                _ => {}
            }
            if want && self.discord.is_none() {
                let mut x = discord::Discord::start(d.application_id.trim().to_string());
                if let Some(a) = &self.activity {
                    x.set(a.clone());
                }
                self.discord = Some(x);
            }
            let o = &config.obs;
            match &mut self.obs {
                Some(x) if !o.enabled => {
                    x.stop();
                    self.obs = None;
                }
                Some(x) => x.configure(o),
                None if o.enabled => {
                    let mut x = obs::Obs::start(o.clone());
                    if let Some(a) = &self.activity {
                        x.set(a.clone());
                    }
                    self.obs = Some(x);
                }
                None => {}
            }
        }
        #[cfg(not(desktop))]
        let _ = config;
    }

    /// Stop them (the app ends): a recording started for a play is
    /// stopped.
    pub fn stop(&mut self) {
        #[cfg(desktop)]
        {
            self.discord = None;
            if let Some(mut o) = self.obs.take() {
                o.stop();
            }
        }
    }

    /// The player is on `a` now (told on when it changed).
    pub fn set_activity(&mut self, a: Activity) {
        if self.activity.as_ref() == Some(&a) {
            return;
        }
        #[cfg(desktop)]
        {
            if let Some(d) = &mut self.discord {
                d.set(a.clone());
            }
            if let Some(o) = &mut self.obs {
                o.set(a.clone());
            }
        }
        self.activity = Some(a);
    }

    /// Discord's state for the settings.
    pub fn discord_status(&self, config: &Config) -> Status {
        let d = &config.discord;
        if !d.enabled {
            return Status::Off;
        }
        if d.application_id.trim().is_empty() {
            return Status::Needs(tr!("a Discord application id"));
        }
        #[cfg(desktop)]
        if let Some(x) = &self.discord {
            return x.status();
        }
        Status::Connecting
    }

    /// OBS's state for the settings.
    pub fn obs_status(&self, config: &Config) -> Status {
        if !config.obs.enabled {
            return Status::Off;
        }
        #[cfg(desktop)]
        if let Some(x) = &self.obs {
            return x.status();
        }
        Status::Connecting
    }
}
