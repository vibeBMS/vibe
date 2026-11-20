//! Putting an update in place (the desktop executables).

use super::{own_app, Offer};
use bmseeker::progress::Progress;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Download `o` next to `exe` (next to the macOS app it runs in), check it
/// and put it in the running executable's (the app's) place.
pub fn install(o: &Offer, exe: &Path, progress: &Arc<Progress>) -> Result<(), String> {
    let app = own_app(exe);
    let dir = app
        .unwrap_or(exe)
        .parent()
        .ok_or("the executable has no folder")?;
    // what an update that was cut short left
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with(".vibebms-update-") && n.ends_with(".tmp") {
            let p = e.path();
            let _ = fs::remove_file(&p).or_else(|_| fs::remove_dir_all(&p));
        }
    }
    let tmp = temp(dir, "");
    let r = download(&o.url, &tmp, o.size, &o.sha256, progress).and_then(|()| {
        match app {
            Some(app) => replace_app(&tmp, app, exe)?,
            None => replace_running(&tmp)?,
        }
        log::info!("updated {} to {}", app.unwrap_or(exe).display(), o.version);
        Ok(())
    });
    let _ = fs::remove_file(&tmp);
    r
}

/// A name for this update's own temporary file or folder in `dir` (what an
/// update cut short leaves is deleted by the next).
fn temp(dir: &Path, what: &str) -> PathBuf {
    dir.join(format!(".vibebms-update-{}{what}.tmp", std::process::id()))
}

/// Put the app in `zip` (`tools/package_macos.sh`) in the place of `app`,
/// whose executable `exe` is running, and sign it ad hoc again.
fn replace_app(zip: &Path, app: &Path, exe: &Path) -> Result<(), String> {
    let dir = app.parent().ok_or("the app has no folder")?;
    let unpacked = temp(dir, "-new");
    let _ = fs::remove_dir_all(&unpacked);
    let r = (|| {
        // ditto keeps the app's modes and signature as they are
        run(std::process::Command::new("ditto")
            .args(["-x", "-k"])
            .arg(zip)
            .arg(&unpacked))
        .map_err(|e| format!("cannot unpack the update ({e})"))?;
        let new = fs::read_dir(&unpacked)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or("the update has no app in it")?;
        let name = exe.file_name().ok_or("the executable has no name")?;
        let new_exe = new.join("Contents/MacOS").join(name);
        if !new_exe.is_file() {
            return Err(format!("the update has no {}", new_exe.display()));
        }
        swap_app(&new, app)?;
        sign(app);
        Ok(())
    })();
    let _ = fs::remove_dir_all(&unpacked);
    r
}

/// `new` in the place of `app`: the old app is moved aside first, put back
/// if `new` cannot take its place, deleted once it has (the running
/// executable keeps running from the deleted file).
fn swap_app(new: &Path, app: &Path) -> Result<(), String> {
    let old = temp(app.parent().ok_or("the app has no folder")?, "-old");
    let _ = fs::remove_dir_all(&old);
    fs::rename(app, &old).map_err(|e| format!("cannot move vibeBMS aside ({e})"))?;
    if let Err(e) = fs::rename(new, app) {
        let _ = fs::rename(&old, app);
        return Err(format!("cannot put the update in place ({e})"));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// Sign `app` ad hoc again (the signature covers the whole bundle), when
/// `codesign` is there; the app starts without it too.
fn sign(app: &Path) {
    let r = run(std::process::Command::new("codesign")
        .args(["--force", "--deep", "-s", "-"])
        .arg(app));
    match r {
        Ok(()) => log::info!("signed {} ad hoc", app.display()),
        Err(e) => log::warn!("codesign {}: {e}", app.display()),
    }
}

/// Run `cmd`; its error output when it fails.
fn run(cmd: &mut std::process::Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Download `url` into `to`; it must be `size` bytes whose
/// SHA-256 is `sha256`. However long it takes: it ends when it is larger
/// than `size` or no data comes for a while (`platform::stream`).
fn download(
    url: &str,
    to: &Path,
    size: u64,
    sha256: &str,
    progress: &Arc<Progress>,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::sync::Mutex;
    struct St {
        file: std::fs::File,
        hash: Sha256,
        n: u64,
    }
    let file = std::fs::File::create(to).map_err(|e| format!("cannot write the update: {e}"))?;
    let st = Arc::new(Mutex::new(St {
        file,
        hash: Sha256::new(),
        n: 0,
    }));
    progress.start_file(Some(size));
    let (sink, progress) = (st.clone(), progress.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    crate::platform::stream(
        url,
        size,
        None,
        move |c| {
            let mut g = sink.lock().unwrap_or_else(|e| e.into_inner());
            g.n += c.len() as u64;
            g.hash.update(c);
            g.file
                .write_all(c)
                .map_err(|e| format!("cannot write the update ({e})"))?;
            progress.add(c.len() as u64);
            Ok(())
        },
        move |r, _| {
            let _ = tx.send(r);
        },
    );
    let r = rx
        .recv()
        .map_err(|_| "the download ended early".to_string())?;
    let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
    r.map_err(|e| format!("download failed ({e})"))?;
    if g.n != size {
        return Err(format!(
            "the download is {} bytes, not {size}: try again later",
            g.n
        ));
    }
    let got = vibebms_core::hex(&std::mem::take(&mut g.hash).finalize());
    if !got.eq_ignore_ascii_case(sha256) {
        return Err("the download is damaged (its checksum is wrong): try again later".into());
    }
    g.file
        .sync_all()
        .map_err(|e| format!("cannot write the update ({e})"))?;
    Ok(())
}

/// Put the executable `new` in the running executable's place (on Unix
/// an atomic rename that keeps its permissions; on Windows the running one
/// is renamed away first).
fn replace_running(new: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(unix)]
    let exe = super::exe_path();
    self_replace::self_replace(new).map_err(|e| format!("cannot replace vibeBMS ({e})"))?;
    // it must be there and start (macOS sets no quarantine attribute on a
    // file the app wrote itself, but it does need the executable bit)
    #[cfg(unix)]
    if let Some(exe) = exe {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(&exe).map_err(|e| format!("the update is not in place ({e})"))?;
        if m.permissions().mode() & 0o111 == 0 {
            let mode = m.permissions().mode() | 0o755;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(mode))
                .map_err(|e| format!("the update cannot be made executable ({e})"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-shot HTTP server answering `body` (with `len` as its
    /// Content-Length).
    fn serve(body: Vec<u8>, status: &str) -> String {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let status = status.to_string();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        });
        format!("http://{addr}/vibebms")
    }

    #[test]
    fn downloads_are_checked() {
        let dir = std::env::temp_dir().join(format!("vibebms-update-d-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let to = dir.join("new");
        // sha256("test")
        let sha = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        let p = Progress::new();
        download(&serve(b"test".to_vec(), "200 OK"), &to, 4, sha, &p).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"test");
        assert_eq!(p.bytes(), (4, Some(4)));
        // damaged, cut short, too long, refused
        let e = download(&serve(b"tesT".to_vec(), "200 OK"), &to, 4, sha, &p).unwrap_err();
        assert!(e.contains("checksum"), "{e}");
        let e = download(&serve(b"tes".to_vec(), "200 OK"), &to, 4, sha, &p).unwrap_err();
        assert!(e.contains("not 4"), "{e}");
        let e = download(&serve(b"tests".to_vec(), "200 OK"), &to, 4, sha, &p).unwrap_err();
        assert!(e.contains("larger"), "{e}");
        let e = download(&serve(Vec::new(), "404 Not Found"), &to, 4, sha, &p).unwrap_err();
        assert!(e.contains("404"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apps_are_swapped() {
        let dir = std::env::temp_dir().join(format!("vibebms-update-app-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (app, new) = (dir.join("vibeBMS.app"), dir.join("unpacked/vibeBMS.app"));
        for (a, v) in [(&app, "old"), (&new, "new")] {
            fs::create_dir_all(a.join("Contents/MacOS")).unwrap();
            fs::write(a.join("Contents/MacOS/vibebms"), v).unwrap();
        }
        swap_app(&new, &app).unwrap();
        assert_eq!(
            fs::read(app.join("Contents/MacOS/vibebms")).unwrap(),
            b"new"
        );
        assert!(!new.exists());
        // nothing is left beside it
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        // a new app that is not there: the old one stays
        let e = swap_app(&dir.join("missing.app"), &app).unwrap_err();
        assert!(e.contains("in place"), "{e}");
        assert_eq!(
            fs::read(app.join("Contents/MacOS/vibebms")).unwrap(),
            b"new"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Set by [`replacing_a_running_executable`] for the copy it starts.
    const CHILD_ENV: &str = "VIBEBMS_UPDATE_TEST_NEW";

    /// The copy's side: replace itself with the file the variable names.
    #[test]
    fn replace_child() {
        if let Some(new) = std::env::var_os(CHILD_ENV) {
            replace_running(Path::new(&new)).unwrap();
        }
    }

    /// A copy of this test executable, started, replaces itself with
    /// another file through [`replace_running`].
    #[test]
    fn replacing_a_running_executable() {
        let me = std::env::current_exe().unwrap();
        // next to it (the same file system: a hard link, not a copy)
        let dir = me
            .parent()
            .unwrap()
            .join(format!("update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) { "dummy.exe" } else { "dummy" };
        let dummy = dir.join(name);
        if std::fs::hard_link(&me, &dummy).is_err() {
            std::fs::copy(&me, &dummy).unwrap();
        }
        // the new "executable": a script on Unix (it can be run after)
        let new = dir.join("new.tmp");
        std::fs::write(&new, "#!/bin/sh\necho replaced\n").unwrap();
        let out = std::process::Command::new(&dummy)
            .args([
                "--exact",
                "update::desktop::tests::replace_child",
                "--test-threads=1",
            ])
            .env(CHILD_ENV, &new)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert_eq!(
            std::fs::read(&dummy).unwrap(),
            b"#!/bin/sh\necho replaced\n"
        );
        // the test executable itself is untouched
        assert!(std::fs::metadata(&me).unwrap().len() > 1000);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dummy).unwrap().permissions().mode();
            assert_ne!(mode & 0o111, 0, "{mode:o}");
            let run = std::process::Command::new(&dummy).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&run.stdout), "replaced\n");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
