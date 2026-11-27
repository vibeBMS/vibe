//! The graphs a skin places, drawn by the scene with the look the skin
//! asks for.

/// A graph drawn by the scene, with the look the skin asks for.
#[derive(Clone, Debug)]
pub enum Graph {
    Gauge(GaugeGraph),
    Notes(NotesGraph),
    Bpm(BpmGraph),
    Timing(TimingGraph),
    Visualizer(VisualizerGraph),
}

impl Graph {
    /// Milliseconds the graph takes to appear (0 = the scene's choice).
    pub fn delay(&self) -> i64 {
        match self {
            Graph::Gauge(g) => g.delay,
            Graph::Notes(g) => g.delay,
            Graph::Bpm(g) => g.delay,
            Graph::Timing(_) | Graph::Visualizer(_) => 0,
        }
    }
}

/// The cells of a gauge a skin asks for: each is drawn every frame.
pub(super) fn gauge_parts(n: i64) -> usize {
    n.clamp(1, 1000) as usize
}

/// The ms a timing visualizer shows to either side, of what a skin asks
/// for: a tick is drawn every 10.
pub(super) fn visualizer_range(ms: i64) -> f32 {
    ms.clamp(1, 2000) as f32
}

/// The 1 ms bins of a timing distribution graph a skin asks for: each is
/// counted and drawn every frame.
pub(super) fn timing_bins(n: i64) -> i64 {
    n.clamp(3, 2001)
}

/// The gauge over the play (result).
#[derive(Clone, Debug, Default)]
pub struct GaugeGraph {
    /// Per gauge kind (assist, easy, normal, hard, ex-hard, hazard): line
    /// above the border, background above, line below, background below.
    /// `None`: the built-in skin's look (palette colours, filled, guides).
    pub colors: Option<Vec<[f32; 4]>>,
    pub line_width: f32,
    pub delay: i64,
}

/// beatoraja's notes distribution: per second, one chip per note.
#[derive(Clone, Debug, Default)]
pub struct NotesGraph {
    /// 0 note types, 1 judgements, 2 judgements with FAST / SLOW.
    pub kind: usize,
    pub delay: i64,
    /// Dark background with guides.
    pub back: bool,
    pub order_reverse: bool,
    pub no_gap: bool,
    pub no_gap_x: bool,
    /// The built-in skin's look: with FAST / SLOW, FAST up and SLOW down
    /// per section.
    pub built_in: bool,
}

/// BPM over the chart.
#[derive(Clone, Debug, Default)]
pub struct BpmGraph {
    /// Main, min, max, other BPM, stop line, transition line.
    pub colors: Vec<[f32; 4]>,
    pub line_width: f32,
    pub delay: i64,
}

/// Distribution of timing errors.
#[derive(Clone, Debug, Default)]
pub struct TimingGraph {
    /// Width in 1 ms bins.
    pub bins: i64,
    /// Graph, average, deviation, then the PG .. PR window bands. `None`:
    /// the built-in skin's look (FAST / SLOW coloured, average and spread
    /// as text).
    pub colors: Option<Vec<[f32; 4]>>,
    pub line_width: f32,
    pub average: bool,
    pub deviation: bool,
}

/// Recent timing errors as ticks over the judge windows (play).
#[derive(Clone, Debug, Default)]
pub struct VisualizerGraph {
    /// beatoraja's hit error visualizer rather than the timing visualizer.
    pub hit_error: bool,
    /// Half width in ms.
    pub range_ms: f32,
    /// Line, centre, PG .. PR bands (and the moving average).
    pub colors: Vec<[f32; 4]>,
    pub line_width: f32,
    pub transparent_poor: bool,
    pub decay: bool,
    /// Ticks coloured by judgement.
    pub by_judge: bool,
    /// Moving average line.
    pub ema: bool,
}
