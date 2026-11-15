//! What a link holds, for an index of an event's works: whether it still
//! downloads, and what comes: the song itself (its charts with their
//! sounds), an add-on (charts without most of their sounds: a 差分), or
//! media alone (a BGA, a sound pack). A player can then try the working
//! links first and put add-ons into the song's folder.
//!
//! Natively only: the archive is downloaded and unpacked into a temporary
//! folder, then removed.

use crate::fs::Disk;
use crate::hosts::{Fetch, Want};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// An archive came.
    Archive,
    /// A chart file of its own came.
    Chart,
    /// A page (a person must download from it).
    Page,
    /// Not there any more (HTTP 404 / 410).
    Gone,
    /// It failed otherwise (maybe for now: a quota, a timeout).
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The song: a chart with its sounds.
    Main,
    /// Charts without most of their sounds (a 差分 for the song).
    Addon,
    /// No chart: a BGA, sounds, a readme.
    Media,
}

/// What a link gave.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Checked {
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// zip / 7z / rar / bms…
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub format: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub charts: usize,
    /// A chart's title (the first with the most sounds there).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

impl Checked {
    fn state(state: State, error: String) -> Checked {
        Checked {
            state,
            kind: None,
            format: String::new(),
            size: 0,
            charts: 0,
            title: String::new(),
            error,
        }
    }

    /// It downloads, as the song itself.
    pub fn is_main(&self) -> bool {
        matches!(self.state, State::Archive | State::Chart) && self.kind == Some(Kind::Main)
    }
}

/// A share of a chart's sounds an archive must hold for it to be the song.
const MAIN_COVERAGE: f32 = 0.9;

/// What the files unpacked into `dir` are: (kind, charts, a title).
pub fn classify(dir: &Path) -> (Kind, usize, String) {
    let mut charts: Vec<(f32, String)> = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let mut names = Vec::new();
        let mut here = Vec::new();
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                names.push(crate::import::file_name(&p));
                if crate::placement::is_chart(&p) {
                    here.push(p);
                }
            }
        }
        // (a few charts per folder tell; a pack can hold hundreds)
        for c in here.iter().take(8) {
            if let Some(info) = crate::placement::chart_info(&Disk, c) {
                charts.push((crate::placement::coverage(&names, &info.sounds), info.title));
            }
        }
        if here.len() > 8 {
            charts.extend((8..here.len()).map(|_| (-1.0, String::new())));
        }
    }
    let n = charts.len();
    let best = charts
        .iter()
        .filter(|(c, _)| *c >= 0.0)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .cloned();
    match best {
        None if n == 0 => (Kind::Media, 0, String::new()),
        None => (Kind::Addon, n, String::new()),
        Some((c, title)) if c >= MAIN_COVERAGE => (Kind::Main, n, title),
        Some((_, title)) => (Kind::Addon, n, title),
    }
}

/// What `link` gives now (the song `want`'s, if it is a pack of many);
/// files go through `tmp`, which is emptied after.
pub fn probe(link: &str, want: Option<Want>, tmp: &Path) -> Checked {
    let _ = std::fs::remove_dir_all(tmp);
    if let Err(e) = std::fs::create_dir_all(tmp) {
        return Checked::state(State::Error, e.to_string());
    }
    let file = tmp.join("download");
    let r = if crate::install::chart_link(link) {
        crate::hosts::download_to(link, &file)
            .map_err(Fetch::Failed)
            .and_then(|_| {
                let page =
                    std::fs::read(&file).is_ok_and(|b| b.trim_ascii_start().starts_with(b"<"));
                if page {
                    Err(Fetch::Page)
                } else {
                    Ok(())
                }
            })
    } else {
        crate::hosts::fetch_for(link, want, &file)
    };
    let out = match r {
        Err(Fetch::Page) => Checked::state(State::Page, String::new()),
        Err(Fetch::Failed(e)) => {
            let gone = ["HTTP 404", "HTTP 410"].iter().any(|g| e.contains(g));
            Checked::state(if gone { State::Gone } else { State::Error }, e)
        }
        Ok(()) => {
            let size = std::fs::metadata(&file).map_or(0, |m| m.len());
            if crate::install::chart_link(link) {
                Checked {
                    state: State::Chart,
                    kind: Some(Kind::Addon),
                    format: "bms".into(),
                    size,
                    charts: 1,
                    ..Checked::state(State::Chart, String::new())
                }
            } else {
                let format = crate::hosts::sniff(&file).unwrap_or("zip").to_string();
                let dir: PathBuf = tmp.join("unpacked");
                match crate::import::unpack_into(&file, &format!("a.{format}"), &dir) {
                    Ok(_) => {
                        let _ = std::fs::remove_file(&file);
                        let (kind, charts, title) = classify(&dir);
                        Checked {
                            state: State::Archive,
                            kind: Some(kind),
                            format,
                            size,
                            charts,
                            title,
                            error: String::new(),
                        }
                    }
                    Err(e) => Checked {
                        format,
                        size,
                        ..Checked::state(State::Error, format!("unpacking: {e}"))
                    },
                }
            }
        }
    };
    let _ = std::fs::remove_dir_all(tmp);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        let d = std::env::temp_dir().join(format!("bmseeker-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let put = |p: &str, b: &str| {
            let p = d.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b).unwrap();
        };
        let chart = "#TITLE Song\n#WAV01 a.wav\n#WAV02 b.wav\n#00111:0102\n";
        put("song/x.bms", chart);
        put("song/a.ogg", "");
        put("song/b.ogg", "");
        assert_eq!(classify(&d).0, Kind::Main);
        assert_eq!(classify(&d).2, "Song");
        std::fs::remove_file(d.join("song/b.ogg")).unwrap();
        assert_eq!(classify(&d).0, Kind::Addon);
        let _ = std::fs::remove_dir_all(&d);
        put("bga/movie.mp4", "");
        assert_eq!(classify(&d), (Kind::Media, 0, String::new()));
        let _ = std::fs::remove_dir_all(&d);
    }
}
