//! What the select screen says beyond the selected chart's own facts: the
//! song's other charts, whether a play would be saved and the lamp a clear
//! would give, the IR's state, the chart's replay and text files, and a
//! folder's counts.

use super::tree::{Bar, Distribution};
use super::SelectScene;
use crate::app::Ctx;
use crate::config::{HiSpeedMode, KeyFilter, PlayConfig, Versus};
use crate::library::Song;
use crate::sysound::Sound;
use std::cell::Ref;
use std::path::PathBuf;
use vibebms_core::score::{Assist, AssistFacts, ClearType};
use vibebms_ir::lamps::RankView;
use vibebms_ir::outbox::State;

/// The song's charts shown by difficulty: BEGINNER .. INSANE.
pub(crate) const SIBLINGS: usize = 5;

/// Whether a play of the selected chart would be kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Keep {
    All,
    /// An assisted play keeps its lamp and play count only.
    LampOnly,
    /// Autoplay and VS keep nothing.
    Nothing,
}

impl Keep {
    pub fn label(self) -> &'static str {
        match self {
            Keep::All => "SAVE",
            Keep::LampOnly => "LAMP ONLY",
            Keep::Nothing => "NO SAVE",
        }
    }
}

/// The chart's note kinds are not known (libraries scanned before they
/// were kept have zeros).
fn unknown(s: &Song) -> bool {
    s.note_kinds == [0; 5]
}

/// What a play with the options chosen would be kept as, and the lamp a
/// clear would give (`None`: nothing is kept). The chart's facts decide
/// which options take effect (as `scene::play::setup` finds them); with
/// no chart chosen every option counts.
pub(crate) fn outcome(p: &PlayConfig, song: Option<&Song>) -> (Keep, Option<ClearType>) {
    // (a FREQ below 0 saves nothing: `scene::play::setup`)
    if p.autoplay || p.versus == Versus::Local || p.freq < 0 {
        return (Keep::Nothing, None);
    }
    let has = |f: fn(&Song) -> bool| song.is_none_or(f);
    let double = song.is_some_and(|s| s.mode.is_double());
    let facts = AssistFacts {
        lane: [p.random_1p]
            .into_iter()
            .chain(double.then_some(p.random_2p))
            .collect(),
        auto_scratch: p.auto_scratch
            && has(|s| {
                s.mode.has_scratch() && (unknown(s) || s.note_kinds[2] + s.note_kinds[3] > 0)
            }),
        legacy_note: p.legacy_note && has(|s| s.has_ln),
        no_mine: p.no_mine && has(|s| unknown(s) || s.note_kinds[4] > 0),
        battle: p.battle && has(|s| !s.mode.is_double()),
        wide_judge: p.expand_judge && p.judge_rates.iter().any(|r| *r > 100),
        bpm_guide: p.bpm_guide && has(|s| s.min_bpm < s.max_bpm),
    };
    match facts.level() {
        Assist::None => (Keep::All, Some(p.gauge.clear_type())),
        Assist::Light => (Keep::LampOnly, Some(ClearType::LightAssistEasy)),
        Assist::Full => (Keep::LampOnly, Some(ClearType::AssistEasy)),
    }
}

/// The IR in one line ("ONLINE · Bokutachi · 1 waiting"), and whether it
/// is reached (`None`: no account is on).
pub(crate) fn network(ctx: &Ctx) -> (String, Option<bool>) {
    let Some(a) = ctx.config.ir.primary() else {
        return ("OFF".into(), None);
    };
    let waiting: Vec<_> = (ctx.net.ir.items())
        .filter(|i| {
            matches!(
                i.state,
                State::Queued | State::Sending | State::Polling | State::Checking | State::Retry
            )
        })
        .collect();
    let offline = (waiting.iter()).any(|i| i.last.as_ref().is_some_and(|l| l.status.is_none()));
    let mut parts = vec![
        if offline { "OFFLINE" } else { "ONLINE" }.to_string(),
        a.title().to_string(),
    ];
    if !waiting.is_empty() {
        parts.push(format!("{} waiting", waiting.len()));
    }
    if ctx.net.ir.reads_only() {
        parts.push("another tab sends".into());
    }
    (parts.join(" · "), Some(!offline))
}

/// The ASSIST panel's options that are on: "OFF", the one's name, or how
/// many.
pub(crate) fn assist_summary(p: &PlayConfig) -> String {
    let on: Vec<&str> = [
        (p.auto_scratch, "AUTO SCRATCH"),
        (p.expand_judge, "EXPAND JUDGE"),
        (p.hispeed_mode == HiSpeedMode::Constant, "CONSTANT"),
        (p.judge_area, "JUDGE AREA"),
        (p.legacy_note, "LEGACY NOTE"),
        (p.mark_notes, "MARK NOTE"),
        (p.bpm_guide, "BPM GUIDE"),
        (p.no_mine, "NO MINE"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    match on.as_slice() {
        [] => "OFF".into(),
        [one] => one.to_string(),
        many => format!("{} ON", many.len()),
    }
}

/// The lane cover's parts ("24% · LIFT 10%"; "OFF").
pub(crate) fn lane_cover_summary(p: &PlayConfig) -> String {
    let pct = |v: f32| format!("{:.0}%", v * 100.0);
    let mut parts = Vec::new();
    if p.lane_cover || p.sudden > 0.0 {
        parts.push(pct(p.sudden));
    }
    if p.hidden > 0.0 {
        parts.push(format!("HIDDEN+ {}", pct(p.hidden)));
    }
    if p.lift > 0.0 {
        parts.push(format!("LIFT {}", pct(p.lift)));
    }
    match parts.is_empty() {
        true => "OFF".into(),
        false => parts.join(" · "),
    }
}

/// A chart's key mode as the key filter names it ("7KEYS").
pub(crate) fn mode_label(s: &Song) -> String {
    match KeyFilter::ALL.iter().skip(1).find(|f| f.keeps(s.mode)) {
        Some(f) => f.label().into(),
        None => format!("{}KEYS", s.mode.keys_per_player()),
    }
}

/// The share of a chart's players that cleared it (%), when the ranking
/// has every player's row.
pub(crate) fn clear_rate(v: &RankView) -> Option<i64> {
    let n = v.entries.len();
    if n == 0 || (v.players as usize) > n {
        return None;
    }
    let cleared = (v.entries.iter())
        .filter(|e| e.clear >= ClearType::AssistEasy)
        .count();
    Some((cleared * 100 / n) as i64)
}

/// A folder's charts, those installed, played and cleared (numbers
/// 9300-9303).
pub(crate) fn folder_facts(count: usize, (lamps, _): &Distribution) -> [i64; 4] {
    let sum = |l: &[u32]| l.iter().map(|n| *n as i64).sum::<i64>();
    let installed = sum(lamps);
    [
        (count as i64).max(installed),
        installed,
        installed - lamps[0] as i64,
        sum(&lamps[ClearType::AssistEasy as usize..]),
    ]
}

/// The selected chart's files the select screen offers: a replay, texts.
#[derive(Default)]
pub(crate) struct ChartFiles {
    /// The chart (its path, LN and rule modes) these are of.
    key: String,
    pub replay: bool,
    pub texts: Vec<PathBuf>,
}

/// The folder of a chart's file: its song's (`Song::folder` is the
/// library folder it is listed under).
pub(crate) fn song_dir(s: &Song) -> String {
    let p = std::path::Path::new(&s.path);
    p.parent()
        .map_or_else(String::new, |d| d.to_string_lossy().into_owned())
}

/// A chart folder's text files (LR2's READ TEXT), by name.
pub(crate) fn texts_of(song: &Song) -> Vec<PathBuf> {
    let Some(dir) = std::path::Path::new(&song.path).parent() else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = crate::platform::vfs::read_dir(dir)
        .map(|es| {
            (es.into_iter())
                .filter(|e| !e.is_dir)
                .map(|e| e.path)
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("txt")))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

impl SelectScene {
    /// The song's charts by difficulty: of the charts in the selected
    /// chart's file's folder and key mode, the selected one for its
    /// difficulty, else the highest level of each (library indices).
    pub(crate) fn siblings(&self, ctx: &Ctx, i: usize) -> [Option<usize>; SIBLINGS] {
        let mut out = [None; SIBLINGS];
        let songs = &ctx.lib.songs;
        let Some(me) = songs.get(i) else {
            return out;
        };
        let level = |k: usize| songs[k].level_num();
        for &k in self.song_dirs.get(&song_dir(me)).into_iter().flatten() {
            let s = &songs[k];
            let Some(slot) = (s.difficulty as usize)
                .checked_sub(1)
                .filter(|d| *d < SIBLINGS)
            else {
                continue;
            };
            if s.mode != me.mode || out[slot] == Some(i) {
                continue;
            }
            if k == i || out[slot].is_none_or(|o| level(k) > level(o)) {
                out[slot] = Some(k);
            }
        }
        out
    }

    /// The song's chart of difficulty slot `k`: its bar chosen, else put in
    /// place of the selected one.
    pub(super) fn pick_sibling(&mut self, ctx: &mut Ctx, k: usize) {
        let Some((i, _)) = self.selected_song(ctx) else {
            return;
        };
        let Some(j) = self.siblings(ctx, i).get(k).copied().flatten() else {
            return;
        };
        if j == i {
            return;
        }
        match self
            .bars
            .iter()
            .position(|b| matches!(b, Bar::Song(x) if *x == j))
        {
            Some(at) => self.cursor = at,
            None => self.bars[self.cursor] = Bar::Song(j),
        }
        ctx.play_sound(Sound::Scratch);
        self.moved(ctx);
    }

    /// The selected chart's replay and text files, looked for once per
    /// chart (and LN and rule mode).
    pub(crate) fn chart_files(&self, ctx: &Ctx, song: &Song) -> Ref<'_, ChartFiles> {
        let p = &ctx.config.play;
        let key = format!(
            "{}\u{1f}{:?}\u{1f}{:?}",
            song.path, p.ln_mode, p.judge_preset
        );
        if self.files.borrow().key != key {
            let ln = song.score_ln(p.ln_mode) as i32;
            let replay = crate::replay::exists(
                &ctx.paths.root,
                &song.sha256,
                ln,
                p.judge_preset,
                song.fixed_ln(),
            );
            *self.files.borrow_mut() = ChartFiles {
                key,
                replay,
                texts: texts_of(song),
            };
        }
        self.files.borrow()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibebms_core::gauge::GaugeType;
    use vibebms_core::lane_option::LaneOption;

    #[test]
    fn what_a_play_would_keep() {
        let mut p = PlayConfig::default();
        let song = Song {
            mode: vibebms_core::PlayMode::Beat7K,
            note_kinds: [500, 0, 20, 0, 0],
            min_bpm: 150.0,
            max_bpm: 150.0,
            ..Default::default()
        };
        p.gauge = GaugeType::Hard;
        assert_eq!(outcome(&p, Some(&song)), (Keep::All, Some(ClearType::Hard)));
        // no mines, no BPM change: NO MINE and BPM GUIDE do nothing
        (p.no_mine, p.bpm_guide) = (true, true);
        assert_eq!(outcome(&p, Some(&song)).0, Keep::All);
        // ...without a chart they might
        assert_eq!(outcome(&p, None).0, Keep::LampOnly);
        p.random_1p = LaneOption::Random;
        assert_eq!(outcome(&p, Some(&song)).0, Keep::All);
        p.auto_scratch = true;
        let full = (Keep::LampOnly, Some(ClearType::AssistEasy));
        assert_eq!(outcome(&p, Some(&song)), full);
        p.autoplay = true;
        assert_eq!(outcome(&p, Some(&song)), (Keep::Nothing, None));
    }

    #[test]
    fn summaries() {
        let mut p = PlayConfig {
            hispeed_mode: HiSpeedMode::Floating,
            ..Default::default()
        };
        assert_eq!(assist_summary(&p), "OFF");
        assert_eq!(lane_cover_summary(&p), "OFF");
        p.no_mine = true;
        assert_eq!(assist_summary(&p), "NO MINE");
        p.mark_notes = true;
        assert_eq!(assist_summary(&p), "2 ON");
        (p.lane_cover, p.sudden, p.lift) = (true, 0.24, 0.1);
        assert_eq!(lane_cover_summary(&p), "24% · LIFT 10%");
    }

    #[test]
    fn folder_counts() {
        let mut lamps = [0u32; 11];
        (lamps[0], lamps[1], lamps[6]) = (3, 2, 5);
        assert_eq!(folder_facts(12, &(lamps, [0; 9])), [12, 10, 7, 5]);
    }
}
