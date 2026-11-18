//! The app's Java side (platform/android) through JNI.

use jni::objects::{JClass, JObject, JValue};
use jni::JNIEnv;

/// Run `f` with the JVM and the activity (the app's NativeActivity).
pub fn with_java<R>(
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

/// One of the app's classes (`io.github.vibebms.<name>`), through the
/// activity's class loader: this thread is not a Java one, and the system
/// loader does not know app classes.
pub fn app_class<'a>(
    env: &mut JNIEnv<'a>,
    activity: &JObject,
    name: &str,
) -> jni::errors::Result<JClass<'a>> {
    let loader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string(format!("io.github.vibebms.{name}"))?;
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

/// Ask for the display's highest refresh rate (screens of 90 Hz and more
/// show apps at 60 unless asked); that rate (Hz).
pub fn use_highest_refresh_rate() -> Option<f64> {
    let r = with_java(|env, activity| {
        let class = app_class(env, activity, "RefreshRate")?;
        env.call_static_method(
            class,
            "useHighest",
            "(Landroid/app/Activity;)F",
            &[JValue::Object(activity)],
        )?
        .f()
    });
    r.map_err(|e| log::warn!("refresh rate: {e}"))
        .ok()
        .map(f64::from)
}

/// The game on the whole screen: the status and navigation bars hidden
/// (immersive; a swipe from an edge shows them for a moment). Again when
/// the app gets the focus back: the system may have shown them meanwhile.
pub fn hide_system_bars() {
    let r = with_java(|env, activity| {
        let class = app_class(env, activity, "SystemBars")?;
        env.call_static_method(
            class,
            "hide",
            "(Landroid/app/Activity;)V",
            &[JValue::Object(activity)],
        )?
        .v()
    });
    if let Err(e) = r {
        log::warn!("system bars: {e}");
    }
}
