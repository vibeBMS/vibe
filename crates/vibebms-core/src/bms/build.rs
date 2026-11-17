//! The last stage: objects on lanes, long notes paired, times from the
//! timing map: the chart.

use super::place::{ObjKind, Placed, RawObj, SCRATCH};
use super::source::Source;
use crate::chart::*;
use crate::mode::PlayMode;
use crate::timing::{TimingEvent, TimingMap};
use std::collections::HashMap;

pub(super) fn build(src: Source, p: Placed, pms: bool) -> Chart {
    let mut meta = src.meta;
    let mut chart = Chart::default();

    // sounds / images, by id
    let mut wav_map: HashMap<u32, SoundId> = HashMap::new();
    let mut wav_ids: Vec<_> = src.wav.keys().copied().collect();
    wav_ids.sort_unstable();
    for id in wav_ids {
        let file = chart.sound_files.len() as u32;
        chart.sound_files.push(src.wav[&id].to_owned());
        wav_map.insert(id, chart.sounds.len() as SoundId);
        chart.sounds.push(SoundSlice {
            file,
            start: 0,
            end: None,
        });
    }
    let snd = |id: u32| wav_map.get(&id).copied().unwrap_or(NO_SOUND);
    let mut bmp_map: HashMap<u32, u32> = HashMap::new();
    let mut bmp_ids: Vec<_> = src.bmp.keys().copied().collect();
    bmp_ids.sort_unstable();
    for id in bmp_ids {
        bmp_map.insert(id, chart.images.len() as u32);
        chart.images.push(src.bmp[&id].to_owned());
    }

    let mode = if pms {
        PlayMode::Popn9K
    } else {
        let double = p
            .objs
            .iter()
            .any(|o| o.player == 1 && o.kind != ObjKind::Invisible);
        let seven = p
            .objs
            .iter()
            .any(|o| (o.key == 5 || o.key == 6) && o.kind != ObjKind::Invisible);
        match (double, seven) {
            (false, false) => PlayMode::Beat5K,
            (false, true) => PlayMode::Beat7K,
            (true, false) => PlayMode::Beat10K,
            (true, true) => PlayMode::Beat14K,
        }
    };
    meta.mode = Some(mode);
    let lane_of = |o: &RawObj| -> Option<u8> {
        let key = if o.key == SCRATCH {
            mode.keys_per_player()
        } else {
            o.key
        };
        if key >= mode.lanes_per_player() || o.player >= mode.players() {
            return None;
        }
        Some((o.player * mode.lanes_per_player() + key) as u8)
    };

    meta.has_stop = p
        .timing
        .iter()
        .any(|(_, e)| matches!(e, TimingEvent::StopBeats(_)));
    let timing = TimingMap::build(src.initial_bpm, p.timing);
    meta.initial_bpm = src.initial_bpm;
    let t_of = |beat: f64| timing.time_at_beat(beat);
    let p_of = |beat: f64| timing.pos_at_beat(beat);
    let note = |lane: u8, o: &RawObj, sound: SoundId, kind: NoteKind| Note {
        lane,
        time: t_of(o.beat),
        beat: o.beat,
        pos: p_of(o.beat),
        sound,
        kind,
    };

    // Visible notes: later definitions override earlier ones at the same position.
    let lanes = mode.lane_count();
    let by_place = || -> Vec<HashMap<(u32, u64, u64), RawObj>> {
        std::iter::repeat_with(HashMap::new).take(lanes).collect()
    };
    let (mut visible, mut longs) = (by_place(), by_place());
    let mut notes: Vec<Note> = Vec::new();
    for o in p.objs {
        let Some(lane) = lane_of(&o) else { continue };
        match o.kind {
            ObjKind::Visible => {
                visible[lane as usize].insert(o.pos_key, o);
            }
            ObjKind::Long => {
                longs[lane as usize].insert(o.pos_key, o);
            }
            ObjKind::Invisible => {
                meta.has_invisible = true;
                notes.push(note(lane, &o, snd(o.id), NoteKind::Invisible));
            }
            ObjKind::Mine => {
                meta.has_mine = true;
                let damage = o.id as f32;
                notes.push(note(lane, &o, snd(0), NoteKind::Mine { damage }));
            }
        }
    }

    let ln_type = meta.ln_type;
    let long = |lane: u8, s: &RawObj, end_beat: f64, end_sound: SoundId| {
        let kind = NoteKind::Long {
            end_time: t_of(end_beat),
            end_beat,
            end_pos: p_of(end_beat),
            end_sound,
            ln_type,
        };
        note(lane, s, snd(s.id), kind)
    };
    for (lane, (visible, longs)) in visible.into_iter().zip(longs).enumerate() {
        let lane = lane as u8;
        // LNTYPE 1 channel long notes: pair consecutive objects
        let mut ln: Vec<RawObj> = longs.into_values().collect();
        ln.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        // (sorted by start and apart: a note is looked up by binary search)
        let mut ranges: Vec<(f64, f64)> = Vec::new();
        for pair in ln.chunks(2) {
            if let [s, e] = pair {
                let end_sound = snd(e.id);
                let end_sound = if end_sound == snd(s.id) {
                    NO_SOUND
                } else {
                    end_sound
                };
                notes.push(long(lane, s, e.beat, end_sound));
                ranges.push((s.beat, e.beat));
                meta.has_long = true;
            } else {
                // dangling start: treat as a normal note
                notes.push(note(lane, &pair[0], snd(pair[0].id), NoteKind::Normal));
            }
        }
        // normal channel notes, with LNOBJ terminators
        let mut vis: Vec<RawObj> = visible.into_values().collect();
        vis.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        let in_ln = |beat: f64| {
            let after = ranges.partition_point(|(s, _)| *s <= beat);
            after > 0 && beat <= ranges[after - 1].1
        };
        let mut pending: Option<RawObj> = None;
        for o in vis {
            if in_ln(o.beat) {
                continue; // overlaps a channel LN
            }
            if src.lnobj.contains(&o.id) {
                if let Some(s) = pending.take() {
                    // the marker only ends the note: its #WAV is not the end's
                    // sound (as beatoraja; a channel LN's end keeps its own)
                    notes.push(long(lane, &s, o.beat, NO_SOUND));
                    meta.has_long = true;
                }
                continue;
            }
            if let Some(s) = pending.replace(o) {
                notes.push(note(lane, &s, snd(s.id), NoteKind::Normal));
            }
        }
        if let Some(s) = pending {
            notes.push(note(lane, &s, snd(s.id), NoteKind::Normal));
        }
    }

    notes.sort_by(|a, b| a.time.cmp(&b.time).then(a.lane.cmp(&b.lane)));
    chart.notes = notes;

    let mut bgm = p.bgm;
    bgm.sort_by(|a, b| a.0.total_cmp(&b.0));
    chart.bgm = bgm
        .into_iter()
        .filter_map(|(beat, id)| {
            let sound = snd(id);
            (sound != NO_SOUND).then(|| BgmEvent {
                time: t_of(beat),
                sound,
            })
        })
        .collect();

    let mut bga = p.bga;
    bga.sort_by(|a, b| a.0.total_cmp(&b.0));
    chart.bga = bga
        .into_iter()
        .map(|(beat, layer, id)| BgaEvent {
            time: t_of(beat),
            layer,
            image: bmp_map.get(&id).copied(),
        })
        .collect();
    meta.has_bga = !chart.bga.is_empty();

    chart.bar_lines = p
        .measure_start
        .iter()
        .map(|b| BarLine {
            time: t_of(*b),
            pos: p_of(*b),
        })
        .collect();

    chart.timing = timing;
    chart.meta = meta;
    chart.compute_bpm_stats();
    chart
}
