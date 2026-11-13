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
        // the e*/l* fields have no "exact" bucket: count exact hits as early
        let c = |j: usize, late: usize| s.counts[j][late] + if late == 0 { s.just[j] } else { 0 };
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
            date: web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
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

/// An HTTP request an [`IrClient`] wants sent. The application performs it
/// with its own HTTP stack (native or browser) and feeds the body back.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    fn get(url: String) -> Self {
        HttpRequest {
            method: "GET",
            url,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
}

/// HTTP-free IR client: builds requests and parses responses.
pub trait IrClient: Send + Sync {
    fn name(&self) -> &str;
    fn can_submit(&self) -> bool {
        false
    }
    fn ranking_request(&self, chart: &ChartRef) -> IrResult<HttpRequest>;
    fn parse_ranking(&self, body: &[u8]) -> IrResult<Vec<IrScore>>;
    fn submit_request(&self, _score: &ScoreSubmission) -> IrResult<HttpRequest> {
        Err(IrError::Unsupported)
    }
    fn login_request(&self, _user: &str, _password: &str) -> IrResult<HttpRequest> {
        Err(IrError::Unsupported)
    }
    /// Returns the API token from a login response.
    fn parse_login(&self, _body: &[u8]) -> IrResult<String> {
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

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---------------------------------------------------------------- LR2IR --

pub const LR2IR_BASE: &str = "http://www.dream-pro.info/~lavalse/LR2IR";

pub struct Lr2Ir {
    base: String,
}

impl Lr2Ir {
    pub fn new(base: &str) -> Self {
        let base = if base.is_empty() { LR2IR_BASE } else { base };
        Lr2Ir {
            base: base.trim_end_matches('/').to_string(),
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

    fn ranking_request(&self, chart: &ChartRef) -> IrResult<HttpRequest> {
        if chart.md5.is_empty() {
            return Err(IrError::Unsupported); // LR2IR is keyed by MD5 (no bmson)
        }
        Ok(HttpRequest::get(format!(
            "{}/2/getrankingxml.cgi?id=1&songmd5={}&lastupdate=",
            self.base, chart.md5
        )))
    }

    fn parse_ranking(&self, body: &[u8]) -> IrResult<Vec<IrScore>> {
        Ok(parse_lr2ir_ranking(&encoding_rs::SHIFT_JIS.decode(body).0))
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
}

impl JsonIr {
    pub fn new(name: &str, base: &str, token: &str) -> Self {
        JsonIr {
            name: name.to_string(),
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    fn req(&self, method: &'static str, path: String, body: Option<Vec<u8>>) -> HttpRequest {
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        if !self.token.is_empty() {
            headers.push(("Authorization".into(), format!("Bearer {}", self.token)));
        }
        HttpRequest {
            method,
            url: format!("{}{path}", self.base),
            headers,
            body: body.unwrap_or_default(),
        }
    }
}

#[derive(Deserialize)]
struct LoginResponse {
    token: String,
}

fn fmt_err(e: serde_json::Error) -> IrError {
    IrError::Format(e.to_string())
}

impl IrClient for JsonIr {
    fn name(&self) -> &str {
        &self.name
    }

    fn can_submit(&self) -> bool {
        !self.token.is_empty()
    }

    fn ranking_request(&self, chart: &ChartRef) -> IrResult<HttpRequest> {
        Ok(self.req(
            "GET",
            format!(
                "/ranking?sha256={}&md5={}&lntype={}",
                enc(&chart.sha256),
                enc(&chart.md5),
                enc(&chart.ln_type)
            ),
            None,
        ))
    }

    fn parse_ranking(&self, body: &[u8]) -> IrResult<Vec<IrScore>> {
        serde_json::from_slice(body).map_err(fmt_err)
    }

    fn submit_request(&self, score: &ScoreSubmission) -> IrResult<HttpRequest> {
        if self.token.is_empty() {
            return Err(IrError::NotLoggedIn);
        }
        Ok(self.req(
            "POST",
            "/scores".into(),
            Some(serde_json::to_vec(score).map_err(fmt_err)?),
        ))
    }

    fn login_request(&self, user: &str, password: &str) -> IrResult<HttpRequest> {
        let body = serde_json::to_vec(&serde_json::json!({ "user": user, "password": password }))
            .map_err(fmt_err)?;
        Ok(self.req("POST", "/login".into(), Some(body)))
    }

    fn parse_login(&self, body: &[u8]) -> IrResult<String> {
        Ok(serde_json::from_slice::<LoginResponse>(body)
            .map_err(fmt_err)?
            .token)
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
    fn json_ir_requests() {
        let ir = JsonIr::new("x", "https://ir.example/api/", "tok");
        let r = ir
            .ranking_request(&ChartRef {
                sha256: "ab".into(),
                ln_type: "LN".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            r.url,
            "https://ir.example/api/ranking?sha256=ab&md5=&lntype=LN"
        );
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer tok"));
        assert_eq!(ir.parse_login(br#"{"token":"t2"}"#).unwrap(), "t2");
    }

    #[test]
    fn submission_from_score() {
        let mut s = Score {
            total_notes: 3,
            passed: 3,
            ..Default::default()
        };
        s.add(0, 1);
        s.add(0, -1);
        s.add(4, -1);
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
