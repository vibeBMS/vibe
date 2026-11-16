//! `link resolve`, `link get`, `song find`.

use crate::{Run, SongArgs};
use bmseeker::hosts::{self, Fetch, Resolver};
use bmseeker::links::Links;
use bmseeker::net::{run, Client};
use bmseeker::sources::{Query, Sources};
use clap::Subcommand;
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum Link {
    /// What each link leads to: a file (its URL and name), a page for a
    /// browser, or an error. Nothing is downloaded.
    Resolve {
        #[arg(required = true)]
        links: Vec<String>,
        #[command(flatten)]
        song: SongArgs,
    },
    /// Download the file the links lead to, trying them in turn.
    Get {
        links: Vec<String>,
        /// A folder (the file keeps its own name) or a file. Default: here.
        #[arg(short, long, default_value = ".")]
        out: PathBuf,
        #[command(flatten)]
        song: SongArgs,
        /// Then the song's links from its sources (BMS SEARCH...).
        #[arg(long)]
        search: bool,
        /// Keep a file that is not an archive when it is no page (a chart
        /// or movie posted on its own).
        #[arg(long)]
        any: bool,
    },
}

#[derive(Subcommand)]
pub enum Song {
    /// The links the sources know for a song (or its separate chart).
    Find {
        #[command(flatten)]
        song: SongArgs,
        /// Its separate chart's links (差分), not the song's.
        #[arg(long)]
        diff: bool,
        /// Difficulty tables whose links count ("table" source).
        #[arg(long = "table", value_name = "URL")]
        tables: Vec<String>,
        /// TSV lists of links (md5, url_diff, url; a file or URL).
        #[arg(long = "list", value_name = "FILE|URL")]
        lists: Vec<String>,
    },
}

pub fn link(r: &Run, c: Link) -> Result<(), String> {
    match c {
        Link::Resolve { links, song } => {
            let http = Client::new(r.config());
            for l in links {
                let mut res = Resolver::new(&l, Some(song.want()), &r.cx);
                let got = run(&mut res, &http);
                let host = res.host();
                let rec = match &got {
                    Ok(t) => json!({"link": l, "state": "file", "host": host, "url": t.url,
                        "name": t.name, "encrypted": t.mega_key.is_some()}),
                    Err(Fetch::Page) => json!({"link": l, "state": "page", "host": host}),
                    Err(Fetch::Failed(e)) => {
                        json!({"link": l, "state": state_of(e), "host": host, "error": e})
                    }
                };
                r.out.record(rec, || match got {
                    Ok(t) => format!(
                        "{l}\n  file: {}{}",
                        t.url,
                        t.name.map(|n| format!(" ({n})")).unwrap_or_default()
                    ),
                    Err(Fetch::Page) => format!("{l}\n  a page: open it in a browser"),
                    Err(Fetch::Failed(e)) => format!("{l}\n  {e}"),
                });
            }
            Ok(())
        }
        Link::Get {
            links,
            out,
            song,
            search,
            any,
        } => get(r, &links, &out, &song, search, any),
    }
}

/// "gone" for a file that is no more (404 / 410, MEGA's -9), else "error".
pub fn state_of(e: &str) -> &'static str {
    if ["HTTP 404", "HTTP 410", "[-9]", "[-2]"]
        .iter()
        .any(|g| e.contains(g))
    {
        "gone"
    } else {
        "error"
    }
}

fn get(
    r: &Run,
    links: &[String],
    out: &Path,
    song: &SongArgs,
    search: bool,
    any: bool,
) -> Result<(), String> {
    if links.is_empty() && !search {
        return Err("no link (give links, or --search with --title / --md5)".into());
    }
    let into_dir = out.is_dir() || out.as_os_str().to_string_lossy().ends_with('/');
    if into_dir {
        std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    let tmp = if into_dir {
        out.join(format!(".bmseeker-{}", std::process::id()))
    } else {
        out.to_path_buf()
    };
    let mut errors = Vec::new();
    // links that gave a page: a browser may get the file from them
    let mut pages = Vec::new();
    let mut tried: Vec<String> = Vec::new();
    let mut try_link = |l: &str, errors: &mut Vec<String>, pages: &mut Vec<String>| {
        if tried.iter().any(|t| t == l) {
            return None;
        }
        tried.push(l.to_string());
        match hosts::fetch_named(l, Some(song.want()), &tmp, any, &r.cx) {
            Ok(name) => Some((l.to_string(), name)),
            Err(Fetch::Page) => {
                errors.push(format!("{l}: a page, not a file"));
                pages.push(l.to_string());
                None
            }
            Err(Fetch::Failed(e)) => {
                errors.push(format!("{l}: {e}"));
                None
            }
        }
    };
    let mut got = links
        .iter()
        .find_map(|l| try_link(l, &mut errors, &mut pages));
    if got.is_none() && search {
        let sources = Sources::from_config(r.config(), Vec::new());
        let q = Query {
            md5: song.md5.clone(),
            want: song.want(),
            diff: false,
        };
        let found = run(&mut sources.find_all(&q), &Client::new(r.config()));
        if found.is_empty() {
            errors.push("the sources know no other link".into());
        }
        got = found
            .iter()
            .find_map(|(_, l)| try_link(l, &mut errors, &mut pages));
    }
    let Some((link, name)) = got else {
        let _ = std::fs::remove_file(&tmp);
        r.out.record(json!({"errors": errors, "pages": pages}), || {
            errors.join("\n")
        });
        return Err("no link gave the file".into());
    };
    let file = if into_dir {
        let f = out.join(&name);
        std::fs::rename(&tmp, &f).map_err(|e| format!("{}: {e}", f.display()))?;
        f
    } else {
        tmp
    };
    let format = hosts::sniff(&file).unwrap_or("file");
    r.out.record(
        json!({"file": file, "name": name, "format": format, "link": link, "errors": errors}),
        || format!("{} ({format}, from {link})", file.display()),
    );
    Ok(())
}

pub fn song(r: &Run, c: Song) -> Result<(), String> {
    let Song::Find {
        song,
        diff,
        tables,
        lists,
    } = c;
    let mut known = vec![("table", Links::default())];
    for t in &tables {
        match crate::table::load(r, t) {
            Ok(t) => known[0].1 = std::mem::take(&mut known[0].1).and(Links::from_tables([&t])),
            Err(e) => r.out.note(format!("{t}: {e}")),
        }
    }
    if r.config().uses("stella") {
        if let Some(b) = fetch_list(&r.config().stella_list, r) {
            known.push(("stella", Links::from_stella(&b)));
        }
    }
    let mut tsv = Links::default();
    for l in &lists {
        if let Some(b) = fetch_list(l, r) {
            tsv = tsv.and(Links::from_tsv(&String::from_utf8_lossy(&b)));
        }
    }
    known.push(("tsv", tsv));
    let sources = Sources::from_config(r.config(), known);
    let q = Query {
        md5: song.md5.clone(),
        want: song.want(),
        diff,
    };
    for (source, url) in run(&mut sources.find_all(&q), &Client::new(r.config())) {
        r.out.record(json!({"source": source, "url": url}), || {
            format!("{source}\t{url}")
        });
    }
    Ok(())
}

/// A list's bytes: a URL or a file (a failure noted, not fatal).
pub fn fetch_list(at: &str, r: &Run) -> Option<Vec<u8>> {
    let got = if at.starts_with("http") {
        bmseeker::net::Http::send(
            &Client::new(r.config()),
            &bmseeker::net::Request::Get(at.into()),
        )
    } else {
        std::fs::read(at).map_err(|e| e.to_string())
    };
    got.map_err(|e| r.out.note(format!("{at}: {e}"))).ok()
}
