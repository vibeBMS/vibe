//! Plays a key log on a chart headless and prints the result as JSON: the
//! vibeBMS side of the comparison with other players (`tools/e2e`).
//!
//! `cargo run -p vibebms-core --example e2e_play -- <case.json>...`
//!
//! A case: `{"name", "chart" (a path, from the case file's folder),
//! "rules" (RuleOptions as a replay keeps them; missing fields: their
//! defaults), "seed" (the chart's #RANDOM, default 1), "inputs": [[time µs,
//! lane, key, pressed], ...]}`. Lanes are vibeBMS's (7 keys: 0..6 the keys,
//! 7 the scratch, key 0 up and 1 down). The play stops where a survival
//! gauge fails, as a play does. One JSON line per case.

use serde_json::{json, Value};
use std::path::Path;
use vibebms_core::engine::InputEvent;
use vibebms_core::judge::Judge;
use vibebms_core::play::{build_chart, build_engine, RuleOptions};
use vibebms_core::score::Assist;

fn play(case_path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(case_path).map_err(|e| e.to_string())?;
    let case: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let dir = case_path.parent().unwrap_or(Path::new("."));
    let chart_path = dir.join(case["chart"].as_str().ok_or("no chart")?);
    let bytes = std::fs::read(&chart_path).map_err(|e| format!("{}: {e}", chart_path.display()))?;
    let rules: RuleOptions =
        serde_json::from_value(case["rules"].clone()).map_err(|e| format!("rules: {e}"))?;
    let seed = case["seed"].as_i64().unwrap_or(1);
    let chart = build_chart(&chart_path, &bytes, &rules, seed, None)?.chart;
    let mut inputs: Vec<InputEvent> = (case["inputs"].as_array().ok_or("no inputs")?.iter())
        .map(|i| InputEvent {
            time: i[0].as_i64().unwrap_or(0),
            lane: i[1].as_u64().unwrap_or(0) as u8,
            key: i[2].as_u64().unwrap_or(0) as u8,
            pressed: i[3].as_bool().unwrap_or(false),
        })
        .collect();
    inputs.sort_by_key(|i| i.time);
    let (mut e, total) = build_engine(&chart, &rules, None, false);
    // the judge advanced every millisecond, each input at its own time
    let end = chart.length() + 3_000_000;
    let mut next = inputs.iter().peekable();
    let mut t = -1_000_000;
    let mut failed = false;
    while t <= end && !failed {
        while let Some(i) = next.next_if(|i| i.time <= t) {
            e.update(i.time);
            e.input(*i);
            e.update(i.time);
        }
        e.update(t);
        failed = e.gauge.is_failed_now();
        t += 1_000;
    }
    let sc = &e.score;
    let gauge_clear = match failed {
        true => None,
        false => e.gauge.final_clear_gauge().map(|g| g.clear_type()),
    };
    let side = |j: Judge| {
        let k = j as usize;
        json!({"early": sc.counts[k][0], "late": sc.counts[k][1], "just": sc.just[k]})
    };
    Ok(json!({
        "name": case["name"],
        "mode": format!("{:?}", chart.mode()),
        "notes": sc.total_notes,
        "total": total,
        "judges": {
            "pg": side(Judge::PGreat), "gr": side(Judge::Great), "gd": side(Judge::Good),
            "bd": side(Judge::Bad), "pr": side(Judge::Poor), "ms": side(Judge::Miss),
        },
        "ex": sc.ex_score(),
        "max_combo": sc.max_combo,
        "fast": sc.fast(),
        "slow": sc.slow(),
        "passed": sc.passed,
        "gauge": e.gauge.value(),
        "failed": failed,
        "clear": sc.clear_type(gauge_clear, Assist::None) as i64,
        "ghost": e.ghost.iter().map(|j| *j as u8).collect::<Vec<_>>(),
    }))
}

fn main() {
    let mut code = 0;
    for a in std::env::args().skip(1) {
        match play(Path::new(&a)) {
            Ok(v) => println!("{v}"),
            Err(e) => {
                eprintln!("{a}: {e}");
                println!("{}", json!({"case": a, "error": e}));
                code = 1;
            }
        }
    }
    std::process::exit(code);
}
