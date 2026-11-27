//! Reading an archive: its list of files, the files wanted (into memory
//! or a folder), the rest not kept.

#[cfg(target_arch = "wasm32")]
use crate::platform::vfs;
use bmseeker::fs::ReadSeek;
use bmseeker::unpack::{Error, Format, Limits, Member, Sink, Take, HEAD};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The largest chart file read by the scan (larger ones are no chart: the
/// decoders' own limit).
const CHART_MAX: u64 = vibebms_core::limits::MAX_FILE as u64;

/// The charts of one archive the scan reads, in all (bytes). They stay in
/// memory until the scan has parsed them, and several archives are read at
/// once. An archive with more is not one the library lists ([`too_large`]).
/// A song's charts are some 300 KB (four to six of 50-80 KB): this holds a
/// pack of some 3500 songs (850 where memory is short).
const CHARTS_MAX: u64 = if cfg!(small_device) {
    256 << 20
} else {
    1 << 30
};

/// What is read of an archive at most, whatever is wanted of it: the
/// files it may list (each costs memory and a line of
/// `cache/archives.json`: a pack of thousands of songs has some hundreds of
/// thousands; one with more is not read, [`too_large`]), and what it may
/// decode to (bytes; in a 7z or a solid RAR what is passed by counts too:
/// a small one can decode to anything, and a real one this large takes
/// minutes to reach a song in).
const LIMITS: Limits = Limits {
    bytes: 64 << 30,
    members: if cfg!(small_device) {
        250_000
    } else {
        1_000_000
    },
};

/// How many times `archive` was opened (tests).
#[cfg(test)]
pub fn times_opened(archive: &Path) -> usize {
    super::state().opened.get(archive).copied().unwrap_or(0)
}

/// What is told an archive's file is read on ([`watched`]).
pub type Moved = Arc<dyn Fn() + Send + Sync>;

thread_local! {
    static WATCH: RefCell<Option<Moved>> = const { RefCell::new(None) };
}

/// `work()`, `moved` told as it reads on in the files of archives it reads
/// on this thread (at each [`WATCHED_EVERY`] bytes; `moved` keeps its own
/// pace): the scan tells a slow read from one that hangs.
pub fn watched<T>(moved: Moved, work: impl FnOnce() -> T) -> T {
    let before = WATCH.replace(Some(moved));
    let out = work();
    WATCH.set(before);
    out
}

/// How many bytes read on of an archive's file tell [`watched`]'s `moved`.
const WATCHED_EVERY: usize = 256 << 10;

/// An archive's file, [`watched`].
struct Watched {
    r: Box<dyn ReadSeek + Send>,
    moved: Moved,
    read: usize,
}

impl Read for Watched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.r.read(buf)?;
        self.read += n;
        if self.read >= WATCHED_EVERY {
            self.read = 0;
            (self.moved)();
        }
        Ok(n)
    }
}

impl Seek for Watched {
    fn seek(&mut self, to: io::SeekFrom) -> io::Result<u64> {
        self.r.seek(to)
    }
}

fn open(archive: &Path) -> io::Result<Box<dyn ReadSeek + Send>> {
    #[cfg(test)]
    {
        *super::state()
            .opened
            .entry(archive.to_path_buf())
            .or_default() += 1;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let r: Box<dyn ReadSeek + Send> = Box::new(io::BufReader::new(std::fs::File::open(archive)?));
    // (in memory once read in: `vfs::Prefetch`)
    #[cfg(target_arch = "wasm32")]
    let r: Box<dyn ReadSeek + Send> = Box::new(io::Cursor::new(vfs::read(archive)?));
    Ok(match WATCH.with_borrow(Option::clone) {
        Some(moved) => Box::new(Watched { r, moved, read: 0 }),
        None => r,
    })
}

/// Where the files wanted go.
pub enum Dest {
    Memory,
    /// Into memory, for the scan: a file larger than a chart may be
    /// ([`CHART_MAX`]) is passed by (listed, not kept).
    Charts,
    #[cfg(not(target_arch = "wasm32"))]
    Folder(PathBuf),
}

/// What [`unpack`] read.
#[derive(Default)]
pub struct Unpacked {
    pub format: String,
    /// Every file in the archive and its size (when asked for).
    pub listing: Option<BTreeMap<PathBuf, u64>>,
    /// The files wanted, in memory ([`Dest::Memory`]).
    pub files: Vec<(PathBuf, Vec<u8>)>,
}

impl Dest {
    /// The largest file kept (a larger one is passed by).
    fn each(&self) -> u64 {
        match self {
            Dest::Charts => CHART_MAX,
            _ => u64::MAX,
        }
    }
}

/// The archive holds more than is read of one (the files wanted, or its
/// list): it will until it changes.
fn too_large() -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, "the files wanted are too large")
}

/// Read the archive `archive`: the files `want` takes into `dest` (at
/// most `cap` bytes of them), and with `list` the list of all its files.
/// `count`: how many files are wanted, if known (by their paths: a path
/// the archive has twice is one; the reading ends once that many are
/// taken: a 7z, a solid RAR or a .tar.gz is not decoded past the last of
/// them; not with `list`). An error of kind `WouldBlock` (web): the
/// archive is not in memory yet; of kind `OutOfMemory`: the files wanted
/// are more than `cap`, or it lists more files than an archive may
/// ([`LIMITS`]); of kind `InvalidData`: it cannot be read, or decodes to
/// more than one may.
pub fn unpack(
    archive: &Path,
    want: &(dyn Fn(&Path) -> bool + Sync),
    list: bool,
    count: Option<usize>,
    dest: &Dest,
    cap: u64,
) -> io::Result<Unpacked> {
    unpack_within(archive, want, list, count, dest, cap, LIMITS)
}

/// [`unpack`], within `limits`.
fn unpack_within(
    archive: &Path,
    want: &(dyn Fn(&Path) -> bool + Sync),
    list: bool,
    count: Option<usize>,
    dest: &Dest,
    cap: u64,
    limits: Limits,
) -> io::Result<Unpacked> {
    let mut r = open(archive)?;
    // (a tar's first header; a gzip's compressed)
    let mut head = Vec::with_capacity(HEAD);
    (&mut r).take(HEAD as u64).read_to_end(&mut head)?;
    r.seek(io::SeekFrom::Start(0))?;
    let format = bmseeker::unpack::sniff(&head)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not an archive"))?;
    let mut sink = Taken {
        want,
        dest,
        list,
        count: count.filter(|_| !list),
        // (a RAR may not give a file's size: its bytes are counted)
        sizeless: list && format == Format::Rar,
        left: cap,
        listing: BTreeMap::new(),
        files: Vec::new(),
        taken: HashSet::new(),
        keep: Keep::Skip,
        too_large: false,
    };
    let r = bmseeker::unpack::read(r, format, limits, &mut sink);
    if sink.too_large {
        return Err(too_large());
    }
    match r {
        Ok(()) => {}
        Err(Error::TooMany(_)) => return Err(too_large()),
        Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
    }
    Ok(Unpacked {
        format: format.ext().into(),
        listing: list.then_some(sink.listing),
        files: sink.files,
    })
}

/// What [`unpack`] reads an archive into: the files wanted kept, the
/// others listed.
struct Taken<'a> {
    want: &'a (dyn Fn(&Path) -> bool + Sync),
    dest: &'a Dest,
    list: bool,
    count: Option<usize>,
    /// The files offered without a size are written (counted) to list them.
    sizeless: bool,
    /// What may still be kept.
    left: u64,
    listing: BTreeMap<PathBuf, u64>,
    files: Vec<(PathBuf, Vec<u8>)>,
    /// The paths of the files wanted taken (a path twice in the archive is
    /// one file).
    taken: HashSet<PathBuf>,
    /// Where the file being written goes.
    keep: Keep,
    too_large: bool,
}

enum Keep {
    /// Counted only.
    Skip,
    Memory(Vec<u8>),
    #[cfg(not(target_arch = "wasm32"))]
    File(io::BufWriter<std::fs::File>),
}

/// A file being written: its bytes where [`Keep`] says, within what is
/// left.
struct Writing<'a> {
    keep: &'a mut Keep,
    too_large: &'a mut bool,
    dest: &'a Dest,
    n: u64,
    left: u64,
}

impl Write for Writing<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.n += buf.len() as u64;
        if self.n > self.dest.each() {
            // passed by: what was kept of it goes
            *self.keep = Keep::Skip;
        }
        // (the scan's is judged once whole: it may yet be one passed by)
        let kept = !matches!(self.keep, Keep::Skip);
        if kept && self.n > self.left && !matches!(self.dest, Dest::Charts) {
            *self.too_large = true;
            return Err(too_large());
        }
        match &mut *self.keep {
            Keep::Skip => {}
            Keep::Memory(v) => v.extend_from_slice(buf),
            #[cfg(not(target_arch = "wasm32"))]
            Keep::File(f) => f.write_all(buf)?,
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        match &mut *self.keep {
            #[cfg(not(target_arch = "wasm32"))]
            Keep::File(f) => f.flush(),
            _ => Ok(()),
        }
    }
}

impl Sink for Taken<'_> {
    fn open(&mut self, m: &Member) -> Result<Take<'_>, String> {
        if self.too_large {
            return Err(too_large().to_string());
        }
        if self.count.is_some_and(|n| self.taken.len() >= n) {
            return Ok(Take::Stop);
        }
        let each = self.dest.each();
        // (by the size it says, before any of it is read)
        self.keep = if (self.want)(&m.rel) && m.size <= each {
            if m.size > self.left {
                self.too_large = true;
                return Err(too_large().to_string());
            }
            match self.dest {
                Dest::Memory | Dest::Charts => Keep::Memory(Vec::new()),
                #[cfg(not(target_arch = "wasm32"))]
                Dest::Folder(d) => {
                    let to = d.join(&m.rel);
                    let made = to.parent().map_or(Ok(()), std::fs::create_dir_all);
                    let f = made.and_then(|_| std::fs::File::create(&to));
                    Keep::File(io::BufWriter::new(f.map_err(|e| e.to_string())?))
                }
            }
        } else if self.sizeless && m.size == 0 {
            Keep::Skip
        } else {
            if self.list {
                self.listing.insert(m.rel.clone(), m.size);
            }
            return Ok(Take::Skip);
        };
        Ok(Take::Write(Box::new(Writing {
            keep: &mut self.keep,
            too_large: &mut self.too_large,
            dest: self.dest,
            n: 0,
            left: self.left,
        })))
    }
    fn close(&mut self, m: Member) {
        if self.list {
            self.listing.insert(m.rel.clone(), m.size);
        }
        let keep = std::mem::replace(&mut self.keep, Keep::Skip);
        if matches!(keep, Keep::Skip) {
            return;
        }
        // (the bytes written; a chart's may be more than the room: it is
        // judged once whole)
        let Some(left) = self.left.checked_sub(m.size) else {
            self.too_large = true;
            return;
        };
        self.left = left;
        if let Keep::Memory(mut v) = keep {
            // (kept while its song is: not the room it grew into)
            v.shrink_to_fit();
            self.files.push((m.rel.clone(), v));
        }
        self.taken.insert(m.rel);
    }
    /// A file that could not be written in full: it goes.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    fn drop_partial(&mut self, rel: &Path) {
        self.keep = Keep::Skip;
        #[cfg(not(target_arch = "wasm32"))]
        if let Dest::Folder(d) = self.dest {
            let _ = std::fs::remove_file(d.join(rel));
        }
    }
}

/// The charts of `archive` and the files the scan needs: `want` (none:
/// all its charts, and its list of files). For the scan: a file larger
/// than a chart may be is passed by unread, and an archive with more than
/// [`CHARTS_MAX`] of charts, or more files than one may list, is not read
/// (an error of kind `OutOfMemory`: it is too large until it changes).
pub fn read_charts(archive: &Path, want: Option<&HashSet<PathBuf>>) -> io::Result<Unpacked> {
    let take = |r: &Path| vibebms_core::is_chart(r) && want.is_none_or(|w| w.contains(r));
    let count = want.map(HashSet::len);
    unpack(
        archive,
        &take,
        want.is_none(),
        count,
        &Dest::Charts,
        CHARTS_MAX,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of a RAR read alone: decoding stops once it is read.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn read_up_to_the_files_wanted() {
        let d = std::env::temp_dir().join(format!("vibebms-arcfs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let rar = d.join("song.rar");
        std::fs::write(&rar, bmseeker::testkit::SONG_RAR).unwrap();
        let want = |r: &Path| r == Path::new("song/a.bms");
        let u = unpack(&rar, &want, false, Some(1), &Dest::Memory, 1 << 20).unwrap();
        assert_eq!(
            u.files,
            [(PathBuf::from("song/a.bms"), b"#TITLE A".to_vec())]
        );
        // listed in full
        let u = unpack(&rar, &|_| false, true, None, &Dest::Memory, 0).unwrap();
        assert_eq!(u.format, "rar");
        assert_eq!(
            u.listing.unwrap().into_iter().collect::<Vec<_>>(),
            [
                (PathBuf::from("song/01.ogg"), 3),
                (PathBuf::from("song/a.bms"), 8)
            ]
        );
        // more than may be kept
        let all = |_: &Path| true;
        let e = unpack(&rar, &all, false, None, &Dest::Memory, 4)
            .err()
            .unwrap();
        assert_eq!(e.kind(), io::ErrorKind::OutOfMemory);
        std::fs::remove_dir_all(&d).ok();
    }

    /// An archive read under `watched` tells it as it reads on; one read
    /// after it does not.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reading_on_is_told() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let d = std::env::temp_dir().join(format!("vibebms-arcfs-watch-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let tar = d.join("p.tar");
        let big = vec![b'x'; 3 * WATCHED_EVERY];
        let files: [(&str, &[u8]); 2] = [("Song/s.wav", &big), ("Song/a.bms", b"#TITLE A")];
        std::fs::write(&tar, bmseeker::testkit::tar(&files)).unwrap();
        let told = Arc::new(AtomicUsize::new(0));
        let t = told.clone();
        let moved: Moved = Arc::new(move || {
            t.fetch_add(1, Ordering::Relaxed);
        });
        let u = watched(moved, || read_charts(&tar, None)).unwrap();
        assert_eq!(u.files.len(), 1);
        let n = told.load(Ordering::Relaxed);
        assert!(n >= 2, "{n}");
        read_charts(&tar, None).unwrap();
        assert_eq!(told.load(Ordering::Relaxed), n);
        std::fs::remove_dir_all(&d).ok();
    }

    /// What the scan reads of an archive: its charts, in all no more than
    /// it is given room for; a file larger than a chart may be is passed
    /// by; an archive that lists or decodes to more than one may is not
    /// read.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_scan_reads_within_limits() {
        use io::ErrorKind::{InvalidData, OutOfMemory};
        let d = std::env::temp_dir().join(format!("vibebms-arcfs-scan-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let big = vec![b' '; CHART_MAX as usize + 1];
        let files: [(&str, &[u8]); 4] = [
            ("Song/a.bms", b"#TITLE A"),
            ("Song/big.bms", &big),
            ("Song/s.wav", b"RIFF"),
            ("Song/z.bme", b"#TITLE Z"),
        ];
        let charts = [
            (PathBuf::from("Song/a.bms"), b"#TITLE A".to_vec()),
            (PathBuf::from("Song/z.bme"), b"#TITLE Z".to_vec()),
        ];
        use bmseeker::testkit::{tar, tar_gz, zip};
        for (name, bytes) in [
            ("p.zip", zip(&files)),
            ("p.tar", tar(&files)),
            ("p.tar.gz", tar_gz(&files)),
        ] {
            let a = d.join(name);
            std::fs::write(&a, bytes).unwrap();
            let u = read_charts(&a, None).unwrap();
            assert_eq!(u.files, charts, "{name}");
            let listed: Vec<(PathBuf, u64)> = u.listing.unwrap().into_iter().collect();
            let sizes = [8, big.len() as u64, 4, 8];
            let all = files
                .iter()
                .zip(sizes)
                .map(|((p, _), n)| (PathBuf::from(*p), n));
            assert_eq!(listed, all.collect::<Vec<_>>(), "{name}");
            // asked for by name, the large one is still none
            let want = ["Song/big.bms", "Song/z.bme"].map(PathBuf::from).into();
            let u = read_charts(&a, Some(&want)).unwrap();
            assert_eq!(u.files, charts[1..], "{name}");

            // 16 bytes of charts
            let chart = |r: &Path| vibebms_core::is_chart(r);
            let read = |cap| unpack(&a, &chart, true, None, &Dest::Charts, cap).map(|u| u.files);
            assert_eq!(read(16).unwrap(), charts, "{name}");
            assert_eq!(read(15).err().unwrap().kind(), OutOfMemory, "{name}");
            // 4 files
            let list = |limits| {
                unpack_within(&a, &|_| false, true, None, &Dest::Memory, 0, limits)
                    .map(|u| u.listing.unwrap().len())
            };
            let entries = |members| Limits { members, ..LIMITS };
            assert_eq!(list(entries(4)).unwrap(), 4, "{name}");
            assert_eq!(
                list(entries(3)).err().unwrap().kind(),
                OutOfMemory,
                "{name}"
            );
            // 32 MiB to decode: a .tar.gz's, decompressed to be listed (a
            // zip's and a tar's files are listed unread)
            let decode = |bytes| Limits { bytes, ..LIMITS };
            assert_eq!(list(decode(64 << 20)).unwrap(), 4, "{name}");
            match name {
                "p.tar.gz" => assert_eq!(
                    list(decode(1 << 20)).err().unwrap().kind(),
                    InvalidData,
                    "{name}"
                ),
                _ => assert_eq!(list(decode(1 << 20)).unwrap(), 4, "{name}"),
            }
        }
        std::fs::remove_dir_all(&d).ok();
    }

    /// Names that are a device's on Windows, or that it would change, are
    /// other names in the list and on the disk, in every format.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn names_are_files_on_every_system() {
        let d = std::env::temp_dir().join(format!("vibebms-arcfs-names-{}", std::process::id()));
        let files = [
            ("Song/a.bms. ", "#TITLE A"),
            ("Song/aux.wav", "RIFF"),
            ("Song/COM1/x?.bmp", "BM"),
        ];
        let safe = ["Song/_COM1/x_.bmp", "Song/_aux.wav", "Song/a.bms"].map(PathBuf::from);
        use bmseeker::testkit::{tar, zip};
        for (name, bytes) in [("n.zip", zip(&files)), ("n.tar", tar(&files))] {
            let out = d.join(name).with_extension("out");
            std::fs::create_dir_all(&out).unwrap();
            let a = d.join(name);
            std::fs::write(&a, bytes).unwrap();
            let dest = Dest::Folder(out.clone());
            let u = unpack(&a, &|_| true, true, None, &dest, 1 << 20).unwrap();
            let listed: Vec<PathBuf> = u.listing.unwrap().into_keys().collect();
            assert_eq!(listed, safe, "{name}");
            for rel in &safe {
                assert!(out.join(rel).is_file(), "{name}: {}", rel.display());
            }
            // read by the name listed
            let want = |r: &Path| r == safe[1];
            let u = unpack(&a, &want, false, Some(1), &Dest::Memory, 1 << 20).unwrap();
            assert_eq!(u.files, [(safe[1].clone(), b"RIFF".to_vec())], "{name}");
        }
        std::fs::remove_dir_all(&d).ok();
    }

    /// A zip is read as BMSeeker reads one: a file more than 32 folders
    /// deep is not listed; one whose local header names another file than
    /// its directory is listed and refused when it is read; one joined
    /// after another zip is read from its first byte (the zip before it,
    /// whose end is near enough the file's to be found).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn zips_as_bmseeker_reads_them() {
        let d = std::env::temp_dir().join(format!("vibebms-arcfs-zips-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let list = |name: &str, bytes: &[u8]| {
            let a = d.join(name);
            std::fs::write(&a, bytes).unwrap();
            let u = unpack(&a, &|_| false, true, None, &Dest::Memory, 0)?;
            Ok::<_, io::Error>(u.listing.unwrap().into_keys().collect::<Vec<_>>())
        };
        let deep = "d/".repeat(33) + "x.wav";
        let files = [("Song/a.bms", "#TITLE A"), (deep.as_str(), "RIFF")];
        let z = bmseeker::testkit::zip(&files);
        assert_eq!(list("deep.zip", &z).unwrap(), [PathBuf::from("Song/a.bms")]);
        let mut other = bmseeker::testkit::zip(&[("x.txt", "x")]);
        other.extend(&z);
        assert_eq!(
            list("joined.zip", &other).unwrap(),
            [PathBuf::from("x.txt")]
        );
        let at = z.windows(10).position(|w| w == b"Song/a.bms").unwrap();
        let mut differs = z.clone();
        differs[at..at + 10].copy_from_slice(b"Song/b.bms");
        let chart = [PathBuf::from("Song/a.bms")];
        assert_eq!(list("differs.zip", &differs).unwrap(), chart);
        let want = |r: &Path| r == chart[0];
        let a = d.join("differs.zip");
        let e = unpack(&a, &want, false, Some(1), &Dest::Memory, 1 << 20)
            .err()
            .unwrap();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{e}");
        assert!(e.to_string().contains("local header"), "{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    /// A RAR's file whose size it does not give is listed by the bytes it
    /// unpacks to, and read when there is room for them.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_rar_file_of_no_size_is_listed_and_read() {
        let d = std::env::temp_dir().join(format!("vibebms-arcfs-sizeless-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let rar = d.join("p.rar");
        std::fs::write(&rar, bmseeker::testkit::SIZELESS_RAR).unwrap();
        let u = unpack(&rar, &|_| false, true, None, &Dest::Memory, 0).unwrap();
        let listed: Vec<(PathBuf, u64)> = u.listing.unwrap().into_iter().collect();
        assert_eq!(listed, [("a.zip".into(), 4330), ("b.zip".into(), 8281)]);
        let want = |r: &Path| r == Path::new("a.zip");
        let u = unpack(&rar, &want, false, Some(1), &Dest::Memory, 8 << 10).unwrap();
        assert_eq!(u.files[0].1.len(), 4330);
        let e = unpack(&rar, &want, false, Some(1), &Dest::Memory, 4 << 10)
            .err()
            .unwrap();
        assert_eq!(e.kind(), io::ErrorKind::OutOfMemory);
        std::fs::remove_dir_all(&d).ok();
    }

    /// A path an archive has twice (names alike once made safe, a tar's
    /// file appended again) is one of the files counted: the files after
    /// it are still read.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_path_twice_is_one_file_counted() {
        let d = std::env::temp_dir().join(format!("vibebms-arcfs-twice-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let files = [
            ("Song//a.bms", "#TITLE X"),
            ("Song/a.bms", "#TITLE A"),
            ("Song/key.wav", "RIFF"),
        ];
        use bmseeker::testkit::{tar, zip};
        for (name, bytes) in [("t.zip", zip(&files)), ("t.tar", tar(&files))] {
            let a = d.join(name);
            std::fs::write(&a, bytes).unwrap();
            let u = unpack(&a, &|_| false, true, None, &Dest::Memory, 0).unwrap();
            let count = u.listing.unwrap().len();
            assert_eq!(count, 2, "{name}");
            let song = |r: &Path| r.starts_with("Song");
            let u = unpack(&a, &song, false, Some(count), &Dest::Memory, 1 << 20).unwrap();
            let got: Vec<&Path> = u.files.iter().map(|(p, _)| p.as_path()).collect();
            assert_eq!(
                got,
                ["Song/a.bms", "Song/a.bms", "Song/key.wav"].map(Path::new),
                "{name}"
            );
        }
        std::fs::remove_dir_all(&d).ok();
    }

    /// A file an archive gives no size of (a RAR may not) is listed by
    /// the bytes it unpacks to; one with a size is listed by it, unread.
    #[test]
    fn a_file_of_no_size_is_listed_by_its_bytes() {
        let mut sink = Taken {
            want: &|_| false,
            dest: &Dest::Memory,
            list: true,
            count: None,
            sizeless: true,
            left: 0,
            listing: BTreeMap::new(),
            files: Vec::new(),
            taken: HashSet::new(),
            keep: Keep::Skip,
            too_large: false,
        };
        let m = |rel: &str, size| Member {
            rel: PathBuf::from(rel),
            size,
            mtime: None,
        };
        let Ok(Take::Write(mut w)) = sink.open(&m("a.wav", 0)) else {
            panic!("not written");
        };
        w.write_all(b"RIFFxxxx").unwrap();
        drop(w);
        sink.close(m("a.wav", 8));
        assert!(matches!(sink.open(&m("b.wav", 3)), Ok(Take::Skip)));
        let listed: Vec<(PathBuf, u64)> = sink.listing.into_iter().collect();
        assert_eq!(listed, [("a.wav".into(), 8), ("b.wav".into(), 3)]);
        assert!(sink.files.is_empty());
    }
}
