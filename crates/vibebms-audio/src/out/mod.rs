//! Where the mixer's frames go: a cpal stream (`cpal`, silent by the wall
//! clock while there is no device: `null`; on Windows a WASAPI stream of
//! its own first: `wasapi`), or on the web an AudioWorklet (`web`, the
//! worklet's side in `worklet`); or made when asked for (`offline`).

pub(crate) mod cpal;
#[cfg(target_os = "ios")]
mod ios;
mod null;
pub(crate) mod offline;
#[cfg(any(windows, test))]
mod wasapi;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(any(target_arch = "wasm32", test))]
mod worklet;

use crate::voice::Cmd;
use web_time::Instant;

pub(crate) trait Output {
    /// Hand a command to the mixer.
    fn send(&self, cmd: Cmd);
    /// Called every frame (see `Mixer::keep_alive`): the device, when the
    /// output was opened again on another one.
    fn keep_alive(&mut self) -> Option<Device> {
        None
    }
    /// Output latency in µs when the output knows it better than one
    /// buffer.
    fn latency_us(&self) -> Option<i64> {
        None
    }
    fn resume(&self) {}
    /// A message for the player about the output (exclusive mode asked
    /// for and not had), once.
    fn take_notice(&mut self) -> Option<String> {
        None
    }
    /// An offline output: its frames up to `us` on the judge's clock
    /// appended to `out` (stereo, interleaved, clipped); `false` for an
    /// output that plays them itself.
    fn render_until(&mut self, _us: f64, _out: &mut Vec<f32>) -> bool {
        false
    }
}

/// What an output opened.
pub(crate) struct Device {
    pub rate: u32,
    pub buffer: u32,
    pub name: String,
}

/// µs on the judge's clock from `epoch` to `t`.
pub(crate) fn us_since(epoch: Instant, t: Instant) -> f64 {
    if t >= epoch {
        t.duration_since(epoch).as_secs_f64() * 1e6
    } else {
        -(epoch.duration_since(t).as_secs_f64() * 1e6)
    }
}
