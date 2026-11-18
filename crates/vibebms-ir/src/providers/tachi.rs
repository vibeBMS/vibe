//! Bokutachi (Tachi): scores by Direct Manual, PB lists for lamps, chart
//! leaderboards. From Tachi's public docs and game configs:
//!
//! - `POST /ir/direct-manual/import` takes a BATCH-MANUAL document with
//!   an API key holding `submit_score`. It answers 200 with the import
//!   document, or 202 with `{url, importID}`: then poll
//!   `GET /api/v1/imports/:importID/poll-status` until `importStatus` is
//!   `completed`. `X-User-Intent: true` only when the user pressed a
//!   button for the import.
//! - Every answer is `{success, description, body}`; `description` is for
//!   people only.
//! - `GET /api/v1/status` says who the key is (`whoami`) and what it may
//!   do (`permissions`).
//! - `GET /api/v1/users/:userID/games/:game/:playtype/pbs/all` lists a
//!   player's PBs with their charts (hashes, Tachi's `chartID`);
//!   `GET /api/v1/games/:game/:playtype/charts/:chartID/pbs` is a chart's
//!   leaderboard.
//! - The same score imported twice is one score (Tachi's score ids are
//!   hashes of the score), so sending a frozen body again is safe.
//! - `bms` `7K`/`14K` and `pms` `Keyboard`/`Controller`; judgements
//!   `pgreat` (`cool` in PMS), `great`, `good`, `bad`, `poor`; `scoreMeta`
//!   is a strict schema: BMS takes `random`, `inputDevice`, `client` and
//!   `gauge`, PMS `random`, `client` and `gauge`. `client` admits only
//!   `lr2oraja`/`LR2` (BMS) and `beatoraja` (PMS), which vibeBMS is not: it
//!   is left out.
//! - 500 requests a minute.

use crate::eligibility::{device_of, Device};
use crate::http::{enc, HttpRequest, HttpResponse};
use crate::lamps::{tachi_lamp, to_tachi_lamp, LampSet, RankEntry, RankView, RemoteLamp};
use crate::provider::{
    by_status, need, AccountInfo, AuthKind, Avail, Caps, ChartKeys, Creds, Frozen, IrError, Op,
    Outcome, Provider, ProviderId, Rate,
};
use crate::rule::Profile;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use vibebms_core::gauge::GaugeType;
use vibebms_core::judge::Judge;
use vibebms_core::lane_option::LaneOption;
use vibebms_core::record::{Platform, PlayRecord};
use vibebms_core::PlayMode;

const BASE: &str = "https://boku.tachi.ac";
/// `meta.service`: 2 to 15 characters.
pub const SERVICE: &str = "vibeBMS";

pub struct Tachi;

/// Tachi's game and playtype of a play in `mode` (a PMS play's by its
/// device), `None`: Tachi has none (5 and 10 keys).
fn playtype(mode: PlayMode, pms: Option<Device>) -> Option<(&'static str, &'static str)> {
    Some(match mode {
        PlayMode::Beat7K => ("bms", "7K"),
        PlayMode::Beat14K => ("bms", "14K"),
        PlayMode::Popn9K => match pms? {
            Device::Keyboard => ("pms", "Keyboard"),
            Device::Controller => ("pms", "Controller"),
        },
        PlayMode::Beat5K | PlayMode::Beat10K => return None,
    })
}

/// The playtypes a player's lamps are read for, one `scope` each.
pub const PLAYTYPES: [&str; 4] = ["7K", "14K", "Keyboard", "Controller"];

/// The game of a playtype (`scope` of a lamp request).
fn game_of(playtype: &str) -> Option<&'static str> {
    match playtype {
        "7K" | "14K" => Some("bms"),
        "Keyboard" | "Controller" => Some("pms"),
        _ => None,
    }
}

fn random(o: LaneOption) -> Option<&'static str> {
    match o {
        LaneOption::Off => Some("NONRAN"),
        LaneOption::Mirror => Some("MIRROR"),
        LaneOption::Random => Some("RANDOM"),
        LaneOption::RRandom => Some("R-RANDOM"),
        LaneOption::SRandom => Some("S-RANDOM"),
        // a lane order drawn at random, of a half each (Tachi has no
        // name of its own for them)
        LaneOption::FRandom | LaneOption::MfRandom => Some("RANDOM"),
        _ => None,
    }
}

fn gauge(g: GaugeType) -> Option<&'static str> {
    match g {
        GaugeType::Easy => Some("EASY"),
        GaugeType::Normal => Some("NORMAL"),
        GaugeType::Hard => Some("HARD"),
        GaugeType::ExHard => Some("EX-HARD"),
        _ => None,
    }
}

/// The BATCH-MANUAL score of `rec`.
fn score(rec: &PlayRecord, game: &str) -> Result<Value, IrError> {
    let (res, rules) = (&rec.result, &rec.rules);
    let s = &res.score;
    let lamp = to_tachi_lamp(res.clear)
        .ok_or_else(|| IrError::Unsendable(format!("lamp {}", res.clear.label())))?;
    // early / late as the e*/l* fields count them: a hit with no timing
    // error is early; POOR includes empty POOR
    let side = |j: Judge, late: usize| {
        let j = j as usize;
        s.counts[j][late] + if late == 0 { s.just[j] } else { 0 }
    };
    let el = |j: Judge| [side(j, 0), side(j, 1)];
    let [epg, lpg] = el(Judge::PGreat);
    let [egr, lgr] = el(Judge::Great);
    let [egd, lgd] = el(Judge::Good);
    let [ebd, lbd] = el(Judge::Bad);
    let [epr, lpr] = [0, 1].map(|late| side(Judge::Poor, late) + side(Judge::Miss, late));
    let mut judgements = json!({
        "great": egr + lgr, "good": egd + lgd, "bad": ebd + lbd, "poor": epr + lpr,
    });
    judgements[if game == "pms" { "cool" } else { "pgreat" }] = (epg + lpg).into();

    let mut optional = json!({
        "fast": s.fast(), "slow": s.slow(), "maxCombo": s.max_combo, "bp": s.miss_count(),
        "gauge": res.gauge_value,
        "epg": epg, "lpg": lpg, "egr": egr, "lgr": lgr, "egd": egd, "lgd": lgd,
        "ebd": ebd, "lbd": lbd, "epr": epr, "lpr": lpr,
    });
    if !res.gauge_log.is_empty() {
        // a survival gauge's history is null from where it emptied
        let survival = res.gauge >= GaugeType::Hard;
        let mut dead = false;
        let history: Vec<Value> = res
            .gauge_log
            .iter()
            .map(|&v| {
                let was_dead = dead;
                dead |= survival && v <= 0.0;
                if was_dead {
                    Value::Null
                } else {
                    v.into()
                }
            })
            .collect();
        optional["gaugeHistory"] = history.into();
    }

    let mut meta = Map::new();
    let lanes: Option<Vec<&str>> = rules.assist.lane.iter().map(|&o| random(o)).collect();
    match lanes.as_deref() {
        Some([one]) if rec.chart.mode != PlayMode::Beat14K => {
            meta.insert("random".into(), (*one).into());
        }
        Some(&[p1, p2]) if rec.chart.mode == PlayMode::Beat14K => {
            meta.insert("random".into(), json!([p1, p2]));
        }
        _ => {}
    }
    if game == "bms" {
        let device = match device_of(&rec.device) {
            Some(Device::Keyboard) => Some("KEYBOARD"),
            Some(Device::Controller) => Some("BM_CONTROLLER"),
            None => None,
        };
        if let Some(d) = device {
            meta.insert("inputDevice".into(), d.into());
        }
    }
    if let Some(g) = gauge(res.gauge) {
        meta.insert("gauge".into(), g.into());
    }

    let identifier = [&rec.chart.sha256, &rec.chart.md5]
        .into_iter()
        .find(|h| !h.is_empty())
        .ok_or_else(|| IrError::Unsendable("no chart hash".into()))?;
    Ok(json!({
        "score": s.ex_score(),
        "lamp": lamp,
        "matchType": "bmsChartHash",
        "identifier": identifier,
        "timeAchieved": rec.ended_ms,
        "judgements": judgements,
        "optional": optional,
        "scoreMeta": meta,
    }))
}

impl Provider for Tachi {
    fn id(&self) -> ProviderId {
        ProviderId::Bokutachi
    }

    fn caps(&self, _on: Platform) -> Caps {
        Caps {
            submit: Avail::Yes,
            ranking: Avail::Yes,
            chart_info: Avail::No,
            player_lamps: Avail::Yes,
            // the rival routes are not in Tachi's API docs
            rivals: Avail::No,
            chart_key: ChartKeys::Either,
            auth: AuthKind::Token,
            // the docs ask to stay well under 100 a minute
            rate: Rate {
                per_min: 30,
                burst: 5,
                concurrent: 2,
            },
        }
    }

    /// One play per import, so an import's failure is the play's.
    fn freeze(&self, rec: &PlayRecord, _: &str, _: Profile) -> Result<Frozen, IrError> {
        let (game, playtype) =
            playtype(rec.chart.mode, device_of(&rec.device)).ok_or_else(|| {
                IrError::Unsendable(format!(
                    "no Bokutachi playtype for {}",
                    rec.chart.mode.label()
                ))
            })?;
        let doc = json!({
            "meta": {"game": game, "playtype": playtype, "service": SERVICE},
            "scores": [score(rec, game)?],
        });
        Ok(Frozen {
            content_type: "application/json".into(),
            body: doc.to_string(),
        })
    }

    fn request(&self, op: &Op, creds: Option<&Creds>) -> Result<HttpRequest, IrError> {
        let get = |path: String| {
            let req = HttpRequest::new("GET", format!("{BASE}{path}"));
            match creds {
                // the docs ask for the key on every request
                Some(c) => req.bearer(&c.token),
                None => req,
            }
        };
        Ok(match *op {
            Op::Verify => {
                need(creds)?;
                get("/api/v1/status".into())
            }
            Op::Submit {
                body, user_intent, ..
            } => {
                let req = HttpRequest::new("POST", format!("{BASE}/ir/direct-manual/import"))
                    .bearer(&need(creds)?.token)
                    .json(&body.content_type, &body.body);
                if user_intent {
                    req.header("X-User-Intent", "true")
                } else {
                    req
                }
            }
            Op::Poll { remote } => {
                need(creds)?;
                get(format!("/api/v1/imports/{}/poll-status", enc(remote)))
            }
            Op::Ranking { chart, view, .. } => {
                let pms = match view {
                    "Controller" => Device::Controller,
                    _ => Device::Keyboard,
                };
                let (game, pt) = playtype(chart.mode, Some(pms)).ok_or(IrError::NotOffered)?;
                let id = chart.remote.as_deref().ok_or(IrError::NoChartId)?;
                get(format!("/api/v1/games/{game}/{pt}/charts/{}/pbs", enc(id)))
            }
            Op::PlayerLamps { player, scope, .. } => {
                let game = game_of(scope)
                    .ok_or_else(|| IrError::Unsendable(format!("no playtype {scope:?}")))?;
                get(format!(
                    "/api/v1/users/{}/games/{game}/{scope}/pbs/all",
                    enc(player)
                ))
            }
            _ => return Err(IrError::NotOffered),
        })
    }

    fn parse(&self, op: &Op, resp: &HttpResponse) -> Outcome {
        if let Some(o) = by_status(resp, || description(resp), &[]) {
            return o;
        }
        let read = match (op, resp.status) {
            (Op::Submit { .. }, 202) => body(resp).map(|b: Queued| Outcome::Deferred {
                remote: b.import_id,
            }),
            (_, 200..=299) => read(op, resp),
            _ => return Outcome::Rejected(description(resp)),
        };
        read.unwrap_or_else(|e| Outcome::Rejected(format!("bad response: {e}")))
    }
}

/// The envelope's `description`, else the status.
fn description(resp: &HttpResponse) -> String {
    #[derive(Deserialize)]
    struct Failed {
        description: String,
    }
    serde_json::from_slice::<Failed>(&resp.body)
        .map(|f| f.description)
        .unwrap_or_else(|_| format!("HTTP {}", resp.status))
}

/// The envelope's `body` as `T`.
fn body<T: DeserializeOwned>(resp: &HttpResponse) -> Result<T, serde_json::Error> {
    #[derive(Deserialize)]
    struct Envelope<T> {
        body: T,
    }
    serde_json::from_slice::<Envelope<T>>(&resp.body).map(|e| e.body)
}

fn read(op: &Op, resp: &HttpResponse) -> Result<Outcome, serde_json::Error> {
    Ok(match op {
        Op::Verify => {
            let s: Status = body(resp)?;
            match s.whoami {
                Some(id) => Outcome::Account(AccountInfo {
                    id: id.to_string(),
                    name: String::new(),
                    // the docs' example spells it score_submit
                    can_submit: s
                        .permissions
                        .iter()
                        .any(|p| p == "submit_score" || p == "score_submit"),
                }),
                None => Outcome::Auth("Bokutachi does not know this key".into()),
            }
        }
        Op::Submit { .. } => imported(body(resp)?),
        Op::Poll { .. } => {
            let p: PollStatus = body(resp)?;
            match (p.import_status.as_str(), p.import) {
                ("completed", Some(doc)) => imported(doc),
                _ => Outcome::Pending,
            }
        }
        Op::Ranking { .. } => {
            let b: ChartPbs = body(resp)?;
            let names: HashMap<u64, String> =
                b.users.into_iter().map(|u| (u.id, u.username)).collect();
            let players = b.pbs.first().map_or(0, |p| p.ranking_data.out_of);
            let entries = b
                .pbs
                .into_iter()
                .map(|p| RankEntry {
                    rank: p.ranking_data.rank,
                    player: names.get(&p.user_id).cloned().unwrap_or_default(),
                    player_id: p.user_id.to_string(),
                    clear: tachi_lamp(&p.score_data.lamp).unwrap_or_default(),
                    ex: p.score_data.score as u32,
                    bp: p.score_data.optional.bp,
                    max_combo: p.score_data.optional.max_combo,
                    notes: None,
                    client: None,
                    at_ms: p.time_achieved,
                })
                .collect();
            Outcome::Ranking(RankView {
                players,
                entries,
                etag: None,
            })
        }
        Op::PlayerLamps { .. } => {
            let b: Pbs = body(resp)?;
            let charts: HashMap<&str, &ChartData> = b
                .charts
                .iter()
                .map(|c| (c.chart_id.as_str(), &c.data))
                .collect();
            let mut set = LampSet::default();
            for pb in &b.pbs {
                let (Some(clear), Some(c)) = (
                    tachi_lamp(&pb.score_data.lamp),
                    charts.get(pb.chart_id.as_str()),
                ) else {
                    continue;
                };
                let lamp = RemoteLamp {
                    clear,
                    ex: Some(pb.score_data.score as u32),
                    bp: pb.score_data.optional.bp,
                    at_ms: pb.time_achieved,
                    remote: Some(pb.chart_id.clone()),
                };
                let hash = if c.hash_sha256.is_empty() {
                    &c.hash_md5
                } else {
                    &c.hash_sha256
                };
                set.insert(hash, lamp);
            }
            Outcome::Lamps(set)
        }
        _ => Outcome::Rejected("not offered".into()),
    })
}

/// What an import did with the one score it had.
fn imported(doc: ImportDoc) -> Outcome {
    if let Some(id) = doc.score_ids.into_iter().next() {
        return Outcome::Sent {
            remote: Some(id),
            note: None,
        };
    }
    match doc.errors.into_iter().next() {
        // kept as an orphan, imported when Tachi learns the chart
        Some(e) if e.kind == "SongOrChartNotFound" => Outcome::Sent {
            remote: None,
            note: Some("chart not on Bokutachi yet".into()),
        },
        Some(e) => Outcome::Rejected(e.message.unwrap_or(e.kind)),
        // nothing new: Tachi had this score already
        None => Outcome::Sent {
            remote: None,
            note: Some("already on Bokutachi".into()),
        },
    }
}

#[derive(Deserialize)]
struct Status {
    whoami: Option<u64>,
    #[serde(default)]
    permissions: Vec<String>,
}

#[derive(Deserialize)]
struct Queued {
    #[serde(rename = "importID")]
    import_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PollStatus {
    import_status: String,
    import: Option<ImportDoc>,
}

#[derive(Deserialize)]
struct ImportDoc {
    #[serde(rename = "scoreIDs", default)]
    score_ids: Vec<String>,
    #[serde(default)]
    errors: Vec<ImportError>,
}

#[derive(Deserialize)]
struct ImportError {
    #[serde(rename = "type")]
    kind: String,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pb {
    #[serde(rename = "chartID", default)]
    chart_id: String,
    #[serde(rename = "userID", default)]
    user_id: u64,
    time_achieved: Option<i64>,
    score_data: ScoreData,
    #[serde(default)]
    ranking_data: RankingData,
}

#[derive(Deserialize)]
struct ScoreData {
    score: f64,
    lamp: String,
    #[serde(default)]
    optional: Optional,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Optional {
    bp: Option<u32>,
    max_combo: Option<u32>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RankingData {
    rank: u32,
    out_of: u32,
}

#[derive(Deserialize)]
struct Pbs {
    pbs: Vec<Pb>,
    charts: Vec<Chart>,
}

#[derive(Deserialize)]
struct Chart {
    #[serde(rename = "chartID")]
    chart_id: String,
    data: ChartData,
}

#[derive(Deserialize)]
struct ChartData {
    #[serde(rename = "hashMD5", default)]
    hash_md5: String,
    #[serde(rename = "hashSHA256", default)]
    hash_sha256: String,
}

#[derive(Deserialize)]
struct ChartPbs {
    pbs: Vec<Pb>,
    users: Vec<User>,
}

#[derive(Deserialize)]
struct User {
    id: u64,
    username: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Secret;
    use crate::outbox::{Item, Outbox, Reply, State};
    use crate::provider::ChartId;
    use vibebms_core::record::{ClientId, VERSION};
    use vibebms_core::score::ClearType;

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/tachi/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(path).unwrap()
    }

    fn resp(status: u16, name: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: Vec::new(),
            body: fixture(name),
        }
    }

    fn creds() -> Creds {
        Creds::token(Secret::new("tachi-key"))
    }

    /// A 7-key play under LR2 rules, HARD CLEAR, on a keyboard.
    fn play() -> PlayRecord {
        let mut r = PlayRecord {
            v: VERSION,
            client: ClientId {
                name: "vibeBMS".into(),
                ..Default::default()
            },
            ended_ms: 1_767_225_600_123,
            ..Default::default()
        };
        r.chart.sha256 = "ab".repeat(32);
        r.chart.md5 = "cd".repeat(16);
        r.chart.mode = PlayMode::Beat7K;
        r.rules.assist.lane = vec![LaneOption::Random];
        r.device.keyboard = 918;
        let res = &mut r.result;
        res.clear = ClearType::Hard;
        res.gauge = GaugeType::Hard;
        res.gauge_value = 84.0;
        res.gauge_log = vec![100.0, 50.0, 84.0];
        let s = &mut res.score;
        s.counts[Judge::PGreat as usize] = [390, 380];
        s.just[Judge::PGreat as usize] = 10;
        s.counts[Judge::Great as usize] = [50, 80];
        s.counts[Judge::Good as usize] = [3, 2];
        s.counts[Judge::Bad as usize] = [1, 0];
        s.counts[Judge::Poor as usize] = [0, 1];
        s.counts[Judge::Miss as usize] = [0, 1];
        s.max_combo = 500;
        s.total_notes = 917;
        s.passed = 917;
        r
    }

    fn golden(name: &str) -> Value {
        serde_json::from_slice(&fixture(name)).unwrap()
    }

    #[test]
    fn a_play_frozen_as_batch_manual() {
        let f = Tachi.freeze(&play(), "p", Profile::VibeLr2).unwrap();
        assert_eq!(f.content_type, "application/json");
        let got: Value = serde_json::from_str(&f.body).unwrap();
        assert_eq!(got, golden("submit_7k.json"));

        // PMS on a controller: its playtype, `cool`, no inputDevice
        let mut pms = play();
        pms.chart.mode = PlayMode::Popn9K;
        pms.device.keyboard = 0;
        pms.device.gamepad = 918;
        pms.result.clear = ClearType::Max;
        let f = Tachi.freeze(&pms, "p", Profile::VibeBeatoraja).unwrap();
        let got: Value = serde_json::from_str(&f.body).unwrap();
        assert_eq!(got, golden("submit_pms.json"));

        // DP: the random of both sides
        let mut dp = play();
        dp.chart.mode = PlayMode::Beat14K;
        dp.rules.assist.lane = vec![LaneOption::Mirror, LaneOption::Off];
        let got: Value =
            serde_json::from_str(&Tachi.freeze(&dp, "p", Profile::VibeLr2).unwrap().body).unwrap();
        assert_eq!(got["meta"]["playtype"], "14K");
        assert_eq!(
            got["scores"][0]["scoreMeta"]["random"],
            json!(["MIRROR", "NONRAN"])
        );
    }

    #[test]
    fn what_cannot_be_sent() {
        let mut five = play();
        five.chart.mode = PlayMode::Beat5K;
        assert!(Tachi.freeze(&five, "p", Profile::VibeLr2).is_err());
        let mut mixed = play();
        mixed.chart.mode = PlayMode::Popn9K;
        mixed.device.gamepad = 1;
        assert!(Tachi.freeze(&mixed, "p", Profile::VibeBeatoraja).is_err());
        let mut assisted = play();
        assisted.result.clear = ClearType::AssistEasy;
        assert!(Tachi.freeze(&assisted, "p", Profile::VibeLr2).is_err());
        // a survival gauge's history after it emptied is null
        let mut failed = play();
        failed.result.clear = ClearType::Failed;
        failed.result.gauge_log = vec![100.0, 0.0, 0.0];
        let got: Value =
            serde_json::from_str(&Tachi.freeze(&failed, "p", Profile::VibeLr2).unwrap().body)
                .unwrap();
        let s = &got["scores"][0];
        assert_eq!(s["lamp"], "FAILED");
        assert_eq!(s["optional"]["gaugeHistory"], json!([100.0, 0.0, null]));
    }

    #[test]
    fn requests() {
        let body = Tachi.freeze(&play(), "p", Profile::VibeLr2).unwrap();
        let submit = |user_intent| Op::Submit {
            body: &body,
            key: "k",
            user_intent,
        };
        assert_eq!(
            Tachi.request(&submit(false), None),
            Err(IrError::NoCredentials)
        );
        let r = Tachi.request(&submit(true), Some(&creds())).unwrap();
        assert_eq!(
            (r.method, r.url.as_str()),
            ("POST", "https://boku.tachi.ac/ir/direct-manual/import")
        );
        assert_eq!(r.body, body.body.as_bytes());
        let has = |r: &HttpRequest, k: &str, v: &str| r.headers.contains(&(k.into(), v.into()));
        assert!(has(&r, "Authorization", "Bearer tachi-key"));
        assert!(has(&r, "Content-Type", "application/json"));
        assert!(has(&r, "X-User-Intent", "true"));
        let r = Tachi.request(&submit(false), Some(&creds())).unwrap();
        assert!(!r.headers.iter().any(|(k, _)| k == "X-User-Intent"));

        let url = |op: &Op| Tachi.request(op, Some(&creds())).unwrap().url;
        assert_eq!(url(&Op::Verify), "https://boku.tachi.ac/api/v1/status");
        assert_eq!(
            url(&Op::Poll { remote: "imp1" }),
            "https://boku.tachi.ac/api/v1/imports/imp1/poll-status"
        );
        let lamps = |scope| Op::PlayerLamps {
            player: "1",
            scope,
            view: "",
            etag: None,
        };
        assert_eq!(
            url(&lamps("14K")),
            "https://boku.tachi.ac/api/v1/users/1/games/bms/14K/pbs/all"
        );
        assert_eq!(
            url(&lamps("Controller")),
            "https://boku.tachi.ac/api/v1/users/1/games/pms/Controller/pbs/all"
        );
        assert!(Tachi.request(&lamps("5K"), None).is_err());
        let mut chart = ChartId {
            mode: PlayMode::Popn9K,
            ..Default::default()
        };
        fn rank(chart: &ChartId) -> Op<'_> {
            Op::Ranking {
                chart,
                view: "Controller",
                limit: 0,
                etag: None,
            }
        }
        assert_eq!(Tachi.request(&rank(&chart), None), Err(IrError::NoChartId));
        chart.remote = Some("c1".into());
        // read routes need no key
        let r = Tachi.request(&rank(&chart), None).unwrap();
        assert_eq!(
            r.url,
            "https://boku.tachi.ac/api/v1/games/pms/Controller/charts/c1/pbs"
        );
        assert!(!r.headers.iter().any(|(k, _)| k == "Authorization"));
        assert_eq!(
            Tachi.request(&Op::Rivals, Some(&creds())),
            Err(IrError::NotOffered)
        );
        // no request prints the key
        for op in [
            submit(true),
            Op::Verify,
            Op::Poll { remote: "i" },
            lamps("7K"),
        ] {
            let shown = format!("{:?}", Tachi.request(&op, Some(&creds())).unwrap());
            assert!(!shown.contains("tachi-key"), "{shown}");
        }
    }

    #[test]
    fn a_key_is_checked() {
        let v = Tachi.parse(&Op::Verify, &resp(200, "status.json"));
        assert_eq!(
            v,
            Outcome::Account(AccountInfo {
                id: "1".into(),
                name: String::new(),
                can_submit: true
            })
        );
        assert!(matches!(
            Tachi.parse(&Op::Verify, &resp(200, "status_anonymous.json")),
            Outcome::Auth(_)
        ));
        assert_eq!(
            Tachi.parse(&Op::Verify, &resp(401, "error_401.json")),
            Outcome::Auth("This endpoint requires the submit_score permission.".into())
        );
    }

    /// A play through the outbox against canned answers: 202, polls, done.
    #[test]
    fn submit_then_poll() {
        let t0 = 1_767_225_700_000;
        let mut o = Outbox::default();
        let item = Item::freeze(&Tachi, "1", &play(), "p", Profile::VibeLr2, false, t0).unwrap();
        let id = item.id.clone();
        o.push(item);
        let step = |o: &mut Outbox, now: i64, status: u16, name: &str| {
            assert_eq!(o.next(now, |_| true), [id.as_str()]);
            let i = o.item(&id).unwrap();
            let req = Tachi.request(&i.op(), Some(&creds())).unwrap();
            let outcome = Tachi.parse(&i.op(), &resp(status, name));
            o.settle(&id, Reply::Answer { status, outcome }, now);
            req
        };
        let posted = step(&mut o, t0, 202, "import_202.json");
        assert_eq!(posted.method, "POST");
        let i = o.item(&id).unwrap();
        assert_eq!(i.state, State::Polling);
        assert_eq!(
            i.remote.as_deref(),
            Some("bea63277b54a5846bab1fd3a6ce54bfd41276857")
        );
        let polled = step(&mut o, t0 + 1000, 200, "poll_ongoing.json");
        assert!(polled.url.ends_with("/poll-status"));
        assert_eq!(o.item(&id).unwrap().state, State::Polling);
        step(&mut o, t0 + 3000, 200, "poll_completed.json");
        assert_eq!(o.item(&id).unwrap().state, State::Done);
    }

    #[test]
    fn what_imports_end_as() {
        let poll = Op::Poll { remote: "i" };
        let score = "R6fad5f4947454d8238b45d6a1255d63be4da1130bf91fe9d05df29765a148da8";
        assert_eq!(
            Tachi.parse(&poll, &resp(200, "poll_completed.json")),
            Outcome::Sent {
                remote: Some(score.into()),
                note: None
            }
        );
        assert_eq!(
            Tachi.parse(&poll, &resp(200, "poll_orphan.json")),
            Outcome::Sent {
                remote: None,
                note: Some("chart not on Bokutachi yet".into())
            }
        );
        assert_eq!(
            Tachi.parse(&poll, &resp(200, "poll_duplicate.json")),
            Outcome::Sent {
                remote: None,
                note: Some("already on Bokutachi".into())
            }
        );
        assert_eq!(
            Tachi.parse(&poll, &resp(200, "poll_invalid.json")),
            Outcome::Rejected(
                "Invalid score of 9999, expected a number between 0 and 1800.".into()
            )
        );
        assert_eq!(
            Tachi.parse(&poll, &resp(200, "poll_ongoing.json")),
            Outcome::Pending
        );
        // an import done at once (no score import workers)
        let body = Frozen {
            content_type: String::new(),
            body: String::new(),
        };
        let submit = Op::Submit {
            body: &body,
            key: "k",
            user_intent: false,
        };
        assert_eq!(
            Tachi.parse(&submit, &resp(200, "import_200.json")),
            Outcome::Sent {
                remote: Some(score.into()),
                note: None
            }
        );
        let bad = HttpResponse {
            status: 400,
            headers: Vec::new(),
            body: br#"{"success":false,"description":"Invalid BATCH-MANUAL."}"#.to_vec(),
        };
        assert_eq!(
            Tachi.parse(&submit, &bad),
            Outcome::Rejected("Invalid BATCH-MANUAL.".into())
        );
        let busy = HttpResponse {
            status: 429,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert_eq!(
            Tachi.parse(&submit, &busy),
            Outcome::Retry {
                after: None,
                why: "HTTP 429".into()
            }
        );
    }

    #[test]
    fn pbs_and_leaderboards() {
        let op = Op::PlayerLamps {
            player: "1",
            scope: "7K",
            view: "",
            etag: None,
        };
        let Outcome::Lamps(set) = Tachi.parse(&op, &resp(200, "pbs_all.json")) else {
            panic!()
        };
        let hard = set.get("", &"1".repeat(64)).unwrap();
        assert_eq!(
            (hard.clear, hard.ex, hard.bp, hard.at_ms),
            (
                ClearType::Hard,
                Some(1690),
                Some(4),
                Some(1_767_225_600_123)
            )
        );
        assert_eq!(hard.remote.as_deref(), Some("c7k1"));
        // a chart without its SHA-256: by MD5
        let failed = set.get("FEDCBA9876543210FEDCBA9876543210", "").unwrap();
        assert_eq!((failed.clear, failed.bp), (ClearType::Failed, None));

        let chart = ChartId {
            mode: PlayMode::Beat7K,
            remote: Some("c7k1".into()),
            ..Default::default()
        };
        let op = Op::Ranking {
            chart: &chart,
            view: "",
            limit: 0,
            etag: None,
        };
        let Outcome::Ranking(v) = Tachi.parse(&op, &resp(200, "chart_pbs.json")) else {
            panic!()
        };
        assert_eq!(v.players, 2);
        let e = &v.entries[1];
        assert_eq!(
            (
                e.rank,
                e.player.as_str(),
                e.player_id.as_str(),
                e.clear,
                e.ex
            ),
            (2, "player two", "2", ClearType::Normal, 1500)
        );
        assert_eq!(v.entries[0].max_combo, Some(500));
    }
}
