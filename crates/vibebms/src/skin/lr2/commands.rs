//! The commands of an LR2 skin, by what they make: header and
//! resources, objects, the music select bar list, charts, and the play
//! field (lanes, notes, judgements); `#DST_*` lines of other objects last.

use super::*;
use vibebms_core::judge::Judge;

/// A command line: its name (upper case), fields and their numbers, and
/// its first field as text.
#[derive(Clone, Copy)]
pub(super) struct Line<'a> {
    pub cmd: &'a str,
    pub f: &'a [&'a str],
    pub v: [i64; 22],
    pub s1: &'a str,
}

pub(super) type LaneImage = fn(&mut LaneSkin) -> &mut Option<Src>;

/// The note images of a lane (`#SRC_NOTE,…,lane,…`): where each command
/// puts its image (the first one given counts).
const LANE_IMAGES: [(&str, LaneImage); 15] = [
    ("#SRC_NOTE", |l| &mut l.note),
    ("#SRC_LN_START", |l| &mut l.ln_start),
    ("#SRC_LN_END", |l| &mut l.ln_end),
    ("#SRC_LN_BODY", |l| &mut l.ln_body),
    ("#SRC_LN_BODY_INACTIVE", |l| &mut l.ln_body),
    ("#SRC_LN_BODY_ACTIVE", |l| &mut l.ln_body_active),
    ("#SRC_MINE", |l| &mut l.mine),
    ("#SRC_HCN_START", |l| &mut l.hcn_start),
    ("#SRC_HCN_END", |l| &mut l.hcn_end),
    ("#SRC_HCN_BODY", |l| &mut l.hcn_body),
    ("#SRC_HCN_BODY", |l| &mut l.hcn_body_active),
    ("#SRC_HCN_BODY_INACTIVE", |l| &mut l.hcn_body),
    ("#SRC_HCN_BODY_ACTIVE", |l| &mut l.hcn_body_active),
    ("#SRC_HCN_DAMAGE", |l| &mut l.hcn_damage),
    ("#SRC_HCN_REACTIVE", |l| &mut l.hcn_reactive),
];

impl Loader<'_> {
    /// One command (`#IF` levels are dealt with before).
    pub(super) fn command(&mut self, l: Line) {
        let done = self.header_cmd(l)
            || self.object_cmd(l)
            || self.bar_cmd(l)
            || self.chart_cmd(l)
            || self.field_cmd(l);
        if !done {
            self.other_cmd(l);
        }
    }

    /// Header, options, includes and the image and font lists.
    fn header_cmd(&mut self, l: Line) -> bool {
        let Line { cmd, f, v, s1 } = l;
        match cmd {
            "#INFORMATION" => {
                self.skin.kind = SkinType::from_id(v[1] as i32);
                if self.skin.kind == SkinType::SkinSelect {
                    self.skin.skin_select = Some(Default::default());
                }
                self.skin.name = f.get(2).unwrap_or(&"").trim().to_string();
                self.skin.author = f.get(3).unwrap_or(&"").trim().to_string();
            }
            // a header command: LR2 builds, beatoraja and Lunatic Vibes
            // read it from the skin file itself, not from what it includes
            "#RESOLUTION" if self.depth == 0 => {
                let sizes = [
                    (640.0, 480.0),
                    (1280.0, 720.0),
                    (1920.0, 1080.0),
                    (3840.0, 2160.0),
                ];
                // `#RESOLUTION,<width>,<height>` (as bmz-player takes it)
                let size = match (v[1], v[2]) {
                    (w, h) if w > 3 && h > 3 => Some((w as f32, h as f32)),
                    _ => s1.parse::<usize>().ok().and_then(|i| sizes.get(i)).copied(),
                };
                if let Some((w, h)) = size {
                    self.skin.width = w;
                    self.skin.height = h;
                    self.resolution_set = true;
                }
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
                    let def = f.get(3).map(|s| s.trim().to_string()).unwrap_or_default();
                    let want = self.user_files.get(s1).cloned().unwrap_or(def);
                    self.files.insert(p.trim().replace('\\', "/"), want);
                }
            }
            "#SETOPTION" => {
                self.options.insert(v[1] as i32, v[2] >= 1);
            }
            "#INCLUDE" => {
                if self.depth < 8 {
                    let p = self.path(s1);
                    if let Some(b) = self.includes.read(&p) {
                        self.depth += 1;
                        for l in vibebms_core::encoding::decode_text(&b).lines() {
                            self.line(l);
                        }
                        self.depth -= 1;
                    }
                }
            }
            // the image the previous screen's skin had at this index (a
            // result skin going on with the play skin's background)
            "#IMAGE" if s1.eq_ignore_ascii_case("CONTINUE") => {
                let i = self.images.len();
                let p = LAST_IMAGES
                    .lock()
                    .ok()
                    .and_then(|l| l.get(i).cloned().flatten());
                self.images.push(p);
            }
            "#IMAGE" => {
                let p = self.path(s1);
                self.images.push(Some(p));
            }
            "#TRANSCOLOR" => {
                let c = |n: i64| n.clamp(0, 255) as u8;
                self.skin.trans_color = Some([c(v[1]), c(v[2]), c(v[3])]);
            }
            "#STARTINPUT" => self.skin.input_ms = v[1],
            "#SCENETIME" => self.skin.scene_ms = v[1],
            "#FADEOUT" => self.skin.fadeout_ms = v[1],
            "#FINISHMARGIN" => self.skin.finish_margin_ms = v[1],
            // beatoraja: a list of images, and an object showing the one a
            // skin value picks (`#SRC_IMAGESET,cycle,timer,value,n,set...`)
            "#IMAGESET" => self.imagesets.push(src(&v, &self.images)),
            "#LR2FONT" => {
                // a missing font keeps its index (`CONTINUE` placeholders)
                let p = self.path(s1);
                let font = load_font(&p, &self.root, self.images.measure).map(|(f, imgs)| {
                    let base = self.images.len();
                    for (path, size) in imgs {
                        self.images.push_measured(path, size);
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
            "#STRETCH" => self.stretch = v[1] as i32,
            // a horizontal play skin: `#HORIZONTAL` (`#HORIZONTAL,0` is off)
            "#HORIZONTAL" => self.skin.horizontal = s1.is_empty() || v[1] != 0,
            "#PLAYSTART" => self.skin.play_start_ms = Some(v[1].clamp(0, 10_000)),
            "#CLOSE" => self.skin.close_ms = Some(v[1].clamp(0, 10_000)),
            "#JUDGETIMER" => self.skin.judge_timer = Judge::of(v[1].clamp(0, 3) as usize),
            "#CUSTOMOFFSET" => {
                if let Ok(id) = f.get(2).unwrap_or(&"").trim().parse::<i32>() {
                    let v = self.user_offsets.get(s1).copied().unwrap_or_default();
                    self.skin.offsets.insert(id, v.map(|x| x as f32));
                }
            }
            _ => return false,
        }
        true
    }

    /// Images, numbers, texts, bars, sliders, gauges and covers.
    fn object_cmd(&mut self, l: Line) -> bool {
        let Line { cmd, f, v, .. } = l;
        match cmd {
            "#SRC_IMAGE" => {
                self.cur = None;
                if v[2] >= 100 {
                    self.push(Kind::RefImage(v[2] as i32));
                } else if let Some(s) = src(&v, &self.images) {
                    self.push(Kind::Image(s));
                }
            }
            "#SRC_IMAGESET" => {
                self.cur = None;
                let n = v[4].clamp(0, 16) as usize;
                let mut srcs: Vec<Src> = (0..n)
                    .map_while(|i| {
                        let k = usize::try_from(*v.get(5 + i)?).ok()?;
                        self.imagesets.get(k).cloned().flatten()
                    })
                    .collect();
                for s in &mut srcs {
                    s.cycle = v[1];
                    s.timer = v[2] as i32;
                }
                if !srcs.is_empty() {
                    self.push(Kind::ImageSet {
                        srcs,
                        id: v[3] as i32,
                    });
                }
            }
            // an image shown while the mouse is over part of it:
            // ...,panel,x2,y2,w2,h2 (relative to the destination)
            "#SRC_ONMOUSE" => {
                self.cur = None;
                if let Some(s_) = src(&v, &self.images) {
                    self.push(Kind::Image(s_));
                    let i = self.skin.objects.len() - 1;
                    self.skin.objects[i].dst.hover = Some(Rect::new(
                        v[12] as f32,
                        v[13] as f32,
                        v[14] as f32,
                        v[15] as f32,
                    ));
                    if v[11] > 0 {
                        self.panel_ops.push((i, 20 + v[11] as i32));
                    }
                }
            }
            "#SRC_BUTTON" => {
                // gr,x,y,w,h,divx,divy,cycle,timer,type,click,panel: one frame
                // per button value, clicking fires the button type
                self.cur = None;
                let Some(s_) = src(&v, &self.images) else {
                    return true;
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
                // as beatoraja reads LR2 buttons: plus-only > 0 adds,
                // < 0 subtracts, 0 splits the button left / right
                let click = match v[14] {
                    n if n > 0 => 0,
                    n if n < 0 => 1,
                    _ => 2,
                };
                self.skin.objects[i].act = Some((id, click));
                if v[13] > 0 {
                    self.panel_ops.push((i, 20 + v[13] as i32));
                }
            }
            "#SRC_NUMBER" | "#SRC_NOWCOMBO_1P" | "#SRC_NOWCOMBO_2P" | "#SRC_NOWCOMBO_3P" => {
                self.cur = None;
                let Some(mut s) = src(&v, &self.images) else {
                    return true;
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
                    return true;
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
                    let side = judge_side(cmd);
                    let Some(j) = judge_slot(v[1]) else {
                        return true;
                    };
                    let keta = v[13].clamp(1, 32) as usize;
                    self.skin.judges[side].combo[j] = Some((s, Dst::default(), keta));
                }
            }
            "#DST_NOWCOMBO_1P" | "#DST_NOWCOMBO_2P" | "#DST_NOWCOMBO_3P" => {
                let side = judge_side(cmd);
                if let Some((_, d, _)) =
                    judge_slot(v[1]).and_then(|j| self.skin.judges[side].combo[j].as_mut())
                {
                    add_dst(d, &v);
                }
            }
            "#SRC_TEXT" => {
                let font = self.font(v[2]);
                self.push(Kind::Text {
                    id: v[3] as i32,
                    font,
                    align: v[4] as i32,
                    overflow: 0,
                    constant: None,
                    face: None,
                    fx: TextFx::default(),
                    size: None,
                })
            }
            "#SRC_BARGRAPH" => {
                self.cur = None;
                if let Some(s) = src(&v, &self.images) {
                    self.push(Kind::Bar {
                        src: s,
                        id: v[11] as i32 + 100,
                        dir: v[12] as i32,
                        minmax: None,
                    });
                }
            }
            "#SRC_SLIDER_REFNUMBER" => {
                // ...,muki,range,number,disable,min,max
                self.cur = None;
                if let Some(s) = src(&v, &self.images) {
                    self.push(Kind::Slider {
                        src: s,
                        id: v[13] as i32,
                        dir: v[11] as i32,
                        range: v[12] as f32,
                        minmax: Some((v[15] as f32, v[16] as f32)),
                    });
                }
            }
            "#SRC_BARGRAPH_REFNUMBER" => {
                // ...,number,muki,min,max
                self.cur = None;
                if let Some(s) = src(&v, &self.images) {
                    self.push(Kind::Bar {
                        src: s,
                        id: v[11] as i32,
                        dir: v[12] as i32,
                        minmax: Some((v[13] as f32, v[14] as f32)),
                    });
                }
            }
            "#SRC_SLIDER" => {
                self.cur = None;
                if let Some(s) = src(&v, &self.images) {
                    self.push(Kind::Slider {
                        src: s,
                        id: v[13] as i32,
                        dir: v[11] as i32,
                        range: v[12] as f32,
                        minmax: None,
                    });
                }
            }
            // _EX: beatoraja's 8 images (EX gauges second), same layout
            "#SRC_GROOVEGAUGE" | "#SRC_GROOVEGAUGE_EX" => {
                self.cur = None;
                if let Some(s) = src(&v, &self.images) {
                    let parts = if v[13] > 0 {
                        gauge_parts(v[13])
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
                        side: v[1].clamp(0, 1) as usize,
                    });
                }
            }
            "#SRC_BGA" => self.push(Kind::Bga),
            "#SRC_JUDGELINE" => {
                self.cur = None;
                if let Some(s) = src(&v, &self.images) {
                    self.push(Kind::JudgeLine(s));
                }
            }
            // beatoraja: the hidden cover and the LIFT cover, drawn above a
            // disappear line (,..,line,line follows LIFT)
            "#SRC_HIDDEN" | "#SRC_LIFT" => {
                self.cur = None;
                if let Some(src) = src(&v, &self.images) {
                    let given = |i: usize| f.get(i).is_some_and(|x| !x.trim().is_empty());
                    self.push(Kind::Cover {
                        src,
                        line: (given(11) && v[11] > 0).then_some(v[11] as f32),
                        link_lift: !given(12) || v[12] != 0,
                    });
                }
            }
            _ => return false,
        }
        true
    }

    /// The music select bar list.
    fn bar_cmd(&mut self, l: Line) -> bool {
        let Line { cmd, v, .. } = l;
        match cmd {
            // music select bar list
            "#BAR_CENTER" => self.songlist().center = bar_slot(v[1]),
            "#BAR_AVAILABLE" => {
                let (a, b) = (bar_slot(v[1]), bar_slot(v[2]));
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
                    return true;
                };
                let sz = src(&v, &self.images);
                let sl = self.songlist();
                if sl.body.len() <= t {
                    sl.body.resize(t + 1, None);
                }
                sl.body[t] = sz;
            }
            "#DST_BAR_BODY_OFF" | "#DST_BAR_BODY_ON" => {
                let slot = bar_slot(v[1]);
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
                let Some(s_) = src(&v, &self.images) else {
                    return true;
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
            "#SRC_BAR_LAMP" | "#SRC_BAR_MY_LAMP" | "#SRC_BAR_RIVAL_LAMP" => {
                let Some(k) = LR2_LAMPS.get(v[1].max(0) as usize).copied() else {
                    return true;
                };
                let s_ = src(&v, &self.images);
                let lamps = bar_lamps(self.songlist(), cmd);
                if lamps.len() <= k {
                    lamps.resize(k + 1, None);
                }
                lamps[k] = s_.map(|s_| (s_, Dst::default()));
            }
            "#DST_BAR_LAMP" | "#DST_BAR_MY_LAMP" | "#DST_BAR_RIVAL_LAMP" => {
                let Some(k) = LR2_LAMPS.get(v[1].max(0) as usize).copied() else {
                    return true;
                };
                if let Some(Some((_, d))) = bar_lamps(self.songlist(), cmd).get_mut(k) {
                    add_dst(d, &v);
                }
            }
            // labels (0 LN, 1 #RANDOM, 2 mines, 3 CN, 4 HCN) and a class
            // course's trophy (0 bronze, 1 silver, 2 gold)
            "#SRC_BAR_LABEL" | "#SRC_BAR_TROPHY" => {
                let k = v[1];
                let s_ = src(&v, &self.images);
                let (set, n) = bar_marks(self.songlist(), cmd);
                if let Some(k) = usize::try_from(k).ok().filter(|k| *k < n) {
                    if set.len() <= k {
                        set.resize(k + 1, None);
                    }
                    set[k] = s_.map(|s_| (s_, Dst::default()));
                }
            }
            "#DST_BAR_LABEL" | "#DST_BAR_TROPHY" => {
                let k = usize::try_from(v[1]).unwrap_or(usize::MAX);
                if let Some(Some((_, d))) = bar_marks(self.songlist(), cmd).0.get_mut(k) {
                    add_dst(d, &v);
                }
            }
            // a folder's lamp distribution (type 0: 11 cells a frame, lamps
            // from the best on the left); type 1, the DJ levels, is not
            // drawn
            "#SRC_BAR_GRAPH" => {
                if v[1] == 0 {
                    let s_ = src(&v, &self.images);
                    let sl = self.songlist();
                    let g = sl
                        .graph
                        .get_or_insert_with(|| (Dst::default(), None, false));
                    g.1 = s_;
                } else {
                    log::debug!("lr2 skin: #SRC_BAR_GRAPH type {} is not supported", v[1]);
                }
            }
            "#DST_BAR_GRAPH" => {
                if v[1] == 0 {
                    if let Some((d, ..)) = self.songlist().graph.as_mut() {
                        add_dst(d, &v);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// Charts (beatoraja's look and geometry; the DST point size and
    /// colour of LR2 are not used).
    fn chart_cmd(&mut self, l: Line) -> bool {
        let Line { cmd, f, v, .. } = l;
        match cmd {
            "#SRC_GAUGECHART_1P" | "#SRC_GAUGECHART" => {
                // a second index only recolours LR2's points: one chart
                self.cur = None;
                if v[1] == 0 {
                    self.push(Kind::Graph(Graph::Gauge(GaugeGraph {
                        colors: Some(crate::skin::beatoraja::gauge_colors(
                            &serde_json::Value::Null,
                        )),
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
                    built_in: false,
                })));
                self.chart_field = Some((v[11] as f32, v[12] as f32));
            }
            "#SRC_BPMCHART" => {
                self.push(Kind::Graph(Graph::Bpm(BpmGraph {
                    colors: colors(
                        f,
                        5,
                        &["00ff00", "0000ff", "ff0000", "ffff00", "ff00ff", "7f7f7f"],
                    ),
                    line_width: v[4].max(1) as f32,
                    delay: v[3],
                })));
                self.chart_field = Some((v[1] as f32, v[2] as f32));
            }
            // beatoraja timing visualizer: w,h,judge width (ms),line width,
            // line / centre / PG..PR colours,transparent POOR,decay
            "#SRC_TIMING_1P" => {
                self.push(Kind::Graph(Graph::Visualizer(VisualizerGraph {
                    hit_error: false,
                    range_ms: visualizer_range(v[6]),
                    colors: colors(
                        f,
                        8,
                        &[
                            "00FF00FF", "FFFFFFFF", "000088FF", "008800FF", "888800FF", "880000FF",
                            "000000FF",
                        ],
                    ),
                    line_width: v[7].clamp(1, 4) as f32,
                    transparent_poor: v[15] == 1,
                    decay: v[16] == 1,
                    by_judge: false,
                    ema: false,
                })));
                self.chart_field = Some((v[4] as f32, v[5] as f32));
            }
            "#SRC_TIMINGCHART_1P" => {
                self.push(Kind::Graph(Graph::Timing(TimingGraph {
                    bins: timing_bins(v[4]),
                    colors: Some(colors(
                        f,
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
            | "#DST_TIMINGCHART_1P"
            | "#DST_TIMING_1P" => {
                if let (Some(i), Some((w, h))) = (self.cur, self.chart_field) {
                    let mut v2 = v;
                    v2[4] -= h as i64;
                    v2[5] = w as i64;
                    v2[6] = h as i64;
                    v2[9..=11].fill(255);
                    let d = &mut self.skin.objects[i].dst;
                    if d.keys.is_empty() {
                        d.offsets.extend(line_offsets(f));
                    }
                    add_dst(d, &v2);
                }
            }
            "#SRC_SCORECHART" => self.push(Kind::Graph(Graph::Notes(NotesGraph {
                kind: 1,
                ..Default::default()
            }))),
            _ => return false,
        }
        true
    }

    /// The play field: lanes, notes, bar lines and judgements.
    fn field_cmd(&mut self, l: Line) -> bool {
        let Line { cmd, v, .. } = l;
        let lane_images = LANE_IMAGES.iter().filter(|(n, _)| *n == cmd);
        let mut found = false;
        for (_, slot) in lane_images {
            self.lane_src(&v, *slot);
            found = true;
        }
        if found {
            return true;
        }
        match cmd {
            "#SRC_LINE" => self.pending_line = src(&v, &self.images),
            "#DST_LINE" => {
                if let Some(s) = self.pending_line.take() {
                    let mut d = Dst::default();
                    add_dst(&mut d, &v);
                    self.skin.bar_lines.push((s, d));
                } else if let Some((_, d)) = self.skin.bar_lines.last_mut() {
                    add_dst(d, &v);
                }
            }
            "#DST_NOTE2" => self.skin.note_fall_y = Some(v[1] as f32),
            "#DST_NOTE_EXPANSION_RATE" => {
                self.skin.note_expansion =
                    Some((v[1] as f32, v[2] as f32)).filter(|&(w, h)| w != 100.0 || h != 100.0)
            }
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
            "#SRC_NOWJUDGE_1P" | "#SRC_NOWJUDGE_2P" | "#SRC_NOWJUDGE_3P" => {
                self.cur = None;
                let side = judge_side(cmd);
                let Some(j) = judge_slot(v[1]) else {
                    return true;
                };
                if let Some(s) = src(&v, &self.images) {
                    let js = &mut self.skin.judges[side];
                    js.judge[j] = Some((s, Dst::default()));
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
            "#DST_NOWJUDGE_1P" | "#DST_NOWJUDGE_2P" | "#DST_NOWJUDGE_3P" => {
                let side = judge_side(cmd);
                if let Some((_, d)) =
                    judge_slot(v[1]).and_then(|j| self.skin.judges[side].judge[j].as_mut())
                {
                    add_dst(d, &v);
                }
            }
            _ => return false,
        }
        true
    }

    /// `#DST_*` of the object made last, and what is not supported.
    fn other_cmd(&mut self, l: Line) {
        let Line { cmd, f, v, .. } = l;
        match cmd {
            c if c.starts_with("#DST_") => {
                if let Some(i) = self.cur {
                    let first = self.skin.objects[i].dst.keys.is_empty();
                    if first {
                        // beatoraja offsets: built-in ones, then 21.. of the line
                        let d = &mut self.skin.objects[i].dst;
                        d.stretch = self.stretch;
                        match c {
                            "#DST_HIDDEN" => d.offsets.extend([OFFSET_LIFT, OFFSET_HIDDEN_COVER]),
                            "#DST_LIFT" => d.offsets.push(OFFSET_LIFT),
                            _ => {}
                        }
                        d.offsets.extend(line_offsets(f));
                    }
                    let o = &mut self.skin.objects[i];
                    if let Kind::Gauge { step, .. } = &o.kind {
                        // the gauge width covers all parts; keep the per-part size
                        let mut v2 = v;
                        if step.0.abs() >= 1.0 {
                            v2[5] = step.0.abs() as i64;
                        }
                        add_dst(&mut o.dst, &v2);
                    } else {
                        // images are mirrored by a negative size
                        let mirror = matches!(o.kind, Kind::Image(_) | Kind::ImageSet { .. });
                        add_dst_signed(&mut o.dst, &v, mirror);
                    }
                }
            }
            // an object vibeBMS does not draw (#SRC_ONMOUSE hover images,
            // ...): its #DST lines must not move the object before it
            c if c.starts_with("#SRC_") => {
                log::debug!("lr2 skin: {c} is not supported");
                self.cur = None
            }
            c => log::debug!("lr2 skin: {c} is not supported"),
        }
    }
}

/// LR2's bar lamps (no play, failed, easy, clear, hard, full combo, ...)
/// as clear types.
const LR2_LAMPS: [usize; 8] = [0, 1, 4, 5, 6, 8, 9, 10];

/// The bar labels or trophies a `#SRC_BAR_LABEL` / `_TROPHY` command
/// sets, and how many there are.
fn bar_marks<'a>(sl: &'a mut SongListSkin, cmd: &str) -> (&'a mut Vec<Option<(Src, Dst)>>, usize) {
    if cmd.ends_with("_TROPHY") {
        (&mut sl.trophy, 3)
    } else {
        (&mut sl.label, BAR_LABELS)
    }
}

/// The bar lamps a `#SRC_BAR_*LAMP` / `#DST_BAR_*LAMP` command sets.
fn bar_lamps<'a>(sl: &'a mut SongListSkin, cmd: &str) -> &'a mut Vec<Option<(Src, Dst)>> {
    if cmd.ends_with("_MY_LAMP") {
        &mut sl.my_lamp
    } else if cmd.ends_with("_RIVAL_LAMP") {
        &mut sl.rival_lamp
    } else {
        &mut sl.lamp
    }
}
