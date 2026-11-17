//! Music select values and the bar list for select skins (LR2 / beatoraja ids).
//!
//! Option values and buttons: beatoraja ids where they exist (42 random,
//! 40 gauge, 308 LN mode, 15 play, 16 autoplay, 13 key config), plus
//! vibeBMS ids for its own select options: text `9000 + i` is the value and
//! `9100 + i` the name of option `i`, and a clickable object with event
//! `9000 + i` cycles it (see `SelectOption`); event 9900 leaves the
//! current folder.

use super::options::SelectOption;
use super::preview::Rank;
use super::tree::{table, Bar, Node};
use super::{SelectScene, REFERENCE_BPMS};
use crate::app::{Ctx, Draw};
use crate::config::{Bpms, HiSpeedMode};
use crate::gfx::text::Align;
use crate::gfx::{Rect, TexId};
use crate::library::{BestScore, Song};
use crate::scene::skinvals::*;
use crate::skin::render::{Hooks, LoadedSkin, Xf};
use crate::skin::*;
use std::cell::OnceCell;
use vibebms_core::score::{ClearType, DjLevel};

/// vibeBMS select option ids (text value / name, click event).
pub const OPTION_TEXT: i32 = 9000;
pub const OPTION_NAME: i32 = 9100;
pub const OPTION_EVENT: i32 = 9000;
/// Click event: leave the current folder.
pub const EVENT_BACK: i32 = 9900;
/// vibeBMS: watch the chosen chart's replay (its best play's, else its
/// latest).
pub const EVENT_REPLAY: i32 = 9901;

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
    table: OnceCell<Option<(String, String)>>,
    clock: Clock,
}

/// What a bar shows.
pub struct BarInfo {
    pub kind: usize,
    pub title: String,
    pub level: Option<i64>,
    pub difficulty: usize,
    pub lamp: Option<ClearType>,
}

impl<'a> SelectState<'a> {
    pub fn new(s: &'a SelectScene, ctx: &'a Ctx) -> SelectState<'a> {
        let bar = s.bars.get(s.cursor);
        let song = match bar {
            Some(Bar::Song(i)) => ctx.lib.songs.get(*i),
            _ => None,
        };
        SelectState {
            s,
            ctx,
            bar,
            song,
            best: song.and_then(|s| ctx.best(s, ctx.config.play.ln_mode)),
            chart: song.map(ChartFacts::of_song),
            bpms: song.map_or(REFERENCE_BPMS, Bpms::of_song),
            rank: song.and_then(|x| s.ranking.get(&x.sha256)),
            table: OnceCell::new(),
            clock: Clock::now(),
        }
    }

    /// IR ranking of the selected chart, once loaded.
    fn ranking(&self) -> Option<&[vibebms_ir::IrScore]> {
        match self.rank? {
            Rank::Done(v) => Some(v),
            _ => None,
        }
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
                let s = ctx.lib.songs.get(*k)?;
                BarInfo {
                    kind: BAR_SONG,
                    title: s.full_title(),
                    level: Some(s.level_num()),
                    difficulty: s.difficulty.clamp(0, 6) as usize,
                    lamp: Some(
                        ctx.best(s, ctx.config.play.ln_mode)
                            .map(|b| b.clear)
                            .unwrap_or(ClearType::NoPlay),
                    ),
                }
            }
            Bar::Missing(m) => BarInfo {
                kind: BAR_NOSONG,
                title: m.title.clone(),
                // the table's level ("★12" -> 12)
                level: m
                    .level
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok(),
                difficulty: 0,
                lamp: None,
            },
            Bar::Course { table: t, course } => BarInfo {
                kind: BAR_GRADE,
                title: table(ctx, t)
                    .and_then(|t| t.header.courses.get(*course))
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                level: None,
                difficulty: 0,
                lamp: None,
            },
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
            3 => matches!(self.bar, Some(Bar::Course { .. })),
            5 => song.is_some(),
            32 | 33 | 40..=43 | 1046 => return play_option(p.autoplay, p.bga, p.gauge, id),
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
            21..=26 => s.panel_on[(id - 21) as usize]
                .filter(|_| s.panel == (id - 20) as usize)
                .map(|t| s.host.ms(t)),
            31..=36 => s.panel_off[(id - 31) as usize]
                .filter(|_| s.panel != (id - 30) as usize)
                .map(|t| s.host.ms(t)),
            _ => s.host.timer(id),
        }
    }

    fn option(&self, id: i32) -> bool {
        let v = match id.abs() {
            21..=26 => self.s.panel == (id.abs() - 20) as usize,
            n @ 1500..=1819 => {
                crate::scene::option_panel::option(&self.ctx.config.play, n).unwrap_or(false)
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
            71 | 75..=84 | 102 | 103 if song.is_some() => return best_number(self.best, id),
            370 => return best_number(self.best, id),
            72 => song?.notes * 2,
            74 => song?.notes,
            90..=92 | 96 | 400 => return chart_number(self.chart.as_ref()?, id),
            300 => match self.bar {
                Some(Bar::Folder { count, .. }) => *count as i64,
                _ => self.s.bars.len() as i64,
            },
            10 | 310..=313 => return speed_number(p, &self.bpms, id),
            180 => self.ranking()?.len() as i64,
            380..=389 => self.ranking()?.get((id - 380) as usize)?.ex_score as i64,
            390..=399 => self.ranking()?.get((id - 390) as usize)?.clear as i64,
            350..=368 => return song_number(song?, id),
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
        config_button(self.ctx, id).or_else(|| self.number(id))
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
                o.value(self.ctx, song.map(Bpms::of_song))
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
        size,
        [0.0, 0.0, 0.0, 0.8],
        Align::Right,
    );
    d.text(&label, x, y, size, [0.92, 0.96, 1.0, 1.0], Align::Right);
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
            if let Some(body) = sl
                .body
                .get(info.kind)
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
            // a table song downloading: how far it has got, over its bar
            if let Some(Bar::Missing(m)) = s.bars.get(idx) {
                if let Some(p) = self.st.ctx.song_fetch.progress(&m.title, &m.artist) {
                    download_progress(d, sr, p, now);
                }
            }
        }
    }
}
