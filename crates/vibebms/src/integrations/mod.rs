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
mod worker;
#[cfg(desktop)]
mod ws;

use crate::config::Config;
#[cfg(desktop)]
use crate::config::ObsConfig;
use crate::paths::Paths;

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
            difficulty: crate::app::screenshot::difficulty(song.difficulty),
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

/// The integrations running (as the settings say) and the activity they
/// were last told.
pub struct Integrations {
    #[cfg(desktop)]
    discord: Option<discord::Discord>,
    #[cfg(desktop)]
    obs: Option<obs::Obs>,
    /// The root's credentials: OBS's password, with the address it was
    /// entered for.
    #[cfg(desktop)]
    secrets: crate::ir::creds::Keys,
    activity: Option<Activity>,
}

/// What the settings say of OBS's password.
#[cfg(desktop)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObsPassword {
    None,
    Kept,
    /// Kept for another address than the settings' now: not used.
    Elsewhere,
}

impl Integrations {
    /// None running yet; the secrets are those under `paths`.
    pub fn new(paths: &Paths) -> Integrations {
        #[cfg(not(desktop))]
        let _ = paths;
        Integrations {
            #[cfg(desktop)]
            discord: None,
            #[cfg(desktop)]
            obs: None,
            #[cfg(desktop)]
            secrets: crate::ir::creds::Keys::open_root(paths),
            activity: None,
        }
    }

    /// The password an older configuration kept in `[obs]` goes to the
    /// credentials, for the address beside it, unless one is kept already
    /// (a configuration never puts a password in the place of one the
    /// player entered). True when the configuration held one (it is then
    /// saved without it).
    #[cfg(desktop)]
    pub fn take_old_password(&mut self, obs: &mut ObsConfig) -> bool {
        let old = std::mem::take(&mut obs.old_password.0);
        if old.is_empty() {
            return false;
        }
        if !self.secrets.has_obs() {
            if let Err(e) = self.set_obs_password(obs, old) {
                log::warn!("credentials: {e}");
            }
        }
        true
    }

    /// Keep `password` for the OBS at `obs`'s address (empty: forget it).
    #[cfg(desktop)]
    pub fn set_obs_password(&mut self, obs: &ObsConfig, password: String) -> std::io::Result<()> {
        self.secrets.set_obs(password, &obs.host, obs.port)
    }

    /// Whether a password is kept for OBS, and for `obs`'s address.
    #[cfg(desktop)]
    pub fn obs_password(&self, obs: &ObsConfig) -> ObsPassword {
        match self.secrets.obs(&obs.host, obs.port) {
            Some(_) => ObsPassword::Kept,
            None if self.secrets.has_obs() => ObsPassword::Elsewhere,
            None => ObsPassword::None,
        }
    }

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
            let password = self.secrets.obs(&o.host, o.port).unwrap_or("");
            match &mut self.obs {
                Some(x) if !o.enabled => {
                    x.stop();
                    self.obs = None;
                }
                Some(x) => x.configure(o, password),
                None if o.enabled => {
                    let mut x = obs::Obs::start(o.clone(), password.to_string());
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
