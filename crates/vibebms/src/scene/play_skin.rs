//! Drawing the play scene with an LR2 skin.

use super::play::PlayScene;
use crate::app::{Ctx, Draw};
use crate::config::HiSpeedMode;
use crate::gfx::{Gfx, Rect, TexId};
use crate::platform::Instant;
use crate::skin::render::{Hooks, LoadedSkin, Xf};
use crate::skin::{SkinState, SkinType, OFFSET_HIDDEN_COVER, OFFSET_LANECOVER, OFFSET_LIFT};
use std::path::PathBuf;
use vibebms_core::chart::NoteKind;
use vibebms_core::gauge::GaugeType;
use vibebms_core::judge::*;
use vibebms_core::PlayMode;

pub fn skin_type(mode: PlayMode) -> SkinType {
    match mode {
        PlayMode::Beat5K => SkinType::Play5,
        PlayMode::Beat7K => SkinType::Play7,
        PlayMode::Beat10K => SkinType::Play10,
        PlayMode::Beat14K => SkinType::Play14,
        PlayMode::Popn9K => SkinType::Play9,
    }
}

/// LR2 timer lane index (0 = scratch, 1.. = keys, +10 for 2P) -> our lane.
fn lane_from_lr2(mode: PlayMode, idx: i32) -> Option<usize> {
    let (p, k) = ((idx / 10) as usize, (idx % 10) as usize);
    if p >= mode.players() {
        return None;
    }
    if k == 0 {
        mode.scratch_lane(p)
    } else {
        (k - 1 < mode.keys_per_player()).then(|| p * mode.lanes_per_player() + k - 1)
    }
}

/// The built-in default skin for a mode, side and screen orientation.
pub fn default_skin(mode: PlayMode, p2: bool, portrait: bool) -> PathBuf {
    let name = match mode {
        PlayMode::Beat14K => "play14".to_string(),
        PlayMode::Beat10K => "play10".to_string(),
        // 9 buttons have no scratch, so one centred layout serves both sides
        m => format!(
            "play{}{}{}",
            m.keys_per_player(),
            if portrait { "_portrait" } else { "" },
            if p2 && m.has_scratch() { "_2p" } else { "" }
        ),
    };
    PathBuf::from(crate::platform::vfs::BUILTIN)
        .join("skins/default")
        .join(name + ".json")
}

/// Resolve a `config.skins` value: empty = the built-in default; anything
/// else is a skin file relative to the data root (or absolute).
pub fn configured_path(ctx: &Ctx, value: &str) -> Option<PathBuf> {
    // "classic" was the removed code-drawn view: use the default
    if value.is_empty() || value == "classic" {
        return None;
    }
    let p = PathBuf::from(value);
    Some(if p.is_absolute() {
        p
    } else {
        ctx.paths.root.join(p)
    })
}

/// Load the configured skin for this chart's mode, falling back to the
/// built-in default when it is missing, broken or of another type.
pub fn load_play_skin(ctx: &Ctx, gfx: &mut Gfx, scene: &PlayScene) -> Option<LoadedSkin> {
    let mode = scene.mode();
    let default = default_skin(
        mode,
        ctx.config.play.side == crate::config::Side::P2,
        ctx.height > ctx.width,
    );
    let value = ctx
        .config
        .skins
        .get(mode.label())
        .cloned()
        .unwrap_or_default();
    let configured = configured_path(ctx, &value);
    let st = SceneState {
        s: scene,
        ctx,
        now: Instant::now(),
    };
    let stat = |id: i32| st.static_option(id);
    for path in configured.into_iter().chain(std::iter::once(default)) {
        let opts = ctx.skin_user_options(&value, &path);
        match crate::skin::load_file(
            &path,
            &ctx.paths.skins,
            &opts,
            &ctx.skin_user_offsets(&value),
            &stat,
            &crate::skin::image_size,
        ) {
            Ok(skin) if skin.kind == skin_type(mode) => return Some(LoadedSkin::load(skin, gfx)),
            Ok(_) => log::warn!("{}: not a {} skin", path.display(), mode.label()),
            Err(e) => log::warn!("skin: {e}"),
        }
    }
    None
}

pub struct SceneState<'a> {
    pub s: &'a PlayScene,
    pub ctx: &'a Ctx,
    pub now: Instant,
}

impl SceneState<'_> {
    /// Pacemaker target: the personal best EX score (0 if never played)
    /// and that score prorated to the notes passed so far.
    fn target(&self) -> (i64, f64) {
        let s = self.s;
        let best = self
            .ctx
            .best(&s.req.song, s.ln_mode() as i32)
            .map(|b| b.ex_score as i64)
            .unwrap_or(0);
        let now = match self.engine().map(|e| &e.score) {
            Some(sc) if sc.total_notes > 0 => {
                best as f64 * sc.passed as f64 / sc.total_notes as f64
            }
            _ => 0.0,
        };
        (best, now)
    }

    fn ms(&self, t: Instant) -> i64 {
        let base = self.s.scene_start;
        if t >= base {
            t.duration_since(base).as_millis() as i64
        } else {
            -(base.duration_since(t).as_millis() as i64)
        }
    }

    /// Options that do not change during play (also used at load time).
    fn static_option(&self, id: i32) -> Option<bool> {
        let s = self.s;
        let m = &s.chart.meta;
        let p = &self.ctx.config.play;
        let mode = s.mode();
        Some(match id {
            30 => true,
            31 => false,
            32 => !s.req.autoplay,
            33 => s.req.autoplay,
            34 => true,
            35..=37 => false,
            // score graph (pacemaker) shown
            38 => false,
            39 => true,
            40 => !p.bga,
            41 => p.bga,
            42 => !p.gauge.is_survival(),
            43 => p.gauge.is_survival(),
            1046 => matches!(p.gauge, GaugeType::ExHard | GaugeType::ExHardClass),
            50 => true,
            51 => false,
            // the song's lamp so far (skins pick images by it at load time)
            100..=105 | 1100..=1104 => {
                let ln = if m.has_long { s.ln as i32 } else { 0 };
                let c = self
                    .ctx
                    .best(&s.req.song, ln)
                    .map(|b| b.clear)
                    .unwrap_or_default();
                return super::skinned::lamp_option(c, id);
            }
            150..=155 => m.difficulty == id - 150,
            160 => mode == PlayMode::Beat7K,
            161 => mode == PlayMode::Beat5K,
            162 => mode == PlayMode::Beat14K,
            163 => mode == PlayMode::Beat10K,
            164 => mode == PlayMode::Popn9K,
            170 => !m.has_bga,
            171 => m.has_bga,
            172 => !m.has_long,
            173 => m.has_long,
            176 => (m.max_bpm - m.min_bpm).abs() < 0.01,
            177 => (m.max_bpm - m.min_bpm).abs() >= 0.01,
            178 => !m.has_random,
            179 => m.has_random,
            180..=184 => {
                matches!(m.judge_rank, vibebms_core::chart::JudgeRank::Rank(r) if r == id - 180)
            }
            190 => m.stagefile.is_empty(),
            191 => !m.stagefile.is_empty(),
            192 => m.banner.is_empty(),
            193 => !m.banner.is_empty(),
            194 => m.backbmp.is_empty(),
            195 => !m.backbmp.is_empty(),
            271 => p.lane_cover || p.sudden > 0.0,
            272 => p.lift > 0.0,
            273 => p.hidden > 0.0,
            290 => s.req.course.is_some(),
            400 => p.hispeed_mode == HiSpeedMode::Constant,
            _ => return None,
        })
    }

    fn engine(&self) -> Option<&vibebms_core::engine::Engine> {
        self.s.engine.as_ref()
    }

    /// Scroll speed as a beatoraja-style hi-speed multiplier:
    /// 240000 / (BPM x ms a note spends crossing the whole lane). The BPM is
    /// the main BPM with floating hi-speed and the current one with constant.
    pub fn hispeed(&self) -> f64 {
        let s = self.s;
        let p = &self.ctx.config.play;
        let bpm = match p.hispeed_mode {
            HiSpeedMode::Constant => s
                .chart
                .timing
                .bpm_at_time(s.chart_us(self.now).max(0) as f64),
            HiSpeedMode::Floating => s.chart.meta.main_bpm,
        };
        let bpm = if bpm > 0.0 { bpm } else { 130.0 };
        let cover = if p.lane_cover || p.sudden > 0.0 {
            p.sudden
        } else {
            0.0
        };
        let visible = (1.0 - cover - p.lift).max(0.05) as f64;
        240_000.0 * visible / (bpm * p.duration_ms.max(1) as f64)
    }
}

impl SkinState for SceneState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        let s = self.s;
        let mode = s.mode();
        let lane_t = |base: i32, v: &[Option<Instant>]| -> Option<i64> {
            let side = (id - base) / 10;
            let lane = lane_from_lr2(mode, (id - base) % 10 + side * 10)?;
            v.get(lane).copied().flatten().map(|t| self.ms(t))
        };
        match id {
            0 => Some(0),
            1 => Some(0),
            40 => s.ready_at.map(|t| self.ms(t)),
            41 => s.ready_at.map(|_| self.ms(s.start)),
            46 | 47 | 446 | 447 => s.last_judge[(id % 2 == 1) as usize].map(|j| self.ms(j.2)),
            2 | 3 => s.ending_at().map(|t| self.ms(t)),
            44 => self
                .engine()
                .filter(|e| e.gauge.current().is_max())
                .map(|_| 0),
            48 | 143 => s.finished_instant.map(|t| self.ms(t)),
            50..=69 => lane_t(50, &s.bomb_at),
            70..=89 => {
                let lane = lane_from_lr2(mode, id - 70)?;
                self.engine()?.holding(lane).map(|_| 0)
            }
            100..=119 => {
                let v = lane_t(100, &s.key_flash)?;
                let off = s
                    .key_off
                    .get(lane_from_lr2(mode, id - 100)?)
                    .copied()
                    .flatten()
                    .map(|t| self.ms(t));
                (off.map(|o| o < v).unwrap_or(true)).then_some(v)
            }
            120..=139 => {
                let lane = lane_from_lr2(mode, id - 120)?;
                let off = s.key_off.get(lane).copied().flatten()?;
                let on = s.key_flash.get(lane).copied().flatten();
                (on.map(|o| o <= off).unwrap_or(true)).then(|| self.ms(off))
            }
            140 => {
                let t = s.chart_us(self.now) as f64;
                let tm = &s.chart.timing;
                let beat_t = tm.time_at_beat(tm.beat_at_time(t).floor());
                Some(self.ms(s.start) + beat_t / 1000)
            }
            _ => None,
        }
    }

    fn button(&self, id: i32) -> Option<i64> {
        let s = self.s;
        match id {
            // last judgement of a lane / of a side
            500..=519 => {
                let lane = lane_from_lr2(s.mode(), id - 500)?;
                s.lane_judge.get(lane).copied().flatten().map(|j| j as i64)
            }
            520 | 521 => s.last_judge[(id - 520) as usize].map(|j| j.0 as i64),
            _ => self.number(id),
        }
    }

    fn offset(&self, id: i32) -> Option<[f32; 6]> {
        let p = &self.ctx.config.play;
        // lane height in skin pixels (the first lane of the note field)
        let lane_h = self
            .s
            .skin
            .as_ref()?
            .skin
            .lanes
            .iter()
            .find_map(|l| l.rect)
            .map(|r| r.h)?;
        let span = lane_h * (1.0 - p.lift);
        Some(match id {
            OFFSET_LIFT => [0.0, p.lift * lane_h, 0.0, 0.0, 0.0, 0.0],
            OFFSET_LANECOVER if p.lane_cover || p.sudden > 0.0 => {
                [0.0, -p.sudden * span, 0.0, 0.0, 0.0, 0.0]
            }
            OFFSET_LANECOVER => [0.0; 6],
            OFFSET_HIDDEN_COVER if p.hidden > 0.0 => [0.0, p.hidden * span, 0.0, 0.0, 0.0, 0.0],
            OFFSET_HIDDEN_COVER => [0.0, 0.0, 0.0, 0.0, 0.0, -255.0],
            _ => return None,
        })
    }

    fn option(&self, id: i32) -> bool {
        let (abs, neg) = (id.abs(), id < 0);
        let v = self.static_option(abs).unwrap_or_else(|| {
            let Some(e) = self.engine() else { return false };
            let sc = &e.score;
            let lj = self.s.last_judge[0];
            match abs {
                80 => self.s.engine.is_none(),
                81 => self.s.engine.is_some(),
                200..=207 => {
                    vibebms_core::score::DjLevel::from_rate(sc.ex_score(), sc.passed * 2) as i32
                        == 7 - (abs - 200)
                }
                230..=240 => {
                    let g = e.gauge.value();
                    let b = ((g / 10.0).floor() as i32).min(10);
                    b == abs - 230
                }
                1240 => e.gauge.current().is_qualified(),
                241..=246 => lj.map(|j| j.0 == (abs - 241) as usize).unwrap_or(false),
                1242 => lj.map(|j| j.1 > 0).unwrap_or(false),
                1243 => lj.map(|j| j.1 < 0).unwrap_or(false),
                _ => false,
            }
        });
        v != neg
    }

    fn number(&self, id: i32) -> Option<i64> {
        if let Some(v) = super::skinned::clock_number(id) {
            return Some(v);
        }
        let s = self.s;
        let p = &self.ctx.config.play;
        let m = &s.chart.meta;
        let e = self.engine();
        let sc = e.map(|e| &e.score);
        let t_ms = s.chart_us(self.now).max(0) / 1000;
        let len_ms = s.chart.length() / 1000;
        let rate = |r: f64| (r * 10000.0).round() as i64;
        Some(match id {
            10 => (self.hispeed() * 100.0).round() as i64,
            310 => (self.hispeed() * 100.0).round() as i64 / 100,
            311 => (self.hispeed() * 100.0).round() as i64 % 100,
            12 => p.judge_offset_ms as i64,
            14 => (p.sudden * 1000.0) as i64,
            312 => p.duration_ms as i64,
            313 => p.duration_ms as i64 * 3 / 5,
            314 => (p.lift * 1000.0) as i64,
            315 => (p.hidden * 1000.0) as i64,
            20 => self.ctx.fps as i64,
            71 | 101 => sc?.ex_score() as i64,
            72 => sc?.max_ex() as i64,
            74 | 106 => sc?.total_notes as i64,
            75 | 105 => sc?.max_combo as i64,
            76 => sc?.miss_count() as i64,
            90 => m.max_bpm.round() as i64,
            91 => m.min_bpm.round() as i64,
            92 => m.main_bpm.round() as i64,
            96 => m.level.trim().parse::<f64>().ok()? as i64,
            100 => {
                let sc = sc?;
                let n = sc.total_notes.max(1) as i64;
                (sc.count(PGREAT) as i64 * 150_000
                    + sc.count(GREAT) as i64 * 100_000
                    + sc.count(GOOD) as i64 * 20_000)
                    / n
                    + sc.max_combo as i64 * 50_000 / n
            }
            102 => rate(sc?.current_rate()) / 100,
            103 => rate(sc?.current_rate()) % 100,
            104 => e?.combo() as i64,
            107 => e?.gauge.value() as i64,
            407 => ((e?.gauge.value() * 10.0) as i64) % 10,
            110 => sc?.count(PGREAT) as i64,
            111 => sc?.count(GREAT) as i64,
            112 => sc?.count(GOOD) as i64,
            113 => sc?.count(BAD) as i64,
            114 => sc?.count(POOR) as i64 + sc?.count(MISS) as i64,
            410..=419 => {
                let j = ((id - 410) / 2) as usize;
                sc?.counts[j][((id - 410) % 2) as usize] as i64
            }
            420 => sc?.count(MISS) as i64,
            425 => sc?.combo_breaks as i64,
            423 => sc?.fast() as i64,
            424 => sc?.slow() as i64,
            115 => rate(sc?.rate()) / 100,
            116 => rate(sc?.rate()) % 100,
            121 | 150 | 151 | 170 => self.target().0,
            // differences to the best so far (pacemaker)
            128 | 152 | 153 => sc?.ex_score() as i64 - self.target().1.round() as i64,
            172 => sc?.ex_score() as i64 - self.target().0,
            122 | 157 => {
                let max = sc?.max_ex().max(1) as f64;
                rate(self.target().0 as f64 / max) / 100
            }
            123 | 158 => {
                let max = sc?.max_ex().max(1) as f64;
                rate(self.target().0 as f64 / max) % 100
            }
            160 => s
                .chart
                .timing
                .bpm_at_time(s.chart_us(self.now).max(0) as f64)
                .round() as i64,
            161 => t_ms / 60000,
            162 => (t_ms / 1000) % 60,
            163 => (len_ms - t_ms).max(0) / 60000,
            164 => ((len_ms - t_ms).max(0) / 1000) % 60,
            1163 => len_ms / 60000,
            1164 => (len_ms / 1000) % 60,
            165 => 100,
            _ => return None,
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let m = &self.s.chart.meta;
        Some(match id {
            2 => self.ctx.config.player_name.clone(),
            10 => m.title.clone(),
            11 => m.subtitle.clone(),
            12 => format!("{} {}", m.title, m.subtitle).trim().to_string(),
            13 => m.genre.clone(),
            14 => m.artist.clone(),
            15 => m.subartist.clone(),
            16 => format!("{} {}", m.artist, m.subartist).trim().to_string(),
            _ => return None,
        })
    }

    fn rate(&self, id: i32) -> f32 {
        let s = self.s;
        let prog = (s.chart_us(self.now).max(0) as f32 / s.chart.length().max(1) as f32).min(1.0);
        let sc = self.engine().map(|e| &e.score);
        match id {
            6 | 101 => prog,
            4 => self.ctx.config.play.sudden,
            102 => 1.0,
            110 => sc.map(|s| s.current_rate() as f32).unwrap_or(0.0),
            111 => sc.map(|s| s.rate() as f32).unwrap_or(0.0),
            // best (= target) so far / overall, as a share of the maximum
            112..=115 => {
                let max = sc.map(|s| s.max_ex()).unwrap_or(0).max(1) as f32;
                let (best, now) = self.target();
                if id % 2 == 0 {
                    now as f32 / max
                } else {
                    best as f32 / max
                }
            }
            _ => 0.0,
        }
    }
}

pub struct SceneHooks<'a> {
    pub st: &'a SceneState<'a>,
}

impl Hooks for SceneHooks<'_> {
    fn graph(&mut self, d: &mut Draw, graph: &crate::skin::Graph, r: Rect, color: [f32; 4]) {
        use crate::skin::Graph;
        let s = self.st.s;
        let a = super::graphs::Area {
            r,
            color,
            reveal: 1.0,
        };
        match graph {
            Graph::Notes(g) => {
                let Some(e) = s.engine.as_ref() else {
                    return;
                };
                let cols = super::graphs::columns(e.notes(), s.mode, s.chart.length());
                // play position, once the chart runs
                let cursor = s
                    .ready_at
                    .map(|_| s.chart_us(self.st.now).max(0) as f64 / 1000.0);
                super::graphs::notes(d, &a, g, &cols[g.kind.min(2)], cursor);
            }
            Graph::Bpm(g) => {
                let log: Vec<(i64, f64)> = s
                    .chart
                    .timing
                    .bpm_changes()
                    .into_iter()
                    .map(|c| (c.time, c.bpm))
                    .collect();
                super::graphs::bpm(d, &a, g, &log, s.chart.length(), s.chart.meta.main_bpm);
            }
            Graph::Visualizer(g) => {
                let Some(e) = s.engine.as_ref() else {
                    return;
                };
                let recent: Vec<i64> = e.timing_log.iter().map(|t| t.1).collect();
                super::graphs::visualizer(d, &a, g, &recent, &e.windows().note);
            }
            Graph::Gauge(_) | Graph::Timing(_) => {}
        }
    }

    fn notes(&mut self, d: &mut Draw, skin: &LoadedSkin, xf: &Xf, now: i64, st: &dyn SkinState) {
        let s = self.st.s;
        let Some(engine) = s.engine.as_ref() else {
            return;
        };
        let p = &self.st.ctx.config.play;
        let t_vis = s.chart_us(self.st.now) + p.visual_offset_ms as i64 * 1000;
        let cur_pos = s.chart.timing.pos_at_time(t_vis as f64);
        let main_bpm = if s.chart.meta.main_bpm > 0.0 {
            s.chart.meta.main_bpm
        } else {
            130.0
        };
        let dur_us = p.duration_ms as f64 * 1000.0;
        let notes = engine.notes();
        let ident = crate::skin::Frame {
            rect: Rect::default(),
            color: [1.0; 4],
            angle: 0.0,
            blend: crate::gfx::Blend::Alpha,
            center: (0.5, 0.5),
        };
        for (lane, ls) in skin.skin.lanes.iter().enumerate() {
            let Some(lr) = ls.rect else { continue };
            let r = xf.rect(lr);
            let judge_y = r.bottom() - p.lift * r.h;
            let top = r.y
                + if p.lane_cover || p.sudden > 0.0 {
                    p.sudden * r.h
                } else {
                    0.0
                };
            let visible = (judge_y - top).max(1.0);
            let ppb = visible as f64 / (dur_us / 1_000_000.0 * main_bpm / 60.0);
            let y_of = |pos: f64, time: i64| -> f32 {
                match p.hispeed_mode {
                    HiSpeedMode::Floating => judge_y - ((pos - cur_pos) * ppb) as f32,
                    HiSpeedMode::Constant => {
                        judge_y - ((time - t_vis) as f64 / dur_us * visible as f64) as f32
                    }
                }
            };
            let note_h = |src: &crate::skin::Src| match ls.note_h {
                Some(h) => h * xf.sy,
                None => src.frames.first().map(|f| f[3] * xf.sy).unwrap_or(4.0),
            };
            // HIDDEN+ hides notes above the judge line
            let bottom = judge_y + 2.0 - p.hidden * (judge_y - top);
            d.b.set_clip(Some(Rect::new(r.x, top, r.w, (bottom - top).max(0.0))));
            let Some(list) = s.lane_notes.get(lane) else {
                continue;
            };
            for &ni in &list[s.lane_cursor.get(lane).copied().unwrap_or(0)..] {
                let n = &notes[ni];
                let cn = &s.chart.notes[n.chart_index];
                let y = y_of(cn.pos, n.time);
                if n.time > t_vis + 2_000_000 && y < r.y - r.h {
                    break;
                }
                match &cn.kind {
                    NoteKind::Long {
                        end_pos, end_time, ..
                    } => {
                        if n.is_done() {
                            continue;
                        }
                        let holding = engine.holding(lane) == Some(ni);
                        let ye = y_of(*end_pos, *end_time);
                        let yh = if holding { judge_y } else { y };
                        let body = if holding {
                            ls.ln_body_active.as_ref().or(ls.ln_body.as_ref())
                        } else {
                            ls.ln_body.as_ref()
                        };
                        if let Some(b) = body {
                            skin.blit(
                                d,
                                b,
                                now,
                                st,
                                Rect::new(r.x, ye, r.w, (yh - ye).max(0.0)),
                                &ident,
                            );
                        }
                        if let Some(e) = ls.ln_end.as_ref().or(ls.note.as_ref()) {
                            let h = note_h(e);
                            skin.blit(d, e, now, st, Rect::new(r.x, ye - h, r.w, h), &ident);
                        }
                        if let Some(h0) = ls.ln_start.as_ref().or(ls.note.as_ref()) {
                            let h = note_h(h0);
                            skin.blit(d, h0, now, st, Rect::new(r.x, yh - h, r.w, h), &ident);
                        }
                    }
                    NoteKind::Mine { .. } => {
                        if let Some(m) = ls.mine.as_ref() {
                            let h = note_h(m);
                            skin.blit(d, m, now, st, Rect::new(r.x, y - h, r.w, h), &ident);
                        }
                    }
                    _ => {
                        if n.is_done() {
                            continue;
                        }
                        if let Some(src) = ls.note.as_ref() {
                            let h = note_h(src);
                            skin.blit(d, src, now, st, Rect::new(r.x, y - h, r.w, h), &ident);
                        }
                    }
                }
            }
            d.b.set_clip(None);
        }
        // bar lines span each lane group
        for (src, dst) in &skin.skin.bar_lines {
            if let Some(fr) = dst.eval(now, st) {
                let field = xf.rect(fr.rect);
                let judge_y = field.bottom() - p.lift * field.h;
                let top = field.y + p.sudden * field.h;
                let ppb =
                    (judge_y - top).max(1.0) as f64 / (dur_us / 1_000_000.0 * main_bpm / 60.0);
                for bl in &s.chart.bar_lines {
                    let y = judge_y - ((bl.pos - cur_pos) * ppb) as f32;
                    if y > judge_y || y < top {
                        continue;
                    }
                    let h = src
                        .frames
                        .first()
                        .map(|f| f[3] * xf.sy)
                        .unwrap_or(1.0)
                        .max(1.0);
                    skin.blit(d, src, now, st, Rect::new(field.x, y - h, field.w, h), &fr);
                }
            }
        }
    }

    fn bga(&mut self, d: &mut Draw, rect: Rect) {
        let s = self.st.s;
        d.b.rect(rect, [0.0, 0.0, 0.0, 1.0]);
        if !self.st.ctx.config.play.bga {
            return;
        }
        let layers = s.bga_textures(self.st.now);
        if layers.is_empty() && !s.chart.meta.has_bga {
            // charts without BGA show their stage file instead
            if let Some(t) = s.stage_tex {
                d.image_fit(t, rect, 0.8);
            }
        }
        for t in layers {
            d.image_fit(t, rect, 1.0);
        }
    }

    fn ref_image(&self, id: i32) -> Option<TexId> {
        match id {
            100 | 101 => self.st.s.stage_tex,
            _ => None,
        }
    }

    fn gauge(&self) -> (f32, f32, bool, usize) {
        match self.st.s.engine.as_ref() {
            Some(e) => {
                let g = e.gauge.current();
                (g.value(), g.elem.border, g.ty.is_survival(), g.ty as usize)
            }
            None => (0.0, 80.0, false, 2),
        }
    }

    fn judge(&self, side: usize) -> Option<(usize, u32)> {
        self.st.s.last_judge[side.min(1)].map(|j| (j.0, j.3))
    }
}
