//! Internet Ranking (IR): a small REST/JSON protocol (`docs/IR_PROTOCOL.md`)
//! whose score payload mirrors beatoraja's `IRScoreData`.
//!
//! The client is HTTP-free: it builds [`HttpRequest`]s and parses response
//! bodies; the application sends them with its own HTTP stack.

use serde::{Deserialize, Serialize};
use vibebms_core::score::{ClearType, Score};

/// The chart a request is about.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartRef {
    pub md5: String,
    pub sha256: String,
    /// "LN" / "CN" / "HCN"
    pub ln_type: String,
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

/// An HTTP request for the application to send.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// User-facing IR settings (stored in the config file).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct IrConfig {
    pub enabled: bool,
    pub name: String,
    /// Base URL of the IR's API.
    pub url: String,
    /// API token (from the IR's web site); needed to submit scores.
    pub token: String,
    pub auto_submit: bool,
}

/// A JSON IR client.
pub struct JsonIr {
    base: String,
    token: String,
}

impl JsonIr {
    /// The client for a configuration, if it is enabled and has a URL.
    pub fn new(cfg: &IrConfig) -> Option<JsonIr> {
        (cfg.enabled && !cfg.url.trim().is_empty()).then(|| JsonIr {
            base: cfg.url.trim().trim_end_matches('/').to_string(),
            token: cfg.token.clone(),
        })
    }

    fn req(&self, method: &'static str, path: String, body: Vec<u8>) -> HttpRequest {
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if !body.is_empty() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        if !self.token.is_empty() {
            headers.push(("Authorization".into(), format!("Bearer {}", self.token)));
        }
        HttpRequest {
            method,
            url: format!("{}{path}", self.base),
            headers,
            body,
        }
    }

    pub fn ranking_request(&self, chart: &ChartRef) -> HttpRequest {
        self.req(
            "GET",
            format!(
                "/ranking?sha256={}&md5={}&lntype={}",
                enc(&chart.sha256),
                enc(&chart.md5),
                enc(&chart.ln_type)
            ),
            Vec::new(),
        )
    }

    pub fn parse_ranking(body: &[u8]) -> Result<Vec<IrScore>, String> {
        serde_json::from_slice(body).map_err(|e| format!("bad response: {e}"))
    }

    /// The submission request (`Err` without an API token).
    pub fn submit_request(&self, score: &ScoreSubmission) -> Result<HttpRequest, String> {
        if self.token.is_empty() {
            return Err("no API token".into());
        }
        let body = serde_json::to_vec(score).map_err(|e| e.to_string())?;
        Ok(self.req("POST", "/scores".into(), body))
    }
}

/// Percent-encode a query value.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_ir_requests() {
        let cfg = IrConfig {
            enabled: true,
            url: "https://ir.example/api/".into(),
            token: "tok".into(),
            ..Default::default()
        };
        let ir = JsonIr::new(&cfg).unwrap();
        let r = ir.ranking_request(&ChartRef {
            sha256: "ab".into(),
            ln_type: "LN".into(),
            ..Default::default()
        });
        assert_eq!(
            r.url,
            "https://ir.example/api/ranking?sha256=ab&md5=&lntype=LN"
        );
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer tok"));
        assert!(JsonIr::new(&IrConfig::default()).is_none());
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
