//! Song select: folders, difficulty tables, courses, search, options.

use crate::app::{Ctx, Draw, Next, Scene};
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
use crate::theme;
use crate::ui;
use std::collections::HashMap;
use std::path::Path;
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Root,
    Folder(String),
    All,
    Table(usize),
    Level(usize, String),
    Courses(usize),
    Search(String),
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
        level: String,
        /// Download pages: the song, and the chart when it is a separate
        /// (差分) chart for that song.
        url: String,
        url_diff: String,
    },
    Course {
        table: usize,
        course: usize,
    },
}

pub(crate) enum Rank {
    Loading(Receiver<Result<Vec<IrScore>, String>>),
    Done(Vec<IrScore>),
    Error,
}

pub struct SelectScene {
    version: u64,
    /// Last position of each turntable axis (it scrolls the list).
    axis_last: HashMap<u64, f32>,
    pub(crate) node: Node,
    stack: Vec<(Node, usize)>,
    pub(crate) bars: Vec<Bar>,
    pub(crate) cursor: usize,
    pub(crate) scroll: f32,
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
    banner: Option<(String, Option<TexId>)>,
    pub(crate) ranking: HashMap<String, Rank>,
    quit_armed: Option<Instant>,
    pub(crate) created: Instant,
    pub(crate) skin: Option<LoadedSkin>,
    skin_tried: bool,
    /// Clickable skin objects from the last frame: (rect, event, click mode).
    skin_clicks: Vec<(Rect, i32, i32)>,
    /// Open skin panel (1..=6) and when panels last opened / closed.
    pub(crate) panel: usize,
    pub(crate) panel_on: [Option<Instant>; 6],
    pub(crate) panel_off: [Option<Instant>; 6],
    /// Lamp / rank counts per folder (bar key), computed when first shown.
    dist_cache: std::cell::RefCell<HashMap<String, Distribution>>,
}

/// Clear lamp counts (by clear type) and DJ rank counts (AAA..F, no play).
pub(crate) type Distribution = ([u32; 11], [u32; 9]);

fn lower(s: &str) -> String {
    s.to_lowercase()
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
            search_edit: None,
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
            banner: None,
            ranking: HashMap::new(),
            quit_armed: None,
            created: Instant::now(),
            skin: None,
            skin_tried: false,
            skin_clicks: Vec::new(),
            panel: 0,
            panel_on: [None; 6],
            panel_off: [None; 6],
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
        self.bars = self.build(ctx, &self.node.clone());
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
            Bar::Missing { title, .. } => format!("m:{title}"),
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
                for (i, t) in ctx.tables.iter().enumerate() {
                    bars.push(Bar::Folder {
                        label: t.header.name.clone(),
                        sub: format!("{} levels", t.levels().len()),
                        node: Node::Table(i),
                        count: t.entries.len(),
                    });
                    if !t.header.courses.is_empty() {
                        bars.push(Bar::Folder {
                            label: format!("{} COURSES", t.header.name),
                            sub: String::new(),
                            node: Node::Courses(i),
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
                let Some(table) = ctx.tables.get(*t) else {
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
                            node: Node::Level(*t, l),
                            count: entries.len(),
                        }
                    })
                    .collect()
            }
            Node::Level(t, l) => {
                let Some(table) = ctx.tables.get(*t) else {
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
                            level: table.label(&e.level),
                            url: e.url.clone(),
                            url_diff: e.url_diff.clone(),
                        },
                    })
                    .collect()
            }
            Node::Courses(t) => ctx
                .tables
                .get(*t)
                .map(|tb| {
                    (0..tb.header.courses.len())
                        .map(|c| Bar::Course {
                            table: *t,
                            course: c,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    fn enter(&mut self, ctx: &mut Ctx, node: Node) {
        let bars = self.build(ctx, &node);
        if bars.is_empty() {
            ctx.toast("Empty folder");
            return;
        }
        self.stack.push((self.node.clone(), self.cursor));
        self.node = node;
        self.bars = bars;
        self.cursor = 0;
        self.scroll = 0.0;
        self.moved(ctx);
    }

    fn back(&mut self, ctx: &mut Ctx) -> bool {
        if let Some((n, c)) = self.stack.pop() {
            self.node = n;
            self.bars = self.build(ctx, &self.node.clone());
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
    }

    fn step(&mut self, ctx: &mut Ctx, d: i32) {
        if self.bars.is_empty() {
            return;
        }
        let n = self.bars.len() as i32;
        self.cursor = ((self.cursor as i32 + d).rem_euclid(n)) as usize;
        self.moved(ctx);
    }

    fn selected_song<'a>(&self, ctx: &'a Ctx) -> Option<(usize, &'a Song)> {
        match self.bars.get(self.cursor) {
            Some(Bar::Song(i)) => ctx.songs.get(*i).map(|s| (*i, s)),
            _ => None,
        }
    }

    fn decide(&mut self, ctx: &mut Ctx, autoplay: bool) -> Next {
        match self.bars.get(self.cursor).cloned() {
            Some(Bar::Folder { node, .. }) => {
                self.enter(ctx, node);
                Next::Stay
            }
            Some(Bar::Song(i)) => {
                let song = ctx.songs[i].clone();
                ctx.mixer.stop(PREVIEW_KEY);
                self.preview_for = None;
                Next::Push(Box::new(PlayScene::new(
                    ctx,
                    PlayRequest {
                        song,
                        autoplay: autoplay || ctx.config.play.autoplay,
                        course: None,
                    },
                )))
            }
            Some(Bar::Missing {
                url,
                url_diff,
                title,
                ..
            }) => {
                // the song's page, then the separate chart's (the chart goes
                // into the song's folder)
                let pages: Vec<&str> = [url.as_str(), url_diff.as_str()]
                    .into_iter()
                    .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
                    .collect();
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
            Some(Bar::Course { table, course }) => {
                let Some(c) = ctx
                    .tables
                    .get(table)
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
                let run = CourseRun::new(c.name.clone(), songs, c.constraint.clone());
                let first = run.songs[0].clone();
                Next::Push(Box::new(PlayScene::new(
                    ctx,
                    PlayRequest {
                        song: first,
                        autoplay: autoplay || ctx.config.play.autoplay,
                        course: Some(run),
                    },
                )))
            }
            None => Next::Stay,
        }
    }

    fn option(&mut self, ctx: &mut Ctx, i: usize) -> Next {
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
            4 => p.duration_ms = p.duration_ms.saturating_sub(25).max(100),
            5 => p.duration_ms = (p.duration_ms + 25).min(5000),
            6 => {
                p.side = match p.side {
                    crate::config::Side::P1 => crate::config::Side::P2,
                    crate::config::Side::P2 => crate::config::Side::P1,
                }
            }
            7 => p.judge_preset = cycle(&JudgePreset::ALL, p.judge_preset),
            8 => p.autoplay = !p.autoplay,
            9 => return Next::Push(Box::new(crate::scene::settings::SettingsScene::new(ctx))),
            _ => {}
        }
        ctx.save_config();
        Next::Stay
    }

    pub(crate) fn option_items(ctx: &Ctx) -> Vec<(String, String)> {
        let p = &ctx.config.play;
        vec![
            ("RANDOM".into(), p.random_1p.label().into()),
            ("GAUGE".into(), p.gauge.label().into()),
            ("AUTO SHIFT".into(), p.gauge_auto_shift.label().into()),
            ("LN MODE".into(), format!("{:?}", p.ln_mode).to_uppercase()),
            ("DURATION −".into(), format!("{}", p.duration_ms)),
            ("DURATION +".into(), format!("GN {}", p.duration_ms * 3 / 5)),
            ("SIDE".into(), format!("{:?}", p.side)),
            ("JUDGE".into(), p.judge_preset.label().into()),
            (
                "AUTOPLAY".into(),
                if p.autoplay { "ON" } else { "OFF" }.into(),
            ),
            ("SETTINGS".into(), "OPEN".into()),
        ]
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
        if !ctx.config.audio.preview
            || self.preview_for.is_some()
            || self.last_move.elapsed() < Duration::from_millis(450)
        {
            return;
        }
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        if song.preview.is_empty() {
            return;
        }
        let dir = Path::new(&song.path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let name = song.preview.clone();
        self.preview_for = Some(song.path.clone());
        let rate = ctx.mixer.sample_rate;
        let (tx, rx) = channel();
        self.preview_rx = Some(rx);
        crate::platform::spawn("preview", move || {
            let s =
                crate::platform::vfs::find_by_stem(&dir, &name, crate::platform::vfs::AUDIO_EXTS)
                    .and_then(|p| {
                        let ext = p
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        let bytes = crate::platform::vfs::read(&p).ok()?;
                        vibebms_audio::decode_bytes(bytes, &ext, rate).ok()
                    })
                    .map(Arc::new);
            let _ = tx.send(s);
        });
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
        let key = self.bar_key(ctx, bar);
        if let Some(d) = self.dist_cache.borrow().get(&key) {
            return Some(*d);
        }
        let mut lamps = [0u32; 11];
        let mut ranks = [0u32; 9];
        let ln = ctx.config.play.ln_mode as i32;
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
            54 => {
                use vibebms_core::lane_option::DoubleOption;
                p.dp_option = match p.dp_option {
                    DoubleOption::Off => DoubleOption::Flip,
                    DoubleOption::Flip => DoubleOption::Off,
                };
                true
            }
            55 => {
                use crate::config::HiSpeedMode;
                p.hispeed_mode = match p.hispeed_mode {
                    HiSpeedMode::Floating => HiSpeedMode::Constant,
                    HiSpeedMode::Constant => HiSpeedMode::Floating,
                };
                true
            }
            72 => {
                p.bga = !p.bga;
                true
            }
            _ => false,
        };
        if done {
            ctx.save_config();
            return Next::Stay;
        }
        match act {
            15 => self.decide(ctx, false),
            16 => self.decide(ctx, true),
            13 | 14 => self.option(ctx, 9),
            42 => self.option(ctx, 0),
            40 => self.option(ctx, 1),
            308 => self.option(ctx, 3),
            super::select_skin::EVENT_BACK => {
                self.back(ctx);
                Next::Stay
            }
            n if (OPTION_EVENT..OPTION_EVENT + 100).contains(&n) => {
                self.option(ctx, (n - OPTION_EVENT) as usize)
            }
            _ => Next::Stay,
        }
    }

    fn handle_nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        match nav {
            Nav::Up => self.step(ctx, -1),
            Nav::Down => self.step(ctx, 1),
            Nav::Left => {
                if !self.back(ctx) {
                    self.step(ctx, -10)
                }
            }
            Nav::Right => match self.bars.get(self.cursor) {
                Some(Bar::Folder { .. }) => return self.decide(ctx, false),
                _ => self.step(ctx, 10),
            },
            Nav::Decide => return self.decide(ctx, false),
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
        let target = self.cursor as f32;
        let diff = target - self.scroll;
        if diff.abs() > self.bars.len() as f32 * 0.5 {
            self.scroll = target; // wrapped around
        } else {
            self.scroll += diff * 0.3;
            if diff.abs() < 0.01 {
                self.scroll = target;
            }
        }
        self.update_preview(ctx);
        self.update_ranking(ctx);
        self.update_banner(ctx, gfx);
        if !self.skin_tried {
            self.skin_tried = true;
            let skin = {
                let st = SelectState { s: self, ctx, now };
                let stat = |id: i32| st.static_option(id);
                skinned::load(ctx, gfx, SkinType::Select, &stat)
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
        // search box
        if let Some(q) = self.search_edit.as_mut() {
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
                    KeyCode::F2 => return self.option(ctx, 0),
                    KeyCode::F3 => return self.option(ctx, 1),
                    KeyCode::F4 => return self.option(ctx, 3),
                    KeyCode::F5 => return self.decide(ctx, true),
                    KeyCode::F6 => return self.option(ctx, 6),
                    KeyCode::F7 => return self.option(ctx, 4),
                    KeyCode::F8 => return self.option(ctx, 5),
                    KeyCode::F9 => return self.option(ctx, 7),
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
            InputEv::Pad {
                id, pressed: true, ..
            } => {
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
                    self.step(ctx, step);
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
                                let s = (*ss - dy / bh).clamp(0.0, (self.bars.len() - 1) as f32);
                                self.scroll = s;
                                let c = s.round() as usize;
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
        // scores may have changed
        self.dist_cache.borrow_mut().clear();
        self.rebuild(ctx);
        self.moved(ctx);
    }

    fn exit(&mut self, ctx: &mut Ctx, gfx: &mut Gfx) {
        ctx.mixer.stop(PREVIEW_KEY);
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
            let ui = ctx.ui();
            let r = Rect::new(
                ctx.width * 0.25,
                ctx.height * 0.45,
                ctx.width * 0.5,
                44.0 * ui,
            );
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

    pub(crate) fn path_label(&self, ctx: &Ctx) -> String {
        let name = |n: &Node| -> String {
            match n {
                Node::Root => "ROOT".into(),
                Node::All => "ALL SONGS".into(),
                Node::Folder(f) => Path::new(f)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                Node::Table(t) | Node::Courses(t) => ctx
                    .tables
                    .get(*t)
                    .map(|t| t.header.name.clone())
                    .unwrap_or_default(),
                Node::Level(t, l) => ctx.tables.get(*t).map(|t| t.label(l)).unwrap_or_default(),
                Node::Search(q) => format!("SEARCH \"{q}\""),
            }
        };
        let mut parts: Vec<String> = self.stack.iter().map(|(n, _)| name(n)).collect();
        parts.push(name(&self.node));
        parts.join(" › ")
    }
}
