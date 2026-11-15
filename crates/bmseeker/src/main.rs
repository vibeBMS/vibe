//! `bmseeker`: difficulty tables and the songs they list, from a terminal.
//!
//! ```text
//! bmseeker table <table URL>                      the table's levels
//! bmseeker missing <table URL> <library> [-l LEVEL] [--get]
//!                                                 its songs the library lacks
//!                                                 (--get: download them)
//! bmseeker fetch <link> <file> [-t TITLE] [-a ARTIST]
//!                                                 the archive a link leads to
//! bmseeker install <link>... <library> [--diff] [-t TITLE] [-a ARTIST] [--md5 MD5]
//!                                                 a song (or its separate
//!                                                 chart) into the library, the
//!                                                 links tried in turn
//! bmseeker import <file> <library>                a downloaded file into it
//! bmseeker place <chart> <library> [--to DIR] [--min PERCENT]
//!                                                 a chart's song folder: the
//!                                                 candidates, or move it there
//! bmseeker pending <library>                      charts waiting for their song
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
    use bmseeker::links::{self, Links};
    use bmseeker::placement::{self, Rules};
    use bmseeker::table::{self, DifficultyTable, TableEntry};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    const USAGE: &str = "usage:
  bmseeker table <table URL>
  bmseeker missing <table URL> <library> [-l LEVEL] [--get]
  bmseeker fetch <link> <file> [-t TITLE] [-a ARTIST]
  bmseeker install <link>... <library> [--diff] [-t TITLE] [-a ARTIST] [--md5 MD5]
  bmseeker import <file> <library>
  bmseeker place <chart> <library> [--to DIR] [--min PERCENT]
  bmseeker pending <library>
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
                "-l", "-t", "-a", "--to", "--min", "--also", "--links", "--md5",
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
                        for (ls, diff) in
                            [(links.song_links(e), false), (links.chart_links(e), true)]
                        {
                            if !ls.is_empty() || (!diff && md5.is_some()) {
                                let downloads = library.join(DOWNLOADS);
                                let done =
                                    install::links(&downloads, &library, &want, &ls, diff, md5);
                                report(&e.title, done);
                            }
                        }
                    }
                }
                Ok(())
            }
            Some("fetch") => {
                let (link, out) = (a.need(0)?, Path::new(a.need(1)?));
                match hosts::fetch_for(link, Some(a.want()), out) {
                    Ok(()) => {
                        println!("{} ({})", out.display(), hosts::sniff(out).unwrap_or("?"));
                        Ok(())
                    }
                    Err(Fetch::Page) => Err(format!("{link} is a page: download it in a browser")),
                    Err(Fetch::Failed(e)) => Err(e),
                }
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
                let done =
                    install::links(&downloads, &library, &want, &links, a.has("--diff"), md5);
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
            _ => Err(USAGE.into()),
        }
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
                hosts::get(url)
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
        if let Some(b) = get(links::STELLA_UPLOADS) {
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
        table::fetch_table(&table::Net, url).map_err(|e| e.to_string())
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
