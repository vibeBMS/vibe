//! What the background work reports, for the shared state to act on.

/// What a background subsystem's `poll` reports, for the app to act on.
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
