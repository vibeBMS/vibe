//! BMSON (JSON chart format) decoder.

use crate::chart::*;
use crate::mode::PlayMode;
use crate::timing::{TimingEvent, TimingMap};
use crate::ChartError;
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Bmson {
    info: Info,
    lines: Option<Vec<BarLineDef>>,
    bpm_events: Vec<BpmEv>,
    stop_events: Vec<StopEv>,
    scroll_events: Vec<ScrollEv>,
    sound_channels: Vec<SoundChannel>,
    mine_channels: Vec<MineChannel>,
    key_channels: Vec<MineChannel>,
    bga: Bga,
}

#[derive(Deserialize)]
#[serde(default)]
struct Info {
    title: String,
    subtitle: String,
    artist: String,
    subartists: Vec<String>,
    genre: String,
    mode_hint: String,
    chart_name: String,
    level: i64,
    init_bpm: f64,
    judge_rank: f64,
    total: f64,
    back_image: String,
    eyecatch_image: String,
    banner_image: String,
    preview_music: String,
    resolution: f64,
    ln_type: i32,
}

impl Default for Info {
    fn default() -> Self {
        Info {
            title: String::new(),
            subtitle: String::new(),
            artist: String::new(),
            subartists: Vec::new(),
            genre: String::new(),
            mode_hint: "beat-7k".into(),
            chart_name: String::new(),
            level: 0,
            init_bpm: 130.0,
            judge_rank: 100.0,
            total: 100.0,
            back_image: String::new(),
            eyecatch_image: String::new(),
            banner_image: String::new(),
            preview_music: String::new(),
            resolution: 240.0,
            ln_type: 0,
        }
    }
}

#[derive(Deserialize)]
struct BarLineDef {
    y: f64,
}
#[derive(Deserialize)]
struct BpmEv {
    y: f64,
    bpm: f64,
}
#[derive(Deserialize)]
struct StopEv {
    y: f64,
    duration: f64,
}
#[derive(Deserialize)]
struct ScrollEv {
    y: f64,
    rate: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SoundChannel {
    name: String,
    notes: Vec<BNote>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BNote {
    x: serde_json::Value,
    y: f64,
    l: f64,
    c: bool,
    t: i32,
    up: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MineChannel {
    name: String,
    notes: Vec<MineNote>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MineNote {
    x: serde_json::Value,
    y: f64,
    damage: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Bga {
    bga_header: Vec<BgaHeader>,
    bga_events: Vec<BgaEv>,
    layer_events: Vec<BgaEv>,
    poor_events: Vec<BgaEv>,
}
#[derive(Deserialize)]
struct BgaHeader {
    id: i64,
    name: String,
}
#[derive(Deserialize)]
struct BgaEv {
    y: f64,
    id: i64,
}

fn mode_from_hint(h: &str) -> PlayMode {
    match h {
        "beat-5k" => PlayMode::Beat5K,
        "beat-10k" => PlayMode::Beat10K,
        "beat-14k" => PlayMode::Beat14K,
        "popn-5k" | "popn-9k" => PlayMode::Popn9K,
        _ => PlayMode::Beat7K,
    }
}

/// bmson lane `x` (1-based) to internal lane.
fn map_lane(mode: PlayMode, x: i64) -> Option<u8> {
    if x <= 0 {
        return None;
    }
    let x = x as usize - 1;
    let lane = match mode {
        PlayMode::Popn9K => (x < 9).then_some(x),
        PlayMode::Beat5K | PlayMode::Beat10K => {
            // beat-5k: 1-5 keys, 8 scratch (beatoraja convention); 10k: +8 for 2P
            let player = x / 8;
            let k = x % 8;
            let lane = match k {
                0..=4 => Some(k),
                7 => Some(5),
                _ => None,
            };
            lane.filter(|_| player < mode.players())
                .map(|l| l + player * 6)
        }
        PlayMode::Beat7K | PlayMode::Beat14K => (x < 8 * mode.players()).then_some(x),
    };
    lane.map(|l| l as u8)
}

fn x_of(v: &serde_json::Value) -> i64 {
    v.as_i64()
        .or_else(|| v.as_f64().map(|f| f as i64))
        .unwrap_or(0)
}

pub fn parse_bmson(bytes: &[u8]) -> Result<Chart, ChartError> {
    let b: Bmson = serde_json::from_slice(bytes).map_err(|e| ChartError::Parse(e.to_string()))?;
    let info = &b.info;
    let res = if info.resolution > 0.0 {
        info.resolution
    } else {
        240.0
    };
    let beat = |y: f64| y / res;
    let mode = mode_from_hint(&info.mode_hint);
    let default_ln = match info.ln_type {
        1 => Some(LnType::Ln),
        2 => Some(LnType::Cn),
        3 => Some(LnType::Hcn),
        _ => None,
    };

    let mut ev = Vec::new();
    for e in &b.bpm_events {
        ev.push((beat(e.y), TimingEvent::Bpm(e.bpm)));
    }
    for e in &b.stop_events {
        ev.push((beat(e.y), TimingEvent::StopBeats(e.duration / res)));
    }
    for e in &b.scroll_events {
        ev.push((beat(e.y), TimingEvent::Scroll(e.rate)));
    }
    let timing = TimingMap::build(info.init_bpm, ev);
    let t = |y: f64| timing.time_at_beat(beat(y));
    let p = |y: f64| timing.pos_at_beat(beat(y));

    let mut chart = Chart::default();
    let mut meta = ChartMeta {
        title: info.title.clone(),
        subtitle: if info.subtitle.is_empty() {
            info.chart_name.clone()
        } else {
            info.subtitle.clone()
        },
        artist: info.artist.clone(),
        subartist: info.subartists.join(" "),
        genre: info.genre.clone(),
        level: info.level.to_string(),
        mode: Some(mode),
        initial_bpm: info.init_bpm,
        judge_rank: JudgeRank::Bmson(info.judge_rank),
        // bmson total is a percentage of the default TOTAL; resolved in `Chart::total`
        total: None,
        ln_type: default_ln,
        stagefile: info.eyecatch_image.clone(),
        banner: info.banner_image.clone(),
        backbmp: info.back_image.clone(),
        preview: info.preview_music.clone(),
        md5: String::new(),
        sha256: crate::bms::sha256_hex(bytes),
        has_stop: !b.stop_events.is_empty(),
        ..Default::default()
    };

    for ch in &b.sound_channels {
        let file = chart.sound_files.len() as u32;
        chart.sound_files.push(ch.name.clone());
        let mut notes: Vec<&BNote> = ch.notes.iter().collect();
        notes.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
        // Each note plays the channel's file from the point the previous
        // restart (c == false) happened, until the next note in the channel.
        let mut restart_time = 0i64;
        for (i, n) in notes.iter().enumerate() {
            let time = t(n.y);
            if !n.c {
                restart_time = time;
            }
            let next_time = notes[i + 1..].iter().map(|m| t(m.y)).find(|nt| *nt > time);
            let start = time - restart_time;
            let end = next_time.map(|nt| nt - restart_time);
            let sound = chart.sounds.len() as SoundId;
            chart.sounds.push(SoundSlice { file, start, end });
            let x = x_of(&n.x);
            match map_lane(mode, x) {
                Some(lane) if !n.up => {
                    let kind = if n.l > 0.0 {
                        meta.has_long = true;
                        let ln_type = match n.t {
                            1 => Some(LnType::Ln),
                            2 => Some(LnType::Cn),
                            3 => Some(LnType::Hcn),
                            _ => default_ln,
                        };
                        NoteKind::Long {
                            end_time: t(n.y + n.l),
                            end_beat: beat(n.y + n.l),
                            end_pos: p(n.y + n.l),
                            end_sound: NO_SOUND,
                            ln_type,
                        }
                    } else {
                        NoteKind::Normal
                    };
                    chart.notes.push(Note {
                        lane,
                        time,
                        beat: beat(n.y),
                        pos: p(n.y),
                        sound,
                        kind,
                    });
                }
                Some(_) => {}
                None => chart.bgm.push(BgmEvent { time, sound }),
            }
        }
    }
    for (channels, mine) in [(&b.mine_channels, true), (&b.key_channels, false)] {
        for ch in channels {
            let file = chart.sound_files.len() as u32;
            chart.sound_files.push(ch.name.clone());
            let sound = chart.sounds.len() as SoundId;
            chart.sounds.push(SoundSlice {
                file,
                start: 0,
                end: None,
            });
            for n in &ch.notes {
                if let Some(lane) = map_lane(mode, x_of(&n.x)) {
                    let kind = if mine {
                        meta.has_mine = true;
                        NoteKind::Mine {
                            damage: n.damage as f32,
                        }
                    } else {
                        meta.has_invisible = true;
                        NoteKind::Invisible
                    };
                    chart.notes.push(Note {
                        lane,
                        time: t(n.y),
                        beat: beat(n.y),
                        pos: p(n.y),
                        sound,
                        kind,
                    });
                }
            }
        }
    }

    let mut id_map = std::collections::HashMap::new();
    for h in &b.bga.bga_header {
        id_map.insert(h.id, chart.images.len() as u32);
        chart.images.push(h.name.clone());
    }
    for (evs, layer) in [
        (&b.bga.bga_events, BgaLayer::Base),
        (&b.bga.layer_events, BgaLayer::Layer),
        (&b.bga.poor_events, BgaLayer::Poor),
    ] {
        for e in evs {
            chart.bga.push(BgaEvent {
                time: t(e.y),
                layer,
                image: id_map.get(&e.id).copied(),
            });
        }
    }
    chart.bga.sort_by_key(|e| e.time);
    meta.has_bga = !chart.bga.is_empty();

    chart
        .notes
        .sort_by(|a, b| a.time.cmp(&b.time).then(a.lane.cmp(&b.lane)));
    chart.bgm.sort_by_key(|e| e.time);

    let last = chart.notes.iter().map(|n| match n.kind {
        NoteKind::Long { end_beat, .. } => end_beat,
        _ => n.beat,
    });
    let last = last.fold(0.0f64, f64::max);
    chart.bar_lines = match &b.lines {
        Some(l) if !l.is_empty() => l
            .iter()
            .map(|l| BarLine {
                time: t(l.y),
                pos: p(l.y),
            })
            .collect(),
        _ => (0..=((last / 4.0).ceil() as usize + 1))
            .map(|m| BarLine {
                time: timing.time_at_beat(m as f64 * 4.0),
                pos: timing.pos_at_beat(m as f64 * 4.0),
            })
            .collect(),
    };
    chart.timing = timing;
    chart.meta = meta;
    // resolve bmson TOTAL (percent of the default)
    let default_total = chart.total(LnType::Ln);
    chart.meta.total = Some(if info.total > 0.0 {
        info.total / 100.0 * default_total
    } else {
        default_total
    });
    chart.compute_bpm_stats();
    Ok(chart)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_bmson() {
        let src = r#"{
            "version": "1.0.0",
            "info": {"title": "t", "artist": "a", "init_bpm": 120, "resolution": 240, "mode_hint": "beat-7k"},
            "sound_channels": [
                {"name": "a.wav", "notes": [{"x": 1, "y": 0, "l": 0, "c": false}, {"x": 8, "y": 240, "l": 480, "c": true}, {"x": 0, "y": 960, "l": 0, "c": false}]}
            ]
        }"#;
        let c = parse_bmson(src.as_bytes()).unwrap();
        assert_eq!(c.mode(), PlayMode::Beat7K);
        assert_eq!(c.note_count(), 2);
        assert_eq!(c.notes[1].lane, 7);
        assert_eq!(c.notes[1].time, 500_000);
        assert_eq!(c.notes[1].end_time(), 1_500_000);
        assert_eq!(c.sounds[1].start, 500_000);
        assert_eq!(c.bgm.len(), 1);
        assert!(c.meta.total.unwrap() >= 260.0);
    }
}
