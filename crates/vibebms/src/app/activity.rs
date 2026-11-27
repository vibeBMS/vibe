//! Whether the app is the one in use, and so whether it is heard
//! (`audio.mute_inactive`).
//!
//! The windowing system tells it in pieces: the window has the focus or
//! not, is hidden (minimised, covered, a hidden browser tab, an app in the
//! background) or not, and the app is active or not (iOS: until the app
//! resigns active, as when Control Center or another app's window is
//! pulled over it). Out of play every one of them silences vibeBMS; in play
//! only a hidden or inactive app, as a desktop player may click another
//! window while playing on a controller, except on phones and tablets,
//! where the app that lost the focus gets no touches, keys or controllers
//! either: there, as out of play.

/// What the platform said of the app lately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Activity {
    /// The window has the input focus (the web: the page has it).
    pub focused: bool,
    /// Covered, out of sight, or the app in the background (winit's
    /// `Occluded`; the web: the tab is hidden).
    pub occluded: bool,
    /// Minimised (where winit does not tell it as `Occluded`).
    pub minimized: bool,
    /// The app is not active (winit's `Suspended`: iOS resigning active,
    /// Android's surface gone, the web page put away).
    pub suspended: bool,
    /// The in-app download browser is up: its own window (or Android's
    /// dialog) may have the focus, which is vibeBMS in use all the same.
    pub browsing: bool,
}

impl Default for Activity {
    fn default() -> Self {
        Activity {
            focused: true,
            occluded: false,
            minimized: false,
            suspended: false,
            browsing: false,
        }
    }
}

/// How the platform's input follows the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Desktop and the web: a controller (or a keyboard hook) may still
    /// play a window without the focus.
    Desktop,
    /// Phones and tablets: input goes to the focused app only.
    Mobile,
}

impl Platform {
    pub const THIS: Platform = if cfg!(mobile) {
        Platform::Mobile
    } else {
        Platform::Desktop
    };
}

impl Activity {
    /// Whether vibeBMS should be silent: `in_play` a chart is being
    /// played, `enabled` the setting is on.
    pub fn silent(self, in_play: bool, platform: Platform, enabled: bool) -> bool {
        if !enabled {
            return false;
        }
        if self.occluded || self.minimized || self.suspended {
            return true;
        }
        !(self.focused || self.browsing) && (!in_play || platform == Platform::Mobile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(focused: bool, occluded: bool, minimized: bool, suspended: bool) -> Activity {
        Activity {
            focused,
            occluded,
            minimized,
            suspended,
            browsing: false,
        }
    }

    #[test]
    fn out_of_play_anything_but_the_focus_silences() {
        for p in [Platform::Desktop, Platform::Mobile] {
            assert!(!Activity::default().silent(false, p, true));
            assert!(a(false, false, false, false).silent(false, p, true));
            assert!(a(true, true, false, false).silent(false, p, true));
            assert!(a(true, false, true, false).silent(false, p, true));
            assert!(a(true, false, false, true).silent(false, p, true));
        }
    }

    #[test]
    fn in_play_a_desktop_window_without_the_focus_is_heard() {
        let unfocused = a(false, false, false, false);
        assert!(!unfocused.silent(true, Platform::Desktop, true));
        assert!(unfocused.silent(true, Platform::Mobile, true));
        for p in [Platform::Desktop, Platform::Mobile] {
            assert!(!Activity::default().silent(true, p, true));
            assert!(a(false, true, false, false).silent(true, p, true));
            assert!(a(true, false, true, false).silent(true, p, true));
            assert!(a(true, false, false, true).silent(true, p, true));
        }
    }

    #[test]
    fn the_download_browser_with_the_focus_is_the_app_in_use() {
        let browsing = Activity {
            focused: false,
            browsing: true,
            ..Default::default()
        };
        assert!(!browsing.silent(false, Platform::Desktop, true));
        assert!(!browsing.silent(false, Platform::Mobile, true));
        let hidden = Activity {
            occluded: true,
            ..browsing
        };
        assert!(hidden.silent(false, Platform::Desktop, true));
    }

    #[test]
    fn the_setting_off_is_always_heard() {
        for p in [Platform::Desktop, Platform::Mobile] {
            for play in [false, true] {
                assert!(!a(false, true, true, true).silent(play, p, false));
            }
        }
    }
}
