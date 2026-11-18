//! The WASAPI stream: opened and rendered on a thread of its own, woken by
//! the device's event. It renders as the cpal stream does (`Mix`), with the
//! callback's time from the stream's IAudioClock as cpal takes it there,
//! so the frame clock is fitted the same way.

use super::{exclusive_formats, exclusive_periods, shared_period, write, Format, Kind, Mode};
use crate::clock::{FrameClock, Obs};
use crate::out::cpal::Mix;
use crate::out::{us_since, Device};
use crate::AudioConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use wasapi::{AudioClient, Direction, SampleType, StreamMode, TimingMode, WaveFormat};
use web_time::Instant;

/// `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED`: the device takes the period only
/// aligned to its own steps.
const NOT_ALIGNED: i32 = 0x8889_0019_u32 as i32;

/// The device gave no event this long (ms): it is gone.
const SILENCE_MS: u32 = 1000;

/// The rendering thread; stopped when dropped.
pub(crate) struct Stream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn text(e: wasapi::WasapiError) -> String {
    e.to_string()
}

impl Format {
    fn wave(self) -> WaveFormat {
        let (store, valid, float) = self.kind.bits();
        let t = if float {
            SampleType::Float
        } else {
            SampleType::Int
        };
        WaveFormat::new(
            store as usize,
            valid as usize,
            t,
            self.rate as usize,
            self.channels as usize,
            None,
        )
    }

    fn of(w: &WaveFormat) -> Option<Format> {
        let float = matches!(w.get_subformat().ok()?, SampleType::Float);
        Some(Format {
            rate: w.get_samplespersec(),
            channels: w.get_nchannels(),
            kind: Kind::of(w.get_bitspersample(), w.get_validbitspersample(), float)?,
        })
    }
}

/// A client set up: the format it plays, its period (frames) and how it
/// was opened.
struct Opened {
    client: AudioClient,
    fmt: Format,
    period: u32,
    how: &'static str,
}

/// The mixer's rate: as asked, else the engine's.
fn rate(cfg: &AudioConfig, mix: Option<Format>) -> u32 {
    match cfg.sample_rate {
        0 => mix.map_or(48_000, |m| m.rate),
        r => r,
    }
}

fn exclusive(dev: &wasapi::Device, cfg: &AudioConfig) -> Result<Opened, String> {
    let probe = dev.get_iaudioclient().map_err(text)?;
    let mix = probe.get_mixformat().ok().as_ref().and_then(Format::of);
    let own: Vec<Format> = [
        dev.get_device_format().ok().as_ref().and_then(Format::of),
        mix,
    ]
    .into_iter()
    .flatten()
    .collect();
    let (fmt, wave) = exclusive_formats(rate(cfg, mix), &own)
        .into_iter()
        .find_map(|f| Some((f, probe.is_supported_exclusive_with_quirks(&f.wave()).ok()?)))
        .ok_or("the device takes none of the formats tried")?;
    let (default, min) = probe.get_device_period().map_err(text)?;
    let mut last = String::from("no period");
    for p in exclusive_periods(min, default) {
        let mut period = probe
            .calculate_aligned_period_near(p, Some(128), &wave)
            .map_err(text)?;
        for _ in 0..2 {
            let mut client = dev.get_iaudioclient().map_err(text)?;
            let mode = StreamMode::EventsExclusive { period_hns: period };
            match client.initialize_client(&wave, Direction::Render, mode, None) {
                Ok(()) => {
                    let period = client.get_buffer_size().map_err(text)?;
                    return Ok(Opened {
                        client,
                        fmt,
                        period,
                        how: "exclusive",
                    });
                }
                // once more, on a new client, at the size it says
                Err(wasapi::WasapiError::Windows(e)) if e.code().0 == NOT_ALIGNED => {
                    let frames = client.get_buffer_size().map_err(text)?;
                    period = wasapi::calculate_period_100ns(frames as i64, fmt.rate as i64);
                    last = e.to_string();
                }
                Err(e) => {
                    last = e.to_string();
                    break;
                }
            }
        }
    }
    Err(last)
}

fn shared(dev: &wasapi::Device, cfg: &AudioConfig) -> Result<Opened, String> {
    let wave = dev
        .get_iaudioclient()
        .and_then(|c| c.get_mixformat())
        .map_err(text)?;
    let mix = Format::of(&wave);
    // IAudioClient3 plays the engine's own format only
    if let Some(m) = mix.filter(|m| cfg.sample_rate == 0 || cfg.sample_rate == m.rate) {
        match low_latency(dev, &wave, m, cfg.buffer_frames) {
            Ok(o) => return Ok(o),
            Err(e) => log::info!("audio: no IAudioClient3 period ({e}); the usual shared mode"),
        }
    }
    let fmt = Format {
        rate: rate(cfg, mix),
        channels: 2,
        kind: Kind::F32,
    };
    let mut client = dev.get_iaudioclient().map_err(text)?;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: wasapi::calculate_period_100ns(
            cfg.buffer_frames as i64,
            fmt.rate as i64,
        ),
    };
    client
        .initialize_client(&fmt.wave(), Direction::Render, mode, None)
        .map_err(text)?;
    let period = client.get_buffer_size().map_err(text)?;
    Ok(Opened {
        client,
        fmt,
        period,
        how: "shared",
    })
}

fn low_latency(
    dev: &wasapi::Device,
    wave: &WaveFormat,
    fmt: Format,
    want: u32,
) -> Result<Opened, String> {
    let mut client = dev.get_iaudioclient().map_err(text)?;
    let p = client.get_shared_mode_engine_period(wave).map_err(text)?;
    let period = shared_period(want, p.fundamental, p.min, p.max);
    client
        .initialize_shared_audio_stream(wave, TimingMode::Events, period)
        .map_err(text)?;
    Ok(Opened {
        client,
        fmt,
        period,
        how: "shared, low latency",
    })
}

/// An output stream on the default device in `mode` that renders `mix`;
/// `lost` is set when its device goes away.
pub(crate) fn open(
    mode: Mode,
    cfg: &AudioConfig,
    mix: &Arc<Mutex<Mix>>,
    lost: &Arc<AtomicBool>,
    epoch: Instant,
) -> Result<(Stream, Device), String> {
    let (tx, rx) = sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let (cfg, mix, lost, halt) = (cfg.clone(), mix.clone(), lost.clone(), stop.clone());
    let thread = std::thread::Builder::new()
        .name("wasapi".into())
        .spawn(move || run(mode, &cfg, &mix, &lost, &halt, epoch, tx))
        .map_err(|e| e.to_string())?;
    match rx.recv() {
        Ok(Ok(dev)) => Ok((
            Stream {
                stop,
                thread: Some(thread),
            },
            dev,
        )),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err("the audio thread ended".into()),
    }
}

/// The thread: open, say what opened (`tx`), render until `stop`.
fn run(
    mode: Mode,
    cfg: &AudioConfig,
    mix: &Mutex<Mix>,
    lost: &AtomicBool,
    stop: &AtomicBool,
    epoch: Instant,
    tx: SyncSender<Result<Device, String>>,
) {
    // the result says whether COM was set up already; either way it is
    let _ = wasapi::initialize_mta();
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
    };
    // SAFETY: the pseudo handle of this thread
    if let Err(e) = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) }
    {
        log::debug!("audio: thread priority: {e}");
    }
    let setup = || {
        let dev = wasapi::DeviceEnumerator::new()
            .and_then(|e| e.get_default_device(Direction::Render))
            .map_err(text)?;
        let o = match mode {
            Mode::Exclusive => exclusive(&dev, cfg)?,
            Mode::Shared => shared(&dev, cfg)?,
        };
        let name = dev.get_friendlyname().unwrap_or_else(|_| "output".into());
        let render = o.client.get_audiorenderclient().map_err(text)?;
        let clock = o.client.get_audioclock().map_err(text)?;
        let event = o.client.set_get_eventhandle().map_err(text)?;
        // a buffer of silence before it starts
        let n = o.client.get_available_space_in_frames().map_err(text)?;
        render
            .write_to_device(&vec![0; n as usize * o.fmt.frame_bytes()], None)
            .map_err(text)?;
        // a new stream: its clock is fitted anew
        mix.lock().unwrap().state.clock = FrameClock::new(o.fmt.rate);
        o.client.start_stream().map_err(text)?;
        let dev = Device {
            rate: o.fmt.rate,
            buffer: o.period,
            name: format!("{name} (WASAPI {})", o.how),
        };
        Ok::<_, String>((o, render, clock, event, dev))
    };
    let (o, render, clock, event, dev) = match setup() {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(Err(e));
            return;
        }
    };
    let _ = tx.send(Ok(dev));
    let mut buf = Vec::new();
    let mut step = || -> Result<(), String> {
        event.wait_for_event(SILENCE_MS).map_err(text)?;
        let n = o.client.get_available_space_in_frames().map_err(text)? as usize;
        if n == 0 || stop.load(Ordering::Acquire) {
            return Ok(());
        }
        buf.clear();
        buf.resize(n * o.fmt.frame_bytes(), 0);
        // the main thread holds it only to reopen or render silently
        if let Ok(mut m) = mix.try_lock() {
            // the position's performance counter, in 100 ns
            let (_, qpc) = clock.get_position().map_err(text)?;
            let obs = Obs {
                frame: m.state.frame,
                frames: n,
                stream_us: Some(qpc as f64 / 10.0),
                now_us: us_since(epoch, Instant::now()),
            };
            write(o.fmt, m.render(n, Some(obs)), &mut buf);
        }
        render.write_to_device(&buf, None).map_err(text)
    };
    while !stop.load(Ordering::Acquire) {
        if let Err(e) = step() {
            log::error!("audio stream error: {e}");
            lost.store(true, Ordering::Release);
            break;
        }
    }
    let _ = o.client.stop_stream();
}
