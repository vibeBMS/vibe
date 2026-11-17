//! One IR service as a sans-IO adapter: it builds one request per
//! operation and reads its answer; the application sends the request
//! (`platform::http::exchange`) and gives the answer back. Nothing here
//! reads a clock or stores a credential.

use crate::eligibility::ProviderKind;
use crate::http::{HttpRequest, HttpResponse, RetryAfter, Secret};
use crate::lamps::{ChartInfo, LampSet, RankView};
use crate::rule::Profile;
use serde::{Deserialize, Serialize};
use std::fmt;
use vibebms_core::chart::LnType;
use vibebms_core::record::{Platform, PlayRecord};
use vibebms_core::PlayMode;

/// An IR account's service. Stored in the outbox and as a cache key by
/// its text (`bokutachi`, `bmsir`, `rianir`, `vibe-json:0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ProviderId {
    /// A vibe-json server: the user's n-th.
    VibeJson(u8),
    Bokutachi,
    BmsIr,
    RianIr,
}

impl ProviderId {
    /// Its gate in `eligibility`.
    pub fn kind(self) -> ProviderKind {
        match self {
            ProviderId::VibeJson(_) => ProviderKind::VibeJson,
            ProviderId::Bokutachi => ProviderKind::Bokutachi,
            ProviderId::BmsIr => ProviderKind::BmsIr,
            ProviderId::RianIr => ProviderKind::RianIr,
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderId::VibeJson(n) => write!(f, "vibe-json:{n}"),
            ProviderId::Bokutachi => f.write_str("bokutachi"),
            ProviderId::BmsIr => f.write_str("bmsir"),
            ProviderId::RianIr => f.write_str("rianir"),
        }
    }
}

impl From<ProviderId> for String {
    fn from(p: ProviderId) -> String {
        p.to_string()
    }
}

impl TryFrom<String> for ProviderId {
    type Error = String;

    fn try_from(s: String) -> Result<ProviderId, String> {
        Ok(match s.as_str() {
            "bokutachi" => ProviderId::Bokutachi,
            "bmsir" => ProviderId::BmsIr,
            "rianir" => ProviderId::RianIr,
            _ => s
                .strip_prefix("vibe-json:")
                .and_then(|n| n.parse().ok())
                .map(ProviderId::VibeJson)
                .ok_or_else(|| format!("unknown IR provider {s:?}"))?,
        })
    }
}

/// What a provider offers here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    pub submit: Avail,
    /// One chart's leaderboard.
    pub ranking: Avail,
    /// Chart facts (title, notes, players) in bulk.
    pub chart_info: Avail,
    /// One player's lamps in bulk.
    pub player_lamps: Avail,
    /// The server knows the user's rivals.
    pub rivals: Avail,
    pub chart_key: ChartKeys,
    pub auth: AuthKind,
    /// Our own budget, below the server's limit.
    pub rate: Rate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Avail {
    Yes,
    No,
    Unavailable(Why),
}

/// Why an operation the service has is not offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// The operator has not allowed vibeBMS yet.
    OperatorPending,
    /// The service sends no CORS headers: not from the browser.
    NoCors,
    /// Only for a build CI published (`builds.json`).
    NeedsOfficialBuild,
    NeedsAccount,
}

/// The chart hashes a service knows charts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKeys {
    Md5Only,
    Sha256Only,
    Either,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthKind {
    None,
    /// An API key or token the user pastes.
    Token,
}

/// A request budget: `per_min` on average, `burst` at once after a pause,
/// `concurrent` in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    pub per_min: u32,
    pub burst: u32,
    pub concurrent: u32,
}

/// A chart a request is about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChartId {
    pub md5: String,
    pub sha256: String,
    pub mode: PlayMode,
    /// The LN mode its scores are filed under (`LnProfile::score_ln`).
    pub ln: LnType,
    /// The service's own id of the chart, where it needs one (Tachi's
    /// `chartID`, learned from its PB lists).
    pub remote: Option<String>,
}

/// A body frozen for the outbox: made once per play and provider, sent
/// byte for byte on every attempt. No credential is in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frozen {
    pub content_type: String,
    pub body: String,
}

/// The credentials of an account, given to [`Provider::request`] only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creds {
    /// The player id the service also wants, empty when the token says it.
    pub player: String,
    pub token: Secret,
}

impl Creds {
    pub fn token(token: Secret) -> Creds {
        Creds {
            player: String::new(),
            token,
        }
    }
}

/// Who an account is, as the service says.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub id: String,
    /// Empty when the service does not say.
    pub name: String,
    /// The credentials allow submitting scores.
    pub can_submit: bool,
}

/// One request.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum Op<'a> {
    /// Check the credentials, learn the account.
    Verify,
    /// Send a frozen body. `key` is the outbox item's id (an idempotency
    /// key); `user_intent`: the user pressed a button for it.
    Submit {
        body: &'a Frozen,
        key: &'a str,
        user_intent: bool,
    },
    /// Ask after a deferred submission (Tachi's import).
    Poll { remote: &'a str },
    /// A chart's leaderboard. `view`: whose scores (BMS-IR's
    /// `client_view`; Tachi's PMS playtype). `etag`: the cached answer's,
    /// for a 304 (not on the web: the browser revalidates by itself).
    Ranking {
        chart: &'a ChartId,
        view: &'a str,
        limit: u8,
        etag: Option<&'a str>,
    },
    /// Facts of up to 50 charts by MD5.
    ChartInfo { md5: &'a [&'a str] },
    /// One player's lamps: `scope` is a difficulty table (BMS-IR) or a
    /// playtype (Tachi).
    PlayerLamps {
        player: &'a str,
        scope: &'a str,
        view: &'a str,
        etag: Option<&'a str>,
    },
    /// The rivals the user set on the server.
    Rivals,
}

/// What an answer means.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Outcome {
    Account(AccountInfo),
    /// Taken. `remote`: the service's id of the score; `note`: a remark
    /// for the user (a chart the service does not know yet).
    Sent {
        remote: Option<String>,
        note: Option<String>,
    },
    /// Accepted for later: poll `remote`.
    Deferred {
        remote: String,
    },
    /// Still being processed: poll again.
    Pending,
    Ranking(RankView),
    Charts(Vec<ChartInfo>),
    Lamps(LampSet),
    Rivals(Vec<PlayerRef>),
    /// The cached answer still holds (304).
    NotModified,
    /// The server does not have this optional route.
    Unsupported,
    /// Try again later: 408, 429, 5xx. `after`: the server's
    /// `Retry-After`, or its documented default when the header is
    /// missing or unreadable.
    Retry {
        after: Option<RetryAfter>,
        why: String,
    },
    /// The credentials were refused (401 / 403): final for the account.
    Auth(String),
    /// Refused for good (another 4xx, a failed import).
    Rejected(String),
}

/// A player on a service.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrError {
    /// The operation needs credentials and none were given.
    NoCredentials,
    /// The provider does not offer the operation.
    NotOffered,
    /// The service knows the chart only by its own id, not yet learned.
    NoChartId,
    /// The play cannot be put in the service's terms.
    Unsendable(String),
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrError::NoCredentials => f.write_str("not signed in"),
            IrError::NotOffered => f.write_str("not offered by this IR"),
            IrError::NoChartId => f.write_str("chart not known to this IR yet"),
            IrError::Unsendable(why) => write!(f, "cannot be sent: {why}"),
        }
    }
}

impl std::error::Error for IrError {}

pub trait Provider {
    fn id(&self) -> ProviderId;
    fn caps(&self, on: Platform) -> Caps;
    /// The body for the outbox of a play `eligibility` let through as
    /// `profile`, without any credential. Called once per play.
    fn freeze(&self, rec: &PlayRecord, play_id: &str, profile: Profile) -> Result<Frozen, IrError>;
    /// The request for `op`, with what depends on the credentials.
    fn request(&self, op: &Op, creds: Option<&Creds>) -> Result<HttpRequest, IrError>;
    fn parse(&self, op: &Op, resp: &HttpResponse) -> Outcome;
}

/// What a status means whatever the service: 401/403 refuse the
/// credentials; 408, 429 and 5xx are worth another try (`Retry-After`, or
/// the service's default from `defaults` as `(status, seconds)`).
pub(crate) fn by_status(
    resp: &HttpResponse,
    why: impl FnOnce() -> String,
    defaults: &[(u16, u64)],
) -> Option<Outcome> {
    match resp.status {
        401 | 403 => Some(Outcome::Auth(why())),
        408 | 429 | 500..=599 => {
            let after = resp.retry_after().or_else(|| {
                defaults
                    .iter()
                    .find(|(s, _)| *s == resp.status)
                    .map(|&(_, secs)| RetryAfter::Secs(secs))
            });
            Some(Outcome::Retry { after, why: why() })
        }
        _ => None,
    }
}

/// `creds`, or `NoCredentials`.
pub(crate) fn need(creds: Option<&Creds>) -> Result<&Creds, IrError> {
    creds.ok_or(IrError::NoCredentials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_are_text() {
        for p in [
            ProviderId::VibeJson(3),
            ProviderId::Bokutachi,
            ProviderId::BmsIr,
            ProviderId::RianIr,
        ] {
            let j = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<ProviderId>(&j).unwrap(), p);
        }
        assert_eq!(
            serde_json::to_string(&ProviderId::VibeJson(0)).unwrap(),
            "\"vibe-json:0\""
        );
        assert!(serde_json::from_str::<ProviderId>("\"lr2ir\"").is_err());
    }

    #[test]
    fn statuses_mean_the_same_everywhere() {
        let resp = |status, headers: &[(&str, &str)]| HttpResponse {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: Vec::new(),
        };
        let why = || "x".to_string();
        assert_eq!(
            by_status(&resp(403, &[]), why, &[]),
            Some(Outcome::Auth("x".into()))
        );
        assert_eq!(
            by_status(&resp(429, &[("Retry-After", "7")]), why, &[(429, 60)]),
            Some(Outcome::Retry {
                after: Some(RetryAfter::Secs(7)),
                why: "x".into()
            })
        );
        assert_eq!(
            by_status(&resp(503, &[]), why, &[(503, 10)]),
            Some(Outcome::Retry {
                after: Some(RetryAfter::Secs(10)),
                why: "x".into()
            })
        );
        assert_eq!(
            by_status(&resp(502, &[]), why, &[(503, 10)]),
            Some(Outcome::Retry {
                after: None,
                why: "x".into()
            })
        );
        assert_eq!(by_status(&resp(404, &[]), why, &[]), None);
        assert_eq!(by_status(&resp(200, &[]), why, &[]), None);
    }
}
