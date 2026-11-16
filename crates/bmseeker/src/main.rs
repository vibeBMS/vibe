//! `bmseeker`: difficulty tables and the songs they list, from a terminal.
//!
//! ```text
//! bmseeker table <table URL>                      the table's levels
//! bmseeker missing <table URL> <library> [-l LEVEL] [--get]
//!                                                 its songs the library lacks
//!                                                 (--get: download them)
//! bmseeker fetch <link>... <file or folder> [-t TITLE] [-a ARTIST]
//!         [--md5 MD5] [--search] [--any] [--json]
//!                                                 the archive the links lead
//!                                                 to, tried in turn (then BMS
//!                                                 SEARCH's for the song with
//!                                                 --search / --md5); into a
//!                                                 folder under its own name
//! bmseeker lookup [-t TITLE] [-a ARTIST] [--md5 MD5]
//!                                                 a song's download links on
//!                                                 BMS SEARCH
//! bmseeker install <link>... <library> [--diff] [-t TITLE] [-a ARTIST] [--md5 MD5]
//!                                                 a song (or its separate
//!                                                 chart) into the library, the
//!                                                 links tried in turn
//! bmseeker import <file> <library>                a downloaded file into it
//! bmseeker place <chart> <library> [--to DIR] [--min PERCENT]
//!                                                 a chart's song folder: the
//!                                                 candidates, or move it there
//! bmseeker pending <library>                      charts waiting for their song
//! bmseeker manbow <event number> <folder>
//!                                                 a manbow event's works, each
//!                                                 with its links and which to
//!                                                 take, into <folder>/works.json
//!                                                 (<folder>/overrides.json:
//!                                                 the choices made by hand)
//! bmseeker check-links <works.json> <links.json> [--days N] [--minutes M]
//!                                                 what each work's links give
//!                                                 (an event index's: main /
//!                                                 add-on / media, gone…)
//! bmseeker export lr2 <table URL> <folder>
//!                                                 the table as LR2 custom
//!                                                 folders (add <folder> to LR2)
//! bmseeker export bmt <table URL> <beatoraja table folder>
//!                                                 the table as beatoraja's cache
//! ```
//!
//! `missing` and `export` look a chart's other links up by its MD5 (where
//! its own is missing, dead, or does not download): in the table, the
//! tables `--also <URL>` names, the Stella uploader's list, TSV lists
//! `--links <file or URL>` names (`md5`, `url_diff`, `url`), and last (for a
//! song, when downloading; `install --md5` too) BMS SEARCH; `--own-links`:
//! the table's links only.

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = cli::run(&args) {
        eprintln!("bmseeker: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod cli {
    use bmseeker::export;
    use bmseeker::fs::Disk;
    use bmseeker::hosts::{self, Fetch, Want};
    use bmseeker::install::{self, Done};
    use bmseeker::links::Links;
    use bmseeker::placement::{self, Rules};
    use bmseeker::table::{self, DifficultyTable, TableEntry};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    const USAGE: &str = "usage:
  bmseeker table <table URL>
  bmseeker missing <table URL> <library> [-l LEVEL] [--get]
  bmseeker fetch <link>... <file or folder> [-t TITLE] [-a ARTIST] [--md5 MD5] [--search] [--any] [--json]
  bmseeker lookup [-t TITLE] [-a ARTIST] [--md5 MD5]
  bmseeker install <link>... <library> [--diff] [-t TITLE] [-a ARTIST] [--md5 MD5]
  bmseeker import <file> <library>
  bmseeker place <chart> <library> [--to DIR] [--min PERCENT]
  bmseeker pending <library>
  bmseeker manbow <event number> <folder>
  bmseeker check-links <works.json> <links.json> [--days N] [--minutes M]
  bmseeker export lr2 <table URL> <folder>
  bmseeker export bmt <table URL> <beatoraja table folder>
(missing / export: [--also TABLE_URL]... [--links TSV]... [--own-links])";

    /// The arguments: positional ones, and the value of each `-x VALUE` /
    /// `--flag`.
    struct Args<'a> {
        pos: Vec<&'a str>,
        opts: Vec<(&'a str, Option<&'a str>)>,
    }

    impl<'a> Args<'a> {
        fn parse(args: &'a [String], with_value: &[&str]) -> Args<'a> {
            let (mut pos, mut opts) = (Vec::new(), Vec::new());
            let mut it = args.iter();
            while let Some(a) = it.next() {
                if with_value.contains(&a.as_str()) {
                    opts.push((a.as_str(), it.next().map(String::as_str)));
                } else if a.starts_with('-') {
                    opts.push((a.as_str(), None));
                } else {
                    pos.push(a.as_str());
                }
            }
            Args { pos, opts }
        }
        /// Every value of `-x VALUE` given more than once.
        fn all(&self, name: &str) -> Vec<&'a str> {
            self.opts
                .iter()
                .filter(|(n, _)| *n == name)
                .filter_map(|(_, v)| *v)
                .collect()
        }
        fn get(&self, name: &str) -> Option<&'a str> {
            self.opts
                .iter()
                .find(|(n, _)| *n == name)
                .and_then(|(_, v)| *v)
        }
        fn has(&self, name: &str) -> bool {
            self.opts.iter().any(|(n, _)| *n == name)
        }
        fn need(&self, i: usize) -> Result<&'a str, String> {
            self.pos
                .get(i + 1)
                .copied()
                .ok_or_else(|| USAGE.to_string())
        }
        fn want(&self) -> Want {
            Want {
                title: self.get("-t").unwrap_or("download").into(),
                artist: self.get("-a").unwrap_or_default().into(),
            }
        }
    }

    pub fn run(args: &[String]) -> Result<(), String> {
        let a = Args::parse(
            args,
            &[
                "-l",
                "-t",
                "-a",
                "--to",
                "--min",
                "--also",
                "--links",
                "--md5",
                "--days",
                "--minutes",
            ],
        );
        match a.pos.first().copied() {
            Some("table") => {
                let t = load(a.need(0)?)?;
                println!(
                    "{} [{}]: {} charts",
                    t.header.name,
                    t.header.symbol,
                    t.entries.len()
                );
                for l in t.levels() {
                    let n = t.entries.iter().filter(|e| e.level == l).count();
                    println!("  {}  {n}", t.label(&l));
                }
                Ok(())
            }
            Some("missing") => {
                let (t, links) = load_filled(a.need(0)?, &a)?;
                let library = PathBuf::from(a.need(1)?);
                let have = scan(&library);
                let level = a.get("-l");
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
                    println!(
                        "{}\t{} — {}\t{}\t{}",
                        t.label(&e.level),
                        e.title,
                        e.artist,
                        e.url,
                        e.url_diff
                    );
                }
                eprintln!("{} of {} charts missing", missing.len(), listed.len());
                if a.has("--get") {
                    for e in missing {
                        let want = Want {
                            title: e.title.clone(),
                            artist: e.artist.clone(),
                        };
                        // the table's links, then the others known for the chart
                        // (and BMS SEARCH's for the song, unless --own-links)
                        let md5 = (!a.has("--own-links")).then_some(e.md5.as_str());
                        let mut song = Vec::new();
                        for (ls, diff) in
                            [(links.song_links(e), false), (links.chart_links(e), true)]
                        {
                            // the song brought the chart: no separate chart to get
                            let has = install::added(&song)
                                .any(|dir| install::holds_chart(&Disk, dir, &e.md5));
                            if diff && has {
                                break;
                            }
                            if !ls.is_empty() || (!diff && md5.is_some()) {
                                let downloads = library.join(DOWNLOADS);
                                let q = bmseeker::sources::Query {
                                    md5: md5.unwrap_or_default().to_string(),
                                    want: want.clone(),
                                    diff,
                                };
                                let cx = hosts::Context::default();
                                let sources =
                                    bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
                                let done =
                                    install::links(&downloads, &library, &q, &ls, &sources, &cx);
                                if !diff {
                                    song = done
                                        .iter()
                                        .filter_map(|d| match d {
                                            Done::Added(p, n) => Some(Done::Added(p.clone(), *n)),
                                            _ => None,
                                        })
                                        .collect();
                                }
                                report(&e.title, done);
                            }
                        }
                    }
                }
                Ok(())
            }
            Some("fetch") => {
                let (out, links) = match a.pos.split_last() {
                    Some((out, rest))
                        if rest.len() >= 2 || (rest.len() == 1 && a.has("--search")) =>
                    {
                        (Path::new(*out), &rest[1..])
                    }
                    _ => return Err(USAGE.into()),
                };
                fetch(links, out, &a)
            }
            Some("lookup") => {
                for l in bmseeker::bmssearch::lookup(
                    a.get("--md5").unwrap_or(""),
                    &a.want(),
                    &Default::default(),
                ) {
                    println!("{l}");
                }
                Ok(())
            }
            Some("install") => {
                // <link>... <library>: the links tried in turn
                let (library, links) = match a.pos.split_last() {
                    Some((lib, rest))
                        if rest.len() >= 2 || (a.has("--md5") && !rest.is_empty()) =>
                    {
                        (PathBuf::from(lib), &rest[1..])
                    }
                    _ => return Err(USAGE.into()),
                };
                let links: Vec<String> = links.iter().map(|l| l.to_string()).collect();
                let want = a.want();
                let downloads = library.join(DOWNLOADS);
                let md5 = a.get("--md5");
                let q = bmseeker::sources::Query {
                    md5: md5.unwrap_or_default().to_string(),
                    want: want.clone(),
                    diff: a.has("--diff"),
                };
                let cx = hosts::Context::default();
                let sources = bmseeker::sources::Sources::from_config(&cx.config, Vec::new());
                let done = install::links(&downloads, &library, &q, &links, &sources, &cx);
                report(&want.title, done);
                Ok(())
            }
            Some("import") => {
                let (file, library) = (Path::new(a.need(0)?), Path::new(a.need(1)?));
                let r = bmseeker::import::import(&Disk, file, library)?;
                println!("{r:?}");
                Ok(())
            }
            Some("place") => {
                let (chart, library) = (Path::new(a.need(0)?), Path::new(a.need(1)?));
                if let Some(dir) = a.get("--to") {
                    return match placement::bring(&Disk, chart, Path::new(dir)) {
                        Some(at) => {
                            println!("{}", at.display());
                            Ok(())
                        }
                        None => Err(format!("{} stays where it is", chart.display())),
                    };
                }
                let mut rules = Rules::default();
                if let Some(m) = a.get("--min").and_then(|m| m.parse::<f32>().ok()) {
                    rules.min_coverage = m / 100.0;
                }
                let siblings = !chart.parent().is_some_and(|p| p.ends_with(DOWNLOADS));
                let (located, candidates) =
                    placement::locate_with(&Disk, chart, library, siblings, rules)
                        .ok_or_else(|| format!("{}: not a chart", chart.display()))?;
                println!("{located:?}");
                for (c, label) in candidates.iter().take(10) {
                    println!(
                        "  {:>5.1}% {} {label}",
                        c.coverage * 100.0,
                        if c.same_song {
                            "same song "
                        } else {
                            "          "
                        },
                    );
                }
                Ok(())
            }
            Some("pending") => {
                let library = PathBuf::from(a.need(0)?);
                let waiting = library.join(DOWNLOADS);
                let charts: Vec<PathBuf> = std::fs::read_dir(&waiting)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| placement::is_chart(p))
                    .collect();
                for c in &charts {
                    let (located, best) =
                        placement::locate_with(&Disk, c, &library, false, Rules::default())
                            .unwrap_or((placement::Located::NoSong, Vec::new()));
                    let best = best
                        .first()
                        .map(|(c, l)| format!("  best: {:.1}% {l}", c.coverage * 100.0))
                        .unwrap_or_default();
                    println!("{}  {located:?}{best}", c.display());
                }
                eprintln!("{} charts waiting", charts.len());
                Ok(())
            }
            Some("export") => {
                let (kind, out) = (a.need(0)?, PathBuf::from(a.need(2)?));
                let (t, _) = load_filled(a.need(1)?, &a)?;
                let write = |p: &Path, b: &[u8]| {
                    if let Some(d) = p.parent() {
                        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
                    }
                    std::fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()))
                };
                match kind {
                    "lr2" => {
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
                        println!("{}: {} folders", dir.display(), folders.len());
                    }
                    "bmt" => {
                        let file = out.join(export::bmt_file_name(&t.source_url));
                        write(&file, &export::beatoraja_bmt(&t))?;
                        println!("{}", file.display());
                    }
                    _ => return Err(USAGE.into()),
                }
                Ok(())
            }
            Some("manbow") => {
                let event: u32 = a.need(0)?.parse().map_err(|_| USAGE.to_string())?;
                manbow_works(event, Path::new(a.need(1)?))
            }
            Some("check-links") => check_links(
                Path::new(a.need(0)?),
                Path::new(a.need(1)?),
                a.get("--days").and_then(|d| d.parse().ok()).unwrap_or(30),
                a.get("--minutes").and_then(|m| m.parse().ok()),
            ),
            _ => Err(USAGE.into()),
        }
    }

    /// A link's check, and when (Unix seconds).
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Dated {
        #[serde(flatten)]
        checked: bmseeker::probe::Checked,
        at: u64,
    }

    /// Check the links of the works in `works` (an event's works.json:
    /// `title`, `artist`, `links` of URLs or `{url}`) into `out` (URL →
    /// what it gave, and when), skipping those checked within `days` (a
    /// failure that may pass: within a day). Saved after each, so a run cut
    /// short (`minutes`) goes on from there next time.
    /// `fetch`: the first of the links (then BMS SEARCH's) that gives an
    /// archive (`--any`: or a file that is not a page), into `out` (a
    /// folder: under the file's own name).
    fn fetch(links: &[&str], out: &Path, a: &Args) -> Result<(), String> {
        let want = a.want();
        let into_dir = out.is_dir();
        let tmp = if into_dir {
            out.join(format!(".bmseeker-{}", std::process::id()))
        } else {
            out.to_path_buf()
        };
        let mut errors = Vec::new();
        let mut tried: Vec<String> = Vec::new();
        // links that gave a page: a browser may get the file from them
        let mut pages: Vec<String> = Vec::new();
        let mut try_link = |link: &str, errors: &mut Vec<String>| -> Option<(String, String)> {
            if tried.iter().any(|t| t == link) {
                return None;
            }
            tried.push(link.to_string());
            match hosts::fetch_named(
                link,
                Some(want.clone()),
                &tmp,
                a.has("--any"),
                &hosts::Context::default(),
            ) {
                Ok(name) => Some((link.to_string(), name)),
                Err(Fetch::Page) => {
                    errors.push(format!("{link}: a page, not a file"));
                    pages.push(link.to_string());
                    None
                }
                Err(Fetch::Failed(e)) => {
                    errors.push(format!("{link}: {e}"));
                    None
                }
            }
        };
        let mut got = links.iter().find_map(|l| try_link(l, &mut errors));
        if got.is_none() && (a.has("--search") || a.has("--md5")) {
            let found = bmseeker::bmssearch::lookup(
                a.get("--md5").unwrap_or(""),
                &want,
                &Default::default(),
            );
            if found.is_empty() {
                errors.push("BMS SEARCH: nothing for the song".into());
            }
            got = found.iter().find_map(|l| try_link(l, &mut errors));
        }
        drop(try_link);
        let json = a.has("--json");
        let Some((link, name)) = got else {
            let _ = std::fs::remove_file(&tmp);
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "errors": errors, "pages": pages })
                );
            }
            return Err(errors.join("\n"));
        };
        let file = if into_dir {
            let f = out.join(&name);
            std::fs::rename(&tmp, &f).map_err(|e| format!("{}: {e}", f.display()))?;
            f
        } else {
            tmp
        };
        let format = hosts::sniff(&file).unwrap_or("file");
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "file": file, "name": name, "format": format,
                    "link": link, "errors": errors,
                })
            );
        } else {
            println!("{} ({format}, from {link})", file.display());
        }
        Ok(())
    }

    /// `manbow`: `<dir>/works.json` for a manbow event, as the BMS mirror's
    /// tools read it.
    fn manbow_works(event: u32, dir: &Path) -> Result<(), String> {
        use bmseeker::manbow;
        use serde_json::{json, Map, Value};
        let page = |u: &str| bmseeker::net::get(u).map(|b| manbow::decode(&b));
        let base = bmseeker::config::Config::default().manbow;
        let works = manbow::works(
            &base,
            event,
            &page(&manbow::url_list_url(&base, event))?,
            &page(&manbow::work_list_url(&base, event))?,
        );
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let overrides: Map<String, Value> = std::fs::read(dir.join("overrides.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut out = Vec::new();
        for w in &works {
            let mut o = Map::new();
            o.insert("num".into(), json!(w.num));
            o.insert("title".into(), json!(w.title));
            o.insert("artist".into(), json!(w.artist));
            o.insert("team".into(), json!(w.team));
            o.insert("kind".into(), json!(w.kind));
            o.insert("size_kb".into(), json!(w.size_kb));
            o.insert("page".into(), json!(w.page));
            let links: Vec<Value> = w
                .links
                .iter()
                .map(|(l, u)| json!({ "label": l, "url": u }))
                .collect();
            o.insert("links".into(), Value::Array(links));
            o.insert("main".into(), json!(w.pick.main));
            o.insert("extra".into(), json!(w.pick.extra));
            if !w.pick.review.is_empty() {
                o.insert("review".into(), json!(w.pick.review));
            }
            if let Some(Value::Object(by_hand)) = overrides.get(&w.num.to_string()) {
                for (k, v) in by_hand {
                    o.insert(k.clone(), v.clone());
                }
                o.shift_remove("review");
            }
            out.push(Value::Object(o));
        }
        // as Python writes it (one space a level), which the repository keeps
        let mut buf = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
        serde::Serialize::serialize(&out, &mut ser).map_err(|e| e.to_string())?;
        buf.push(b'\n');
        std::fs::write(dir.join("works.json"), buf).map_err(|e| e.to_string())?;
        let review = works
            .iter()
            .filter(|w| !w.pick.review.is_empty() && !overrides.contains_key(&w.num.to_string()))
            .count();
        let disq = works.iter().filter(|w| w.kind == "Disq").count();
        println!(
            "{} works, {review} to review, {disq} disqualified",
            works.len()
        );
        Ok(())
    }

    fn check_links(
        works: &Path,
        out: &Path,
        days: u64,
        minutes: Option<u64>,
    ) -> Result<(), String> {
        use bmseeker::probe::State;
        use std::collections::BTreeMap;
        let read = |p: &Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
        let works: Vec<serde_json::Value> =
            serde_json::from_slice(&read(works)?).map_err(|e| e.to_string())?;
        let mut done: BTreeMap<String, Dated> = std::fs::read(out)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let now = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        };
        let start = now();
        let save = |done: &BTreeMap<String, Dated>| -> Result<(), String> {
            let j = serde_json::to_vec_pretty(done).map_err(|e| e.to_string())?;
            let tmp = out.with_extension("json~");
            std::fs::write(&tmp, j).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, out).map_err(|e| e.to_string())
        };
        let tmp = std::env::temp_dir().join(format!("bmseeker-check-{}", std::process::id()));
        let s = |w: &serde_json::Value, k: &str| {
            w.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
        };
        let (mut checked, mut skipped) = (0, 0);
        for w in &works {
            let want = Want {
                title: s(w, "title"),
                artist: s(w, "artist"),
            };
            let links: Vec<String> = w
                .get("links")
                .and_then(|l| l.as_array())
                .into_iter()
                .flatten()
                .filter_map(|l| l.as_str().or_else(|| l.get("url").and_then(|u| u.as_str())))
                .map(|u| u.trim().to_string())
                .filter(|u| u.starts_with("http"))
                .collect();
            for url in links {
                let fresh = done.get(&url).is_some_and(|d| {
                    let age = now().saturating_sub(d.at);
                    let keep = if d.checked.state == State::Error {
                        1
                    } else {
                        days
                    };
                    age < keep * 86_400
                });
                if fresh {
                    skipped += 1;
                    continue;
                }
                if minutes.is_some_and(|m| now() - start > m * 60) {
                    save(&done)?;
                    eprintln!("time is up: {checked} checked, {skipped} fresh; run again to go on");
                    return Ok(());
                }
                let c = bmseeker::probe::probe(
                    &url,
                    Some(want.clone()),
                    &tmp,
                    &hosts::Context::default(),
                );
                eprintln!(
                    "{} | {:?} {:?} {} MB {} charts {} | {url}",
                    want.title,
                    c.state,
                    c.kind,
                    c.size >> 20,
                    c.charts,
                    c.error
                );
                done.insert(
                    url,
                    Dated {
                        checked: c,
                        at: now(),
                    },
                );
                save(&done)?;
                checked += 1;
            }
        }
        save(&done)?;
        eprintln!("{checked} checked, {skipped} fresh");
        Ok(())
    }

    /// Where downloads wait in a library (charts until their song comes).
    const DOWNLOADS: &str = ".bmseeker";

    fn report(title: &str, done: Vec<Done>) {
        for d in done {
            match d {
                Done::Added(dir, n) => println!("{title}: added {} ({n} files)", dir.display()),
                Done::Placed(p) => println!("{title}: {}", p.display()),
                Done::Waiting(p) => println!("{title}: {} waits for its song", p.display()),
                Done::Page(u) => println!("{title}: a page, download it in a browser: {u}"),
                Done::Failed(e) => println!("{title}: failed: {e}"),
            }
        }
    }

    /// Other links by MD5: the tables `--also` names, the Stella uploader's
    /// list, and the TSV lists `--links` names (a file or URL each).
    fn links(t: &DifficultyTable, a: &Args) -> Links {
        let get = |url: &str| {
            if url.starts_with("http") {
                bmseeker::net::get(url)
            } else {
                std::fs::read(url).map_err(|e| e.to_string())
            }
            .map_err(|e| eprintln!("{url}: {e}"))
            .ok()
        };
        let mut out = Links::from_tables([t]);
        for url in a.all("--also") {
            match load(url) {
                Ok(other) => out = out.and(Links::from_tables([&other])),
                Err(e) => eprintln!("{url}: {e}"),
            }
        }
        if let Some(b) = get(&bmseeker::config::Config::default().stella_list) {
            out = out.and(Links::from_stella(&b));
        }
        for list in a.all("--links") {
            if let Some(b) = get(list) {
                out = out.and(Links::from_tsv(&String::from_utf8_lossy(&b)));
            }
        }
        out
    }

    /// A table and the other links known for its charts (none with
    /// `--own-links`); its missing or dead links filled.
    fn load_filled(url: &str, a: &Args) -> Result<(DifficultyTable, Links), String> {
        let mut t = load(url)?;
        if a.has("--own-links") {
            return Ok((t, Links::default()));
        }
        let links = links(&t, a);
        let n = links.fill(&mut t.entries);
        eprintln!(
            "{n} charts' links filled; links known for {} charts",
            links.len()
        );
        Ok((t, links))
    }

    fn load(url: &str) -> Result<DifficultyTable, String> {
        table::fetch_table(&bmseeker::net::Client::new(&Default::default()), url)
            .map_err(|e| e.to_string())
    }

    /// The MD5 and SHA-256 (hex) of every chart below `dir`.
    fn scan(dir: &Path) -> HashSet<String> {
        use md5::Digest;
        let mut out = HashSet::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(d) = todo.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                // (not the charts waiting in `DOWNLOADS`)
                if p.is_dir() && !p.ends_with(DOWNLOADS) {
                    todo.push(p);
                } else if p.is_file() && bmseeker::placement::is_chart(&p) {
                    if let Ok(b) = std::fs::read(&p) {
                        let hex =
                            |h: &[u8]| h.iter().map(|x| format!("{x:02x}")).collect::<String>();
                        out.insert(hex(&md5::Md5::digest(&b)));
                        out.insert(hex(&sha2::Sha256::digest(&b)));
                    }
                }
            }
        }
        out.remove("");
        out
    }
}
