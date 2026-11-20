//! The limits of a chart that is read. No real chart reaches them; a file
//! past one is refused by the decoders ([`ChartError::Parse`]), so that
//! what a file says cannot make reading it, listing it or playing it cost
//! without bound. Whatever takes a [`Chart`] from the decoders (the library
//! scan, previews, graphs, the loudness meter) counts on them and has no
//! limit of its own.

use crate::chart::Chart;
use crate::ChartError;

/// A chart file's size (bytes).
pub const MAX_FILE: usize = 32 << 20;

/// The time (µs) of a chart's last note or sound: 24 hours.
pub const MAX_LENGTH_US: i64 = 24 * 3_600 * 1_000_000;

/// A chart's objects: notes, background sounds, BGA and timing events,
/// channel lines and bar lines, sound and image definitions.
pub const MAX_OBJECTS: usize = 4_000_000;

/// A header's text and a file name (bytes): the rest is cut off.
pub const MAX_TEXT: usize = 1024;

/// `#RANDOM` and `#SWITCH` blocks open at once. (One left open by every
/// `#RANDOM` of a file that never writes `#ENDRANDOM` counts.)
pub const MAX_NESTING: usize = 65_536;

/// The largest number a chart gives (a BPM, a measure length, a `#TOTAL`,
/// a scroll rate), either side of 0.
pub const MAX_NUMBER: f64 = 1e9;

fn refused(what: String) -> ChartError {
    ChartError::Parse(what)
}

/// A file of `len` bytes may be read.
pub(crate) fn file(len: usize) -> Result<(), ChartError> {
    match len > MAX_FILE {
        true => Err(refused(format!(
            "the file is larger than {} MiB",
            MAX_FILE >> 20
        ))),
        false => Ok(()),
    }
}

/// A chart of `n` objects may be read.
pub(crate) fn objects(n: usize) -> Result<(), ChartError> {
    match n > MAX_OBJECTS {
        true => Err(refused(format!("more than {MAX_OBJECTS} objects"))),
        false => Ok(()),
    }
}

/// `n` blocks may be open.
pub(crate) fn nesting(n: usize) -> Result<(), ChartError> {
    match n > MAX_NESTING {
        true => Err(refused(format!(
            "more than {MAX_NESTING} #RANDOM blocks open"
        ))),
        false => Ok(()),
    }
}

/// A number a chart gives, if it is one the arithmetic on it stays finite
/// with.
pub(crate) fn number(v: f64) -> Option<f64> {
    (v.is_finite() && v.abs() <= MAX_NUMBER).then_some(v)
}

fn clip(text: &mut String) {
    if text.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}

/// A chart as a decoder made it, within the limits: refused when a note
/// or a sound is past [`MAX_LENGTH_US`]; its bar lines and BGA events past
/// it (after the last note, at a tempo near 0) left out; its texts cut to
/// [`MAX_TEXT`].
pub(crate) fn check(mut chart: Chart) -> Result<Chart, ChartError> {
    let within = |t: i64| (-MAX_LENGTH_US..=MAX_LENGTH_US).contains(&t);
    let notes = chart.notes.iter().flat_map(|n| [n.time, n.end_time()]);
    if !notes.chain(chart.bgm.iter().map(|b| b.time)).all(within) {
        return Err(refused(format!(
            "the chart is longer than {} hours",
            MAX_LENGTH_US / 3_600_000_000
        )));
    }
    chart.bar_lines.retain(|b| within(b.time));
    chart.bga.retain(|b| within(b.time));
    let m = &mut chart.meta;
    let texts = [
        &mut m.title,
        &mut m.subtitle,
        &mut m.artist,
        &mut m.subartist,
        &mut m.genre,
        &mut m.level,
        &mut m.stagefile,
        &mut m.banner,
        &mut m.backbmp,
        &mut m.preview,
    ];
    let names = chart.sound_files.iter_mut().chain(&mut chart.images);
    texts.into_iter().chain(names).for_each(clip);
    Ok(chart)
}
