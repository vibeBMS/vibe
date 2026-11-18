//! The mixer: voices started on frames, cut by a newer voice of the same
//! key, bus and master volumes (the master gliding to a new value). No I/O and no clock but its frames: an
//! output (`out`) hands it commands and asks it for buffers.

use crate::clock::{FrameClock, Obs};
use crate::decode::Sample;
use crate::{AudioConfig, PlayOptions};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    Key = 0,
    Bgm = 1,
    System = 2,
}

/// Volume groups a voice may be in (`PlayOptions::group`), besides none.
pub const GROUPS: u8 = 8;

/// When a scheduled voice starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Due {
    /// µs on the judge's clock since the mixer's epoch, turned into a frame
    /// by the fitted clock (`clock`) when it comes.
    Us(f64),
    /// This frame (the web's output turns moments into frames itself).
    #[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
    Frame(u64),
}

pub(crate) struct Voice {
    pub sample: Arc<Sample>,
    pub pos: usize,
    pub start: usize,
    pub end: usize,
    pub start_at: u64,
    /// Not started yet: `start_at` is set when this comes.
    pub due: Option<Due>,
    pub cut_at: u64,
    pub vol: f32,
    pub key: u64,
    pub bus: Bus,
    pub looping: bool,
    pub group: u8,
}

impl Voice {
    /// `o.at` is the caller's: what it means as `due` depends on the output.
    pub fn new(sample: Arc<Sample>, o: &PlayOptions, due: Option<Due>) -> Voice {
        let end = o.end.unwrap_or(sample.frames).min(sample.frames);
        let start = o.offset.min(end);
        Voice {
            sample,
            pos: start,
            start,
            end,
            // a voice played now gets its frame when the mixer takes it
            start_at: if due.is_some() { u64::MAX } else { 0 },
            due,
            cut_at: u64::MAX,
            vol: o.volume,
            key: o.key,
            bus: o.bus,
            looping: o.looping,
            group: o.group.min(GROUPS),
        }
    }

    /// Add the voice's part of the buffer starting at frame `now` to `mix`
    /// (interleaved stereo) at `gain`; false once it is done.
    fn render(&mut self, mix: &mut [f32], now: u64, gain: f32) -> bool {
        let end_frame = now + (mix.len() / 2) as u64;
        if self.start_at >= end_frame {
            return true;
        }
        if self.cut_at <= now {
            return false;
        }
        let first = self.start_at.saturating_sub(now) as usize;
        let last = (self.cut_at.min(end_frame) - now) as usize;
        let mut i = first;
        while i < last {
            if self.pos >= self.end {
                if self.looping && self.end > self.start {
                    self.pos = self.start;
                } else {
                    return false;
                }
            }
            let n = (last - i).min(self.end - self.pos);
            if gain != 0.0 {
                self.sample
                    .mix_into(self.pos, &mut mix[i * 2..(i + n) * 2], gain);
            }
            self.pos += n;
            i += n;
        }
        self.cut_at > end_frame && (self.pos < self.end || self.looping)
    }
}

/// Hands a voice's sample back to the output to drop out of the audio
/// thread (it may be the last reference: freeing megabytes in the audio
/// callback could make it late); false, for `retain`.
fn retire(to: &SyncSender<Arc<Sample>>, v: &Voice) -> bool {
    let _ = to.try_send(v.sample.clone());
    false
}

pub(crate) enum Cmd {
    Play(Voice),
    Volume(u64, f32),
    Stop(u64),
    StopBus(Bus),
    BusVolume(Bus, f32),
    BusGain(Bus, f32),
    Master(f32),
    GroupVolume(u8, f32),
}

pub(crate) struct State {
    pub voices: Vec<Voice>,
    pub frame: u64,
    bus: [f32; 3],
    /// Each bus's gain besides its volume (loudness normalisation).
    gain: [f32; 3],
    /// Group 0 (none) is always 1.
    group: [f32; GROUPS as usize + 1],
    /// The master volume asked for, and the one the last frame had: it
    /// glides to the one asked for by `glide` a frame (full scale in
    /// `MASTER_GLIDE_MS`), so a mute fades rather than clicks.
    master: f32,
    master_now: f32,
    glide: f32,
    mix: Vec<f32>,
    max_voices: usize,
    /// Turns `Due::Us` into frames: fed by the output with what it knows
    /// of the time.
    pub clock: FrameClock,
    retired: SyncSender<Arc<Sample>>,
}

impl State {
    /// `retired` takes the samples of the voices that end (room for the
    /// voices that end between two reads is twice `max_voices`).
    pub fn new(cfg: &AudioConfig, rate: u32, retired: SyncSender<Arc<Sample>>) -> State {
        State {
            voices: Vec::with_capacity(cfg.max_voices),
            frame: 0,
            bus: [cfg.key_volume, cfg.bgm_volume, cfg.system_volume],
            gain: [1.0; 3],
            group: [1.0; GROUPS as usize + 1],
            master: cfg.master,
            master_now: cfg.master,
            glide: 1000.0 / (crate::MASTER_GLIDE_MS as f32 * rate.max(1) as f32),
            mix: vec![0.0; 8192],
            max_voices: cfg.max_voices.max(16),
            clock: FrameClock::new(rate),
            retired,
        }
    }

    /// A voice of key `key` starts at `at`: the others of that key that
    /// start before end there (one voice per key).
    fn cut_key(&mut self, key: u64, at: u64, except: Option<usize>) {
        if key == 0 {
            return;
        }
        for (i, o) in self.voices.iter_mut().enumerate() {
            if o.key == key && o.due.is_none() && o.start_at <= at && Some(i) != except {
                o.cut_at = o.cut_at.min(at);
            }
        }
    }

    pub fn apply(&mut self, c: Cmd) {
        match c {
            Cmd::Play(mut v) => {
                if v.due.is_none() {
                    // played now (or late): it starts here, which is what
                    // the eviction below goes by
                    v.start_at = self.frame;
                    self.cut_key(v.key, v.start_at, None);
                }
                if self.voices.len() >= self.max_voices {
                    // drop the voice that started first
                    if let Some(i) = (0..self.voices.len()).min_by_key(|i| self.voices[*i].start_at)
                    {
                        retire(&self.retired, &self.voices.swap_remove(i));
                    }
                }
                self.voices.push(v);
            }
            Cmd::Volume(k, vol) => self
                .voices
                .iter_mut()
                .filter(|v| v.key == k)
                .for_each(|v| v.vol = vol),
            Cmd::Stop(k) => self
                .voices
                .retain(|v| v.key != k || retire(&self.retired, v)),
            Cmd::StopBus(b) => self
                .voices
                .retain(|v| v.bus != b || retire(&self.retired, v)),
            Cmd::BusVolume(b, v) => self.bus[b as usize] = v,
            Cmd::BusGain(b, v) => self.gain[b as usize] = v,
            Cmd::Master(v) => {
                self.master = v;
                // (set up before anything was heard: at once)
                if self.frame == 0 {
                    self.master_now = v;
                }
            }
            Cmd::GroupVolume(g, v) if (1..=GROUPS).contains(&g) => self.group[g as usize] = v,
            Cmd::GroupVolume(..) => {}
        }
    }

    /// The next `frames` stereo frames (interleaved, unclipped), with what
    /// the output knows of the time (see `clock`).
    pub fn render(&mut self, frames: usize, obs: Option<Obs>) -> &[f32] {
        if let Some(o) = obs {
            self.clock.observe(o);
        }
        if self.mix.len() < frames * 2 {
            self.mix.resize(frames * 2, 0.0);
        }
        let now = self.frame;
        let end_frame = now + frames as u64;
        // voices due in this buffer get their frame; one already past
        // starts now, from its beginning
        for i in 0..self.voices.len() {
            let f = match self.voices[i].due {
                None => continue,
                Some(Due::Us(us)) => self.clock.frame_at(us),
                Some(Due::Frame(f)) => f as f64,
            };
            if f < end_frame as f64 {
                let at = (f.round().max(0.0) as u64).max(now);
                self.voices[i].start_at = at;
                self.voices[i].due = None;
                self.cut_key(self.voices[i].key, at, Some(i));
            }
        }
        let mix = &mut self.mix[..frames * 2];
        mix.fill(0.0);
        // the master in the voices' gain, or while it glides on the sum
        let steady = self.master_now == self.master;
        let master = if steady { self.master } else { 1.0 };
        let bus = [0, 1, 2].map(|b| self.bus[b] * self.gain[b]);
        let (group, retired) = (self.group, &self.retired);
        self.voices.retain_mut(|v| {
            let g = v.vol * bus[v.bus as usize] * group[v.group as usize] * master;
            v.render(mix, now, g) || retire(retired, v)
        });
        if !steady {
            let (to, step) = (self.master, self.glide);
            let mut m = self.master_now;
            for f in mix.as_chunks_mut::<2>().0 {
                m = if m < to {
                    (m + step).min(to)
                } else {
                    (m - step).max(to)
                };
                f[0] *= m;
                f[1] *= m;
            }
            self.master_now = m;
        }
        self.frame = end_frame;
        mix
    }
}

#[inline]
pub(crate) fn soft_clip(x: f32) -> f32 {
    // transparent below ~0.8, smooth limiting above
    if x.abs() <= 0.8 {
        x
    } else {
        let s = x.signum();
        let a = x.abs() - 0.8;
        s * (0.8 + 0.2 * (a / (a + 0.2)))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    pub fn state() -> State {
        let cfg = AudioConfig {
            master: 1.0,
            key_volume: 1.0,
            bgm_volume: 1.0,
            system_volume: 1.0,
            max_voices: 64,
            ..Default::default()
        };
        State::new(&cfg, 44_100, sync_channel(1).0)
    }

    pub fn sample(frames: usize, v: i16) -> Arc<Sample> {
        Arc::new(Sample {
            channels: 1,
            frames,
            data: vec![v; frames],
        })
    }

    fn voice(s: &Arc<Sample>, due: Option<Due>, key: u64) -> Voice {
        let o = PlayOptions {
            key,
            ..Default::default()
        };
        Voice::new(s.clone(), &o, due)
    }

    #[test]
    fn group_volumes_scale_their_voices() {
        let mut st = state();
        let s = sample(16, 16384);
        let o = |group| PlayOptions {
            group,
            ..Default::default()
        };
        st.apply(Cmd::GroupVolume(2, 0.25));
        st.apply(Cmd::Play(Voice::new(s.clone(), &o(2), None)));
        assert!((st.render(4, None)[0] - 0.125).abs() < 1e-3);
        st.apply(Cmd::GroupVolume(2, 1.0));
        st.apply(Cmd::Play(Voice::new(s, &o(0), None)));
        assert!((st.render(4, None)[0] - 1.0).abs() < 1e-3);
    }

    /// A new master volume is reached over at most `MASTER_GLIDE_MS`, a
    /// step each frame: a mute fades out instead of cutting off.
    #[test]
    fn master_glides_to_a_new_volume() {
        let mut st = state();
        let s = sample(44_100, 16384);
        st.apply(Cmd::Play(Voice::new(s, &PlayOptions::default(), None)));
        assert!((st.render(4, None)[0] - 0.5).abs() < 1e-3);
        st.apply(Cmd::Master(0.0));
        let glide = 44_100 * crate::MASTER_GLIDE_MS as usize / 1000;
        let mix = st.render(glide, None).to_vec();
        // falling frame by frame, silent at the end
        assert!(mix[0] < 0.5 && mix[0] > 0.499);
        assert!((mix[glide] - 0.25).abs() < 1e-3);
        assert!(mix
            .chunks(2)
            .zip(mix.chunks(2).skip(1))
            .all(|(a, b)| b[0] <= a[0]));
        assert!(mix[2 * glide - 2].abs() < 1e-3);
        assert!(st.render(8, None).iter().all(|x| *x == 0.0));
        // and back
        st.apply(Cmd::Master(1.0));
        let mix = st.render(glide, None).to_vec();
        assert!(mix[0] > 0.0 && mix[0] < 0.001);
        assert!((mix[2 * glide - 2] - 0.5).abs() < 1e-3);
        assert!((st.render(4, None)[0] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn bus_gains_scale_their_bus_besides_its_volume() {
        let mut st = state();
        let s = sample(16, 16384);
        let o = |bus| PlayOptions {
            bus,
            ..Default::default()
        };
        st.apply(Cmd::BusVolume(Bus::Key, 0.5));
        st.apply(Cmd::BusGain(Bus::Key, 0.5));
        st.apply(Cmd::BusGain(Bus::Bgm, 2.0));
        st.apply(Cmd::Play(Voice::new(s.clone(), &o(Bus::Key), None)));
        assert!((st.render(4, None)[0] - 0.125).abs() < 1e-3);
        st.apply(Cmd::Stop(0));
        st.apply(Cmd::Play(Voice::new(s.clone(), &o(Bus::Bgm), None)));
        assert!((st.render(4, None)[0] - 1.0).abs() < 1e-3);
        st.apply(Cmd::Stop(0));
        st.apply(Cmd::Play(Voice::new(s, &o(Bus::System), None)));
        assert!((st.render(4, None)[0] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn scheduled_start_is_sample_accurate() {
        let mut st = state();
        let s = sample(10, 16384);
        st.apply(Cmd::Play(voice(&s, Some(Due::Frame(5)), 0)));
        let mix = st.render(8, None);
        assert_eq!(mix[2 * 4], 0.0);
        assert!((mix[2 * 5] - 0.5).abs() < 1e-3);
        let mix = st.render(8, None);
        // frames 8..15 continue the voice (frames 3..9 of sample at 8..12)
        assert!((mix[2 * 6] - 0.5).abs() < 1e-3);
        assert_eq!(mix[2 * 7], 0.0);
        assert!(st.voices.is_empty());
    }

    /// Callbacks of a device `ppm` off the system clock for ten minutes;
    /// a sound due every 7 s, sent 150 ms ahead: each starts where a key
    /// pressed at its moment would (half a buffer later on average), not
    /// where the nominal rate would put it.
    #[test]
    fn deadlines_stay_on_the_judge_clock_with_a_drifting_device() {
        for ppm in [-200.0f64, 200.0] {
            let mut st = state();
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
                    st.apply(Cmd::Play(voice(&s, Some(Due::Us(next)), key)));
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
                st.render(buf, Some(obs));
                due.retain(|(k, d)| match st.voices.iter().find(|v| v.key == *k) {
                    Some(v) if v.due.is_none() => {
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
        for due in [Due::Us(0.0), Due::Frame(0)] {
            let mut st = state();
            st.render(480, None);
            let s = sample(100, 16384);
            st.apply(Cmd::Play(voice(&s, Some(due), 3)));
            let mix = st.render(10, None);
            assert!((mix[0] - 0.5).abs() < 1e-3);
            assert_eq!(st.voices[0].start_at, 480);
            assert_eq!(st.voices[0].pos, 10);
        }
    }

    /// A keysound played now does not cut a voice of the same key that is
    /// due later; that one cuts the keysound when it starts. Due on the
    /// judge's clock (native) or on a frame (web) alike.
    #[test]
    fn a_scheduled_voice_is_cut_only_by_a_later_one() {
        // frame 441: 10 ms at the nominal 44.1 kHz
        for due in [Due::Us(10_000.0), Due::Frame(441)] {
            let mut st = state();
            let s = sample(1000, 16384);
            st.apply(Cmd::Play(voice(&s, Some(due), 9)));
            st.apply(Cmd::Play(voice(&s, None, 9)));
            st.render(100, None);
            assert_eq!(st.voices.len(), 2);
            let mix = st.render(400, None);
            assert!((mix[2 * 340] - 0.5).abs() < 1e-3);
            assert!((mix[2 * 341] - 0.5).abs() < 1e-3);
            // the scheduled one started and cut the first there
            assert_eq!(st.voices.len(), 1);
            assert_eq!(st.voices[0].start_at, 441);
        }
    }

    #[test]
    fn same_key_cuts_previous_voice() {
        let mut st = state();
        let s = sample(100, 16384);
        st.apply(Cmd::Play(voice(&s, None, 7)));
        st.apply(Cmd::Play(voice(&s, Some(Due::Frame(10)), 7)));
        let mix = st.render(20, None);
        // before frame 10 one voice, afterwards still exactly one voice
        assert!((mix[2 * 5] - 0.5).abs() < 1e-3);
        assert!((mix[2 * 15] - 0.5).abs() < 1e-3);
        assert_eq!(st.voices.len(), 1);
    }

    /// Mono and stereo voices (sliced, looping, scheduled) over buffers of
    /// odd sizes sum to what reading them frame by frame gives.
    #[test]
    fn render_equals_the_frame_by_frame_sum() {
        let mut st = state();
        let pcm = |ch: u16, frames: usize| {
            Arc::new(Sample {
                channels: ch,
                frames,
                data: (0..frames * ch as usize)
                    .map(|i| (i * 7919 % 65536) as u16 as i16)
                    .collect(),
            })
        };
        let (mono, stereo) = (pcm(1, 300), pcm(2, 250));
        // (sample, start_at, slice, looping, volume)
        let voices = [
            (&mono, 0u64, 0..300, false, 1.0),
            (&stereo, 0, 20..200, false, 0.7),
            (&mono, 50, 10..90, true, 0.3),
            (&stereo, 123, 0..61, true, 1.3),
        ];
        for (s, start_at, r, looping, vol) in voices.iter().cloned() {
            let o = PlayOptions {
                volume: vol,
                offset: r.start,
                end: Some(r.end),
                looping,
                ..Default::default()
            };
            let due = (start_at > 0).then_some(Due::Frame(start_at));
            st.apply(Cmd::Play(Voice::new(s.clone(), &o, due)));
        }
        let mut t = 0;
        for buf in [37, 64, 100, 1, 128, 77, 200] {
            let mix = st.render(buf, None).to_vec();
            for f in 0..buf {
                let (mut l, mut r) = (0.0f32, 0.0f32);
                for (s, start_at, range, looping, vol) in voices.iter().cloned() {
                    let Some(k) = (t + f).checked_sub(start_at as usize) else {
                        continue;
                    };
                    if !looping && k >= range.len() {
                        continue;
                    }
                    let i = range.start + k % range.len();
                    let ch = s.channels as usize;
                    let at = |c: usize| s.data[i * ch + c.min(ch - 1)] as f32 / 32768.0 * vol;
                    l += at(0);
                    r += at(1);
                }
                assert_eq!((mix[2 * f], mix[2 * f + 1]), (l, r), "frame {}", t + f);
            }
            t += buf;
        }
    }

    /// With all voices in use, the one that started first makes room,
    /// whether it was played now or scheduled.
    #[test]
    fn a_full_mixer_drops_the_voice_that_started_first() {
        let mut st = state();
        let s = sample(1000, 16384);
        st.render(100, None);
        st.apply(Cmd::Play(voice(&s, Some(Due::Us(0.0)), 1)));
        st.render(100, None);
        assert_eq!(st.voices[0].start_at, 100);
        for key in 2..=64 {
            st.apply(Cmd::Play(voice(&s, None, key)));
        }
        st.render(100, None);
        st.apply(Cmd::Play(voice(&s, None, 65)));
        st.render(1, None);
        let mut keys: Vec<u64> = st.voices.iter().map(|v| v.key).collect();
        keys.sort();
        assert_eq!(keys, (2..=65).collect::<Vec<_>>());
    }

    /// A sample whose last reference was a voice is dropped by whoever
    /// reads `retired`, not in the callback.
    #[test]
    fn the_samples_of_ended_voices_go_back() {
        let mut st = state();
        let (retire, retired) = sync_channel(4);
        st.retired = retire;
        st.apply(Cmd::Play(voice(&sample(10, 1), None, 0)));
        st.apply(Cmd::Play(voice(&sample(1000, 1), None, 4)));
        st.render(20, None);
        st.apply(Cmd::Stop(4));
        st.render(1, None);
        assert!(st.voices.is_empty());
        let back: Vec<_> = retired.try_iter().collect();
        assert_eq!(
            back.iter().map(|s| s.frames).collect::<Vec<_>>(),
            [10, 1000]
        );
        assert!(back.iter().all(|s| Arc::strong_count(s) == 1));
    }
}
