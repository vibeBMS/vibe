//! The app's Java side (platform/android) through JNI.

use jni::objects::{JClass, JObject, JString, JValue};
use jni::{jni_sig, jni_str, Env};

/// Run `f` with the JVM and the activity (the app's NativeActivity).
pub fn with_java<R>(
    f: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<R>,
) -> Result<R, String> {
    let ctx = ndk_context::android_context();
    // SAFETY: android-activity sets the context to the process' JavaVM and
    // the activity (a global reference that lives as long as the process)
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    // (attaches this thread for good; the local references made in `f` go
    // with the frame pushed for it)
    vm.attach_current_thread(|env| -> jni::errors::Result<_> {
        // SAFETY: as above
        let activity = unsafe { JObject::from_raw(env, ctx.context().cast()) };
        let r = f(env, &activity);
        if env.exception_check() {
            env.exception_describe();
            env.exception_clear();
        }
        Ok(r)
    })
    .and_then(|r| r)
    .map_err(|e| e.to_string())
}

/// The text of `s`, a `java.lang.String`.
pub fn string(env: &Env, s: JObject) -> jni::errors::Result<String> {
    // SAFETY: the callers' `s` are what methods returning a `String` gave
    unsafe { JString::from_raw(env, s.into_raw()) }.try_to_string(env)
}

/// One of the app's classes (`io.github.vibebms.<name>`), through the
/// activity's class loader: this thread is not a Java one, and the system
/// loader does not know app classes.
pub fn app_class<'a>(
    env: &mut Env<'a>,
    activity: &JObject,
    name: &str,
) -> jni::errors::Result<JClass<'a>> {
    let loader = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let name = env.new_string(format!("io.github.vibebms.{name}"))?;
    let class = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    // SAFETY: `ClassLoader.loadClass` returns a `Class` (or throws)
    Ok(unsafe { JClass::from_raw(env, class.into_raw()) })
}

/// Ask for the display's highest refresh rate (screens of 90 Hz and more
/// show apps at 60 unless asked); that rate (Hz).
pub fn use_highest_refresh_rate() -> Option<f64> {
    let r = with_java(|env, activity| {
        let class = app_class(env, activity, "RefreshRate")?;
        env.call_static_method(
            class,
            jni_str!("useHighest"),
            jni_sig!("(Landroid/app/Activity;)F"),
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
            jni_str!("hide"),
            jni_sig!("(Landroid/app/Activity;)V"),
            &[JValue::Object(activity)],
        )?
        .v()
    });
    if let Err(e) = r {
        log::warn!("system bars: {e}");
    }
}

/// The app's locale as a BCP 47 tag (`Locale.getDefault()`: the system's,
/// or the one chosen for this app).
pub fn locale() -> Option<String> {
    with_java(|env, _| {
        let class = env.find_class(jni_str!("java/util/Locale"))?;
        let l = env
            .call_static_method(
                &class,
                jni_str!("getDefault"),
                jni_sig!("()Ljava/util/Locale;"),
                &[],
            )?
            .l()?;
        let tag = env
            .call_method(
                &l,
                jni_str!("toLanguageTag"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        string(env, tag)
    })
    .ok()
}
