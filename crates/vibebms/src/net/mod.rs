//! Downloads: the hosts songs come from, the tables' links and where a
//! song is placed, the import of files, a song's download and the queue
//! of a table's, the event mirror, and the skin catalog.

pub mod bulk;
pub mod events;
pub mod hosts;
#[cfg(not(target_arch = "wasm32"))]
pub mod import;
pub mod links;
pub mod placement;
pub mod skinstore;
pub mod songfetch;
