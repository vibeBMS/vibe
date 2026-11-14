//! The in-app browser on Android: `io.github.vibebms.Browser` (Java, in
//! platform/android) shows a web view over the game and saves what pages
//! download to `<root>/downloads/`; this side opens it through JNI, polls
//! its events and brings finished downloads into the library (`import`).

use crate::app::Ctx;
use crate::import::{self, Imported};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::JNIEnv;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

type ImportResult = Result<Imported, String>;

/// The pages still to show, whether the browser is up, the imports running.
#[derive(Default)]
pub struct Downloads {
    open: bool,
    next: VecDeque<String>,
    imports: Option<(Sender<ImportResult>, Receiver<ImportResult>)>,
}

/// Run `f` with the JVM and the activity (the app's NativeActivity).
fn with_java<R>(
    f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>,
) -> Result<R, String> {
    let ctx = ndk_context::android_context();
    // SAFETY: android-activity sets the context to the process' JavaVM and
    // the activity (a global reference that lives as long as the process)
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm
        .attach_current_thread_permanently()
        .map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
    let r = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    r.map_err(|e| e.to_string())
}

/// The Browser class (through the activity's class loader: this thread is
/// not a Java one, and the system loader does not know app classes).
fn browser_class<'a>(env: &mut JNIEnv<'a>, activity: &JObject) -> jni::errors::Result<JClass<'a>> {
    let loader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string("io.github.vibebms.Browser")?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    Ok(JClass::from(class))
}

fn open(url: &str, dir: &std::path::Path) -> Result<(), String> {
    with_java(|env, activity| {
        let class = browser_class(env, activity)?;
        let url = env.new_string(url)?;
        let dir = env.new_string(dir.to_string_lossy())?;
        env.call_static_method(
            class,
            "open",
            "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)V",
            &[
                JValue::Object(activity),
                JValue::Object(&url),
                JValue::Object(&dir),
            ],
        )?;
        Ok(())
    })
}

/// The browser's events since the last call.
fn events() -> Vec<String> {
    with_java(|env, activity| {
        let class = browser_class(env, activity)?;
        let mut out = Vec::new();
        loop {
            let ev = env
                .call_static_method(&class, "poll", "()Ljava/lang/String;", &[])?
                .l()?;
            if ev.is_null() {
                break;
            }
            out.push(env.get_string(&JString::from(ev))?.into());
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
                    ctx.toast("Could not open a browser");
                }
            }
        }
    }

    /// The browser is no window of ours.
    pub fn window_event(&mut self, _id: WindowId, _event: &WindowEvent) -> bool {
        false
    }

    /// Once per frame (while the browser is up or imports run).
    pub fn poll(&mut self, ctx: &mut Ctx) {
        if self.open {
            for ev in events() {
                let (kind, rest) = ev.split_at(ev.char_indices().nth(1).map_or(ev.len(), |c| c.0));
                match kind {
                    "S" => ctx.toast(format!("Downloading {rest}…")),
                    "E" => ctx.toast(format!("Download failed: {rest}")),
                    "C" => self.open = false,
                    "F" => {
                        let (tx, _) = self.imports.get_or_insert_with(channel);
                        let (tx, file) = (tx.clone(), PathBuf::from(rest));
                        let library = ctx.paths.root.join("bms");
                        crate::platform::spawn("import", move || {
                            let _ = tx.send(import::import(&file, &library));
                        });
                    }
                    _ => {}
                }
            }
        }
        let done: Vec<_> = self
            .imports
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default();
        for r in done {
            let report = import::report(ctx, r);
            if report.song_arrived {
                if let Some(url) = self.next.pop_front() {
                    if open(&url, &ctx.paths.root.join("downloads")).is_ok() {
                        ctx.toast("Now the chart: download it from this page");
                    }
                }
            }
            if let Some((chart, _)) = report.ask {
                ctx.toast(format!(
                    "{}: several songs could be it; it stays in downloads",
                    import::file_name(&chart)
                ));
            }
            if report.rescan {
                ctx.ensure_scanned("bms");
                ctx.start_scan();
            }
        }
    }
}
