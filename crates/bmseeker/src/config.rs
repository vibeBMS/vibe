//! Every place bmseeker reads from, and how it behaves, in one value. The
//! defaults are the public services BMS players use; a JSON file (or the
//! fields a program sets) changes any of them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Sent with requests (hosts answer browsers better).
    pub user_agent: String,
    /// The BMS event mirror's list of events (vibeBMS/BMS `events.json`):
    /// a manbow work it holds is downloaded from there. Empty: none.
    pub mirror_catalog: String,
    /// manbow's event pages (`event.cgi`).
    pub manbow: String,
    /// The BMS SEARCH API.
    pub bmssearch_api: String,
    /// The Stella uploader's list of charts and their links.
    pub stella_list: String,
    /// The MEGA API.
    pub mega_api: String,
    /// Where else a song's links are looked for, in order: `table` (the
    /// difficulty tables given), `stella`, `tsv` (link lists given),
    /// `bmssearch`.
    pub sources: Vec<String>,
    /// Difficulty tables to use when none is given.
    pub tables: Vec<String>,
    /// The folder (in a library) downloads are kept in until imported.
    pub downloads: String,
    /// How much of a chart's sounds a song folder must hold to take it
    /// (0..1).
    pub min_coverage: f32,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36".into(),
            mirror_catalog: "https://raw.githubusercontent.com/vibeBMS/BMS/main/events.json".into(),
            manbow: "https://manbow.nothing.sh/event/event.cgi".into(),
            bmssearch_api: "https://api.bmssearch.net/v1".into(),
            stella_list: "https://stellabms.xyz/score_upload_full.json".into(),
            mega_api: "https://g.api.mega.co.nz/cs?id=0".into(),
            sources: ["table", "stella", "tsv", "bmssearch"].map(String::from).to_vec(),
            tables: [
                "https://darksabun.club/table/archive/insane1/",
                "https://darksabun.club/table/archive/normal1/",
            ]
            .map(String::from)
            .to_vec(),
            downloads: ".bmseeker".into(),
            min_coverage: 0.99,
        }
    }
}

impl Config {
    /// A JSON file's settings over the defaults (fields left out keep
    /// theirs).
    pub fn from_json(text: &str) -> Result<Config, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    /// Whether `name` is one of the [`Config::sources`].
    pub fn uses(&self, name: &str) -> bool {
        self.sources.iter().any(|s| s == name)
    }

    /// The settings file: `path`, else `$BMSEEKER_CONFIG`, else
    /// `<config folder>/bmseeker/config.json` if there is one, else the
    /// defaults.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: Option<&std::path::Path>) -> Result<Config, String> {
        let file = path
            .map(std::path::Path::to_path_buf)
            .or_else(|| std::env::var_os("BMSEEKER_CONFIG").map(Into::into))
            .or_else(|| {
                let dir = std::env::var_os("XDG_CONFIG_HOME")
                    .map(std::path::PathBuf::from)
                    .or_else(|| std::env::var_os("APPDATA").map(Into::into))
                    .or_else(|| {
                        std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config"))
                    })?;
                Some(dir.join("bmseeker").join("config.json")).filter(|f| f.exists())
            });
        match file {
            None => Ok(Config::default()),
            Some(f) => {
                let text =
                    std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
                Config::from_json(&text).map_err(|e| format!("{}: {e}", f.display()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_files() {
        let c = Config::from_json(r#"{"mirror_catalog": "", "sources": ["bmssearch"]}"#).unwrap();
        assert_eq!(c.mirror_catalog, "");
        assert!(c.uses("bmssearch") && !c.uses("stella"));
        assert_eq!(c.manbow, Config::default().manbow);
        assert!(Config::from_json("{").is_err());
    }
}
