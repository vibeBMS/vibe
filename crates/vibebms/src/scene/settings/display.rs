//! DISPLAY & AUDIO: the screen, the volumes and sounds, and the devices.

use super::row::{percent, Row, Rows};
use super::{skins, Sub};
use crate::app::Ctx;
use crate::config::PresentMode;
use crate::scene::option_panel::{cycle, onoff};
use crate::scene::skinned;
use crate::skin::SkinType;
use vibebms_audio::Bus;

/// A volume in 5 % steps.
fn volume(v: &mut f32, d: i32) -> f32 {
    *v = (*v + d as f32 * 0.05).clamp(0.0, 1.0);
    *v
}

pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    let d = &ctx.config.display;
    let a = &ctx.config.audio;
    let mix = &ctx.audio.mixer;
    b.header("SCREEN");
    b.extend([
        Row::adjust(
            "display.fullscreen",
            "Full screen",
            onoff(d.fullscreen),
            |c, _| {
                c.config.display.fullscreen ^= true;
                c.view.want_fullscreen = Some(c.config.display.fullscreen);
            },
        )
        .desc("Window or full screen (F11).")
        .keys("fullscreen window"),
        Row::adjust(
            "display.present_mode",
            "Present mode",
            d.present_mode.label(),
            |c, d| {
                use PresentMode::*;
                let m = &mut c.config.display.present_mode;
                *m = cycle(&[VSync, LowLatency, Immediate], *m, d);
                c.view.want_present_mode = true;
            },
        )
        .desc("SMOOTH (VSYNC) has the most lag; LOW LATENCY less; UNLIMITED can tear.")
        .keys("vsync latency tearing smooth unlimited"),
        Row::adjust(
            "display.max_fps",
            "Max FPS",
            match d.max_fps {
                0 => "UNLIMITED".to_string(),
                n => n.to_string(),
            },
            |c, d| {
                let steps = [0u32, 60, 75, 120, 144, 165, 240, 360, 480, 1000];
                c.config.display.max_fps = cycle(&steps, c.config.display.max_fps, d)
            },
        )
        .desc("The frame rate cap; 0 = unlimited.")
        .keys("frame rate limit"),
        Row::adjust(
            "display.ui_scale",
            "UI scale",
            percent(d.ui_scale),
            |c, d| {
                let s = &mut c.config.display.ui_scale;
                *s = (*s + d as f32 * 0.05).clamp(0.5, 3.0)
            },
        )
        .desc("The size of menus and text outside skins.")
        .keys("zoom size"),
        Row::adjust("display.show_fps", "Show FPS", onoff(d.show_fps), |c, _| {
            c.config.display.show_fps ^= true
        })
        .desc("Frame rate in the corner."),
    ]);
    b.header("SOUND");
    let sound_set = ctx.config.skins.get("SOUND_SET").map_or("", String::as_str);
    b.extend([
        Row::adjust(
            "audio.master",
            "Master volume",
            percent(a.master),
            |c, d| {
                let v = volume(&mut c.config.audio.master, d);
                c.audio.mixer.set_master(v);
            },
        )
        .desc("Everything vibeBMS plays."),
        Row::adjust("audio.key", "Keysound volume", percent(a.key), |c, d| {
            let v = volume(&mut c.config.audio.key, d);
            c.audio.mixer.set_bus_volume(Bus::Key, v);
        })
        .desc("The sounds of the notes you hit."),
        Row::adjust("audio.bgm", "BGM volume", percent(a.bgm), |c, d| {
            let v = volume(&mut c.config.audio.bgm, d);
            c.audio.mixer.set_bus_volume(Bus::Bgm, v);
        })
        .desc("The chart's background sounds."),
        Row::adjust(
            "audio.system",
            "System volume",
            percent(a.system),
            |c, d| {
                let v = volume(&mut c.config.audio.system, d);
                c.audio.mixer.set_bus_volume(Bus::System, v);
            },
        )
        .desc("Menu sounds and the song select music."),
        Row::adjust(
            "audio.preview",
            "Song preview",
            match (a.preview, a.preview_once) {
                (false, _) => "OFF",
                (true, true) => "ONCE",
                (true, false) => "LOOP",
            },
            |c, d| {
                // ONCE, LOOP, OFF
                let a = &mut c.config.audio;
                let i = match (a.preview, a.preview_once) {
                    (true, true) => 0,
                    (true, false) => 1,
                    (false, _) => 2,
                };
                let i = (i + d).rem_euclid(3);
                (a.preview, a.preview_once) = (i < 2, i == 0);
                c.audio.mixer.stop(crate::scene::select::PREVIEW_KEY);
            },
        )
        .desc("Play the chart's preview in the song select: LOOP, ONCE or OFF."),
        Row::adjust(
            "audio.normalize",
            "Normalize loudness",
            onoff(a.normalize),
            |c, _| c.config.audio.normalize ^= true,
        )
        .desc("Charts and song previews at the same loudness (a chart is measured when first played).")
        .keys("loudness lufs volume normalise replaygain"),
        Row::adjust(
            "audio.normalize_target",
            "Loudness target",
            format!("{:.0} LUFS", a.normalize_target),
            |c, d| {
                let (lo, hi) = crate::loudness::TARGET_RANGE;
                let t = &mut c.config.audio.normalize_target;
                *t = (t.round() + d as f32).clamp(lo, hi);
                let norm = crate::loudness::system_target(&c.config.audio);
                c.audio.sounds.renormalize(&c.audio.mixer, norm);
            },
        )
        .desc("The loudness normalized sounds play at; lower is quieter.")
        .keys("loudness lufs volume normalise"),
        Row::adjust(
            "audio.normalize_system",
            "Normalize sound set music",
            onoff(a.normalize_system),
            |c, _| {
                c.config.audio.normalize_system ^= true;
                let norm = crate::loudness::system_target(&c.config.audio);
                c.audio.sounds.renormalize(&c.audio.mixer, norm);
            },
        )
        .desc("The sound set's music (select, decide, .loop files) at the loudness target too.")
        .keys("loudness lufs volume normalise bgm"),
        Row::adjust(
            "audio.sound_set",
            "Sound set",
            skinned::choice_name(ctx, SkinType::SoundSet, sound_set).0,
            |c, d| {
                skins::cycle_skin(c, SkinType::SoundSet, d);
                c.play_sound(crate::sysound::Sound::OptionChange);
            },
        )
        .desc("The menu and system sounds. Enter: its options.")
        .opens(Sub::Skin(SkinType::SoundSet)),
    ]);
    b.group(
        "display.advanced",
        "ADVANCED",
        "Audio buffer size, output and GPU",
    );
    b.push(
        Row::adjust(
            "audio.buffer_frames",
            "Audio buffer size",
            match a.buffer_frames {
                0 => "DEFAULT".to_string(),
                n => format!("{n} frames"),
            },
            |c, d| {
                let all = [0, 64, 128, 256, 512, 1024, 2048];
                c.config.audio.buffer_frames = cycle(&all, c.config.audio.buffer_frames, d)
            },
        )
        .desc("Smaller is less lag, but may crackle. Applies after a restart.")
        .keys("latency"),
    );
    #[cfg(windows)]
    b.push(
        Row::adjust(
            "audio.exclusive",
            "Output mode",
            if a.exclusive { "EXCLUSIVE" } else { "SHARED" },
            |c, _| c.config.audio.exclusive ^= true,
        )
        .desc("EXCLUSIVE (WASAPI) has the least lag; others cannot play. Applies after a restart.")
        .keys("wasapi exclusive shared latency"),
    );
    b.extend([
        Row::note(
            "info.output",
            "Output",
            format!(
                "{} {} Hz, {:.1} ms",
                mix.device_name,
                mix.sample_rate,
                mix.latency_us() as f64 / 1000.0
            ),
        )
        .desc("The sound device, its rate and latency.")
        .keys("audio device latency"),
        Row::note("info.gpu", "GPU", ctx.view.gpu_name.clone())
            .desc("The graphics device drawing vibeBMS.")
            .keys("graphics"),
    ]);
}

/// DISPLAY & AUDIO's chips on the top level.
pub(super) fn chips(ctx: &Ctx) -> Vec<String> {
    let d = &ctx.config.display;
    let fps = match d.max_fps {
        0 => String::new(),
        n => format!(" · {n} FPS"),
    };
    vec![
        format!("{}{fps}", d.present_mode.label()),
        format!("master {}", percent(ctx.config.audio.master)),
    ]
}
