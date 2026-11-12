//! Internet Ranking (IR) integration.
//!
//! beatoraja loads IR implementations as Java plugins, which a native player
//! cannot use. vibeBMS instead defines the [`IrClient`] trait and ships:
//!
//! * [`Lr2Ir`]: read-only rankings from LR2IR (`getrankingxml.cgi`).
//! * [`JsonIr`]: a small documented REST/JSON protocol (see
//!   `docs/IR_PROTOCOL.md`) whose payload mirrors beatoraja's
//!   `IRScoreData`, so IR servers can add a vibeBMS endpoint easily.
//!
//! New IR services are added by implementing [`IrClient`] and registering
//! them in [`create_client`].

use serde::{Deserialize, Serialize};
use vibebms_core::score::{ClearType, Score};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ChartRef {
    pub md5: String,
    pub sha256: String,
    pub title: String,
    pub artist: String,
    /// e.g. "7KEYS"
    pub mode: String,
    /// "LN" / "CN" / "HCN"
    pub ln_type: String,
    pub notes: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct IrScore {
    pub rank: u32,
    pub player: String,
    pub player_id: String,
    pub clear: ClearType,
    pub ex_score: u32,
    pub pgreat: u32,
    pub great: u32,
    pub max_combo: u32,
    pub min_bp: u32,
    pub notes: u32,
}

/// Payload sent on submission (field names follow beatoraja's IRScoreData).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ScoreSubmission {
    pub sha256: String,
    pub md5: String,
    pub lntype: String,
    pub clear: ClearType,
    pub epg: u32,
    pub lpg: u32,
    pub egr: u32,
    pub lgr: u32,
    pub egd: u32,
    pub lgd: u32,
    pub ebd: u32,
    pub lbd: u32,
    pub epr: u32,
    pub lpr: u32,
    pub ems: u32,
    pub lms: u32,
    pub maxcombo: u32,
    pub notes: u32,
    pub passnotes: u32,
    pub minbp: u32,
    pub exscore: u32,
    /// Lane option name(s), e.g. "RANDOM" or "MIRROR/OFF".
    pub option: String,
    pub seed: i64,
    pub gauge: String,
    pub judge_preset: String,
    pub device: String,
    /// Unix seconds.
    pub date: u64,
    pub client: String,
}

impl ScoreSubmission {
    pub fn from_score(chart: &ChartRef, s: &Score, clear: ClearType) -> Self {
        let c = |j: usize, late: usize| s.counts[j][late];
        ScoreSubmission {
            sha256: chart.sha256.clone(),
            md5: chart.md5.clone(),
            lntype: chart.ln_type.clone(),
            clear,
            epg: c(0, 0),
            lpg: c(0, 1),
            egr: c(1, 0),
            lgr: c(1, 1),
            egd: c(2, 0),
            lgd: c(2, 1),
            ebd: c(3, 0),
            lbd: c(3, 1),
            epr: c(4, 0),
            lpr: c(4, 1),
            ems: c(5, 0),
            lms: c(5, 1),
            maxcombo: s.max_combo,
            notes: s.total_notes,
            passnotes: s.passed,
            minbp: s.miss_count(),
            exscore: s.ex_score(),
            client: concat!("vibeBMS ", env!("CARGO_PKG_VERSION")).into(),
            date: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            ..Default::default()
        }
    }
}

#[derive(Debug)]
pub enum IrError {
    Unsupported,
    NotLoggedIn,
    Http(String),
    Format(String),
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrError::Unsupported => write!(f, "not supported by this IR"),
            IrError::NotLoggedIn => write!(f, "not logged in"),
            IrError::Http(e) => write!(f, "network error: {e}"),
            IrError::Format(e) => write!(f, "bad response: {e}"),
        }
    }
}
impl std::error::Error for IrError {}

pub type IrResult<T> = Result<T, IrError>;

pub trait IrClient: Send + Sync {
    fn name(&self) -> &str;
    fn can_submit(&self) -> bool {
        false
    }
    fn login(&mut self, _user: &str, _password: &str) -> IrResult<()> {
        Err(IrError::Unsupported)
    }
    fn ranking(&self, chart: &ChartRef) -> IrResult<Vec<IrScore>>;
    fn submit(&self, _score: &ScoreSubmission) -> IrResult<()> {
        Err(IrError::Unsupported)
    }
    /// Web page for a chart, if the IR has one.
    fn chart_url(&self, _chart: &ChartRef) -> Option<String> {
        None
    }
}

/// User-facing IR configuration (stored in the portable config file).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct IrConfig {
    pub enabled: bool,
    /// "lr2ir" or "json"
    pub kind: String,
    pub name: String,
    pub url: String,
    pub user: String,
    /// API token for JSON IRs (obtained by logging in or from the IR site).
    pub token: String,
    pub auto_submit: bool,
}

impl Default for IrConfig {
    fn default() -> Self {
        IrConfig {
            enabled: true,
            kind: "lr2ir".into(),
            name: "LR2IR".into(),
            url: LR2IR_BASE.into(),
            user: String::new(),
            token: String::new(),
            auto_submit: false,
        }
    }
}

pub fn create_client(cfg: &IrConfig) -> Option<Box<dyn IrClient>> {
    if !cfg.enabled {
        return None;
    }
    match cfg.kind.as_str() {
        "lr2ir" => Some(Box::new(Lr2Ir::new(&cfg.url))),
        "json" => Some(Box::new(JsonIr::new(&cfg.name, &cfg.url, &cfg.token))),
        _ => None,
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("vibeBMS/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn read_body(resp: ureq::Response) -> IrResult<Vec<u8>> {
    use std::io::Read;
    let mut v = Vec::new();
    resp.into_reader()
        .take(16 << 20)
        .read_to_end(&mut v)
        .map_err(|e| IrError::Http(e.to_string()))?;
    Ok(v)
}

// ---------------------------------------------------------------- LR2IR --

pub const LR2IR_BASE: &str = "http://www.dream-pro.info/~lavalse/LR2IR";

pub struct Lr2Ir {
    base: String,
    agent: ureq::Agent,
}

impl Lr2Ir {
    pub fn new(base: &str) -> Self {
        let base = if base.is_empty() { LR2IR_BASE } else { base };
        Lr2Ir {
            base: base.trim_end_matches('/').to_string(),
            agent: agent(),
        }
    }
}

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&close)? + a;
    Some(s[a..b].trim())
}

/// LR2IR clear codes: 1 FAILED, 2 EASY, 3 CLEAR, 4 HARD, 5 FULL COMBO.
fn lr2_clear(v: u32) -> ClearType {
    match v {
        1 => ClearType::Failed,
        2 => ClearType::Easy,
        3 => ClearType::Normal,
        4 => ClearType::Hard,
        5 => ClearType::FullCombo,
        _ => ClearType::NoPlay,
    }
}

pub fn parse_lr2ir_ranking(xml: &str) -> Vec<IrScore> {
    let mut out = Vec::new();
    for (i, block) in xml.split("<score>").skip(1).enumerate() {
        let n = |t: &str| {
            tag(block, t)
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0)
        };
        let pg = n("pg");
        let gr = n("gr");
        out.push(IrScore {
            rank: i as u32 + 1,
            player: tag(block, "name").unwrap_or("").to_string(),
            player_id: tag(block, "id").unwrap_or("").to_string(),
            clear: lr2_clear(n("clear")),
            ex_score: pg * 2 + gr,
            pgreat: pg,
            great: gr,
            max_combo: n("combo"),
            min_bp: n("minbp"),
            notes: n("notes"),
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.ex_score));
    for (i, s) in out.iter_mut().enumerate() {
        s.rank = i as u32 + 1;
    }
    out
}

impl IrClient for Lr2Ir {
    fn name(&self) -> &str {
        "LR2IR"
    }

    fn ranking(&self, chart: &ChartRef) -> IrResult<Vec<IrScore>> {
        if chart.md5.is_empty() {
            return Err(IrError::Unsupported); // LR2IR is keyed by MD5 (no bmson)
        }
        let url = format!(
            "{}/2/getrankingxml.cgi?id=1&songmd5={}&lastupdate=",
            self.base, chart.md5
        );
        let resp = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| IrError::Http(e.to_string()))?;
        let body = read_body(resp)?;
        let text = encoding_rs::SHIFT_JIS.decode(&body).0;
        Ok(parse_lr2ir_ranking(&text))
    }

    fn chart_url(&self, chart: &ChartRef) -> Option<String> {
        (!chart.md5.is_empty())
            .then(|| format!("{}/search.cgi?mode=ranking&bmsmd5={}", self.base, chart.md5))
    }
}

// ----------------------------------------------------------------- JSON --

/// Generic JSON IR (see `docs/IR_PROTOCOL.md`).
pub struct JsonIr {
    name: String,
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl JsonIr {
    pub fn new(name: &str, base: &str, token: &str) -> Self {
        JsonIr {
            name: name.to_string(),
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            agent: agent(),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    fn auth(&self, r: ureq::Request) -> ureq::Request {
        if self.token.is_empty() {
            r
        } else {
            r.set("Authorization", &format!("Bearer {}", self.token))
        }
    }
}

#[derive(Deserialize)]
struct LoginResponse {
    token: String,
}

impl IrClient for JsonIr {
    fn name(&self) -> &str {
        &self.name
    }

    fn can_submit(&self) -> bool {
        !self.token.is_empty()
    }

    fn login(&mut self, user: &str, password: &str) -> IrResult<()> {
        let resp = self
            .agent
            .post(&format!("{}/login", self.base))
            .send_json(serde_json::json!({ "user": user, "password": password }))
            .map_err(|e| IrError::Http(e.to_string()))?;
        let r: LoginResponse = serde_json::from_slice(&read_body(resp)?)
            .map_err(|e| IrError::Format(e.to_string()))?;
        self.token = r.token;
        Ok(())
    }

    fn ranking(&self, chart: &ChartRef) -> IrResult<Vec<IrScore>> {
        let r = self
            .auth(self.agent.get(&format!("{}/ranking", self.base)))
            .query("sha256", &chart.sha256)
            .query("md5", &chart.md5)
            .query("lntype", &chart.ln_type);
        let resp = r.call().map_err(|e| IrError::Http(e.to_string()))?;
        serde_json::from_slice(&read_body(resp)?).map_err(|e| IrError::Format(e.to_string()))
    }

    fn submit(&self, score: &ScoreSubmission) -> IrResult<()> {
        if self.token.is_empty() {
            return Err(IrError::NotLoggedIn);
        }
        self.auth(self.agent.post(&format!("{}/scores", self.base)))
            .send_json(serde_json::to_value(score).map_err(|e| IrError::Format(e.to_string()))?)
            .map_err(|e| IrError::Http(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lr2ir_xml() {
        let xml = "#<?xml version=\"1.0\" encoding=\"shift_jis\"?><ranking>\
            <score><name>alice</name><id>1</id><clear>4</clear><notes>500</notes><combo>300</combo><pg>400</pg><gr>80</gr><minbp>5</minbp></score>\
            <score><name>bob</name><id>2</id><clear>5</clear><notes>500</notes><combo>500</combo><pg>450</pg><gr>50</gr><minbp>0</minbp></score>\
            </ranking><lastupdate>x</lastupdate>";
        let r = parse_lr2ir_ranking(xml);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].player, "bob");
        assert_eq!(r[0].ex_score, 950);
        assert_eq!(r[0].clear, ClearType::FullCombo);
        assert_eq!(r[1].rank, 2);
    }

    #[test]
    fn submission_from_score() {
        let mut s = Score {
            total_notes: 3,
            passed: 3,
            ..Default::default()
        };
        s.add(0, true);
        s.add(0, false);
        s.add(4, false);
        let sub = ScoreSubmission::from_score(
            &ChartRef {
                sha256: "x".into(),
                ..Default::default()
            },
            &s,
            ClearType::Normal,
        );
        assert_eq!((sub.epg, sub.lpg, sub.lpr, sub.exscore), (1, 1, 1, 4));
        let j = serde_json::to_value(&sub).unwrap();
        assert_eq!(j["clear"], "Normal");
    }
}
