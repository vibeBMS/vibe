use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, Track, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

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

    /// The sample played `speed` times as fast, its pitch with it (as a
    /// turntable): resampled, linearly.
    pub fn resampled(&self, speed: f64) -> Sample {
        let ch = self.channels.max(1) as usize;
        let frames = (self.frames as f64 / speed).round() as usize;
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

/// Decode in-memory file data (`ext` is a format hint such as "ogg").
pub fn decode_bytes(
    bytes: impl Into<Arc<Vec<u8>>>,
    ext: &str,
    rate: u32,
) -> Result<Sample, String> {
    let bytes = Bytes(bytes.into());
    decode_symphonia(bytes.clone(), ext, rate).or_else(|e| {
        // tolerate broken RIFF headers that symphonia rejects
        decode_wav_fallback(&bytes.0, rate).ok_or(e)
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

fn decode_symphonia(bytes: Bytes, ext: &str, rate: u32) -> Result<Sample, String> {
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
                let c = conv.get_or_insert_with(|| Convert::new(channels, sr, rate, n_frames));
                decoded.copy_to_vec_interleaved(&mut buf);
                c.push(&buf);
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }
    let conv = conv.unwrap_or_else(|| {
        let channels = params.channels.as_ref().map_or(2, |c| c.count() as u16);
        Convert::new(channels, params.sample_rate.unwrap_or(44100), rate, None)
    });
    Ok(conv.finish())
}

/// Minimal PCM RIFF reader that ignores bogus chunk sizes.
fn decode_wav_fallback(b: &[u8], rate: u32) -> Option<Sample> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let mut p = 12;
    let (mut ch, mut sr, mut bits, mut fmt) = (0u16, 0u32, 0u16, 0u16);
    while p + 8 <= b.len() {
        let id = &b[p..p + 4];
        let size = u32::from_le_bytes(b[p + 4..p + 8].try_into().ok()?) as usize;
        let body = p + 8;
        if id == b"fmt " && body + 16 <= b.len() {
            fmt = u16::from_le_bytes([b[body], b[body + 1]]);
            ch = u16::from_le_bytes([b[body + 2], b[body + 3]]);
            sr = u32::from_le_bytes(b[body + 4..body + 8].try_into().ok()?);
            bits = u16::from_le_bytes([b[body + 14], b[body + 15]]);
        } else if id == b"data" && ch > 0 {
            let end = (body + size).min(b.len());
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
            let mut conv = Convert::new(ch, sr, rate, Some(frames as u64));
            let mut pcm = Vec::new();
            for part in d.chunks(width * ch as usize * 4096) {
                pcm.clear();
                pcm.extend(part.chunks_exact(width).map(read));
                conv.push(&pcm);
            }
            return Some(conv.finish());
        }
        p = body + size + (size & 1);
    }
    None
}

/// Downmixes to <= 2 channels, resamples (linear) and quantises to i16, a
/// decoded packet at a time: no whole file in `f32`.
struct Convert {
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
}

impl Convert {
    fn new(channels: u16, sr: u32, rate: u32, frames: Option<u64>) -> Convert {
        let ch = channels.max(1) as usize;
        let out_ch = ch.min(2);
        let ratio = (sr != rate && sr != 0).then(|| sr as f64 / rate as f64);
        let out_frames = frames.map_or(0, |f| (f as f64 / ratio.unwrap_or(1.0)) as usize);
        Convert {
            ch,
            out_ch,
            ratio,
            pending: Vec::new(),
            base: 0,
            seen: 0,
            next: 0,
            data: Vec::with_capacity(out_frames * out_ch),
        }
    }

    /// The next frames, interleaved.
    fn push(&mut self, pcm: &[f32]) {
        let (ch, out_ch) = (self.ch, self.out_ch);
        let q = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
        let frames = pcm.chunks_exact(ch);
        self.seen += frames.len();
        let Some(ratio) = self.ratio else {
            for f in frames {
                self.data.extend((0..out_ch).map(|c| q(f[c.min(ch - 1)])));
            }
            return;
        };
        for f in frames {
            self.pending.extend((0..out_ch).map(|c| f[c.min(ch - 1)]));
        }
        self.resample(ratio, false);
    }

    /// The output frames the input so far makes (at the end, the last ones
    /// from the last input frame).
    fn resample(&mut self, ratio: f64, end: bool) {
        let q = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
        let out_ch = self.out_ch;
        // as many as the whole input would make
        let limit = ((self.seen as f64) / ratio).floor() as usize;
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
    }

    fn finish(mut self) -> Sample {
        if let Some(ratio) = self.ratio {
            self.resample(ratio, true);
        }
        self.data.shrink_to_fit();
        Sample {
            channels: self.out_ch as u16,
            frames: self.data.len() / self.out_ch,
            data: self.data,
        }
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

/// All sound files of a chart, decoded in parallel.
#[derive(Default)]
pub struct SoundBank {
    pub samples: Vec<Option<Arc<Sample>>>,
}

impl SoundBank {
    /// Load with custom path resolution and file access (virtual file systems).
    pub fn load_with(
        files: &[String],
        rate: u32,
        progress: &AtomicUsize,
        resolve: &(dyn Fn(&str) -> Option<PathBuf> + Sync),
        read: &(dyn Fn(&Path) -> Option<Arc<Vec<u8>>> + Sync),
    ) -> SoundBank {
        let n = files.len();
        let mut slots: Vec<Option<Arc<Sample>>> = vec![None; n];
        let load_one = |i: usize| -> Option<Arc<Sample>> {
            let p = resolve(&files[i])?;
            let bytes = read(&p)?;
            match decode_bytes(bytes, &ext_of(&p), rate) {
                Ok(s) => Some(Arc::new(s)),
                Err(e) => {
                    log::debug!("decode {}: {e}", p.display());
                    None
                }
            }
        };
        let results = std::sync::Mutex::new(&mut slots);
        parallel_for(n, 16, |i| {
            let r = load_one(i);
            results.lock().unwrap()[i] = r;
            progress.fetch_add(1, Ordering::Relaxed);
        });
        SoundBank { samples: slots }
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
        let fast = s.resampled(2.0);
        assert_eq!((fast.frames, fast.data.as_slice()), (2, &[0i16, 200][..]));
        let slow = s.resampled(0.5);
        assert_eq!(slow.data, [0, 50, 100, 150, 200, 250, 300, 300]);
    }

    use super::*;

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
                let mut c = Convert::new(ch, sr, rate, None);
                let mut rest = &pcm[..];
                for n in [1usize, 7, 64, 333, 2, 1024].iter().cycle() {
                    let k = (n * ch as usize).min(rest.len());
                    c.push(&rest[..k]);
                    rest = &rest[k..];
                    if rest.is_empty() {
                        break;
                    }
                }
                let got = c.finish();
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
                let s = (ext == "wav").then(|| decode_wav_fallback(&bytes, rate).unwrap());
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
