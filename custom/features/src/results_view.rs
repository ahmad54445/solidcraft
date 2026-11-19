//! Which result the 3D view shows, and building its coloured elements (docs/10-viewport-results.md).
//!
//! Building runs on a worker thread (rule V1, P1); the UI only reads [`shown`]. Values stay SI in
//! the result store and are scaled to the display units here. Derived components (principal
//! values, VMax) and nodal averages are computed in one pass over the flat columns (P2, P3).

#[cfg(feature = "ui")]
use std::collections::HashMap;
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

/// Local axes 1, 2, 3: red, green, blue (as CSiBridge).
pub const AXIS_COLOURS: [[u8; 3]; 3] = [[220, 50, 40], [40, 170, 60], [40, 90, 220]];

/// Envelope side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Step {
    #[default]
    Max,
    Min,
}

/// Value at a shell joint: the element's own (may jump between elements) or the average of all
/// elements meeting there (smooth).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Averaging {
    #[default]
    Element,
    Nodal,
}

/// What the user chose.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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
    /// Per shell: coloured by the result (not halftone / colour override) – tells "no result"
    /// from "not contoured" for a shell without a value (docs/10 §1.1).
    pub shell_contoured: Vec<bool>,
    /// The shown shell component and load, for the snap's second line (e.g. "Mxx · DC").
    pub shell_component: String,
    /// How shells are drawn (shrink, extrude): the snap follows the drawn surface.
    pub view: vv::ViewOptions,
}

/// The values of the shown result, for value snap.
pub fn snap_data() -> Arc<SnapData> {
    SNAP.read().unwrap_or_else(PoisonError::into_inner).clone()
}
static REPAINT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn shown() -> Shown {
    SHOWN.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Changes when the active display window changes (docs/14): a build started for another window
/// is not swapped into this one.
static EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The active display window's result, kept while another window is active (docs/14).
pub fn park() -> (Shown, Arc<SnapData>) {
    EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (shown(), snap_data())
}

/// Make a window's kept result the shown one. A build that was running for it is started again.
pub fn unpark(shown: Shown, snap: Arc<SnapData>) {
    EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let busy = shown.busy;
    *SHOWN.write().unwrap_or_else(PoisonError::into_inner) = Shown { busy: false, ..shown };
    *SNAP.write().unwrap_or_else(PoisonError::into_inner) = snap;
    if busy {
        refresh();
    }
    repaint();
}

/// How a worker asks the UI to draw again (set once by the UI).
pub fn set_repaint(f: Box<dyn Fn() + Send + Sync>) {
    let _ = REPAINT.set(f);
}

pub(crate) fn repaint() {
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
    let epoch = EPOCH.load(std::sync::atomic::Ordering::Relaxed);
    std::thread::spawn(move || {
        let outcome = build_with(&transfer::geometry(), &transfer::results(), &choice, units, &settings);
        // A newer choice may have been made meanwhile: only the latest one is shown; another
        // display window may have become active (docs/14).
        let mut s = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
        if s.choice != choice || s.units != Some(units) || vv::settings() != settings || EPOCH.load(std::sync::atomic::Ordering::Relaxed) != epoch {
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

/// Set the choice and the typed legend ranges without building (a window restored before a model
/// is there, docs/14 §10.4); [`show_kept_or_first`] builds it after the transfer.
pub fn set_choice_quiet(choice: Choice, manual_shell: Option<[f32; 2]>, manual_frame: Option<[f32; 2]>) {
    let mut s = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
    s.choice = choice;
    s.manual_shell = manual_shell;
    s.manual_frame = manual_frame;
}

/// Show `choice` with these typed legend ranges (a view template applied, docs/14 §10.1); a load
/// the model has not is replaced by the first one.
pub fn select_with_ranges(mut choice: Choice, manual_shell: Option<[f32; 2]>, manual_frame: Option<[f32; 2]>, units: Units) {
    let loads = loads();
    if choice.load.as_ref().is_none_or(|l| !loads.contains(l)) {
        choice.load = loads.into_iter().next();
    }
    {
        let mut s = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
        s.manual_shell = manual_shell;
        s.manual_frame = manual_frame;
    }
    select(choice, units);
}

/// After a transfer: the shown choice again when the new model has its load (a window keeps its
/// result, docs/14 R4), else the first load.
pub fn show_kept_or_first() {
    let s = shown();
    if s.choice.load.as_ref().is_some_and(|l| loads().contains(l)) {
        select(s.choice, crate::model::read(|f| f.units));
    } else {
        show_first();
    }
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
    build_hidden(g, blocks, choice, units, settings, None)
}

/// [`build_with`] with these elements hidden by Hide / Isolate (an inactive display window's own,
/// docs/14) instead of the active window's.
pub fn build_hidden(g: &Geometry, blocks: &[ResultBlock], choice: &Choice, units: Units, settings: &Settings, hidden: Option<&crate::selection::Set>) -> Result<Built, String> {
    let find = |family: &str| choice.load.as_ref().and_then(|l| blocks.iter().find(|b| &b.name == l && b.family == family));
    let shell_block = find("shell_results");
    let frame_block = find("frame_results");
    let (mut shell_look, mut frame_look) = vv::looks(g, settings);
    match hidden {
        Some(h) => crate::visibility::apply_set(h, &mut shell_look, &mut frame_look),
        None => crate::visibility::apply(&mut shell_look, &mut frame_look), // Hide / Isolate (docs/13 §8)
    }

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

        // Shells grouped by how they are coloured; their shape follows View Options (docs/12 §2.2).
        let opts = &settings.view;
        let mut shell_groups: BTreeMap<Option<[u8; 3]>, (Vec<Triangle>, Vec<Segment>)> = BTreeMap::new();
        let mut edges: Vec<Segment> = Vec::new();
        for (e, s) in g.shells.iter().enumerate() {
            let colour = match shell_look[e] {
                Look::Hidden => continue,
                Look::Normal if shell_contour(e) => None,
                Look::Normal => Some(settings.shells.surface),
                Look::Halftone => Some(vv::halftone(settings.shells.surface)),
                Look::Colour(c) => Some(c),
            };
            let v = shell_values.get(e).copied().unwrap_or([0.0; 4]);
            let n = if s[3] == u32::MAX { 3 } else { 4 };
            let thickness = g.shell_thickness.get(s[4] as usize).copied().unwrap_or(0.0);
            let faces = shell_faces(g, e, n, opts, thickness);
            let (tris, segs) = shell_groups.entry(colour).or_default();
            for face in &faces {
                if opts.fill {
                    tris.push(Triangle { corners: [face[0], face[1], face[2]], values: [v[0], v[1], v[2]] });
                    if n == 4 {
                        tris.push(Triangle { corners: [face[0], face[2], face[3]], values: [v[0], v[2], v[3]] });
                    }
                } else {
                    for k in 0..n {
                        segs.push(Segment { ends: [face[k], face[(k + 1) % n]], values: [v[k], v[(k + 1) % n]] });
                    }
                }
                if opts.edges && opts.fill {
                    for k in 0..n {
                        edges.push(Segment { ends: [face[k], face[(k + 1) % n]], values: [0.0, 0.0] });
                    }
                }
            }
            // Extruded slab: the four side faces between bottom and top.
            if faces.len() == 2 && opts.fill {
                let (b, t) = (faces[0], faces[1]);
                for k in 0..n {
                    let k2 = (k + 1) % n;
                    tris.push(Triangle { corners: [b[k], b[k2], t[k2]], values: [v[k], v[k2], v[k2]] });
                    tris.push(Triangle { corners: [b[k], t[k2], t[k]], values: [v[k], v[k2], v[k]] });
                }
            }
        }
        for (colour, (triangles, segments)) in shell_groups {
            let coloring = match (colour, shell_range) {
                (None, Some(r)) => {
                    shell_layer = Some(layers.len());
                    Coloring::Ramp(r)
                }
                (None, None) => Coloring::Plain(settings.shells.surface),
                (Some(c), _) => Coloring::Plain(c),
            };
            layers.push(Layer { triangles, segments, coloring });
        }
        if !edges.is_empty() {
            layers.push(Layer { triangles: Vec::new(), segments: edges, coloring: Coloring::Plain(settings.shells.line) });
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
            // Shrink Objects: the drawn part keeps SHRINK of the length around the midpoint.
            let (t0, t1) = if settings.view.shrink { ((1.0 - vv::SHRINK as f32) / 2.0, (1.0 + vv::SHRINK as f32) / 2.0) } else { (0.0, 1.0) };
            let at = |sta: f64| {
                let t = t0 + (t1 - t0) * (sta as f32 / len).clamp(0.0, 1.0);
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
            };
            let list = frame_groups.entry(colour).or_default();
            match frame_values.get(e).filter(|pts| pts.len() >= 2 && colour.is_none()) {
                Some(pts) => list.extend(pts.windows(2).map(|w| Segment { ends: [at(w[0].0), at(w[1].0)], values: [w[0].1, w[1].1] })),
                None => {
                    let v = frame_values.get(e).and_then(|pts| pts.first()).map_or(0.0, |x| x.1);
                    list.push(Segment { ends: [at(0.0), at(f64::from(len))], values: [v, v] });
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
        // Local axes (Visibility/Graphics › Local Axes) as 3D lines in the scene: drawn with the
        // model's depth (hidden behind surfaces), built once per change – nothing per frame.
        let mut axes: [Vec<Segment>; 3] = Default::default();
        let mut add_triad = |origin: [f64; 3], length: f64, m: &[f64; 9]| {
            let Some(dirs) = crate::transfer::local_axes(m) else { return };
            let o = origin.map(|x| x as f32);
            for (k, d) in dirs.iter().enumerate() {
                let tip = [origin[0] + d[0] * length, origin[1] + d[1] * length, origin[2] + d[2] * length].map(|x| x as f32);
                axes[k].push(Segment { ends: [o, tip], values: [0.0, 0.0] });
            }
        };
        let dist = |a: [f64; 3], b: [f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        if settings.frames.visible && settings.frames.axes.visible {
            for (e, f) in g.frames.iter().enumerate() {
                if frame_look[e] == Look::Hidden {
                    continue;
                }
                let (Some(a), Some(b), Some(m)) = (g.joints.get(f[0] as usize), g.joints.get(f[1] as usize), g.frame_axes.get(e)) else { continue };
                let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
                add_triad(mid, 0.25 * dist(*a, *b), m);
            }
        }
        if settings.shells.visible && settings.shells.axes.visible {
            for (e, s) in g.shells.iter().enumerate() {
                if shell_look[e] == Look::Hidden {
                    continue;
                }
                let Some(m) = g.shell_axes.get(e) else { continue };
                let corners: Vec<[f64; 3]> = s[..4].iter().filter(|j| **j != u32::MAX).filter_map(|j| g.joints.get(*j as usize).copied()).collect();
                if corners.len() < 3 {
                    continue;
                }
                let n = corners.len() as f64;
                let c = [0, 1, 2].map(|i| corners.iter().map(|p| p[i]).sum::<f64>() / n);
                let size = dist(corners[0], corners[2]);
                // Lift the origin off the surface along axis 3 so the in-plane axes 1 and 2 do not
                // fight the surface for depth (on the top face when extruded).
                let thickness = g.shell_thickness.get(s[4] as usize).copied().unwrap_or(0.0);
                let lift = 0.01 * size + if settings.view.view_type == vv::ViewType::Extrude { thickness / 2.0 } else { 0.0 };
                let a3 = crate::transfer::local_axes(m).map_or([0.0; 3], |a| a[2]);
                add_triad([c[0] + a3[0] * lift, c[1] + a3[1] * lift, c[2] + a3[2] * lift], 0.25 * size, m);
            }
        }
        let half = settings.shells.axes.halftone || settings.frames.axes.halftone;
        for (segments, rgb) in axes.into_iter().zip(AXIS_COLOURS) {
            if !segments.is_empty() {
                layers.push(Layer { triangles: Vec::new(), segments, coloring: Coloring::Plain(if half { vv::halftone(rgb) } else { rgb }) });
            }
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
        shell_contoured: (0..g.shells.len()).map(|e| settings.contour && shell_look[e] == Look::Normal).collect(),
        shell_component: match &choice.load {
            Some(load) => format!("{} · {load}", SHELL_PANEL_NAMES[choice.shell.min(14)]),
            None => String::new(),
        },
        view: settings.view.clone(),
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

/// The faces of shell `e` as View Options draw it: its corners (shrunk around the centre for
/// Shrink Objects); for Extrude a bottom and a top face, half the thickness each way along local
/// axis 3 (from CSiBridge; the corner normal when the axes are missing).
#[cfg(feature = "ui")]
fn shell_faces(g: &Geometry, e: usize, n: usize, opts: &vv::ViewOptions, thickness: f64) -> Vec<[[f32; 3]; 4]> {
    let s = g.shells[e];
    let corner = |k: usize| g.joints.get(s[k] as usize).copied().unwrap_or([0.0; 3]);
    let mut pts: [[f64; 3]; 4] = [[0.0; 3]; 4];
    for (k, slot) in pts.iter_mut().enumerate().take(n) {
        *slot = corner(k);
    }
    if opts.shrink {
        let c = [0, 1, 2].map(|i| pts[..n].iter().map(|p| p[i]).sum::<f64>() / n as f64);
        for p in pts[..n].iter_mut() {
            for i in 0..3 {
                p[i] = c[i] + (p[i] - c[i]) * vv::SHRINK;
            }
        }
    }
    let as_f32 = |q: [[f64; 3]; 4]| q.map(|p| p.map(|x| x as f32));
    if opts.view_type != vv::ViewType::Extrude || thickness <= 0.0 {
        return vec![as_f32(pts)];
    }
    let normal = g.shell_axes.get(e).and_then(crate::transfer::local_axes).map(|a| a[2]).unwrap_or_else(|| {
        let (a, b, c) = (pts[0], pts[1], pts[2]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let x = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt().max(f64::EPSILON);
        x.map(|c| c / l)
    });
    let shift = |d: f64| pts.map(|p| [p[0] + normal[0] * d, p[1] + normal[1] * d, p[2] + normal[2] * d]);
    vec![as_f32(shift(-thickness / 2.0)), as_f32(shift(thickness / 2.0))]
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

/// The 8 stored values (F11 … V23, SI) per corner of the shells in `elements` – the Cut Diagram's
/// input (docs/16 R2). One pass over the block; only the wanted elements are kept. `nodal`: each
/// corner gets the mean at its joint over every element `counts(e)` meeting there (as the contour's
/// Avg. Nodal, rule P3).
pub(crate) fn shell_corner_tensors(g: &Geometry, block: &ResultBlock, step: Step, nodal: bool, counts: &dyn Fn(usize) -> bool, elements: &[usize]) -> Result<HashMap<usize, [[f64; 8]; 4]>, String> {
    let b = Block::read(block)?;
    let (e, j, k, n) = (b.named("element"), b.named("joint"), b.named("step_type"), b.named("step_num"));
    let columns: Option<Vec<(usize, bool)>> = (4..12).map(|c| b.column(c)).collect();
    let (Some(e), Some(j), Some(k), Some(n), Some(columns)) = (e, j, k, n, columns) else {
        return Err("shell result block is missing columns".into());
    };
    let read = |r: usize| {
        let mut v = [0.0f64; 8];
        for (slot, c) in v.iter_mut().zip(&columns) {
            *slot = b.f64(*c, r);
        }
        v
    };
    let (keep, _) = step_filter(&b, k, n, step);
    let mut out: HashMap<usize, [[f64; 8]; 4]> = elements.iter().filter(|e| **e < g.shells.len()).map(|e| (*e, [[0.0; 8]; 4])).collect();
    let mut joints: HashMap<u32, ([f64; 8], u32)> = HashMap::new();
    if nodal {
        for el in out.keys() {
            for jt in g.shells[*el][..4].iter().filter(|j| **j != u32::MAX) {
                joints.insert(*jt, ([0.0; 8], 0));
            }
        }
    }
    for r in (0..b.rows).filter(|r| keep(*r)) {
        let (el, jt) = (b.u32(e, r) as usize, b.u32(j, r));
        if nodal {
            if counts(el)
                && let Some((sum, count)) = joints.get_mut(&jt)
            {
                for (s, v) in sum.iter_mut().zip(read(r)) {
                    *s += v;
                }
                *count += 1;
            }
        } else if let (Some(o), Some(s)) = (out.get_mut(&el), g.shells.get(el))
            && let Some(c) = s[..4].iter().position(|x| *x == jt)
        {
            o[c] = read(r);
        }
    }
    if nodal {
        for (el, o) in out.iter_mut() {
            for (c, slot) in o.iter_mut().enumerate() {
                if let Some((sum, count)) = joints.get(&g.shells[*el][c]).filter(|(_, n)| *n > 0) {
                    *slot = sum.map(|v| v / f64::from(*count));
                }
            }
        }
    }
    Ok(out)
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

/// A value under the cursor (docs/10 §1.1).
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    /// World point (m): the corner, the station, or the point on the shell under the cursor.
    pub at: [f64; 3],
    /// The value there; NaN when there is none (see `note`).
    pub value: f32,
    pub unit: String,
    /// The shell under the cursor (`None` for a frame station).
    pub shell: Option<usize>,
    /// Snapped to a corner (within the radius) rather than a point inside the shell.
    pub corner: bool,
    /// Why there is no value: "no result" for this load case / component, or "not contoured"
    /// (halftone, colour override); `None` with a NaN value: no result chosen (contour off).
    pub note: Option<&'static str>,
}

/// Shells by screen cell (32 px), for finding the shell under the cursor without testing every
/// shell (rule S3). Build it once per camera (it projects every shown shell), query per hover.
pub struct SnapIndex {
    cell: f32,
    cells: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

/// The faces of shell `e` as drawn (View Options: shrink, extrude; docs/12 §2.2).
pub fn drawn_faces(g: &Geometry, d: &SnapData, e: usize) -> Vec<[[f32; 3]; 4]> {
    let s = g.shells[e];
    let n = if s[3] == u32::MAX { 3 } else { 4 };
    let thickness = g.shell_thickness.get(s[4] as usize).copied().unwrap_or(0.0);
    shell_faces(g, e, n, &d.view, thickness)
}

impl SnapIndex {
    pub fn new(g: &Geometry, d: &SnapData, project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>) -> Self {
        let cell = 32.0f32;
        let mut cells: std::collections::HashMap<(i32, i32), Vec<u32>> = std::collections::HashMap::new();
        for e in 0..g.shells.len() {
            if !d.shell_shown.get(e).copied().unwrap_or(true) {
                continue; // hidden: looked through (rule G5)
            }
            let n = if g.shells[e][3] == u32::MAX { 3 } else { 4 };
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for face in drawn_faces(g, d, e) {
                for p in &face[..n] {
                    if let Some(sp) = project(p.map(f64::from)) {
                        for i in 0..2 {
                            lo[i] = lo[i].min(sp[i]);
                            hi[i] = hi[i].max(sp[i]);
                        }
                    }
                }
            }
            if lo[0] > hi[0] {
                continue;
            }
            // A shell larger than the screen is kept in a bounded number of cells around it.
            let span = |a: f32, b: f32| ((a / cell).floor() as i32).max(-2048)..=((b / cell).floor() as i32).min(2048);
            for x in span(lo[0], hi[0]) {
                for y in span(lo[1], hi[1]) {
                    cells.entry((x, y)).or_default().push(e as u32);
                }
            }
        }
        SnapIndex { cell, cells }
    }

    fn at(&self, p: [f32; 2]) -> &[u32] {
        self.cells.get(&((p[0] / self.cell).floor() as i32, (p[1] / self.cell).floor() as i32)).map_or(&[], Vec::as_slice)
    }
}

/// The point of triangle `t` (world) that projects onto `cursor`: its barycentric (s, t) for
/// `t[0] + s (t[1] − t[0]) + t (t[2] − t[0])`. Projection is a homography on the plane, so a few
/// Newton steps from the screen barycentrics are exact (perspective and orthographic alike).
fn point_on_triangle(t: [[f64; 3]; 3], cursor: [f32; 2], start: [f64; 2], project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>) -> [f64; 2] {
    let at = |s: f64, u: f64| [0, 1, 2].map(|i| t[0][i] + s * (t[1][i] - t[0][i]) + u * (t[2][i] - t[0][i]));
    let screen = |s: f64, u: f64| project(at(s, u)).map(|p| [f64::from(p[0]), f64::from(p[1])]);
    let target = [f64::from(cursor[0]), f64::from(cursor[1])];
    let [mut s, mut u] = start;
    for _ in 0..6 {
        let (Some(f), Some(fs), Some(fu)) = (screen(s, u), screen(s + 1e-4, u), screen(s, u + 1e-4)) else { break };
        let r = [f[0] - target[0], f[1] - target[1]];
        if r[0].abs() + r[1].abs() < 1e-3 {
            break;
        }
        let (a, b) = ((fs[0] - f[0]) / 1e-4, (fu[0] - f[0]) / 1e-4);
        let (c, dd) = ((fs[1] - f[1]) / 1e-4, (fu[1] - f[1]) / 1e-4);
        let det = a * dd - b * c;
        if det.abs() < 1e-12 {
            break;
        }
        s -= (dd * r[0] - b * r[1]) / det;
        u -= (-c * r[0] + a * r[1]) / det;
    }
    [s.clamp(0.0, 1.0), u.clamp(0.0, 1.0)]
}

/// Screen barycentrics (s, t) of `p` in triangle a, b, c.
fn screen_barycentric(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> [f64; 2] {
    let (v0, v1, v2) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
    let den = f64::from(v0[0] * v1[1] - v1[0] * v0[1]);
    if den.abs() < 1e-12 {
        return [0.0, 0.0];
    }
    let s = f64::from(v2[0] * v1[1] - v1[0] * v2[1]) / den;
    let t = f64::from(v0[0] * v2[1] - v2[0] * v0[1]) / den;
    [s, t]
}

/// The value under `cursor` (docs/10 §1.1, rules S1–S4): on the shown shell under the cursor
/// closest to the viewer (ties: lower element number), its corner within `radius` px, else the
/// value at the cursor's point interpolated from the corner values on the drawn triangle – the
/// same interpolation the contour colour uses. A frame station within `radius` wins when at least
/// as close. A shell without a value still answers (NaN value and a note).
pub fn snap(g: &Geometry, d: &SnapData, cursor: [f32; 2], project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>, eye: [f64; 3], radius: f32) -> Option<Snap> {
    let index = SnapIndex::new(g, d, project);
    snap_indexed(g, d, &index, cursor, project, eye, radius)
}

/// [`snap`] with a [`SnapIndex`] built for the current camera.
pub fn snap_indexed(g: &Geometry, d: &SnapData, index: &SnapIndex, cursor: [f32; 2], project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>, eye: [f64; 3], radius: f32) -> Option<Snap> {
    let dist_eye = |p: [f64; 3]| ((p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2)).sqrt();
    let dist_px = |s: [f32; 2]| ((s[0] - cursor[0]).powi(2) + (s[1] - cursor[1]).powi(2)).sqrt();

    // Shell under the cursor, closest to the viewer: (depth, shell, world point, corner weights).
    let mut shell: Option<(f64, usize, [f64; 3], [f64; 4], [[f32; 3]; 4])> = None;
    for &e in index.at(cursor) {
        let e = e as usize;
        let n = if g.shells[e][3] == u32::MAX { 3 } else { 4 };
        let tris: &[[usize; 3]] = if n == 4 { &[[0, 1, 2], [0, 2, 3]] } else { &[[0, 1, 2]] };
        for face in drawn_faces(g, d, e) {
            let world = |k: usize| face[k].map(f64::from);
            let screen = |k: usize| project(world(k));
            for t in tris {
                let (Some(a), Some(b), Some(c)) = (screen(t[0]), screen(t[1]), screen(t[2])) else { continue };
                if !in_triangle(cursor, a, b, c) {
                    continue;
                }
                let tri = [world(t[0]), world(t[1]), world(t[2])];
                let [s, u] = point_on_triangle(tri, cursor, screen_barycentric(cursor, a, b, c), project);
                let p = [0, 1, 2].map(|i| tri[0][i] + s * (tri[1][i] - tri[0][i]) + u * (tri[2][i] - tri[0][i]));
                let mut w = [0.0; 4];
                w[t[0]] += 1.0 - s - u;
                w[t[1]] += s;
                w[t[2]] += u;
                let depth = dist_eye(p);
                let nearer = shell.as_ref().is_none_or(|(d0, e0, ..)| depth < *d0 - 1e-9 || ((depth - *d0).abs() <= 1e-9 && e < *e0));
                if nearer {
                    shell = Some((depth, e, p, w, face));
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
        (ref sh, Some((fd, k))) if sh.as_ref().is_none_or(|(sd, ..)| fd <= *sd + 1e-6) => {
            let (p, v) = d.frame_points[k];
            Some(Snap { at: p, value: v, unit: d.frame_unit.clone(), shell: None, corner: true, note: None })
        }
        (Some((_, e, p, w, face)), _) => {
            let n = if g.shells[e][3] == u32::MAX { 3 } else { 4 };
            let values = d.shell_values.get(e).copied();
            let note = match values {
                None => None, // no result chosen: the shell's name only
                Some(v) if v[..n].iter().any(|x| x.is_nan()) => {
                    Some(if d.shell_contoured.get(e).copied().unwrap_or(true) { "no result" } else { "not contoured" })
                }
                Some(_) => None,
            };
            let usable = values.filter(|_| note.is_none());
            // A corner within the radius: its own value (as before).
            let near = (0..n)
                .filter_map(|k| project(face[k].map(f64::from)).map(|sp| (k, dist_px(sp))))
                .filter(|(_, dpx)| *dpx <= radius)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let (at, value, corner) = match near {
                Some((k, _)) => (face[k].map(f64::from), usable.map_or(f32::NAN, |v| v[k]), true),
                None => {
                    let value = usable.map_or(f32::NAN, |v| (0..n).map(|k| w[k] * f64::from(v[k])).sum::<f64>() as f32);
                    (p, value, false)
                }
            };
            Some(Snap { at, value, unit: d.shell_unit.clone(), shell: Some(e), corner, note })
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

/// Whether a shown shell lies between `eye` and `p` (labels behind surfaces are not drawn,
/// docs/10 P4). Ray test against the shells' mid-surface triangles; `own` (the shell the label
/// belongs to) is skipped. Linear in the number of shells – run once per camera stop, not per frame.
pub fn hidden_by_shells(g: &Geometry, shown: &[bool], eye: [f64; 3], p: [f64; 3], own: Option<usize>) -> bool {
    let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
    let corner = |j: u32| g.joints.get(j as usize).copied();
    for (e, s) in g.shells.iter().enumerate() {
        if Some(e) == own || !shown.get(e).copied().unwrap_or(true) {
            continue;
        }
        let tris: &[[usize; 3]] = if s[3] == u32::MAX { &[[0, 1, 2]] } else { &[[0, 1, 2], [0, 2, 3]] };
        for t in tris {
            let (Some(a), Some(b), Some(c)) = (corner(s[t[0]]), corner(s[t[1]]), corner(s[t[2]])) else { continue };
            if let Some(t) = ray_triangle(eye, d, a, b, c)
                && t > 1e-6
                && t < 1.0 - 1e-6
            {
                return true;
            }
        }
    }
    false
}

/// Möller–Trumbore: the ray parameter t of `origin + t·dir` hitting triangle (a, b, c).
fn ray_triangle(origin: [f64; 3], dir: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    let sub = |u: [f64; 3], v: [f64; 3]| [u[0] - v[0], u[1] - v[1], u[2] - v[2]];
    let cross = |u: [f64; 3], v: [f64; 3]| [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let (e1, e2) = (sub(b, a), sub(c, a));
    let h = cross(dir, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-12 {
        return None;
    }
    let f = 1.0 / det;
    let s = sub(origin, a);
    let u = f * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = f * dot(dir, q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(f * dot(e2, q))
}

/// Fast "is a shown shell between the eye and this point?" for many points at one camera stop:
/// the shells' triangles are bucketed once into a screen grid, so each point is ray-tested only
/// against the triangles that cover its cell (instead of all of them).
pub struct Occluder<'a> {
    g: &'a Geometry,
    eye: [f64; 3],
    cell: f32,
    /// grid cell → (shell, triangle corners as joint indices)
    buckets: std::collections::HashMap<(i32, i32), Vec<(usize, [u32; 3])>>,
}

impl<'a> Occluder<'a> {
    /// `project` maps world points to the screen (`None` behind the camera); `cell` in px.
    pub fn new(g: &'a Geometry, shown: &[bool], eye: [f64; 3], project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>, cell: f32) -> Self {
        let screen: Vec<Option<[f32; 2]>> = g.joints.iter().map(|p| project(*p)).collect();
        let mut buckets: std::collections::HashMap<(i32, i32), Vec<(usize, [u32; 3])>> = std::collections::HashMap::new();
        for (e, s) in g.shells.iter().enumerate() {
            if !shown.get(e).copied().unwrap_or(true) {
                continue;
            }
            let tris: &[[usize; 3]] = if s[3] == u32::MAX { &[[0, 1, 2]] } else { &[[0, 1, 2], [0, 2, 3]] };
            for t in tris {
                let corners = [s[t[0]], s[t[1]], s[t[2]]];
                let pts: Option<Vec<[f32; 2]>> = corners.iter().map(|j| screen.get(*j as usize).copied().flatten()).collect();
                let Some(pts) = pts else { continue }; // partly behind the camera: cannot cover a visible point reliably
                let (x0, x1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
                let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
                let (cx0, cx1, cy0, cy1) = ((x0 / cell).floor() as i32, (x1 / cell).floor() as i32, (y0 / cell).floor() as i32, (y1 / cell).floor() as i32);
                if (cx1 - cx0 + 1) * (cy1 - cy0 + 1) > 4096 {
                    continue; // a huge sliver on screen: skip rather than flood the grid
                }
                for cx in cx0..=cx1 {
                    for cy in cy0..=cy1 {
                        buckets.entry((cx, cy)).or_default().push((e, corners));
                    }
                }
            }
        }
        Self { g, eye, cell, buckets }
    }

    /// Whether a shown shell (other than `own`) lies between the eye and `p`, which is at `screen`.
    pub fn hidden(&self, p: [f64; 3], screen: [f32; 2], own: Option<usize>) -> bool {
        let key = ((screen[0] / self.cell).floor() as i32, (screen[1] / self.cell).floor() as i32);
        let Some(list) = self.buckets.get(&key) else { return false };
        let d = [p[0] - self.eye[0], p[1] - self.eye[1], p[2] - self.eye[2]];
        let corner = |j: u32| self.g.joints.get(j as usize).copied();
        list.iter().any(|(e, c)| {
            Some(*e) != own
                && matches!((corner(c[0]), corner(c[1]), corner(c[2])), (Some(a), Some(b), Some(cc))
                    if ray_triangle(self.eye, d, a, b, cc).is_some_and(|t| t > 1e-6 && t < 1.0 - 1e-6))
        })
    }
}
