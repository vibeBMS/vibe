//! The app's own sound: the sound sets' system sounds, and loudness
//! normalisation of charts, previews and the sound sets' music.

pub mod loudness;
pub mod sysound;

/// The mixer voice of the song select's chart preview (above the sound
/// sets' voices).
pub const PREVIEW_KEY: u64 = u64::MAX - 1;
