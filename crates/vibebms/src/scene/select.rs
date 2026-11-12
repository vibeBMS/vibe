//! Song select: folders, difficulty tables, courses, search, options.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Color, Gfx, Rect, TexId};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::library::Song;
use crate::scene::play::{CourseRun, PlayRequest, PlayScene};
use crate::theme::Theme;
use crate::ui;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vibebms_audio::{Bus, PlayOptions, Sample};
use vibebms_core::gauge::{GaugeAutoShift, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::LaneOption;
use vibebms_core::score::ClearType;
use vibebms_core::LnType;
use vibebms_ir::IrScore;
use winit::keyboard::KeyCode;

const PREVIEW_KEY: u64 = u64::MAX - 1;

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Root,
    Folder(String),
    All,
    Table(usize),
    Level(usize, String),
    Courses(usize),
    Search(String),
}

#[derive(Clone, Debug)]
enum Bar {
    Folder {
        label: String,
        sub: String,
        node: Node,
        count: usize,
        color: Color,
    },
    Song(usize),
    Missing {
        title: String,
        artist: String,
        level: String,
        url: String,
    },
    Course {
        table: usize,
        course: usize,
    },
}

enum Rank {
    Loading(Receiver<Result<Vec<IrScore>, String>>),
    Done(Vec<IrScore>),
    Error(String),
}

pub struct SelectScene {
    version: u64,
    node: Node,
    stack: Vec<(Node, usize)>,
    bars: Vec<Bar>,
    cursor: usize,
    scroll: f32,
    search_edit: Option<String>,
    bar_rects: Vec<(Rect, usize)>,
    opt_rects: Vec<(Rect, usize)>,
    drag: Option<(u64, f32, f32, bool)>,
    last_move: Instant,
    held_nav: Option<(Nav, Instant, Instant)>,
    md5_map: HashMap<String, usize>,
    sha_map: HashMap<String, usize>,
    labels: HashMap<usize, Vec<String>>,
    preview_for: Option<String>,
    preview_rx: Option<Receiver<Option<Arc<Sample>>>>,
    banner: Option<(String, Option<TexId>)>,
    ranking: HashMap<String, Rank>,
    quit_armed: Option<Instant>,
}

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
            opt_rects: Vec::new(),
            drag: None,
            last_move: Instant::now(),
            held_nav: None,
            md5_map: HashMap::new(),
            sha_map: HashMap::new(),
            labels: HashMap::new(),
            preview_for: None,
            preview_rx: None,
            banner: None,
            ranking: HashMap::new(),
            quit_armed: None,
        };
        s.rebuild(ctx);
        s
    }

    fn rebuild(&mut self, ctx: &Ctx) {
        self.version = ctx.library_version;
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
                    color: rgba(0x4fc3f7ff),
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
                        color: rgba(0xe0c060ff),
                    });
                }
                for (i, t) in ctx.tables.iter().enumerate() {
                    bars.push(Bar::Folder {
                        label: t.header.name.clone(),
                        sub: format!("{} levels", t.levels().len()),
                        node: Node::Table(i),
                        count: t.entries.len(),
                        color: rgba(0x9be564ff),
                    });
                    if !t.header.courses.is_empty() {
                        bars.push(Bar::Folder {
                            label: format!("{} COURSES", t.header.name),
                            sub: String::new(),
                            node: Node::Courses(i),
                            count: t.header.courses.len(),
                            color: rgba(0xff8a65ff),
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
                            color: rgba(0x9be564ff),
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
            Some(Bar::Missing { url, title, .. }) => {
                if url.is_empty() {
                    ctx.toast(format!("Not installed: {title}"));
                } else {
                    ctx.toast(format!("Not installed: {title} — {url}"));
                }
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

    fn option_items(ctx: &Ctx) -> Vec<(String, String)> {
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
            ("SETTINGS".into(), "⚙".into()),
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
        std::thread::spawn(move || {
            let s = vibebms_audio::resolve_sound_path(&dir, &name)
                .and_then(|p| vibebms_audio::decode_file(&p, rate).ok())
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
                        Err(e) => Rank::Error(e),
                    };
                }
            }
        }
        if self.last_move.elapsed() < Duration::from_millis(700) {
            return;
        }
        let Some(cfg) = ctx.config.ir.iter().find(|c| c.enabled).cloned() else {
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
            title: song.title.clone(),
            artist: song.artist.clone(),
            mode: song.mode.clone(),
            ln_type: format!("{:?}", ctx.config.play.ln_mode).to_uppercase(),
            notes: song.notes as u32,
        };
        let (tx, rx) = channel();
        self.ranking.insert(song.sha256.clone(), Rank::Loading(rx));
        std::thread::spawn(move || {
            let r = match vibebms_ir::create_client(&cfg) {
                Some(c) => c.ranking(&chart).map_err(|e| e.to_string()),
                None => Err("IR disabled".into()),
            };
            let _ = tx.send(r);
        });
    }

    fn update_banner(&mut self, ctx: &Ctx, gfx: &mut Gfx) {
        let want = self.selected_song(ctx).and_then(|(_, s)| {
            let img = if !s.stagefile.is_empty() {
                &s.stagefile
            } else {
                &s.banner
            };
            (!img.is_empty()).then(|| {
                Path::new(&s.path)
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(img)
                    .to_string_lossy()
                    .into_owned()
            })
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
            Nav::Option => return self.option(ctx, 0),
        }
        Next::Stay
    }
}

/// Resolve an image path, trying common extension substitutions.
pub fn resolve_image(p: &Path) -> Option<std::path::PathBuf> {
    if p.is_file() {
        return Some(p.to_path_buf());
    }
    let stem = p.file_stem()?.to_string_lossy().to_lowercase();
    let dir = p.parent()?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|q| {
            q.file_stem()
                .map(|s| s.to_string_lossy().to_lowercase() == stem)
                .unwrap_or(false)
                && q.extension()
                    .map(|e| {
                        matches!(
                            e.to_string_lossy().to_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "bmp" | "gif"
                        )
                    })
                    .unwrap_or(false)
        })
}

fn lamp_name(c: ClearType) -> &'static str {
    c.label()
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
        Next::Stay
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
                thread_local!(static LAST: std::cell::RefCell<HashMap<u64, f32>> = std::cell::RefCell::new(HashMap::new()));
                let step = LAST.with(|l| {
                    let mut l = l.borrow_mut();
                    let last = *l.entry(*id).or_insert(*value);
                    let mut d = value - last;
                    if d > 1.0 {
                        d -= 2.0
                    } else if d < -1.0 {
                        d += 2.0
                    }
                    if d.abs() >= 0.06 {
                        l.insert(*id, *value);
                        if d > 0.0 {
                            1
                        } else {
                            -1
                        }
                    } else {
                        0
                    }
                });
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
                    if let Some(i) = ui::hit(&self.opt_rects, *x, *y) {
                        return self.option(ctx, i);
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
                            } else if self.header_back_hit(ctx, *x, *y) {
                                self.back(ctx);
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
        self.rebuild(ctx);
        self.moved(ctx);
    }

    fn exit(&mut self, ctx: &mut Ctx, gfx: &mut Gfx) {
        ctx.mixer.stop(PREVIEW_KEY);
        if let Some((_, Some(t))) = self.banner.take() {
            gfx.free_texture(t);
        }
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let ui = ctx.ui();
        let th = ctx.theme.clone();
        let (w, h) = (ctx.width, ctx.height);
        if let Some(bg) = ctx.bg_texture {
            d.image_cover(bg, Rect::new(0.0, 0.0, w, h), 0.35);
        }
        let header_h = 44.0 * ui;
        let opt_h = if ctx.portrait() {
            2.0 * 58.0 * ui
        } else {
            62.0 * ui
        };
        // header
        d.b.rect(Rect::new(0.0, 0.0, w, header_h), with_alpha(th.panel, 1.0));
        d.text(
            "vibeBMS",
            14.0 * ui,
            9.0 * ui,
            22.0 * ui,
            th.accent,
            Align::Left,
        );
        let path = self.path_label(ctx);
        d.text_fit(
            &path,
            130.0 * ui,
            12.0 * ui,
            17.0 * ui,
            th.text,
            Align::Left,
            w * 0.5,
        );
        if !self.stack.is_empty() {
            d.text(
                "◀ BACK",
                w * 0.5 + 150.0 * ui,
                12.0 * ui,
                16.0 * ui,
                th.text_dim,
                Align::Left,
            );
        }
        let status = if let Some(s) = &ctx.scan {
            format!(
                "Scanning… {} found, {}/{} parsed",
                s.found.load(Ordering::Relaxed),
                s.parsed.load(Ordering::Relaxed),
                s.to_parse.load(Ordering::Relaxed)
            )
        } else if ctx.table_rx.is_some() {
            "Updating tables…".to_string()
        } else {
            format!("{} charts", ctx.songs.len())
        };
        d.text(
            &status,
            w - 12.0 * ui,
            13.0 * ui,
            15.0 * ui,
            th.text_dim,
            Align::Right,
        );

        let body = Rect::new(0.0, header_h, w, h - header_h - opt_h);
        let (info_r, list_r) = if ctx.portrait() {
            let ih = body.h * 0.36;
            (
                Rect::new(0.0, body.y, w, ih),
                Rect::new(0.0, body.y + ih, w, body.h - ih),
            )
        } else {
            let iw = w * 0.4;
            (
                Rect::new(0.0, body.y, iw, body.h),
                Rect::new(iw, body.y, w - iw, body.h),
            )
        };
        self.draw_info(ctx, d, info_r.inset(10.0 * ui), &th, ui);
        self.draw_list(ctx, d, list_r, &th, ui);

        // options bar
        self.opt_rects.clear();
        let items = Self::option_items(ctx);
        let rows = if ctx.portrait() { 2 } else { 1 };
        let per_row = items.len().div_ceil(rows);
        let bw = w / per_row as f32;
        let bh = opt_h / rows as f32;
        for (i, (cap, val)) in items.iter().enumerate() {
            let (row, col) = (i / per_row, i % per_row);
            let r = Rect::new(
                col as f32 * bw + 3.0,
                h - opt_h + row as f32 * bh + 3.0,
                bw - 6.0,
                bh - 6.0,
            );
            ui::option_button(d, r, cap, val, &th, ui);
            self.opt_rects.push((r, i));
        }
        if let Some(q) = &self.search_edit {
            let r = Rect::new(w * 0.2, h * 0.4, w * 0.6, 56.0 * ui);
            ui::panel(d, r, &th);
            let caret = if (now.elapsed().as_millis() / 500) % 2 == 0 {
                "_"
            } else {
                " "
            };
            d.text(
                &format!("Search: {q}{caret}"),
                r.x + 16.0 * ui,
                r.y + 16.0 * ui,
                22.0 * ui,
                th.text,
                Align::Left,
            );
        }
        if ctx.songs.is_empty() && ctx.scan.is_none() {
            let msg = format!("No BMS found. Put BMS folders next to the executable ({}) or drop a folder onto this window.", ctx.paths.root.display());
            d.text_fit(
                &msg,
                w * 0.5,
                h * 0.5,
                18.0 * ui,
                th.text,
                Align::Center,
                w * 0.9,
            );
        }
    }
}

impl SelectScene {
    fn path_label(&self, ctx: &Ctx) -> String {
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

    fn header_back_hit(&self, ctx: &Ctx, x: f32, y: f32) -> bool {
        let ui = ctx.ui();
        !self.stack.is_empty()
            && y < 44.0 * ui
            && x > ctx.width * 0.5 + 140.0 * ui
            && x < ctx.width * 0.5 + 260.0 * ui
    }

    fn draw_list(&mut self, ctx: &Ctx, d: &mut Draw, r: Rect, th: &Theme, ui: f32) {
        self.bar_rects.clear();
        let bh = 46.0 * ui;
        let center_y = r.y + r.h * 0.42;
        let visible = (r.h / bh).ceil() as i32 + 2;
        d.b.set_clip(Some(r));
        let first = (self.scroll - visible as f32 * 0.5).floor() as i32;
        for k in first..first + visible + 1 {
            if k < 0 || k as usize >= self.bars.len() {
                continue;
            }
            let i = k as usize;
            let y = center_y + (k as f32 - self.scroll) * bh;
            if y + bh < r.y || y > r.bottom() {
                continue;
            }
            let selected = i == self.cursor;
            let indent = if selected { 0.0 } else { 24.0 * ui };
            let br = Rect::new(
                r.x + 12.0 * ui + indent,
                y + 2.0,
                r.w - 24.0 * ui - indent,
                bh - 4.0,
            );
            self.bar_rects.push((br, i));
            let bg = if selected {
                with_alpha(th.accent, 0.3)
            } else {
                with_alpha(th.panel, 0.92)
            };
            d.b.rect(br, bg);
            if selected {
                d.b.rect_outline(br, 2.0, th.accent);
            }
            let ts = 19.0 * ui;
            let ty = br.y + (br.h - ts) * 0.5 - 2.0 * ui;
            match &self.bars[i] {
                Bar::Folder {
                    label,
                    sub,
                    count,
                    color,
                    ..
                } => {
                    d.b.rect(Rect::new(br.x, br.y, 6.0 * ui, br.h), *color);
                    d.text_fit(
                        &format!("▸ {label}"),
                        br.x + 16.0 * ui,
                        ty,
                        ts,
                        th.text,
                        Align::Left,
                        br.w * 0.7,
                    );
                    let info = if matches!(
                        &self.bars[i],
                        Bar::Folder {
                            node: Node::Level(..),
                            ..
                        }
                    ) {
                        sub.clone()
                    } else {
                        format!("{count}")
                    };
                    d.text(
                        &info,
                        br.right() - 12.0 * ui,
                        ty + 2.0 * ui,
                        15.0 * ui,
                        th.text_dim,
                        Align::Right,
                    );
                }
                Bar::Song(si) => {
                    let s = &ctx.songs[*si];
                    let lamp = ctx
                        .best(s, ctx.config.play.ln_mode as i32)
                        .map(|b| b.clear)
                        .unwrap_or(ClearType::NoPlay);
                    d.b.rect(Rect::new(br.x, br.y, 8.0 * ui, br.h), Theme::lamp(lamp));
                    let lv_col = difficulty_color(s.difficulty);
                    let lvr = Rect::new(
                        br.x + 14.0 * ui,
                        br.y + 6.0 * ui,
                        44.0 * ui,
                        br.h - 12.0 * ui,
                    );
                    d.b.rect(lvr, with_alpha(lv_col, 0.25));
                    d.text_fit(
                        &s.level,
                        lvr.x + lvr.w * 0.5,
                        lvr.y + (lvr.h - 16.0 * ui) * 0.5,
                        16.0 * ui,
                        lv_col,
                        Align::Center,
                        lvr.w - 4.0,
                    );
                    let tag = match s.mode.as_str() {
                        "7KEYS" => "",
                        m => m,
                    };
                    let right = if tag.is_empty() {
                        0.0
                    } else {
                        d.measure(tag, 13.0 * ui) + 16.0 * ui
                    };
                    d.text_fit(
                        &s.full_title(),
                        br.x + 66.0 * ui,
                        ty,
                        ts,
                        th.text,
                        Align::Left,
                        br.w - 80.0 * ui - right,
                    );
                    if !tag.is_empty() {
                        d.text(
                            tag,
                            br.right() - 10.0 * ui,
                            ty + 3.0 * ui,
                            13.0 * ui,
                            th.accent,
                            Align::Right,
                        );
                    }
                }
                Bar::Missing { title, level, .. } => {
                    d.b.rect(Rect::new(br.x, br.y, 8.0 * ui, br.h), rgba(0x303030ff));
                    d.text(
                        level,
                        br.x + 18.0 * ui,
                        ty + 2.0 * ui,
                        14.0 * ui,
                        th.text_dim,
                        Align::Left,
                    );
                    d.text_fit(
                        title,
                        br.x + 76.0 * ui,
                        ty,
                        ts,
                        with_alpha(th.text_dim, 0.8),
                        Align::Left,
                        br.w - 90.0 * ui,
                    );
                }
                Bar::Course { table, course } => {
                    let name = ctx
                        .tables
                        .get(*table)
                        .and_then(|t| t.header.courses.get(*course))
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    d.b.rect(Rect::new(br.x, br.y, 8.0 * ui, br.h), rgba(0xff8a65ff));
                    d.text_fit(
                        &format!("◆ {name}"),
                        br.x + 18.0 * ui,
                        ty,
                        ts,
                        th.text,
                        Align::Left,
                        br.w - 30.0 * ui,
                    );
                }
            }
        }
        d.b.set_clip(None);
        if !self.bars.is_empty() {
            // scrollbar
            let frac = self.cursor as f32 / (self.bars.len().max(2) - 1) as f32;
            let sb = Rect::new(
                r.right() - 5.0 * ui,
                r.y + frac * (r.h - 40.0 * ui),
                4.0 * ui,
                40.0 * ui,
            );
            d.b.rect(sb, with_alpha(th.accent, 0.6));
        }
    }

    fn draw_info(&mut self, ctx: &Ctx, d: &mut Draw, r: Rect, th: &Theme, ui: f32) {
        ui::panel(d, r, th);
        let x = r.x + 14.0 * ui;
        let mut y = r.y + 12.0 * ui;
        let maxw = r.w - 28.0 * ui;
        match self.bars.get(self.cursor) {
            Some(Bar::Song(i)) => {
                let s = &ctx.songs[*i];
                let img_h = (r.h * 0.34).min(r.w * 0.45);
                if let Some((_, Some(t))) = &self.banner {
                    d.image_fit(*t, Rect::new(x, y, maxw, img_h), 1.0);
                    y += img_h + 8.0 * ui;
                }
                if !s.genre.is_empty() {
                    d.text_fit(&s.genre, x, y, 14.0 * ui, th.text_dim, Align::Left, maxw);
                    y += 20.0 * ui;
                }
                d.text_fit(&s.title, x, y, 26.0 * ui, th.text, Align::Left, maxw);
                y += 32.0 * ui;
                if !s.subtitle.is_empty() {
                    d.text_fit(&s.subtitle, x, y, 16.0 * ui, th.text, Align::Left, maxw);
                    y += 22.0 * ui;
                }
                let artist = if s.subartist.is_empty() {
                    s.artist.clone()
                } else {
                    format!("{} / {}", s.artist, s.subartist)
                };
                d.text_fit(&artist, x, y, 16.0 * ui, th.text_dim, Align::Left, maxw);
                y += 28.0 * ui;
                let bpm = if (s.max_bpm - s.min_bpm).abs() < 0.01 {
                    format!("{:.0}", s.main_bpm)
                } else {
                    format!("{:.0}-{:.0} ({:.0})", s.min_bpm, s.max_bpm, s.main_bpm)
                };
                let flags: Vec<&str> = [
                    (s.has_ln, "LN"),
                    (s.has_mine, "MINE"),
                    (s.has_random, "RANDOM"),
                    (s.has_stop, "STOP"),
                    (s.has_bga, "BGA"),
                ]
                .iter()
                .filter(|f| f.0)
                .map(|f| f.1)
                .collect();
                let lines = [
                    format!("{}  LEVEL {}  BPM {}", s.mode, s.level, bpm),
                    format!(
                        "NOTES {}  TOTAL {:.0}  {}:{:02}",
                        s.notes,
                        s.total,
                        s.length_ms / 60000,
                        (s.length_ms / 1000) % 60
                    ),
                    flags.join("  "),
                ];
                for l in lines.iter().filter(|l| !l.is_empty()) {
                    d.text_fit(l, x, y, 15.0 * ui, th.text, Align::Left, maxw);
                    y += 21.0 * ui;
                }
                if let Some(labels) = self.labels.get(i) {
                    d.text_fit(
                        &labels.join("  "),
                        x,
                        y,
                        15.0 * ui,
                        rgba(0x9be564ff),
                        Align::Left,
                        maxw,
                    );
                    y += 21.0 * ui;
                }
                y += 6.0 * ui;
                if let Some(b) = ctx.best(s, ctx.config.play.ln_mode as i32) {
                    d.b.rect(Rect::new(x, y, 6.0 * ui, 46.0 * ui), Theme::lamp(b.clear));
                    d.text(
                        lamp_name(b.clear),
                        x + 14.0 * ui,
                        y,
                        17.0 * ui,
                        Theme::lamp(b.clear),
                        Align::Left,
                    );
                    let dj = vibebms_core::score::DjLevel::from_rate(b.ex_score, b.notes * 2);
                    d.text(
                        &format!(
                            "EX {}  {:.2}%  {}   BP {}   PLAY {}",
                            b.ex_score,
                            b.rate() * 100.0,
                            dj.label(),
                            if b.min_bp == u32::MAX { 0 } else { b.min_bp },
                            b.play_count
                        ),
                        x + 14.0 * ui,
                        y + 23.0 * ui,
                        15.0 * ui,
                        th.text,
                        Align::Left,
                    );
                    y += 56.0 * ui;
                } else {
                    d.text("NO PLAY", x, y, 16.0 * ui, th.text_dim, Align::Left);
                    y += 28.0 * ui;
                }
                // IR ranking
                if y < r.bottom() - 40.0 * ui {
                    match self.ranking.get(&s.sha256) {
                        Some(Rank::Done(v)) => {
                            let name = ctx
                                .config
                                .ir
                                .iter()
                                .find(|c| c.enabled)
                                .map(|c| c.name.clone())
                                .unwrap_or_default();
                            d.text(
                                &format!("{name} RANKING ({})", v.len()),
                                x,
                                y,
                                14.0 * ui,
                                th.accent,
                                Align::Left,
                            );
                            y += 20.0 * ui;
                            for e in v
                                .iter()
                                .take(((r.bottom() - y) / (18.0 * ui)).max(0.0) as usize)
                            {
                                d.b.rect(
                                    Rect::new(x, y + 3.0 * ui, 4.0 * ui, 12.0 * ui),
                                    Theme::lamp(e.clear),
                                );
                                d.text_fit(
                                    &format!("{:>3}. {}", e.rank, e.player),
                                    x + 10.0 * ui,
                                    y,
                                    14.0 * ui,
                                    th.text,
                                    Align::Left,
                                    maxw * 0.6,
                                );
                                d.text(
                                    &format!("{}  BP{}", e.ex_score, e.min_bp),
                                    r.right() - 14.0 * ui,
                                    y,
                                    14.0 * ui,
                                    th.text_dim,
                                    Align::Right,
                                );
                                y += 18.0 * ui;
                            }
                        }
                        Some(Rank::Loading(_)) => {
                            d.text("IR: loading…", x, y, 14.0 * ui, th.text_dim, Align::Left);
                        }
                        Some(Rank::Error(e)) => {
                            d.text_fit(
                                &format!("IR: {e}"),
                                x,
                                y,
                                13.0 * ui,
                                th.text_dim,
                                Align::Left,
                                maxw,
                            );
                        }
                        None => {}
                    }
                }
            }
            Some(Bar::Folder {
                label, sub, count, ..
            }) => {
                d.text_fit(label, x, y, 26.0 * ui, th.text, Align::Left, maxw);
                y += 34.0 * ui;
                d.text_fit(sub, x, y, 15.0 * ui, th.text_dim, Align::Left, maxw);
                y += 24.0 * ui;
                d.text(
                    &format!("{count} items"),
                    x,
                    y,
                    16.0 * ui,
                    th.text,
                    Align::Left,
                );
            }
            Some(Bar::Missing {
                title,
                artist,
                level,
                url,
            }) => {
                d.text(level, x, y, 18.0 * ui, rgba(0x9be564ff), Align::Left);
                y += 28.0 * ui;
                d.text_fit(title, x, y, 24.0 * ui, th.text, Align::Left, maxw);
                y += 32.0 * ui;
                d.text_fit(artist, x, y, 16.0 * ui, th.text_dim, Align::Left, maxw);
                y += 30.0 * ui;
                d.text(
                    "NOT INSTALLED",
                    x,
                    y,
                    16.0 * ui,
                    rgba(0xff6b6bff),
                    Align::Left,
                );
                y += 24.0 * ui;
                d.text_fit(url, x, y, 13.0 * ui, th.text_dim, Align::Left, maxw);
            }
            Some(Bar::Course { table, course }) => {
                if let Some(c) = ctx
                    .tables
                    .get(*table)
                    .and_then(|t| t.header.courses.get(*course))
                {
                    d.text_fit(&c.name, x, y, 26.0 * ui, th.text, Align::Left, maxw);
                    y += 36.0 * ui;
                    let n = c.md5.len().max(c.sha256.len());
                    for k in 0..n {
                        let md5 = c.md5.get(k).map(|s| s.to_lowercase()).unwrap_or_default();
                        let sha = c
                            .sha256
                            .get(k)
                            .map(|s| s.to_lowercase())
                            .unwrap_or_default();
                        let (t, col) = match self.find(&md5, &sha) {
                            Some(i) => (ctx.songs[i].full_title(), th.text),
                            None => ("(not installed)".to_string(), rgba(0xff6b6bff)),
                        };
                        d.text_fit(
                            &format!("{}. {t}", k + 1),
                            x,
                            y,
                            16.0 * ui,
                            col,
                            Align::Left,
                            maxw,
                        );
                        y += 22.0 * ui;
                    }
                    if !c.constraint.is_empty() {
                        y += 6.0 * ui;
                        d.text_fit(
                            &c.constraint.join(", "),
                            x,
                            y,
                            14.0 * ui,
                            th.text_dim,
                            Align::Left,
                            maxw,
                        );
                    }
                }
            }
            None => {}
        }
    }
}

fn difficulty_color(d: i32) -> Color {
    rgba(match d {
        1 => 0x66dd66ff,
        2 => 0x55aaffff,
        3 => 0xffcc33ff,
        4 => 0xff5555ff,
        5 => 0xcc66ffff,
        _ => 0xbbbbbbff,
    })
}
