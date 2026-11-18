//! The system's file manager and default applications (desktop): a chart's
//! folder with its file selected, a text file in its viewer.

use std::path::Path;

/// The file manager can be opened here (Windows, macOS, Linux and the BSDs).
pub const CAN_OPEN_FILES: bool = cfg!(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
));

/// `path` (a file) shown in the system's file manager: its folder opened with
/// it selected (Windows' Explorer, macOS's Finder; on Linux a file manager
/// that answers the freedesktop `FileManager1` call: Nautilus, Dolphin, Nemo,
/// Caja, Thunar...; else the folder opened with `xdg-open`). False where
/// there is none. The command runs off the main thread.
pub fn reveal(path: &Path) -> bool {
    if !CAN_OPEN_FILES {
        return false;
    }
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        if let Err(e) = reveal_now(&path) {
            log::warn!("show {}: {e}", path.display());
        }
    });
    true
}

/// `path` opened in the application the system has for it (a `.txt` file in
/// its text viewer). False where there is none.
pub fn open_file(path: &Path) -> bool {
    if !CAN_OPEN_FILES {
        return false;
    }
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        if let Err(e) = open_now(&path) {
            log::warn!("open {}: {e}", path.display());
        }
    });
    true
}

#[cfg(target_os = "windows")]
fn reveal_now(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    // explorer parses its own command line: `/select,"<path>"` as one argument
    let mut arg = std::ffi::OsString::from("/select,\"");
    arg.push(path.as_os_str());
    arg.push("\"");
    std::process::Command::new("explorer")
        .raw_arg(arg)
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn reveal_now(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()?
        .wait()?;
    Ok(())
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn reveal_now(path: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let uri = file_uri(path);
    let shown = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--reply-timeout=3000",
            "--dest=org.freedesktop.FileManager1",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{uri}"))
        .arg("string:")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !shown {
        // the folder, at least
        let dir = path.parent().unwrap_or(path);
        Command::new("xdg-open").arg(dir).spawn()?.wait()?;
    }
    Ok(())
}

#[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
fn reveal_now(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_now(path: &Path) -> std::io::Result<()> {
    // explorer opens a file in its default application
    std::process::Command::new("explorer").arg(path).spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_now(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()?
        .wait()?;
    Ok(())
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn open_now(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()?
        .wait()?;
    Ok(())
}

#[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
fn open_now(_: &Path) -> std::io::Result<()> {
    Ok(())
}

/// A `file://` URI of an absolute path, its bytes outside the unreserved set
/// percent-encoded (as the `FileManager1` call takes them).
#[cfg_attr(
    any(
        target_os = "windows",
        target_os = "macos",
        target_arch = "wasm32",
        target_os = "android",
        target_os = "ios"
    ),
    allow(dead_code)
)]
pub(crate) fn file_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::from("file://");
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_are_percent_encoded() {
        assert_eq!(
            file_uri(Path::new("/bms/a b/曲#1.bms")),
            "file:///bms/a%20b/%E6%9B%B2%231.bms"
        );
    }
}
