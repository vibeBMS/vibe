//! LR2 skin loader (`.lr2skin` / `.csv`).
//!
//! Supported: `#INFORMATION`, `#RESOLUTION`, `#CUSTOMOPTION`, `#CUSTOMFILE`,
//! `#INCLUDE`, `#IF/#ELSEIF/#ELSE/#ENDIF`, `#SETOPTION`, `#IMAGE`,
//! `#SRC/#DST_IMAGE`, `_NUMBER`, `_TEXT` (drawn with system fonts),
//! `_BARGRAPH`, `_SLIDER`, `_GROOVEGAUGE`, `_BGA`, `_NOTE`, `_LN_*`,
//! `_MINE`, `_LINE`, `_JUDGELINE`, `_NOWJUDGE_1P/2P`, `_NOWCOMBO_1P/2P`,
//! `#STARTINPUT`, `#SCENETIME`, `#FADEOUT`, `#FINISHMARGIN`, `#PLAYSTART`,
//! `#CLOSE`, and the note / BPM / timing charts.
//!
//! beatoraja's additions: `#IMAGESET`, `.cim` images, `_HCN_*`, `_HIDDEN`,
//! `_LIFT`, `_TIMING_1P`, `_GROOVEGAUGE_EX`, `_SLIDER_REFNUMBER`,
//! `_BARGRAPH_REFNUMBER`, `_NOWJUDGE_3P` / `_NOWCOMBO_3P` (PMS),
//! `#DST_NOTE2`, `#DST_NOTE_EXPANSION_RATE`, `#JUDGETIMER`, `#STRETCH`,
//! `#CUSTOMOFFSET` and destination offsets (fields 21 on).
//! `#SRC_ONMOUSE` images show while the mouse is over them.
//! Not yet: `#LOADSTART` / `#LOADEND`.

mod commands;

use super::*;
use crate::platform::vfs;
use commands::{LaneImage, Line};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn ints(f: &[&str]) -> [i64; 22] {
    let mut v = [0i64; 22];
    for (i, s) in f.iter().enumerate().skip(1).take(21) {
        let t: String = s
            .replace('!', "-")
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        v[i] = t.parse().unwrap_or(0);
    }
    v
}

/// Colours from fields `first..` ("RRGGBB[AA]"), `defaults` where empty.
fn colors(f: &[&str], first: usize, defaults: &[&str]) -> Vec<[f32; 4]> {
    defaults
        .iter()
        .enumerate()
        .map(|(k, d)| {
            let s = f.get(first + k).map(|s| s.trim()).filter(|s| !s.is_empty());
            super::beatoraja::hex_color(s.unwrap_or(d))
        })
        .collect()
}

fn src(v: &[i64; 22], images: &ImageFiles) -> Option<Src> {
    let img = v[2] as usize;
    let (iw, ih) = images.size(img)?;
    let (x, y) = (v[3] as f32, v[4] as f32);
    let w = if v[5] < 0 { iw as f32 } else { v[5] as f32 };
    let h = if v[6] < 0 { ih as f32 } else { v[6] as f32 };
    Some(Src {
        image: img,
        frames: Src::grid([x, y, w, h], v[7], v[8]),
        cycle: v[9],
        timer: v[10] as i32,
    })
}

/// The judge display of a `_1P` / `_2P` / `_3P` command (beatoraja's
/// third one is the PMS middle region).
fn judge_side(cmd: &str) -> usize {
    if cmd.ends_with("3P") {
        2
    } else {
        usize::from(cmd.ends_with("2P"))
    }
}

/// beatoraja offset ids after an LR2 destination's 21 fields.
fn line_offsets(f: &[&str]) -> Vec<i32> {
    f.iter()
        .skip(21)
        .filter_map(|s| {
            let t: String = s
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '-')
                .collect();
            t.parse().ok()
        })
        .collect()
}

fn add_dst(dst: &mut Dst, v: &[i64; 22]) {
    add_dst_signed(dst, v, false)
}

/// A destination key; with `mirror` a negative width / height stays (the
/// image is drawn mirrored), else the rect is turned around.
fn add_dst_signed(dst: &mut Dst, v: &[i64; 22], mirror: bool) {
    let (mut x, mut y, mut w, mut h) = (v[3] as f32, v[4] as f32, v[5] as f32, v[6] as f32);
    if w < 0.0 && !mirror {
        x += w;
        w = -w;
    }
    if h < 0.0 && !mirror {
        y += h;
        h = -h;
    }
    if dst.keys.is_empty() {
        dst.acc = v[7] as i32;
        dst.blend = v[12] as i32;
        dst.center = v[15].clamp(0, 9) as usize;
        dst.loop_ms = v[16];
        dst.timer = v[17] as i32;
        dst.ops = [v[18], v[19], v[20]]
            .iter()
            .filter(|o| **o != 0)
            .map(|o| *o as i32)
            .collect();
    }
    let c = |n: i64| (n.clamp(0, 255) as f32) / 255.0;
    dst.add_key(Key {
        time: v[2],
        rect: Rect::new(x, y, w, h),
        color: [c(v[9]), c(v[10]), c(v[11]), c(v[8])],
        angle: v[14] as f32,
    });
}

/// Map an LR2 lane id (0 = 1P scratch, 1..9 = 1P keys, 10 = 2P scratch, ...).
fn lane_of(mode: vibebms_core::PlayMode, id: i64) -> Option<usize> {
    let (p, k) = ((id / 10) as usize, (id % 10) as usize);
    if p >= mode.players() {
        return None;
    }
    if k == 0 {
        return mode.scratch_lane(p);
    }
    (k - 1 < mode.keys_per_player()).then(|| p * mode.lanes_per_player() + k - 1)
}

#[derive(Default)]
struct Cond {
    skip: bool,
    taken: bool,
}

struct Loader<'a> {
    root: PathBuf,
    dir: PathBuf,
    options: HashMap<i32, bool>,
    static_option: &'a dyn Fn(i32) -> Option<bool>,
    files: HashMap<String, String>,
    skin: Skin,
    images: ImageFiles<'a>,
    cur: Option<usize>,
    pending_line: Option<Src>,
    /// beatoraja `#IMAGESET` images, chosen from by `#SRC_IMAGESET`.
    imagesets: Vec<Option<Src>>,
    /// beatoraja `#STRETCH` for the images that follow.
    stretch: i32,
    /// User values of `#CUSTOMOFFSET`s, by name.
    user_offsets: HashMap<String, [i32; 6]>,
    /// The user's `#CUSTOMFILE` choices, by name.
    user_files: HashMap<String, String>,
    /// Buttons that belong to a panel: (object, panel option id).
    panel_ops: Vec<(usize, i32)>,
    /// `#LR2FONT` list (indexed by `#SRC_TEXT`): font in `Skin::fonts`.
    fonts: Vec<Option<usize>>,
    /// Size of the chart a `#SRC_*CHART*` defines (its `#DST` gives the
    /// bottom-left corner).
    chart_field: Option<(f32, f32)>,
    /// `#RESOLUTION` was given (else it is guessed from the layout).
    resolution_set: bool,
    depth: usize,
    conds: Vec<Cond>,
}

impl Loader<'_> {
    fn font(&self, i: i64) -> Option<usize> {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.fonts.get(i).copied().flatten())
    }

    fn opt(&self, id: i32) -> bool {
        let (abs, neg) = (id.abs(), id < 0);
        let v = self
            .options
            .get(&abs)
            .copied()
            .or_else(|| (self.static_option)(abs))
            .unwrap_or(false);
        v != neg
    }

    fn cond(&self, f: &[&str]) -> bool {
        f.iter().skip(1).filter(|s| !s.trim().is_empty()).all(|s| {
            let t: String = s
                .replace('!', "-")
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '-')
                .collect();
            t.parse::<i32>().map(|o| self.opt(o)).unwrap_or(false)
        })
    }

    /// Resolve `LR2files\Theme\...` and wildcards (`#CUSTOMFILE`). Files
    /// outside the skins folder are not read (an empty path).
    fn path(&self, raw: &str) -> PathBuf {
        let mut s = raw.trim().replace('\\', "/");
        let base = match s.to_ascii_lowercase().find("lr2files/theme/") {
            Some(i) => {
                s = s[i + "lr2files/theme/".len()..].to_string();
                &self.root
            }
            None => &self.dir,
        };
        let p = vfs::normalize(&base.join(&s));
        if Path::new(&s).has_root() || !p.starts_with(vfs::normalize(&self.root)) {
            log::warn!("skin: {raw} is outside the skins folder");
            return PathBuf::new();
        }
        self.glob(&vfs::resolve_case(&p), raw)
    }

    fn glob(&self, p: &Path, raw: &str) -> PathBuf {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.contains('*') {
            return p.to_path_buf();
        }
        let dir = p.parent().unwrap_or(Path::new(""));
        let (pre, post) = name.split_once('*').unwrap_or((&name, ""));
        let (pre, post) = (pre.to_lowercase(), post.to_lowercase());
        let mut cands: Vec<PathBuf> = vfs::read_dir(dir)
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.path)
            .filter(|c| {
                let n = c
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                n.starts_with(&pre) && n.ends_with(&post) && n.len() >= pre.len() + post.len()
            })
            .collect();
        cands.sort();
        let want = self
            .files
            .get(&raw.replace('\\', "/"))
            .cloned()
            .unwrap_or_default()
            .to_lowercase();
        cands
            .iter()
            .find(|c| {
                !want.is_empty()
                    && c.file_stem()
                        .map(|s| s.to_string_lossy().to_lowercase().contains(&want))
                        .unwrap_or(false)
            })
            .or(cands.first())
            .cloned()
            .unwrap_or_else(|| p.to_path_buf())
    }

    fn push(&mut self, kind: Kind) {
        self.skin.objects.push(Obj {
            kind,
            dst: Dst::default(),
            act: None,
        });
        self.cur = Some(self.skin.objects.len() - 1);
    }

    /// The bar list, added to the draw order at its first command.
    fn songlist(&mut self) -> &mut SongListSkin {
        if self.skin.songlist.is_none() {
            self.skin.songlist = Some(SongListSkin::default());
            self.push(Kind::SongList);
            self.cur = None;
        }
        self.skin.songlist.as_mut().unwrap()
    }

    fn mode(&self) -> vibebms_core::PlayMode {
        self.skin
            .kind
            .mode()
            .unwrap_or(vibebms_core::PlayMode::Beat7K)
    }

    /// A note image of the lane `v` names (the first one given counts).
    fn lane_src(&mut self, v: &[i64; 22], image: LaneImage) {
        let mode = self.mode();
        let Some(l) = lane_of(mode, v[1]) else { return };
        let Some(s) = src(v, &self.images) else {
            return;
        };
        if self.skin.lanes.len() < mode.lane_count() {
            self.skin
                .lanes
                .resize(mode.lane_count(), LaneSkin::default());
        }
        let slot = image(&mut self.skin.lanes[l]);
        if slot.is_none() {
            *slot = Some(s);
        }
    }

    fn line(&mut self, raw: &str) {
        let line = raw.trim_end_matches(['\r', '\n']);
        if !line.starts_with('#') {
            return;
        }
        // skins saved from spreadsheets carry stray quotes ("…,930\"")
        let line = line.replace('"', "");
        let f: Vec<&str> = line.split(',').collect();
        let cmd = f[0].trim().to_ascii_uppercase();
        // LR2 decides each #IF level by its own condition: the innermost
        // level says whether a line is read, even inside a level skipped
        let skipping = self.conds.last().is_some_and(|c| c.skip);
        match cmd.as_str() {
            "#IF" => {
                let ok = self.cond(&f);
                self.conds.push(Cond {
                    skip: !ok,
                    taken: ok,
                });
            }
            "#ELSEIF" => {
                if let Some(c) = self.conds.last() {
                    let ok = !c.taken && self.cond(&f);
                    let c = self.conds.last_mut().unwrap();
                    c.skip = !ok;
                    c.taken |= ok;
                }
            }
            "#ELSE" => {
                if let Some(c) = self.conds.last_mut() {
                    c.skip = c.taken;
                    c.taken = true;
                }
            }
            "#ENDIF" => {
                self.conds.pop();
            }
            _ if skipping => {}
            cmd => self.command(Line {
                cmd,
                f: &f,
                v: ints(&f),
                s1: f.get(1).map_or("", |s| s.trim()),
            }),
        }
    }
}

/// The images of the last screen of the play flow, for `#IMAGE,CONTINUE`.
static LAST_IMAGES: std::sync::Mutex<Vec<Option<PathBuf>>> = std::sync::Mutex::new(Vec::new());

/// The skin of a screen of the play flow (select to result) was loaded:
/// its images are what the next screen goes on with (none when it is not
/// an LR2 skin). Other screens (key config, skin select and its previews)
/// leave them.
pub fn keep_images(skin: &Skin) {
    if let Ok(mut last) = LAST_IMAGES.lock() {
        *last = match SkinFormat::of(&skin.path) {
            Some(SkinFormat::Lr2) => skin.images.clone(),
            _ => Vec::new(),
        };
    }
}

/// Load an LR2 skin with the user's choices: options, `#CUSTOMFILE` files
/// and `#CUSTOMOFFSET` values (by name). `static_option` answers chart /
/// config dependent `#IF` conditions at load time; `image_size` checks (and
/// measures) image files.
pub fn load_with(
    path: &Path,
    skins_root: &Path,
    user: &UserSettings,
    static_option: &dyn Fn(i32) -> Option<bool>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let (user_options, user_offsets) = (&user.options, &user.offsets);
    let bytes = vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = vibebms_core::encoding::decode_text(&bytes);
    let mut l = Loader {
        root: skins_root.to_path_buf(),
        dir: path.parent().unwrap_or(Path::new("")).to_path_buf(),
        options: user_options.clone(),
        static_option,
        files: HashMap::new(),
        skin: Skin::new(path, SkinType::Play7, 640.0, 480.0),
        images: ImageFiles::new(image_size),
        cur: None,
        pending_line: None,
        imagesets: Vec::new(),
        stretch: 0,
        user_offsets: user_offsets.clone(),
        user_files: user.files.clone(),
        panel_ops: Vec::new(),
        fonts: Vec::new(),
        resolution_set: false,
        chart_field: None,
        depth: 0,
        conds: Vec::new(),
    };
    for line in text.lines() {
        l.line(line);
    }
    // a button inside a panel is shown (and clickable) only while it is open
    for (i, op) in std::mem::take(&mut l.panel_ops) {
        l.skin.objects[i].dst.ops.push(op);
    }
    let mut skin = l.skin;
    skin.images = l.images.finish();
    resolve_static_ops(&mut skin, &l.options);
    if !l.resolution_set {
        (skin.width, skin.height) = guess_resolution(&skin);
    }
    // text is anchored at x: its left edge, centre or right edge by align
    let anchor = |d: &mut Dst, align: i32| {
        let k = align.clamp(0, 2) as f32 * 0.5;
        for key in &mut d.keys {
            key.rect.x -= key.rect.w * k;
        }
    };
    for o in &mut skin.objects {
        if let Kind::Text { align, .. } = o.kind {
            anchor(&mut o.dst, align);
        }
    }
    if let Some((d, align, ..)) = skin.songlist.as_mut().and_then(|s| s.text.as_mut()) {
        anchor(d, *align);
    }
    // beatoraja's own key config skin is empty: its screen draws the rows
    if skin.objects.is_empty() && skin.kind != SkinType::KeyConfig {
        return Err(format!("{}: no drawable objects", path.display()));
    }
    Ok(skin)
}

/// Screen size of a skin without `#RESOLUTION`: HD-patched LR2 builds run
/// skins made for 1280x720 / 1920x1080 without saying so. Lunatic Vibes'
/// guess: where the objects' last positions reach (a full-screen image
/// counts as twenty).
fn guess_resolution(skin: &Skin) -> (f32, f32) {
    let mut rects: Vec<Rect> = skin
        .objects
        .iter()
        .filter_map(|o| o.dst.keys.last().map(|k| k.rect))
        .collect();
    if let Some(sl) = &skin.songlist {
        rects.extend(
            sl.on
                .iter()
                .chain(&sl.off)
                .flatten()
                .filter_map(|d| d.keys.last().map(|k| k.rect)),
        );
    }
    let (mut sd, mut hd, mut fhd) = (0usize, 0usize, 0usize);
    let beyond = |r: &Rect, w: f32, h: f32| r.x > w || r.y > h || r.right() > w || r.bottom() > h;
    for r in &rects {
        if beyond(r, 1920.0, 1080.0) {
            continue;
        }
        let full = |w: f32, h: f32| r.x == 0.0 && r.y == 0.0 && r.w == w && r.h == h;
        if beyond(r, 1280.0, 720.0) {
            fhd += 1;
        } else if beyond(r, 640.0, 480.0) {
            hd += if full(1280.0, 720.0) { 20 } else { 1 };
        } else {
            sd += if full(640.0, 480.0) { 20 } else { 1 };
        }
    }
    let n = (sd + hd + fhd).max(1) as f32;
    if fhd as f32 / n > 0.5 {
        (1920.0, 1080.0)
    } else if hd as f32 / n > 0.35 {
        (1280.0, 720.0)
    } else {
        (640.0, 480.0)
    }
}

/// Unicode characters of an LR2 font code (Shift_JIS based, as beatoraja
/// maps them).
fn font_chars(code: i64) -> Vec<char> {
    if code == 288 {
        return vec!['\u{301c}', '\u{ff5e}'];
    }
    let bytes: Vec<u8> = if code >= 8127 {
        let c = (code + 49281) as u16;
        vec![(c >> 8) as u8, c as u8]
    } else if code >= 256 {
        let c = (code + 32832) as u16;
        vec![(c >> 8) as u8, c as u8]
    } else if code >= 0 {
        vec![code as u8]
    } else {
        return Vec::new();
    };
    let (s, _, err) = encoding_rs::SHIFT_JIS.decode(&bytes);
    if err {
        return Vec::new();
    }
    s.chars().take(1).collect()
}

/// Read an `.lr2font`: the font and its texture files (with sizes, `None`
/// when missing). Glyph image indices are positions in that list.
#[allow(clippy::type_complexity)]
pub fn load_font(
    path: &Path,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Option<(BitmapFont, Vec<(PathBuf, Option<(u32, u32)>)>)> {
    let bytes = vfs::read(path).ok()?;
    let text = vibebms_core::encoding::decode_text(&bytes);
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut font = BitmapFont::default();
    let mut images = Vec::new();
    let mut tex: HashMap<i64, usize> = HashMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        let num = |i: usize| -> i64 {
            f.get(i)
                .map(|s| s.replace('!', "-").replace(' ', ""))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        };
        match f[0].to_ascii_uppercase().as_str() {
            "#S" => font.size = num(1) as f32,
            "#M" => font.margin = num(1) as f32,
            "#T" => {
                let p = vfs::resolve_case(
                    &dir.join(f.get(2).copied().unwrap_or("").replace('\\', "/")),
                );
                let size = image_size(&p);
                if size.is_some() {
                    tex.insert(num(1), images.len());
                    images.push((p, size));
                }
            }
            "#R" => {
                if let Some(&img) = tex.get(&num(2)) {
                    let r = [num(3) as f32, num(4) as f32, num(5) as f32, num(6) as f32];
                    for c in font_chars(num(1)) {
                        font.glyphs.insert(c, (img, r));
                    }
                }
            }
            _ => {}
        }
    }
    (!font.glyphs.is_empty()).then_some((font, images))
}

/// The `#CUSTOMOPTION`s of a skin (includes and conditions are not followed).
/// beatoraja `#CUSTOMOFFSET,name,id,x,y,w,h,angle,alpha` (1 = adjustable)
/// of an LR2 skin header.
pub fn custom_offsets(path: &Path) -> Vec<CustomOffset> {
    let Ok(b) = vfs::read(path) else {
        return Vec::new();
    };
    vibebms_core::encoding::decode_text(&b)
        .lines()
        .filter(|l| {
            l.trim_start()
                .to_ascii_uppercase()
                .starts_with("#CUSTOMOFFSET,")
        })
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            let id: i32 = f.get(2)?.parse().ok()?;
            let mut fields = [true; 6];
            for (k, b) in fields.iter_mut().enumerate() {
                if let Some(Ok(n)) = f
                    .get(3 + k)
                    .filter(|x| !x.is_empty())
                    .map(|x| x.parse::<i32>())
                {
                    *b = n > 0;
                }
            }
            Some(CustomOffset {
                name: f.get(1)?.to_string(),
                id,
                fields,
            })
        })
        .collect()
}

pub fn custom_options(path: &Path) -> Vec<CustomOption> {
    let Ok(b) = vfs::read(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for l in vibebms_core::encoding::decode_text(&b).lines() {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        match f[0].to_ascii_uppercase().as_str() {
            "#CUSTOMOPTION" => {
                let (Some(name), Some(Ok(first))) = (f.get(1), f.get(2).map(|x| x.parse::<i32>()))
                else {
                    continue;
                };
                let labels: Vec<String> = f
                    .iter()
                    .skip(3)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                out.push(CustomOption {
                    name: name.to_string(),
                    first_id: first,
                    ops: (0..labels.len() as i32).map(|i| first + i).collect(),
                    labels,
                    default: 0,
                });
            }
            // a choice of files: their names without extension
            "#CUSTOMFILE" => {
                let (Some(name), Some(p)) = (f.get(1), f.get(2).and_then(|p| theme_file(path, p)))
                else {
                    continue;
                };
                let labels: Vec<String> = glob_paths(&p)
                    .iter()
                    .filter(|c| !c.to_string_lossy().contains('*'))
                    .filter_map(|c| c.file_stem().map(|n| n.to_string_lossy().into_owned()))
                    .collect();
                let def = f.get(3).map(|d| d.to_lowercase()).unwrap_or_default();
                if labels.len() > 1 {
                    out.push(CustomOption {
                        default: labels
                            .iter()
                            .position(|l| !def.is_empty() && l.to_lowercase().contains(&def))
                            .unwrap_or(0),
                        name: name.to_string(),
                        first_id: 0,
                        labels,
                        ops: Vec::new(),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// A skin's `LR2files\Theme\<theme>\...` path without knowing the skins
/// folder: under the skin's ancestor named `<theme>`, else under the one
/// holding the path.
fn theme_file(skin: &Path, raw: &str) -> Option<PathBuf> {
    let s = raw.replace('\\', "/");
    let i = s.to_ascii_lowercase().find("lr2files/theme/")?;
    let rest = &s[i + "lr2files/theme/".len()..];
    let (theme, tail) = rest.split_once('/')?;
    let dirs: Vec<&Path> = skin.ancestors().skip(1).collect();
    let named = dirs.iter().find(|a| {
        a.file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(theme))
    });
    let base = named.or_else(|| {
        dirs.iter().find(|a| {
            Path::new(tail)
                .parent()
                .is_some_and(|t| vfs::is_dir(&a.join(t)))
        })
    })?;
    Some(vfs::resolve_case(&base.join(tail)))
}

/// Read only the header: (type, name).
pub fn header(path: &Path) -> Option<(SkinType, String)> {
    let b = vfs::read(path).ok()?;
    let text = vibebms_core::encoding::decode_text(&b);
    text.lines()
        .find(|l| l.to_ascii_uppercase().starts_with("#INFORMATION"))
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (
                SkinType::from_id(f.get(1).and_then(|s| s.trim().parse().ok()).unwrap_or(0)),
                f.get(2).unwrap_or(&"").trim().to_string(),
            )
        })
}

/// Resolve a `*` wildcard in the file name to the first matching file.
pub fn glob_path(p: &Path) -> PathBuf {
    glob_paths(p)
        .into_iter()
        .next()
        .unwrap_or_else(|| p.to_path_buf())
}

/// The files matching a `*` in the file name of `p` (sorted), else `p`.
pub fn glob_paths(p: &Path) -> Vec<PathBuf> {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some((pre, post)) = name.split_once('*') else {
        return vec![p.to_path_buf()];
    };
    let (pre, post) = (pre.to_lowercase(), post.to_lowercase());
    let dir = p.parent().unwrap_or(Path::new(""));
    let mut c: Vec<PathBuf> = vfs::read_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.path)
        .filter(|c| {
            let n = c
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            n.starts_with(&pre) && n.ends_with(&post) && n.len() >= pre.len() + post.len()
        })
        .collect();
    c.sort();
    c
}

/// Play skins below `dir`: LR2 (`.lr2skin`) and beatoraja (`.json`, `.luaskin`).
pub fn find_skins(dir: &Path) -> Vec<(PathBuf, SkinType, String)> {
    fn walk(d: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 4 {
            return;
        }
        for e in vfs::read_dir(d).unwrap_or_default() {
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
    files
        .into_iter()
        .filter_map(|p| super::header(&p).map(|(t, n)| (p, t, n)))
        .collect()
}

#[cfg(test)]
mod tests;
