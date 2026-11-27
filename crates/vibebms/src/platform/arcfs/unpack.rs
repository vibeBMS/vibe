//! Reading an archive: its list of files, the files wanted (into memory
//! or a folder), the rest not kept.

use super::safe_rel;
use crate::platform::vfs;
use bmseeker::fs::ReadSeek;
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

/// What is read of an archive at most, whatever is wanted of it.
#[derive(Clone, Copy)]
struct Limits {
    /// The files it may list (each costs memory and a line of
    /// `cache/archives.json`): a pack of thousands of songs has some
    /// hundreds of thousands. One with more is not read ([`too_large`]).
    entries: usize,
    /// What a 7z, RAR, LZH or tar may decode to (bytes; what is passed by
    /// counts too): a small one can decode to anything, and a real one
    /// this large takes minutes to reach a song in.
    decode: u64,
}

const LIMITS: Limits = Limits {
    entries: if cfg!(small_device) {
        250_000
    } else {
        1_000_000
    },
    decode: 64 << 30,
};

/// How many times `archive` was opened (tests).
#[cfg(test)]
pub fn times_opened(archive: &Path) -> usize {
    super::state().opened.get(archive).copied().unwrap_or(0)
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
    {
        Ok(Box::new(io::BufReader::new(std::fs::File::open(archive)?)))
    }
    // (in memory once read in: `vfs::Prefetch`)
    #[cfg(target_arch = "wasm32")]
    {
        Ok(Box::new(io::Cursor::new(vfs::read(archive)?)))
    }
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

/// The first bytes of an archive its format is told by.
const HEAD: usize = 4096;

/// Said by [`Capture`] once it has what it wants (the rest is not decoded).
const ENOUGH: &str = "vibebms: the files wanted are read";

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
/// `count`: how many files are wanted, if known (a 7z, RAR, LZH or tar is
/// not decoded past the last of them; not with `list`). An error of kind
/// `WouldBlock` (web): the archive is not in memory yet; of kind
/// `OutOfMemory`: the files wanted are more than `cap`, or it lists more
/// files than an archive may ([`LIMITS`]); of kind `InvalidData`: it
/// cannot be read, or decodes to more than one may.
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
    if format == bmseeker::unpack::Format::Zip {
        return unzip(r, want, list, dest, cap, limits.entries);
    }
    let cap = Capture {
        want,
        dest,
        count: count.filter(|_| !list),
        cap,
        entries: limits.entries,
        reader: Mutex::new(Some(r)),
        out: Mutex::default(),
    };
    let r = bmseeker::unpack::unpack(
        bmseeker::unpack::Blob::File(archive.to_path_buf()),
        &cap,
        Path::new(""),
        limits.decode,
    );
    let out = cap.out.into_inner().unwrap_or_else(|e| e.into_inner());
    match r {
        _ if out.too_large => return Err(too_large()),
        Ok(_) => {}
        Err(e) if e.contains(ENOUGH) && !list => {}
        Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    }
    Ok(Unpacked {
        format: format.ext().into(),
        listing: list.then_some(out.listing),
        files: out.files,
    })
}

/// A zip: its list from its central directory, the files wanted read
/// where they are (nothing else decoded).
fn unzip(
    r: Box<dyn ReadSeek + Send>,
    want: &(dyn Fn(&Path) -> bool + Sync),
    list: bool,
    dest: &Dest,
    cap: u64,
    entries: usize,
) -> io::Result<Unpacked> {
    let bad = |e: zip::result::ZipError| io::Error::new(io::ErrorKind::InvalidData, e);
    let mut z = zip::ZipArchive::new(r).map_err(bad)?;
    if z.len() > entries {
        return Err(too_large());
    }
    let mut listing = BTreeMap::new();
    let mut wanted = Vec::new();
    for i in 0..z.len() {
        let e = z.by_index_data(i).map_err(bad)?;
        // (as BMSeeker unpacks a zip: no folders, no links)
        if e.is_dir() || e.is_symlink() {
            continue;
        }
        let Some(rel) = safe_rel(&vfs::decode_name(e.name_raw())) else {
            continue;
        };
        if want(&rel) {
            wanted.push((i, rel.clone()));
        }
        listing.insert(rel, e.size());
    }
    let mut files = Vec::new();
    let mut left = cap;
    let each = dest.each();
    for (i, rel) in wanted {
        let e = z.by_index(i).map_err(bad)?;
        // (by the size it says, before any of it is read)
        if e.size() > each {
            continue;
        }
        if e.size() > left {
            return Err(too_large());
        }
        let mut e = e.take(left.min(each).saturating_add(1));
        match dest {
            Dest::Memory | Dest::Charts => {
                let mut b = Vec::new();
                e.read_to_end(&mut b)?;
                // (larger than it said)
                if b.len() as u64 > each {
                    continue;
                }
                left = left.checked_sub(b.len() as u64).ok_or_else(too_large)?;
                files.push((rel, b));
            }
            #[cfg(not(target_arch = "wasm32"))]
            Dest::Folder(d) => {
                let p = d.join(&rel);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut f = io::BufWriter::new(std::fs::File::create(&p)?);
                let n = io::copy(&mut e, &mut f)?;
                f.flush()?;
                left = left.checked_sub(n).ok_or_else(too_large)?;
            }
        }
    }
    Ok(Unpacked {
        format: "zip".into(),
        listing: list.then_some(listing),
        files,
    })
}

/// BMSeeker's unpacking into this: the files wanted kept, the others only
/// counted.
struct Capture<'a> {
    want: &'a (dyn Fn(&Path) -> bool + Sync),
    dest: &'a Dest,
    count: Option<usize>,
    cap: u64,
    /// The files it may list.
    entries: usize,
    /// The archive, read once.
    reader: Mutex<Option<Box<dyn ReadSeek + Send>>>,
    out: Mutex<CaptureOut>,
}

#[derive(Default)]
struct CaptureOut {
    listing: BTreeMap<PathBuf, u64>,
    files: Vec<(PathBuf, Vec<u8>)>,
    /// Files wanted taken.
    taken: usize,
    kept: u64,
    too_large: bool,
}

enum Keep {
    Skip,
    Memory(Vec<u8>),
    #[cfg(not(target_arch = "wasm32"))]
    File(io::BufWriter<std::fs::File>),
}

/// A file being unpacked.
struct Member<'a> {
    cap: &'a Capture<'a>,
    /// Its path (`None`: its name is no file's, and it is not listed).
    rel: Option<PathBuf>,
    keep: Keep,
    n: u64,
    /// What it may keep.
    left: u64,
}

impl Write for Member<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.n += buf.len() as u64;
        if self.n > self.cap.dest.each() {
            // passed by: what was kept of it goes
            self.keep = Keep::Skip;
        }
        let over = || {
            self.cap
                .out
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .too_large = true;
            too_large()
        };
        match &mut self.keep {
            Keep::Skip => {}
            Keep::Memory(v) => {
                // (the scan's is judged once whole: it may yet be one
                // passed by)
                if self.n > self.left && !matches!(self.cap.dest, Dest::Charts) {
                    return Err(over());
                }
                v.extend_from_slice(buf);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Keep::File(f) => {
                if self.n > self.left {
                    return Err(over());
                }
                f.write_all(buf)?;
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        match &mut self.keep {
            #[cfg(not(target_arch = "wasm32"))]
            Keep::File(f) => f.flush(),
            _ => Ok(()),
        }
    }
}

impl Drop for Member<'_> {
    fn drop(&mut self) {
        let Some(rel) = self.rel.take() else { return };
        let mut out = self.cap.out.lock().unwrap_or_else(|e| e.into_inner());
        out.listing.insert(rel.clone(), self.n);
        match std::mem::replace(&mut self.keep, Keep::Skip) {
            Keep::Skip => return,
            Keep::Memory(_) if self.n > self.left => {
                out.too_large = true;
                return;
            }
            Keep::Memory(mut v) => {
                // (kept while its song is: not the room it grew into)
                v.shrink_to_fit();
                out.files.push((rel, v))
            }
            #[cfg(not(target_arch = "wasm32"))]
            Keep::File(_) => {}
        }
        out.taken += 1;
        out.kept += self.n;
    }
}

impl bmseeker::fs::Fs for Capture<'_> {
    fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            p.display().to_string(),
        ))
    }
    fn read_dir(&self, _: &Path) -> io::Result<Vec<bmseeker::fs::Entry>> {
        Ok(Vec::new())
    }
    fn rename(&self, _: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn stamp(&self, _: &Path) -> Option<(i64, u64)> {
        None
    }
    /// A file that could not be unpacked in full: it goes.
    fn remove(&self, p: &Path) -> io::Result<()> {
        let Some(p) = &safe_rel(&p.to_string_lossy()) else {
            return Ok(());
        };
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = out.files.iter().position(|(r, _)| r == p) {
            let (_, b) = out.files.remove(i);
            out.taken -= 1;
            out.kept -= b.len() as u64;
        }
        out.listing.remove(p);
        #[cfg(not(target_arch = "wasm32"))]
        if let Dest::Folder(d) = self.dest {
            let _ = std::fs::remove_file(d.join(p));
        }
        Ok(())
    }
}

impl bmseeker::fs::Store for Capture<'_> {
    fn create(&self, p: &Path, _mtime: Option<i64>) -> io::Result<Box<dyn Write + '_>> {
        let (left, enough) = {
            let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
            if out.too_large || out.listing.len() >= self.entries {
                out.too_large = true;
                return Err(too_large());
            }
            (
                self.cap.saturating_sub(out.kept),
                self.count.is_some_and(|n| out.taken >= n),
            )
        };
        if enough {
            return Err(io::Error::other(ENOUGH));
        }
        // (BMSeeker's path of it, made a file's name here too)
        let rel = safe_rel(&p.to_string_lossy());
        let keep = match &rel {
            Some(r) if (self.want)(r) => match self.dest {
                Dest::Memory | Dest::Charts => Keep::Memory(Vec::new()),
                #[cfg(not(target_arch = "wasm32"))]
                Dest::Folder(d) => {
                    let to = d.join(r);
                    if let Some(parent) = to.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    Keep::File(io::BufWriter::new(std::fs::File::create(to)?))
                }
            },
            _ => Keep::Skip,
        };
        Ok(Box::new(Member {
            cap: self,
            rel,
            keep,
            n: 0,
            left,
        }))
    }
    fn remove_dir(&self, _: &Path) {}
    /// The archive (once); an archive in it is not read.
    fn open(&self, p: &Path) -> io::Result<Box<dyn ReadSeek + Send>> {
        let r = self.reader.lock().unwrap_or_else(|e| e.into_inner()).take();
        r.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, p.display().to_string()))
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
            let entries = |entries| Limits { entries, ..LIMITS };
            assert_eq!(list(entries(4)).unwrap(), 4, "{name}");
            assert_eq!(
                list(entries(3)).err().unwrap().kind(),
                OutOfMemory,
                "{name}"
            );
            // 32 MiB to decode, in the formats read from their start
            if name != "p.zip" {
                let decode = |decode| Limits { decode, ..LIMITS };
                assert_eq!(list(decode(64 << 20)).unwrap(), 4, "{name}");
                assert_eq!(
                    list(decode(1 << 20)).err().unwrap().kind(),
                    InvalidData,
                    "{name}"
                );
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
}
