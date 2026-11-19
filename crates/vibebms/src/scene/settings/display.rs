//! DISPLAY & AUDIO: the screen, the volumes and sounds, and the devices.

use super::row::onoff;
use super::row::{percent, Row, Rows};
use super::{skins, Sub};
use crate::app::Ctx;
use crate::config::PresentMode;
use crate::scene::cycle;
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
    b.header(tr!("SCREEN"));
    b.extend([
        Row::adjust(
            "display.fullscreen",
            tr!("Full screen"),
            onoff(d.fullscreen),
            |c, _| {
                c.config.display.fullscreen ^= true;
                c.view.want_fullscreen = Some(c.config.display.fullscreen);
            },
        )
        .desc(tr!("Window or full screen (F11)."))
        .keys("fullscreen window"),
        Row::adjust(
            "display.present_mode",
            tr!("Present mode"),
            d.present_mode.label(),
            |c, d| {
                use PresentMode::*;
                let m = &mut c.config.display.present_mode;
                *m = cycle(&[VSync, LowLatency, Immediate], *m, d);
                c.view.want_present_mode = true;
            },
        )
        .desc(tr!(
            "SMOOTH (VSYNC) has the most lag; LOW LATENCY less; UNLIMITED can tear."
        ))
        .keys("vsync latency tearing smooth unlimited"),
        Row::adjust(
            "display.max_fps",
            tr!("Max FPS"),
            match d.max_fps {
                0 => tr!("UNLIMITED").to_string(),
                n => n.to_string(),
            },
            |c, d| {
                let steps = [0u32, 60, 75, 120, 144, 165, 240, 360, 480, 1000];
                c.config.display.max_fps = cycle(&steps, c.config.display.max_fps, d)
            },
        )
        .desc(tr!("The frame rate cap; 0 = unlimited."))
        .keys("frame rate limit"),
        Row::adjust(
            "display.ui_scale",
            tr!("UI scale"),
            percent(d.ui_scale),
            |c, d| {
                let s = &mut c.config.display.ui_scale;
                *s = (*s + d as f32 * 0.05).clamp(0.5, 3.0)
            },
        )
        .desc(tr!("The size of menus and text outside skins."))
        .keys("zoom size"),
        Row::adjust(
            "display.show_fps",
            tr!("Show FPS"),
            onoff(d.show_fps),
            |c, _| c.config.display.show_fps ^= true,
        )
        .desc(tr!("Frame rate in the corner.")),
        Row::adjust(
            "display.show_latency",
            tr!("Latency overlay"),
            onoff(d.show_latency),
            |c, _| c.config.display.show_latency ^= true,
        )
        .desc(tr!(
            "Frame time, audio callback interval and input delay (median, 95th percentile, \
             maximum) in the top left."
        ))
        .keys("diagnostics debug timing"),
    ]);
    b.header(tr!("SOUND"));
    let sound_set = ctx.config.skins.get("SOUND_SET").map_or("", String::as_str);
    b.extend([
        Row::adjust(
            "audio.master",
            tr!("Master volume"),
            percent(a.master),
            |c, d| {
                let v = volume(&mut c.config.audio.master, d);
                c.audio.set_master(v);
            },
        )
        .desc(tr!("Everything vibeBMS plays.")),
        Row::adjust("audio.key", tr!("Keysound volume"), percent(a.key), |c, d| {
            let v = volume(&mut c.config.audio.key, d);
            c.audio.mixer.set_bus_volume(Bus::Key, v);
        })
        .desc(tr!("The sounds of the notes you hit.")),
        Row::adjust("audio.bgm", tr!("BGM volume"), percent(a.bgm), |c, d| {
            let v = volume(&mut c.config.audio.bgm, d);
            c.audio.mixer.set_bus_volume(Bus::Bgm, v);
        })
        .desc(tr!("The chart's background sounds.")),
        Row::adjust(
            "audio.system",
            tr!("System volume"),
            percent(a.system),
            |c, d| {
                let v = volume(&mut c.config.audio.system, d);
                c.audio.mixer.set_bus_volume(Bus::System, v);
            },
        )
        .desc(tr!("Menu sounds and the song select music.")),
        Row::adjust(
            "audio.preview",
            tr!("Song preview"),
            match (a.preview, a.preview_once) {
                (false, _) => tr!("OFF"),
                (true, true) => tr!("ONCE"),
                (true, false) => tr!("LOOP"),
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
        .desc(tr!("Play the chart's preview in the song select: LOOP, ONCE or OFF.")),
        Row::adjust(
            "audio.mute_inactive",
            tr!("Mute when inactive"),
            onoff(a.mute_inactive),
            |c, _| c.config.audio.mute_inactive ^= true,
        )
        .desc(
            tr!("Silent while another app or window has the focus or vibeBMS is hidden; \
             in play only while it is hidden (on a phone or tablet also without the focus)."),
        )
        .keys("background focus minimised silence"),
        Row::adjust(
            "audio.normalize",
            tr!("Normalize loudness"),
            onoff(a.normalize),
            |c, _| c.config.audio.normalize ^= true,
        )
        .desc(tr!("Charts and song previews at the same loudness (a chart is measured when first played)."))
        .keys("loudness lufs volume normalise replaygain"),
        Row::adjust(
            "audio.normalize_target",
            tr!("Loudness target"),
            format!("{:.0} LUFS", a.normalize_target),
            |c, d| {
                let (lo, hi) = crate::loudness::TARGET_RANGE;
                let t = &mut c.config.audio.normalize_target;
                *t = (t.round() + d as f32).clamp(lo, hi);
                let norm = crate::loudness::system_target(&c.config.audio);
                c.audio.sounds.renormalize(&c.audio.mixer, norm);
            },
        )
        .desc(tr!("The loudness normalized sounds play at; lower is quieter."))
        .keys("loudness lufs volume normalise"),
        Row::adjust(
            "audio.normalize_system",
            tr!("Normalize sound set music"),
            onoff(a.normalize_system),
            |c, _| {
                c.config.audio.normalize_system ^= true;
                let norm = crate::loudness::system_target(&c.config.audio);
                c.audio.sounds.renormalize(&c.audio.mixer, norm);
            },
        )
        .desc(tr!("The sound set's music (select, decide, .loop files) at the loudness target too."))
        .keys("loudness lufs volume normalise bgm"),
        Row::adjust(
            "audio.sound_set",
            tr!("Sound set"),
            skinned::choice_name(ctx, SkinType::SoundSet, sound_set).0,
            |c, d| {
                skins::cycle_skin(c, SkinType::SoundSet, d);
                c.play_sound(crate::sysound::Sound::OptionChange);
            },
        )
        .desc(tr!("The menu and system sounds. Enter: its options."))
        .opens(Sub::Skin(SkinType::SoundSet)),
    ]);
    b.group(
        "display.advanced",
        tr!("ADVANCED"),
        tr!("Audio buffer size, output and GPU"),
    );
    b.push(
        Row::adjust(
            "audio.buffer_frames",
            tr!("Audio buffer size"),
            match a.buffer_frames {
                0 => tr!("DEFAULT").to_string(),
                n => tr!("{n} frames", n = n),
            },
            |c, d| {
                let all = [0, 64, 128, 256, 512, 1024, 2048];
                c.config.audio.buffer_frames = cycle(&all, c.config.audio.buffer_frames, d)
            },
        )
        .desc(tr!(
            "Smaller is less lag, but may crackle. Applies after a restart."
        ))
        .keys("latency"),
    );
    #[cfg(windows)]
    b.push(
        Row::adjust(
            "audio.exclusive",
            tr!("Output mode"),
            if a.exclusive {
                tr!("EXCLUSIVE")
            } else {
                tr!("SHARED")
            },
            |c, _| c.config.audio.exclusive ^= true,
        )
        .desc(tr!(
            "EXCLUSIVE (WASAPI) has the least lag; others cannot play. Applies after a restart."
        ))
        .keys("wasapi exclusive shared latency"),
    );
    #[cfg(not(target_arch = "wasm32"))]
    b.push(
        Row::adjust(
            "display.backend",
            tr!("Graphics API"),
            d.backend.label(),
            |c, d| {
                let mut all = vec![crate::config::GraphicsBackend::Auto];
                all.extend(crate::gfx::available_backends());
                let b = &mut c.config.display.backend;
                *b = cycle(&all, *b, d);
            },
        )
        .desc(tr!(
            "Applies at the next launch. The graphics API the GPU is driven with (AUTO: vibeBMS \
             chooses); one that does not start is replaced by the next that does."
        ))
        .keys("renderer backend vulkan metal directx dx12 opengl gpu"),
    );
    b.extend([
        Row::note(
            "info.output",
            tr!("Output"),
            format!(
                "{} {} Hz, {:.1} ms",
                mix.device_name,
                mix.sample_rate,
                mix.latency_us() as f64 / 1000.0
            ),
        )
        .desc(tr!("The sound device, its rate and latency."))
        .keys("audio device latency"),
        Row::note("info.gpu", "GPU", ctx.view.gpu_name.clone())
            .desc(tr!("The graphics device drawing vibeBMS."))
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
        tr!("master {v}", v = percent(ctx.config.audio.master)),
    ]
}
