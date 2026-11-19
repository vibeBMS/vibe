//! Internet ranking settings: the accounts, whose rankings select shows,
//! when plays are sent. Keys are never here: they are in the credentials
//! store (`ir::creds`), so a shared configuration carries none.

use serde::{Deserialize, Serialize};
use vibebms_ir::ProviderId;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IrSettings {
    /// The account whose rankings the music select shows (`None`: the
    /// first enabled one).
    pub primary: Option<ProviderId>,
    pub submit: SubmitPolicy,
    /// One per provider (vibeBMS IRs: one per slot).
    pub accounts: Vec<IrAccount>,
    /// Whose IR lamps the music select shows beside the player's own.
    #[serde(skip_serializing_if = "Beside::is_mine")]
    pub beside: Beside,
}

/// Whose IR lamps show beside the player's (skins with a rival lamp show
/// them there; the others show the player's own only).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Beside {
    /// The player's own on the IR (of the first account that reads them).
    #[default]
    Mine,
    Off,
    /// A rival's (or the player's own) on one account.
    Player(PlayerKey),
    /// Another profile's records (a local rival; its id).
    Profile(String),
}

impl Beside {
    fn is_mine(&self) -> bool {
        *self == Beside::Mine
    }
}

/// A player on an IR account.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlayerKey {
    pub provider: ProviderId,
    pub player: String,
}

vibebms_core::choices! {
    /// When a play goes to the outbox.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum SubmitPolicy {
        /// As the result screen opens.
        #[default]
        Auto => "AUTO",
        /// When the player asks for it on the result screen.
        Ask => "ASK",
        Off => "OFF",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IrAccount {
    pub provider: ProviderId,
    /// A vibeBMS IR's name and its API's base URL (BMS-IR: its read API
    /// elsewhere, a test server; empty: bms-ir.org).
    pub name: String,
    pub url: String,
    pub enabled: bool,
    /// Its plays are sent (as `IrSettings::submit` says).
    pub submit: bool,
    /// Who the server says the account is (its name, its id), once the
    /// key was checked.
    pub user: String,
    pub user_id: String,
    /// BMS-IR: whose scores count (its `client_view`), the difficulty
    /// tables whose lamps are read, and the rivals' player ids (the
    /// player's own is `user_id`, entered).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub view: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rivals: Vec<String>,
    /// A vibeBMS IR's web page of a chart, as its `/me` gave it
    /// (`{sha256}`, `{md5}` in it).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub chart_page: String,
    /// A key a configuration of before kept (`[ir_server]`'s token): moved
    /// to the credentials store as the app starts, never written here.
    #[serde(skip_serializing)]
    pub old_token: OldToken,
}

impl Default for IrAccount {
    fn default() -> Self {
        IrAccount {
            provider: ProviderId::VibeJson(0),
            name: String::new(),
            url: String::new(),
            enabled: true,
            submit: true,
            user: String::new(),
            user_id: String::new(),
            view: String::new(),
            tables: Vec::new(),
            rivals: Vec::new(),
            chart_page: String::new(),
            old_token: OldToken::default(),
        }
    }
}

/// The name of a provider, for people.
pub fn provider_name(p: ProviderId) -> &'static str {
    match p {
        ProviderId::VibeJson(_) => "vibeBMS IR",
        ProviderId::Bokutachi => "Bokutachi",
        ProviderId::BmsIr => "BMS-IR",
        ProviderId::RianIr => "rianIR",
    }
}

impl IrAccount {
    pub fn title(&self) -> &str {
        let url = self.url.trim().trim_end_matches('/');
        match self.provider {
            ProviderId::VibeJson(_) if !self.name.is_empty() => &self.name,
            ProviderId::VibeJson(_) if !url.is_empty() => url,
            p => provider_name(p),
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
    pub fn account(&self, p: ProviderId) -> Option<&IrAccount> {
        self.accounts.iter().find(|a| a.provider == p)
    }

    pub fn account_mut(&mut self, p: ProviderId) -> Option<&mut IrAccount> {
        self.accounts.iter_mut().find(|a| a.provider == p)
    }

    /// The account the music select's rankings come from.
    pub fn primary(&self) -> Option<&IrAccount> {
        let on = || self.accounts.iter().filter(|a| a.enabled);
        on().find(|a| Some(a.provider) == self.primary)
            .or_else(|| on().next())
    }

    /// A slot no vibeBMS IR account has.
    pub fn free_slot(&self) -> Option<u8> {
        (0..=u8::MAX).find(|&n| self.account(ProviderId::VibeJson(n)).is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A BMS-IR account and a rival beside: written only when set, read
    /// back as they were.
    #[test]
    fn read_accounts_round_trip() {
        let plain = IrSettings {
            accounts: vec![IrAccount::default()],
            ..Default::default()
        };
        let text = toml::to_string_pretty(&plain).unwrap();
        assert!(
            !text.contains("beside") && !text.contains("tables"),
            "{text}"
        );
        let s = IrSettings {
            accounts: vec![IrAccount {
                provider: ProviderId::BmsIr,
                user_id: "191167".into(),
                view: "bmz-lr2".into(),
                tables: vec!["official".into(), "satellite".into()],
                rivals: vec!["100005".into()],
                ..Default::default()
            }],
            beside: Beside::Player(PlayerKey {
                provider: ProviderId::BmsIr,
                player: "100005".into(),
            }),
            ..Default::default()
        };
        let text = toml::to_string_pretty(&s).unwrap();
        assert_eq!(toml::from_str::<IrSettings>(&text).unwrap(), s, "{text}");
        let off: IrSettings = toml::from_str("beside = \"off\"").unwrap();
        assert_eq!(off.beside, Beside::Off);
    }
}
