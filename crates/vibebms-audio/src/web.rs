//! The web build's audio: the mixer runs in an AudioWorklet, on the
//! browser's audio thread. The page sends it the samples (once each, when
//! first played) and what to play when; a key's sound starts in the next
//! render quantum (128 frames), not behind a queue the page must keep full,
//! and nothing the page does late (drawing, input) breaks the sound.
//!
//! The worklet's frame count is the mixer's clock; it reports it with the
//! context time, so the page knows the frame playing now and, with the
//! context's output timestamp, the frame heard now.

use crate::decode::Sample;
use crate::mixer::{Bus, PlayOptions};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// The mixer, as the Rust one (`mixer::State::render`): voices from a frame,
/// cut by a newer voice of the same key, bus and master volumes, a soft
/// clip. Messages: ["s", id, channels, frames, Int16Array] a sample,
/// ["f", id] free it, ["p", id, start, end, at (-1: now), volume, key, bus,
/// loop] play, ["v", key, volume], ["x", key] stop, ["xb", bus] stop a bus,
/// ["b", bus, volume], ["m", volume]. Every 8 quanta it reports [frame,
/// currentTime] (the frame its quantum starts with, and when).
const PROCESSOR: &str = r#"
const S = 1 / 32768;
function clip(x) {
  const a = Math.abs(x);
  if (a <= 0.8) return x;
  const e = a - 0.8;
  return Math.sign(x) * (0.8 + 0.2 * (e / (e + 0.2)));
}
class VibeMix extends AudioWorkletProcessor {
  constructor(o) {
    super();
    this.samples = new Map();
    this.voices = [];
    this.frame = 0;
    this.bus = [1, 1, 1];
    this.master = 1;
    this.max = (o && o.processorOptions && o.processorOptions.maxVoices) || 512;
    this.n = 0;
    this.port.onmessage = (e) => this.cmd(e.data);
  }
  cmd(m) {
    switch (m[0]) {
      case "s": this.samples.set(m[1], { ch: m[2], frames: m[3], d: m[4] }); break;
      case "f": this.samples.delete(m[1]); break;
      case "p": {
        const s = this.samples.get(m[1]);
        if (!s) return;
        const at = m[4] < 0 ? this.frame : m[4];
        if (m[6]) for (const o of this.voices)
          if (o.key === m[6]) o.cut = Math.min(o.cut, Math.max(at, this.frame));
        if (this.voices.length >= this.max) {
          let k = 0;
          for (let i = 1; i < this.voices.length; i++)
            if (this.voices[i].at < this.voices[k].at) k = i;
          this.voices.splice(k, 1);
        }
        const end = Math.min(m[3], s.frames);
        this.voices.push({ s, pos: Math.min(m[2], end), start: Math.min(m[2], end), end,
          at, cut: Infinity, vol: m[5], key: m[6], bus: m[7], loop: m[8] });
        break;
      }
      case "v": for (const o of this.voices) if (o.key === m[1]) o.vol = m[2]; break;
      case "x": this.voices = this.voices.filter((o) => o.key !== m[1]); break;
      case "xb": this.voices = this.voices.filter((o) => o.bus !== m[1]); break;
      case "b": this.bus[m[1]] = m[2]; break;
      case "m": this.master = m[1]; break;
    }
  }
  process(inputs, outputs) {
    const out = outputs[0];
    const L = out[0], R = out[1] || out[0];
    const n = L.length;
    if ((this.n++ & 7) === 0) this.port.postMessage([this.frame, currentTime]);
    L.fill(0);
    if (R !== L) R.fill(0);
    const now = this.frame, end = now + n;
    const bus = this.bus, master = this.master;
    this.voices = this.voices.filter((v) => {
      if (v.at >= end) return true;
      if (v.cut <= now) return false;
      let i = Math.max(0, v.at - now);
      const last = Math.min(v.cut, end) - now;
      const g = v.vol * bus[v.bus] * master * S;
      const d = v.s.d, ch = v.s.ch;
      while (i < last) {
        if (v.pos >= v.end) {
          if (v.loop && v.end > v.start) v.pos = v.start;
          else return false;
        }
        const k = Math.min(last - i, v.end - v.pos);
        if (g !== 0) {
          if (ch === 1) {
            for (let j = 0; j < k; j++) { const x = d[v.pos + j] * g; L[i + j] += x; R[i + j] += x; }
          } else {
            for (let j = 0; j < k; j++) {
              const b = (v.pos + j) * ch;
              L[i + j] += d[b] * g;
              R[i + j] += d[b + 1] * g;
            }
          }
        }
        v.pos += k;
        i += k;
      }
      return v.cut > end && (v.pos < v.end || v.loop);
    });
    for (let i = 0; i < n; i++) { L[i] = clip(L[i]); if (R !== L) R[i] = clip(R[i]); }
    this.frame = end;
    return true;
  }
}
registerProcessor("vibe-mix", VibeMix);
"#;

/// The mixer's clock: (frame, at this context time).
#[derive(Clone, Copy, Default)]
struct Clock {
    frame: f64,
    at: f64,
}

/// The node's message handler, kept alive with it.
type OnMessage = Rc<RefCell<Option<Closure<dyn FnMut(web_sys::MessageEvent)>>>>;

/// The samples sent to the worklet (by the address of their `Arc`), with
/// their id there; dropped there when nothing else holds them.
#[derive(Default)]
struct Sent {
    ids: HashMap<usize, (u32, Arc<Sample>)>,
    next: u32,
}

pub(crate) struct WebOut {
    pub ctx: web_sys::AudioContext,
    pub rate: u32,
    node: Rc<RefCell<Option<web_sys::AudioWorkletNode>>>,
    clock: Rc<Cell<Option<Clock>>>,
    sent: Rc<RefCell<Sent>>,
    /// Messages before the node is up (the module loads first).
    early: Rc<RefCell<Vec<js_sys::Array>>>,
    _ready: Closure<dyn FnMut(JsValue)>,
    _message: OnMessage,
    _sweep: Closure<dyn FnMut()>,
}

fn js_f64(o: &JsValue, k: &str) -> Option<f64> {
    js_sys::Reflect::get(o, &JsValue::from_str(k))
        .ok()?
        .as_f64()
}

fn msg(parts: &[JsValue]) -> js_sys::Array {
    parts.iter().collect()
}

impl WebOut {
    pub fn open(max_voices: usize) -> Result<WebOut, String> {
        let ctx = web_sys::AudioContext::new().map_err(|e| format!("{e:?}"))?;
        let rate = ctx.sample_rate() as u32;
        let worklet = ctx
            .audio_worklet()
            .map_err(|e| format!("no AudioWorklet: {e:?}"))?;
        let url = {
            let parts = js_sys::Array::of1(&JsValue::from_str(PROCESSOR));
            let opts = web_sys::BlobPropertyBag::new();
            opts.set_type("text/javascript");
            web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts)
                .and_then(|b| web_sys::Url::create_object_url_with_blob(&b))
                .map_err(|e| format!("{e:?}"))?
        };
        let node: Rc<RefCell<Option<web_sys::AudioWorkletNode>>> = Rc::default();
        let clock: Rc<Cell<Option<Clock>>> = Rc::default();
        let early: Rc<RefCell<Vec<js_sys::Array>>> = Rc::default();
        let message: OnMessage = Rc::default();
        let ready = {
            let (ctx, node, clock, early, message) = (
                ctx.clone(),
                node.clone(),
                clock.clone(),
                early.clone(),
                message.clone(),
            );
            Closure::<dyn FnMut(JsValue)>::new(move |_| {
                let opts = web_sys::AudioWorkletNodeOptions::new();
                opts.set_number_of_inputs(0);
                opts.set_output_channel_count(&js_sys::Array::of1(&JsValue::from(2)));
                let po = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&po, &"maxVoices".into(), &(max_voices as f64).into());
                opts.set_processor_options(Some(&po));
                let n = match web_sys::AudioWorkletNode::new_with_options(&ctx, "vibe-mix", &opts) {
                    Ok(n) => n,
                    Err(e) => return log::warn!("audio worklet: {e:?}"),
                };
                let _ = n.connect_with_audio_node(&ctx.destination());
                let clock = clock.clone();
                let on = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
                    move |e: web_sys::MessageEvent| {
                        let a: js_sys::Array = e.data().unchecked_into();
                        if let (Some(frame), Some(at)) = (a.get(0).as_f64(), a.get(1).as_f64()) {
                            clock.set(Some(Clock { frame, at }));
                        }
                    },
                );
                if let Ok(port) = n.port() {
                    port.set_onmessage(Some(on.as_ref().unchecked_ref()));
                    // what was asked before it was up (the samples' data
                    // has been handed over: its buffers go with it)
                    for m in early.borrow_mut().drain(..) {
                        let _ = post(&port, &m);
                    }
                }
                *message.borrow_mut() = Some(on);
                *node.borrow_mut() = Some(n);
                log::info!("audio: worklet mixer running");
            })
        };
        let p = worklet.add_module(&url).map_err(|e| format!("{e:?}"))?;
        let _ = p.then(&ready);
        // samples nothing else holds any more: freed in the worklet
        let sent: Rc<RefCell<Sent>> = Rc::default();
        let sweep = {
            let (sent, node, early) = (sent.clone(), node.clone(), early.clone());
            Closure::<dyn FnMut()>::new(move || {
                let mut s = sent.borrow_mut();
                let gone: Vec<usize> = s
                    .ids
                    .iter()
                    .filter(|(_, (_, a))| Arc::strong_count(a) == 1)
                    .map(|(k, _)| *k)
                    .collect();
                for k in gone {
                    if let Some((id, _)) = s.ids.remove(&k) {
                        send(&node, &early, msg(&["f".into(), id.into()]));
                    }
                }
            })
        };
        if let Some(w) = web_sys::window() {
            let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(
                sweep.as_ref().unchecked_ref(),
                2000,
            );
        }
        Ok(WebOut {
            ctx,
            rate,
            node,
            clock,
            sent,
            early,
            _ready: ready,
            _message: message,
            _sweep: sweep,
        })
    }

    fn send(&self, m: js_sys::Array) {
        send(&self.node, &self.early, m);
    }

    /// The worklet's id of `sample`, sending it the first time.
    fn sample_id(&self, sample: &Arc<Sample>) -> u32 {
        let key = Arc::as_ptr(sample) as usize;
        let mut s = self.sent.borrow_mut();
        if let Some((id, _)) = s.ids.get(&key) {
            return *id;
        }
        let id = s.next;
        s.next += 1;
        s.ids.insert(key, (id, sample.clone()));
        drop(s);
        let data = js_sys::Int16Array::new_with_length(sample.data.len() as u32);
        data.copy_from(&sample.data);
        self.send(msg(&[
            "s".into(),
            id.into(),
            (sample.channels as u32).into(),
            (sample.frames as f64).into(),
            data.into(),
        ]));
        id
    }

    pub fn play(&self, sample: &Arc<Sample>, o: &PlayOptions) {
        let id = self.sample_id(sample);
        let end = o.end.unwrap_or(sample.frames).min(sample.frames);
        let at = o.start_frame.map_or(-1.0, |f| f as f64);
        self.send(msg(&[
            "p".into(),
            id.into(),
            (o.offset.min(end) as f64).into(),
            (end as f64).into(),
            at.into(),
            o.volume.into(),
            (o.key as f64).into(),
            (o.bus as u32).into(),
            o.looping.into(),
        ]));
    }

    pub fn set_volume(&self, key: u64, vol: f32) {
        self.send(msg(&["v".into(), (key as f64).into(), vol.into()]));
    }
    pub fn stop(&self, key: u64) {
        self.send(msg(&["x".into(), (key as f64).into()]));
    }
    pub fn stop_bus(&self, bus: Bus) {
        self.send(msg(&["xb".into(), (bus as u32).into()]));
    }
    pub fn set_bus_volume(&self, bus: Bus, v: f32) {
        self.send(msg(&["b".into(), (bus as u32).into(), v.into()]));
    }
    pub fn set_master(&self, v: f32) {
        self.send(msg(&["m".into(), v.into()]));
    }

    /// The frame the worklet is mixing now (estimated from its last report).
    pub fn current_frame(&self) -> u64 {
        match self.clock.get() {
            Some(c) => {
                (c.frame + (self.ctx.current_time() - c.at).max(0.0) * self.rate as f64) as u64
            }
            None => 0,
        }
    }

    /// The frame being heard now (None until the worklet runs): the frame
    /// of the context time now coming out of the speakers.
    pub fn heard_frame(&self) -> Option<f64> {
        let c = self.clock.get()?;
        let ctx: &JsValue = self.ctx.as_ref();
        // the context time now heard: getOutputTimestamp, else currentTime
        // less the output latency
        let now = js_sys::Reflect::get(ctx, &JsValue::from_str("getOutputTimestamp"))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
            .and_then(|f| f.call0(ctx).ok())
            .and_then(|ts| {
                let ctx_t = js_f64(&ts, "contextTime")?;
                let perf_t = js_f64(&ts, "performanceTime")?;
                let perf_now = web_sys::window()?.performance()?.now();
                (ctx_t > 0.0).then(|| ctx_t + (perf_now - perf_t) / 1000.0)
            })
            .unwrap_or_else(|| {
                let lat = js_f64(ctx, "outputLatency")
                    .filter(|l| *l > 0.0)
                    .or_else(|| js_f64(ctx, "baseLatency"))
                    .unwrap_or(0.0);
                self.ctx.current_time() - lat
            });
        Some(c.frame + (now - c.at) * self.rate as f64)
    }

    pub fn resume(&self) {
        let _ = self.ctx.resume();
    }
}

/// Post `m` to the worklet (its sample data's buffer handed over), or keep
/// it until the worklet is up.
fn send(
    node: &Rc<RefCell<Option<web_sys::AudioWorkletNode>>>,
    early: &Rc<RefCell<Vec<js_sys::Array>>>,
    m: js_sys::Array,
) {
    match node.borrow().as_ref().and_then(|n| n.port().ok()) {
        Some(port) => {
            let _ = post(&port, &m);
        }
        None => early.borrow_mut().push(m),
    }
}

fn post(port: &web_sys::MessagePort, m: &js_sys::Array) -> Result<(), JsValue> {
    // a sample's data: its buffer moves, not copied again
    let data = m.get(4);
    if m.get(0).as_string().as_deref() == Some("s") {
        if let Some(a) = data.dyn_ref::<js_sys::Int16Array>() {
            return port.post_message_with_transferable(m, &js_sys::Array::of1(&a.buffer()));
        }
    }
    port.post_message(m)
}
