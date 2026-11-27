//! What a skin reads of the game while it is drawn.

/// Game state queried by the skin each frame.
pub trait SkinState {
    /// Start time (scene ms) of a timer, `None` if off. Timer 0 = scene start.
    fn timer(&self, id: i32) -> Option<i64>;
    /// LR2 option (negative id = negated).
    fn option(&self, id: i32) -> bool;
    fn number(&self, id: i32) -> Option<i64>;
    fn text(&self, id: i32) -> Option<String>;
    /// 0..1 for bar graphs and sliders.
    fn rate(&self, id: i32) -> f32;
    /// Index of the image an image set / button shows (button values are
    /// a namespace of their own; defaults to the number with that id).
    fn button(&self, id: i32) -> Option<i64> {
        self.number(id)
    }
    /// Clear lamp counts (by clear type, 11) or DJ rank counts (AAA..F,
    /// then no play) of the selected folder.
    fn distribution(&self, rank: bool) -> Option<Vec<u32>> {
        let _ = rank;
        None
    }
    /// beatoraja offset (x, y, w, h, angle, alpha) in skin pixels, y up.
    fn offset(&self, id: i32) -> Option<[f32; 6]> {
        let _ = id;
        None
    }
}
