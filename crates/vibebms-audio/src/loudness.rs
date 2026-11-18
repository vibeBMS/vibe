//! Loudness as ITU-R BS.1770-4 measures it (LUFS, integrated, gated), and
//! the sample peak: of a stereo signal fed in pieces ([`Meter`]), or of
//! sounds placed in time and mixed as they are measured ([`measure_mix`],
//! a chart's sounds autoplayed), without holding the whole mix.
//!
//! The signal is K-weighted (the standard's high shelf and high pass, made
//! for the meter's rate), its mean square taken over 400 ms blocks every
//! 100 ms; blocks under -70 LUFS are left out, then those 10 LU under the
//! mean of the rest.

use crate::decode::Sample;

/// A signal's integrated loudness (`None`: silent, every block under the
/// absolute gate) and its sample peak (1.0 = full scale).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Loudness {
    pub lufs: Option<f32>,
    pub peak: f32,
}

/// The gain that brings `l` to `target` LUFS: at most `max_boost_db` up
/// and no further than its peak reaching `ceiling`, at most 30 dB down; 1
/// for silence.
pub fn normal_gain(l: &Loudness, target: f32, max_boost_db: f32, ceiling: f32) -> f32 {
    let Some(lufs) = l.lufs else { return 1.0 };
    let db = (target - lufs).clamp(-30.0, max_boost_db.max(0.0));
    let g = 10f32.powf(db / 20.0);
    if g > 1.0 && l.peak > 0.0 {
        g.min((ceiling / l.peak).max(1.0))
    } else {
        g
    }
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        // transposed direct form II
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// BS.1770's K-weighting at `rate`: the head's high shelf (+4 dB above
/// about 1.7 kHz), then the RLB high pass (about 38 Hz).
fn k_weighting(rate: f64) -> [Biquad; 2] {
    let (f0, gain_db, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [
            (vh + vb * k / q + k * k) / a0,
            2.0 * (k * k - vh) / a0,
            (vh - vb * k / q + k * k) / a0,
        ],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let high_pass = Biquad {
        b: [1.0, -2.0, 1.0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    [shelf, high_pass]
}

/// Measures a stereo signal fed to it in pieces.
pub struct Meter {
    /// Per channel.
    filters: [[Biquad; 2]; 2],
    hop: usize,
    /// Frames into the hop, and its sum of squares (both channels).
    in_hop: usize,
    sum: f64,
    /// The last four hops' sums (a block is four hops).
    hops: [f64; 4],
    hops_seen: usize,
    /// Each block's mean square (both channels summed).
    blocks: Vec<f32>,
    peak: f32,
}

impl Meter {
    pub fn new(rate: u32) -> Meter {
        let w = k_weighting(rate.max(1) as f64);
        Meter {
            filters: [w, w],
            hop: (rate as usize / 10).max(1),
            in_hop: 0,
            sum: 0.0,
            hops: [0.0; 4],
            hops_seen: 0,
            blocks: Vec::new(),
            peak: 0.0,
        }
    }

    /// Interleaved stereo frames.
    pub fn push(&mut self, stereo: &[f32]) {
        for f in stereo.as_chunks::<2>().0 {
            self.frame(f[0], f[1]);
        }
    }

    /// A sample's frames (mono played on both sides, as the mixer plays it).
    pub fn push_sample(&mut self, s: &Sample) {
        let k = 1.0 / 32768.0;
        match s.channels {
            1 => s.data.iter().for_each(|&v| {
                let v = v as f32 * k;
                self.frame(v, v)
            }),
            _ => s.data.as_chunks::<2>().0.iter().for_each(|f| {
                self.frame(f[0] as f32 * k, f[1] as f32 * k);
            }),
        }
    }

    #[inline]
    fn frame(&mut self, l: f32, r: f32) {
        self.peak = self.peak.max(l.abs()).max(r.abs());
        let [fl, fr] = &mut self.filters;
        let l = fl[0].run(l as f64);
        let l = fl[1].run(l);
        let r = fr[0].run(r as f64);
        let r = fr[1].run(r);
        self.sum += l * l + r * r;
        self.in_hop += 1;
        if self.in_hop == self.hop {
            self.end_hop();
        }
    }

    fn end_hop(&mut self) {
        self.hops[self.hops_seen % 4] = self.sum;
        self.hops_seen += 1;
        self.sum = 0.0;
        self.in_hop = 0;
        if self.hops_seen >= 4 {
            let ms = self.hops.iter().sum::<f64>() / (self.hop * 4) as f64;
            self.blocks.push(ms as f32);
        }
    }

    /// The loudness of what was fed (a signal shorter than one block is
    /// measured as one block, its missing part silent).
    pub fn finish(mut self) -> Loudness {
        if self.blocks.is_empty() {
            while self.hops_seen < 4 {
                self.end_hop();
            }
        }
        let lufs = |ms: f64| -0.691 + 10.0 * ms.log10();
        let mean = |v: &mut dyn Iterator<Item = f64>| {
            let (s, n) = v.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
            (n > 0).then(|| s / n as f64)
        };
        let above = |gate: f64| {
            self.blocks
                .iter()
                .map(|b| *b as f64)
                .filter(move |b| lufs(*b) > gate)
        };
        let relative = mean(&mut above(-70.0)).map(|m| lufs(m) - 10.0);
        let lufs = relative
            .and_then(|g| mean(&mut above(g.max(-70.0))))
            .map(|m| lufs(m) as f32);
        Loudness {
            lufs,
            peak: self.peak,
        }
    }
}

/// A sound placed in a mix: frames `from..to` of `sample` from frame `at`
/// (all at the sample's rate).
#[derive(Clone, Copy)]
pub struct Placed<'a> {
    pub sample: &'a Sample,
    pub at: usize,
    pub from: usize,
    pub to: usize,
}

/// The loudness of `sounds` (at `rate`) mixed, measured as the mix is made
/// a tenth of a second at a time. `step` > 1 takes every `step`-th frame
/// only (a meter at `rate / step`, for slow machines: frequencies above its
/// Nyquist fold down, a little off for bright music).
pub fn measure_mix(sounds: &mut Vec<Placed>, rate: u32, step: usize) -> Loudness {
    let step = step.max(1);
    let mrate = (rate as usize / step).max(1);
    let mut meter = Meter::new(mrate as u32);
    sounds.retain(|p| p.to > p.from && p.from < p.sample.frames);
    sounds.sort_by_key(|p| p.at);
    // (in frames of the meter: where each starts, how many it has)
    let start = |p: &Placed| p.at.div_ceil(step);
    let len = |p: &Placed| (p.to.min(p.sample.frames) - p.from).div_ceil(step);
    let end = sounds.iter().map(|p| start(p) + len(p)).max().unwrap_or(0);
    let hop = (mrate / 10).max(1);
    let mut buf = vec![0f32; hop * 2];
    let mut next = 0;
    let mut active: Vec<Placed> = Vec::new();
    let k = 1.0 / 32768.0;
    let mut pos = 0;
    while pos < end {
        let n = hop.min(end - pos);
        let out = &mut buf[..n * 2];
        out.fill(0.0);
        while next < sounds.len() && start(&sounds[next]) < pos + n {
            active.push(sounds[next]);
            next += 1;
        }
        active.retain(|p| {
            let (s, l) = (start(p), len(p));
            let a = s.max(pos);
            let b = (s + l).min(pos + n);
            let ch = p.sample.channels.max(1) as usize;
            let data = &p.sample.data;
            let src = |j: usize| p.from + (j - s) * step;
            for j in a..b {
                let f = src(j);
                let o = &mut out[(j - pos) * 2..(j - pos) * 2 + 2];
                if ch == 1 {
                    let v = data[f] as f32 * k;
                    o[0] += v;
                    o[1] += v;
                } else {
                    o[0] += data[f * 2] as f32 * k;
                    o[1] += data[f * 2 + 1] as f32 * k;
                }
            }
            s + l > pos + n
        });
        meter.push(out);
        pos += n;
    }
    meter.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `secs` of a sine of `hz` at `amp` (full scale 1) on both channels.
    fn sine(rate: u32, hz: f64, amp: f64, secs: f64) -> Vec<f32> {
        let n = (rate as f64 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (amp * (std::f64::consts::TAU * hz * i as f64 / rate as f64).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    fn measure(rate: u32, sig: &[f32]) -> Loudness {
        let mut m = Meter::new(rate);
        m.push(sig);
        m.finish()
    }

    #[test]
    fn k_weighting_is_the_standards_at_48k() {
        let [s, h] = k_weighting(48_000.0);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-8;
        assert!(near(s.b[0], 1.53512485958697) && near(s.b[1], -2.69169618940638));
        assert!(near(s.b[2], 1.19839281085285));
        assert!(near(s.a[0], -1.69065929318241) && near(s.a[1], 0.73248077421585));
        assert!(near(h.a[0], -1.99004745483398) && near(h.a[1], 0.99007225036621));
    }

    #[test]
    fn a_1k_sine_reads_its_level() {
        // a 997 Hz sine at full scale on one channel is -3.01 LUFS; on both,
        // its level in dBFS
        for rate in [44_100, 48_000] {
            for (amp, want) in [(1.0, 0.0), (0.1, -20.0), (0.01, -40.0)] {
                let l = measure(rate, &sine(rate, 997.0, amp, 5.0));
                let got = l.lufs.unwrap();
                assert!((got - want).abs() < 0.1, "{rate} Hz, {amp}: {got}");
                assert!((l.peak - amp as f32).abs() < 1e-3);
            }
            let one: Vec<f32> = sine(rate, 997.0, 1.0, 5.0)
                .chunks(2)
                .flat_map(|f| [f[0], 0.0])
                .collect();
            let got = measure(rate, &one).lufs.unwrap();
            assert!((got + 3.01).abs() < 0.1, "{got}");
        }
    }

    #[test]
    fn the_weighting_favours_the_highs_and_cuts_the_lows() {
        let rate = 48_000;
        let at = |hz| measure(rate, &sine(rate, hz, 0.1, 5.0)).lufs.unwrap();
        // the +4 dB shelf at high frequencies (where 1 kHz has +0.7), the
        // high pass under 40 Hz
        assert!((at(8000.0) - -16.7).abs() < 0.2, "{}", at(8000.0));
        assert!(at(20.0) < -32.0, "{}", at(20.0));
    }

    #[test]
    fn gates_leave_out_silence_and_quiet_parts() {
        let rate = 48_000;
        let mut sig = sine(rate, 997.0, 0.1, 10.0);
        sig.extend(vec![0.0; rate as usize * 20]);
        // silence (absolute gate): still -20
        let got = measure(rate, &sig).lufs.unwrap();
        assert!((got + 20.0).abs() < 0.2, "{got}");
        // a part 30 dB down (relative gate): still -20
        sig.extend(sine(rate, 997.0, 0.00316, 10.0));
        let got = measure(rate, &sig).lufs.unwrap();
        assert!((got + 20.0).abs() < 0.2, "{got}");
        // a part 6 dB down is within the gate: between
        sig.extend(sine(rate, 997.0, 0.05, 10.0));
        let got = measure(rate, &sig).lufs.unwrap();
        assert!(got < -20.5 && got > -26.0, "{got}");
        // silence only
        assert_eq!(measure(rate, &vec![0.0; 96_000]).lufs, None);
        assert_eq!(Meter::new(rate).finish().lufs, None);
    }

    fn tone(rate: u32, hz: f64, amp: f64, secs: f64) -> Sample {
        let data: Vec<i16> = sine(rate, hz, amp, secs)
            .chunks(2)
            .map(|f| (f[0] * 32767.0).round() as i16)
            .collect();
        Sample {
            channels: 1,
            frames: data.len(),
            data,
        }
    }

    #[test]
    fn mixes_placed_sounds() {
        let rate = 48_000;
        let t = tone(rate, 997.0, 0.1, 1.0);
        let r = rate as usize;
        // ten seconds of a tone, a second at a time
        let mut v: Vec<Placed> = (0..10)
            .map(|i| Placed {
                sample: &t,
                at: i * r,
                from: 0,
                to: r,
            })
            .collect();
        let l = measure_mix(&mut v, rate, 1);
        assert!((l.lufs.unwrap() + 20.0).abs() < 0.15, "{l:?}");
        // every second frame: about the same
        let l2 = measure_mix(&mut v, rate, 2);
        assert!((l2.lufs.unwrap() + 20.0).abs() < 0.2, "{l2:?}");
        // two at once (in phase): 6 dB up, the peak doubled
        let mut two: Vec<Placed> = v.iter().flat_map(|p| [*p, *p]).collect();
        let l = measure_mix(&mut two, rate, 1);
        assert!((l.lufs.unwrap() + 13.98).abs() < 0.15, "{l:?}");
        assert!((l.peak - 0.2).abs() < 1e-3);
        // a slice of a sound, and sounds past its end left out
        let mut part = vec![
            Placed {
                sample: &t,
                at: 0,
                from: r / 2,
                to: r * 5,
            },
            Placed {
                sample: &t,
                at: 0,
                from: r * 2,
                to: r * 3,
            },
        ];
        let l = measure_mix(&mut part, rate, 1);
        assert!((l.lufs.unwrap() + 20.0).abs() < 0.3, "{l:?}");
        assert_eq!(measure_mix(&mut Vec::new(), rate, 1).lufs, None);
    }

    #[test]
    fn gains() {
        let l = |lufs, peak| Loudness {
            lufs: Some(lufs),
            peak,
        };
        let db = |g: f32| 20.0 * g.log10();
        // down to the target
        assert!((db(normal_gain(&l(-8.0, 1.0), -12.0, 12.0, 0.8)) + 4.0).abs() < 1e-3);
        // up, but not past the ceiling
        assert!((db(normal_gain(&l(-20.0, 0.1), -12.0, 12.0, 0.8)) - 8.0).abs() < 1e-3);
        assert!((normal_gain(&l(-20.0, 0.4), -12.0, 12.0, 0.8) - 2.0).abs() < 1e-3);
        assert_eq!(normal_gain(&l(-20.0, 0.9), -12.0, 12.0, 0.8), 1.0);
        // at most the boost asked
        assert!((db(normal_gain(&l(-40.0, 0.001), -12.0, 12.0, 0.8)) - 12.0).abs() < 1e-3);
        assert_eq!(normal_gain(&Loudness::default(), -12.0, 12.0, 0.8), 1.0);
    }
}
