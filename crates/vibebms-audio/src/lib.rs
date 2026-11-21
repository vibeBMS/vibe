//! Audio for vibeBMS.
//!
//! * [`loudness`]: BS.1770 loudness of a signal or of sounds mixed.
//! * [`decode`]: decode WAV/OGG/MP3/FLAC into 16-bit PCM at the mixer rate,
//!   with BMS-style file name fallback (`a.wav` -> `a.ogg`).
//! * [`Mixer`]: one mixer (`voice`) with sample-accurate scheduling (BGM is
//!   queued ahead as moments on the judge's clock, turned into frames by
//!   the output's clock: see `clock`), on an output (`out`): a cpal stream,
//!   silence by the wall clock without a device, or on the web an
//!   AudioWorklet; or offline, its frames made when asked for.

mod clock;
pub mod decode;
pub mod diag;
pub mod loudness;
mod out;
mod voice;

pub use decode::{
    decode_bytes, decode_bytes_within, decode_file, parallel_for, Budget, Sample, SoundBank,
};
pub use voice::{Bus, GROUPS};

use out::{us_since, Device, Output};
use std::sync::Arc;
use voice::{Cmd, Due, Voice};
use web_time::Instant;

/// How long the master volume takes to glide over its whole range
/// (`Mixer::set_master`): a mute fades out in this time.
pub const MASTER_GLIDE_MS: u32 = 100;

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
    /// Windows: the device in WASAPI exclusive mode (else shared).
    #[cfg(windows)]
    pub exclusive: bool,
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
            #[cfg(windows)]
            exclusive: false,
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
    /// Its volume group (`Mixer::set_group_volume`; 0: none).
    pub group: u8,
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
            group: 0,
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

    /// A mixer that plays nothing: its frames are made by
    /// [`Mixer::render_until`] (a video's sound track), at `rate`, frame 0
    /// at the moment it is made.
    pub fn offline(cfg: &AudioConfig, rate: u32) -> Mixer {
        let epoch = Instant::now();
        let rate = rate.max(1);
        let out = out::offline::OfflineOut::new(cfg, rate);
        let dev = Device {
            rate,
            buffer: 0,
            name: "offline".into(),
        };
        Mixer::on(Box::new(out), dev, epoch)
    }

    /// An offline mixer ([`Mixer::offline`]): its frames from the last
    /// call up to `until` appended to `out` (stereo, interleaved, clipped
    /// as an output plays them); `false` (nothing made) for one that plays
    /// to a device.
    pub fn render_until(&mut self, until: Instant, out: &mut Vec<f32>) -> bool {
        self.out.render_until(us_since(self.epoch, until), out)
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
    /// does not stall. A message for the player when the output did not
    /// open as asked (Windows: exclusive mode).
    pub fn keep_alive(&mut self) -> Option<String> {
        // the sounds are decoded at the rate in use: that stays
        if let Some(dev) = self.out.keep_alive() {
            self.buffer_frames = dev.buffer;
            self.device_name = dev.name;
        }
        self.out.take_notice()
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
        self.out
            .send(Cmd::Play(Voice::new(sample.clone(), &o, due)));
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
    /// A gain on bus `bus` besides its volume: the chart's sounds brought
    /// to a common loudness (`loudness`).
    pub fn set_bus_gain(&self, bus: Bus, g: f32) {
        self.out.send(Cmd::BusGain(bus, g));
    }
    /// The master volume: it glides there, full scale in
    /// [`MASTER_GLIDE_MS`] (0 fades the output out).
    pub fn set_master(&self, v: f32) {
        self.out.send(Cmd::Master(v));
    }
    /// The volume of every voice of group `g` (1..=`GROUPS`), those to
    /// come too: a NONSTOP course's stage fading in or out.
    pub fn set_group_volume(&self, g: u8, v: f32) {
        self.out.send(Cmd::GroupVolume(g, v));
    }

    /// Browsers start audio suspended until a user gesture; call on input.
    pub fn resume(&self) {
        self.out.resume();
    }
}
