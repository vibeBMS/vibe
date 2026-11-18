//! Windows: the mixer on a WASAPI stream of its own, for what cpal does not
//! do. Two modes:
//!
//! - **Shared** (the default): through IAudioClient3 at a small period of
//!   the engine's (the buffer setting, rounded to one it takes; its
//!   smallest when the setting is 0) where Windows and the driver offer
//!   it, else the usual shared stream (at the buffer setting's length).
//! - **Exclusive**: the device alone, event driven at its smallest period,
//!   in a format it takes (`exclusive_formats`). Failing that, shared mode,
//!   and the player is told.
//!
//! What here decides is plain code, tested on every platform; the stream
//! itself (`stream`) is Windows only.

#[cfg(windows)]
mod stream;
#[cfg(windows)]
pub(crate) use stream::{open, Stream};

use crate::voice::soft_clip;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Shared,
    Exclusive,
}

/// A sample format a device may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    F32,
    /// 24 bits in three bytes.
    I24,
    /// 24 bits in the top three of four bytes.
    I24In32,
    I16,
    I32,
}

impl Kind {
    /// Bits per sample stored and used, and float or not.
    fn bits(self) -> (u16, u16, bool) {
        match self {
            Kind::F32 => (32, 32, true),
            Kind::I24 => (24, 24, false),
            Kind::I24In32 => (32, 24, false),
            Kind::I16 => (16, 16, false),
            Kind::I32 => (32, 32, false),
        }
    }

    fn bytes(self) -> usize {
        self.bits().0 as usize / 8
    }

    /// The kind of `store` bits per sample of which `valid` are used.
    fn of(store: u16, valid: u16, float: bool) -> Option<Kind> {
        [Kind::F32, Kind::I24, Kind::I24In32, Kind::I16, Kind::I32]
            .into_iter()
            .find(|k| k.bits() == (store, valid, float))
    }

    /// `x` (-1..1) as one sample of this kind, in `out`.
    fn put(self, x: f32, out: &mut [u8]) {
        let int = |max: f64| (x.clamp(-1.0, 1.0) as f64 * max).round() as i32;
        match self {
            Kind::F32 => out.copy_from_slice(&x.to_le_bytes()),
            Kind::I16 => out.copy_from_slice(&(int(32_767.0) as i16).to_le_bytes()),
            Kind::I24 => out.copy_from_slice(&int(8_388_607.0).to_le_bytes()[..3]),
            Kind::I24In32 => out.copy_from_slice(&(int(8_388_607.0) << 8).to_le_bytes()),
            Kind::I32 => out.copy_from_slice(&int(2_147_483_647.0).to_le_bytes()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Format {
    pub rate: u32,
    pub channels: u16,
    pub kind: Kind,
}

impl Format {
    fn frame_bytes(self) -> usize {
        self.kind.bytes() * self.channels as usize
    }
}

/// The kinds tried for a rate, best first.
const KINDS: [Kind; 4] = [Kind::F32, Kind::I24, Kind::I24In32, Kind::I16];

/// The formats to ask a device for in exclusive mode, best first: the
/// mixer's rate (`rate`) in stereo as 32-bit float, 24-bit (packed, then
/// in 32 bits), 16-bit; then the device's own formats (`own`: the one
/// Windows has set for it, the engine's), as they are and in those kinds.
pub(crate) fn exclusive_formats(rate: u32, own: &[Format]) -> Vec<Format> {
    let mut v: Vec<Format> = Vec::new();
    let stereo = KINDS.map(|kind| Format {
        rate,
        channels: 2,
        kind,
    });
    let theirs = own
        .iter()
        .flat_map(|o| std::iter::once(*o).chain(KINDS.map(|kind| Format { kind, ..*o })));
    for f in stereo.into_iter().chain(theirs) {
        if !v.contains(&f) {
            v.push(f);
        }
    }
    v
}

/// The exclusive-mode periods to try (100 ns units): the device's smallest
/// (`min`), then its default, then twice that.
pub(crate) fn exclusive_periods(min: i64, default: i64) -> Vec<i64> {
    let mut v: Vec<i64> = Vec::new();
    for p in [min, default, 2 * default] {
        if p > 0 && !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

/// IAudioClient3's period (frames) for a buffer of `want` frames (0: the
/// smallest): the nearest multiple of the engine's `fundamental` period
/// within its `min..=max`.
pub(crate) fn shared_period(want: u32, fundamental: u32, min: u32, max: u32) -> u32 {
    let f = fundamental.max(1);
    let want = if want == 0 { min } else { want };
    let nearest = (want.saturating_add(f / 2) / f).max(1) * f;
    nearest.clamp(min, max.max(min))
}

/// Open in exclusive mode when asked (`exclusive`), else, or when that
/// fails, in shared mode (`open`): what opened, and when exclusive mode was
/// asked for and not had, the message for the player.
pub(crate) fn exclusive_or_shared<T>(
    exclusive: bool,
    mut open: impl FnMut(Mode) -> Result<T, String>,
) -> (Result<T, String>, Option<String>) {
    let mut notice = None;
    if exclusive {
        match open(Mode::Exclusive) {
            Ok(t) => return (Ok(t), None),
            Err(e) => {
                log::warn!("audio: exclusive mode: {e}");
                notice = Some(format!("Exclusive audio output failed ({e}): shared mode"));
            }
        }
    }
    (open(Mode::Shared), notice)
}

/// The mixer's stereo frames `src`, soft clipped, as frames of `fmt` in
/// `dst` (as many as both hold): mono takes the mean of the two, channels
/// past two are silent.
pub(crate) fn write(fmt: Format, src: &[f32], dst: &mut [u8]) {
    let (size, mono) = (fmt.kind.bytes(), fmt.channels == 1);
    for (o, d) in src
        .as_chunks::<2>()
        .0
        .iter()
        .zip(dst.chunks_exact_mut(fmt.frame_bytes()))
    {
        let (l, r) = (soft_clip(o[0]), soft_clip(o[1]));
        for (c, s) in d.chunks_exact_mut(size).enumerate() {
            let x = match c {
                0 if mono => (l + r) * 0.5,
                0 => l,
                1 => r,
                _ => 0.0,
            };
            fmt.kind.put(x, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(rate: u32, channels: u16, kind: Kind) -> Format {
        Format {
            rate,
            channels,
            kind,
        }
    }

    #[test]
    fn exclusive_formats_try_the_mixers_rate_first_then_the_devices() {
        let v = exclusive_formats(44_100, &[]);
        assert_eq!(
            v,
            [Kind::F32, Kind::I24, Kind::I24In32, Kind::I16].map(|k| f(44_100, 2, k))
        );
        // the device set to 48 kHz 24-bit, the engine at 48 kHz float, 6
        // channels
        let own = [f(48_000, 2, Kind::I24), f(48_000, 6, Kind::F32)];
        let v = exclusive_formats(44_100, &own);
        assert_eq!(v[..4], exclusive_formats(44_100, &[])[..]);
        assert_eq!(v[4], own[0]);
        assert_eq!(v[5], f(48_000, 2, Kind::F32));
        assert!(v.contains(&f(48_000, 6, Kind::I16)));
        // nothing twice
        for (i, a) in v.iter().enumerate() {
            assert!(!v[i + 1..].contains(a), "{a:?}");
        }
        assert_eq!(v.len(), 4 + 4 + 4);
        // the device at the mixer's rate: nothing new
        assert_eq!(exclusive_formats(48_000, &[own[0]]).len(), 4);
    }

    #[test]
    fn kinds_read_back_from_their_bits() {
        for k in KINDS.into_iter().chain([Kind::I32]) {
            let (s, v, fl) = k.bits();
            assert_eq!(Kind::of(s, v, fl), Some(k));
        }
        assert_eq!(Kind::of(64, 64, true), None);
        assert_eq!(Kind::of(32, 20, false), None);
    }

    #[test]
    fn exclusive_periods_fall_back_to_the_default_then_twice() {
        assert_eq!(
            exclusive_periods(30_000, 100_000),
            [30_000, 100_000, 200_000]
        );
        assert_eq!(exclusive_periods(100_000, 100_000), [100_000, 200_000]);
        assert_eq!(exclusive_periods(0, 100_000), [100_000, 200_000]);
    }

    #[test]
    fn shared_periods_follow_the_buffer_setting_in_the_engines_steps() {
        // 48 kHz: steps of 48 frames from 144 to 480
        let p = |want| shared_period(want, 48, 144, 480);
        assert_eq!(p(0), 144);
        assert_eq!(p(64), 144);
        assert_eq!(p(256), 240);
        assert_eq!(p(200), 192);
        assert_eq!(p(2048), 480);
        // a driver with one period only
        assert_eq!(shared_period(64, 480, 480, 480), 480);
        assert_eq!(shared_period(64, 0, 0, 0), 0);
    }

    #[test]
    fn exclusive_falls_back_to_shared_and_says_so() {
        let mut tried = Vec::new();
        let (r, n) = exclusive_or_shared(true, |m| {
            tried.push(m);
            match m {
                Mode::Exclusive => Err("device in use".to_string()),
                Mode::Shared => Ok(m),
            }
        });
        assert_eq!(
            (r, tried),
            (Ok(Mode::Shared), vec![Mode::Exclusive, Mode::Shared])
        );
        assert!(n.unwrap().contains("device in use"));
        // had: no word
        assert_eq!(exclusive_or_shared(true, Ok), (Ok(Mode::Exclusive), None));
        // not asked: shared only
        let mut tried = Vec::new();
        let (r, n) = exclusive_or_shared(false, |m| {
            tried.push(m);
            Ok::<_, String>(m)
        });
        assert_eq!((r, n, tried), (Ok(Mode::Shared), None, vec![Mode::Shared]));
        // shared failing too: its error, and the word
        let (r, n) = exclusive_or_shared::<()>(true, |_| Err("no device".into()));
        assert_eq!(r, Err("no device".to_string()));
        assert!(n.is_some());
    }

    #[test]
    fn frames_are_written_in_the_devices_format() {
        let src = [0.5, -0.5, 1.0, -2.0];
        let mut b = vec![0u8; 16];
        write(f(48_000, 2, Kind::F32), &src, &mut b);
        assert_eq!(f32::from_le_bytes(b[..4].try_into().unwrap()), 0.5);
        assert_eq!(f32::from_le_bytes(b[4..8].try_into().unwrap()), -0.5);
        // soft clipped
        let r = f32::from_le_bytes(b[12..16].try_into().unwrap());
        assert!(r > -1.0 && r < -0.8, "{r}");

        let mut b = vec![0u8; 8];
        write(f(48_000, 2, Kind::I16), &src, &mut b);
        assert_eq!(i16::from_le_bytes([b[0], b[1]]), 16_384);
        assert_eq!(i16::from_le_bytes([b[2], b[3]]), -16_384);

        let mut b = vec![0u8; 6];
        write(f(48_000, 2, Kind::I24), &src[..2], &mut b);
        let s24 = |x: &[u8]| i32::from_le_bytes([0, x[0], x[1], x[2]]) >> 8;
        assert_eq!((s24(&b[..3]), s24(&b[3..])), (4_194_304, -4_194_304));

        let mut b = vec![0u8; 8];
        write(f(48_000, 2, Kind::I24In32), &src[..2], &mut b);
        assert_eq!(
            i32::from_le_bytes(b[..4].try_into().unwrap()),
            4_194_304 << 8
        );

        // mono: the mean; four channels: two silent
        let mut b = vec![0u8; 2];
        write(f(48_000, 1, Kind::I16), &[0.5, 0.25], &mut b);
        assert_eq!(i16::from_le_bytes([b[0], b[1]]), 12_288);
        let mut b = vec![0xffu8; 16];
        write(f(48_000, 4, Kind::F32), &[0.5, 0.25], &mut b);
        assert_eq!(b[8..], [0; 8]);
    }
}
