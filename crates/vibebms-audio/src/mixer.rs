use crate::decode::Sample;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
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
    /// Absolute mixer frame to start at (`None` = immediately).
    pub start_frame: Option<u64>,
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
    StopAll,
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
}

impl State {
    fn handle(&mut self) {
        while let Ok(c) = self.rx.try_recv() {
            match c {
                Cmd::Play(v) => {
                    if v.key != 0 {
                        for o in self.voices.iter_mut().filter(|o| o.key == v.key) {
                            o.cut_at = o.cut_at.min(v.start_at.max(self.frame));
                        }
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
                Cmd::StopAll => self.voices.clear(),
                Cmd::BusVolume(b, v) => self.bus[b as usize] = v,
                Cmd::Master(v) => self.master = v,
            }
        }
    }

    /// Render `frames` stereo frames into `self.mix`.
    fn render(&mut self, frames: usize) {
        self.handle();
        if self.mix.len() < frames * 2 {
            self.mix.resize(frames * 2, 0.0);
        }
        let mix = &mut self.mix[..frames * 2];
        mix.fill(0.0);
        let now = self.frame;
        let end_frame = now + frames as u64;
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
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub device_name: String,
    _stream: Option<cpal::Stream>,
    /// Null output (no device): frames advance with wall-clock time.
    null_start: Option<Instant>,
    null_state: Option<std::sync::Mutex<State>>,
}

impl Mixer {
    pub fn new(cfg: &AudioConfig) -> Mixer {
        let (tx, rx) = channel();
        let frame = Arc::new(AtomicU64::new(0));
        let state = State {
            rx,
            voices: Vec::with_capacity(cfg.max_voices),
            frame: 0,
            bus: [cfg.key_volume, cfg.bgm_volume, cfg.system_volume],
            master: cfg.master,
            mix: vec![0.0; 8192],
            max_voices: cfg.max_voices.max(16),
            shared_frame: frame.clone(),
        };
        match open_stream(cfg, state) {
            Ok((stream, rate, buf, name)) => {
                log::info!("audio: {name} {rate} Hz, buffer {buf} frames");
                Mixer {
                    tx,
                    frame,
                    sample_rate: rate,
                    buffer_frames: buf,
                    device_name: name,
                    _stream: Some(stream),
                    null_start: None,
                    null_state: None,
                }
            }
            Err((e, state)) => {
                log::warn!("audio output unavailable ({e}); using silent output");
                Mixer {
                    tx,
                    frame,
                    sample_rate: 44100,
                    buffer_frames: 512,
                    device_name: "none".into(),
                    _stream: None,
                    null_start: Some(Instant::now()),
                    null_state: Some(std::sync::Mutex::new(*state)),
                }
            }
        }
    }

    /// Frames rendered so far (the mixer clock).
    pub fn current_frame(&self) -> u64 {
        if let (Some(start), Some(st)) = (self.null_start, &self.null_state) {
            let target = (start.elapsed().as_secs_f64() * self.sample_rate as f64) as u64;
            let mut s = st.lock().unwrap();
            while s.frame < target {
                let n = ((target - s.frame) as usize).min(4096);
                s.render(n);
            }
            return s.frame;
        }
        self.frame.load(Ordering::Acquire)
    }

    pub fn us_to_frames(&self, us: i64) -> i64 {
        us * self.sample_rate as i64 / 1_000_000
    }

    /// Output latency estimate in µs (one buffer).
    pub fn latency_us(&self) -> i64 {
        self.buffer_frames as i64 * 1_000_000 / self.sample_rate.max(1) as i64
    }

    pub fn play(&self, sample: &Arc<Sample>, o: PlayOptions) {
        let end = o.end.unwrap_or(sample.frames).min(sample.frames);
        let start = o.offset.min(end);
        let _ = self.tx.send(Cmd::Play(Voice {
            sample: sample.clone(),
            pos: start,
            start,
            end,
            start_at: o.start_frame.unwrap_or(0),
            cut_at: u64::MAX,
            vol: o.volume,
            key: o.key,
            bus: o.bus,
            looping: o.looping,
        }));
    }

    pub fn set_volume(&self, key: u64, vol: f32) {
        let _ = self.tx.send(Cmd::Volume(key, vol));
    }
    pub fn stop(&self, key: u64) {
        let _ = self.tx.send(Cmd::Stop(key));
    }
    pub fn stop_bus(&self, bus: Bus) {
        let _ = self.tx.send(Cmd::StopBus(bus));
    }
    pub fn stop_all(&self) {
        let _ = self.tx.send(Cmd::StopAll);
    }
    pub fn set_bus_volume(&self, bus: Bus, v: f32) {
        let _ = self.tx.send(Cmd::BusVolume(bus, v));
    }
    /// Browsers start audio suspended until a user gesture; call on input.
    pub fn resume(&self) {
        if let Some(s) = &self._stream {
            let _ = s.play();
        }
    }

    pub fn set_master(&self, v: f32) {
        let _ = self.tx.send(Cmd::Master(v));
    }
}

type OpenOk = (cpal::Stream, u32, u32, String);

fn open_stream(cfg: &AudioConfig, state: State) -> Result<OpenOk, (String, Box<State>)> {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        return Err(("no output device".into(), Box::new(state)));
    };
    let name = device.name().unwrap_or_else(|_| "output".into());
    let def = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => return Err((e.to_string(), Box::new(state))),
    };
    let mut rate = def.sample_rate().0;
    let fmt = def.sample_format();
    let mut buf_range = None;
    if let Ok(cfgs) = device.supported_output_configs() {
        for c in cfgs {
            if c.sample_format() != fmt || c.channels() != def.channels() {
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
    let mut config = cpal::StreamConfig {
        channels: def.channels(),
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
    let shared = std::sync::Arc::new(std::sync::Mutex::new(Some(state)));
    let try_build = |config: &cpal::StreamConfig| -> Result<cpal::Stream, String> {
        let st = shared.lock().unwrap().take().ok_or("state consumed")?;
        let r = match fmt {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, st),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, st),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, st),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, st),
            other => Err((
                format!("unsupported sample format {other:?}"),
                Box::new(st_dummy()),
            )),
        };
        match r {
            Ok(s) => Ok(s),
            Err((e, st)) => {
                *shared.lock().unwrap() = Some(*st);
                Err(e)
            }
        }
    };
    let stream = match try_build(&config) {
        Ok(s) => s,
        Err(e) if buf != 0 => {
            log::warn!("buffer size {buf} rejected ({e}); using device default");
            config.buffer_size = cpal::BufferSize::Default;
            buf = 0;
            match try_build(&config) {
                Ok(s) => s,
                Err(e) => {
                    return Err((
                        e,
                        Box::new(shared.lock().unwrap().take().unwrap_or_else(st_dummy)),
                    ))
                }
            }
        }
        Err(e) => {
            return Err((
                e,
                Box::new(shared.lock().unwrap().take().unwrap_or_else(st_dummy)),
            ))
        }
    };
    if let Err(e) = stream.play() {
        return Err((e.to_string(), Box::new(st_dummy())));
    }
    let buf = if buf == 0 { (rate / 100).max(256) } else { buf };
    Ok((stream, rate, buf, name))
}

fn st_dummy() -> State {
    let (_tx, rx) = channel();
    State {
        rx,
        voices: Vec::new(),
        frame: 0,
        bus: [1.0; 3],
        master: 1.0,
        mix: Vec::new(),
        max_voices: 16,
        shared_frame: Arc::new(AtomicU64::new(0)),
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: State,
) -> Result<cpal::Stream, (String, Box<State>)>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    // The state is moved into the callback; on failure we cannot get it back,
    // so build with a cell that the error path can reclaim.
    let cell = Arc::new(std::sync::Mutex::new(Some(state)));
    let cb_cell = cell.clone();
    let mut local: Option<State> = None;
    let result = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            if local.is_none() {
                local = cb_cell.lock().ok().and_then(|mut g| g.take());
            }
            let Some(st) = local.as_mut() else {
                data.iter_mut().for_each(|d| *d = T::from_sample(0.0));
                return;
            };
            let frames = data.len() / channels.max(1);
            st.render(frames);
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
        |e| log::error!("audio stream error: {e}"),
        None,
    );
    match result {
        Ok(s) => Ok(s),
        Err(e) => {
            let st = cell.lock().unwrap().take().unwrap_or_else(st_dummy);
            Err((e.to_string(), Box::new(st)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> (Sender<Cmd>, State) {
        let (tx, rx) = channel();
        let st = State {
            rx,
            voices: Vec::new(),
            frame: 0,
            bus: [1.0; 3],
            master: 1.0,
            mix: Vec::new(),
            max_voices: 64,
            shared_frame: Arc::new(AtomicU64::new(0)),
        };
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
