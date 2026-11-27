use super::HitMap;
use crate::config::KeyConfig;
use crate::ctx::Ctx;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::theme;
use crate::gfx::{Draw, Rect};
use crate::input::{nav_from_key, nav_from_pad_rows, InputEv, Nav, Phase};
use winit::keyboard::KeyCode;

/// A message over the screen that asks OK or Cancel: its title, its text
/// (wrapped) and the two buttons. ←→ (and the turntable's keys) choose a
/// button, Enter (a white key) takes the one chosen, Cancel at first;
/// Esc (a blue key) cancels; a tap takes the button it is lifted on.
#[derive(Debug, Default)]
pub struct Notice {
    /// OK is chosen (else Cancel).
    ok: bool,
    /// Where each button was drawn last: true OK, false Cancel.
    buttons: HitMap<bool>,
    /// The button a pointer went down on.
    pressed: Option<(u64, bool)>,
}

impl Notice {
    /// What `ev` answers: `Some(true)` OK, `Some(false)` Cancel, `None`
    /// still asking.
    pub fn input(&mut self, ev: &InputEv, keys: &KeyConfig) -> Option<bool> {
        let nav = match ev {
            // (the press that opened it, held down, does not answer it)
            InputEv::Key {
                pressed: true,
                repeat: true,
                ..
            } => None,
            InputEv::Key {
                code,
                pressed: true,
                ..
            } => match code {
                KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::Tab => Some(Nav::Up),
                KeyCode::Space => Some(Nav::Decide),
                c => nav_from_key(*c, keys),
            },
            InputEv::Pad {
                id, pressed: true, ..
            } => nav_from_pad_rows(*id, keys),
            InputEv::Pointer {
                id,
                phase: Phase::Down,
                x,
                y,
                ..
            } => {
                self.pressed = self.buttons.at(*x, *y).map(|b| (*id, *b));
                None
            }
            InputEv::Pointer {
                id,
                phase: Phase::Up,
                x,
                y,
                ..
            } => {
                let down = self.pressed.take();
                let up = self.buttons.at(*x, *y).copied();
                return down.filter(|d| d.0 == *id && Some(d.1) == up).map(|d| d.1);
            }
            _ => None,
        };
        match nav? {
            Nav::Up | Nav::Down => {
                self.ok ^= true;
                None
            }
            Nav::Decide => Some(self.ok),
            Nav::Back | Nav::Close => Some(false),
            Nav::Settings | Nav::Option => None,
        }
    }

    /// The dialog: `title`, then `text` (paragraphs split at "\n"), then
    /// the buttons OK and `cancel`.
    pub fn draw(&mut self, ctx: &Ctx, d: &mut Draw, title: &str, text: &str, cancel: &str) {
        let ui = ctx.ui();
        let u = |v: f32| v * ui;
        let (w, h) = (ctx.view.width, ctx.view.height);
        let pw = (w - u(32.0)).min(u(640.0));
        let size = u(16.0);
        let line = size * 1.45;
        let lines: Vec<Vec<String>> = text
            .split('\n')
            .map(|p| super::wrap(d, p, size, pw - u(36.0), 8))
            .collect();
        let n = lines.iter().map(Vec::len).sum::<usize>() as f32;
        let gaps = lines.len().saturating_sub(1) as f32;
        let (bw, bh) = (u(150.0).min((pw - u(46.0)) / 2.0), u(38.0));
        let ph = u(18.0 + 30.0 + 8.0) + n * line + gaps * u(8.0) + u(20.0) + bh + u(18.0);
        let r = Rect::new((w - pw) / 2.0, ((h - ph) / 2.0).max(u(8.0)), pw, ph);
        super::modal(d, w, h, r);
        let x = r.x + u(18.0);
        let mut y = r.y + u(18.0);
        d.text(
            title,
            x,
            y,
            TextStyle::new(u(22.0), theme::ACCENT, Align::Left).fit(pw - u(36.0)),
        );
        y += u(30.0 + 8.0);
        for p in &lines {
            for l in p {
                d.text(l, x, y, TextStyle::new(size, theme::TEXT, Align::Left));
                y += line;
            }
            y += u(8.0);
        }
        let by = r.bottom() - bh - u(18.0);
        let okr = Rect::new(r.right() - u(18.0) - bw, by, bw, bh);
        let cr = Rect::new(okr.x - u(10.0) - bw, by, bw, bh);
        self.buttons.clear();
        self.buttons
            .push(super::button(d, okr, "OK", ui, self.ok), true);
        self.buttons
            .push(super::button(d, cr, cancel, ui, !self.ok), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Instant;

    fn key(code: KeyCode) -> InputEv {
        InputEv::Key {
            code,
            pressed: true,
            repeat: false,
            time: Instant::now(),
            text: None,
        }
    }

    fn pointer(phase: Phase, x: f32, y: f32) -> InputEv {
        InputEv::Pointer {
            id: 1,
            phase,
            x,
            y,
            time: Instant::now(),
            touch: true,
        }
    }

    fn drawn() -> Notice {
        let mut n = Notice::default();
        n.buttons.push(Rect::new(100.0, 60.0, 50.0, 30.0), true);
        n.buttons.push(Rect::new(40.0, 60.0, 50.0, 30.0), false);
        n
    }

    #[test]
    fn keys_choose_cancel_first() {
        let keys = KeyConfig::default();
        let mut n = drawn();
        assert_eq!(n.input(&key(KeyCode::Enter), &keys), Some(false));
        assert_eq!(n.input(&key(KeyCode::ArrowRight), &keys), None);
        assert_eq!(n.input(&key(KeyCode::Enter), &keys), Some(true));
        // a held key's repeats answer nothing
        let held = InputEv::Key {
            code: KeyCode::Enter,
            pressed: true,
            repeat: true,
            time: Instant::now(),
            text: None,
        };
        assert_eq!(n.input(&held, &keys), None);
        assert_eq!(n.input(&key(KeyCode::Escape), &keys), Some(false));
        assert_eq!(n.input(&key(KeyCode::KeyQ), &keys), None);
    }

    #[test]
    fn a_tap_takes_the_button_it_is_lifted_on() {
        let keys = KeyConfig::default();
        let mut n = drawn();
        // lifted elsewhere: nothing
        assert_eq!(n.input(&pointer(Phase::Down, 110.0, 70.0), &keys), None);
        assert_eq!(n.input(&pointer(Phase::Up, 10.0, 10.0), &keys), None);
        // a lift without its press (the tap that opened it): nothing
        assert_eq!(n.input(&pointer(Phase::Up, 110.0, 70.0), &keys), None);
        n.input(&pointer(Phase::Down, 110.0, 70.0), &keys);
        assert_eq!(n.input(&pointer(Phase::Up, 112.0, 72.0), &keys), Some(true));
        n.input(&pointer(Phase::Down, 50.0, 70.0), &keys);
        assert_eq!(n.input(&pointer(Phase::Up, 50.0, 70.0), &keys), Some(false));
    }
}
