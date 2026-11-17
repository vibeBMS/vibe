//! Keys, controllers and touches into the judges and the panels.

use super::control::adjust_cover;
use super::{PlayScene, Stage};
use crate::app::{Ctx, Next};
use crate::config::HISPEED_STEP;
use crate::input::{InputEv, LaneEv};
use crate::platform::Instant;
use vibebms_core::record::InputSource;
use winit::keyboard::KeyCode;

impl PlayScene {
    pub(super) fn on_input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        let mut lane_evs = Vec::new();
        // (releases that never came are no presses)
        let src = match ev {
            InputEv::Key { .. } => Some(InputSource::Keyboard),
            InputEv::Pad { .. } | InputEv::Axis { .. } => Some(InputSource::Gamepad),
            InputEv::Pointer { .. } => Some(InputSource::Touch),
            _ => None,
        };
        match ev {
            InputEv::Key {
                code,
                pressed,
                repeat,
                time,
                ..
            } => {
                if *repeat {
                    return Next::Stay;
                }
                if let Some(m) = self.controls.mapper.as_mut() {
                    if let Some((lane, sub)) = m.key(*code) {
                        let e = LaneEv {
                            lane,
                            sub,
                            pressed: *pressed,
                            time: *time,
                        };
                        if m.passes(&e) {
                            lane_evs.push(e);
                        } else {
                            // chatter: dropped (not an unbound key)
                            return Next::Stay;
                        }
                    }
                }
                if lane_evs.is_empty() && *pressed {
                    match code {
                        KeyCode::Escape => {
                            if matches!(self.stage, Stage::Loading(_)) {
                                return Next::Pop;
                            }
                            self.controls.quit = true;
                        }
                        KeyCode::ArrowUp | KeyCode::ArrowDown => {
                            let step = match code {
                                KeyCode::ArrowUp => HISPEED_STEP,
                                _ => -HISPEED_STEP,
                            };
                            let bpm = self.bpms_at(*time);
                            ctx.config.play.add_hispeed(step, &bpm);
                        }
                        KeyCode::PageUp => adjust_cover(&mut ctx.config.play, 0.02),
                        KeyCode::PageDown => adjust_cover(&mut ctx.config.play, -0.02),
                        KeyCode::ArrowLeft => ctx.config.play.judge_offset_ms -= 1,
                        KeyCode::ArrowRight => ctx.config.play.judge_offset_ms += 1,
                        _ => {}
                    }
                }
            }
            InputEv::Pad { id, pressed, time } => {
                // VS: a button pressed while loading picks the 1P controller
                let loading = matches!(self.stage, Stage::Loading(_));
                if self.session.vs && *pressed && loading && *id < 2_000_000 {
                    let swap = *id >= 1_000_000;
                    if swap != self.controls.swap_pads {
                        self.controls.swap_pads = swap;
                        ctx.toast(format!("1P: controller {}", swap as u8 + 1));
                    }
                    return Next::Stay;
                }
                if let Some(m) = self.controls.mapper.as_mut() {
                    if let Some((lane, sub)) = m.pad(*id) {
                        let e = LaneEv {
                            lane,
                            sub,
                            pressed: *pressed,
                            time: *time,
                        };
                        if m.passes(&e) {
                            lane_evs.push(e);
                        }
                    }
                }
            }
            InputEv::Axis { id, value, time } => {
                if let Some(m) = self.controls.mapper.as_mut() {
                    m.axis(*id, *value, *time, &mut lane_evs);
                }
            }
            InputEv::Pointer {
                id,
                phase,
                x,
                y,
                time,
                ..
            } => {
                let thr = ctx.config.touch.scratch_threshold_deg;
                if ctx.touch_enabled() {
                    let t = &mut self.touch;
                    t.input
                        .pointer(&t.zones, *id, *phase, *x, *y, *time, thr, &mut lane_evs);
                }
            }
            // releases that never came: let go of what is held
            InputEv::Released { time, touch_only } => {
                self.touch.input.release_all(*time, &mut lane_evs);
                if !touch_only {
                    if let Some(m) = self.controls.mapper.as_mut() {
                        m.released_all();
                    }
                    let held: Vec<(u8, u8)> = self
                        .session
                        .sides
                        .iter()
                        .filter(|s| s.player())
                        .flat_map(|s| s.engine.held())
                        .collect();
                    for (lane, sub) in held {
                        if !lane_evs.iter().any(|e| e.lane == lane && e.sub == sub) {
                            lane_evs.push(LaneEv {
                                lane,
                                sub,
                                pressed: false,
                                time: *time,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        let mut fed = false;
        for e in lane_evs {
            if !matches!(self.stage, Stage::Loading(_)) && self.control(ctx, &e) {
                continue;
            }
            if matches!(self.stage, Stage::Playing) {
                self.feed(ctx, e, src);
                fed = true;
            }
        }
        if fed && self.req.replay.is_none() {
            // judge now and start the keys' sounds without waiting for the
            // next frame
            let t = self.session.chart_us(Instant::now());
            let offsets = self.offsets(ctx);
            self.session.judge_keys(t, offsets, &mut self.out);
            self.play_out(ctx);
        }
        Next::Stay
    }
}
