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
//! * [`play`]: the rule options and the chart and judge they make
//! * [`replay`]: a play's inputs, to judge it again (headless)

/// An enum of options picked from a list, with `ALL` (in declaration
/// order) and `label()`, so the list cannot drift from the enum:
/// `choices! { pub enum X { A => "A", B = 1 => "B" } }`.
#[macro_export]
macro_rules! choices {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident $(= $disc:literal)? => $label:literal,)+
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $($(#[$vmeta])* $variant $(= $disc)?,)+
        }

        impl $name {
            pub const ALL: [$name; [$($name::$variant),+].len()] = [$($name::$variant),+];

            pub fn label(self) -> &'static str {
                match self {
                    $($name::$variant => $label,)+
                }
            }
        }

        impl $crate::Labeled for $name {
            fn label(self) -> &'static str {
                $name::label(self)
            }

            fn from_label(label: &str) -> Option<$name> {
                $name::ALL.into_iter().find(|v| v.label() == label)
            }
        }
    };
}

/// A `choices!` enum, known by its label.
pub trait Labeled: Sized {
    fn label(self) -> &'static str;
    fn from_label(label: &str) -> Option<Self>;
}

/// Serde of a `choices!` enum by its label, for files that store the label
/// (`#[serde(with = "vibebms_core::by_label")]`); a label not known reads
/// as the default.
pub mod by_label {
    use super::Labeled;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<T: Labeled + Copy, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(v.label())
    }

    pub fn deserialize<'de, T: Labeled + Default, D: Deserializer<'de>>(
        d: D,
    ) -> Result<T, D::Error> {
        let label = String::deserialize(d)?;
        Ok(T::from_label(&label).unwrap_or_default())
    }
}

pub mod bms;
pub mod bmson;
pub mod bot;
pub mod chart;
pub mod encoding;
pub mod engine;
pub mod gauge;
pub mod judge;
pub mod lane_option;
pub mod mode;
pub mod play;
pub mod replay;
pub mod rng;
pub mod score;
pub mod timing;

pub use chart::{Chart, ChartMeta, LnProfile, LnType, Note, NoteKind};
pub use mode::PlayMode;

/// Decode any supported chart file based on its extension.
pub fn load_chart(path: &std::path::Path, opts: &bms::ParseOptions) -> Result<Chart, ChartError> {
    let bytes = std::fs::read(path).map_err(|e| ChartError::Io(e.to_string()))?;
    load_chart_bytes(path, &bytes, opts)
}

/// Decode chart bytes; `path` selects the format by extension and is stored in the meta.
pub fn load_chart_bytes(
    path: &std::path::Path,
    bytes: &[u8],
    opts: &bms::ParseOptions,
) -> Result<Chart, ChartError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let mut chart = if ext == "bmson" {
        bmson::parse_bmson(bytes)?
    } else {
        bms::parse_bms(bytes, &ext, opts)?
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
