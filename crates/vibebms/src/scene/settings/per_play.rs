//! The options a player changes between songs: they live in the song
//! select's panels (PLAY OPTION, ASSIST, SYSTEM OPTION) and its tabs, and
//! the settings' search points to them.

/// An option that lives in the song select: what search finds for it.
pub(super) struct Moved {
    pub label: &'static str,
    pub keys: &'static str,
    /// Where it is on the song select.
    pub place: &'static str,
}

pub(super) const MOVED: &[Moved] = &[
    Moved {
        label: tr_key!("Random"),
        keys: "random mirror s-random h-random r-random lane option 1p 2p",
        place: tr_key!("PLAY OPTION keys 1 / 2 (2P: 6 / 7), hold START"),
    },
    Moved {
        label: tr_key!("DP option and battle"),
        keys: "flip double play battle sp both sides",
        place: tr_key!("PLAY OPTION key 4 (DP OPTION · BATTLE)"),
    },
    Moved {
        label: tr_key!("Play side"),
        keys: "1p 2p side left right",
        place: tr_key!("PLAY OPTION › MORE (F6)"),
    },
    Moved {
        label: tr_key!("Hi-speed and HS-FIX"),
        keys: "hispeed hi speed green number gn scroll speed hs-fix floating",
        place: tr_key!("PLAY OPTION › MORE and key 5 (F7 / F8)"),
    },
    Moved {
        label: tr_key!("Lane cover, SUDDEN+, HIDDEN+, LIFT"),
        keys: "lanecover sudden hidden lift white number",
        place: tr_key!("PLAY OPTION › MORE › LANE COVER; in play, the usual cover keys"),
    },
    Moved {
        label: tr_key!("Target score"),
        keys: "target pacemaker rival ghost",
        place: tr_key!("PLAY OPTION, the turntable"),
    },
    Moved {
        label: tr_key!("Gauge"),
        keys: "gauge groove easy hard ex-hard hazard assist",
        place: tr_key!("PLAY OPTION key 3 (F3)"),
    },
    Moved {
        label: tr_key!("Gauge auto shift"),
        keys: "gas gauge auto shift best clear",
        place: tr_key!("SYSTEM OPTION key 2, hold START + SELECT"),
    },
    Moved {
        label: tr_key!("Assist options"),
        keys: "auto scratch legacy note mines no mine expand judge judge area mark note bpm guide \
               scroll remove mine add longnote add ln add chart modifier",
        place: tr_key!("ASSIST, hold SELECT"),
    },
    Moved {
        label: tr_key!("Random trainer"),
        keys: "random trainer lane order fixed",
        place: tr_key!(
            "RANDOM: TRAINER (PLAY OPTION keys 1 / 2); its order in GAMEPLAY › ADVANCED"
        ),
    },
    Moved {
        label: tr_key!("FREQ"),
        keys: "freq pitch speed semitone tempo rate faster slower",
        place: tr_key!("ASSIST, hold SELECT and turn the turntable"),
    },
    Moved {
        label: tr_key!("Autoplay"),
        keys: "auto play demo",
        place: tr_key!("the AUTOPLAY button, or F5"),
    },
    Moved {
        label: tr_key!("BGA"),
        keys: "bga movie video background animation",
        place: tr_key!("SYSTEM OPTION key 1"),
    },
    Moved {
        label: tr_key!("FAST / SLOW display"),
        keys: "fast slow timing display early late",
        place: tr_key!("SYSTEM OPTION"),
    },
    Moved {
        label: tr_key!("Sort"),
        keys: "sort order title level clear score",
        place: tr_key!("the SORT tab"),
    },
    Moved {
        label: tr_key!("Key mode filter"),
        keys: "filter keys 7keys 14keys 5keys",
        place: tr_key!("the KEYS tab"),
    },
    Moved {
        label: tr_key!("Difficulty filter"),
        keys: "filter difficulty beginner normal hyper another insane",
        place: tr_key!("the DIFFICULTY tab"),
    },
    Moved {
        label: tr_key!("Random select"),
        keys: "random select pick not clear not hard",
        place: tr_key!("RANDOM SELECT, the bar at the top of each folder"),
    },
];
