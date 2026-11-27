//! The practice screen: a chart's section chosen over a graph of its
//! notes, with the speed, lead-in, gauge, `#RANK`, TOTAL, lane options
//! and LOOP; the play from there (`play::PracticeRun`), and when it comes
//! back the last attempt's result. Nothing played from here is recorded.

use crate::config::{PlayConfig, PracticeConfig};
use crate::ctx::Ctx;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{with_alpha, Draw, Gfx, Rect};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase, Repeat};
use crate::library::Song;
use crate::platform::vfs::Prefetch;
use crate::platform::{Duration, Instant, Job};
use crate::scene::graphs::{self, Area};
use crate::scene::play::{PlayRequest, PlayScene, PracticeResult, PracticeRun};
use crate::scene::{cycle, Next, Scene};
use crate::skin::TimingGraph;
use crate::sysound::Sound;
use crate::theme;
use crate::ui::{self, HitMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use vibebms_core::chart::{Chart, JudgeRank};
use vibebms_core::judge::Judge;
use vibebms_core::lane_option::LaneOption;
use vibebms_core::practice::{self, Practice, LEAD_IN_MAX, SPEED_MAX, SPEED_MIN, SPEED_STEP};
use vibebms_core::LnPref;
use winit::keyboard::KeyCode;

/// A held ← → steps again after this, then every `REPEAT_EVERY`.
const REPEAT_DELAY: Duration = Duration::from_millis(300);
const REPEAT_EVERY: Duration = Duration::from_millis(60);
/// TOTAL in place of the chart's: its range and step.
const TOTAL_MIN: u32 = 50;
const TOTAL_MAX: u32 = 2000;
const TOTAL_STEP: u32 = 10;
const RANKS: [&str; 5] = ["VERY HARD", "HARD", "NORMAL", "EASY", "VERY EASY"];

/// The rows, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    From,
    To,
    LeadIn,
    Speed,
    Gauge,
    GaugeStart,
    Rank,
    Total,
    Random,
    Random2,
    LnMode,
    Loop,
    Start,
}

const ROWS: [Row; 13] = [
    Row::From,
    Row::To,
    Row::LeadIn,
    Row::Speed,
    Row::Gauge,
    Row::GaugeStart,
    Row::Rank,
    Row::Total,
    Row::Random,
    Row::Random2,
    Row::LnMode,
    Row::Loop,
    Row::Start,
];

impl Row {
    fn name(self) -> &'static str {
        match self {
            Row::From => tr!("SECTION START"),
            Row::To => tr!("SECTION END"),
            Row::LeadIn => tr!("LEAD-IN"),
            Row::Speed => tr!("SPEED"),
            Row::Gauge => "GAUGE",
            Row::GaugeStart => tr!("GAUGE START"),
            Row::Rank => tr!("JUDGE RANK"),
            Row::Total => "TOTAL",
            Row::Random => "RANDOM",
            Row::Random2 => "RANDOM 2P",
            Row::LnMode => "LN MODE",
            Row::Loop => tr!("LOOP"),
            Row::Start => "START",
        }
    }
}

/// The chart as read for the screen: its measures and their notes.
struct Read {
    chart: Chart,
    bars: Vec<i64>,
    density: Vec<u32>,
}

impl Read {
    fn measures(&self) -> usize {
        self.bars.len() - 1
    }
}

/// What a tap reaches.
#[derive(Clone, Copy, Debug)]
enum Hit {
    /// A row; its value (its halves step it back and on).
    Row(usize),
    Value(usize),
    Graph,
}

pub struct PracticeScene {
    song: Song,
    /// Every attempt's `#RANDOM` and lane options are rolled from it.
    seed: i64,
    prefetch: Option<Prefetch>,
    job: Option<Job<Result<Read, String>>>,
    read: Option<Read>,
    from: usize,
    to: usize,
    sel: usize,
    /// ← → (or a controller's keys) held: the step and its repeat.
    held: Option<Repeat<i32>>,
    result: Arc<Mutex<Option<PracticeResult>>>,
    hits: HitMap<Hit>,
    graph: Rect,
    back: Rect,
}

impl PracticeScene {
    pub fn new(ctx: &Ctx, song: Song) -> PracticeScene {
        let [from, to] = ctx
            .config
            .practice
            .sections
            .get(&song.sha256)
            .copied()
            .unwrap_or([0, usize::MAX]);
        PracticeScene {
            prefetch: Some(Prefetch::files(vec![PathBuf::from(&song.path)])),
            song,
            seed: crate::platform::unix_ms(),
            job: None,
            read: None,
            from,
            to,
            sel: 0,
            held: None,
            result: Arc::new(Mutex::new(None)),
            hits: HitMap::default(),
            graph: Rect::default(),
            back: Rect::default(),
        }
    }

    /// The chart read (with the play's `#RANDOM` seed and options), off
    /// the main thread once its file is there.
    fn poll(&mut self, ctx: &mut Ctx) -> Option<Next> {
        if self.prefetch.take_if(|p| p.finished()).is_some() {
            let path = PathBuf::from(&self.song.path);
            let (rules, seed) = (ctx.config.play.rules.clone(), self.seed);
            self.job = Some(Job::spawn("practice", move |_| {
                let bytes = crate::platform::vfs::read(&path).map_err(|e| e.to_string())?;
                let chart =
                    vibebms_core::play::build_chart(&path, &bytes, &rules, seed, None)?.chart;
                let bars = practice::measures(&chart);
                let density = practice::density(&chart, &bars);
                Ok(Read {
                    chart,
                    bars,
                    density,
                })
            }));
        }
        match self.job.as_mut()?.poll()? {
            Ok(r) => {
                let last = r.measures() - 1;
                self.to = self.to.min(last);
                self.from = self.from.min(self.to);
                self.read = Some(r);
                self.job = None;
                None
            }
            Err(e) => {
                ctx.toast(tr!("Cannot load chart: {e}", e = e));
                Some(Next::Pop)
            }
        }
    }

    fn rows(&self) -> Vec<Row> {
        let double = self
            .read
            .as_ref()
            .is_some_and(|r| r.chart.mode().is_double());
        ROWS.into_iter()
            .filter(|r| *r != Row::Random2 || double)
            .collect()
    }

    /// The practice play the settings `c` make.
    fn run(&self, c: &PracticeConfig) -> PracticeRun {
        PracticeRun {
            practice: Practice {
                from: self.from,
                to: self.to,
                lead_in: c.lead_in,
                speed: c.speed,
                rank: c.rank,
                total: c.total,
            },
            gauge: c.gauge,
            gauge_start: c.gauge_start.map(|v| v.clamp(2, 100) as f32),
            repeat: c.repeat,
            seed: self.seed,
            result: self.result.clone(),
            viewer: false,
        }
    }

    /// Row `row`'s value.
    fn value(&self, ctx: &Ctx, row: Row) -> String {
        let c = &ctx.config.practice;
        let p = &ctx.config.play;
        // a measure, and when it starts (the section's end: when it ends)
        let bar = |m: usize, end: usize| match &self.read {
            Some(r) => format!("#{m:03}  {}", clock(r.bars[end.min(r.measures())])),
            None => "…".into(),
        };
        match row {
            Row::From => bar(self.from, self.from),
            Row::To => bar(self.to, self.to.saturating_add(1)),
            Row::LeadIn => match c.lead_in {
                0 => tr!("NONE").into(),
                1 => tr!("1 MEASURE").into(),
                n => tr!("{n} MEASURES", n = n),
            },
            Row::Speed => format!("{}%", c.speed),
            Row::Gauge => c.gauge.label().into(),
            Row::GaugeStart => match c.gauge_start {
                Some(v) => format!("{v}%"),
                None => tr!("THE GAUGE'S OWN").into(),
            },
            Row::Rank => match c.rank {
                Some(r) => RANKS[r.clamp(0, 4) as usize].into(),
                None => tr!("CHART ({value})", value = self.chart_rank()),
            },
            Row::Total => match (c.total, self.chart_total(p)) {
                (Some(t), _) => t.to_string(),
                (None, Some(t)) => tr!("CHART ({value})", value = format!("{t:.0}")),
                (None, None) => tr!("CHART").into(),
            },
            Row::Random => p.random_1p.label().into(),
            Row::Random2 => p.random_2p.label().into(),
            Row::LnMode => p.ln_pref().label(),
            Row::Loop => if c.repeat { tr!("ON") } else { tr!("OFF") }.into(),
            Row::Start => format!("▶  {}", tr!("PLAY")),
        }
    }

    fn chart_rank(&self) -> String {
        match self.read.as_ref().map(|r| r.chart.meta.judge_rank) {
            Some(JudgeRank::Rank(r)) if (0..5).contains(&r) => RANKS[r as usize].into(),
            Some(JudgeRank::DefExRank(x) | JudgeRank::Bmson(x)) => format!("{x:.0}%"),
            _ => "NORMAL".into(),
        }
    }

    fn chart_total(&self, p: &PlayConfig) -> Option<f64> {
        let r = self.read.as_ref()?;
        Some(r.chart.total_of(p.ln_mode, p.gauge_preset, None))
    }

    /// Step row `row` by `d`.
    fn step(&mut self, ctx: &mut Ctx, row: Row, d: i32) -> Next {
        let last = self.read.as_ref().map_or(0, |r| r.measures() - 1) as i64;
        let total = self.chart_total(&ctx.config.play);
        let c = &mut ctx.config.practice;
        let p = &mut ctx.config.play;
        let by = |v: i64, lo: i64, hi: i64| (v + d as i64).clamp(lo, hi);
        match row {
            Row::From => {
                self.from = by(self.from as i64, 0, last) as usize;
                self.to = self.to.max(self.from);
            }
            Row::To => {
                self.to = by(self.to as i64, 0, last) as usize;
                self.from = self.from.min(self.to);
            }
            Row::LeadIn => c.lead_in = by(c.lead_in as i64, 0, LEAD_IN_MAX as i64) as usize,
            Row::Speed => {
                let s = c.speed as i32 + d * SPEED_STEP as i32;
                c.speed = (s.max(0) as u32).clamp(SPEED_MIN, SPEED_MAX);
            }
            Row::Gauge => c.gauge = cycle(p.gauge_preset.selectable(), c.gauge, d),
            // its own, then 2 to 100 % in 2 % steps
            Row::GaugeStart => {
                let k = by(c.gauge_start.map_or(0, |v| v as i64 / 2), 0, 50);
                c.gauge_start = (k > 0).then_some(k as u32 * 2);
            }
            Row::Rank => {
                let k = by(c.rank.map_or(-1, |r| r as i64), -1, 4);
                c.rank = (k >= 0).then_some(k as i32);
            }
            // the chart's, then from it in steps of 10
            Row::Total => {
                c.total = match c.total {
                    None if d > 0 => {
                        let t = total.unwrap_or(300.0) / TOTAL_STEP as f64;
                        Some((t.round() as u32 * TOTAL_STEP).clamp(TOTAL_MIN, TOTAL_MAX))
                    }
                    None => None,
                    Some(t) if d < 0 && t <= TOTAL_MIN => None,
                    Some(t) => {
                        let t = t as i64 + d as i64 * TOTAL_STEP as i64;
                        Some((t.max(0) as u32).clamp(TOTAL_MIN, TOTAL_MAX))
                    }
                }
            }
            Row::Random => p.random_1p = cycle(&LaneOption::ALL, p.random_1p, d),
            Row::Random2 => p.random_2p = cycle(&LaneOption::ALL, p.random_2p, d),
            Row::LnMode => {
                let ln = cycle(&LnPref::ALL, p.ln_pref(), d);
                p.set_ln_pref(ln)
            }
            Row::Loop => c.repeat ^= true,
            Row::Start => return if d > 0 { self.start(ctx) } else { Next::Stay },
        }
        ctx.play_sound(Sound::OptionChange);
        Next::Stay
    }

    fn select(&mut self, ctx: &mut Ctx, d: i32) {
        let n = self.rows().len() as i32;
        self.sel = (self.sel as i32 + d).rem_euclid(n) as usize;
        self.held = None;
        ctx.play_sound(Sound::Select);
    }

    fn sel_row(&self) -> Row {
        let rows = self.rows();
        rows[self.sel.min(rows.len() - 1)]
    }

    /// Keep the section (and the settings) for the next time.
    fn keep(&self, ctx: &mut Ctx) {
        if self.read.is_some() {
            let s = &mut ctx.config.practice.sections;
            s.insert(self.song.sha256.clone(), [self.from, self.to]);
        }
        ctx.save_config();
    }

    /// The play of the section.
    fn start(&mut self, ctx: &mut Ctx) -> Next {
        if self.read.is_none() {
            return Next::Stay;
        }
        self.keep(ctx);
        let run = self.run(&ctx.config.practice);
        ctx.play_sound(Sound::Decide);
        let req = PlayRequest {
            song: self.song.clone(),
            autoplay: false,
            course: None,
            replay: None,
            vs: None,
            practice: Some(run),
        };
        Next::Push(Box::new(PlayScene::new(ctx, req)))
    }

    fn nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        match nav {
            Nav::Up => self.select(ctx, -1),
            Nav::Down => self.select(ctx, 1),
            Nav::Decide => return self.press(ctx, 1),
            Nav::Close => return self.press(ctx, -1),
            Nav::Option => return self.start(ctx),
            Nav::Back | Nav::Settings => return self.leave(ctx),
        }
        Next::Stay
    }

    /// ← or → (`d`) pressed on the selected row: it steps, and again while
    /// held.
    fn press(&mut self, ctx: &mut Ctx, d: i32) -> Next {
        self.held = Some(Repeat::new(d, Instant::now(), REPEAT_DELAY));
        self.step(ctx, self.sel_row(), d)
    }

    fn leave(&mut self, ctx: &mut Ctx) -> Next {
        self.keep(ctx);
        Next::Pop
    }

    /// The measure at `x` on the graph.
    fn measure_at(&self, x: f32) -> Option<usize> {
        let r = self.read.as_ref()?;
        let g = self.graph;
        let (b0, b1) = (r.bars[0], r.bars[r.measures()]);
        let k = ((x - g.x) / g.w.max(1.0)).clamp(0.0, 1.0) as f64;
        let t = b0 + (k * (b1 - b0) as f64) as i64;
        let m = r.bars.partition_point(|b| *b <= t).saturating_sub(1);
        Some(m.min(r.measures() - 1))
    }

    /// A tap on the graph moves the section's nearer edge there.
    fn tap_graph(&mut self, ctx: &mut Ctx, x: f32) {
        let Some(m) = self.measure_at(x) else {
            return;
        };
        if m.abs_diff(self.from) <= m.abs_diff(self.to) && m <= self.to {
            self.from = m;
        } else {
            self.to = m.max(self.from);
        }
        ctx.play_sound(Sound::OptionChange);
    }

    fn pointer(&mut self, ctx: &mut Ctx, x: f32, y: f32) -> Next {
        if self.back.contains(x, y) {
            return self.leave(ctx);
        }
        let Some((r, hit)) = self.hits.hit(x, y).map(|(r, h)| (r, *h)) else {
            return Next::Stay;
        };
        match hit {
            Hit::Graph => self.tap_graph(ctx, x),
            Hit::Row(i) => {
                self.sel = i;
                if self.sel_row() == Row::Start {
                    return self.start(ctx);
                }
            }
            Hit::Value(i) => {
                self.sel = i;
                let d = if x < r.x + r.w * 0.5 { -1 } else { 1 };
                return self.step(ctx, self.sel_row(), d);
            }
        }
        Next::Stay
    }

    fn draw_graph(&mut self, ctx: &Ctx, d: &mut Draw, panel: Rect) {
        let u = ctx.ui();
        ui::panel(d, panel);
        let g = Rect::new(
            panel.x + 14.0 * u,
            panel.y + 30.0 * u,
            panel.w - 28.0 * u,
            panel.h - 52.0 * u,
        );
        self.graph = g;
        self.hits.push(panel, Hit::Graph);
        let Some(r) = self.read.as_ref() else {
            d.text(
                tr!("LOADING…"),
                panel.x + panel.w * 0.5,
                panel.y + panel.h * 0.45,
                TextStyle::new(16.0 * u, theme::TEXT_DIM, Align::Center),
            );
            return;
        };
        let c = &ctx.config.practice;
        let n = r.measures();
        let (b0, b1) = (r.bars[0], r.bars[n]);
        let x_of = |t: i64| g.x + g.w * ((t - b0) as f64 / (b1 - b0).max(1) as f64) as f32;
        let rate = |m: usize| r.density[m] as f64 / ((r.bars[m + 1] - r.bars[m]).max(1) as f64);
        let top = (0..n).map(rate).fold(1e-12, f64::max);
        let lead = self.from.saturating_sub(c.lead_in);
        // the section and its lead-in behind the notes
        let band = |a: usize, b: usize| {
            Rect::new(x_of(r.bars[a]), g.y, x_of(r.bars[b]) - x_of(r.bars[a]), g.h)
        };
        d.b.rect(band(lead, self.from), with_alpha(theme::ACCENT, 0.06));
        let sec = band(self.from, self.to + 1);
        d.b.rect(sec, with_alpha(theme::ACCENT, 0.14));
        for m in 0..n {
            let (x0, x1) = (x_of(r.bars[m]), x_of(r.bars[m + 1]));
            let h = g.h * (rate(m) / top) as f32;
            let color = if (self.from..=self.to).contains(&m) {
                theme::ACCENT
            } else if (lead..self.from).contains(&m) {
                with_alpha(theme::ACCENT, 0.45)
            } else {
                with_alpha(theme::TEXT_DIM, 0.55)
            };
            let w = (x1 - x0 - 1.0).max(1.0);
            d.b.rect(Rect::new(x0, g.bottom() - h, w, h), color);
        }
        for x in [sec.x, sec.right()] {
            d.b.rect(
                Rect::new(x - 1.0, g.y - 4.0 * u, 2.0, g.h + 8.0 * u),
                theme::ACCENT,
            );
        }
        d.b.rect(
            Rect::new(g.x, g.bottom(), g.w, 1.0),
            with_alpha(theme::TEXT_DIM, 0.6),
        );
        // measure numbers under it, about every 70 px
        let every = [1, 2, 4, 8, 16, 32, 64, 128]
            .into_iter()
            .find(|k| g.w * *k as f32 / n as f32 >= 70.0 * u)
            .unwrap_or(256);
        let small = TextStyle::new(11.0 * u, theme::TEXT_DIM, Align::Center);
        for m in (0..=n).step_by(every) {
            d.text(
                &format!("{m}"),
                x_of(r.bars[m]),
                g.bottom() + 4.0 * u,
                small,
            );
        }
        // what the section is
        let notes: u32 = r.density[self.from..=self.to].iter().sum();
        let real = (r.bars[self.to + 1] - r.bars[self.from]) as f64 * 100.0 / c.speed as f64;
        let what = tr!(
            "#{from} – #{to}   ·   {n} MEASURES   ·   {notes} NOTES   ·   {time} AT {speed}%",
            from = format!("{:03}", self.from),
            to = format!("{:03}", self.to),
            n = self.to - self.from + 1,
            notes = notes,
            time = clock(real as i64),
            speed = c.speed
        );
        d.text(
            "NOTES",
            panel.x + 14.0 * u,
            panel.y + 9.0 * u,
            TextStyle::new(12.0 * u, theme::TEXT_DIM, Align::Left),
        );
        d.text(
            &what,
            panel.right() - 14.0 * u,
            panel.y + 8.0 * u,
            TextStyle::new(13.0 * u, theme::TEXT, Align::Right).fit(panel.w - 90.0 * u),
        );
    }

    fn draw_rows(&mut self, ctx: &Ctx, d: &mut Draw, panel: Rect) {
        let u = ctx.ui();
        ui::panel(d, panel);
        let rows = self.rows();
        let pad = 10.0 * u;
        let rh = ((panel.h - 2.0 * pad) / rows.len() as f32).min(40.0 * u);
        let size = (rh * 0.42).min(15.0 * u);
        for (i, row) in rows.iter().enumerate() {
            let r = Rect::new(
                panel.x + pad,
                panel.y + pad + i as f32 * rh,
                panel.w - 2.0 * pad,
                rh - 4.0 * u,
            );
            let on = i == self.sel;
            if *row == Row::Start {
                let b = Rect::new(r.x + r.w * 0.25, r.y, r.w * 0.5, r.h);
                ui::button(d, b, &self.value(ctx, *row), u, on);
                self.hits.push(b, Hit::Row(i));
                continue;
            }
            ui::row(d, r, on);
            self.hits.push(r, Hit::Row(i));
            let ty = r.y + (r.h - size) * 0.5;
            d.text(
                row.name(),
                r.x + 12.0 * u,
                ty,
                TextStyle::new(size * 0.9, theme::TEXT_DIM, Align::Left),
            );
            let v = Rect::new(r.x + r.w * 0.42, r.y, r.w * 0.58, r.h);
            self.hits.push(v, Hit::Value(i));
            let arrow = TextStyle::new(
                size,
                with_alpha(theme::ACCENT, if on { 1.0 } else { 0.4 }),
                Align::Center,
            );
            d.text("◀", v.x + 12.0 * u, ty, arrow);
            d.text("▶", v.right() - 14.0 * u, ty, arrow);
            d.text(
                &self.value(ctx, *row),
                v.x + v.w * 0.5,
                ty,
                TextStyle::new(size, theme::TEXT, Align::Center).fit(v.w - 56.0 * u),
            );
        }
    }

    fn draw_result(&self, ctx: &Ctx, d: &mut Draw, panel: Rect) {
        let u = ctx.ui();
        ui::panel(d, panel);
        let x = panel.x + 16.0 * u;
        let mut y = panel.y + 12.0 * u;
        let head = TextStyle::new(13.0 * u, theme::TEXT_DIM, Align::Left);
        let result = self.result.lock().ok().and_then(|r| r.clone());
        let Some(res) = result else {
            d.text(tr!("LAST TRY"), x, y, head);
            let lines = [
                tr!("No try yet: choose a section, then PLAY."),
                "",
                tr!("Practice plays are not recorded: no score,"),
                tr!("lamp, replay or IR entry."),
                "",
                tr!("In play: R or Backspace starts the section"),
                tr!("again, Esc comes back here."),
            ];
            for (i, l) in lines.iter().enumerate() {
                d.text(
                    l,
                    x,
                    y + (28.0 + 20.0 * i as f32) * u,
                    TextStyle::new(14.0 * u, theme::TEXT, Align::Left).fit(panel.w - 32.0 * u),
                );
            }
            return;
        };
        let s = &res.score;
        d.text(
            &format!("{}  ·  #{}", tr!("LAST TRY"), res.attempt),
            x,
            y,
            head,
        );
        let color = match res.outcome {
            "CLEAR" => theme::ACCENT,
            "FAILED" => theme::NOTE_SCRATCH,
            _ => theme::TEXT,
        };
        d.text(
            &format!(
                "{}   {} {:.0}%",
                res.outcome,
                res.gauge.label(),
                res.gauge_value
            ),
            panel.right() - 16.0 * u,
            y,
            TextStyle::new(13.0 * u, color, Align::Right),
        );
        y += 26.0 * u;
        let line = 21.0 * u;
        let col = (panel.w - 32.0 * u) * 0.5;
        let style = TextStyle::new(14.0 * u, theme::TEXT, Align::Left);
        let num = TextStyle::new(14.0 * u, theme::TEXT, Align::Right);
        let counts = Judge::ALL.map(|j| (j.label(), s.count(j)));
        let rate = s.rate() * 100.0;
        let more = [
            ("EX SCORE", format!("{} ({rate:.2}%)", s.ex_score())),
            ("MAX COMBO", s.max_combo.to_string()),
            ("FAST", s.fast().to_string()),
            ("SLOW", s.slow().to_string()),
            (
                "NOTES",
                format!("{} / {}", s.passed.min(s.total_notes), s.total_notes),
            ),
            ("", String::new()),
        ];
        for (i, (name, n)) in counts.iter().enumerate() {
            let yy = y + i as f32 * line;
            d.text(name, x, yy, style);
            d.text(&n.to_string(), x + col - 16.0 * u, yy, num);
            let (name, v) = &more[i];
            d.text(name, x + col, yy, style);
            d.text(v, panel.right() - 16.0 * u, yy, num);
        }
        y += 6.0 * line + 8.0 * u;
        let g = Rect::new(
            x,
            y,
            panel.w - 32.0 * u,
            (panel.bottom() - y - 14.0 * u).max(20.0),
        );
        d.b.rect(g, with_alpha(theme::BACKGROUND, 0.6));
        let range = res
            .windows
            .get(2)
            .map_or(100, |w| w.0.abs().max(w.1.abs()) / 1000);
        let tg = TimingGraph {
            bins: range * 2 + 1,
            colors: None,
            line_width: 1.0,
            average: true,
            deviation: false,
        };
        let area = Area {
            r: g,
            color: [1.0; 4],
            reveal: 1.0,
        };
        graphs::timing(d, &area, &tg, &res.timing_log, &res.windows);
    }
}

/// `us` as m:ss.s.
fn clock(us: i64) -> String {
    let ds = us.max(0) / 100_000;
    format!("{}:{:02}.{}", ds / 600, ds / 10 % 60, ds % 10)
}

impl Scene for PracticeScene {
    fn update(&mut self, ctx: &mut Ctx, _gfx: &mut Gfx, now: Instant) -> Next {
        if let Some(next) = self.poll(ctx) {
            return next;
        }
        let due = self.held.as_mut().is_some_and(|r| r.due(now, REPEAT_EVERY));
        if let Some(d) = self.held.as_ref().filter(|_| due).map(|r| r.key) {
            let row = self.sel_row();
            if matches!(row, Row::Start | Row::Loop) {
                self.held = None;
            } else {
                return self.step(ctx, row, d);
            }
        }
        Next::Stay
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        match ev {
            InputEv::Key {
                code,
                pressed,
                repeat,
                ..
            } => {
                if !*pressed {
                    self.held = None;
                    return Next::Stay;
                }
                if *repeat {
                    return Next::Stay;
                }
                match code {
                    KeyCode::ArrowLeft => return self.press(ctx, -1),
                    KeyCode::ArrowRight => return self.press(ctx, 1),
                    KeyCode::Enter | KeyCode::NumpadEnter => return self.start(ctx),
                    KeyCode::PageUp => self.select(ctx, -4),
                    KeyCode::PageDown => self.select(ctx, 4),
                    c => {
                        if let Some(n) = nav_from_key(*c, &ctx.config.keys) {
                            return self.nav(ctx, n);
                        }
                    }
                }
            }
            InputEv::Pad { id, pressed, .. } => {
                if !*pressed {
                    self.held = None;
                } else if let Some(n) = nav_from_pad(*id, &ctx.config.keys) {
                    return self.nav(ctx, n);
                }
            }
            InputEv::Wheel { dy } => {
                let n = (-dy).round() as i32;
                if n != 0 {
                    self.select(ctx, n);
                }
            }
            InputEv::Pointer {
                phase: Phase::Down,
                x,
                y,
                ..
            } => return self.pointer(ctx, *x, *y),
            _ => {}
        }
        Next::Stay
    }

    fn exit(&mut self, ctx: &mut Ctx, _gfx: &mut Gfx) {
        ctx.save_config();
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, _now: Instant) {
        let u = ctx.ui();
        let (w, h) = (ctx.view.width, ctx.view.height);
        d.b.rect(Rect::new(0.0, 0.0, w, h), theme::BACKGROUND);
        self.hits.clear();
        let m = 16.0 * u;
        d.text(
            tr!("PRACTICE"),
            m,
            12.0 * u,
            TextStyle::new(22.0 * u, theme::ACCENT, Align::Left),
        );
        let title = format!("{}   [{}]", self.song.full_title(), self.song.level);
        d.text(
            &title,
            m + 130.0 * u,
            16.0 * u,
            TextStyle::new(17.0 * u, theme::TEXT, Align::Left).fit(w - 280.0 * u),
        );
        self.back = ui::back_button(d, w, u);
        let top = 52.0 * u;
        let hint = 26.0 * u;
        if ctx.portrait() {
            let gh = (h * 0.16).max(120.0 * u);
            self.draw_graph(ctx, d, Rect::new(m, top, w - 2.0 * m, gh));
            let ry = top + gh + 10.0 * u;
            let rh = (h - ry - hint) * 0.58;
            self.draw_rows(ctx, d, Rect::new(m, ry, w - 2.0 * m, rh));
            let y = ry + rh + 10.0 * u;
            self.draw_result(ctx, d, Rect::new(m, y, w - 2.0 * m, h - y - hint));
        } else {
            let gh = (h * 0.27).clamp(120.0 * u, 200.0 * u);
            self.draw_graph(ctx, d, Rect::new(m, top, w - 2.0 * m, gh));
            let y = top + gh + 10.0 * u;
            let rw = (w - 2.0 * m) * 0.56;
            self.draw_rows(ctx, d, Rect::new(m, y, rw, h - y - hint));
            let x = m + rw + 10.0 * u;
            self.draw_result(ctx, d, Rect::new(x, y, w - m - x, h - y - hint));
        }
        d.text(
            tr!("↑↓ row   ←→ change   Enter / START play   tap the graph: the section's nearer edge   Esc back"),
            w * 0.5,
            h - hint + 6.0 * u,
            TextStyle::new(12.0 * u, theme::TEXT_DIM, Align::Center).fit(w - 2.0 * m),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibebms_core::play::{build_chart, RuleOptions};

    /// The screen on made-up chart `keys` (read as the job would).
    fn scene(ctx: &Ctx, keys: usize) -> PracticeScene {
        let song = Song {
            sha256: "c".repeat(64),
            path: "song/chart.bme".into(),
            ..Song::default()
        };
        let mut s = PracticeScene::new(ctx, song);
        let text = vibebms_core::bot::chart_text(keys, 3);
        let path = std::path::Path::new("song/chart.bme");
        let chart = build_chart(path, text.as_bytes(), &RuleOptions::default(), 1, None)
            .unwrap()
            .chart;
        let bars = practice::measures(&chart);
        let density = practice::density(&chart, &bars);
        s.read = Some(Read {
            chart,
            bars,
            density,
        });
        s.prefetch = None;
        s.to = s.to.min(s.read.as_ref().unwrap().measures() - 1);
        s
    }

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("vibebms-practice-{name}-{}", std::process::id()))
    }

    #[test]
    fn the_section_and_the_settings_step_within_their_ranges() {
        let dir = root("steps");
        let mut ctx = Ctx::for_test(dir.clone());
        let mut s = scene(&ctx, 7);
        let last = s.read.as_ref().unwrap().measures() - 1;
        // the whole chart at first; the end never before the start
        assert_eq!((s.from, s.to), (0, last));
        s.step(&mut ctx, Row::To, -(last as i32) - 5);
        assert_eq!((s.from, s.to), (0, 0));
        for _ in 0..3 {
            s.step(&mut ctx, Row::From, 1);
        }
        assert_eq!((s.from, s.to), (3, 3));
        s.step(&mut ctx, Row::To, 4);
        assert_eq!((s.from, s.to), (3, 7));
        // the speed in 5 % steps, 50 to 200
        for _ in 0..20 {
            s.step(&mut ctx, Row::Speed, -1);
        }
        assert_eq!(ctx.config.practice.speed, SPEED_MIN);
        s.step(&mut ctx, Row::Speed, 1);
        assert_eq!(ctx.config.practice.speed, 55);
        for _ in 0..40 {
            s.step(&mut ctx, Row::Speed, 1);
        }
        assert_eq!(ctx.config.practice.speed, SPEED_MAX);
        // the gauge's own start, then 2 % steps
        s.step(&mut ctx, Row::GaugeStart, 1);
        assert_eq!(ctx.config.practice.gauge_start, Some(2));
        s.step(&mut ctx, Row::GaugeStart, -1);
        assert_eq!(ctx.config.practice.gauge_start, None);
        // #RANK: the chart's, VERY HARD .. VERY EASY
        s.step(&mut ctx, Row::Rank, 1);
        assert_eq!(ctx.config.practice.rank, Some(0));
        assert_eq!(s.value(&ctx, Row::Rank), "VERY HARD");
        // TOTAL: from the chart's, rounded to 10
        let total = s.chart_total(&ctx.config.play).unwrap();
        s.step(&mut ctx, Row::Total, 1);
        let t = ctx.config.practice.total.unwrap();
        assert_eq!(t % TOTAL_STEP, 0);
        assert!((t as f64 - total).abs() <= 5.0);
        s.step(&mut ctx, Row::Total, -1);
        assert_eq!(ctx.config.practice.total, Some(t - TOTAL_STEP));
        // lead-in 0 to 4
        for _ in 0..9 {
            s.step(&mut ctx, Row::LeadIn, 1);
        }
        assert_eq!(ctx.config.practice.lead_in, LEAD_IN_MAX);
        // RANDOM 2P only for a double play chart
        assert!(!s.rows().contains(&Row::Random2));
        assert!(scene(&ctx, 14).rows().contains(&Row::Random2));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_play_gets_the_settings_and_the_section_is_kept() {
        let dir = root("keep");
        let mut ctx = Ctx::for_test(dir.clone());
        let mut s = scene(&ctx, 7);
        (s.from, s.to) = (4, 9);
        let c = &mut ctx.config.practice;
        (c.speed, c.lead_in, c.repeat, c.gauge_start) = (75, 2, true, Some(60));
        let run = s.run(&ctx.config.practice);
        assert_eq!((run.practice.from, run.practice.to), (4, 9));
        assert_eq!((run.practice.speed, run.practice.lead_in), (75, 2));
        assert_eq!((run.repeat, run.gauge_start), (true, Some(60.0)));
        assert_eq!(run.label(), "PRACTICE  #004-#009  75%");
        // its rules: its gauge, no GAUGE AUTO SHIFT
        let rules = run.rules(&ctx.config.play.rules);
        assert_eq!(rules.gauge, ctx.config.practice.gauge);
        assert_eq!(
            rules.gauge_auto_shift,
            vibebms_core::gauge::GaugeAutoShift::None
        );
        // leaving keeps the chart's section in the config file
        assert!(matches!(s.leave(&mut ctx), Next::Pop));
        let back = crate::config::Config::load(&ctx.paths.config_file());
        assert_eq!(back.practice.sections[&"c".repeat(64)], [4, 9]);
        assert_eq!(back.practice.speed, 75);
        // and the next time it opens there
        let again = PracticeScene::new(&ctx, s.song.clone());
        assert_eq!((again.from, again.to), (4, 9));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_tap_on_the_graph_moves_the_nearer_edge() {
        let dir = root("tap");
        let mut ctx = Ctx::for_test(dir.clone());
        let mut s = scene(&ctx, 7);
        let r = s.read.as_ref().unwrap();
        let (b0, b1) = (r.bars[0], r.bars[r.measures()]);
        let x_of = |m: usize| 100.0 + 1000.0 * (r.bars[m] - b0) as f32 / (b1 - b0) as f32 + 1.0;
        let (x5, x12, x20) = (x_of(5), x_of(12), x_of(20));
        s.graph = Rect::new(100.0, 0.0, 1000.0, 100.0);
        (s.from, s.to) = (8, 16);
        s.tap_graph(&mut ctx, x5);
        assert_eq!((s.from, s.to), (5, 16));
        s.tap_graph(&mut ctx, x20);
        assert_eq!((s.from, s.to), (5, 20));
        s.tap_graph(&mut ctx, x12);
        assert_eq!((s.from, s.to), (12, 20));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clock_reads_minutes_seconds_tenths() {
        assert_eq!(clock(0), "0:00.0");
        assert_eq!(clock(83_450_000), "1:23.4");
        assert_eq!(clock(-5), "0:00.0");
    }
}
