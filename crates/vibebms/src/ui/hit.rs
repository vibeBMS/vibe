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

    /// As `hit`, else the nearest rect that, grown to at least `min` wide
    /// and high about its centre, holds (`x`, `y`): small targets on a
    /// small screen.
    pub fn hit_near(&self, x: f32, y: f32, min: f32) -> Option<(Rect, &T)> {
        self.hit(x, y).or_else(|| {
            let grown = self.items.iter().rev().filter_map(|(r, v)| {
                let (gx, gy) = ((min - r.w).max(0.0) * 0.5, (min - r.h).max(0.0) * 0.5);
                let g = Rect::new(r.x - gx, r.y - gy, r.w + 2.0 * gx, r.h + 2.0 * gy);
                let dx = (r.x - x).max(x - r.right()).max(0.0);
                let dy = (r.y - y).max(y - r.bottom()).max(0.0);
                g.contains(x, y).then_some((dx * dx + dy * dy, (*r, v)))
            });
            grown.min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, h)| h)
        })
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

    #[test]
    fn small_targets_grow() {
        let mut m = HitMap::default();
        m.push(Rect::new(0.0, 0.0, 20.0, 20.0), 1);
        m.push(Rect::new(30.0, 0.0, 20.0, 20.0), 2);
        assert_eq!(m.hit_near(10.0, 25.0, 0.0).map(|h| *h.1), None);
        // grown to 40: (10, 25) is under 1, (27, 10) nearer 2
        assert_eq!(m.hit_near(10.0, 25.0, 40.0).map(|h| *h.1), Some(1));
        assert_eq!(m.hit_near(27.0, 10.0, 40.0).map(|h| *h.1), Some(2));
        assert_eq!(m.hit_near(10.0, 10.0, 40.0).map(|h| *h.1), Some(1));
        assert_eq!(m.hit_near(10.0, 45.0, 40.0).map(|h| *h.1), None);
    }
}
