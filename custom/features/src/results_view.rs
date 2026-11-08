//! Which result the 3D view shows, and building its coloured elements (docs/10-viewport-results.md).
//!
//! Building runs on a worker thread (rule V1, P1); the UI only reads [`shown`]. Values stay SI in
//! the result store and are scaled to the display units here. Derived components (principal
//! values, VMax) and nodal averages are computed in one pass over the flat columns (P2, P3).

#[cfg(feature = "ui")]
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, OnceLock, PoisonError, RwLock};

use crate::transfer::{self, Geometry, ResultBlock};
use crate::units::Units;
use crate::vv::{self, Look, Settings};

/// Shell components: 0–7 are stored (CSiBridge names), 8–14 are computed in Factory.
/// Panel names (MIDAS style) in [`SHELL_PANEL_NAMES`].
pub const SHELL_COMPONENTS: [&str; 15] = ["F11", "F22", "F12", "M11", "M22", "M12", "V13", "V23", "Fmax", "Fmin", "|F|max", "Mmax", "Mmin", "|M|max", "VMax"];
/// The same components as the results panel names them (docs/10 §7.2).
pub const SHELL_PANEL_NAMES: [&str; 15] = ["Fxx", "Fyy", "Fxy", "Mxx", "Myy", "Mxy", "Vxx", "Vyy", "Fmax", "Fmin", "FMax", "Mmax", "Mmin", "MMax", "VMax"];
pub const FRAME_COMPONENTS: [&str; 6] = ["P", "V2", "V3", "T", "M2", "M3"];

/// Envelope side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Max,
    Min,
}

/// Value at a shell joint: the element's own (may jump between elements) or the average of all
/// elements meeting there (smooth).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Averaging {
    #[default]
    Element,
    Nodal,
}

/// What the user chose.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choice {
    /// Load case or combination; `None` = elements without colours.
    pub load: Option<String>,
    pub shell: usize,
    pub frame: usize,
    pub step: Step,
    pub averaging: Averaging,
    /// Avg. Nodal over shown elements only.
    pub active_only: bool,
}

/// What the view shows now (for the status bar, the panel and the legend).
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
    /// Typed legend ranges (None = automatic).
    pub manual_shell: Option<[f32; 2]>,
    pub manual_frame: Option<[f32; 2]>,
    /// Which drawn layer carries each contour (for range changes without rebuilding).
    pub shell_layer: Option<usize>,
    pub frame_layer: Option<usize>,
    pub layer_count: usize,
}

/// One colour bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Legend {
    pub title: String,
    pub unit: String,
    pub range: [f32; 2],
}

static SHOWN: LazyLock<RwLock<Shown>> = LazyLock::new(|| RwLock::new(Shown::default()));
static SNAP: LazyLock<RwLock<Arc<SnapData>>> = LazyLock::new(|| RwLock::new(Arc::new(SnapData::default())));

/// What value snap and value labels need: the shown values in display units (shared, swapped
/// per result). Elements that are hidden or not contoured have NaN values (rule G5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapData {
    /// Per shell, the value at its corners n1..n4.
    pub shell_values: Vec<[f32; 4]>,
    /// Frame result stations: world position (m) and value.
    pub frame_points: Vec<([f64; 3], f32)>,
    pub shell_unit: String,
    pub frame_unit: String,
    /// Per shell / frame: drawn at all (not hidden by Visibility/Graphics).
    pub shell_shown: Vec<bool>,
    pub frame_shown: Vec<bool>,
}

/// The values of the shown result, for value snap.
pub fn snap_data() -> Arc<SnapData> {
    SNAP.read().unwrap_or_else(PoisonError::into_inner).clone()
}
static REPAINT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn shown() -> Shown {
    SHOWN.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// How a worker asks the UI to draw again (set once by the UI).
pub fn set_repaint(f: Box<dyn Fn() + Send + Sync>) {
    let _ = REPAINT.set(f);
}

fn repaint() {
    if let Some(repaint) = REPAINT.get() {
        repaint();
    }
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
    let settings = vv::settings();
    {
        let mut s = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
        s.choice = choice.clone();
        s.units = Some(units);
        s.busy = true;
    }
    std::thread::spawn(move || {
        let outcome = build_with(&transfer::geometry(), &transfer::results(), &choice, units, &settings);
        // A newer choice may have been made meanwhile: only the latest one is shown.
        let mut s = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
        if s.choice != choice || s.units != Some(units) || vv::settings() != settings {
            return;
        }
        match outcome {
            Ok(built) => {
                #[cfg(feature = "ui")]
                {
                    custom_bridge::view::show(&built.layers, built.bounds);
                    let mut ranges = vec![None; built.layers.len()];
                    if let (Some(k), Some(r)) = (built.shell_layer, s.manual_shell) {
                        ranges[k] = Some(r);
                    }
                    if let (Some(k), Some(r)) = (built.frame_layer, s.manual_frame) {
                        ranges[k] = Some(r);
                    }
                    if ranges.iter().any(Option::is_some) {
                        custom_bridge::view::set_ranges(&ranges);
                    }
                    s.layer_count = built.layers.len();
                }
                *SNAP.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(built.snap.clone());
                s.shell = built.shell.map(|l| Legend { range: s.manual_shell.unwrap_or(l.range), ..l });
                s.frame = built.frame.map(|l| Legend { range: s.manual_frame.unwrap_or(l.range), ..l });
                s.shell_layer = built.shell_layer;
                s.frame_layer = built.frame_layer;
                s.envelope = built.envelope;
                s.error = None;
            }
            Err(e) => s.error = Some(e),
        }
        s.busy = false;
        drop(s);
        repaint();
    });
}

/// Build the view again with the current choice (after Visibility/Graphics changed).
pub fn refresh() {
    let s = shown();
    if let Some(units) = s.units {
        select(s.choice, units);
    }
}

/// Typed legend ranges (None = automatic). Changes only the colour scale: nothing is uploaded
/// (rule V2 / P5) unless switching back to automatic, which needs the computed range again.
pub fn set_manual_ranges(shell: Option<[f32; 2]>, frame: Option<[f32; 2]>) {
    let mut s = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
    let back_to_auto = (s.manual_shell.is_some() && shell.is_none()) || (s.manual_frame.is_some() && frame.is_none());
    s.manual_shell = shell;
    s.manual_frame = frame;
    if back_to_auto {
        drop(s);
        refresh();
        return;
    }
    #[cfg(feature = "ui")]
    {
        let mut ranges = vec![None; s.layer_count];
        if let (Some(k), Some(r)) = (s.shell_layer, shell) {
            ranges[k] = Some(r);
        }
        if let (Some(k), Some(r)) = (s.frame_layer, frame) {
            ranges[k] = Some(r);
        }
        custom_bridge::view::set_ranges(&ranges);
    }
    if let (Some(l), Some(r)) = (s.shell.as_mut(), shell) {
        l.range = r;
    }
    if let (Some(l), Some(r)) = (s.frame.as_mut(), frame) {
        l.range = r;
    }
    drop(s);
    repaint();
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
    /// Index of the contoured shell / frame layer in `layers`.
    pub shell_layer: Option<usize>,
    pub frame_layer: Option<usize>,
    pub bounds: Option<[[f64; 3]; 2]>,
    pub shell: Option<Legend>,
    pub frame: Option<Legend>,
    pub envelope: bool,
    /// Per shell corner (n1..n4) the shown value; per frame its (station, value) points.
    pub shell_values: Vec<[f32; 4]>,
    pub frame_values: Vec<Vec<(f64, f32)>>,
    /// The same values arranged for value snap (hidden / not contoured: NaN, left out).
    pub snap: SnapData,
}

/// Colour the transferred elements by `choice` with the default view settings.
pub fn build(g: &Geometry, blocks: &[ResultBlock], choice: &Choice, units: Units) -> Result<Built, String> {
    build_with(g, blocks, choice, units, &Settings::default())
}

/// Colour the transferred elements by `choice`, drawn as the Visibility/Graphics `settings` say.
pub fn build_with(g: &Geometry, blocks: &[ResultBlock], choice: &Choice, units: Units, settings: &Settings) -> Result<Built, String> {
    let find = |family: &str| choice.load.as_ref().and_then(|l| blocks.iter().find(|b| &b.name == l && b.family == family));
    let shell_block = find("shell_results");
    let frame_block = find("frame_results");
    let (shell_look, frame_look) = vv::looks(g, settings);

    let (mut shell_values, shell_env) = match shell_block {
        Some(b) => shell_corner_values(g, &Block::read(b)?, choice.shell.min(14), choice.step)?,
        None => (Vec::new(), false),
    };
    if choice.averaging == Averaging::Nodal && !shell_values.is_empty() {
        let shown_only = |e: usize| !choice.active_only || shell_look[e] != Look::Hidden;
        average_at_joints(g, &mut shell_values, shown_only);
    }
    let (frame_values, frame_env) = match frame_block {
        Some(b) => frame_station_values(g, &Block::read(b)?, 4 + choice.frame.min(5), choice.step)?,
        None => (Vec::new(), false),
    };

    // Display units (stored values are N, m).
    let (shell_scale, shell_unit) = shell_unit(choice.shell, units);
    let (frame_scale, frame_unit) = frame_unit(choice.frame, units);
    let shell_values: Vec<[f32; 4]> = shell_values.iter().map(|v| v.map(|x| (f64::from(x) / shell_scale) as f32)).collect();
    let frame_values: Vec<Vec<(f64, f32)>> = frame_values.iter().map(|pts| pts.iter().map(|(s, v)| (*s, (f64::from(*v) / frame_scale) as f32)).collect()).collect();

    // Contoured = shown normally, contour on, values present.
    let shell_contour = |e: usize| settings.contour && shell_look[e] == Look::Normal && !shell_values.is_empty();
    let frame_contour = |e: usize| settings.contour && frame_look[e] == Look::Normal && frame_values.get(e).is_some_and(|p| !p.is_empty());
    let shell_range = range((0..g.shells.len()).filter(|e| shell_contour(*e)).flat_map(|e| shell_values[e]));
    let frame_range = range((0..g.frames.len()).filter(|e| frame_contour(*e)).flat_map(|e| frame_values[e].iter().map(|(_, v)| *v)));
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
    for (e, s) in g.shells.iter().enumerate() {
        if shell_look[e] != Look::Hidden {
            s[..4].iter().filter(|j| **j != u32::MAX).for_each(|j| grow(*j));
        }
    }
    for (e, f) in g.frames.iter().enumerate() {
        if frame_look[e] != Look::Hidden {
            grow(f[0]);
            grow(f[1]);
        }
    }

    // One layer per look (rule G1): contour, plain category colour, halftone, each filter colour.
    #[cfg(feature = "ui")]
    let (layers, shell_layer, frame_layer) = {
        use custom_bridge::view::{Coloring, Layer, Segment, Triangle};
        let mut layers: Vec<Layer> = Vec::new();
        let mut shell_layer = None;
        let mut frame_layer = None;

        // Shells grouped by how they are coloured.
        let mut shell_groups: BTreeMap<Option<[u8; 3]>, Vec<Triangle>> = BTreeMap::new();
        for (e, s) in g.shells.iter().enumerate() {
            let colour = match shell_look[e] {
                Look::Hidden => continue,
                Look::Normal if shell_contour(e) => None,
                Look::Normal => Some(settings.shells.surface),
                Look::Halftone => Some(vv::halftone(settings.shells.surface)),
                Look::Colour(c) => Some(c),
            };
            let v = shell_values.get(e).copied().unwrap_or([0.0; 4]);
            let tri = |a: usize, b: usize, c: usize| Triangle { corners: [p(s[a]), p(s[b]), p(s[c])], values: [v[a], v[b], v[c]] };
            let list = shell_groups.entry(colour).or_default();
            list.push(tri(0, 1, 2));
            if s[3] != u32::MAX {
                list.push(tri(0, 2, 3));
            }
        }
        for (colour, triangles) in shell_groups {
            let coloring = match (colour, shell_range) {
                (None, Some(r)) => {
                    shell_layer = Some(layers.len());
                    Coloring::Ramp(r)
                }
                (None, None) => Coloring::Plain(settings.shells.surface),
                (Some(c), _) => Coloring::Plain(c),
            };
            layers.push(Layer { triangles, segments: Vec::new(), coloring });
        }

        // Frames grouped the same way.
        let mut frame_groups: BTreeMap<Option<[u8; 3]>, Vec<Segment>> = BTreeMap::new();
        for (e, f) in g.frames.iter().enumerate() {
            let colour = match frame_look[e] {
                Look::Hidden => continue,
                Look::Normal if frame_contour(e) => None,
                Look::Normal => Some(settings.frames.line),
                Look::Halftone => Some(vv::halftone(settings.frames.line)),
                Look::Colour(c) => Some(c),
            };
            let (a, b) = (p(f[0]), p(f[1]));
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt().max(f32::EPSILON);
            let at = |sta: f64| {
                let t = (sta as f32 / len).clamp(0.0, 1.0);
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
            };
            let list = frame_groups.entry(colour).or_default();
            match frame_values.get(e).filter(|pts| pts.len() >= 2 && colour.is_none()) {
                Some(pts) => list.extend(pts.windows(2).map(|w| Segment { ends: [at(w[0].0), at(w[1].0)], values: [w[0].1, w[1].1] })),
                None => {
                    let v = frame_values.get(e).and_then(|pts| pts.first()).map_or(0.0, |x| x.1);
                    list.push(Segment { ends: [a, b], values: [v, v] });
                }
            }
        }
        for (colour, segments) in frame_groups {
            let coloring = match (colour, frame_range) {
                (None, Some(r)) => {
                    frame_layer = Some(layers.len());
                    Coloring::Ramp(r)
                }
                (None, None) => Coloring::Plain(settings.frames.line),
                (Some(c), _) => Coloring::Plain(c),
            };
            layers.push(Layer { triangles: Vec::new(), segments, coloring });
        }
        (layers, shell_layer, frame_layer)
    };
    #[cfg(not(feature = "ui"))]
    let (shell_layer, frame_layer): (Option<usize>, Option<usize>) = (None, None);

    // Frame stations as world points (m) for value snap – contoured frames only.
    let mut frame_points = Vec::new();
    for (e, f) in g.frames.iter().enumerate() {
        if !frame_contour(e) {
            continue;
        }
        let (Some(a), Some(b)) = (g.joints.get(f[0] as usize), g.joints.get(f[1] as usize)) else { continue };
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt().max(f64::EPSILON);
        for (sta, v) in frame_values.get(e).into_iter().flatten() {
            let t = (sta / len).clamp(0.0, 1.0);
            frame_points.push(([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t], *v));
        }
    }
    let snap = SnapData {
        shell_values: if shell_range.is_some() {
            (0..g.shells.len()).map(|e| if shell_contour(e) { shell_values[e] } else { [f32::NAN; 4] }).collect()
        } else {
            Vec::new()
        },
        frame_points,
        shell_unit: shell_unit.clone(),
        frame_unit: frame_unit.clone(),
        shell_shown: shell_look.iter().map(|l| *l != Look::Hidden).collect(),
        frame_shown: frame_look.iter().map(|l| *l != Look::Hidden).collect(),
    };

    Ok(Built {
        #[cfg(feature = "ui")]
        layers,
        shell_layer,
        frame_layer,
        snap,
        bounds,
        shell: shell_range.map(|range| Legend { title: format!("Shells: {}", label(&choice.load, SHELL_PANEL_NAMES[choice.shell.min(14)])), unit: shell_unit, range }),
        frame: frame_range.map(|range| Legend { title: format!("Frames: {}", label(&choice.load, FRAME_COMPONENTS[choice.frame.min(5)])), unit: frame_unit, range }),
        envelope: shell_env || frame_env,
        shell_values,
        frame_values,
    })
}

/// Avg. Nodal (rule P3): at each joint the mean of the corner values of the elements meeting
/// there (only those `counts(e)`), then written back to every corner at that joint.
fn average_at_joints(g: &Geometry, values: &mut [[f32; 4]], counts: impl Fn(usize) -> bool) {
    let mut sum = vec![0.0f64; g.joints.len()];
    let mut n = vec![0u32; g.joints.len()];
    for (e, s) in g.shells.iter().enumerate().filter(|(e, _)| counts(*e)) {
        for c in 0..4 {
            if let Some(j) = s.get(c).filter(|j| **j != u32::MAX).map(|j| *j as usize)
                && j < sum.len()
            {
                sum[j] += f64::from(values[e][c]);
                n[j] += 1;
            }
        }
    }
    for (e, s) in g.shells.iter().enumerate() {
        for c in 0..4 {
            if let Some(j) = s.get(c).filter(|j| **j != u32::MAX).map(|j| *j as usize)
                && n.get(j).is_some_and(|k| *k > 0)
            {
                values[e][c] = (sum[j] / f64::from(n[j])) as f32;
            }
        }
    }
}

/// The value of shell `component` (see [`SHELL_COMPONENTS`]) from the 8 stored values
/// F11 F22 F12 M11 M22 M12 V13 V23 (rule A5: the rest is computed here).
pub fn shell_component(component: usize, v: [f64; 8]) -> f64 {
    let principal = |a: f64, b: f64, ab: f64| {
        let c = (a + b) / 2.0;
        let r = (((a - b) / 2.0).powi(2) + ab * ab).sqrt();
        (c + r, c - r)
    };
    let abs_max = |(hi, lo): (f64, f64)| if hi.abs() >= lo.abs() { hi } else { lo };
    match component {
        0..=7 => v[component],
        8 => principal(v[0], v[1], v[2]).0,
        9 => principal(v[0], v[1], v[2]).1,
        10 => abs_max(principal(v[0], v[1], v[2])),
        11 => principal(v[3], v[4], v[5]).0,
        12 => principal(v[3], v[4], v[5]).1,
        13 => abs_max(principal(v[3], v[4], v[5])),
        _ => (v[6] * v[6] + v[7] * v[7]).sqrt(),
    }
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
    if (3..=5).contains(&component) || (11..=13).contains(&component) {
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

/// Shell value of `component` per element corner (n1..n4); columns: element, joint, step_type,
/// step_num, then the 8 stored values (F11 … V23).
fn shell_corner_values(g: &Geometry, b: &Block, component: usize, step: Step) -> Result<(Vec<[f32; 4]>, bool), String> {
    let (e, j, k, n) = (b.named("element"), b.named("joint"), b.named("step_type"), b.named("step_num"));
    let columns: Option<Vec<(usize, bool)>> = (4..12).map(|c| b.column(c)).collect();
    let (Some(e), Some(j), Some(k), Some(n), Some(columns)) = (e, j, k, n, columns) else {
        return Err("shell result block is missing columns".into());
    };
    let value = |r: usize| {
        let mut v = [0.0f64; 8];
        for (slot, c) in v.iter_mut().zip(&columns) {
            *slot = b.f64(*c, r);
        }
        shell_component(component, v)
    };
    let (keep, envelope) = step_filter(b, k, n, step);
    let mut out = vec![[0.0f32; 4]; g.shells.len()];
    for r in (0..b.rows).filter(|r| keep(*r)) {
        let (el, jt) = (b.u32(e, r) as usize, b.u32(j, r));
        if let (Some(s), Some(o)) = (g.shells.get(el), out.get_mut(el))
            && let Some(c) = s[..4].iter().position(|x| *x == jt)
        {
            o[c] = value(r) as f32;
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

/// A value under the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    /// World point (m) of the corner or station.
    pub at: [f64; 3],
    pub value: f32,
    pub unit: String,
}

/// The value under `cursor`: on the shell under the cursor closest to the viewer, its nearest
/// corner; or a frame station within `radius` pixels (frames win when at least as close).
/// `project` maps a world point to the screen. Linear in the number of elements (fine for the
/// selections of phase 1; a bounding-volume tree follows for millions).
pub fn snap(g: &Geometry, d: &SnapData, cursor: [f32; 2], project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>, eye: [f64; 3], radius: f32) -> Option<Snap> {
    let dist_eye = |p: [f64; 3]| ((p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2)).sqrt();
    let dist_px = |s: [f32; 2]| ((s[0] - cursor[0]).powi(2) + (s[1] - cursor[1]).powi(2)).sqrt();

    // Shell under the cursor, closest to the viewer.
    let mut shell: Option<(f64, usize)> = None;
    if !d.shell_values.is_empty() {
        let screen: Vec<Option<[f32; 2]>> = g.joints.iter().map(|p| project(*p)).collect();
        for (e, s) in g.shells.iter().enumerate() {
            let n = if s[3] == u32::MAX { 3 } else { 4 };
            let corner = |k: usize| screen.get(s[k] as usize).copied().flatten();
            let tris: &[[usize; 3]] = if n == 4 { &[[0, 1, 2], [0, 2, 3]] } else { &[[0, 1, 2]] };
            let inside = tris.iter().any(|t| match (corner(t[0]), corner(t[1]), corner(t[2])) {
                (Some(a), Some(b), Some(c)) => in_triangle(cursor, a, b, c),
                _ => false,
            });
            if inside && d.shell_values.get(e).is_some_and(|v| !v[0].is_nan()) {
                let depth = (0..n).filter_map(|k| g.joints.get(s[k] as usize)).map(|p| dist_eye(*p)).fold(f64::INFINITY, f64::min);
                if shell.is_none_or(|(d0, _)| depth < d0) {
                    shell = Some((depth, e));
                }
            }
        }
    }

    // Frame station within the radius, closest to the viewer.
    let mut frame: Option<(f64, usize)> = None;
    for (k, (p, _)) in d.frame_points.iter().enumerate() {
        if let Some(s) = project(*p)
            && dist_px(s) <= radius
        {
            let depth = dist_eye(*p);
            if frame.is_none_or(|(d0, _)| depth < d0) {
                frame = Some((depth, k));
            }
        }
    }

    match (shell, frame) {
        (_, Some((fd, k))) if shell.is_none_or(|(sd, _)| fd <= sd + 1e-6) => {
            let (p, v) = d.frame_points[k];
            Some(Snap { at: p, value: v, unit: d.frame_unit.clone() })
        }
        (Some((_, e)), _) => {
            let s = g.shells[e];
            let n = if s[3] == u32::MAX { 3 } else { 4 };
            let k = (0..n)
                .filter_map(|k| g.joints.get(s[k] as usize).and_then(|p| project(*p)).map(|sp| (k, dist_px(sp))))
                .min_by(|a, b| a.1.total_cmp(&b.1))?
                .0;
            let at = *g.joints.get(s[k] as usize)?;
            Some(Snap { at, value: d.shell_values.get(e)?[k], unit: d.shell_unit.clone() })
        }
        _ => None,
    }
}

fn in_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let side = |u: [f32; 2], v: [f32; 2]| (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0]);
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}
