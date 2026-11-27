//! One chart held in several folders. Records of one SHA-256 are one
//! chart: the lists that are not a view of one folder show it once, and
//! wherever a chart is found by its hash the same copy of it is taken, the
//! first in library order, a loose file before one inside an archive. An
//! MD5 can be shared by different charts (a collision, or a file made to
//! match one): then the title and artist the finder was given choose.

use crate::library::Song;
use crate::placement::{same_artist, same_title};
use std::borrow::Cow;
use std::collections::HashMap;

/// The library's charts by hash, made once per library (`songs` in
/// library order, as `Ctx::reload_library` sorts them).
#[derive(Default)]
pub struct Charts {
    /// Each record's chart.
    chart: Vec<u32>,
    /// Each chart's copy taken (a record).
    taken: Vec<usize>,
    /// The records inside an archive.
    packed: Vec<bool>,
    by_sha: HashMap<String, u32>,
    /// The charts of each MD5, in the order of their copies taken.
    by_md5: HashMap<String, Vec<u32>>,
}

/// Of the charts of one MD5, how many a lookup compares names with: a pack
/// can hold thousands of one MD5 (collisions are cheap to chain), and a
/// table thousands of rows of it.
const NAMED_OF: usize = 16;

/// A hash as it is looked up: trimmed, lower case.
fn key(h: &str) -> Cow<'_, str> {
    let h = h.trim();
    if h.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(h.to_ascii_lowercase())
    } else {
        Cow::Borrowed(h)
    }
}

impl Charts {
    pub fn new(songs: &[Song]) -> Charts {
        let mut c = Charts {
            chart: Vec::with_capacity(songs.len()),
            packed: songs.iter().map(Song::in_archive).collect(),
            ..Default::default()
        };
        for (i, s) in songs.iter().enumerate() {
            let sha = key(&s.sha256);
            let n = match c.by_sha.get(sha.as_ref()) {
                Some(&n) => {
                    let t = &mut c.taken[n as usize];
                    if c.packed[*t] && !c.packed[i] {
                        *t = i;
                    }
                    n
                }
                None => {
                    let n = c.taken.len() as u32;
                    c.taken.push(i);
                    // (a record without a SHA-256 is a chart of its own)
                    if !sha.is_empty() {
                        c.by_sha.insert(sha.into_owned(), n);
                    }
                    n
                }
            };
            c.chart.push(n);
        }
        let mut order: Vec<u32> = (0..c.taken.len() as u32).collect();
        order.sort_by_key(|&n| c.rank(c.taken[n as usize]));
        for n in order {
            let md5 = key(&songs[c.taken[n as usize]].md5);
            if !md5.is_empty() {
                c.by_md5.entry(md5.into_owned()).or_default().push(n);
            }
        }
        c
    }

    /// The order copies are taken in: loose files first, then by library
    /// order.
    fn rank(&self, i: usize) -> (bool, usize) {
        (self.packed[i], i)
    }

    /// How many charts there are (copies counted once).
    pub fn count(&self) -> usize {
        self.taken.len()
    }

    /// The chart of this SHA-256 (its copy taken).
    pub fn by_sha(&self, sha: &str) -> Option<usize> {
        let n = *self.by_sha.get(key(sha).as_ref())?;
        Some(self.taken[n as usize])
    }

    /// The chart of this MD5: of several charts with it, the first (of the
    /// first [`NAMED_OF`]) whose title (and artist, when one is given) are
    /// `title`'s and `artist`'s, else the first.
    pub fn by_md5(&self, songs: &[Song], md5: &str, title: &str, artist: &str) -> Option<usize> {
        let found = self.by_md5.get(key(md5).as_ref())?;
        let copies = || found.iter().map(|&n| self.taken[n as usize]);
        let named = |i: &usize| {
            let s = &songs[*i];
            (same_title(&s.title, title) || same_title(&s.full_title(), title))
                && (artist.is_empty() || same_artist(&s.artist, artist))
        };
        (found.len() > 1 && !title.is_empty())
            .then(|| copies().take(NAMED_OF).find(named))
            .flatten()
            .or_else(|| copies().next())
    }

    /// The chart of this SHA-256, else of this MD5 (`by_md5`).
    pub fn find(
        &self,
        songs: &[Song],
        md5: &str,
        sha: &str,
        title: &str,
        artist: &str,
    ) -> Option<usize> {
        self.by_sha(sha)
            .or_else(|| self.by_md5(songs, md5, title, artist))
    }

    /// `v` (records) with each chart once: of its copies there, the one
    /// taken first (`rank`). The order is kept.
    pub fn one_each(&self, v: &mut Vec<usize>) {
        if self.taken.len() == self.chart.len() {
            return;
        }
        let mut kept = vec![usize::MAX; self.taken.len()];
        for &i in v.iter() {
            let k = &mut kept[self.chart[i] as usize];
            if *k == usize::MAX || self.rank(i) < self.rank(*k) {
                *k = i;
            }
        }
        v.retain(|&i| kept[self.chart[i] as usize] == i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(path: &str, sha: &str, md5: &str, title: &str) -> Song {
        Song {
            path: path.into(),
            sha256: sha.into(),
            md5: md5.into(),
            title: title.into(),
            artist: "someone".into(),
            ..Default::default()
        }
    }

    /// Copies of one SHA-256 are one chart, the first loose one taken; a
    /// record without a SHA-256 is never merged.
    #[test]
    fn copies_are_one_chart() {
        let songs = [
            song("/bms/z.zip!/a/x.bms", "s1", "m1", "X"),
            song("/bms/b/x.bms", "S1", "m1", "X"),
            song("/bms/c/x.bms", "s1", "m1", "X"),
            song("/bms/d/y.bms", "", "m2", "Y"),
            song("/bms/e/y.bms", "", "m2", "Y"),
            song("/bms/p.zip!/q.bms", "s3", "m3", "Q"),
            song("/bms/r.zip!/q.bms", "s3", "m3", "Q"),
        ];
        let c = Charts::new(&songs);
        assert_eq!(c.count(), 4);
        assert_eq!(c.by_sha("s1"), Some(1));
        assert_eq!(c.by_sha("S1 "), Some(1));
        assert_eq!(c.by_md5(&songs, "M1", "", ""), Some(1));
        assert_eq!(c.by_sha("s3"), Some(5));
        assert_eq!(c.by_sha(""), None);
        assert_eq!(c.by_md5(&songs, "", "", ""), None);
        let mut taken: Vec<usize> = (0..songs.len()).collect();
        c.one_each(&mut taken);
        assert_eq!(taken, [1, 3, 4, 5]);
        let mut v = vec![6, 4, 2, 0, 3, 5];
        c.one_each(&mut v);
        assert_eq!(v, [4, 2, 3, 5]);
        // a list with only the archived copy keeps it
        let mut v = vec![0, 6];
        c.one_each(&mut v);
        assert_eq!(v, [0, 6]);
    }

    /// Two charts of one MD5: the one whose title and artist are the
    /// finder's, else (no title, or none alike) the first.
    #[test]
    fn md5_collision_goes_by_title() {
        let mut crafted = song("/bms/a/evil.bms", "s1", "m", "Something Else");
        crafted.artist = "nobody".into();
        let songs = [
            crafted,
            song("/bms/b/real.bms", "s2", "m", "Real Song [ANOTHER]"),
        ];
        let c = Charts::new(&songs);
        assert_eq!(c.by_md5(&songs, "m", "Real Song", "someone"), Some(1));
        assert_eq!(c.by_md5(&songs, "m", "Real Song", ""), Some(1));
        assert_eq!(c.by_md5(&songs, "m", "", ""), Some(0));
        assert_eq!(c.by_md5(&songs, "m", "Unknown", ""), Some(0));
        assert_eq!(c.by_md5(&songs, "m", "Real Song", "nobody"), Some(0));
        // the SHA-256 needs no hint
        assert_eq!(c.find(&songs, "m", "s1", "Real Song", ""), Some(0));
        assert_eq!(c.find(&songs, "m", "s9", "Real Song", ""), Some(1));
    }

    /// Thousands of charts of one MD5 and a table of thousands of rows of
    /// it: each row looks at a few of them, not at all.
    #[test]
    fn many_charts_of_one_md5_are_looked_up_quickly() {
        // (titles as long as many are)
        let title = |k: usize| format!("T{k} - a song with a long title [ANOTHER] -remix-");
        let songs: Vec<Song> = (0..5000)
            .map(|k| song(&format!("/bms/{k}/x.bms"), &format!("s{k}"), "m", &title(k)))
            .collect();
        let c = Charts::new(&songs);
        let begun = std::time::Instant::now();
        for _ in 0..5000 {
            assert_eq!(c.by_md5(&songs, "m", &title(99_999), ""), Some(0));
        }
        // (all of them, one by one: minutes)
        assert!(begun.elapsed().as_secs() < 10, "{:?}", begun.elapsed());
        assert_eq!(c.by_md5(&songs, "m", &title(15), ""), Some(15));
        assert_eq!(c.by_md5(&songs, "m", &title(16), ""), Some(0));
    }
}
