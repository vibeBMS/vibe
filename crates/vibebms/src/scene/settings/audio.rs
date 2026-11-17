use super::{display, percent, Row};
use crate::app::Ctx;
use crate::scene::option_panel::{cycle, onoff};
use crate::scene::skinned;
use crate::skin::SkinType;
use vibebms_audio::Bus;

/// A volume in 5 % steps.
fn volume(v: &mut f32, d: i32) -> f32 {
    *v = (*v + d as f32 * 0.05).clamp(0.0, 1.0);
    *v
}

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let a = &ctx.config.audio;
    let mix = &ctx.audio.mixer;
    let sound_set = ctx.config.skins.get("SOUND_SET").map_or("", String::as_str);
    vec![
        Row::adjust("Master volume", percent(a.master), |c, d| {
            let v = volume(&mut c.config.audio.master, d);
            c.audio.mixer.set_master(v);
        }),
        Row::adjust("Keysound volume", percent(a.key), |c, d| {
            let v = volume(&mut c.config.audio.key, d);
            c.audio.mixer.set_bus_volume(Bus::Key, v);
        }),
        Row::adjust("BGM volume", percent(a.bgm), |c, d| {
            let v = volume(&mut c.config.audio.bgm, d);
            c.audio.mixer.set_bus_volume(Bus::Bgm, v);
        }),
        Row::adjust("System volume", percent(a.system), |c, d| {
            let v = volume(&mut c.config.audio.system, d);
            c.audio.mixer.set_bus_volume(Bus::System, v);
        }),
        Row::adjust(
            "Buffer size (restart)",
            format!("{} frames", a.buffer_frames),
            |c, d| {
                let all = [0, 64, 128, 256, 512, 1024, 2048];
                c.config.audio.buffer_frames = cycle(&all, c.config.audio.buffer_frames, d)
            },
        ),
        Row::adjust("Song preview", onoff(a.preview), |c, _| {
            c.config.audio.preview ^= true
        }),
        Row::adjust(
            "Sound set",
            skinned::choice_name(ctx, SkinType::SoundSet, sound_set).0,
            |c, d| {
                display::cycle_skin(c, SkinType::SoundSet, d);
                c.play_sound(crate::sysound::Sound::OptionChange);
            },
        ),
        Row::note(
            "Output",
            format!(
                "{} {} Hz, {:.1} ms",
                mix.device_name,
                mix.sample_rate,
                mix.latency_us() as f64 / 1000.0
            ),
        ),
    ]
}
