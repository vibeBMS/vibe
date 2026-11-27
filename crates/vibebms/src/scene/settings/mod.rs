//! In-game settings: every option lives here (no external launcher).
//!
//! A top level of eight pages as cards; each page has sections and
//! foldable groups, and long lists open sub-pages (with a breadcrumb).
//! Search (`/`) finds any row, and points to the options that live in the
//! song select instead.
//!
//! Keyboard: arrows move (←→ change a value), Enter opens or acts, Esc
//! goes back a level. Mouse / touch: tap a card, a page, a row (the left
//! or right half of a value lowers or raises it), the search field, BACK.
//! Controller: the turntable moves, white keys decide / raise, blue keys
//! lower / go back, START goes to the pages, SELECT leaves.

mod controls;
mod courses;
mod display;
mod draw;
mod gameplay;
mod library;
mod online;
mod per_play;
mod play_extras;
mod row;
mod search;
mod skins;
#[cfg(desktop)]
mod streaming;
mod system;
#[cfg(target_arch = "wasm32")]
mod web_storage;

use super::keybind::{self, Learn, Learned};
use crate::ctx::Ctx;
use crate::gfx::{Draw, Gfx};
use crate::input::{nav_from_key, nav_from_pad, AxisStepper, InputEv, Nav, Phase};
use crate::platform::{Duration, Instant};
use crate::scene::{Next, Scene};
use crate::skin::SkinType;
use crate::ui::{self, Armed, ListView, TextEvent, TextInput};
use controls::KeySet;
use row::{Act, Folds, Kind, Row, Rows};
use std::cell::RefCell;
use std::collections::HashSet;
use vibebms_core::course::CourseKind;
use vibebms_ir::ProviderId;
use winit::keyboard::KeyCode;

/// How often the rows are built again while nothing is done to them: they
/// show what goes on meanwhile (downloads, scans, update checks).
const REFRESH: Duration = Duration::from_millis(250);
/// The changes the top level lists.
const RECENT: usize = 3;

/// The pages, in menu order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Page {
    Gameplay,
    Controls,
    Display,
    Skins,
    Library,
    Courses,
    Online,
    System,
}

impl Page {
    const ALL: [Page; 8] = [
        Page::Gameplay,
        Page::Controls,
        Page::Display,
        Page::Skins,
        Page::Library,
        Page::Courses,
        Page::Online,
        Page::System,
    ];

    fn name(self) -> &'static str {
        match self {
            Page::Gameplay => tr!("GAMEPLAY"),
            Page::Controls => tr!("CONTROLS"),
            Page::Display => tr!("DISPLAY & AUDIO"),
            Page::Skins => tr!("SKINS"),
            Page::Library => tr!("LIBRARY"),
            Page::Courses => tr!("COURSES"),
            Page::Online => tr!("ONLINE"),
            Page::System => tr!("SYSTEM"),
        }
    }

    /// Its card on the top level: initials, what it covers, a sentence.
    fn about(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Page::Gameplay => (
                "GP",
                tr!("RULES, TIMING"),
                tr!("Judge windows, gauge rules, judge and visual offsets: how every play is judged."),
            ),
            Page::Controls => (
                "KB",
                tr!("KEYS, PADS, TOUCH"),
                tr!("Keyboard and controller bindings, the turntable, the touch layout and its size."),
            ),
            Page::Display => (
                "AV",
                tr!("SCREEN, SOUND"),
                tr!("Full screen, frame rate, UI scale; volumes, song preview, the sound set, latency."),
            ),
            Page::Skins => (
                "SK",
                tr!("LOOK OF EACH SCREEN"),
                tr!("Pick a skin per screen and set its options; install skins from the catalog."),
            ),
            Page::Library => (
                "LB",
                tr!("SONGS, TABLES"),
                tr!("Song folders and rescans, difficulty tables, download sources."),
            ),
            Page::Courses => (
                "CR",
                tr!("YOUR COURSES"),
                tr!("Make and edit courses, import LR2 courses, RANDOM MIX rules."),
            ),
            Page::Online => (
                "IR",
                tr!("IR, RANKINGS, RIVALS"),
                tr!("Accounts on Bokutachi, BMS-IR and vibeBMS IR servers; sending scores; the outbox."),
            ),
            Page::System => (
                "SY",
                if cfg!(desktop) {
                    tr!("PROFILES, UPDATES, STREAMING")
                } else {
                    tr!("PROFILES, UPDATES")
                },
                if cfg!(desktop) {
                    tr!("Player name, profiles (switch, make, delete), updates, your data; Discord and OBS.")
                } else {
                    tr!("Player name, profiles (switch, make, delete), version and updates, your data.")
                },
            ),
        }
    }

    fn rows(self, b: &mut Rows) {
        match self {
            Page::Gameplay => gameplay::rows(b),
            Page::Controls => controls::rows(b),
            Page::Display => display::rows(b),
            Page::Skins => skins::rows(b),
            Page::Library => library::rows(b),
            Page::Courses => courses::rows(b),
            Page::Online => online::rows(b),
            Page::System => system::rows(b),
        }
    }

    /// The live state its card shows.
    fn chips(self, ctx: &Ctx) -> Vec<String> {
        let c = &ctx.config;
        match self {
            Page::Gameplay => vec![
                tr!("{judge} judge", judge = c.play.judge_preset.label()),
                tr!("offset {ms}", ms = row::ms(c.play.judge_offset_ms)),
            ],
            Page::Controls => {
                let k = &c.keys.p1;
                let pad = k.keys.iter().chain([&k.scratch_up, &k.scratch_down]);
                let pad = pad.into_iter().any(|l| !l.pad.is_empty()) || !k.scratch_axis.is_empty();
                let touch = format!("{:?}", c.touch.mode).to_uppercase();
                vec![
                    if pad {
                        tr!("controller")
                    } else {
                        tr!("keyboard")
                    }
                    .into(),
                    tr!("touch {mode}", mode = touch),
                ]
            }
            Page::Display => display::chips(ctx),
            Page::Skins => skins::chips(ctx),
            Page::Library => library::chips(ctx),
            Page::Courses => {
                let st = &ctx.lib.courses;
                let n: usize = [CourseKind::Expert, CourseKind::Nonstop, CourseKind::Class]
                    .into_iter()
                    .map(|k| st.of_kind(k).len())
                    .sum();
                vec![tr!("{n} courses", n = n)]
            }
            Page::Online => online::chips(ctx),
            Page::System => vec![
                (ctx.profiles.registry.name(&ctx.paths.profile)).to_string(),
                crate::update::VERSION.to_string(),
            ],
        }
    }

    fn step(self, d: i32) -> Page {
        crate::scene::cycle(&Page::ALL, self, d)
    }
}

/// The pages long lists open.
#[derive(Clone, Debug, PartialEq)]
enum Sub {
    ExpandJudge,
    Keys(KeySet),
    /// A screen's skin: its options and offsets.
    Skin(SkinType),
    Catalog,
    /// A catalog skin, by id.
    CatalogSkin(String),
    /// A difficulty table, by URL.
    Table(String),
    Course(u64),
    Account(ProviderId),
    Outbox,
    /// A profile, by id.
    Profile(String),
    #[cfg(target_arch = "wasm32")]
    Storage,
}

impl Sub {
    fn title(&self, ctx: &Ctx) -> String {
        match self {
            Sub::ExpandJudge => tr!("EXPANDED JUDGE WINDOWS").into(),
            Sub::Keys(k) => k.title().into(),
            Sub::Skin(t) => skins::screen_name(*t).to_uppercase(),
            Sub::Catalog => tr!("GET SKINS").into(),
            Sub::CatalogSkin(id) => {
                skins::entry(ctx, id).map_or_else(String::new, |e| e.1.name.clone())
            }
            Sub::Table(url) => (ctx.config.tables.iter())
                .find(|t| t.url == *url)
                .map_or_else(String::new, |t| {
                    if t.name.is_empty() {
                        t.url.clone()
                    } else {
                        t.name.clone()
                    }
                }),
            Sub::Course(id) => ctx
                .lib
                .courses
                .get(*id)
                .map_or_else(String::new, |c| c.title.clone()),
            Sub::Account(p) => ctx
                .config
                .ir
                .account(*p)
                .map_or("", |a| a.title())
                .to_string(),
            Sub::Outbox => tr!("OUTBOX").into(),
            Sub::Profile(id) => ctx.profiles.registry.name(id).to_string(),
            #[cfg(target_arch = "wasm32")]
            Sub::Storage => tr!("STORAGE").into(),
        }
    }

    fn rows(&self, b: &mut Rows) {
        match self {
            Sub::ExpandJudge => gameplay::expand_judge(b),
            Sub::Keys(k) => controls::keys(b, *k),
            Sub::Skin(t) => skins::skin(b, *t),
            Sub::Catalog => skins::catalog(b),
            Sub::CatalogSkin(id) => skins::catalog_skin(b, id),
            Sub::Table(url) => library::table(b, url),
            Sub::Course(id) => courses::editor(b, *id),
            Sub::Account(p) => online::account(b, *p),
            Sub::Outbox => online::outbox(b),
            Sub::Profile(id) => system::profile(b, id),
            #[cfg(target_arch = "wasm32")]
            Sub::Storage => web_storage::rows(b),
        }
    }

    /// What it shows is still there (a removed account, table, course or
    /// skin closes its page).
    fn exists(&self, ctx: &Ctx) -> bool {
        match self {
            Sub::CatalogSkin(id) => skins::entry(ctx, id).is_some(),
            Sub::Table(url) => ctx.config.tables.iter().any(|t| t.url == *url),
            Sub::Course(id) => ctx.lib.courses.get(*id).is_some(),
            Sub::Account(p) => ctx.config.ir.account(*p).is_some(),
            Sub::Profile(id) => ctx.profiles.registry.has(id),
            _ => true,
        }
    }
}

/// What is on the screen: the cards, a page, or a sub-page.
#[derive(Clone, Debug, PartialEq)]
enum View {
    Top,
    Page(Page),
    Sub(Sub),
}

impl View {
    fn rows(&self, ctx: &Ctx, armed: &Armed<Confirm>, folds: Folds) -> Vec<Row> {
        let mut b = Rows::new(ctx, armed, folds);
        match self {
            View::Top => {}
            View::Page(p) => p.rows(&mut b),
            View::Sub(s) => s.rows(&mut b),
        }
        b.v
    }

    fn title(&self, ctx: &Ctx) -> String {
        match self {
            View::Top => String::new(),
            View::Page(p) => p.name().into(),
            View::Sub(s) => s.title(ctx),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TextField {
    PlayerName,
    AddFolder,
    AddTable,
    /// Account `.0` of the IR settings: its name, URL, key, player id,
    /// rivals' player ids.
    IrName(usize),
    IrUrl(usize),
    IrKey(usize),
    IrPlayer(usize),
    IrRivals(usize),
    AddIrServer,
    CourseTitle,
    ImportLr2,
    ImportScores,
    TrainerOrder,
    /// A new profile's name.
    NewProfile,
    /// Profile `.0`'s (of the list) name.
    ProfileName(usize),
    /// SYSTEM › STREAMING (desktop).
    #[cfg_attr(not(desktop), allow(dead_code))]
    DiscordAppId,
    #[cfg_attr(not(desktop), allow(dead_code))]
    ObsHost,
    #[cfg_attr(not(desktop), allow(dead_code))]
    ObsPort,
    #[cfg_attr(not(desktop), allow(dead_code))]
    ObsPassword,
    /// The OBS scene of screen `.0` (`ObsScenes::get`).
    #[cfg_attr(not(desktop), allow(dead_code))]
    ObsScene(usize),
}

impl TextField {
    fn title(self) -> &'static str {
        match self {
            TextField::PlayerName => tr!("Player name"),
            TextField::AddFolder => tr!("Add a folder"),
            TextField::AddTable => tr!("Add a table (URL)"),
            TextField::IrName(_) => tr!("IR name"),
            TextField::IrUrl(_) => tr!("IR URL"),
            TextField::IrKey(_) => tr!("API key (paste)"),
            TextField::IrPlayer(_) => tr!("BMS-IR player id (or its page's address)"),
            TextField::IrRivals(_) => tr!("Rivals' BMS-IR player ids"),
            TextField::AddIrServer => tr!("vibeBMS IR server URL"),
            TextField::CourseTitle => tr!("Course title"),
            TextField::ImportLr2 => tr!("LR2's folder (or its song.db)"),
            TextField::ImportScores => tr!("Score database (or its folder)"),
            TextField::TrainerOrder => tr!("TRAINER's lane order (keys from the left, as 3142576)"),
            TextField::NewProfile => tr!("New profile's name"),
            TextField::ProfileName(_) => tr!("Profile name"),
            TextField::DiscordAppId => tr!("Discord application id"),
            TextField::ObsHost => tr!("OBS host"),
            TextField::ObsPort => tr!("OBS WebSocket port"),
            TextField::ObsPassword => tr!("OBS WebSocket password (paste)"),
            TextField::ObsScene(_) => tr!("OBS scene name"),
        }
    }
}

/// The screens rows open.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Screen {
    KeyConfig,
    SkinSelect,
    /// The skin select on the sound sets.
    SoundSets,
    TouchLayout,
}

/// A row pressed once that a second press confirms.
#[derive(Debug, PartialEq)]
enum Confirm {
    /// Install the installed catalog skin with this id again.
    Reinstall(String),
    DeleteSkin(String),
    /// Delete the profile with this id.
    DeleteProfile(String),
    #[cfg(target_arch = "wasm32")]
    DeleteKept(usize),
}

/// A question a row asks before it acts: a dialog over the settings, OK
/// or Cancel.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    /// Turn third parties' copies on (`third_party_mirrors`): asked until
    /// the player has said OK once (`third_party_acknowledged`).
    ThirdParty,
}

impl Ask {
    fn title(self) -> &'static str {
        match self {
            Ask::ThirdParty => tr!("Use third-party sources?"),
        }
    }

    fn text(self) -> &'static str {
        match self {
            Ask::ThirdParty => crate::hosts::third_party_notice(),
        }
    }

    /// OK: what was asked.
    fn accept(self, ctx: &mut Ctx) {
        match self {
            Ask::ThirdParty => {
                ctx.config.third_party_acknowledged = true;
                ctx.config.third_party_mirrors = true;
                ctx.net.hosts = crate::hosts::context(&ctx.config);
            }
        }
    }
}

/// What the player did in settings this session: the groups opened, the
/// last changes (the top level lists them).
#[derive(Default)]
struct Session {
    open: HashSet<&'static str>,
    recent: Vec<String>,
}

thread_local! {
    static SESSION: RefCell<Session> = RefCell::default();
}

/// Search as it is typed: the field, and where each result is.
struct Search {
    input: TextInput,
    /// For each row shown, the views down to it (none: a pointer to the
    /// song select).
    paths: Vec<Vec<View>>,
}

/// What a tap lands on.
#[derive(Clone)]
enum Target {
    Back,
    Home,
    Search,
    Page(Page),
    /// Row `.0`; the middle of its value at `.1` (a tap left of it
    /// lowers the value).
    Row(usize, f32),
    Link(Act),
}

pub struct SettingsScene {
    view: View,
    /// The views above this one, each with its selection and scroll.
    parents: Vec<(View, usize, f32)>,
    rows: Vec<Row>,
    /// When `rows` were built.
    built: Instant,
    /// The rows (scrolled in pixels), or on the top level the cards.
    list: ListView,
    /// On a page, the rail of pages has the focus (else its rows).
    rail: bool,
    search: Option<Search>,
    edit: Option<(TextField, TextInput)>,
    /// A row's question, waiting for OK or Cancel.
    ask: Option<(Ask, ui::Notice)>,
    learn: Learn,
    armed: Armed<Confirm>,
    /// What was drawn where (the last frame).
    hits: ui::HitMap<Target>,
    lay: draw::Layout,
    status: draw::Status,
    /// Cards per line on the top level; cells per line in a grid.
    card_cols: usize,
    grid_cols: usize,
    /// The turntable moves the selection.
    axis: AxisStepper,
    /// Where an act asked to go (another screen, or out of the app).
    then: Option<Next>,
}

impl SettingsScene {
    pub fn new(ctx: &mut Ctx) -> SettingsScene {
        let mut s = SettingsScene {
            view: View::Top,
            parents: Vec::new(),
            rows: Vec::new(),
            built: Instant::now(),
            list: ListView::new(),
            rail: false,
            search: None,
            edit: None,
            ask: None,
            learn: Learn::default(),
            armed: Armed::default(),
            hits: ui::HitMap::default(),
            lay: draw::Layout::default(),
            status: draw::Status::default(),
            card_cols: 4,
            grid_cols: 3,
            axis: AxisStepper::default(),
            then: None,
        };
        s.refresh(ctx);
        s
    }

    /// Opened on `page` (its rows chosen).
    pub fn at(ctx: &mut Ctx, page: Page) -> SettingsScene {
        let mut s = SettingsScene::new(ctx);
        s.open_page(ctx, page);
        s
    }

    fn refresh(&mut self, ctx: &Ctx) {
        self.built = Instant::now();
        self.status = draw::Status::read(ctx);
        if let Some(s) = self.search.as_mut() {
            let (rows, paths) = search::find(ctx, &self.armed, &s.input.buf);
            self.rows = rows;
            s.paths = paths;
            self.list.sel = self.list.sel.min(self.rows.len().saturating_sub(1));
            return;
        }
        if let View::Sub(s) = &self.view {
            if !s.exists(ctx) {
                return self.pop(ctx);
            }
        }
        SESSION.with(|s| {
            self.rows = self
                .view
                .rows(ctx, &self.armed, Folds::Session(&s.borrow().open));
        });
        if self.view != View::Top && !self.rows.get(self.list.sel).is_some_and(Row::focusable) {
            self.list.sel = self.next_focusable(self.list.sel, 1);
        }
    }

    /// The first focusable row from `i` on, in direction `d` (wrapping).
    fn next_focusable(&self, i: usize, d: i32) -> usize {
        let n = self.rows.len();
        (0..n)
            .map(|k| (i as i32 + d * k as i32).rem_euclid(n.max(1) as i32) as usize)
            .find(|&k| self.rows[k].focusable())
            .unwrap_or(0)
    }

    fn show(&mut self, ctx: &Ctx, view: View) {
        self.view = view;
        self.list.sel = 0;
        self.list.scroll = 0.0;
        self.list.follow = true;
        self.armed.clear();
        self.refresh(ctx);
    }

    /// A page from the top level (its rows chosen).
    fn open_page(&mut self, ctx: &Ctx, page: Page) {
        self.search = None;
        self.parents = vec![(
            View::Top,
            Page::ALL.iter().position(|p| *p == page).unwrap_or(0),
            0.0,
        )];
        self.rail = false;
        self.show(ctx, View::Page(page));
    }

    /// Down into `sub`.
    fn push(&mut self, ctx: &mut Ctx, sub: Sub) {
        if let Sub::Course(id) = sub {
            // the course C adds charts to in the song select
            ctx.lib.courses.editing = Some(id);
        }
        let here = std::mem::replace(&mut self.view, View::Top);
        self.parents.push((here, self.list.sel, self.list.scroll));
        self.show(ctx, View::Sub(sub));
    }

    /// Up to the parent, on the row it was left from.
    fn pop(&mut self, ctx: &Ctx) {
        let left = self.page();
        let Some((view, sel, scroll)) = self.parents.pop() else {
            return;
        };
        self.show(ctx, view);
        self.list.scroll = scroll;
        self.list.sel = match self.view {
            // the card of the page left (the rail may have moved)
            View::Top => left.map_or(sel, |p| Page::ALL.iter().position(|q| *q == p).unwrap_or(0)),
            _ => sel.min(self.rows.len().saturating_sub(1)),
        };
    }

    /// The page shown or the one the sub-page is under.
    fn page(&self) -> Option<Page> {
        std::iter::once(&self.view)
            .chain(self.parents.iter().map(|p| &p.0))
            .find_map(|v| match v {
                View::Page(p) => Some(*p),
                _ => None,
            })
    }

    /// Back a level: search closes, a sub-page goes to its parent, the
    /// rows to the rail, the rail to the top level, the top level leaves.
    fn back(&mut self, ctx: &mut Ctx) -> Next {
        if self.search.take().is_some() {
            self.list.sel = 0;
            self.refresh(ctx);
            return Next::Stay;
        }
        match self.view {
            View::Top => return Self::leave(ctx),
            View::Page(_) if !self.rail => self.rail = true,
            _ => self.pop(ctx),
        }
        Next::Stay
    }

    fn leave(ctx: &mut Ctx) -> Next {
        ctx.save_config();
        Next::Pop
    }

    fn row(&self) -> Option<&Row> {
        self.rows.get(self.list.sel)
    }

    fn act(&self) -> Act {
        self.row().map_or(Act::None, |r| r.act)
    }

    /// Fold or unfold group `id`.
    fn fold(&mut self, ctx: &Ctx, id: &'static str) {
        SESSION.with(|s| {
            let open = &mut s.borrow_mut().open;
            if !open.remove(id) {
                open.insert(id);
            }
        });
        self.refresh(ctx);
    }

    /// Choose the selected row: ←→ (`d`), or Enter / a tap (`enter`).
    fn activate(&mut self, ctx: &mut Ctx, d: i32, enter: bool) {
        let Some(row) = self.row() else {
            return;
        };
        if let Kind::Group { id, .. } = row.kind {
            return self.fold(ctx, id);
        }
        if enter && self.search.is_some() {
            return self.jump(ctx);
        }
        let act = row.act;
        if let Some(sub) = row.open.clone() {
            if enter || (d > 0 && !act.adjusts()) {
                self.armed.clear();
                return self.push(ctx, sub);
            }
        }
        if !act.confirms() {
            self.armed.clear();
        }
        let step = if d == 0 { 1 } else { d };
        match act {
            Act::None => return,
            Act::Adjust(f) => f(ctx, step),
            Act::Run(f, i) | Act::Toggle(f, i) => f(ctx, i),
            Act::CycleSkin(t) => skins::cycle_skin(ctx, t, step),
            Act::SkinOption(t, i) => skins::step_skin_option(ctx, t, i, step),
            Act::SkinOffset(t, i, f) => skins::step_skin_offset(ctx, t, i, f, step),
            Act::SkinGet(i) => skins::get(ctx, &mut self.armed, i),
            Act::SkinDelete(i) => skins::delete(ctx, &mut self.armed, i),
            Act::ProfileDelete(i) => system::delete_profile(ctx, &mut self.armed, i),
            #[cfg(target_arch = "wasm32")]
            Act::DeleteKept(i) => web_storage::delete(ctx, &mut self.armed, i),
            Act::Text(tf) if enter || d != 0 => {
                let cur = match tf {
                    TextField::PlayerName => ctx.config.player_name.clone(),
                    TextField::CourseTitle => courses::title(ctx),
                    TextField::TrainerOrder => ctx.config.play.trainer_order.clone(),
                    TextField::NewProfile => String::new(),
                    TextField::ProfileName(i) => system::profile_name(ctx, i),
                    #[cfg(desktop)]
                    TextField::DiscordAppId
                    | TextField::ObsHost
                    | TextField::ObsPort
                    | TextField::ObsPassword
                    | TextField::ObsScene(_) => streaming::current(ctx, tf),
                    tf => online::current(ctx, tf),
                };
                self.edit = Some((tf, TextInput::new(cur)));
            }
            Act::Bind(slot) if enter || d != 0 => self.learn.start(slot),
            Act::Text(_) | Act::Bind(_) => {}
            Act::Open(s) => {
                ctx.save_config();
                use super::skinselect::SkinSelectScene;
                self.then = Some(Next::Push(match s {
                    Screen::KeyConfig => Box::new(super::keyconfig::KeyConfigScene::new()),
                    Screen::SkinSelect => Box::new(SkinSelectScene::new(None)),
                    Screen::SoundSets => Box::new(SkinSelectScene::new(Some(SkinType::SoundSet))),
                    Screen::TouchLayout => {
                        Box::new(super::touch_layout::TouchLayoutScene::new(ctx))
                    }
                }));
            }
            Act::Restart => {
                ctx.net.updater.relaunch_on_exit();
                self.then = Some(Next::Quit);
            }
            Act::Ask(q) => {
                self.ask = Some((q, ui::Notice::default()));
                return;
            }
        }
        ctx.save_config();
        let changed = act
            .adjusts()
            .then(|| self.row().map(|r| (r.id, r.label.clone())));
        self.refresh(ctx);
        if let Some(Some(key)) = changed {
            self.remember(key);
        }
    }

    /// Row (`id`, `label`) changed: the top level lists it.
    fn remember(&self, (id, label): (&'static str, String)) {
        let Some(r) = self.rows.iter().find(|r| r.id == id && r.label == label) else {
            return;
        };
        let line = format!("{} {}", r.label, r.value);
        SESSION.with(|s| {
            let recent = &mut s.borrow_mut().recent;
            recent.retain(|l| !l.starts_with(&r.label));
            recent.insert(0, line);
            recent.truncate(RECENT);
        });
    }

    /// Search's chosen result: its page (and sub-page), the row chosen.
    fn jump(&mut self, ctx: &mut Ctx) {
        let Some(s) = self.search.as_ref() else {
            return;
        };
        let Some(path) = s.paths.get(self.list.sel).cloned() else {
            return;
        };
        let Some(row) = self.rows.get(self.list.sel).cloned() else {
            return;
        };
        let Some(View::Page(page)) = path.first() else {
            return;
        };
        if let Some(g) = row.group {
            SESSION.with(|s| s.borrow_mut().open.insert(g));
        }
        self.open_page(ctx, *page);
        for v in &path[1..] {
            if let View::Sub(sub) = v {
                self.push(ctx, sub.clone());
            }
        }
        if let Some(i) = (self.rows.iter()).position(|r| r.id == row.id && r.label == row.label) {
            self.list.select(i);
        }
    }

    fn open_search(&mut self, ctx: &Ctx) {
        if self.search.is_none() {
            self.search = Some(Search {
                input: TextInput::default(),
                paths: Vec::new(),
            });
            self.list.sel = 0;
            self.list.scroll = 0.0;
            self.refresh(ctx);
        }
    }

    /// Move the selection by `d`: cards, pages or rows (in a grid, a cell
    /// at a time).
    fn step(&mut self, ctx: &Ctx, d: i32) {
        self.list.follow = true;
        match self.view {
            View::Top if self.search.is_none() => {
                self.list.sel =
                    (self.list.sel as i32 + d).rem_euclid(Page::ALL.len() as i32) as usize
            }
            View::Page(p) if self.rail && self.search.is_none() => {
                self.show(ctx, View::Page(p.step(d)));
                self.rail = true;
            }
            _ => {
                let n = self.rows.len().max(1) as i32;
                let from = (self.list.sel as i32 + d).rem_euclid(n) as usize;
                self.list.sel = self.next_focusable(from, d.signum());
            }
        }
    }

    /// The run of grid cells around row `i`: its first row and length.
    fn grid(&self, i: usize) -> Option<(usize, usize)> {
        let cell = |k: usize| matches!(self.rows.get(k).map(|r| &r.kind), Some(Kind::Cell { .. }));
        if !cell(i) {
            return None;
        }
        let start = (0..=i).rev().take_while(|&k| cell(k)).last()?;
        let len = (start..self.rows.len()).take_while(|&k| cell(k)).count();
        Some((start, len))
    }

    /// An arrow key: ←→ `dx`, ↑↓ `dy`. The cards and a grid move in two
    /// directions; on the rail ↑↓ choose the page and → goes to its rows;
    /// on the rows ↑↓ move and ←→ change the value.
    fn arrow(&mut self, ctx: &mut Ctx, dx: i32, dy: i32) {
        self.list.follow = true;
        if self.view == View::Top {
            let cols = self.card_cols as i32;
            let n = Page::ALL.len() as i32;
            let to = self.list.sel as i32 + if dx != 0 { dx } else { dy * cols };
            self.list.sel = to.rem_euclid(n) as usize;
            return;
        }
        if !self.on_rows() {
            match (dx, dy) {
                (0, dy) => self.step(ctx, dy),
                (1, _) => self.rail = false,
                _ => {}
            }
            return;
        }
        if let Some((start, len)) = self.grid(self.list.sel) {
            let (cols, at) = (self.grid_cols as i32, (self.list.sel - start) as i32);
            let to = if dx != 0 { at + dx } else { at + dy * cols };
            if (0..len as i32).contains(&to) {
                self.list.sel = start + to as usize;
            } else if dy != 0 {
                // out of the grid, on to the rows around it
                self.list.sel = if dy < 0 { start } else { start + len - 1 };
                self.step(ctx, dy);
            }
            return;
        }
        match (dx, dy) {
            (0, dy) => self.step(ctx, dy),
            (dx, _) => self.activate(ctx, dx, false),
        }
    }

    /// A controller (or the keys bound to the lanes), as on the music
    /// select: the turntable moves, a white key decides (raises a value),
    /// a blue key lowers a value or goes back, START goes to the pages,
    /// SELECT leaves.
    fn nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        let on_rows = self.on_rows();
        let adjust = on_rows && self.act().adjusts() && !self.in_grid();
        match nav {
            Nav::Up => self.step(ctx, -1),
            Nav::Down => self.step(ctx, 1),
            Nav::Decide if !on_rows => self.enter(ctx),
            Nav::Decide if adjust => self.activate(ctx, 1, false),
            Nav::Decide => self.activate(ctx, 0, true),
            Nav::Close if adjust => self.activate(ctx, -1, false),
            Nav::Option if matches!(self.view, View::Page(_)) => self.rail = true,
            Nav::Close | Nav::Back | Nav::Option => return self.back(ctx),
            Nav::Settings => return Self::leave(ctx),
        }
        Next::Stay
    }

    /// The rows (or search's results) have the focus: not the cards, not
    /// the rail.
    fn on_rows(&self) -> bool {
        self.search.is_some()
            || match self.view {
                View::Top => false,
                View::Page(_) => !self.rail,
                View::Sub(_) => true,
            }
    }

    fn in_grid(&self) -> bool {
        self.grid(self.list.sel).is_some()
    }

    /// Enter off the rows: the card's page, or from the rail its rows.
    fn enter(&mut self, ctx: &Ctx) {
        match self.view {
            View::Top => self.open_page(ctx, Page::ALL[self.list.sel.min(Page::ALL.len() - 1)]),
            _ => self.rail = false,
        }
    }

    fn commit_text(&mut self, ctx: &mut Ctx, tf: TextField, s: String) {
        let s = s.trim().to_string();
        match tf {
            TextField::PlayerName => ctx.config.player_name = s,
            TextField::AddFolder => {
                if !s.is_empty() {
                    if !std::path::Path::new(&s).is_dir() && !ctx.paths.root.join(&s).is_dir() {
                        ctx.toast(tr!("Not a folder: {s}", s = s));
                        return;
                    }
                    ctx.config.bms_dirs.push(s);
                    ctx.start_scan();
                }
            }
            TextField::AddTable => {
                if !s.is_empty() {
                    ctx.config.tables.push(crate::config::TableConfig {
                        name: String::new(),
                        url: s,
                        enabled: true,
                    });
                    ctx.tables.refresh(&ctx.config.tables, false);
                }
            }
            TextField::CourseTitle => courses::rename(ctx, s),
            TextField::NewProfile => system::new_profile(ctx, &s),
            TextField::ProfileName(i) => system::rename_profile(ctx, i, &s),
            TextField::ImportLr2 => courses::import(ctx, &s),
            TextField::ImportScores => library::import_scores(ctx, &s),
            TextField::TrainerOrder => match vibebms_core::lane_option::trainer_order(&s) {
                Some(_) => ctx.config.play.trainer_order = s,
                None => {
                    ctx.toast(tr!(
                        "Not an order of the keys: {s} (each of 1 to n once)",
                        s = s
                    ));
                    return;
                }
            },
            #[cfg(desktop)]
            TextField::DiscordAppId
            | TextField::ObsHost
            | TextField::ObsPort
            | TextField::ObsPassword
            | TextField::ObsScene(_) => {
                if !streaming::commit(ctx, tf, s) {
                    return;
                }
            }
            tf => online::commit(ctx, tf, s),
        }
        ctx.save_config();
        self.refresh(ctx);
    }

    /// An input while a row waits for its key.
    fn learn_input(&mut self, ctx: &mut Ctx, ev: &InputEv) {
        let Some(slot) = self.learn.slot else {
            return;
        };
        // KEYBOARD SUB's page: a second keyboard key only
        let sub = matches!(self.view, View::Sub(Sub::Keys(KeySet::SideSub(_))));
        match self.learn.input(ev) {
            Learned::Waiting | Learned::Cancel => return,
            Learned::Clear if sub => keybind::clear_sub(&mut ctx.config.keys, slot),
            Learned::Clear => keybind::clear(&mut ctx.config.keys, slot, None),
            Learned::Bind(Some(key), _, _) if sub => {
                keybind::assign_sub(&mut ctx.config.keys, slot, key);
                self.list.sel = (self.list.sel + 1).min(self.rows.len().saturating_sub(1));
            }
            Learned::Bind(..) if sub => return,
            Learned::Bind(key, pad, axis) => {
                keybind::assign(&mut ctx.config.keys, slot, key, pad, axis);
                // on to the next row for quick setup
                self.list.sel = (self.list.sel + 1).min(self.rows.len().saturating_sub(1));
            }
        }
        self.learn.stop();
        ctx.save_config();
        self.refresh(ctx);
    }

    /// Keys while search is open: they type, except those that move,
    /// change, jump or close.
    fn search_input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Option<Next> {
        let s = self.search.as_mut()?;
        if let InputEv::Key {
            code,
            pressed: true,
            ..
        } = ev
        {
            match code {
                KeyCode::Escape => return Some(self.back(ctx)),
                KeyCode::ArrowUp => self.step(ctx, -1),
                KeyCode::ArrowDown => self.step(ctx, 1),
                KeyCode::ArrowLeft => self.activate(ctx, -1, false),
                KeyCode::ArrowRight => self.activate(ctx, 1, false),
                KeyCode::Enter | KeyCode::NumpadEnter => self.activate(ctx, 0, true),
                KeyCode::Backspace if s.input.buf.is_empty() => return Some(self.back(ctx)),
                _ => return self.type_search(ctx, ev),
            }
            return Some(Next::Stay);
        }
        match ev {
            InputEv::Text(_) => self.type_search(ctx, ev),
            _ => None,
        }
    }

    fn type_search(&mut self, ctx: &Ctx, ev: &InputEv) -> Option<Next> {
        let s = self.search.as_mut()?;
        let before = s.input.buf.clone();
        if let TextEvent::Cancel = s.input.input(ev) {
            return None;
        }
        if s.input.buf != before {
            self.list.sel = 0;
            self.list.scroll = 0.0;
            self.refresh(ctx);
        }
        Some(Next::Stay)
    }

    fn input_inner(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        if self.learn.on() {
            self.learn_input(ctx, ev);
            return Next::Stay;
        }
        if let Some((q, n)) = self.ask.as_mut() {
            if let Some(ok) = n.input(ev, &ctx.config.keys) {
                let q = *q;
                self.ask = None;
                if ok {
                    q.accept(ctx);
                    ctx.save_config();
                    let changed = self.row().map(|r| (r.id, r.label.clone()));
                    self.refresh(ctx);
                    if let Some(key) = changed {
                        self.remember(key);
                    }
                }
            }
            return Next::Stay;
        }
        if let Some((tf, t)) = self.edit.as_mut() {
            if let InputEv::Dropped(p) = ev {
                t.buf.push_str(&p.to_string_lossy());
                return Next::Stay;
            }
            match t.input(ev) {
                TextEvent::Editing => {}
                TextEvent::Cancel => self.edit = None,
                TextEvent::Commit(s) => {
                    let tf = *tf;
                    self.edit = None;
                    self.commit_text(ctx, tf, s);
                }
            }
            return Next::Stay;
        }
        if let Some(next) = self.search_input(ctx, ev) {
            return next;
        }
        if matches!(ev, InputEv::Key { pressed: true, .. }) {
            self.list.follow = true;
        }
        let on_rows = self.on_rows();
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                text,
                ..
            } => match code {
                _ if *code == KeyCode::Slash || text.as_deref() == Some("/") => {
                    self.open_search(ctx)
                }
                KeyCode::Escape | KeyCode::Backspace => return self.back(ctx),
                KeyCode::F1 | KeyCode::Tab => return Self::leave(ctx),
                KeyCode::ArrowUp => self.arrow(ctx, 0, -1),
                KeyCode::ArrowDown => self.arrow(ctx, 0, 1),
                KeyCode::ArrowLeft => self.arrow(ctx, -1, 0),
                KeyCode::ArrowRight => self.arrow(ctx, 1, 0),
                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space if on_rows => {
                    self.activate(ctx, 0, true)
                }
                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => self.enter(ctx),
                KeyCode::PageUp | KeyCode::PageDown => {
                    if let (Some(p), View::Page(_)) = (self.page(), &self.view) {
                        let d = if *code == KeyCode::PageUp { -1 } else { 1 };
                        self.show(ctx, View::Page(p.step(d)));
                    }
                }
                // keys bound to the lanes (a controller seen as a keyboard)
                other => {
                    if let Some(nav) = nav_from_key(*other, &ctx.config.keys) {
                        return self.nav(ctx, nav);
                    }
                }
            },
            InputEv::Pad {
                id, pressed: true, ..
            } => {
                if let Some(nav) = nav_from_pad(*id, &ctx.config.keys) {
                    return self.nav(ctx, nav);
                }
            }
            InputEv::Axis { id, value, .. } if ctx.config.keys.any_axis_is_scratch => {
                match self.axis.step(*id, *value) {
                    0 => {}
                    d if d > 0 => return self.nav(ctx, Nav::Down),
                    _ => return self.nav(ctx, Nav::Up),
                }
            }
            _ => return self.pointer(ctx, ev),
        }
        Next::Stay
    }

    /// The mouse and touches: a drag scrolls the rows, a tap (lifted where
    /// it went down) acts on what is under it.
    fn pointer(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        match ev {
            InputEv::Wheel { dy } => self.list.wheel(-dy * draw::ROW_H * ctx.ui()),
            InputEv::Pointer {
                id,
                phase: Phase::Move,
                y,
                ..
            } => {
                self.list.drag_to(*id, *y, ui::DRAG_SLOP * ctx.ui());
            }
            InputEv::Pointer {
                id,
                phase: Phase::Up,
                ..
            } => {
                if let Some((x, y)) = self.list.release(*id) {
                    self.tap_row(ctx, x, y);
                }
            }
            InputEv::Pointer {
                id,
                phase: Phase::Down,
                x,
                y,
                ..
            } => match self.hits.at(*x, *y).cloned() {
                Some(Target::Back) => return self.back(ctx),
                Some(Target::Home) => {
                    self.search = None;
                    self.parents.clear();
                    let sel = self
                        .page()
                        .map_or(0, |p| Page::ALL.iter().position(|q| *q == p).unwrap_or(0));
                    self.show(ctx, View::Top);
                    self.list.sel = sel;
                }
                Some(Target::Search) => self.open_search(ctx),
                Some(Target::Page(p)) => self.open_page(ctx, p),
                Some(Target::Link(Act::Run(f, i))) => {
                    f(ctx, i);
                    ctx.save_config();
                    self.refresh(ctx);
                }
                Some(Target::Row(..)) => self.list.press(*id, *x, *y),
                Some(Target::Link(_)) | None => {}
            },
            _ => {}
        }
        Next::Stay
    }

    /// A tap on the rows at (`x`, `y`): the row's action, by where on it.
    fn tap_row(&mut self, ctx: &mut Ctx, x: f32, y: f32) {
        let Some((r, Target::Row(i, mid))) = self.hits.hit(x, y).map(|(r, t)| (r, t.clone()))
        else {
            return;
        };
        self.rail = false;
        self.list.sel = i;
        let Some(row) = self.row() else {
            return;
        };
        let adjusts = row.act.adjusts() && !matches!(row.kind, Kind::Cell { .. });
        let on_value = x > r.x + r.w * 0.55;
        if adjusts && on_value {
            // on the value: its left half lowers it
            self.activate(ctx, if x < mid { -1 } else { 1 }, false);
        } else if adjusts && row.open.is_none() && self.search.is_none() {
            self.activate(ctx, 1, false);
        } else {
            self.activate(ctx, 0, true);
        }
    }
}

impl Scene for SettingsScene {
    fn update(&mut self, ctx: &mut Ctx, _gfx: &mut Gfx, now: Instant) -> Next {
        if now.saturating_duration_since(self.built) >= REFRESH {
            self.refresh(ctx);
        }
        Next::Stay
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        let next = self.input_inner(ctx, ev);
        self.then.take().unwrap_or(next)
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, _now: Instant) {
        self.place(ctx);
        self.draw_all(ctx, d)
    }
}

#[cfg(test)]
mod tests;
