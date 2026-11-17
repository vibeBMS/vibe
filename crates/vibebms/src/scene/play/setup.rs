//! A play made from its loaded chart: the rules and judges of each side,
//! what is recorded, the sounds, the BGA, the skin.

use super::bga::Bga;
use super::chart::{make_engine, Rules};
use super::loading::Loaded;
use super::session::{Feed, Role, Session, SideState};
use super::skin::ghost_ex;
use super::sound::Audio;
use super::{battle_skin_fits, single_mode, Facts, PlayScene, Stage, Versus};
use crate::app::Ctx;
use crate::gfx::Gfx;
use crate::input::LaneMapper;
use crate::platform::{Duration, Instant};
use crate::replay::{Replay, Slot};
use vibebms_core::chart::NO_SOUND;
use vibebms_core::gauge::{GaugeAutoShift, GaugePreset};
use vibebms_core::LnType;

impl PlayScene {
    pub(super) fn setup(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, l: Box<Loaded>) {
        let Loaded {
            chart,
            bank,
            images,
            videos,
            stagefile,
            seed,
            mut facts,
            ln_profile,
        } = *l;
        let mode = chart.mode();
        let vs = self.req.is_vs();
        let ghost = match &self.req.vs {
            Some(Versus::Ghost(g, _)) => Some(g.clone()),
            _ => None,
        };
        // G-BATTLE: the player plays the first side as a single play
        let input_mode = match ghost {
            Some(_) => single_mode(mode),
            None => mode,
        };
        let skin_mode = match ghost {
            Some(_) if !battle_skin_fits(ctx) => input_mode,
            _ => mode,
        };
        let replay_play = self.req.replay.as_ref().map(|r| r.play.clone());
        let p = replay_play.as_ref().unwrap_or(&ctx.config.play);
        let course = self.req.course.as_ref();
        // beatoraja: windows wider than standard and a BPM guide on a chart
        // that changes BPM are assists (only the lamp is kept)
        facts.wide_judge = p.expand_judge && p.judge_rates.iter().any(|r| *r > 100);
        facts.bpm_guide = p.bpm_guide && chart.meta.min_bpm < chart.meta.max_bpm;
        // VS: each side is a single play (with its own lane option)
        if vs {
            facts.battle = false;
            facts.lane.truncate(1);
        }
        let unranked = course
            .map(|_| "COURSE")
            .or(self.req.replay.as_ref().map(|_| "REPLAY"))
            .or(vs.then_some("VS"));
        // LN mode: course constraints may force one
        let mut ln = p.ln_mode;
        if let Some(c) = course {
            if c.has("ln") {
                ln = LnType::Ln;
            } else if c.has("cn") {
                ln = LnType::Cn;
            } else if c.has("hcn") {
                ln = LnType::Hcn;
            }
        }
        let score_ln = ln_profile.score_ln(ln);
        let mut rules = Rules::of(p);
        rules.ln = ln;
        rules.autoplay = self.req.autoplay;
        if course.map(|c| c.has("gauge_lr2")).unwrap_or(false) {
            rules.gauge_preset = GaugePreset::Lr2;
        }
        if course.is_some() {
            rules.gauge = p.gauge.to_class();
            rules.gas = GaugeAutoShift::None;
        }
        // AUTO SCRATCH is an assist when it plays scored notes
        facts.auto_scratch = p.auto_scratch
            && chart
                .notes
                .iter()
                .any(|n| n.is_playable() && mode.is_scratch(n.lane as usize));
        // G-BATTLE, VS: the player judges the first side alone
        let side = (ghost.is_some() || vs).then_some(0);
        let (mut engine, total) = make_engine(&chart, &rules, side);
        if let Some(v) = course.and_then(|c| c.gauge_values.as_ref()) {
            engine.gauge.set_values(v);
        }
        // a play of its own (not a course stage): kept as a replay
        if self.req.replay.is_none() && !self.req.autoplay && course.is_none() && !vs {
            engine.record();
        }
        // the best play's ghost: MY BEST's pacemaker at the same note
        let best_ex = match self.req.replay {
            Some(_) => None,
            None => Replay::load_filed(
                &ctx.paths.root,
                &self.req.song.sha256,
                score_ln as i32,
                ln_profile.has_long() && !ln_profile.undefined,
                Slot::Best,
            )
            .filter(|r| r.keepable())
            .map(|r| ghost_ex(&r.ghost)),
        };
        let role = match self.req.replay.clone() {
            Some(r) => Role::Replay(Feed::new(r, 0)),
            None => Role::Player,
        };
        let mut sides = vec![SideState::new(engine, role)];
        // VS: the second player with their own options
        if vs {
            let mut r = Rules::of(&p.for_side(1));
            r.ln = ln;
            r.autoplay = self.req.autoplay;
            let (engine, _) = make_engine(&chart, &r, Some(1));
            sides.push(SideState::new(engine, Role::Player));
        }
        // the opponent: its replay's own rules on the second side
        if let Some(g) = &ghost {
            let mut r = Rules::of(&g.play);
            r.autoplay = false;
            let (engine, _) = make_engine(&chart, &r, Some(1));
            let lpp = input_mode.lanes_per_player() as u8;
            sides.push(SideState::new(
                engine,
                Role::Ghost {
                    feed: Feed::new(g.clone(), lpp),
                    desync: false,
                },
            ));
        }
        // keysounds of auto lanes are scheduled ahead like BGM for exact
        // timing (the players' lanes: an opponent is silent)
        let auto_notes = chart
            .notes
            .iter()
            .filter(|n| n.is_playable() && n.sound != NO_SOUND)
            .filter_map(|n| {
                let lane = n.lane as usize;
                let k = sides.iter().position(|s| {
                    !s.ghost() && s.engine.judges(n.lane) && s.engine.is_auto(lane)
                })?;
                Some((n.time, n.sound, k))
            })
            .collect();
        let mut mapper = LaneMapper::new(&ctx.config.keys, input_mode);
        mapper.swap_pads = self.controls.swap_pads;
        self.controls.mapper = Some(mapper);
        self.bga = Bga::new(gfx, images, &videos, stagefile);
        // (G-BATTLE: the opponent's lane option)
        let random_1p = ghost.as_ref().map_or(p.random_1p, |g| g.play.random_1p);
        let mut opts = vec![random_1p.label()];
        if input_mode.is_double() {
            opts.push(p.random_2p.label());
        }
        if ghost.is_some() {
            opts.push("G-BATTLE");
        }
        if vs {
            opts.push("VS");
        }
        if p.auto_scratch {
            opts.push("AUTO-SCR");
        }
        if p.legacy_note {
            opts.push("LEGACY");
        }
        // a song from an older library: its note counts and densities
        // (skin numbers 350-365) from the chart
        let song = &mut self.req.song;
        if song.note_kinds.iter().all(|n| *n == 0) {
            (song.note_kinds, song.density) = crate::library::chart_info(&chart);
        }
        self.facts = Facts {
            skin_mode,
            input_mode,
            score_ln,
            total,
            seed,
            assist: facts.level(),
            unranked,
            options_label: opts.join(" / "),
            best_ex,
        };
        self.session = Session::new(chart, sides, vs, self.session.start);
        self.audio = Audio::new(bank, auto_notes);
        self.skin = super::skin::load(ctx, gfx, self);
        // start: a lead-in so the first notes scroll in (the skin's
        // #PLAYSTART, else 1.5 s)
        let lead_ms = self.skin.as_ref().and_then(|s| s.skin.play_start_ms);
        let lead = Duration::from_millis(lead_ms.unwrap_or(1500) as u64);
        let now = Instant::now();
        self.session.start = now + lead;
        self.session.ready_at = Some(now);
        self.stage = Stage::Playing;
        self.skin_panel = self
            .skin
            .as_ref()
            .is_some_and(|sk| sk.skin.objects.iter().any(|o| o.dst.timer == 21));
        if let Some(sk) = &self.skin {
            log::info!("skin: {}", sk.skin.name);
        }
    }
}
