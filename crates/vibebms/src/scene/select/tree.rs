//! The folders and bars of the list: what each folder holds, its name and
//! its lamp / rank counts.

use super::SelectScene;
use crate::config::SortMode;
use crate::ctx::Ctx;
use crate::library::Song;
use crate::scene::skinvals::rank_index;
use std::collections::HashMap;
use std::path::Path;
use vibebms_core::course::CourseKind;
use vibebms_core::score::{ClearType, DjLevel};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Node {
    Root,
    Folder(String),
    All,
    /// The player's favourite charts and songs' charts.
    Favorites,
    /// A folder made of the records or the library (`Smart`).
    Smart(Smart),
    /// A table, by its URL (`ctx.tables.list` is re-sorted as tables arrive).
    Table(String),
    Level(String, String),
    Courses(String),
    /// The player's courses of a kind (LR2's course folders).
    MyCourses(CourseKind),
    Search(String),
    /// SAME FOLDER: the charts in a chart's folder (`song_dir`).
    SameFolder(String),
    /// DOWNLOADS: the bulk download queue (`ctx.net.bulk`).
    Downloads,
    /// A mirrored event, by its index's URL (the events are rebuilt as the
    /// lists arrive).
    Archive(String),
    /// The open folder's own bar, first in its list: choosing it closes
    /// the folder (as in IIDX and LR2).
    Close(Box<Node>),
}

/// The folders made of the records and the library, under the root as
/// FAVORITE is (beatoraja's and bmz-player's kinds). Their charts are the
/// ones the filters keep, by the records of the LN and rule modes chosen.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Smart {
    /// Charts whose lamp a play raised to a clear, or whose best EX score
    /// a play raised, in the last `RECENT_DAYS`.
    LampUpdate,
    ScoreUpdate,
    /// CLEAR TYPE: a folder per lamp (`ClearType as u8`), and a lamp's.
    ClearTypes,
    Clear(u8),
    /// SCORE RANK: a folder per DJ level (`rank_index`: AAA 0 .. F 7, no
    /// play 8), and a level's.
    Ranks,
    Rank(u8),
    /// LEVEL: a folder per level, and a level's.
    Levels,
    Level(String),
    /// Charts whose file came (or changed) in the last `RECENT_DAYS`: the
    /// time the ADDED sort goes by.
    New,
}

/// The days LAMP UPDATE, SCORE UPDATE and NEW look back.
pub(crate) const RECENT_DAYS: i64 = 7;

const SMART_ROOT: [(Smart, &str); 6] = [
    (Smart::LampUpdate, "LAMP UPDATE"),
    (Smart::ScoreUpdate, "SCORE UPDATE"),
    (Smart::ClearTypes, "CLEAR TYPE"),
    (Smart::Ranks, "SCORE RANK"),
    (Smart::Levels, "LEVEL"),
    (Smart::New, "NEW"),
];

const RANK_LABELS: [&str; 9] = ["AAA", "AA", "A", "B", "C", "D", "E", "F", "NO PLAY"];

/// A chart's DJ level as `Smart::Rank` has it.
fn rank_of(best: Option<&crate::library::BestScore>) -> u8 {
    match best {
        Some(b) => rank_index(DjLevel::from_rate(b.ex_score, b.notes * 2)).clamp(0, 7) as u8,
        None => 8,
    }
}

impl Smart {
    /// Whether `s` is in this folder of charts (at Unix time `now`).
    fn has(&self, ctx: &Ctx, s: &Song, now: i64) -> bool {
        let best = ctx.best(s, ctx.config.play.ln_pref());
        let since = now - RECENT_DAYS * 86_400;
        match self {
            Smart::LampUpdate => best.is_some_and(|b| b.lamp_updated >= since),
            Smart::ScoreUpdate => best.is_some_and(|b| b.score_updated >= since),
            Smart::Clear(c) => best.map_or(ClearType::NoPlay, |b| b.clear) as u8 == *c,
            Smart::Rank(r) => rank_of(best) == *r,
            Smart::Level(l) => s.level.trim() == l,
            Smart::New => s.added >= since,
            Smart::ClearTypes | Smart::Ranks | Smart::Levels => false,
        }
    }

    /// A folder's name.
    fn name(&self) -> String {
        match self {
            Smart::Clear(c) => ClearType::ALL[*c as usize % ClearType::ALL.len()]
                .label()
                .into(),
            Smart::Rank(r) => RANK_LABELS[*r as usize % RANK_LABELS.len()].into(),
            Smart::Level(l) => format!("LEVEL {l}"),
            f => (SMART_ROOT.iter())
                .find(|(k, _)| k == f)
                .map_or_else(String::new, |(_, n)| n.to_string()),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Bar {
    Folder {
        label: String,
        sub: String,
        node: Node,
        count: usize,
    },
    Song(usize),
    Missing(Missing),
    Course(CourseBar),
    /// RANDOM SELECT: plays one of the folder's charts
    /// (`SelectConfig::random`).
    Random,
    /// The bulk download queue's commands and charts.
    Bulk(BulkBar),
}

/// A bar of the bulk downloads.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BulkBar {
    /// DOWNLOAD MISSING: every chart of this table or level the library
    /// lacks queued (how many).
    Queue(Node, usize),
    RetryFailed,
    CancelAll,
    ClearFinished,
    /// A chart in the queue (`ctx.net.bulk.items`).
    Item(usize),
}

impl Missing {
    /// What the bulk queue downloads of it.
    pub(crate) fn wanted(&self) -> crate::net::songfetch::Wanted {
        crate::net::songfetch::Wanted {
            title: self.title.clone(),
            artist: self.artist.clone(),
            md5: self.md5.clone(),
            org_md5: self.org_md5.clone(),
            level: self.level.clone(),
            url: self.url.clone(),
            url_diff: self.url_diff.clone(),
        }
    }
}

/// A course in a list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CourseBar {
    /// A table's (its URL), by its index there.
    Table(String, usize),
    /// The player's, by its id.
    Mine(u64),
    /// RANDOM MIX: makes a course of the kind.
    RandomMix(CourseKind),
}

/// A chart a table or an event lists that is not in the library.
#[derive(Clone, Debug, Default)]
pub(crate) struct Missing {
    pub title: String,
    pub artist: String,
    /// The chart's (for its other links).
    pub md5: String,
    /// The song's own charts a table names (where a separate chart goes).
    pub org_md5: Vec<String>,
    /// More links for the song, after `url` (an event work's other links,
    /// then its page).
    pub more: Vec<String>,
    /// Links of add-ons (差分) for the song: into its folder after it.
    pub addons: Vec<String>,
    pub level: String,
    /// Download pages: the song, and the chart when it is a separate
    /// (差分) chart for that song.
    pub url: String,
    pub url_diff: String,
    /// A mirrored work: (event's URL, work number) to download directly.
    pub archive: Option<(String, u32)>,
}

/// Clear lamp counts (by clear type) and DJ rank counts (AAA..F, no play).
pub(crate) type Distribution = ([u32; 11], [u32; 9]);

/// The key mode and difficulty filters keep `s`.
pub(crate) fn shown(ctx: &Ctx, s: &Song) -> bool {
    let f = &ctx.config.select;
    f.key_filter.keeps(s.mode) && f.difficulty_filter.keeps(s.difficulty)
}

/// The bars RANDOM SELECT may draw from `bars`: the charts whose lamp
/// (under the LN and rule modes chosen) its kind takes.
pub(crate) fn random_candidates(ctx: &Ctx, bars: &[Bar]) -> Vec<usize> {
    let ln = ctx.config.play.ln_pref();
    let kind = ctx.config.select.random;
    (0..bars.len())
        .filter(|&i| match &bars[i] {
            Bar::Song(k) => {
                let s = &ctx.lib.songs[*k];
                kind.keeps(ctx.best(s, ln).map_or(ClearType::NoPlay, |b| b.clear))
            }
            _ => false,
        })
        .collect()
}

/// `s` is a favourite chart, or of a favourite song.
pub(crate) fn favorite(ctx: &Ctx, s: &Song) -> bool {
    ctx.lib.db.as_ref().is_some_and(|l| l.favorites().has(s))
}

/// Charts (indices into the library) in the order chosen; ties by title,
/// difficulty and level.
fn sort_songs(ctx: &Ctx, v: &mut [usize]) {
    let songs = &ctx.lib.songs;
    let ln = ctx.config.play.ln_pref();
    let sort = ctx.config.select.sort;
    // the record under the LN and rule modes chosen
    let best = |s: &Song| ctx.best(s, ln);
    let first = |s: &Song| -> i64 {
        match sort {
            SortMode::Title | SortMode::Artist => 0,
            SortMode::Level => s.level_num(),
            SortMode::Clear => best(s).map_or(0, |b| b.clear as i64),
            // (not played: below 0 %)
            SortMode::Score => best(s).map_or(-1, |b| (b.rate() * 1e6) as i64),
            SortMode::MissCount => best(s).map_or(u32::MAX, |b| b.min_bp) as i64,
            SortMode::LastPlayed => -best(s).map_or(0, |b| b.last_played),
            SortMode::Added => -s.added,
            SortMode::Bpm => (s.main_bpm * 1000.0) as i64,
            SortMode::Notes => s.notes,
        }
    };
    v.sort_by_cached_key(|i| {
        let s = &songs[*i];
        let artist = match sort {
            SortMode::Artist => s.artist.to_lowercase(),
            _ => String::new(),
        };
        (
            first(s),
            artist,
            s.title.to_lowercase(),
            s.difficulty,
            s.level_num(),
        )
    });
}

/// The table at `url` (see [`Node::Table`]).
pub(crate) fn table<'a>(ctx: &'a Ctx, url: &str) -> Option<&'a bmseeker::table::DifficultyTable> {
    ctx.tables.list.iter().find(|t| t.source_url == url)
}

/// The index of the event at `url` in `ctx.net.archives.events` now.
pub(super) fn event(ctx: &Ctx, url: &str) -> Option<usize> {
    ctx.net.archives.events.iter().position(|e| e.url == url)
}

impl SelectScene {
    /// The library's chart a table entry names.
    fn entry_chart(&self, ctx: &Ctx, e: &bmseeker::table::TableEntry) -> Option<usize> {
        (self.charts).find(&ctx.lib.songs, &e.md5, &e.sha256, &e.title, &e.artist)
    }

    /// How many charts of the library the filters and `has` keep, a chart
    /// in several folders once (what a list not of one folder shows).
    fn count(&self, ctx: &Ctx, has: impl Fn(&Song) -> bool) -> usize {
        let songs = &ctx.lib.songs;
        let mut v: Vec<usize> = (0..songs.len())
            .filter(|&i| shown(ctx, &songs[i]) && has(&songs[i]))
            .collect();
        self.charts.one_each(&mut v);
        v.len()
    }

    /// What keeps a bar recognisable when the list is built again.
    pub(super) fn bar_key(&self, ctx: &Ctx, b: &Bar) -> String {
        match b {
            Bar::Folder { label, .. } => format!("f:{label}"),
            Bar::Song(i) => format!(
                "s:{}",
                ctx.lib.songs.get(*i).map(|s| s.path.as_str()).unwrap_or("")
            ),
            Bar::Missing(m) => format!("m:{}\u{1f}{}", m.title, m.artist),
            Bar::Course(c) => format!("c:{c:?}"),
            Bar::Random => "r".into(),
            Bar::Bulk(b) => format!("b:{b:?}"),
        }
    }

    /// The bars of `node`: the charts the key mode and difficulty filters
    /// keep, in the order chosen when `sort`.
    fn build(&self, ctx: &Ctx, node: &Node, sort: bool) -> Vec<Bar> {
        let songs = &ctx.lib.songs;
        let shown = |s: &Song| shown(ctx, s);
        // with `once` (a list not of one folder), a chart in several
        // folders once: of its copies the filters keep (as `count`)
        let rows = |v: Vec<usize>, once: bool| -> Vec<Bar> {
            let mut v: Vec<usize> = v.into_iter().filter(|i| shown(&songs[*i])).collect();
            if once {
                self.charts.one_each(&mut v);
            }
            if sort {
                sort_songs(ctx, &mut v);
            }
            v.into_iter().map(Bar::Song).collect()
        };
        let sorted = |v| rows(v, false);
        let once = |v| rows(v, true);
        match node {
            Node::Close(_) => Vec::new(),
            Node::Root => {
                let mut bars = vec![Bar::Folder {
                    label: "ALL SONGS".into(),
                    sub: String::new(),
                    node: Node::All,
                    count: self.count(ctx, |_| true),
                }];
                // the bulk downloads, while there are any
                let queue = &ctx.net.bulk;
                if !queue.items.is_empty() {
                    let [q, d, ..] = queue.counts();
                    bars.push(Bar::Folder {
                        label: "DOWNLOADS".into(),
                        sub: format!("{} to go", q + d),
                        node: Node::Downloads,
                        count: queue.items.len(),
                    });
                }
                bars.push(Bar::Folder {
                    label: "FAVORITE".into(),
                    sub: "♥".into(),
                    node: Node::Favorites,
                    count: self.count(ctx, |s| favorite(ctx, s)),
                });
                let now = crate::platform::unix_now() as i64;
                for (f, _) in SMART_ROOT {
                    let count = match &f {
                        Smart::ClearTypes | Smart::Ranks | Smart::Levels => {
                            self.count(ctx, |_| true)
                        }
                        f => self.count(ctx, |s| f.has(ctx, s, now)),
                    };
                    bars.push(Bar::Folder {
                        label: f.name(),
                        sub: "virtual".into(),
                        node: Node::Smart(f),
                        count,
                    });
                }
                // (a folder's count: the charts in it)
                let mut folders: Vec<(String, usize)> = Vec::new();
                let mut idx: HashMap<&str, usize> = HashMap::new();
                for s in songs.iter().filter(|s| shown(s)) {
                    match idx.get(s.folder.as_str()) {
                        Some(i) => folders[*i].1 += 1,
                        None => {
                            idx.insert(&s.folder, folders.len());
                            folders.push((s.folder.clone(), 1));
                        }
                    }
                }
                folders.sort_by_cached_key(|a| a.0.to_lowercase());
                for (f, n) in folders {
                    let name = Path::new(&f)
                        .file_name()
                        .map(|s| {
                            let s = s.to_string_lossy();
                            crate::platform::arcfs::shown_name(&s).to_string()
                        })
                        .unwrap_or_else(|| f.clone());
                    bars.push(Bar::Folder {
                        label: name,
                        sub: f.clone(),
                        node: Node::Folder(f),
                        count: n,
                    });
                }
                for ev in ctx.net.archives.events.iter() {
                    if ev.works.is_empty() {
                        continue;
                    }
                    bars.push(Bar::Folder {
                        label: ev.label().to_string(),
                        sub: if ev.mirrored { "mirror" } else { "links" }.into(),
                        node: Node::Archive(ev.url.clone()),
                        count: ev.works.len(),
                    });
                }
                // the player's courses, by kind (LR2's course folders)
                for kind in [CourseKind::Expert, CourseKind::Nonstop, CourseKind::Class] {
                    bars.push(Bar::Folder {
                        label: format!("{} COURSES", kind.label()),
                        sub: "courses".into(),
                        node: Node::MyCourses(kind),
                        count: ctx.lib.courses.of_kind(kind).len(),
                    });
                }
                for t in ctx.tables.list.iter() {
                    bars.push(Bar::Folder {
                        label: t.header.name.clone(),
                        sub: format!("{} levels", t.levels().len()),
                        node: Node::Table(t.source_url.clone()),
                        count: t.entries.len(),
                    });
                    if !t.header.courses.is_empty() {
                        bars.push(Bar::Folder {
                            label: format!("{} COURSES", t.header.name),
                            sub: String::new(),
                            node: Node::Courses(t.source_url.clone()),
                            count: t.header.courses.len(),
                        });
                    }
                }
                bars
            }
            Node::All => once((0..songs.len()).collect()),
            Node::Favorites => once(
                (0..songs.len())
                    .filter(|i| favorite(ctx, &songs[*i]))
                    .collect(),
            ),
            Node::Folder(f) => sorted(
                (0..songs.len())
                    .filter(|i| &songs[*i].folder == f)
                    .collect(),
            ),
            Node::Smart(f) => {
                let now = crate::platform::unix_now() as i64;
                let kept = || songs.iter().filter(|s| shown(s));
                // a folder of folders: one per value with charts
                let sub: Vec<Smart> = match f {
                    Smart::ClearTypes => {
                        (0..ClearType::ALL.len() as u8).map(Smart::Clear).collect()
                    }
                    Smart::Ranks => (0..RANK_LABELS.len() as u8).map(Smart::Rank).collect(),
                    Smart::Levels => {
                        let mut levels: Vec<&str> = kept().map(|s| s.level.trim()).collect();
                        levels
                            .sort_by_key(|l| (l.parse::<f64>().map_or(i64::MAX, |v| v as i64), *l));
                        levels.dedup();
                        levels.into_iter().map(|l| Smart::Level(l.into())).collect()
                    }
                    f => {
                        return once(
                            (0..songs.len())
                                .filter(|i| f.has(ctx, &songs[*i], now))
                                .collect(),
                        )
                    }
                };
                sub.into_iter()
                    .filter_map(|f| {
                        let count = self.count(ctx, |s| f.has(ctx, s, now));
                        (count > 0).then(|| Bar::Folder {
                            label: f.name(),
                            sub: "virtual".into(),
                            node: Node::Smart(f),
                            count,
                        })
                    })
                    .collect()
            }
            Node::Search(q) => {
                let q = q.to_lowercase();
                let has = |s: &str| s.to_lowercase().contains(&q);
                once(
                    (0..songs.len())
                        .filter(|i| {
                            let s = &songs[*i];
                            has(&s.title) || has(&s.artist) || has(&s.genre) || has(&s.subtitle)
                        })
                        .collect(),
                )
            }
            Node::Downloads => {
                let [queued, downloading, _, failed] = ctx.net.bulk.counts();
                let mut bars = Vec::new();
                if queued + downloading > 0 {
                    bars.push(Bar::Bulk(BulkBar::CancelAll));
                }
                if failed > 0 {
                    bars.push(Bar::Bulk(BulkBar::RetryFailed));
                }
                if ctx.net.bulk.items.iter().any(|i| !i.state.active()) {
                    bars.push(Bar::Bulk(BulkBar::ClearFinished));
                }
                bars.extend((0..ctx.net.bulk.items.len()).map(|k| Bar::Bulk(BulkBar::Item(k))));
                bars
            }
            Node::SameFolder(dir) => sorted(
                (0..songs.len())
                    .filter(|i| super::status::song_dir(&songs[*i]) == *dir)
                    .collect(),
            ),
            Node::Table(t) => {
                let Some(table) = table(ctx, t) else {
                    return Vec::new();
                };
                table
                    .levels()
                    .into_iter()
                    .map(|l| {
                        let entries: Vec<_> =
                            table.entries.iter().filter(|e| e.level == l).collect();
                        let have = entries
                            .iter()
                            .filter(|e| self.entry_chart(ctx, e).is_some())
                            .count();
                        Bar::Folder {
                            label: table.label(&l),
                            sub: format!("{have}/{}", entries.len()),
                            node: Node::Level(t.clone(), l),
                            count: entries.len(),
                        }
                    })
                    .collect()
            }
            Node::Archive(url) => {
                let Some(e) = event(ctx, url) else {
                    return Vec::new();
                };
                let ev = &ctx.net.archives.events[e];
                let root = &ctx.paths.root;
                let mut bars = Vec::new();
                for w in &ev.works {
                    // the folder it was downloaded to, else the charts with its
                    // title and artist (a chart in several folders once)
                    let dir = ctx.net.archives.installed_dir(&ev.name, w.num);
                    let mine: Vec<usize> = match &dir {
                        Some(dir) => {
                            let dir = root.join(dir);
                            (0..songs.len())
                                .filter(|i| Path::new(&songs[*i].path).starts_with(&dir))
                                .collect()
                        }
                        None => (0..songs.len())
                            .filter(|i| {
                                crate::net::placement::same_title(&songs[*i].title, &w.title)
                                    && crate::net::placement::same_artist(
                                        &songs[*i].artist,
                                        &w.artist,
                                    )
                            })
                            .collect(),
                    };
                    if mine.is_empty() {
                        bars.push(Bar::Missing(Missing {
                            title: w.title.clone(),
                            artist: w.artist.clone(),
                            more: w.links.iter().skip(1).chain([&w.page]).cloned().collect(),
                            addons: w.addons.clone(),
                            url: w.links.first().cloned().unwrap_or_else(|| w.page.clone()),
                            archive: ctx
                                .net
                                .archives
                                .can_download(e, w.num)
                                .then(|| (url.clone(), w.num)),
                            ..Missing::default()
                        }));
                    } else {
                        bars.extend(rows(mine, dir.is_none()));
                    }
                }
                bars
            }
            Node::Level(t, l) => {
                let Some(table) = table(ctx, t) else {
                    return Vec::new();
                };
                table
                    .entries
                    .iter()
                    .filter(|e| &e.level == l)
                    .filter_map(|e| match self.entry_chart(ctx, e) {
                        Some(i) => shown(&songs[i]).then_some(Bar::Song(i)),
                        None => Some(Bar::Missing(Missing {
                            title: e.title.clone(),
                            artist: e.artist.clone(),
                            md5: e.md5.clone(),
                            org_md5: e.org_md5.clone(),
                            level: table.label(&e.level),
                            url: e.url.clone(),
                            url_diff: e.url_diff.clone(),
                            ..Missing::default()
                        })),
                    })
                    .collect()
            }
            Node::Courses(t) => table(ctx, t)
                .map(|tb| {
                    (0..tb.header.courses.len())
                        .map(|c| Bar::Course(CourseBar::Table(t.clone(), c)))
                        .collect()
                })
                .unwrap_or_default(),
            // newest first, then (expert, NONSTOP) RANDOM MIX
            Node::MyCourses(kind) => ctx
                .lib
                .courses
                .of_kind(*kind)
                .iter()
                .map(|c| Bar::Course(CourseBar::Mine(c.id)))
                .chain(
                    (*kind != CourseKind::Class)
                        .then_some(Bar::Course(CourseBar::RandomMix(*kind))),
                )
                .collect(),
        }
    }

    /// The bars of `node` as the list shows them: below the root, the
    /// folder's own bar first (choosing it closes the folder), then RANDOM
    /// SELECT where there are charts.
    pub(super) fn listing(&self, ctx: &Ctx, node: &Node) -> Vec<Bar> {
        let mut bars = self.build(ctx, node, true);
        let count = bars.len();
        // a table's or a level's: DOWNLOAD MISSING
        let missing = self.missing_of(ctx, node).len();
        if missing > 0 {
            bars.insert(0, Bar::Bulk(BulkBar::Queue(node.clone(), missing)));
        }
        if bars.iter().any(|b| matches!(b, Bar::Song(_))) {
            bars.insert(0, Bar::Random);
        }
        if *node != Node::Root && count > 0 {
            bars.insert(
                0,
                Bar::Folder {
                    label: self.node_name(ctx, node),
                    sub: "close".into(),
                    node: Node::Close(Box::new(node.clone())),
                    count,
                },
            );
        }
        bars
    }

    /// Lamp / rank distribution of the folder at bar `i` (counted when
    /// first asked for).
    pub(crate) fn folder_distribution(&self, ctx: &Ctx, i: usize) -> Option<Distribution> {
        let node = match self.bars.get(i)? {
            Bar::Folder {
                node: Node::Close(inner),
                ..
            } => inner.as_ref(),
            Bar::Folder { node, .. } => node,
            _ => return None,
        };
        if let Some(d) = self.dist_cache.borrow().get(node) {
            return Some(*d);
        }
        let mut lamps = [0u32; 11];
        let mut ranks = [0u32; 9];
        let ln = ctx.config.play.ln_pref();
        let mut count = |bars: &[Bar]| {
            for b in bars {
                let Some(song) = (match b {
                    Bar::Song(k) => ctx.lib.songs.get(*k),
                    _ => None,
                }) else {
                    continue;
                };
                match ctx.best(song, ln) {
                    Some(best) => {
                        lamps[(best.clear as usize).min(10)] += 1;
                        let r = rank_index(DjLevel::from_rate(best.ex_score, best.notes * 2));
                        ranks[r.clamp(0, 7) as usize] += 1;
                    }
                    None => {
                        lamps[0] += 1;
                        ranks[8] += 1;
                    }
                }
            }
        };
        let bars = self.build(ctx, node, false);
        count(&bars);
        // tables: count their level folders too
        for b in &bars {
            if let Bar::Folder { node, .. } = b {
                count(&self.build(ctx, node, false));
            }
        }
        let d = (lamps, ranks);
        self.dist_cache.borrow_mut().insert(node.clone(), d);
        Some(d)
    }

    /// The charts a table (`Node::Table`) or a level lists that the
    /// library lacks, as the bulk queue takes them.
    pub(crate) fn missing_of(&self, ctx: &Ctx, node: &Node) -> Vec<crate::net::songfetch::Wanted> {
        let of = |bars: Vec<Bar>| -> Vec<crate::net::songfetch::Wanted> {
            (bars.into_iter())
                .filter_map(|b| match b {
                    Bar::Missing(m) => Some(m.wanted()),
                    _ => None,
                })
                .collect()
        };
        match node {
            Node::Level(..) => of(self.build(ctx, node, false)),
            Node::Table(_) => (self.build(ctx, node, false).into_iter())
                .flat_map(|b| match b {
                    Bar::Folder { node, .. } => of(self.build(ctx, &node, false)),
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The name of a folder, as the path shows it.
    pub(super) fn node_name(&self, ctx: &Ctx, n: &Node) -> String {
        match n {
            Node::Root => "ROOT".into(),
            Node::All => "ALL SONGS".into(),
            Node::Favorites => "FAVORITE".into(),
            Node::Smart(f) => f.name(),
            Node::Folder(f) => Path::new(f)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Node::Table(t) | Node::Courses(t) => table(ctx, t)
                .map(|t| t.header.name.clone())
                .unwrap_or_default(),
            Node::MyCourses(k) => format!("{} COURSES", k.label()),
            Node::Level(t, l) => table(ctx, t).map(|t| t.label(l)).unwrap_or_default(),
            Node::Search(q) => format!("SEARCH \"{q}\""),
            Node::Downloads => "DOWNLOADS".into(),
            Node::SameFolder(d) => format!(
                "SAME FOLDER {}",
                Path::new(d)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            Node::Archive(url) => event(ctx, url)
                .map(|e| ctx.net.archives.events[e].label().to_string())
                .unwrap_or_default(),
            Node::Close(inner) => self.node_name(ctx, inner),
        }
    }

    /// The open folders, from the root.
    pub(crate) fn path_label(&self, ctx: &Ctx) -> String {
        let mut parts: Vec<String> = self
            .stack
            .iter()
            .map(|(n, _)| self.node_name(ctx, n))
            .collect();
        parts.push(self.node_name(ctx, &self.node));
        parts.join(" › ")
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::config::{DifficultyFilter, KeyFilter};
    use crate::library::{BestScore, RecordKey};
    use std::sync::Arc;
    use vibebms_core::judge::JudgePreset;
    use vibebms_core::score::ClearType;
    use vibebms_core::PlayMode;

    fn song(title: &str, mode: PlayMode, difficulty: i32, level: &str, added: i64) -> Song {
        Song {
            path: format!("/bms/{title}.bms"),
            folder: "/bms".into(),
            sha256: title.into(),
            title: title.into(),
            artist: format!("artist {}", 9 - added),
            mode,
            difficulty,
            level: level.into(),
            added,
            ..Default::default()
        }
    }

    fn titles(s: &SelectScene, ctx: &Ctx, node: &Node) -> Vec<String> {
        s.build(ctx, node, true)
            .iter()
            .filter_map(|b| match b {
                Bar::Song(i) => Some(ctx.lib.songs[*i].title.clone()),
                _ => None,
            })
            .collect()
    }

    /// The sorts (ties by title) and the filters, which the root's counts
    /// follow; records are the rule mode's chosen.
    #[test]
    fn sorts_and_filters() {
        let root = std::env::temp_dir().join(format!("vibebms-sort-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        ctx.lib.songs = Arc::new(vec![
            song("a", PlayMode::Beat7K, 4, "12", 1),
            song("b", PlayMode::Beat7K, 2, "3", 3),
            song("c", PlayMode::Beat14K, 4, "7", 2),
            song("d", PlayMode::Popn9K, 5, "3", 0),
        ]);
        let mut rec = |id: &str, clear, ex, bp, rule| {
            let b = BestScore {
                clear,
                ex_score: ex,
                notes: 100,
                min_bp: bp,
                ..Default::default()
            };
            ctx.lib.scores.insert(RecordKey::new(id, 0, rule), b);
        };
        rec("a", ClearType::Hard, 150, 5, JudgePreset::Beatoraja);
        rec("b", ClearType::Easy, 180, 20, JudgePreset::Beatoraja);
        rec("c", ClearType::FullCombo, 100, 0, JudgePreset::Lr2);
        let s = SelectScene::new(&mut ctx);
        let mut order = |sort| {
            ctx.config.select.sort = sort;
            titles(&s, &ctx, &Node::All).join("")
        };
        assert_eq!(order(SortMode::Title), "abcd");
        assert_eq!(order(SortMode::Level), "bdca");
        // not played (c's record is LR2's), then EASY, HARD
        assert_eq!(order(SortMode::Clear), "cdba");
        assert_eq!(order(SortMode::Score), "cdab");
        assert_eq!(order(SortMode::MissCount), "abcd");
        assert_eq!(order(SortMode::Added), "bcad");
        assert_eq!(order(SortMode::Artist), "bcad");
        ctx.config.play.judge_preset = JudgePreset::Lr2;
        ctx.config.select.sort = SortMode::Clear;
        assert_eq!(titles(&s, &ctx, &Node::All).join(""), "abdc");
        ctx.config.select.sort = SortMode::Title;
        ctx.config.select.key_filter = KeyFilter::K7;
        assert_eq!(titles(&s, &ctx, &Node::All).join(""), "ab");
        ctx.config.select.difficulty_filter = DifficultyFilter::Another;
        assert_eq!(titles(&s, &ctx, &Node::All).join(""), "a");
        let counts: Vec<usize> = s
            .build(&ctx, &Node::Root, true)
            .iter()
            .filter_map(|b| match b {
                Bar::Folder {
                    node: Node::All | Node::Folder(_),
                    count,
                    ..
                } => Some(*count),
                _ => None,
            })
            .collect();
        assert_eq!(counts, [1, 1]);
        // nothing left: no folder of it
        ctx.config.select.key_filter = KeyFilter::K5;
        let folders = s.build(&ctx, &Node::Root, true);
        assert!(!folders.iter().any(|b| matches!(
            b,
            Bar::Folder {
                node: Node::Folder(_),
                ..
            }
        )));
        std::fs::remove_dir_all(&root).ok();
    }

    /// The FAVORITE folder: favourite charts and the charts of favourite
    /// songs, filtered and sorted as any; their bars marked.
    #[test]
    fn favorite_folder() {
        let root = std::env::temp_dir().join(format!("vibebms-favs-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut ctx = Ctx::for_test(root.clone());
        let mut c = song("c", PlayMode::Beat7K, 2, "1", 0);
        c.path = "/bms/other/c.bms".into();
        ctx.lib.songs = Arc::new(vec![
            song("a", PlayMode::Beat7K, 4, "12", 1),
            song("b", PlayMode::Beat14K, 2, "3", 3),
            c,
        ]);
        let mut lib = crate::library::Library::open(&root, &root, |_, _| None);
        lib.toggle_favorite(&ctx.lib.songs[2], false);
        // a and b are of one song (folder)
        lib.toggle_favorite(&ctx.lib.songs[1], true);
        ctx.lib.db = Some(lib);
        let s = SelectScene::new(&mut ctx);
        assert_eq!(titles(&s, &ctx, &Node::Favorites).join(""), "abc");
        ctx.config.select.key_filter = KeyFilter::K7;
        assert_eq!(titles(&s, &ctx, &Node::Favorites).join(""), "ac");
        let fav = s.build(&ctx, &Node::Root, true);
        assert!(matches!(
            &fav[1],
            Bar::Folder {
                node: Node::Favorites,
                count: 2,
                ..
            }
        ));
        let mut s = s;
        s.bars = vec![Bar::Song(0)];
        let st = super::super::SelectState::new(&s, &ctx);
        assert_eq!(st.bar(0).unwrap().title, "♥ a");
        std::fs::remove_dir_all(&root).ok();
    }

    /// The virtual folders: what the last week raised or added, lamps, DJ
    /// levels and levels, by the records of the rule mode chosen and with
    /// the filters.
    #[test]
    fn virtual_folders() {
        let root = std::env::temp_dir().join(format!("vibebms-smart-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let now = crate::platform::unix_now() as i64;
        let (recent, old) = (now - 3 * 86_400, now - 30 * 86_400);
        ctx.lib.songs = Arc::new(vec![
            song("a", PlayMode::Beat7K, 4, "12", recent),
            song("b", PlayMode::Beat7K, 2, "3", old),
            song("c", PlayMode::Beat14K, 4, "12", old),
            song("d", PlayMode::Beat7K, 2, "10", old),
        ]);
        let mut rec = |id: &str, clear, ex, lamp, score, rule| {
            let b = BestScore {
                clear,
                ex_score: ex,
                notes: 100,
                lamp_updated: lamp,
                score_updated: score,
                ..Default::default()
            };
            ctx.lib.scores.insert(RecordKey::new(id, 0, rule), b);
        };
        rec(
            "a",
            ClearType::Hard,
            190,
            old,
            recent,
            JudgePreset::Beatoraja,
        );
        rec(
            "b",
            ClearType::Easy,
            120,
            recent,
            old,
            JudgePreset::Beatoraja,
        );
        rec(
            "c",
            ClearType::Hard,
            150,
            recent,
            recent,
            JudgePreset::Beatoraja,
        );
        rec(
            "d",
            ClearType::FullCombo,
            200,
            recent,
            recent,
            JudgePreset::Lr2,
        );
        let s = SelectScene::new(&mut ctx);
        let names = |ctx: &Ctx, n: &Node| -> Vec<String> {
            s.build(ctx, n, true)
                .iter()
                .map(|b| match b {
                    Bar::Folder { label, count, .. } => format!("{label} {count}"),
                    Bar::Song(i) => ctx.lib.songs[*i].title.clone(),
                    _ => String::new(),
                })
                .collect()
        };
        let smart = |f| Node::Smart(f);
        assert_eq!(titles(&s, &ctx, &smart(Smart::LampUpdate)).join(""), "bc");
        assert_eq!(titles(&s, &ctx, &smart(Smart::ScoreUpdate)).join(""), "ac");
        assert_eq!(titles(&s, &ctx, &smart(Smart::New)).join(""), "a");
        assert_eq!(
            names(&ctx, &smart(Smart::ClearTypes)),
            ["NO PLAY 1", "EASY 1", "HARD 2"]
        );
        assert_eq!(titles(&s, &ctx, &smart(Smart::Clear(6))).join(""), "ac");
        // 190 of 200 is AAA, 150 A, 120 B
        assert_eq!(
            names(&ctx, &smart(Smart::Ranks)),
            ["AAA 1", "A 1", "B 1", "NO PLAY 1"]
        );
        assert_eq!(
            names(&ctx, &smart(Smart::Levels)),
            ["LEVEL 3 1", "LEVEL 10 1", "LEVEL 12 2"]
        );
        assert_eq!(
            titles(&s, &ctx, &smart(Smart::Level("12".into()))).join(""),
            "ac"
        );
        // the root: after FAVORITE, with counts
        let root_names = names(&ctx, &Node::Root);
        assert_eq!(
            root_names[2..8],
            [
                "LAMP UPDATE 2",
                "SCORE UPDATE 2",
                "CLEAR TYPE 4",
                "SCORE RANK 4",
                "LEVEL 4",
                "NEW 1"
            ]
        );
        // the key mode filter and the rule mode
        ctx.config.select.key_filter = KeyFilter::K7;
        assert_eq!(titles(&s, &ctx, &smart(Smart::LampUpdate)).join(""), "b");
        assert_eq!(
            names(&ctx, &smart(Smart::Levels)),
            ["LEVEL 3 1", "LEVEL 10 1", "LEVEL 12 1"]
        );
        ctx.config.play.judge_preset = JudgePreset::Lr2;
        assert_eq!(titles(&s, &ctx, &smart(Smart::LampUpdate)).join(""), "d");
        assert_eq!(
            names(&ctx, &smart(Smart::ClearTypes)),
            ["NO PLAY 2", "FULL COMBO 1"]
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// RANDOM SELECT heads every folder with charts (after its own bar)
    /// and draws from the charts its kind takes, by the lamps of the rule
    /// mode chosen.
    #[test]
    fn random_select() {
        use crate::config::RandomSelect;
        let root = std::env::temp_dir().join(format!("vibebms-rand-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        ctx.lib.songs = Arc::new(vec![
            song("a", PlayMode::Beat7K, 4, "12", 1),
            song("b", PlayMode::Beat7K, 2, "3", 3),
            song("c", PlayMode::Beat7K, 2, "5", 2),
            song("d", PlayMode::Beat7K, 2, "5", 2),
        ]);
        for (id, clear) in [
            ("a", ClearType::Easy),
            ("b", ClearType::Hard),
            ("c", ClearType::FullCombo),
        ] {
            let b = BestScore {
                clear,
                ..Default::default()
            };
            let key = RecordKey::new(id, 0, JudgePreset::Beatoraja);
            ctx.lib.scores.insert(key, b);
        }
        let s = SelectScene::new(&mut ctx);
        let bars = s.listing(&ctx, &Node::All);
        assert!(matches!(bars[..2], [Bar::Folder { .. }, Bar::Random]));
        assert!(!s
            .listing(&ctx, &Node::Root)
            .iter()
            .any(|b| matches!(b, Bar::Random)));
        let mut drawn = |kind| {
            ctx.config.select.random = kind;
            random_candidates(&ctx, &bars)
                .iter()
                .map(|i| match bars[*i] {
                    Bar::Song(k) => ctx.lib.songs[k].title.clone(),
                    _ => unreachable!(),
                })
                .collect::<String>()
        };
        assert_eq!(drawn(RandomSelect::Any), "abcd");
        assert_eq!(drawn(RandomSelect::NotCleared), "ad");
        assert_eq!(drawn(RandomSelect::NotHard), "ad");
        assert_eq!(drawn(RandomSelect::NotFc), "abd");
        ctx.config.play.judge_preset = JudgePreset::Lr2;
        ctx.config.select.random = RandomSelect::NotFc;
        assert_eq!(random_candidates(&ctx, &bars).len(), 4);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A level's (and its table's) missing charts: DOWNLOAD MISSING queues
    /// them, and DOWNLOADS at the root lists the queue.
    #[test]
    fn download_missing_queues_a_level() {
        use bmseeker::table::{DifficultyTable, TableEntry};
        let root = std::env::temp_dir().join(format!("vibebms-bulk-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let mut have = song("have", PlayMode::Beat7K, 4, "1", 0);
        have.md5 = "a".repeat(32);
        ctx.lib.songs = Arc::new(vec![have]);
        let entry = |md5: char, level: &str| TableEntry {
            md5: md5.to_string().repeat(32),
            level: level.into(),
            title: format!("song {md5}"),
            ..Default::default()
        };
        ctx.tables.list = vec![DifficultyTable {
            source_url: "t".into(),
            entries: vec![
                entry('a', "1"),
                entry('b', "1"),
                entry('c', "1"),
                entry('d', "2"),
            ],
            ..Default::default()
        }];
        let mut s = SelectScene::new(&mut ctx);
        let level = Node::Level("t".into(), "1".into());
        assert_eq!(s.missing_of(&ctx, &level).len(), 2);
        assert_eq!(s.missing_of(&ctx, &Node::Table("t".into())).len(), 3);
        s.enter(&mut ctx, level.clone());
        let at = (s.bars.iter())
            .position(|b| matches!(b, Bar::Bulk(BulkBar::Queue(_, 2))))
            .unwrap();
        s.cursor = at;
        s.decide(&mut ctx, false);
        assert_eq!(ctx.net.bulk.counts(), [2, 0, 0, 0]);
        let root_bars = s.build(&ctx, &Node::Root, true);
        assert!(root_bars.iter().any(|b| matches!(
            b,
            Bar::Folder {
                node: Node::Downloads,
                count: 2,
                ..
            }
        )));
        let queue = s.build(&ctx, &Node::Downloads, true);
        assert!(matches!(
            queue[..],
            [
                Bar::Bulk(BulkBar::CancelAll),
                Bar::Bulk(BulkBar::Item(0)),
                Bar::Bulk(BulkBar::Item(1))
            ]
        ));
        std::fs::remove_dir_all(&root).ok();
    }

    /// A copy of chart `sha` in folder `dir` (below `/bms`).
    fn copy(title: &str, dir: &str, sha: &str) -> Song {
        Song {
            path: format!("/bms/{dir}/{title}.bms"),
            folder: format!("/bms/{dir}"),
            sha256: sha.into(),
            md5: format!("md5 of {sha}"),
            ..song(title, PlayMode::Beat7K, 2, "5", 0)
        }
    }

    /// The paths of the charts `node` lists, and the counts of the root's
    /// folders.
    fn paths(s: &SelectScene, ctx: &Ctx, node: &Node) -> Vec<String> {
        (s.build(ctx, node, true).iter())
            .filter_map(|b| match b {
                Bar::Song(i) => Some(ctx.lib.songs[*i].path.clone()),
                _ => None,
            })
            .collect()
    }

    fn root_counts(s: &SelectScene, ctx: &Ctx) -> Vec<String> {
        (s.build(ctx, &Node::Root, true).iter())
            .filter_map(|b| match b {
                Bar::Folder { label, count, .. } => Some(format!("{label} {count}")),
                _ => None,
            })
            .collect()
    }

    /// A chart in two folders: one row in ALL SONGS, search, the virtual
    /// folders and their counts; its row in each of its folders.
    #[test]
    fn copies_are_one_chart() {
        let root = std::env::temp_dir().join(format!("vibebms-copies-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        ctx.lib.songs = Arc::new(vec![
            copy("x", "a", "sx"),
            copy("x", "b", "sx"),
            copy("y", "b", "sy"),
        ]);
        let s = SelectScene::new(&mut ctx);
        let (a, b) = ("/bms/a/x.bms", "/bms/b/x.bms");
        assert_eq!(paths(&s, &ctx, &Node::All), [a, "/bms/b/y.bms"]);
        assert_eq!(paths(&s, &ctx, &Node::Search("x".into())), [a]);
        assert_eq!(
            paths(&s, &ctx, &Node::Smart(Smart::Level("5".into()))).len(),
            2
        );
        assert_eq!(paths(&s, &ctx, &Node::Folder("/bms/a".into())), [a]);
        assert_eq!(
            paths(&s, &ctx, &Node::Folder("/bms/b".into())),
            [b, "/bms/b/y.bms"]
        );
        assert_eq!(paths(&s, &ctx, &Node::SameFolder("/bms/b".into())).len(), 2);
        let counts = root_counts(&s, &ctx);
        assert_eq!(counts[0], "ALL SONGS 2");
        assert!(counts.contains(&"LEVEL 2".to_string()), "{counts:?}");
        assert!(counts.contains(&"a 1".to_string()), "{counts:?}");
        assert!(counts.contains(&"b 2".to_string()), "{counts:?}");
        // RANDOM SELECT draws from the rows
        let bars = s.listing(&ctx, &Node::All);
        assert_eq!(random_candidates(&ctx, &bars).len(), 2);
        std::fs::remove_dir_all(&root).ok();
    }

    /// One file held as a BMS and a PMS (the extension sets the mode):
    /// under a key filter each list shows the copy it keeps, and every
    /// folder's count is its rows.
    #[test]
    fn count_and_rows_agree_under_a_filter() {
        let root = std::env::temp_dir().join(format!("vibebms-filtered-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut ctx = Ctx::for_test(root.clone());
        let pms = Song {
            path: "/bms/b/x.pms".into(),
            mode: PlayMode::Popn9K,
            ..copy("x", "b", "sx")
        };
        ctx.lib.songs = Arc::new(vec![copy("x", "a", "sx"), pms, copy("y", "c", "sy")]);
        let mut lib = crate::library::Library::open(&root, &root, |_, _| None);
        lib.toggle_favorite(&ctx.lib.songs[0], false);
        ctx.lib.db = Some(lib);
        let s = SelectScene::new(&mut ctx);
        // the rows of `node`, or of the folders it holds, against `count`
        fn agree(s: &SelectScene, ctx: &Ctx, node: &Node, count: usize) {
            let bars = s.build(ctx, node, true);
            let rows = bars.iter().filter(|b| matches!(b, Bar::Song(_))).count();
            let mut folders = 0;
            for b in &bars {
                if let Bar::Folder { node: n, count, .. } = b {
                    folders += count;
                    agree(s, ctx, n, *count);
                }
            }
            assert_eq!(rows + folders, count, "{node:?}");
        }
        for (filter, all) in [(KeyFilter::All, 2), (KeyFilter::K9, 1), (KeyFilter::K7, 2)] {
            ctx.config.select.key_filter = filter;
            let mut lists = 0;
            for b in s.build(&ctx, &Node::Root, true) {
                let Bar::Folder { node, count, .. } = b else {
                    continue;
                };
                if matches!(
                    node,
                    Node::All | Node::Favorites | Node::Smart(_) | Node::Folder(_)
                ) {
                    agree(&s, &ctx, &node, count);
                    lists += 1;
                }
                if node == Node::All {
                    assert_eq!(count, all, "{filter:?}");
                }
            }
            assert!(lists >= 8, "{filter:?}");
            let found = paths(&s, &ctx, &Node::Search("x".into()));
            assert_eq!(found.len(), 1, "{filter:?}");
        }
        ctx.config.select.key_filter = KeyFilter::K9;
        assert_eq!(paths(&s, &ctx, &Node::All), ["/bms/b/x.pms"]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A copy inside an archive and a loose one: the loose one is the
    /// row; three copies are one row; records without a SHA-256 are never
    /// one chart.
    #[test]
    fn archived_three_and_unhashed_copies() {
        let root = std::env::temp_dir().join(format!("vibebms-copies3-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let packed = Song {
            path: "/bms/a.zip!/x.bms".into(),
            folder: "/bms/a.zip!".into(),
            ..copy("x", "a", "sx")
        };
        ctx.lib.songs = Arc::new(vec![
            packed,
            copy("x", "b", "sx"),
            copy("x", "c", "sx"),
            copy("z", "d", ""),
            copy("z", "e", ""),
        ]);
        let s = SelectScene::new(&mut ctx);
        assert_eq!(
            paths(&s, &ctx, &Node::All),
            ["/bms/b/x.bms", "/bms/d/z.bms", "/bms/e/z.bms"]
        );
        assert_eq!(root_counts(&s, &ctx)[0], "ALL SONGS 3");
        for dir in ["/bms/a.zip!", "/bms/b", "/bms/c"] {
            assert_eq!(paths(&s, &ctx, &Node::Folder(dir.into())).len(), 1, "{dir}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// The favourites: a song favourite (by folder) on a later copy lists
    /// that copy, once.
    #[test]
    fn favorite_copy() {
        let root = std::env::temp_dir().join(format!("vibebms-favcopy-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut ctx = Ctx::for_test(root.clone());
        ctx.lib.songs = Arc::new(vec![copy("x", "a", "sx"), copy("x", "b", "sx")]);
        let mut lib = crate::library::Library::open(&root, &root, |_, _| None);
        lib.toggle_favorite(&ctx.lib.songs[1], true);
        ctx.lib.db = Some(lib);
        let s = SelectScene::new(&mut ctx);
        assert_eq!(paths(&s, &ctx, &Node::Favorites), ["/bms/b/x.bms"]);
        assert!(root_counts(&s, &ctx).contains(&"FAVORITE 1".to_string()));
        std::fs::remove_dir_all(&root).ok();
    }

    /// A table entry and a course stage take the first copy in library
    /// order, whatever other charts are around it.
    #[test]
    fn tables_and_courses_take_the_first_copy() {
        use bmseeker::table::{DifficultyTable, TableEntry};
        let root = std::env::temp_dir().join(format!("vibebms-first-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        ctx.tables.list = vec![DifficultyTable {
            source_url: "t".into(),
            entries: vec![TableEntry {
                md5: "md5 of sx".into(),
                level: "1".into(),
                ..Default::default()
            }],
            ..Default::default()
        }];
        ctx.lib.courses.list.push(crate::library::courses::Course {
            id: 1,
            title: "C".into(),
            stages: vec![crate::library::courses::Stage {
                md5: "md5 of sx".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let (x, y, z) = (
            copy("x", "a", "sx"),
            copy("y", "a", "sy"),
            copy("z", "a", "sz"),
        );
        let x2 = copy("x", "b", "sx");
        for order in [
            vec![x.clone(), y.clone(), x2.clone()],
            vec![z.clone(), x.clone(), x2.clone(), y.clone()],
        ] {
            ctx.lib.songs = Arc::new(order);
            ctx.lib.version += 1;
            let s = SelectScene::new(&mut ctx);
            let level = Node::Level("t".into(), "1".into());
            assert_eq!(paths(&s, &ctx, &level), ["/bms/a/x.bms"]);
            let stages = s.course_facts(&ctx, &CourseBar::Mine(1), |f| f.songs.clone());
            let got: Vec<&str> = stages
                .iter()
                .map(|&i| ctx.lib.songs[i].path.as_str())
                .collect();
            assert_eq!(got, ["/bms/a/x.bms"]);
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// Two charts of one MD5 (a collision): a table entry's title and
    /// artist choose; without a title, the first.
    #[test]
    fn md5_collision_goes_by_title() {
        use bmseeker::table::{DifficultyTable, TableEntry};
        let root = std::env::temp_dir().join(format!("vibebms-collide-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let crafted = Song {
            md5: "m".into(),
            artist: "someone else".into(),
            ..copy("aaa crafted", "a", "s1")
        };
        let real = Song {
            md5: "m".into(),
            ..copy("real song", "b", "s2")
        };
        ctx.lib.songs = Arc::new(vec![crafted, real]);
        let entry = |title: &str, level: &str| TableEntry {
            md5: "m".into(),
            title: title.into(),
            artist: "artist 9".into(),
            level: level.into(),
            ..Default::default()
        };
        ctx.tables.list = vec![DifficultyTable {
            source_url: "t".into(),
            entries: vec![entry("Real Song", "1"), entry("", "2")],
            ..Default::default()
        }];
        let s = SelectScene::new(&mut ctx);
        let level = |l: &str| Node::Level("t".into(), l.into());
        assert_eq!(paths(&s, &ctx, &level("1")), ["/bms/b/real song.bms"]);
        assert_eq!(paths(&s, &ctx, &level("2")), ["/bms/a/aaa crafted.bms"]);
        std::fs::remove_dir_all(&root).ok();
    }
}
