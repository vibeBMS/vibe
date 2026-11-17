//! Internet ranking settings: the accounts, whose rankings select shows,
//! when plays are sent. Keys are never here: they are in the credentials
//! store (`ir::creds`), so a shared configuration carries none.

use serde::{Deserialize, Serialize};
use vibebms_ir::eligibility::ProviderKind;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IrSettings {
    /// The account whose rankings the music select shows (`IrAccount::key`;
    /// empty: the first enabled one).
    pub primary: String,
    pub submit: SubmitPolicy,
    pub accounts: Vec<IrAccount>,
}

/// When a play goes to the outbox.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubmitPolicy {
    /// As the result screen opens.
    #[default]
    Auto,
    /// When the player asks for it on the result screen.
    Ask,
    Off,
}

impl SubmitPolicy {
    pub const ALL: [SubmitPolicy; 3] = [SubmitPolicy::Auto, SubmitPolicy::Ask, SubmitPolicy::Off];

    pub fn label(self) -> &'static str {
        match self {
            SubmitPolicy::Auto => "AUTO",
            SubmitPolicy::Ask => "ASK",
            SubmitPolicy::Off => "OFF",
        }
    }
}

/// An IR the player uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    /// A server of vibeBMS's own protocol (`docs/IR_PROTOCOL.md`).
    #[default]
    #[serde(rename = "vibe-json")]
    VibeJson,
    #[serde(rename = "bokutachi")]
    Bokutachi,
    #[serde(rename = "bmsir")]
    BmsIr,
    #[serde(rename = "rianir")]
    RianIr,
}

impl Provider {
    pub fn kind(self) -> ProviderKind {
        match self {
            Provider::VibeJson => ProviderKind::VibeJson,
            Provider::Bokutachi => ProviderKind::Bokutachi,
            Provider::BmsIr => ProviderKind::BmsIr,
            Provider::RianIr => ProviderKind::RianIr,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Provider::VibeJson => "vibeBMS IR",
            Provider::Bokutachi => "Bokutachi",
            Provider::BmsIr => "BMS-IR",
            Provider::RianIr => "rianIR",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IrAccount {
    pub provider: Provider,
    /// A vibeBMS IR's name and its API's base URL.
    pub name: String,
    pub url: String,
    pub enabled: bool,
    /// Its plays are sent (as `IrSettings::submit` says).
    pub submit: bool,
    /// Who the server says the account is (shown only).
    pub user: String,
    /// A key a configuration of before kept (`[ir_server]`'s token): moved
    /// to the credentials store as the app starts, never written here.
    #[serde(skip_serializing)]
    pub old_token: OldToken,
}

impl Default for IrAccount {
    fn default() -> Self {
        IrAccount {
            provider: Provider::default(),
            name: String::new(),
            url: String::new(),
            enabled: true,
            submit: true,
            user: String::new(),
            old_token: OldToken::default(),
        }
    }
}

impl IrAccount {
    /// What names the account in the credentials store and as the
    /// primary: the provider, a vibeBMS IR's URL with it.
    pub fn key(&self) -> String {
        match self.provider {
            Provider::VibeJson => format!("vibe-json {}", self.base_url()),
            Provider::Bokutachi => "bokutachi".into(),
            Provider::BmsIr => "bmsir".into(),
            Provider::RianIr => "rianir".into(),
        }
    }

    /// `url` without spaces or a final `/`.
    pub fn base_url(&self) -> &str {
        self.url.trim().trim_end_matches('/')
    }

    pub fn title(&self) -> &str {
        match self.provider {
            Provider::VibeJson if !self.name.is_empty() => &self.name,
            Provider::VibeJson if !self.url.is_empty() => self.base_url(),
            p => p.name(),
        }
    }
}

/// A key read from an old configuration; `Debug` does not show it.
#[derive(Clone, Default, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct OldToken(pub String);

impl std::fmt::Debug for OldToken {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "" } else { "<redacted>" })
    }
}

impl IrSettings {
    /// The account the music select's rankings come from.
    pub fn primary(&self) -> Option<&IrAccount> {
        let on = || self.accounts.iter().filter(|a| a.enabled);
        on().find(|a| a.key() == self.primary)
            .or_else(|| on().next())
    }

    /// The account of `p` (one of each but vibeBMS IRs), made when there
    /// is none.
    pub fn account_mut(&mut self, p: Provider) -> &mut IrAccount {
        let i = match self.accounts.iter().position(|a| a.provider == p) {
            Some(i) => i,
            None => {
                self.accounts.push(IrAccount {
                    provider: p,
                    enabled: false,
                    ..Default::default()
                });
                self.accounts.len() - 1
            }
        };
        &mut self.accounts[i]
    }
}
