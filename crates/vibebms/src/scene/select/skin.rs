//! Music select values and the bar list for select skins (LR2 / beatoraja ids).
//!
//! Option values and buttons: beatoraja ids where they exist (42 random,
//! 40 gauge, 308 LN mode, 15 play, 16 autoplay, 13 key config), plus
//! vibeBMS ids for its own select options: text `9000 + i` is the value and
//! `9100 + i` the name of option `i`, and a clickable object with event
//! `9000 + i` cycles it (see `SelectOption`); event 9900 leaves the
//! current folder; text 9200 is the selected chart's IR line (the lamp
//! beside the player's, the rank).
//!
//! IR: ranking rows 120-129 (names), 380-389 (EX), 390-399 (lamps), 179
//! the player's rank, 180 the chart's players. The bar list draws the
//! lamp of the player chosen in `IrSettings::beside` as the rival lamp.

use super::options::{self, SelectOption};
use super::preview::Rank;
use super::status::{self, SIBLINGS};
use super::tree::{table, Bar, CourseBar, Node};
use super::{SelectScene, REFERENCE_BPMS};
use crate::app::{Ctx, Draw};
use crate::config::{Bpms, HiSpeedMode, KeyFilter, RandomSelect, SortMode};
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{Rect, TexId};
use crate::library::{BestScore, Song};
use crate::scene::skinvals::*;
use crate::skin::render::{Hooks, LoadedSkin, Xf};
use crate::skin::*;
use std::cell::OnceCell;
use vibebms_core::course::CourseKind;
use vibebms_core::score::{ClearType, DjLevel};
use vibebms_ir::lamps::RankView;
use vibebms_ir::ProviderId;

/// vibeBMS select option ids (text value / name, click event).
pub const OPTION_TEXT: i32 = 9000;
pub const OPTION_NAME: i32 = 9100;
pub const OPTION_EVENT: i32 = 9000;
/// Click event: leave the current folder.
pub const EVENT_BACK: i32 = 9900;
/// vibeBMS: watch the chosen chart's replay (its best play's, else its
/// latest).
pub const EVENT_REPLAY: i32 = 9901;
/// vibeBMS text: the selected chart on the IR ("IR HARD   #12 / 345").
pub const TEXT_IR: i32 = 9200;

/// vibeBMS events: read the chart's text files, open the search box, and
/// choose the song's chart of difficulty slot k (BEGINNER .. INSANE) with
/// `EVENT_SIBLING + k`.
pub const EVENT_READ_TEXT: i32 = 9902;
pub const EVENT_SEARCH: i32 = 9903;
/// vibeBMS event: a panel's body, so a tap on it reaches nothing under it
/// (a tap beside the panel closes it).
pub const EVENT_PANEL: i32 = 9904;
pub const EVENT_SIBLING: i32 = 9910;

/// vibeBMS texts: the chart's key mode ("7KEYS"), the search shown, when
/// the chart was last played, what a play would be kept as ("SAVE",
/// "LAMP ONLY", "NO SAVE"), the lamp a clear would give, the IR's state,
/// the ASSIST options on and the lane cover.
pub const TEXT_MODE: i32 = 9210;
pub const TEXT_QUERY: i32 = 9211;
pub const TEXT_LAST_PLAYED: i32 = 9212;
pub const TEXT_SAVE: i32 = 9213;
pub const TEXT_CLEAR: i32 = 9214;
pub const TEXT_NETWORK: i32 = 9215;
pub const TEXT_ASSIST: i32 = 9216;
pub const TEXT_COVER: i32 = 9217;

/// vibeBMS options: the chart has a replay, text files; a play would be
/// saved / keep its lamp only / be kept not at all (9230-9232); the IR is
/// reached / not (9233, 9234); a clear would give lamp c (9240 + c); the
/// IR clear rate is known.
pub const OPTION_REPLAY: i32 = 9220;
pub const OPTION_HAS_TEXT: i32 = 9221;
/// vibeBMS option: a search's results are shown.
pub const OPTION_SEARCH: i32 = 9222;
/// vibeBMS options: full screen can be switched (desktop); RANDOM 2P
/// takes effect (BATTLE, a double play chart, or no chart chosen).
pub const OPTION_WINDOWED: i32 = 9237;
pub const OPTION_RANDOM_2P: i32 = 9238;

/// vibeBMS texts 9600 + 3t, + 1, + 2: PLAY OPTION's key option t
/// (RANDOM, GAUGE, DP OPTION · BATTLE, HS-FIX, RANDOM 2P, TARGET with the
/// IR rivals) one back, now, one on.
pub const TEXT_KEY_OPTION: i32 = 9600;

/// The events of each panel's tiles, in order: the keys (↑↓, the
/// turntable) move a focus between them, ← → (black / white keys) step
/// the focused one. vibeBMS option `OPTION_FOCUS + k`: tile k has it.
pub const PANEL_TILES: [&[i32]; PANELS] = [
    // PLAY OPTION: RANDOM, GAUGE, DP OPTION, HS-FIX, RANDOM 2P, TARGET;
    // HI-SPEED, LANE COVER, SUDDEN+, HIDDEN+, LIFT, LN MODE, ASSIST, PLAY
    // SIDE, JUDGE RULES
    &[
        42, 40, 54, 55, 43, 77, 9005, 9018, 9032, 9033, 9034, 9003, 2, 9006, 9007,
    ],
    // ASSIST: beatoraja's seven, AUTO SCRATCH
    &[301, 302, 303, 304, 305, 306, 307, 9035],
    // SYSTEM OPTION: BGA, BGA SIZE, GAUGE AUTO SHIFT, JUDGE AUTO ADJUST,
    // JUDGE TIMING, GREEN NUMBER, VISUAL OFFSET, GHOST POSITION, SCORE
    // GRAPH, FAST / SLOW, SCREEN MODE; KEY CONFIG, SKIN / SOUND, SETTINGS
    &[
        9036, 9029, 9002, 9037, 9019, 9020, 9021, 9030, 9031, 9022, 9023, 13, 14, 9009,
    ],
    &[],
    &[],
    // EFFECTS: the four volumes, PREVIEW, SKIN / SOUND
    &[9024, 9025, 9026, 9027, 9028, 14],
    // TAG EDITOR: FAVORITE SONG, FAVORITE CHART, RANDOM SELECT's charts
    &[9015, 9014, 9016],
    &[],
    &[],
];
pub const OPTION_FOCUS: i32 = 9700;
pub const OPTION_SAVE: i32 = 9230;
pub const OPTION_ONLINE: i32 = 9233;
pub const OPTION_OFFLINE: i32 = 9234;
pub const OPTION_CLEAR: i32 = 9240;
pub const OPTION_CLEAR_RATE: i32 = 9251;

/// vibeBMS numbers: the IR clear rate (%), the volumes (master, key,
/// BGM, system; %; also rates 0-1), a folder's charts, installed, played
/// and cleared (9300-9303).
pub const NUMBER_CLEAR_RATE: i32 = 9250;
pub const NUMBER_VOLUME: i32 = 9260;
pub const NUMBER_FOLDER: i32 = 9300;
/// vibeBMS numbers and rates: the selected folder's charts of lamp c
/// (9310 + c, beatoraja order; 9321 FULL COMBO or better) and of DJ level
/// r (9330 + r: AAA .. F, then not played); the rates are of the largest
/// count of their kind (the levels' without the not played).
pub const FOLDER_LAMPS: i32 = 9310;
pub const FOLDER_RANKS: i32 = 9330;

/// vibeBMS option 9480 + t: the selected bar is of type t (`BAR_*`).
pub const OPTION_BAR: i32 = 9480;

/// The song's charts by difficulty slot k (BEGINNER .. INSANE): option
/// `SIBLING + k` there is one, `SIBLING_CURRENT + k` it is the selected
/// chart; number `SIBLING + k` its level, `SIBLING_LAMP + k` its lamp;
/// rate `SIBLING + k` its level of 12.
pub const SIBLING: i32 = 9450;
pub const SIBLING_CURRENT: i32 = 9460;
pub const SIBLING_LAMP: i32 = 9470;

/// Skin panels: LR2's 1-6 (ops 21-26, timers 21-26 opened and 31-36
/// closed), then vibeBMS's 7-9 (ops 9027-9029, timers 9027-9029 and
/// 9037-9039). The default skin's are PLAY OPTION, ASSIST, SYSTEM OPTION,
/// SCORE STATUS, PLAYER STATUS, EFFECTS, TAG EDITOR and HELP.
pub const PANELS: usize = 9;

/// The panel option (and opened timer) `id` shows.
pub fn panel_of_op(id: i32) -> Option<usize> {
    match id {
        21..=26 => Some((id - 20) as usize),
        9027..=9029 => Some((id - 9020) as usize),
        _ => None,
    }
}

/// The panel closed timer `id` counts from.
fn panel_of_closed_timer(id: i32) -> Option<usize> {
    match id {
        31..=36 => Some((id - 30) as usize),
        9037..=9039 => Some((id - 9030) as usize),
        _ => None,
    }
}

/// The select screen's values in one frame: what the selected bar is
/// looked up once.
pub struct SelectState<'a> {
    pub s: &'a SelectScene,
    pub ctx: &'a Ctx,
    bar: Option<&'a Bar>,
    /// The selected chart, its record and facts.
    song: Option<&'a Song>,
    best: Option<&'a BestScore>,
    chart: Option<ChartFacts<'a>>,
    /// Its BPMs, else 150 (beatoraja shows the hi-speed on folders too).
    bpms: Bpms,
    rank: Option<&'a Rank>,
    /// The song's charts by difficulty (`SelectScene::siblings`).
    siblings: [Option<&'a Song>; SIBLINGS],
    /// Whose IR lamps show beside the player's.
    beside: Option<(ProviderId, &'a str)>,
    table: OnceCell<Option<(String, String)>>,
    clock: Clock,
}

/// What a bar shows.
pub struct BarInfo {
    /// Its `BAR_*` type, and the beatoraja one (0-7) for skins without a
    /// body for vibeBMS's types.
    pub kind: usize,
    pub base: usize,
    pub title: String,
    pub level: Option<i64>,
    pub difficulty: usize,
    pub lamp: Option<ClearType>,
    /// The lamp of the player chosen beside (a song's, while one is).
    pub rival: Option<ClearType>,
    /// A folder's charts, a course's stages.
    pub count: Option<i64>,
}

/// A course's bar type.
fn course_kind(k: CourseKind) -> usize {
    match k {
        CourseKind::Expert => BAR_GRADE,
        CourseKind::Nonstop => BAR_NONSTOP,
        CourseKind::Class => BAR_CLASS,
    }
}

/// A folder's bar type, and the beatoraja one.
fn folder_kind(node: &Node) -> (usize, usize) {
    match node {
        Node::Table(_) | Node::Level(..) => (BAR_TABLE, BAR_TABLE),
        Node::Search(_) => (BAR_SEARCH, BAR_SEARCH),
        Node::Archive(_) => (BAR_EVENT, BAR_FOLDER),
        Node::Favorites => (BAR_FAVORITE, BAR_FOLDER),
        Node::Smart(_) => (BAR_VIRTUAL, BAR_FOLDER),
        Node::MyCourses(k) => (course_kind(*k), BAR_FOLDER),
        Node::Courses(_) => (BAR_CLASS, BAR_FOLDER),
        Node::Close(inner) => folder_kind(inner),
        Node::Root | Node::Folder(_) | Node::All => (BAR_FOLDER, BAR_FOLDER),
    }
}

impl<'a> SelectState<'a> {
    pub fn new(s: &'a SelectScene, ctx: &'a Ctx) -> SelectState<'a> {
        let bar = s.bars.get(s.cursor);
        let song = match bar {
            Some(Bar::Song(i)) => ctx.lib.songs.get(*i),
            _ => None,
        };
        let siblings = match bar {
            Some(Bar::Song(i)) => s.siblings(ctx, *i).map(|k| k.map(|k| &ctx.lib.songs[k])),
            _ => [None; SIBLINGS],
        };
        SelectState {
            siblings,
            s,
            ctx,
            bar,
            song,
            best: song.and_then(|s| ctx.best(s, ctx.config.play.ln_mode)),
            chart: song.map(ChartFacts::of_song),
            bpms: song.map_or(REFERENCE_BPMS, Bpms::of_song),
            rank: song.and_then(|x| s.ranking.get(&x.sha256)),
            beside: ctx.net.ir.beside(&ctx.config.ir),
            table: OnceCell::new(),
            clock: Clock::now(),
        }
    }

    /// IR ranking of the selected chart, once loaded.
    fn ranking(&self) -> Option<&[vibebms_ir::lamps::RankEntry]> {
        self.rank_view().map(|v| v.entries.as_slice())
    }

    fn rank_view(&self) -> Option<&RankView> {
        match self.rank? {
            Rank::Done(v) => Some(v),
            _ => None,
        }
    }

    /// The player's place in the selected chart's ranking.
    fn my_rank(&self) -> Option<u32> {
        let me = &self.ctx.config.ir.primary()?.user_id;
        let mine = self
            .ranking()?
            .iter()
            .find(|e| !me.is_empty() && e.player_id == *me);
        mine.map(|e| e.rank)
    }

    /// The chart's players in the ranking (the rows when the server does
    /// not say).
    fn players(&self) -> Option<u32> {
        let v = self.rank_view()?;
        Some(if v.players > 0 {
            v.players
        } else {
            v.entries.len() as u32
        })
    }

    /// The lamp of the player chosen beside on a song.
    fn rival_lamp(&self, s: &Song) -> Option<ClearType> {
        let (p, player) = self.beside?;
        let lamp = self.ctx.net.ir.lamp(p, player, &s.md5, &s.sha256);
        Some(lamp.map_or(ClearType::NoPlay, |l| l.clear))
    }

    /// The selected chart on the IR: the lamp beside the player's (whose,
    /// unless the player's own), the player's rank of the chart's players.
    fn ir_line(&self) -> Option<String> {
        let song = self.song?;
        let mut parts = Vec::new();
        if let (Some(lamp), Some((p, player))) = (self.rival_lamp(song), self.beside) {
            let ir = &self.ctx.config.ir;
            let mine = ir.account(p).is_some_and(|a| a.user_id == player);
            let who = match self.ctx.net.ir.player_name(p, player) {
                Some(name) if !mine => name,
                _ => "IR",
            };
            parts.push(format!("{who} {}", lamp.label()));
        }
        if let Some(n) = self.players() {
            let rank = self.my_rank().map_or("—".into(), |r| r.to_string());
            parts.push(format!("#{rank} / {n}"));
        }
        (!parts.is_empty()).then(|| parts.join("   "))
    }

    /// The selected folder's charts of lamp / DJ level `id`
    /// (`FOLDER_LAMPS`, `FOLDER_RANKS`), and the largest count of its kind.
    fn folder_count(&self, id: i32) -> Option<(u32, u32)> {
        let (lamps, ranks) = self.s.folder_distribution(self.ctx, self.s.cursor)?;
        let (set, k): (&[u32], i32) = match id >= FOLDER_RANKS {
            true => (&ranks, id - FOLDER_RANKS),
            false => (&lamps, id - FOLDER_LAMPS),
        };
        let n = match k {
            11 if id < FOLDER_RANKS => set[ClearType::FullCombo as usize..].iter().sum(),
            k => *set.get(k as usize)?,
        };
        let played = if id >= FOLDER_RANKS { &set[..8] } else { set };
        Some((n, played.iter().copied().max().unwrap_or(0)))
    }

    /// Volume `k`: master, key sounds, BGM, system.
    fn volume(&self, k: i32) -> Option<f32> {
        let a = &self.ctx.config.audio;
        [a.master, a.key, a.bgm, a.system].get(k as usize).copied()
    }

    /// The table that lists the selected chart.
    fn table(&self) -> Option<&(String, String)> {
        self.table
            .get_or_init(|| {
                let s = self.song?;
                table_of(self.ctx, &s.md5, &s.sha256)
            })
            .as_ref()
    }

    fn sibling(&self, id: i32, base: i32) -> Option<&'a Song> {
        *self.siblings.get(usize::try_from(id - base).ok()?)?
    }

    /// A chart's lamp under the LN and rule modes chosen.
    fn lamp_of(&self, s: &Song) -> ClearType {
        let best = self.ctx.best(s, self.ctx.config.play.ln_mode);
        best.map_or(ClearType::NoPlay, |b| b.clear)
    }

    /// The vibeBMS options of the selected bar's state.
    fn vibe_option(&self, id: i32) -> Option<bool> {
        let ctx = self.ctx;
        let files = || self.song.map(|s| self.s.chart_files(ctx, s));
        Some(match id {
            OPTION_REPLAY => files().is_some_and(|f| f.replay),
            OPTION_HAS_TEXT => files().is_some_and(|f| !f.texts.is_empty()),
            n @ OPTION_SAVE..=9232 => {
                status::outcome(&ctx.config.play, self.song).0 as i32 == n - OPTION_SAVE
            }
            OPTION_ONLINE | OPTION_OFFLINE => status::network(ctx).1 == Some(id == OPTION_ONLINE),
            n @ OPTION_CLEAR..=9250 => status::outcome(&ctx.config.play, self.song)
                .1
                .is_some_and(|c| c as i32 == n - OPTION_CLEAR),
            OPTION_CLEAR_RATE => self.rank_view().and_then(status::clear_rate).is_some(),
            OPTION_SEARCH => self.s.query().is_some(),
            OPTION_WINDOWED => super::options::WINDOWED,
            OPTION_RANDOM_2P => {
                ctx.config.play.battle || self.song.is_none_or(|s| s.mode.is_double())
            }
            n @ OPTION_BAR..=9492 => self
                .bar(self.s.cursor)
                .is_some_and(|b| b.kind as i32 == n - OPTION_BAR),
            n @ SIBLING..=9454 => self.sibling(n, SIBLING).is_some(),
            n @ SIBLING_CURRENT..=9464 => (self.sibling(n, SIBLING_CURRENT).zip(self.song))
                .is_some_and(|(a, b)| std::ptr::eq(a, b)),
            _ => return None,
        })
    }

    pub fn bar(&self, i: usize) -> Option<BarInfo> {
        let ctx = self.ctx;
        let plain = |kind, base, title| BarInfo {
            kind,
            base,
            title,
            level: None,
            difficulty: 0,
            lamp: None,
            rival: None,
            count: None,
        };
        Some(match self.s.bars.get(i)? {
            Bar::Folder {
                label, node, count, ..
            } => {
                let (kind, base) = folder_kind(node);
                BarInfo {
                    count: Some(*count as i64),
                    ..plain(kind, base, label.clone())
                }
            }
            Bar::Song(k) => {
                let s = ctx.lib.songs.get(*k)?;
                BarInfo {
                    // a favourite: marked
                    title: match super::tree::favorite(ctx, s) {
                        true => format!("♥ {}", s.full_title()),
                        false => s.full_title(),
                    },
                    level: Some(s.level_num()),
                    difficulty: s.difficulty.clamp(0, 6) as usize,
                    lamp: Some(
                        ctx.best(s, ctx.config.play.ln_mode)
                            .map(|b| b.clear)
                            .unwrap_or(ClearType::NoPlay),
                    ),
                    rival: self.rival_lamp(s),
                    ..plain(BAR_SONG, BAR_SONG, String::new())
                }
            }
            Bar::Random => plain(
                BAR_COMMAND,
                BAR_COMMAND,
                match ctx.config.select.random {
                    RandomSelect::Any => "RANDOM SELECT".into(),
                    r => format!("RANDOM SELECT ({})", r.label()),
                },
            ),
            Bar::Missing(m) => BarInfo {
                // the table's level ("★12" -> 12)
                level: m
                    .level
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok(),
                ..plain(BAR_NOSONG, BAR_NOSONG, m.title.clone())
            },
            Bar::Course(c) => {
                let (kind, stages) = match c {
                    CourseBar::Mine(id) => ctx
                        .lib
                        .courses
                        .get(*id)
                        .map_or((CourseKind::Expert, None), |c| {
                            (c.kind, Some(c.stages.len()))
                        }),
                    CourseBar::Table(url, k) => (
                        CourseKind::Class,
                        table(ctx, url)
                            .and_then(|t| t.header.courses.get(*k))
                            .map(|c| c.md5.len().max(c.sha256.len())),
                    ),
                    CourseBar::RandomMix(kind) => (*kind, None),
                };
                BarInfo {
                    lamp: super::launch::course_lamp(ctx, c),
                    count: stages.map(|n| n as i64),
                    ..plain(
                        course_kind(kind),
                        BAR_GRADE,
                        super::launch::course_title(ctx, c),
                    )
                }
            }
        })
    }

    /// Options fixed while a bar stays selected (also used at load time).
    pub fn static_option(&self, id: i32) -> Option<bool> {
        let p = &self.ctx.config.play;
        let song = self.song;
        if let Some(v) = self.chart.as_ref().and_then(|c| chart_option(c, id)) {
            return Some(v);
        }
        Some(match id {
            1 => matches!(self.bar, Some(Bar::Folder { .. })),
            2 => matches!(self.bar, Some(Bar::Song(_) | Bar::Missing(_))),
            3 => matches!(self.bar, Some(Bar::Course(_))),
            5 => song.is_some(),
            32 | 33 | 40..=43 | 1046 => return play_option(p.autoplay, p.bga, p.gauge, id),
            30 | 31 | 34..=39 => return display_option(p, id),
            100..=105 | 1100..=1104 => {
                let c = self.best.map(|b| b.clear).unwrap_or(ClearType::NoPlay);
                song.is_some() && lamp_option(c, id).unwrap_or(false)
            }
            // no chart: no stage file, no banner
            190 | 192 => true,
            320..=327 => self
                .best
                .map(|b| rank_index(DjLevel::from_rate(b.ex_score, b.notes * 2)) == id - 320)
                .unwrap_or(false),
            400 => p.hispeed_mode == HiSpeedMode::Constant,
            _ => return None,
        })
    }
}

impl SkinState for SelectState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        let s = self.s;
        match id {
            10 | 11 => Some(s.host.ms(s.last_move)),
            _ => {
                if let Some(n) = panel_of_op(id) {
                    s.panel_on[n - 1]
                        .filter(|_| s.panel == n)
                        .map(|t| s.host.ms(t))
                } else if let Some(n) = panel_of_closed_timer(id) {
                    s.panel_off[n - 1]
                        .filter(|_| s.panel != n)
                        .map(|t| s.host.ms(t))
                } else {
                    s.host.timer(id)
                }
            }
        }
    }

    fn option(&self, id: i32) -> bool {
        let v = match id.abs() {
            n @ (21..=26 | 9027..=9029) => panel_of_op(n) == Some(self.s.panel),
            n @ 1500..=1819 => {
                crate::scene::option_panel::option(&self.ctx.config.play, n).unwrap_or(false)
            }
            n @ 9220..=9492 => self.vibe_option(n).unwrap_or(false),
            n @ OPTION_FOCUS..=9799 => {
                let s = self.s;
                s.panel > 0
                    && !s.holding()
                    && s.focus as i32 == n - OPTION_FOCUS
                    && !PANEL_TILES[s.panel - 1].is_empty()
            }
            601 => matches!(self.rank, Some(Rank::Loading(_))),
            602 => matches!(self.rank, Some(Rank::Done(_))),
            604 => matches!(self.rank, Some(Rank::Error)),
            n => self.static_option(n).unwrap_or(false),
        };
        v != (id < 0)
    }

    fn number(&self, id: i32) -> Option<i64> {
        if let Some(v) = self
            .clock
            .number(id)
            .or_else(|| player_number(self.ctx, id))
        {
            return Some(v);
        }
        let p = &self.ctx.config.play;
        let song = self.song;
        Some(match id {
            20 => self.ctx.view.fps as i64,
            12 => p.judge_offset_ms as i64,
            // a selected song's record (0 when never played)
            71 | 75..=84 | 102 | 103 | BEST_MIN_CB if song.is_some() => {
                return best_number(self.best, id)
            }
            370 => return best_number(self.best, id),
            72 => song?.notes * 2,
            74 => song?.notes,
            90..=92 | 96 | 400 => return chart_number(self.chart.as_ref()?, id),
            300 => match self.bar {
                Some(Bar::Folder { count, .. }) => *count as i64,
                _ => self.s.bars.len() as i64,
            },
            10 | 310..=313 => return speed_number(p, &self.bpms, id),
            179 => self.my_rank()? as i64,
            180 => self.players()? as i64,
            380..=389 => self.ranking()?.get((id - 380) as usize)?.ex as i64,
            390..=399 => self.ranking()?.get((id - 390) as usize)?.clear as i64,
            350..=368 => return song_number(song?, id),
            NUMBER_CLEAR_RATE => status::clear_rate(self.rank_view()?)?,
            n @ NUMBER_VOLUME..=9263 => (self.volume(n - NUMBER_VOLUME)? * 100.0).round() as i64,
            n @ NUMBER_FOLDER..=9303 => {
                let Some(Bar::Folder { count, .. }) = self.bar else {
                    return None;
                };
                let d = self.s.folder_distribution(self.ctx, self.s.cursor)?;
                status::folder_facts(*count, &d)[(n - NUMBER_FOLDER) as usize]
            }
            n @ FOLDER_LAMPS..=9338 => self.folder_count(n)?.0 as i64,
            n @ SIBLING..=9454 => self.sibling(n, SIBLING)?.level_num(),
            n @ SIBLING_LAMP..=9474 => self.lamp_of(self.sibling(n, SIBLING_LAMP)?) as i64,
            1163 => song?.length_ms / 60000,
            1164 => (song?.length_ms / 1000) % 60,
            _ => return None,
        })
    }

    fn distribution(&self, rank: bool) -> Option<Vec<u32>> {
        let (l, r) = self.s.folder_distribution(self.ctx, self.s.cursor)?;
        Some(if rank { r.to_vec() } else { l.to_vec() })
    }

    fn button(&self, id: i32) -> Option<i64> {
        use crate::scene::option_panel::index_of;
        let sel = &self.ctx.config.select;
        Some(match id {
            // LR2 / beatoraja: difficulty filter, key mode filter (in
            // beatoraja's order), sort
            10 => sel.difficulty_filter as i64,
            11 => index_of(&KeyFilter::ALL, sel.key_filter),
            12 => index_of(&SortMode::ALL, sel.sort),
            _ => return config_button(self.ctx, id).or_else(|| self.number(id)),
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let song = self.song;
        Some(match id {
            2 => self.ctx.config.player_name.clone(),
            // folders and courses show their name as the title
            10 | 12 if song.is_none() => self.bar(self.s.cursor)?.title,
            11 if song.is_none() => match self.bar? {
                Bar::Folder { sub, .. } => sub.clone(),
                _ => return None,
            },
            14 | 16 if song.is_none() => match self.bar? {
                Bar::Missing(m) => m.artist.clone(),
                _ => return None,
            },
            10..=16 | 1030 | 1031 => return chart_text(&ChartText::of_song(song?), id),
            120..=129 => self.ranking()?.get((id - 120) as usize)?.player.clone(),
            TEXT_IR => return self.ir_line(),
            n @ TEXT_KEY_OPTION..=9617 => {
                let (t, k) = (
                    ((n - TEXT_KEY_OPTION) / 3) as usize,
                    (n - TEXT_KEY_OPTION) % 3,
                );
                let rivals = crate::scene::target::rivals(self.ctx);
                let mut p = self.ctx.config.play.clone();
                if k != 1 {
                    options::step_key_option(&mut p, t, k - 1, &self.bpms, &rivals);
                }
                options::key_option_label(&p, t, &rivals)
            }
            TEXT_MODE => status::mode_label(song?),
            TEXT_QUERY => self.s.query().unwrap_or_default().into(),
            TEXT_LAST_PLAYED => {
                let t = self.best.filter(|_| song.is_some())?.last_played;
                let local = chrono::DateTime::from_timestamp(t, 0)?.with_timezone(&chrono::Local);
                (t > 0).then(|| local.format("%Y-%m-%d").to_string())?
            }
            TEXT_SAVE => status::outcome(&self.ctx.config.play, song)
                .0
                .label()
                .into(),
            TEXT_CLEAR => match status::outcome(&self.ctx.config.play, song).1 {
                Some(c) => c.label().into(),
                None => "—".into(),
            },
            TEXT_NETWORK => status::network(self.ctx).0,
            TEXT_ASSIST => status::assist_summary(&self.ctx.config.play),
            TEXT_COVER => status::lane_cover_summary(&self.ctx.config.play),
            30 => self
                .s
                .search_edit
                .as_ref()
                .map_or(String::new(), |t| t.buf.clone()),
            1000 => self.s.path_label(self.ctx),
            // the table being browsed, else the selected chart's
            1001..=1003 => match &self.s.node {
                Node::Level(t, l) => {
                    let t = table(self.ctx, t)?;
                    match id {
                        1001 => t.header.name.clone(),
                        1002 => t.label(l),
                        _ => format!("{}{}", t.label(l), t.header.name),
                    }
                }
                Node::Table(t) | Node::Courses(t) if id != 1002 => {
                    table(self.ctx, t)?.header.name.clone()
                }
                _ => return table_text(self.table()?, id),
            },
            1010 => format!("vibeBMS {}", env!("CARGO_PKG_VERSION")),
            n if (OPTION_TEXT..OPTION_TEXT + 100).contains(&n) => {
                let o = SelectOption::ALL.get((n - OPTION_TEXT) as usize)?;
                o.value(self.ctx, song.map(Bpms::of_song), song)
            }
            n if (OPTION_NAME..OPTION_NAME + 100).contains(&n) => SelectOption::ALL
                .get((n - OPTION_NAME) as usize)?
                .name()
                .into(),
            _ => return None,
        })
    }

    fn rate(&self, id: i32) -> f32 {
        let n = self.s.bars.len();
        match id {
            1 if n > 1 => self.s.scroll.rem_euclid(n as f32) / (n - 1) as f32,
            110 | 113 => self.best.map(|b| b.rate() as f32).unwrap_or(0.0),
            NUMBER_VOLUME..=9263 => self.volume(id - NUMBER_VOLUME).unwrap_or(0.0),
            FOLDER_LAMPS..=9338 => self
                .folder_count(id)
                .map_or(0.0, |(n, max)| n as f32 / max.max(1) as f32),
            SIBLING..=9454 => self
                .sibling(id, SIBLING)
                .map_or(0.0, |s| (s.level_num() as f32 / 12.0).clamp(0.0, 1.0)),
            _ => 0.0,
        }
    }
}

pub struct SelectHooks<'a> {
    pub st: &'a SelectState<'a>,
    pub banner: Option<TexId>,
    /// Bars drawn: their indices.
    pub bars: crate::ui::HitMap<usize>,
    /// How far down the screen a bar moves per list step (px), when the
    /// list goes down the screen.
    pub pitch: Option<f32>,
}

/// A download's progress over its song's bar `r`: a strip along its bottom
/// (filled by the share in, or a moving segment while that is not known)
/// and a line saying where it is ("12.3 / 40.0 MB", "Unpacking…").
fn download_progress(d: &mut Draw, r: Rect, p: &bmseeker::progress::Progress, now: i64) {
    use bmseeker::progress::Stage;
    let h = (r.h * 0.14).clamp(3.0, 8.0);
    let strip = Rect::new(r.x, r.bottom() - h, r.w, h);
    d.b.rect(strip, [0.0, 0.0, 0.0, 0.55]);
    let fill = [0.35, 0.75, 1.0, 0.95];
    match (p.stage(), p.fraction()) {
        (Stage::Downloading, Some(f)) => d.b.rect(Rect::new(r.x, strip.y, r.w * f, h), fill),
        (Stage::Adding, _) => d.b.rect(strip, [0.45, 0.85, 0.5, 0.95]),
        // not known: a segment going across
        _ => {
            let w = r.w * 0.25;
            let t = (now.rem_euclid(1600) as f32) / 1600.0;
            let x = r.x - w + (r.w + w) * t;
            let (a, b) = (x.max(r.x), (x + w).min(r.right()));
            if b > a {
                d.b.rect(Rect::new(a, strip.y, b - a, h), fill);
            }
        }
    }
    let size = (r.h * 0.32).clamp(10.0, 18.0);
    // (left of the level number at the bar's right end)
    let (x, y) = (r.right() - r.h * 1.9, strip.y - size * 1.2);
    let label = p.label();
    d.text(
        &label,
        x + 1.0,
        y + 1.0,
        TextStyle::new(size, [0.0, 0.0, 0.0, 0.8], Align::Right),
    );
    d.text(
        &label,
        x,
        y,
        TextStyle::new(size, [0.92, 0.96, 1.0, 1.0], Align::Right),
    );
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Rect::new(l(a.x, b.x), l(a.y, b.y), l(a.w, b.w), l(a.h, b.h))
}

impl Hooks for SelectHooks<'_> {
    fn ref_image(&self, id: i32) -> Option<TexId> {
        match id {
            100 | 102 => self.banner,
            _ => None,
        }
    }

    fn song_list(
        &mut self,
        d: &mut Draw,
        skin: &LoadedSkin,
        xf: &Xf,
        now: i64,
        st: &dyn SkinState,
    ) {
        let Some(sl) = skin.skin.songlist.as_ref() else {
            return;
        };
        let s = self.st.s;
        let n = s.bars.len();
        let slots = sl.off.len().max(sl.on.len());
        if n == 0 || slots == 0 {
            return;
        }
        let pos = s.scroll;
        let base = pos.floor();
        let frac = pos - base;
        let slot_frame = |k: isize, on: bool| {
            let k = k.clamp(0, slots as isize - 1) as usize;
            let (first, other) = if on {
                (&sl.on, &sl.off)
            } else {
                (&sl.off, &sl.on)
            };
            let d = first.get(k).and_then(Option::as_ref);
            d.or_else(|| other.get(k).and_then(Option::as_ref))?
                .eval(now, st)
        };
        let centre = sl.center as isize;
        self.pitch = slot_frame(centre, false)
            .zip(slot_frame(centre - 1, false))
            .map(|(a, b)| (xf.rect(a.rect).y - xf.rect(b.rect).y).abs())
            .filter(|p| *p >= 1.0);
        // the cursor in unwrapped list coordinates nearest to the scroll position
        let cur = {
            let c = s.cursor as f32;
            let k = ((pos - c) / n as f32).round();
            (c + k * n as f32) as i64
        };
        for slot in 0..slots {
            let unwrapped = base as i64 + slot as i64 - sl.center as i64;
            let idx = unwrapped.rem_euclid(n as i64) as usize;
            // a short list repeats: only the instance at the cursor is selected
            let selected = unwrapped == cur;
            let (Some(a), Some(b)) = (
                slot_frame(slot as isize, selected),
                slot_frame(slot as isize - 1, selected),
            ) else {
                continue;
            };
            // items move one slot up as the scroll position advances
            let r = lerp_rect(a.rect, b.rect, frac);
            let fr = Frame { rect: r, ..a };
            let Some(info) = self.st.bar(idx) else {
                continue;
            };
            let sr = xf.rect(r);
            let kind = match info.kind < sl.body.len() {
                true => info.kind,
                false => info.base,
            };
            if let Some(body) = sl
                .body
                .get(kind)
                .or(sl.body.first())
                .and_then(|b| b.as_ref())
            {
                skin.blit(d, body, now, st, sr, &fr);
            }
            if sl.clickable.is_empty() || sl.clickable.contains(&slot) {
                self.bars.push(sr, idx);
            }
            let sub = |dst: &Dst| {
                dst.eval(now, st).map(|f| {
                    let rr = Rect::new(r.x + f.rect.x, r.y + f.rect.y, f.rect.w, f.rect.h);
                    (xf.rect(rr), f)
                })
            };
            if let Some((t, align, overflow, font)) = sl.text.as_ref() {
                let chart = matches!(info.base, BAR_SONG | BAR_NOSONG);
                let t = sl.folder_text.as_ref().filter(|_| !chart).unwrap_or(t);
                if let Some((tr, f)) = sub(t) {
                    let mut c = f.color;
                    c[3] *= fr.color[3];
                    // not installed: dimmed
                    if info.kind == BAR_NOSONG {
                        c[3] *= 0.45;
                    }
                    // the selected bar's title scrolls when it is too long
                    let shown = selected.then(|| now - st.timer(11).unwrap_or(0));
                    let face = sl.text_face.as_deref().and_then(|p| d.text.face(p));
                    d.text.set_face(face.unwrap_or(0));
                    let (al, of, fx, size) = (*align, *overflow, &sl.text_fx, sl.text_size);
                    skin.text(d, &info.title, tr, al, of, c, *font, shown, fx, size, xf.sy);
                    d.text.set_face(0);
                }
            }
            let numbers = [
                (
                    info.level,
                    sl.level.get(info.difficulty).and_then(Option::as_ref),
                ),
                (info.count, sl.count.as_ref()),
            ];
            for (n, (font, dst)) in numbers.iter().filter_map(|(n, f)| n.zip(*f)) {
                if let Some((lr, f)) = sub(dst) {
                    skin.number(
                        d,
                        &font.src,
                        font.digits,
                        font.keta,
                        font.align,
                        None,
                        n,
                        now,
                        st,
                        lr,
                        &f,
                    );
                }
            }
            if let Some((dst, gsrc, rank)) = sl.graph.as_ref() {
                if let Some((l, r)) = self.st.s.folder_distribution(self.st.ctx, idx) {
                    if let Some((gr, f)) = sub(dst) {
                        let counts: &[u32] = if *rank { &r } else { &l };
                        skin.distribution(d, gsrc.as_ref(), counts, *rank, now, st, gr, &f);
                    }
                }
            }
            // with a rival chosen, the pair where the skin has one
            let pair = !sl.my_lamp.is_empty() && !sl.rival_lamp.is_empty();
            let lamps = match info.rival {
                Some(r) if pair => [(info.lamp, &sl.my_lamp), (Some(r), &sl.rival_lamp)],
                _ => [(info.lamp, &sl.lamp), (None, &sl.lamp)],
            };
            for (clear, set) in lamps {
                if let Some(Some((src, dst))) = clear.and_then(|c| set.get(c as usize)) {
                    if let Some((lr, f)) = sub(dst) {
                        skin.blit(d, src, now, st, lr, &f);
                    }
                }
            }
            // a table song downloading: how far it has got, over its bar
            if let Some(Bar::Missing(m)) = s.bars.get(idx) {
                if let Some(p) = self.st.ctx.net.song_fetch.progress(&m.title, &m.artist) {
                    download_progress(d, sr, p, now);
                }
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::config::IrAccount;
    use std::sync::Arc;
    use vibebms_ir::lamps::RankEntry;

    /// The player's IR lamps (as a start finds them in the cache) beside
    /// the local lamp of each song bar, and the selected chart's IR line
    /// and rank numbers.
    #[test]
    fn ir_lamps_beside_and_the_rank() {
        let root = std::env::temp_dir().join(format!("vibebms-select-ir-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let song = |md5: &str, sha: &str| Song {
            md5: md5.into(),
            sha256: sha.into(),
            title: md5.into(),
            ..Default::default()
        };
        let (a, b) = ("a".repeat(32), "b".repeat(32));
        ctx.lib.songs = Arc::new(vec![song(&a, "1"), song(&b, "2")]);
        ctx.config.ir.accounts = vec![IrAccount {
            provider: ProviderId::BmsIr,
            user_id: "9".into(),
            tables: vec!["official".into()],
            ..Default::default()
        }];
        let now = crate::platform::unix_ms();
        let file = ctx.paths.cache.join("ir/bmsir/9/official.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let set = serde_json::json!({"view": "", "fetched_ms": now, "set": {
            "player_name": "me", "by_chart": {a.clone(): {"clear": "Hard"}}}});
        std::fs::write(&file, set.to_string()).unwrap();
        ctx.net.ir.poll(now, &mut ctx.config.ir);
        let mut s = SelectScene::new(&mut ctx);
        s.bars = vec![Bar::Song(0), Bar::Song(1)];
        s.cursor = 0;
        let view = RankView {
            players: 40,
            entries: vec![RankEntry {
                rank: 3,
                player_id: "9".into(),
                ..Default::default()
            }],
            etag: None,
        };
        s.ranking.insert("1".into(), Rank::Done(view));
        let st = SelectState::new(&s, &ctx);
        let rival = |i| st.bar(i).unwrap().rival;
        assert_eq!(
            (rival(0), rival(1)),
            (Some(ClearType::Hard), Some(ClearType::NoPlay))
        );
        assert_eq!(st.text(TEXT_IR).as_deref(), Some("IR HARD   #3 / 40"));
        assert_eq!((st.number(179), st.number(180)), (Some(3), Some(40)));
        // none beside: no rival lamps; the rank stays
        ctx.config.ir.beside = crate::config::Beside::Off;
        let st = SelectState::new(&s, &ctx);
        assert_eq!(st.bar(0).unwrap().rival, None);
        assert_eq!(st.text(TEXT_IR).as_deref(), Some("#3 / 40"));
        std::fs::remove_dir_all(&root).ok();
    }

    fn chart(folder: &str, mode: vibebms_core::PlayMode, difficulty: i32, level: &str) -> Song {
        Song {
            path: format!("{folder}/{difficulty}_{level}_{mode:?}.bms"),
            sha256: format!("{folder}{difficulty}{level}{mode:?}"),
            folder: folder.into(),
            title: format!("{difficulty}-{level}"),
            mode,
            difficulty,
            level: level.into(),
            ..Default::default()
        }
    }

    /// A song's charts by difficulty: the selected one in its slot, else
    /// the highest level; other key modes and folders apart. Choosing a
    /// slot moves to its bar, or puts the chart in place of the selected.
    #[test]
    fn the_songs_other_charts() {
        use vibebms_core::PlayMode::{Beat14K, Beat7K};
        let root = std::env::temp_dir().join(format!("vibebms-select-sib-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        // (b's chart is in a's library folder, not in its song's folder)
        let mut other = chart("b", Beat7K, 1, "1");
        other.folder = "a".into();
        ctx.lib.songs = Arc::new(vec![
            chart("a", Beat7K, 2, "5"),
            chart("a", Beat7K, 4, "11"),
            chart("a", Beat7K, 4, "12"),
            chart("a", Beat14K, 1, "3"),
            other,
        ]);
        let mut s = SelectScene::new(&mut ctx);
        s.bars = vec![Bar::Song(1), Bar::Song(0)];
        s.cursor = 0;
        let st = SelectState::new(&s, &ctx);
        let levels: Vec<_> = (0..5).map(|k| st.number(SIBLING + k)).collect();
        assert_eq!(levels, [None, Some(5), None, Some(11), None]);
        assert!(st.option(SIBLING + 1) && !st.option(SIBLING));
        assert!(st.option(SIBLING_CURRENT + 3) && !st.option(SIBLING_CURRENT + 1));
        assert_eq!(st.number(SIBLING_LAMP + 1), Some(ClearType::NoPlay as i64));
        assert_eq!(st.rate(SIBLING + 1), 5.0 / 12.0);
        assert_eq!(st.text(TEXT_MODE).as_deref(), Some("7KEYS"));
        // NORMAL is listed: its bar is chosen
        s.pick_sibling(&mut ctx, 1);
        assert_eq!(s.cursor, 1);
        // the other ANOTHER, the highest, is not: it takes the bar's place
        let st = SelectState::new(&s, &ctx);
        assert_eq!(st.number(SIBLING + 3), Some(12));
        s.pick_sibling(&mut ctx, 3);
        assert!(matches!(s.bars[..], [Bar::Song(1), Bar::Song(2)]));
        std::fs::remove_dir_all(&root).ok();
    }

    /// PLAY OPTION's key options one back, now and one on; the keys move
    /// a focus over the open panel's tiles and step the focused one.
    #[test]
    fn panel_tiles_and_focus() {
        use crate::input::Nav;
        use vibebms_core::lane_option::LaneOption;
        let root = std::env::temp_dir().join(format!("vibebms-select-pnl-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let mut s = SelectScene::new(&mut ctx);
        let st = SelectState::new(&s, &ctx);
        let last = LaneOption::ALL[LaneOption::ALL.len() - 1].label();
        let random: Vec<_> = (0..3).map(|k| st.text(TEXT_KEY_OPTION + k)).collect();
        assert_eq!(
            random,
            [Some(last.into()), Some("OFF".into()), Some("MIRROR".into())]
        );
        assert_eq!(st.text(TEXT_KEY_OPTION + 16).as_deref(), Some("BEST"));
        assert!(!st.option(OPTION_FOCUS));
        s.toggle_panel(1);
        assert!(SelectState::new(&s, &ctx).option(OPTION_FOCUS));
        s.panel_nav(&mut ctx, Nav::Down);
        assert!(SelectState::new(&s, &ctx).option(OPTION_FOCUS + 1));
        let gauge = ctx.config.play.gauge;
        s.panel_nav(&mut ctx, Nav::Decide);
        assert_ne!(ctx.config.play.gauge, gauge);
        s.panel_nav(&mut ctx, Nav::Close);
        assert_eq!(ctx.config.play.gauge, gauge);
        // around the end; Back closes
        s.panel_nav(&mut ctx, Nav::Up);
        s.panel_nav(&mut ctx, Nav::Up);
        assert_eq!(s.focus, PANEL_TILES[0].len() - 1);
        s.panel_nav(&mut ctx, Nav::Back);
        assert_eq!(s.panel, 0);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A folder's counts and bar types, the status line and the files a
    /// chart has.
    #[test]
    fn folders_status_and_files() {
        let root = std::env::temp_dir().join(format!("vibebms-select-st-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let dir = root.join("bms/song");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("readme.TXT"), "hello").unwrap();
        let mut a = chart(
            dir.to_str().unwrap(),
            vibebms_core::PlayMode::Beat7K,
            4,
            "11",
        );
        a.path = dir.join("a.bms").to_string_lossy().into_owned();
        let b = chart("b", vibebms_core::PlayMode::Beat7K, 4, "11");
        ctx.lib.songs = Arc::new(vec![a, b]);
        let mut s = SelectScene::new(&mut ctx);
        let folder = |node: Node, count: usize| Bar::Folder {
            label: "F".into(),
            sub: String::new(),
            node,
            count,
        };
        s.bars = vec![
            Bar::Song(0),
            folder(Node::All, 2),
            folder(Node::Favorites, 0),
            folder(Node::MyCourses(CourseKind::Nonstop), 0),
        ];
        let st = SelectState::new(&s, &ctx);
        assert!(st.option(OPTION_HAS_TEXT) && !st.option(OPTION_REPLAY));
        assert_eq!(s.files.borrow().texts, [dir.join("readme.TXT")]);
        assert_eq!(st.text(TEXT_SAVE).as_deref(), Some("SAVE"));
        assert!(st.option(OPTION_SAVE) && !st.option(OPTION_SAVE + 2));
        let lamp = ctx.config.play.gauge.clear_type();
        assert_eq!(st.text(TEXT_CLEAR).as_deref(), Some(lamp.label()));
        assert!(st.option(OPTION_CLEAR + lamp as i32));
        assert_eq!(st.text(TEXT_NETWORK).as_deref(), Some("OFF"));
        assert!(!st.option(OPTION_ONLINE) && !st.option(OPTION_OFFLINE));
        let kinds: Vec<_> = (1..4)
            .map(|i| st.bar(i).map(|b| (b.kind, b.base)))
            .collect();
        let want = [
            (BAR_FOLDER, BAR_FOLDER),
            (BAR_FAVORITE, BAR_FOLDER),
            (BAR_NONSTOP, BAR_FOLDER),
        ];
        assert_eq!(kinds, want.map(Some));
        assert_eq!(st.bar(1).unwrap().count, Some(2));
        s.cursor = 1;
        let st = SelectState::new(&s, &ctx);
        let facts: Vec<_> = (0..4).map(|k| st.number(NUMBER_FOLDER + k)).collect();
        assert_eq!(facts, [2, 2, 0, 0].map(Some));
        assert_eq!(st.number(NUMBER_FOLDER), Some(2));
        ctx.config.play.autoplay = true;
        let st = SelectState::new(&s, &ctx);
        assert_eq!(st.text(TEXT_SAVE).as_deref(), Some("NO SAVE"));
        assert_eq!(st.text(TEXT_CLEAR).as_deref(), Some("—"));
        std::fs::remove_dir_all(&root).ok();
    }
}
