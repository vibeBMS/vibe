//! Compile Lua 5.4 and its C library (`shim/`) for wasm32-unknown-unknown
//! with clang. Lua's sources are lua-src's, the same files mlua compiles
//! on the other targets (feature "vendored"); lua-src is a build
//! dependency here so that cargo fetches it for a web build too. Other
//! targets build nothing here.
//!
//! Lua raises errors with setjmp / longjmp; clang lowers them to Wasm
//! exception handling (`-mllvm -wasm-enable-sjlj`), whose runtime is in
//! `shim/libc.c`.

use std::path::{Path, PathBuf};

/// SHA-256 of the Lua 5.4 sources compiled ([`hash`]): those of lua-src
/// 551.1.1. Cargo checks the crate it fetches against Cargo.lock, but the
/// sources here are found by name among those unpacked (CARGO_HOME), so
/// they are checked here. When lua-src is updated (Cargo.toml and
/// Cargo.lock; Dependabot does it on its own) the web build stops until
/// this is set again: see what changed in Lua's sources, then set it to
/// the hash the build names.
const LUA_SHA256: &str = "60f5cf723a4418912c16fb560597fd541bfd68513964774704b666cf667ce976";

/// The version of lua-src Cargo.lock names (one: the web's Lua is the
/// other targets').
fn locked_version(here: &Path) -> String {
    let lock = here
        .ancestors()
        .map(|d| d.join("Cargo.lock"))
        .find(|p| p.is_file())
        .expect("no Cargo.lock above the crate");
    let text = std::fs::read_to_string(&lock).unwrap();
    let mut versions: Vec<String> = text
        .split("[[package]]")
        .filter_map(|p| {
            let field = |k: &str| {
                p.lines()
                    .find_map(|l| l.strip_prefix(k))
                    .map(|v| v.trim().trim_matches('"').to_string())
            };
            (field("name = ").as_deref() == Some("lua-src"))
                .then(|| field("version = "))
                .flatten()
        })
        .collect();
    match versions.len() {
        0 => panic!("Cargo.lock has no lua-src"),
        1 => versions.remove(0),
        _ => panic!(
            "Cargo.lock has lua-src {}: the web build compiles one Lua, the one the other \
             targets compile; make every crate take one version",
            versions.join(" and ")
        ),
    }
}

/// The files of Lua's sources in `dir` that are compiled or included
/// (`.c`, `.h`), in name order.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "c" || e == "h"))
        .collect();
    files.sort();
    files
}

/// SHA-256 of `files` in their order: of each, its name, a zero byte, its
/// length (8 bytes, little-endian) and its bytes.
fn hash(files: &[PathBuf]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for f in files {
        let bytes = std::fs::read(f).unwrap();
        h.update(f.file_name().unwrap().as_encoded_bytes());
        h.update([0]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Lua 5.4's sources in lua-src as cargo unpacked it (the version
/// Cargo.lock names, in the registry's sources: CARGO_HOME) and its files
/// compiled or included: those whose hash is [`LUA_SHA256`]. lua-src does
/// not tell where its files are, and a build dependency is always fetched
/// and unpacked there.
fn lua_sources(here: &Path) -> (PathBuf, Vec<PathBuf>) {
    let version = locked_version(here);
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cargo")))
        .expect("neither CARGO_HOME nor HOME is set");
    let mut found: Vec<PathBuf> = std::fs::read_dir(home.join("registry/src"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join(format!("lua-src-{version}")))
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|d| {
            d.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("lua-5.4."))
                && d.join("lua.h").is_file()
        })
        .collect();
    found.sort();
    let mut seen = Vec::new();
    for dir in found {
        let files = sources(&dir);
        let got = hash(&files);
        if got == LUA_SHA256 {
            return (dir, files);
        }
        seen.push(format!("{} has {got}", dir.display()));
    }
    if seen.is_empty() {
        panic!(
            "lua-src {version}'s Lua 5.4 sources are not in {}/registry/src \
             (lua-src is a build dependency: cargo unpacks it there)",
            home.display()
        );
    }
    panic!(
        "lua-src {version}'s Lua 5.4 sources are not the ones LUA_SHA256 in \
         crates/vibebms-lua-web/build.rs names ({LUA_SHA256}): {}. If lua-src was \
         updated, see what changed in its Lua sources and set LUA_SHA256 to the hash \
         of the ones to compile",
        seen.join("; ")
    );
}

fn main() {
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if arch != "wasm32" || os != "unknown" {
        return;
    }
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let base = || {
        let mut b = cc::Build::new();
        b.warnings(false)
            .flag("-nostdlibinc")
            .include(here.join("shim/include"))
            .flag("-mexception-handling")
            .flag("-mllvm")
            .flag("-wasm-enable-eh")
            .flag("-mllvm")
            .flag("-wasm-enable-sjlj");
        b
    };
    // the Wasm exception handling every browser runs (clang 19 and later
    // know the option; before, it was the only kind)
    let probe = out.join("legacy_eh_probe.c");
    std::fs::write(&probe, "int vblua_probe;\n").unwrap();
    let legacy = base()
        .flag("-mllvm")
        .flag("-wasm-use-legacy-eh=true")
        .file(&probe)
        .cargo_metadata(false)
        .cargo_warnings(false)
        .try_compile("vblua_probe")
        .is_ok();

    let mut b = base();
    if legacy {
        b.flag("-mllvm").flag("-wasm-use-legacy-eh=true");
    }
    // as lua-src builds Lua 5.4 for the other targets (its sources, not
    // its build: that one is for WASI)
    let _ = lua_src::Lua54;
    let (lua, files) = lua_sources(here);
    println!("cargo:rerun-if-changed={}", lua.display());
    b.define("LUA_COMPAT_5_3", None);
    b.include(&lua);
    let compiled = files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "c"));
    b.files(compiled).file(here.join("shim/libc.c"));
    if let Err(e) = b.try_compile("vibebms_lua") {
        panic!(
            "compiling Lua for the web needs clang with the WebAssembly target \
             (and llvm-ar); set CC_wasm32_unknown_unknown / AR_wasm32_unknown_unknown \
             if they are not found as clang / llvm-ar: {e}"
        );
    }
}
