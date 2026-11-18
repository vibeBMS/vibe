//! No device and no clock of its own: the mixer's frames are made when
//! asked for, up to a moment on the judge's clock (`Mixer::render_until`),
//! for a video's sound track rendered offline.

use super::Output;
use crate::decode::Sample;
use crate::voice::{soft_clip, Cmd, State};
use crate::AudioConfig;
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender};
use std::sync::Arc;

/// Frames rendered at a time.
const BLOCK: usize = 1024;

pub(crate) struct OfflineOut {
    tx: Sender<Cmd>,
    rx: Receiver<Cmd>,
    state: State,
    retired: Receiver<Arc<Sample>>,
    rate: u32,
}

impl OfflineOut {
    /// Frame 0 is the mixer's epoch. Its clock is never fitted: a sound
    /// due at a moment starts on the frame of that moment at `rate`.
    pub fn new(cfg: &AudioConfig, rate: u32) -> OfflineOut {
        let (tx, rx) = channel();
        let (retire, retired) = sync_channel(cfg.max_voices.max(16) * 2);
        OfflineOut {
            tx,
            rx,
            state: State::new(cfg, rate, retire),
            retired,
            rate,
        }
    }
}

impl Output for OfflineOut {
    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    fn keep_alive(&mut self) -> Option<super::Device> {
        self.retired.try_iter().for_each(drop);
        None
    }

    fn latency_us(&self) -> Option<i64> {
        Some(0)
    }

    fn render_until(&mut self, us: f64, out: &mut Vec<f32>) -> bool {
        let target = (us.max(0.0) * self.rate as f64 / 1e6).floor() as u64;
        while self.state.frame < target {
            for c in self.rx.try_iter() {
                self.state.apply(c);
            }
            let frames = ((target - self.state.frame) as usize).min(BLOCK);
            let mix = self.state.render(frames, None);
            out.extend(mix.iter().map(|&x| soft_clip(x)));
            self.retired.try_iter().for_each(drop);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::voice::tests::sample;
    use crate::{AudioConfig, Mixer, PlayOptions};
    use std::time::Duration;

    /// Frames are made only when asked for, as many as the time asks; a
    /// sound due at a moment starts on its frame, one played now on the
    /// frame the mixer is at.
    #[test]
    fn frames_are_made_up_to_the_moment_asked() {
        let cfg = AudioConfig {
            master: 1.0,
            ..Default::default()
        };
        let mut m = Mixer::offline(&cfg, 1000);
        let t0 = m.epoch;
        let s = sample(5, 16384);
        let o = PlayOptions {
            at: Some(t0 + Duration::from_millis(10)),
            ..Default::default()
        };
        m.play(&s, o);
        let mut out = Vec::new();
        assert!(m.render_until(t0 + Duration::from_millis(20), &mut out));
        assert_eq!(out.len(), 2 * 20);
        let first = out.chunks(2).position(|f| f[0] != 0.0);
        assert_eq!(first, Some(10));
        assert_eq!(out.chunks(2).filter(|f| f[0] != 0.0).count(), 5);
        // now: from frame 20
        m.play(&s, PlayOptions::default());
        out.clear();
        m.render_until(t0 + Duration::from_millis(30), &mut out);
        assert_eq!(out.len(), 2 * 10);
        assert!(out[0] != 0.0 && out[2 * 5] == 0.0);
        // (a moment already rendered makes nothing)
        out.clear();
        m.render_until(t0 + Duration::from_millis(25), &mut out);
        assert!(out.is_empty());
        assert_eq!(m.latency_us(), 0);
    }
}
