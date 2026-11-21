use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, Track, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// The longest sound decoded on its own ([`decode_bytes`]: a song's
/// preview, a guide sound), in seconds at the mixer's rate: one that plays
/// on is cut there. However long a file says it is, and whatever sample
/// rate it states, no more comes out of it. A play's sounds have no such
/// cap: their budget bounds them ([`max_play_bytes`]).
pub const MAX_SOUND_SECS: u64 = 30 * 60;

/// Where memory is short: 32-bit targets (the web), phones.
const SMALL: bool = cfg!(any(
    target_pointer_width = "32",
    target_os = "android",
    target_os = "ios"
));

/// The decoded PCM (bytes) the sounds of one play take together at most:
/// what is past it is cut or left out, the files after it in the chart's
/// order first. A play's sounds decoded are about its files as 16-bit WAV,
/// tens to hundreds of megabytes for most charts, 600-900 MB for the
/// heaviest keysound charts at 48 kHz; this is some three hours of stereo
/// at 48 kHz. On a phone a quarter of the device's memory, between 512 MiB
/// and that (known on 64-bit Android: `/proc/meminfo`; iOS and 32-bit
/// targets, the web among them, keep 512 MiB).
pub fn max_play_bytes() -> usize {
    static BYTES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BYTES.get_or_init(|| play_bytes(SMALL, device_memory()))
}

/// [`max_play_bytes`] on a device with memory short or not, of `memory`
/// bytes if it is known.
fn play_bytes(small: bool, memory: Option<u64>) -> usize {
    const MOST: u64 = 2 << 30;
    const LEAST: u64 = 512 << 20;
    match (small, memory) {
        (false, _) => MOST as usize,
        (true, Some(m)) => (m / 4).clamp(LEAST, MOST) as usize,
        (true, None) => LEAST as usize,
    }
}

/// The device's memory (bytes), where it is known and a play's sounds may
/// take more than the least for it: 64-bit Android.
fn device_memory() -> Option<u64> {
    #[cfg(all(target_os = "android", target_pointer_width = "64"))]
    if let Ok(t) = std::fs::read_to_string("/proc/meminfo") {
        return mem_total(&t);
    }
    None
}

/// `MemTotal` of a `/proc/meminfo` (bytes).
#[cfg(any(test, all(target_os = "android", target_pointer_width = "64")))]
fn mem_total(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kb.checked_mul(1024)
}

/// The output frames reserved for a sound before it is decoded, per byte
/// of its file: what the header says of its length is believed only as far
/// as a file of that size could bear it out (PCM is a byte or more a
/// sample; speech-rate MP3 and Vorbis are some ten frames a byte), and the
/// rest grows as it is decoded.
const RESERVE_PER_BYTE: usize = 16;

/// Bytes of PCM the sounds decoded together may still take, and how long
/// each may be at most (seconds).
pub struct Budget {
    left: AtomicUsize,
    longest: u64,
}

impl Budget {
    pub fn new(bytes: usize, longest_secs: u64) -> Budget {
        Budget {
            left: AtomicUsize::new(bytes),
            longest: longest_secs,
        }
    }

    /// Up to `want` bytes of it: how many there were.
    fn take(&self, want: usize) -> usize {
        let mut left = self.left.load(Ordering::Relaxed);
        loop {
            let got = want.min(left);
            let set = self.left.compare_exchange_weak(
                left,
                left - got,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            match set {
                Ok(_) => return got,
                Err(now) => left = now,
            }
        }
    }
}

/// Decoded PCM at the mixer's sample rate.
#[derive(Debug, Default)]
pub struct Sample {
    /// 1 or 2 (interleaved).
    pub channels: u16,
    pub frames: usize,
    pub data: Vec<i16>,
}

impl Sample {
    /// Add frames `from..` times `gain` to `out`, interleaved stereo (as
    /// many frames as it holds).
    #[inline]
    pub fn mix_into(&self, from: usize, out: &mut [f32], gain: f32) {
        let g = gain / 32768.0;
        if self.channels == 1 {
            let data = &self.data[from..from + out.len() / 2];
            for (o, &s) in out.as_chunks_mut::<2>().0.iter_mut().zip(data) {
                let v = s as f32 * g;
                o[0] += v;
                o[1] += v;
            }
        } else {
            let data = &self.data[from * 2..from * 2 + out.len()];
            for (o, &s) in out.iter_mut().zip(data) {
                *o += s as f32 * g;
            }
        }
    }

    /// The PCM's size in bytes.
    fn bytes(&self) -> usize {
        std::mem::size_of_val(self.data.as_slice())
    }

    /// Cut to its first frames, `bytes` of them at most.
    fn cut(&mut self, bytes: usize) {
        let ch = self.channels.max(1) as usize;
        self.frames = self.frames.min(bytes / (ch * 2));
        self.data.truncate(self.frames * ch);
        self.data.shrink_to_fit();
    }

    /// The sample played `speed` times as fast, its pitch with it (as a
    /// turntable): resampled, linearly; of `room` bytes at most (cut there).
    pub fn resampled(&self, speed: f64, room: usize) -> Sample {
        let ch = self.channels.max(1) as usize;
        let frames = ((self.frames as f64 / speed).round() as usize).min(room / (ch * 2));
        let mut data = Vec::with_capacity(frames * ch);
        for i in 0..frames {
            let x = i as f64 * speed;
            let (a, f) = (x as usize, (x - x.floor()) as f32);
            let b = (a + 1).min(self.frames.saturating_sub(1));
            for c in 0..ch {
                let at = |k: usize| self.data.get(k * ch + c).copied().unwrap_or(0) as f32;
                data.push((at(a) * (1.0 - f) + at(b) * f).round() as i16);
            }
        }
        Sample {
            channels: self.channels,
            frames,
            data,
        }
    }
}

/// The lower-case extension of `path`: the format hint.
fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Decode any supported file into interleaved PCM at `rate`.
pub fn decode_file(path: &Path, rate: u32) -> Result<Sample, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    decode_bytes(bytes, &ext_of(path), rate)
}

/// A file's bytes, shared by symphonia and the WAV fallback.
#[derive(Clone)]
struct Bytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Decode in-memory file data (`ext` is a format hint such as "ogg"): its
/// first [`MAX_SOUND_SECS`].
pub fn decode_bytes(
    bytes: impl Into<Arc<Vec<u8>>>,
    ext: &str,
    rate: u32,
) -> Result<Sample, String> {
    let alone = Budget::new(max_play_bytes(), MAX_SOUND_SECS);
    decode_bytes_within(bytes, ext, rate, &alone)
}

/// [`decode_bytes`] of a sound that takes its bytes from `budget` (it is
/// cut where that runs out).
pub fn decode_bytes_within(
    bytes: impl Into<Arc<Vec<u8>>>,
    ext: &str,
    rate: u32,
    budget: &Budget,
) -> Result<Sample, String> {
    decode_within(bytes.into(), ext, rate, budget).map(|(s, _)| s)
}

/// A sound decoded, and whether `budget` ran out for it (it is cut).
type Decoded = (Sample, bool);

fn decode_within(
    bytes: Arc<Vec<u8>>,
    ext: &str,
    rate: u32,
    budget: &Budget,
) -> Result<Decoded, String> {
    let bytes = Bytes(bytes);
    decode_symphonia(bytes.clone(), ext, rate, budget).or_else(|e| {
        // tolerate broken RIFF headers that symphonia rejects
        decode_wav_fallback(&bytes.0, rate, budget).ok_or(e)
    })
}

/// The sample rate an audio file is recorded at (`ext`: a format hint),
/// without decoding it.
pub fn native_rate(bytes: Vec<u8>, ext: &str) -> Option<u32> {
    let riff = (bytes.len() >= 28 && &bytes[0..4] == b"RIFF" && &bytes[12..16] == b"fmt ")
        .then(|| u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]));
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(Bytes(Arc::new(bytes)))),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension(ext);
    let probed = symphonia::default::get_probe().probe(
        &hint,
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    );
    let rate = probed.ok().and_then(|f| {
        audio_track(&*f)?
            .codec_params
            .as_ref()?
            .audio()?
            .sample_rate
    });
    // (a RIFF header symphonia rejects: its fmt chunk's)
    rate.or(riff).filter(|&r| r > 0)
}

/// The first audio track with a known codec.
fn audio_track(format: &dyn FormatReader) -> Option<&Track> {
    format.first_track_known_codec(TrackType::Audio)
}

/// Decoder options: gapless, so a keysound starts on its first sample: an
/// MP3's encoder delay (about 25 ms with LAME) and a stream's padding are
/// trimmed.
fn decoder_options() -> AudioDecoderOptions {
    AudioDecoderOptions::default().gapless(true)
}

fn decode_symphonia(
    bytes: Bytes,
    ext: &str,
    rate: u32,
    budget: &Budget,
) -> Result<Decoded, String> {
    let len = bytes.0.len();
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(ext);
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;
    let track = audio_track(&*format).ok_or("no audio track")?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("no audio track")?
        .clone();
    // the playable frames: gapless, the decoder trims the rest
    let n_frames = track.num_frames;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &decoder_options())
        .map_err(|e| e.to_string())?;
    let mut conv: Option<Convert> = None;
    let mut buf: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            // the end, or a stream cut short (as before)
            Ok(None) | Err(SymError::IoError(_)) | Err(SymError::ResetRequired) => break,
            Err(e) => {
                if conv.is_none() {
                    return Err(e.to_string());
                }
                break;
            }
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = decoded.spec();
                let (channels, sr) = (spec.channels().count() as u16, spec.rate());
                let c = conv
                    .get_or_insert_with(|| Convert::new(channels, sr, rate, n_frames, len, budget));
                decoded.copy_to_vec_interleaved(&mut buf);
                if !c.push(&buf) {
                    break;
                }
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }
    let conv = conv.unwrap_or_else(|| {
        let channels = params.channels.as_ref().map_or(2, |c| c.count() as u16);
        let sr = params.sample_rate.unwrap_or(44100);
        Convert::new(channels, sr, rate, None, len, budget)
    });
    Ok(conv.finish())
}

/// Minimal PCM RIFF reader that ignores bogus chunk sizes.
fn decode_wav_fallback(b: &[u8], rate: u32, budget: &Budget) -> Option<Decoded> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let mut p = 12usize;
    let (mut ch, mut sr, mut bits, mut fmt) = (0u16, 0u32, 0u16, 0u16);
    // (a chunk's size is the file's word and `usize` may be 32 bits: the
    // sums are checked)
    loop {
        let body = p.checked_add(8).filter(|&body| body <= b.len())?;
        let id = &b[p..p + 4];
        let size = u32::from_le_bytes(b[p + 4..body].try_into().ok()?) as usize;
        if id == b"fmt " && b.len() - body >= 16 {
            fmt = u16::from_le_bytes([b[body], b[body + 1]]);
            ch = u16::from_le_bytes([b[body + 2], b[body + 3]]);
            sr = u32::from_le_bytes(b[body + 4..body + 8].try_into().ok()?);
            bits = u16::from_le_bytes([b[body + 14], b[body + 15]]);
        } else if id == b"data" && ch > 0 {
            let end = body.saturating_add(size).min(b.len());
            let d = &b[body..end];
            let (width, read): (usize, fn(&[u8]) -> f32) = match (fmt, bits) {
                (1, 8) => (1, |c| (c[0] as f32 - 128.0) / 128.0),
                (1, 16) => (2, |c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0),
                (1, 24) => (3, |c| {
                    (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0
                }),
                (1, 32) => (4, |c| {
                    i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0
                }),
                (3, 32) => (4, |c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])),
                _ => return None,
            };
            let frames = d.len() / width / ch as usize;
            let mut conv = Convert::new(ch, sr, rate, Some(frames as u64), b.len(), budget);
            let mut pcm = Vec::new();
            for part in d.chunks(width * ch as usize * 4096) {
                pcm.clear();
                pcm.extend(part.chunks_exact(width).map(read));
                if !conv.push(&pcm) {
                    break;
                }
            }
            return Some(conv.finish());
        }
        p = body.checked_add(size)?.checked_add(size & 1)?;
    }
}

/// Downmixes to <= 2 channels, resamples (linear) and quantises to i16, a
/// decoded packet at a time: no whole file in `f32`. What it makes is as
/// long as the frames that really come, up to the budget's longest sound
/// and what it has left.
struct Convert<'a> {
    ch: usize,
    out_ch: usize,
    /// Input frames per output frame (`None`: the rate is kept).
    ratio: Option<f64>,
    /// Input frames from `base` on, downmixed: the ones the next output
    /// frames are interpolated from.
    pending: Vec<f32>,
    base: usize,
    /// Input frames so far.
    seen: usize,
    /// The next output frame.
    next: usize,
    data: Vec<i16>,
    /// The output frames there is room for so far (taken from `budget`),
    /// of `cap` at most.
    room: usize,
    cap: usize,
    budget: &'a Budget,
    /// The budget gave less than was asked.
    starved: bool,
}

impl<'a> Convert<'a> {
    /// `frames`: how many the file says it has; `file_len`: its size.
    fn new(
        channels: u16,
        sr: u32,
        rate: u32,
        frames: Option<u64>,
        file_len: usize,
        budget: &'a Budget,
    ) -> Convert<'a> {
        let ch = channels.max(1) as usize;
        let out_ch = ch.min(2);
        let ratio = (sr != rate && sr != 0).then(|| sr as f64 / rate as f64);
        let cap =
            usize::try_from((rate as u64).saturating_mul(budget.longest)).unwrap_or(usize::MAX);
        let stated = frames.map_or(0, |f| (f as f64 / ratio.unwrap_or(1.0)) as usize);
        let reserve = stated
            .min(file_len.saturating_mul(RESERVE_PER_BYTE))
            .min(cap);
        let mut data = Vec::new();
        // (refused: it grows as the frames come)
        let _ = data.try_reserve_exact(reserve.saturating_mul(out_ch));
        Convert {
            ch,
            out_ch,
            ratio,
            pending: Vec::new(),
            base: 0,
            seen: 0,
            next: 0,
            data,
            room: 0,
            cap,
            budget,
            starved: false,
        }
    }

    /// Room for the output frames up to the `to`th: how far there is.
    fn room(&mut self, to: usize) -> usize {
        let to = to.min(self.cap);
        if to > self.room {
            let frame = self.out_ch * size_of::<i16>();
            let want = (to - self.room).saturating_mul(frame);
            let got = self.budget.take(want);
            self.starved |= got < want;
            self.room += got / frame;
        }
        self.room.min(to)
    }

    /// The next frames, interleaved; whether there is room for more.
    fn push(&mut self, pcm: &[f32]) -> bool {
        let (ch, out_ch) = (self.ch, self.out_ch);
        let q = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
        let frames = pcm.chunks_exact(ch);
        self.seen += frames.len();
        let Some(ratio) = self.ratio else {
            let have = self.data.len() / out_ch;
            let want = have + frames.len();
            let room = self.room(want);
            for f in frames.take(room - have) {
                self.data.extend((0..out_ch).map(|c| q(f[c.min(ch - 1)])));
            }
            return room == want;
        };
        for f in frames {
            self.pending.extend((0..out_ch).map(|c| f[c.min(ch - 1)]));
        }
        self.resample(ratio, false)
    }

    /// The output frames the input so far makes (at the end, the last ones
    /// from the last input frame); whether there was room for them all.
    fn resample(&mut self, ratio: f64, end: bool) -> bool {
        let q = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
        let out_ch = self.out_ch;
        // as many as the whole input would make
        let whole = ((self.seen as f64) / ratio).floor() as usize;
        let limit = self.room(whole);
        while self.next < limit {
            let pos = self.next as f64 * ratio;
            let i = pos as usize;
            if !end && i + 1 >= self.seen {
                break;
            }
            let t = (pos - i as f64) as f32;
            let j = (i + 1).min(self.seen.saturating_sub(1));
            for c in 0..out_ch {
                let a = self.pending[(i - self.base) * out_ch + c];
                let b = self.pending[(j - self.base) * out_ch + c];
                self.data.push(q(a + (b - a) * t));
            }
            self.next += 1;
        }
        // the input frames still needed start at the next output's
        let keep = ((self.next as f64 * ratio) as usize).clamp(self.base, self.seen);
        self.pending.drain(..(keep - self.base) * out_ch);
        self.base = keep;
        limit == whole
    }

    fn finish(mut self) -> Decoded {
        if let Some(ratio) = self.ratio {
            self.resample(ratio, true);
        }
        self.data.shrink_to_fit();
        let s = Sample {
            channels: self.out_ch as u16,
            frames: self.data.len() / self.out_ch,
            data: self.data,
        };
        (s, self.starved)
    }
}

/// Call `f(i)` for `i in 0..n`, on up to `max_threads` threads natively
/// (each taking the next `i` when it is free); in turn on the web.
pub fn parallel_for<F: Fn(usize) + Sync>(n: usize, max_threads: usize, f: F) {
    let threads = if cfg!(target_arch = "wasm32") {
        1
    } else {
        std::thread::available_parallelism()
            .map_or(4, |t| t.get())
            .clamp(1, max_threads.max(1))
            .min(n)
    };
    if threads <= 1 {
        return (0..n).for_each(f);
    }
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                f(i);
            });
        }
    });
}

/// Sounds decoded at once from one budget of `most` bytes (which got what
/// depends on how the threads ran) made what the budget gives taken in
/// their order: the same at every load. Each fits whole up to the first
/// that does not, which is cut where the budget ends (decoded `again` with
/// what is left when it came out shorter), and the ones after it are left
/// out. That one and the ones left out (that were decoded), if any.
fn fit_in_order(
    decoded: &mut [Option<Decoded>],
    most: usize,
    again: impl Fn(usize, &Budget) -> Option<Decoded>,
) -> Option<(usize, Vec<usize>)> {
    let mut left = most;
    let m = decoded.iter().position(|d| match d {
        Some((s, starved)) if !starved && s.bytes() <= left => {
            left -= s.bytes();
            false
        }
        Some(_) => true,
        None => false,
    })?;
    let out = (m + 1..decoded.len())
        .filter(|&k| decoded[k].take().is_some())
        .collect();
    let cut = match decoded[m].take() {
        Some((mut s, starved)) if !starved || s.bytes() >= left => {
            s.cut(left);
            s
        }
        _ => again(m, &Budget::new(left, u64::MAX)).map_or_else(Sample::default, |d| d.0),
    };
    decoded[m] = Some((cut, true));
    Some((m, out))
}

/// All sound files of a chart, decoded in parallel: each file once, shared
/// by the sounds that name it, and all of them within [`max_play_bytes`].
#[derive(Default)]
pub struct SoundBank {
    pub samples: Vec<Option<Arc<Sample>>>,
}

/// The bytes the samples of `sounds` take (one that several are, once).
fn held<'s>(sounds: impl Iterator<Item = &'s Option<Arc<Sample>>>) -> usize {
    let each: HashMap<_, _> = sounds
        .flatten()
        .map(|s| (Arc::as_ptr(s), s.bytes()))
        .collect();
    each.values().sum()
}

impl SoundBank {
    /// Load with custom path resolution and file access (virtual file systems).
    pub fn load_with(
        files: &[String],
        rate: u32,
        progress: &AtomicUsize,
        resolve: &dyn Fn(&str) -> Option<PathBuf>,
        read: &(dyn Fn(&Path) -> Option<Arc<Vec<u8>>> + Sync),
    ) -> SoundBank {
        let paths: Vec<_> = files.iter().map(|f| resolve(f)).collect();
        SoundBank::load_paths(&paths, rate, progress, read)
    }

    /// The sounds whose files are at `paths` (`None`: a sound without one).
    pub fn load_paths(
        paths: &[Option<PathBuf>],
        rate: u32,
        progress: &AtomicUsize,
        read: &(dyn Fn(&Path) -> Option<Arc<Vec<u8>>> + Sync),
    ) -> SoundBank {
        // each file once: its path and the sounds that are it
        let mut files: Vec<(&Path, Vec<usize>)> = Vec::new();
        let mut file_of: HashMap<&Path, usize> = HashMap::new();
        for (i, p) in paths.iter().enumerate() {
            let Some(p) = p.as_deref() else {
                progress.fetch_add(1, Ordering::Relaxed);
                continue;
            };
            let k = *file_of.entry(p).or_insert_with(|| {
                files.push((p, Vec::new()));
                files.len() - 1
            });
            files[k].1.push(i);
        }
        let most = max_play_bytes();
        let load_one = |p: &Path, budget: &Budget| -> Option<Decoded> {
            match decode_within(read(p)?, &ext_of(p), rate, budget) {
                Ok(d) => Some(d),
                Err(e) => {
                    log::debug!("decode {}: {e}", p.display());
                    None
                }
            }
        };
        // (several at once, the budget taken as they come)
        let budget = Budget::new(most, u64::MAX);
        let decoded: Vec<Mutex<Option<Decoded>>> = files.iter().map(|_| Mutex::default()).collect();
        parallel_for(files.len(), 16, |k| {
            let (p, sounds) = &files[k];
            *decoded[k].lock().unwrap() = load_one(p, &budget);
            progress.fetch_add(sounds.len(), Ordering::Relaxed);
        });
        let mut decoded: Vec<Option<Decoded>> = decoded
            .into_iter()
            .map(|d| d.into_inner().unwrap())
            .collect();
        let again = |k: usize, b: &Budget| load_one(files[k].0, b);
        if let Some((m, out)) = fit_in_order(&mut decoded, most, again) {
            let names: Vec<String> = (out.iter().take(20))
                .map(|&k| files[k].0.display().to_string())
                .collect();
            let frames = decoded[m].as_ref().map_or(0, |d| d.0.frames);
            log::warn!(
                "sounds: more than {} MiB decoded: {} cut at {:.1} s, {} files after it left out: {}{}",
                most >> 20,
                files[m].0.display(),
                frames as f64 / rate.max(1) as f64,
                out.len(),
                names.join(", "),
                if out.len() > names.len() { ", ..." } else { "" },
            );
        }
        let mut samples = vec![None; paths.len()];
        for ((_, sounds), d) in files.iter().zip(decoded) {
            let s = d.map(|(s, _)| Arc::new(s));
            for &i in sounds {
                samples[i] = s.clone();
            }
        }
        SoundBank { samples }
    }

    /// The sounds `sounds` played `speed` times as fast (and as high): a
    /// sample several of them are, once, and the bank still within
    /// [`max_play_bytes`] (what does not fit is cut).
    pub fn resample(&mut self, sounds: Range<usize>, speed: f64) {
        let (before, rest) = self.samples.split_at_mut(sounds.start);
        let (these, after) = rest.split_at_mut(sounds.len());
        let mut room = max_play_bytes().saturating_sub(held(before.iter().chain(after.iter())));
        let mut done: HashMap<*const Sample, Arc<Sample>> = HashMap::new();
        for s in these.iter_mut().flatten() {
            let r = done.entry(Arc::as_ptr(s)).or_insert_with(|| {
                let r = s.resampled(speed, room);
                room -= r.bytes();
                Arc::new(r)
            });
            *s = r.clone();
        }
    }

    pub fn get(&self, file: u32) -> Option<&Arc<Sample>> {
        self.samples.get(file as usize).and_then(|s| s.as_ref())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resampled_plays_faster_and_higher() {
        let s = super::Sample {
            channels: 1,
            frames: 4,
            data: vec![0, 100, 200, 300],
        };
        let fast = s.resampled(2.0, usize::MAX);
        assert_eq!((fast.frames, fast.data.as_slice()), (2, &[0i16, 200][..]));
        let slow = s.resampled(0.5, usize::MAX);
        assert_eq!(slow.data, [0, 50, 100, 150, 200, 250, 300, 300]);
        // no longer than there is room for, however slow
        assert_eq!(s.resampled(0.5, 6).data, [0, 50, 100]);
        assert_eq!(s.resampled(0.0, 4).data, [0, 0]);
    }

    use super::*;

    fn ample() -> Budget {
        Budget::new(usize::MAX, MAX_SOUND_SECS)
    }

    /// A 16-bit PCM WAV of `ch` channels.
    fn wav(sr: u32, frames: usize, ch: u16) -> Vec<u8> {
        let mut b = Vec::new();
        let data_len = (frames * 2 * ch as usize) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&ch.to_le_bytes());
        b.extend_from_slice(&sr.to_le_bytes());
        b.extend_from_slice(&(sr * 2 * ch as u32).to_le_bytes());
        b.extend_from_slice(&(2 * ch).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames * ch as usize {
            b.extend_from_slice(&(((i as f32 * 0.05).sin() * 10000.0) as i16).to_le_bytes());
        }
        b
    }

    #[test]
    fn native_rates() {
        assert_eq!(native_rate(wav(22_050, 100, 1), "wav"), Some(22_050));
        assert_eq!(native_rate(wav(48_000, 100, 2), ""), Some(48_000));
        // a broken RIFF size: the fmt chunk's rate
        let mut b = wav(32_000, 100, 1);
        b[4..8].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(native_rate(b, "wav"), Some(32_000));
        assert_eq!(native_rate(b"not audio".to_vec(), "ogg"), None);
    }

    /// How a file was converted before, all at once: what the conversion
    /// packet by packet must equal.
    fn convert_whole(channels: u16, sr: u32, pcm: &[f32], rate: u32) -> Vec<i16> {
        let ch = channels.max(1) as usize;
        let in_frames = pcm.len() / ch;
        let out_ch = ch.min(2);
        let get = |f: usize, c: usize| pcm[f * ch + c.min(ch - 1)];
        let q = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
        if sr == rate || sr == 0 {
            return (0..in_frames)
                .flat_map(|f| (0..out_ch).map(move |c| q(get(f, c))))
                .collect();
        }
        let ratio = sr as f64 / rate as f64;
        let out_frames = ((in_frames as f64) / ratio).floor() as usize;
        let mut data = Vec::new();
        for o in 0..out_frames {
            let pos = o as f64 * ratio;
            let i = pos as usize;
            let t = (pos - i as f64) as f32;
            let j = (i + 1).min(in_frames.saturating_sub(1));
            for c in 0..out_ch {
                let (a, b) = (get(i, c), get(j, c));
                data.push(q(a + (b - a) * t));
            }
        }
        data
    }

    /// A whole file in `f32`, as symphonia decodes it.
    fn decode_whole(bytes: Vec<u8>, ext: &str) -> (u16, u32, Vec<f32>) {
        let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
        let mut hint = Hint::new();
        hint.with_extension(ext);
        let mut format = symphonia::default::get_probe()
            .probe(&hint, mss, Default::default(), Default::default())
            .unwrap();
        let track = format.default_track(TrackType::Audio).unwrap();
        let id = track.id;
        let params = track.codec_params.as_ref().unwrap().audio().unwrap();
        let mut dec = symphonia::default::get_codecs()
            .make_audio_decoder(params, &decoder_options())
            .unwrap();
        let (mut ch, mut sr, mut out) = (0, 0, Vec::new());
        while let Ok(Some(p)) = format.next_packet() {
            if p.track_id != id {
                continue;
            }
            let Ok(d) = dec.decode(&p) else { continue };
            let spec = d.spec();
            (ch, sr) = (spec.channels().count() as u16, spec.rate());
            let mut b = Vec::<f32>::new();
            d.copy_to_vec_interleaved(&mut b);
            out.extend_from_slice(&b);
        }
        (ch, sr, out)
    }

    #[test]
    fn packets_convert_as_the_whole_file_did() {
        let rates = [
            (44_100, 44_100),
            (0, 44_100),
            (22_050, 44_100),
            (48_000, 44_100),
        ];
        let rates = rates
            .into_iter()
            .chain([(192_000, 44_100), (8_000, 48_000), (44_100, 48_000)]);
        for (sr, rate) in rates {
            for ch in [1u16, 2, 6] {
                let frames = 1500 + ch as usize;
                let pcm: Vec<f32> = (0..frames * ch as usize)
                    .map(|i| ((i * 7919 % 2003) as f32 / 1001.0 - 1.0) * 1.1)
                    .collect();
                let budget = ample();
                let mut c = Convert::new(ch, sr, rate, None, 0, &budget);
                let mut rest = &pcm[..];
                for n in [1usize, 7, 64, 333, 2, 1024].iter().cycle() {
                    let k = (n * ch as usize).min(rest.len());
                    c.push(&rest[..k]);
                    rest = &rest[k..];
                    if rest.is_empty() {
                        break;
                    }
                }
                let got = c.finish().0;
                let want = convert_whole(ch, sr, &pcm, rate);
                assert_eq!(got.channels, ch.min(2), "{sr} -> {rate}, {ch} ch");
                assert_eq!(got.frames * got.channels as usize, want.len());
                assert!(got.data == want, "{sr} -> {rate}, {ch} ch");
            }
        }
    }

    #[test]
    fn files_decode_as_the_whole_file_did() {
        let ogg = include_bytes!("../tests/data/tone.ogg").to_vec();
        let files = [
            (wav(22_050, 3000, 2), "wav"),
            (wav(44_100, 5000, 1), "wav"),
            (ogg, "ogg"),
        ];
        for (bytes, ext) in files {
            let (ch, sr, pcm) = decode_whole(bytes.clone(), ext);
            assert!(!pcm.is_empty());
            for rate in [sr, 44_100, 48_000] {
                let s = decode_bytes(bytes.clone(), ext, rate).unwrap();
                assert!(
                    s.data == convert_whole(ch, sr, &pcm, rate),
                    "{ext} {sr} -> {rate}"
                );
                let s =
                    (ext == "wav").then(|| decode_wav_fallback(&bytes, rate, &ample()).unwrap().0);
                assert!(s.is_none_or(|s| s.data == convert_whole(ch, sr, &pcm, rate)));
            }
        }
    }

    #[test]
    fn gapless_vorbis_is_its_playable_frames() {
        // no encoder delay or padding: as many frames as the stream plays
        let ogg = include_bytes!("../tests/data/tone.ogg").to_vec();
        let mss = MediaSourceStream::new(
            Box::new(std::io::Cursor::new(ogg.clone())),
            Default::default(),
        );
        let mut hint = Hint::new();
        hint.with_extension("ogg");
        let format = symphonia::default::get_probe()
            .probe(&hint, mss, Default::default(), Default::default())
            .unwrap();
        let track = format.default_track(TrackType::Audio).unwrap();
        let rate = track
            .codec_params
            .as_ref()
            .unwrap()
            .audio()
            .unwrap()
            .sample_rate
            .unwrap();
        let playable = track.num_frames.unwrap();
        let s = decode_bytes(ogg, "ogg", rate).unwrap();
        assert_eq!((s.channels, s.frames as u64), (2, playable));
    }

    #[test]
    fn a_header_is_believed_as_far_as_its_file_bears_it_out() {
        // 100 frames, and a data chunk that says 4 GB
        let mut b = wav(44_100, 100, 1);
        b[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        let budget = ample();
        let c = Convert::new(1, 44_100, 48_000, Some(1 << 31), b.len(), &budget);
        assert!(c.data.capacity() <= b.len() * RESERVE_PER_BYTE);
        let s = decode_bytes(b, "wav", 44_100).unwrap();
        assert!(s.frames <= 100 && s.data.capacity() <= 100);
    }

    #[test]
    fn a_sound_is_cut_at_its_longest() {
        // (a mixer at 2 Hz: an hour of it is few frames)
        let cap = 2 * MAX_SOUND_SECS as usize;
        let b = wav(2, cap + 500, 1);
        assert_eq!(decode_bytes(b.clone(), "wav", 2).unwrap().frames, cap);
        assert_eq!(decode_wav_fallback(&b, 2, &ample()).unwrap().0.frames, cap);
        // a file that states a low rate is stretched no further
        let s = decode_wav_fallback(&wav(1, 5000, 2), 4, &ample())
            .unwrap()
            .0;
        assert_eq!(s.frames, 4 * MAX_SOUND_SECS as usize);
    }

    #[test]
    fn sounds_decoded_together_share_a_budget() {
        let b = Arc::new(wav(44_100, 1000, 2));
        let budget = Budget::new(1200 * 4, MAX_SOUND_SECS);
        let frames = |rate| {
            decode_within(b.clone(), "wav", rate, &budget)
                .unwrap()
                .0
                .frames
        };
        assert_eq!(
            [frames(44_100), frames(44_100), frames(44_100)],
            [1000, 200, 0]
        );
        // resampled too
        let budget = Budget::new(300 * 4, MAX_SOUND_SECS);
        let s = decode_within(b.clone(), "wav", 48_000, &budget).unwrap().0;
        assert_eq!((s.frames, s.data.len()), (300, 600));
    }

    /// What a play's budget leaves out is what it would leave out taken in
    /// the chart's order, however the threads ran.
    #[test]
    fn the_budget_is_taken_in_the_charts_order() {
        let sample = |frames: usize| Sample {
            channels: 1,
            frames,
            data: (0..frames as i16).collect(),
        };
        let lens = |d: &[Option<Decoded>]| -> Vec<Option<usize>> {
            d.iter().map(|d| d.as_ref().map(|d| d.0.frames)).collect()
        };
        let again = |k: usize, b: &Budget| {
            assert_eq!(k, 1);
            Some((sample(b.left.load(Ordering::Relaxed) / 2), true))
        };
        // the second had less than its turn: decoded again with what is left
        let mut d = vec![
            Some((sample(50), false)),
            Some((sample(5), true)),
            None,
            Some((sample(20), false)),
        ];
        assert_eq!(fit_in_order(&mut d, 160, again), Some((1, vec![3])));
        assert_eq!(lens(&d), [Some(50), Some(30), None, None]);
        // the second whole, but past its turn: cut
        let mut d = vec![
            Some((sample(50), false)),
            Some((sample(70), false)),
            Some((sample(0), true)),
        ];
        assert_eq!(fit_in_order(&mut d, 160, again), Some((1, vec![2])));
        assert_eq!(lens(&d), [Some(50), Some(30), None]);
        // all whole within it: as they are
        let mut d = vec![Some((sample(50), false)), None, Some((sample(30), false))];
        assert_eq!(fit_in_order(&mut d, 160, again), None);
        assert_eq!(lens(&d), [Some(50), None, Some(30)]);
    }

    /// A play's sound is as long as its budget holds; one decoded alone is
    /// cut at [`MAX_SOUND_SECS`].
    #[test]
    fn a_play_sound_is_bounded_by_the_budget_only() {
        // (a mixer at 2 Hz: an hour of it is few frames)
        let long = 2 * MAX_SOUND_SECS as usize + 500;
        let file = Arc::new(wav(2, long, 1));
        let bank = SoundBank::load_paths(
            &[Some(PathBuf::from("a.wav"))],
            2,
            &AtomicUsize::new(0),
            &|_| Some(file.clone()),
        );
        assert_eq!(bank.get(0).unwrap().frames, long);
    }

    #[test]
    fn a_phone_takes_a_quarter_of_its_memory_for_a_play() {
        let meminfo = "MemTotal:        7812500 kB\nMemFree:          100000 kB\n";
        assert_eq!(mem_total(meminfo), Some(8_000_000_000));
        assert_eq!(mem_total("MemFree: 1 kB\n"), None);
        assert_eq!(play_bytes(false, None), 2 << 30);
        assert_eq!(play_bytes(true, None), 512 << 20);
        assert_eq!(play_bytes(true, Some(4 << 30)), 1 << 30);
        assert_eq!(play_bytes(true, Some(1 << 30)), 512 << 20);
        assert_eq!(play_bytes(true, Some(16 << 30)), 2 << 30);
    }

    #[test]
    fn a_file_several_sounds_name_is_decoded_once() {
        let file = Arc::new(wav(44_100, 400, 1));
        let (reads, progress) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let names = ["a.wav", "b.wav", "a.wav", "", "a.wav"].map(String::from);
        let mut bank = SoundBank::load_with(
            &names,
            44_100,
            &progress,
            &|name| (!name.is_empty()).then(|| PathBuf::from(name)),
            &|_| {
                reads.fetch_add(1, Ordering::Relaxed);
                Some(file.clone())
            },
        );
        assert_eq!(reads.load(Ordering::Relaxed), 2);
        assert_eq!(progress.load(Ordering::Relaxed), 5);
        let one = |bank: &SoundBank, a, b| Arc::ptr_eq(bank.get(a).unwrap(), bank.get(b).unwrap());
        assert!(one(&bank, 0, 2) && one(&bank, 0, 4) && !one(&bank, 0, 1));
        assert!(bank.get(3).is_none());
        // at another speed: still one sample, and the sounds left as they
        // were keep theirs
        bank.resample(2..5, 0.5);
        assert!(one(&bank, 2, 4) && !one(&bank, 0, 2));
        let frames = |i| bank.get(i).unwrap().frames;
        assert_eq!((frames(0), frames(2)), (400, 800));
    }

    #[test]
    fn chunk_sizes_that_do_not_add_up_end_the_fallback() {
        // a chunk of nearly 4 GB (where `usize` is 32 bits, its end is
        // where it began), and a data chunk of as much
        let mut b = b"RIFF\0\0\0\0WAVEjunk".to_vec();
        b.extend_from_slice(&0xffff_fff8u32.to_le_bytes());
        assert!(decode_wav_fallback(&b, 44_100, &ample()).is_none());
        let mut b = wav(44_100, 10, 1);
        b[40..44].copy_from_slice(&0xffff_fffcu32.to_le_bytes());
        let s = decode_wav_fallback(&b, 44_100, &ample()).unwrap().0;
        assert_eq!(s.frames, 10);
    }

    #[test]
    fn decode_resample_and_load() {
        let dir = std::env::temp_dir().join(format!("vibebms-audio-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kick.wav"), wav(22050, 22050, 1)).unwrap();
        let s = decode_file(&dir.join("kick.wav"), 44100).unwrap();
        assert_eq!(s.channels, 1);
        assert!((s.frames as i64 - 44100).abs() <= 2);
        let progress = AtomicUsize::new(0);
        let bank = SoundBank::load_with(
            &["kick.wav".into(), "missing.wav".into()],
            48000,
            &progress,
            &|name| Some(dir.join(name)),
            &|p| std::fs::read(p).ok().map(Arc::new),
        );
        assert!(bank.get(0).is_some());
        assert!(bank.get(1).is_none());
        assert_eq!(progress.load(Ordering::Relaxed), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
