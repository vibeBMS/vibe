//! What the background work reports, for the shared state to act on.

/// What a background subsystem's `poll` reports (a service's own report
/// made one of these), for `Ctx` to act on.
#[derive(Debug, PartialEq)]
pub enum AppEvent {
    Toast(String),
    /// Files came into the library: scan it again.
    Rescan,
    /// What the music select lists changed (an event's works, a table).
    LibraryChanged,
    /// A download page that needs a person: in the in-app browser where
    /// there is one (what it downloads goes to the library).
    Browse(String),
    /// A page for the system's browser.
    OpenUrl(String),
    /// Skins were added or replaced.
    SkinsChanged,
    /// An IR account was checked (who it is, whether its key holds): the
    /// configuration keeps it. (The outbox's progress is read as it is
    /// shown.)
    IrChanged,
    /// The player's own scores read from an IR: to be taken into the
    /// records (`ir::download`).
    IrScores(Box<crate::ir::download::Download>),
}

impl From<crate::archive::Polled> for AppEvent {
    fn from(p: crate::archive::Polled) -> AppEvent {
        use crate::archive::Polled as P;
        match p {
            P::Toast(s) => AppEvent::Toast(s),
            P::Rescan => AppEvent::Rescan,
            P::Browse(url) => AppEvent::Browse(url),
            P::LibraryChanged => AppEvent::LibraryChanged,
        }
    }
}

impl From<crate::songfetch::Polled> for AppEvent {
    fn from(p: crate::songfetch::Polled) -> AppEvent {
        use crate::songfetch::Polled as P;
        match p {
            P::Toast(s) => AppEvent::Toast(s),
            P::Rescan => AppEvent::Rescan,
            P::Browse(url) => AppEvent::Browse(url),
        }
    }
}

impl From<crate::skinstore::Polled> for AppEvent {
    fn from(p: crate::skinstore::Polled) -> AppEvent {
        use crate::skinstore::Polled as P;
        match p {
            P::Toast(s) => AppEvent::Toast(s),
            P::SkinsChanged => AppEvent::SkinsChanged,
            P::OpenUrl(url) => AppEvent::OpenUrl(url),
        }
    }
}

impl From<crate::ir::Polled> for AppEvent {
    fn from(p: crate::ir::Polled) -> AppEvent {
        use crate::ir::Polled as P;
        match p {
            P::Toast(s) => AppEvent::Toast(s),
            P::Changed => AppEvent::IrChanged,
            P::Scores(d) => AppEvent::IrScores(d),
        }
    }
}
