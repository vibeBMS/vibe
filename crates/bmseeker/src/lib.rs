//! BMS library tools, as a BMS player needs them and a command-line tool
//! (`bmseeker`) offers them:
//!
//! - [`table`]: difficulty tables (bmstable: 発狂BMS, Satellite, Stella, ...);
//! - [`hosts`]: from the link an author or a table gives to the archive
//!   (Google Drive, Dropbox, MediaFire, MEGA, event venues, ...);
//! - [`unpack`] / [`import`]: archives (.zip, .7z, .rar) into the library;
//! - [`placement`]: which song's folder a chart of its own (差分) belongs in;
//! - [`install`]: a table's song from its links into the library;
//! - [`links`]: links for a table's songs whose own are missing or dead.
//!
//! The parts that touch files take an [`fs::Fs`], so a player can keep its
//! library somewhere other than a disk (the vibeBMS web build keeps it in
//! memory). The network parts are sans-IO steps ([`hosts::Resolver`],
//! [`table::TableLoader`]) with blocking drivers for native builds.

pub mod fs;
pub mod hosts;
// archives from a disk (a web page has none)
#[cfg(not(target_arch = "wasm32"))]
pub mod import;
pub mod install;
pub mod links;
pub mod placement;
pub mod table;
pub mod unpack;

/// A file name from an archive: UTF-8, else Shift_JIS / EUC-KR as BMS packs
/// use.
pub fn decode_name(raw: &[u8]) -> String {
    vibebms_core::encoding::decode_text(raw)
}
