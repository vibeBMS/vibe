//! Core gameplay engine for vibeBMS.
//!
//! This crate has no platform, graphics or audio dependencies so that it can be
//! compiled for every target (desktop x86/ARM, iOS, Android) and unit-tested
//! headlessly.
//!
//! * [`bms`] / [`bmson`]: chart decoders producing a [`chart::Chart`]
//! * [`timing`]: beat <-> time <-> scroll-position conversion (BPM, STOP, SCROLL)
//! * [`judge`]: judge windows (beatoraja / LR2 / IIDX-style presets)
//! * [`gauge`]: groove gauges incl. EX-HARD, HAZARD, dan (class) gauges and GAS
//! * [`score`]: EX score, clear lamps, DJ level
//! * [`lane_option`]: RANDOM / MIRROR / S-RANDOM / H-RANDOM / ... and assist options
//! * [`engine`]: the real-time judge engine (normal notes, LN / CN / HCN, mines, BSS)

pub mod bms;
pub mod bmson;
pub mod chart;
pub mod encoding;
pub mod engine;
pub mod gauge;
pub mod judge;
pub mod lane_option;
pub mod mode;
pub mod rng;
pub mod score;
pub mod timing;

pub use chart::{Chart, ChartMeta, LnType, Note, NoteKind};
pub use mode::PlayMode;

/// Decode any supported chart file based on its extension.
pub fn load_chart(path: &std::path::Path, opts: &bms::ParseOptions) -> Result<Chart, ChartError> {
    let bytes = std::fs::read(path).map_err(|e| ChartError::Io(e.to_string()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let mut chart = if ext == "bmson" {
        bmson::parse_bmson(&bytes)?
    } else {
        bms::parse_bms(&bytes, &ext, opts)?
    };
    chart.meta.path = Some(path.to_path_buf());
    Ok(chart)
}

/// File extensions recognised as playable charts.
pub const CHART_EXTENSIONS: &[&str] = &["bms", "bme", "bml", "pms", "bmson"];

#[derive(Debug, Clone, PartialEq)]
pub enum ChartError {
    Io(String),
    Parse(String),
    Empty,
}

impl std::fmt::Display for ChartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChartError::Io(e) => write!(f, "I/O error: {e}"),
            ChartError::Parse(e) => write!(f, "parse error: {e}"),
            ChartError::Empty => write!(f, "chart has no playable notes"),
        }
    }
}

impl std::error::Error for ChartError {}
