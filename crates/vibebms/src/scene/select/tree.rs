//! The folders and bars of the list: what each folder holds, its name and
//! its lamp / rank counts.

use super::SelectScene;
use crate::app::Ctx;
use crate::config::SortMode;
use crate::library::Song;
use crate::scene::skinvals::rank_index;
use std::collections::HashMap;
use std::path::Path;
use vibebms_core::course::CourseKind;
use vibebms_core::score::DjLevel;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Node {
    Root,
    Folder(String),
    All,
    /// A table, by its URL (`ctx.tables.list` is re-sorted as tables arrive).
    Table(String),
    Level(String, String),
    Courses(String),
    /// The player's courses of a kind (LR2's course folders).
    MyCourses(CourseKind),
    Search(String),
    /// A mirrored event, by its index's URL (the events are rebuilt as the
    /// lists arrive).
    Archive(String),
    /// The open folder's own bar, first in its list: choosing it closes
    /// the folder (as in IIDX and LR2).
    Close(Box<Node>),
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
}

/// A course in a list.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Charts (indices into the library) in the order chosen; ties by title,
/// difficulty and level.
fn sort_songs(ctx: &Ctx, v: &mut [usize]) {
    let songs = &ctx.lib.songs;
    let ln = ctx.config.play.ln_mode;
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
    pub(super) fn find(&self, md5: &str, sha: &str) -> Option<usize> {
        (!sha.is_empty())
            .then(|| self.sha_map.get(sha))
            .flatten()
            .or_else(|| (!md5.is_empty()).then(|| self.md5_map.get(md5)).flatten())
            .copied()
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
        }
    }

    /// The bars of `node`: the charts the key mode and difficulty filters
    /// keep, in the order chosen when `sort`.
    fn build(&self, ctx: &Ctx, node: &Node, sort: bool) -> Vec<Bar> {
        let songs = &ctx.lib.songs;
        let shown = |s: &Song| shown(ctx, s);
        let sorted = |v: Vec<usize>| -> Vec<Bar> {
            let mut v: Vec<usize> = v.into_iter().filter(|i| shown(&songs[*i])).collect();
            if sort {
                sort_songs(ctx, &mut v);
            }
            v.into_iter().map(Bar::Song).collect()
        };
        match node {
            Node::Close(_) => Vec::new(),
            Node::Root => {
                let mut bars = vec![Bar::Folder {
                    label: "ALL SONGS".into(),
                    sub: String::new(),
                    node: Node::All,
                    count: songs.iter().filter(|s| shown(s)).count(),
                }];
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
                        .map(|s| s.to_string_lossy().into_owned())
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
            Node::All => sorted((0..songs.len()).collect()),
            Node::Folder(f) => sorted(
                (0..songs.len())
                    .filter(|i| &songs[*i].folder == f)
                    .collect(),
            ),
            Node::Search(q) => {
                let q = q.to_lowercase();
                let has = |s: &str| s.to_lowercase().contains(&q);
                sorted(
                    (0..songs.len())
                        .filter(|i| {
                            let s = &songs[*i];
                            has(&s.title) || has(&s.artist) || has(&s.genre) || has(&s.subtitle)
                        })
                        .collect(),
                )
            }
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
                            .filter(|e| self.find(&e.md5, &e.sha256).is_some())
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
                    // the folder it was downloaded to, else a chart with its
                    // title and artist
                    let mine: Vec<usize> = match ctx.net.archives.installed_dir(&ev.name, w.num) {
                        Some(dir) => {
                            let dir = root.join(dir);
                            (0..songs.len())
                                .filter(|i| Path::new(&songs[*i].path).starts_with(&dir))
                                .collect()
                        }
                        None => (0..songs.len())
                            .filter(|i| {
                                crate::placement::same_title(&songs[*i].title, &w.title)
                                    && crate::placement::same_artist(&songs[*i].artist, &w.artist)
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
                        bars.extend(sorted(mine));
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
                    .filter_map(|e| match self.find(&e.md5, &e.sha256) {
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
    /// folder's own bar first (choosing it closes the folder).
    pub(super) fn listing(&self, ctx: &Ctx, node: &Node) -> Vec<Bar> {
        let mut bars = self.build(ctx, node, true);
        if *node != Node::Root && !bars.is_empty() {
            bars.insert(
                0,
                Bar::Folder {
                    label: self.node_name(ctx, node),
                    sub: "close".into(),
                    node: Node::Close(Box::new(node.clone())),
                    count: bars.len(),
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
        let ln = ctx.config.play.ln_mode;
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

    /// The name of a folder, as the path shows it.
    pub(super) fn node_name(&self, ctx: &Ctx, n: &Node) -> String {
        match n {
            Node::Root => "ROOT".into(),
            Node::All => "ALL SONGS".into(),
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
}
