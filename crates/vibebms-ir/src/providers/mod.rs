//! One adapter per IR service.

pub mod bmsir_read;
pub mod tachi;
pub mod vibe_json;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use crate::rule::Profile;
    use serde_json::Value;
    use vibebms_core::record::{ClientId, PlayRecord};
    use vibebms_core::score::ClearType;
    use vibebms_core::PlayMode;

    /// Every `client` and `service` field in `v`.
    fn names<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    match v.as_str() {
                        Some(s) if k == "client" || k == "service" => out.push(s),
                        _ => names(v, out),
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|v| names(v, out)),
            _ => {}
        }
    }

    /// vibeBMS says it is vibeBMS: only the rule profile may name the game
    /// its rules follow.
    #[test]
    fn no_adapter_sends_another_clients_name() {
        let vj = vibe_json::VibeJson::new(0, "https://ir.example").unwrap();
        let adapters: [&dyn Provider; 2] = [&vj, &tachi::Tachi];
        for mode in [PlayMode::Beat7K, PlayMode::Beat14K, PlayMode::Popn9K] {
            for profile in Profile::ALL {
                let mut rec = PlayRecord {
                    client: ClientId {
                        name: "vibeBMS".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                rec.chart.mode = mode;
                rec.chart.sha256 = "ab".repeat(32);
                rec.device.keyboard = 10;
                rec.result.clear = ClearType::Hard;
                for a in adapters {
                    let body = a.freeze(&rec, "p", profile).unwrap().body;
                    let json: Value = serde_json::from_str(&body).unwrap();
                    let mut found = Vec::new();
                    names(&json, &mut found);
                    assert!(!found.is_empty(), "{body}");
                    for n in found {
                        let n = n.to_ascii_lowercase();
                        assert!(
                            !["lr2", "oraja", "beatoraja"].iter().any(|w| n.contains(w)),
                            "{n} in {body}"
                        );
                    }
                }
            }
        }
    }
}
