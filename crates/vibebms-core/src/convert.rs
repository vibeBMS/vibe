//! KEY CONVERT: a chart played in another key mode (bmz-player's key mode
//! conversion). Each note keeps its time and kind and moves to a lane of
//! the target mode by a fixed table, so a replay of the play makes the
//! same chart. Such a play is not saved and goes to no IR.
//!
//! Lanes are numbered as `PlayMode` numbers them: 5K `[k1..k5, S]`, 7K
//! `[k1..k7, S]`, 9K `[b1..b9]`, 4K / 6K / 8K their keys left to right.
//!
//! | from | to | lanes |
//! |---|---|---|
//! | 7K | 9K | keys 1-7 on buttons 2-8, the scratch on button 1 |
//! | 9K | 7K | buttons 2-8 on keys 1-7, buttons 1 and 9 on the scratch |
//! | 5K | 7K | keys 1-5 on keys 2-6, the scratch on the scratch |
//! | 4K | 7K | keys 1, 2, 4, 5 (where the `#4K` chart has them) |
//! | 6K | 7K | keys 1-3, 5-7 (where the `#6K` chart has them) |
//! | 8K | 7K | its first key on the scratch, the others on keys 1-7 |
//!
//! TO 9KEYS from 5K, 4K, 6K or 8K goes through 7K. A note that lands on a
//! lane where a note of another of the chart's lanes is (at its time, or
//! inside a long note there) plays as a background sound instead.

use crate::chart::{BgmEvent, Chart, Note, NoteKind, NO_SOUND};
use crate::mode::PlayMode;
use serde::{Deserialize, Serialize};

crate::choices! {
    /// The key mode a chart is played in (KEY CONVERT).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum KeyConvert {
        #[default]
        Off => "OFF",
        To7 => "7KEYS",
        To9 => "9KEYS",
    }
}

impl KeyConvert {
    pub fn is_off(&self) -> bool {
        *self == KeyConvert::Off
    }
}

/// The lane each of `from`'s lanes goes to in 7K (`None`: none). For 4K,
/// 6K and 8K it is where the chart has the lane in 7K's channels.
fn to_seven(from: PlayMode) -> Option<&'static [u8]> {
    Some(match from {
        PlayMode::Beat5K => &[1, 2, 3, 4, 5, 7],
        PlayMode::Popn9K => &[7, 0, 1, 2, 3, 4, 5, 6, 7],
        m => return m.seven_key_lanes(),
    })
}

/// 7K's lanes in 9K.
const SEVEN_TO_NINE: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 0];

/// The mode `from` is played in under `to`, and where each of its lanes
/// goes (`None`: no conversion, the chart plays as it is).
pub fn lane_map(from: PlayMode, to: KeyConvert) -> Option<(PlayMode, Vec<u8>)> {
    match to {
        KeyConvert::Off => None,
        KeyConvert::To7 => to_seven(from).map(|m| (PlayMode::Beat7K, m.to_vec())),
        KeyConvert::To9 => {
            let seven: Vec<u8> = match from {
                PlayMode::Beat7K => (0..8).collect(),
                PlayMode::Popn9K => return None,
                _ => to_seven(from)?.to_vec(),
            };
            let nine = seven.iter().map(|l| SEVEN_TO_NINE[*l as usize]).collect();
            Some((PlayMode::Popn9K, nine))
        }
    }
}

/// The mode `from` is played in under `to` (itself when not converted).
pub fn target(from: PlayMode, to: KeyConvert) -> PlayMode {
    lane_map(from, to).map_or(from, |(m, _)| m)
}

/// Play `chart` in the mode `to` makes of it: `Some` of the mode it was
/// in when it was converted, `None` when not (OFF, or no table for its
/// mode).
pub fn convert(chart: &mut Chart, to: KeyConvert) -> Option<PlayMode> {
    let from = chart.mode();
    let (mode, map) = lane_map(from, to)?;
    // (each note with the lane it came from; one on no lane of its mode
    // stays on none)
    let mut notes: Vec<(u8, Note)> = std::mem::take(&mut chart.notes)
        .into_iter()
        .filter_map(|mut n| {
            let from = n.lane;
            n.lane = *map.get(from as usize)?;
            Some((from, n))
        })
        .collect();
    notes.sort_by(|(_, a), (_, b)| a.time.cmp(&b.time).then(a.lane.cmp(&b.lane)));
    // what takes each lane, up to when: the lanes it came from and the end
    // of their last note there (a long note's end). Notes of one lane of
    // the chart stay as the chart has them.
    let mut busy: Vec<Vec<(u8, i64)>> = vec![Vec::new(); mode.lane_count()];
    let mut kept = Vec::with_capacity(notes.len());
    for (from, n) in notes {
        let lane = n.lane as usize;
        // (an invisible note takes no place)
        let taken = (busy[lane].iter()).any(|(f, until)| *f != from && n.time <= *until);
        if taken && !matches!(n.kind, NoteKind::Invisible) {
            if n.sound != NO_SOUND && n.is_playable() {
                chart.bgm.push(BgmEvent {
                    time: n.time,
                    sound: n.sound,
                });
            }
            if let NoteKind::Long {
                end_time,
                end_sound,
                ..
            } = n.kind
            {
                if end_sound != NO_SOUND {
                    chart.bgm.push(BgmEvent {
                        time: end_time,
                        sound: end_sound,
                    });
                }
            }
            continue;
        }
        if n.is_playable() || matches!(n.kind, NoteKind::Mine { .. }) {
            match busy[lane].iter_mut().find(|(f, _)| *f == from) {
                Some((_, until)) => *until = (*until).max(n.end_time()),
                None => busy[lane].push((from, n.end_time())),
            }
        }
        kept.push(n);
    }
    chart.notes = kept;
    chart.bgm.sort_by_key(|b| b.time);
    chart.meta.mode = Some(mode);
    Some(from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::ChartMeta;

    fn note(lane: u8, time: i64) -> Note {
        Note {
            lane,
            time,
            beat: 0.0,
            pos: 0.0,
            sound: 1,
            kind: NoteKind::Normal,
        }
    }

    fn chart(mode: PlayMode, notes: Vec<Note>) -> Chart {
        Chart {
            meta: ChartMeta {
                mode: Some(mode),
                ..Default::default()
            },
            notes,
            ..Default::default()
        }
    }

    fn lanes(c: &Chart) -> Vec<u8> {
        c.notes.iter().map(|n| n.lane).collect()
    }

    #[test]
    fn tables() {
        use KeyConvert::*;
        let m = lane_map;
        assert_eq!(
            m(PlayMode::Beat7K, To9),
            Some((PlayMode::Popn9K, vec![1, 2, 3, 4, 5, 6, 7, 0]))
        );
        assert_eq!(
            m(PlayMode::Popn9K, To7),
            Some((PlayMode::Beat7K, vec![7, 0, 1, 2, 3, 4, 5, 6, 7]))
        );
        assert_eq!(
            m(PlayMode::Beat5K, To7),
            Some((PlayMode::Beat7K, vec![1, 2, 3, 4, 5, 7]))
        );
        assert_eq!(
            m(PlayMode::Beat5K, To9),
            Some((PlayMode::Popn9K, vec![2, 3, 4, 5, 6, 0]))
        );
        assert_eq!(
            m(PlayMode::Key8, To9),
            Some((PlayMode::Popn9K, vec![0, 1, 2, 3, 4, 5, 6, 7]))
        );
        // nothing to do, or no table
        for (from, to) in [
            (PlayMode::Beat7K, To7),
            (PlayMode::Popn9K, To9),
            (PlayMode::Beat14K, To7),
            (PlayMode::Beat10K, To9),
            (PlayMode::Beat7K, Off),
        ] {
            assert_eq!(m(from, to), None, "{from:?} {to:?}");
            assert_eq!(target(from, to), from);
        }
        // every table lands on the target's lanes
        for from in PlayMode::ALL {
            for to in KeyConvert::ALL {
                if let Some((mode, map)) = lane_map(from, to) {
                    assert_eq!(map.len(), from.lane_count());
                    assert!(map.iter().all(|l| (*l as usize) < mode.lane_count()));
                }
            }
        }
    }

    #[test]
    fn seven_to_nine_and_back() {
        let notes = (0..8).map(|l| note(l, 1000 * l as i64)).collect();
        let mut c = chart(PlayMode::Beat7K, notes);
        assert_eq!(convert(&mut c, KeyConvert::To9), Some(PlayMode::Beat7K));
        assert_eq!(c.mode(), PlayMode::Popn9K);
        assert_eq!(lanes(&c), vec![1, 2, 3, 4, 5, 6, 7, 0]);
        // 9K's buttons 1 and 9 share the scratch: a chord of them leaves
        // one note, the other's sound plays as BGM
        let mut c = chart(PlayMode::Popn9K, vec![note(0, 0), note(8, 0), note(4, 0)]);
        assert_eq!(convert(&mut c, KeyConvert::To7), Some(PlayMode::Popn9K));
        assert_eq!(c.mode(), PlayMode::Beat7K);
        assert_eq!(lanes(&c), vec![3, 7]);
        assert_eq!(c.bgm.len(), 1);
        assert_eq!(c.bgm[0].time, 0);
    }

    #[test]
    fn a_note_inside_a_long_note_is_bgm() {
        let long = Note {
            kind: NoteKind::Long {
                end_time: 500,
                end_beat: 0.0,
                end_pos: 0.0,
                end_sound: NO_SOUND,
                ln_type: None,
            },
            ..note(0, 0)
        };
        // 9K buttons 1 (long, 0-500) and 9 (at 300, and at 600) on the scratch
        let mut c = chart(PlayMode::Popn9K, vec![long, note(8, 300), note(8, 600)]);
        convert(&mut c, KeyConvert::To7);
        assert_eq!(c.notes.iter().map(|n| n.time).collect::<Vec<_>>(), [0, 600]);
        assert_eq!(c.bgm.len(), 1);
        assert_eq!(c.bgm[0].time, 300);
    }

    #[test]
    fn off_and_no_table_leave_it() {
        let mut c = chart(PlayMode::Beat14K, vec![note(3, 0)]);
        assert_eq!(convert(&mut c, KeyConvert::To7), None);
        assert_eq!(convert(&mut c, KeyConvert::Off), None);
        assert_eq!((c.mode(), lanes(&c)), (PlayMode::Beat14K, vec![3]));
    }
}
