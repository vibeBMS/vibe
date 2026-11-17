//! Song select: folders, difficulty tables, courses, search, options.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::{Bpms, HsFix, Versus};
use crate::gfx::text::Align;
use crate::gfx::{with_alpha, Gfx, Rect, TexId};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::library::Song;
use crate::platform::{Duration, Instant};
use crate::scene::play::{CourseRun, PlayRequest, PlayScene};
use crate::scene::select_skin::{SelectHooks, SelectState};
use crate::scene::skinned;
use crate::skin::render::LoadedSkin;
use crate::skin::SkinType;
use crate::sysound::Sound;
use crate::theme;
use crate::ui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use vibebms_audio::{Bus, PlayOptions, Sample};
use vibebms_core::gauge::{GaugeAutoShift, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::LaneOption;
use vibebms_core::score::DjLevel;
use vibebms_core::LnType;
use vibebms_ir::IrScore;
use winit::keyboard::KeyCode;

const PREVIEW_KEY: u64 = u64::MAX - 1;

/// Time constant (s) of the list easing toward the cursor: 30 % of the
/// way per frame at 60 Hz, as it went by frames before.
const SCROLL_TAU: f32 = 0.0467;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Root,
    Folder(String),
    All,
    /// A table, by its URL (`ctx.tables` is re-sorted as tables arrive).
    Table(String),
    Level(String, String),
    Courses(String),
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
    Missing {
        title: String,
        artist: String,
        /// The chart's (for its other links).
        md5: String,
        /// The song's own charts a table names (where a separate chart
        /// goes).
        org_md5: Vec<String>,
        /// More links for the song, after `url` (an event work's other
        /// links, then its page).
        more: Vec<String>,
        /// Links of add-ons (差分) for the song: into its folder after it.
        addons: Vec<String>,
        level: String,
        /// Download pages: the song, and the chart when it is a separate
        /// (差分) chart for that song.
        url: String,
        url_diff: String,
        /// A mirrored work: (event's URL, work number) to download directly.
        archive: Option<(String, u32)>,
    },
    Course {
        /// The table's URL.
        table: String,
        course: usize,
    },
}

/// The table at `url` (see [`Node::Table`]).
pub(crate) fn table<'a>(ctx: &'a Ctx, url: &str) -> Option<&'a bmseeker::table::DifficultyTable> {
    ctx.tables.iter().find(|t| t.source_url == url)
}

/// The index of the event at `url` in `ctx.archives.events` now.
fn event(ctx: &Ctx, url: &str) -> Option<usize> {
    ctx.archives.events.iter().position(|e| e.url == url)
}

pub(crate) enum Rank {
    Loading(Receiver<Result<Vec<IrScore>, String>>),
    Done(Vec<IrScore>),
    Error,
}

pub struct SelectScene {
    version: u64,
    /// Web: the kept song folder a first Delete press asked about.
    #[cfg(target_arch = "wasm32")]
    delete_armed: Option<PathBuf>,
    /// Last position of each turntable axis (it scrolls the list).
    axis_last: HashMap<u64, f32>,
    pub(crate) node: Node,
    stack: Vec<(Node, usize)>,
    pub(crate) bars: Vec<Bar>,
    pub(crate) cursor: usize,
    pub(crate) scroll: f32,
    /// When `scroll` last eased toward the cursor.
    scrolled_at: Instant,
    pub(crate) search_edit: Option<String>,
    bar_rects: Vec<(Rect, usize)>,
    drag: Option<(u64, f32, f32, bool)>,
    pub(crate) last_move: Instant,
    held_nav: Option<(Nav, Instant, Instant)>,
    md5_map: HashMap<String, usize>,
    sha_map: HashMap<String, usize>,
    labels: HashMap<usize, Vec<String>>,
    preview_for: Option<String>,
    preview_rx: Option<Receiver<Option<Arc<Sample>>>>,
    /// The preview's file being read (the web: from the browser's storage)
    /// before it is decoded.
    preview_fetch: Option<(crate::platform::vfs::Prefetch, PathBuf)>,
    banner: Option<(String, Option<TexId>)>,
    pub(crate) ranking: HashMap<String, Rank>,
    quit_armed: Option<Instant>,
    pub(crate) created: Instant,
    pub(crate) skin: Option<LoadedSkin>,
    skin_tried: bool,
    /// The sound set's BGM is playing; the panel its sounds last saw.
    bgm_on: bool,
    sound_panel: usize,
    /// Clickable skin objects from the last frame: (rect, event, click mode).
    skin_clicks: Vec<(Rect, i32, i32)>,
    /// Open skin panel (1..=6) and when panels last opened / closed.
    pub(crate) panel: usize,
    pub(crate) panel_on: [Option<Instant>; 6],
    pub(crate) panel_off: [Option<Instant>; 6],
    /// START / SELECT held: the option panels (see `option_panel`).
    start_held: bool,
    select_held: bool,
    /// Number key 5 held: the detail panel, as START + SELECT.
    num5_held: bool,
    /// A held panel key that repeats: (key, next step, steps so far).
    panel_repeat: Option<(usize, Instant, u32)>,
    /// Lamp / rank counts per folder (bar key), computed when first shown.
    dist_cache: std::cell::RefCell<HashMap<String, Distribution>>,
}

/// Clear lamp counts (by clear type) and DJ rank counts (AAA..F, no play).
pub(crate) type Distribution = ([u32; 11], [u32; 9]);

/// What VS needs of the controls, said when it is chosen.
fn vs_hint(ctx: &mut Ctx) {
    if super::play::vs_touch_too_small(ctx) {
        ctx.toast(super::play::VS_TOUCH_TOO_SMALL.to_string());
        return;
    }
    let pads = ctx.config.keys.p2.keys.iter().any(|k| !k.pad.is_empty());
    if !ctx.touch_enabled() && !pads {
        ctx.toast(
            "VS: 1P and 2P keys on one keyboard; many keyboards miss keys when several are held \
             (rollover)"
                .to_string(),
        );
    }
}

/// The select options (skin texts `9100 + i`, values `9000 + i`).
pub(crate) const OPTION_NAMES: [&str; 11] = [
    "RANDOM",
    "GAUGE",
    "AUTO SHIFT",
    "LN MODE",
    "HI-SPEED −",
    "HI-SPEED +",
    "SIDE",
    "JUDGE",
    "AUTOPLAY",
    "SETTINGS",
    "BATTLE",
];

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// A song's preview: the chart's `#PREVIEW`, else a sound in its folder
/// named `preview…` (as beatoraja finds one).
fn preview_file(song: &Song) -> Option<PathBuf> {
    use crate::platform::vfs::{find_by_stem, read_dir, AUDIO_EXTS};
    let dir = Path::new(&song.path).parent()?;
    if !song.preview.is_empty() {
        if let Some(p) = find_by_stem(dir, &song.preview, AUDIO_EXTS) {
            return Some(p);
        }
    }
    let mut found: Vec<PathBuf> = read_dir(dir)
        .ok()?
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.path)
        .filter(|p| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase());
            let ext = p.extension().map(|s| s.to_string_lossy().to_lowercase());
            stem.is_some_and(|s| s.starts_with("preview"))
                && ext.is_some_and(|e| AUDIO_EXTS.contains(&e.as_str()))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

impl SelectScene {
    pub fn new(ctx: &mut Ctx) -> SelectScene {
        let mut s = SelectScene {
            version: u64::MAX,
            node: Node::Root,
            stack: Vec::new(),
            bars: Vec::new(),
            cursor: 0,
            scroll: 0.0,
            scrolled_at: Instant::now(),
            search_edit: None,
            #[cfg(target_arch = "wasm32")]
            delete_armed: None,
            bar_rects: Vec::new(),
            drag: None,
            last_move: Instant::now(),
            held_nav: None,
            md5_map: HashMap::new(),
            sha_map: HashMap::new(),
            labels: HashMap::new(),
            axis_last: HashMap::new(),
            preview_for: None,
            preview_rx: None,
            preview_fetch: None,
            banner: None,
            ranking: HashMap::new(),
            quit_armed: None,
            created: Instant::now(),
            skin: None,
            skin_tried: false,
            bgm_on: false,
            sound_panel: 0,
            skin_clicks: Vec::new(),
            panel: 0,
            panel_on: [None; 6],
            panel_off: [None; 6],
            start_held: false,
            select_held: false,
            num5_held: false,
            panel_repeat: None,
            dist_cache: Default::default(),
        };
        s.rebuild(ctx);
        s
    }

    fn rebuild(&mut self, ctx: &Ctx) {
        self.version = ctx.library_version;
        self.dist_cache.borrow_mut().clear();
        self.md5_map.clear();
        self.sha_map.clear();
        for (i, s) in ctx.songs.iter().enumerate() {
            if !s.md5.is_empty() {
                self.md5_map.insert(s.md5.clone(), i);
            }
            self.sha_map.insert(s.sha256.clone(), i);
        }
        self.labels.clear();
        for t in &ctx.tables {
            for e in &t.entries {
                if let Some(i) = self.find(&e.md5, &e.sha256) {
                    self.labels.entry(i).or_default().push(t.label(&e.level));
                }
            }
        }
        let key = self.current_key(ctx);
        self.bars = self.listing(ctx, &self.node.clone());
        // keep the cursor on the same item if possible
        self.cursor = key
            .and_then(|k| self.bars.iter().position(|b| self.bar_key(ctx, b) == k))
            .unwrap_or(self.cursor.min(self.bars.len().saturating_sub(1)));
    }

    fn find(&self, md5: &str, sha: &str) -> Option<usize> {
        (!sha.is_empty())
            .then(|| self.sha_map.get(sha))
            .flatten()
            .or_else(|| (!md5.is_empty()).then(|| self.md5_map.get(md5)).flatten())
            .copied()
    }

    fn bar_key(&self, ctx: &Ctx, b: &Bar) -> String {
        match b {
            Bar::Folder { label, .. } => format!("f:{label}"),
            Bar::Song(i) => format!(
                "s:{}",
                ctx.songs.get(*i).map(|s| s.path.as_str()).unwrap_or("")
            ),
            Bar::Missing { title, artist, .. } => format!("m:{title}\u{1f}{artist}"),
            Bar::Course { table, course } => format!("c:{table}:{course}"),
        }
    }

    fn current_key(&self, ctx: &Ctx) -> Option<String> {
        self.bars.get(self.cursor).map(|b| self.bar_key(ctx, b))
    }

    fn build(&self, ctx: &Ctx, node: &Node) -> Vec<Bar> {
        let songs = &ctx.songs;
        let sorted = |mut v: Vec<usize>| -> Vec<Bar> {
            v.sort_by(|a, b| {
                let (x, y) = (&songs[*a], &songs[*b]);
                lower(&x.title)
                    .cmp(&lower(&y.title))
                    .then(x.difficulty.cmp(&y.difficulty))
                    .then(x.level_num().cmp(&y.level_num()))
            });
            v.into_iter().map(Bar::Song).collect()
        };
        match node {
            Node::Close(_) => Vec::new(),
            Node::Root => {
                let mut bars = vec![Bar::Folder {
                    label: "ALL SONGS".into(),
                    sub: String::new(),
                    node: Node::All,
                    count: songs.len(),
                }];
                let mut folders: Vec<(String, usize)> = Vec::new();
                let mut idx: HashMap<&str, usize> = HashMap::new();
                for s in songs.iter() {
                    match idx.get(s.folder.as_str()) {
                        Some(i) => folders[*i].1 += 1,
                        None => {
                            idx.insert(&s.folder, folders.len());
                            folders.push((s.folder.clone(), 1));
                        }
                    }
                }
                folders.sort_by_key(|a| lower(&a.0));
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
                for ev in ctx.archives.events.iter() {
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
                for t in ctx.tables.iter() {
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
                let q = lower(q);
                sorted(
                    (0..songs.len())
                        .filter(|i| {
                            let s = &songs[*i];
                            lower(&s.title).contains(&q)
                                || lower(&s.artist).contains(&q)
                                || lower(&s.genre).contains(&q)
                                || lower(&s.subtitle).contains(&q)
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
                let ev = &ctx.archives.events[e];
                let root = &ctx.paths.root;
                let mut bars = Vec::new();
                for w in &ev.works {
                    // the folder it was downloaded to, else a chart with its
                    // title and artist
                    let mine: Vec<usize> = match ctx.archives.installed_dir(&ev.name, w.num) {
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
                        bars.push(Bar::Missing {
                            title: w.title.clone(),
                            artist: w.artist.clone(),
                            md5: String::new(),
                            org_md5: Vec::new(),
                            more: w.links.iter().skip(1).chain([&w.page]).cloned().collect(),
                            addons: w.addons.clone(),
                            level: String::new(),
                            url: w.links.first().cloned().unwrap_or_else(|| w.page.clone()),
                            url_diff: String::new(),
                            archive: ctx
                                .archives
                                .can_download(e, w.num)
                                .then(|| (url.clone(), w.num)),
                        });
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
                    .map(|e| match self.find(&e.md5, &e.sha256) {
                        Some(i) => Bar::Song(i),
                        None => Bar::Missing {
                            title: e.title.clone(),
                            artist: e.artist.clone(),
                            md5: e.md5.clone(),
                            org_md5: e.org_md5.clone(),
                            more: Vec::new(),
                            addons: Vec::new(),
                            level: table.label(&e.level),
                            url: e.url.clone(),
                            url_diff: e.url_diff.clone(),
                            archive: None,
                        },
                    })
                    .collect()
            }
            Node::Courses(t) => table(ctx, t)
                .map(|tb| {
                    (0..tb.header.courses.len())
                        .map(|c| Bar::Course {
                            table: t.clone(),
                            course: c,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// The bars of `node` as the list shows them: below the root, the
    /// folder's own bar first (choosing it closes the folder).
    fn listing(&self, ctx: &Ctx, node: &Node) -> Vec<Bar> {
        let mut bars = self.build(ctx, node);
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

    fn enter(&mut self, ctx: &mut Ctx, node: Node) {
        let bars = self.listing(ctx, &node);
        if bars.is_empty() {
            ctx.toast("Empty folder");
            return;
        }
        self.stack.push((self.node.clone(), self.cursor));
        ctx.play_sound(Sound::FolderOpen);
        self.node = node;
        self.bars = bars;
        // the folder's own bar (LR2: deciding on it closes the folder)
        self.cursor = 0;
        self.scroll = self.cursor as f32;
        self.moved(ctx);
    }

    fn back(&mut self, ctx: &mut Ctx) -> bool {
        if let Some((n, c)) = self.stack.pop() {
            ctx.play_sound(Sound::FolderClose);
            self.node = n;
            self.bars = self.listing(ctx, &self.node.clone());
            self.cursor = c.min(self.bars.len().saturating_sub(1));
            self.scroll = self.cursor as f32;
            self.moved(ctx);
            true
        } else {
            false
        }
    }

    fn moved(&mut self, ctx: &mut Ctx) {
        self.last_move = Instant::now();
        if self.preview_for.take().is_some() {
            ctx.mixer.stop(PREVIEW_KEY);
        }
        self.preview_rx = None;
        self.preview_fetch = None;
    }

    fn step(&mut self, ctx: &mut Ctx, d: i32) {
        if self.bars.is_empty() {
            return;
        }
        let n = self.bars.len() as i32;
        self.cursor = ((self.cursor as i32 + d).rem_euclid(n)) as usize;
        if d != 0 {
            ctx.play_sound(Sound::Scratch);
        }
        self.moved(ctx);
    }

    /// Web: delete the chosen song's folder kept in the browser, on a
    /// second press of Delete.
    #[cfg(target_arch = "wasm32")]
    fn delete_kept(&mut self, ctx: &mut Ctx) {
        let bms = ctx.paths.root.join("bms");
        let Some(dir) = self.selected_song(ctx).and_then(|(_, s)| {
            let rest = Path::new(&s.path).strip_prefix(&bms).ok()?;
            Some(bms.join(rest.components().next()?.as_os_str()))
        }) else {
            return;
        };
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.delete_armed.as_ref() != Some(&dir) {
            ctx.toast(format!(
                "Press Delete again to delete {name} from this browser"
            ));
            self.delete_armed = Some(dir);
            return;
        }
        self.delete_armed = None;
        crate::platform::vfs::remove_dir(&dir);
        ctx.toast(format!("Deleted {name}"));
    }

    /// Watch the chosen chart's replay: its best play's, else its latest.
    fn watch_replay(&mut self, ctx: &mut Ctx) -> Next {
        let Some((_, song)) = self.selected_song(ctx) else {
            return Next::Stay;
        };
        let song = song.clone();
        let ln = song.score_ln(ctx.config.play.ln_mode) as i32;
        let fixed = song.fixed_ln();
        let load = |slot| {
            crate::replay::Replay::load_filed(&ctx.paths.root, &song.sha256, ln, fixed, slot)
        };
        let Some(r) = load(crate::replay::Slot::Best).or_else(|| load(crate::replay::Slot::Last))
        else {
            ctx.toast(format!("No replay of {}", song.title));
            return Next::Stay;
        };
        ctx.mixer.stop(PREVIEW_KEY);
        self.preview_for = None;
        self.stop_bgm(ctx);
        Next::Push(Box::new(PlayScene::new(
            ctx,
            PlayRequest {
                song,
                autoplay: false,
                course: None,
                replay: Some(std::sync::Arc::new(r)),
                vs: None,
            },
        )))
    }

    fn selected_song<'a>(&self, ctx: &'a Ctx) -> Option<(usize, &'a Song)> {
        match self.bars.get(self.cursor) {
            Some(Bar::Song(i)) => ctx.songs.get(*i).map(|s| (*i, s)),
            _ => None,
        }
    }

    fn decide(&mut self, ctx: &mut Ctx, autoplay: bool) -> Next {
        match self.bars.get(self.cursor).cloned() {
            Some(Bar::Folder {
                node: Node::Close(_),
                ..
            }) => {
                self.back(ctx);
                Next::Stay
            }
            Some(Bar::Folder { node, .. }) => {
                self.enter(ctx, node);
                Next::Stay
            }
            Some(Bar::Song(i)) => {
                let song = ctx.songs[i].clone();
                // G-BATTLE: the opponent's replay, if it can be one
                let slot = match ctx.config.play.versus {
                    Versus::GhostBest => Some(crate::replay::Slot::Best),
                    Versus::GhostLast => Some(crate::replay::Slot::Last),
                    _ => None,
                };
                let vs = match slot.map(|s| super::play::ghost_for(ctx, &song, s).map(|r| (r, s))) {
                    Some(Ok((r, s))) => Some(super::play::Versus::Ghost(r, s)),
                    Some(Err(e)) => {
                        ctx.toast(e);
                        return Next::Stay;
                    }
                    None if ctx.config.play.versus == Versus::Local => {
                        if let Some(why) = super::play::vs_refusal(ctx, &song) {
                            ctx.toast(why);
                            return Next::Stay;
                        }
                        Some(super::play::Versus::Local)
                    }
                    None => None,
                };
                ctx.mixer.stop(PREVIEW_KEY);
                self.preview_for = None;
                self.stop_bgm(ctx);
                let req = PlayRequest {
                    song,
                    autoplay: autoplay || ctx.config.play.autoplay,
                    course: None,
                    replay: None,
                    vs,
                };
                Next::Push(super::decide::start(ctx, req))
            }
            Some(Bar::Missing {
                archive: Some((url, num)),
                title,
                ..
            }) => {
                let root = ctx.paths.root.clone();
                let got = event(ctx, &url).ok_or_else(|| "no such event".to_string());
                let msg = match got.and_then(|e| ctx.archives.download(&root, e, num)) {
                    Ok(m) => m,
                    Err(why) => format!("{title}: {why}"),
                };
                ctx.toast(msg);
                Next::Stay
            }
            Some(Bar::Missing {
                url,
                url_diff,
                title,
                artist,
                md5,
                org_md5,
                more,
                addons,
                ..
            }) => {
                // straight from the links (as the event mirror's works)
                // chosen again while it downloads: cancelled
                if ctx.song_fetch.cancel(&title, &artist) {
                    ctx.toast(format!("{title}: cancelled"));
                    return Next::Stay;
                }
                if ctx.song_fetch.is_busy(&title, &artist) {
                    ctx.toast(format!("{title}: being added to the library"));
                    return Next::Stay;
                }
                let root = ctx.paths.root.clone();
                // its own links, then the others known for the chart
                let e = bmseeker::table::TableEntry {
                    md5: md5.clone(),
                    url: url.clone(),
                    url_diff: url_diff.clone(),
                    ..Default::default()
                };
                let mut song = ctx.links.links.song_links(&e);
                for u in more {
                    if !song.contains(&u) {
                        song.push(u);
                    }
                }
                // its separate chart's links; add-ons each after
                let entry = bmseeker::install::Entry {
                    md5: md5.clone(),
                    want: crate::hosts::Want {
                        title: title.clone(),
                        artist: artist.clone(),
                    },
                    org_md5,
                    song,
                    chart: ctx.links.links.chart_links(&e),
                    ..Default::default()
                };
                // the song is in the library (another of its charts): what
                // to say when nothing can be tried
                let have_song = !md5.is_empty()
                    && ctx.songs.iter().any(|s| {
                        crate::placement::same_name(&s.title, &title)
                            && crate::placement::same_artist(&s.artist, &artist)
                    });
                if ctx.song_fetch.start(&root, entry, addons) {
                    ctx.toast(format!("Downloading {title}… (choose it again to cancel)"));
                    return Next::Stay;
                }
                if have_song {
                    ctx.toast(format!(
                        "{title}: the song is in the library, but this chart has no download link"
                    ));
                    return Next::Stay;
                }
                // the song's page, then the separate chart's (the chart goes
                // into the song's folder)
                let pages: Vec<&str> = [url.as_str(), url_diff.as_str()]
                    .into_iter()
                    .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
                    .collect();
                if crate::app::IN_APP_BROWSER && !pages.is_empty() {
                    ctx.toast(format!("Download {title} in the browser window"));
                    ctx.browse = Some(pages.iter().map(|u| u.to_string()).collect());
                    return Next::Stay;
                }
                let opened = pages
                    .iter()
                    .filter(|u| crate::platform::open_url(u))
                    .count();
                ctx.toast(match (pages.len(), opened) {
                    (0, _) => format!("Not installed: {title} (no download link)"),
                    (_, 0) => format!("Not installed: {title} — {}", pages.join("  ")),
                    (1, _) => format!("Opened the download page of {title}"),
                    _ => format!(
                        "Opened the download pages of {title}: put the chart into the song's folder"
                    ),
                });
                Next::Stay
            }
            Some(Bar::Course { table: url, course }) => {
                let Some(c) = table(ctx, &url)
                    .and_then(|t| t.header.courses.get(course))
                    .cloned()
                else {
                    return Next::Stay;
                };
                let mut songs = Vec::new();
                let n = c.md5.len().max(c.sha256.len());
                for k in 0..n {
                    let md5 = c.md5.get(k).map(String::as_str).unwrap_or("");
                    let sha = c.sha256.get(k).map(String::as_str).unwrap_or("");
                    match self.find(&md5.to_lowercase(), &sha.to_lowercase()) {
                        Some(i) => songs.push(ctx.songs[i].clone()),
                        None => {
                            ctx.toast(format!(
                                "Course {}: chart #{} is not installed",
                                c.name,
                                k + 1
                            ));
                            return Next::Stay;
                        }
                    }
                }
                if songs.is_empty() {
                    return Next::Stay;
                }
                ctx.mixer.stop(PREVIEW_KEY);
                self.stop_bgm(ctx);
                let run = CourseRun::new(c.name.clone(), songs, c.constraint.clone());
                let first = run.songs[0].clone();
                let req = PlayRequest {
                    song: first,
                    autoplay: autoplay || ctx.config.play.autoplay,
                    course: Some(run),
                    replay: None,
                    vs: None,
                };
                Next::Push(super::decide::start(ctx, req))
            }
            None => Next::Stay,
        }
    }

    fn option(&mut self, ctx: &mut Ctx, i: usize) -> Next {
        let bpm = &self
            .selected_song(ctx)
            .map_or(Bpms::flat(150.0), |(_, s)| Bpms::of_song(s));
        let p = &mut ctx.config.play;
        fn cycle<T: Copy + PartialEq>(all: &[T], v: T) -> T {
            let i = all.iter().position(|x| *x == v).unwrap_or(0);
            all[(i + 1) % all.len()]
        }
        match i {
            0 => p.random_1p = cycle(&LaneOption::ALL, p.random_1p),
            1 => p.gauge = cycle(&GaugeType::SELECTABLE, p.gauge),
            2 => p.gauge_auto_shift = cycle(&GaugeAutoShift::ALL, p.gauge_auto_shift),
            3 => p.ln_mode = cycle(&[LnType::Ln, LnType::Cn, LnType::Hcn], p.ln_mode),
            4 => p.add_hispeed(-crate::config::HISPEED_STEP, bpm),
            5 => p.add_hispeed(crate::config::HISPEED_STEP, bpm),
            6 => {
                p.side = match p.side {
                    crate::config::Side::P1 => crate::config::Side::P2,
                    crate::config::Side::P2 => crate::config::Side::P1,
                }
            }
            7 => p.judge_preset = cycle(&JudgePreset::ALL, p.judge_preset),
            8 => p.autoplay = !p.autoplay,
            9 => return Next::Push(Box::new(crate::scene::settings::SettingsScene::new(ctx))),
            10 => {
                // OFF, BATTLE, BATTLE AS, G-BATTLE (MY BEST, LAST), VS
                let all = super::option_panel::battle_choices(true);
                let row: Vec<i64> = all.iter().copied().filter(|i| *i != 1).collect();
                let label = super::option_panel::step_battle(p, &row, 1);
                // (skins other than the default show no such row)
                ctx.toast(format!("BATTLE: {label}"));
                if ctx.config.play.versus == Versus::Local {
                    vs_hint(ctx);
                }
            }
            _ => {}
        }
        ctx.play_sound(Sound::OptionChange);
        ctx.save_config();
        Next::Stay
    }

    /// The value of option `i` (see `OPTION_NAMES`); `bpm` is the
    /// selected chart's main BPM (for its green number).
    pub(crate) fn option_value(ctx: &Ctx, bpm: Option<Bpms>, i: usize) -> Option<String> {
        let p = &ctx.config.play;
        Some(match i {
            0 => p.random_1p.label().into(),
            1 => p.gauge.label().into(),
            2 => p.gauge_auto_shift.label().into(),
            3 => format!("{:?}", p.ln_mode).to_uppercase(),
            4 => format!("{:.2}", p.hispeed_at(&bpm.unwrap_or(Bpms::flat(150.0)))),
            5 => match (bpm, p.hs_fix) {
                (b, fix) if fix != HsFix::Off => {
                    format!("GN {} FIX", p.green(&b.unwrap_or(Bpms::flat(150.0))))
                }
                (Some(b), _) => format!("GN {}", p.green(&b)),
                (None, _) => format!("{:.2}", p.hispeed),
            },
            6 => format!("{:?}", p.side),
            7 => p.judge_preset.label().into(),
            8 => if p.autoplay { "ON" } else { "OFF" }.into(),
            9 => "OPEN".into(),
            10 => match super::option_panel::battle_index(p) {
                2 => "BATTLE",
                3 => "BATTLE AS",
                4 => "G-BEST",
                5 => "G-LAST",
                6 => "VS",
                _ => "OFF",
            }
            .into(),
            _ => return None,
        })
    }

    /// The sound set's BGM while no song preview plays, and the option
    /// panels' open / close sounds.
    fn update_sounds(&mut self, ctx: &mut Ctx) {
        let want = self.preview_for.is_none();
        if want != self.bgm_on {
            self.bgm_on = want;
            if want {
                ctx.play_sound(Sound::Select);
            } else {
                ctx.stop_sound(Sound::Select);
            }
        }
        if self.panel != self.sound_panel {
            ctx.play_sound(if self.panel > 0 {
                Sound::OptionOpen
            } else {
                Sound::OptionClose
            });
            self.sound_panel = self.panel;
        }
    }

    fn stop_bgm(&mut self, ctx: &mut Ctx) {
        ctx.stop_sound(Sound::Select);
        self.bgm_on = true; // until resumed
    }

    fn update_preview(&mut self, ctx: &mut Ctx) {
        if let Some(rx) = &self.preview_rx {
            if let Ok(s) = rx.try_recv() {
                self.preview_rx = None;
                if let Some(s) = s {
                    ctx.mixer.play(
                        &s,
                        PlayOptions {
                            key: PREVIEW_KEY,
                            bus: Bus::Bgm,
                            volume: 0.8,
                            looping: true,
                            ..Default::default()
                        },
                    );
                }
            }
        }
        // its file read: decoded (off the main thread natively)
        if self
            .preview_fetch
            .as_ref()
            .is_some_and(|(f, _)| f.finished())
        {
            let (_, p) = self.preview_fetch.take().expect("checked");
            let rate = ctx.mixer.sample_rate;
            let (tx, rx) = channel();
            self.preview_rx = Some(rx);
            crate::platform::spawn("preview", move || {
                let ext = p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let s = crate::platform::vfs::read(&p)
                    .ok()
                    .and_then(|bytes| vibebms_audio::decode_bytes(bytes, &ext, rate).ok())
                    .map(Arc::new);
                let _ = tx.send(s);
            });
            return;
        }
        if !ctx.config.audio.preview
            || self.preview_for.is_some()
            || self.last_move.elapsed() < Duration::from_millis(450)
        {
            return;
        }
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        self.preview_for = Some(song.path.clone());
        if let Some(p) = preview_file(song) {
            self.preview_fetch = Some((crate::platform::vfs::Prefetch::files(vec![p.clone()]), p));
        }
    }

    fn update_ranking(&mut self, ctx: &mut Ctx) {
        for r in self.ranking.values_mut() {
            if let Rank::Loading(rx) = r {
                if let Ok(res) = rx.try_recv() {
                    *r = match res {
                        Ok(v) => Rank::Done(v),
                        Err(e) => {
                            log::warn!("IR ranking: {e}");
                            Rank::Error
                        }
                    };
                }
            }
        }
        if self.last_move.elapsed() < Duration::from_millis(700) {
            return;
        }
        let Some(client) = vibebms_ir::JsonIr::new(&ctx.config.ir_server) else {
            return;
        };
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        if self.ranking.contains_key(&song.sha256) {
            return;
        }
        let chart = vibebms_ir::ChartRef {
            md5: song.md5.clone(),
            sha256: song.sha256.clone(),
            ln_type: format!("{:?}", ctx.config.play.ln_mode).to_uppercase(),
        };
        let (tx, rx) = channel();
        self.ranking.insert(song.sha256.clone(), Rank::Loading(rx));
        crate::platform::ir_send(client.ranking_request(&chart), move |body| {
            let _ = tx.send(body.and_then(|b| vibebms_ir::JsonIr::parse_ranking(&b)));
        });
    }

    fn update_banner(&mut self, ctx: &Ctx, gfx: &mut Gfx) {
        let want = self.selected_song(ctx).and_then(|(_, s)| {
            let img = if !s.stagefile.is_empty() {
                &s.stagefile
            } else {
                &s.banner
            };
            let dir = Path::new(&s.path).parent().unwrap_or(Path::new(""));
            crate::platform::vfs::inside(dir, img).map(|p| p.to_string_lossy().into_owned())
        });
        let cur = self.banner.as_ref().map(|b| b.0.clone());
        if want == cur {
            return;
        }
        if self.last_move.elapsed() < Duration::from_millis(120) && want.is_some() {
            return; // don't decode while scrolling fast
        }
        if let Some((_, Some(t))) = self.banner.take() {
            gfx.free_texture(t);
        }
        if let Some(p) = want {
            let tex = resolve_image(Path::new(&p)).and_then(|rp| gfx.load_image(&rp, false));
            self.banner = Some((p, tex));
        }
    }

    /// Lamp / rank distribution of the folder at bar `i` (cached).
    pub(crate) fn folder_distribution(&self, ctx: &Ctx, i: usize) -> Option<Distribution> {
        let bar = self.bars.get(i)?;
        let Bar::Folder { node, .. } = bar else {
            return None;
        };
        let node = match node {
            Node::Close(inner) => inner.as_ref(),
            n => n,
        };
        let key = self.bar_key(ctx, bar);
        if let Some(d) = self.dist_cache.borrow().get(&key) {
            return Some(*d);
        }
        let mut lamps = [0u32; 11];
        let mut ranks = [0u32; 9];
        let ln = ctx.config.play.ln_mode;
        let mut count = |bars: &[Bar]| {
            for b in bars {
                if let Bar::Song(k) = b {
                    let Some(song) = ctx.songs.get(*k) else {
                        continue;
                    };
                    match ctx.best(song, ln) {
                        Some(best) => {
                            lamps[(best.clear as usize).min(10)] += 1;
                            let r = super::skinned::rank_index(DjLevel::from_rate(
                                best.ex_score,
                                best.notes * 2,
                            ));
                            ranks[r.clamp(0, 7) as usize] += 1;
                        }
                        None => {
                            lamps[0] += 1;
                            ranks[8] += 1;
                        }
                    }
                }
            }
        };
        let bars = self.build(ctx, node);
        count(&bars);
        // tables: count their level folders too
        for b in &bars {
            if let Bar::Folder { node, .. } = b {
                count(&self.build(ctx, node));
            }
        }
        let d = (lamps, ranks);
        self.dist_cache.borrow_mut().insert(key, d);
        Some(d)
    }

    /// Open / close a skin panel (1..=6; 0 closes the open one).
    pub(crate) fn toggle_panel(&mut self, n: usize) {
        let now = Instant::now();
        if self.panel > 0 {
            self.panel_off[self.panel - 1] = Some(now);
        }
        self.panel = if n == self.panel || n == 0 {
            0
        } else {
            n.min(6)
        };
        if self.panel > 0 {
            self.panel_on[self.panel - 1] = Some(now);
        }
    }

    /// Does the skin have panels (objects shown only with a panel option)?
    fn skin_has_panels(&self) -> bool {
        self.skin
            .as_ref()
            .map(|s| {
                s.skin
                    .objects
                    .iter()
                    .any(|o| o.dst.ops.iter().any(|op| (21..=26).contains(op)))
            })
            .unwrap_or(false)
    }

    /// A clicked skin object (LR2 / beatoraja button ids, or vibeBMS ids).
    fn skin_event(&mut self, ctx: &mut Ctx, act: i32) -> Next {
        use super::select_skin::OPTION_EVENT;
        let p = &mut ctx.config.play;
        let done = match act {
            1..=9 => {
                self.toggle_panel(act as usize);
                true
            }
            43 => {
                let all = &vibebms_core::lane_option::LaneOption::ALL;
                let i = all.iter().position(|o| *o == p.random_2p).unwrap_or(0);
                p.random_2p = all[(i + 1) % all.len()];
                true
            }
            // beatoraja option buttons: as the panel keys that change them
            40 | 54 | 55 | 301..=307 | 75 | 78 => {
                let b = Bpms::flat(150.0);
                let (panel, key) = match act {
                    40 => (1, 2),
                    54 => (1, 3),
                    55 => (1, 4),
                    75 => (3, 2),
                    78 => (3, 1),
                    n => (2, (n - 301) as usize),
                };
                super::option_panel::key(p, panel, key, &b);
                true
            }
            77 => {
                super::option_panel::turn(p, 1, -1, &Bpms::flat(150.0));
                true
            }
            72 => {
                p.bga = !p.bga;
                true
            }
            _ => false,
        };
        if done {
            ctx.play_sound(Sound::OptionChange);
            ctx.save_config();
            return Next::Stay;
        }
        match act {
            15 => self.decide(ctx, false),
            16 => self.decide(ctx, true),
            // beatoraja / LR2: the key config and skin select screens
            13 => {
                self.stop_bgm(ctx);
                Next::Push(super::keyconfig::open(ctx))
            }
            14 => Next::Push(super::skinselect::open(ctx, None)),
            42 => self.option(ctx, 0),
            40 => self.option(ctx, 1),
            308 => self.option(ctx, 3),
            super::select_skin::EVENT_BACK => {
                self.back(ctx);
                Next::Stay
            }
            super::select_skin::EVENT_REPLAY => self.watch_replay(ctx),
            n if (OPTION_EVENT..OPTION_EVENT + 100).contains(&n) => {
                self.option(ctx, (n - OPTION_EVENT) as usize)
            }
            _ => Next::Stay,
        }
    }

    /// START / SELECT pressed or released: open the panel they hold.
    fn hold(&mut self, start: Option<bool>, select: Option<bool>) {
        if let Some(v) = start {
            self.start_held = v;
        }
        if let Some(v) = select {
            self.select_held = v;
        }
        let want = if self.num5_held {
            3
        } else {
            super::option_panel::panel_of(self.start_held, self.select_held)
        };
        if want != self.panel {
            self.toggle_panel(want);
            self.panel_repeat = None;
        }
    }

    fn holding(&self) -> bool {
        self.start_held || self.select_held || self.num5_held
    }

    /// A held panel key let go: stop its repeat.
    fn panel_release(&mut self, ctx: &mut Ctx, k: usize) {
        if self.panel_repeat.is_some_and(|r| r.0 == k) {
            self.panel_repeat = None;
            ctx.save_config();
        }
    }

    /// A key (0..=6) or turntable step (7 up, 8 down) with a panel held.
    fn panel_input(&mut self, ctx: &mut Ctx, k: usize) {
        use super::option_panel;
        let b = self
            .selected_song(ctx)
            .map_or(Bpms::flat(150.0), |(_, s)| Bpms::of_song(s));
        let p = &mut ctx.config.play;
        let said = match k {
            7 | 8 => option_panel::turn(p, self.panel, if k == 7 { 1 } else { -1 }, &b),
            _ => option_panel::key(p, self.panel, k, &b),
        };
        if option_panel::repeats(self.panel, k) {
            // beatoraja: repeat after 300 ms, every 50 ms
            self.panel_repeat = Some((k, Instant::now() + Duration::from_millis(300), 0));
        }
        if let Some(msg) = said {
            ctx.play_sound(Sound::OptionChange);
            // the skin shows the change on its panel; without one, a toast
            if !self.skin_has_panels() {
                ctx.toast(msg);
            }
            ctx.save_config();
        }
    }

    fn handle_nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        match nav {
            Nav::Up => self.step(ctx, -1),
            Nav::Down => self.step(ctx, 1),
            Nav::Decide => return self.decide(ctx, false),
            Nav::Close if self.panel > 0 => self.toggle_panel(0),
            Nav::Close => {
                self.back(ctx);
            }
            Nav::Back if self.panel > 0 => self.toggle_panel(0),
            Nav::Back => {
                if !self.back(ctx) {
                    if let Some(t) = self.quit_armed {
                        if t.elapsed() < Duration::from_secs(2) {
                            return Next::Quit;
                        }
                    }
                    self.quit_armed = Some(Instant::now());
                    ctx.toast("Press back again to quit");
                }
            }
            Nav::Settings => {
                return Next::Push(Box::new(crate::scene::settings::SettingsScene::new(ctx)))
            }
            // skins with panels open the option panel, others cycle RANDOM
            Nav::Option if self.skin_has_panels() => self.toggle_panel(1),
            Nav::Option => return self.option(ctx, 0),
        }
        Next::Stay
    }
}

/// Which panel input a key is: 1P keys 1..7 (0..=6), turntable up (7) or
/// down (8).
fn panel_key(
    keys: &crate::config::KeyConfig,
    has: impl Fn(&crate::config::LaneKeys) -> bool,
) -> Option<usize> {
    let p = &keys.p1;
    if let Some(i) = p.keys.iter().take(7).position(&has) {
        return Some(i);
    }
    if has(&p.scratch_up) {
        return Some(7);
    }
    if has(&p.scratch_down) {
        return Some(8);
    }
    None
}

/// Resolve an image path, trying common extension substitutions.
pub fn resolve_image(p: &Path) -> Option<std::path::PathBuf> {
    let dir = p.parent()?;
    let name = p.file_name()?.to_string_lossy();
    crate::platform::vfs::find_by_stem(dir, &name, crate::platform::vfs::IMAGE_EXTS)
}

impl Scene for SelectScene {
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        if self.version != ctx.library_version {
            self.rebuild(ctx);
        }
        // a newer nightly was found (once)
        if let Some(n) = ctx.updater.take_notice() {
            ctx.toast(n);
        }
        // a held green number key (panel 3): 1 ms steps, 10 ms after 50
        if let Some((k, next, n)) = self.panel_repeat {
            if now >= next && self.holding() {
                let b = self
                    .selected_song(ctx)
                    .map_or(Bpms::flat(150.0), |(_, s)| Bpms::of_song(s));
                let step = if n > 50 { 10.0 } else { 1.0 };
                let msg = super::option_panel::repeat(&mut ctx.config.play, k, step, &b);
                if !self.skin_has_panels() {
                    ctx.toast(msg);
                }
                self.panel_repeat = Some((k, now + Duration::from_millis(50), n + 1));
            }
        }
        // key repeat for held navigation
        if let Some((nav, since, last)) = self.held_nav {
            let held = now.duration_since(since);
            let interval = if held > Duration::from_millis(1500) {
                25
            } else {
                70
            };
            if held > Duration::from_millis(300)
                && now.duration_since(last) > Duration::from_millis(interval)
            {
                self.held_nav = Some((nav, since, now));
                self.step(ctx, if nav == Nav::Up { -1 } else { 1 });
            }
        }
        // the list wraps around: scroll the short way to the cursor, the
        // same share of the way per second at any frame rate
        let n = self.bars.len().max(1) as f32;
        let target = self.cursor as f32;
        let mut diff = (target - self.scroll).rem_euclid(n);
        if diff > n * 0.5 {
            diff -= n;
        }
        let dt = now
            .saturating_duration_since(self.scrolled_at)
            .as_secs_f32();
        self.scrolled_at = now;
        if diff.abs() < 0.01 {
            self.scroll = target;
        } else {
            let k = 1.0 - (-dt / SCROLL_TAU).exp();
            self.scroll = (self.scroll + diff * k).rem_euclid(n);
        }
        self.update_preview(ctx);
        self.update_sounds(ctx);
        self.update_ranking(ctx);
        self.update_banner(ctx, gfx);
        if !self.skin_tried {
            self.skin_tried = true;
            let skin = {
                let st = SelectState { s: self, ctx, now };
                let stat = |id: i32| st.static_option(id);
                skinned::load(ctx, gfx, SkinType::Select, &stat, &st)
            };
            self.skin = skin;
        }
        if let Some(sk) = self.skin.as_mut() {
            sk.load_pending(gfx);
        }
        // events skin scripts asked for
        let events = self.skin.as_ref().map(|s| s.script_events());
        for ev in events.unwrap_or_default() {
            let next = self.skin_event(ctx, ev);
            if !matches!(next, Next::Stay) {
                return next;
            }
        }
        Next::Stay
    }

    fn realtime(&self) -> bool {
        self.skin.is_some()
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        // the library read again since the last frame (an earlier input
        // rescanned): the bars' song indices are of the old list
        if self.version != ctx.library_version {
            self.rebuild(ctx);
        }
        // search box
        if let Some(q) = self.search_edit.as_mut() {
            // on-screen keyboards (their return key ends it) and touches:
            // the box searches, anywhere else closes it
            let done = match ev {
                InputEv::Text(t) if t.contains(['\n', '\r']) => Some(true),
                InputEv::Text(t) => {
                    q.extend(t.chars().filter(|c| !c.is_control()));
                    None
                }
                InputEv::Pointer {
                    phase: crate::input::Phase::Down,
                    x,
                    y,
                    ..
                } => Some(search_box(ctx).contains(*x, *y)),
                _ => None,
            };
            if let Some(search) = done {
                let q = std::mem::take(q);
                self.search_edit = None;
                if search && !q.trim().is_empty() {
                    self.enter(ctx, Node::Search(q.trim().to_string()));
                }
                return Next::Stay;
            }
            if let InputEv::Key {
                code,
                pressed: true,
                text,
                ..
            } = ev
            {
                match code {
                    KeyCode::Escape => self.search_edit = None,
                    KeyCode::Enter | KeyCode::NumpadEnter => {
                        let q = std::mem::take(q);
                        self.search_edit = None;
                        if !q.trim().is_empty() {
                            self.enter(ctx, Node::Search(q.trim().to_string()));
                        }
                    }
                    KeyCode::Backspace => {
                        q.pop();
                    }
                    _ => {
                        if let Some(t) = text {
                            q.extend(t.chars().filter(|c| !c.is_control()));
                        }
                    }
                }
            }
            return Next::Stay;
        }
        match ev {
            InputEv::Key {
                code,
                pressed,
                repeat,
                ..
            } => {
                let keys = &ctx.config.keys;
                let (is_start, is_select) = (
                    keys.start.keys.contains(code),
                    keys.select.keys.contains(code),
                );
                if is_start || is_select {
                    if !*repeat {
                        self.hold(is_start.then_some(*pressed), is_select.then_some(*pressed));
                    }
                    return Next::Stay;
                }
                if *code == KeyCode::Digit5 || *code == KeyCode::Numpad5 {
                    if !*repeat {
                        self.num5_held = *pressed;
                        self.hold(None, None);
                    }
                    return Next::Stay;
                }
                if self.holding() {
                    if let Some(k) = panel_key(keys, |l| l.keys.contains(code)) {
                        if *pressed && !*repeat {
                            self.panel_input(ctx, k);
                        } else if !*pressed {
                            self.panel_release(ctx, k);
                        }
                    }
                    return Next::Stay;
                }
                if !*pressed {
                    if let Some((nav, _, _)) = self.held_nav {
                        if nav_from_key(*code, &ctx.config.keys) == Some(nav) {
                            self.held_nav = None;
                        }
                    }
                    return Next::Stay;
                }
                if *repeat {
                    return Next::Stay; // own repeat handling
                }
                match code {
                    // beatoraja number keys: 3 LN mode, 6 key config (7 skin
                    // select: beatoraja's F12 rescans here)
                    KeyCode::Digit3 | KeyCode::Numpad3 => return self.option(ctx, 3),
                    KeyCode::Digit6 | KeyCode::Numpad6 => return self.skin_event(ctx, 13),
                    KeyCode::Digit7 | KeyCode::Numpad7 => return self.skin_event(ctx, 14),
                    KeyCode::F2 => return self.option(ctx, 0),
                    KeyCode::F3 => return self.option(ctx, 1),
                    KeyCode::F4 => return self.option(ctx, 3),
                    KeyCode::F5 => return self.decide(ctx, true),
                    KeyCode::F6 => return self.option(ctx, 6),
                    KeyCode::F7 => return self.option(ctx, 4),
                    KeyCode::F8 => return self.option(ctx, 5),
                    KeyCode::F9 => return self.option(ctx, 7),
                    KeyCode::F1 => return self.option(ctx, 10),
                    #[cfg(target_arch = "wasm32")]
                    KeyCode::Delete => {
                        self.delete_kept(ctx);
                        return Next::Stay;
                    }
                    KeyCode::KeyP => return self.watch_replay(ctx),
                    KeyCode::F12 => {
                        ctx.start_scan();
                        ctx.refresh_tables(true);
                        return Next::Stay;
                    }
                    KeyCode::Slash | KeyCode::F10 => {
                        self.search_edit = Some(String::new());
                        return Next::Stay;
                    }
                    KeyCode::PageUp => {
                        self.step(ctx, -10);
                        return Next::Stay;
                    }
                    KeyCode::PageDown => {
                        self.step(ctx, 10);
                        return Next::Stay;
                    }
                    _ => {}
                }
                if let Some(nav) = nav_from_key(*code, &ctx.config.keys) {
                    if matches!(nav, Nav::Up | Nav::Down) {
                        self.held_nav = Some((nav, Instant::now(), Instant::now()));
                    }
                    return self.handle_nav(ctx, nav);
                }
            }
            InputEv::Pad { id, pressed, .. } => {
                let keys = &ctx.config.keys;
                let (is_start, is_select) =
                    (keys.start.pad.contains(id), keys.select.pad.contains(id));
                if is_start || is_select {
                    self.hold(is_start.then_some(*pressed), is_select.then_some(*pressed));
                    return Next::Stay;
                }
                if self.holding() {
                    if let Some(k) = panel_key(keys, |l| l.pad.contains(id)) {
                        if *pressed {
                            self.panel_input(ctx, k);
                        } else {
                            self.panel_release(ctx, k);
                        }
                    }
                    return Next::Stay;
                }
                if !*pressed {
                    return Next::Stay;
                }
                if let Some(nav) = nav_from_pad(*id, &ctx.config.keys) {
                    return self.handle_nav(ctx, nav);
                }
            }
            InputEv::Axis { value, id, .. } => {
                // turntable scrolls the list
                let l = &mut self.axis_last;
                let last = *l.entry(*id).or_insert(*value);
                let mut d = value - last;
                if d > 1.0 {
                    d -= 2.0
                } else if d < -1.0 {
                    d += 2.0
                }
                let step = if d.abs() >= 0.06 {
                    l.insert(*id, *value);
                    if d > 0.0 {
                        1
                    } else {
                        -1
                    }
                } else {
                    0
                };
                if step != 0 && ctx.config.keys.any_axis_is_scratch {
                    if self.holding() {
                        self.panel_input(ctx, if step < 0 { 7 } else { 8 });
                    } else {
                        self.step(ctx, step);
                    }
                }
            }
            InputEv::Wheel { dy } => {
                let n = (-dy * 1.0).round() as i32;
                if n != 0 {
                    self.step(ctx, n);
                }
            }
            InputEv::Pointer {
                id, phase, x, y, ..
            } => match phase {
                Phase::Down => {
                    if let Some(&(_, act, _)) = self
                        .skin_clicks
                        .iter()
                        .rev()
                        .find(|(r, _, _)| r.contains(*x, *y))
                    {
                        if self.skin.as_ref().is_some_and(|s| s.click(act)) {
                            return Next::Stay; // a skin script event
                        }
                        return self.skin_event(ctx, act);
                    }
                    self.drag = Some((*id, *y, self.scroll, false));
                }
                Phase::Move => {
                    if let Some((did, sy, ss, moved)) = self.drag.as_mut() {
                        if *did == *id {
                            let bh = 46.0 * ctx.ui();
                            let dy = *y - *sy;
                            if dy.abs() > 8.0 {
                                *moved = true;
                            }
                            if *moved && !self.bars.is_empty() {
                                // the list wraps around, as with the keys
                                let n = self.bars.len() as f32;
                                let s = (*ss - dy / bh).rem_euclid(n);
                                self.scroll = s;
                                let c = (s.round() as usize) % self.bars.len();
                                if c != self.cursor {
                                    self.cursor = c;
                                    self.moved(ctx);
                                }
                            }
                        }
                    }
                }
                Phase::Up => {
                    if let Some((did, _, _, moved)) = self.drag.take() {
                        if did == *id && !moved {
                            if let Some(i) = ui::hit(&self.bar_rects, *x, *y) {
                                if i == self.cursor {
                                    return self.decide(ctx, false);
                                }
                                self.cursor = i;
                                self.moved(ctx);
                            }
                        }
                    }
                }
            },
            InputEv::Dropped(p) if p.is_dir() => {
                let s = p.to_string_lossy().into_owned();
                if !ctx.config.bms_dirs.contains(&s) {
                    ctx.config.bms_dirs.push(s.clone());
                    ctx.save_config();
                    ctx.toast(format!("Added BMS folder: {s}"));
                    ctx.start_scan();
                }
            }
            _ => {}
        }
        Next::Stay
    }

    fn resumed(&mut self, ctx: &mut Ctx) {
        // the BGM again (a screen over this one may have stopped it)
        self.bgm_on = false;
        // scores may have changed
        self.dist_cache.borrow_mut().clear();
        self.rebuild(ctx);
        self.moved(ctx);
    }

    fn exit(&mut self, ctx: &mut Ctx, gfx: &mut Gfx) {
        ctx.mixer.stop(PREVIEW_KEY);
        self.stop_bgm(ctx);
        if let Some(mut s) = self.skin.take() {
            s.free(gfx);
        }
        if let Some((_, Some(t))) = self.banner.take() {
            gfx.free_texture(t);
        }
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        self.draw_skinned(ctx, d, now);
    }
}

impl SelectScene {
    fn draw_skinned(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let view = Rect::new(0.0, 0.0, ctx.width, ctx.height);
        let (bars, clicks) = {
            let st = SelectState { s: self, ctx, now };
            let mut hooks = SelectHooks {
                st: &st,
                banner: self.banner.as_ref().and_then(|b| b.1),
                bars: Vec::new(),
                clicks: Vec::new(),
            };
            let ms = now.saturating_duration_since(self.created).as_millis() as i64;
            if let Some(sk) = self.skin.as_ref() {
                sk.draw(d, view, ms, &st, &mut hooks);
            }
            (hooks.bars, hooks.clicks)
        };
        self.bar_rects = bars;
        self.skin_clicks = clicks;
        // search box (typing is not part of skins)
        if let Some(q) = &self.search_edit {
            ctx.text_input = true;
            let ui = ctx.ui();
            let r = search_box(ctx);
            d.b.rect(r, with_alpha(theme::PANEL, 0.97));
            d.b.rect_outline(r, 2.0, theme::ACCENT);
            d.text_fit(
                &format!("SEARCH: {q}_"),
                r.x + 12.0 * ui,
                r.y + 10.0 * ui,
                20.0 * ui,
                theme::TEXT,
                Align::Left,
                r.w - 24.0 * ui,
            );
        }
    }

    /// The name of a folder, as the path shows it.
    fn node_name(&self, ctx: &Ctx, n: &Node) -> String {
        {
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
                Node::Level(t, l) => table(ctx, t).map(|t| t.label(l)).unwrap_or_default(),
                Node::Search(q) => format!("SEARCH \"{q}\""),
                Node::Archive(url) => event(ctx, url)
                    .map(|e| ctx.archives.events[e].label().to_string())
                    .unwrap_or_default(),
                Node::Close(inner) => self.node_name(ctx, inner),
            }
        }
    }

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

/// The search box: in the upper half, above an on-screen keyboard.
fn search_box(ctx: &Ctx) -> Rect {
    let ui = ctx.ui();
    Rect::new(
        ctx.width * 0.25,
        ctx.height * 0.22,
        ctx.width * 0.5,
        44.0 * ui,
    )
}
