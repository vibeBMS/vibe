//! Audio for vibeBMS.
//!
//! * [`decode`]: decode WAV/OGG/MP3/FLAC into 16-bit PCM at the mixer rate,
//!   with BMS-style file name fallback (`a.wav` -> `a.ogg`).
//! * [`Mixer`]: a cpal output stream with a lock-free-ish command queue and
//!   sample-accurate scheduling (BGM is queued ahead with exact start frames).

pub mod decode;
pub mod mixer;

pub use decode::{decode_bytes, decode_file, resolve_sound_path, Sample, SoundBank};
pub use mixer::{AudioConfig, Bus, Mixer, PlayOptions};
