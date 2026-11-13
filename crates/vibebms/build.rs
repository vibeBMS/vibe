// `include_dir!` (platform/vfs.rs) embeds assets/builtin, but Cargo does not
// know about those files; rebuild when any of them changes.
fn main() {
    println!("cargo:rerun-if-changed=assets/builtin");
}
