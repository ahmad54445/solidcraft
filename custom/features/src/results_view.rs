//! Which result the 3D view shows, and building its coloured elements (docs/10-viewport-results.md).
//!
//! Building runs on a worker thread (rule V1); the UI only reads [`shown`]. Values stay SI in the
//! result store and are scaled to the display units here.

use std::sync::{LazyLock, OnceLock, PoisonError, RwLock};

use crate::transfer::{self, Geometry, ResultBlock};
use crate::units::Units;

pub const SHELL_COMPONENTS: [&str; 8] = ["F11", "F22", "F12", "M11", "M22", "M12", "V13", "V23"];
pub const FRAME_COMPONENTS: [&str; 6] = ["P", "V2", "V3", "T", "M2", "M3"];

/// Envelope side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Max,
    Min,
}

/// What the user chose.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choice {
    /// Load case or combination; `None` = elements without colours.
    pub load: Option<String>,
    pub shell: usize,
    pub frame: usize,
    pub step: Step,
}

/// What the view shows now (for the status bar and the legend).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub choice: Choice,
    pub units: Option<Units>,
    pub shell: Option<Legend>,
    pub frame: Option<Legend>,
    /// The load has Max/Min rows.
    pub envelope: bool,
    pub busy: bool,
    pub error: Option<String>,
}

/// One colour bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Legend {
    pub title: String,
    pub unit: String,
    pub range: [f32; 2],
}

static SHOWN: LazyLock<RwLock<Shown>> = LazyLock::new(|| RwLock::new(Shown::default()));
static REPAINT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn shown() -> Shown {
    SHOWN.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// How a worker asks the UI to draw again (set once by the UI).
pub fn set_repaint(f: Box<dyn Fn() + Send + Sync>) {
    let _ = REPAINT.set(f);
}

/// Load cases / combinations with stored results, in transfer order.
pub fn loads() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for b in transfer::results() {
        if !names.contains(&b.name) {
            names.push(b.name);
        }
    }
    names
}

/// Show `choice` with `units`: builds on a worker thread, then swaps it into the view.
pub fn select(choice: Choice, units: Units) {
    {
        let mut s = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
        s.choice = choice.clone();
        s.units = Some(units);
        s.busy = true;
    }
    std::thread::spawn(move || {
        let outcome = build(&transfer::geometry(), &transfer::results(), &choice, units);
        // A newer choice may have been made meanwhile: only the latest one is shown.
        let mut s = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
        if s.choice != choice || s.units != Some(units) {
            return;
        }
        match outcome {
            Ok(built) => {
                #[cfg(feature = "ui")]
                custom_bridge::view::show(&built.layers, built.bounds);
                s.shell = built.shell;
                s.frame = built.frame;
                s.envelope = built.envelope;
                s.error = None;
            }
            Err(e) => s.error = Some(e),
        }
        s.busy = false;
        drop(s);
        if let Some(repaint) = REPAINT.get() {
            repaint();
        }
    });
}

/// After a transfer: the first load, the first components, current units.
pub fn show_first() {
    let units = crate::model::read(|f| f.units);
    select(Choice { load: loads().into_iter().next(), ..Default::default() }, units);
}

/// The result of [`build`].
pub struct Built {
    #[cfg(feature = "ui")]
    pub layers: Vec<custom_bridge::view::Layer>,
    pub bounds: Option<[[f64; 3]; 2]>,
    pub shell: Option<Legend>,
    pub frame: Option<Legend>,
    pub envelope: bool,
    /// Per shell corner (n1..n4) the shown value; per frame its (station, value) points.
    pub shell_values: Vec<[f32; 4]>,
    pub frame_values: Vec<Vec<(f64, f32)>>,
}

/// Shell colours when no result is shown.
#[cfg(feature = "ui")]
const SHELL_PLAIN: [u8; 3] = [190, 196, 206];
#[cfg(feature = "ui")]
const FRAME_PLAIN: [u8; 3] = [70, 72, 84];

/// Colour the transferred elements by `choice`.
pub fn build(g: &Geometry, blocks: &[ResultBlock], choice: &Choice, units: Units) -> Result<Built, String> {
    let find = |family: &str| choice.load.as_ref().and_then(|l| blocks.iter().find(|b| &b.name == l && b.family == family));
    let shell_block = find("shell_results");
    let frame_block = find("frame_results");

    let (shell_values, shell_env) = match shell_block {
        Some(b) => shell_corner_values(g, &Block::read(b)?, 4 + choice.shell.min(7), choice.step)?,
        None => (Vec::new(), false),
    };
    let (frame_values, frame_env) = match frame_block {
        Some(b) => frame_station_values(g, &Block::read(b)?, 4 + choice.frame.min(5), choice.step)?,
        None => (Vec::new(), false),
    };

    // Display units (stored values are N, m).
    let (shell_scale, shell_unit) = shell_unit(choice.shell, units);
    let (frame_scale, frame_unit) = frame_unit(choice.frame, units);
    let shell_values: Vec<[f32; 4]> = shell_values.iter().map(|v| v.map(|x| (f64::from(x) / shell_scale) as f32)).collect();
    let frame_values: Vec<Vec<(f64, f32)>> = frame_values.iter().map(|pts| pts.iter().map(|(s, v)| (*s, (f64::from(*v) / frame_scale) as f32)).collect()).collect();

    let shell_range = range(shell_values.iter().flatten().copied());
    let frame_range = range(frame_values.iter().flatten().map(|(_, v)| *v));
    let label = |load: &Option<String>, comp: &str| format!("{} – {comp}", load.as_deref().unwrap_or(""));

    #[cfg(feature = "ui")]
    let p = |j: u32| g.joints.get(j as usize).map_or([0.0; 3], |p| [p[0] as f32, p[1] as f32, p[2] as f32]);
    let mut bounds: Option<[[f64; 3]; 2]> = None;
    let mut grow = |j: u32| {
        if let Some(q) = g.joints.get(j as usize) {
            let b = bounds.get_or_insert([*q, *q]);
            for k in 0..3 {
                b[0][k] = b[0][k].min(q[k]);
                b[1][k] = b[1][k].max(q[k]);
            }
        }
    };
    for s in &g.shells {
        s[..4].iter().filter(|j| **j != u32::MAX).for_each(|j| grow(*j));
    }
    for f in &g.frames {
        grow(f[0]);
        grow(f[1]);
    }

    #[cfg(feature = "ui")]
    let layers = {
        use custom_bridge::view::{Coloring, Layer, Segment, Triangle};
        let mut shells = Vec::with_capacity(g.shells.len() * 2);
        for (e, s) in g.shells.iter().enumerate() {
            let v = shell_values.get(e).copied().unwrap_or([0.0; 4]);
            let tri = |a: usize, b: usize, c: usize| Triangle { corners: [p(s[a]), p(s[b]), p(s[c])], values: [v[a], v[b], v[c]] };
            shells.push(tri(0, 1, 2));
            if s[3] != u32::MAX {
                shells.push(tri(0, 2, 3));
            }
        }
        let mut frames = Vec::with_capacity(g.frames.len());
        for (e, f) in g.frames.iter().enumerate() {
            let (a, b) = (p(f[0]), p(f[1]));
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt().max(f32::EPSILON);
            let at = |sta: f64| {
                let t = (sta as f32 / len).clamp(0.0, 1.0);
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
            };
            match frame_values.get(e).filter(|pts| pts.len() >= 2) {
                Some(pts) => frames.extend(pts.windows(2).map(|w| Segment { ends: [at(w[0].0), at(w[1].0)], values: [w[0].1, w[1].1] })),
                None => {
                    let v = frame_values.get(e).and_then(|pts| pts.first()).map_or(0.0, |x| x.1);
                    frames.push(Segment { ends: [a, b], values: [v, v] });
                }
            }
        }
        vec![
            Layer { triangles: shells, segments: Vec::new(), coloring: shell_range.map_or(Coloring::Plain(SHELL_PLAIN), Coloring::Ramp) },
            Layer { triangles: Vec::new(), segments: frames, coloring: frame_range.map_or(Coloring::Plain(FRAME_PLAIN), Coloring::Ramp) },
        ]
    };

    Ok(Built {
        #[cfg(feature = "ui")]
        layers,
        bounds,
        shell: shell_range.map(|range| Legend { title: format!("Shells: {}", label(&choice.load, SHELL_COMPONENTS[choice.shell.min(7)])), unit: shell_unit, range }),
        frame: frame_range.map(|range| Legend { title: format!("Frames: {}", label(&choice.load, FRAME_COMPONENTS[choice.frame.min(5)])), unit: frame_unit, range }),
        envelope: shell_env || frame_env,
        shell_values,
        frame_values,
    })
}

/// Lowest and highest finite value; a single value gets a tiny width so the ramp stays defined.
fn range(values: impl Iterator<Item = f32>) -> Option<[f32; 2]> {
    let (lo, hi) = values.filter(|v| v.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), v| (l.min(v), h.max(v)));
    if lo > hi {
        return None;
    }
    Some(if hi - lo < f32::EPSILON * hi.abs().max(1.0) { [lo - 0.5, hi + 0.5] } else { [lo, hi] })
}

/// Scale (SI per display unit) and unit text of a shell component: forces per length, moments per length.
pub fn shell_unit(component: usize, u: Units) -> (f64, String) {
    let per_length = u.length.to_si();
    if (3..=5).contains(&component) {
        (u.moment.to_si() / per_length, format!("{}/{}", u.moment.symbol(), u.length.symbol()))
    } else {
        (u.force.to_si() / per_length, format!("{}/{}", u.force.symbol(), u.length.symbol()))
    }
}

/// Scale and unit text of a frame component: P, V2, V3 forces; T, M2, M3 moments.
pub fn frame_unit(component: usize, u: Units) -> (f64, String) {
    if component >= 3 { (u.moment.to_si(), u.moment.symbol().to_string()) } else { (u.force.to_si(), u.force.symbol().to_string()) }
}

/// A stored result block read back: its columns by name.
struct Block {
    rows: usize,
    bytes: Vec<u8>,
    /// (name, byte offset, is f64)
    columns: Vec<(String, usize, bool)>,
}

impl Block {
    fn read(b: &ResultBlock) -> Result<Self, String> {
        let bytes = std::fs::read(&b.file).map_err(|e| format!("result store {}: {e}", b.file.display()))?;
        let mut columns = Vec::new();
        let mut at = 0;
        for c in &b.columns {
            let (name, kind) = c.split_once(':').unwrap_or((c, "f64"));
            let f64 = kind == "f64";
            columns.push((name.to_string(), at, f64));
            at += b.rows * if f64 { 8 } else { 4 };
        }
        if at != bytes.len() {
            return Err(format!("result block {} has {} bytes, expected {at}", b.name, bytes.len()));
        }
        Ok(Self { rows: b.rows, bytes, columns })
    }

    fn column(&self, index: usize) -> Option<(usize, bool)> {
        self.columns.get(index).map(|(_, at, f)| (*at, *f))
    }

    fn named(&self, name: &str) -> Option<(usize, bool)> {
        self.columns.iter().find(|(n, ..)| n == name).map(|(_, at, f)| (*at, *f))
    }

    fn u32(&self, (at, _): (usize, bool), row: usize) -> u32 {
        let o = at + row * 4;
        u32::from_le_bytes(self.bytes[o..o + 4].try_into().unwrap_or_default())
    }

    fn f64(&self, (at, _): (usize, bool), row: usize) -> f64 {
        let o = at + row * 8;
        f64::from_le_bytes(self.bytes[o..o + 8].try_into().unwrap_or_default())
    }
}

/// Which rows to use: envelopes Max (1) / Min (2); otherwise single (0); multi-step (3) the first step.
fn step_filter(b: &Block, kinds: (usize, bool), nums: (usize, bool), step: Step) -> (Box<dyn Fn(usize) -> bool + '_>, bool) {
    let has = |k: u32| (0..b.rows).any(|r| b.u32(kinds, r) == k);
    if has(1) || has(2) {
        let want = if step == Step::Max { 1 } else { 2 };
        return (Box::new(move |r| b.u32(kinds, r) == want), true);
    }
    if !has(0) && has(3) {
        let first = (0..b.rows).find(|r| b.u32(kinds, *r) == 3).map(|r| b.f64(nums, r)).unwrap_or(0.0);
        return (Box::new(move |r| b.f64(nums, r) == first), false);
    }
    (Box::new(move |r| b.u32(kinds, r) == 0), false)
}

/// Shell value per element corner (n1..n4); columns: element, joint, step_type, step_num, values…
fn shell_corner_values(g: &Geometry, b: &Block, value_column: usize, step: Step) -> Result<(Vec<[f32; 4]>, bool), String> {
    let (e, j, k, n) = (b.named("element"), b.named("joint"), b.named("step_type"), b.named("step_num"));
    let (Some(e), Some(j), Some(k), Some(n), Some(v)) = (e, j, k, n, b.column(value_column)) else {
        return Err("shell result block is missing columns".into());
    };
    let (keep, envelope) = step_filter(b, k, n, step);
    let mut out = vec![[0.0f32; 4]; g.shells.len()];
    for r in (0..b.rows).filter(|r| keep(*r)) {
        let (el, jt) = (b.u32(e, r) as usize, b.u32(j, r));
        if let (Some(s), Some(o)) = (g.shells.get(el), out.get_mut(el))
            && let Some(c) = s[..4].iter().position(|x| *x == jt)
        {
            o[c] = b.f64(v, r) as f32;
        }
    }
    Ok((out, envelope))
}

/// Frame values per element as (station from I-end in m, value), sorted by station.
fn frame_station_values(g: &Geometry, b: &Block, value_column: usize, step: Step) -> Result<(Vec<Vec<(f64, f32)>>, bool), String> {
    let (e, k, n, s) = (b.named("element"), b.named("step_type"), b.named("step_num"), b.named("station"));
    let (Some(e), Some(k), Some(n), Some(s), Some(v)) = (e, k, n, s, b.column(value_column)) else {
        return Err("frame result block is missing columns".into());
    };
    let (keep, envelope) = step_filter(b, k, n, step);
    let mut out = vec![Vec::new(); g.frames.len()];
    for r in (0..b.rows).filter(|r| keep(*r)) {
        if let Some(pts) = out.get_mut(b.u32(e, r) as usize) {
            pts.push((b.f64(s, r), b.f64(v, r) as f32));
        }
    }
    for pts in &mut out {
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    Ok((out, envelope))
}
