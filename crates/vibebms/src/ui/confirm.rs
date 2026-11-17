/// A choice that a second press confirms ("press again to delete"): the
/// first press on it arms it, a press on anything else disarms it.
#[derive(Debug)]
pub struct Armed<T>(Option<T>);

impl<T> Default for Armed<T> {
    fn default() -> Self {
        Armed(None)
    }
}

impl<T: PartialEq> Armed<T> {
    /// A press on `v`: true when it was armed (it is done, and disarmed),
    /// else it is armed now.
    pub fn confirm(&mut self, v: T) -> bool {
        if self.is(&v) {
            self.0 = None;
            true
        } else {
            self.0 = Some(v);
            false
        }
    }

    /// `v` waits for its second press.
    pub fn is(&self, v: &T) -> bool {
        self.0.as_ref() == Some(v)
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_press_confirms() {
        let mut a = Armed::default();
        assert!(!a.confirm(1));
        assert!(a.is(&1));
        // another choice arms instead
        assert!(!a.confirm(2));
        assert!(!a.is(&1));
        assert!(a.confirm(2));
        assert!(!a.is(&2));
        assert!(!a.confirm(2));
        a.clear();
        assert!(!a.confirm(2));
    }
}
