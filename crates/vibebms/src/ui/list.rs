use super::DragScroll;

/// A list's selected row and scroll position (in the list's own units:
/// pixels or rows). After keys the scroll follows the selection; the wheel
/// and drags move it freely.
#[derive(Debug, Default)]
pub struct ListView {
    pub sel: usize,
    pub scroll: f32,
    /// Keep the selected row in view (set by keys, cleared by the wheel
    /// and drags).
    pub follow: bool,
    drag: Option<DragScroll>,
}

impl ListView {
    pub fn new() -> ListView {
        ListView {
            follow: true,
            ..ListView::default()
        }
    }

    /// Move the selection by `d` rows, wrapping around a list of `len`.
    pub fn step(&mut self, d: i32, len: usize) {
        self.sel = (self.sel as i32 + d).rem_euclid(len.max(1) as i32) as usize;
        self.follow = true;
    }

    /// Choose row `i` and show it.
    pub fn select(&mut self, i: usize) {
        self.sel = i;
        self.follow = true;
    }

    pub fn wheel(&mut self, by: f32) {
        self.follow = false;
        self.scroll += by;
    }

    /// Pointer `id` went down on the list at (`x`, `y`).
    pub fn press(&mut self, id: u64, x: f32, y: f32) {
        self.drag = Some(DragScroll::new(id, x, y, self.scroll));
    }

    /// Pointer `id` moved to `y`: once past `slop` from where it went down
    /// the list follows it. True while it drags the list.
    pub fn drag_to(&mut self, id: u64, y: f32, slop: f32) -> bool {
        let Some(s) = self.drag.as_mut().and_then(|d| d.to(id, y, slop, 1.0)) else {
            return false;
        };
        self.follow = false;
        self.scroll = s;
        true
    }

    /// Pointer `id` lifted: where it went down when it did not drag (a
    /// tap there).
    pub fn release(&mut self, id: u64) -> Option<(f32, f32)> {
        if !self.drag.as_ref()?.is(id) {
            return None;
        }
        let d = self.drag.take()?;
        d.tap(id).then_some((d.x, d.y))
    }

    /// Keep the selected row (from `top` to `bottom`) in a `view` high when
    /// following, and the scroll within a `content` this high.
    pub fn fit(&mut self, top: f32, bottom: f32, view: f32, content: f32) {
        if self.follow {
            if bottom - self.scroll > view {
                self.scroll = bottom - view;
            }
            if top < self.scroll {
                self.scroll = top;
            }
        }
        self.scroll = self.scroll.clamp(0.0, (content - view).max(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_wrap_and_scroll_follows() {
        let mut l = ListView::new();
        l.step(-1, 10);
        assert_eq!(l.sel, 9);
        // rows of 1 in a view of 4: the last row at the bottom
        l.fit(9.0, 10.0, 4.0, 10.0);
        assert_eq!(l.scroll, 6.0);
        l.step(1, 10);
        assert_eq!(l.sel, 0);
        l.fit(0.0, 1.0, 4.0, 10.0);
        assert_eq!(l.scroll, 0.0);
        // a row inside the view: no scroll
        l.select(3);
        l.fit(3.0, 4.0, 4.0, 10.0);
        assert_eq!(l.scroll, 0.0);
        // an empty list
        l.step(1, 0);
        assert_eq!(l.sel, 0);
    }

    #[test]
    fn scroll_stays_in_the_content() {
        let mut l = ListView::new();
        l.wheel(-30.0);
        assert!(!l.follow);
        l.fit(0.0, 1.0, 4.0, 10.0);
        assert_eq!(l.scroll, 0.0);
        l.wheel(100.0);
        l.fit(0.0, 1.0, 4.0, 10.0);
        assert_eq!(l.scroll, 6.0);
        // not following: the selection may be out of view
        l.sel = 0;
        l.fit(0.0, 1.0, 4.0, 10.0);
        assert_eq!(l.scroll, 6.0);
        // a list shorter than its view does not scroll
        l.fit(0.0, 1.0, 40.0, 10.0);
        assert_eq!(l.scroll, 0.0);
    }

    #[test]
    fn a_drag_scrolls_a_tap_does_not() {
        let mut l = ListView::new();
        l.scroll = 50.0;
        l.press(1, 10.0, 200.0);
        // within the slop: still a tap
        assert!(!l.drag_to(1, 205.0, 8.0));
        assert_eq!(l.scroll, 50.0);
        assert_eq!(l.release(1), Some((10.0, 200.0)));
        // past it: the list follows the finger, the lift is no tap
        l.press(1, 10.0, 200.0);
        assert!(l.drag_to(1, 180.0, 8.0));
        assert_eq!(l.scroll, 70.0);
        assert!(l.drag_to(1, 198.0, 8.0));
        assert_eq!(l.scroll, 52.0);
        assert!(!l.follow);
        assert_eq!(l.release(1), None);
        // another pointer does not move it or end it
        l.press(1, 0.0, 0.0);
        assert!(!l.drag_to(2, 100.0, 8.0));
        assert_eq!(l.release(2), None);
        assert_eq!(l.release(1), Some((0.0, 0.0)));
    }
}
