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

impl From<crate::net::events::Polled> for AppEvent {
    fn from(p: crate::net::events::Polled) -> AppEvent {
        use crate::net::events::Polled as P;
        match p {
            P::Toast(s) => AppEvent::Toast(s),
            P::Rescan => AppEvent::Rescan,
            P::Browse(url) => AppEvent::Browse(url),
            P::LibraryChanged => AppEvent::LibraryChanged,
        }
    }
}

impl From<crate::net::songfetch::Polled> for AppEvent {
    fn from(p: crate::net::songfetch::Polled) -> AppEvent {
        use crate::net::songfetch::Polled as P;
        match p {
            P::Toast(s) => AppEvent::Toast(s),
            P::Rescan => AppEvent::Rescan,
            P::Browse(url) => AppEvent::Browse(url),
        }
    }
}

impl From<crate::net::skinstore::Polled> for AppEvent {
    fn from(p: crate::net::skinstore::Polled) -> AppEvent {
        use crate::net::skinstore::Polled as P;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir;
    use crate::net::{events, skinstore, songfetch};

    /// Each service's report is the event `Ctx` acts on for it, with what
    /// it carries.
    #[test]
    fn each_report_is_its_event() {
        let scores = || {
            Box::new(crate::ir::download::Download {
                title: "me".into(),
                ..Default::default()
            })
        };
        let got = vec![
            AppEvent::from(events::Polled::Toast("a".into())),
            events::Polled::Rescan.into(),
            events::Polled::Browse("https://a".into()).into(),
            events::Polled::LibraryChanged.into(),
            songfetch::Polled::Toast("s".into()).into(),
            songfetch::Polled::Rescan.into(),
            songfetch::Polled::Browse("https://s".into()).into(),
            skinstore::Polled::Toast("k".into()).into(),
            skinstore::Polled::SkinsChanged.into(),
            skinstore::Polled::OpenUrl("https://k".into()).into(),
            ir::Polled::Toast("i".into()).into(),
            ir::Polled::Changed.into(),
            ir::Polled::Scores(scores()).into(),
        ];
        let want = vec![
            AppEvent::Toast("a".into()),
            AppEvent::Rescan,
            AppEvent::Browse("https://a".into()),
            AppEvent::LibraryChanged,
            AppEvent::Toast("s".into()),
            AppEvent::Rescan,
            AppEvent::Browse("https://s".into()),
            AppEvent::Toast("k".into()),
            AppEvent::SkinsChanged,
            AppEvent::OpenUrl("https://k".into()),
            AppEvent::Toast("i".into()),
            AppEvent::IrChanged,
            AppEvent::IrScores(scores()),
        ];
        assert_eq!(got, want);
    }
}
