//! The window's events as the scenes' input ([`InputEv`]).

use super::{surface_size, Running};
use crate::input::{InputEv, Phase};
use crate::platform::{Duration, Instant};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};

impl Running {
    /// A touch came just before `now` (the mouse events a touch makes are
    /// not the mouse's).
    fn touched_since(&self, now: Instant) -> bool {
        self.last_touch
            .is_some_and(|t| now.duration_since(t) < Duration::from_millis(600))
    }

    pub(super) fn window_event(&mut self, el: &ActiveEventLoop, event: WindowEvent) {
        // (the web: when the browser got the event, not when this runs)
        #[cfg(target_arch = "wasm32")]
        let now = crate::web::input_time();
        #[cfg(not(target_arch = "wasm32"))]
        let now = Instant::now();
        match event {
            WindowEvent::CloseRequested => {
                self.ctx.save_config();
                el.exit();
            }
            WindowEvent::Resized(_) => {
                let (w, h) = surface_size(&self.window);
                self.gfx.resize(w, h);
                self.ctx.view.width = w as f32;
                self.ctx.view.height = h as f32;
                self.ctx.view.scale = self.window.scale_factor() as f32;
                self.ctx.view.screen = crate::platform::screen::detect(&self.window);
                self.display_changed();
            }
            WindowEvent::Moved(_) => self.display_changed(),
            WindowEvent::RedrawRequested => {
                self.redraw_pending = false;
                self.frame(el)
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    // an on-screen keyboard's characters (iOS) have no key
                    if event.state == ElementState::Pressed {
                        if let Some(t) = event.text.as_ref().filter(|t| !t.is_empty()) {
                            self.dispatch(el, InputEv::Text(t.to_string()));
                        }
                    }
                    return;
                };
                let pressed = event.state == ElementState::Pressed;
                if pressed && !event.repeat && (code == KeyCode::F11) {
                    self.ctx.config.display.fullscreen = !self.ctx.config.display.fullscreen;
                    self.ctx.view.want_fullscreen = Some(self.ctx.config.display.fullscreen);
                    return;
                }
                let text = event.text.as_ref().map(|t| t.to_string());
                self.dispatch(
                    el,
                    InputEv::Key {
                        code,
                        pressed,
                        repeat: event.repeat,
                        time: now,
                        text,
                    },
                );
            }
            WindowEvent::Ime(winit::event::Ime::Commit(t)) if !t.is_empty() => {
                self.dispatch(el, InputEv::Text(t))
            }
            WindowEvent::CursorLeft { .. } => self.ctx.input.pointer = None,
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
                self.ctx.input.pointer = Some(self.cursor);
                if self.mouse_down && !self.touched_since(now) {
                    self.dispatch(
                        el,
                        InputEv::Pointer {
                            id: 0,
                            phase: Phase::Move,
                            x: self.cursor.0,
                            y: self.cursor.1,
                            time: now,
                            touch: false,
                        },
                    );
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if self.touched_since(now) {
                    return;
                }
                self.mouse_down = state == ElementState::Pressed;
                let phase = if self.mouse_down {
                    Phase::Down
                } else {
                    Phase::Up
                };
                self.dispatch(
                    el,
                    InputEv::Pointer {
                        id: 0,
                        phase,
                        x: self.cursor.0,
                        y: self.cursor.1,
                        time: now,
                        touch: false,
                    },
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.dispatch(el, InputEv::Wheel { dy });
            }
            // the page reads touches itself (`web::install_touch_input`)
            #[cfg(target_arch = "wasm32")]
            WindowEvent::Touch(_) => {}
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::Touch(t) => {
                self.ctx.input.touch_seen = true;
                self.last_touch = Some(now);
                use winit::event::TouchPhase;
                let phase = match t.phase {
                    TouchPhase::Started => Phase::Down,
                    TouchPhase::Moved => Phase::Move,
                    TouchPhase::Ended | TouchPhase::Cancelled => Phase::Up,
                };
                self.dispatch(
                    el,
                    InputEv::Pointer {
                        id: t.id + 1,
                        phase,
                        x: t.location.x as f32,
                        y: t.location.y as f32,
                        time: now,
                        touch: true,
                    },
                );
            }
            WindowEvent::DroppedFile(p) => self.dispatch(el, InputEv::Dropped(p)),
            WindowEvent::Focused(false) => {
                self.ctx.save_config();
                // its key and touch releases go elsewhere now
                self.dispatch(
                    el,
                    InputEv::Released {
                        time: now,
                        touch_only: false,
                    },
                );
            }
            WindowEvent::Occluded(true) => self.dispatch(
                el,
                InputEv::Released {
                    time: now,
                    touch_only: false,
                },
            ),
            _ => {}
        }
    }

    /// The page's touches (`web::install_touch_input`) since the last call.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn web_touches(&mut self, el: &ActiveEventLoop) {
        for t in crate::web::take_touches() {
            match t {
                crate::web::TouchIn::Point {
                    id,
                    phase,
                    x,
                    y,
                    time,
                } => {
                    self.ctx.input.touch_seen = true;
                    self.last_touch = Some(Instant::now());
                    if phase == Phase::Up {
                        self.touches.remove(&id);
                    } else {
                        self.touches.insert(id, (x, y));
                    }
                    self.dispatch(
                        el,
                        InputEv::Pointer {
                            id,
                            phase,
                            x,
                            y,
                            time,
                            touch: true,
                        },
                    );
                }
                // fingers held here that are no longer on the screen
                crate::web::TouchIn::Left(left) => {
                    let lost: Vec<(u64, (f32, f32))> = self
                        .touches
                        .iter()
                        .filter(|(id, _)| !left.contains(id))
                        .map(|(id, p)| (*id, *p))
                        .collect();
                    for (id, (x, y)) in lost {
                        self.touches.remove(&id);
                        self.dispatch(
                            el,
                            InputEv::Pointer {
                                id,
                                phase: Phase::Up,
                                x,
                                y,
                                time: Instant::now(),
                                touch: true,
                            },
                        );
                    }
                }
            }
        }
        // no finger on the screen: keys still held lost their release
        if let Some(time) = crate::web::take_touches_gone() {
            self.dispatch(
                el,
                InputEv::Released {
                    time,
                    touch_only: true,
                },
            );
        }
    }
}
