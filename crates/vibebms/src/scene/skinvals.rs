//! Skin values several screens share (LR2 / beatoraja ids): a chart's
//! options, numbers and texts, a play's score, a chart's record, the play
//! settings' buttons, the clock and the player's totals. Each screen
//! answers the ids it has from these.

use crate::app::Ctx;
use crate::config::{Bpms, HiSpeedMode, PlayConfig};
use crate::library::{BestScore, Song};
use crate::platform::Instant;
use vibebms_core::chart::{ChartMeta, JudgeRank};
use vibebms_core::gauge::{GaugeAutoShift, GaugeType};
use vibebms_core::judge::{Judge, JudgePreset};
use vibebms_core::lane_option::LaneOption;
use vibebms_core::score::{ClearType, DjLevel, Score};
use vibebms_core::{LnType, PlayMode};

/// What the chart ids ask of a chart: from the library (`Song`) or the
/// chart itself (`ChartMeta`).
pub struct ChartFacts<'a> {
    pub difficulty: i32,
    pub mode: Option<PlayMode>,
    pub has_bga: bool,
    pub has_ln: bool,
    pub has_random: bool,
    pub has_stop: bool,
    pub min_bpm: f64,
    pub max_bpm: f64,
    pub main_bpm: f64,
    /// `#PLAYLEVEL` as a number.
    pub level: Option<i64>,
    /// The judge rank number (400).
    pub judge: Option<i64>,
    pub stagefile: &'a str,
    pub banner: &'a str,
    /// (The library does not keep it.)
    pub backbmp: Option<&'a str>,
}

impl ChartFacts<'_> {
    pub fn of_song(s: &Song) -> ChartFacts<'_> {
        ChartFacts {
            difficulty: s.difficulty,
            mode: Some(s.mode),
            has_bga: s.has_bga,
            has_ln: s.has_ln,
            has_random: s.has_random,
            has_stop: s.has_stop,
            min_bpm: s.min_bpm,
            max_bpm: s.max_bpm,
            main_bpm: s.main_bpm,
            level: Some(s.level_num()),
            judge: s.judge,
            stagefile: &s.stagefile,
            banner: &s.banner,
            backbmp: None,
        }
    }

    pub fn of_meta(m: &ChartMeta, mode: PlayMode) -> ChartFacts<'_> {
        ChartFacts {
            difficulty: m.difficulty,
            mode: Some(mode),
            has_bga: m.has_bga,
            has_ln: m.has_long,
            has_random: m.has_random,
            has_stop: m.has_stop,
            min_bpm: m.min_bpm,
            max_bpm: m.max_bpm,
            main_bpm: m.main_bpm,
            level: m.level.trim().parse::<f64>().ok().map(|v| v as i64),
            judge: Some(judge_rank_number(m)),
            stagefile: &m.stagefile,
            banner: &m.banner,
            backbmp: Some(&m.backbmp),
        }
    }
}

/// Chart options: difficulty 150-155, mode 160-164, BGA, LN, BPM change,
/// random, judge rank 180-184, images 190-195, stops 1177.
pub fn chart_option(f: &ChartFacts, id: i32) -> Option<bool> {
    use PlayMode::*;
    Some(match id {
        150..=155 => f.difficulty == id - 150,
        160..=164 => {
            f.mode == Some([Beat7K, Beat5K, Beat14K, Beat10K, Popn9K][(id - 160) as usize])
        }
        170 => !f.has_bga,
        171 => f.has_bga,
        172 => !f.has_ln,
        173 => f.has_ln,
        // (BPMs within 0.01 are one BPM)
        176 => (f.max_bpm - f.min_bpm).abs() < 0.01,
        177 => (f.max_bpm - f.min_bpm).abs() >= 0.01,
        178 => !f.has_random,
        179 => f.has_random,
        180..=184 => f.judge.is_some_and(|j| judge_option(j, id)),
        190 => f.stagefile.is_empty(),
        191 => !f.stagefile.is_empty(),
        192 => f.banner.is_empty(),
        193 => !f.banner.is_empty(),
        194 => f.backbmp?.is_empty(),
        195 => !f.backbmp?.is_empty(),
        1177 => f.has_stop,
        _ => return None,
    })
}

/// Chart numbers: BPMs 90-92, level 96, judge rank 400.
pub fn chart_number(f: &ChartFacts, id: i32) -> Option<i64> {
    Some(match id {
        90 => f.max_bpm.round() as i64,
        91 => f.min_bpm.round() as i64,
        92 => f.main_bpm.round() as i64,
        96 => f.level?,
        400 => f.judge?,
        _ => return None,
    })
}

/// beatoraja's chart numbers from the library: notes by kind (350-354),
/// densities (360-365) and TOTAL (368).
pub fn song_number(s: &Song, id: i32) -> Option<i64> {
    let known = s.note_kinds.iter().any(|n| *n > 0);
    let split = |v: f64, after: bool| {
        if after {
            ((v * 100.0) as i64) % 100
        } else {
            v as i64
        }
    };
    Some(match id {
        350..=354 if known => s.note_kinds[(id - 350) as usize],
        360 | 361 if known => split(s.density[1], id == 361),
        362 | 363 if known => split(s.density[2], id == 363),
        364 | 365 if known => split(s.density[0], id == 365),
        368 => s.total as i64,
        _ => return None,
    })
}

/// A chart's texts.
pub struct ChartText<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub genre: &'a str,
    pub artist: &'a str,
    pub subartist: &'a str,
    pub md5: &'a str,
    pub sha256: &'a str,
}

impl<'a> ChartText<'a> {
    pub fn of_song(s: &'a Song) -> ChartText<'a> {
        ChartText {
            title: &s.title,
            subtitle: &s.subtitle,
            genre: &s.genre,
            artist: &s.artist,
            subartist: &s.subartist,
            md5: &s.md5,
            sha256: &s.sha256,
        }
    }

    /// The chart's own texts; the library's hashes.
    pub fn of_meta(m: &'a ChartMeta, s: &'a Song) -> ChartText<'a> {
        ChartText {
            title: &m.title,
            subtitle: &m.subtitle,
            genre: &m.genre,
            artist: &m.artist,
            subartist: &m.subartist,
            md5: &s.md5,
            sha256: &s.sha256,
        }
    }
}

/// Chart texts 10-16 (title, subtitle, both, genre, artist, subartist,
/// both) and the hashes 1030 / 1031.
pub fn chart_text(t: &ChartText, id: i32) -> Option<String> {
    let both = |a: &str, b: &str| format!("{a} {b}").trim().to_string();
    Some(match id {
        10 => t.title.into(),
        11 => t.subtitle.into(),
        12 => both(t.title, t.subtitle),
        13 => t.genre.into(),
        14 => t.artist.into(),
        15 => t.subartist.into(),
        16 => both(t.artist, t.subartist),
        1030 => t.md5.into(),
        1031 => t.sha256.into(),
        _ => return None,
    })
}

/// The first difficulty table that lists a chart: (its name, the chart's
/// level label), as beatoraja finds one for a chart not chosen from a
/// table.
pub fn table_of(ctx: &Ctx, md5: &str, sha256: &str) -> Option<(String, String)> {
    let same = |a: &str, b: &str| !a.is_empty() && a.eq_ignore_ascii_case(b);
    ctx.tables.list.iter().find_map(|t| {
        let e = t
            .entries
            .iter()
            .find(|e| same(&e.md5, md5) || same(&e.sha256, sha256))?;
        Some((t.header.name.clone(), t.label(&e.level)))
    })
}

/// beatoraja's table texts: 1001 name, 1002 level, 1003 level then name.
pub fn table_text((name, level): &(String, String), id: i32) -> Option<String> {
    Some(match id {
        1001 => name.clone(),
        1002 => level.clone(),
        1003 => format!("{level}{name}"),
        _ => return None,
    })
}

/// A play's own score: EX score, notes, combo, judgements (fast / slow),
/// combo breaks, rate 115 / 116 (155 / 156).
pub fn score_number(sc: &Score, id: i32) -> Option<i64> {
    let c = |j: Judge| sc.count(j) as i64;
    Some(match id {
        71 | 101 | 171 => sc.ex_score() as i64,
        72 => sc.max_ex() as i64,
        74 | 106 => sc.total_notes as i64,
        75 | 105 | 174 => sc.max_combo as i64,
        110 => c(Judge::PGreat),
        111 => c(Judge::Great),
        112 => c(Judge::Good),
        113 => c(Judge::Bad),
        115 | 155 => percent_parts(sc.rate()).0,
        116 | 156 => percent_parts(sc.rate()).1,
        410..=419 => sc.counts[((id - 410) / 2) as usize][((id - 410) % 2) as usize] as i64,
        420 => c(Judge::Miss),
        421 | 422 => sc.counts[Judge::Miss as usize][(id - 421) as usize] as i64,
        423 => sc.fast() as i64,
        424 => sc.slow() as i64,
        425 => sc.combo_breaks as i64,
        426 => c(Judge::Poor) + c(Judge::Miss),
        427 => c(Judge::Bad) + c(Judge::Poor) + c(Judge::Miss),
        _ => return None,
    })
}

/// A chart's record (0 for none): EX score, combo, miss count, plays,
/// clears, fails, judgements, rate and lamp.
pub fn best_number(b: Option<&BestScore>, id: i32) -> Option<i64> {
    let v = |f: fn(&BestScore) -> i64| b.map_or(0, f);
    Some(match id {
        71 | 150 | 170 => v(|b| b.ex_score as i64),
        75 | 173 => v(|b| b.max_combo as i64),
        76 | 176 => v(|b| {
            if b.min_bp == u32::MAX {
                0
            } else {
                b.min_bp as i64
            }
        }),
        77 => v(|b| b.play_count as i64),
        78 => v(|b| b.clear_count as i64),
        79 => v(|b| b.play_count.saturating_sub(b.clear_count) as i64),
        80..=84 => b.map_or(0, |b| b.judges[(id - 80) as usize] as i64),
        102 | 122 | 157 | 183 => percent_parts(b.map_or(0.0, BestScore::rate)).0,
        103 | 123 | 158 | 184 => percent_parts(b.map_or(0.0, BestScore::rate)).1,
        370 | 371 => v(|b| b.clear as i64),
        _ => return None,
    })
}

/// A rate (0..1) as a percentage's whole part and two decimals.
pub fn percent_parts(rate: f64) -> (i64, i64) {
    decimals2(rate * 100.0)
}

/// `v` as its whole part and two decimals (rounded).
pub fn decimals2(v: f64) -> (i64, i64) {
    let x = (v * 100.0).round() as i64;
    (x / 100, (x % 100).abs())
}

/// LR2's score (200,000 for all PGREAT and the full combo).
pub fn lr2_score(sc: &Score) -> i64 {
    let n = sc.total_notes.max(1) as i64;
    (sc.count(Judge::PGreat) as i64 * 150_000
        + sc.count(Judge::Great) as i64 * 100_000
        + sc.count(Judge::Good) as i64 * 20_000)
        / n
        + sc.max_combo as i64 * 50_000 / n
}

/// The EX scores of the DJ levels A to AAA (2/9 .. 9/9 of `max`).
pub fn rank_borders(max: i64) -> impl Iterator<Item = i64> {
    (2..=9).map(move |k| (max * k + 8) / 9)
}

/// The play settings shown as options: autoplay 32 / 33, BGA 40 / 41,
/// gauge 42 / 43 (groove / survival), EX-HARD 1046.
pub fn play_option(autoplay: bool, bga: bool, gauge: GaugeType, id: i32) -> Option<bool> {
    Some(match id {
        32 => !autoplay,
        33 => autoplay,
        40 => !bga,
        41 => bga,
        42 => !gauge.is_survival(),
        43 => gauge.is_survival(),
        1046 => matches!(gauge, GaugeType::ExHard | GaugeType::ExHardClass),
        _ => return None,
    })
}

/// The gauge in use (vibeBMS ids 118-125).
pub fn gauge_type_option(g: GaugeType, id: i32) -> Option<bool> {
    Some(match id {
        118 => g == GaugeType::Normal,
        119 => matches!(g, GaugeType::Hard | GaugeType::Class),
        120 => g == GaugeType::Hazard,
        121 => g == GaugeType::Easy,
        124 => g == GaugeType::AssistEasy,
        125 => matches!(
            g,
            GaugeType::ExHard | GaugeType::ExHardClass | GaugeType::ExClass
        ),
        _ => return None,
    })
}

/// The hi-speed numbers at a chart's BPMs: hi-speed 10 (310 / 311 its
/// whole and decimal parts), the notes' duration 312 and green number 313.
pub fn speed_number(p: &PlayConfig, bpms: &Bpms, id: i32) -> Option<i64> {
    let hs = || (p.hispeed_at(bpms) * 100.0).round() as i64;
    Some(match id {
        10 => hs(),
        310 => hs() / 100,
        311 => hs() % 100,
        312 => p.duration_ms(bpms).round() as i64,
        313 => p.green(bpms),
        _ => return None,
    })
}

/// beatoraja's judge rank options (180 very hard .. 184 very easy) of a
/// judge rank number: `#RANK`, or a percentage in steps of 25 from 10.
pub fn judge_option(judge: i64, id: i32) -> bool {
    let k = (id - 180) as i64;
    judge == k || (judge >= 10 && ((judge - 10) / 25).min(4) == k)
}

/// beatoraja's judge rank number (400): `#RANK`, else the percentage.
pub fn judge_rank_number(m: &ChartMeta) -> i64 {
    match m.judge_rank {
        JudgeRank::Rank(r) => r as i64,
        JudgeRank::NoNumber => 2,
        JudgeRank::DefExRank(v) | JudgeRank::Bmson(v) => v.round() as i64,
    }
}

/// LR2 lamp options 100-105 (exactly one holds: lamps LR2 lacks count as
/// the nearest one) and beatoraja's exact 1100-1104.
pub fn lamp_option(c: ClearType, id: i32) -> Option<bool> {
    use ClearType as C;
    Some(match id {
        100 => c == C::NoPlay,
        101 => c == C::Failed,
        102 => matches!(c, C::AssistEasy | C::LightAssistEasy | C::Easy),
        103 => c == C::Normal,
        104 => matches!(c, C::Hard | C::ExHard),
        105 => matches!(c, C::FullCombo | C::Perfect | C::Max),
        1100 => c == C::AssistEasy,
        1101 => c == C::LightAssistEasy,
        1102 => c == C::ExHard,
        1103 => c == C::Perfect,
        1104 => c == C::Max,
        _ => return None,
    })
}

/// Index of a DJ level in LR2 / beatoraja rank options (0 = AAA ... 7 = F).
pub fn rank_index(l: DjLevel) -> i32 {
    7 - l as i32
}

/// beatoraja option indices.
fn random_index(o: LaneOption) -> i64 {
    match o {
        LaneOption::Off => 0,
        LaneOption::Mirror => 1,
        LaneOption::Random => 2,
        LaneOption::RRandom => 3,
        LaneOption::SRandom => 4,
        LaneOption::Spiral => 5,
        LaneOption::HRandom => 6,
        LaneOption::AllScratch => 7,
        LaneOption::RandomEx => 8,
        LaneOption::SRandomEx => 9,
    }
}

/// beatoraja button values (image set indices) of the play settings,
/// shown on every screen.
pub fn config_button(ctx: &Ctx, id: i32) -> Option<i64> {
    use super::option_panel::index_of;
    let p = &ctx.config.play;
    Some(match id {
        40 => gauge_index(p.gauge),
        42 => random_index(p.random_1p),
        43 => random_index(p.random_2p),
        54 => super::option_panel::dp_index(p),
        // beatoraja HS-FIX: off, start, max, main, min BPM
        55 => p.hs_fix as i64,
        // beatoraja BGA: 0 on, 2 off
        72 => {
            if p.bga {
                0
            } else {
                2
            }
        }
        // beatoraja assist buttons vibeBMS has (0 off, 1 on)
        301 => p.expand_judge as i64,
        302 => (p.hispeed_mode == HiSpeedMode::Constant) as i64,
        303 => p.judge_area as i64,
        304 => p.legacy_note as i64,
        305 => p.mark_notes as i64,
        306 => p.bpm_guide as i64,
        307 => p.no_mine as i64,
        75 => p.timing_auto_adjust as i64,
        330 => p.lane_cover as i64,
        331 => (p.lift > 0.0) as i64,
        332 => (p.hidden > 0.0) as i64,
        340 => p.judge_algorithm as i64,
        351 => p.no_mine as i64,
        352 => (p.hispeed_mode == HiSpeedMode::Constant) as i64,
        353 => p.legacy_note as i64,
        78 => index_of(&GaugeAutoShift::ALL, p.gauge_auto_shift),
        // vibeBMS: the options its panels show that beatoraja has not
        1401 => p.auto_scratch as i64,
        1402 => p.lane_cover as i64,
        1403 => index_of(&JudgePreset::ALL, p.judge_preset),
        1404 => index_of(&GaugeAutoShift::ALL, p.gauge_auto_shift),
        1405 => index_of(&crate::config::Target::ALL, p.target),
        // TARGET as LR2 skins for beatoraja draw it (LITONE5): A- .. AAA+,
        // MAX, NEXT (an older list, without MAX-)
        77 => {
            use crate::config::Target;
            match p.target {
                Target::RateMaxMinus | Target::Max => 9,
                Target::RankNext => 10,
                Target::Best => 11,
                t => index_of(&Target::ALL, t),
            }
        }
        308 => match p.ln_mode {
            LnType::Ln => 0,
            LnType::Cn => 1,
            LnType::Hcn => 2,
        },
        _ => return None,
    })
}

/// beatoraja's gauge index (button 40).
pub fn gauge_index(g: GaugeType) -> i64 {
    match g {
        GaugeType::AssistEasy => 0,
        GaugeType::Easy => 1,
        GaugeType::Normal => 2,
        GaugeType::Hard | GaugeType::Class => 3,
        GaugeType::ExHard | GaugeType::ExHardClass | GaugeType::ExClass => 4,
        GaugeType::Hazard => 5,
    }
}

/// Numbers every screen has: the local date and time (LR2 21-26: year,
/// month, day, hour, minute, second).
pub fn clock_number(id: i32) -> Option<i64> {
    (21..=26).contains(&id).then(|| Clock::now().number(id))?
}

/// The local date and time, read once for a frame's clock numbers.
pub struct Clock([i64; 6]);

impl Clock {
    pub fn now() -> Clock {
        use chrono::{Datelike, Timelike};
        let t = chrono::Local::now();
        Clock([
            t.year() as i64,
            t.month() as i64,
            t.day() as i64,
            t.hour() as i64,
            t.minute() as i64,
            t.second() as i64,
        ])
    }

    /// LR2 numbers 21-26 (see [`clock_number`]).
    pub fn number(&self, id: i32) -> Option<i64> {
        let i = usize::try_from(id - 21).ok()?;
        self.0.get(i).copied()
    }
}

/// The player's totals over all charts (beatoraja `PlayerData`; LR2 has
/// 30-37 too): time played 17-19 (h, min, s), plays, clears and fails
/// 30-32, PG / GR / GD / BD / POOR 33-37 (POOR without empty POORs) and
/// notes hit (PG to BD) 333.
pub fn player_number(ctx: &Ctx, id: i32) -> Option<i64> {
    let sum =
        |f: fn(&BestScore) -> u32| -> i64 { ctx.lib.scores.values().map(|b| f(b) as i64).sum() };
    let totals = || ctx.lib.db.as_ref().map(|l| l.totals());
    let secs = || totals().map_or(0, |t| t.play_ms / 1000) as i64;
    Some(match id {
        17 => secs() / 3600,
        18 => secs() / 60 % 60,
        19 => secs() % 60,
        30 => sum(|b| b.play_count),
        31 => sum(|b| b.clear_count),
        32 => sum(|b| b.play_count.saturating_sub(b.clear_count)),
        33..=37 => totals().map_or(0, |t| t.judge((id - 33) as usize)) as i64,
        333 => totals().map_or(0, |t| t.notes()) as i64,
        _ => return None,
    })
}

/// Elapsed ms since `t0` (negative before it).
pub fn ms_since(t0: Instant, now: Instant) -> i64 {
    if now >= t0 {
        now.duration_since(t0).as_millis() as i64
    } else {
        -(t0.duration_since(now).as_millis() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages() {
        assert_eq!(percent_parts(0.75), (75, 0));
        assert_eq!(percent_parts(0.123456), (12, 35));
        // rounded as a whole: 99.996 % is 100.00, not 99.00
        assert_eq!(percent_parts(0.99996), (100, 0));
        assert_eq!(decimals2(-1.25), (-1, 25));
    }

    #[test]
    fn a_library_record_and_its_chart_say_the_same() {
        let m = ChartMeta {
            difficulty: 3,
            min_bpm: 150.0,
            max_bpm: 150.004,
            main_bpm: 150.0,
            level: "12".into(),
            has_long: true,
            stagefile: "s.png".into(),
            ..ChartMeta::default()
        };
        let s = Song {
            difficulty: 3,
            mode: PlayMode::Beat7K,
            min_bpm: 150.0,
            max_bpm: 150.004,
            main_bpm: 150.0,
            level: "12".into(),
            has_ln: true,
            stagefile: "s.png".into(),
            judge: Some(judge_rank_number(&m)),
            ..Song::default()
        };
        let (a, b) = (
            ChartFacts::of_song(&s),
            ChartFacts::of_meta(&m, PlayMode::Beat7K),
        );
        for id in (150..=195).chain([1177]) {
            let backbmp = (194..=195).contains(&id);
            assert_eq!(
                chart_option(&a, id),
                chart_option(&b, id).filter(|_| !backbmp)
            );
        }
        for id in [90, 91, 92, 96, 400] {
            assert_eq!(chart_number(&a, id), chart_number(&b, id));
        }
        // a BPM that only rounding changes is no BPM change
        assert_eq!(chart_option(&a, 176), Some(true));
    }
}
