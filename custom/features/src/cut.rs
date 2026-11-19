//! Cut Diagram (docs/16-cut-diagram.md): a cut through the shown shells and the shell result
//! along it – the crossings, the values (also normal to the cut: Fn, Mn, Vn), the resultant and
//! the table. Drawing, the mode's clicks and the Browser rows are in `ui_cut.rs`; this part has no
//! UI, so it is tested and runs headless.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};

use custom_bridge::{Ctx, Result, Value, json};

use crate::results_view::{self, Averaging, Choice};
use crate::transfer::{self, Geometry, ResultBlock};
use crate::units::Units;
use crate::{model, vv};

/// Components a cut can show: the 15 shell components, then the cut axes (R4).
pub const CUT_COMPONENTS: [&str; 20] = [
    "F11", "F22", "F12", "M11", "M22", "M12", "V13", "V23", "Fmax", "Fmin", "|F|max", "Mmax", "Mmin", "|M|max", "VMax", "Fn", "Fnt", "Mn", "Mnt", "Vn",
];

/// First cut-axes component (Fn).
pub const CUT_AXES: usize = 15;

/// How the cut plane stands (C2): the cut line extruded along global Z, or along a direction
/// (the view direction when drawn, or typed in Properties).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Direction {
    #[default]
    Vertical,
    Along([f64; 3]),
}

/// One cut (docs/16 C5).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Cut {
    pub name: String,
    /// The analysis model's file it belongs to (§4).
    pub model: String,
    /// The two points (m).
    pub p1: [f64; 3],
    pub p2: [f64; 3],
    pub direction: Direction,
    /// `None`: the window's shell component; else an index into [`CUT_COMPONENTS`].
    pub component: Option<usize>,
    /// Diagram height (screen px) of the largest value.
    pub scale_px: f32,
    pub text: bool,
    /// A value at every element edge, not only ends, max and min.
    pub all_values: bool,
    pub fill: bool,
}

impl Default for Cut {
    fn default() -> Self {
        Cut { name: String::new(), model: String::new(), p1: [0.0; 3], p2: [0.0; 3], direction: Direction::Vertical, component: None, scale_px: 60.0, text: true, all_values: false, fill: true }
    }
}

// ---- vectors ----

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn scaled(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|x| x * k)
}
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot(a, a).sqrt();
    (l.is_finite() && l > 1e-12).then(|| scaled(a, 1.0 / l))
}
fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// The cut's frame: along the line `u`, the extrusion `d`, the plane's normal `n`, the length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub d: [f64; 3],
    pub n: [f64; 3],
    pub length: f64,
}

impl Cut {
    /// `None` when the points are equal, not finite, or the line runs along the extrusion.
    pub fn frame(&self) -> Option<Frame> {
        if self.p1.iter().chain(&self.p2).any(|v| !v.is_finite()) {
            return None;
        }
        let d = match self.direction {
            Direction::Vertical => [0.0, 0.0, 1.0],
            Direction::Along(v) => unit(v)?,
        };
        let line = sub(self.p2, self.p1);
        let along = sub(line, scaled(d, dot(line, d)));
        let length = dot(along, along).sqrt();
        let u = unit(along)?;
        (length > 1e-9).then_some(Frame { origin: self.p1, u, d, n: cross(u, d), length })
    }
}

/// A piece of the cut inside one shell triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub shell: usize,
    /// Stations (m from point 1) of its two ends, increasing.
    pub s: [f64; 2],
    /// Its two ends (m).
    pub at: [[f64; 3]; 2],
    /// Per end, the weights of the element's corners n1..n4 (linear in the triangle).
    pub w: [[f64; 4]; 2],
}

/// Where a cut crosses the shells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Crossing {
    /// Sorted by station.
    pub pieces: Vec<Piece>,
    /// Shells lying in the cut plane (R5), skipped.
    pub in_plane: usize,
}

impl Crossing {
    pub fn shells(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.pieces.iter().map(|p| p.shell).collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// The cut through the shells shown (`shown[e]`; empty = all). One pass over the shells (§5).
pub fn crossing(g: &Geometry, frame: &Frame, shown: &[bool]) -> Crossing {
    let mut out = Crossing::default();
    let tol = 1e-9 * (1.0 + frame.length);
    for (e, s) in g.shells.iter().enumerate() {
        if shown.get(e).is_some_and(|v| !v) {
            continue;
        }
        let n = if s[3] == u32::MAX { 3 } else { 4 };
        let mut pts = [[0.0f64; 3]; 4];
        let mut ok = true;
        for (k, p) in pts.iter_mut().enumerate().take(n) {
            match g.joints.get(s[k] as usize) {
                Some(j) => *p = *j,
                None => ok = false,
            }
        }
        if !ok {
            continue;
        }
        let dist = pts.map(|p| dot(sub(p, frame.origin), frame.n));
        if dist[..n].iter().all(|d| d.abs() <= tol) {
            out.in_plane += 1;
            continue;
        }
        if dist[..n].iter().all(|d| *d > tol) || dist[..n].iter().all(|d| *d < -tol) {
            continue;
        }
        let tris: &[[usize; 3]] = if n == 3 { &[[0, 1, 2]] } else { &[[0, 1, 2], [0, 2, 3]] };
        for t in tris {
            if let Some(p) = triangle_piece(e, t, &pts, &dist, frame, tol) {
                out.pieces.push(p);
            }
        }
    }
    out.pieces.sort_by(|a, b| a.s[0].total_cmp(&b.s[0]).then(a.s[1].total_cmp(&b.s[1])));
    out
}

/// The cut inside one triangle (corners `t` of the element), clipped to the stations 0..length.
fn triangle_piece(e: usize, t: &[usize; 3], pts: &[[f64; 3]; 4], dist: &[f64; 4], f: &Frame, tol: f64) -> Option<Piece> {
    let mut ends: Vec<([f64; 3], [f64; 4])> = Vec::new();
    let mut push = |p: [f64; 3], w: [f64; 4]| {
        if !ends.iter().any(|(q, _)| dot(sub(*q, p), sub(*q, p)).sqrt() <= tol) {
            ends.push((p, w));
        }
    };
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let (da, db) = (dist[a], dist[b]);
        if da.abs() <= tol {
            let mut w = [0.0; 4];
            w[a] = 1.0;
            push(pts[a], w);
        }
        if (da < -tol && db > tol) || (da > tol && db < -tol) {
            let r = da / (da - db);
            let mut w = [0.0; 4];
            w[a] = 1.0 - r;
            w[b] = r;
            push(lerp(pts[a], pts[b], r), w);
        }
    }
    if ends.len() != 2 {
        return None;
    }
    let station = |p: [f64; 3]| dot(sub(p, f.origin), f.u);
    let (mut a, mut b) = (ends[0], ends[1]);
    let (mut sa, mut sb) = (station(a.0), station(b.0));
    if sa > sb {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut sa, &mut sb);
    }
    if sb < 0.0 || sa > f.length || sb - sa <= tol {
        return None;
    }
    let clip = |s: f64| {
        let r = ((s - sa) / (sb - sa)).clamp(0.0, 1.0);
        let w: [f64; 4] = std::array::from_fn(|c| a.1[c] + (b.1[c] - a.1[c]) * r);
        (lerp(a.0, b.0, r), w)
    };
    let (s0, s1) = (sa.max(0.0), sb.min(f.length));
    let (p0, w0) = clip(s0);
    let (p1, w1) = clip(s1);
    Some(Piece { shell: e, s: [s0, s1], at: [p0, p1], w: [w0, w1] })
}

/// cos θ, sin θ from shell `e`'s local axis 1 to the cut's normal, in the element plane (R4).
fn cut_angle(g: &Geometry, e: usize, n: [f64; 3]) -> Option<(f64, f64)> {
    let axes = g.shell_axes.get(e).and_then(transfer::local_axes)?;
    let m = unit(sub(n, scaled(axes[2], dot(n, axes[2]))))?;
    Some((dot(m, axes[0]), dot(m, axes[1])))
}

/// The value of `component` ([`CUT_COMPONENTS`]) from the 8 values; the cut axes need the angle.
pub fn value(component: usize, v: [f64; 8], angle: Option<(f64, f64)>) -> Option<f64> {
    if component < CUT_AXES {
        return Some(results_view::shell_component(component, v));
    }
    let (c, s) = angle?;
    let normal = |a: f64, b: f64, ab: f64| a * c * c + b * s * s + 2.0 * ab * s * c;
    let twist = |a: f64, b: f64, ab: f64| (b - a) * s * c + ab * (c * c - s * s);
    Some(match component - CUT_AXES {
        0 => normal(v[0], v[1], v[2]),
        1 => twist(v[0], v[1], v[2]),
        2 => normal(v[3], v[4], v[5]),
        3 => twist(v[3], v[4], v[5]),
        _ => v[6] * c + v[7] * s,
    })
}

/// Scale (SI per display unit) and unit text of a cut component.
pub fn unit_of(component: usize, u: Units) -> (f64, String) {
    let like = match component {
        c if c < CUT_AXES => c,
        15 | 16 => 0,
        17 | 18 => 3,
        _ => 6,
    };
    results_view::shell_unit(like, u)
}

/// Whether ∫ value ds means something (C7): per-length components, not principal values.
pub fn integrable(component: usize) -> bool {
    !(8..CUT_AXES).contains(&component)
}

/// One station of the diagram.
#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    pub s: f64,
    pub at: [f64; 3],
    pub shell: usize,
    /// In display units.
    pub value: f64,
}

/// The diagram of one cut for one result (C3, C6, C7).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagram {
    pub component: usize,
    pub unit: String,
    /// Two stations per piece (entry, exit): jumps between elements stay (R3).
    pub pieces: Vec<[Station; 2]>,
    /// ∫ value ds in display units (value unit × length unit) and its unit.
    pub resultant: Option<(f64, String)>,
    pub frame: Option<Frame>,
    /// Notes: shells in the plane, shells without local axes, no result.
    pub notes: Vec<String>,
}

impl Diagram {
    pub fn max(&self) -> Option<&Station> {
        self.pieces.iter().flatten().max_by(|a, b| a.value.total_cmp(&b.value))
    }
    pub fn min(&self) -> Option<&Station> {
        self.pieces.iter().flatten().min_by(|a, b| a.value.total_cmp(&b.value))
    }
    pub fn largest(&self) -> f64 {
        self.pieces.iter().flatten().map(|p| p.value.abs()).fold(0.0, f64::max)
    }
}

/// The diagram from the crossings and the 8 values per corner of the crossed shells (SI).
pub fn evaluate(g: &Geometry, cut: &Cut, frame: &Frame, crossing: &Crossing, tensors: &HashMap<usize, [[f64; 8]; 4]>, component: usize, units: Units) -> Diagram {
    let (scale, unit_text) = unit_of(component, units);
    let mut d = Diagram { component, unit: unit_text.clone(), frame: Some(*frame), ..Default::default() };
    let mut no_axes = 0usize;
    let mut no_result = 0usize;
    for p in &crossing.pieces {
        let Some(t) = tensors.get(&p.shell) else {
            no_result += 1;
            continue;
        };
        let angle = cut_angle(g, p.shell, frame.n);
        let at_end = |k: usize| {
            let v: [f64; 8] = std::array::from_fn(|i| (0..4).map(|c| p.w[k][c] * t[c][i]).sum());
            value(component, v, angle).map(|x| Station { s: p.s[k], at: p.at[k], shell: p.shell, value: x / scale })
        };
        match (at_end(0), at_end(1)) {
            (Some(a), Some(b)) => d.pieces.push([a, b]),
            _ => no_axes += 1,
        }
    }
    if crossing.in_plane > 0 {
        d.notes.push(format!("{} shells lie in the cut plane and are skipped", crossing.in_plane));
    }
    if no_axes > 0 {
        d.notes.push(format!("{no_axes} pieces left out: their shells have no local axes"));
    }
    if no_result > 0 {
        d.notes.push(format!("{no_result} pieces without a result for this load"));
    }
    if crossing.pieces.is_empty() {
        d.notes.push("the cut crosses no shown shell".into());
    }
    if integrable(component) && !d.pieces.is_empty() {
        let length = units.length.to_si();
        let sum: f64 = d.pieces.iter().map(|[a, b]| (a.value + b.value) / 2.0 * (b.s - a.s) / length).sum();
        let base = unit_text.rsplit_once('/').map(|(u, _)| u.to_string()).unwrap_or(unit_text);
        d.resultant = Some((sum, base));
    }
    let _ = cut;
    d
}

/// The table (C6): station (display length), x, y, z (display length), element, value.
pub fn table(g: &Geometry, d: &Diagram, units: Units) -> Vec<(f64, [f64; 3], String, f64)> {
    let l = units.length.to_si();
    d.pieces
        .iter()
        .flatten()
        .map(|st| (st.s / l, st.at.map(|x| x / l), g.shell_names.get(st.shell).cloned().unwrap_or_default(), st.value))
        .collect()
}

/// The table as text: tab-separated (for Excel) or comma-separated (CSV).
pub fn table_text(g: &Geometry, d: &Diagram, units: Units, sep: char) -> String {
    let len = units.length.symbol();
    let mut out = format!("Station [{len}]{sep}X [{len}]{sep}Y [{len}]{sep}Z [{len}]{sep}Element{sep}{} [{}]\n", CUT_COMPONENTS.get(d.component).copied().unwrap_or(""), d.unit);
    for (s, at, name, v) in table(g, d, units) {
        out.push_str(&format!("{s:.4}{sep}{:.4}{sep}{:.4}{sep}{:.4}{sep}{name}{sep}{v:.6}\n", at[0], at[1], at[2]));
    }
    if let (Some(hi), Some(lo)) = (d.max(), d.min()) {
        out.push_str(&format!("Max{sep}{sep}{sep}{sep}{sep}{:.6}\nMin{sep}{sep}{sep}{sep}{sep}{:.6}\n", hi.value, lo.value));
    }
    if let Some((r, u)) = &d.resultant {
        out.push_str(&format!("Resultant [{u}]{sep}{sep}{sep}{sep}{sep}{r:.6}\n"));
    }
    out
}

/// The 8 values per corner of `elements` for result `choice` (load, step, averaging) (R2).
pub fn tensors(g: &Geometry, blocks: &[ResultBlock], choice: &Choice, shown: &[bool], elements: &[usize]) -> std::result::Result<HashMap<usize, [[f64; 8]; 4]>, String> {
    let Some(load) = &choice.load else { return Ok(HashMap::new()) };
    let Some(block) = blocks.iter().find(|b| &b.name == load && b.family == "shell_results") else { return Ok(HashMap::new()) };
    let counts = |e: usize| !choice.active_only || shown.get(e).copied().unwrap_or(true);
    results_view::shell_corner_tensors(g, block, choice.step, choice.averaging == Averaging::Nodal, &counts, elements)
}

/// The component a cut shows in a window showing `choice`.
pub fn component_of(cut: &Cut, choice: &Choice) -> usize {
    cut.component.filter(|c| *c < CUT_COMPONENTS.len()).unwrap_or(choice.shell.min(CUT_AXES - 1))
}

/// Everything at once (the commands and the tests): crossings, values, diagram.
pub fn compute(g: &Geometry, blocks: &[ResultBlock], cut: &Cut, choice: &Choice, shown: &[bool], units: Units) -> std::result::Result<Diagram, String> {
    let frame = cut.frame().ok_or("the cut's points are equal or not valid")?;
    let c = crossing(g, &frame, shown);
    let t = tensors(g, blocks, choice, shown, &c.shells())?;
    Ok(evaluate(g, cut, &frame, &c, &t, component_of(cut, choice), units))
}

// ---- the cuts (kept in Factory's file, §4) ----

static CUTS: RwLock<Vec<Cut>> = RwLock::new(Vec::new());
static VERSION: AtomicU64 = AtomicU64::new(0);

fn changed() {
    VERSION.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "ui")]
    results_view::repaint();
}

/// Changes with every edit (for saving and caches).
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// Every cut of every model (for saving).
pub fn all() -> Vec<Cut> {
    CUTS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Put back the cuts as stored.
pub fn set_all(cuts: Vec<Cut>) {
    *CUTS.write().unwrap_or_else(PoisonError::into_inner) = cuts;
    changed();
}

/// The model the cuts now belong to.
pub fn current_model() -> String {
    model::read(|f| f.model.file.clone())
}

/// The cuts of the current model, in the order made.
pub fn cuts() -> Vec<Cut> {
    let m = current_model();
    all().into_iter().filter(|c| c.model == m).collect()
}

pub fn get(name: &str) -> Option<Cut> {
    cuts().into_iter().find(|c| c.name == name)
}

fn write<R>(f: impl FnOnce(&mut Vec<Cut>, &str) -> R) -> R {
    let m = current_model();
    let r = f(&mut CUTS.write().unwrap_or_else(PoisonError::into_inner), &m);
    changed();
    r
}

/// A new cut from `p1` to `p2`; its name (`Cut 1`, … or `name` when free). `None` when the points
/// make no cut.
pub fn add(name: Option<&str>, p1: [f64; 3], p2: [f64; 3], direction: Direction) -> Option<String> {
    let mut cut = Cut { p1, p2, direction, model: current_model(), ..Cut::default() };
    cut.frame()?;
    let taken: Vec<String> = cuts().into_iter().map(|c| c.name).collect();
    cut.name = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if !taken.iter().any(|t| t == n) => n.to_string(),
        Some(_) => return None,
        None => (1..).map(|i| format!("Cut {i}")).find(|c| !taken.contains(c)).unwrap_or_default(),
    };
    let name = cut.name.clone();
    write(|all, _| all.push(cut));
    Some(name)
}

/// Replace cut `name` with `cut` (Properties, Redraw). `false`: no such cut, or no valid cut.
pub fn replace(name: &str, mut cut: Cut) -> bool {
    if cut.frame().is_none() || cut.name.trim().is_empty() || (cut.name != name && get(&cut.name).is_some()) {
        return false;
    }
    write(|all, m| match all.iter_mut().find(|c| c.model == m && c.name == name) {
        Some(c) => {
            cut.model = m.to_string();
            *c = cut;
            true
        }
        None => false,
    })
}

pub fn rename(old: &str, new: &str) -> bool {
    let new = new.trim();
    match get(old) {
        Some(c) if !new.is_empty() => replace(old, Cut { name: new.to_string(), ..c }),
        _ => false,
    }
}

pub fn delete(name: &str) -> bool {
    write(|all, m| {
        let before = all.len();
        all.retain(|c| !(c.model == m && c.name == name));
        all.len() != before
    })
}

/// Whether the active view shows cut `name` (R6).
pub fn shown_in_view(name: &str) -> bool {
    !vv::settings().cuts_hidden.iter().any(|n| n == name)
}

/// Show or hide cut `name` in the active view (its V/G settings: a template rules it, R6).
pub fn show_in_view(name: &str, shown: bool) {
    let mut s = vv::settings();
    s.cuts_hidden.retain(|n| n != name);
    if !shown {
        s.cuts_hidden.push(name.to_string());
    }
    vv::set_quiet(s);
    changed();
}

// ---- Cut Diagram mode (C1) ----

/// The mode: on, the cut being redrawn (or a new one), the first point when clicked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mode {
    pub redraw: Option<String>,
    pub first: Option<[f64; 3]>,
}

static MODE: LazyLock<Mutex<Option<Mode>>> = LazyLock::new(|| Mutex::new(None));

pub fn mode() -> Option<Mode> {
    MODE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

pub fn set_mode(m: Option<Mode>) {
    *MODE.lock().unwrap_or_else(PoisonError::into_inner) = m;
    #[cfg(feature = "ui")]
    results_view::repaint();
}

/// A click in the mode at `p` (m), with the view direction `view_dir` (for `Along` cuts only).
/// The second click makes (or redraws) the cut and leaves the mode; returns its name.
pub fn click(p: [f64; 3], vertical: bool, view_dir: [f64; 3]) -> Option<String> {
    let mut m = mode()?;
    let Some(first) = m.first else {
        m.first = Some(p);
        set_mode(Some(m));
        return None;
    };
    let direction = if vertical { Direction::Vertical } else { Direction::Along(view_dir) };
    let made = match &m.redraw {
        Some(name) => get(name).and_then(|c| replace(name, Cut { p1: first, p2: p, direction, ..c }).then(|| name.clone())),
        None => add(None, first, p, direction),
    };
    if made.is_some() {
        set_mode(None);
    }
    made
}

// ---- commands (R8) ----

fn point(params: &Value, key: &str) -> Result<[f64; 3]> {
    let v = params.get(key).and_then(Value::as_array).filter(|a| a.len() == 3).ok_or_else(|| custom_bridge::Error(format!("give {key} as [x, y, z] in m")))?;
    let p: Vec<f64> = v.iter().filter_map(Value::as_f64).filter(|x| x.is_finite()).collect();
    <[f64; 3]>::try_from(p).map_err(|_| custom_bridge::Error(format!("{key}: three finite numbers")))
}

fn cut_param(params: &Value) -> Result<Cut> {
    let name = params.get("cut").and_then(Value::as_str).ok_or_else(|| custom_bridge::Error("give the cut's name".into()))?;
    get(name).ok_or_else(|| custom_bridge::Error(format!("no cut {name:?}")))
}

fn report() -> Result<Value> {
    Ok(json!({ "cuts": cuts().iter().map(|c| json!({ "name": c.name, "p1": c.p1, "p2": c.p2, "shown": shown_in_view(&c.name) })).collect::<Vec<_>>() }))
}

/// `factory.cut.mode`: Cut Diagram mode on (off with `on: false`).
pub fn mode_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let on = params.get("on").and_then(Value::as_bool).unwrap_or(true);
    set_mode(on.then(Mode::default));
    Ok(json!({ "mode": on }))
}

/// `factory.cut.new`: a cut from `p1` to `p2` (m); `direction`: "vertical" (default) or [x, y, z].
pub fn new_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let (p1, p2) = (point(params, "p1")?, point(params, "p2")?);
    let direction = match params.get("direction") {
        None => Direction::Vertical,
        Some(Value::String(s)) if s.eq_ignore_ascii_case("vertical") => Direction::Vertical,
        Some(v) => Direction::Along(point(&json!({ "d": v }), "d")?),
    };
    let name = params.get("name").and_then(Value::as_str);
    let made = add(name, p1, p2, direction).ok_or_else(|| custom_bridge::Error("no cut: equal points, the line along the direction, or the name is taken".into()))?;
    Ok(json!({ "cut": made }))
}

pub fn rename_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let c = cut_param(params)?;
    let new = params.get("name").and_then(Value::as_str).unwrap_or("");
    if !rename(&c.name, new) {
        return Err(custom_bridge::Error(format!("cannot rename {:?} to {new:?}", c.name)));
    }
    report()
}

pub fn delete_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let c = cut_param(params)?;
    delete(&c.name);
    report()
}

/// `factory.cut.show`: show or hide a cut in the active view.
pub fn show_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let c = cut_param(params)?;
    show_in_view(&c.name, params.get("shown").and_then(Value::as_bool).unwrap_or(true));
    report()
}

/// `factory.cut.set`: component (name, or "window"), scale (px), text, fill, all_values.
pub fn set_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let mut c = cut_param(params)?;
    if let Some(name) = params.get("component").and_then(Value::as_str) {
        c.component = if name.eq_ignore_ascii_case("window") {
            None
        } else {
            Some(CUT_COMPONENTS.iter().position(|x| x.eq_ignore_ascii_case(name)).ok_or_else(|| custom_bridge::Error(format!("unknown component {name:?}")))?)
        };
    }
    if let Some(px) = params.get("scale").and_then(Value::as_f64) {
        if !(px.is_finite() && px > 0.0 && px <= 2000.0) {
            return Err(custom_bridge::Error("scale: 0 < px ≤ 2000".into()));
        }
        c.scale_px = px as f32;
    }
    for (key, slot) in [("text", &mut c.text), ("fill", &mut c.fill), ("all_values", &mut c.all_values)] {
        if let Some(b) = params.get(key).and_then(Value::as_bool) {
            *slot = b;
        }
    }
    let name = c.name.clone();
    replace(&name, c);
    report()
}

/// `factory.cut.table`: the table of a cut for the active window's result.
pub fn table_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let c = cut_param(params)?;
    let g = transfer::geometry();
    let units = model::read(|f| f.units);
    let choice = results_view::shown().choice;
    let snap = results_view::snap_data();
    let d = compute(&g, &transfer::results(), &c, &choice, &snap.shell_shown, units).map_err(custom_bridge::Error)?;
    let rows: Vec<Value> = table(&g, &d, units).into_iter().map(|(s, at, e, v)| json!({ "station": s, "at": at, "element": e, "value": v })).collect();
    Ok(json!({
        "cut": c.name,
        "component": CUT_COMPONENTS.get(d.component),
        "unit": d.unit,
        "rows": rows,
        "max": d.max().map(|s| s.value),
        "min": d.min().map(|s| s.value),
        "resultant": d.resultant.as_ref().map(|r| json!({ "value": r.0, "unit": r.1 })),
        "notes": d.notes,
    }))
}

// ---- computed diagrams, per window (§5) ----

type Key = (String, usize, usize);
/// A computed diagram, or why there is none.
type Computed = Arc<std::result::Result<Diagram, String>>;

static DIAGRAMS: LazyLock<Mutex<HashMap<Key, Option<Computed>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The diagram of `cut` for a window showing `choice` with these shells shown – computed off the
/// window thread; `None` while it is being computed (the caller draws nothing yet).
pub fn diagram_for(cut: &Cut, choice: &Choice, snap: &Arc<results_view::SnapData>) -> Option<Computed> {
    let g = transfer::geometry();
    let units = model::read(|f| f.units);
    let key: Key = (format!("{cut:?}{choice:?}{units:?}"), Arc::as_ptr(&g) as usize, Arc::as_ptr(snap) as usize);
    let mut map = DIAGRAMS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(v) = map.get(&key) {
        return v.clone();
    }
    if map.len() > 64 {
        map.clear();
    }
    map.insert(key.clone(), None);
    drop(map);
    let (cut, choice, snap) = (cut.clone(), choice.clone(), snap.clone());
    std::thread::spawn(move || {
        let d = compute(&g, &transfer::results(), &cut, &choice, &snap.shell_shown, units);
        DIAGRAMS.lock().unwrap_or_else(PoisonError::into_inner).insert(key, Some(Arc::new(d)));
        #[cfg(feature = "ui")]
        results_view::repaint();
    });
    None
}
