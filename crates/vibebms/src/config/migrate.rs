//! Settings of older configurations, read the way they are now.

use super::{HISPEED_MAX, HISPEED_MIN, HISPEED_STEP};

/// The BPM a duration from an old configuration is read at.
const LEGACY_BPM: f64 = 150.0;

/// Older configurations stored the duration (ms) instead of the hi-speed:
/// keep the speed it gave at 150 BPM. The mirror's one event (BOF22)
/// became its list of events. Tables that moved are read from their new
/// address. The one IR server (`[ir_server]`) became an account of
/// `[ir]`.
pub fn migrate(v: &mut toml::Value) {
    ir_server(v);
    if let Some(tables) = v.get_mut("tables").and_then(|t| t.as_array_mut()) {
        for t in tables.iter_mut() {
            let Some(url) = t.get_mut("url") else {
                continue;
            };
            if let Some((_, new)) = bmseeker::table::MOVED
                .iter()
                .find(|(old, _)| url.as_str() == Some(*old))
            {
                *url = toml::Value::String(new.to_string());
            }
        }
    }
    if let Some(a) = v.get_mut("archives").and_then(|a| a.as_array_mut()) {
        for u in a.iter_mut() {
            if u.as_str() == Some(crate::archive::OLD_DEFAULT) {
                *u = toml::Value::String(crate::archive::CATALOG.into());
            }
        }
    }
    let Some(play) = v.get_mut("play").and_then(|p| p.as_table_mut()) else {
        return;
    };
    // custom_judge = [pg, gr, gd] became expand_judge + judge_rates
    if let Some(rates) = play.remove("custom_judge") {
        if rates.as_array().is_some_and(|a| a.len() == 3) {
            play.insert("expand_judge".into(), toml::Value::Boolean(true));
            play.insert("judge_rates".into(), rates);
        }
    }
    if play.contains_key("hispeed") {
        return;
    }
    let Some(ms) = play.remove("duration_ms").and_then(|d| d.as_integer()) else {
        return;
    };
    let f = |k: &str| play.get(k).and_then(|x| x.as_float()).unwrap_or(0.0);
    let cover = if play
        .get("lane_cover")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
        || f("sudden") > 0.0
    {
        f("sudden")
    } else {
        0.0
    };
    let visible = (1.0 - cover - f("lift")).max(0.05);
    let hs = 240_000.0 * visible / (LEGACY_BPM * ms.max(1) as f64);
    let hs = ((hs / HISPEED_STEP as f64).round() * HISPEED_STEP as f64)
        .clamp(HISPEED_MIN as f64, HISPEED_MAX as f64);
    play.insert("hispeed".into(), toml::Value::Float(hs));
}

/// `[ir_server]` as a vibeBMS IR account of `[ir]`. Its token is read
/// into the account (`IrAccount::old_token`), which is never written back:
/// the app moves it to the credentials store.
fn ir_server(v: &mut toml::Value) {
    let Some(root) = v.as_table_mut() else {
        return;
    };
    let Some(toml::Value::Table(old)) = root.remove("ir_server") else {
        return;
    };
    let text = |k: &str| old.get(k).and_then(|x| x.as_str()).unwrap_or("");
    if root.contains_key("ir") || text("url").trim().is_empty() {
        return;
    }
    let flag = |k: &str| old.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    let account: toml::Table = [
        ("provider", "vibe-json:0".into()),
        ("name", text("name").into()),
        ("url", text("url").into()),
        ("enabled", flag("enabled").into()),
        ("submit", flag("auto_submit").into()),
        ("old_token", text("token").into()),
    ]
    .into_iter()
    .map(|(k, v): (&str, toml::Value)| (k.to_string(), v))
    .collect();
    let mut ir = toml::Table::new();
    ir.insert("accounts".into(), vec![account].into());
    root.insert("ir".into(), ir.into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn moved_tables() {
        let mut v: toml::Value = toml::from_str(
            "[[tables]]\nname = \"x\"\nurl = \"http://www.ribbit.xyz/bms/tables/insane.html\"\nenabled = true\n",
        )
        .unwrap();
        migrate(&mut v);
        let c: Config = v.try_into().unwrap();
        assert_eq!(c.tables[0].url, bmseeker::table::INSANE);
    }

    #[test]
    fn the_mirror_becomes_its_list_of_events() {
        let mut v: toml::Value = toml::from_str(&format!(
            "archives = [\"{}\", \"https://x/e/index.json\"]\n",
            crate::archive::OLD_DEFAULT
        ))
        .unwrap();
        migrate(&mut v);
        let c: Config = v.try_into().unwrap();
        assert_eq!(
            c.archives,
            [crate::archive::CATALOG, "https://x/e/index.json"]
        );
    }

    #[test]
    fn the_ir_server_becomes_an_account() {
        use vibebms_ir::ProviderId;
        let old =
            "[ir_server]\nenabled = true\nname = \"Mine\"\nurl = \"https://ir.example/api/\"\n\
                   token = \"s3cr3t\"\nauto_submit = false\n";
        let mut v: toml::Value = toml::from_str(old).unwrap();
        migrate(&mut v);
        let c: Config = v.try_into().unwrap();
        let a = &c.ir.accounts[0];
        assert_eq!(
            (a.provider, a.name.as_str(), a.enabled, a.submit),
            (ProviderId::VibeJson(0), "Mine", true, false)
        );
        assert_eq!(a.title(), "Mine");
        assert_eq!(a.old_token.0, "s3cr3t");
        assert_eq!(format!("{a:?}").matches("s3cr3t").count(), 0);
        // never written
        let again = toml::to_string_pretty(&c).unwrap();
        assert!(!again.contains("s3cr3t") && !again.contains("ir_server"));
        let back: Config = toml::from_str(&again).unwrap();
        assert_eq!(back.ir.accounts[0].old_token.0, "");
        // an empty one is no account
        let mut v: toml::Value =
            toml::from_str("[ir_server]\nenabled = false\nurl = \"\"\n").unwrap();
        migrate(&mut v);
        assert!(v.get("ir").is_none() && v.get("ir_server").is_none());
    }

    #[test]
    fn custom_judge_becomes_expand_judge() {
        let mut v: toml::Value =
            toml::from_str("[play]\ncustom_judge = [200, 150, 100]\n").unwrap();
        migrate(&mut v);
        let c: Config = v.try_into().unwrap();
        assert!(c.play.expand_judge);
        assert_eq!(c.play.judge_rates, [200, 150, 100]);
    }
}
