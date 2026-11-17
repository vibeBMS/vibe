//! Where the mixer's frames go: a cpal stream (`cpal`, silent by the wall
//! clock while there is no device: `null`), or on the web an AudioWorklet
//! (`web`, the worklet's side in `worklet`).

pub(crate) mod cpal;
#[cfg(target_os = "ios")]
mod ios;
mod null;
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
