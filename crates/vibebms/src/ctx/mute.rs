//! The output's silence while the app is not the one in use
//! (`app::activity` says when).

use crate::platform::{Duration, Instant};

/// The output's silence: since when it has been asked for.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mute {
    since: Option<Instant>,
}

impl Mute {
    /// The output fades out from now (once).
    pub fn start(&mut self, now: Instant) -> bool {
        let new = self.since.is_none();
        self.since.get_or_insert(now);
        new
    }

    /// Heard again (it was silent).
    pub fn end(&mut self) -> bool {
        self.since.take().is_some()
    }

    pub fn on(&self) -> bool {
        self.since.is_some()
    }

    /// Silent and faded out by `now`: what plays may be stopped without
    /// being heard.
    pub fn faded(&self, now: Instant) -> bool {
        // (a frame's margin past the glide)
        let fade = Duration::from_millis(vibebms_audio::MASTER_GLIDE_MS as u64 + 20);
        self.since.is_some_and(|t| now.duration_since(t) >= fade)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mute_fades_before_it_counts_as_silent() {
        let t = Instant::now();
        let mut m = Mute::default();
        assert!(!m.on() && !m.faded(t));
        assert!(m.start(t));
        // started once: a later start keeps the first moment
        assert!(!m.start(t + Duration::from_millis(50)));
        assert!(m.on());
        assert!(!m.faded(t + Duration::from_millis(60)));
        assert!(m.faded(t + Duration::from_millis(150)));
        assert!(m.end());
        assert!(!m.end());
        assert!(!m.faded(t + Duration::from_secs(1)));
    }
}
