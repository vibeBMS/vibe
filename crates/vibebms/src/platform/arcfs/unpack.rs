//! Reading an archive: its list of files, the files wanted (into memory
//! or a folder), the rest not kept.

use super::{bmseeker_safe_rel, state};
use crate::platform::vfs;
use bmseeker::fs::ReadSeek;
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The largest chart file read by the scan (larger ones are no chart).
const CHART_MAX: u64 = 32 << 20;

/// How many times `archive` was opened (tests).
pub fn times_opened(archive: &Path) -> usize {
    state().opened.get(archive).copied().unwrap_or(0)
}

fn open(archive: &Path) -> io::Result<Box<dyn ReadSeek + Send>> {
    *state().opened.entry(archive.to_path_buf()).or_default() += 1;
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

fn too_large() -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, "the files wanted are too large")
}

/// Read the archive `archive`: the files `want` takes into `dest` (at
/// most `cap` bytes of them), and with `list` the list of all its files.
/// `count`: how many files are wanted, if known (a 7z, RAR, LZH or tar is
/// not decoded past the last of them; not with `list`). An error of kind
/// `WouldBlock` (web): the archive is not in memory yet.
pub fn unpack(
    archive: &Path,
    want: &(dyn Fn(&Path) -> bool + Sync),
    list: bool,
    count: Option<usize>,
    dest: &Dest,
    cap: u64,
) -> io::Result<Unpacked> {
    let mut r = open(archive)?;
    // (a tar's first header; a gzip's compressed)
    let mut head = Vec::with_capacity(HEAD);
    (&mut r).take(HEAD as u64).read_to_end(&mut head)?;
    r.seek(io::SeekFrom::Start(0))?;
    let format = bmseeker::unpack::sniff(&head)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not an archive"))?;
    if format == bmseeker::unpack::Format::Zip {
        return unzip(r, want, list, dest, cap);
    }
    let cap = Capture {
        want,
        dest,
        count: count.filter(|_| !list),
        cap,
        reader: Mutex::new(Some(r)),
        out: Mutex::default(),
    };
    let r = bmseeker::unpack::unpack(
        bmseeker::unpack::Blob::File(archive.to_path_buf()),
        &cap,
        Path::new(""),
        u64::MAX,
    );
    let out = cap.out.into_inner().unwrap_or_else(|e| e.into_inner());
    match r {
        Ok(_) => {}
        Err(e) if e.contains(ENOUGH) && !list => {}
        Err(_) if out.too_large => return Err(too_large()),
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
) -> io::Result<Unpacked> {
    let bad = |e: zip::result::ZipError| io::Error::new(io::ErrorKind::InvalidData, e);
    let mut z = zip::ZipArchive::new(r).map_err(bad)?;
    let mut listing = BTreeMap::new();
    let mut wanted = Vec::new();
    for i in 0..z.len() {
        let e = z.by_index_data(i).map_err(bad)?;
        // (as BMSeeker unpacks a zip: no folders, no links)
        if e.is_dir() || e.is_symlink() {
            continue;
        }
        let Some(rel) = bmseeker_safe_rel(&vfs::decode_name(e.name_raw())) else {
            continue;
        };
        if want(&rel) {
            wanted.push((i, rel.clone()));
        }
        listing.insert(rel, e.size());
    }
    let mut files = Vec::new();
    let mut left = cap;
    for (i, rel) in wanted {
        let e = z.by_index(i).map_err(bad)?;
        if e.size() > left {
            return Err(too_large());
        }
        let mut e = e.take(left + 1);
        match dest {
            Dest::Memory => {
                let mut b = Vec::new();
                e.read_to_end(&mut b)?;
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
    rel: PathBuf,
    keep: Keep,
    n: u64,
    /// What it may keep.
    left: u64,
}

impl Write for Member<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.n += buf.len() as u64;
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
                if self.n > self.left {
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
        let mut out = self.cap.out.lock().unwrap_or_else(|e| e.into_inner());
        out.listing.insert(self.rel.clone(), self.n);
        match std::mem::replace(&mut self.keep, Keep::Skip) {
            Keep::Skip => return,
            Keep::Memory(v) => out.files.push((self.rel.clone(), v)),
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
            let out = self.out.lock().unwrap_or_else(|e| e.into_inner());
            (
                self.cap.saturating_sub(out.kept),
                self.count.is_some_and(|n| out.taken >= n),
            )
        };
        if enough {
            return Err(io::Error::other(ENOUGH));
        }
        let keep = match (self.want)(p) {
            false => Keep::Skip,
            true => match self.dest {
                Dest::Memory => Keep::Memory(Vec::new()),
                #[cfg(not(target_arch = "wasm32"))]
                Dest::Folder(d) => {
                    let to = d.join(p);
                    if let Some(parent) = to.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    Keep::File(io::BufWriter::new(std::fs::File::create(to)?))
                }
            },
        };
        Ok(Box::new(Member {
            cap: self,
            rel: p.to_path_buf(),
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
/// all its charts, and its list of files). For the scan.
pub fn read_charts(archive: &Path, want: Option<&HashSet<PathBuf>>) -> io::Result<Unpacked> {
    let take = |r: &Path| crate::library::is_chart(r) && want.is_none_or(|w| w.contains(r));
    let count = want.map(HashSet::len);
    let mut u = unpack(
        archive,
        &take,
        want.is_none(),
        count,
        &Dest::Memory,
        CHART_MAX * 64,
    )?;
    u.files.retain(|(_, b)| b.len() as u64 <= CHART_MAX);
    Ok(u)
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
}
