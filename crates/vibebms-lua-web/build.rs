//! Compile Lua 5.4 (`lua/`) and its C library (`shim/`) for
//! wasm32-unknown-unknown with clang. Other targets build nothing: there
//! mlua compiles Lua itself (feature "vendored").
//!
//! Lua raises errors with setjmp / longjmp; clang lowers them to Wasm
//! exception handling (`-mllvm -wasm-enable-sjlj`), whose runtime is in
//! `shim/libc.c`.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=lua");
    println!("cargo:rerun-if-changed=shim");
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
    // as lua-src builds Lua 5.4 for the other targets
    b.define("LUA_COMPAT_5_3", None);
    b.include(here.join("lua"));
    let mut files: Vec<_> = std::fs::read_dir(here.join("lua"))
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
