//! Which plays may go to which IR, and as what: one pure function of the
//! play's record and the provider's gate. A play goes only under the rule
//! profile its rules exactly are; touch plays, web plays and plays an open
//! equivalence gap reaches never go to a third-party IR.

use crate::rule::{open_gap, profile_of, Profile};
use vibebms_core::record::{DeviceFacts, Platform, PlayRecord, VERSION};
use vibebms_core::score::{Assist, ClearType};
use vibebms_core::PlayMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    /// A server of vibeBMS's own protocol (`docs/IR_PROTOCOL.md`).
    VibeJson,
    Bokutachi,
    BmsIr,
    RianIr,
}

/// Why a play is not sent: the first gate it failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// An older record, or one without the client that made it.
    OldRecord,
    Autoplay,
    /// A replay watched.
    Replay,
    Versus,
    GhostBattle,
    Battle,
    CourseStage,
    Aborted,
    Assist,
    /// Its rules are no profile's.
    LocalRules,
    Touch,
    WebPlay,
    /// The provider takes no play of this profile in this key mode.
    NotTaken(Profile, PlayMode),
    /// An open gap of the equivalence list reaches it (its id).
    OpenGap(&'static str),
    /// Off in this build (`BOKUTACHI_BMS`).
    NotYet,
    /// The operator has not allowed vibeBMS to submit yet.
    OperatorPending,
    /// A `#RANDOM` / `#SWITCH` chart.
    RandomChart,
    /// Not every judged input from one kind of device (keyboard or
    /// controller), or none at all.
    MixedDevices,
    /// The play's real time does not fit the chart's length.
    WallTime,
}

/// A profile and the key modes a provider takes it in.
#[derive(Debug)]
pub struct Take {
    pub profile: Profile,
    pub modes: &'static [PlayMode],
    /// Switched on in this build.
    pub on: bool,
    /// Filed by device (Tachi's PMS: keyboard or controller).
    pub by_device: bool,
}

/// What a provider takes.
#[derive(Debug)]
pub struct Gate {
    /// Not vibeBMS's own: no touch play, nothing an open gap reaches.
    pub third_party: bool,
    pub takes: &'static [Take],
    /// Plays in the browser build.
    pub web: bool,
    /// Its operator allows vibeBMS to submit.
    pub live: bool,
    /// Charts with `#RANDOM` / `#SWITCH` (one hash, several patterns).
    pub random_charts: bool,
    /// The play's real time must fit the chart's length (rianIR's check).
    pub wall_time: bool,
}

/// Bokutachi BMS 7K/14K (vibe-LR2): off until the LR2 equivalence work
/// (IR-14) is done.
pub const BOKUTACHI_BMS: bool = false;

const BMS_MODES: &[PlayMode] = &[
    PlayMode::Beat5K,
    PlayMode::Beat7K,
    PlayMode::Beat10K,
    PlayMode::Beat14K,
];

const EVERY_PROFILE: &[Take] = &[
    Take {
        profile: Profile::VibeLr2,
        modes: &PlayMode::ALL,
        on: true,
        by_device: false,
    },
    Take {
        profile: Profile::VibeBeatoraja,
        modes: &PlayMode::ALL,
        on: true,
        by_device: false,
    },
];

/// Every play of a profile, labelled: the server ranks touch and web
/// plays as it likes.
pub const VIBE_JSON: Gate = Gate {
    third_party: false,
    takes: EVERY_PROFILE,
    web: true,
    live: true,
    random_charts: true,
    wall_time: false,
};

/// Tachi's `bms` 7K/14K under LR2 rules, `pms` (Keyboard / Controller)
/// under beatoraja rules; 5/10 keys have no Tachi playtype.
pub const BOKUTACHI: Gate = Gate {
    third_party: true,
    takes: &[
        Take {
            profile: Profile::VibeLr2,
            modes: &[PlayMode::Beat7K, PlayMode::Beat14K],
            on: BOKUTACHI_BMS,
            by_device: false,
        },
        Take {
            profile: Profile::VibeBeatoraja,
            modes: &[PlayMode::Popn9K],
            on: true,
            by_device: true,
        },
    ],
    web: false,
    live: true,
    random_charts: false,
    wall_time: false,
};

/// Two client kinds, as BMS-IR lists other players: vibe-LR2 and
/// vibe-beatoraja.
pub const BMS_IR: Gate = Gate {
    third_party: true,
    takes: &[
        Take {
            profile: Profile::VibeLr2,
            modes: BMS_MODES,
            on: true,
            by_device: false,
        },
        Take {
            profile: Profile::VibeBeatoraja,
            modes: &PlayMode::ALL,
            on: true,
            by_device: false,
        },
    ],
    web: false,
    live: false,
    random_charts: true,
    wall_time: false,
};

/// The profile is sent as the play's rules; what it takes is the
/// operator's to say.
pub const RIAN_IR: Gate = Gate {
    third_party: true,
    takes: EVERY_PROFILE,
    web: false,
    live: false,
    random_charts: true,
    wall_time: true,
};

impl ProviderKind {
    pub fn gate(self) -> &'static Gate {
        match self {
            ProviderKind::VibeJson => &VIBE_JSON,
            ProviderKind::Bokutachi => &BOKUTACHI,
            ProviderKind::BmsIr => &BMS_IR,
            ProviderKind::RianIr => &RIAN_IR,
        }
    }
}

/// Whether the play `rec` may go to provider `p`, and under which profile.
pub fn eligible(rec: &PlayRecord, p: ProviderKind) -> Result<Profile, Refusal> {
    passes(rec, p.gate())
}

/// `Err(r)` when `failed`.
fn gate(failed: bool, r: Refusal) -> Result<(), Refusal> {
    if failed {
        Err(r)
    } else {
        Ok(())
    }
}

fn passes(rec: &PlayRecord, g: &Gate) -> Result<Profile, Refusal> {
    let (f, d) = (&rec.flags, &rec.device);
    gate(
        rec.v != VERSION || rec.client.name.is_empty(),
        Refusal::OldRecord,
    )?;
    gate(f.autoplay, Refusal::Autoplay)?;
    gate(f.replay, Refusal::Replay)?;
    gate(f.versus, Refusal::Versus)?;
    gate(f.ghost_battle, Refusal::GhostBattle)?;
    gate(rec.rules.assist.battle, Refusal::Battle)?;
    gate(rec.course.is_some(), Refusal::CourseStage)?;
    gate(f.aborted, Refusal::Aborted)?;
    gate(rec.rules.assist.level() != Assist::None, Refusal::Assist)?;
    let profile = profile_of(rec).ok_or(Refusal::LocalRules)?;
    gate(g.third_party && (d.touch > 0 || d.touch_ui), Refusal::Touch)?;
    gate(!g.web && d.platform == Platform::Web, Refusal::WebPlay)?;
    let mode = rec.chart.mode;
    let take = g
        .takes
        .iter()
        .find(|t| t.profile == profile && t.modes.contains(&mode))
        .ok_or(Refusal::NotTaken(profile, mode))?;
    if let Some(gap) = open_gap(profile, rec).filter(|_| g.third_party) {
        return Err(Refusal::OpenGap(gap.id));
    }
    gate(!take.on, Refusal::NotYet)?;
    gate(!g.live, Refusal::OperatorPending)?;
    gate(
        !g.random_charts && rec.chart.has_random,
        Refusal::RandomChart,
    )?;
    gate(
        take.by_device && device_of(d).is_none(),
        Refusal::MixedDevices,
    )?;
    gate(g.wall_time && !wall_time_fits(rec), Refusal::WallTime)?;
    Ok(profile)
}

/// The one kind of device every judged input came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Keyboard,
    Controller,
}

/// The device a play is filed under (Tachi's PMS playtypes), `None`: both
/// kinds, or neither.
pub fn device_of(d: &DeviceFacts) -> Option<Device> {
    match (d.keyboard > 0, d.gamepad > 0) {
        (true, false) => Some(Device::Keyboard),
        (false, true) => Some(Device::Controller),
        _ => None,
    }
}

/// rianIR's check: real time between 0.85 x length - 5 s and 1.15 x length
/// + 15 s, unless the play failed or the chart has `#RANDOM`.
fn wall_time_fits(rec: &PlayRecord) -> bool {
    let len = rec.chart.length_us as f64 / 1000.0;
    let fits = (0.85 * len - 5000.0..=1.15 * len + 15000.0).contains(&(rec.wall_ms as f64));
    fits || rec.result.clear == ClearType::Failed || rec.chart.has_random
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::{Gap, Reach, EQUIVALENCE};
    use vibebms_core::chart::{JudgeRank, LnType};
    use vibebms_core::engine::JudgeAlgorithm;
    use vibebms_core::gauge::{GaugeAutoShift, GaugePreset, GaugeType};
    use vibebms_core::judge::JudgePreset;
    use vibebms_core::lane_option::LaneOption;
    use vibebms_core::record::{ClientId, CourseStage};
    use vibebms_core::rng::JavaRandom;

    /// A PMS keyboard play under beatoraja rules, cleared: Bokutachi's.
    fn base() -> PlayRecord {
        let mut r = PlayRecord {
            v: VERSION,
            client: ClientId {
                name: "vibeBMS".into(),
                ..Default::default()
            },
            ended_ms: 1_767_225_600_123,
            wall_ms: 120_000,
            ..Default::default()
        };
        r.chart.mode = PlayMode::Popn9K;
        r.chart.length_us = 120_000_000;
        r.chart.ln.undefined = true;
        r.device.keyboard = 900;
        r.device.platform = Platform::Linux;
        r.result.clear = ClearType::Normal;
        r
    }

    /// `base` under LR2 rules, 7 keys.
    fn lr2() -> PlayRecord {
        let mut r = base();
        r.chart.mode = PlayMode::Beat7K;
        r.rules.judge = JudgePreset::Lr2;
        r.rules.gauge_set = GaugePreset::Lr2;
        r
    }

    fn live(g: &Gate) -> Gate {
        Gate { live: true, ..*g }
    }

    #[test]
    fn the_base_plays_go() {
        let b = base();
        assert_eq!(
            eligible(&b, ProviderKind::Bokutachi),
            Ok(Profile::VibeBeatoraja)
        );
        assert_eq!(
            eligible(&b, ProviderKind::VibeJson),
            Ok(Profile::VibeBeatoraja)
        );
        assert_eq!(
            eligible(&lr2(), ProviderKind::VibeJson),
            Ok(Profile::VibeLr2)
        );
    }

    #[test]
    fn one_row_per_gate() {
        type Edit = fn(&mut PlayRecord);
        let bok = &BOKUTACHI;
        let rows: &[(&str, PlayRecord, Edit, &Gate, Refusal)] = &[
            ("old record", base(), |r| r.v = 0, bok, Refusal::OldRecord),
            (
                "no client",
                base(),
                |r| r.client.name.clear(),
                bok,
                Refusal::OldRecord,
            ),
            (
                "autoplay",
                base(),
                |r| r.flags.autoplay = true,
                bok,
                Refusal::Autoplay,
            ),
            (
                "replay",
                base(),
                |r| r.flags.replay = true,
                bok,
                Refusal::Replay,
            ),
            (
                "VS",
                base(),
                |r| r.flags.versus = true,
                bok,
                Refusal::Versus,
            ),
            (
                "G-BATTLE",
                base(),
                |r| r.flags.ghost_battle = true,
                bok,
                Refusal::GhostBattle,
            ),
            (
                "BATTLE",
                base(),
                |r| r.rules.assist.battle = true,
                bok,
                Refusal::Battle,
            ),
            (
                "course",
                base(),
                |r| r.course = Some(CourseStage::default()),
                bok,
                Refusal::CourseStage,
            ),
            (
                "quit",
                base(),
                |r| r.flags.aborted = true,
                bok,
                Refusal::Aborted,
            ),
            (
                "assist",
                base(),
                |r| r.rules.assist.legacy_note = true,
                bok,
                Refusal::Assist,
            ),
            (
                "light assist",
                base(),
                |r| r.rules.assist.lane = vec![LaneOption::HRandom],
                bok,
                Refusal::Assist,
            ),
            (
                "mixed presets",
                base(),
                |r| r.rules.gauge_set = GaugePreset::Lr2,
                bok,
                Refusal::LocalRules,
            ),
            (
                "IIDX judge",
                base(),
                |r| r.rules.judge = JudgePreset::Iidx,
                bok,
                Refusal::LocalRules,
            ),
            (
                "custom windows",
                base(),
                |r| r.rules.judge_rates = Some([50, 50, 100]),
                bok,
                Refusal::LocalRules,
            ),
            (
                "LR2 in CN",
                lr2(),
                |r| r.rules.ln_mode = LnType::Cn,
                &VIBE_JSON,
                Refusal::LocalRules,
            ),
            (
                "LR2, typed HCN",
                lr2(),
                |r| r.chart.ln.typed[LnType::Hcn as usize] = true,
                &VIBE_JSON,
                Refusal::LocalRules,
            ),
            (
                "LR2, #DEFEXRANK",
                lr2(),
                |r| r.chart.judge_rank = JudgeRank::DefExRank(100.0),
                &VIBE_JSON,
                Refusal::LocalRules,
            ),
            (
                "LR2, note choice",
                lr2(),
                |r| r.rules.algorithm = JudgeAlgorithm::Lowest,
                &VIBE_JSON,
                Refusal::LocalRules,
            ),
            (
                "LR2, GAS",
                lr2(),
                |r| r.rules.gas = GaugeAutoShift::BestClear,
                &VIBE_JSON,
                Refusal::LocalRules,
            ),
            (
                "LR2, PMS",
                lr2(),
                |r| r.chart.mode = PlayMode::Popn9K,
                &VIBE_JSON,
                Refusal::LocalRules,
            ),
            ("touch", base(), |r| r.device.touch = 1, bok, Refusal::Touch),
            (
                "touch controls shown",
                base(),
                |r| r.device.touch_ui = true,
                bok,
                Refusal::Touch,
            ),
            (
                "web",
                base(),
                |r| r.device.platform = Platform::Web,
                bok,
                Refusal::WebPlay,
            ),
            (
                "not taken",
                base(),
                |r| r.chart.mode = PlayMode::Beat5K,
                bok,
                Refusal::NotTaken(Profile::VibeBeatoraja, PlayMode::Beat5K),
            ),
            ("LR2 gap", lr2(), |_| {}, bok, Refusal::OpenGap("A1")),
            (
                "5-key EX-HARD",
                base(),
                |r| {
                    r.chart.mode = PlayMode::Beat5K;
                    r.rules.gauge = GaugeType::ExHard;
                },
                &BMS_IR,
                Refusal::OpenGap("B1"),
            ),
            (
                "charge BSS",
                base(),
                |r| {
                    r.chart.mode = PlayMode::Beat7K;
                    r.rules.charge_bss = true;
                },
                &BMS_IR,
                Refusal::OpenGap("B3"),
            ),
            (
                "operator",
                base(),
                |r| r.chart.mode = PlayMode::Beat7K,
                &BMS_IR,
                Refusal::OperatorPending,
            ),
            (
                "#RANDOM",
                base(),
                |r| r.chart.has_random = true,
                bok,
                Refusal::RandomChart,
            ),
            (
                "keyboard and controller",
                base(),
                |r| r.device.gamepad = 1,
                bok,
                Refusal::MixedDevices,
            ),
            (
                "no input",
                base(),
                |r| r.device.keyboard = 0,
                bok,
                Refusal::MixedDevices,
            ),
            (
                "too short",
                base(),
                |r| r.wall_ms = 90_000,
                &live(&RIAN_IR),
                Refusal::WallTime,
            ),
        ];
        for (name, rec, edit, g, want) in rows {
            let mut rec = rec.clone();
            edit(&mut rec);
            assert_eq!(passes(&rec, g), Err(*want), "{name}");
        }
        // the gates that pass on: vibe-json takes what third parties do not
        let mut touch = base();
        touch.device.touch = 3;
        touch.device.platform = Platform::Web;
        assert_eq!(
            eligible(&touch, ProviderKind::VibeJson),
            Ok(Profile::VibeBeatoraja)
        );
        let mut five = base();
        five.chart.mode = PlayMode::Beat5K;
        five.rules.gauge = GaugeType::ExHard;
        assert_eq!(
            eligible(&five, ProviderKind::VibeJson),
            Ok(Profile::VibeBeatoraja)
        );
        // rianIR forgives a failed play and a #RANDOM chart their time
        let mut failed = base();
        failed.wall_ms = 1000;
        failed.result.clear = ClearType::Failed;
        assert!(passes(&failed, &live(&RIAN_IR)).is_ok());
    }

    #[test]
    fn untyped_long_notes_only_ask_for_the_ln_mode() {
        let mut r = lr2();
        r.rules.ln_mode = LnType::Hcn;
        r.chart.ln.undefined = false;
        assert_eq!(profile_of(&r), Some(Profile::VibeLr2));
    }

    #[test]
    fn every_lr2_gap_is_open() {
        let lr2: Vec<&Gap> = EQUIVALENCE
            .iter()
            .filter(|g| g.profile == Profile::VibeLr2)
            .collect();
        let ids: Vec<&str> = lr2.iter().map(|g| g.id).collect();
        assert_eq!(ids, ["A1", "A2", "A3", "A4", "A5", "A6", "A7", "A8", "A9"]);
        assert!(lr2.iter().all(|g| !g.closed && g.reaches == Reach::Every));
    }

    /// A made-up record: any mix of rules, devices and flags.
    fn any(rng: &mut JavaRandom) -> PlayRecord {
        let mut pick = |n: usize| rng.next_int_bounded(n as i32) as usize;
        let mut r = base();
        r.chart.mode = PlayMode::ALL[pick(PlayMode::ALL.len())];
        r.chart.has_random = pick(4) == 0;
        r.chart.ln.undefined = pick(2) == 0;
        r.rules.judge = JudgePreset::ALL[pick(JudgePreset::ALL.len())];
        r.rules.gauge_set = GaugePreset::ALL[pick(GaugePreset::ALL.len())];
        r.rules.gauge = GaugeType::SELECTABLE[pick(GaugeType::SELECTABLE.len())];
        r.rules.gas = GaugeAutoShift::ALL[pick(GaugeAutoShift::ALL.len())];
        r.rules.ln_mode = [LnType::Ln, LnType::Cn, LnType::Hcn][pick(3)];
        r.rules.charge_bss = pick(4) == 0;
        r.device.keyboard = pick(3) as u32;
        r.device.gamepad = pick(3) as u32;
        r.device.touch = pick(2) as u32;
        r.device.touch_ui = pick(2) == 0;
        r.device.platform = [Platform::Windows, Platform::Web, Platform::Android][pick(3)];
        r
    }

    #[test]
    fn touch_and_iidx_plays_never_go_to_a_third_party() {
        let mut rng = JavaRandom::new(17);
        let mut sent = 0;
        for _ in 0..20_000 {
            let r = any(&mut rng);
            let touch = r.device.touch > 0 || r.device.touch_ui;
            let iidx = r.rules.judge == JudgePreset::Iidx || r.rules.gauge_set == GaugePreset::Iidx;
            for g in [&BOKUTACHI, &BMS_IR, &RIAN_IR] {
                let g = Gate {
                    takes: EVERY_PROFILE,
                    ..live(g)
                };
                let ok = passes(&r, &g).is_ok();
                assert!(!(ok && (touch || iidx)), "{r:?}");
                sent += ok as u32;
            }
        }
        // (the sweep reaches plays that do go)
        assert!(sent > 0);
    }
}
