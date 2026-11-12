//! Custom themes: `themes/<name>/theme.toml` (+ optional images/fonts).
//!
//! Every field is optional; missing values use the built-in look.
//! Colors are `"#RRGGBB"` or `"#RRGGBBAA"`.

use crate::gfx::{rgba, Color};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeFile {
    pub name: String,
    pub background: String,
    pub panel: String,
    pub accent: String,
    pub text: String,
    pub text_dim: String,
    pub lane_bg: String,
    pub lane_line: String,
    pub judge_line: String,
    pub note_white: String,
    pub note_blue: String,
    pub note_scratch: String,
    pub note_mine: String,
    pub ln_body_alpha: f32,
    pub key_beam: String,
    /// Optional background image for menus (relative to the theme folder).
    pub background_image: String,
    /// Lane width units: [key, scratch].
    pub lane_width: [f32; 2],
    pub note_height: f32,
}

impl Default for ThemeFile {
    fn default() -> Self {
        ThemeFile {
            name: "default".into(),
            background: "#0b0d14".into(),
            panel: "#161a26e6".into(),
            accent: "#4fc3f7".into(),
            text: "#f2f4f8".into(),
            text_dim: "#8a93a6".into(),
            lane_bg: "#05060a".into(),
            lane_line: "#2a2f3d".into(),
            judge_line: "#ff3b30".into(),
            note_white: "#e8ecf2".into(),
            note_blue: "#3f8cff".into(),
            note_scratch: "#ff4d5e".into(),
            note_mine: "#b02020".into(),
            ln_body_alpha: 0.55,
            key_beam: "#4fc3f7".into(),
            background_image: String::new(),
            lane_width: [1.0, 1.7],
            note_height: 0.018,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub dir: Option<PathBuf>,
    pub background: Color,
    pub panel: Color,
    pub accent: Color,
    pub text: Color,
    pub text_dim: Color,
    pub lane_bg: Color,
    pub lane_line: Color,
    pub judge_line: Color,
    pub note_white: Color,
    pub note_blue: Color,
    pub note_scratch: Color,
    pub note_mine: Color,
    pub ln_body_alpha: f32,
    pub key_beam: Color,
    pub background_image: Option<PathBuf>,
    pub lane_width: [f32; 2],
    pub note_height: f32,
}

fn parse_color(s: &str, fallback: &str) -> Color {
    let h = s.trim().trim_start_matches('#');
    let v = match h.len() {
        6 => u32::from_str_radix(h, 16).ok().map(|v| (v << 8) | 0xff),
        8 => u32::from_str_radix(h, 16).ok(),
        _ => None,
    };
    match v {
        Some(v) => rgba(v),
        None if s != fallback => parse_color(fallback, fallback),
        None => [1.0, 0.0, 1.0, 1.0],
    }
}

impl Theme {
    pub fn load(themes_dir: &Path, name: &str) -> Theme {
        let dir = themes_dir.join(name);
        let file = dir.join("theme.toml");
        let tf: ThemeFile = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| {
                toml::from_str(&s)
                    .map_err(|e| log::warn!("theme: {e}"))
                    .ok()
            })
            .unwrap_or_default();
        if !file.exists() && name == "default" {
            // write the defaults so users have a template to copy
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(s) = toml::to_string_pretty(&ThemeFile::default()) {
                let _ = std::fs::write(&file, s);
            }
        }
        Theme::from_file(&tf, dir.is_dir().then_some(dir))
    }

    pub fn from_file(t: &ThemeFile, dir: Option<PathBuf>) -> Theme {
        let d = ThemeFile::default();
        let c = |v: &str, f: &str| parse_color(v, f);
        Theme {
            background: c(&t.background, &d.background),
            panel: c(&t.panel, &d.panel),
            accent: c(&t.accent, &d.accent),
            text: c(&t.text, &d.text),
            text_dim: c(&t.text_dim, &d.text_dim),
            lane_bg: c(&t.lane_bg, &d.lane_bg),
            lane_line: c(&t.lane_line, &d.lane_line),
            judge_line: c(&t.judge_line, &d.judge_line),
            note_white: c(&t.note_white, &d.note_white),
            note_blue: c(&t.note_blue, &d.note_blue),
            note_scratch: c(&t.note_scratch, &d.note_scratch),
            note_mine: c(&t.note_mine, &d.note_mine),
            ln_body_alpha: t.ln_body_alpha.clamp(0.0, 1.0),
            key_beam: c(&t.key_beam, &d.key_beam),
            background_image: (!t.background_image.is_empty())
                .then(|| dir.clone().unwrap_or_default().join(&t.background_image)),
            lane_width: [t.lane_width[0].max(0.2), t.lane_width[1].max(0.2)],
            note_height: t.note_height.clamp(0.002, 0.1),
            dir,
        }
    }

    pub fn list(themes_dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(themes_dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().join("theme.toml").is_file())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        if !v.iter().any(|n| n == "default") {
            v.insert(0, "default".into());
        }
        v.sort();
        v
    }

    /// Colour for clear lamps.
    pub fn lamp(c: vibebms_core::score::ClearType) -> Color {
        use vibebms_core::score::ClearType::*;
        rgba(match c {
            NoPlay => 0x30333dff,
            Failed => 0x7a1f1fff,
            AssistEasy | LightAssistEasy => 0xa070f0ff,
            Easy => 0x60d060ff,
            Normal => 0x4fa3ffff,
            Hard => 0xf0f0f0ff,
            ExHard => 0xffd23fff,
            FullCombo | Perfect | Max => 0x7ff7ffff,
        })
    }
}
