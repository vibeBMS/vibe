//! Keys, controllers, the turntable and touches on the select screen.

use super::options::SelectOption;
use super::tree::Node;
use super::SelectScene;
use crate::app::{Ctx, Next};
use crate::config::{KeyConfig, LaneKeys};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase, Repeat};
use crate::platform::{Duration, Instant};
use crate::ui::{TextEvent, TextInput, DRAG_SLOP};
use winit::keyboard::KeyCode;

/// A held up / down moves on after 300 ms, every 70 ms, every 25 ms once
/// held for 1.5 s.
const NAV_REPEAT_DELAY: Duration = Duration::from_millis(300);
const NAV_REPEAT_EVERY: Duration = Duration::from_millis(70);
const NAV_REPEAT_FAST: Duration = Duration::from_millis(25);
const NAV_FAST_AFTER: Duration = Duration::from_millis(1500);

/// Back pressed twice within this quits.
const QUIT_AGAIN: Duration = Duration::from_secs(2);

/// What a key does with a panel held: 1P key 1..7 (`Key(0..=6)`), or the
/// turntable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PanelKey {
    Key(usize),
    Up,
    Down,
}

impl PanelKey {
    /// The panel key of a binding `has` finds in the 1P keys.
    fn of(keys: &KeyConfig, has: impl Fn(&LaneKeys) -> bool) -> Option<PanelKey> {
        let p = &keys.p1;
        if let Some(i) = p.keys.iter().take(7).position(&has) {
            return Some(PanelKey::Key(i));
        }
        if has(&p.scratch_up) {
            return Some(PanelKey::Up);
        }
        has(&p.scratch_down).then_some(PanelKey::Down)
    }
}

/// A press on the list: the list follows it once it moves.
#[derive(Debug)]
pub(super) struct Drag {
    id: u64,
    y: f32,
    /// The list's position when it went down.
    scroll: f32,
    moved: bool,
}

impl SelectScene {
    /// Held keys that act again: up / down, and panel keys that repeat.
    pub(super) fn repeat_keys(&mut self, ctx: &mut Ctx, now: Instant) {
        self.repeat_panel_key(ctx, now);
        let Some(r) = self.held_nav.as_mut() else {
            return;
        };
        let every = if r.held(now) > NAV_FAST_AFTER {
            NAV_REPEAT_FAST
        } else {
            NAV_REPEAT_EVERY
        };
        if r.due(now, every) {
            let d = if r.key == Nav::Up { -1 } else { 1 };
            self.step(ctx, d);
        }
    }

    fn nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        match nav {
            Nav::Up => self.step(ctx, -1),
            Nav::Down => self.step(ctx, 1),
            Nav::Decide => return self.decide(ctx, false),
            Nav::Close | Nav::Back if self.panel > 0 => self.toggle_panel(0),
            Nav::Close => {
                self.back(ctx);
            }
            Nav::Back => {
                if !self.back(ctx) {
                    if self.quit_armed.is_some_and(|t| t.elapsed() < QUIT_AGAIN) {
                        return Next::Quit;
                    }
                    self.quit_armed = Some(Instant::now());
                    ctx.toast("Press back again to quit");
                }
            }
            Nav::Settings => {
                return Next::Push(Box::new(crate::scene::settings::SettingsScene::new(ctx)))
            }
            // skins with panels open the option panel, others cycle RANDOM
            Nav::Option if self.skin_has_panels() => self.toggle_panel(1),
            Nav::Option => return self.option(ctx, SelectOption::Random),
        }
        Next::Stay
    }

    /// The search box has the keys: on-screen keyboards (their return key
    /// searches) and touches (the box searches, anywhere else closes it)
    /// too.
    fn search_input(&mut self, ctx: &mut Ctx, ev: &InputEv) {
        let Some(t) = self.search_edit.as_mut() else {
            return;
        };
        match t.input(ev) {
            TextEvent::Editing => {}
            TextEvent::Cancel => self.search_edit = None,
            TextEvent::Commit(q) => {
                self.search_edit = None;
                if !q.trim().is_empty() {
                    self.enter(ctx, Node::Search(q.trim().to_string()));
                }
            }
        }
    }

    fn key(&mut self, ctx: &mut Ctx, code: KeyCode, pressed: bool, repeat: bool) -> Next {
        let keys = &ctx.config.keys;
        let (is_start, is_select) = (
            keys.start.keys.contains(&code),
            keys.select.keys.contains(&code),
        );
        if is_start || is_select {
            if !repeat {
                self.hold(is_start.then_some(pressed), is_select.then_some(pressed));
            }
            return Next::Stay;
        }
        if matches!(code, KeyCode::Digit5 | KeyCode::Numpad5) {
            if !repeat {
                self.num5_held = pressed;
                self.hold(None, None);
            }
            return Next::Stay;
        }
        if self.holding() {
            if let Some(k) = PanelKey::of(keys, |l| l.keys.contains(&code)) {
                if pressed && !repeat {
                    self.panel_input(ctx, k);
                } else if !pressed {
                    self.panel_release(ctx, k);
                }
            }
            return Next::Stay;
        }
        let nav = nav_from_key(code, keys);
        if !pressed {
            if self.held_nav.is_some_and(|r| Some(r.key) == nav) {
                self.held_nav = None;
            }
            return Next::Stay;
        }
        if repeat {
            return Next::Stay; // own repeat handling
        }
        match code {
            // beatoraja number keys: 3 LN mode, 6 key config (7 skin
            // select: beatoraja's F12 rescans here)
            // 1 key mode filter, 2 sort (as beatoraja), 4 difficulty filter
            KeyCode::Digit1 | KeyCode::Numpad1 => return self.option(ctx, SelectOption::KeyFilter),
            KeyCode::Digit2 | KeyCode::Numpad2 => return self.option(ctx, SelectOption::Sort),
            KeyCode::Digit4 | KeyCode::Numpad4 => {
                return self.option(ctx, SelectOption::DifficultyFilter)
            }
            KeyCode::Digit3 | KeyCode::Numpad3 => return self.option(ctx, SelectOption::LnMode),
            KeyCode::Digit6 | KeyCode::Numpad6 => return self.skin_event(ctx, 13),
            KeyCode::Digit7 | KeyCode::Numpad7 => return self.skin_event(ctx, 14),
            KeyCode::F2 => return self.option(ctx, SelectOption::Random),
            KeyCode::F3 => return self.option(ctx, SelectOption::Gauge),
            KeyCode::F4 => return self.option(ctx, SelectOption::LnMode),
            KeyCode::F5 => return self.decide(ctx, true),
            KeyCode::F6 => return self.option(ctx, SelectOption::Side),
            KeyCode::F7 => return self.option(ctx, SelectOption::HiSpeedDown),
            KeyCode::F8 => return self.option(ctx, SelectOption::HiSpeedUp),
            KeyCode::F9 => return self.option(ctx, SelectOption::Judge),
            KeyCode::F1 => return self.option(ctx, SelectOption::Battle),
            #[cfg(target_arch = "wasm32")]
            KeyCode::Delete => self.delete_kept(ctx),
            KeyCode::KeyP => return self.watch_replay(ctx),
            KeyCode::KeyC => self.add_to_course(ctx),
            KeyCode::F12 => {
                ctx.start_scan();
                ctx.tables.refresh(&ctx.config.tables, true);
            }
            KeyCode::Slash | KeyCode::F10 => self.search_edit = Some(TextInput::default()),
            KeyCode::PageUp => self.step(ctx, -10),
            KeyCode::PageDown => self.step(ctx, 10),
            _ => {
                if let Some(nav) = nav {
                    if matches!(nav, Nav::Up | Nav::Down) {
                        self.held_nav = Some(Repeat::new(nav, Instant::now(), NAV_REPEAT_DELAY));
                    }
                    return self.nav(ctx, nav);
                }
            }
        }
        Next::Stay
    }

    fn pad(&mut self, ctx: &mut Ctx, id: u64, pressed: bool) -> Next {
        let keys = &ctx.config.keys;
        let (is_start, is_select) = (keys.start.pad.contains(&id), keys.select.pad.contains(&id));
        if is_start || is_select {
            self.hold(is_start.then_some(pressed), is_select.then_some(pressed));
            return Next::Stay;
        }
        if self.holding() {
            if let Some(k) = PanelKey::of(keys, |l| l.pad.contains(&id)) {
                if pressed {
                    self.panel_input(ctx, k);
                } else {
                    self.panel_release(ctx, k);
                }
            }
            return Next::Stay;
        }
        match nav_from_pad(id, keys) {
            Some(nav) if pressed => self.nav(ctx, nav),
            _ => Next::Stay,
        }
    }

    /// A pointer on the list: a drag scrolls it (around its ends, as the
    /// keys), a tap chooses a bar, a tap on the chosen one decides.
    fn pointer(&mut self, ctx: &mut Ctx, id: u64, phase: Phase, x: f32, y: f32) -> Next {
        match phase {
            Phase::Down => {
                if let Some(next) = self.tap(ctx, x, y) {
                    return next;
                }
                self.drag = Some(Drag {
                    id,
                    y,
                    scroll: self.scroll,
                    moved: false,
                });
            }
            Phase::Move => {
                let slop = DRAG_SLOP * ctx.ui();
                let pitch = self.bar_pitch.unwrap_or(46.0 * ctx.ui());
                let Some(d) = self.drag.as_mut().filter(|d| d.id == id) else {
                    return Next::Stay;
                };
                d.moved |= (y - d.y).abs() > slop;
                if d.moved && !self.bars.is_empty() {
                    let n = self.bars.len();
                    self.scroll = (d.scroll - (y - d.y) / pitch).rem_euclid(n as f32);
                    let c = (self.scroll.round() as usize) % n;
                    if c != self.cursor {
                        self.cursor = c;
                        self.moved(ctx);
                    }
                }
            }
            Phase::Up => {
                let tap = self.drag.take().is_some_and(|d| d.id == id && !d.moved);
                if let Some(&i) = self.bar_rects.at(x, y).filter(|_| tap) {
                    if i == self.cursor {
                        return self.decide(ctx, false);
                    }
                    self.cursor = i;
                    self.moved(ctx);
                }
            }
        }
        Next::Stay
    }

    /// Web: delete the chosen song's folder kept in the browser, on a
    /// second press of Delete.
    #[cfg(target_arch = "wasm32")]
    fn delete_kept(&mut self, ctx: &mut Ctx) {
        let bms = ctx.paths.root.join("bms");
        let Some(dir) = self.selected_song(ctx).and_then(|(_, s)| {
            let rest = std::path::Path::new(&s.path).strip_prefix(&bms).ok()?;
            Some(bms.join(rest.components().next()?.as_os_str()))
        }) else {
            return;
        };
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !self.delete_armed.confirm(dir.clone()) {
            ctx.toast(format!(
                "Press Delete again to delete {name} from this browser"
            ));
            return;
        }
        crate::platform::vfs::remove_dir(&dir);
        ctx.toast(format!("Deleted {name}"));
    }

    pub(super) fn input_event(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        if self.search_edit.is_some() {
            self.search_input(ctx, ev);
            return Next::Stay;
        }
        match ev {
            InputEv::Key {
                code,
                pressed,
                repeat,
                ..
            } => return self.key(ctx, *code, *pressed, *repeat),
            InputEv::Pad { id, pressed, .. } => return self.pad(ctx, *id, *pressed),
            InputEv::Axis { value, id, .. } => {
                let step = self.axis.step(*id, *value);
                if step != 0 && ctx.config.keys.any_axis_is_scratch {
                    if self.holding() {
                        let k = if step < 0 {
                            PanelKey::Up
                        } else {
                            PanelKey::Down
                        };
                        self.panel_input(ctx, k);
                    } else {
                        self.step(ctx, step);
                    }
                }
            }
            InputEv::Wheel { dy } => {
                let n = (-dy).round() as i32;
                if n != 0 {
                    self.step(ctx, n);
                }
            }
            InputEv::Pointer {
                id, phase, x, y, ..
            } => return self.pointer(ctx, *id, *phase, *x, *y),
            InputEv::Dropped(p) if p.is_dir() => {
                let s = p.to_string_lossy().into_owned();
                if !ctx.config.bms_dirs.contains(&s) {
                    ctx.config.bms_dirs.push(s.clone());
                    ctx.save_config();
                    ctx.toast(format!("Added BMS folder: {s}"));
                    ctx.start_scan();
                }
            }
            _ => {}
        }
        Next::Stay
    }
}
