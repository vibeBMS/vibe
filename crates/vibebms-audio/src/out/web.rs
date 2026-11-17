//! The web's output: the mixer runs in an AudioWorklet, on the browser's
//! audio thread, as a second instance of this wasm module (`worklet`). The
//! page sends it the samples (once each, when first played) and the
//! commands; a key's sound starts in the next render quantum (128 frames),
//! not behind a queue the page must keep full, and nothing the page does
//! late (drawing, input) breaks the sound.
//!
//! The worklet's frame count is the mixer's clock; it reports it with the
//! context time, so the page knows the frame heard now (the context's
//! output timestamp, which follows the device's clock) and, from there,
//! the frame a moment on the judge's clock comes on (`Clock`). The reports
//! themselves reach the page too late and too unevenly (it draws between
//! them) to time sounds by.

use super::worklet::{self, Wire};
use super::{us_since, Device, Output};
use crate::decode::Sample;
use crate::voice::{Bus, Cmd, Due};
use crate::AudioConfig;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_time::Instant;

/// The processor: the worklet's instance of the app's module, which it is
/// given (`processorOptions.module`) with the page's imports stubbed (the
/// mixer calls none). Messages: a `Wire` command as a Float64Array, or
/// [id, channels, frames, Int16Array] a sample. Every 8 quanta it reports
/// [frame, currentTime] (the frame its quantum starts with, and when).
const PROCESSOR: &str = r#"
function stubs(module) {
  const imports = {};
  for (const i of WebAssembly.Module.imports(module)) {
    const m = (imports[i.module] ||= {});
    if (i.kind === "function") m[i.name] = () => { throw new Error("vibe-mix calls " + i.name); };
    else if (i.kind === "tag") m[i.name] = new WebAssembly.Tag({ parameters: ["externref"] });
  }
  return imports;
}
class VibeMix extends AudioWorkletProcessor {
  constructor(o) {
    super();
    const { module, maxVoices } = o.processorOptions;
    this.w = new WebAssembly.Instance(module, stubs(module)).exports;
    this.w.vibe_mix_new(maxVoices, sampleRate);
    this.n = 0;
    this.port.onmessage = (e) => this.cmd(e.data);
  }
  cmd(m) {
    const w = this.w;
    if (m instanceof Float64Array) return w.vibe_mix_cmd(...m);
    const [id, channels, frames, data] = m;
    new Int16Array(w.memory.buffer, w.vibe_mix_upload(data.length) >>> 0, data.length).set(data);
    w.vibe_mix_sample(id, channels, frames);
  }
  process(inputs, outputs) {
    const [L, R] = outputs[0], n = L.length, w = this.w;
    if ((this.n++ & 7) === 0) this.port.postMessage([w.vibe_mix_frame(), currentTime]);
    const p = w.vibe_mix_render(n) >>> 0;
    if (!this.l || this.p !== p || this.l.length !== n || this.l.buffer !== w.memory.buffer) {
      this.p = p;
      this.l = new Float32Array(w.memory.buffer, p, n);
      this.r = new Float32Array(w.memory.buffer, p + 4 * n, n);
    }
    L.set(this.l);
    R.set(this.r);
    return true;
  }
}
registerProcessor("vibe-mix", VibeMix);
"#;

/// The worklet's frames against the page's clocks.
struct Clock {
    ctx: web_sys::AudioContext,
    rate: f64,
    epoch: Instant,
    /// The worklet's last report: frame, at this context time (s).
    last: Option<(f64, f64)>,
    /// The frame heard at the epoch (the output timestamp's mapping, which
    /// follows the device), averaged over the reports.
    heard0: Mean,
    /// Frames from the one heard to the one mixed (where a sound played now
    /// starts), averaged: the page sees the mixed one only to a device
    /// buffer.
    ahead: Mean,
}

/// A mean of the last `n` values or so (an exponential one once full).
struct Mean {
    value: f64,
    count: f64,
    n: f64,
}

impl Mean {
    fn new(n: f64) -> Mean {
        Mean {
            value: 0.0,
            count: 0.0,
            n,
        }
    }

    fn add(&mut self, x: f64) {
        self.count = (self.count + 1.0).min(self.n);
        self.value += (x - self.value) / self.count;
    }
}

impl Clock {
    fn report(&mut self, frame: f64, at: f64) {
        self.last = Some((frame, at));
        let heard = self.frame_of(self.heard_time());
        let heard0 = heard - us_since(self.epoch, Instant::now()) * self.rate / 1e6;
        // the worklet skipped frames (it starts late), or the device's
        // clock was reset: a new mapping
        if (heard0 - self.heard0.value).abs() > self.rate / 100.0 {
            self.heard0.count = 0.0;
        }
        self.heard0.add(heard0);
        self.ahead
            .add(self.frame_of(self.ctx.current_time()) - heard);
    }

    /// The worklet's frame of context time `t` (after a report).
    fn frame_of(&self, t: f64) -> f64 {
        let (frame, at) = self.last.unwrap_or_default();
        frame + (t - at) * self.rate
    }

    /// The context time now heard: getOutputTimestamp, else currentTime
    /// less the output latency.
    fn heard_time(&self) -> f64 {
        let ctx: &JsValue = self.ctx.as_ref();
        js_sys::Reflect::get(ctx, &JsValue::from_str("getOutputTimestamp"))
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
            })
    }

    /// The frame a sound due `us` after the epoch starts on (None until
    /// the worklet reports).
    fn frame_at(&self, us: f64) -> Option<f64> {
        self.last?;
        Some(self.heard0.value + self.ahead.value + us * self.rate / 1e6)
    }
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
    ctx: web_sys::AudioContext,
    node: Rc<RefCell<Option<web_sys::AudioWorkletNode>>>,
    clock: Rc<RefCell<Clock>>,
    sent: Rc<RefCell<Sent>>,
    /// Messages before the node is up (the module loads first).
    early: Rc<RefCell<Vec<JsValue>>>,
    _ready: Closure<dyn FnMut(JsValue)>,
    _message: OnMessage,
    _sweep: Closure<dyn FnMut()>,
}

fn js_f64(o: &JsValue, k: &str) -> Option<f64> {
    js_sys::Reflect::get(o, &JsValue::from_str(k))
        .ok()?
        .as_f64()
}

impl WebOut {
    pub fn open(cfg: &AudioConfig, epoch: Instant) -> Result<(WebOut, Device), String> {
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
        let clock = Rc::new(RefCell::new(Clock {
            ctx: ctx.clone(),
            rate: rate as f64,
            epoch,
            last: None,
            heard0: Mean::new(64.0),
            ahead: Mean::new(512.0),
        }));
        let early: Rc<RefCell<Vec<JsValue>>> = Rc::default();
        let message: OnMessage = Rc::default();
        let max_voices = cfg.max_voices.max(16);
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
                let _ = js_sys::Reflect::set(&po, &"module".into(), &wasm_bindgen::module());
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
                            clock.borrow_mut().report(frame, at);
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
                        send(&node, &early, wire(&worklet::free(id)));
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
        let out = WebOut {
            ctx,
            node,
            clock,
            sent,
            early,
            _ready: ready,
            _message: message,
            _sweep: sweep,
        };
        for (bus, v) in [
            (Bus::Key, cfg.key_volume),
            (Bus::Bgm, cfg.bgm_volume),
            (Bus::System, cfg.system_volume),
        ] {
            out.send(Cmd::BusVolume(bus, v));
        }
        out.send(Cmd::Master(cfg.master));
        log::info!("audio: web worklet {rate} Hz");
        let dev = Device {
            rate,
            buffer: 128,
            name: "web".into(),
        };
        Ok((out, dev))
    }

    fn post(&self, m: JsValue) {
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
        let m: js_sys::Array = [
            id.into(),
            (sample.channels as u32).into(),
            (sample.frames as f64).into(),
            JsValue::from(data),
        ]
        .iter()
        .collect();
        self.post(m.into());
        id
    }
}

impl Output for WebOut {
    fn send(&self, mut cmd: Cmd) {
        let mut id = 0;
        if let Cmd::Play(v) = &mut cmd {
            id = self.sample_id(&v.sample);
            if let Some(Due::Us(us)) = v.due {
                // the worklet counts frames only; until it reports, they
                // are not known: now
                let frame = self.clock.borrow().frame_at(us);
                v.due = frame.map(|f| Due::Frame(f.round().max(0.0) as u64));
            }
        }
        self.post(wire(&worklet::encode(&cmd, id)));
    }

    fn latency_us(&self) -> Option<i64> {
        let c = self.clock.borrow();
        c.last?;
        Some((c.ahead.value.max(0.0) * 1e6 / c.rate) as i64)
    }

    fn resume(&self) {
        let _ = self.ctx.resume();
    }
}

fn wire(w: &Wire) -> JsValue {
    js_sys::Float64Array::from(&w[..]).into()
}

/// Post `m` to the worklet, or keep it until the worklet is up.
fn send(
    node: &Rc<RefCell<Option<web_sys::AudioWorkletNode>>>,
    early: &Rc<RefCell<Vec<JsValue>>>,
    m: JsValue,
) {
    match node.borrow().as_ref().and_then(|n| n.port().ok()) {
        Some(port) => {
            let _ = post(&port, &m);
        }
        None => early.borrow_mut().push(m),
    }
}

fn post(port: &web_sys::MessagePort, m: &JsValue) -> Result<(), JsValue> {
    // a sample's data: its buffer moves, not copied again
    if let Some(a) = m.dyn_ref::<js_sys::Array>() {
        if let Some(d) = a.get(3).dyn_ref::<js_sys::Int16Array>() {
            return port.post_message_with_transferable(m, &js_sys::Array::of1(&d.buffer()));
        }
    }
    port.post_message(m)
}
