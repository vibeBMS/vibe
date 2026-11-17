use super::{Act, Row, TextField};
use crate::app::Ctx;
use crate::config::TableConfig;
use crate::scene::option_panel::onoff;

fn toggle(c: &mut Ctx, i: usize) {
    if let Some(t) = c.config.tables.get_mut(i) {
        t.enabled ^= true;
    }
    c.load_cached_tables();
    c.tables.refresh(&c.config.tables, false);
}

fn remove(c: &mut Ctx, i: usize) {
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

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let c = &ctx.config;
    let mut v = Vec::new();
    for (i, t) in c.tables.iter().enumerate() {
        let loaded = ctx
            .tables
            .list
            .iter()
            .find(|d| d.source_url == t.url)
            .map(|d| format!(" [{}]", d.entries.len()))
            .unwrap_or_default();
        let name = if t.name.is_empty() { &t.url } else { &t.name };
        v.push(Row::new(
            format!("{name}{loaded}"),
            onoff(t.enabled),
            Act::Run(toggle, i),
        ));
        v.push(Row::new(
            format!("    {}", t.url),
            "remove ×",
            Act::Run(remove, i),
        ));
    }
    v.extend([
        Row::new("Add table URL…", "+", Act::Text(TextField::AddTable)),
        Row::new(
            "Update all tables",
            "↻",
            Act::Run(|c, _| c.tables.refresh(&c.config.tables, true), 0),
        ),
        Row::adjust(
            "Third-party sources (mirrors, Ginger / Konmai MD5 lookups)",
            onoff(c.third_party_mirrors),
            |c, _| {
                c.config.third_party_mirrors ^= true;
                crate::hosts::set_third_party(c.config.third_party_mirrors);
            },
        ),
        Row::adjust(
            "Name song folders after their charts ([artist] title)",
            onoff(c.name_song_folders),
            |c, _| {
                c.config.name_song_folders ^= true;
                crate::hosts::set_folder_naming(c.config.name_song_folders);
            },
        ),
        Row::new("Restore default tables", "↵", Act::Run(restore_defaults, 0)),
    ]);
    v
}
