//! The web build's audio output: an AudioWorklet that plays the mixer's
//! frames from a queue the page keeps 40 ms ahead (more if it runs dry), and tells how many
//! it has played and when (the audio clock). cpal's web output schedules
//! short buffers from the page's thread; late ones overlap or leave gaps
//! (keysounds sound wrong), and nothing tells how far the sound heard is
//! behind the frames mixed, so notes and sound drift apart.

use crate::mixer::{soft_clip, State};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// The processor: interleaved stereo blocks in, played in order; silence
/// when the queue runs dry. Every 8 render quanta it reports, before
/// playing, [frames played so far, the context time of this quantum's first
/// frame, how many times the queue has run dry].
const PROCESSOR: &str = r#"
class VibeOut extends AudioWorkletProcessor {
  constructor() {
    super();
    this.q = [];
    this.off = 0;
    this.played = 0;
    this.n = 0;
    this.dry = 0;
    this.port.onmessage = (e) => this.q.push(e.data);
  }
  process(inputs, outputs) {
    const out = outputs[0];
    const L = out[0], R = out[1] || out[0];
    if ((this.n++ & 7) === 0) this.port.postMessage([this.played, currentTime, this.dry]);
    let i = 0;
    while (i < L.length && this.q.length) {
      const b = this.q[0];
      const k = Math.min((b.length >> 1) - this.off, L.length - i);
      for (let j = 0; j < k; j++) {
        L[i + j] = b[(this.off + j) * 2];
        R[i + j] = b[(this.off + j) * 2 + 1];
      }
      i += k;
      this.off += k;
      this.played += k;
      if (this.off >= b.length >> 1) { this.q.shift(); this.off = 0; }
    }
    if (i < L.length && this.played > 0) this.dry++;
    for (; i < L.length; i++) { L[i] = 0; R[i] = 0; }
    return true;
  }
}
registerProcessor("vibe-out", VibeOut);
"#;

/// The node's message handler, kept alive with it.
type OnMessage = Rc<RefCell<Option<Closure<dyn FnMut(web_sys::MessageEvent)>>>>;

/// How far ahead of what plays the page keeps the queue at first (a key's
/// sound is heard that much after it is mixed), more each time it runs dry
/// (the page's thread was late), up to `LEAD_MAX_S`.
const LEAD_S: f64 = 0.04;
const LEAD_STEP_S: f64 = 0.01;
const LEAD_MAX_S: f64 = 0.15;
/// Frames mixed at a time.
const CHUNK: usize = 256;

/// The audio clock: (mixer frames played, at this context time).
#[derive(Clone, Copy, Default)]
struct Played {
    frames: f64,
    at: f64,
    /// Times the queue ran dry.
    dry: f64,
}

pub(crate) struct WebOut {
    pub ctx: web_sys::AudioContext,
    pub rate: u32,
    played: Rc<Cell<Option<Played>>>,
    _tick: Closure<dyn FnMut()>,
    _ready: Closure<dyn FnMut(JsValue)>,
    _message: OnMessage,
}

fn js_f64(o: &JsValue, k: &str) -> Option<f64> {
    js_sys::Reflect::get(o, &JsValue::from_str(k))
        .ok()?
        .as_f64()
}

impl WebOut {
    pub fn open(state: State) -> Result<WebOut, (String, Box<State>)> {
        let ctx = match web_sys::AudioContext::new() {
            Ok(c) => c,
            Err(e) => return Err((format!("{e:?}"), Box::new(state))),
        };
        let rate = ctx.sample_rate() as u32;
        let worklet = match ctx.audio_worklet() {
            Ok(w) => w,
            Err(e) => return Err((format!("no AudioWorklet: {e:?}"), Box::new(state))),
        };
        let blob = {
            let parts = js_sys::Array::of1(&JsValue::from_str(PROCESSOR));
            let opts = web_sys::BlobPropertyBag::new();
            opts.set_type("text/javascript");
            web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts)
                .and_then(|b| web_sys::Url::create_object_url_with_blob(&b))
        };
        let url = match blob {
            Ok(u) => u,
            Err(e) => return Err((format!("{e:?}"), Box::new(state))),
        };
        let state = Rc::new(RefCell::new(state));
        let node: Rc<RefCell<Option<web_sys::AudioWorkletNode>>> = Rc::default();
        let played: Rc<Cell<Option<Played>>> = Rc::default();
        let message: OnMessage = Rc::default();
        // the module loaded: the node, its reports
        let ready = {
            let (ctx, node, played, message) =
                (ctx.clone(), node.clone(), played.clone(), message.clone());
            Closure::<dyn FnMut(JsValue)>::new(move |_| {
                let opts = web_sys::AudioWorkletNodeOptions::new();
                opts.set_number_of_inputs(0);
                opts.set_output_channel_count(&js_sys::Array::of1(&JsValue::from(2)));
                let n = match web_sys::AudioWorkletNode::new_with_options(&ctx, "vibe-out", &opts) {
                    Ok(n) => n,
                    Err(e) => return log::warn!("audio worklet: {e:?}"),
                };
                let _ = n.connect_with_audio_node(&ctx.destination());
                let played = played.clone();
                let on = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
                    move |e: web_sys::MessageEvent| {
                        let d = e.data();
                        let a: js_sys::Array = d.unchecked_into();
                        if let (Some(frames), Some(at)) = (a.get(0).as_f64(), a.get(1).as_f64()) {
                            let dry = a.get(2).as_f64().unwrap_or(0.0);
                            played.set(Some(Played { frames, at, dry }));
                        }
                    },
                );
                if let Ok(port) = n.port() {
                    port.set_onmessage(Some(on.as_ref().unchecked_ref()));
                }
                *message.borrow_mut() = Some(on);
                *node.borrow_mut() = Some(n);
                log::info!("audio: worklet running");
            })
        };
        match worklet.add_module(&url) {
            Ok(p) => {
                let _ = p.then(&ready);
            }
            Err(e) => {
                let st = Rc::try_unwrap(state)
                    .map_or_else(|_| crate::mixer::st_dummy(), |s| s.into_inner());
                return Err((format!("{e:?}"), Box::new(st)));
            }
        }
        // keep the queue LEAD_S ahead of what has played
        let tick = {
            let (ctx, node, played, state) = (ctx.clone(), node, played.clone(), state);
            let sent = Cell::new(0u64);
            let (lead, dry_seen) = (Cell::new(LEAD_S), Cell::new(0.0));
            // when the lead last changed (context time)
            let lead_at = Cell::new(0.0);
            let block = js_sys::Float32Array::new_with_length((CHUNK * 2) as u32);
            Closure::<dyn FnMut()>::new(move || {
                let Some(n) = node.borrow().clone() else {
                    return;
                };
                let Ok(port) = n.port() else { return };
                let rate = ctx.sample_rate() as f64;
                let playing = match played.get() {
                    Some(p) => {
                        let now = ctx.current_time();
                        if p.dry > dry_seen.get() {
                            dry_seen.set(p.dry);
                            lead.set((lead.get() + LEAD_STEP_S).min(LEAD_MAX_S));
                            lead_at.set(now);
                            log::info!("audio: ran dry, {:.0} ms ahead now", lead.get() * 1000.0);
                        } else if lead.get() > LEAD_S && now - lead_at.get() > 20.0 {
                            // (a while without: a tab in the background, gone)
                            lead.set((lead.get() - LEAD_STEP_S).max(LEAD_S));
                            lead_at.set(now);
                        }
                        p.frames + (ctx.current_time() - p.at).max(0.0) * rate
                    }
                    None => 0.0,
                };
                let target = playing + lead.get() * rate;
                let mut st = state.borrow_mut();
                let mut out = vec![0f32; CHUNK * 2];
                while (sent.get() as f64) < target {
                    st.render(CHUNK);
                    for (o, m) in out.iter_mut().zip(&st.mix[..CHUNK * 2]) {
                        *o = soft_clip(*m);
                    }
                    block.copy_from(&out);
                    // (a copy: the message takes its own)
                    let _ = port.post_message(&js_sys::Float32Array::new(&block));
                    sent.set(sent.get() + CHUNK as u64);
                }
            })
        };
        if let Some(w) = web_sys::window() {
            let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                5,
            );
        }
        Ok(WebOut {
            ctx,
            rate,
            played,
            _tick: tick,
            _ready: ready,
            _message: message,
        })
    }

    /// The mixer frame being heard now (None until the output runs): the
    /// frames played by the context time now coming out of the speakers.
    pub fn heard_frame(&self) -> Option<f64> {
        let p = self.played.get()?;
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
        Some(p.frames + (now - p.at) * self.rate as f64)
    }

    pub fn resume(&self) {
        let _ = self.ctx.resume();
    }
}
