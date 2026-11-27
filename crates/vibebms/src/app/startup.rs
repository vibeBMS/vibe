//! The start's background work. The caches the first frame does without
//! (the difficulty tables, the events' indexes, the skin catalog) are read
//! in jobs and taken in as they come, the song select listing their folders
//! from then (`lib.version`); what is stale in them is fetched then.

use crate::archive::Archives;
use crate::config::TableConfig;
use crate::ctx::Ctx;
use crate::platform::Job;
use crate::skinstore::SkinStore;
use bmseeker::links::Links;
use bmseeker::table::DifficultyTable;

/// The tables as cached, and their links.
type Cached = (Vec<DifficultyTable>, Links);

#[derive(Default)]
pub struct Startup {
    /// The enabled tables, with the configuration's tables they were read
    /// for.
    tables: Option<Job<(Vec<TableConfig>, Cached)>>,
    events: Option<Job<Archives>>,
    skins: Option<Job<SkinStore>>,
}

impl Startup {
    /// The caches read in the background; the Stella uploader's list, the
    /// library's scan and the update check started.
    pub fn begin(ctx: &mut Ctx) -> Startup {
        let paths = ctx.paths.clone();
        let tables = ctx.config.tables.clone();
        let p = paths.clone();
        let tables = Job::spawn("tables", move |_| {
            let cached = crate::ctx::tables::read_cached(&p, &tables);
            (tables, cached)
        });
        let (p, urls) = (paths.clone(), ctx.config.archives.clone());
        let events = Job::spawn("events", move |_| Archives::load(&p, &urls));
        let skins = Job::spawn("skins", move |_| SkinStore::load(&paths));
        ctx.tables.links.start(&ctx.paths.cache);
        // the web build's first scan comes once the songs kept in the
        // browser are mounted
        if !crate::platform::IS_WEB {
            ctx.start_scan();
        }
        // a newer nightly (desktop CI builds), in the background
        ctx.net.updater.check(ctx.config.check_updates);
        Startup {
            tables: Some(tables),
            events: Some(events),
            skins: Some(skins),
        }
    }

    /// What has been read taken into `ctx`, and what is stale fetched.
    pub fn poll(&mut self, ctx: &mut Ctx) {
        if self.tables.is_none() && self.events.is_none() && self.skins.is_none() {
            return;
        }
        for (read_for, (list, links)) in Job::drain_slot(&mut self.tables) {
            // the tables were changed, or one was fetched, meanwhile: read
            // again as they are now
            if read_for == ctx.config.tables && ctx.tables.list.is_empty() {
                ctx.tables.set(list, links);
            } else {
                ctx.tables.load_cached(&ctx.paths, &ctx.config.tables);
            }
            ctx.tables.refresh(&ctx.config.tables, false);
            ctx.lib.version += 1;
        }
        for a in Job::drain_slot(&mut self.events) {
            ctx.net.archives = a;
            ctx.net.archives.refresh(false);
            ctx.lib.version += 1;
        }
        for s in Job::drain_slot(&mut self.skins) {
            ctx.skins.store = s;
            ctx.skins.store.refresh(false);
        }
        if self.tables.is_none() && self.events.is_none() && self.skins.is_none() {
            super::diag::startup("tables, events, skins read");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `startup` polled until its caches are in (at most a few seconds).
    fn land(startup: &mut Startup, ctx: &mut Ctx) {
        let t = std::time::Instant::now();
        while startup.tables.is_some() || startup.events.is_some() || startup.skins.is_some() {
            assert!(t.elapsed().as_secs() < 10, "the caches did not come");
            startup.poll(ctx);
            std::thread::yield_now();
        }
    }

    /// The tables come after the start, as cached; tables changed before
    /// they came are read as they are then.
    #[test]
    fn caches_come_in_the_background() {
        let root = std::env::temp_dir().join(format!("vibebms-startup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut ctx = Ctx::for_test(root.clone());
        // (the Stella list from a fresh cache, not fetched)
        std::fs::write(ctx.paths.cache.join("links-stella.json"), "[]").unwrap();
        let url = "https://tables.invalid/t.html";
        ctx.config.tables = vec![TableConfig {
            name: "T".into(),
            url: url.into(),
            enabled: true,
        }];
        ctx.config.archives.clear();
        let table = DifficultyTable {
            source_url: url.into(),
            fetched_at: crate::platform::unix_now(),
            ..Default::default()
        };
        let file = (ctx.paths.tables).join(format!(
            "{}.json",
            vibebms_core::bms::md5_hex(url.as_bytes())
        ));
        std::fs::write(&file, serde_json::to_vec(&table).unwrap()).unwrap();
        let version = ctx.lib.version;
        let mut startup = Startup::begin(&mut ctx);
        land(&mut startup, &mut ctx);
        assert_eq!(ctx.tables.list, [table]);
        assert!(ctx.lib.version > version);
        assert!(!ctx.skins.store.catalog.skins.is_empty());
        // turned off before the caches came: not listed
        let mut startup = Startup::begin(&mut ctx);
        ctx.config.tables[0].enabled = false;
        ctx.load_cached_tables();
        land(&mut startup, &mut ctx);
        assert!(ctx.tables.list.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
