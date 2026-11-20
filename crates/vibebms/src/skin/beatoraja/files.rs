//! The files a skin names: `filepath` choices, wildcards, letter case,
//! and BMFont fonts.

use super::*;

/// Read a BMFont text font (`.fnt`): the font and its page image files
/// (glyph image indices are page ids; a page outside the skin's theme is
/// an empty path).
pub(super) fn load_bmfont(path: &Path, home: &Home) -> Option<(BitmapFont, Vec<PathBuf>)> {
    let bytes = vfs::read(path).ok()?;
    let text = vibebms_core::encoding::decode_text(&bytes);
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut font = BitmapFont::default();
    let mut pages: Vec<PathBuf> = Vec::new();
    let (mut size, mut line_h) = (0.0f32, 0.0f32);
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}');
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        // key=value pairs; values may be quoted
        let mut kv: HashMap<&str, String> = HashMap::new();
        let mut rest = rest.trim();
        while let Some((k, v)) = rest.split_once('=') {
            let (val, after) = match v.strip_prefix('"') {
                Some(q) => q.split_once('"').unwrap_or((q, "")),
                None => v.split_once(' ').unwrap_or((v, "")),
            };
            kv.insert(k.trim(), val.to_string());
            rest = after.trim();
        }
        let n = |k: &str| kv.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        match tag {
            "info" => size = n("size").abs(),
            "common" => line_h = n("lineHeight"),
            "page" => {
                let id = n("id") as usize;
                if id < 64 {
                    if pages.len() <= id {
                        pages.resize(id + 1, PathBuf::new());
                    }
                    let file = kv.get("file").map_or("", String::as_str);
                    pages[id] = (home.file_from(dir, file))
                        .map(|p| vfs::resolve_case(&p))
                        .unwrap_or_default();
                }
            }
            "char" => {
                let Some(c) = char::from_u32(n("id") as u32) else {
                    continue;
                };
                let page = n("page") as usize;
                if page < pages.len() {
                    font.glyphs
                        .insert(c, (page, [n("x"), n("y"), n("width"), n("height")]));
                    font.metrics
                        .insert(c, [n("xoffset"), n("yoffset"), n("xadvance")]);
                }
            }
            _ => {}
        }
    }
    font.size = if line_h > 0.0 { line_h } else { size };
    // beatoraja falls back to the line height when there is no size
    font.em = if size > 0.0 { size } else { font.size };
    (!font.glyphs.is_empty() && font.size > 0.0).then_some((font, pages))
}

/// A `filepath` customisation: name, path (with a `*`), default file.
#[derive(Clone)]
pub(super) struct FilePath {
    pub name: String,
    pub path: String,
    def: String,
}

pub(super) fn filepaths(sk: &Value) -> Vec<FilePath> {
    arr(sk.get("filepath"))
        .iter()
        .map(|f| FilePath {
            name: s(f, "name").to_string(),
            path: s(f, "path").replace('\\', "/"),
            def: s(f, "def").to_string(),
        })
        .filter(|f| f.path.contains('*'))
        .collect()
}

/// The files a `filepath` offers (names, sorted): `*|n|` marks one of
/// several choices over the same folder.
pub(super) fn filepath_files(home: &Home, path: &str) -> Vec<String> {
    let pattern = match (path.find('|'), path.rfind('|')) {
        (Some(a), Some(b)) => format!("{}{}", &path[..a], &path[b + 1..]),
        _ => path.to_string(),
    };
    let Some(pattern) = home.file(&pattern) else {
        return Vec::new();
    };
    super::lr2::glob_paths(&vfs::resolve_case(&pattern))
        .iter()
        .filter(|p| !p.to_string_lossy().contains('*'))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect()
}

/// The file chosen for a `filepath`: the user's, else the default (a name
/// with or without its extension), else the first.
pub(super) fn filepath_chosen(
    home: &Home,
    f: &FilePath,
    files: &HashMap<String, String>,
) -> Option<String> {
    let all = filepath_files(home, &f.path);
    let stem = |n: &str| {
        Path::new(n)
            .file_stem()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let want = files.get(&f.name).unwrap_or(&f.def).to_lowercase();
    all.iter()
        .find(|n| n.to_lowercase() == want)
        .or_else(|| all.iter().find(|n| !want.is_empty() && stem(n) == want))
        .or(all.first())
        .cloned()
}

/// The file a skin names: a `filepath` whose path starts it puts the
/// chosen file in place of its `*`; another `*` picks one of the matching
/// files at random; other paths are matched to the files' letter case.
/// `None` for a path out of the skin's theme: no folder is listed for it
/// and no file looked for.
///
/// Written from a behaviour specification, not from another program's
/// code.
pub(super) fn skin_path(
    home: &Home,
    fps: &[FilePath],
    files: &HashMap<String, String>,
    path: &str,
) -> Option<PathBuf> {
    let path = path.replace('\\', "/");
    let on_disk = |name: &str| home.file(name).map(|p| vfs::resolve_case(&p));
    for f in fps {
        let Some(rest) = path.strip_prefix(f.path.as_str()) else {
            continue;
        };
        // no file to choose: try the next entry
        let Some(chosen) = filepath_chosen(home, f, files) else {
            continue;
        };
        let before_star = &f.path[..f.path.find('*').unwrap_or(f.path.len())];
        let folder = &before_star[..before_star.rfind('/').map_or(0, |i| i + 1)];
        return on_disk(&format!("{folder}{chosen}{rest}"));
    }
    let Some(star) = path.rfind('*') else {
        return on_disk(&path);
    };
    let after = &path[star + 1..];
    let suffix = match (after.find('|'), after.rfind('|')) {
        (Some(a), Some(b)) => format!("{}{}", &after[..a], &after[b + 1..]),
        _ => after.to_string(),
    }
    .to_lowercase();
    let folder = on_disk(path[..star].rfind('/').map_or("", |i| &path[..i]))?;
    let mut found: Vec<PathBuf> = vfs::read_dir(&folder)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.path)
        .filter(|p| p.to_string_lossy().to_lowercase().ends_with(&suffix))
        .collect();
    if found.is_empty() {
        return home.file(&path);
    }
    found.sort();
    let nanos = crate::platform::SystemTime::now()
        .duration_since(crate::platform::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos()) as usize;
    Some(found.swap_remove(nanos % found.len()))
}
