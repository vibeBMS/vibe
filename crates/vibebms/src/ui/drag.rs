//! A press that scrolls what it went down on once it moves.

/// A pointer pressed on something that scrolls: past a slop from where it
/// went down it drags (the scroll follows it), else its lift is a tap.
#[derive(Clone, Copy, Debug)]
pub struct DragScroll {
    id: u64,
    /// Where it went down.
    pub x: f32,
    pub y: f32,
    /// The scroll position then.
    scroll: f32,
    moved: bool,
}

impl DragScroll {
    /// Pointer `id` went down at (`x`, `y`) with the scroll at `scroll`.
    pub fn new(id: u64, x: f32, y: f32, scroll: f32) -> DragScroll {
        DragScroll {
            id,
            x,
            y,
            scroll,
            moved: false,
        }
    }

    /// Whether it is pointer `id`'s.
    pub fn is(&self, id: u64) -> bool {
        self.id == id
    }

    /// Pointer `id` moved to `y`: once it has gone past `slop` from where
    /// it went down, the scroll that follows it (`unit` px a step: a pixel
    /// of a list, a bar, a line). `None` for another pointer, or a press
    /// still within the slop.
    pub fn to(&mut self, id: u64, y: f32, slop: f32, unit: f32) -> Option<f32> {
        if id != self.id {
            return None;
        }
        self.moved |= (y - self.y).abs() > slop;
        self.moved.then(|| self.scroll - (y - self.y) / unit)
    }

    /// Whether pointer `id` lifting is a tap of this press (it did not
    /// drag).
    pub fn tap(&self, id: u64) -> bool {
        self.id == id && !self.moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_until_past_the_slop_then_a_drag() {
        let mut d = DragScroll::new(1, 10.0, 200.0, 50.0);
        assert_eq!(d.to(1, 205.0, 8.0, 1.0), None);
        assert!(d.tap(1));
        // in pixels, and in steps of 40 px
        assert_eq!(d.to(1, 180.0, 8.0, 1.0), Some(70.0));
        assert_eq!(d.to(1, 120.0, 8.0, 40.0), Some(52.0));
        // past it once, it drags back within it too
        assert_eq!(d.to(1, 198.0, 8.0, 1.0), Some(52.0));
        assert!(!d.tap(1));
        // another pointer neither moves it nor taps
        assert_eq!(d.to(2, 0.0, 8.0, 1.0), None);
        let d = DragScroll::new(1, 0.0, 0.0, 0.0);
        assert!(!d.tap(2) && d.is(1) && !d.is(2));
    }
}
