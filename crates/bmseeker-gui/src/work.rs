//! What runs off the window's thread: table loading (each on its own
//! thread) and one worker for the rest, in order (downloads one at a time).

use bmseeker::fs::Disk;
use bmseeker::install::Done;
use bmseeker::links::Links;
use bmseeker::placement::{Candidate, Located, Rules};
use bmseeker::table::{DifficultyTable, TableEntry};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};

/// Where downloads wait in the library (charts until their song comes), as
/// the command-line tool keeps them.
pub const DOWNLOADS: &str = ".bmseeker";

pub enum Job {
    Links,
    Scan(PathBuf),
    /// The charts waiting in the library's downloads.
    Waiting(PathBuf),
    /// A song: its links, then its separate chart's (each to try in turn).
    Install {
        library: PathBuf,
        entry: Box<TableEntry>,
        song: Vec<String>,
        chart: Vec<String>,
    },
    ExportLr2(Vec<DifficultyTable>, PathBuf),
    ExportBmt(Vec<DifficultyTable>, PathBuf),
}

/// A chart waiting for its song, where it may go, and the candidates.
pub type Waiting = (PathBuf, Located, Vec<(Candidate, String)>);

pub enum Msg {
    Table(String, Result<DifficultyTable, String>),
    Links(Links),
    /// The library's chart hashes, and how many charts.
    Scanned(HashSet<String>, usize),
    Installed(String, Vec<Done>),
    Waiting(Vec<Waiting>),
    Log(String),
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    bmseeker::hosts::get(url)
}

/// A table, on a thread of its own (the window woken when it is in).
pub fn load_table(url: String, tx: Sender<Msg>, ctx: eframe::egui::Context) {
    std::thread::spawn(move || {
        let r =
            bmseeker::table::fetch_table(&bmseeker::table::Net, &url).map_err(|e| e.to_string());
        let _ = tx.send(Msg::Table(url, r));
        ctx.request_repaint();
    });
}

/// The worker; the window is woken for each message.
pub fn start(tx: Sender<Msg>, ctx: eframe::egui::Context) -> Sender<Job> {
    let (jobs, rx) = channel::<Job>();
    std::thread::spawn(move || {
        let send = |m: Msg| {
            let _ = tx.send(m);
            ctx.request_repaint();
        };
        for job in rx {
            match job {
                Job::Links => {
                    let links = get(bmseeker::links::STELLA_UPLOADS)
                        .map(|b| Links::from_stella(&b))
                        .unwrap_or_else(|e| {
                            send(Msg::Log(format!(
                                "{}: {e}",
                                bmseeker::links::STELLA_UPLOADS
                            )));
                            Links::default()
                        });
                    send(Msg::Links(links));
                }
                Job::Scan(library) => {
                    let (owned, n) = crate::scan::scan(&library);
                    send(Msg::Scanned(owned, n));
                }
                Job::Waiting(library) => send(Msg::Waiting(waiting(&library))),
                Job::Install {
                    library,
                    entry,
                    song,
                    chart,
                } => {
                    let want = bmseeker::hosts::Want {
                        title: entry.title.clone(),
                        artist: entry.artist.clone(),
                    };
                    let downloads = library.join(DOWNLOADS);
                    let mut done = Vec::new();
                    for (links, diff) in [(song, false), (chart, true)] {
                        if !links.is_empty() {
                            done.extend(bmseeker::install::links(
                                &downloads, &library, &want, &links, diff,
                            ));
                        }
                    }
                    if done.is_empty() {
                        done.push(Done::Failed("no link".into()));
                    }
                    send(Msg::Installed(entry.title.clone(), done));
                }
                Job::ExportLr2(tables, out) => {
                    for t in &tables {
                        send(Msg::Log(export_lr2(t, &out)));
                    }
                }
                Job::ExportBmt(tables, out) => {
                    for t in &tables {
                        let file = out.join(bmseeker::export::bmt_file_name(&t.source_url));
                        let r = std::fs::create_dir_all(&out).and_then(|_| {
                            std::fs::write(&file, bmseeker::export::beatoraja_bmt(t))
                        });
                        send(Msg::Log(match r {
                            Ok(()) => format!("{}: {}", t.header.name, file.display()),
                            Err(e) => format!("{}: {e}", file.display()),
                        }));
                    }
                }
            }
        }
    });
    jobs
}

fn export_lr2(t: &DifficultyTable, out: &Path) -> String {
    let dir = out.join(bmseeker::install::safe_name(&t.header.name));
    let _ = std::fs::create_dir_all(&dir);
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if e.path().extension().is_some_and(|x| x == "lr2folder") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let folders = bmseeker::export::lr2_folders(t);
    for (name, text) in &folders {
        if let Err(e) = std::fs::write(dir.join(name), text) {
            return format!("{}: {e}", dir.display());
        }
    }
    format!(
        "{}: {} LR2 folders in {}",
        t.header.name,
        folders.len(),
        dir.display()
    )
}

/// The charts waiting in the library's downloads, with where they may go.
fn waiting(library: &Path) -> Vec<Waiting> {
    std::fs::read_dir(library.join(DOWNLOADS))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| bmseeker::placement::is_chart(p))
        .filter_map(|p| {
            let (located, candidates) =
                bmseeker::placement::locate_with(&Disk, &p, library, false, Rules::default())?;
            Some((p, located, candidates))
        })
        .collect()
}
