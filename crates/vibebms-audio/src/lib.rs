//! Audio for vibeBMS.
//!
//! * [`decode`]: decode WAV/OGG/MP3/FLAC into 16-bit PCM at the mixer rate,
//!   with BMS-style file name fallback (`a.wav` -> `a.ogg`).
//! * [`Mixer`]: one mixer (`voice`) with sample-accurate scheduling (BGM is
//!   queued ahead as moments on the judge's clock, turned into frames by
//!   the output's clock: see `clock`), on an output (`out`): a cpal stream,
//!   silence by the wall clock without a device, or on the web an
//!   AudioWorklet.

mod clock;
pub mod decode;
mod out;
mod voice;

pub use decode::{decode_bytes, decode_file, Sample, SoundBank};
pub use voice::Bus;

use out::{us_since, Device, Output};
use std::sync::Arc;
use voice::{Cmd, Due, Voice};
use web_time::Instant;

#[derive(Clone, Debug)]
pub struct AudioConfig {
    /// Requested buffer size in frames (0 = device default).
    pub buffer_frames: u32,
    /// Preferred sample rate (0 = device default).
    pub sample_rate: u32,
    pub master: f32,
    pub key_volume: f32,
    pub bgm_volume: f32,
    pub system_volume: f32,
    pub max_voices: usize,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            buffer_frames: 256,
            sample_rate: 0,
            master: 0.8,
            key_volume: 1.0,
            bgm_volume: 1.0,
            system_volume: 0.8,
            max_voices: 512,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlayOptions {
    /// Voices sharing a non-zero key are monophonic: a new one cuts the old one.
    pub key: u64,
    pub volume: f32,
    pub bus: Bus,
    /// Start so it sounds together with a key pressed at this moment (on
    /// the clock the judge uses; `None` = now). The output turns it into a
    /// frame by its own clock as the moment comes, so a device clock that
    /// drifts does not move it.
    pub at: Option<Instant>,
    /// Slice inside the sample (frames).
    pub offset: usize,
    pub end: Option<usize>,
    pub looping: bool,
}

impl Default for PlayOptions {
    fn default() -> Self {
        PlayOptions {
            key: 0,
            volume: 1.0,
            bus: Bus::Key,
            at: None,
            offset: 0,
            end: None,
            looping: false,
        }
    }
}

pub struct Mixer {
    out: Box<dyn Output>,
    /// What deadlines (`PlayOptions::at`) are counted from.
    epoch: Instant,
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub device_name: String,
}

impl Mixer {
    pub fn new(cfg: &AudioConfig) -> Mixer {
        let epoch = Instant::now();
        // the web: the mixer in an AudioWorklet (cpal where there is none)
        #[cfg(target_arch = "wasm32")]
        match out::web::WebOut::open(cfg, epoch) {
            Ok((w, dev)) => return Mixer::on(Box::new(w), dev, epoch),
            Err(e) => log::warn!("audio worklet unavailable ({e}); using cpal"),
        }
        let (out, dev) = out::cpal::CpalOut::open(cfg, epoch);
        Mixer::on(Box::new(out), dev, epoch)
    }

    fn on(out: Box<dyn Output>, dev: Device, epoch: Instant) -> Mixer {
        Mixer {
            out,
            epoch,
            sample_rate: dev.rate,
            buffer_frames: dev.buffer,
            device_name: dev.name,
        }
    }

    /// Call every frame: an output whose device went away (unplugged, a
    /// driver reset) is opened again on what is the default device now;
    /// meanwhile the mixer goes on silently with the clock, so the play
    /// does not stall.
    pub fn keep_alive(&mut self) {
        // the sounds are decoded at the rate in use: that stays
        if let Some(dev) = self.out.keep_alive() {
            self.buffer_frames = dev.buffer;
            self.device_name = dev.name;
        }
    }

    pub fn us_to_frames(&self, us: i64) -> i64 {
        us * self.sample_rate as i64 / 1_000_000
    }

    /// Output latency estimate in µs: on the web what the browser reports
    /// (the worklet's frames to the speaker), else one buffer.
    pub fn latency_us(&self) -> i64 {
        self.out.latency_us().unwrap_or_else(|| {
            self.buffer_frames as i64 * 1_000_000 / self.sample_rate.max(1) as i64
        })
    }

    pub fn play(&self, sample: &Arc<Sample>, o: PlayOptions) {
        let due = o.at.map(|t| Due::Us(us_since(self.epoch, t)));
        self.out.send(Cmd::Play(Voice::new(sample.clone(), &o, due)));
    }

    pub fn set_volume(&self, key: u64, vol: f32) {
        self.out.send(Cmd::Volume(key, vol));
    }
    pub fn stop(&self, key: u64) {
        self.out.send(Cmd::Stop(key));
    }
    pub fn stop_bus(&self, bus: Bus) {
        self.out.send(Cmd::StopBus(bus));
    }
    pub fn set_bus_volume(&self, bus: Bus, v: f32) {
        self.out.send(Cmd::BusVolume(bus, v));
    }
    pub fn set_master(&self, v: f32) {
        self.out.send(Cmd::Master(v));
    }

    /// Browsers start audio suspended until a user gesture; call on input.
    pub fn resume(&self) {
        self.out.resume();
    }
}
