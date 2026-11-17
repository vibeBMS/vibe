//! A made-up player and charts for tests (here and in the app's): it
//! presses notes with some timing error, misses some, lets long notes go
//! early now and then and presses where there is no note. The same seed
//! plays the same.

use crate::chart::{Chart, NoteKind};
use crate::engine::{Engine, InputEvent};
use crate::rng::JavaRandom;

/// A chart in BMS text: `keys` 5, 7, 10 or 14 (with scratch), notes, long
/// notes (`#LNTYPE 1`, no LN mode of their own), mines, a BPM change and a
/// stop.
pub fn chart_text(keys: usize, seed: i64) -> String {
    let mut rng = JavaRandom::new(seed);
    let side: Vec<&str> = if keys % 5 == 0 {
        vec!["1", "2", "3", "4", "5", "6"]
    } else {
        vec!["1", "2", "3", "4", "5", "8", "9", "6"]
    };
    // (note channel's first digit, long note's, mine's)
    let sides: &[(char, char, char)] = if keys >= 10 {
        &[('1', '5', 'D'), ('2', '6', 'E')]
    } else {
        &[('1', '5', 'D')]
    };
    let mut s = format!(
        "#PLAYER {}\n#BPM 150\n#LNTYPE 1\n#WAV01 a.wav\n#WAV02 b.wav\n#WAV03 c.wav\n\
         #BPM01 180\n#STOP01 48\n#00203:96\n#00508:01\n#00609:01\n",
        if keys >= 10 { 3 } else { 1 }
    );
    let chans: Vec<(String, String, String)> = sides
        .iter()
        .flat_map(|(n, l, m)| {
            side.iter()
                .map(move |c| (format!("{n}{c}"), format!("{l}{c}"), format!("{m}{c}")))
        })
        .collect();
    for m in 1..24i32 {
        for (c, lc, mc) in &chans {
            // notes: 8th / 16th rows, sparse to dense
            let steps = if m % 3 == 0 { 16 } else { 8 };
            let mut row = String::new();
            for _ in 0..steps {
                let on = rng.next_int_bounded(100) < 18 + m;
                row.push_str(if on {
                    ["01", "02", "03"][rng.next_int_bounded(3) as usize]
                } else {
                    "00"
                });
            }
            s.push_str(&format!("#{m:03}{c}:{row}\n"));
            // long notes: start and end on the LN channel
            if rng.next_int_bounded(4) == 0 {
                let mut ln = ["00"; 8];
                let a = rng.next_int_bounded(5) as usize;
                let b = a + 1 + rng.next_int_bounded((7 - a) as i32) as usize;
                ln[a] = "02";
                ln[b] = "02";
                s.push_str(&format!("#{m:03}{lc}:{}\n", ln.concat()));
            }
            // a mine now and then
            if rng.next_int_bounded(9) == 0 {
                s.push_str(&format!("#{m:03}{mc}:000A\n"));
            }
        }
    }
    s
}

/// The inputs of a play of `chart`'s lanes (one player's, or all), in
/// time order.
pub fn inputs(chart: &Chart, player: Option<usize>, seed: i64) -> Vec<InputEvent> {
    let mode = chart.mode();
    let mut rng = JavaRandom::new(seed);
    let mut out = Vec::new();
    // timing error up to about ±70 ms, mostly smaller
    let jitter = |rng: &mut JavaRandom, ms: i32| -> i64 {
        (rng.next_int_bounded(ms * 2 + 1) - ms + rng.next_int_bounded(ms + 1) - ms / 2) as i64
            * 1000
    };
    for lane in 0..mode.lane_count() {
        if player.is_some_and(|p| mode.player_of_lane(lane) != p) {
            continue;
        }
        let scratch = mode.is_scratch(lane);
        let notes: Vec<_> = chart
            .notes
            .iter()
            .filter(|n| n.lane as usize == lane && n.is_playable())
            .collect();
        let mut free_at = i64::MIN;
        for (k, n) in notes.iter().enumerate() {
            let next = notes.get(k + 1).map_or(i64::MAX, |m| m.time);
            let roll = rng.next_int_bounded(100);
            if roll < 7 {
                continue; // missed
            }
            let key = if scratch { (k % 2) as u8 } else { 0 };
            let press = (n.time + jitter(&mut rng, 70)).max(free_at + 1_000);
            let release = match n.kind {
                NoteKind::Long { end_time, .. } => {
                    if rng.next_int_bounded(100) < 15 {
                        (n.time + end_time) / 2
                    } else {
                        end_time + jitter(&mut rng, 60)
                    }
                }
                _ => press + 45_000,
            }
            .min(next.saturating_sub(30_000))
            .max(press + 1_000);
            let ev = |time, pressed| InputEvent {
                time,
                lane: lane as u8,
                key,
                pressed,
            };
            out.push(ev(press, true));
            out.push(ev(release, false));
            free_at = release;
            // a press where there is no note
            if roll >= 96 && next.saturating_sub(release) > 300_000 {
                let t = release + 150_000;
                out.push(ev(t, true));
                out.push(ev(t + 20_000, false));
                free_at = t + 20_000;
            }
        }
    }
    out.sort_by_key(|e| e.time);
    out
}

/// Drive `engine` as the play scene does: frames every 1/60 s, the inputs
/// up to each frame first, now and then judged at once (an input's own
/// update), until `end` µs.
pub fn drive(engine: &mut Engine, inputs: &[InputEvent], end: i64) {
    let mut i = 0;
    let mut t = -1_000_000;
    let mut last = t;
    while t < end {
        while let Some(e) = inputs.get(i).filter(|e| e.time <= t) {
            engine.input(*e);
            if i % 5 == 0 {
                last = last.max(e.time + 2_000);
                engine.update(last);
            }
            i += 1;
        }
        last = last.max(t);
        engine.update(last);
        t += 16_667;
    }
}
