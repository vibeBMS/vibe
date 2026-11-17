//! The hi-speed the notes are drawn at while playing: a change the player
//! makes reaches the notes over `hs_anim_ms` (ease-out cubic) and is shown
//! for a moment over the note field; a change the chart makes (FLOATING
//! through a BPM change) is drawn at once.

use crate::config::{Bpms, HsType, PlayConfig};
use crate::platform::{Duration, Instant};

/// How long a change is shown.
pub(super) const SHOWN: Duration = Duration::from_millis(1200);

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HsView {
    from: f32,
    to: f32,
    since: Option<Instant>,
    anim: Duration,
    /// What the player set, the last frame (`setting`).
    set: Option<[u32; 8]>,
    /// When the player last changed it (the change shown).
    pub changed: Option<Instant>,
}

/// The settings a player changes the hi-speed with.
fn setting(p: &PlayConfig) -> [u32; 8] {
    [
        p.hispeed.to_bits(),
        p.fixed_duration_ms.to_bits(),
        p.hs_level as u32,
        p.hs_fix as u32,
        p.hs_type as u32,
        p.sudden.to_bits() * (p.lane_cover || p.sudden > 0.0) as u32,
        p.lift.to_bits(),
        p.lane_cover as u32,
    ]
}

impl HsView {
    /// The hi-speed of the frame at `now` with the player's options `p` and
    /// the chart's BPMs `b` (of the moment).
    pub fn update(&mut self, p: &PlayConfig, b: &Bpms, now: Instant) {
        let target = p.hispeed_at(b);
        let set = setting(p);
        let by_player = self.set.is_some_and(|s| s != set);
        if by_player {
            self.changed = Some(now);
        }
        self.set = Some(set);
        if target == self.to {
            return;
        }
        let anim = Duration::from_millis(p.hs_anim_ms.min(1000) as u64);
        if by_player && !anim.is_zero() && self.since.is_some() {
            self.from = self.at(now);
            self.anim = anim;
        } else {
            self.from = target;
        }
        self.to = target;
        self.since = Some(now);
    }

    /// The hi-speed the notes are drawn at, at `now`.
    pub fn at(&self, now: Instant) -> f32 {
        let Some(since) = self.since.filter(|_| !self.anim.is_zero()) else {
            return self.to;
        };
        let t = now.saturating_duration_since(since).as_secs_f32() / self.anim.as_secs_f32();
        if t >= 1.0 {
            return self.to;
        }
        let e = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * e
    }

    /// The change to show at `now` (`None` once `SHOWN` has passed), with
    /// how far it has faded (0 .. 1).
    pub fn shown(&self, p: &PlayConfig, b: &Bpms, now: Instant) -> Option<(String, f32)> {
        let age = now.saturating_duration_since(self.changed?);
        if age >= SHOWN {
            return None;
        }
        let what = match p.hs_type {
            HsType::Normal => format!("NHS {}", p.hs_level),
            HsType::Floating => format!("FHS {:.2}", p.hispeed_at(b)),
            _ => format!("HS {:.2}", p.hispeed_shown(b)),
        };
        let fade = (age.as_secs_f32() / SHOWN.as_secs_f32() * 4.0 - 3.0).max(0.0);
        Some((format!("{what}   GN {}", p.green(b)), fade))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_a_change_and_shows_it() {
        let b = Bpms::flat(150.0);
        let mut p = PlayConfig::default();
        let t0 = Instant::now();
        let mut v = HsView::default();
        v.update(&p, &b, t0);
        assert_eq!((v.at(t0), v.changed), (2.5, None));
        p.hispeed = 3.5;
        v.update(&p, &b, t0);
        assert_eq!(v.at(t0), 2.5);
        // ease-out cubic: 87.5 % of the way at half the time
        let half = v.at(t0 + Duration::from_millis(75));
        assert!((half - 3.375).abs() < 1e-3, "{half}");
        assert_eq!(v.at(t0 + Duration::from_millis(150)), 3.5);
        let (text, fade) = v.shown(&p, &b, t0).unwrap();
        assert_eq!((text.as_str(), fade), ("HS 3.50   GN 274", 0.0));
        assert!(v.shown(&p, &b, t0 + SHOWN).is_none());
        // FLOATING through a BPM change: at once, nothing shown
        p.set_hs_type(HsType::Floating, &b);
        let t1 = t0 + Duration::from_secs(5);
        v.update(&p, &b.at(150.0), t1);
        let t2 = t1 + Duration::from_secs(5);
        v.update(&p, &b.at(300.0), t2);
        assert!((v.at(t2) - 1.75).abs() < 1e-3);
        assert!(v.shown(&p, &b, t2).is_none());
        // 0 ms: at once
        p.hs_anim_ms = 0;
        p.add_hispeed(0.25, &b.at(300.0));
        v.update(&p, &b.at(300.0), t2);
        assert!((v.at(t2) - 2.0).abs() < 1e-3);
    }
}
