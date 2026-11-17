//! Internet ranking: what the app keeps and sends for the IR servers
//! (`vibebms-ir` has the protocols).

pub mod build_id;
pub mod store;

use crate::paths::Paths;
use vibebms_core::record::PlayRecord;

/// The IR's state: the history of plays.
pub struct IrService {
    history: store::History,
}

impl IrService {
    /// The service over the files in `paths` (`now_ms`: Unix ms).
    pub fn new(paths: &Paths, now_ms: i64) -> IrService {
        let mut history = store::History::open(paths.history.clone(), now_ms);
        history.compact(now_ms);
        IrService { history }
    }

    /// A play has ended: it gets its play id and goes into the history
    /// (a play of the player's own: not autoplay, not a replay watched).
    pub fn record(&mut self, rec: &mut PlayRecord) {
        if rec.flags.autoplay || rec.flags.replay {
            return;
        }
        if let Err(e) = self.history.add(rec) {
            log::warn!("play history: {e}");
        }
    }
}
