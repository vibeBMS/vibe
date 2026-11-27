//! This launch's command-line options, which the saved config leaves out
//! (`app::cli` reads them).

use crate::config::Config;

/// What this launch's options changed in the config: the settings, with
/// their values before and after (`None`: not written, at its default).
/// The config is saved without them (a setting changed in the game since
/// is saved as it is then).
#[derive(Clone, Debug, Default)]
pub struct Overrides {
    keys: Vec<(String, Option<toml::Value>, Option<toml::Value>)>,
}

type Changes = Vec<(String, Option<toml::Value>, Option<toml::Value>)>;

/// Every setting that differs between `a` and `b`, by its path.
fn diff(path: &str, a: Option<&toml::Value>, b: Option<&toml::Value>, out: &mut Changes) {
    match (a, b) {
        (Some(toml::Value::Table(x)), Some(toml::Value::Table(y))) => {
            let keys = x.keys().chain(y.keys().filter(|k| !x.contains_key(*k)));
            for k in keys {
                let p = match path {
                    "" => k.clone(),
                    _ => format!("{path}.{k}"),
                };
                diff(&p, x.get(k), y.get(k), out);
            }
        }
        _ if a != b => out.push((path.to_string(), a.cloned(), b.cloned())),
        _ => {}
    }
}

/// The table holding `a.b.c` in `v`, and the last key.
pub(crate) fn parent_mut<'a, 'k>(
    v: &'a mut toml::Value,
    key: &'k str,
) -> Option<(&'a mut toml::Table, &'k str)> {
    let (head, last) = match key.rsplit_once('.') {
        Some((h, l)) => (Some(h), l),
        None => (None, key),
    };
    let t = match head {
        Some(h) => path_mut(v, h)?,
        None => v,
    };
    Some((t.as_table_mut()?, last))
}

/// The value at `a.b.c` in `v`, if there is one.
pub(crate) fn path_mut<'a>(v: &'a mut toml::Value, key: &str) -> Option<&'a mut toml::Value> {
    key.split('.')
        .try_fold(v, |v, k| v.as_table_mut().and_then(|t| t.get_mut(k)))
}

impl Overrides {
    /// What differs between the config `before` this launch's options and
    /// `after` them.
    pub fn between(before: &Config, after: &Config) -> Result<Overrides, String> {
        // (the usual launch: nothing to compare)
        if after == before {
            return Ok(Overrides::default());
        }
        let before = toml::Value::try_from(before).map_err(|e| e.to_string())?;
        let after = toml::Value::try_from(after).map_err(|e| e.to_string())?;
        let mut keys = Vec::new();
        diff("", Some(&before), Some(&after), &mut keys);
        Ok(Overrides { keys })
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// `config` as it is saved: each setting still at this launch's value
    /// at the value it had before.
    pub fn for_saving(&self, config: &Config) -> Option<Config> {
        if self.keys.is_empty() {
            return None;
        }
        let mut v = toml::Value::try_from(config).ok()?;
        for (key, before, after) in &self.keys {
            let Some((t, k)) = parent_mut(&mut v, key) else {
                continue;
            };
            if t.get(k) != after.as_ref() {
                continue;
            }
            match before {
                Some(b) => {
                    t.insert(k.to_string(), b.clone());
                }
                None => {
                    t.remove(k);
                }
            }
        }
        v.try_into().ok()
    }

    /// The settings changed, as `key = value` lines.
    pub fn describe(&self) -> Vec<String> {
        (self.keys.iter())
            .map(|(k, _, after)| match after {
                Some(a) => format!("{k} = {a}"),
                None => format!("{k} (default)"),
            })
            .collect()
    }
}
