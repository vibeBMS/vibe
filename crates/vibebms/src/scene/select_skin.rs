//! Music select values and the bar list for select skins (LR2 / beatoraja ids).
//!
//! Option values and buttons: beatoraja ids where they exist (42 random,
//! 40 gauge, 308 LN mode, 15 play, 16 autoplay, 13 key config), plus
//! vibeBMS ids for its own select options: text `9000 + i` is the value and
//! `9100 + i` the name of option `i`, and a clickable object with event
//! `9000 + i` cycles it (see `SelectScene::option`); event 9900 leaves the
//! current folder.

use super::select::{Bar, Node, SelectScene};
use super::skinned::{ms_since, rank_index};
use crate::app::{Ctx, Draw};
use crate::config::HiSpeedMode;
use crate::gfx::{Rect, TexId};
use crate::library::{BestScore, Song};
use crate::platform::Instant;
use crate::skin::render::{Hooks, LoadedSkin, Xf};
use crate::skin::*;
use vibebms_core::gauge::GaugeType;
use vibebms_core::lane_option::LaneOption;
use vibebms_core::score::{ClearType, DjLevel};
use vibebms_core::{LnType, PlayMode};

/// vibeBMS select option ids (text value / name, click event).
pub const OPTION_TEXT: i32 = 9000;
pub const OPTION_NAME: i32 = 9100;
pub const OPTION_EVENT: i32 = 9000;
/// Click event: leave the current folder.
pub const EVENT_BACK: i32 = 9900;

pub struct SelectState<'a> {
    pub s: &'a SelectScene,
    pub ctx: &'a Ctx,
    pub now: Instant,
}

/// What a bar shows.
pub struct BarInfo {
    pub kind: usize,
    pub title: String,
    pub level: Option<i64>,
    pub difficulty: usize,
    pub lamp: Option<ClearType>,
}

impl SelectState<'_> {
    pub fn song(&self) -> Option<&Song> {
        match self.s.bars.get(self.s.cursor) {
            Some(Bar::Song(i)) => self.ctx.songs.get(*i),
            _ => None,
        }
    }

    /// IR ranking of the selected chart, once loaded.
    fn ranking(&self) -> Option<&[vibebms_ir::IrScore]> {
        match self.s.ranking.get(&self.song()?.sha256)? {
            super::select::Rank::Done(v) => Some(v),
            _ => None,
        }
    }

    fn best(&self) -> Option<&BestScore> {
        self.song()
            .and_then(|s| self.ctx.best(s, self.ctx.config.play.ln_mode as i32))
    }

    pub fn bar(&self, i: usize) -> Option<BarInfo> {
        let ctx = self.ctx;
        Some(match self.s.bars.get(i)? {
            Bar::Folder { label, node, .. } => BarInfo {
                kind: match node {
                    Node::Table(_) | Node::Level(..) => BAR_TABLE,
                    Node::Search(_) => BAR_SEARCH,
                    _ => BAR_FOLDER,
                },
                title: label.clone(),
                level: None,
                difficulty: 0,
                lamp: None,
            },
            Bar::Song(k) => {
                let s = ctx.songs.get(*k)?;
                BarInfo {
                    kind: BAR_SONG,
                    title: s.full_title(),
                    level: Some(s.level_num()),
                    difficulty: s.difficulty.clamp(0, 6) as usize,
                    lamp: Some(
                        ctx.best(s, ctx.config.play.ln_mode as i32)
                            .map(|b| b.clear)
                            .unwrap_or(ClearType::NoPlay),
                    ),
                }
            }
            Bar::Missing { title, level, .. } => BarInfo {
                kind: BAR_NOSONG,
                title: title.clone(),
                // the table's level ("★12" -> 12)
                level: level
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok(),
                difficulty: 0,
                lamp: None,
            },
            Bar::Course { table, course } => BarInfo {
                kind: BAR_GRADE,
                title: ctx
                    .tables
                    .get(*table)
                    .and_then(|t| t.header.courses.get(*course))
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                level: None,
                difficulty: 0,
                lamp: None,
            },
        })
    }

    fn mode(&self) -> Option<PlayMode> {
        let m = &self.song()?.mode;
        PlayMode::ALL.iter().copied().find(|p| p.label() == m)
    }

    /// Options fixed while a bar stays selected (also used at load time).
    pub fn static_option(&self, id: i32) -> Option<bool> {
        let p = &self.ctx.config.play;
        let song = self.song();
        let best = self.best();
        let bar = self.s.bars.get(self.s.cursor);
        Some(match id {
            1 => matches!(bar, Some(Bar::Folder { .. })),
            2 => matches!(bar, Some(Bar::Song(_) | Bar::Missing { .. })),
            3 => matches!(bar, Some(Bar::Course { .. })),
            5 => song.is_some(),
            32 => !p.autoplay,
            33 => p.autoplay,
            40 => !p.bga,
            41 => p.bga,
            42 => !p.gauge.is_survival(),
            43 => p.gauge.is_survival(),
            1046 => matches!(p.gauge, GaugeType::ExHard | GaugeType::ExHardClass),
            100..=105 | 1100..=1104 => {
                let c = best.map(|b| b.clear).unwrap_or(ClearType::NoPlay);
                song.is_some() && super::skinned::lamp_option(c, id).unwrap_or(false)
            }
            150..=155 => song.map(|s| s.difficulty == id - 150).unwrap_or(false),
            160..=164 => {
                let m = [
                    PlayMode::Beat7K,
                    PlayMode::Beat5K,
                    PlayMode::Beat14K,
                    PlayMode::Beat10K,
                    PlayMode::Popn9K,
                ][(id - 160) as usize];
                self.mode() == Some(m)
            }
            170 => song.map(|s| !s.has_bga).unwrap_or(false),
            171 => song.map(|s| s.has_bga).unwrap_or(false),
            172 => song.map(|s| !s.has_ln).unwrap_or(false),
            173 => song.map(|s| s.has_ln).unwrap_or(false),
            176 => song.map(|s| s.min_bpm == s.max_bpm).unwrap_or(false),
            177 => song.map(|s| s.min_bpm != s.max_bpm).unwrap_or(false),
            1177 => song.map(|s| s.has_stop).unwrap_or(false),
            178 => song.map(|s| !s.has_random).unwrap_or(false),
            179 => song.map(|s| s.has_random).unwrap_or(false),
            190 => song.map(|s| s.stagefile.is_empty()).unwrap_or(true),
            191 => song.map(|s| !s.stagefile.is_empty()).unwrap_or(false),
            192 => song.map(|s| s.banner.is_empty()).unwrap_or(true),
            193 => song.map(|s| !s.banner.is_empty()).unwrap_or(false),
            320..=327 => best
                .map(|b| rank_index(DjLevel::from_rate(b.ex_score, b.notes * 2)) == id - 320)
                .unwrap_or(false),
            400 => p.hispeed_mode == HiSpeedMode::Constant,
            _ => return None,
        })
    }
}

/// beatoraja option indices.
fn random_index(o: LaneOption) -> i64 {
    match o {
        LaneOption::Off => 0,
        LaneOption::Mirror => 1,
        LaneOption::Random => 2,
        LaneOption::RRandom => 3,
        LaneOption::SRandom => 4,
        LaneOption::Spiral => 5,
        LaneOption::HRandom => 6,
        LaneOption::AllScratch => 7,
        LaneOption::RandomEx => 8,
        LaneOption::SRandomEx => 9,
    }
}

impl SkinState for SelectState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        let s = self.s;
        match id {
            0 => Some(0),
            1 => Some(s.skin.as_ref().map(|k| k.skin.input_ms).unwrap_or(0)),
            10 | 11 => Some(ms_since(s.created, s.last_move)),
            21..=26 => s.panel_on[(id - 21) as usize]
                .filter(|_| s.panel == (id - 20) as usize)
                .map(|t| ms_since(s.created, t)),
            31..=36 => s.panel_off[(id - 31) as usize]
                .filter(|_| s.panel != (id - 30) as usize)
                .map(|t| ms_since(s.created, t)),
            _ => None,
        }
    }

    fn option(&self, id: i32) -> bool {
        let rank = self.song().and_then(|s| self.s.ranking.get(&s.sha256));
        let v = match id.abs() {
            21..=26 => self.s.panel == (id.abs() - 20) as usize,
            601 => matches!(rank, Some(super::select::Rank::Loading(_))),
            602 => matches!(rank, Some(super::select::Rank::Done(_))),
            604 => matches!(rank, Some(super::select::Rank::Error)),
            n => self.static_option(n).unwrap_or(false),
        };
        v != (id < 0)
    }

    fn number(&self, id: i32) -> Option<i64> {
        if let Some(v) = super::skinned::clock_number(id) {
            return Some(v);
        }
        let p = &self.ctx.config.play;
        let song = self.song();
        let best = self.best();
        // score values exist only for a selected song (0 when never played)
        let b = |f: fn(&BestScore) -> i64| song.map(|_| best.map(f).unwrap_or(0));
        Some(match id {
            20 => self.ctx.fps as i64,
            30 => self.ctx.scores.values().map(|b| b.play_count as i64).sum(),
            71 => b(|b| b.ex_score as i64)?,
            72 => song.map(|s| s.notes * 2)?,
            74 => song.map(|s| s.notes)?,
            75 => b(|b| b.max_combo as i64)?,
            76 => b(|b| {
                if b.min_bp == u32::MAX {
                    0
                } else {
                    b.min_bp as i64
                }
            })?,
            77 => b(|b| b.play_count as i64)?,
            78 => b(|b| b.clear_count as i64)?,
            79 => b(|b| (b.play_count - b.clear_count) as i64)?,
            80..=84 => song.map(|_| {
                best.map(|b| b.judges[(id - 80) as usize] as i64)
                    .unwrap_or(0)
            })?,
            90 => song.map(|s| s.max_bpm.round() as i64)?,
            91 => song.map(|s| s.min_bpm.round() as i64)?,
            92 => song.map(|s| s.main_bpm.round() as i64)?,
            96 => song.map(|s| s.level_num())?,
            102 => song.map(|_| (best.map(|b| b.rate()).unwrap_or(0.0) * 100.0).floor() as i64)?,
            103 => song.map(|_| {
                ((best.map(|b| b.rate()).unwrap_or(0.0) * 10000.0).round() as i64) % 100
            })?,
            300 => match self.s.bars.get(self.s.cursor) {
                Some(Bar::Folder { count, .. }) => *count as i64,
                _ => self.s.bars.len() as i64,
            },
            312 => p.duration_ms as i64,
            313 => p.duration_ms as i64 * 3 / 5,
            370 => best.map(|b| b.clear as i64).unwrap_or(0),
            180 => self.ranking()?.len() as i64,
            380..=389 => self.ranking()?.get((id - 380) as usize)?.ex_score as i64,
            390..=399 => self.ranking()?.get((id - 390) as usize)?.clear as i64,
            1163 => song.map(|s| s.length_ms / 60000)?,
            1164 => song.map(|s| (s.length_ms / 1000) % 60)?,
            _ => return None,
        })
    }

    fn distribution(&self, rank: bool) -> Option<Vec<u32>> {
        let (l, r) = self.s.folder_distribution(self.ctx, self.s.cursor)?;
        Some(if rank { r.to_vec() } else { l.to_vec() })
    }

    fn button(&self, id: i32) -> Option<i64> {
        let p = &self.ctx.config.play;
        Some(match id {
            40 => match p.gauge {
                GaugeType::AssistEasy => 0,
                GaugeType::Easy => 1,
                GaugeType::Normal => 2,
                GaugeType::Hard | GaugeType::Class => 3,
                GaugeType::ExHard | GaugeType::ExHardClass | GaugeType::ExClass => 4,
                GaugeType::Hazard => 5,
            },
            42 => random_index(p.random_1p),
            43 => random_index(p.random_2p),
            54 => p.dp_option as i64,
            // beatoraja HS-FIX: 0 off, 3 main BPM
            55 => match p.hispeed_mode {
                HiSpeedMode::Floating => 3,
                HiSpeedMode::Constant => 0,
            },
            // beatoraja BGA: 0 on, 2 off
            72 => {
                if p.bga {
                    0
                } else {
                    2
                }
            }
            308 => match p.ln_mode {
                LnType::Ln => 0,
                LnType::Cn => 1,
                LnType::Hcn => 2,
            },
            _ => return self.number(id),
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let song = self.song();
        let opts = SelectScene::option_items(self.ctx);
        Some(match id {
            2 => self.ctx.config.player_name.clone(),
            // folders and courses show their name as the title
            10 | 12 => match song {
                Some(s) if id == 12 => s.full_title(),
                Some(s) => s.title.clone(),
                None => self.bar(self.s.cursor)?.title,
            },
            11 => match self.s.bars.get(self.s.cursor)? {
                Bar::Folder { sub, .. } => sub.clone(),
                _ => song?.subtitle.clone(),
            },
            120..=129 => self.ranking()?.get((id - 120) as usize)?.player.clone(),
            13 => song?.genre.clone(),
            14 | 16 if song.is_none() => match self.s.bars.get(self.s.cursor)? {
                Bar::Missing { artist, .. } => artist.clone(),
                _ => return None,
            },
            14 => song?.artist.clone(),
            15 => song?.subartist.clone(),
            16 => {
                let s = song?;
                format!("{} {}", s.artist, s.subartist).trim().to_string()
            }
            30 => self.s.search_edit.clone().unwrap_or_default(),
            1000 => self.s.path_label(self.ctx),
            1001 | 1003 => match &self.s.node {
                Node::Table(t) | Node::Level(t, _) | Node::Courses(t) => {
                    self.ctx.tables.get(*t)?.header.name.clone()
                }
                _ => return None,
            },
            1002 => match &self.s.node {
                Node::Level(t, l) => self.ctx.tables.get(*t)?.label(l),
                _ => return None,
            },
            1010 => format!("vibeBMS {}", env!("CARGO_PKG_VERSION")),
            n if (OPTION_TEXT..OPTION_TEXT + 100).contains(&n) => {
                opts.get((n - OPTION_TEXT) as usize)?.1.clone()
            }
            n if (OPTION_NAME..OPTION_NAME + 100).contains(&n) => {
                opts.get((n - OPTION_NAME) as usize)?.0.clone()
            }
            _ => return None,
        })
    }

    fn rate(&self, id: i32) -> f32 {
        let n = self.s.bars.len();
        match id {
            1 if n > 1 => self.s.scroll.rem_euclid(n as f32) / (n - 1) as f32,
            110 | 113 => self.best().map(|b| b.rate() as f32).unwrap_or(0.0),
            _ => 0.0,
        }
    }
}

pub struct SelectHooks<'a> {
    pub st: &'a SelectState<'a>,
    pub banner: Option<TexId>,
    /// Screen rects of drawn bars: (rect, bar index).
    pub bars: Vec<(Rect, usize)>,
    /// Clickable objects: (rect, event, click mode).
    pub clicks: Vec<(Rect, i32, i32)>,
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

    fn clickable(&mut self, rect: Rect, act: i32, click: i32) {
        self.clicks.push((rect, act, click));
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
            let d = if on {
                sl.on
                    .get(k)
                    .cloned()
                    .flatten()
                    .or(sl.off.get(k).cloned().flatten())
            } else {
                sl.off
                    .get(k)
                    .cloned()
                    .flatten()
                    .or(sl.on.get(k).cloned().flatten())
            }?;
            d.eval(now, st)
        };
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
            if let Some(body) = sl
                .body
                .get(info.kind)
                .or(sl.body.first())
                .and_then(|b| b.as_ref())
            {
                skin.blit(d, body, now, st, sr, &fr);
            }
            if sl.clickable.is_empty() || sl.clickable.contains(&slot) {
                self.bars.push((sr, idx));
            }
            let sub = |dst: &Dst| {
                dst.eval(now, st).map(|f| {
                    let rr = Rect::new(r.x + f.rect.x, r.y + f.rect.y, f.rect.w, f.rect.h);
                    (xf.rect(rr), f)
                })
            };
            if let Some((t, align, overflow, font)) = sl.text.as_ref() {
                if let Some((tr, f)) = sub(t) {
                    let mut c = f.color;
                    c[3] *= fr.color[3];
                    // not installed: dimmed
                    if info.kind == BAR_NOSONG {
                        c[3] *= 0.45;
                    }
                    skin.text(d, &info.title, tr, *align, *overflow, c, *font);
                }
            }
            if let (Some(lv), Some(Some((font, dst)))) = (info.level, sl.level.get(info.difficulty))
            {
                if let Some((lr, f)) = sub(dst) {
                    skin.number(
                        d,
                        &font.src,
                        font.digits,
                        font.keta,
                        font.align,
                        None,
                        lv,
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
            if let Some(Some((src, dst))) = info.lamp.and_then(|c| sl.lamp.get(c as usize)) {
                if let Some((lr, f)) = sub(dst) {
                    skin.blit(d, src, now, st, lr, &f);
                }
            }
        }
    }
}
