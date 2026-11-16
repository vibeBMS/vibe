//! `bmseeker`: difficulty tables, downloads from where BMS authors link
//! songs, a library's charts and BMS events, from a terminal or a script.
//!
//! `bmseeker [--json] [--config FILE] <noun> <verb> ...`. With `--json`
//! every command writes JSON Lines (one record a line, as it goes); the
//! `event` commands always do, and read records (JSON Lines, or a JSON
//! array or object) from a file or `-` (stdin).

mod event;
mod io;
mod library;
mod link;
mod table;

use bmseeker::config::Config;
use bmseeker::hosts::{Context, Want};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "bmseeker", version, about, propagate_version = true)]
struct Cli {
    /// Write JSON Lines (one record a line).
    #[arg(long, global = true)]
    json: bool,
    /// Settings (JSON; else $BMSEEKER_CONFIG, else <config folder>/bmseeker/config.json).
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Links where authors put their archives.
    #[command(subcommand)]
    Link(link::Link),
    /// Where a song's links are known (its own aside).
    #[command(subcommand)]
    Song(link::Song),
    /// Difficulty tables.
    #[command(subcommand)]
    Table(table::Table),
    /// A library of songs: archives in, charts into their songs' folders.
    #[command(subcommand)]
    Library(library::Library),
    /// BMS events (manbow): their works and how their links fare.
    #[command(subcommand)]
    Event(event::Event),
}

/// A song looked for: by its chart's MD5, its title and artist.
#[derive(Args, Clone, Debug, Default)]
pub struct SongArgs {
    /// The song's title (a folder of many songs' archives: its archive).
    #[arg(short, long, default_value = "")]
    title: String,
    /// The song's artist.
    #[arg(short, long, default_value = "")]
    artist: String,
    /// One of its charts' MD5 (BMS SEARCH finds its work by it).
    #[arg(long, default_value = "")]
    md5: String,
}

impl SongArgs {
    pub fn want(&self) -> Want {
        Want {
            title: self.title.clone(),
            artist: self.artist.clone(),
        }
    }
}

/// What every command runs with.
pub struct Run {
    pub out: io::Out,
    pub cx: Context,
}

impl Run {
    pub fn config(&self) -> &Config {
        &self.cx.config
    }
}

fn main() {
    let cli = Cli::parse();
    let config = match Config::load(cli.config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bmseeker: {e}");
            std::process::exit(2);
        }
    };
    let run = Run {
        out: io::Out { json: cli.json },
        cx: Context::new(config),
    };
    let r = match cli.command {
        Command::Link(c) => link::link(&run, c),
        Command::Song(c) => link::song(&run, c),
        Command::Table(c) => table::table(&run, c),
        Command::Library(c) => library::library(&run, c),
        Command::Event(c) => event::event(&run, c),
    };
    if let Err(e) = r {
        eprintln!("bmseeker: {e}");
        std::process::exit(1);
    }
}
