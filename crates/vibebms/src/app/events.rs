//! The window's events as the scenes' input ([`InputEv`]).

use super::activity::{Activity, Platform};
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
                self.ctx.streaming.stop();
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
                let text = event.text.as_ref().map(|t| t.to_string());
                self.key(
                    el,
                    InputEv::Key {
                        code,
                        pressed: event.state == ElementState::Pressed,
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
                // (iOS: when it happened; UIKit hands touches over once a frame)
                #[cfg(target_os = "ios")]
                let now = crate::platform::ios::touch_time(t.id, t.phase).unwrap_or(now);
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
            WindowEvent::Focused(focused) => {
                // (the web: the canvas' focus, not the page's: see
                // `web::install_page_focus`)
                #[cfg(not(target_arch = "wasm32"))]
                self.set_activity(|a| {
                    a.focused = focused;
                    a.minimized &= !focused;
                });
                #[cfg(target_os = "android")]
                if focused {
                    crate::platform::android::hide_system_bars();
                }
                if !focused {
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
            }
            WindowEvent::Occluded(occluded) => {
                self.set_activity(|a| a.occluded = occluded);
                if occluded {
                    self.dispatch(
                        el,
                        InputEv::Released {
                            time: now,
                            touch_only: false,
                        },
                    );
                }
            }
            _ => {}
        }
    }

    /// What the platform tells of the app's use changed by `f`: silent
    /// at once if it is not in use now; heard again at the next frame
    /// (`update_mute`), once the scenes have seen it.
    pub(super) fn set_activity(&mut self, f: impl FnOnce(&mut Activity)) {
        let before = self.activity;
        f(&mut self.activity);
        if self.activity == before {
            return;
        }
        log::debug!("activity: {:?}", self.activity);
        self.update_mute(false);
        if !self.redraw_pending {
            self.redraw();
        }
    }

    /// Silence the output while the app is not in use (`activity`); with
    /// `unsilence`, hear it again when it is.
    pub(super) fn update_mute(&mut self, unsilence: bool) {
        let in_play = self.scenes.last().is_some_and(|s| s.in_play());
        let enabled = self.ctx.config.audio.mute_inactive;
        if self.activity.silent(in_play, Platform::THIS, enabled) {
            self.ctx.audio.silence(Instant::now());
        } else if unsilence {
            self.ctx.audio.unsilence(self.ctx.config.audio.master);
        }
    }

    /// While the window has no focus, now and then: whether it is
    /// minimised (X11 and Wayland do not tell it as `Occluded`).
    #[cfg(desktop)]
    pub(super) fn check_minimized(&mut self, now: Instant) {
        if self.activity.focused || now < self.minimized_check {
            return;
        }
        self.minimized_check = now + Duration::from_millis(250);
        let m = self.window.is_minimized() == Some(true);
        self.set_activity(|a| a.minimized = m);
    }

    /// A key (`InputEv::Key`), or the window's own use of it.
    fn key(&mut self, el: &ActiveEventLoop, ev: InputEv) {
        match ev {
            // Print Screen (F13 on Apple keyboards): a screenshot. Windows
            // sends only its release.
            InputEv::Key {
                code: KeyCode::PrintScreen | KeyCode::F13,
                pressed: false,
                ..
            } => self.ctx.view.want_screenshot = true,
            InputEv::Key {
                code: KeyCode::F11,
                pressed: true,
                repeat: false,
                ..
            } => {
                self.ctx.config.display.fullscreen = !self.ctx.config.display.fullscreen;
                self.ctx.view.want_fullscreen = Some(self.ctx.config.display.fullscreen);
            }
            _ => self.dispatch(el, ev),
        }
    }

    /// The hardware keyboard's keys (`platform::ios`) since the last call.
    #[cfg(target_os = "ios")]
    pub(super) fn ios_keys(&mut self, el: &ActiveEventLoop) {
        for (code, pressed, time) in crate::platform::ios::take_keys() {
            // a text field's (the on-screen keyboard's) return and delete
            // come from winit too
            if self.ime_on
                && matches!(
                    code,
                    KeyCode::Backspace | KeyCode::Enter | KeyCode::NumpadEnter
                )
            {
                continue;
            }
            self.key(
                el,
                InputEv::Key {
                    code,
                    pressed,
                    repeat: false,
                    time,
                    text: None,
                },
            );
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
