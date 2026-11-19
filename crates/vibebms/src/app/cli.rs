//! The command line (desktop):
//!
//! `vibebms [<chart>] [options]` starts at song select, or plays a chart
//! directly. Options for this launch only (never saved to the config):
//! `--gauge`, `--random`, `--hs`, `--judge`, `--ln`, `--autoplay`, `--set
//! <section.key>=<value>`; `--print-effective-options` prints the play
//! options they make and exits. `--ghost=best|last` and `--vs` choose a
//! battle for the chart. Subcommands: `songs scan`, `songs list [--json]`,
//! `export video` (`export`).

use super::{Ctx, Scene};
use crate::config::Config;
use crate::scene::play::{ghost_for, vs_refusal, PlayRequest, PlayScene, Versus};
use serde::Serialize;
use std::path::PathBuf;
use vibebms_core::Labeled;

pub const USAGE: &str = "\
usage: vibebms [<chart>] [options]
       vibebms songs scan
       vibebms songs list [--json]
       vibebms export video <chart> [--replay <slot>] [--out <file>] [--size WxH] [--fps <fps>]

  <chart>                     play this chart at start (song select after it)
  -a, --autoplay              autoplay it (no chart: AUTOPLAY on in song select)
  --ghost=best|last           G-BATTLE against that replay of the chart
  --vs                        two players here (VS)

options for this launch (not saved to the config):
  --gauge=<gauge>             ASSIST EASY, EASY, NORMAL, HARD, EX-HARD, HAZARD, ...
  --random=<option>           OFF, MIRROR, RANDOM, R-RANDOM, S-RANDOM, H-RANDOM, ...
  --hs=<hi-speed>             the hi-speed multiplier, e.g. 3.25
  --judge=<preset>            beatoraja, LR2, IIDX
  --ln=<mode>                 LN, CN, HCN, FORCE LN, FORCE CN, FORCE HCN
  --set <section.key>=<value> any setting of config.toml, e.g. --set play.sudden=0.3
  --print-effective-options   print the play options these make, and exit

viewer for BMS editors (uBMplay's command line):
  -P [-N<measure>] <chart>    autoplay the chart from the measure (0: the start); a viewer
                              already running takes the request (reloads, seeks) instead
  -S                          stop the running viewer's play

  songs scan                  scan the BMS folders (as F12 does) and exit
  songs list [--json]         list the library as last scanned and exit
  export video <chart> ...    render its autoplay or a replay to a video file
                              (`vibebms export video --help`)
  -h, --help                  this text";

/// What the command line asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cli {
    pub chart: Option<PathBuf>,
    pub autoplay: bool,
    pub ghost: Option<String>,
    pub vs: bool,
    /// `--gauge` and the like: (option, value), in their order.
    pub overrides: Vec<(String, String)>,
    /// `--set`: (section.key, value).
    pub sets: Vec<(String, String)>,
    pub print_options: bool,
    pub command: Option<Command>,
    /// `-P`: the viewer's play (`-N`: from that measure).
    pub viewer: bool,
    pub measure: Option<usize>,
    /// `-S`: stop the viewer's play.
    pub stop: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Help,
    SongsScan,
    SongsList { json: bool },
}

/// The overriding options.
const OVERRIDES: [&str; 5] = ["gauge", "random", "hs", "judge", "ln"];

impl Cli {
    /// The arguments after the program's name.
    pub fn parse(args: &[String]) -> Result<Cli, String> {
        let mut cli = Cli::default();
        let mut it = args.iter().peekable();
        if it.peek().is_some_and(|a| *a == "songs") {
            it.next();
            cli.command = Some(match it.next().map(String::as_str) {
                Some("scan") => Command::SongsScan,
                Some("list") => Command::SongsList {
                    json: it.any(|a| a == "--json"),
                },
                Some(o) => return Err(format!("songs: unknown command {o:?} (scan, list)")),
                None => return Err("songs: scan or list".into()),
            });
            return Ok(cli);
        }
        while let Some(a) = it.next() {
            // `--name=value`, or `--name value`
            let (name, inline) = match a.split_once('=') {
                Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
                _ => (a.as_str(), None),
            };
            let mut value = |what: &str| -> Result<String, String> {
                inline
                    .clone()
                    .or_else(|| it.next().cloned())
                    .ok_or_else(|| format!("{what} needs a value"))
            };
            // uBMplay's -N<measure>, -N <measure>
            if let Some(m) = name.strip_prefix("-N") {
                let m = match m {
                    "" => value("-N")?,
                    m => m.to_string(),
                };
                cli.measure = Some(m.trim().parse().map_err(|_| format!("-N{m}: a measure"))?);
                cli.viewer = true;
                continue;
            }
            match name {
                "-P" | "--viewer" => cli.viewer = true,
                "-S" | "--viewer-stop" => cli.stop = true,
                "-h" | "--help" => cli.command = Some(Command::Help),
                "-a" | "--autoplay" => cli.autoplay = true,
                "--vs" => cli.vs = true,
                "--ghost" => cli.ghost = Some(value(name)?),
                "--print-effective-options" => cli.print_options = true,
                "--set" => {
                    let v = value(name)?;
                    let (k, v) = v
                        .split_once('=')
                        .ok_or_else(|| format!("--set {v}: <section.key>=<value>"))?;
                    cli.sets.push((k.trim().to_string(), v.trim().to_string()));
                }
                n if n.strip_prefix("--").is_some_and(|o| OVERRIDES.contains(&o)) => {
                    let v = value(n)?;
                    cli.overrides.push((n[2..].to_string(), v));
                }
                n if n.starts_with('-') && n.len() > 1 => {
                    return Err(format!("unknown option {n}"));
                }
                _ if cli.chart.is_none() => cli.chart = Some(PathBuf::from(a)),
                _ => return Err(format!("one chart only ({a:?})")),
            }
        }
        Ok(cli)
    }
}

/// What `run` keeps of the command line for the window (desktop): what it
/// asks for, and the settings read with this launch's options applied.
#[derive(Default)]
pub struct Launch {
    pub cli: Cli,
    /// `None` on the web and mobile (read as the window is made).
    pub config: Option<Config>,
    pub overrides: Overrides,
}

/// A `choices!` value by its label or its name in the config, any case,
/// spaces, `-` and `_` left out (`exhard`, `EX-HARD`, `ExHard`).
fn pick<T: Labeled + Serialize + Copy>(all: &[T], s: &str) -> Option<T> {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let want = norm(s);
    all.iter().copied().find(|v| {
        let name = toml::Value::try_from(v)
            .ok()
            .and_then(|n| n.as_str().map(str::to_string));
        norm(v.label()) == want || name.is_some_and(|n| norm(&n) == want)
    })
}

/// The overriding options applied to `config`.
fn apply_options(cli: &Cli, config: &mut Config) -> Result<(), String> {
    use vibebms_core::gauge::GaugeType;
    use vibebms_core::judge::JudgePreset;
    use vibebms_core::lane_option::LaneOption;
    use vibebms_core::LnPref;
    let p = &mut config.play;
    // --autoplay without a chart: AUTOPLAY on in song select
    if cli.autoplay && cli.chart.is_none() {
        p.autoplay = true;
    }
    for (o, v) in &cli.overrides {
        let bad = || format!("--{o}: {v:?} is not one of its values (see --help)");
        match o.as_str() {
            "gauge" => p.gauge = pick(&GaugeType::ALL, v).ok_or_else(bad)?,
            "random" => p.random_1p = pick(&LaneOption::ALL, v).ok_or_else(bad)?,
            "judge" => p.judge_preset = pick(&JudgePreset::ALL, v).ok_or_else(bad)?,
            "hs" => {
                let hs: f32 = v.parse().map_err(|_| bad())?;
                if !(0.01..=20.0).contains(&hs) {
                    return Err(format!("--hs: {hs} is not within 0.01 .. 20"));
                }
                p.hispeed = hs;
            }
            "ln" => {
                let norm = |s: &str| s.replace([' ', '-', '_'], "").to_lowercase();
                let ln = (LnPref::ALL.into_iter())
                    .find(|l| norm(&l.label()) == norm(v))
                    .ok_or_else(bad)?;
                p.set_ln_pref(ln);
            }
            _ => {}
        }
    }
    if !cli.sets.is_empty() {
        let mut root = toml::Value::try_from(&*config).map_err(|e| e.to_string())?;
        for (key, v) in &cli.sets {
            // a TOML value, else a string as it is
            let value = toml::from_str::<toml::Table>(&format!("v = {v}"))
                .ok()
                .and_then(|mut t| t.remove("v"))
                .unwrap_or_else(|| toml::Value::String(v.clone()));
            // (a setting at its default is not written: added)
            let (t, k) = parent_mut(&mut root, key)
                .ok_or_else(|| format!("--set {key}: no such setting in config.toml"))?;
            t.insert(k.to_string(), value);
        }
        let set: Config = root
            .try_into()
            .map_err(|e: toml::de::Error| format!("--set: {}", e.message()))?;
        // a key the config does not know is dropped on the way (one set
        // to its default is not written either, but changes the config)
        let mut back = toml::Value::try_from(&set).map_err(|e| e.to_string())?;
        for (key, _) in &cli.sets {
            if path_mut(&mut back, key).is_none() && *config == set {
                return Err(format!("--set {key}: no such setting in config.toml"));
            }
        }
        *config = set;
    }
    Ok(())
}

/// The value at `a.b.c` in `v`, if there is one.
fn path_mut<'a>(v: &'a mut toml::Value, key: &str) -> Option<&'a mut toml::Value> {
    key.split('.')
        .try_fold(v, |v, k| v.as_table_mut().and_then(|t| t.get_mut(k)))
}

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
fn parent_mut<'a, 'k>(
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

impl Overrides {
    /// `cli`'s options applied to `config`.
    pub fn apply(cli: &Cli, config: &mut Config) -> Result<Overrides, String> {
        let unchanged = config.clone();
        apply_options(cli, config)?;
        // (the usual launch: nothing to compare)
        if *config == unchanged {
            return Ok(Overrides::default());
        }
        let before = toml::Value::try_from(&unchanged).map_err(|e| e.to_string())?;
        let after = toml::Value::try_from(&*config).map_err(|e| e.to_string())?;
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

/// Windows release builds have no console of their own: print to the
/// one they were started from.
fn attach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn AttachConsole(pid: u32) -> i32;
        }
        // ATTACH_PARENT_PROCESS
        unsafe {
            AttachConsole(u32::MAX);
        }
    }
}

/// Before the window: a command line that does not read, `--help`, the
/// subcommands and `--print-effective-options` are done here; `Err(code)`
/// is the exit code then.
pub fn run_before_window(paths: &crate::paths::Paths) -> Result<Launch, i32> {
    if crate::platform::IS_WEB || cfg!(mobile) {
        return Ok(Launch::default());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(desktop)]
    if args.first().is_some_and(|a| a == "export") {
        attach_console();
        return Err(super::export::run(paths, &args[1..]));
    }
    let cli = match Cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            attach_console();
            eprintln!("vibebms: {e}\n\n{USAGE}");
            return Err(2);
        }
    };
    // the viewer: a request for the one running, else this one is it
    if cli.stop {
        super::viewer::forward_to(super::viewer::PORT, &super::viewer::Request::Stop);
        return Err(0);
    }
    if cli.viewer {
        let Some(chart) = &cli.chart else {
            attach_console();
            eprintln!("vibebms: -P needs a chart\n\n{USAGE}");
            return Err(2);
        };
        let path = std::path::absolute(chart).unwrap_or_else(|_| chart.clone());
        let req = super::viewer::Request::Play {
            path,
            measure: cli.measure.unwrap_or(0),
        };
        if super::viewer::forward_to(super::viewer::PORT, &req) {
            return Err(0);
        }
    }
    match &cli.command {
        Some(Command::Help) => {
            attach_console();
            println!("{USAGE}");
            return Err(0);
        }
        Some(Command::SongsScan) => {
            attach_console();
            return Err(songs_scan(paths));
        }
        Some(Command::SongsList { json }) => {
            attach_console();
            return Err(songs_list(paths, *json));
        }
        None => {}
    }
    let mut config = crate::profiles::load_config(paths);
    let overrides = match Overrides::apply(&cli, &mut config) {
        Ok(o) => o,
        Err(e) => {
            attach_console();
            eprintln!("vibebms: {e}");
            return Err(2);
        }
    };
    if cli.print_options {
        attach_console();
        for l in overrides.describe() {
            println!("# overridden: {l}");
        }
        let v = toml::Value::try_from(&config).ok();
        let play = v.as_ref().and_then(|v| v.get("play")).cloned();
        let mut t = toml::Table::new();
        t.insert(
            "play".into(),
            play.unwrap_or(toml::Value::Table(Default::default())),
        );
        println!("{}", toml::to_string_pretty(&t).unwrap_or_default());
        return Err(0);
    }
    Ok(Launch {
        cli,
        config: Some(config),
        overrides,
    })
}

/// `songs scan`: the BMS folders scanned into the song cache.
fn songs_scan(paths: &crate::paths::Paths) -> i32 {
    use std::sync::atomic::Ordering;
    let config = crate::profiles::load_config(paths);
    let roots = super::ctx::scan_roots(&config, paths);
    let skip = super::ctx::internal_dirs(paths);
    for r in &roots {
        println!("scanning {}", r.display());
    }
    let progress = std::sync::Arc::new(crate::library::ScanProgress::default());
    let t = std::time::Instant::now();
    crate::library::scan(paths.songs_cache(), roots, skip, progress.clone());
    let unread = progress.unread.lock().map(|u| u.len()).unwrap_or(0);
    println!(
        "{} charts found, {} read ({} changed{}), {:.1} s",
        progress.found.load(Ordering::Relaxed),
        progress.parsed.load(Ordering::Relaxed),
        if progress.changed.load(Ordering::Relaxed) {
            "library"
        } else {
            "nothing"
        },
        if unread > 0 {
            format!(", {unread} could not be read")
        } else {
            String::new()
        },
        t.elapsed().as_secs_f64()
    );
    0
}

/// `songs list`: the song cache, a line per chart or as JSON.
fn songs_list(paths: &crate::paths::Paths, json: bool) -> i32 {
    let mut songs = crate::library::cached_songs(&paths.songs_cache());
    songs.sort_by(|a, b| a.path.cmp(&b.path));
    if json {
        match serde_json::to_string_pretty(&songs) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("vibebms: {e}");
                return 1;
            }
        }
        return 0;
    }
    for s in &songs {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            s.md5,
            s.mode.label(),
            s.level,
            s.full_title(),
            s.path
        );
    }
    eprintln!("{} charts", songs.len());
    0
}

/// A chart through the file system (`pack.zip!/Song/a.bme` is a chart in
/// an archive).
pub(super) fn read_chart(path: &std::path::Path) -> Result<vibebms_core::Chart, String> {
    let b = crate::platform::vfs::read(path).map_err(|e| e.to_string())?;
    vibebms_core::load_chart_bytes(path, &b, &Default::default()).map_err(|e| e.to_string())
}

/// The viewer's play: `path` autoplayed from `measure` (as practice plays
/// it: from the measure's start with what was sounding there; nothing
/// recorded), its chart read again.
pub(super) fn viewer_scene(
    ctx: &mut Ctx,
    path: &std::path::Path,
    measure: usize,
) -> Option<Box<dyn Scene>> {
    let c = match read_chart(path) {
        Ok(c) => c,
        Err(e) => {
            ctx.toast(format!("{}: {e}", path.display()));
            return None;
        }
    };
    let song = crate::library::song_from_chart(path, String::new(), &c);
    let p = &ctx.config.play;
    let run = crate::scene::play::PracticeRun {
        practice: vibebms_core::practice::Practice {
            from: measure,
            to: usize::MAX,
            lead_in: 0,
            speed: 100,
            rank: None,
            total: None,
        },
        gauge: p.gauge,
        gauge_start: None,
        repeat: false,
        seed: crate::platform::unix_ms(),
        result: Default::default(),
        viewer: true,
    };
    let req = PlayRequest {
        song,
        autoplay: true,
        course: None,
        replay: None,
        vs: None,
        practice: Some(run),
    };
    Some(Box::new(PlayScene::new(ctx, req)))
}

/// The play `cli` asks for, if it names a chart.
pub(super) fn play_scene(ctx: &mut Ctx, cli: &Cli) -> Option<Box<dyn Scene>> {
    let path = cli.chart.clone()?;
    if cli.viewer {
        let path = std::path::absolute(&path).unwrap_or(path);
        return viewer_scene(ctx, &path, cli.measure.unwrap_or(0));
    }
    let c = match read_chart(&path) {
        Ok(c) => c,
        Err(e) => {
            ctx.toast(format!("{}: {e}", path.display()));
            return None;
        }
    };
    let song = crate::library::song_from_chart(&path, String::new(), &c);
    // --ghost best|last: G-BATTLE against that replay
    let ghost = cli.ghost.as_deref().map(|g| {
        let slot = if g == "last" {
            crate::replay::Slot::Last
        } else {
            crate::replay::Slot::Best
        };
        ghost_for(ctx, &song, slot).map(|r| (r, slot))
    });
    let vs = match ghost {
        Some(Ok((r, slot))) => Some(Versus::Ghost(r, slot)),
        Some(Err(e)) => {
            ctx.toast(e);
            None
        }
        // --vs: two players here
        None if cli.vs => match vs_refusal(ctx, &song) {
            Some(why) => {
                ctx.toast(why.to_string());
                None
            }
            None => Some(Versus::Local),
        },
        None => None,
    };
    let req = PlayRequest {
        song,
        autoplay: cli.autoplay,
        course: None,
        replay: None,
        vs,
        practice: None,
    };
    Some(Box::new(PlayScene::new(ctx, req)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibebms_core::gauge::GaugeType;
    use vibebms_core::judge::JudgePreset;
    use vibebms_core::lane_option::LaneOption;

    fn parse(s: &str) -> Result<Cli, String> {
        let v: Vec<String> = s.split_whitespace().map(String::from).collect();
        Cli::parse(&v)
    }

    #[test]
    fn parses_the_command_line() {
        let c = parse("song.bme --autoplay --gauge=HARD --random mirror --hs 3.5").unwrap();
        assert_eq!(c.chart, Some(PathBuf::from("song.bme")));
        assert!(c.autoplay);
        assert_eq!(
            c.overrides,
            [
                ("gauge".to_string(), "HARD".to_string()),
                ("random".into(), "mirror".into()),
                ("hs".into(), "3.5".into())
            ]
        );
        assert_eq!(
            parse("--set play.sudden=0.3").unwrap().sets,
            [("play.sudden".to_string(), "0.3".to_string())]
        );
        assert_eq!(
            parse("songs list --json").unwrap().command,
            Some(Command::SongsList { json: true })
        );
        assert_eq!(
            parse("songs scan").unwrap().command,
            Some(Command::SongsScan)
        );
        assert!(parse("--gauge").is_err());
        assert!(parse("--frobnicate").is_err());
        assert!(parse("a.bms b.bms").is_err());
        assert!(parse("songs dance").is_err());
        // uBMplay's, as the editors call it
        let c = parse("-P -N12 ___bmse_temp.bms").unwrap();
        assert!(c.viewer && !c.stop);
        assert_eq!(
            (c.measure, c.chart),
            (Some(12), Some("___bmse_temp.bms".into()))
        );
        assert_eq!(parse("-N 3 a.bms").unwrap().measure, Some(3));
        assert!(parse("-S").unwrap().stop);
        assert!(parse("-Nx a.bms").is_err());
    }

    #[test]
    fn overrides_apply_and_are_not_saved() {
        let cli = parse(
            "--gauge ex-hard --random S-RANDOM --hs 2.5 --judge lr2 --ln force_cn \
             --set play.sudden=0.25 --set display.show_fps=true",
        )
        .unwrap();
        let mut c = Config::default();
        let saved = c.clone();
        let ov = Overrides::apply(&cli, &mut c).unwrap();
        assert_eq!(c.play.gauge, GaugeType::ExHard);
        assert_eq!(c.play.random_1p, LaneOption::SRandom);
        assert_eq!(c.play.hispeed, 2.5);
        assert_eq!(c.play.judge_preset, JudgePreset::Lr2);
        assert!(c.play.ln_pref().force);
        assert_eq!(c.play.sudden, 0.25);
        assert!(c.display.show_fps);
        assert!(!ov.is_empty());
        // saved as before; a setting changed in the game since is kept
        assert_eq!(ov.for_saving(&c).unwrap(), saved);
        c.play.hispeed = 4.0;
        let s = ov.for_saving(&c).unwrap();
        assert_eq!(s.play.hispeed, 4.0);
        assert_eq!(s.play.gauge, saved.play.gauge);
        // values that are none of the option's
        let mut c = Config::default();
        assert!(Overrides::apply(&parse("--gauge hardest").unwrap(), &mut c).is_err());
        assert!(Overrides::apply(&parse("--hs fast").unwrap(), &mut c).is_err());
        assert!(Overrides::apply(&parse("--set play.nothing=1").unwrap(), &mut c).is_err());
        assert!(Overrides::apply(&parse("--set play.hispeed=fast").unwrap(), &mut c).is_err());
        // a setting at its default (not written) can be set
        Overrides::apply(&parse("--set play.freq=2").unwrap(), &mut c).unwrap();
        assert_eq!(c.play.freq, 2);
        // nothing asked, or the value it has: nothing overridden
        let before = c.clone();
        for args in ["", "song.bme", "--set play.freq=2"] {
            let ov = Overrides::apply(&parse(args).unwrap(), &mut c).unwrap();
            assert!(ov.is_empty() && ov.for_saving(&c).is_none(), "{args}");
            assert_eq!(c, before);
        }
    }
}
