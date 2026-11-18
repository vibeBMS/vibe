//! LIBRARY: the song folders, the difficulty tables (a sub-page each),
//! where downloads come from, and on the web the songs kept in the
//! browser.

use super::row::{Act, Row, Rows};
use super::{Sub, TextField};
use crate::app::Ctx;
use crate::config::TableConfig;
use crate::scene::option_panel::onoff;

fn remove_folder(c: &mut Ctx, i: usize) {
    if i < c.config.bms_dirs.len() {
        c.config.bms_dirs.remove(i);
        c.start_scan();
    }
}

#[cfg(desktop)]
fn browse(c: &mut Ctx, _: usize) {
    if let Some(p) = rfd::FileDialog::new().set_title("BMS folder").pick_folder() {
        let s = p.to_string_lossy().into_owned();
        if !c.config.bms_dirs.contains(&s) {
            c.config.bms_dirs.push(s);
            c.start_scan();
        }
    }
}

/// Scores from LR2, beatoraja or LR2oraja (`scoredb`): `path` is the
/// player's database, or a folder that has one.
pub(super) fn import_scores(c: &mut Ctx, path: &str) {
    use crate::scoredb;
    if path.is_empty() {
        return;
    }
    let read = scoredb::find(std::path::Path::new(path)).and_then(|f| {
        let bytes = crate::platform::vfs::read(&f).map_err(|e| e.to_string())?;
        scoredb::read(&bytes, scoredb::is_lr2oraja(&f))
    });
    let rows = match read {
        Ok(r) => r,
        Err(e) => return c.toast(format!("Score import: {e}")),
    };
    let (records, unknown) = scoredb::records(&rows, &c.lib.songs);
    let Some(db) = c.lib.db.as_mut() else {
        return;
    };
    let (imported, updated) = db.import(&records);
    c.lib.scores = db.scores();
    c.lib.version += 1;
    c.toast(format!(
        "{} scores: {imported} imported, {updated} updated, {unknown} unknown charts, {} rows skipped",
        rows.source.label(),
        rows.skipped
    ));
}

#[cfg(desktop)]
fn browse_scores(c: &mut Ctx, _: usize) {
    if let Some(p) = rfd::FileDialog::new()
        .set_title(
            "LR2's LR2files/Database/Score/<player>.db, or beatoraja's player/<name>/score.db",
        )
        .add_filter("score database", &["db"])
        .pick_file()
    {
        import_scores(c, &p.to_string_lossy());
    }
}

fn toggle_table(c: &mut Ctx, i: usize) {
    if let Some(t) = c.config.tables.get_mut(i) {
        t.enabled ^= true;
    }
    c.load_cached_tables();
    c.tables.refresh(&c.config.tables, false);
}

fn remove_table(c: &mut Ctx, i: usize) {
    if i < c.config.tables.len() {
        c.config.tables.remove(i);
        c.load_cached_tables();
    }
}

fn restore_defaults(c: &mut Ctx, _: usize) {
    for (n, u) in bmseeker::table::DEFAULT_TABLES {
        if !c.config.tables.iter().any(|t| t.url == *u) {
            c.config.tables.push(TableConfig {
                name: n.to_string(),
                url: u.to_string(),
                enabled: true,
            });
        }
    }
    c.load_cached_tables();
    c.tables.refresh(&c.config.tables, false);
}

/// The data folder: where vibeBMS keeps songs, scores and settings.
pub(super) fn data_folder(ctx: &Ctx) -> Row {
    Row::note(
        "info.data_folder",
        "Data folder",
        ctx.paths.root.display().to_string(),
    )
    .desc("Where vibeBMS keeps songs, scores and settings (portable).")
    .keys("root portable path")
}

/// Table `t`'s name and how many entries it has read.
fn table_name(ctx: &Ctx, t: &TableConfig) -> (String, Option<usize>) {
    let loaded = (ctx.tables.list.iter())
        .find(|d| d.source_url == t.url)
        .map(|d| d.entries.len());
    let name = if t.name.is_empty() { &t.url } else { &t.name };
    (name.clone(), loaded)
}

pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    let c = &ctx.config;
    b.header("SONG FOLDERS");
    b.push(data_folder(ctx));
    b.push(
        Row::adjust(
            "scan_root",
            "Scan the data folder",
            onoff(c.scan_root),
            |c, _| c.config.scan_root ^= true,
        )
        .desc("Also look for BMS in the data folder itself."),
    );
    for (i, d) in c.bms_dirs.iter().enumerate() {
        b.push(
            Row::new(
                "library.folder",
                d.clone(),
                "remove ×",
                Act::Run(remove_folder, i),
            )
            .desc("A folder scanned for BMS. Enter: remove."),
        );
    }
    #[cfg(desktop)]
    b.push(
        Row::new(
            "library.browse",
            "Browse for a folder…",
            "browse",
            Act::Run(browse, 0),
        )
        .desc("Pick a BMS folder to add."),
    );
    b.push(
        Row::new(
            "library.add_folder",
            "Add a folder…",
            "+",
            Act::Text(TextField::AddFolder),
        )
        .desc("Type or paste its path, or drop a folder on the window.")
        .keys("bms folder path"),
    );
    b.push(
        Row::new(
            "library.rescan",
            "Rescan",
            format!("{} charts", ctx.lib.songs.len()),
            Act::Run(|c, _| c.start_scan(), 0),
        )
        .desc("Read the folders again for new or changed charts.")
        .keys("scan library refresh"),
    );
    b.header("SCORES");
    b.push(
        Row::new(
            "library.import_scores",
            "Import scores…",
            "+",
            Act::Text(TextField::ImportScores),
        )
        .desc("From LR2, beatoraja or LR2oraja: type or paste a score database or its folder.")
        .keys("lr2 beatoraja lr2oraja score.db lamps"),
    );
    #[cfg(desktop)]
    b.push(
        Row::new(
            "library.browse_scores",
            "Browse for a score database…",
            "browse",
            Act::Run(browse_scores, 0),
        )
        .desc("Pick LR2's <player>.db or beatoraja's score.db; its scores are imported."),
    );
    let on = c.tables.iter().filter(|t| t.enabled).count();
    b.header(format!("DIFFICULTY TABLES · {on} OF {}", c.tables.len()));
    for (i, t) in c.tables.iter().enumerate() {
        let (name, loaded) = table_name(ctx, t);
        let read = loaded.map_or(String::new(), |n| format!("{n} charts · "));
        b.push(
            Row::new(
                "tables.table",
                name,
                onoff(t.enabled),
                Act::Toggle(toggle_table, i),
            )
            .desc(format!(
                "{read}Its folder in the song select. Enter: URL, update, remove."
            ))
            .opens(Sub::Table(t.url.clone()))
            .keys("difficulty table"),
        );
    }
    b.extend([
        Row::new(
            "tables.add",
            "Add a table…",
            "+",
            Act::Text(TextField::AddTable),
        )
        .desc("Paste a difficulty table's URL.")
        .keys("table url"),
        Row::new(
            "tables.update_all",
            "Update all tables",
            "↻",
            Act::Run(|c, _| c.tables.refresh(&c.config.tables, true), 0),
        )
        .desc("Read every table again from its site."),
    ]);
    b.group(
        "library.downloads",
        "DOWNLOADS",
        "Third-party sources, song folder names, the default tables",
    );
    b.extend([
        Row::adjust(
            "third_party_mirrors",
            "Third-party sources",
            onoff(c.third_party_mirrors),
            |c, _| {
                c.config.third_party_mirrors ^= true;
                c.net.hosts = crate::hosts::context(&c.config);
            },
        )
        .desc("Use mirrors and MD5 lookups (Ginger, Konmai) to find downloads.")
        .keys("mirror ginger konmai md5"),
        Row::adjust(
            "name_song_folders",
            "Name song folders after their charts",
            onoff(c.name_song_folders),
            |c, _| {
                c.config.name_song_folders ^= true;
                c.net.hosts = crate::hosts::context(&c.config);
            },
        )
        .desc("New downloads go into \"[artist] title\" folders."),
        Row::new(
            "tables.restore_defaults",
            "Restore default tables",
            "restore",
            Act::Run(restore_defaults, 0),
        )
        .desc("Add back the tables vibeBMS comes with."),
    ]);
    #[cfg(target_arch = "wasm32")]
    {
        b.header("STORAGE");
        b.push(
            Row::sub(
                "storage",
                "Songs kept in this browser",
                super::web_storage::summary(),
                Sub::Storage,
            )
            .desc("How much room they take; delete those you no longer play.")
            .keys("opfs space quota browser"),
        );
    }
}

/// LIBRARY › a difficulty table: its address, on / off, update, remove.
pub(super) fn table(b: &mut Rows, url: &str) {
    let ctx = b.ctx;
    let Some((i, t)) = (ctx.config.tables.iter())
        .enumerate()
        .find(|(_, t)| t.url == url)
    else {
        return;
    };
    let (_, loaded) = table_name(ctx, t);
    b.header("TABLE");
    b.extend([
        Row::note("tables.url", "URL", "").desc(t.url.clone()),
        Row::new(
            "tables.table",
            "Shown",
            onoff(t.enabled),
            Act::Toggle(toggle_table, i),
        )
        .desc("Its folder in the song select."),
        Row::new(
            "tables.update",
            "Update",
            loaded.map_or("not read yet".into(), |n| format!("{n} charts ↻")),
            Act::Run(|c, _| c.tables.refresh(&c.config.tables, true), 0),
        )
        .desc("Read the tables again from their sites."),
        Row::new(
            "tables.remove",
            "Remove",
            "remove ×",
            Act::Run(remove_table, i),
        )
        .desc("Forget this table."),
    ]);
}

/// LIBRARY's chips on the top level.
pub(super) fn chips(ctx: &Ctx) -> Vec<String> {
    let on = ctx.config.tables.iter().filter(|t| t.enabled).count();
    let n = ctx.lib.songs.len();
    vec![
        format!("{n} chart{}", if n == 1 { "" } else { "s" }),
        format!("{on} tables"),
    ]
}
