//! Settings of older configurations, read the way they are now.

use super::{CATALOG, HISPEED_MAX, HISPEED_MIN, HISPEED_STEP, OLD_DEFAULT};

/// The BPM a duration from an old configuration is read at.
const LEGACY_BPM: f64 = 150.0;

/// Older configurations stored the duration (ms) instead of the hi-speed:
/// keep the speed it gave at 150 BPM. The mirror's one event (BOF22)
/// became its list of events. Tables that moved are read from their new
/// address. The one IR server (`[ir_server]`) became an account of
/// `[ir]`. The perspective lanes' touch area of an older layout, left at
/// its default, becomes the current default. The secrets it kept are taken
/// out of it ([`Old`]).
pub fn migrate(v: &mut toml::Value) -> Old {
    let old = Old {
        ir_token: ir_server(v),
        obs_password: obs_password(v),
    };
    settings(v);
    old
}

/// The secrets an older configuration kept, which are no settings any
/// more: they go to the credentials store (`ir::creds`).
#[derive(Default)]
pub struct Old {
    /// `[ir_server]`'s token, of the account it became.
    pub ir_token: String,
    /// `[obs]`'s password.
    pub obs_password: String,
}

/// `[obs]`'s password, taken out.
fn obs_password(v: &mut toml::Value) -> String {
    let obs = v.get_mut("obs").and_then(|o| o.as_table_mut());
    match obs.and_then(|o| o.remove("password")) {
        Some(toml::Value::String(p)) => p,
        _ => String::new(),
    }
}

fn settings(v: &mut toml::Value) {
    perspective(v);
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
            if u.as_str() == Some(OLD_DEFAULT) {
                *u = toml::Value::String(CATALOG.into());
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

/// `[perspective]` of an older lane layout (no `layout`): a touch area
/// left at that layout's default takes the current one's (the judge line
/// moved up; the old height would leave little room above it). A height
/// the player chose is kept.
fn perspective(v: &mut toml::Value) {
    use super::{PerspectiveConfig, OLD_TOUCH_HEIGHTS, PERSPECTIVE_LAYOUT};
    let Some(p) = v.get_mut("perspective").and_then(|p| p.as_table_mut()) else {
        return;
    };
    if p.contains_key("layout") {
        return;
    }
    let old = p.get("touch_height").and_then(|h| h.as_float());
    if old.is_some_and(|h| {
        OLD_TOUCH_HEIGHTS
            .iter()
            .any(|&d| (h - d as f64).abs() < 1e-4)
    }) {
        let new = PerspectiveConfig::default().touch_height;
        p.insert("touch_height".into(), toml::Value::Float(new as f64));
    }
    p.insert(
        "layout".into(),
        toml::Value::Integer(PERSPECTIVE_LAYOUT as i64),
    );
}

/// `[ir_server]` as a vibeBMS IR account of `[ir]`: the first, and its
/// token (empty when no account was made).
fn ir_server(v: &mut toml::Value) -> String {
    let Some(root) = v.as_table_mut() else {
        return String::new();
    };
    let Some(toml::Value::Table(old)) = root.remove("ir_server") else {
        return String::new();
    };
    let text = |k: &str| old.get(k).and_then(|x| x.as_str()).unwrap_or("");
    if root.contains_key("ir") || text("url").trim().is_empty() {
        return String::new();
    }
    let flag = |k: &str| old.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    let account: toml::Table = [
        ("provider", "vibe-json:0".into()),
        ("name", text("name").into()),
        ("url", text("url").into()),
        ("enabled", flag("enabled").into()),
        ("submit", flag("auto_submit").into()),
    ]
    .into_iter()
    .map(|(k, v): (&str, toml::Value)| (k.to_string(), v))
    .collect();
    let token = text("token").to_string();
    let mut ir = toml::Table::new();
    ir.insert("accounts".into(), vec![account].into());
    root.insert("ir".into(), ir.into());
    token
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn old_perspective_touch_area() {
        // an older layout's default moves to the current one
        for old in ["0.4", "0.30000001192092896"] {
            let mut v: toml::Value =
                toml::from_str(&format!("[perspective]\ntouch_height = {old}\n")).unwrap();
            migrate(&mut v);
            let c: Config = v.try_into().unwrap();
            assert_eq!(c.perspective, crate::config::PerspectiveConfig::default());
        }
        // a height chosen is kept, and so is one saved with this layout
        for s in [
            "[perspective]\ntouch_height = 0.55\n",
            "[perspective]\ntouch_height = 0.4\nlayout = 2\n",
        ] {
            let mut v: toml::Value = toml::from_str(s).unwrap();
            let h = v["perspective"]["touch_height"].as_float().unwrap() as f32;
            migrate(&mut v);
            let c: Config = v.try_into().unwrap();
            assert_eq!((c.perspective.touch_height, c.perspective.layout), (h, 2));
        }
    }

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
            OLD_DEFAULT
        ))
        .unwrap();
        migrate(&mut v);
        let c: Config = v.try_into().unwrap();
        assert_eq!(c.archives, [CATALOG, "https://x/e/index.json"]);
    }

    #[test]
    fn the_ir_server_becomes_an_account() {
        use vibebms_ir::ProviderId;
        let old =
            "[ir_server]\nenabled = true\nname = \"Mine\"\nurl = \"https://ir.example/api/\"\n\
                   token = \"s3cr3t\"\nauto_submit = false\n";
        let c = Config::parse(old).unwrap().0;
        let a = &c.ir.accounts[0];
        assert_eq!(
            (a.provider, a.name.as_str(), a.enabled, a.submit),
            (ProviderId::VibeJson(0), "Mine", true, false)
        );
        assert_eq!(a.title(), "Mine");
        assert_eq!(a.old_token.0, "s3cr3t");
        assert_eq!(format!("{a:?}").matches("s3cr3t").count(), 0);
        // never written
        let again = c.to_toml().unwrap();
        assert!(!again.contains("s3cr3t") && !again.contains("ir_server"));
        let back = Config::parse(&again).unwrap().0;
        assert_eq!(back.ir.accounts[0].old_token.0, "");
        // an empty one is no account
        let mut v: toml::Value =
            toml::from_str("[ir_server]\nenabled = false\nurl = \"\"\n").unwrap();
        migrate(&mut v);
        assert!(v.get("ir").is_none() && v.get("ir_server").is_none());
    }

    /// A key is read from an `[ir_server]` of before and from nowhere
    /// else: an account's own settings cannot name one (a configuration
    /// from someone else would put its key in the place of the player's).
    #[test]
    fn an_account_cannot_name_a_key() {
        let text = "[[ir.accounts]]\nprovider = \"bokutachi\"\nold_token = \"theirs\"\n\
                    token = \"theirs\"\n";
        let c = Config::parse(text).unwrap().0;
        assert_eq!(c.ir.accounts[0].old_token.0, "");
        // an `[ir_server]` beside an `[ir]` is no account
        let both = format!("[ir_server]\nurl = \"https://x\"\ntoken = \"theirs\"\n{text}");
        let c = Config::parse(&both).unwrap().0;
        assert_eq!(c.ir.accounts.len(), 1);
        assert_eq!(c.ir.accounts[0].old_token.0, "");
    }

    /// `[obs]`'s password of before leaves the settings: read once for the
    /// credentials store, never shown by `Debug`, never written.
    #[test]
    fn the_obs_password_leaves_the_settings() {
        let old = "[obs]\nenabled = true\nhost = \"127.0.0.1\"\npassword = \"s3cr3t\"\n";
        let c = Config::parse(old).unwrap().0;
        assert!(c.obs.enabled);
        assert_eq!(c.obs.old_password.0, "s3cr3t");
        assert!(!format!("{:?}", c.obs).contains("s3cr3t"));
        let again = c.to_toml().unwrap();
        assert!(
            !again.contains("s3cr3t") && !again.contains("password"),
            "{again}"
        );
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
