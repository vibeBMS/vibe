//! LR2's courses from its song database (`LR2files/Database/song.db`): its
//! `expert`, `nonstop` and `grade` tables (`docs/LR2_COURSES.md`, 2), read
//! with a small SQLite reader of vibeBMS's own (tables only: no SQL, no
//! index, no write), so that it builds for every target, the web's too.

use vibebms_core::course::CourseDef;

/// One row of a table: its columns' values, by name.
pub type Row = Vec<(String, Value)>;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    /// Text as stored (LR2 keeps Shift_JIS).
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

impl Value {
    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Real(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<String> {
        match self {
            Value::Text(b) | Value::Blob(b) => Some(vibebms_core::encoding::decode_text(b)),
            Value::Int(v) => Some(v.to_string()),
            _ => None,
        }
    }
}

/// A SQLite database file in memory.
pub struct Db<'a> {
    b: &'a [u8],
    page: usize,
    usable: usize,
}

fn be(b: &[u8]) -> u64 {
    b.iter().fold(0, |v, x| (v << 8) | *x as u64)
}

/// A varint at `b[0..]`: its value and length.
fn varint(b: &[u8]) -> Option<(u64, usize)> {
    let mut v = 0u64;
    for i in 0..9 {
        let x = *b.get(i)?;
        if i == 8 {
            return Some(((v << 8) | x as u64, 9));
        }
        v = (v << 7) | (x & 0x7f) as u64;
        if x & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

impl<'a> Db<'a> {
    pub fn open(b: &'a [u8]) -> Result<Db<'a>, String> {
        if b.len() < 100 || &b[..16] != b"SQLite format 3\0" {
            return Err("not a SQLite database".into());
        }
        let page = match be(&b[16..18]) as usize {
            1 => 65536,
            p => p,
        };
        if !(512..=65536).contains(&page) || !page.is_power_of_two() {
            return Err("bad page size".into());
        }
        let usable = page - b[20] as usize;
        Ok(Db { b, page, usable })
    }

    fn page_bytes(&self, n: u64) -> Option<&'a [u8]> {
        let start = (n as usize).checked_sub(1)? * self.page;
        self.b.get(start..start + self.page)
    }

    /// The payload of a cell of `size` bytes whose local part starts at
    /// `p[at..]` (with its overflow pages).
    fn payload(&self, p: &'a [u8], at: usize, size: usize) -> Option<Vec<u8>> {
        let u = self.usable;
        let x = u - 35;
        let local = if size <= x {
            size
        } else {
            let m = (u - 12) * 32 / 255 - 23;
            let k = m + (size - m) % (u - 4);
            if k <= x {
                k
            } else {
                m
            }
        };
        let mut out = p.get(at..at + local)?.to_vec();
        let mut next = if local < size {
            be(p.get(at + local..at + local + 4)?)
        } else {
            0
        };
        let mut guard = 0;
        while out.len() < size && next != 0 {
            let o = self.page_bytes(next)?;
            let take = (size - out.len()).min(u - 4);
            out.extend_from_slice(o.get(4..4 + take)?);
            next = be(&o[..4]);
            guard += 1;
            if guard > self.b.len() / self.page {
                return None;
            }
        }
        (out.len() == size).then_some(out)
    }

    /// Every (rowid, record) of the table b-tree at page `root`.
    fn rows(&self, root: u64, out: &mut Vec<(i64, Vec<u8>)>, depth: usize) -> Option<()> {
        if depth > 64 {
            return None;
        }
        let p = self.page_bytes(root)?;
        let h = if root == 1 { 100 } else { 0 };
        let kind = *p.get(h)?;
        let cells = be(p.get(h + 3..h + 5)?) as usize;
        let interior = kind == 0x05;
        if !interior && kind != 0x0d {
            return None;
        }
        let ptrs = h + if interior { 12 } else { 8 };
        for i in 0..cells {
            let c = be(p.get(ptrs + 2 * i..ptrs + 2 * i + 2)?) as usize;
            if interior {
                self.rows(be(p.get(c..c + 4)?), out, depth + 1)?;
            } else {
                let (size, n1) = varint(p.get(c..)?)?;
                let (rowid, n2) = varint(p.get(c + n1..)?)?;
                let rec = self.payload(p, c + n1 + n2, size as usize)?;
                out.push((rowid as i64, rec));
            }
        }
        if interior {
            self.rows(be(p.get(h + 8..h + 12)?), out, depth + 1)?;
        }
        Some(())
    }

    /// The values of a record.
    fn record(rec: &[u8]) -> Option<Vec<Value>> {
        let (hlen, mut i) = varint(rec)?;
        let mut body = hlen as usize;
        let mut out = Vec::new();
        while i < hlen as usize {
            let (t, n) = varint(rec.get(i..)?)?;
            i += n;
            let int = |len: usize| -> Option<i64> {
                let b = rec.get(body..body + len)?;
                // sign-extended big-endian
                let v = be(b) as i64;
                let shift = 64 - 8 * len as u32;
                Some((v << shift) >> shift)
            };
            let (v, len) = match t {
                0 => (Value::Null, 0),
                1..=4 => (Value::Int(int(t as usize)?), t as usize),
                5 => (Value::Int(int(6)?), 6),
                6 => (Value::Int(int(8)?), 8),
                7 => (Value::Real(f64::from_bits(be(rec.get(body..body + 8)?))), 8),
                8 => (Value::Int(0), 0),
                9 => (Value::Int(1), 0),
                t if t >= 12 => {
                    let len = ((t - 12) / 2) as usize;
                    let b = rec.get(body..body + len)?.to_vec();
                    let v = if t % 2 == 1 {
                        Value::Text(b)
                    } else {
                        Value::Blob(b)
                    };
                    (v, len)
                }
                _ => return None,
            };
            body += len;
            out.push(v);
        }
        Some(out)
    }

    /// The rows of table `name` (`None`: no such table), each value by its
    /// column's name; an `INTEGER PRIMARY KEY` column holds the rowid.
    pub fn table(&self, name: &str) -> Option<Vec<Row>> {
        let mut master = Vec::new();
        self.rows(1, &mut master, 0)?;
        let (root, sql) = master.iter().find_map(|(_, rec)| {
            let v = Self::record(rec)?;
            let s = |i: usize| v.get(i).and_then(Value::text);
            (s(0)? == "table" && s(1)?.eq_ignore_ascii_case(name))
                .then(|| Some((v.get(3)?.int()?, s(4)?)))?
        })?;
        let (cols, key) = columns(&sql);
        let mut rows = Vec::new();
        self.rows(root as u64, &mut rows, 0)?;
        Some(
            rows.into_iter()
                .filter_map(|(rowid, rec)| {
                    let vals = Self::record(&rec)?;
                    Some(
                        cols.iter()
                            .enumerate()
                            .map(|(i, c)| {
                                let v = match vals.get(i) {
                                    _ if Some(i) == key => Value::Int(rowid),
                                    Some(v) => v.clone(),
                                    None => Value::Null,
                                };
                                (c.clone(), v)
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }
}

/// The column names of a `CREATE TABLE` statement, and which is an
/// `INTEGER PRIMARY KEY` (the rowid).
fn columns(sql: &str) -> (Vec<String>, Option<usize>) {
    let (Some(a), Some(b)) = (sql.find('('), sql.rfind(')')) else {
        return (Vec::new(), None);
    };
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0, a + 1);
    for (i, c) in sql[..b].char_indices().skip(a + 1) {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&sql[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&sql[start..b]);
    let mut cols = Vec::new();
    let mut key = None;
    for p in parts {
        let p = p.trim();
        let first = p.split_whitespace().next().unwrap_or("");
        let upper = p.to_ascii_uppercase();
        if ["PRIMARY", "UNIQUE", "CHECK", "FOREIGN", "CONSTRAINT"]
            .contains(&first.to_ascii_uppercase().as_str())
        {
            continue;
        }
        let words: Vec<&str> = upper.split_whitespace().collect();
        if words.get(1) == Some(&"INTEGER") && upper.contains("PRIMARY KEY") {
            key = Some(cols.len());
        }
        cols.push(first.trim_matches(['"', '`', '[', ']']).to_string());
    }
    (cols, key)
}

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
    let db = Db::open(bytes)?;
    let mut out = Vec::new();
    let mut found = false;
    for name in ["expert", "nonstop", "grade"] {
        let Some(mut rows) = db.table(name) else {
            continue;
        };
        found = true;
        let get = |r: &Row, c: &str| {
            r.iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(c))
                .map(|v| v.1.clone())
        };
        rows.sort_by_key(|r| std::cmp::Reverse(get(r, "id").and_then(|v| v.int()).unwrap_or(0)));
        for r in rows {
            let Some(def) = get(&r, "hash")
                .and_then(|v| v.text())
                .and_then(|h| CourseDef::parse(&h))
            else {
                continue;
            };
            if def.stages.is_empty() {
                continue;
            }
            out.push(Lr2Course {
                title: get(&r, "title").and_then(|v| v.text()).unwrap_or_default(),
                def,
                ir: get(&r, "ir").and_then(|v| v.int()) == Some(1),
            });
        }
    }
    if !found {
        return Err("no course tables (expert, nonstop, grade) in it".into());
    }
    Ok(out)
}

/// A minimal SQLite writer, to make the tests' `song.db`: one leaf page
/// per table (small tables only), `CREATE TABLE` statements as given.
#[cfg(test)]
pub(crate) mod write {
    const PAGE: usize = 4096;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        if v <= 0x7f {
            out.push(v as u8);
            return;
        }
        let mut b = Vec::new();
        while v > 0 {
            b.push((v & 0x7f) as u8);
            v >>= 7;
        }
        b.reverse();
        let n = b.len();
        for (i, x) in b.into_iter().enumerate() {
            out.push(if i + 1 < n { x | 0x80 } else { x });
        }
    }

    pub enum V<'a> {
        Null,
        Int(i64),
        Text(&'a [u8]),
    }

    fn record(vals: &[V]) -> Vec<u8> {
        let (mut head, mut body) = (Vec::new(), Vec::new());
        for v in vals {
            match v {
                V::Null => head.push(0),
                V::Int(i) => {
                    head.push(6);
                    body.extend_from_slice(&i.to_be_bytes());
                }
                V::Text(t) => {
                    varint(13 + 2 * t.len() as u64, &mut head);
                    body.extend_from_slice(t);
                }
            }
        }
        let mut r = Vec::new();
        varint(head.len() as u64 + 1, &mut r);
        r.extend(head);
        r.extend(body);
        r
    }

    /// A leaf table page of `rows` (rowid, values); `h` its header's place.
    fn leaf(rows: &[(i64, Vec<V>)], h: usize) -> Vec<u8> {
        let mut p = vec![0u8; PAGE];
        let mut end = PAGE;
        let mut ptrs = Vec::new();
        for (rowid, vals) in rows {
            let rec = record(vals);
            let mut cell = Vec::new();
            varint(rec.len() as u64, &mut cell);
            varint(*rowid as u64, &mut cell);
            cell.extend(rec);
            end -= cell.len();
            p[end..end + cell.len()].copy_from_slice(&cell);
            ptrs.push(end as u16);
        }
        p[h] = 0x0d;
        p[h + 3..h + 5].copy_from_slice(&(rows.len() as u16).to_be_bytes());
        p[h + 5..h + 7].copy_from_slice(&(end as u16).to_be_bytes());
        for (i, ptr) in ptrs.iter().enumerate() {
            p[h + 8 + 2 * i..h + 10 + 2 * i].copy_from_slice(&ptr.to_be_bytes());
        }
        p
    }

    /// A table: its name, its `CREATE TABLE` statement, its rows (rowid,
    /// values).
    pub type Table<'a> = (&'a str, &'a str, Vec<(i64, Vec<V<'a>>)>);

    /// A database of `tables`.
    pub fn db(tables: &[Table]) -> Vec<u8> {
        let master: Vec<(i64, Vec<V>)> = tables
            .iter()
            .enumerate()
            .map(|(i, (name, sql, _))| {
                let v = vec![
                    V::Text(b"table"),
                    V::Text(name.as_bytes()),
                    V::Text(name.as_bytes()),
                    V::Int(i as i64 + 2),
                    V::Text(sql.as_bytes()),
                ];
                (i as i64 + 1, v)
            })
            .collect();
        let mut out = leaf(&master, 100);
        let pages = tables.len() as u32 + 1;
        out[..16].copy_from_slice(b"SQLite format 3\0");
        out[16..18].copy_from_slice(&(PAGE as u16).to_be_bytes());
        out[18] = 1;
        out[19] = 1;
        out[21] = 64;
        out[22] = 32;
        out[23] = 32;
        out[24..28].copy_from_slice(&1u32.to_be_bytes());
        out[28..32].copy_from_slice(&pages.to_be_bytes());
        out[40..44].copy_from_slice(&1u32.to_be_bytes());
        out[44..48].copy_from_slice(&4u32.to_be_bytes());
        out[56..60].copy_from_slice(&1u32.to_be_bytes());
        out[92..96].copy_from_slice(&1u32.to_be_bytes());
        out[96..100].copy_from_slice(&3_045_000u32.to_be_bytes());
        for (_, _, rows) in tables {
            out.extend(leaf(rows, 0));
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::write::{db, V};
    use super::*;
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
        assert_eq!(rows[1][0], ("id".to_string(), Value::Int(3)));
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

    #[test]
    fn long_records_follow_their_overflow_pages() {
        // a page of 4096 holds a record of 4061 bytes at most in place
        let long = vec![b'x'; 9000];
        let mut bytes = db(&[(
            "t",
            "CREATE TABLE t (a TEXT)",
            vec![(1, vec![V::Text(b"short")])],
        )]);
        // make the row's payload overflow: rebuild by hand on page 2
        let rec = {
            let mut r = vec![3u8];
            let n = 13 + 2 * long.len() as u64;
            r.extend([
                0x80 | (n >> 14) as u8 & 0x7f,
                0x80 | ((n >> 7) & 0x7f) as u8,
                (n & 0x7f) as u8,
            ]);
            r[0] = 4;
            r.extend(&long);
            r
        };
        let u = 4096usize;
        let m = (u - 12) * 32 / 255 - 23;
        let k = m + (rec.len() - m) % (u - 4);
        let local = if k <= u - 35 { k } else { m };
        let page2 = 4096;
        let mut leaf = vec![0u8; 4096];
        let mut cell = vec![0x80 | (rec.len() >> 7) as u8, (rec.len() & 0x7f) as u8, 1];
        cell.extend(&rec[..local]);
        let mut rest: Vec<&[u8]> = rec[local..].chunks(u - 4).collect();
        let first = (bytes.len() / 4096 + 1) as u32;
        cell.extend(first.to_be_bytes());
        let at = 4096 - cell.len();
        leaf[at..].copy_from_slice(&cell);
        leaf[0] = 0x0d;
        leaf[3..5].copy_from_slice(&1u16.to_be_bytes());
        leaf[5..7].copy_from_slice(&(at as u16).to_be_bytes());
        leaf[8..10].copy_from_slice(&(at as u16).to_be_bytes());
        bytes[page2..page2 + 4096].copy_from_slice(&leaf);
        let n = rest.len();
        for (i, chunk) in rest.drain(..).enumerate() {
            let mut p = vec![0u8; 4096];
            let next = if i + 1 < n { first + i as u32 + 1 } else { 0 };
            p[..4].copy_from_slice(&next.to_be_bytes());
            p[4..4 + chunk.len()].copy_from_slice(chunk);
            bytes.extend(p);
        }
        let rows = Db::open(&bytes).unwrap().table("t").unwrap();
        assert_eq!(rows[0][0].1, Value::Text(long));
    }
}
