//! Lua 5.4 for the web build (wasm32-unknown-unknown), where mlua cannot
//! build it: `build.rs` compiles Lua's C sources with clang against a small
//! C library of its own (`shim/`), and mlua (feature "external") links to
//! them. This crate holds the part of that C library written in Rust:
//! allocation, math, `strtod`, float formatting, the clock, the time zone
//! and the console.
//!
//! On other targets it is empty.

#[cfg(any(test, all(target_arch = "wasm32", target_os = "unknown")))]
mod cfmt;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod imp;

/// Makes the linker keep this crate (and with it Lua) when nothing else in
/// it is named.
pub fn linked() {}
