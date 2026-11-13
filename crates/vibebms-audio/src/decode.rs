use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decoded PCM at the mixer's sample rate.
#[derive(Debug, Default)]
pub struct Sample {
    pub channels: u16,
    pub frames: usize,
    pub data: Vec<i16>,
}

impl Sample {
    #[inline]
    pub fn frame(&self, i: usize) -> (f32, f32) {
        const S: f32 = 1.0 / 32768.0;
        if self.channels == 1 {
            let v = self.data[i] as f32 * S;
            (v, v)
        } else {
            let b = i * self.channels as usize;
            (self.data[b] as f32 * S, self.data[b + 1] as f32 * S)
        }
    }
}

const EXTS: [&str; 5] = ["wav", "ogg", "mp3", "flac", "opus"];

/// Resolve a BMS sound reference. BMS files frequently reference `x.wav`
/// while shipping `x.ogg`, and file systems may be case sensitive.
pub fn resolve_sound_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    let p = dir.join(&name);
    if p.is_file() {
        return Some(p);
    }
    let rel = Path::new(&name);
    let parent = dir.join(rel.parent().unwrap_or(Path::new("")));
    let stem = rel.file_stem()?.to_string_lossy().to_lowercase();
    let entries = std::fs::read_dir(&parent).ok()?;
    let mut best: Option<(usize, PathBuf)> = None;
    for e in entries.flatten() {
        let path = e.path();
        let Some(fs) = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()) else {
            continue;
        };
        if fs != stem {
            continue;
        }
        let ext = path
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if let Some(rank) = EXTS.iter().position(|x| *x == ext) {
            if best.as_ref().map(|b| rank < b.0).unwrap_or(true) {
                best = Some((rank, path));
            }
        }
    }
    best.map(|b| b.1)
}

/// Decode any supported file into interleaved PCM at `rate`.
pub fn decode_file(path: &Path, rate: u32) -> Result<Sample, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    decode_bytes(bytes, &ext, rate)
}

/// Decode in-memory file data (`ext` is a format hint such as "ogg").
pub fn decode_bytes(bytes: Vec<u8>, ext: &str, rate: u32) -> Result<Sample, String> {
    match decode_symphonia(bytes.clone(), ext) {
        Ok((ch, sr, pcm)) => Ok(convert(ch, sr, &pcm, rate)),
        Err(e) => {
            // tolerate broken RIFF headers that symphonia rejects
            if let Some((ch, sr, pcm)) = decode_wav_fallback(&bytes) {
                return Ok(convert(ch, sr, &pcm, rate));
            }
            Err(e)
        }
    }
}

fn decode_symphonia(bytes: Vec<u8>, ext: &str) -> Result<(u16, u32, Vec<f32>), String> {
    let file = std::io::Cursor::new(bytes);
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(ext);
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio track")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(2);
    let mut sr = track.codec_params.sample_rate.unwrap_or(44100);
    let mut buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(_)) | Err(SymError::ResetRequired) => break,
            Err(e) => {
                if out.is_empty() {
                    return Err(e.to_string());
                }
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                channels = spec.channels.count() as u16;
                sr = spec.rate;
                let need = decoded.capacity() as u64;
                if buf
                    .as_ref()
                    .map(|b| b.capacity() < need as usize)
                    .unwrap_or(true)
                {
                    buf = Some(SampleBuffer::new(need, spec));
                }
                let b = buf.as_mut().unwrap();
                b.copy_interleaved_ref(decoded);
                out.extend_from_slice(b.samples());
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }
    Ok((channels.max(1), sr, out))
}

/// Minimal PCM RIFF reader that ignores bogus chunk sizes.
fn decode_wav_fallback(b: &[u8]) -> Option<(u16, u32, Vec<f32>)> {
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
            let pcm: Vec<f32> = match (fmt, bits) {
                (1, 8) => d.iter().map(|x| (*x as f32 - 128.0) / 128.0).collect(),
                (1, 16) => d
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                    .collect(),
                (1, 24) => d
                    .chunks_exact(3)
                    .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
                    .collect(),
                (1, 32) => d
                    .chunks_exact(4)
                    .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0)
                    .collect(),
                (3, 32) => d
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect(),
                _ => return None,
            };
            return Some((ch, sr, pcm));
        }
        p = body + size + (size & 1);
    }
    None
}

/// Downmix to <= 2 channels, resample (linear) and quantise to i16.
fn convert(channels: u16, sr: u32, pcm: &[f32], rate: u32) -> Sample {
    let ch = channels.max(1) as usize;
    let in_frames = pcm.len() / ch;
    let out_ch = if ch == 1 { 1 } else { 2 };
    let get = |f: usize, c: usize| -> f32 {
        if ch == 1 {
            pcm[f]
        } else {
            pcm[f * ch + c.min(ch - 1)]
        }
    };
    let q = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
    if sr == rate || sr == 0 {
        let mut data = Vec::with_capacity(in_frames * out_ch);
        for f in 0..in_frames {
            for c in 0..out_ch {
                data.push(q(get(f, c)));
            }
        }
        return Sample {
            channels: out_ch as u16,
            frames: in_frames,
            data,
        };
    }
    let ratio = sr as f64 / rate as f64;
    let out_frames = ((in_frames as f64) / ratio).floor() as usize;
    let mut data = Vec::with_capacity(out_frames * out_ch);
    for o in 0..out_frames {
        let pos = o as f64 * ratio;
        let i = pos as usize;
        let t = (pos - i as f64) as f32;
        let j = (i + 1).min(in_frames.saturating_sub(1));
        for c in 0..out_ch {
            let a = get(i, c);
            let b = get(j, c);
            data.push(q(a + (b - a) * t));
        }
    }
    Sample {
        channels: out_ch as u16,
        frames: out_frames,
        data,
    }
}

/// All sound files of a chart, decoded in parallel.
#[derive(Default)]
pub struct SoundBank {
    pub samples: Vec<Option<Arc<Sample>>>,
}

impl SoundBank {
    /// `progress` is incremented once per file.
    pub fn load(dir: &Path, files: &[String], rate: u32, progress: &AtomicUsize) -> SoundBank {
        let resolve = |name: &str| resolve_sound_path(dir, name);
        let read = |p: &Path| std::fs::read(p).ok();
        Self::load_with(files, rate, progress, &resolve, &read)
    }

    /// Load with custom path resolution and file access (virtual file systems).
    pub fn load_with(
        files: &[String],
        rate: u32,
        progress: &AtomicUsize,
        resolve: &(dyn Fn(&str) -> Option<PathBuf> + Sync),
        read: &(dyn Fn(&Path) -> Option<Vec<u8>> + Sync),
    ) -> SoundBank {
        let n = files.len();
        let mut slots: Vec<Option<Arc<Sample>>> = vec![None; n];
        let load_one = |i: usize| -> Option<Arc<Sample>> {
            let p = resolve(&files[i])?;
            let bytes = read(&p)?;
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            match decode_bytes(bytes, &ext, rate) {
                Ok(s) => Some(Arc::new(s)),
                Err(e) => {
                    log::debug!("decode {}: {e}", p.display());
                    None
                }
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            for (i, slot) in slots.iter_mut().enumerate() {
                *slot = load_one(i);
                progress.fetch_add(1, Ordering::Relaxed);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(1, 16);
            let next = AtomicUsize::new(0);
            let results = std::sync::Mutex::new(&mut slots);
            std::thread::scope(|s| {
                for _ in 0..threads {
                    s.spawn(|| loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            break;
                        }
                        let r = load_one(i);
                        results.lock().unwrap()[i] = r;
                        progress.fetch_add(1, Ordering::Relaxed);
                    });
                }
            });
        }
        SoundBank { samples: slots }
    }

    pub fn get(&self, file: u32) -> Option<&Arc<Sample>> {
        self.samples.get(file as usize).and_then(|s| s.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, sr: u32, frames: usize) {
        let mut b = Vec::new();
        let data_len = (frames * 2) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&sr.to_le_bytes());
        b.extend_from_slice(&(sr * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            b.extend_from_slice(&(((i as f32 * 0.05).sin() * 10000.0) as i16).to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn decode_and_resample_and_fallback_names() {
        let dir = std::env::temp_dir().join(format!("vibebms-audio-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_wav(&dir.join("Kick.WAV"), 22050, 22050);
        let p = resolve_sound_path(&dir, "kick.ogg").expect("fallback by stem");
        let s = decode_file(&p, 44100).unwrap();
        assert_eq!(s.channels, 1);
        assert!((s.frames as i64 - 44100).abs() <= 2);
        let progress = AtomicUsize::new(0);
        let bank = SoundBank::load(
            &dir,
            &["kick.wav".into(), "missing.wav".into()],
            48000,
            &progress,
        );
        assert!(bank.get(0).is_some());
        assert!(bank.get(1).is_none());
        assert_eq!(progress.load(Ordering::Relaxed), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
