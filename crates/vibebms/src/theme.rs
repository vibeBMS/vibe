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
    pub note_white: String,
    pub note_blue: String,
    pub note_scratch: String,
    /// Optional background image for menus (relative to the theme folder).
    pub background_image: String,
    /// Touch control width units: [key, scratch].
    pub lane_width: [f32; 2],
}

impl Default for ThemeFile {
    fn default() -> Self {
        ThemeFile {
            name: "default".into(),
            background: "#0b0d14".into(),
            panel: "#141824ee".into(),
            accent: "#2de2c7".into(),
            text: "#e2e8f5".into(),
            text_dim: "#808caa".into(),
            note_white: "#e8ecf2".into(),
            note_blue: "#3f8cff".into(),
            note_scratch: "#ff4d5e".into(),
            background_image: String::new(),
            lane_width: [1.0, 1.7],
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
    pub note_white: Color,
    pub note_blue: Color,
    pub note_scratch: Color,
    pub background_image: Option<PathBuf>,
    pub lane_width: [f32; 2],
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
        if name == "default" {
            // the built-in look; its values are written out (and refreshed
            // when they change) only as a template to copy into a new theme
            let template = dir.join("theme.toml.example");
            if let Ok(t) = toml::to_string_pretty(&ThemeFile::default()) {
                if crate::platform::vfs::read_to_string(&template)
                    .ok()
                    .as_deref()
                    != Some(&t)
                {
                    let _ = crate::platform::vfs::write(&template, t.as_bytes());
                }
            }
            return Theme::from_file(&ThemeFile::default(), None);
        }
        let tf: ThemeFile = crate::platform::vfs::read_to_string(&file)
            .ok()
            .and_then(|s| {
                toml::from_str(&s)
                    .map_err(|e| log::warn!("theme: {e}"))
                    .ok()
            })
            .unwrap_or_default();
        Theme::from_file(&tf, crate::platform::vfs::is_dir(&dir).then_some(dir))
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
            note_white: c(&t.note_white, &d.note_white),
            note_blue: c(&t.note_blue, &d.note_blue),
            note_scratch: c(&t.note_scratch, &d.note_scratch),
            background_image: (!t.background_image.is_empty())
                .then(|| dir.clone().unwrap_or_default().join(&t.background_image)),
            lane_width: [t.lane_width[0].max(0.2), t.lane_width[1].max(0.2)],
            dir,
        }
    }

    pub fn list(themes_dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = crate::platform::vfs::read_dir(themes_dir)
            .map(|rd| {
                rd.into_iter()
                    .filter(|e| {
                        e.is_dir && crate::platform::vfs::is_file(&e.path.join("theme.toml"))
                    })
                    .filter_map(|e| e.path.file_name().map(|n| n.to_string_lossy().into_owned()))
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
