//! USB HID keyboard usages (page 7), as iOS' GameController framework
//! tells keys (`GCKeyCode`), to winit's key codes.

use winit::keyboard::KeyCode::{self, *};

/// Usages 0x04 (A) to 0x73 (F24), in order (0x32, the second Backslash:
/// the non-US # key, where ISO keyboards have it).
#[rustfmt::skip]
const MAIN: [KeyCode; 0x70] = [
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
    Enter, Escape, Backspace, Tab, Space, Minus, Equal, BracketLeft, BracketRight,
    Backslash, Backslash, Semicolon, Quote, Backquote, Comma, Period, Slash, CapsLock,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    PrintScreen, ScrollLock, Pause, Insert, Home, PageUp, Delete, End, PageDown,
    ArrowRight, ArrowLeft, ArrowDown, ArrowUp,
    NumLock, NumpadDivide, NumpadMultiply, NumpadSubtract, NumpadAdd, NumpadEnter,
    Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    Numpad0, NumpadDecimal, IntlBackslash, ContextMenu, Power, NumpadEqual,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
];

pub fn key_code(usage: isize) -> Option<KeyCode> {
    Some(match usage {
        0x04..=0x73 => MAIN[usage as usize - 0x04],
        0x85 => NumpadComma,
        0x87 => IntlRo,
        0x88 => KanaMode,
        0x89 => IntlYen,
        0x8a => Convert,
        0x8b => NonConvert,
        0x90 => Lang1,
        0x91 => Lang2,
        0xe0 => ControlLeft,
        0xe1 => ShiftLeft,
        0xe2 => AltLeft,
        0xe3 => SuperLeft,
        0xe4 => ControlRight,
        0xe5 => ShiftRight,
        0xe6 => AltRight,
        0xe7 => SuperRight,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usages_are_their_keys() {
        assert_eq!(key_code(0x04), Some(KeyA));
        assert_eq!(key_code(0x1d), Some(KeyZ));
        assert_eq!(key_code(0x27), Some(Digit0));
        assert_eq!(key_code(0x2c), Some(Space));
        assert_eq!(key_code(0x3a), Some(F1));
        assert_eq!(key_code(0x52), Some(ArrowUp));
        assert_eq!(key_code(0x62), Some(Numpad0));
        assert_eq!(key_code(0x68), Some(F13));
        assert_eq!(key_code(0x73), Some(F24));
        assert_eq!(key_code(0xe1), Some(ShiftLeft));
        assert_eq!(key_code(0x03), None);
        assert_eq!(key_code(0x74), None);
    }
}
