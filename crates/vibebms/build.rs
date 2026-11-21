use std::env::var;
use std::path::Path;

#[path = "src/update/key.rs"]
mod key;

fn main() {
    // `include_dir!` (platform/vfs.rs) embeds assets/builtin, but Cargo does
    // not know about those files; rebuild when any of them changes.
    println!("cargo:rerun-if-changed=assets/builtin");

    // the key the self-update's list must be signed by (update::KEY), when
    // the repository has one (docs/BUILDING.md, "Self-update"). The folder
    // is watched, not the file: Cargo runs a build script on every build
    // while a file it watches does not exist.
    println!("cargo:rerun-if-changed=../../tools");
    let file = Path::new(&var("CARGO_MANIFEST_DIR").unwrap()).join("../../tools/update_key.pub");
    let key = match std::fs::read_to_string(&file) {
        Ok(text) => match key::parse(&text) {
            Ok(k) => format!("Some({k:?})"),
            Err(e) => panic!("{}: {e}", file.display()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "None".into(),
        Err(e) => panic!("{}: {e}", file.display()),
    };
    std::fs::write(
        Path::new(&var("OUT_DIR").unwrap()).join("update_key.rs"),
        key,
    )
    .unwrap();

    // Platform groups, named once (`#[cfg(desktop)]` instead of the
    // `any(...)` lists). Cargo.toml's target tables cannot use them and keep
    // their own lists.
    let os = var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let desktop = var("CARGO_CFG_WINDOWS").is_ok() || os == "macos" || os == "linux";
    let mobile = os == "android" || os == "ios";
    let web = var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32");
    let browser = var("CARGO_FEATURE_BROWSER").is_ok();
    let groups = [
        // the executables that update themselves (`update`)
        ("desktop", desktop),
        // phones and tablets
        ("mobile", mobile),
        // smaller memory and CPU budgets (the web, phones)
        ("small_device", web || mobile),
        // songs are downloaded in an in-app web view (`browser`)
        (
            "in_app_browser",
            os == "android" || (browser && (desktop || os == "ios")),
        ),
        // game controllers through gilrs (`input::gamepad`; the web through
        // the browser's Gamepad API)
        ("gamepads", !mobile),
    ];
    for (name, on) in groups {
        println!("cargo::rustc-check-cfg=cfg({name})");
        if on {
            println!("cargo::rustc-cfg={name}");
        }
    }
}
