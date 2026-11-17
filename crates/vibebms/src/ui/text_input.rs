use super::HitMap;
use crate::app::{Ctx, Draw};
use crate::gfx::text::Align;
use crate::gfx::{with_alpha, Rect};
use crate::input::{InputEv, Phase};
use crate::theme;
use winit::keyboard::KeyCode;

/// A line of text being typed: from keys, an on-screen keyboard or an
/// input method (`InputEv::Text`), with touches on its OK / Cancel and
/// a tap outside it to cancel.
#[derive(Debug, Default)]
pub struct TextInput {
    pub buf: String,
    /// Where it was drawn last: a tap outside cancels.
    area: Rect,
    /// Its buttons: true OK, false Cancel.
    buttons: HitMap<bool>,
}

#[derive(Debug, PartialEq)]
pub enum TextEvent {
    Editing,
    Commit(String),
    Cancel,
}

impl TextInput {
    pub fn new(buf: String) -> TextInput {
        TextInput {
            buf,
            ..TextInput::default()
        }
    }

    pub fn input(&mut self, ev: &InputEv) -> TextEvent {
        match ev {
            // on-screen keyboards (their return key ends it) and input methods
            InputEv::Text(t) if t.contains(['\n', '\r']) => self.commit(),
            InputEv::Text(t) => self.type_text(t),
            InputEv::Pointer {
                phase: Phase::Down,
                x,
                y,
                ..
            } => match self.buttons.at(*x, *y) {
                Some(true) => self.commit(),
                Some(false) => TextEvent::Cancel,
                None if !self.area.contains(*x, *y) => TextEvent::Cancel,
                None => TextEvent::Editing,
            },
            InputEv::Key {
                code,
                pressed: true,
                text,
                ..
            } => match code {
                KeyCode::Escape => TextEvent::Cancel,
                KeyCode::Enter | KeyCode::NumpadEnter => self.commit(),
                KeyCode::Backspace => {
                    self.buf.pop();
                    TextEvent::Editing
                }
                _ => self.type_text(text.as_deref().unwrap_or_default()),
            },
            _ => TextEvent::Editing,
        }
    }

    fn type_text(&mut self, t: &str) -> TextEvent {
        self.buf.extend(t.chars().filter(|c| !c.is_control()));
        TextEvent::Editing
    }

    fn commit(&mut self) -> TextEvent {
        TextEvent::Commit(std::mem::take(&mut self.buf))
    }

    /// A box at `r` showing `label` and the text; a tap on it commits.
    pub fn draw_box(&mut self, ctx: &mut Ctx, d: &mut Draw, r: Rect, label: &str) {
        // (an on-screen keyboard or input method comes up)
        ctx.text_input = true;
        let ui = ctx.ui();
        self.area = r;
        self.buttons.clear();
        self.buttons.push(r, true);
        d.b.rect(r, with_alpha(theme::PANEL, 0.97));
        d.b.rect_outline(r, 2.0, theme::ACCENT);
        d.text_fit(
            &format!("{label}{}_", self.buf),
            r.x + 12.0 * ui,
            r.y + 10.0 * ui,
            20.0 * ui,
            theme::TEXT,
            Align::Left,
            r.w - 24.0 * ui,
        );
    }

    /// A dialog over the screen with `title`, the text (`masked`: as dots)
    /// and OK / Cancel buttons.
    pub fn draw_modal(&mut self, ctx: &mut Ctx, d: &mut Draw, title: &str, masked: bool) {
        ctx.text_input = true;
        let ui = ctx.ui();
        let (w, h) = (ctx.width, ctx.height);
        // the on-screen keyboard (iOS, Android) covers the lower half
        let r = Rect::new(w * 0.1, h * 0.2, w * 0.8, 140.0 * ui);
        self.area = r;
        super::modal(d, w, h, r);
        let shown = if masked {
            "•".repeat(self.buf.chars().count())
        } else {
            self.buf.clone()
        };
        d.text(
            &format!("{title}  (Enter = OK, Esc = cancel)"),
            r.x + 14.0 * ui,
            r.y + 10.0 * ui,
            15.0 * ui,
            theme::TEXT_DIM,
            Align::Left,
        );
        d.text_fit(
            &format!("{shown}_"),
            r.x + 14.0 * ui,
            r.y + 42.0 * ui,
            22.0 * ui,
            theme::TEXT,
            Align::Left,
            r.w - 28.0 * ui,
        );
        let (bw, bh) = (120.0 * ui, 36.0 * ui);
        let by = r.y + r.h - bh - 12.0 * ui;
        let ok = Rect::new(r.x + r.w - 14.0 * ui - bw, by, bw, bh);
        let cancel = Rect::new(ok.x - 10.0 * ui - bw, by, bw, bh);
        self.buttons.clear();
        self.buttons
            .push(super::button(d, ok, "OK", ui, true), true);
        self.buttons
            .push(super::button(d, cancel, "Cancel", ui, false), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Instant;

    fn key(code: KeyCode, text: Option<&str>) -> InputEv {
        InputEv::Key {
            code,
            pressed: true,
            repeat: false,
            time: Instant::now(),
            text: text.map(Into::into),
        }
    }

    fn down(x: f32, y: f32) -> InputEv {
        InputEv::Pointer {
            id: 1,
            phase: Phase::Down,
            x,
            y,
            time: Instant::now(),
            touch: true,
        }
    }

    fn boxed() -> TextInput {
        let mut t = TextInput::new("ab".into());
        t.area = Rect::new(0.0, 0.0, 200.0, 100.0);
        t.buttons.push(Rect::new(100.0, 60.0, 50.0, 30.0), true);
        t.buttons.push(Rect::new(40.0, 60.0, 50.0, 30.0), false);
        t
    }

    #[test]
    fn hardware_keys() {
        let mut t = boxed();
        assert_eq!(t.input(&key(KeyCode::KeyC, Some("c"))), TextEvent::Editing);
        // keys without text, and control characters, type nothing
        t.input(&key(KeyCode::ShiftLeft, None));
        t.input(&key(KeyCode::Tab, Some("\t")));
        assert_eq!(t.buf, "abc");
        t.input(&key(KeyCode::Backspace, None));
        assert_eq!(t.buf, "ab");
        // key repeats type too; releases do not
        let typed = |pressed, repeat| InputEv::Key {
            code: KeyCode::KeyX,
            pressed,
            repeat,
            time: Instant::now(),
            text: Some("x".into()),
        };
        t.input(&typed(true, true));
        t.input(&typed(false, false));
        assert_eq!(t.buf, "abx");
        assert_eq!(
            t.input(&key(KeyCode::NumpadEnter, None)),
            TextEvent::Commit("abx".into())
        );
        assert_eq!(t.input(&key(KeyCode::Escape, None)), TextEvent::Cancel);
    }

    #[test]
    fn on_screen_keyboards_and_input_methods() {
        let mut t = boxed();
        assert_eq!(t.input(&InputEv::Text("日本".into())), TextEvent::Editing);
        assert_eq!(t.buf, "ab日本");
        // the return key ends it
        assert_eq!(
            t.input(&InputEv::Text("\n".into())),
            TextEvent::Commit("ab日本".into())
        );
        assert_eq!(t.buf, "");
    }

    #[test]
    fn touches() {
        let mut t = boxed();
        // in the box but on no button: still editing
        assert_eq!(t.input(&down(10.0, 10.0)), TextEvent::Editing);
        assert_eq!(t.input(&down(60.0, 70.0)), TextEvent::Cancel);
        assert_eq!(t.input(&down(110.0, 70.0)), TextEvent::Commit("ab".into()));
        // outside cancels
        let mut t = boxed();
        assert_eq!(t.input(&down(300.0, 10.0)), TextEvent::Cancel);
        assert_eq!(t.buf, "ab");
        // only presses count
        let up = InputEv::Pointer {
            id: 1,
            phase: Phase::Up,
            x: 300.0,
            y: 10.0,
            time: Instant::now(),
            touch: true,
        };
        assert_eq!(t.input(&up), TextEvent::Editing);
    }
}
