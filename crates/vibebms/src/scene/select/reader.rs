//! READ TEXT: the selected chart's text files (LR2's readme viewer) in a
//! scrolling box over the select screen (`ui::Reader`). Skins cannot
//! scroll text, so the scene draws it.

use super::SelectScene;
use crate::ctx::Ctx;
use crate::gfx::Draw;
use crate::input::InputEv;
use crate::platform::vfs::{self, Prefetch};
use crate::ui::{self, Read};
use std::borrow::Cow;
use std::path::PathBuf;
use winit::keyboard::KeyCode;

pub(super) struct Reader {
    /// The files while they are read (the web: from the browser's storage).
    fetch: Option<Prefetch>,
    /// The file (index in `files`) each line is of.
    line_file: Vec<usize>,
    files: Vec<PathBuf>,
    view: ui::Reader,
}

impl Reader {
    fn new(title: String, files: Vec<PathBuf>) -> Reader {
        Reader {
            fetch: Some(Prefetch::files(files.clone())),
            line_file: Vec::new(),
            files,
            view: ui::Reader::new(tr!("READ TEXT").into(), title),
        }
    }

    /// The files read: their lines, each file after its name when there
    /// are several.
    fn update(&mut self) {
        if self.fetch.take_if(|f| f.finished()).is_none() {
            return;
        }
        let several = self.files.len() > 1;
        let mut lines: Vec<Cow<'static, str>> = Vec::new();
        for (k, p) in self.files.iter().enumerate() {
            if several {
                if k > 0 {
                    lines.push(Cow::Borrowed(""));
                }
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                let name = format!("\u{2014} {} \u{2014}", name.unwrap_or_default());
                lines.push(Cow::Owned(name));
            }
            match vfs::read(p) {
                Ok(b) => {
                    let text = vibebms_core::encoding::decode_text(&b);
                    lines.extend(text.lines().map(|l| Cow::Owned(l.replace('\t', "    "))));
                }
                Err(e) => lines.push(Cow::Owned(format!("({e})"))),
            }
            self.line_file.resize(lines.len(), k);
        }
        self.view.set_lines(lines);
    }

    /// The file shown at the top of the view (the only one, or the one
    /// whose text is there).
    fn file_shown(&self) -> Option<&PathBuf> {
        let k = self.line_file.get(self.view.top_line()).copied();
        self.files.get(k.unwrap_or(0))
    }

    /// The file shown in the system's text viewer (desktop; one in an
    /// archive: the archive in the file manager).
    fn open_shown(&self) {
        let Some(p) = self.file_shown() else { return };
        let disk = super::files::on_disk(p);
        if disk == *p {
            crate::platform::open::open_file(p);
        } else {
            crate::platform::open::reveal(&disk);
        }
    }

    fn draw(&mut self, ctx: &Ctx, d: &mut Draw) {
        // OPEN (O): the file shown in the system's viewer
        let open = crate::platform::open::CAN_OPEN_FILES.then(|| tr!("OPEN (O)"));
        self.view.draw(ctx, d, open);
    }

    /// An input while the text is open; false: it closes. T closes it as
    /// it opened it.
    fn input(&mut self, ctx: &Ctx, ev: &InputEv) -> bool {
        if let InputEv::Key {
            code,
            pressed: true,
            ..
        } = ev
        {
            match code {
                KeyCode::KeyT => return false,
                KeyCode::KeyO => {
                    self.open_shown();
                    return true;
                }
                _ => {}
            }
        }
        match self.view.input(ctx, ev) {
            Read::Open => true,
            Read::Closed => false,
            Read::Button => {
                self.open_shown();
                true
            }
        }
    }
}

impl SelectScene {
    /// Open the selected chart's text files.
    pub(super) fn read_text(&mut self, ctx: &mut Ctx) {
        let Some((_, song)) = self.selected_song(ctx) else {
            return;
        };
        let files = self.chart_files(ctx, song).texts.clone();
        if files.is_empty() {
            ctx.toast(tr!("{title}: no text files", title = song.title));
            return;
        }
        self.reader = Some(Reader::new(song.full_title(), files));
    }

    pub(super) fn update_reader(&mut self) {
        if let Some(r) = self.reader.as_mut() {
            r.update();
        }
    }

    pub(super) fn draw_reader(&mut self, ctx: &Ctx, d: &mut Draw) {
        if let Some(r) = self.reader.as_mut() {
            r.draw(ctx, d);
        }
    }

    /// An input for the open text: true when it had one.
    pub(super) fn reader_input(&mut self, ctx: &Ctx, ev: &InputEv) -> bool {
        let Some(r) = self.reader.as_mut() else {
            return false;
        };
        if !r.input(ctx, ev) {
            self.reader = None;
        }
        true
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The chart's text files are read (Shift_JIS too), each after its
    /// name when there are several.
    #[test]
    fn reads_the_files() {
        let dir = std::env::temp_dir().join(format!("vibebms-reader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        // "曲" in Shift_JIS
        std::fs::write(&a, b"\x8b\xc8\r\nline\ttwo").unwrap();
        std::fs::write(&b, "third").unwrap();
        let mut r = Reader::new("song".into(), vec![a, b]);
        r.update();
        assert_eq!(
            r.view.lines().unwrap(),
            [
                "\u{2014} a.txt \u{2014}",
                "\u{66f2}",
                "line    two",
                "",
                "\u{2014} b.txt \u{2014}",
                "third"
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
