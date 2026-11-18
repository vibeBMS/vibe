//! A settings row and the builder pages emit them with: sections, foldable
//! groups (their rows only while open), and the section each row is in.

use super::{Confirm, Screen, Sub, TextField};
use crate::app::Ctx;
use crate::scene::keybind::Slot;
use crate::skin::SkinType;
use crate::ui::Armed;
use std::collections::HashSet;

/// What choosing a row does.
#[derive(Clone, Copy)]
pub(super) enum Act {
    None,
    /// Change a value by a step (←→; Enter and taps step up).
    Adjust(fn(&mut Ctx, i32)),
    /// Do `.0` for item `.1` of the page's list (a folder, a table, ...).
    Run(fn(&mut Ctx, usize), usize),
    /// Turn item `.1` on or off: ←→ and Enter alike.
    Toggle(fn(&mut Ctx, usize), usize),
    Text(TextField),
    Bind(Slot),
    Open(Screen),
    /// The skin chosen for a type: the default, then the installed ones.
    CycleSkin(SkinType),
    /// Custom option `.1` of the skin chosen for `.0`.
    SkinOption(SkinType, usize),
    /// Field `.2` (x, y, w, h, angle, alpha) of offset `.1` of that skin.
    SkinOffset(SkinType, usize, usize),
    /// Install catalog skin `.0` (an installed one again on a second
    /// press; a downloading one: cancel).
    SkinGet(usize),
    /// Delete catalog skin `.0`, on a second press.
    SkinDelete(usize),
    /// Delete profile `.0` (of the list), on a second press.
    ProfileDelete(usize),
    /// Web: delete kept song folder `.0` (see `web_storage`), on a second
    /// press.
    #[cfg(target_arch = "wasm32")]
    DeleteKept(usize),
    /// Leave and start the updated executable.
    Restart,
}

impl Act {
    /// ←→ step the value (else they do nothing, and Enter acts).
    pub(super) fn adjusts(self) -> bool {
        matches!(
            self,
            Act::Adjust(_)
                | Act::Toggle(..)
                | Act::CycleSkin(_)
                | Act::SkinOption(..)
                | Act::SkinOffset(..)
        )
    }

    /// The act waits for a second press ([`Confirm`]).
    pub(super) fn confirms(self) -> bool {
        match self {
            Act::SkinGet(_) | Act::SkinDelete(_) | Act::ProfileDelete(_) => true,
            #[cfg(target_arch = "wasm32")]
            Act::DeleteKept(_) => true,
            _ => false,
        }
    }
}

/// The colour of a card's state chip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Tone {
    Accent,
    Amber,
    Red,
    Dim,
}

#[derive(Clone)]
pub(super) enum Kind {
    Item,
    /// A section's title; never selected. Its links are tapped (each a
    /// shortcut the rows below also offer).
    Header(Vec<(&'static str, Act)>),
    /// A foldable group of rows: Enter or a tap folds it.
    Group {
        id: &'static str,
        open: bool,
    },
    /// A line across the page that only says something.
    Banner,
    /// An account: a tile with its initials, a state chip.
    Card {
        badge: &'static str,
        state: String,
        tone: Tone,
    },
    /// A check box in a grid of them.
    Cell {
        on: bool,
    },
}

#[derive(Clone)]
pub(super) struct Row {
    /// What it is, for search and the tests: the config key it sets or
    /// the action it takes (rows of a list share one).
    pub id: &'static str,
    pub label: String,
    /// One line under the label.
    pub desc: String,
    /// Shown at the right (with ◀ ▶ around it when ←→ change it).
    pub value: String,
    pub act: Act,
    /// The sub-page Enter opens (←→ still change the value).
    pub open: Option<Sub>,
    pub kind: Kind,
    /// More words search finds it by.
    pub keys: &'static str,
    /// Inside a group (indented).
    pub group: Option<&'static str>,
    /// The section or group it is in.
    pub section: String,
}

impl Row {
    pub(super) fn new(
        id: &'static str,
        label: impl Into<String>,
        value: impl Into<String>,
        act: Act,
    ) -> Row {
        Row {
            id,
            label: label.into(),
            desc: String::new(),
            value: value.into(),
            act,
            open: None,
            kind: Kind::Item,
            keys: "",
            group: None,
            section: String::new(),
        }
    }

    pub(super) fn adjust(
        id: &'static str,
        label: impl Into<String>,
        value: impl Into<String>,
        f: fn(&mut Ctx, i32),
    ) -> Row {
        Row::new(id, label, value, Act::Adjust(f))
    }

    /// A line that only says something.
    pub(super) fn note(
        id: &'static str,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Row {
        Row::new(id, label, value, Act::None)
    }

    /// A row opening sub-page `sub`.
    pub(super) fn sub(
        id: &'static str,
        label: impl Into<String>,
        value: impl Into<String>,
        sub: Sub,
    ) -> Row {
        Row::new(id, label, value, Act::None).opens(sub)
    }

    pub(super) fn desc(mut self, d: impl Into<String>) -> Row {
        self.desc = d.into();
        self
    }

    pub(super) fn keys(mut self, k: &'static str) -> Row {
        self.keys = k;
        self
    }

    pub(super) fn opens(mut self, sub: Sub) -> Row {
        self.open = Some(sub);
        self
    }

    pub(super) fn kind(mut self, k: Kind) -> Row {
        self.kind = k;
        self
    }

    /// Up / down and taps land on it.
    pub(super) fn focusable(&self) -> bool {
        !matches!(self.kind, Kind::Header(_) | Kind::Banner)
    }

    /// Enter or a tap does something.
    pub(super) fn acts(&self) -> bool {
        self.open.is_some()
            || !matches!(self.act, Act::None)
            || matches!(self.kind, Kind::Group { .. })
    }

    /// Everything search reads, lowercase.
    pub(super) fn haystack(&self) -> String {
        format!("{} {} {} {}", self.label, self.desc, self.keys, self.id).to_lowercase()
    }
}

/// Which groups are open: those the player opened this session, or all
/// (search and the page tree see every row).
pub(super) enum Folds<'a> {
    Session(&'a HashSet<&'static str>),
    All,
}

/// The rows of a page as it builds them.
pub(super) struct Rows<'a> {
    pub ctx: &'a Ctx,
    pub armed: &'a Armed<Confirm>,
    folds: Folds<'a>,
    pub v: Vec<Row>,
    section: String,
    group: Option<&'static str>,
    /// In a folded group: its rows are left out.
    hidden: bool,
}

impl<'a> Rows<'a> {
    pub(super) fn new(ctx: &'a Ctx, armed: &'a Armed<Confirm>, folds: Folds<'a>) -> Rows<'a> {
        Rows {
            ctx,
            armed,
            folds,
            v: Vec::new(),
            section: String::new(),
            group: None,
            hidden: false,
        }
    }

    /// A section; it ends the group before it.
    pub(super) fn header(&mut self, title: impl Into<String>) {
        self.header_with(title, Vec::new());
    }

    pub(super) fn header_with(
        &mut self,
        title: impl Into<String>,
        links: Vec<(&'static str, Act)>,
    ) {
        let title = title.into();
        (self.group, self.hidden) = (None, false);
        self.section = title.clone();
        self.v
            .push(Row::note("section", title, "").kind(Kind::Header(links)));
    }

    /// A foldable group: the rows pushed after it, up to the next header
    /// or group, are its own. `summary` names them.
    pub(super) fn group(&mut self, id: &'static str, title: &str, summary: &str) {
        let open = match self.folds {
            Folds::Session(s) => s.contains(id),
            Folds::All => true,
        };
        self.section = title.to_string();
        self.group = None;
        self.hidden = false;
        self.push(
            Row::note("group", title, "")
                .desc(summary)
                .kind(Kind::Group { id, open }),
        );
        (self.group, self.hidden) = (Some(id), !open);
    }

    pub(super) fn push(&mut self, mut r: Row) {
        if self.hidden {
            return;
        }
        r.group = self.group;
        r.section.clone_from(&self.section);
        self.v.push(r);
    }

    pub(super) fn extend(&mut self, rows: impl IntoIterator<Item = Row>) {
        for r in rows {
            self.push(r);
        }
    }
}

/// `v` (0..1) in percent.
pub(super) fn percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

/// `n` bytes as MB for people.
pub(super) fn mb(n: u64) -> String {
    format!("{} MB", (n + (1 << 19)) >> 20)
}

/// Milliseconds with their sign.
pub(super) fn ms(v: i32) -> String {
    format!("{v:+} ms")
}
