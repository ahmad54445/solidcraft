//! Cut Diagram (docs/16-cut-diagram.md): a cut through the shown shells and the shell result
//! along it – the crossings, the values (also normal to the cut: Fn, Mn, Vn), the resultant and
//! the table. Drawing, the mode's clicks and the Browser rows are in `ui_cut.rs`; this part has no
//! UI, so it is tested and runs headless.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// Phase 1's *along the view direction* (or typed): the line extruded along this direction.
    Along([f64; 3]),
    /// Horizontal, at the level of point 1 (§12.1).
    Horizontal,
    /// Through points 1, 2 and this third point (§12.1).
    Plane3([f64; 3]),
}

/// The plane type chosen in the mode (§12.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Vertical,
    Horizontal,
    ThreePoints,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Vertical, Kind::Horizontal, Kind::ThreePoints];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Vertical => "Vertical (Z)",
            Kind::Horizontal => "Horizontal",
            Kind::ThreePoints => "3 points",
        }
    }

    /// Clicks it takes.
    pub fn clicks(self) -> usize {
        if self == Kind::ThreePoints { 3 } else { 2 }
    }

    /// The kind a kept cut was made with (Redraw keeps it).
    pub fn of(d: Direction) -> Kind {
        match d {
            Direction::Horizontal => Kind::Horizontal,
            Direction::Plane3(_) => Kind::ThreePoints,
            _ => Kind::Vertical,
        }
    }
}

/// One cut (docs/16 C5).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Cut {
    pub name: String,
    /// Section ID (§14.1): fixed, given once, never reused in the model; 0 = not given yet.
    pub id: u32,
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
    /// Draw the values square to each element (local axis 3) instead of in the cut plane (§12.3).
    pub draw_normal: bool,
    /// *Only near the line*: shells within this band width (m) around the drawn line (§12.4).
    pub band: Option<f64>,
}

impl Default for Cut {
    fn default() -> Self {
        Cut { name: String::new(), id: 0, model: String::new(), p1: [0.0; 3], p2: [0.0; 3], direction: Direction::Vertical, component: None, scale_px: 60.0, text: true, all_values: false, fill: true, draw_normal: false, band: None }
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

/// The cut's frame (§12.2): along the line `u`, in the plane square to the line `d = n × u`, the
/// plane's normal `n`, the length, and the band around the line (§12.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub d: [f64; 3],
    pub n: [f64; 3],
    pub length: f64,
    pub band: Option<f64>,
}

impl Cut {
    /// `None` when the points are equal, not finite, or the line runs along the extrusion.
    pub fn frame(&self) -> Option<Frame> {
        if self.p1.iter().chain(&self.p2).any(|v| !v.is_finite()) {
            return None;
        }
        let band = self.band.filter(|b| b.is_finite() && *b > 0.0);
        let line = sub(self.p2, self.p1);
        // A line extruded along `e`: the line without its `e` part, normal u × e.
        let extruded = |e: [f64; 3]| -> Option<Frame> {
            let along = sub(line, scaled(e, dot(line, e)));
            let length = dot(along, along).sqrt();
            let u = unit(along)?;
            (length > 1e-9).then_some(Frame { origin: self.p1, u, d: e, n: cross(u, e), length, band })
        };
        const Z: [f64; 3] = [0.0, 0.0, 1.0];
        match self.direction {
            Direction::Vertical => extruded(Z),
            Direction::Along(v) => extruded(unit(v)?),
            Direction::Horizontal => {
                let f = extruded(Z)?;
                Some(Frame { n: Z, d: cross(Z, f.u), ..f })
            }
            Direction::Plane3(p3) => {
                if p3.iter().any(|v| !v.is_finite()) {
                    return None;
                }
                let length = dot(line, line).sqrt();
                let u = unit(line)?;
                let normal = cross(line, sub(p3, self.p1));
                // 3 points (nearly) in one line give no plane.
                if dot(normal, normal).sqrt() <= 1e-9 * length * length.max(1.0) {
                    return None;
                }
                let n = unit(normal)?;
                (length > 1e-9).then_some(Frame { origin: self.p1, u, d: cross(n, u), n, length, band })
            }
        }
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

/// One shell's part of the section (§14.2): where the plane cuts it, widened to its thickness –
/// a quad in the cut plane (m).
#[derive(Clone, Debug, PartialEq)]
pub struct Slab {
    pub shell: usize,
    pub quad: [[f64; 3]; 4],
}

/// Where a cut crosses the shells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Crossing {
    /// Sorted by station.
    pub pieces: Vec<Piece>,
    /// The section (§14.2): one slab per shell the plane cuts within the line's stations – also
    /// shells cut square to the line (a web under a vertical cut), which give no piece.
    pub section: Vec<Slab>,
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
        let mut seg: Vec<[f64; 3]> = Vec::new();
        for t in tris {
            if let Some(ends) = tri_ends(t, &pts, &dist, tol) {
                seg.extend([ends[0].0, ends[1].0]);
                if let Some(p) = triangle_piece(e, ends, frame, tol) {
                    out.pieces.push(p);
                }
            }
        }
        let thickness = g.shell_thickness.get(s[4] as usize).copied().filter(|t| t.is_finite() && *t > 0.0).unwrap_or(0.0);
        if let Some(quad) = slab(&pts[..n], &seg, thickness, frame, tol) {
            out.section.push(Slab { shell: e, quad });
        }
    }
    out.pieces.sort_by(|a, b| a.s[0].total_cmp(&b.s[0]).then(a.s[1].total_cmp(&b.s[1])));
    out
}

/// Where the plane cuts one triangle (corners `t` of the element): the two ends and their corner
/// weights.
fn tri_ends(t: &[usize; 3], pts: &[[f64; 3]; 4], dist: &[f64; 4], tol: f64) -> Option<[([f64; 3], [f64; 4]); 2]> {
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
    Some([ends[0], ends[1]])
}

/// The cut inside one triangle (its two ends), clipped to the stations 0..length.
fn triangle_piece(e: usize, ends: [([f64; 3], [f64; 4]); 2], f: &Frame, tol: f64) -> Option<Piece> {
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
    // Only near the line (§12.4): the piece's middle within half the band of the line.
    if !in_band(f, lerp(p0, p1, 0.5)) {
        return None;
    }
    Some(Piece { shell: e, s: [s0, s1], at: [p0, p1], w: [w0, w1] })
}

/// Whether a point lies within the band around the line (§12.4); always without a band.
fn in_band(f: &Frame, p: [f64; 3]) -> bool {
    let Some(b) = f.band else { return true };
    let r = sub(p, f.origin);
    let off = sub(r, scaled(f.u, dot(r, f.u)));
    dot(off, off).sqrt() <= b / 2.0
}

/// One shell's slab of the section (§14.2) from the points where the plane cuts it (`seg`), its
/// corners (for its normal) and thickness: the cut through the shell's mid-surface clipped to the
/// line's stations, widened in the plane to the shell's thickness – thickness / |sin| of the angle
/// between shell and plane, so a skew cut shows the wider strip it really makes.
fn slab(corners: &[[f64; 3]], seg: &[[f64; 3]], thickness: f64, f: &Frame, tol: f64) -> Option<[[f64; 3]; 4]> {
    // The two farthest points: both triangles of a quad give one slab.
    let mut best = (0.0, [0.0; 3], [0.0; 3]);
    for (i, a) in seg.iter().enumerate() {
        for b in &seg[i + 1..] {
            let l = dot(sub(*b, *a), sub(*b, *a));
            if l > best.0 {
                best = (l, *a, *b);
            }
        }
    }
    let (_, mut a, mut b) = best;
    if best.0.sqrt() <= tol {
        return None;
    }
    let station = |p: [f64; 3]| dot(sub(p, f.origin), f.u);
    let (mut sa, mut sb) = (station(a), station(b));
    if sa > sb {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut sa, &mut sb);
    }
    if sb < -tol || sa > f.length + tol {
        return None;
    }
    if sb - sa > tol {
        let at = |s: f64| lerp(a, b, ((s - sa) / (sb - sa)).clamp(0.0, 1.0));
        (a, b) = (at(sa.max(0.0)), at(sb.min(f.length)));
    }
    if !in_band(f, lerp(a, b, 0.5)) {
        return None;
    }
    let m = unit(cross(sub(corners[1], corners[0]), sub(corners[2], corners[0])))?;
    let w = unit(cross(f.n, sub(b, a)))?;
    let h = thickness / 2.0 / dot(m, w).abs().max(0.2);
    let off = scaled(w, h);
    let add = |p: [f64; 3], k: f64| [p[0] + off[0] * k, p[1] + off[1] * k, p[2] + off[2] * k];
    Some([add(a, 1.0), add(b, 1.0), add(b, -1.0), add(a, -1.0)])
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
    /// The section the cut makes (§14.2).
    pub section: Vec<Slab>,
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
    let mut d = Diagram { component, unit: unit_text.clone(), frame: Some(*frame), section: crossing.section.clone(), ..Default::default() };
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
    if crossing.pieces.is_empty() && crossing.section.is_empty() {
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
/// Per model, the highest Section ID given (§14.1): IDs are never reused.
static NEXT: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Section IDs shown as letters (`A–A`) instead of numbers (`S1`) (§14.1).
static LETTERS: AtomicBool = AtomicBool::new(false);
/// The cuts chosen (Browser click, Select Cuts, §14.3), by name.
static SELECTION: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Select Cuts mode is on (§14.3).
static PICKING: AtomicBool = AtomicBool::new(false);

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

/// Put back the cuts as stored; cuts kept without an ID get the next ones, in their order (§14.1).
pub fn set_all(mut cuts: Vec<Cut>) {
    {
        let mut next = NEXT.lock().unwrap_or_else(PoisonError::into_inner);
        for c in &cuts {
            let hi = next.entry(c.model.clone()).or_insert(0);
            *hi = (*hi).max(c.id);
        }
        for c in cuts.iter_mut().filter(|c| c.id == 0) {
            let hi = next.entry(c.model.clone()).or_insert(0);
            *hi += 1;
            c.id = *hi;
        }
    }
    *CUTS.write().unwrap_or_else(PoisonError::into_inner) = cuts;
    SELECTION.lock().unwrap_or_else(PoisonError::into_inner).clear();
    changed();
}

/// The highest ID given per model (for saving).
pub fn next_ids() -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = NEXT.lock().unwrap_or_else(PoisonError::into_inner).iter().map(|(k, v)| (k.clone(), *v)).collect();
    v.sort();
    v
}

/// Put back the highest IDs as stored (before [`set_all`]).
pub fn set_next_ids(v: Vec<(String, u32)>) {
    *NEXT.lock().unwrap_or_else(PoisonError::into_inner) = v.into_iter().collect();
}

/// Section IDs shown as letters (§14.1).
pub fn letters() -> bool {
    LETTERS.load(Ordering::Relaxed)
}

pub fn set_letters(on: bool) {
    if LETTERS.swap(on, Ordering::Relaxed) != on {
        changed();
    }
}

/// `1 → A, 26 → Z, 27 → AA`.
fn letters_of(mut n: u32) -> String {
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push(char::from(b'A' + (n % 26) as u8));
        n /= 26;
    }
    s.iter().rev().collect()
}

/// The Section ID as shown (§14.1): `S3`, or `C–C` with letters.
pub fn id_text(id: u32) -> String {
    if letters() {
        let l = letters_of(id);
        format!("{l}–{l}")
    } else {
        format!("S{id}")
    }
}

/// The tag at the cut line's ends: `S3`, or `C` with letters.
pub fn id_tag(id: u32) -> String {
    if letters() { letters_of(id) } else { format!("S{id}") }
}

/// A cut of the current model by name, by Section ID (`S3`, `C–C`, `C`) or by number.
pub fn find(key: &str) -> Option<Cut> {
    let key = key.trim();
    let list = cuts();
    if let Some(c) = list.iter().find(|c| c.name == key) {
        return Some(c.clone());
    }
    let n = key.parse::<u32>().ok().or_else(|| key.strip_prefix(['S', 's']).and_then(|r| r.parse().ok()));
    let upper = key.to_ascii_uppercase();
    let letters = upper.split(['–', '-']).next().unwrap_or("").trim().to_string();
    list.into_iter().find(|c| Some(c.id) == n || (!letters.is_empty() && letters_of(c.id) == letters))
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
    let used = all().iter().filter(|c| c.model == cut.model).map(|c| c.id).max().unwrap_or(0);
    let taken: Vec<String> = cuts().into_iter().map(|c| c.name).collect();
    cut.name = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if !taken.iter().any(|t| t == n) => n.to_string(),
        Some(_) => return None,
        None => (1..).map(|i| format!("Cut {i}")).find(|c| !taken.contains(c)).unwrap_or_default(),
    };
    let name = cut.name.clone();
    {
        let mut next = NEXT.lock().unwrap_or_else(PoisonError::into_inner);
        let hi = next.entry(cut.model.clone()).or_insert(0);
        *hi = (*hi).max(used) + 1;
        cut.id = *hi;
    }
    write(|all, _| all.push(cut));
    Some(name)
}

/// Replace cut `name` with `cut` (Properties, Redraw). `false`: no such cut, or no valid cut.
pub fn replace(name: &str, mut cut: Cut) -> bool {
    if cut.frame().is_none() || cut.name.trim().is_empty() || (cut.name != name && get(&cut.name).is_some()) {
        return false;
    }
    let new_name = cut.name.clone();
    let done = write(|all, m| match all.iter_mut().find(|c| c.model == m && c.name == name) {
        Some(c) => {
            cut.model = m.to_string();
            cut.id = c.id; // the ID never changes (§14.1)
            *c = cut;
            true
        }
        None => false,
    });
    if done && new_name != name {
        for s in SELECTION.lock().unwrap_or_else(PoisonError::into_inner).iter_mut().filter(|s| *s == name) {
            *s = new_name.clone();
        }
    }
    done
}

pub fn rename(old: &str, new: &str) -> bool {
    let new = new.trim();
    match get(old) {
        Some(c) if !new.is_empty() => replace(old, Cut { name: new.to_string(), ..c }),
        _ => false,
    }
}

pub fn delete(name: &str) -> bool {
    SELECTION.lock().unwrap_or_else(PoisonError::into_inner).retain(|s| s != name);
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

// ---- selecting cuts (§14.3) ----

/// The cuts chosen, by name (current model, in the order made).
pub fn selection() -> Vec<String> {
    let chosen = SELECTION.lock().unwrap_or_else(PoisonError::into_inner).clone();
    cuts().into_iter().map(|c| c.name).filter(|n| chosen.contains(n)).collect()
}

/// How a new pick meets the selection: replace it, add (Shift), remove (Ctrl).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum How {
    #[default]
    Replace,
    Add,
    Remove,
}

pub fn select(names: &[String], how: How) {
    {
        let mut s = SELECTION.lock().unwrap_or_else(PoisonError::into_inner);
        match how {
            How::Replace => *s = names.to_vec(),
            How::Add => {
                let new: Vec<String> = names.iter().filter(|n| !s.contains(n)).cloned().collect();
                s.extend(new);
            }
            How::Remove => s.retain(|n| !names.contains(n)),
        }
    }
    changed();
}

pub fn picking() -> bool {
    PICKING.load(Ordering::Relaxed)
}

/// Select Cuts mode on / off; it ends the Cut Diagram mode (one mode at a time).
pub fn set_picking(on: bool) {
    if on {
        set_mode(None);
    }
    PICKING.store(on, Ordering::Relaxed);
    #[cfg(feature = "ui")]
    results_view::repaint();
}

fn cross2(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Whether segments a–b and c–d meet (touching counts).
fn segments_meet(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (d1, d2, d3, d4) = (cross2(c, d, a), cross2(c, d, b), cross2(a, b, c), cross2(a, b, d));
    let on = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| r[0] >= p[0].min(q[0]) && r[0] <= p[0].max(q[0]) && r[1] >= p[1].min(q[1]) && r[1] <= p[1].max(q[1]);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    (d1 == 0.0 && on(c, d, a)) || (d2 == 0.0 && on(c, d, b)) || (d3 == 0.0 && on(a, b, c)) || (d4 == 0.0 && on(a, b, d))
}

fn inside(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut odd = false;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) {
            odd = !odd;
        }
    }
    odd
}

/// Whether the drawn polyline `line` crosses `shape` (2-D, §14.3): a segment (2 points) or a closed
/// polygon (3 or more) – an edge crossed, or the line inside it.
pub fn line_hits(line: &[[f64; 2]], shape: &[[f64; 2]]) -> bool {
    let finite = |p: &[f64; 2]| p[0].is_finite() && p[1].is_finite();
    if line.len() < 2 || shape.len() < 2 || !line.iter().chain(shape).all(finite) {
        return false;
    }
    let closed = shape.len() >= 3;
    let edges = if closed { shape.len() } else { 1 };
    for l in line.windows(2) {
        for i in 0..edges {
            if segments_meet(l[0], l[1], shape[i], shape[(i + 1) % shape.len()]) {
                return true;
            }
        }
    }
    closed && inside(line[0], shape)
}

/// The cuts (name, 2-D shapes: the cut line and the section's quads) a drawn line crosses.
pub fn hit_cuts(line: &[[f64; 2]], cuts: &[(String, Vec<Vec<[f64; 2]>>)]) -> Vec<String> {
    cuts.iter().filter(|(_, shapes)| shapes.iter().any(|s| line_hits(line, s))).map(|(n, _)| n.clone()).collect()
}

/// The 3-D shapes Select Cuts tests for `cut` (§14.3): the cut line, and the section's quads
/// unless the view draws *Line only*.
pub fn pick_shapes(cut: &Cut, d: Option<&Diagram>, line_only: bool) -> Vec<Vec<[f64; 3]>> {
    let Some(f) = cut.frame() else { return Vec::new() };
    let mut v = vec![vec![f.origin, [0, 1, 2].map(|i| f.origin[i] + f.u[i] * f.length)]];
    if !line_only && let Some(d) = d {
        v.extend(d.section.iter().map(|s| s.quad.to_vec()));
    }
    v
}

// ---- Cut Diagram mode (C1) ----

/// The mode: the cut being redrawn (or a new one), the plane type, the points clicked so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mode {
    pub redraw: Option<String>,
    pub kind: Kind,
    pub points: Vec<[f64; 3]>,
}

impl Mode {
    /// Redraw cut `name` with the type it was made with.
    pub fn redraw(name: &str) -> Mode {
        Mode { redraw: Some(name.to_string()), kind: get(name).map(|c| Kind::of(c.direction)).unwrap_or_default(), points: Vec::new() }
    }
}

static MODE: LazyLock<Mutex<Option<Mode>>> = LazyLock::new(|| Mutex::new(None));

pub fn mode() -> Option<Mode> {
    MODE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

pub fn set_mode(m: Option<Mode>) {
    if m.is_some() {
        PICKING.store(false, Ordering::Relaxed);
    }
    *MODE.lock().unwrap_or_else(PoisonError::into_inner) = m;
    #[cfg(feature = "ui")]
    results_view::repaint();
}

/// A click in the mode at `p` (m). The last click of the plane type (2, or 3 for *3 points*) makes
/// (or redraws) the cut and leaves the mode; returns its name. A last click that makes no cut
/// (points equal or in one line) is dropped and the mode stays.
pub fn click(p: [f64; 3]) -> Option<String> {
    let mut m = mode()?;
    m.points.push(p);
    if m.points.len() < m.kind.clicks() {
        set_mode(Some(m));
        return None;
    }
    let (p1, p2) = (m.points[0], m.points[1]);
    let direction = match m.kind {
        Kind::Vertical => Direction::Vertical,
        Kind::Horizontal => Direction::Horizontal,
        Kind::ThreePoints => Direction::Plane3(p),
    };
    let made = match &m.redraw {
        Some(name) => get(name).and_then(|c| replace(name, Cut { p1, p2, direction, ..c }).then(|| name.clone())),
        None => add(None, p1, p2, direction),
    };
    if made.is_some() {
        set_mode(None);
    } else {
        m.points.pop();
        set_mode(Some(m));
    }
    made
}

/// *Global plane* (§12.1): the points and direction of a cut in plane `plane` ("XY", "XZ", "YZ")
/// at `at` (m), from `from` to `to` along the plane's axis (XZ: X, YZ: Y, XY: `axis` X or Y), the
/// line placed at `other` (m) on the remaining axis. `None` for an unknown plane or axis.
pub fn global(plane: &str, at: f64, axis: char, from: f64, to: f64, other: f64) -> Option<([f64; 3], [f64; 3], Direction)> {
    match (plane.to_ascii_uppercase().as_str(), axis.to_ascii_uppercase()) {
        ("XZ", _) => Some(([from, at, other], [to, at, other], Direction::Vertical)),
        ("YZ", _) => Some(([at, from, other], [at, to, other], Direction::Vertical)),
        ("XY", 'X') => Some(([from, other, at], [to, other, at], Direction::Horizontal)),
        ("XY", 'Y') => Some(([other, from, at], [other, to, at], Direction::Horizontal)),
        _ => None,
    }
}

// ---- commands (R8) ----

fn point(params: &Value, key: &str) -> Result<[f64; 3]> {
    let v = params.get(key).and_then(Value::as_array).filter(|a| a.len() == 3).ok_or_else(|| custom_bridge::Error(format!("give {key} as [x, y, z] in m")))?;
    let p: Vec<f64> = v.iter().filter_map(Value::as_f64).filter(|x| x.is_finite()).collect();
    <[f64; 3]>::try_from(p).map_err(|_| custom_bridge::Error(format!("{key}: three finite numbers")))
}

fn cut_param(params: &Value) -> Result<Cut> {
    let name = params.get("cut").and_then(Value::as_str).ok_or_else(|| custom_bridge::Error("give the cut's name or Section ID".into()))?;
    find(name).ok_or_else(|| custom_bridge::Error(format!("no cut {name:?}")))
}

fn report() -> Result<Value> {
    Ok(json!({ "cuts": cuts().iter().map(|c| json!({ "id": id_text(c.id), "name": c.name, "p1": c.p1, "p2": c.p2, "shown": shown_in_view(&c.name) })).collect::<Vec<_>>() }))
}

/// `factory.cut.mode`: Cut Diagram mode on (off with `on: false`).
pub fn mode_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let on = params.get("on").and_then(Value::as_bool).unwrap_or(true);
    set_mode(on.then(Mode::default));
    Ok(json!({ "mode": on }))
}

/// `factory.cut.new`: a cut from `p1` to `p2` (m) – `type`: "vertical" (default), "horizontal",
/// "3points" (with `p3`), or `direction` [x, y, z]; or by a global plane: `plane` XY | XZ | YZ,
/// `at`, `from`, `to`, `axis`? (X | Y, for XY), `other`? (m, default 0) (§12.5).
pub fn new_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let num = |k: &str| params.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| custom_bridge::Error(format!("give {k} (m)")));
    let (p1, p2, direction) = if let Some(plane) = params.get("plane").and_then(Value::as_str) {
        let axis = params.get("axis").and_then(Value::as_str).and_then(|a| a.chars().next()).unwrap_or('X');
        let other = params.get("other").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
        global(plane, num("at")?, axis, num("from")?, num("to")?, other).ok_or_else(|| custom_bridge::Error(format!("plane {plane:?}: XY, XZ or YZ; axis X or Y")))?
    } else {
        let (p1, p2) = (point(params, "p1")?, point(params, "p2")?);
        let kind = params.get("type").and_then(Value::as_str).map(str::to_ascii_lowercase);
        let direction = match (kind.as_deref(), params.get("direction")) {
            (Some("horizontal"), _) => Direction::Horizontal,
            (Some("3points"), _) => Direction::Plane3(point(params, "p3")?),
            (Some("vertical"), _) | (None, None) => Direction::Vertical,
            (None, Some(Value::String(s))) if s.eq_ignore_ascii_case("vertical") => Direction::Vertical,
            (None, Some(v)) => Direction::Along(point(&json!({ "d": v }), "d")?),
            (Some(t), _) => return Err(custom_bridge::Error(format!("type {t:?}: vertical, horizontal or 3points"))),
        };
        (p1, p2, direction)
    };
    let name = params.get("name").and_then(Value::as_str);
    let made = add(name, p1, p2, direction).ok_or_else(|| custom_bridge::Error("no cut: equal points, the line along the direction, or the name is taken".into()))?;
    let id = get(&made).map(|c| id_text(c.id)).unwrap_or_default();
    Ok(json!({ "cut": made, "id": id }))
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

/// `factory.cut.set`: component (name, or "window"), scale (px), text, fill, all_values,
/// draw_normal, band (m; 0 = the whole plane).
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
    if let Some(b) = params.get("band").and_then(Value::as_f64) {
        c.band = (b.is_finite() && b > 0.0).then_some(b);
    }
    for (key, slot) in [("text", &mut c.text), ("fill", &mut c.fill), ("all_values", &mut c.all_values), ("draw_normal", &mut c.draw_normal)] {
        if let Some(b) = params.get(key).and_then(Value::as_bool) {
            *slot = b;
        }
    }
    let name = c.name.clone();
    replace(&name, c);
    report()
}

/// `factory.cut.pick`: Select Cuts mode on (off with `on: false`) (§14.3).
pub fn pick_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let on = params.get("on").and_then(Value::as_bool).unwrap_or(true);
    set_picking(on);
    Ok(json!({ "picking": on }))
}

/// `factory.cut.select` (§14.3): `line` [[x, y, z], …] (m) seen along `view` [x, y, z] (default
/// from above, [0, 0, -1]) selects the shown cuts it crosses; or `ids` (names or IDs); or `clear`.
/// `how`: "replace" (default), "add", "remove". Returns the selected IDs.
pub fn select_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let how = match params.get("how").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        None | Some("replace") => How::Replace,
        Some("add") => How::Add,
        Some("remove") => How::Remove,
        Some(h) => return Err(custom_bridge::Error(format!("how {h:?}: replace, add or remove"))),
    };
    if params.get("clear").and_then(Value::as_bool) == Some(true) {
        select(&[], How::Replace);
    } else if let Some(ids) = params.get("ids").and_then(Value::as_array) {
        let mut names = Vec::new();
        for k in ids {
            let k = k.as_str().map(str::to_string).or_else(|| k.as_u64().map(|n| n.to_string())).unwrap_or_default();
            names.push(find(&k).ok_or_else(|| custom_bridge::Error(format!("no cut {k:?}")))?.name);
        }
        select(&names, how);
    } else if let Some(line) = params.get("line").and_then(Value::as_array) {
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for (i, p) in line.iter().enumerate() {
            pts.push(point(&json!({ "p": p }), "p").map_err(|e| custom_bridge::Error(format!("line[{i}]: {}", e.0)))?);
        }
        if pts.len() < 2 {
            return Err(custom_bridge::Error("line: at least two points".into()));
        }
        let view = match params.get("view") {
            Some(v) => point(&json!({ "view": v }), "view")?,
            None => [0.0, 0.0, -1.0],
        };
        let v = unit(view).ok_or_else(|| custom_bridge::Error("view: not a direction".into()))?;
        let up = if v[2].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
        let e1 = unit(cross(up, v)).unwrap_or([1.0, 0.0, 0.0]);
        let e2 = cross(v, e1);
        let flat = |p: &[f64; 3]| [dot(*p, e1), dot(*p, e2)];
        let g = transfer::geometry();
        let s = vv::settings();
        let units = model::read(|f| f.units);
        let choice = results_view::shown().choice;
        let snap = results_view::snap_data();
        let blocks = transfer::results();
        let shapes: Vec<(String, Vec<Vec<[f64; 2]>>)> = cuts()
            .into_iter()
            .filter(|c| !s.cuts_hidden.contains(&c.name))
            .map(|c| {
                let d = compute(&g, &blocks, &c, &choice, &snap.shell_shown, units).ok();
                let sh = pick_shapes(&c, d.as_ref(), s.cut_section_line).iter().map(|q| q.iter().map(flat).collect()).collect();
                (c.name, sh)
            })
            .collect();
        let line2: Vec<[f64; 2]> = pts.iter().map(flat).collect();
        select(&hit_cuts(&line2, &shapes), how);
    } else {
        return Err(custom_bridge::Error("give line, ids or clear".into()));
    }
    let chosen: Vec<String> = selection().iter().filter_map(|n| get(n)).map(|c| id_text(c.id)).collect();
    Ok(json!({ "selected": chosen }))
}

/// `factory.cut.options`: `ids` "numbers" (S1) or "letters" (A–A) (§14.1).
pub fn options_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    match params.get("ids").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        Some("numbers") => set_letters(false),
        Some("letters") => set_letters(true),
        None => {}
        Some(x) => return Err(custom_bridge::Error(format!("ids {x:?}: numbers or letters"))),
    }
    Ok(json!({ "ids": if letters() { "letters" } else { "numbers" } }))
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
        "id": id_text(c.id),
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
