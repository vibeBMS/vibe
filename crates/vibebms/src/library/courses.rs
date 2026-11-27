//! The player's courses (`docs/LR2_COURSES.md`): made in the settings'
//! course editor, by RANDOM MIX, or imported from LR2's `song.db`; kept in
//! `<root>/scores/courses.json`. The difficulty tables' dan courses are
//! played as class courses beside them.

use crate::library::charts::Charts;
use crate::library::{RecordKey, Song};
use crate::platform::vfs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use vibebms_core::course::{CourseDef, CourseKind, Join, MAX_STAGES};
use vibebms_core::rng::JavaRandom;
use vibebms_core::PlayMode;

/// A stage: its chart's MD5 (the course's identity), and what is known of
/// it to find and show it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stage {
    pub md5: String,
    pub sha256: String,
    pub title: String,
}

impl Stage {
    pub fn of(s: &Song) -> Stage {
        Stage {
            md5: s.md5.clone(),
            sha256: s.sha256.clone(),
            title: s.full_title(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Course {
    /// Its place in the list (newest, the highest, first), as LR2's `id`.
    pub id: u64,
    pub title: String,
    pub kind: CourseKind,
    /// NONSTOP: the join after each stage but the last.
    pub joins: Vec<Join>,
    pub stages: Vec<Stage>,
    /// One of LR2's internet-ranking courses (read-only in LR2).
    pub ir: bool,
}

impl Course {
    /// What makes the course (and its identity, `CourseDef::id`).
    pub fn def(&self) -> CourseDef {
        let n = self.stages.len();
        CourseDef {
            kind: self.kind,
            joins: match self.kind {
                CourseKind::Nonstop => (0..n.saturating_sub(1))
                    .map(|k| self.joins.get(k).copied().unwrap_or_default())
                    .collect(),
                _ => Vec::new(),
            },
            stages: self.stages.iter().map(|s| s.md5.to_lowercase()).collect(),
        }
    }
}

/// RANDOM MIX's settings (LR2's `<course>` settings, S-OLR2b 2.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RandomMix {
    /// Stages; outside 1 to 5 a random count of 2 to 5.
    pub stages: u32,
    /// Level limits (0: none).
    pub min_level: i64,
    pub max_level: i64,
    /// BPM limits (0: none).
    pub min_bpm: f64,
    pub max_bpm: f64,
    /// Above 0: the first chart has one BPM, the others within this of it.
    pub bpm_range: f64,
    /// From 1: each chart its song's chart nearest this level.
    pub optimum_level: i64,
    /// NONSTOP's joins.
    pub join: Join,
    /// The key mode its charts are of (LR2: the song select's filter).
    #[serde(with = "vibebms_core::by_label")]
    pub mode: PlayMode,
}

impl Default for RandomMix {
    fn default() -> Self {
        RandomMix {
            stages: 4,
            min_level: 0,
            max_level: 0,
            min_bpm: 0.0,
            max_bpm: 0.0,
            bpm_range: 10.0,
            optimum_level: 0,
            join: Join::Fade,
            mode: PlayMode::Beat7K,
        }
    }
}

impl RandomMix {
    pub fn is_default(&self) -> bool {
        *self == RandomMix::default()
    }
}

/// The title RANDOM MIX gives the course it makes (replacing the last).
pub const RANDOM_MIX: &str = "RANDOM MIX";

#[derive(Default)]
pub struct CourseStore {
    file: PathBuf,
    pub list: Vec<Course>,
    /// The course open in the editor (the song select's C adds to it).
    pub editing: Option<u64>,
}

impl CourseStore {
    pub fn open(dir: &Path) -> CourseStore {
        let file = dir.join("courses.json");
        let list = vfs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        CourseStore {
            file,
            list,
            editing: None,
        }
    }

    pub fn save(&self) {
        match serde_json::to_vec_pretty(&self.list) {
            Ok(j) => {
                if let Err(e) = vfs::write(&self.file, &j) {
                    log::error!("saving courses: {e}");
                }
            }
            Err(e) => log::error!("saving courses: {e}"),
        }
    }

    /// The courses of `kind`, newest first.
    pub fn of_kind(&self, kind: CourseKind) -> Vec<&Course> {
        let mut v: Vec<&Course> = self.list.iter().filter(|c| c.kind == kind).collect();
        v.sort_by_key(|c| std::cmp::Reverse(c.id));
        v
    }

    pub fn get(&self, id: u64) -> Option<&Course> {
        self.list.iter().find(|c| c.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Course> {
        self.list.iter_mut().find(|c| c.id == id)
    }

    fn next_id(&self) -> u64 {
        self.list.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }

    /// A new course, first in the list: its id.
    pub fn add(&mut self, mut c: Course) -> u64 {
        c.id = self.next_id();
        let id = c.id;
        self.list.push(c);
        self.save();
        id
    }

    pub fn remove(&mut self, id: u64) {
        self.list.retain(|c| c.id != id);
        self.save();
    }

    /// Move course `id` `d` places along its kind's list (-1: up), by
    /// swapping ids with the course there.
    pub fn shift(&mut self, id: u64, d: i32) {
        let Some(kind) = self.get(id).map(|c| c.kind) else {
            return;
        };
        let ids: Vec<u64> = self.of_kind(kind).iter().map(|c| c.id).collect();
        let Some(i) = ids.iter().position(|x| *x == id) else {
            return;
        };
        let Some(&other) = ids.get((i as i64 + d as i64).max(0) as usize) else {
            return;
        };
        for c in &mut self.list {
            if c.id == id {
                c.id = other;
            } else if c.id == other {
                c.id = id;
            }
        }
        self.save();
    }

    /// Add LR2's courses (`lr2db::courses`, newest first) that are not
    /// here yet (by identity): how many were added.
    pub fn import(
        &mut self,
        courses: Vec<crate::library::lr2db::Lr2Course>,
        songs: &[Song],
    ) -> usize {
        let have: Vec<String> = self.list.iter().map(|c| c.def().id()).collect();
        let charts = Charts::new(songs);
        let mut added = 0;
        for c in courses.into_iter().rev() {
            if have.contains(&c.def.id()) {
                continue;
            }
            let stages = c
                .def
                .stages
                .iter()
                .map(|md5| match charts.by_md5(songs, md5, "", "") {
                    Some(i) => Stage::of(&songs[i]),
                    None => Stage {
                        md5: md5.clone(),
                        ..Default::default()
                    },
                })
                .collect();
            let id = self.next_id();
            self.list.push(Course {
                id,
                title: c.title,
                kind: c.def.kind,
                joins: c.def.joins,
                stages,
                ir: c.ir,
            });
            added += 1;
        }
        if added > 0 {
            self.save();
        }
        added
    }

    /// RANDOM MIX (S-OLR2b 2.5): a course of `kind` (expert or NONSTOP)
    /// drawn from the library's charts of key mode `mode` within `rules`,
    /// in place of the last one: its id.
    pub fn random_mix(
        &mut self,
        kind: CourseKind,
        rules: &RandomMix,
        songs: &[Song],
        charts: &Charts,
        mode: PlayMode,
        seed: i64,
    ) -> Result<u64, String> {
        let stages = mix(rules, songs, charts, mode, seed);
        if stages.is_empty() {
            return Err("RANDOM MIX: no chart fits its settings".into());
        }
        self.list
            .retain(|c| !(c.kind == kind && c.title == RANDOM_MIX));
        let joins = vec![rules.join; stages.len().saturating_sub(1)];
        Ok(self.add(Course {
            id: 0,
            title: RANDOM_MIX.into(),
            kind,
            joins: if kind == CourseKind::Nonstop {
                joins
            } else {
                Vec::new()
            },
            stages: stages.iter().map(|s| Stage::of(s)).collect(),
            ir: false,
        }))
    }
}

/// A course's record (a NONSTOP play's, an expert or class course's
/// result), `key` its identity (`CourseDef::id`) with the play's LN mode
/// and rule mode, kept as a song's (S-OLR2b 3.8, 4.4): the best EX score
/// with its counts (strictly higher), the lamp only raised, max combo, min
/// BP and CB, play and clear counts. (The one place course records are
/// written.)
pub fn save_course(
    lib: &mut crate::library::Library,
    key: &RecordKey,
    s: &vibebms_core::score::Score,
    clear: vibebms_core::score::ClearType,
    play_ms: u64,
) -> crate::library::BestScore {
    lib.record_play(key, s, clear, play_ms)
}

/// A course's stage into its chart's own record `key`, partly
/// (`Library::record_stage`).
pub fn save_stage(
    lib: &mut crate::library::Library,
    key: &RecordKey,
    s: &vibebms_core::score::Score,
    clear: vibebms_core::score::ClearType,
) -> crate::library::BestScore {
    lib.record_stage(key, s, clear)
}

fn song_dir(s: &Song) -> &Path {
    Path::new(&s.path).parent().unwrap_or(Path::new(""))
}

/// RANDOM MIX's draw: the charts in random order, within the limits; with
/// a BPM range the first of one BPM and the others near it; no two of one
/// song (folder), a chart in several folders drawn as one; then each its
/// song's chart nearest the optimum level.
fn mix<'a>(
    r: &RandomMix,
    songs: &'a [Song],
    charts: &Charts,
    mode: PlayMode,
    seed: i64,
) -> Vec<&'a Song> {
    let mut rng = JavaRandom::new(seed);
    let count = if (1..=MAX_STAGES as u32).contains(&r.stages) {
        r.stages as usize
    } else {
        2 + rng.next_int_bounded(4) as usize
    };
    let (mut lo, mut hi) = (r.min_level, r.max_level);
    if lo > 0 && hi > 0 && lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    let level_ok = |s: &Song| {
        let l = s.level_num();
        (lo <= 0 || l >= lo) && (hi <= 0 || l <= hi)
    };
    let bpm_ok = |s: &Song| {
        (r.min_bpm <= 0.0 || s.main_bpm >= r.min_bpm)
            && (r.max_bpm <= 0.0 || s.main_bpm <= r.max_bpm)
    };
    // (a chart once: of its copies those limits keep)
    let mut pool: Vec<usize> = (0..songs.len())
        .filter(|&i| {
            let s = &songs[i];
            s.mode == mode && level_ok(s) && bpm_ok(s)
        })
        .collect();
    charts.one_each(&mut pool);
    let mut pool: Vec<&Song> = pool.into_iter().map(|i| &songs[i]).collect();
    // Fisher-Yates with the seed's sequence
    for i in (1..pool.len()).rev() {
        let j = rng.next_int_bounded(i as i32 + 1) as usize;
        pool.swap(i, j);
    }
    let mut out: Vec<&Song> = Vec::new();
    for s in pool {
        if out.len() >= count {
            break;
        }
        if out.iter().any(|o| song_dir(o) == song_dir(s)) {
            continue;
        }
        if r.bpm_range > 0.0 {
            match out.first() {
                None if s.min_bpm != s.max_bpm => continue,
                Some(f) if (s.main_bpm - f.main_bpm).abs() > r.bpm_range => continue,
                _ => {}
            }
        }
        out.push(s);
    }
    if r.optimum_level >= 1 {
        for s in &mut out {
            if let Some(best) = songs
                .iter()
                .filter(|o| song_dir(o) == song_dir(s) && o.mode == mode && level_ok(o))
                .min_by_key(|o| (o.level_num() - r.optimum_level).abs())
            {
                *s = best;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(dir: &str, level: &str, bpm: f64, md5: char) -> Song {
        Song {
            path: format!("/bms/{dir}/{level}.bms"),
            md5: std::iter::repeat_n(md5, 32).collect(),
            sha256: std::iter::repeat_n(md5, 64).collect(),
            title: format!("{dir} {level}"),
            level: level.into(),
            mode: PlayMode::Beat7K,
            min_bpm: bpm,
            max_bpm: bpm,
            main_bpm: bpm,
            ..Default::default()
        }
    }

    fn store(name: &str) -> CourseStore {
        let dir = std::env::temp_dir().join(format!("vibebms-{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        CourseStore::open(&dir)
    }

    #[test]
    fn courses_are_kept_reordered_and_imported() {
        let songs = [song("a", "5", 150.0, 'a'), song("b", "7", 150.0, 'b')];
        let mut st = store("courses");
        st.list.clear();
        let one = st.add(Course {
            title: "ONE".into(),
            kind: CourseKind::Nonstop,
            stages: songs.iter().map(Stage::of).collect(),
            ..Default::default()
        });
        let two = st.add(Course {
            title: "TWO".into(),
            kind: CourseKind::Nonstop,
            ..Default::default()
        });
        let titles = |st: &CourseStore| -> Vec<String> {
            st.of_kind(CourseKind::Nonstop)
                .iter()
                .map(|c| c.title.clone())
                .collect()
        };
        assert_eq!(titles(&st), ["TWO", "ONE"]);
        st.shift(one, -1);
        assert_eq!(titles(&st), ["ONE", "TWO"]);
        // a NONSTOP course's joins: one per join, the default where unset
        // (the two swapped places)
        assert_eq!(st.get(two).unwrap().title, "ONE");
        let def = st.get(one).unwrap().def();
        assert!(def.joins.is_empty());
        let def = st.list.iter().find(|c| c.title == "ONE").unwrap().def();
        assert_eq!(def.joins, [Join::Fade]);
        // kept
        let again = CourseStore::open(st.file.parent().unwrap());
        assert_eq!(again.list, st.list);
        // LR2's courses: one already here (same identity) is not added
        // again; stages found in the library
        let lr2 = vec![
            crate::library::lr2db::Lr2Course {
                title: "LR2".into(),
                def: CourseDef {
                    kind: CourseKind::Expert,
                    joins: Vec::new(),
                    stages: vec![songs[1].md5.clone(), "f".repeat(32)],
                },
                ir: false,
            },
            crate::library::lr2db::Lr2Course {
                title: "SAME".into(),
                def,
                ir: false,
            },
        ];
        assert_eq!(st.import(lr2, &songs), 1);
        let c = &st.of_kind(CourseKind::Expert)[0];
        assert_eq!(
            (c.title.as_str(), c.stages[0].title.as_str()),
            ("LR2", "b 7")
        );
        assert!(c.stages[1].sha256.is_empty());
        st.remove(c.id);
        assert!(st.of_kind(CourseKind::Expert).is_empty());
        let _ = std::fs::remove_file(&st.file);
    }

    #[test]
    fn random_mix_follows_its_settings() {
        let songs = [
            song("a", "3", 150.0, 'a'),
            song("a", "9", 150.0, 'b'),
            song("b", "8", 155.0, 'c'),
            song("c", "12", 150.0, 'd'),
            song("d", "6", 200.0, 'e'),
            Song {
                min_bpm: 100.0,
                ..song("e", "7", 150.0, 'f')
            },
        ];
        let r = RandomMix {
            stages: 5,
            max_level: 10,
            bpm_range: 10.0,
            optimum_level: 8,
            ..Default::default()
        };
        for seed in 0..20 {
            let got = mix(&r, &songs, &Charts::new(&songs), PlayMode::Beat7K, seed);
            // level 12 and BPM 200 and the song of two BPMs (as first) out;
            // one chart a folder; folder a's nearest 8: its 9
            let dirs: Vec<&Path> = got.iter().map(|s| song_dir(s)).collect();
            assert!(got.len() <= 3, "{seed}: {dirs:?}");
            assert!(got.iter().all(|s| s.level_num() <= 10));
            assert!(got
                .iter()
                .all(|s| (s.main_bpm - got[0].main_bpm).abs() <= 10.0));
            for (i, d) in dirs.iter().enumerate() {
                assert!(!dirs[..i].contains(d));
            }
            if let Some(a) = got.iter().find(|s| song_dir(s) == Path::new("/bms/a")) {
                assert_eq!(a.level, "9");
            }
            assert_eq!(got[0].min_bpm, got[0].max_bpm);
        }
        let mut st = store("mix");
        st.list.clear();
        let id = st
            .random_mix(
                CourseKind::Nonstop,
                &r,
                &songs,
                &Charts::new(&songs),
                PlayMode::Beat7K,
                1,
            )
            .unwrap();
        let again = st
            .random_mix(
                CourseKind::Nonstop,
                &r,
                &songs,
                &Charts::new(&songs),
                PlayMode::Beat7K,
                2,
            )
            .unwrap();
        // the last RANDOM MIX is replaced
        assert_eq!(st.of_kind(CourseKind::Nonstop).len(), 1);
        assert_ne!(id, 0);
        let c = st.get(again).unwrap();
        assert_eq!(c.joins.len(), c.stages.len() - 1);
        assert!(st
            .random_mix(
                CourseKind::Expert,
                &r,
                &songs,
                &Charts::new(&songs),
                PlayMode::Beat14K,
                1
            )
            .is_err());
        let _ = std::fs::remove_file(&st.file);
    }

    /// A chart held in two folders is drawn once.
    #[test]
    fn mix_draws_copies_once() {
        let songs = [song("a", "5", 150.0, 'a'), song("b", "5", 150.0, 'a')];
        let r = RandomMix {
            stages: 2,
            bpm_range: 0.0,
            ..Default::default()
        };
        for seed in 0..10 {
            let got = mix(&r, &songs, &Charts::new(&songs), PlayMode::Beat7K, seed);
            assert_eq!(got.len(), 1, "{seed}");
            assert_eq!(got[0].path, "/bms/a/5.bms");
        }
    }

    /// A file held as a BMS and a PMS is drawn as the copy of the mode
    /// asked for, whichever is first.
    #[test]
    fn mix_draws_the_copy_of_its_mode() {
        let pms = Song {
            path: "/bms/b/5.pms".into(),
            mode: PlayMode::Popn9K,
            ..song("b", "5", 150.0, 'a')
        };
        let songs = [song("a", "5", 150.0, 'a'), pms];
        let r = RandomMix {
            stages: 1,
            bpm_range: 0.0,
            ..Default::default()
        };
        for (mode, path) in [
            (PlayMode::Beat7K, "/bms/a/5.bms"),
            (PlayMode::Popn9K, "/bms/b/5.pms"),
        ] {
            let got = mix(&r, &songs, &Charts::new(&songs), mode, 1);
            let got: Vec<&str> = got.iter().map(|s| s.path.as_str()).collect();
            assert_eq!(got, [path], "{mode:?}");
        }
    }
}
