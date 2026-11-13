//! LR2 skin loader (`.lr2skin` / `.csv`).
//!
//! Supported: `#INFORMATION`, `#RESOLUTION`, `#CUSTOMOPTION`, `#CUSTOMFILE`,
//! `#INCLUDE`, `#IF/#ELSEIF/#ELSE/#ENDIF`, `#SETOPTION`, `#IMAGE`,
//! `#SRC/#DST_IMAGE`, `_NUMBER`, `_TEXT` (drawn with system fonts),
//! `_BARGRAPH`, `_SLIDER`, `_GROOVEGAUGE`, `_BGA`, `_NOTE`, `_LN_*`,
//! `_MINE`, `_LINE`, `_JUDGELINE`, `_NOWJUDGE_1P/2P`, `_NOWCOMBO_1P/2P`,
//! `#STARTINPUT`, `#SCENETIME`, `#FADEOUT`, `#FINISHMARGIN`.
//! Not yet: onmouse, charts beyond the gauge / score chart.

use super::*;
use crate::platform::vfs;
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

fn src(v: &[i64; 22], image_sizes: &[Option<(u32, u32)>]) -> Option<Src> {
    let img = v[2] as usize;
    let (iw, ih) = image_sizes.get(img).copied().flatten()?;
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

fn add_dst(dst: &mut Dst, v: &[i64; 22]) {
    let (mut x, mut y, mut w, mut h) = (v[3] as f32, v[4] as f32, v[5] as f32, v[6] as f32);
    if w < 0.0 {
        x += w;
        w = -w;
    }
    if h < 0.0 {
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
    sizes: Vec<Option<(u32, u32)>>,
    image_size: &'a dyn Fn(&Path) -> Option<(u32, u32)>,
    cur: Option<usize>,
    lane_kind: usize,
    judge_side: usize,
    pending_line: Option<Src>,
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
        self.glob(&p, raw)
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

    fn lane_src(&mut self, v: &[i64; 22], which: usize) {
        let mode = self.mode();
        let Some(l) = lane_of(mode, v[1]) else { return };
        let Some(s) = src(v, &self.sizes) else { return };
        if self.skin.lanes.len() < mode.lane_count() {
            self.skin
                .lanes
                .resize(mode.lane_count(), LaneSkin::default());
        }
        let ls = &mut self.skin.lanes[l];
        let slot = match which {
            0 => &mut ls.note,
            1 => &mut ls.ln_start,
            2 => &mut ls.ln_end,
            3 => &mut ls.ln_body,
            4 => &mut ls.ln_body_active,
            _ => &mut ls.mine,
        };
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
        let top = self.conds_top();
        match cmd.as_str() {
            "#IF" => {
                let ok = !top.0 && self.cond(&f);
                self.conds.push(Cond {
                    skip: !ok,
                    taken: ok,
                });
                return;
            }
            "#ELSEIF" => {
                if let Some(c) = self.conds.last() {
                    let (taken, parent_skip) = (c.taken, self.parent_skip());
                    let ok = !taken && !parent_skip && self.cond(&f);
                    let c = self.conds.last_mut().unwrap();
                    c.skip = !ok;
                    c.taken |= ok;
                }
                return;
            }
            "#ELSE" => {
                let parent_skip = self.parent_skip();
                if let Some(c) = self.conds.last_mut() {
                    c.skip = c.taken || parent_skip;
                    c.taken = true;
                }
                return;
            }
            "#ENDIF" => {
                self.conds.pop();
                return;
            }
            _ => {}
        }
        if top.0 {
            return;
        }
        let v = ints(&f);
        let s1 = f.get(1).map(|s| s.trim()).unwrap_or("");
        match cmd.as_str() {
            "#INFORMATION" => {
                self.skin.kind = SkinType::from_id(v[1] as i32);
                self.skin.name = f.get(2).unwrap_or(&"").trim().to_string();
                self.skin.author = f.get(3).unwrap_or(&"").trim().to_string();
            }
            "#RESOLUTION" => {
                let (w, h) = [
                    (640.0, 480.0),
                    (1280.0, 720.0),
                    (1920.0, 1080.0),
                    (3840.0, 2160.0),
                ][(v[1].clamp(0, 3)) as usize];
                self.skin.width = w;
                self.skin.height = h;
                self.resolution_set = true;
            }
            "#CUSTOMOPTION" => {
                let labels: Vec<String> = f
                    .iter()
                    .skip(3)
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                let first = v[2] as i32;
                for i in 0..labels.len() as i32 {
                    self.options.entry(first + i).or_insert(i == 0);
                }
                let ops = (0..labels.len() as i32).map(|i| first + i).collect();
                self.skin.custom_options.push(CustomOption {
                    name: s1.to_string(),
                    first_id: first,
                    labels,
                    ops,
                    default: 0,
                });
            }
            "#CUSTOMFILE" => {
                if let Some(p) = f.get(2) {
                    self.files.insert(
                        p.trim().replace('\\', "/"),
                        f.get(3).map(|s| s.trim().to_string()).unwrap_or_default(),
                    );
                }
            }
            "#SETOPTION" => {
                self.options.insert(v[1] as i32, v[2] >= 1);
            }
            "#INCLUDE" => {
                if self.depth < 8 {
                    let p = self.path(s1);
                    if let Ok(b) = vfs::read(&p) {
                        self.depth += 1;
                        for l in vibebms_core::encoding::decode_text(&b).lines() {
                            self.line(l);
                        }
                        self.depth -= 1;
                    }
                }
            }
            "#IMAGE" => {
                let p = self.path(s1);
                let size = (self.image_size)(&p);
                self.sizes.push(size);
                self.skin.images.push(size.map(|_| p));
            }
            "#TRANSCOLOR" => {
                let c = |n: i64| n.clamp(0, 255) as u8;
                self.skin.trans_color = Some([c(v[1]), c(v[2]), c(v[3])]);
            }
            "#STARTINPUT" => self.skin.input_ms = v[1],
            "#SCENETIME" => self.skin.scene_ms = v[1],
            "#FADEOUT" => self.skin.fadeout_ms = v[1],
            "#FINISHMARGIN" => self.skin.finish_margin_ms = v[1],
            "#SRC_IMAGE" => {
                self.cur = None;
                if v[2] >= 100 {
                    self.push(Kind::RefImage(v[2] as i32));
                } else if let Some(s) = src(&v, &self.sizes) {
                    self.push(Kind::Image(s));
                }
            }
            "#SRC_BUTTON" => {
                // gr,x,y,w,h,divx,divy,cycle,timer,type,click,panel: one frame
                // per button value, clicking fires the button type
                self.cur = None;
                let Some(s_) = src(&v, &self.sizes) else {
                    return;
                };
                let srcs = s_
                    .frames
                    .iter()
                    .map(|f| Src {
                        frames: vec![*f],
                        cycle: 0,
                        ..s_.clone()
                    })
                    .collect();
                let id = v[11] as i32;
                self.push(Kind::ImageSet { srcs, id });
                let i = self.skin.objects.len() - 1;
                self.skin.objects[i].act = Some((id, v[12] as i32));
                if v[13] > 0 {
                    self.panel_ops.push((i, 20 + v[13] as i32));
                }
            }
            "#SRC_NUMBER" | "#SRC_NOWCOMBO_1P" | "#SRC_NOWCOMBO_2P" => {
                self.cur = None;
                let Some(mut s) = src(&v, &self.sizes) else {
                    return;
                };
                let n = s.frames.len();
                let digits = if n % 24 == 0 {
                    12
                } else if n % 11 == 0 {
                    s.frames.truncate(11);
                    11
                } else if n >= 10 {
                    s.frames.truncate(10);
                    10
                } else {
                    return;
                };
                if cmd == "#SRC_NUMBER" {
                    self.push(Kind::Number {
                        src: s,
                        digits,
                        id: v[11] as i32,
                        align: v[12] as i32,
                        keta: v[13].clamp(1, 32) as usize,
                        pad: (digits > 10).then_some(10),
                    });
                } else {
                    let side = usize::from(cmd.ends_with("2P"));
                    let j = if v[1] <= 5 {
                        (5 - v[1]) as usize
                    } else {
                        return;
                    };
                    let keta = v[13].clamp(1, 32) as usize;
                    self.skin.judges[side].combo[j] = Some((s, Dst::default(), keta));
                    self.judge_side = side;
                    self.lane_kind = j;
                }
            }
            "#DST_NOWCOMBO_1P" | "#DST_NOWCOMBO_2P" => {
                let side = usize::from(cmd.ends_with("2P"));
                if v[1] <= 5 {
                    if let Some((_, d, _)) =
                        self.skin.judges[side].combo[(5 - v[1]) as usize].as_mut()
                    {
                        add_dst(d, &v);
                    }
                }
            }
            "#LR2FONT" => {
                // a missing font keeps its index (`CONTINUE` placeholders)
                let p = self.path(s1);
                let font = load_font(&p, self.image_size).map(|(f, imgs)| {
                    let base = self.skin.images.len();
                    for (path, size) in imgs {
                        self.sizes.push(size);
                        self.skin.images.push(size.map(|_| path));
                    }
                    let mut f = f;
                    for g in f.glyphs.values_mut() {
                        g.0 += base;
                    }
                    self.skin.fonts.push(f);
                    self.skin.fonts.len() - 1
                });
                self.fonts.push(font);
            }
            "#SRC_TEXT" => {
                let font = self.font(v[2]);
                self.push(Kind::Text {
                    id: v[3] as i32,
                    font,
                    align: v[4] as i32,
                    overflow: 0,
                })
            }
            "#SRC_BARGRAPH" => {
                self.cur = None;
                if let Some(s) = src(&v, &self.sizes) {
                    self.push(Kind::Bar {
                        src: s,
                        id: v[11] as i32 + 100,
                        dir: v[12] as i32,
                    });
                }
            }
            "#SRC_SLIDER" => {
                self.cur = None;
                if let Some(s) = src(&v, &self.sizes) {
                    self.push(Kind::Slider {
                        src: s,
                        id: v[13] as i32,
                        dir: v[11] as i32,
                        range: v[12] as f32,
                    });
                }
            }
            "#SRC_GROOVEGAUGE" => {
                self.cur = None;
                if let Some(s) = src(&v, &self.sizes) {
                    let parts = if v[13] > 0 {
                        v[13] as usize
                    } else if self.mode() == vibebms_core::PlayMode::Popn9K {
                        24
                    } else {
                        50
                    };
                    let (pw, ph) = (s.frames[0][2], s.frames[0][3]);
                    self.push(Kind::Gauge {
                        src: s,
                        part_w: pw,
                        part_h: ph,
                        step: (v[11] as f32, v[12] as f32),
                        parts,
                        split: false,
                    });
                }
            }
            "#SRC_BGA" => self.push(Kind::Bga),
            // music select bar list
            "#BAR_CENTER" => self.songlist().center = v[1].max(0) as usize,
            "#BAR_AVAILABLE" => {
                let (a, b) = (v[1].max(0) as usize, v[2].max(0) as usize);
                self.songlist().clickable = (a.min(b)..=a.max(b)).collect();
            }
            "#SRC_BAR_BODY" => {
                // LR2 bar types -> BAR_* (song, folder, custom folder, new,
                // rival, ?, course, ...)
                const MAP: [usize; 10] = [
                    BAR_SONG,
                    BAR_FOLDER,
                    BAR_TABLE,
                    BAR_SONG,
                    BAR_FOLDER,
                    BAR_COMMAND,
                    BAR_GRADE,
                    BAR_NOGRADE,
                    BAR_SEARCH,
                    BAR_NOSONG,
                ];
                let Some(t) = MAP.get(v[1].max(0) as usize).copied() else {
                    return;
                };
                let sz = src(&v, &self.sizes);
                let sl = self.songlist();
                if sl.body.len() <= t {
                    sl.body.resize(t + 1, None);
                }
                sl.body[t] = sz;
            }
            "#DST_BAR_BODY_OFF" | "#DST_BAR_BODY_ON" => {
                let slot = v[1].max(0) as usize;
                let on = cmd.ends_with("ON");
                let sl = self.songlist();
                let list = if on { &mut sl.on } else { &mut sl.off };
                if list.len() <= slot {
                    list.resize(slot + 1, None);
                }
                add_dst(list[slot].get_or_insert_with(Dst::default), &v);
            }
            "#SRC_BAR_TITLE" => {
                if v[1] == 0 {
                    let font = self.font(v[2]);
                    let sl = self.songlist();
                    let t = sl.text.get_or_insert_with(|| (Dst::default(), 0, 0, None));
                    t.1 = v[4] as i32;
                    t.3 = font;
                }
            }
            "#DST_BAR_TITLE" => {
                if v[1] == 0 {
                    let sl = self.songlist();
                    let (d, ..) = sl.text.get_or_insert_with(|| (Dst::default(), 0, 0, None));
                    add_dst(d, &v);
                }
            }
            "#SRC_BAR_LEVEL" => {
                let Some(s_) = src(&v, &self.sizes) else {
                    return;
                };
                let n = s_.frames.len();
                let digits = if n % 24 == 0 {
                    12
                } else if n % 11 == 0 {
                    11
                } else {
                    10
                };
                let k = v[1].clamp(0, 6) as usize;
                let font = NumberFont {
                    src: s_,
                    digits,
                    keta: v[13].clamp(1, 32) as usize,
                    align: v[12] as i32,
                };
                let sl = self.songlist();
                if sl.level.len() <= k {
                    sl.level.resize(k + 1, None);
                }
                sl.level[k] = Some((font, Dst::default()));
            }
            "#DST_BAR_LEVEL" => {
                let k = v[1].clamp(0, 6) as usize;
                if let Some(Some((_, d))) = self.songlist().level.get_mut(k) {
                    add_dst(d, &v);
                }
            }
            "#SRC_BAR_LAMP" => {
                // LR2 lamps: no play, failed, easy, clear, hard, full combo, ...
                const MAP: [usize; 8] = [0, 1, 4, 5, 6, 8, 9, 10];
                let Some(k) = MAP.get(v[1].max(0) as usize).copied() else {
                    return;
                };
                let s_ = src(&v, &self.sizes);
                let sl = self.songlist();
                if sl.lamp.len() <= k {
                    sl.lamp.resize(k + 1, None);
                }
                sl.lamp[k] = s_.map(|s_| (s_, Dst::default()));
            }
            "#DST_BAR_LAMP" => {
                const MAP: [usize; 8] = [0, 1, 4, 5, 6, 8, 9, 10];
                let Some(k) = MAP.get(v[1].max(0) as usize).copied() else {
                    return;
                };
                if let Some(Some((_, d))) = self.songlist().lamp.get_mut(k) {
                    add_dst(d, &v);
                }
            }
            // charts (beatoraja's look and geometry; the DST point size
            // and colour of LR2 are not used)
            "#SRC_GAUGECHART_1P" | "#SRC_GAUGECHART" => {
                // a second index only recolours LR2's points: one chart
                self.cur = None;
                if v[1] == 0 {
                    self.push(Kind::Graph(Graph::Gauge(GaugeGraph {
                        colors: Some(super::beatoraja::gauge_colors(&serde_json::Value::Null)),
                        line_width: v[6].max(1) as f32,
                        delay: (v[14] - v[13]).max(0),
                    })));
                    self.chart_field = Some((v[11] as f32, v[12] as f32));
                }
            }
            "#SRC_NOTECHART_1P" | "#SRC_NOTECHART" => {
                self.push(Kind::Graph(Graph::Notes(NotesGraph {
                    kind: v[1].clamp(0, 2) as usize,
                    delay: v[15],
                    back: v[16] == 0,
                    order_reverse: v[17] != 0,
                    no_gap: v[18] != 0,
                    no_gap_x: v[19] != 0,
                })));
                self.chart_field = Some((v[11] as f32, v[12] as f32));
            }
            "#SRC_BPMCHART" => {
                self.push(Kind::Graph(Graph::Bpm(BpmGraph {
                    colors: colors(
                        &f,
                        5,
                        &["00ff00", "0000ff", "ff0000", "ffff00", "ff00ff", "7f7f7f"],
                    ),
                    line_width: v[4].max(1) as f32,
                    delay: v[3],
                })));
                self.chart_field = Some((v[1] as f32, v[2] as f32));
            }
            "#SRC_TIMINGCHART_1P" => {
                self.push(Kind::Graph(Graph::Timing(TimingGraph {
                    bins: v[4].max(3),
                    colors: Some(colors(
                        &f,
                        7,
                        &[
                            "00FF00FF", "FFFFFFFF", "FFFFFFFF", "000088FF", "008800FF", "888800FF",
                            "880000FF", "000000FF",
                        ],
                    )),
                    line_width: v[6].max(1) as f32,
                    average: v[15] != 0,
                    deviation: v[16] != 0,
                })));
                self.chart_field = Some((v[4] as f32, v[5] as f32));
            }
            "#DST_GAUGECHART_1P"
            | "#DST_GAUGECHART"
            | "#DST_NOTECHART_1P"
            | "#DST_NOTECHART"
            | "#DST_BPMCHART"
            | "#DST_TIMINGCHART_1P" => {
                if let (Some(i), Some((w, h))) = (self.cur, self.chart_field) {
                    let mut v2 = v;
                    v2[4] -= h as i64;
                    v2[5] = w as i64;
                    v2[6] = h as i64;
                    v2[9..=11].fill(255);
                    add_dst(&mut self.skin.objects[i].dst, &v2);
                }
            }
            "#SRC_SCORECHART" => self.push(Kind::Graph(Graph::Notes(NotesGraph {
                kind: 1,
                ..Default::default()
            }))),
            "#SRC_JUDGELINE" => {
                self.cur = None;
                if let Some(s) = src(&v, &self.sizes) {
                    self.push(Kind::JudgeLine(s));
                }
            }
            "#SRC_LINE" => self.pending_line = src(&v, &self.sizes),
            "#DST_LINE" => {
                if let Some(s) = self.pending_line.take() {
                    let mut d = Dst::default();
                    add_dst(&mut d, &v);
                    self.skin.bar_lines.push((s, d));
                } else if let Some((_, d)) = self.skin.bar_lines.last_mut() {
                    add_dst(d, &v);
                }
            }
            "#SRC_NOTE" => self.lane_src(&v, 0),
            "#SRC_LN_START" => self.lane_src(&v, 1),
            "#SRC_LN_END" => self.lane_src(&v, 2),
            "#SRC_LN_BODY" | "#SRC_LN_BODY_INACTIVE" => self.lane_src(&v, 3),
            "#SRC_LN_BODY_ACTIVE" => self.lane_src(&v, 4),
            "#SRC_MINE" => self.lane_src(&v, 5),
            "#DST_NOTE" => {
                let mode = self.mode();
                if let Some(l) = lane_of(mode, v[1]) {
                    if self.skin.lanes.len() < mode.lane_count() {
                        self.skin
                            .lanes
                            .resize(mode.lane_count(), LaneSkin::default());
                    }
                    // x, y, w, h is the note at the judge line: the lane runs
                    // from the top of the screen down to y + h, notes are h tall
                    if self.skin.lanes[l].rect.is_none() {
                        self.skin.lanes[l].rect = Some(Rect::new(
                            v[3] as f32,
                            0.0,
                            v[5] as f32,
                            (v[4] + v[6]) as f32,
                        ));
                        self.skin.lanes[l].note_h = Some(v[6] as f32);
                    }
                    if !self
                        .skin
                        .objects
                        .iter()
                        .any(|o| matches!(o.kind, Kind::Notes))
                    {
                        self.skin.objects.push(Obj {
                            kind: Kind::Notes,
                            dst: Dst::default(),
                            act: None,
                        });
                    }
                }
                self.cur = None;
            }
            "#SRC_NOWJUDGE_1P" | "#SRC_NOWJUDGE_2P" => {
                self.cur = None;
                let side = usize::from(cmd.ends_with("2P"));
                if v[1] > 5 {
                    return;
                }
                if let Some(s) = src(&v, &self.sizes) {
                    let js = &mut self.skin.judges[side];
                    js.judge[(5 - v[1]) as usize] = Some((s, Dst::default()));
                    js.shift = v[11] != 1;
                    if !self
                        .skin
                        .objects
                        .iter()
                        .any(|o| matches!(o.kind, Kind::Judge { side: s } if s == side))
                    {
                        self.skin.objects.push(Obj {
                            kind: Kind::Judge { side },
                            dst: Dst::default(),
                            act: None,
                        });
                    }
                }
            }
            "#DST_NOWJUDGE_1P" | "#DST_NOWJUDGE_2P" => {
                let side = usize::from(cmd.ends_with("2P"));
                if v[1] <= 5 {
                    if let Some((_, d)) = self.skin.judges[side].judge[(5 - v[1]) as usize].as_mut()
                    {
                        add_dst(d, &v);
                    }
                }
            }
            c if c.starts_with("#DST_") => {
                if let Some(i) = self.cur {
                    let o = &mut self.skin.objects[i];
                    if let Kind::Gauge { step, .. } = &o.kind {
                        // the gauge width covers all parts; keep the per-part size
                        let mut v2 = v;
                        if step.0.abs() >= 1.0 {
                            v2[5] = step.0.abs() as i64;
                        }
                        add_dst(&mut o.dst, &v2);
                    } else {
                        add_dst(&mut o.dst, &v);
                    }
                }
            }
            _ => {}
        }
    }
}

// condition stack lives outside `Loader` to keep the borrowck simple
impl Loader<'_> {
    fn conds_top(&self) -> (bool,) {
        (self.conds.iter().any(|c| c.skip),)
    }
    fn parent_skip(&self) -> bool {
        let n = self.conds.len();
        n > 1 && self.conds[..n - 1].iter().any(|c| c.skip)
    }
}

/// Load an LR2 skin. `static_option` answers chart/config dependent `#IF`
/// conditions at load time; `image_size` checks (and measures) image files.
pub fn load(
    path: &Path,
    skins_root: &Path,
    user_options: &HashMap<i32, bool>,
    static_option: &dyn Fn(i32) -> Option<bool>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let bytes = vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = vibebms_core::encoding::decode_text(&bytes);
    let mut l = Loader {
        root: skins_root.to_path_buf(),
        dir: path.parent().unwrap_or(Path::new("")).to_path_buf(),
        options: user_options.clone(),
        static_option,
        files: HashMap::new(),
        skin: Skin {
            path: path.to_path_buf(),
            name: String::new(),
            author: String::new(),
            kind: SkinType::Play7,
            width: 640.0,
            height: 480.0,
            images: Vec::new(),
            objects: Vec::new(),
            lanes: Vec::new(),
            judges: Default::default(),
            bar_lines: Vec::new(),
            songlist: None,
            custom_options: Vec::new(),
            scene_ms: 0,
            input_ms: 0,
            fadeout_ms: 0,
            finish_margin_ms: 0,
            script: None,
            fonts: Vec::new(),
            trans_color: None,
            offsets: HashMap::new(),
        },
        sizes: Vec::new(),
        image_size,
        cur: None,
        lane_kind: 0,
        judge_side: 0,
        pending_line: None,
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
    let _ = (l.lane_kind, l.judge_side);
    // a button inside a panel is shown (and clickable) only while it is open
    for (i, op) in std::mem::take(&mut l.panel_ops) {
        l.skin.objects[i].dst.ops.push(op);
    }
    let known = l.options.clone();
    resolve_static_ops(&mut l.skin, &known);
    if !l.resolution_set {
        let (w, h) = guess_resolution(&l.skin);
        l.skin.width = w;
        l.skin.height = h;
    }
    // text is anchored at x: its left edge, centre or right edge by align
    let anchor = |d: &mut Dst, align: i32| {
        let k = align.clamp(0, 2) as f32 * 0.5;
        for key in &mut d.keys {
            key.rect.x -= key.rect.w * k;
        }
    };
    for o in &mut l.skin.objects {
        if let Kind::Text { align, .. } = o.kind {
            anchor(&mut o.dst, align);
        }
    }
    if let Some((d, align, ..)) = l.skin.songlist.as_mut().and_then(|s| s.text.as_mut()) {
        anchor(d, *align);
    }
    if l.skin.objects.is_empty() {
        return Err(format!("{}: no drawable objects", path.display()));
    }
    Ok(l.skin)
}

/// Screen size of a skin without `#RESOLUTION`: HD-patched LR2 builds run
/// skins made for 1280x720 / 1920x1080 without saying so. The smallest
/// size holding nine in ten on-screen destinations.
fn guess_resolution(skin: &Skin) -> (f32, f32) {
    let mut right = Vec::new();
    let mut bottom = Vec::new();
    for k in skin.objects.iter().flat_map(|o| &o.dst.keys) {
        let r = k.rect;
        // ignore what starts off screen (slide-in animations)
        if r.x >= 0.0 && r.y >= 0.0 && r.w > 0.0 && r.h > 0.0 {
            right.push(r.x + r.w);
            bottom.push(r.y + r.h);
        }
    }
    for l in skin.lanes.iter().filter_map(|l| l.rect) {
        right.push(l.right());
        bottom.push(l.bottom());
    }
    if right.is_empty() {
        return (640.0, 480.0);
    }
    // animations move things off screen: most, not all, must fit
    let p90 = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v[(v.len() - 1) * 9 / 10]
    };
    let (r, b) = (p90(&mut right), p90(&mut bottom));
    [(640.0, 480.0), (1280.0, 720.0), (1920.0, 1080.0)]
        .into_iter()
        .find(|(w, h)| r <= w + 1.0 && b <= h + 1.0)
        .unwrap_or((3840.0, 2160.0))
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
                let p = dir.join(f.get(2).copied().unwrap_or("").replace('\\', "/"));
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
pub fn custom_options(path: &Path) -> Vec<CustomOption> {
    let Ok(b) = vfs::read(path) else {
        return Vec::new();
    };
    vibebms_core::encoding::decode_text(&b)
        .lines()
        .filter(|l| {
            l.trim_start()
                .to_ascii_uppercase()
                .starts_with("#CUSTOMOPTION,")
        })
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            let first: i32 = f.get(2)?.parse().ok()?;
            let labels: Vec<String> = f
                .iter()
                .skip(3)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            Some(CustomOption {
                name: f.get(1)?.to_string(),
                first_id: first,
                ops: (0..labels.len() as i32).map(|i| first + i).collect(),
                labels,
                default: 0,
            })
        })
        .collect()
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
mod tests {
    use super::*;

    #[test]
    fn lr2_fonts() {
        // ASCII, hiragana (two-byte range) and kanji (upper range) codes
        assert_eq!(font_chars(65), vec!['A']);
        assert_eq!(font_chars(0x82a0 - 32832), vec!['あ']);
        assert_eq!(font_chars(0xe040 - 49281), vec!['漾']);
        assert_eq!(font_chars(288).len(), 2);
        let dir = std::env::temp_dir().join(format!("vibebms-lr2font-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("f")).unwrap();
        std::fs::write(
            dir.join("f/a.lr2font"),
            "#S,20,\n#M,!2,\n#T,0,t.png\n#T,1,missing.png\n#R,65,0,0,0,10,20\n#R,66,1,0,0,10,20\n",
        )
        .unwrap();
        let csv = "#LR2FONT,CONTINUE\n#LR2FONT,f/a.lr2font\n#IMAGE,x.png\n\
#SRC_TEXT,0,1,10,1\n#DST_TEXT,0,0,100,50,80,20,0,255,255,255,255\n";
        std::fs::write(dir.join("s.lr2skin"), csv).unwrap();
        let size = |p: &Path| (!p.ends_with("missing.png")).then_some((64, 64));
        let s = load(
            &dir.join("s.lr2skin"),
            &dir,
            &HashMap::new(),
            &|_| None,
            &size,
        )
        .unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(s.fonts.len(), 1);
        let f = &s.fonts[0];
        assert_eq!((f.size, f.margin, f.glyphs.len()), (20.0, -2.0, 1));
        assert!(s.images[f.glyphs[&'A'].0]
            .as_ref()
            .unwrap()
            .ends_with("f/t.png"));
        // font 1 (0 is a placeholder); centred text is anchored at x
        assert!(matches!(
            s.objects[0].kind,
            Kind::Text {
                font: Some(0),
                align: 1,
                ..
            }
        ));
        assert_eq!(s.objects[0].dst.keys[0].rect.x, 60.0);
    }

    #[test]
    fn parses_select_bars() {
        let dir = std::env::temp_dir().join(format!("vibebms-sel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let csv = "#INFORMATION,5,Sel,me\n#IMAGE,a.png\n\
#SRC_BAR_BODY,0,0,0,0,100,20,1,1,0,0\n#SRC_BAR_BODY,1,0,0,20,100,20,1,1,0,0\n\
#DST_BAR_BODY_OFF,0,0,10,10,100,20,0,255,255,255,255\n#DST_BAR_BODY_OFF,1,0,10,30,100,20,0,255,255,255,255\n\
#DST_BAR_BODY_ON,1,0,0,30,110,20,0,255,255,255,255\n#BAR_CENTER,1\n#BAR_AVAILABLE,0,1\n\
#DST_BAR_TITLE,0,0,20,2,70,16,0,255,255,255,255\n\
#SRC_BAR_LEVEL,2,0,0,40,100,10,10,1,0,0,0,0,2\n#DST_BAR_LEVEL,2,0,80,4,8,10,0,255,255,255,255\n\
#SRC_BAR_LAMP,3,0,0,50,4,20,1,1,0,0\n#DST_BAR_LAMP,3,0,2,0,4,20,0,255,255,255,255\n";
        std::fs::write(dir.join("select.lr2skin"), csv).unwrap();
        let size = |_: &Path| Some((128, 64));
        let s = load(
            &dir.join("select.lr2skin"),
            &dir,
            &HashMap::new(),
            &|_| None,
            &size,
        )
        .unwrap();
        assert_eq!(s.kind, SkinType::Select);
        assert!(matches!(s.objects[0].kind, Kind::SongList));
        let sl = s.songlist.unwrap();
        assert_eq!((sl.center, sl.clickable.clone()), (1, vec![0, 1]));
        assert_eq!(sl.off.len(), 2);
        assert!(sl.on[1].is_some() && sl.on[0].is_none());
        assert!(sl.body[BAR_SONG].is_some() && sl.body[BAR_FOLDER].is_some());
        assert!(sl.text.is_some());
        let (font, _) = sl.level[2].as_ref().unwrap();
        assert_eq!((font.digits, font.keta), (10, 2));
        // LR2 lamp 3 (clear) is beatoraja's NORMAL (5)
        assert!(sl.lamp[5].is_some());
    }

    #[test]
    fn parses_buttons() {
        let dir = std::env::temp_dir().join(format!("vibebms-btn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let csv = "#INFORMATION,5,Btn,me\n#IMAGE,a.png\n\
#SRC_BUTTON,0,0,0,0,100,40,1,4,0,0,42,0,1\n#DST_BUTTON,0,0,10,10,100,10,0,255,255,255,255,0,0,0,0,0,0,32\n\
#SRC_BUTTON,0,0,0,40,30,10,1,1,0,0,15,0,0\n#DST_BUTTON,0,0,10,30,30,10,0,255,255,255,255\n";
        std::fs::write(dir.join("b.lr2skin"), csv).unwrap();
        let size = |_: &Path| Some((128, 64));
        let s = load(
            &dir.join("b.lr2skin"),
            &dir,
            &HashMap::new(),
            &|_| None,
            &size,
        )
        .unwrap();
        let Kind::ImageSet { srcs, id } = &s.objects[0].kind else {
            panic!()
        };
        // one frame per button value, fired as button 42 (RANDOM)
        assert_eq!((srcs.len(), *id), (4, 42));
        assert_eq!(s.objects[0].act, Some((42, 0)));
        // panel 1 adds option 21 to the destination's own conditions
        assert_eq!(s.objects[0].dst.ops, vec![32, 21]);
        assert_eq!(s.objects[1].act, Some((15, 0)));
        assert!(s.objects[1].dst.ops.is_empty());
    }

    #[test]
    fn parses_play_skin() {
        let dir = std::env::temp_dir().join(format!("vibebms-skin-{}", std::process::id()));
        let theme = dir.join("MySkin");
        std::fs::create_dir_all(&theme).unwrap();
        let csv = "#INFORMATION,0,Test,me\n#CUSTOMOPTION,Lane,900,Dark,Light\n#IMAGE,LR2files\\Theme\\MySkin\\a.png\n#IMAGE,missing.png\n\
#IF,901\n#SRC_IMAGE,0,0,0,0,10,10,1,1,0,0\n#DST_IMAGE,0,0,0,0,10,10,0,255,255,255,255\n#ELSE\n#SRC_IMAGE,0,0,0,0,20,20,2,1,100,0\n#DST_IMAGE,0,0,5,5,20,20,0,255,255,255,255,0,0,0,0,-1,41,32\n#DST_IMAGE,0,1000,50,5,20,20\n#ENDIF\n\
#SRC_NUMBER,0,0,0,0,110,10,11,1,0,0,101,0,6\n#DST_NUMBER,0,0,100,100,8,10,0,255,255,255,255\n\
#SRC_NOTE,1,0,0,0,10,4,1,1,0,0\n#SRC_NOTE,0,0,0,0,20,4,1,1,0,0\n#DST_NOTE,0,0,30,0,20,400,0,255,255,255,255\n#DST_NOTE,1,0,50,0,10,400,0,255,255,255,255\n\
#SRC_NOWJUDGE_1P,5,0,0,0,40,10,1,1,0,0\n#DST_NOWJUDGE_1P,5,0,60,200,40,10,0,255,255,255,255,0,0,0,0,-1,46\n";
        std::fs::write(theme.join("play.lr2skin"), csv).unwrap();
        let size = |p: &Path| {
            p.file_name()
                .map(|n| n == "a.png")
                .unwrap_or(false)
                .then_some((128, 64))
        };
        let s = load(
            &theme.join("play.lr2skin"),
            &dir,
            &HashMap::new(),
            &|_| None,
            &size,
        )
        .unwrap();
        assert_eq!(s.name, "Test");
        assert_eq!(s.kind, SkinType::Play7);
        assert_eq!(s.images.len(), 2);
        assert!(s.images[1].is_none());
        // option 900 (Dark) is the default, so the #ELSE branch is used
        let Kind::Image(src) = &s.objects[0].kind else {
            panic!()
        };
        assert_eq!(src.frames.len(), 2);
        assert_eq!(s.objects[0].dst.keys.len(), 2);
        assert_eq!(s.objects[0].dst.timer, 41);
        assert_eq!(s.objects[0].dst.ops, vec![32]);
        assert!(matches!(
            s.objects[1].kind,
            Kind::Number {
                digits: 11,
                id: 101,
                keta: 6,
                ..
            }
        ));
        assert_eq!(s.lanes[7].rect.unwrap().w, 20.0); // scratch
        assert!(s.lanes[0].note.is_some());
        assert!(s.objects.iter().any(|o| matches!(o.kind, Kind::Notes)));
        assert!(s.judges[0].judge[0].is_some()); // LR2 index 5 = PGREAT
        std::fs::remove_dir_all(&dir).ok();
    }
}
