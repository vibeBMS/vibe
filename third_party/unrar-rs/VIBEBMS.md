# unrar-rs 0.16.1, patched

A copy of [unrar-rs](https://crates.io/crates/unrar-rs) 0.16.1
(<https://github.com/scryer-media/rarpar>; license: `LICENSE`, Apache-2.0
with the UnRAR restriction) used through `[patch.crates-io]`, so the web
build can unpack .rar archives.

The one change: `src/rar4/mod.rs` `local_datetime_to_system_time` calls
`libc::mktime`, which wasm32-unknown-unknown lacks; there it computes the
time as UTC instead. Example, bench and test targets are left out of
`Cargo.toml` (their files are not copied).

Drop this copy for the crates.io release once one builds for wasm32.
