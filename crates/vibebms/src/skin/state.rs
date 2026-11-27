//! What a skin reads of the game while it is drawn.

use std::borrow::Cow;

/// Game state queried by the skin each frame.
pub trait SkinState {
    /// Start time (scene ms) of a timer, `None` if off. Timer 0 = scene start.
    fn timer(&self, id: i32) -> Option<i64>;
    /// LR2 option (negative id = negated).
    fn option(&self, id: i32) -> bool;
    fn number(&self, id: i32) -> Option<i64>;
    fn text(&self, id: i32) -> Option<Cow<'_, str>>;
    /// 0..1 for bar graphs and sliders.
    fn rate(&self, id: i32) -> f32;
    /// Index of the image an image set / button shows (button values are
    /// a namespace of their own; defaults to the number with that id).
    fn button(&self, id: i32) -> Option<i64> {
        self.number(id)
    }
    /// Clear lamp counts (by clear type, 11) or DJ rank counts (AAA..F,
    /// then no play) of the selected folder.
    fn distribution(&self, rank: bool) -> Option<Counts> {
        let _ = rank;
        None
    }
    /// beatoraja offset (x, y, w, h, angle, alpha) in skin pixels, y up.
    fn offset(&self, id: i32) -> Option<[f32; 6]> {
        let _ = id;
        None
    }
}

/// Counts of a folder's charts by lamp or rank ([`SkinState::distribution`]),
/// kept without a heap.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    n: [u32; Counts::MAX],
    len: usize,
}

impl Counts {
    pub const MAX: usize = 11;

    /// The first [`Counts::MAX`] of `c`.
    pub fn new(c: &[u32]) -> Counts {
        let len = c.len().min(Counts::MAX);
        let mut n = [0; Counts::MAX];
        n[..len].copy_from_slice(&c[..len]);
        Counts { n, len }
    }
}

impl std::ops::Deref for Counts {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        &self.n[..self.len]
    }
}

impl std::fmt::Debug for Counts {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// More counts than there are kinds: the first ones, without a panic.
    #[test]
    fn counts_keep_at_most_their_kinds() {
        let many: Vec<u32> = (1..=14).collect();
        assert_eq!(&*Counts::new(&many), &many[..Counts::MAX]);
        assert_eq!(&*Counts::new(&[3, 1]), &[3, 1]);
    }
}
