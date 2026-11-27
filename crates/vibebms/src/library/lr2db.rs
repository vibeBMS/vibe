//! LR2's courses from its song database (`LR2files/Database/song.db`): its
//! `expert`, `nonstop` and `grade` tables (`docs/LR2_COURSES.md`, 2), read
//! with vibeBMS's own SQLite reader (`sqlite`).

use crate::library::sqlite::{get, no_panic, Db, Value};
use vibebms_core::course::CourseDef;

/// A course read from LR2: its title, its definition, and whether it was
/// an internet-ranking (read-only) course.
#[derive(Clone, Debug, PartialEq)]
pub struct Lr2Course {
    pub title: String,
    pub def: CourseDef,
    pub ir: bool,
}

/// The courses of an LR2 `song.db`, newest (highest id) first per table:
/// `expert`, `nonstop`, `grade`. A row is played as the kind its packed
/// string says (S-OLR2b 2.3); a row whose string is not one is left out.
pub fn courses(bytes: &[u8]) -> Result<Vec<Lr2Course>, String> {
    no_panic(|| {
        let db = Db::open(bytes)?;
        let mut out = Vec::new();
        let mut found = false;
        for name in ["expert", "nonstop", "grade"] {
            let Some(rows) = db.table_rows(name) else {
                continue;
            };
            found = true;
            let mut table: Vec<(i64, Lr2Course)> = rows
                .filter_map(|r| {
                    let def = get(&r, "hash")
                        .and_then(|v| v.text())
                        .and_then(|h| CourseDef::parse(&h))
                        .filter(|d| !d.stages.is_empty())?;
                    let course = Lr2Course {
                        title: get(&r, "title").and_then(Value::text).unwrap_or_default(),
                        def,
                        ir: get(&r, "ir").and_then(Value::int) == Some(1),
                    };
                    Some((get(&r, "id").and_then(|v| v.int()).unwrap_or(0), course))
                })
                .collect();
            table.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
            out.extend(table.into_iter().map(|(_, c)| c));
        }
        if !found {
            return Err("no course tables (expert, nonstop, grade) in it".into());
        }
        Ok(out)
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::library::sqlite::write::{db, V};
    use vibebms_core::course::{CourseKind, Join};

    const SQL: &str = "CREATE TABLE expert ( id INTEGER PRIMARY KEY, title TEXT, line INTEGER, \
                       hash TEXT, ir INTEGER )";

    fn md5(c: char) -> String {
        std::iter::repeat_n(c, 32).collect()
    }

    /// A `song.db` as LR2 keeps its courses (S-OLR2b 2.2): its three
    /// tables, titles in Shift_JIS.
    pub(crate) fn song_db() -> Vec<u8> {
        let ns = CourseDef {
            kind: CourseKind::Nonstop,
            joins: vec![Join::Cut],
            stages: vec![md5('a'), md5('b')],
        }
        .id();
        // a class row, made by an LR2 player with a password
        let grade = format!("00000000002000000000000000009f3a{}{}", md5('c'), md5('d'));
        let expert = CourseDef {
            kind: CourseKind::Expert,
            joins: Vec::new(),
            stages: vec![md5('e')],
        }
        .id();
        // "段位" in Shift_JIS
        let sjis: &[u8] = &[0x92, 0x69, 0x88, 0xca];
        db(&[
            (
                "expert",
                SQL,
                vec![
                    (
                        1,
                        vec![
                            V::Null,
                            V::Text(b"OLD"),
                            V::Int(7),
                            V::Text(expert.as_bytes()),
                            V::Int(0),
                        ],
                    ),
                    (
                        3,
                        vec![
                            V::Null,
                            V::Text(b"NEW"),
                            V::Int(7),
                            V::Text(expert.as_bytes()),
                            V::Int(1),
                        ],
                    ),
                    (
                        4,
                        vec![
                            V::Null,
                            V::Text(b"EMPTY"),
                            V::Int(7),
                            V::Text(&[b'0'; 32]),
                            V::Int(0),
                        ],
                    ),
                ],
            ),
            (
                "nonstop",
                &SQL.replace("expert", "nonstop"),
                vec![(
                    1,
                    vec![
                        V::Null,
                        V::Text(b"NS"),
                        V::Int(7),
                        V::Text(ns.as_bytes()),
                        V::Int(0),
                    ],
                )],
            ),
            (
                "grade",
                &SQL.replace("expert", "grade"),
                vec![(
                    2,
                    vec![
                        V::Null,
                        V::Text(sjis),
                        V::Int(7),
                        V::Text(grade.as_bytes()),
                        V::Int(0),
                    ],
                )],
            ),
        ])
    }

    #[test]
    fn reads_lr2s_course_tables() {
        let bytes = song_db();
        let rows = Db::open(&bytes).unwrap().table("expert").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1][0], ("id".into(), Value::Int(3)));
        assert_eq!(rows[1][2].1, Value::Int(7));
        let c = courses(&bytes).unwrap();
        let titles: Vec<&str> = c.iter().map(|c| c.title.as_str()).collect();
        // newest first per table; a course of no stage left out
        assert_eq!(titles, ["NEW", "OLD", "NS", "段位"]);
        assert!(c[0].ir && !c[1].ir);
        assert_eq!(c[2].def.kind, CourseKind::Nonstop);
        assert_eq!(c[2].def.joins, [Join::Cut]);
        assert_eq!(c[3].def.kind, CourseKind::Class);
        assert_eq!(c[3].def.stages, [md5('c'), md5('d')]);
        assert!(courses(b"not a database").is_err());
        let none = db(&[("song", "CREATE TABLE song (hash TEXT)", Vec::new())]);
        assert!(courses(&none).is_err());
    }
}
