//! vibeBMS's own protocol (`docs/IR_PROTOCOL.md`): a small REST/JSON API
//! any server can speak, whose score payload mirrors beatoraja's
//! `IRScoreData`. Version 2 adds the play's id, rule profile and devices,
//! an `Idempotency-Key`, an account check and optional lamp and rival
//! routes; a version 1 server still takes what version 2 sends.

use crate::http::{enc, HttpRequest, HttpResponse, Secret};
use crate::lamps::{LampSet, RankEntry, RankView, RemoteLamp};
use crate::provider::{
    by_status, need, AuthKind, Avail, Caps, ChartId, ChartKeys, Creds, Frozen, IrError, Op,
    Outcome, Provider, ProviderId, Rate,
};
use crate::rule::Profile;
use serde::{Deserialize, Serialize};
use vibebms_core::chart::LnType;
use vibebms_core::judge::Judge;
use vibebms_core::record::{DeviceFacts, Platform, PlayRecord};
use vibebms_core::score::{ClearType, Score};

/// The protocol version this client speaks.
pub const VERSION: u32 = 2;

/// A vibe-json server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VibeJson {
    slot: u8,
    base: String,
    /// Its web page of a chart (`/me`'s `chart_page`), if it has one.
    page: String,
}

impl VibeJson {
    /// The user's `slot`-th server at base URL `url`, if there is one.
    pub fn new(slot: u8, url: &str) -> Option<VibeJson> {
        let base = url.trim().trim_end_matches('/');
        (!base.is_empty()).then(|| VibeJson {
            slot,
            base: base.to_string(),
            page: String::new(),
        })
    }

    /// With the server's chart page (`AccountInfo::chart_page`): an
    /// `http(s)` address with `{sha256}` or `{md5}` in it.
    pub fn with_chart_page(mut self, page: &str) -> VibeJson {
        self.page = page.trim().to_string();
        self
    }

    fn get(&self, path: &str, creds: Option<&Creds>) -> HttpRequest {
        let req = HttpRequest::new("GET", format!("{}{path}", self.base));
        match creds {
            Some(c) => req.bearer(&c.token),
            None => req,
        }
    }

    fn post(&self, body: &Frozen, token: &Secret) -> HttpRequest {
        HttpRequest::new("POST", format!("{}/scores", self.base))
            .bearer(token)
            .json(&body.content_type, &body.body)
    }
}

/// The `lntype` of a chart's scores.
fn ln_wire(ln: LnType) -> &'static str {
    match ln {
        LnType::Ln => "LN",
        LnType::Cn => "CN",
        LnType::Hcn => "HCN",
    }
}

impl Provider for VibeJson {
    fn id(&self) -> ProviderId {
        ProviderId::VibeJson(self.slot)
    }

    fn caps(&self, _on: Platform) -> Caps {
        Caps {
            submit: Avail::Yes,
            ranking: Avail::Yes,
            chart_info: Avail::No,
            player_lamps: Avail::Yes,
            rivals: Avail::Yes,
            chart_key: ChartKeys::Either,
            auth: AuthKind::Token,
            rate: Rate {
                per_min: 30,
                burst: 5,
                concurrent: 2,
            },
        }
    }

    fn freeze(&self, rec: &PlayRecord, play_id: &str, profile: Profile) -> Result<Frozen, IrError> {
        let sub = ScoreSubmission::from_record(rec, play_id, profile);
        Ok(Frozen {
            content_type: "application/json".into(),
            body: serde_json::to_string(&sub).map_err(|e| IrError::Unsendable(e.to_string()))?,
        })
    }

    fn request(&self, op: &Op, creds: Option<&Creds>) -> Result<HttpRequest, IrError> {
        Ok(match *op {
            Op::Verify => self.get("/me", Some(need(creds)?)),
            Op::Submit { body, key, .. } => self
                .post(body, &need(creds)?.token)
                .header("Idempotency-Key", key),
            Op::Ranking {
                chart, limit, etag, ..
            } => {
                let mut path = format!(
                    "/ranking?sha256={}&md5={}&lntype={}",
                    enc(&chart.sha256),
                    enc(&chart.md5),
                    ln_wire(chart.ln)
                );
                if limit > 0 {
                    path += &format!("&limit={limit}");
                }
                let req = self.get(&path, creds);
                match etag {
                    Some(t) => req.header("If-None-Match", t),
                    None => req,
                }
            }
            Op::PlayerLamps { player, etag, .. } => {
                let req = self.get(&format!("/players/{}/lamps", enc(player)), creds);
                match etag {
                    Some(t) => req.header("If-None-Match", t),
                    None => req,
                }
            }
            Op::Rivals => self.get("/rivals", Some(need(creds)?)),
            Op::Poll { .. } | Op::ChartInfo { .. } => return Err(IrError::NotOffered),
        })
    }

    fn chart_page(&self, chart: &ChartId, _view: &str) -> Result<String, IrError> {
        let p = &self.page;
        let web = p.starts_with("https://") || p.starts_with("http://");
        if !web || !(p.contains("{sha256}") || p.contains("{md5}")) {
            return Err(IrError::NotOffered);
        }
        if p.contains("{sha256}") && chart.sha256.is_empty()
            || p.contains("{md5}") && chart.md5.is_empty()
        {
            return Err(IrError::NoChartId);
        }
        Ok(p.replace("{sha256}", &enc(&chart.sha256))
            .replace("{md5}", &enc(&chart.md5)))
    }

    fn parse(&self, op: &Op, resp: &HttpResponse) -> Outcome {
        if let Some(o) = by_status(resp, || error_text(resp), &[]) {
            return o;
        }
        let ok = (200..300).contains(&resp.status);
        match (op, resp.status) {
            // the same idempotency key again: the server has the play
            (Op::Submit { .. }, 409) => Outcome::Sent {
                remote: None,
                note: None,
            },
            (_, 304) => Outcome::NotModified,
            // a version 1 server: no such route
            (Op::Verify | Op::PlayerLamps { .. } | Op::Rivals, 404) => Outcome::Unsupported,
            (_, _) if !ok => Outcome::Rejected(error_text(resp)),
            (Op::Submit { .. }, _) => Outcome::Sent {
                remote: None,
                note: None,
            },
            (Op::Verify, _) => json(resp, Outcome::Account),
            (Op::Ranking { .. }, _) => match ranking_rows(&resp.body) {
                Ok((players, rows)) => Outcome::Ranking(RankView {
                    players: players.unwrap_or(rows.len() as u32),
                    entries: rows.into_iter().map(IrScore::entry).collect(),
                    etag: resp.etag(),
                }),
                Err(e) => Outcome::Rejected(e),
            },
            (Op::PlayerLamps { .. }, _) => json(resp, |l: Lamps| {
                let mut set = LampSet {
                    player_name: l.player_name,
                    etag: resp.etag(),
                    ..Default::default()
                };
                for x in l.lamps {
                    let hash = if x.sha256.is_empty() {
                        &x.md5
                    } else {
                        &x.sha256
                    };
                    let lamp = RemoteLamp {
                        clear: x.clear,
                        ex: x.ex_score,
                        bp: x.min_bp,
                        at_ms: x.date_ms,
                        remote: None,
                    };
                    set.insert(hash, lamp);
                }
                Outcome::Lamps(set)
            }),
            (Op::Rivals, _) => json(resp, Outcome::Rivals),
            (Op::Poll { .. } | Op::ChartInfo { .. }, _) => Outcome::Rejected("not offered".into()),
        }
    }
}

/// The answer's JSON as `T`, made an outcome.
fn json<T: serde::de::DeserializeOwned>(
    resp: &HttpResponse,
    f: impl FnOnce(T) -> Outcome,
) -> Outcome {
    match serde_json::from_slice(&resp.body) {
        Ok(v) => f(v),
        Err(e) => Outcome::Rejected(format!("bad response: {e}")),
    }
}

/// A refusal's words: `{"error": "..."}`, else the body, else the status.
fn error_text(resp: &HttpResponse) -> String {
    #[derive(Deserialize)]
    struct E {
        error: String,
    }
    match serde_json::from_slice::<E>(&resp.body) {
        Ok(e) => e.error,
        Err(_) if !resp.body.is_empty() => resp.text(),
        Err(_) => format!("HTTP {}", resp.status),
    }
}

/// A ranking: version 1's bare array, or version 2's object.
fn ranking_rows(body: &[u8]) -> Result<(Option<u32>, Vec<IrScore>), String> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Ranking {
        V1(Vec<IrScore>),
        V2 {
            players: Option<u32>,
            entries: Vec<IrScore>,
        },
    }
    match serde_json::from_slice(body) {
        Ok(Ranking::V1(rows)) => Ok((None, rows)),
        Ok(Ranking::V2 { players, entries }) => Ok((players, entries)),
        Err(e) => Err(format!("bad response: {e}")),
    }
}

#[derive(Deserialize)]
struct Lamps {
    #[serde(default)]
    player_name: String,
    lamps: Vec<Lamp>,
}

#[derive(Deserialize)]
struct Lamp {
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    md5: String,
    clear: ClearType,
    ex_score: Option<u32>,
    min_bp: Option<u32>,
    date_ms: Option<i64>,
}

/// A ranking row.
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

impl IrScore {
    fn entry(self) -> RankEntry {
        RankEntry {
            rank: self.rank,
            player: self.player,
            player_id: self.player_id,
            clear: self.clear,
            ex: self.ex_score,
            bp: Some(self.min_bp),
            max_combo: Some(self.max_combo),
            notes: Some(self.notes),
            client: None,
            at_ms: None,
        }
    }
}

/// The score sent (field names follow beatoraja's IRScoreData; version 2's
/// additions after `client`).
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
    #[serde(default)]
    pub v: u32,
    /// The play's id: the same play sent twice is one play.
    #[serde(default)]
    pub play_id: String,
    /// The rule profile (`vibe-lr2`, `vibe-beatoraja`).
    #[serde(default)]
    pub rule: String,
    /// `date` in Unix ms.
    #[serde(default)]
    pub date_ms: i64,
    #[serde(default)]
    pub devices: DeviceFacts,
}

impl ScoreSubmission {
    /// The submission of a play at `date` (Unix seconds).
    pub fn from_score(chart: &ChartRef, s: &Score, clear: ClearType, date: u64) -> Self {
        // the e*/l* fields have no "exact" bucket: count exact hits as early
        let c = |j: Judge, late: usize| {
            let j = j as usize;
            s.counts[j][late] + if late == 0 { s.just[j] } else { 0 }
        };
        ScoreSubmission {
            sha256: chart.sha256.clone(),
            md5: chart.md5.clone(),
            lntype: chart.ln_type.clone(),
            clear,
            epg: c(Judge::PGreat, 0),
            lpg: c(Judge::PGreat, 1),
            egr: c(Judge::Great, 0),
            lgr: c(Judge::Great, 1),
            egd: c(Judge::Good, 0),
            lgd: c(Judge::Good, 1),
            ebd: c(Judge::Bad, 0),
            lbd: c(Judge::Bad, 1),
            epr: c(Judge::Poor, 0),
            lpr: c(Judge::Poor, 1),
            ems: c(Judge::Miss, 0),
            lms: c(Judge::Miss, 1),
            maxcombo: s.max_combo,
            notes: s.total_notes,
            passnotes: s.passed,
            minbp: s.miss_count(),
            exscore: s.ex_score(),
            client: concat!("vibeBMS ", env!("CARGO_PKG_VERSION")).into(),
            date,
            ..Default::default()
        }
    }

    /// The version 2 submission of the play `rec`.
    pub fn from_record(rec: &PlayRecord, play_id: &str, profile: Profile) -> Self {
        let (r, res, d) = (&rec.rules, &rec.result, &rec.device);
        let chart = ChartRef {
            md5: rec.chart.md5.clone(),
            sha256: rec.chart.sha256.clone(),
            ln_type: ln_wire(rec.ln_filed()).into(),
        };
        let lanes: Vec<&str> = r.assist.lane.iter().map(|o| o.label()).collect();
        ScoreSubmission {
            option: lanes.join("/"),
            seed: r.seed,
            gauge: res.gauge.label().into(),
            judge_preset: format!("{} / {}", r.judge.label(), r.gauge_set.label()),
            device: if d.touch > 0 || d.touch_ui {
                "TOUCH"
            } else {
                "KEYBOARD/CONTROLLER"
            }
            .into(),
            client: format!("{} {}", rec.client.name, rec.client.version),
            v: VERSION,
            play_id: play_id.into(),
            rule: profile.wire().into(),
            date_ms: rec.ended_ms,
            devices: *d,
            ..Self::from_score(&chart, &res.score, res.clear, (rec.ended_ms / 1000) as u64)
        }
    }
}

/// The chart of a submission.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartRef {
    pub md5: String,
    pub sha256: String,
    /// "LN" / "CN" / "HCN"
    pub ln_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::RetryAfter;
    use crate::provider::{AccountInfo, ChartId, PlayerRef};
    use vibebms_core::lane_option::LaneOption;
    use vibebms_core::record::{ClientId, VERSION as RECORD};
    use vibebms_core::PlayMode;

    fn server() -> VibeJson {
        VibeJson::new(1, " https://ir.example/api/ ").unwrap()
    }

    fn creds() -> Creds {
        Creds::token(Secret::new("tok"))
    }

    fn resp(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: vec![("ETag".into(), "\"e1\"".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    fn record() -> PlayRecord {
        let mut r = PlayRecord {
            v: RECORD,
            client: ClientId {
                name: "vibeBMS".into(),
                ..Default::default()
            },
            ended_ms: 1_767_225_600_123,
            ..Default::default()
        };
        r.client.version = "0.1.0".into();
        r.chart.md5 = "m".into();
        r.chart.sha256 = "s".into();
        r.chart.mode = PlayMode::Beat7K;
        r.rules.seed = 99;
        r.rules.assist.lane = vec![LaneOption::Random];
        r.device.keyboard = 5;
        let s = &mut r.result.score;
        s.total_notes = 3;
        s.passed = 3;
        s.add(Judge::PGreat, 1);
        s.add(Judge::PGreat, 0);
        s.add(Judge::Poor, -1);
        r.result.clear = ClearType::Normal;
        r
    }

    #[test]
    fn a_play_is_frozen_with_its_version_2_fields() {
        let f = server()
            .freeze(&record(), "p1", Profile::VibeBeatoraja)
            .unwrap();
        assert_eq!(f.content_type, "application/json");
        let j: serde_json::Value = serde_json::from_str(&f.body).unwrap();
        assert_eq!(
            (&j["v"], &j["play_id"], &j["rule"]),
            (&2.into(), &"p1".into(), &"vibe-beatoraja".into())
        );
        assert_eq!(
            (&j["date"], &j["date_ms"]),
            (&1_767_225_600u64.into(), &1_767_225_600_123i64.into())
        );
        assert_eq!(
            (&j["epg"], &j["lpr"], &j["exscore"]),
            (&2.into(), &1.into(), &4.into())
        );
        assert_eq!((&j["option"], &j["seed"]), (&"RANDOM".into(), &99.into()));
        assert_eq!(j["client"], "vibeBMS 0.1.0");
        assert_eq!(j["lntype"], "LN");
        assert_eq!(j["devices"]["keyboard"], 5);
        // a version 1 field set is all still there
        let v1: ScoreSubmission = serde_json::from_str(&f.body).unwrap();
        assert_eq!(v1.clear, ClearType::Normal);
    }

    #[test]
    fn requests() {
        let ir = server();
        assert_eq!(ir.id(), ProviderId::VibeJson(1));
        let body = Frozen {
            content_type: "application/json".into(),
            body: "{}".into(),
        };
        let submit = Op::Submit {
            body: &body,
            key: "k1",
            user_intent: false,
        };
        assert_eq!(ir.request(&submit, None), Err(IrError::NoCredentials));
        let r = ir.request(&submit, Some(&creds())).unwrap();
        assert_eq!(
            (r.method, r.url.as_str()),
            ("POST", "https://ir.example/api/scores")
        );
        let h = |name: &str| {
            r.headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(h("Authorization"), Some("Bearer tok"));
        assert_eq!(h("Idempotency-Key"), Some("k1"));
        assert_eq!(h("Content-Type"), Some("application/json"));
        assert_eq!(r.body, b"{}");

        let chart = ChartId {
            sha256: "ab".into(),
            ln: LnType::Hcn,
            ..Default::default()
        };
        let rank = Op::Ranking {
            chart: &chart,
            view: "",
            limit: 20,
            etag: Some("\"e1\""),
        };
        let r = ir.request(&rank, None).unwrap();
        assert_eq!(
            r.url,
            "https://ir.example/api/ranking?sha256=ab&md5=&lntype=HCN&limit=20"
        );
        assert!(r
            .headers
            .contains(&("If-None-Match".into(), "\"e1\"".into())));
        assert!(!r.headers.iter().any(|(k, _)| k == "Authorization"));
        let lamps = Op::PlayerLamps {
            player: "a b",
            scope: "",
            view: "",
            etag: None,
        };
        assert_eq!(
            ir.request(&lamps, None).unwrap().url,
            "https://ir.example/api/players/a%20b/lamps"
        );
        assert_eq!(
            ir.request(&Op::Verify, Some(&creds())).unwrap().url,
            "https://ir.example/api/me"
        );
        assert_eq!(
            ir.request(&Op::Poll { remote: "x" }, Some(&creds())),
            Err(IrError::NotOffered)
        );
        // no request prints its token
        for op in [submit, rank, lamps, Op::Verify, Op::Rivals] {
            let shown = format!("{:?}", ir.request(&op, Some(&creds())).unwrap());
            assert!(!shown.contains("tok"), "{shown}");
        }
    }

    #[test]
    fn answers() {
        let ir = server();
        let body = Frozen {
            content_type: "application/json".into(),
            body: "{}".into(),
        };
        let submit = Op::Submit {
            body: &body,
            key: "k",
            user_intent: false,
        };
        let sent = Outcome::Sent {
            remote: None,
            note: None,
        };
        assert_eq!(ir.parse(&submit, &resp(201, "")), sent);
        assert_eq!(ir.parse(&submit, &resp(409, "")), sent);
        assert_eq!(
            ir.parse(&submit, &resp(400, r#"{"error":"unknown chart"}"#)),
            Outcome::Rejected("unknown chart".into())
        );
        assert_eq!(
            ir.parse(&submit, &resp(401, "")),
            Outcome::Auth("HTTP 401".into())
        );
        let mut busy = resp(503, "down");
        busy.headers.push(("Retry-After".into(), "30".into()));
        assert_eq!(
            ir.parse(&submit, &busy),
            Outcome::Retry {
                after: Some(RetryAfter::Secs(30)),
                why: "down".into()
            }
        );
        assert_eq!(
            ir.parse(
                &Op::Verify,
                &resp(200, r#"{"id":"7","name":"me","can_submit":true}"#)
            ),
            Outcome::Account(AccountInfo {
                id: "7".into(),
                name: "me".into(),
                can_submit: true,
                chart_page: String::new(),
            })
        );
        // (version 2: with its chart pages)
        let page = r#"{"id":"7","name":"me","can_submit":true,
            "chart_page":"https://ir.example/charts/{sha256}"}"#;
        let Outcome::Account(info) = ir.parse(&Op::Verify, &resp(200, page)) else {
            panic!()
        };
        assert_eq!(info.chart_page, "https://ir.example/charts/{sha256}");
        assert_eq!(ir.parse(&Op::Verify, &resp(404, "")), Outcome::Unsupported);
        assert_eq!(ir.parse(&Op::Rivals, &resp(404, "")), Outcome::Unsupported);
        assert_eq!(
            ir.parse(&Op::Rivals, &resp(200, r#"[{"id":"2","name":"r"}]"#)),
            Outcome::Rivals(vec![PlayerRef {
                id: "2".into(),
                name: "r".into()
            }])
        );
        let lamps = Op::PlayerLamps {
            player: "7",
            scope: "",
            view: "",
            etag: None,
        };
        let Outcome::Lamps(set) = ir.parse(
            &lamps,
            &resp(
                200,
                r#"{"player_name":"me","lamps":[{"sha256":"AA","clear":"Hard","ex_score":10},{"md5":"bb","clear":"Easy"}]}"#,
            ),
        ) else {
            panic!()
        };
        assert_eq!(set.get("", "aa").unwrap().ex, Some(10));
        assert_eq!(set.get("BB", "").unwrap().clear, ClearType::Easy);
        assert_eq!(set.etag.as_deref(), Some("\"e1\""));
    }

    /// A version 1 server's ranking (the array docs/IR_PROTOCOL.md showed),
    /// and a version 2 one.
    #[test]
    fn rankings_of_both_versions() {
        let ir = server();
        let chart = ChartId::default();
        let op = Op::Ranking {
            chart: &chart,
            view: "",
            limit: 0,
            etag: None,
        };
        let row = r#"{"rank": 1, "player": "name", "player_id": "123", "clear": "Hard", "ex_score": 1800,
 "pgreat": 800, "great": 200, "max_combo": 999, "min_bp": 3, "notes": 1000}"#;
        let Outcome::Ranking(v1) = ir.parse(&op, &resp(200, &format!("[{row}]"))) else {
            panic!()
        };
        assert_eq!(v1.players, 1);
        let e = &v1.entries[0];
        assert_eq!(
            (e.rank, e.player.as_str(), e.clear, e.ex, e.bp),
            (1, "name", ClearType::Hard, 1800, Some(3))
        );
        let v2 = format!(r#"{{"players": 345, "entries": [{row}]}}"#);
        let Outcome::Ranking(v2) = ir.parse(&op, &resp(200, &v2)) else {
            panic!()
        };
        assert_eq!((v2.players, v2.entries.len()), (345, 1));
        assert_eq!(ir.parse(&op, &resp(304, "")), Outcome::NotModified);
        assert!(matches!(
            ir.parse(&op, &resp(200, "<html>")),
            Outcome::Rejected(_)
        ));
    }

    #[test]
    fn submission_from_score() {
        let mut s = Score {
            total_notes: 3,
            passed: 3,
            ..Default::default()
        };
        s.add(Judge::PGreat, 1);
        s.add(Judge::PGreat, -1);
        s.add(Judge::Poor, -1);
        let sub = ScoreSubmission::from_score(
            &ChartRef {
                sha256: "x".into(),
                ..Default::default()
            },
            &s,
            ClearType::Normal,
            7,
        );
        assert_eq!((sub.epg, sub.lpg, sub.lpr, sub.exscore), (1, 1, 1, 4));
        let j = serde_json::to_value(&sub).unwrap();
        assert_eq!(j["clear"], "Normal");
    }

    /// A server's chart page, as its `/me` gave it; none without one.
    #[test]
    fn chart_pages() {
        let chart = ChartId {
            sha256: "ab".repeat(32),
            md5: "cd".repeat(16),
            ..Default::default()
        };
        assert_eq!(server().chart_page(&chart, ""), Err(IrError::NotOffered));
        let ir = server().with_chart_page(" https://ir.example/charts/{sha256}?md5={md5} ");
        assert_eq!(
            ir.chart_page(&chart, "").unwrap(),
            format!(
                "https://ir.example/charts/{}?md5={}",
                "ab".repeat(32),
                "cd".repeat(16)
            )
        );
        // a bmson has no MD5
        let ir = server().with_chart_page("https://ir.example/c/{md5}");
        assert_eq!(
            ir.chart_page(&ChartId::default(), ""),
            Err(IrError::NoChartId)
        );
        // only a web address
        let ir = server().with_chart_page("javascript:alert({sha256})");
        assert_eq!(ir.chart_page(&chart, ""), Err(IrError::NotOffered));
    }
}
