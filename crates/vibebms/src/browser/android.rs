//! The in-app browser on Android: `io.github.vibebms.Browser` (Java, in
//! platform/android) shows a web view over the game and saves what pages
//! download to `<root>/downloads/`, asking the player about each first (a
//! dialog of its own, over the web view); this side opens it through JNI,
//! polls its events and brings finished downloads into the library
//! (`import`).

use super::imports::Imports;
use super::MAX_DOWNLOAD;
use crate::app::{Ctx, Draw};
use crate::input::InputEv;
use crate::platform::android::{app_class, string, with_java};
use jni::objects::JValue;
use jni::{jni_sig, jni_str};
use std::collections::VecDeque;
use std::path::PathBuf;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

/// The pages still to show, whether the browser is up, the imports running.
#[derive(Default)]
pub struct Downloads {
    open: bool,
    next: VecDeque<String>,
    imports: Imports,
}

fn open(url: &str, dir: &std::path::Path) -> Result<(), String> {
    with_java(|env, activity| {
        let class = app_class(env, activity, "Browser")?;
        let url = env.new_string(url)?;
        let dir = env.new_string(dir.to_string_lossy())?;
        // run on each page (a OneDrive share downloads without a press)
        let script = env.new_string(bmseeker::hosts::PRESS_DOWNLOAD_JS)?;
        // the question about a download, and its button beside OK
        let ask = env.new_string(tr!("Save this download?"))?;
        let no = env.new_string(tr!("Don't save"))?;
        env.call_static_method(
            class,
            jni_str!("open"),
            jni_sig!(
                "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;JLjava/lang/String;Ljava/lang/String;)V"
            ),
            &[
                JValue::Object(activity),
                JValue::Object(&url),
                JValue::Object(&dir),
                JValue::Object(&script),
                JValue::Long(MAX_DOWNLOAD as i64),
                JValue::Object(&ask),
                JValue::Object(&no),
            ],
        )?;
        Ok(())
    })
}

/// The browser's events since the last call.
fn events() -> Vec<String> {
    with_java(|env, activity| {
        let class = app_class(env, activity, "Browser")?;
        let mut out = Vec::new();
        loop {
            let ev = env
                .call_static_method(
                    &class,
                    jni_str!("poll"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            if ev.is_null() {
                break;
            }
            out.push(string(env, ev)?);
        }
        Ok(out)
    })
    .unwrap_or_default()
}

impl Downloads {
    /// Show `pages` one after another: the first now, the next after a
    /// download from it has arrived.
    pub fn browse(
        &mut self,
        _el: &ActiveEventLoop,
        _game: &winit::window::Window,
        ctx: &mut Ctx,
        pages: Vec<String>,
    ) {
        let mut pages: VecDeque<String> = pages.into();
        let Some(first) = pages.pop_front() else {
            return;
        };
        self.next = pages;
        match open(&first, &ctx.paths.root.join("downloads")) {
            Ok(()) => self.open = true,
            Err(e) => {
                log::warn!("in-app browser: {e}");
                if !crate::platform::open_url(&first) {
                    ctx.toast(tr!("Could not open a browser"));
                }
            }
        }
    }

    /// The browser is no window of ours.
    pub fn window_event(&mut self, _id: WindowId, _event: &WindowEvent) -> bool {
        false
    }

    /// The browser is up (its dialog has the focus).
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The question about a download is the Java side's.
    pub fn input(&mut self, _ctx: &mut Ctx, _ev: &InputEv) -> bool {
        false
    }

    pub fn draw(&mut self, _ctx: &Ctx, _d: &mut Draw) {}

    /// Once per frame (while the browser is up or imports run).
    pub fn poll(&mut self, ctx: &mut Ctx) {
        if self.open {
            for ev in events() {
                let (kind, rest) = ev.split_at(ev.char_indices().nth(1).map_or(ev.len(), |c| c.0));
                match kind {
                    "S" => ctx.toast(tr!("Downloading {name}…", name = rest)),
                    "E" => ctx.toast(tr!("Download failed: {url}", url = rest)),
                    "B" => ctx.toast(tr!(
                        "One download at a time: {name} was not started",
                        name = rest
                    )),
                    "L" => ctx.toast(tr!(
                        "{name} is larger than {gb} GB: not kept",
                        name = rest,
                        gb = MAX_DOWNLOAD >> 30
                    )),
                    "C" => self.open = false,
                    "F" => self
                        .imports
                        .start(PathBuf::from(rest), ctx.paths.root.join("bms")),
                    _ => {}
                }
            }
        }
        let next = &mut self.next;
        self.imports.poll(ctx, |ctx| {
            if let Some(url) = next.pop_front() {
                if open(&url, &ctx.paths.root.join("downloads")).is_ok() {
                    ctx.toast(tr!("Now the chart: download it from this page"));
                }
            }
        });
    }
}
