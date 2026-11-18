//! A play made from its loaded chart: the rules and judges of each side,
//! what is recorded, the sounds, the BGA, the skin.

use super::bga::Bga;
use super::loading::Loaded;
use super::session::{Feed, Role, Session, SideState};
use super::skin::ghost_ex;
use super::sound::Audio;
use super::{battle_skin_fits, single_mode, Facts, PlayScene, Stage, Versus};
use crate::app::Ctx;
use crate::gfx::Gfx;
use crate::input::LaneMapper;
use crate::platform::{Duration, Instant};
use crate::replay::Slot;
use vibebms_core::chart::NO_SOUND;
use vibebms_core::play::build_engine;
use vibebms_core::record::{ChartFacts, RuleFacts};

impl PlayScene {
    pub(super) fn setup(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, l: Box<Loaded>) {
        let Loaded {
            chart,
            bank,
            images,
            videos,
            stagefile,
            backbmp,
            loudness,
            seed,
            mut facts,
            ln_profile,
            nonstop,
            section,
        } = *l;
        crate::loudness::start_play(ctx, &self.req.song.sha256, loudness);
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
        // the chart's own mode (BATTLE, VS, G-BATTLE: the single play's)
        let own_mode = match facts.battle || ghost.is_some() {
            true => single_mode(mode),
            false => mode,
        };
        // VS: each side is a single play (with its own lane option)
        if vs {
            facts.battle = false;
            facts.lane.truncate(1);
        }
        // a NONSTOP course is recorded as a song is, under its identity;
        // an expert or class course's stages each partly, then the course
        let practice = self.req.practice.as_ref();
        let unranked = course
            .filter(|c| !c.nonstop())
            .map(|_| "COURSE")
            .or(self.req.replay.as_ref().map(|_| "REPLAY"))
            .or(vs.then_some("VS"))
            .or(practice.map(|_| "PRACTICE"));
        // a course's rules (a replay's play options have them), practice's
        let rules = match (course, &self.req.replay, practice) {
            (Some(c), None, _) => c.rules(&p.rules),
            (_, _, Some(run)) => run.rules(&p.rules),
            _ => p.rules.clone(),
        };
        let ln = rules.ln_mode;
        let score_ln = ln_profile.score_ln(ln);
        // AUTO SCRATCH is an assist when it plays scored notes
        facts.auto_scratch = p.auto_scratch
            && chart
                .notes
                .iter()
                .any(|n| n.is_playable() && mode.is_scratch(n.lane as usize));
        // G-BATTLE, VS: the player judges the first side alone
        let side = (ghost.is_some() || vs).then_some(0);
        let (mut engine, total) = match (practice, &section) {
            (Some(run), Some(s)) => (
                vibebms_core::practice::engine(&chart, s, &rules, run.gauge_start),
                s.total,
            ),
            _ => build_engine(&chart, &rules, side, self.req.autoplay),
        };
        if let Some(ns) = &nonstop {
            engine.switch_rules(ns.rule_switches(rules.judge_preset, mode.family()));
        }
        let chart_facts = ChartFacts::new(&chart, own_mode, ln_profile, total);
        let rule_facts = RuleFacts::new(&rules, &engine, &chart_facts, facts, seed);
        // an expert or class course's later stage: the gauges as the last
        // left them (a replay of one: as they were given)
        let carried = match &self.req.replay {
            Some(r) => r.course.as_ref().and_then(|c| c.gauge.clone()),
            None => course.and_then(|c| c.gauge_values.clone()),
        };
        if let Some(v) = &carried {
            engine.gauge.set_values(v);
        }
        if keeps_replay(&self.req) {
            engine.record();
        }
        // the best play's ghost: MY BEST's pacemaker at the same note
        let best_ex = match (&self.req.replay, practice) {
            (Some(_), _) | (_, Some(_)) => None,
            _ => crate::replay::load_filed(
                &ctx.paths.root,
                &self.req.replay_key(),
                score_ln as i32,
                rules.judge_preset,
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
        engine.set_miss_sound(ctx.config.play.miss_keysound);
        let mut sides = vec![SideState::new(engine, role)];
        // VS: the second player with their own options
        if vs {
            let mut r = p.for_side(1).rules;
            r.ln_mode = ln;
            let (mut engine, _) = build_engine(&chart, &r, Some(1), self.req.autoplay);
            engine.set_miss_sound(ctx.config.play.miss_keysound);
            sides.push(SideState::new(engine, Role::Player));
        }
        // the opponent: its replay's own rules on the second side
        if let Some(g) = &ghost {
            let (engine, _) = build_engine(&chart, &g.play, Some(1), false);
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
        self.bga = Bga::new(gfx, images, &videos, stagefile, backbmp);
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
            unranked,
            options_label: opts.join(" / "),
            best_ex,
            chart: chart_facts,
            rules: rule_facts,
            seed,
            gauge_in: carried,
        };
        let nonstop = nonstop.map(std::sync::Arc::new);
        self.session = Session::new(chart, sides, vs, self.session.start);
        self.session.set_nonstop(nonstop);
        let cp = &ctx.config.play;
        self.session.fx = super::session::Fx {
            guide_se: cp.guide_se,
            // (a practice section ends where it ends)
            result_wait: cp.result_wait && practice.is_none(),
            good_clears_poor: cp.good_clears_poor,
        };
        if ctx.config.play.guide_se {
            let set = ctx.config.skins.get("SOUND_SET").map_or("", String::as_str);
            let rate = ctx.audio.mixer.sample_rate;
            self.guide = Some(super::guide::GuideSounds::load(set, &ctx.paths.root, rate));
        }
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
        if let (Some(_), Some(section)) = (practice, section) {
            // (its lead-in starts at 0, the music as it is there)
            self.audio.start_at(&ctx.audio.mixer, &self.session, 0);
            self.practice = Some(super::practice::Again::new(rules, section, lead));
        }
        self.skin_panel = self
            .skin
            .as_ref()
            .is_some_and(|sk| sk.skin.objects.iter().any(|o| o.dst.timer == 21));
        if let Some(sk) = &self.skin {
            log::info!("skin: {}", sk.skin.name);
        }
    }
}

/// Whether a play keeps its replay: one of the player's own (not watched,
/// autoplay, VS or practice).
pub(super) fn keeps_replay(req: &super::PlayRequest) -> bool {
    req.replay.is_none() && !req.autoplay && !req.is_vs() && req.practice.is_none()
}
