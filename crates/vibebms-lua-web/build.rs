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

/// Lua 5.4's sources in lua-src as cargo unpacked it: the version
/// Cargo.lock names, in the registry's sources (CARGO_HOME). lua-src does
/// not tell where its files are, and a build dependency is always fetched
/// and unpacked there.
fn lua_sources(here: &Path) -> PathBuf {
    let lock = here
        .ancestors()
        .map(|d| d.join("Cargo.lock"))
        .find(|p| p.is_file())
        .expect("no Cargo.lock above the crate");
    let text = std::fs::read_to_string(&lock).unwrap();
    let version = text
        .split("[[package]]")
        .find_map(|p| {
            let field = |k: &str| {
                p.lines()
                    .find_map(|l| l.strip_prefix(k))
                    .map(|v| v.trim().trim_matches('"').to_string())
            };
            (field("name = ").as_deref() == Some("lua-src"))
                .then(|| field("version = "))
                .flatten()
        })
        .expect("Cargo.lock has no lua-src");
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cargo")))
        .expect("neither CARGO_HOME nor HOME is set");
    let mut registries: Vec<_> = std::fs::read_dir(home.join("registry/src"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join(format!("lua-src-{version}")))
        .filter(|d| d.is_dir())
        .collect();
    registries.sort();
    for crate_dir in registries {
        let mut lua: Vec<_> = std::fs::read_dir(&crate_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|d| {
                d.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("lua-5.4."))
                    && d.join("lua.h").is_file()
            })
            .collect();
        if lua.len() == 1 {
            return lua.remove(0);
        }
    }
    panic!(
        "lua-src {version}'s Lua 5.4 sources are not in {}/registry/src \
         (lua-src is a build dependency: cargo unpacks it there)",
        home.display()
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
    let lua = lua_sources(here);
    println!("cargo:rerun-if-changed={}", lua.display());
    b.define("LUA_COMPAT_5_3", None);
    b.include(&lua);
    let mut files: Vec<_> = std::fs::read_dir(&lua)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "c"))
        .collect();
    files.sort();
    b.files(files).file(here.join("shim/libc.c"));
    if let Err(e) = b.try_compile("vibebms_lua") {
        panic!(
            "compiling Lua for the web needs clang with the WebAssembly target \
             (and llvm-ar); set CC_wasm32_unknown_unknown / AR_wasm32_unknown_unknown \
             if they are not found as clang / llvm-ar: {e}"
        );
    }
}
