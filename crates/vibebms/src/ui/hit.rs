use crate::gfx::Rect;

/// What was drawn where in the last frame, for the pointer: a point hits
/// the rect drawn last (on top).
#[derive(Debug)]
pub struct HitMap<T> {
    items: Vec<(Rect, T)>,
}

impl<T> Default for HitMap<T> {
    fn default() -> Self {
        HitMap { items: Vec::new() }
    }
}

impl<T> HitMap<T> {
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn push(&mut self, r: Rect, v: T) {
        self.items.push((r, v));
    }

    /// The topmost rect at (`x`, `y`) and what it stands for.
    pub fn hit(&self, x: f32, y: f32) -> Option<(Rect, &T)> {
        self.items
            .iter()
            .rev()
            .find(|(r, _)| r.contains(x, y))
            .map(|(r, v)| (*r, v))
    }

    pub fn at(&self, x: f32, y: f32) -> Option<&T> {
        self.hit(x, y).map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = &(Rect, T)> {
        self.items.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_drawn_is_on_top() {
        let mut m = HitMap::default();
        m.push(Rect::new(0.0, 0.0, 100.0, 100.0), 1);
        m.push(Rect::new(50.0, 50.0, 100.0, 100.0), 2);
        assert_eq!(m.at(10.0, 10.0), Some(&1));
        assert_eq!(m.at(60.0, 60.0), Some(&2));
        assert_eq!(m.hit(120.0, 120.0).map(|h| h.0.x), Some(50.0));
        // right and bottom edges are outside
        assert_eq!(m.at(150.0, 60.0), None);
        assert_eq!(m.at(-1.0, 10.0), None);
        m.clear();
        assert_eq!(m.at(10.0, 10.0), None);
    }
}
