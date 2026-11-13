//! The screen's physical size, for controls sized in millimetres.
//!
//! No platform reports it reliably (a browser's inch is always 96 CSS
//! pixels), so the density is guessed from the kind of device; the
//! settings take the real diagonal to override the guess.

use winit::window::Window;

/// The screen in device pixels, and a guess of the logical pixels (points)
/// per inch.
#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub size_px: (f32, f32),
    pub points_per_inch: f32,
}

impl Screen {
    /// Device pixels per millimetre: from the diagonal in inches when the
    /// user gave it, else from the guess.
    pub fn px_per_mm(&self, scale: f32, inches: f32) -> f32 {
        if inches > 0.0 {
            self.size_px.0.hypot(self.size_px.1) / (inches * 25.4)
        } else {
            self.points_per_inch * scale / 25.4
        }
    }

    /// The diagonal the guess amounts to, in inches.
    pub fn guessed_inches(&self, scale: f32) -> f32 {
        self.size_px.0.hypot(self.size_px.1) / (self.points_per_inch * scale)
    }
}

/// Points per inch of an Apple device from its short side in points.
fn apple(short_pt: f32, scale: f32) -> f32 {
    if short_pt >= 700.0 {
        // iPads are 264 ppi at 2x, the 8.3" mini 326 ppi
        if (short_pt - 744.0).abs() < 1.0 {
            163.0
        } else {
            132.0
        }
    } else if scale >= 2.5 {
        // iPhones at 3x are about 460 ppi, at 2x 326 ppi
        153.0
    } else {
        163.0
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn detect(window: &Window) -> Screen {
    let scale = window.scale_factor() as f32;
    let size = window
        .current_monitor()
        .map(|m| m.size())
        .unwrap_or_else(|| window.inner_size());
    let (w, h) = (size.width as f32, size.height as f32);
    let points_per_inch = if cfg!(target_os = "android") {
        // a density-independent pixel is 1/160 inch
        160.0
    } else if cfg!(target_os = "ios") {
        apple(w.min(h) / scale, scale)
    } else if cfg!(target_os = "macos") {
        110.0
    } else {
        // Windows and Linux scale from 96 dpi
        96.0
    };
    Screen {
        size_px: (w, h),
        points_per_inch,
    }
}

#[cfg(target_arch = "wasm32")]
pub fn detect(window: &Window) -> Screen {
    let fallback = || {
        let s = window.inner_size();
        Screen {
            size_px: (s.width as f32, s.height as f32),
            points_per_inch: 96.0,
        }
    };
    let Some(w) = web_sys::window() else {
        return fallback();
    };
    let dpr = w.device_pixel_ratio() as f32;
    let Some((sw, sh)) = w
        .screen()
        .ok()
        .and_then(|s| Some((s.width().ok()? as f32, s.height().ok()? as f32)))
    else {
        return fallback();
    };
    let nav = w.navigator();
    let ua = nav.user_agent().unwrap_or_default();
    // iPadOS Safari calls itself a Mac; a Mac has no touch screen
    let ios = ua.contains("iPhone")
        || ua.contains("iPad")
        || (ua.contains("Macintosh") && nav.max_touch_points() > 1);
    let points_per_inch = if ios {
        apple(sw.min(sh), dpr)
    } else if ua.contains("Android") {
        160.0
    } else if ua.contains("Macintosh") {
        110.0
    } else {
        96.0
    };
    Screen {
        size_px: (sw * dpr, sh * dpr),
        points_per_inch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipad_13_is_its_real_size() {
        // iPad Pro 13": 2752 x 2064 at 2x, 264 ppi
        let s = Screen {
            size_px: (2752.0, 2064.0),
            points_per_inch: apple(1032.0, 2.0),
        };
        assert!((s.guessed_inches(2.0) - 13.0).abs() < 0.1);
        assert!((s.px_per_mm(2.0, 0.0) - 264.0 / 25.4).abs() < 0.01);
        // a given diagonal wins
        assert!((s.px_per_mm(2.0, 13.0) - 3440.0 / (13.0 * 25.4)).abs() < 0.01);
    }
}
