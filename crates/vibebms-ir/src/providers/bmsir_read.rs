//! BMS-IR's public read API (its wiki, "外部ツール向けの読み取りAPI"):
//! a chart's ranking, chart facts and a player's lamps on one difficulty
//! table, read without an account.
//!
//! - `GET /new/api/v1/public/{chart,ranking,player_lamps}.json`, `GET`
//!   only, JSON with `schema_version` 1 and `ok`; a failure is
//!   `{"ok": false, "error": "<code>"}` with its status.
//! - Charts by MD5 only: a bmson has none and cannot be asked for.
//! - `client_view` picks whose scores count (`lr2` when left out; `all`,
//!   `lr2oraja`, `beatoraja`, `bmz-player`, `all_clients`, ...).
//! - 120 requests a minute per address (`429`, `Retry-After: 60`), 8 at
//!   once for everyone (`503`, `Retry-After: 10`); answers are cached five
//!   minutes and carry an `ETag` (`If-None-Match` gives a `304`).
//! - `Access-Control-Allow-Origin: *`, but no `Access-Control-Expose-Headers`:
//!   a page reads neither `ETag` nor `Retry-After` (the documented waits
//!   apply), and an `If-None-Match` of its own would cost a preflight. On
//!   the web the app passes no `etag`; the browser's cache revalidates.

use crate::http::{enc, unix_ms, HttpRequest, HttpResponse};
use crate::lamps::{bmsir_lamp, ChartInfo, LampSet, RankEntry, RankView, RemoteLamp};
use crate::provider::{
    by_status, AuthKind, Avail, Caps, ChartKeys, Creds, Frozen, IrError, Op, Outcome, Provider,
    ProviderId, Rate,
};
use crate::rule::Profile;
use serde::Deserialize;
use serde_json::Value;
use vibebms_core::record::{Platform, PlayRecord};
use vibebms_core::score::ClearType;

const BASE: &str = "https://www.bms-ir.org/new/api/v1/public";
/// Charts per `chart.json` request.
pub const CHARTS_PER_REQUEST: usize = 50;
/// The documented waits, for when `Retry-After` cannot be read.
const WAITS: &[(u16, u64)] = &[(429, 60), (503, 10)];

pub struct BmsIrRead;

impl Provider for BmsIrRead {
    fn id(&self) -> ProviderId {
        ProviderId::BmsIr
    }

    fn caps(&self, _on: Platform) -> Caps {
        Caps {
            submit: Avail::No,
            ranking: Avail::Yes,
            chart_info: Avail::Yes,
            player_lamps: Avail::Yes,
            // rivals are player ids the user enters
            rivals: Avail::No,
            chart_key: ChartKeys::Md5Only,
            auth: AuthKind::None,
            // half the server's 120 a minute
            rate: Rate {
                per_min: 60,
                burst: 5,
                concurrent: 2,
            },
        }
    }

    fn freeze(&self, _: &PlayRecord, _: &str, _: Profile) -> Result<Frozen, IrError> {
        Err(IrError::NotOffered)
    }

    fn request(&self, op: &Op, _creds: Option<&Creds>) -> Result<HttpRequest, IrError> {
        let (path, etag) = match *op {
            Op::Ranking {
                chart,
                view,
                limit,
                etag,
            } => {
                if chart.md5.is_empty() {
                    return Err(IrError::NoChartId);
                }
                let mut q = format!("ranking.json?md5={}", enc(&chart.md5));
                if limit > 0 {
                    q += &format!("&limit={}", limit.min(100));
                }
                (q + &view_param(view), etag)
            }
            Op::ChartInfo { md5 } => {
                if md5.is_empty() || md5.len() > CHARTS_PER_REQUEST {
                    return Err(IrError::Unsendable(format!(
                        "1 to {CHARTS_PER_REQUEST} charts per request"
                    )));
                }
                let list: Vec<String> = md5.iter().map(|m| enc(m)).collect();
                (format!("chart.json?md5={}", list.join(",")), None)
            }
            Op::PlayerLamps {
                player,
                scope,
                view,
                etag,
            } => (
                format!(
                    "player_lamps.json?id={}&table={}{}",
                    enc(player),
                    enc(scope),
                    view_param(view)
                ),
                etag,
            ),
            _ => return Err(IrError::NotOffered),
        };
        let req = HttpRequest::new("GET", format!("{BASE}/{path}"));
        Ok(match etag {
            Some(t) => req.header("If-None-Match", t),
            None => req,
        })
    }

    fn parse(&self, op: &Op, resp: &HttpResponse) -> Outcome {
        if let Some(o) = by_status(resp, || error_code(resp), WAITS) {
            return o;
        }
        match (op, resp.status) {
            (_, 304) => Outcome::NotModified,
            // no score on the chart yet
            (Op::Ranking { .. }, 404) if error_code(resp) == "chart_not_found" => {
                Outcome::Ranking(RankView {
                    etag: resp.etag(),
                    ..Default::default()
                })
            }
            (_, 200) => match read(op, resp) {
                Ok(o) => o,
                Err(e) => Outcome::Rejected(format!("bad response: {e}")),
            },
            _ => Outcome::Rejected(error_code(resp)),
        }
    }
}

fn view_param(view: &str) -> String {
    if view.is_empty() {
        String::new()
    } else {
        format!("&client_view={}", enc(view))
    }
}

/// The error code of a failure (`player_not_public`, ...), else the status.
fn error_code(resp: &HttpResponse) -> String {
    #[derive(Deserialize)]
    struct Failure {
        error: String,
    }
    serde_json::from_slice::<Failure>(&resp.body)
        .map(|f| f.error)
        .unwrap_or_else(|_| format!("HTTP {}", resp.status))
}

fn read(op: &Op, resp: &HttpResponse) -> Result<Outcome, serde_json::Error> {
    let etag = resp.etag();
    Ok(match op {
        Op::Ranking { .. } => {
            let r: Ranking = serde_json::from_slice(&resp.body)?;
            Outcome::Ranking(RankView {
                players: r.chart.players,
                entries: r.entries.into_iter().map(Entry::entry).collect(),
                etag,
            })
        }
        Op::ChartInfo { .. } => {
            let r: Charts = serde_json::from_slice(&resp.body)?;
            let found = r.charts.into_iter().filter(|c| c.found);
            Outcome::Charts(found.map(Chart::info).collect())
        }
        Op::PlayerLamps { .. } => {
            let r: Lamps = serde_json::from_slice(&resp.body)?;
            let mut set = LampSet {
                player_name: r.player_name,
                etag,
                ..Default::default()
            };
            for c in r.charts {
                // an absent lamp is NO PLAY
                if let Some(clear) = bmsir_lamp(&c.lamp).filter(|&l| l != ClearType::NoPlay) {
                    let lamp = RemoteLamp {
                        clear,
                        ..Default::default()
                    };
                    set.insert(&c.md5, lamp);
                }
            }
            Outcome::Lamps(set)
        }
        _ => Outcome::Rejected("not offered".into()),
    })
}

#[derive(Deserialize)]
struct Charts {
    charts: Vec<Chart>,
}

#[derive(Deserialize)]
struct Chart {
    md5: String,
    #[serde(default)]
    found: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(default)]
    artist: String,
    /// A number in `chart.json`, text in `player_lamps.json`.
    #[serde(default)]
    level: Value,
    #[serde(default)]
    notes: u32,
    #[serde(default)]
    players: u32,
}

impl Chart {
    fn info(self) -> ChartInfo {
        ChartInfo {
            md5: self.md5,
            title: self.title,
            subtitle: self.subtitle,
            artist: self.artist,
            level: text(&self.level),
            notes: self.notes,
            players: self.players,
        }
    }
}

#[derive(Deserialize)]
struct Ranking {
    chart: Chart,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    rank: u32,
    player_id: Value,
    #[serde(default)]
    player_name: String,
    ex_score: u32,
    lamp: String,
    bp: Option<u32>,
    max_combo: Option<u32>,
    notes: Option<u32>,
    client: Option<String>,
    achieved_at: Option<String>,
}

impl Entry {
    fn entry(self) -> RankEntry {
        RankEntry {
            rank: self.rank,
            player: self.player_name,
            player_id: text(&self.player_id),
            clear: bmsir_lamp(&self.lamp).unwrap_or_default(),
            ex: self.ex_score,
            bp: self.bp,
            max_combo: self.max_combo,
            notes: self.notes,
            client: self.client,
            at_ms: self.achieved_at.as_deref().and_then(iso_ms),
        }
    }
}

#[derive(Deserialize)]
struct Lamps {
    #[serde(default)]
    player_name: String,
    charts: Vec<LampRow>,
}

#[derive(Deserialize)]
struct LampRow {
    md5: String,
    lamp: String,
}

/// A JSON number or string as text.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// `2026-09-20T23:33:29Z` as Unix ms.
fn iso_ms(s: &str) -> Option<i64> {
    let (date, time) = s.strip_suffix('Z')?.split_once('T')?;
    let mut d = date.split('-').map(|n| n.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let time = time.split('.').next()?;
    let mut t = time.split(':').map(|n| n.parse::<u32>().ok());
    let (h, mi, se) = (t.next()??, t.next()??, t.next()??);
    Some(unix_ms(y, mo as u32, da as u32, h, mi, se))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::RetryAfter;
    use crate::provider::ChartId;

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/bmsir/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap()
    }

    fn resp(status: u16, body: Vec<u8>) -> HttpResponse {
        HttpResponse {
            status,
            headers: vec![("etag".into(), "\"6a08856ca3f51b00b7025f3bbd84ddcf\"".into())],
            body,
        }
    }

    const MD5: &str = "dabc6c70ca4dac9f0060a17a9218a1af";

    fn chart() -> ChartId {
        ChartId {
            md5: MD5.into(),
            ..Default::default()
        }
    }

    #[test]
    fn requests() {
        let c = chart();
        let rank = Op::Ranking {
            chart: &c,
            view: "all_clients",
            limit: 3,
            etag: Some("\"e\""),
        };
        let r = BmsIrRead.request(&rank, None).unwrap();
        assert_eq!(
            (r.method, r.url.as_str()),
            (
                "GET",
                "https://www.bms-ir.org/new/api/v1/public/ranking.json?md5=dabc6c70ca4dac9f0060a17a9218a1af&limit=3&client_view=all_clients"
            )
        );
        assert_eq!(
            r.headers,
            [
                ("Accept".into(), "application/json".into()),
                ("If-None-Match".into(), "\"e\"".into())
            ]
        );
        // the server's default count and view
        let rank = Op::Ranking {
            chart: &c,
            view: "",
            limit: 0,
            etag: None,
        };
        assert!(BmsIrRead
            .request(&rank, None)
            .unwrap()
            .url
            .ends_with(&format!("ranking.json?md5={MD5}")));
        let bmson = ChartId::default();
        let rank = Op::Ranking {
            chart: &bmson,
            view: "",
            limit: 0,
            etag: None,
        };
        assert_eq!(BmsIrRead.request(&rank, None), Err(IrError::NoChartId));

        let md5 = ["a", "b"];
        let r = BmsIrRead.request(&Op::ChartInfo { md5: &md5 }, None);
        assert!(r.unwrap().url.ends_with("chart.json?md5=a,b"));
        let many = ["a"; 51];
        assert!(BmsIrRead
            .request(&Op::ChartInfo { md5: &many }, None)
            .is_err());

        let lamps = Op::PlayerLamps {
            player: "100002",
            scope: "satellite",
            view: "all_clients",
            etag: None,
        };
        assert!(BmsIrRead
            .request(&lamps, None)
            .unwrap()
            .url
            .ends_with("player_lamps.json?id=100002&table=satellite&client_view=all_clients"));
        assert_eq!(
            BmsIrRead.request(&Op::Verify, None),
            Err(IrError::NotOffered)
        );
        assert_eq!(
            BmsIrRead.freeze(&PlayRecord::default(), "p", Profile::VibeLr2),
            Err(IrError::NotOffered)
        );
    }

    #[test]
    fn a_ranking() {
        let c = chart();
        let op = Op::Ranking {
            chart: &c,
            view: "all_clients",
            limit: 3,
            etag: None,
        };
        let Outcome::Ranking(v) = BmsIrRead.parse(&op, &resp(200, fixture("ranking.json"))) else {
            panic!()
        };
        assert_eq!((v.players, v.entries.len()), (89, 3));
        assert_eq!(
            v.etag.as_deref(),
            Some("\"6a08856ca3f51b00b7025f3bbd84ddcf\"")
        );
        let e = &v.entries[0];
        assert_eq!(
            (e.rank, e.player.as_str(), e.player_id.as_str()),
            (1, "player one", "100001")
        );
        assert_eq!(
            (e.clear, e.ex, e.bp, e.max_combo, e.notes),
            (ClearType::Hard, 5124, Some(13), Some(2003), Some(2655))
        );
        assert_eq!(e.client.as_deref(), Some("lr2"));
        assert_eq!(e.at_ms, Some(unix_ms(2026, 9, 20, 23, 33, 29)));
        assert_eq!(v.entries[1].player, "プレイヤー");
        assert_eq!(v.entries[2].clear, ClearType::ExHard);
        // unchanged, and a chart nobody played
        assert_eq!(
            BmsIrRead.parse(&op, &resp(304, Vec::new())),
            Outcome::NotModified
        );
        let none = br#"{"schema_version":1,"ok":false,"error":"chart_not_found"}"#;
        let Outcome::Ranking(v) = BmsIrRead.parse(&op, &resp(404, none.to_vec())) else {
            panic!()
        };
        assert_eq!((v.players, v.entries.len()), (0, 0));
    }

    #[test]
    fn chart_facts() {
        let md5 = [MD5, "0123456789abcdef0123456789abcdef"];
        let op = Op::ChartInfo { md5: &md5 };
        let Outcome::Charts(c) = BmsIrRead.parse(&op, &resp(200, fixture("chart.json"))) else {
            panic!()
        };
        // the chart BMS-IR has no record of is left out
        assert_eq!(
            c,
            [ChartInfo {
                md5: MD5.into(),
                title: "duty".into(),
                subtitle: "[free]".into(),
                artist: "Hate / obj : cieu".into(),
                level: "25".into(),
                notes: 2655,
                players: 89,
            }]
        );
    }

    #[test]
    fn a_players_lamps() {
        let op = Op::PlayerLamps {
            player: "100002",
            scope: "satellite",
            view: "all_clients",
            etag: None,
        };
        let Outcome::Lamps(set) = BmsIrRead.parse(&op, &resp(200, fixture("player_lamps.json")))
        else {
            panic!()
        };
        assert_eq!(set.player_name, "プレイヤー");
        let lamp = |md5: &str| set.get(md5, "").map(|l| l.clear);
        assert_eq!(
            lamp("011d370c8a0d66bf3175a88609dca383"),
            Some(ClearType::FullCombo)
        );
        assert_eq!(
            lamp("02c4d238c7ab99ae088bae519d92dc06"),
            Some(ClearType::Perfect)
        );
        assert_eq!(
            lamp("031a08514d26511c2ccb714331bbdd2a"),
            Some(ClearType::LightAssistEasy)
        );
        assert_eq!(
            lamp("03e8c2316d4dad4653234a0b185f90e1"),
            Some(ClearType::Failed)
        );
        assert_eq!(lamp("01af4c0984ed654897885308bc10083d"), None);
        assert_eq!(set.by_chart.len(), 4);
        assert_eq!(
            BmsIrRead.parse(&op, &resp(404, fixture("error.json"))),
            Outcome::Rejected("player_not_public".into())
        );
    }

    #[test]
    fn limits_and_failures() {
        let op = Op::Verify;
        let mut busy = resp(
            503,
            br#"{"schema_version":1,"ok":false,"error":"busy"}"#.to_vec(),
        );
        assert_eq!(
            BmsIrRead.parse(&op, &busy),
            Outcome::Retry {
                after: Some(RetryAfter::Secs(10)),
                why: "busy".into()
            }
        );
        // unread on the web: the documented minute
        let slow = resp(429, br#"{"error":"rate_limited"}"#.to_vec());
        assert_eq!(
            BmsIrRead.parse(&op, &slow),
            Outcome::Retry {
                after: Some(RetryAfter::Secs(60)),
                why: "rate_limited".into()
            }
        );
        busy.headers.push(("Retry-After".into(), "3".into()));
        assert!(matches!(
            BmsIrRead.parse(&op, &busy),
            Outcome::Retry {
                after: Some(RetryAfter::Secs(3)),
                ..
            }
        ));
        let bad = resp(400, br#"{"ok":false,"error":"invalid_md5"}"#.to_vec());
        assert_eq!(
            BmsIrRead.parse(&op, &bad),
            Outcome::Rejected("invalid_md5".into())
        );
        let c = chart();
        let rank = Op::Ranking {
            chart: &c,
            view: "",
            limit: 0,
            etag: None,
        };
        assert!(matches!(
            BmsIrRead.parse(&rank, &resp(200, b"<html>".to_vec())),
            Outcome::Rejected(_)
        ));
        assert_eq!(iso_ms("2026-01-01T00:00:00.5Z"), Some(1_767_225_600_000));
        assert_eq!(iso_ms("2026-01-01 00:00:00"), None);
    }
}
