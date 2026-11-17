//! LR2's three course kinds (`docs/LR2_COURSES.md`, S-OLR2b): expert
//! (free) courses, NONSTOP (connected) courses and class (dan) courses;
//! a course's identity (LR2's packed course string) and its lamps.

use crate::score::{ClearType, Score};
use serde::{Deserialize, Serialize};

crate::choices! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum CourseKind {
        /// Each stage its own play, the gauge carried over (S-OLR2b 4).
        Expert = 0 => "EXPERT",
        /// All stages joined into one chart, played as one (S-OLR2b 3).
        Nonstop = 1 => "NONSTOP",
        /// An expert course with LR2's forced settings (S-OLR2b 5).
        #[default]
        Class = 2 => "CLASS",
    }
}

crate::choices! {
    /// How a NONSTOP course passes from one stage to the next (the digit
    /// of its packed string, S-OLR2b 3.1). The names are descriptive:
    /// LR2's menu text comes from the theme.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum Join {
        #[default]
        Fade = 0 => "FADE",
        FadeMatch = 1 => "FADE + TEMPO",
        Cut = 2 => "CUT",
        CutMatch = 3 => "CUT + TEMPO",
        Gap1 = 4 => "GAP 1",
        Gap2 = 5 => "GAP 2",
    }
}

impl Join {
    pub fn digit(self) -> char {
        (b'0' + self as u8) as char
    }

    pub fn of_digit(c: char) -> Option<Join> {
        Join::ALL.get(c.to_digit(10)? as usize).copied()
    }

    /// Stage N+1 is played at stage N's tempo (and pitch).
    pub fn tempo_match(self) -> bool {
        matches!(self, Join::FadeMatch | Join::CutMatch)
    }

    /// How long (µs) stage N's sound fades out after the join.
    pub fn fade_out(self) -> i64 {
        match self {
            Join::Fade => 10_000_000,
            Join::Cut | Join::CutMatch => 200_000,
            Join::FadeMatch | Join::Gap1 | Join::Gap2 => 0,
        }
    }

    /// How long (µs) stage N+1's sound fades in before the join.
    pub fn fade_in(self) -> i64 {
        match self {
            Join::Fade | Join::FadeMatch | Join::Gap1 => 5_000_000,
            Join::Cut | Join::CutMatch => 200_000,
            Join::Gap2 => 0,
        }
    }
}

/// The most stages a course plays (S-OLR2b 2.3).
pub const MAX_STAGES: usize = 5;
/// The most joins the packed string holds.
const MAX_JOINS: usize = 10;

/// A course as LR2 keeps it: its kind, its joins (NONSTOP) and its
/// stages' chart MD5s.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseDef {
    pub kind: CourseKind,
    /// The join after each stage but the last (NONSTOP; else empty).
    pub joins: Vec<Join>,
    /// The stages' MD5s (lower-case hex), in order.
    pub stages: Vec<String>,
}

impl CourseDef {
    /// The course's identity: LR2's packed course string (S-OLR2b 2.3),
    /// a 32-character header (a join digit per join, the kind digit at
    /// 11, zeros, and at 29 to 32 the start of the MD5 of the player's
    /// password: vibeBMS has none and writes `0000`, so that a course is
    /// the same for every player and can be exchanged), then each stage's
    /// MD5. A course's records are kept under it.
    pub fn id(&self) -> String {
        let mut s = String::with_capacity(32 * (self.stages.len() + 1));
        for k in 0..MAX_JOINS {
            let j = (self.kind == CourseKind::Nonstop)
                .then(|| self.joins.get(k))
                .flatten();
            s.push(j.map_or('0', |j| j.digit()));
        }
        s.push((b'0' + self.kind as u8) as char);
        s.push_str(&"0".repeat(21));
        for md5 in &self.stages {
            s.push_str(&md5.to_ascii_lowercase());
        }
        s
    }

    /// The course of a packed string (an LR2 `song.db` row's `hash`), if
    /// it is one: its header's kind digit, its joins (NONSTOP: one per
    /// join), its stages; the password characters are not read.
    pub fn parse(id: &str) -> Option<CourseDef> {
        let s = id.trim();
        if s.len() < 32 || !s.len().is_multiple_of(32) || !s.is_ascii() {
            return None;
        }
        let head: Vec<char> = s[..32].chars().collect();
        let kind = CourseKind::ALL
            .into_iter()
            .find(|k| head[10] == (b'0' + *k as u8) as char)?;
        let stages: Vec<String> = (1..s.len() / 32)
            .map(|i| s[32 * i..32 * (i + 1)].to_ascii_lowercase())
            .collect();
        if stages
            .iter()
            .any(|m| !m.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return None;
        }
        let joins = match kind {
            CourseKind::Nonstop => (0..stages.len().saturating_sub(1).min(MAX_JOINS))
                .map(|k| Join::of_digit(head[k]).unwrap_or_default())
                .collect(),
            _ => Vec::new(),
        };
        Some(CourseDef {
            kind,
            joins,
            stages,
        })
    }
}

/// The lamp of an expert or class course (S-OLR2b 4.4): FAILED when the
/// gauge ended below 2 or the last stage was not finished; else FULL
/// COMBO when the course's max combo is its notes; else the gauge's
/// clear lamp when it ends strictly above 2 (`gauge_clear`, HAZARD's a
/// CLEAR); else FAILED. No lamp cap applies.
pub fn course_lamp(
    finished: bool,
    gauge: f32,
    max_combo: u32,
    notes: u32,
    gauge_clear: ClearType,
) -> ClearType {
    if !finished || gauge < 2.0 {
        ClearType::Failed
    } else if max_combo >= notes {
        ClearType::FullCombo
    } else if gauge > 2.0 {
        match gauge_clear {
            ClearType::FullCombo => ClearType::Normal,
            c => c,
        }
    } else {
        ClearType::Failed
    }
}

/// An expert or class course's running totals (S-OLR2b 4.2): what the
/// stages played add up to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CourseTotals {
    /// Judgement counts (empty POORs included, mines not), max combo and
    /// notes of the stages played.
    pub score: Score,
    /// The combo running across the stages.
    pub combo: u32,
}

impl CourseTotals {
    /// Stage `s` played on: `mines` of its POORs were mines (left out),
    /// `combo` its combo as it ended, `before_break` its combo up to its
    /// first break (all its combo when it has none).
    pub fn add(&mut self, s: &Score, mines: u32, combo: u32, before_break: Option<u32>) {
        let t = &mut self.score;
        for j in 0..6 {
            for side in 0..2 {
                t.counts[j][side] += s.counts[j][side];
            }
            t.just[j] += s.just[j];
        }
        // the mines were counted with the empty POORs
        let miss = crate::judge::Judge::Miss as usize;
        let mut left = mines;
        let [early, late] = &mut t.counts[miss];
        for slot in [&mut t.just[miss], early, late] {
            let k = left.min(*slot);
            *slot -= k;
            left -= k;
        }
        t.total_notes += s.total_notes;
        t.passed += s.passed;
        t.combo_breaks += s.combo_breaks;
        let carried = match before_break {
            Some(b) => {
                t.max_combo = t.max_combo.max(self.combo + b);
                combo
            }
            None => self.combo + combo,
        };
        self.combo = carried;
        t.max_combo = t.max_combo.max(carried).max(s.max_combo);
    }

    /// The course's lamp as `course_lamp`, after the last stage played
    /// with `gauge` left on a gauge whose clear lamp is `gauge_clear`.
    pub fn lamp(&self, finished: bool, gauge: f32, gauge_clear: ClearType) -> ClearType {
        course_lamp(
            finished,
            gauge,
            self.score.max_combo,
            self.score.total_notes,
            gauge_clear,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::Judge;

    fn md5(c: char) -> String {
        std::iter::repeat_n(c, 32).collect()
    }

    #[test]
    fn identity_round_trip() {
        let nonstop = CourseDef {
            kind: CourseKind::Nonstop,
            joins: vec![Join::Cut, Join::Gap2],
            stages: vec![md5('a'), md5('b'), md5('c')],
        };
        let id = nonstop.id();
        assert_eq!(id.len(), 32 * 4);
        assert_eq!(&id[..32], "25000000001000000000000000000000");
        assert_eq!(&id[28..32], "0000");
        assert_eq!(CourseDef::parse(&id), Some(nonstop.clone()));
        // other kinds: joins all '0'
        for kind in [CourseKind::Expert, CourseKind::Class] {
            let c = CourseDef {
                kind,
                joins: Vec::new(),
                stages: vec![md5('d'), md5('e')],
            };
            let id = c.id();
            assert_eq!(&id[..11], &format!("0000000000{}", kind as u8));
            assert_eq!(CourseDef::parse(&id), Some(c));
        }
        // an LR2 player's string: the password's characters are not read,
        // upper-case MD5s are taken as lower-case
        let lr2 = format!(
            "2500000000100000000000000000{}{}{}",
            "9f3a",
            md5('A'),
            md5('b')
        );
        let c = CourseDef::parse(&lr2).unwrap();
        assert_eq!(c.joins, vec![Join::Cut]);
        assert_eq!(c.stages[0], md5('a'));
        assert_ne!(c.id(), lr2);
        // not a course string
        assert_eq!(CourseDef::parse("123"), None);
        assert_eq!(
            CourseDef::parse(&format!("{}{}", &id[..32], md5('x'))),
            None
        );
        let bad_kind = format!("{}7{}{}", "0".repeat(10), "0".repeat(21), md5('a'));
        assert_eq!(CourseDef::parse(&bad_kind), None);
    }

    #[test]
    fn course_lamps() {
        use ClearType::*;
        // strictly above 2: a course that ends at exactly 2 fails
        assert_eq!(course_lamp(true, 2.0, 10, 20, Normal), Failed);
        assert_eq!(course_lamp(true, 2.1, 10, 20, Normal), Normal);
        assert_eq!(course_lamp(true, 2.1, 10, 20, Hard), Hard);
        assert_eq!(course_lamp(true, 50.0, 10, 20, FullCombo), Normal);
        // a full combo whatever the gauge, if it lives
        assert_eq!(course_lamp(true, 2.0, 20, 20, Easy), FullCombo);
        assert_eq!(course_lamp(false, 90.0, 20, 20, Hard), Failed);
        assert_eq!(course_lamp(true, 1.9, 20, 20, Hard), Failed);
    }

    #[test]
    fn totals_carry_the_combo_and_leave_mines_out() {
        let stage = |pg: u32, poor: u32, miss: u32, max_combo: u32| {
            let mut s = Score {
                total_notes: pg + poor,
                passed: pg + poor,
                max_combo,
                combo_breaks: poor,
                ..Default::default()
            };
            s.counts[Judge::PGreat as usize][0] = pg;
            s.counts[Judge::Poor as usize][1] = poor;
            s.just[Judge::Miss as usize] = miss;
            s
        };
        let mut t = CourseTotals::default();
        // 10 notes, no break, 2 empty POORs of which 1 a mine
        t.add(&stage(10, 0, 2, 10), 1, 10, None);
        assert_eq!((t.combo, t.score.max_combo), (10, 10));
        assert_eq!(t.score.count(Judge::Miss), 1);
        // 5 notes on, then a POOR, then 4: the course combo reached 15
        t.add(&stage(9, 1, 0, 5), 0, 4, Some(5));
        assert_eq!((t.combo, t.score.max_combo), (4, 15));
        assert_eq!((t.score.total_notes, t.score.ex_score()), (20, 38));
        assert_eq!(t.lamp(true, 30.0, ClearType::Normal), ClearType::Normal);
        // all without a break: a full combo
        let mut fc = CourseTotals::default();
        fc.add(&stage(10, 0, 0, 10), 0, 10, None);
        fc.add(&stage(7, 0, 0, 7), 0, 7, None);
        assert_eq!(fc.score.max_combo, 17);
        assert_eq!(fc.lamp(true, 2.0, ClearType::Hard), ClearType::FullCombo);
    }
}
