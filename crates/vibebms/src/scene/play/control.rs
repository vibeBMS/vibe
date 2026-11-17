//! START / SELECT while playing: the hi-speed and lane cover panel, quitting.

use super::PlayScene;
use crate::app::Ctx;
use crate::config::{Bpms, PlayConfig, HISPEED_STEP};
use crate::input::{LaneEv, LaneMapper};
use crate::platform::{Duration, Instant};
use crate::touch;
use vibebms_core::PlayMode;

/// beatoraja's controls while playing: with START held the white keys
/// lower the hi-speed and the blue keys raise it (0.25 steps), the
/// turntable moves the lane cover (LIFT when no cover is shown); with
/// SELECT held the keys and turntable change the hi-speed by 0.01. START twice shows / hides the
/// cover; START and SELECT held for a second end the play.
#[derive(Default)]
pub(super) struct Control {
    pub start: bool,
    pub select: bool,
    last_start: Option<Instant>,
    pub both_since: Option<Instant>,
    /// Turntable held with the panel open: direction and last step.
    scratch: Option<(i32, Instant)>,
    /// SUDDEN+ to bring back with the cover.
    stash: f32,
    /// When the panel opened / last closed (skin timers 21 / 31).
    pub opened: Option<Instant>,
    pub closed: Option<Instant>,
}

impl Control {
    pub fn open(&self) -> bool {
        self.start || self.select
    }

    /// One turntable step in `dir` (START: cover, SELECT: hi-speed).
    fn turn(&self, p: &mut PlayConfig, dir: i32, bpm: &Bpms) {
        if self.start {
            adjust_cover(p, dir as f32 * 0.005);
        } else {
            p.add_hispeed(dir as f32 * 0.01, bpm);
        }
    }
}

/// The lane cover when it shows (or there is no LIFT), else the LIFT.
pub(super) fn adjust_cover(p: &mut PlayConfig, d: f32) {
    if p.lane_cover || p.sudden > 0.0 || p.lift <= 0.0 {
        p.sudden = (p.sudden + d).clamp(0.0, 0.8);
    } else {
        p.lift = (p.lift - d).clamp(0.0, 0.6);
    }
}

/// What reaches the judges and the panels.
#[derive(Default)]
pub(super) struct Controls {
    /// START / SELECT: the hi-speed panel (VS: each player's).
    pub panel: [Control; 2],
    pub mapper: Option<LaneMapper>,
    /// VS: the first two controllers swapped (the 1P controller pressed
    /// while loading was the second one).
    pub swap_pads: bool,
    /// START + SELECT held for a second, or Esc.
    pub quit: bool,
    /// A key went to the judge (a play quit before any did is not one).
    pub touched: bool,
}

impl Controls {
    /// A turntable held with a panel open keeps turning its value; START
    /// and SELECT held for a second (either player's) quit.
    pub fn tick(&mut self, play: &mut PlayConfig, bpm: &Bpms, now: Instant) {
        for (side, c) in self.panel.iter_mut().enumerate() {
            if let Some((dir, last)) = c.scratch.filter(|_| c.open()) {
                if now.duration_since(last) >= Duration::from_millis(50) {
                    let mut p = play.for_side(side);
                    c.turn(&mut p, dir, bpm);
                    c.scratch = Some((dir, now));
                    play.set_side(side, &p);
                }
            }
        }
        if self.panel.iter().any(|c| {
            c.both_since
                .is_some_and(|t| now.duration_since(t) >= Duration::from_secs(1))
        }) {
            self.quit = true;
        }
    }
}

impl PlayScene {
    /// The player side a key or button belongs to (VS: each player's
    /// START / SELECT and keys; else the one player's).
    fn control_side(&self, lane: u8) -> usize {
        if !self.session.vs {
            return 0;
        }
        match lane {
            touch::START | touch::SELECT => 0,
            touch::START2 | touch::SELECT2 => 1,
            l => self.session.mode.player_of_lane(l as usize).min(1),
        }
    }

    /// START / SELECT and what they do to the keys: `true` when the event
    /// was taken (it does not reach the judge).
    pub(super) fn control(&mut self, ctx: &mut Ctx, e: &LaneEv) -> bool {
        // the second player's START / SELECT do nothing out of VS
        if !self.session.vs && matches!(e.lane, touch::START2 | touch::SELECT2) {
            return true;
        }
        let bpm = &self.bpms_at(e.time);
        let side = self.control_side(e.lane);
        // the side's own options (VS: the second player's)
        let mut p = ctx.config.play.for_side(side);
        let taken = control(
            &mut self.controls.panel[side],
            self.session.mode,
            &mut p,
            bpm,
            e,
        );
        if taken {
            ctx.config.play.set_side(side, &p);
        }
        taken
    }
}

/// What START / SELECT and the keys do to `p` with them held (one player's
/// `Control`): `true` when the event was taken.
pub(super) fn control(
    c: &mut Control,
    mode: PlayMode,
    p: &mut PlayConfig,
    bpm: &Bpms,
    e: &LaneEv,
) -> bool {
    let was_open = c.open();
    match e.lane {
        touch::START | touch::START2 => {
            c.start = e.pressed;
            if e.pressed {
                let twice = c
                    .last_start
                    .is_some_and(|t| e.time.duration_since(t) < Duration::from_millis(500));
                if twice {
                    // show / hide the lane cover
                    if p.lane_cover || p.sudden > 0.0 {
                        c.stash = p.sudden;
                        p.lane_cover = false;
                        p.sudden = 0.0;
                    } else {
                        p.lane_cover = true;
                        p.sudden = if c.stash > 0.0 { c.stash } else { 0.2 };
                    }
                }
                c.last_start = (!twice).then_some(e.time);
            }
        }
        touch::SELECT | touch::SELECT2 => c.select = e.pressed,
        _ if !c.open() => return false,
        // releases go on to the judge (the key may have been down first)
        _ if !e.pressed => {
            if mode.is_scratch(e.lane as usize) {
                c.scratch = None;
            }
            return false;
        }
        lane if mode.is_scratch(lane as usize) => {
            let dir = if e.sub == 0 { -1 } else { 1 };
            c.turn(p, dir, bpm);
            c.scratch = Some((dir, e.time));
        }
        lane => {
            let blue = lane as usize % mode.lanes_per_player() % 2 == 1;
            // START: 0.25 steps, SELECT: 0.01; white slower, black faster
            let step = if c.start { HISPEED_STEP } else { 0.01 };
            p.add_hispeed(if blue { step } else { -step }, bpm);
        }
    }
    c.both_since = (c.start && c.select).then(|| c.both_since.unwrap_or(e.time));
    match (was_open, c.open()) {
        (false, true) => (c.opened, c.closed) = (Some(e.time), None),
        (true, false) => (c.opened, c.closed) = (None, Some(e.time)),
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(lane: u8, sub: u8, pressed: bool, time: Instant) -> LaneEv {
        LaneEv {
            lane,
            sub,
            pressed,
            time,
        }
    }

    #[test]
    fn start_twice_shows_and_hides_the_cover() {
        let (mode, bpm) = (PlayMode::Beat7K, Bpms::flat(150.0));
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut c = Control::default();
        let mut p = PlayConfig::default();
        let press = |c: &mut Control, p: &mut PlayConfig, at| {
            assert!(control(c, mode, p, &bpm, &ev(touch::START, 0, true, at)));
            assert!(control(c, mode, p, &bpm, &ev(touch::START, 0, false, at)));
        };
        press(&mut c, &mut p, ms(0));
        assert!(!p.lane_cover);
        press(&mut c, &mut p, ms(300));
        assert!(p.lane_cover && p.sudden == 0.2);
        // (the panel opened and closed with START)
        assert_eq!((c.opened, c.closed), (None, Some(ms(300))));
        p.sudden = 0.35;
        press(&mut c, &mut p, ms(1000));
        press(&mut c, &mut p, ms(1200));
        assert!(!p.lane_cover && p.sudden == 0.0);
        // back as it was hidden
        press(&mut c, &mut p, ms(2000));
        press(&mut c, &mut p, ms(2400));
        assert!(p.lane_cover && p.sudden == 0.35);
        // too slow for twice
        press(&mut c, &mut p, ms(3000));
        press(&mut c, &mut p, ms(3600));
        assert!(p.lane_cover);
    }

    #[test]
    fn the_turntable_keeps_turning_and_start_select_quit() {
        let (mode, bpm) = (PlayMode::Beat7K, Bpms::flat(150.0));
        let scratch = mode.scratch_lane(0).unwrap() as u8;
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut cs = Controls::default();
        let mut p = PlayConfig::default();
        let c = &mut cs.panel[0];
        // START held: the turntable moves the cover, a key goes on
        assert!(control(
            c,
            mode,
            &mut p,
            &bpm,
            &ev(touch::START, 0, true, ms(0))
        ));
        assert!(control(
            c,
            mode,
            &mut p,
            &bpm,
            &ev(scratch, 1, true, ms(10))
        ));
        assert!((p.sudden - 0.005).abs() < 1e-6);
        assert!(!control(
            c,
            mode,
            &mut p,
            &bpm,
            &ev(scratch, 1, false, ms(20))
        ));
        assert!(control(
            c,
            mode,
            &mut p,
            &bpm,
            &ev(scratch, 1, true, ms(30))
        ));
        // held: a step every 50 ms
        cs.tick(&mut p, &bpm, ms(60));
        cs.tick(&mut p, &bpm, ms(80));
        assert!((p.sudden - 0.015).abs() < 1e-6);
        assert!(!cs.quit);
        // START + SELECT for a second
        let c = &mut cs.panel[0];
        assert!(control(
            c,
            mode,
            &mut p,
            &bpm,
            &ev(touch::SELECT, 0, true, ms(100))
        ));
        cs.tick(&mut p, &bpm, ms(1000));
        assert!(!cs.quit);
        cs.tick(&mut p, &bpm, ms(1100));
        assert!(cs.quit);
    }
}
