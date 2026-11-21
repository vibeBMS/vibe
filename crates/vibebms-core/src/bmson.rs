//! BMSON (JSON chart format) decoder.

use crate::chart::*;
use crate::mode::PlayMode;
use crate::timing::{TimingEvent, TimingMap};
use crate::{limits, ChartError};
use serde::de::{self, Deserializer, IgnoredAny, SeqAccess, Visitor};
use serde::Deserialize;
use std::cell::Cell;
use std::marker::PhantomData;

thread_local! {
    /// The list elements the bmson being read may still have
    /// ([`limits::MAX_OBJECTS`]): counted as they are read, so a file of
    /// millions of `{}` is refused before they are all built.
    static LEFT: Cell<usize> = const { Cell::new(0) };
}

/// A list of a bmson, each element counted against [`LEFT`].
fn counted<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    struct List<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for List<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut out = Vec::new();
            while let Some(x) = seq.next_element()? {
                let left = LEFT.get();
                if left == 0 {
                    let more = format!("more than {} objects", limits::MAX_OBJECTS);
                    return Err(de::Error::custom(more));
                }
                LEFT.set(left - 1);
                out.push(x);
            }
            Ok(out)
        }
    }
    d.deserialize_seq(List(PhantomData))
}

/// [`counted`], of a list that may be left out or null.
fn counted_or_none<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Vec<T>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(bound = "T: Deserialize<'de>")]
    struct List<T>(#[serde(deserialize_with = "counted")] Vec<T>);
    Ok(Option::<List<T>>::deserialize(d)?.map(|l| l.0))
}

/// A note's lane: a number; anything else is none (0), passed over unread.
#[derive(Default, Clone, Copy)]
struct Lane(i64);

impl<'de> Deserialize<'de> for Lane {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Lane, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Lane;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a lane")
            }
            fn visit_i64<E>(self, v: i64) -> Result<Lane, E> {
                Ok(Lane(v))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Lane, E> {
                Ok(Lane(i64::try_from(v).unwrap_or(i64::MAX)))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Lane, E> {
                Ok(Lane(v as i64))
            }
            fn visit_bool<E>(self, _: bool) -> Result<Lane, E> {
                Ok(Lane(0))
            }
            fn visit_str<E>(self, _: &str) -> Result<Lane, E> {
                Ok(Lane(0))
            }
            fn visit_unit<E>(self) -> Result<Lane, E> {
                Ok(Lane(0))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Lane, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Lane(0))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut m: A) -> Result<Lane, A::Error> {
                while m.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Lane(0))
            }
        }
        d.deserialize_any(V)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Bmson {
    info: Info,
    #[serde(deserialize_with = "counted_or_none")]
    lines: Option<Vec<BarLineDef>>,
    #[serde(deserialize_with = "counted")]
    bpm_events: Vec<BpmEv>,
    #[serde(deserialize_with = "counted")]
    stop_events: Vec<StopEv>,
    #[serde(deserialize_with = "counted")]
    scroll_events: Vec<ScrollEv>,
    #[serde(deserialize_with = "counted")]
    sound_channels: Vec<SoundChannel>,
    #[serde(deserialize_with = "counted")]
    mine_channels: Vec<MineChannel>,
    #[serde(deserialize_with = "counted")]
    key_channels: Vec<MineChannel>,
    bga: Bga,
}

#[derive(Deserialize)]
#[serde(default)]
struct Info {
    title: String,
    subtitle: String,
    artist: String,
    #[serde(deserialize_with = "counted")]
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
    #[serde(deserialize_with = "counted")]
    notes: Vec<BNote>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BNote {
    x: Lane,
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
    #[serde(deserialize_with = "counted")]
    notes: Vec<MineNote>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MineNote {
    x: Lane,
    y: f64,
    damage: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Bga {
    #[serde(deserialize_with = "counted")]
    bga_header: Vec<BgaHeader>,
    #[serde(deserialize_with = "counted")]
    bga_events: Vec<BgaEv>,
    #[serde(deserialize_with = "counted")]
    layer_events: Vec<BgaEv>,
    #[serde(deserialize_with = "counted")]
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
        // (no bmson mode hint names them: never read so)
        PlayMode::Key4 | PlayMode::Key6 | PlayMode::Key8 => (x < mode.lane_count()).then_some(x),
    };
    lane.map(|l| l as u8)
}

/// The longest chart read: 10,000 measures of 4/4 (in beats).
const MAX_BEATS: f64 = 40_000.0;

/// Every position (pulses) in a bmson: a chart reaching past `MAX_BEATS`
/// (or with a position that is not a number) is refused, before anything
/// is built per measure.
fn check_extent(b: &Bmson, res: f64) -> Result<(), ChartError> {
    let mut ys: Vec<f64> = Vec::new();
    ys.extend(b.lines.iter().flatten().map(|l| l.y));
    ys.extend(b.bpm_events.iter().map(|e| e.y));
    ys.extend(b.stop_events.iter().flat_map(|e| [e.y, e.duration]));
    ys.extend(b.scroll_events.iter().map(|e| e.y));
    for ch in &b.sound_channels {
        ys.extend(ch.notes.iter().flat_map(|n| [n.y, n.y + n.l]));
    }
    for ch in b.mine_channels.iter().chain(&b.key_channels) {
        ys.extend(ch.notes.iter().map(|n| n.y));
    }
    let bga = &b.bga;
    for evs in [&bga.bga_events, &bga.layer_events, &bga.poor_events] {
        ys.extend(evs.iter().map(|e| e.y));
    }
    match ys.iter().find(|y| y.is_nan() || y.abs() / res > MAX_BEATS) {
        Some(y) => Err(ChartError::Parse(format!(
            "bmson position {y} is past {} measures",
            MAX_BEATS / 4.0
        ))),
        None => Ok(()),
    }
}

/// How many objects a bmson has (`limits::MAX_OBJECTS`).
fn object_count(b: &Bmson) -> usize {
    let notes = |n: usize| n + 1;
    let bga = &b.bga;
    b.lines.as_ref().map_or(0, Vec::len)
        + b.bpm_events.len()
        + b.stop_events.len()
        + b.scroll_events.len()
        + (b.sound_channels.iter().map(|c| notes(c.notes.len()))).sum::<usize>()
        + (b.mine_channels.iter().chain(&b.key_channels))
            .map(|c| notes(c.notes.len()))
            .sum::<usize>()
        + bga.bga_header.len()
        + bga.bga_events.len()
        + bga.layer_events.len()
        + bga.poor_events.len()
}

pub fn parse_bmson(bytes: &[u8]) -> Result<Chart, ChartError> {
    limits::file(bytes.len())?;
    LEFT.set(limits::MAX_OBJECTS);
    let b: Bmson = serde_json::from_slice(bytes).map_err(|e| ChartError::Parse(e.to_string()))?;
    limits::objects(object_count(&b))?;
    let info = &b.info;
    let res = if info.resolution > 0.0 && info.resolution.is_finite() {
        info.resolution
    } else {
        240.0
    };
    check_extent(&b, res)?;
    let beat = |y: f64| y / res;
    let mode = mode_from_hint(&info.mode_hint);
    let default_ln = match info.ln_type {
        1 => Some(LnType::Ln),
        2 => Some(LnType::Cn),
        3 => Some(LnType::Hcn),
        _ => None,
    };
    // (a number out of `limits::number`'s range is as one not given)
    let init_bpm = limits::number(info.init_bpm)
        .filter(|b| *b > 0.0)
        .unwrap_or(130.0);
    let total_rate = limits::number(info.total).unwrap_or(100.0);

    let mut ev = Vec::new();
    for e in &b.bpm_events {
        if let Some(bpm) = limits::clamped(e.bpm) {
            ev.push((beat(e.y), TimingEvent::Bpm(bpm)));
        }
    }
    for e in &b.stop_events {
        ev.push((beat(e.y), TimingEvent::StopBeats(e.duration / res)));
    }
    for e in &b.scroll_events {
        if let Some(rate) = limits::clamped(e.rate) {
            ev.push((beat(e.y), TimingEvent::Scroll(rate)));
        }
    }
    let timing = TimingMap::build(init_bpm, ev);
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
        initial_bpm: init_bpm,
        judge_rank: JudgeRank::Bmson(limits::number(info.judge_rank).unwrap_or(100.0)),
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
        notes.sort_by(|a, b| a.y.total_cmp(&b.y));
        let times: Vec<i64> = notes.iter().map(|n| t(n.y)).collect();
        // the first later time after each note's own (`later`: the times
        // after it that no nearer one hides, the nearest last)
        let mut next_times: Vec<Option<i64>> = vec![None; notes.len()];
        let mut later: Vec<i64> = Vec::new();
        for (i, time) in times.iter().enumerate().rev() {
            while later.last().is_some_and(|l| l <= time) {
                later.pop();
            }
            next_times[i] = later.last().copied();
            later.push(*time);
        }
        // Each note plays the channel's file from the point the previous
        // restart (c == false) happened, until the next note in the channel.
        let mut restart_time = 0i64;
        for (i, n) in notes.iter().enumerate() {
            let time = times[i];
            if !n.c {
                restart_time = time;
            }
            let next_time = next_times[i];
            let start = time - restart_time;
            let end = next_time.map(|nt| nt - restart_time);
            let sound = chart.sounds.len() as SoundId;
            chart.sounds.push(SoundSlice { file, start, end });
            let x = n.x.0;
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
                if let Some(lane) = map_lane(mode, n.x.0) {
                    let kind = if mine {
                        meta.has_mine = true;
                        NoteKind::Mine {
                            damage: limits::number(n.damage).unwrap_or(0.0) as f32,
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
        _ => (0..=((last / 4.0).ceil().clamp(0.0, MAX_BEATS / 4.0) as usize).saturating_add(1))
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
    chart.meta.total = Some(if total_rate > 0.0 {
        total_rate / 100.0 * default_total
    } else {
        default_total
    });
    chart.meta.total_rate = Some(total_rate);
    chart.compute_bpm_stats();
    limits::check(chart)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_bmson_positions_are_refused() {
        let chart = |notes: &str, extra: &str| {
            format!(
                r#"{{"version": "1.0.0", "info": {{"title": "t", "init_bpm": 120, "resolution": 240}},
                "sound_channels": [{{"name": "a.wav", "notes": [{notes}]}}]{extra}}}"#
            )
        };
        let ok = chart(r#"{"x": 1, "y": 0, "l": 0, "c": false}"#, "");
        assert!(parse_bmson(ok.as_bytes()).is_ok());
        // a note 10^15 pulses away would build billions of bar lines
        for notes in [
            r#"{"x": 1, "y": 1e15, "l": 0, "c": false}"#,
            r#"{"x": 1, "y": 0, "l": 1e300, "c": false}"#,
            r#"{"x": 1, "y": -1e18, "l": 0, "c": false}"#,
        ] {
            assert!(parse_bmson(chart(notes, "").as_bytes()).is_err(), "{notes}");
        }
        let far_stop = r#", "stop_events": [{"y": 0, "duration": 1e17}]"#;
        let n = r#"{"x": 1, "y": 0, "l": 0, "c": false}"#;
        assert!(parse_bmson(chart(n, far_stop).as_bytes()).is_err());
        // just inside 10,000 measures is read
        let edge = chart(r#"{"x": 1, "y": 9599000, "l": 0, "c": false}"#, "");
        let c = parse_bmson(edge.as_bytes()).unwrap();
        assert!(c.bar_lines.len() <= 10_002);
        // a tiny resolution makes the same positions far away
        let tiny = r#"{"version": "1.0.0", "info": {"title": "t", "init_bpm": 120, "resolution": 1e-12},
            "sound_channels": [{"name": "a.wav", "notes": [{"x": 1, "y": 10, "l": 0, "c": false}]}]}"#;
        assert!(parse_bmson(tiny.as_bytes()).is_err());
    }

    /// Numbers no arithmetic stays finite with are as ones not given (a
    /// tempo change: the fastest there may be): what the chart says of
    /// itself can be written down and read again.
    #[test]
    fn hostile_bmson_numbers_stay_finite() {
        let src = r#"{"version": "1.0.0",
            "info": {"title": "t", "init_bpm": 1e308, "resolution": 240, "total": 1e308, "judge_rank": -1e300},
            "bpm_events": [{"y": 240, "bpm": 1e308}, {"y": 480, "bpm": 180}],
            "scroll_events": [{"y": 0, "rate": 1e308}],
            "mine_channels": [{"name": "m.wav", "notes": [{"x": 2, "y": 0, "damage": 1e308}]}],
            "sound_channels": [{"name": "a.wav", "notes": [{"x": 1, "y": 0}, {"x": 1, "y": 960}]}]}"#;
        let c = parse_bmson(src.as_bytes()).unwrap();
        let m = &c.meta;
        assert_eq!(
            (m.initial_bpm, m.min_bpm, m.max_bpm),
            (130.0, 130.0, limits::MAX_NUMBER)
        );
        assert_eq!(m.total, Some(c.total(LnType::Ln)));
        assert_eq!(m.judge_rank, JudgeRank::Bmson(100.0));
        assert!(c.notes.iter().all(|n| n.pos.is_finite()));
        assert!(matches!(c.notes[1].kind, NoteKind::Mine { damage } if damage == 0.0));
        let json = serde_json::to_string(m).unwrap();
        assert_eq!(serde_json::from_str::<ChartMeta>(&json).unwrap(), *m);
        // a slow tempo that puts a note past 24 hours: refused
        let slow = src.replace("\"bpm\": 180", "\"bpm\": 1e-9");
        assert!(parse_bmson(slow.as_bytes()).is_err());
    }

    /// Many notes at one time are read as fast as any (each note's sound
    /// ends at the next later one).
    #[test]
    fn notes_at_one_time_cost_nothing_each() {
        let notes = r#"{"x": 1, "y": 240},"#.repeat(100_000);
        let src = format!(
            r#"{{"info": {{"init_bpm": 120}}, "sound_channels": [{{"name": "a.wav",
                "notes": [{notes} {{"x": 2, "y": 480}}, {{"x": 3, "y": 0}}]}}]}}"#
        );
        let t = std::time::Instant::now();
        let c = parse_bmson(src.as_bytes()).unwrap();
        assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
        assert_eq!(c.notes.len(), 100_002);
        // the note at 0 plays to 0.5 s, each at 0.5 s to 1 s, the last on
        let ends: Vec<Option<i64>> = [0, 1, 100_000, 100_001]
            .map(|i| c.sounds[c.notes[i].sound as usize].end)
            .to_vec();
        assert_eq!(ends, [Some(500_000), Some(500_000), Some(500_000), None]);
    }

    /// More objects than any chart has: refused.
    #[test]
    fn too_many_objects_are_refused() {
        let lines = |n: usize| {
            format!(
                r#"{{"lines": [{}{{"y": 0}}]}}"#,
                r#"{"y":0},"#.repeat(n - 1)
            )
        };
        assert!(parse_bmson(lines(1000).as_bytes()).is_ok());
        let e = parse_bmson(lines(limits::MAX_OBJECTS + 1).as_bytes());
        assert!(matches!(e, Err(ChartError::Parse(m)) if m.contains("objects")));
    }

    /// The lists are counted as they are read: a file of more empty
    /// objects than a chart may have is refused before it is all read.
    /// A lane that is no number is none, and passed over.
    #[test]
    fn objects_are_counted_as_they_are_read() {
        let mut src = r#"{"sound_channels": [{"name": "a", "notes": ["#.to_string();
        src.push_str(&"{},".repeat(limits::MAX_OBJECTS + 1));
        // (a file that ends wrong: never reached)
        src.push('!');
        let e = parse_bmson(src.as_bytes());
        assert!(matches!(e, Err(ChartError::Parse(m)) if m.contains("objects")));
        let src = r#"{"sound_channels": [{"name": "a", "notes": [
            {"x": 2.0, "y": 0}, {"x": [1, [2], {"x": 3}], "y": 1}, {"x": "3", "y": 2},
            {"x": null, "y": 3}, {"x": {"a": [1]}, "y": 4}, {"x": 18446744073709551615, "y": 5}]}]}"#;
        let c = parse_bmson(src.as_bytes()).unwrap();
        assert_eq!((c.notes.len(), c.bgm.len()), (1, 5));
        assert_eq!(c.notes[0].lane, 1);
    }

    /// A tempo or a scroll rate past the limit is the fastest there may be,
    /// not left out.
    #[test]
    fn a_tempo_past_the_limit_is_clamped() {
        let last = |bpm: &str| {
            let src = format!(
                r#"{{"info": {{"init_bpm": 120}}, "bpm_events": [{{"y": 240, "bpm": {bpm}}}],
                   "scroll_events": [{{"y": 240, "rate": {bpm}}}],
                   "sound_channels": [{{"name": "a", "notes": [{{"x": 1, "y": 96000}}]}}]}}"#
            );
            parse_bmson(src.as_bytes()).unwrap().notes[0].time
        };
        assert_eq!(last("1e10"), last("1e9"));
        assert_eq!(last("-1e10"), last("-1e9"));
        assert!(last("1e9") < 1_000_000);
    }

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
