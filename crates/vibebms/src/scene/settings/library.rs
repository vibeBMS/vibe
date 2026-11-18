use super::{Act, Armed, Confirm, Row, TextField};
use crate::app::Ctx;
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

pub(super) fn rows(ctx: &Ctx, armed: &Armed<Confirm>) -> Vec<Row> {
    let c = &ctx.config;
    let mut v = vec![
        Row::note(
            "Data folder (portable)",
            ctx.paths.root.display().to_string(),
        ),
        Row::adjust(
            "Scan the data folder for BMS",
            onoff(c.scan_root),
            |c, _| c.config.scan_root ^= true,
        ),
    ];
    for (i, d) in c.bms_dirs.iter().enumerate() {
        v.push(Row::new(
            format!("Folder: {d}"),
            "remove ×",
            Act::Run(remove_folder, i),
        ));
    }
    #[cfg(desktop)]
    v.push(Row::new(
        "Browse for a BMS folder…",
        "📂",
        Act::Run(browse, 0),
    ));
    v.push(Row::new(
        "Add BMS folder… (or drop a folder on the window)",
        "+",
        Act::Text(TextField::AddFolder),
    ));
    v.push(Row::new(
        "Rescan library",
        format!("{} charts", ctx.lib.songs.len()),
        Act::Run(|c, _| c.start_scan(), 0),
    ));
    v.push(Row::new(
        "Import scores (LR2, beatoraja, LR2oraja: a score database or its folder)…",
        "+",
        Act::Text(TextField::ImportScores),
    ));
    #[cfg(desktop)]
    v.push(Row::new(
        "Browse for a score database…",
        "📂",
        Act::Run(browse_scores, 0),
    ));
    #[cfg(target_arch = "wasm32")]
    super::web_storage::rows(ctx, armed, &mut v);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = armed;
    v
}
