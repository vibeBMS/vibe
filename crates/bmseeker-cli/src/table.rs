//! `table show`, `table missing`, `table export`.

use crate::library::{done_record, scan};
use crate::link::fetch_list;
use crate::Run;
use bmseeker::export;
use bmseeker::fs::Disk;
use bmseeker::hosts::Want;
use bmseeker::install::{self, Done};
use bmseeker::links::Links;
use bmseeker::net::Client;
use bmseeker::sources::{Query, Sources};
use bmseeker::table::{fetch_table, DifficultyTable, TableEntry};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum Table {
    /// A table's name and its levels, with how many charts each.
    Show { url: String },
    /// The table's charts a library lacks; --get downloads them.
    Missing {
        url: String,
        #[arg(short = 'L', long)]
        library: PathBuf,
        /// One level only.
        #[arg(short, long)]
        level: Option<String>,
        /// Download them (each song, then its separate chart).
        #[arg(long)]
        get: bool,
        #[command(flatten)]
        links: LinkArgs,
    },
    /// The table for LR2 (custom folders) or beatoraja (a table cache).
    Export {
        #[arg(value_enum)]
        to: ExportTo,
        url: String,
        /// LR2: a folder to add to LR2; beatoraja: its table folder.
        out: PathBuf,
        #[command(flatten)]
        links: LinkArgs,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ExportTo {
    Lr2,
    Bmt,
}

/// Where else charts' links are looked for, by MD5.
#[derive(Args, Clone, Default)]
pub struct LinkArgs {
    /// Other tables whose links count.
    #[arg(long = "also", value_name = "URL")]
    also: Vec<String>,
    /// TSV lists of links (md5, url_diff, url; a file or URL).
    #[arg(long = "list", value_name = "FILE|URL")]
    lists: Vec<String>,
    /// The table's own links only.
    #[arg(long)]
    own_links: bool,
}

pub fn table(r: &Run, c: Table) -> Result<(), String> {
    match c {
        Table::Show { url } => {
            let t = load(r, &url)?;
            r.out.record(
                json!({"table": t.header.name, "symbol": t.header.symbol, "charts": t.entries.len()}),
                || format!("{} [{}]: {} charts", t.header.name, t.header.symbol, t.entries.len()),
            );
            for l in t.levels() {
                let n = t.entries.iter().filter(|e| e.level == l).count();
                r.out.record(
                    json!({"level": l, "label": t.label(&l), "charts": n}),
                    || format!("  {}  {n}", t.label(&l)),
                );
            }
            Ok(())
        }
        Table::Missing {
            url,
            library,
            level,
            get,
            links,
        } => missing(r, &url, &library, level.as_deref(), get, &links),
        Table::Export {
            to,
            url,
            out,
            links,
        } => {
            let (t, _) = load_filled(r, &url, &links)?;
            let write = |p: &Path, b: &[u8]| {
                if let Some(d) = p.parent() {
                    std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
                }
                std::fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()))
            };
            match to {
                ExportTo::Lr2 => {
                    let dir = out.join(install::safe_name(&t.header.name));
                    // the levels a table no longer has go
                    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                        if e.path().extension().is_some_and(|x| x == "lr2folder") {
                            let _ = std::fs::remove_file(e.path());
                        }
                    }
                    let folders = export::lr2_folders(&t);
                    for (name, text) in &folders {
                        write(&dir.join(name), text)?;
                    }
                    r.out
                        .record(json!({"folder": dir, "folders": folders.len()}), || {
                            format!("{}: {} folders", dir.display(), folders.len())
                        });
                }
                ExportTo::Bmt => {
                    let file = out.join(export::bmt_file_name(&t.source_url));
                    write(&file, &export::beatoraja_bmt(&t))?;
                    r.out
                        .record(json!({"file": file}), || file.display().to_string());
                }
            }
            Ok(())
        }
    }
}

fn missing(
    r: &Run,
    url: &str,
    library: &Path,
    level: Option<&str>,
    get: bool,
    args: &LinkArgs,
) -> Result<(), String> {
    let (t, links) = load_filled(r, url, args)?;
    let have = scan(library, &r.config().downloads);
    let listed: Vec<&TableEntry> = t
        .entries
        .iter()
        .filter(|e| level.is_none_or(|l| e.level == l))
        .collect();
    let missing: Vec<&TableEntry> = listed
        .iter()
        .copied()
        .filter(|e| !have.contains(&e.md5) && !have.contains(&e.sha256))
        .collect();
    for e in &missing {
        r.out.record(
            json!({"level": e.level, "label": t.label(&e.level), "title": e.title, "artist": e.artist,
                "md5": e.md5, "url": e.url, "url_diff": e.url_diff}),
            || format!("{}\t{} — {}\t{}\t{}", t.label(&e.level), e.title, e.artist, e.url, e.url_diff),
        );
    }
    r.out.note(format!(
        "{} of {} charts missing",
        missing.len(),
        listed.len()
    ));
    if !get {
        return Ok(());
    }
    // the lists' links come with the table's; BMS SEARCH after (unless
    // --own-links)
    let sources = if args.own_links {
        Sources::default()
    } else {
        Sources::from_config(r.config(), Vec::new())
    };
    let downloads = library.join(&r.config().downloads);
    for e in missing {
        let want = Want {
            title: e.title.clone(),
            artist: e.artist.clone(),
        };
        let mut song: Vec<Done> = Vec::new();
        for (own, diff) in [(links.song_links(e), false), (links.chart_links(e), true)] {
            // the song brought the chart: no separate chart to get
            let has = install::added(&song).any(|dir| install::holds_chart(&Disk, dir, &e.md5));
            if diff && (has || own.is_empty()) {
                break;
            }
            let q = Query {
                md5: e.md5.clone(),
                want: want.clone(),
                diff,
            };
            let done = install::links(&downloads, library, &q, &own, &sources, &r.cx);
            for d in &done {
                r.out.record(done_record(&e.title, d), || {
                    crate::library::done_text(&e.title, d)
                });
            }
            if !diff {
                song = done;
            }
        }
    }
    Ok(())
}

pub fn load(r: &Run, url: &str) -> Result<DifficultyTable, String> {
    fetch_table(&Client::new(r.config()), url).map_err(|e| e.to_string())
}

/// A table and the other links known for its charts (none with
/// `--own-links`); its missing or dead links filled.
fn load_filled(r: &Run, url: &str, args: &LinkArgs) -> Result<(DifficultyTable, Links), String> {
    let mut t = load(r, url)?;
    if args.own_links {
        return Ok((t, Links::default()));
    }
    let mut links = Links::default();
    for name in &r.config().sources {
        match name.as_str() {
            "table" => {
                links = links.and(Links::from_tables([&t]));
                for u in &args.also {
                    match load(r, u) {
                        Ok(other) => links = links.and(Links::from_tables([&other])),
                        Err(e) => r.out.note(format!("{u}: {e}")),
                    }
                }
            }
            "stella" => {
                if let Some(b) = fetch_list(&r.config().stella_list, r) {
                    links = links.and(Links::from_stella(&b));
                }
            }
            "tsv" => {
                for l in &args.lists {
                    if let Some(b) = fetch_list(l, r) {
                        links = links.and(Links::from_tsv(&String::from_utf8_lossy(&b)));
                    }
                }
            }
            _ => {}
        }
    }
    let n = links.fill(&mut t.entries);
    r.out.note(format!(
        "{n} charts' links filled; links known for {} charts",
        links.len()
    ));
    Ok((t, links))
}
