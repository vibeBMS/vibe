use std::env::var;

fn main() {
    // `include_dir!` (platform/vfs.rs) embeds assets/builtin, but Cargo does
    // not know about those files; rebuild when any of them changes.
    println!("cargo:rerun-if-changed=assets/builtin");

    // Platform groups, named once (`#[cfg(desktop)]` instead of the
    // `any(...)` lists). Cargo.toml's target tables cannot use them and keep
    // their own lists.
    let os = var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let desktop = var("CARGO_CFG_WINDOWS").is_ok() || os == "macos" || os == "linux";
    let wasm = var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
    let browser = var("CARGO_FEATURE_BROWSER").is_ok();
    let groups = [
        // the executables that update themselves (`update`)
        ("desktop", desktop),
        // phones and tablets
        ("mobile", os == "android" || os == "ios"),
        // songs are downloaded in an in-app web view (`browser`)
        (
            "in_app_browser",
            os == "android" || (browser && (desktop || os == "ios")),
        ),
        // game controllers through gilrs (`input::gamepad`)
        ("gamepads", !wasm && os != "android" && os != "ios"),
    ];
    for (name, on) in groups {
        println!("cargo::rustc-check-cfg=cfg({name})");
        if on {
            println!("cargo::rustc-cfg={name}");
        }
    }
}
