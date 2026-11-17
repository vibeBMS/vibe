use super::{Act, Armed, Confirm, Row, TextField};
use crate::app::Ctx;
use crate::scene::option_panel::onoff;

fn remove_folder(c: &mut Ctx, i: usize) {
    if i < c.config.bms_dirs.len() {
        c.config.bms_dirs.remove(i);
        c.start_scan();
    }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn browse(c: &mut Ctx, _: usize) {
    if let Some(p) = rfd::FileDialog::new().set_title("BMS folder").pick_folder() {
        let s = p.to_string_lossy().into_owned();
        if !c.config.bms_dirs.contains(&s) {
            c.config.bms_dirs.push(s);
            c.start_scan();
        }
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
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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
        format!("{} charts", ctx.songs.len()),
        Act::Run(|c, _| c.start_scan(), 0),
    ));
    #[cfg(target_arch = "wasm32")]
    super::web_storage::rows(ctx, armed, &mut v);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = armed;
    v
}
