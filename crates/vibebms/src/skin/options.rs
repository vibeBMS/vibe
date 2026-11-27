//! What the user chooses for a skin: its options, files and offsets.

use std::collections::HashMap;

/// What the user chose for a skin.
#[derive(Clone, Debug, Default)]
pub struct UserSettings {
    /// Option ids on / off (custom options).
    pub options: HashMap<i32, bool>,
    /// Chosen file per custom file name (LR2 `#CUSTOMFILE`, beatoraja
    /// `filepath`).
    pub files: HashMap<String, String>,
    /// Offset values (x, y, w, h, angle, alpha) by offset name.
    pub offsets: HashMap<String, [i32; 6]>,
}

/// A position adjustment a skin offers (beatoraja `offset`): objects
/// referring to `id` move by the user's values.
#[derive(Clone, Debug)]
pub struct CustomOffset {
    pub name: String,
    pub id: i32,
    /// Which of x, y, w, h, angle, alpha the user may change.
    pub fields: [bool; 6],
}

/// Names of the offset fields, as in `CustomOffset::fields`.
pub const OFFSET_FIELDS: [&str; 6] = ["X", "Y", "W", "H", "ANGLE", "ALPHA"];

/// A skin's user option (LR2 `#CUSTOMOPTION`, beatoraja `property`), or
/// a choice of files (LR2 `#CUSTOMFILE`, beatoraja `filepath`).
#[derive(Clone, Debug)]
pub struct CustomOption {
    pub name: String,
    pub first_id: i32,
    pub labels: Vec<String>,
    /// Option id of each choice (none for a choice of files).
    pub ops: Vec<i32>,
    /// Choice used when none is set.
    pub default: usize,
}

impl CustomOption {
    /// Index of the chosen label (else the default).
    pub fn chosen(&self, label: Option<&str>) -> usize {
        label
            .and_then(|l| self.labels.iter().position(|x| x == l))
            .unwrap_or(self.default)
    }
}

/// The files chosen for a skin's custom files (by name; defaults left out).
pub fn user_files(
    opts: &[CustomOption],
    chosen: Option<&std::collections::BTreeMap<String, String>>,
) -> HashMap<String, String> {
    opts.iter()
        .filter(|o| o.ops.is_empty())
        .filter_map(|o| {
            let l = chosen?.get(&o.name)?;
            o.labels.contains(l).then(|| (o.name.clone(), l.clone()))
        })
        .collect()
}

/// Option ids to enable / disable for the chosen labels (by option name).
pub fn user_options(
    opts: &[CustomOption],
    chosen: Option<&std::collections::BTreeMap<String, String>>,
) -> std::collections::HashMap<i32, bool> {
    let mut out = std::collections::HashMap::new();
    for o in opts {
        let k = o.chosen(chosen.and_then(|c| c.get(&o.name)).map(String::as_str));
        for (i, op) in o.ops.iter().enumerate() {
            out.insert(*op, i == k);
        }
    }
    out
}
