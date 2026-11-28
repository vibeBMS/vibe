//! SYSTEM › Licences: the licence vibeBMS is under and the notices of the
//! works it is built from, in a reader. The texts are the repository's
//! own files, compiled in: what is shown is what CI checks, on every
//! platform (no package carries the files beside the program).

use crate::ui::Reader;
use crate::update::VERSION;
use std::borrow::Cow;

/// The files, in the order shown: vibeBMS's licence, the notices of what
/// it took (and the font licence they point to), the crates' licences.
const FILES: [(&str, &str); 5] = [
    ("LICENSE-MIT", include_str!("../../../../../LICENSE-MIT")),
    (
        "LICENSE-APACHE",
        include_str!("../../../../../LICENSE-APACHE"),
    ),
    (
        "THIRD_PARTY_NOTICES.md",
        include_str!("../../../../../THIRD_PARTY_NOTICES.md"),
    ),
    (
        "crates/vibebms/assets/fonts/OFL.txt",
        include_str!("../../../assets/fonts/OFL.txt"),
    ),
    (
        "THIRD_PARTY_LICENSES.md",
        include_str!("../../../../../THIRD_PARTY_LICENSES.md"),
    ),
];

/// The files' lines, each file after its name (as the song select's
/// reader shows several files); borrowed from the program, but for the
/// few with tabs.
fn lines() -> Vec<Cow<'static, str>> {
    let mut out = Vec::new();
    for (k, (name, text)) in FILES.iter().enumerate() {
        if k > 0 {
            out.push(Cow::Borrowed(""));
        }
        out.push(Cow::Owned(format!("\u{2014} {name} \u{2014}")));
        out.extend(text.lines().map(|l| match l.contains('\t') {
            true => Cow::Owned(l.replace('\t', "    ")),
            false => Cow::Borrowed(l),
        }));
    }
    out
}

pub(super) fn reader() -> Reader {
    let mut r = Reader::new(tr!("LICENCES").into(), format!("vibeBMS {VERSION}"));
    r.set_lines(lines());
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::text::{Align, Text, TextStyle};
    use crate::gfx::{Batch, Draw, Gfx};

    /// Each character of the texts past ASCII is drawn with the bundled
    /// font alone (as the web build is before it has loaded a CJK font):
    /// the Latin ones are Inter's, the Japanese names in the notices are
    /// not, and are drawn as its box, not left out.
    #[test]
    fn every_character_is_drawn() {
        let (gfx, mut text, mut b) = (Gfx::without_gpu(), Text::built_in(), Batch::default());
        let mut chars: Vec<char> = FILES.iter().flat_map(|f| f.1.chars()).collect();
        chars.retain(|c| !c.is_ascii());
        chars.sort();
        chars.dedup();
        assert!(chars.len() < 100, "{chars:?}");
        for c in chars {
            b.clear();
            let mut d = Draw {
                gfx: &gfx,
                text: &mut text,
                b: &mut b,
                pointer: None,
                text_input: false,
            };
            let w = d.text(
                &c.to_string(),
                0.0,
                0.0,
                TextStyle::new(16.0, [1.0; 4], Align::Left),
            );
            assert!(w > 0.0 && b.draws().count() > 0, "{c:?} is not drawn");
        }
    }
}
