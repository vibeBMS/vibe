//! bmseeker with a window: the difficulty tables, which of their charts the
//! library has, the missing ones downloaded (one, or a level's at once),
//! the charts waiting for their song with the folders they may go in, and
//! the tables written out for LR2 and beatoraja.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod config;
mod scan;
mod work;

use bmseeker::install::Done;
use bmseeker::placement::{Candidate, Located};
use bmseeker::table::{DifficultyTable, TableEntry};
use config::Config;
use eframe::egui;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use work::{Job, Msg};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("bmseeker")
            .with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "bmseeker",
        options,
        Box::new(|cc| {
            fonts(&cc.egui_ctx);
            Ok(Box::new(App::new(cc.egui_ctx.clone())))
        }),
    )
}

/// A system font with Japanese and Korean (egui's own fonts have neither),
/// after egui's.
fn fonts(ctx: &egui::Context) {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    const FAMILIES: &[&str] = &[
        "Noto Sans CJK JP",
        "Noto Sans JP",
        "Source Han Sans JP",
        "Yu Gothic UI",
        "Meiryo",
        "MS Gothic",
        "Hiragino Sans",
        "Hiragino Kaku Gothic ProN",
        "Noto Sans CJK KR",
        "Apple SD Gothic Neo",
        "Malgun Gothic",
        "Droid Sans Fallback",
        "IPAGothic",
        "IPAexGothic",
        "TakaoGothic",
        "VL Gothic",
    ];
    // a known one, else any whose name says CJK / Japanese
    let face = FAMILIES
        .iter()
        .find_map(|name| {
            db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(name)],
                ..Default::default()
            })
        })
        .or_else(|| {
            db.faces()
                .find(|f| {
                    f.families.iter().any(|(n, _)| {
                        ["CJK", "JP", "Gothic", "ゴシック"]
                            .iter()
                            .any(|k| n.contains(k))
                    })
                })
                .map(|f| f.id)
        });
    let Some(id) = face else { return };
    let Some(data) = db.with_face_data(id, |data, index| (data.to_vec(), index)) else {
        return;
    };
    let mut defs = egui::FontDefinitions::default();
    let mut font = egui::FontData::from_owned(data.0);
    font.index = data.1;
    defs.font_data
        .insert("cjk".into(), std::sync::Arc::new(font));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        defs.families.entry(family).or_default().push("cjk".into());
    }
    ctx.set_fonts(defs);
}

#[derive(PartialEq)]
enum Tab {
    Tables,
    Pending,
    Settings,
}

/// A chart waiting for its song, with where it may go.
struct Waiting {
    chart: PathBuf,
    located: Located,
    candidates: Vec<(Candidate, String)>,
}

struct App {
    config: Config,
    tables: Vec<DifficultyTable>,
    /// Every link known by MD5: the tables', then the Stella uploader's.
    links: bmseeker::links::Links,
    stella: bmseeker::links::Links,
    /// MD5s and SHA-256s of the library's charts.
    owned: HashSet<String>,
    scanning: bool,
    /// (table, level)
    selected: Option<(usize, String)>,
    filter: String,
    only_missing: bool,
    /// Titles queued or downloading.
    busy: HashSet<String>,
    waiting: Vec<Waiting>,
    log: Vec<String>,
    tab: Tab,
    new_table: String,
    jobs: Sender<Job>,
    ctx: egui::Context,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
}

impl App {
    fn new(ctx: egui::Context) -> App {
        let (tx, rx) = channel();
        let jobs = work::start(tx.clone(), ctx.clone());
        let mut app = App {
            config: Config::load(),
            tables: Vec::new(),
            links: Default::default(),
            stella: Default::default(),
            owned: HashSet::new(),
            scanning: false,
            selected: None,
            filter: String::new(),
            only_missing: false,
            busy: HashSet::new(),
            waiting: Vec::new(),
            log: Vec::new(),
            tab: Tab::Tables,
            new_table: String::new(),
            jobs,
            ctx,
            rx,
            tx,
        };
        if app.config.library.as_os_str().is_empty() {
            app.tab = Tab::Settings;
        }
        app.reload_tables();
        if app.config.fill_links {
            app.send(Job::Links);
        }
        app.rescan();
        app
    }

    fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    fn say(&mut self, s: impl Into<String>) {
        self.log.push(s.into());
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    /// Every link known again (the tables', the list's), and the tables'
    /// missing or dead ones filled from them.
    fn fill_links(&mut self) {
        if !self.config.fill_links {
            return;
        }
        self.links = bmseeker::links::Links::from_tables(&self.tables).and(self.stella.clone());
        for t in &mut self.tables {
            self.links.fill(&mut t.entries);
        }
    }

    fn reload_tables(&mut self) {
        self.tables.clear();
        for url in self.config.tables.clone() {
            work::load_table(url, self.tx.clone(), self.ctx.clone());
        }
    }

    fn rescan(&mut self) {
        if self.config.library.as_os_str().is_empty() || self.scanning {
            return;
        }
        self.scanning = true;
        self.send(Job::Scan(self.config.library.clone()));
        self.send(Job::Waiting(self.config.library.clone()));
    }

    fn owns(&self, e: &TableEntry) -> bool {
        let has = |h: &str| !h.is_empty() && self.owned.contains(&h.to_ascii_lowercase());
        has(&e.md5) || has(&e.sha256)
    }

    fn download(&mut self, e: &TableEntry) {
        if self.busy.contains(&e.title) || self.config.library.as_os_str().is_empty() {
            return;
        }
        self.busy.insert(e.title.clone());
        // its own links, then the others known for the chart
        let (song, chart) = if self.config.fill_links {
            (self.links.song_links(e), self.links.chart_links(e))
        } else {
            let own = |u: &str| u.trim().starts_with("http").then(|| u.trim().to_string());
            (
                own(&e.url).into_iter().collect(),
                own(&e.url_diff).into_iter().collect(),
            )
        };
        self.send(Job::Install {
            library: self.config.library.clone(),
            entry: Box::new(e.clone()),
            song,
            chart,
        });
    }

    fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Table(url, Ok(t)) => {
                    self.say(format!("{}: {} charts", t.header.name, t.entries.len()));
                    self.tables.retain(|o| o.source_url != url);
                    self.tables.push(t);
                    let order = &self.config.tables;
                    self.tables
                        .sort_by_key(|t| order.iter().position(|u| *u == t.source_url));
                    self.fill_links();
                }
                Msg::Table(url, Err(e)) => self.say(format!("{url}: {e}")),
                Msg::Links(links) => {
                    self.say(format!("Stella uploader: links for {} charts", links.len()));
                    self.stella = links;
                    self.fill_links();
                }
                Msg::Scanned(owned, n) => {
                    self.say(format!("library: {n} charts"));
                    self.owned = owned;
                    self.scanning = false;
                }
                Msg::Installed(title, done) => {
                    self.busy.remove(&title);
                    let mut changed = false;
                    for d in done {
                        self.say(match d {
                            Done::Added(dir, n) => {
                                changed = true;
                                format!("{title}: added {} ({n} files)", dir.display())
                            }
                            Done::Placed(p) => {
                                changed = true;
                                format!("{title}: {}", p.display())
                            }
                            Done::Waiting(p) => {
                                format!("{title}: {} waits for its song", p.display())
                            }
                            Done::Page(u) => format!("{title}: no direct download, open {u}"),
                            Done::Failed(e) => format!("{title}: failed ({e})"),
                        });
                    }
                    if changed {
                        self.rescan();
                    } else {
                        self.send(Job::Waiting(self.config.library.clone()));
                    }
                }
                Msg::Waiting(list) => {
                    self.waiting = list
                        .into_iter()
                        .map(|(chart, located, candidates)| Waiting {
                            chart,
                            located,
                            candidates,
                        })
                        .collect();
                }
                Msg::Log(s) => self.say(s),
            }
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        // (table, its name, its URL, its levels: (level, label, owned, all))
        type Levels = Vec<(String, String, usize, usize)>;
        let tree: Vec<(usize, String, String, Levels)> = self
            .tables
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let levels = t
                    .levels()
                    .into_iter()
                    .map(|level| {
                        let (mut have, mut all) = (0, 0);
                        for e in t.entries.iter().filter(|e| e.level == level) {
                            all += 1;
                            if self.owns(e) {
                                have += 1;
                            }
                        }
                        let label = t.label(&level);
                        (level, label, have, all)
                    })
                    .collect();
                (i, t.header.name.clone(), t.source_url.clone(), levels)
            })
            .collect();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, name, url, levels) in tree {
                egui::CollapsingHeader::new(name)
                    .id_salt(url)
                    .default_open(i == 0)
                    .show(ui, |ui| {
                        for (level, label, have, all) in levels {
                            let on = self.selected.as_ref() == Some(&(i, level.clone()));
                            let text = format!("{label}   {have} / {all}");
                            let button = egui::Button::selectable(on, text)
                                .wrap_mode(egui::TextWrapMode::Extend);
                            if ui.add(button).clicked() {
                                self.selected = Some((i, level));
                            }
                        }
                    });
            }
            if self.tables.is_empty() {
                ui.label("Loading tables…");
            }
        });
    }

    fn level(&mut self, ui: &mut egui::Ui) {
        let Some((ti, level)) = self.selected.clone() else {
            ui.label("Pick a level.");
            return;
        };
        let Some(t) = self.tables.get(ti) else { return };
        let entries: Vec<TableEntry> = t
            .entries
            .iter()
            .filter(|e| e.level == level)
            .cloned()
            .collect();
        let label = t.label(&level);
        let missing: Vec<TableEntry> = entries.iter().filter(|e| !self.owns(e)).cloned().collect();
        ui.horizontal(|ui| {
            ui.heading(format!("{}  {label}", t.header.name));
            ui.label(format!(
                "{} / {}",
                entries.len() - missing.len(),
                entries.len()
            ));
        });
        ui.horizontal(|ui| {
            let n = missing
                .iter()
                .filter(|e| !self.busy.contains(&e.title))
                .count();
            if ui
                .add_enabled(
                    n > 0,
                    egui::Button::new(format!("Download the missing ({n})")),
                )
                .clicked()
            {
                for e in &missing {
                    self.download(e);
                }
            }
            ui.checkbox(&mut self.only_missing, "Missing only");
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter"));
        });
        ui.separator();
        let filter = self.filter.to_lowercase();
        let rows: Vec<&TableEntry> = entries
            .iter()
            .filter(|e| !self.only_missing || !self.owns(e))
            .filter(|e| {
                filter.is_empty()
                    || e.title.to_lowercase().contains(&filter)
                    || e.artist.to_lowercase().contains(&filter)
            })
            .collect();
        let mut get = None;
        egui::ScrollArea::vertical().show_rows(ui, 22.0, rows.len(), |ui, range| {
            egui::Grid::new("entries")
                .striped(true)
                .num_columns(4)
                .show(ui, |ui| {
                    for e in &rows[range] {
                        let owned = self.owns(e);
                        // (the text font has no check marks)
                        if owned {
                            ui.colored_label(egui::Color32::from_rgb(110, 200, 120), "have");
                        } else {
                            ui.weak("—");
                        }
                        ui.label(&e.title).on_hover_text(&e.comment);
                        ui.label(&e.artist);
                        ui.horizontal(|ui| {
                            if !owned {
                                if self.busy.contains(&e.title) {
                                    ui.spinner();
                                } else if ui.small_button("Download").clicked() {
                                    get = Some((*e).clone());
                                }
                            }
                            for (name, url) in [("song", &e.url), ("chart", &e.url_diff)] {
                                if url.starts_with("http") {
                                    ui.hyperlink_to(name, url).on_hover_text(url);
                                }
                            }
                        });
                        ui.end_row();
                    }
                });
        });
        if let Some(e) = get {
            self.download(&e);
        }
    }

    fn pending(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(format!("Waiting for their song: {}", self.waiting.len()));
            if ui.button("Check again").clicked() {
                self.send(Job::Waiting(self.config.library.clone()));
            }
        });
        ui.label("Charts downloaded before their song (or with several songs it could be). Pick a folder, or download the song.");
        ui.separator();
        let mut bring = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for w in &self.waiting {
                let name = w.chart.file_name().unwrap_or_default().to_string_lossy();
                ui.strong(name.as_ref());
                ui.label(match &w.located {
                    Located::Move(_) => "its song is here now",
                    Located::Ask(_) => "several songs could be it",
                    _ => "its song is not in the library",
                });
                for (c, label) in w.candidates.iter().take(5) {
                    ui.horizontal(|ui| {
                        ui.label(format!("{:5.1}%", c.coverage * 100.0));
                        ui.label(if c.same_song { "same song" } else { "" });
                        ui.label(label);
                        if ui.small_button("Move here").clicked() {
                            bring = Some((w.chart.clone(), c.dir.clone()));
                        }
                    });
                }
                ui.separator();
            }
        });
        if let Some((chart, dir)) = bring {
            match bmseeker::placement::bring(&bmseeker::fs::Disk, &chart, &dir) {
                Some(at) => self.say(format!("{} → {}", chart.display(), at.display())),
                None => self.say(format!("{}: could not move it", chart.display())),
            }
            self.rescan();
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let folder = |ui: &mut egui::Ui, label: &str, path: &mut PathBuf| -> bool {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(label);
                let mut text = path.display().to_string();
                if ui
                    .add(egui::TextEdit::singleline(&mut text).desired_width(520.0))
                    .changed()
                {
                    *path = PathBuf::from(text);
                    changed = true;
                }
                if ui.button("…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        *path = p;
                        changed = true;
                    }
                }
            });
            changed
        };
        ui.heading("Library");
        let mut changed = folder(ui, "BMS folder", &mut self.config.library);
        ui.label("Songs are unpacked here; downloads wait in its .bmseeker folder.");
        ui.separator();
        ui.heading("Difficulty tables");
        let mut drop = None;
        for (i, url) in self.config.tables.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("Remove").clicked() {
                    drop = Some(i);
                }
                ui.label(url);
            });
        }
        if let Some(i) = drop {
            self.config.tables.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_table)
                    .hint_text("table URL")
                    .desired_width(520.0),
            );
            if ui.button("Add").clicked() && self.new_table.starts_with("http") {
                self.config.tables.push(std::mem::take(&mut self.new_table));
                changed = true;
            }
        });
        if ui
            .checkbox(
                &mut self.config.fill_links,
                "Look up a chart's other links by its MD5 (in the other tables, the Stella uploader's list) when its own is missing, dead or does not download",
            )
            .changed()
        {
            changed = true;
        }
        ui.separator();
        ui.heading("Export");
        changed |= folder(ui, "LR2 custom folders", &mut self.config.lr2_out);
        changed |= folder(ui, "beatoraja table folder", &mut self.config.bmt_out);
        ui.horizontal(|ui| {
            let lr2 = !self.config.lr2_out.as_os_str().is_empty();
            let bmt = !self.config.bmt_out.as_os_str().is_empty();
            if ui
                .add_enabled(lr2, egui::Button::new("Write LR2 folders"))
                .clicked()
            {
                self.send(Job::ExportLr2(
                    self.tables.clone(),
                    self.config.lr2_out.clone(),
                ));
            }
            if ui
                .add_enabled(bmt, egui::Button::new("Write beatoraja tables"))
                .clicked()
            {
                self.send(Job::ExportBmt(
                    self.tables.clone(),
                    self.config.bmt_out.clone(),
                ));
            }
        });
        ui.label("LR2: add the folder to LR2's song folders. beatoraja: its table folder (tablepath in config_sys.json, normally \"table\").");
        if changed {
            self.config.save();
        }
        ui.separator();
        if ui
            .button("Apply (reload the tables, rescan the library)")
            .clicked()
        {
            self.reload_tables();
            self.rescan();
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Tables, "Tables");
                let waiting = format!("Waiting ({})", self.waiting.len());
                ui.selectable_value(&mut self.tab, Tab::Pending, waiting);
                ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
                ui.separator();
                if ui.button("Reload tables").clicked() {
                    self.reload_tables();
                }
                if ui
                    .add_enabled(!self.scanning, egui::Button::new("Rescan library"))
                    .clicked()
                {
                    self.rescan();
                }
                if self.scanning {
                    ui.spinner();
                    ui.label("scanning…");
                }
                if !self.busy.is_empty() {
                    ui.spinner();
                    ui.label(format!("downloading {}", self.busy.len()));
                }
            });
        });
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(110.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for l in &self.log {
                            ui.label(l);
                        }
                    });
            });
        if self.tab == Tab::Tables {
            egui::Panel::left("levels")
                .default_size(250.0)
                .min_size(200.0)
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Tables => self.level(ui),
            Tab::Pending => self.pending(ui),
            Tab::Settings => self.settings(ui),
        });
    }
}
