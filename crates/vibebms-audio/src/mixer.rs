use crate::clock::{FrameClock, Obs};
use crate::decode::Sample;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use web_time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    Key = 0,
    Bgm = 1,
    System = 2,
}

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
    /// Absolute mixer frame to start at (`None` = immediately). The web
    /// build schedules by it.
    pub start_frame: Option<u64>,
    /// Start so it sounds together with a key pressed at this moment (on
    /// the clock the judge uses); turned into a frame in the audio callback
    /// so a device clock that drifts does not move it. Native output only:
    /// it takes the place of `start_frame` there.
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
            start_frame: None,
            at: None,
            offset: 0,
            end: None,
            looping: false,
        }
    }
}

struct Voice {
    sample: Arc<Sample>,
    pos: usize,
    start: usize,
    end: usize,
    start_at: u64,
    /// A deadline (µs since the mixer's epoch) not yet turned into
    /// `start_at`.
    at_us: Option<f64>,
    cut_at: u64,
    vol: f32,
    key: u64,
    bus: Bus,
    looping: bool,
}

enum Cmd {
    Play(Voice),
    Volume(u64, f32),
    Stop(u64),
    StopBus(Bus),
    BusVolume(Bus, f32),
    Master(f32),
}

struct State {
    rx: Receiver<Cmd>,
    voices: Vec<Voice>,
    frame: u64,
    bus: [f32; 3],
    master: f32,
    mix: Vec<f32>,
    max_voices: usize,
    shared_frame: Arc<AtomicU64>,
    clock: FrameClock,
    /// What `Voice::at_us` counts from.
    epoch: Instant,
}

impl State {
    fn new(rx: Receiver<Cmd>, cfg: &AudioConfig, frame: Arc<AtomicU64>, epoch: Instant) -> State {
        State {
            rx,
            voices: Vec::with_capacity(cfg.max_voices),
            frame: 0,
            bus: [cfg.key_volume, cfg.bgm_volume, cfg.system_volume],
            master: cfg.master,
            mix: vec![0.0; 8192],
            max_voices: cfg.max_voices.max(16),
            shared_frame: frame,
            clock: FrameClock::new(44_100),
            epoch,
        }
    }

    /// µs on the judge's clock since the epoch.
    fn us(&self, t: Instant) -> f64 {
        us_since(self.epoch, t)
    }

    /// A voice of key `key` starts at `at`: the others of that key that
    /// start before end there (one voice per key).
    fn cut_key(&mut self, key: u64, at: u64, except: Option<usize>) {
        if key == 0 {
            return;
        }
        for (i, o) in self.voices.iter_mut().enumerate() {
            if o.key == key && o.at_us.is_none() && o.start_at <= at && Some(i) != except {
                o.cut_at = o.cut_at.min(at);
            }
        }
    }

    fn handle(&mut self) {
        while let Ok(c) = self.rx.try_recv() {
            match c {
                Cmd::Play(v) => {
                    if v.at_us.is_none() {
                        self.cut_key(v.key, v.start_at.max(self.frame), None);
                    }
                    if self.voices.len() >= self.max_voices {
                        // drop the voice that started first
                        if let Some(i) =
                            (0..self.voices.len()).min_by_key(|i| self.voices[*i].start_at)
                        {
                            self.voices.swap_remove(i);
                        }
                    }
                    self.voices.push(v);
                }
                Cmd::Volume(k, vol) => self
                    .voices
                    .iter_mut()
                    .filter(|v| v.key == k)
                    .for_each(|v| v.vol = vol),
                Cmd::Stop(k) => self.voices.retain(|v| v.key != k),
                Cmd::StopBus(b) => self.voices.retain(|v| v.bus != b),
                Cmd::BusVolume(b, v) => self.bus[b as usize] = v,
                Cmd::Master(v) => self.master = v,
            }
        }
    }

    /// Render `frames` stereo frames into `self.mix`.
    #[cfg(test)]
    fn render(&mut self, frames: usize) {
        self.render_obs(frames, None);
    }

    /// Render `frames` stereo frames into `self.mix`, with what the
    /// callback knows of the time (see `clock`).
    fn render_obs(&mut self, frames: usize, obs: Option<Obs>) {
        if let Some(o) = obs {
            self.clock.observe(o);
        }
        self.handle();
        if self.mix.len() < frames * 2 {
            self.mix.resize(frames * 2, 0.0);
        }
        let now = self.frame;
        let end_frame = now + frames as u64;
        // deadlines due in this buffer get their frame; one already past
        // starts now, from its beginning
        for i in 0..self.voices.len() {
            let Some(us) = self.voices[i].at_us else {
                continue;
            };
            let f = self.clock.frame_at(us);
            if f < end_frame as f64 {
                let at = (f.round().max(0.0) as u64).max(now);
                self.voices[i].start_at = at;
                self.voices[i].at_us = None;
                self.cut_key(self.voices[i].key, at, Some(i));
            }
        }
        let mix = &mut self.mix[..frames * 2];
        mix.fill(0.0);
        let bus = self.bus;
        let master = self.master;
        self.voices.retain_mut(|v| {
            if v.start_at >= end_frame {
                return true;
            }
            if v.cut_at <= now {
                return false;
            }
            let first = v.start_at.saturating_sub(now) as usize;
            let last = (v.cut_at.min(end_frame) - now) as usize;
            let g = v.vol * bus[v.bus as usize] * master;
            let mut i = first;
            while i < last {
                if v.pos >= v.end {
                    if v.looping && v.end > v.start {
                        v.pos = v.start;
                    } else {
                        return false;
                    }
                }
                let n = (last - i).min(v.end - v.pos);
                if g != 0.0 {
                    for k in 0..n {
                        let (l, r) = v.sample.frame(v.pos + k);
                        mix[(i + k) * 2] += l * g;
                        mix[(i + k) * 2 + 1] += r * g;
                    }
                }
                v.pos += n;
                i += n;
            }
            v.cut_at > end_frame && (v.pos < v.end || v.looping)
        });
        self.frame = end_frame;
        self.shared_frame.store(self.frame, Ordering::Release);
    }
}

fn us_since(epoch: Instant, t: Instant) -> f64 {
    if t >= epoch {
        t.duration_since(epoch).as_secs_f64() * 1e6
    } else {
        -(epoch.duration_since(t).as_secs_f64() * 1e6)
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    // transparent below ~0.8, smooth limiting above
    if x.abs() <= 0.8 {
        x
    } else {
        let s = x.signum();
        let a = x.abs() - 0.8;
        s * (0.8 + 0.2 * (a / (a + 0.2)))
    }
}

pub struct Mixer {
    tx: Sender<Cmd>,
    frame: Arc<AtomicU64>,
    /// What deadlines (`PlayOptions::at`) are counted from.
    epoch: Instant,
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub device_name: String,
    _stream: Option<cpal::Stream>,
    /// The mixer: rendered by the stream's callback, or here while there
    /// is no stream.
    state: Arc<Mutex<State>>,
    /// The stream reported its device gone.
    lost: Arc<AtomicBool>,
    cfg: AudioConfig,
    /// Null output (no device): frames advance with wall-clock time from
    /// this moment and frame.
    null_start: Option<(Instant, u64)>,
    /// When to try to open the output again (after it was lost).
    retry_at: Option<Instant>,
    /// The web build's output (its own clock of what is heard).
    #[cfg(target_arch = "wasm32")]
    web: Option<crate::web::WebOut>,
}

/// How long to wait between tries to reopen a lost output.
const RETRY: std::time::Duration = std::time::Duration::from_secs(3);

impl Mixer {
    pub fn new(cfg: &AudioConfig) -> Mixer {
        let (tx, rx) = channel();
        let frame = Arc::new(AtomicU64::new(0));
        let epoch = Instant::now();
        let state = Arc::new(Mutex::new(State::new(rx, cfg, frame.clone(), epoch)));
        let lost = Arc::new(AtomicBool::new(false));
        let mut m = Mixer {
            tx,
            frame,
            epoch,
            sample_rate: 44100,
            buffer_frames: 512,
            device_name: String::new(),
            _stream: None,
            state,
            lost,
            cfg: cfg.clone(),
            null_start: None,
            retry_at: None,
            #[cfg(target_arch = "wasm32")]
            web: None,
        };
        // the web: the mixer in an AudioWorklet (cpal where there is none)
        #[cfg(target_arch = "wasm32")]
        match crate::web::WebOut::open(cfg.max_voices.max(16)) {
            Ok(w) => {
                log::info!("audio: web worklet {} Hz", w.rate);
                w.set_bus_volume(Bus::Key, cfg.key_volume);
                w.set_bus_volume(Bus::Bgm, cfg.bgm_volume);
                w.set_bus_volume(Bus::System, cfg.system_volume);
                w.set_master(cfg.master);
                m.sample_rate = w.rate;
                m.buffer_frames = 128;
                m.device_name = "web".into();
                m.web = Some(w);
                return m;
            }
            Err(e) => log::warn!("audio worklet unavailable ({e}); using cpal"),
        }
        match open_stream(cfg, &m.state, &m.lost) {
            Ok((stream, rate, buf, name)) => {
                log::info!("audio: {name} {rate} Hz, buffer {buf} frames");
                m.sample_rate = rate;
                m.buffer_frames = buf;
                m.device_name = name;
                m._stream = Some(stream);
            }
            Err(e) => {
                log::warn!("audio output unavailable ({e}); using silent output");
                // the settings' Output row shows why
                m.device_name = format!("none ({e})");
                m.go_silent();
            }
        }
        m
    }

    /// Render by the wall clock from here on (no stream).
    fn go_silent(&mut self) {
        self._stream = None;
        let rate = self.sample_rate;
        let mut st = self.state.lock().unwrap();
        st.clock = FrameClock::new(rate);
        self.null_start = Some((Instant::now(), st.frame));
    }

    /// Call every frame: an output whose device went away (unplugged, a
    /// driver reset) is opened again on what is the default device now;
    /// meanwhile the mixer goes on silently with the clock, so the play
    /// does not stall.
    pub fn keep_alive(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if self.web.is_some() {
            return;
        }
        if self.lost.swap(false, Ordering::AcqRel) && self._stream.is_some() {
            log::warn!("audio: the output device is gone; reopening");
            self.go_silent();
            self.retry_at = Some(Instant::now());
        }
        if !self.retry_at.is_some_and(|t| Instant::now() >= t) {
            return;
        }
        // the sounds are decoded at the rate in use: keep it
        let cfg = AudioConfig {
            sample_rate: self.sample_rate,
            ..self.cfg.clone()
        };
        // frames rendered so far stay the clock's
        self.current_frame();
        match open_stream(&cfg, &self.state, &self.lost) {
            Ok((stream, rate, buf, name)) => {
                log::info!("audio: reopened {name} {rate} Hz, buffer {buf} frames");
                if rate != self.sample_rate {
                    log::warn!(
                        "audio: {rate} Hz now, sounds were decoded for {} Hz",
                        self.sample_rate
                    );
                }
                self.buffer_frames = buf;
                self.device_name = name;
                self._stream = Some(stream);
                self.null_start = None;
                self.retry_at = None;
            }
            Err(e) => {
                log::debug!("audio: reopening failed ({e})");
                self.retry_at = Some(Instant::now() + RETRY);
            }
        }
    }

    /// Frames rendered so far (the mixer clock).
    pub fn current_frame(&self) -> u64 {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = &self.web {
            return w.current_frame();
        }
        if let Some((start, base)) = self.null_start {
            let target = base + (start.elapsed().as_secs_f64() * self.sample_rate as f64) as u64;
            let mut s = self.state.lock().unwrap();
            let start_us = s.us(start);
            while s.frame < target {
                let n = ((target - s.frame) as usize).min(4096);
                // the frames are the clock here
                let t = start_us + (s.frame - base) as f64 * 1e6 / self.sample_rate as f64;
                let obs = Obs {
                    frame: s.frame,
                    frames: n,
                    stream_us: Some(t),
                    now_us: t,
                };
                s.render_obs(n, Some(obs));
            }
            return s.frame;
        }
        self.frame.load(Ordering::Acquire)
    }

    pub fn us_to_frames(&self, us: i64) -> i64 {
        us * self.sample_rate as i64 / 1_000_000
    }

    /// The mixer frame a sound played `us` from now starts on: where to
    /// schedule a chart's sounds so they meet the keysounds of keys pressed
    /// on time (which start when played). Both are heard the output latency
    /// later.
    pub fn frame_in(&self, us: i64) -> i64 {
        self.current_frame() as i64 + self.us_to_frames(us)
    }

    /// Output latency estimate in µs: on the web what the browser reports
    /// (the worklet's frames to the speaker), else one buffer.
    pub fn latency_us(&self) -> i64 {
        #[cfg(target_arch = "wasm32")]
        if let Some(h) = self.web.as_ref().and_then(|w| w.heard_frame()) {
            let ahead = self.current_frame() as f64 - h;
            return (ahead.max(0.0) * 1_000_000.0 / self.sample_rate.max(1) as f64) as i64;
        }
        self.buffer_frames as i64 * 1_000_000 / self.sample_rate.max(1) as i64
    }

    pub fn play(&self, sample: &Arc<Sample>, o: PlayOptions) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = &self.web {
            return w.play(sample, &o);
        }
        let end = o.end.unwrap_or(sample.frames).min(sample.frames);
        let start = o.offset.min(end);
        let at_us = o.at.map(|t| us_since(self.epoch, t));
        let _ = self.tx.send(Cmd::Play(Voice {
            sample: sample.clone(),
            pos: start,
            start,
            end,
            start_at: match at_us {
                Some(_) => u64::MAX,
                None => o.start_frame.unwrap_or(0),
            },
            at_us,
            cut_at: u64::MAX,
            vol: o.volume,
            key: o.key,
            bus: o.bus,
            looping: o.looping,
        }));
    }

    pub fn set_volume(&self, key: u64, vol: f32) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = &self.web {
            return w.set_volume(key, vol);
        }
        let _ = self.tx.send(Cmd::Volume(key, vol));
    }
    pub fn stop(&self, key: u64) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = &self.web {
            return w.stop(key);
        }
        let _ = self.tx.send(Cmd::Stop(key));
    }
    pub fn stop_bus(&self, bus: Bus) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = &self.web {
            return w.stop_bus(bus);
        }
        let _ = self.tx.send(Cmd::StopBus(bus));
    }
    pub fn set_bus_volume(&self, bus: Bus, v: f32) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = &self.web {
            return w.set_bus_volume(bus, v);
        }
        let _ = self.tx.send(Cmd::BusVolume(bus, v));
    }
    /// Browsers start audio suspended until a user gesture; call on input.
    pub fn resume(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = &self.web {
            w.resume();
        }
        if let Some(s) = &self._stream {
            let _ = s.play();
        }
    }

    pub fn set_master(&self, v: f32) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = &self.web {
            return w.set_master(v);
        }
        let _ = self.tx.send(Cmd::Master(v));
    }
}

type OpenOk = (cpal::Stream, u32, u32, String);

/// An output stream on the default device that renders `state`; `lost` is
/// set when its device goes away.
fn open_stream(
    cfg: &AudioConfig,
    state: &Arc<Mutex<State>>,
    lost: &Arc<AtomicBool>,
) -> Result<OpenOk, String> {
    #[cfg(target_os = "ios")]
    crate::ios::activate(cfg.buffer_frames);
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        return Err("no output device".into());
    };
    let name = device.name().unwrap_or_else(|_| "output".into());
    let def = match device.default_output_config() {
        Ok(c) => c,
        // iOS: the output unit's format may not be readable before it
        // runs; it takes 48 kHz stereo f32 all the same
        Err(e) if cfg!(target_os = "ios") => {
            log::warn!("audio: no default output config ({e}); trying 48 kHz stereo");
            cpal::SupportedStreamConfig::new(
                2,
                cpal::SampleRate(48_000),
                cpal::SupportedBufferSize::Unknown,
                cpal::SampleFormat::F32,
            )
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut rate = def.sample_rate().0;
    let fmt = def.sample_format();
    let mut buf_range = None;
    if let Ok(cfgs) = device.supported_output_configs() {
        for c in cfgs {
            if c.sample_format() != fmt || (c.channels() != def.channels() && def.channels() != 0) {
                continue;
            }
            if cfg.sample_rate != 0
                && c.min_sample_rate().0 <= cfg.sample_rate
                && c.max_sample_rate().0 >= cfg.sample_rate
            {
                rate = cfg.sample_rate;
            }
            if let cpal::SupportedBufferSize::Range { min, max } = c.buffer_size() {
                buf_range = Some((*min, *max));
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
    state.lock().unwrap().clock = FrameClock::new(rate);
    let mut config = cpal::StreamConfig {
        channels,
        sample_rate: cpal::SampleRate(rate),
        buffer_size: cpal::BufferSize::Default,
    };
    let mut buf = 0;
    if cfg.buffer_frames > 0 {
        let b = match buf_range {
            Some((min, max)) => cfg.buffer_frames.clamp(min, max.max(min)),
            None => cfg.buffer_frames,
        };
        config.buffer_size = cpal::BufferSize::Fixed(b);
        buf = b;
    }
    let try_build = |config: &cpal::StreamConfig| -> Result<cpal::Stream, String> {
        let (st, l) = (state.clone(), lost.clone());
        match fmt {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, st, l),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, st, l),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, st, l),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, st, l),
            other => Err(format!("unsupported sample format {other:?}")),
        }
    };
    let stream = match try_build(&config) {
        Ok(s) => s,
        Err(e) if buf != 0 => {
            log::warn!("buffer size {buf} rejected ({e}); using device default");
            config.buffer_size = cpal::BufferSize::Default;
            buf = 0;
            try_build(&config)?
        }
        Err(e) => return Err(e),
    };
    stream.play().map_err(|e| e.to_string())?;
    let buf = if buf == 0 { (rate / 100).max(256) } else { buf };
    Ok((stream, rate, buf, name))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<Mutex<State>>,
    lost: Arc<AtomicBool>,
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
                let Ok(mut st) = state.try_lock() else {
                    data.iter_mut().for_each(|d| *d = T::from_sample(0.0));
                    return;
                };
                let frames = data.len() / channels.max(1);
                let cb = info.timestamp().callback;
                let base = *base.get_or_insert(cb);
                let obs = Obs {
                    frame: st.frame,
                    frames,
                    stream_us: cb.duration_since(&base).map(|d| d.as_secs_f64() * 1e6),
                    now_us: st.us(Instant::now()),
                };
                st.render_obs(frames, Some(obs));
                for f in 0..frames {
                    let l = soft_clip(st.mix[f * 2]);
                    let r = soft_clip(st.mix[f * 2 + 1]);
                    let base = f * channels;
                    match channels {
                        1 => data[base] = T::from_sample((l + r) * 0.5),
                        _ => {
                            data[base] = T::from_sample(l);
                            data[base + 1] = T::from_sample(r);
                            for c in 2..channels {
                                data[base + c] = T::from_sample(0.0);
                            }
                        }
                    }
                }
            },
            move |e| {
                log::error!("audio stream error: {e}");
                if matches!(e, cpal::StreamError::DeviceNotAvailable) {
                    lost.store(true, Ordering::Release);
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> (Sender<Cmd>, State) {
        let (tx, rx) = channel();
        let cfg = AudioConfig {
            master: 1.0,
            key_volume: 1.0,
            bgm_volume: 1.0,
            system_volume: 1.0,
            max_voices: 64,
            ..Default::default()
        };
        let st = State::new(rx, &cfg, Arc::new(AtomicU64::new(0)), Instant::now());
        (tx, st)
    }

    fn sample(frames: usize, v: i16) -> Arc<Sample> {
        Arc::new(Sample {
            channels: 1,
            frames,
            data: vec![v; frames],
        })
    }

    fn voice(s: &Arc<Sample>, start_at: u64, key: u64) -> Voice {
        Voice {
            sample: s.clone(),
            pos: 0,
            start: 0,
            end: s.frames,
            start_at,
            at_us: None,
            cut_at: u64::MAX,
            vol: 1.0,
            key,
            bus: Bus::Key,
            looping: false,
        }
    }

    #[test]
    fn scheduled_start_is_sample_accurate() {
        let (tx, mut st) = state();
        let s = sample(10, 16384);
        tx.send(Cmd::Play(voice(&s, 5, 0))).unwrap();
        st.render(8);
        assert_eq!(st.mix[2 * 4], 0.0);
        assert!((st.mix[2 * 5] - 0.5).abs() < 1e-3);
        st.render(8);
        // frames 8..15 continue the voice (frames 3..9 of sample at 8..12)
        assert!((st.mix[2 * 6] - 0.5).abs() < 1e-3);
        assert_eq!(st.mix[2 * 7], 0.0);
        assert!(st.voices.is_empty());
    }

    /// Callbacks of a device `ppm` off the system clock for ten minutes;
    /// a sound due every 7 s, sent 150 ms ahead: each starts where a key
    /// pressed at its moment would (half a buffer later on average), not
    /// where the nominal rate would put it.
    #[test]
    fn deadlines_stay_on_the_judge_clock_with_a_drifting_device() {
        for ppm in [-200.0f64, 200.0] {
            let (tx, mut st) = state();
            st.clock = FrameClock::new(48_000);
            let true_rate = 48_000.0 * (1.0 + ppm * 1e-6);
            let s = sample(4, 16384);
            let buf = 256usize;
            let mut next = 1_000_000.0f64;
            let mut key = 1u64;
            let mut due: Vec<(u64, f64)> = Vec::new();
            let mut worst: f64 = 0.0;
            while (st.frame as f64) < true_rate * 600.0 {
                let t = st.frame as f64 * 1e6 / true_rate;
                if t + 150_000.0 >= next {
                    let mut v = voice(&s, u64::MAX, key);
                    v.at_us = Some(next);
                    tx.send(Cmd::Play(v)).unwrap();
                    due.push((key, next));
                    key += 1;
                    next += 7_000_000.0;
                }
                let obs = Obs {
                    frame: st.frame,
                    frames: buf,
                    stream_us: Some(t + 1e9),
                    now_us: t + 300.0,
                };
                st.render_obs(buf, Some(obs));
                due.retain(|(k, d)| match st.voices.iter().find(|v| v.key == *k) {
                    Some(v) if v.at_us.is_none() => {
                        let heard = v.start_at as f64 * 1e6 / true_rate;
                        // the callbacks run 300 us after their buffers'
                        // timestamps: a key's sound starts that much sooner
                        let want = d + buf as f64 * 0.5e6 / 48_000.0 - 300.0;
                        worst = worst.max((heard - want).abs());
                        false
                    }
                    Some(_) => true,
                    // started and done within one buffer
                    None => false,
                });
            }
            assert!(key > 80);
            // nominal-rate frames would be 120 ms off after 10 minutes
            assert!(worst < 200.0, "{ppm} ppm: {worst} us");
        }
    }

    #[test]
    fn a_late_deadline_starts_now_from_its_beginning() {
        let (tx, mut st) = state();
        st.render(480);
        let s = sample(100, 16384);
        let mut v = voice(&s, u64::MAX, 3);
        // due at frame 0 (long past)
        v.at_us = Some(0.0);
        tx.send(Cmd::Play(v)).unwrap();
        st.render(10);
        assert_eq!(st.voices[0].start_at, 480);
        assert_eq!(st.voices[0].pos, 10);
        assert!((st.mix[0] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn a_scheduled_voice_is_cut_only_by_a_later_one() {
        let (tx, mut st) = state();
        let s = sample(1000, 16384);
        // due at frame 441 (10 ms at the nominal 44.1 kHz)
        let mut v = voice(&s, u64::MAX, 9);
        v.at_us = Some(10_000.0);
        tx.send(Cmd::Play(v)).unwrap();
        // the same sound played now does not cancel the scheduled one
        tx.send(Cmd::Play(voice(&s, 0, 9))).unwrap();
        st.render(100);
        assert_eq!(st.voices.len(), 2);
        st.render(400);
        // the scheduled one started and cut the first there
        assert_eq!(st.voices.len(), 1);
        assert_eq!(st.voices[0].start_at, 441);
        assert!((st.mix[2 * 340] - 0.5).abs() < 1e-3);
        assert!((st.mix[2 * 341] - 0.5).abs() < 1e-3);
    }

    /// The output is lost: the mixer goes on silently from the frames
    /// played, by the wall clock, and tries to reopen (here there may be
    /// no device: it then waits to try again).
    #[test]
    fn a_lost_output_goes_on_silently_and_reopens() {
        let mut m = Mixer::new(&AudioConfig::default());
        let before = m.current_frame();
        m.go_silent();
        m.retry_at = Some(Instant::now());
        std::thread::sleep(std::time::Duration::from_millis(30));
        let f = m.current_frame();
        assert!(f >= before + m.sample_rate as u64 / 100, "{before} -> {f}");
        // a sound played meanwhile is taken (and rendered) by the clock
        let s = sample(10, 16384);
        m.play(&s, PlayOptions::default());
        std::thread::sleep(std::time::Duration::from_millis(5));
        m.current_frame();
        assert!(m.state.lock().unwrap().voices.is_empty());
        m.keep_alive();
        match m._stream {
            Some(_) => assert!(m.null_start.is_none() && m.retry_at.is_none()),
            None => {
                assert!(m.retry_at.is_some_and(|t| t > Instant::now()));
                assert!(m.current_frame() >= f);
            }
        }
    }

    #[test]
    fn same_key_cuts_previous_voice() {
        let (tx, mut st) = state();
        let s = sample(100, 16384);
        tx.send(Cmd::Play(voice(&s, 0, 7))).unwrap();
        tx.send(Cmd::Play(voice(&s, 10, 7))).unwrap();
        st.render(20);
        // before frame 10 one voice, afterwards still exactly one voice
        assert!((st.mix[2 * 5] - 0.5).abs() < 1e-3);
        assert!((st.mix[2 * 15] - 0.5).abs() < 1e-3);
        assert_eq!(st.voices.len(), 1);
    }
}
