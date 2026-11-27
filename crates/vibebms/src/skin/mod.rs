//! Skins (LR2 `.lr2skin` / CSV, beatoraja JSON / Lua) for the game's
//! screens.
//!
//! A loader ([`lr2`], [`beatoraja`]) reads a skin file into the model
//! ([`Skin`]), whose animations are evaluated against the [`SkinState`] a
//! scene provides ([`Dst::eval`]); [`render`] draws it. This module picks
//! the loader for a file ([`load_file`], [`header`]) and lists the skins
//! in a folder ([`find_skins`]).

pub mod beatoraja;
mod eval;
mod graph;
pub mod guard;
mod images;
mod load;
pub mod lr2;
mod luapat;
mod model;
mod options;
pub mod render;
pub mod script;
mod state;
mod types;

pub use eval::Frame;
pub use graph::*;
pub use images::image_size;
use load::no_panic;
pub use load::Home;
pub(crate) use load::{read_media, read_text};
pub use model::*;
pub use options::*;
pub use state::SkinState;
pub use types::*;

use std::collections::HashMap;
use std::path::PathBuf;

/// Load any supported skin file (LR2 CSV, beatoraja JSON or Lua).
/// `static_option` answers LR2 `#IF`s, `state` what a Lua skin reads
/// while it is built. The file is noted as loading until a frame has been
/// drawn with it ready, or what the load made is let go ([`guard`]).
pub fn load_file(
    path: &std::path::Path,
    skins_root: &std::path::Path,
    user: &UserSettings,
    static_option: &dyn Fn(i32) -> Option<bool>,
    state: Option<&dyn SkinState>,
    image_size: &dyn Fn(&std::path::Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let note = guard::loading(path);
    let mut skin = no_panic(path, || {
        if SkinFormat::of(path) == Some(SkinFormat::Lr2) {
            lr2::load_with(path, skins_root, user, static_option, image_size)
        } else {
            beatoraja::load_with(path, skins_root, user, state, image_size)
        }
    })?;
    skin.note = note.map(std::sync::Arc::new);
    Ok(skin)
}

/// The headers read, by file and its (time, size): `None` for a file
/// that has none (or failed to say).
type Headers = HashMap<(PathBuf, (i64, i64)), Option<(SkinType, String)>>;
static HEADERS: std::sync::Mutex<Option<Headers>> = std::sync::Mutex::new(None);

fn header_read(path: &std::path::Path) -> Option<Option<(SkinType, String)>> {
    let key = (path.to_path_buf(), crate::platform::vfs::stamp(path));
    (HEADERS.lock().ok()?.get_or_insert_default().get(&key)).cloned()
}

/// (type, name) of any supported skin file (cached: reading a Lua skin's
/// header runs it, with the files below `skins_root`).
pub fn header(path: &std::path::Path, skins_root: &std::path::Path) -> Option<(SkinType, String)> {
    if let Some(h) = header_read(path) {
        return h;
    }
    let key = (path.to_path_buf(), crate::platform::vfs::stamp(path));
    let format = SkinFormat::of(path)?;
    let read = || {
        Ok(match format {
            SkinFormat::Lr2 => lr2::header(path),
            SkinFormat::Beatoraja => beatoraja::header(path, skins_root),
            SkinFormat::SoundSet => lr2::lr2ss_header(path).map(|h| (SkinType::SoundSet, h.0)),
        })
    };
    let h = no_panic(path, read).unwrap_or_else(|e| {
        log::warn!("skin: {e}");
        None
    });
    HEADERS
        .lock()
        .ok()?
        .get_or_insert_default()
        .insert(key, h.clone());
    h
}

/// The adjustable offsets of a skin file.
pub fn custom_offsets(path: &std::path::Path, skins_root: &std::path::Path) -> Vec<CustomOffset> {
    let read = || {
        Ok(match SkinFormat::of(path) {
            Some(SkinFormat::Lr2) => lr2::custom_offsets(path),
            Some(SkinFormat::Beatoraja) => beatoraja::custom_offsets(path, skins_root),
            _ => Vec::new(),
        })
    };
    no_panic(path, read).unwrap_or_default()
}

/// The custom options of a skin file, without building the skin.
pub fn custom_options(path: &std::path::Path, skins_root: &std::path::Path) -> Vec<CustomOption> {
    let read = || {
        Ok(match SkinFormat::of(path) {
            Some(SkinFormat::Lr2) => lr2::custom_options(path, skins_root),
            Some(SkinFormat::Beatoraja) => beatoraja::custom_options(path, skins_root),
            _ => Vec::new(),
        })
    };
    no_panic(path, read).unwrap_or_default()
}

/// Play skins below `dir`, the skins folder: LR2 (`.lr2skin`) and
/// beatoraja (`.json`, `.luaskin`).
pub fn find_skins(dir: &std::path::Path) -> Vec<(PathBuf, SkinType, String)> {
    fn walk(d: &std::path::Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 4 {
            return;
        }
        for e in crate::platform::vfs::read_dir(d).unwrap_or_default() {
            let ext = e
                .path
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if e.is_dir {
                walk(&e.path, depth + 1, out);
            } else if matches!(ext.as_str(), "lr2skin" | "json" | "luaskin") {
                out.push(e.path);
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, 0, &mut files);
    files.sort();
    // a Lua skin runs to say what it is: once those of a listing have
    // taken `LIST_TIME`, the ones not read before wait for the next
    let mut spent = std::time::Duration::ZERO;
    let mut found = Vec::new();
    for p in files {
        let lua = p
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("luaskin"));
        let runs = lua && header_read(&p).is_none();
        if runs && spent >= LIST_TIME {
            continue;
        }
        let start = crate::platform::Instant::now();
        let header = header(&p, dir);
        if runs {
            spent += start.elapsed();
        }
        found.extend(header.map(|(t, n)| (p, t, n)));
    }
    found
}

/// Time a listing of the skins gives Lua skins to say what they are (a
/// folder of skins that take long to); the rest come with a later one.
#[cfg(not(test))]
const LIST_TIME: std::time::Duration = std::time::Duration::from_secs(4);
#[cfg(test)]
const LIST_TIME: std::time::Duration = script::HEADER_TIME;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_csv_skin_is_an_lr2_skin_throughout() {
        let dir = std::env::temp_dir().join(format!("vibebms-csv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.csv");
        std::fs::write(
            &p,
            "#INFORMATION,7,CSV skin,me\n#CUSTOMOPTION,Lane,900,Dark,Light\n\
             #CUSTOMOFFSET,Judge,10,1,1,0,0,0,1\n",
        )
        .unwrap();
        assert_eq!(
            header(&p, &dir),
            Some((SkinType::Result, "CSV skin".into()))
        );
        assert_eq!(custom_options(&p, &dir).len(), 1);
        assert_eq!(custom_offsets(&p, &dir).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_options_map_to_op_ids() {
        let dir = std::env::temp_dir().join(format!("vibebms-opt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.lr2skin");
        std::fs::write(
            &p,
            "#INFORMATION,0,S,me\n#CUSTOMOPTION,Lane,900,Dark,Light,Glow\n#CUSTOMOPTION,Judge,910,A,B\n",
        )
        .unwrap();
        let opts = custom_options(&p, &dir);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].ops, vec![900, 901, 902]);
        let mut chosen = std::collections::BTreeMap::new();
        chosen.insert("Lane".to_string(), "Glow".to_string());
        let m = user_options(&opts, Some(&chosen));
        assert_eq!((m[&900], m[&901], m[&902]), (false, false, true));
        // unset options use their default (the first choice for LR2)
        assert_eq!((m[&910], m[&911]), (true, false));
    }

    #[test]
    fn custom_files_are_options() {
        let dir = std::env::temp_dir().join(format!("vibebms-cfile-{}", std::process::id()));
        let skins = dir.join("skins");
        let theme = skins.join("MyTheme");
        std::fs::create_dir_all(theme.join("play/bg")).unwrap();
        for f in ["a.png", "b.png", "c.png"] {
            std::fs::write(theme.join("play/bg").join(f), b"").unwrap();
        }
        // the theme folder renamed: found by the path's folders
        let p = theme.join("play/s.lr2skin");
        std::fs::write(
            &p,
            "#INFORMATION,0,S,me\n#CUSTOMFILE,Back,LR2files\\Theme\\Other\\play\\bg\\*.png,b\n",
        )
        .unwrap();
        let opts = custom_options(&p, &skins);
        // another theme's files are choices; files outside the skins
        // folder are not listed
        for f in [
            "skins/Shared/s.png",
            "skins/Shared/t.png",
            "Private/x.png",
            "Private/y.png",
        ] {
            std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
            std::fs::write(dir.join(f), b"").unwrap();
        }
        std::fs::write(
            &p,
            "#INFORMATION,0,S,me\n#CUSTOMFILE,Shared,LR2files\\Theme\\Other\\play\\..\\..\\Shared\\*.png,s\n\
             #CUSTOMFILE,Up,LR2files\\Theme\\Other\\play\\..\\..\\..\\Private\\*.png,x\n",
        )
        .unwrap();
        let other = custom_options(&p, &skins);
        std::fs::remove_dir_all(&dir).ok();
        let names: Vec<&str> = other.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["Shared"]);
        assert_eq!(other[0].labels, vec!["s", "t"]);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].labels, vec!["a", "b", "c"]);
        assert_eq!(opts[0].default, 1);
        let mut chosen = std::collections::BTreeMap::new();
        chosen.insert("Back".to_string(), "c".to_string());
        assert_eq!(user_files(&opts, Some(&chosen))["Back"], "c");
    }

    /// What reads a skin file may fail on it, but not take the game down.
    #[test]
    fn a_panic_while_a_skin_loads_is_an_error() {
        let dir = std::env::temp_dir().join(format!("vibebms-panic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.lr2skin");
        let csv = "#INFORMATION,0,P,me\n#IMAGE,a.png\n#SRC_IMAGE,0,0,0,0,10,10,1,1,0,0\n\
                   #DST_IMAGE,0,0,0,0,10,10,0,255,255,255,255\n";
        std::fs::write(&p, csv).unwrap();
        let user = UserSettings::default();
        let load = |size: &dyn Fn(&std::path::Path) -> Option<(u32, u32)>| {
            load_file(&p, &dir, &user, &|_| None, None, size).map(|s| s.objects.len())
        };
        assert_eq!(load(&|_| Some((16, 16))), Ok(1));
        let e = load(&|_| panic!("no such image reader")).unwrap_err();
        assert!(e.contains("failed (no such image reader)"), "{e}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
