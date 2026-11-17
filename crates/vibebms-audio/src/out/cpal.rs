//! The mixer on a cpal output stream, rendered in its callback. A device
//! that goes away is opened again; meanwhile, and without any device, the
//! mixer goes on silently by the wall clock (`null`).

use super::null::{Silent, SILENT_BUFFER, SILENT_RATE};
use super::{us_since, Device, Output};
use crate::clock::{FrameClock, Obs};
use crate::decode::Sample;
use crate::voice::{soft_clip, Cmd, State};
use crate::AudioConfig;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use web_time::Instant;

/// How long to wait between tries to reopen a lost output.
const RETRY: std::time::Duration = std::time::Duration::from_secs(3);

/// The mixer and its commands: rendered by the stream's callback, or by
/// `keep_alive` while there is no stream.
pub(super) struct Mix {
    rx: Receiver<Cmd>,
    pub state: State,
}

impl Mix {
    pub fn render(&mut self, frames: usize, obs: Option<Obs>) -> &[f32] {
        for c in self.rx.try_iter() {
            self.state.apply(c);
        }
        self.state.render(frames, obs)
    }
}

pub(crate) struct CpalOut {
    tx: Sender<Cmd>,
    mix: Arc<Mutex<Mix>>,
    stream: Option<cpal::Stream>,
    /// The stream reported its device gone.
    lost: Arc<AtomicBool>,
    /// Samples of the voices that ended, to drop here.
    retired: Receiver<Arc<Sample>>,
    cfg: AudioConfig,
    /// The rate the sounds are decoded at.
    rate: u32,
    epoch: Instant,
    /// No stream: rendering by the wall clock.
    silent: Option<Silent>,
    /// When to try to open the output again (after it was lost).
    retry_at: Option<Instant>,
}

impl CpalOut {
    pub fn open(cfg: &AudioConfig, epoch: Instant) -> (CpalOut, Device) {
        let (tx, rx) = channel();
        let (retire, retired) = sync_channel(cfg.max_voices.max(16) * 2);
        let state = State::new(cfg, SILENT_RATE, retire);
        let mut out = CpalOut {
            tx,
            mix: Arc::new(Mutex::new(Mix { rx, state })),
            stream: None,
            lost: Arc::new(AtomicBool::new(false)),
            retired,
            cfg: cfg.clone(),
            rate: SILENT_RATE,
            epoch,
            silent: None,
            retry_at: None,
        };
        match open_stream(cfg, &out.mix, &out.lost, epoch) {
            Ok((stream, dev)) => {
                log::info!(
                    "audio: {} {} Hz, buffer {} frames",
                    dev.name,
                    dev.rate,
                    dev.buffer
                );
                out.rate = dev.rate;
                out.stream = Some(stream);
                (out, dev)
            }
            Err(e) => {
                log::warn!("audio output unavailable ({e}); using silent output");
                out.go_silent();
                let dev = Device {
                    rate: SILENT_RATE,
                    buffer: SILENT_BUFFER,
                    // the settings' Output row shows why
                    name: format!("none ({e})"),
                };
                (out, dev)
            }
        }
    }

    fn go_silent(&mut self) {
        self.stream = None;
        let mut mix = self.mix.lock().unwrap();
        self.silent = Some(Silent::start(&mut mix, self.rate));
    }
}

impl Output for CpalOut {
    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    fn keep_alive(&mut self) -> Option<Device> {
        self.retired.try_iter().for_each(drop);
        if self.lost.swap(false, Ordering::AcqRel) && self.stream.is_some() {
            log::warn!("audio: the output device is gone; reopening");
            self.go_silent();
            self.retry_at = Some(Instant::now());
        }
        if let Some(s) = &self.silent {
            s.render(&mut self.mix.lock().unwrap(), self.epoch);
        }
        if !self.retry_at.is_some_and(|t| Instant::now() >= t) {
            return None;
        }
        // the sounds are decoded at the rate in use: keep it
        let cfg = AudioConfig {
            sample_rate: self.rate,
            ..self.cfg.clone()
        };
        match open_stream(&cfg, &self.mix, &self.lost, self.epoch) {
            Ok((stream, dev)) => {
                log::info!(
                    "audio: reopened {} {} Hz, buffer {} frames",
                    dev.name,
                    dev.rate,
                    dev.buffer
                );
                if dev.rate != self.rate {
                    log::warn!(
                        "audio: {} Hz now, sounds were decoded for {} Hz",
                        dev.rate,
                        self.rate
                    );
                }
                self.stream = Some(stream);
                self.silent = None;
                self.retry_at = None;
                Some(dev)
            }
            Err(e) => {
                log::debug!("audio: reopening failed ({e})");
                self.retry_at = Some(Instant::now() + RETRY);
                None
            }
        }
    }

    fn resume(&self) {
        if let Some(s) = &self.stream {
            let _ = s.play();
        }
    }
}

/// An output stream on the default device that renders `mix`; `lost` is
/// set when its device goes away.
fn open_stream(
    cfg: &AudioConfig,
    mix: &Arc<Mutex<Mix>>,
    lost: &Arc<AtomicBool>,
    epoch: Instant,
) -> Result<(cpal::Stream, Device), String> {
    #[cfg(target_os = "ios")]
    super::ios::activate(cfg.buffer_frames);
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        return Err("no output device".into());
    };
    let name = device
        .description()
        .map_or_else(|_| "output".into(), |d| d.name().to_owned());
    let def = match device.default_output_config() {
        Ok(c) => c,
        // iOS: the output unit's format may not be readable before it
        // runs; it takes 48 kHz stereo f32 all the same
        Err(e) if cfg!(target_os = "ios") => {
            log::warn!("audio: no default output config ({e}); trying 48 kHz stereo");
            cpal::SupportedStreamConfig::new(
                2,
                48_000,
                cpal::SupportedBufferSize::Unknown,
                cpal::SampleFormat::F32,
            )
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut rate = def.sample_rate();
    let fmt = def.sample_format();
    let mut buf_range = None;
    if let Ok(cfgs) = device.supported_output_configs() {
        for c in cfgs {
            if c.sample_format() != fmt || (c.channels() != def.channels() && def.channels() != 0) {
                continue;
            }
            if cfg.sample_rate != 0
                && c.min_sample_rate() <= cfg.sample_rate
                && c.max_sample_rate() >= cfg.sample_rate
            {
                rate = cfg.sample_rate;
            }
            // (0..=0: the backend does not know)
            if let cpal::SupportedBufferSize::Range { min, max } = c.buffer_size() {
                if *max > 0 {
                    buf_range = Some((*min, *max));
                }
            }
        }
    }
    // iOS: cpal reads the output unit's format before it runs, which may
    // say 0 Hz / 0 channels; the unit converts from whatever it is given
    if rate == 0 {
        rate = if cfg.sample_rate != 0 {
            cfg.sample_rate
        } else {
            48_000
        };
    }
    let channels = if def.channels() == 0 {
        2
    } else {
        def.channels()
    };
    // a new stream: its clock is fitted anew
    mix.lock().unwrap().state.clock = FrameClock::new(rate);
    let mut config = cpal::StreamConfig {
        channels,
        sample_rate: rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let mut buf = 0;
    // iOS: no fixed buffer size: the session's preferred I/O buffer
    // duration, set above, sizes it (cpal would take only 256..=4096
    // frames there, and set that duration itself)
    if cfg.buffer_frames > 0 && !cfg!(target_os = "ios") {
        let b = match buf_range {
            Some((min, max)) => cfg.buffer_frames.clamp(min, max.max(min)),
            None => cfg.buffer_frames,
        };
        config.buffer_size = cpal::BufferSize::Fixed(b);
        buf = b;
    }
    let try_build = |config: cpal::StreamConfig| -> Result<cpal::Stream, String> {
        let (m, l) = (mix.clone(), lost.clone());
        // (cpal ranks I24 and F64 above I16 for a device's default)
        match fmt {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, m, l, epoch),
            cpal::SampleFormat::F64 => build::<f64>(&device, config, m, l, epoch),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, m, l, epoch),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, m, l, epoch),
            cpal::SampleFormat::I24 => build::<cpal::I24>(&device, config, m, l, epoch),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, m, l, epoch),
            other => Err(format!("unsupported sample format {other:?}")),
        }
    };
    let stream = match try_build(config) {
        Ok(s) => s,
        Err(e) if matches!(config.buffer_size, cpal::BufferSize::Fixed(_)) => {
            log::warn!("buffer size {buf} rejected ({e}); using device default");
            config.buffer_size = cpal::BufferSize::Default;
            buf = 0;
            try_build(config)?
        }
        Err(e) => return Err(e),
    };
    stream.play().map_err(|e| e.to_string())?;
    let buffer = if buf == 0 { (rate / 100).max(256) } else { buf };
    Ok((stream, Device { rate, buffer, name }))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mix: Arc<Mutex<Mix>>,
    lost: Arc<AtomicBool>,
    epoch: Instant,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    // the stream's first callback timestamp: its clock's origin
    let mut base: Option<cpal::StreamInstant> = None;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                // the main thread holds it only to reopen or render silently
                let Ok(mut m) = mix.try_lock() else {
                    data.iter_mut().for_each(|d| *d = T::from_sample(0.0));
                    return;
                };
                let frames = data.len() / channels.max(1);
                let cb = info.timestamp().callback;
                let base = *base.get_or_insert(cb);
                let obs = Obs {
                    frame: m.state.frame,
                    frames,
                    stream_us: cb
                        .checked_duration_since(base)
                        .map(|d| d.as_secs_f64() * 1e6),
                    now_us: us_since(epoch, Instant::now()),
                };
                let mix = m.render(frames, Some(obs));
                for (o, d) in mix.chunks_exact(2).zip(data.chunks_exact_mut(channels)) {
                    let (l, r) = (soft_clip(o[0]), soft_clip(o[1]));
                    if channels == 1 {
                        d[0] = T::from_sample((l + r) * 0.5);
                    } else {
                        d[0] = T::from_sample(l);
                        d[1] = T::from_sample(r);
                        d[2..].iter_mut().for_each(|x| *x = T::from_sample(0.0));
                    }
                }
            },
            move |e| match e.kind() {
                // the stream goes on
                cpal::ErrorKind::Xrun => log::debug!("audio stream: {e}"),
                // the default output's new device, which it follows
                cpal::ErrorKind::DeviceChanged => log::info!("audio stream: {e}"),
                // it plays no more: opened again
                cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::StreamInvalidated => {
                    log::error!("audio stream error: {e}");
                    lost.store(true, Ordering::Release);
                }
                _ => log::error!("audio stream error: {e}"),
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::tests::sample;
    use crate::voice::{Due, Voice};
    use crate::PlayOptions;
    use std::time::Duration;

    fn silent() -> CpalOut {
        let (mut out, _) = CpalOut::open(&AudioConfig::default(), Instant::now());
        out.go_silent();
        out
    }

    fn voices(out: &CpalOut) -> Vec<(u64, usize)> {
        let m = out.mix.lock().unwrap();
        m.state.voices.iter().map(|v| (v.start_at, v.pos)).collect()
    }

    /// The output is lost: the mixer goes on silently from the frames
    /// played, by the wall clock, and tries to reopen (here there may be
    /// no device: it then waits to try again).
    #[test]
    fn a_lost_output_goes_on_silently_and_reopens() {
        let mut out = silent();
        let before = out.mix.lock().unwrap().state.frame;
        out.retry_at = Some(Instant::now() + Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(30));
        out.keep_alive();
        let f = out.mix.lock().unwrap().state.frame;
        assert!(f >= before + out.rate as u64 / 100, "{before} -> {f}");
        // a sound played meanwhile is taken (and rendered) by the clock
        out.send(Cmd::Play(Voice::new(
            sample(10, 1),
            &PlayOptions::default(),
            None,
        )));
        std::thread::sleep(Duration::from_millis(5));
        out.keep_alive();
        assert!(voices(&out).is_empty());
        out.retry_at = Some(Instant::now());
        out.keep_alive();
        match out.stream {
            Some(_) => assert!(out.silent.is_none() && out.retry_at.is_none()),
            None => assert!(out.retry_at.is_some_and(|t| t > Instant::now())),
        }
    }

    /// Silent output not asked for ten seconds renders only the last one.
    #[test]
    fn a_long_silent_gap_renders_only_its_last_second() {
        let mut out = silent();
        let rate = out.rate as u64;
        let o = PlayOptions::default();
        out.send(Cmd::Play(Voice::new(
            sample(20 * rate as usize, 1),
            &o,
            None,
        )));
        out.keep_alive();
        let f = out.mix.lock().unwrap().state.frame;
        out.silent = Some(Silent {
            since: Instant::now() - Duration::from_secs(10),
            base: f,
            rate: out.rate,
        });
        out.keep_alive();
        assert!(out.mix.lock().unwrap().state.frame >= f + 10 * rate);
        let pos = voices(&out)[0].1 as u64;
        assert!((rate..2 * rate).contains(&pos), "{pos}");
    }

    /// Without a device a sound due at a moment starts on the frame of that
    /// moment (half a buffer later, as a key's sound would).
    #[test]
    fn the_silent_output_starts_a_sound_at_its_moment() {
        let mut out = silent();
        let rate = out.rate as f64;
        out.keep_alive();
        let (since, base) = out.silent.as_ref().map(|s| (s.since, s.base)).unwrap();
        let at = since + Duration::from_millis(20);
        let due = Due::Us(us_since(out.epoch, at));
        let o = PlayOptions::default();
        out.send(Cmd::Play(Voice::new(
            sample(rate as usize, 1),
            &o,
            Some(due),
        )));
        while Instant::now() < at + Duration::from_millis(30) {
            std::thread::sleep(Duration::from_millis(5));
            out.keep_alive();
        }
        let start = voices(&out)[0].0 as f64;
        // (the last silent buffer may be short: half of it, or less)
        let want = base as f64 + 0.020 * rate;
        assert!(
            (want..=want + SILENT_BUFFER as f64 / 2.0 + 1.0).contains(&start),
            "{start} vs {want}"
        );
    }
}
