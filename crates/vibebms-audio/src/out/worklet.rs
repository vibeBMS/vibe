//! The web's mixer in its AudioWorklet: `voice::State` in a second instance
//! of the app's wasm module, run by the processor in `web.rs`. The page
//! sends it the samples and the commands as numbers (`encode`); it renders
//! each quantum into a buffer the processor copies out.

use crate::decode::Sample;
use crate::voice::{soft_clip, Bus, Cmd, Due, State, Voice};
use crate::{AudioConfig, PlayOptions};
use std::collections::BTreeMap;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;

/// A command on its way to the worklet: an opcode and its arguments (a
/// played sample by its id there).
pub(crate) type Wire = [f64; 10];

const PLAY: u8 = 0;
const VOLUME: u8 = 1;
const STOP: u8 = 2;
const STOP_BUS: u8 = 3;
const BUS_VOLUME: u8 = 4;
const MASTER: u8 = 5;
const FREE: u8 = 6;
const GROUP_VOLUME: u8 = 7;

fn wire(op: u8, args: &[f64]) -> Wire {
    let mut w = [0.0; 10];
    w[0] = op as f64;
    w[1..=args.len()].copy_from_slice(args);
    w
}

/// `c` for the worklet. A `Play` is due on a frame or now (its `Due::Us`
/// is the page's to turn into a frame), its sample `sample`.
pub(crate) fn encode(c: &Cmd, sample: u32) -> Wire {
    match c {
        Cmd::Play(v) => {
            let at = match v.due {
                Some(Due::Frame(f)) => f as f64,
                _ => -1.0,
            };
            let args = [
                sample as f64,
                v.start as f64,
                v.end as f64,
                at,
                v.vol as f64,
                v.key as f64,
                v.bus as u32 as f64,
                v.looping as u32 as f64,
                v.group as f64,
            ];
            wire(PLAY, &args)
        }
        Cmd::Volume(k, v) => wire(VOLUME, &[*k as f64, *v as f64]),
        Cmd::Stop(k) => wire(STOP, &[*k as f64]),
        Cmd::StopBus(b) => wire(STOP_BUS, &[*b as u32 as f64]),
        Cmd::BusVolume(b, v) => wire(BUS_VOLUME, &[*b as u32 as f64, *v as f64]),
        Cmd::Master(v) => wire(MASTER, &[*v as f64]),
        Cmd::GroupVolume(g, v) => wire(GROUP_VOLUME, &[*g as f64, *v as f64]),
    }
}

/// The worklet no longer needs sample `id`.
pub(crate) fn free(id: u32) -> Wire {
    wire(FREE, &[id as f64])
}

pub(crate) struct Worklet {
    state: State,
    samples: BTreeMap<u32, Arc<Sample>>,
    retired: Receiver<Arc<Sample>>,
    /// The next sample's data, written by the processor.
    upload: Vec<i16>,
    /// The last quantum: left, then right.
    out: Vec<f32>,
}

impl Worklet {
    pub fn new(max_voices: usize, rate: u32) -> Worklet {
        let cfg = AudioConfig {
            max_voices,
            ..Default::default()
        };
        let (retire, retired) = sync_channel(max_voices.max(16) * 2);
        Worklet {
            state: State::new(&cfg, rate, retire),
            samples: BTreeMap::new(),
            retired,
            upload: Vec::new(),
            out: vec![0.0; 2 * 128],
        }
    }

    /// The frame the next quantum starts with.
    pub fn frame(&self) -> u64 {
        self.state.frame
    }

    /// Room for the next sample's `len` values.
    pub fn upload(&mut self, len: usize) -> &mut [i16] {
        self.upload = vec![0; len];
        &mut self.upload
    }

    /// What was uploaded is sample `id`.
    pub fn add_sample(&mut self, id: u32, channels: u16, frames: usize) {
        let s = Sample {
            channels,
            frames,
            data: std::mem::take(&mut self.upload),
        };
        self.samples.insert(id, Arc::new(s));
    }

    pub fn command(&mut self, w: &Wire) {
        let bus = |i: f64| match i as u32 {
            0 => Bus::Key,
            1 => Bus::Bgm,
            _ => Bus::System,
        };
        let c = match w[0] as u8 {
            PLAY => {
                let Some(s) = self.samples.get(&(w[1] as u32)) else {
                    return;
                };
                let o = PlayOptions {
                    offset: w[2] as usize,
                    end: Some(w[3] as usize),
                    volume: w[5] as f32,
                    key: w[6] as u64,
                    bus: bus(w[7]),
                    looping: w[8] != 0.0,
                    group: w[9] as u8,
                    at: None,
                };
                let due = (w[4] >= 0.0).then_some(Due::Frame(w[4] as u64));
                Cmd::Play(Voice::new(s.clone(), &o, due))
            }
            VOLUME => Cmd::Volume(w[1] as u64, w[2] as f32),
            STOP => Cmd::Stop(w[1] as u64),
            STOP_BUS => Cmd::StopBus(bus(w[1])),
            BUS_VOLUME => Cmd::BusVolume(bus(w[1]), w[2] as f32),
            MASTER => Cmd::Master(w[1] as f32),
            GROUP_VOLUME => Cmd::GroupVolume(w[1] as u8, w[2] as f32),
            FREE => {
                self.samples.remove(&(w[1] as u32));
                return;
            }
            _ => return,
        };
        self.state.apply(c);
        // between quanta: a sample dropped here is not freed in one
        self.retired.try_iter().for_each(drop);
    }

    /// Mix the next quantum of `frames`: left then right, clipped.
    pub fn render(&mut self, frames: usize) -> &[f32] {
        if self.out.len() < 2 * frames {
            self.out.resize(2 * frames, 0.0);
        }
        let mix = self.state.render(frames, None);
        let (l, r) = self.out[..2 * frames].split_at_mut(frames);
        for ((l, r), &[ml, mr]) in l.iter_mut().zip(r).zip(mix.as_chunks::<2>().0) {
            *l = soft_clip(ml);
            *r = soft_clip(mr);
        }
        &self.out[..2 * frames]
    }
}

/// The processor's calls (`web.rs`), on the one worklet of this instance.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::Worklet;
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    thread_local! {
        static MIX: RefCell<Option<Worklet>> = const { RefCell::new(None) };
    }

    fn with<R>(f: impl FnOnce(&mut Worklet) -> R) -> R {
        MIX.with_borrow_mut(|w| f(w.as_mut().expect("vibe_mix_new first")))
    }

    #[wasm_bindgen]
    pub fn vibe_mix_new(max_voices: u32, rate: f64) {
        MIX.set(Some(Worklet::new(max_voices as usize, rate as u32)));
    }

    #[wasm_bindgen]
    #[allow(clippy::too_many_arguments)]
    pub fn vibe_mix_cmd(
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
        h: f64,
        i: f64,
        j: f64,
    ) {
        with(|w| w.command(&[a, b, c, d, e, f, g, h, i, j]));
    }

    #[wasm_bindgen]
    pub fn vibe_mix_upload(len: u32) -> u32 {
        with(|w| w.upload(len as usize).as_mut_ptr() as u32)
    }

    #[wasm_bindgen]
    pub fn vibe_mix_sample(id: u32, channels: u32, frames: u32) {
        with(|w| w.add_sample(id, channels as u16, frames as usize));
    }

    #[wasm_bindgen]
    pub fn vibe_mix_render(frames: u32) -> u32 {
        with(|w| w.render(frames as usize).as_ptr() as u32)
    }

    #[wasm_bindgen]
    pub fn vibe_mix_frame() -> f64 {
        with(|w| w.frame() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::tests::{sample, state};

    fn send(w: &mut Worklet, c: Cmd, id: u32) {
        w.command(&encode(&c, id));
    }

    fn play(s: &Arc<Sample>, due: Option<Due>, o: PlayOptions) -> Cmd {
        Cmd::Play(Voice::new(s.clone(), &o, due))
    }

    /// Commands through the wire mix what they mix given to the mixer
    /// directly, clipped and split into left and right.
    #[test]
    fn the_worklet_mixes_what_the_mixer_does() {
        let mut w = Worklet::new(64, 44_100);
        let mut st = state();
        let pcm: Vec<i16> = (0..600).map(|i| (i * 977 % 50_000) as i16).collect();
        let s = Arc::new(Sample {
            channels: 2,
            frames: 300,
            data: pcm.clone(),
        });
        w.upload(pcm.len()).copy_from_slice(&pcm);
        w.add_sample(3, 2, 300);
        let cmds = || {
            let o = |key, volume, bus, offset, end, looping| PlayOptions {
                key,
                volume,
                bus,
                offset,
                end,
                looping,
                at: None,
                group: 0,
            };
            [
                play(&s, None, o(1, 0.9, Bus::Key, 0, None, false)),
                play(
                    &s,
                    Some(Due::Frame(70)),
                    o(1, 1.5, Bus::Bgm, 10, Some(90), true),
                ),
                play(
                    &s,
                    Some(Due::Frame(200)),
                    o(2, 0.4, Bus::System, 250, None, false),
                ),
                Cmd::BusVolume(Bus::Key, 1.0),
                Cmd::BusVolume(Bus::Bgm, 0.6),
                Cmd::BusVolume(Bus::System, 1.0),
                Cmd::Master(0.9),
                Cmd::Volume(2, 0.8),
            ]
        };
        for (a, b) in cmds().into_iter().zip(cmds()) {
            send(&mut w, a, 3);
            st.apply(b);
        }
        for q in 0..4 {
            if q == 2 {
                send(&mut w, Cmd::Stop(1), 0);
                st.apply(Cmd::Stop(1));
            }
            let want: Vec<f32> = st.render(128, None).iter().map(|x| soft_clip(*x)).collect();
            let got = w.render(128);
            let (l, r) = got.split_at(128);
            for f in 0..128 {
                assert_eq!(
                    (l[f], r[f]),
                    (want[2 * f], want[2 * f + 1]),
                    "quantum {q} frame {f}"
                );
            }
        }
        assert_eq!(w.frame(), 512);
    }

    /// A keysound played now leaves a voice of the same sound scheduled
    /// later alone (it used to cut it to nothing on the web).
    #[test]
    fn a_keysound_now_does_not_cut_a_later_voice_of_its_key() {
        let mut w = Worklet::new(64, 44_100);
        w.upload(1000).fill(16384);
        w.add_sample(0, 1, 1000);
        let o = PlayOptions {
            key: 5,
            ..Default::default()
        };
        let s = sample(1000, 16384);
        send(&mut w, Cmd::Master(1.0), 0);
        send(&mut w, play(&s, Some(Due::Frame(300)), o), 0);
        send(&mut w, play(&s, None, o), 0);
        let mut heard = Vec::new();
        for _ in 0..4 {
            heard.extend_from_slice(&w.render(128)[..128]);
        }
        // the keysound until frame 300, the scheduled one from there
        assert!(heard[..512].iter().all(|x| (x - 0.5).abs() < 1e-3));
        assert_eq!(w.state.voices.len(), 1);
        assert_eq!(w.state.voices[0].start_at, 300);
    }

    #[test]
    fn a_freed_or_unknown_sample_plays_nothing() {
        let mut w = Worklet::new(16, 48_000);
        w.upload(10).fill(1);
        w.add_sample(1, 1, 10);
        w.command(&free(1));
        let s = sample(10, 1);
        send(&mut w, play(&s, None, PlayOptions::default()), 1);
        send(&mut w, play(&s, None, PlayOptions::default()), 2);
        assert!(w.state.voices.is_empty());
    }
}
