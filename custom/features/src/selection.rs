//! Selecting Factory's elements (docs/13-selection.md): the selection set, its history and the
//! tools T1–T11 as plain functions over the transferred geometry. No UI here (that is
//! `ui_select.rs`), so every tool is testable and callable as a command (rule R9).
//!
//! Rules: only shown elements are taken (R2); a tool adds to the selection, or removes in
//! deselect mode (R3, C1); the selection is one bit per element (S4); every change can be taken
//! back (R8, C4: 20 steps).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use crate::results_view;
use crate::transfer::Geometry;

/// One bit per element (rule S4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bits(Vec<u64>);

impl Bits {
    pub fn with_len(n: usize) -> Self {
        Bits(vec![0; n.div_ceil(64)])
    }
    pub fn get(&self, i: usize) -> bool {
        self.0.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
    }
    pub fn set(&mut self, i: usize, on: bool) {
        if self.0.len() <= i / 64 {
            self.0.resize(i / 64 + 1, 0);
        }
        if let Some(w) = self.0.get_mut(i / 64) {
            if on {
                *w |= 1 << (i % 64);
            } else {
                *w &= !(1 << (i % 64));
            }
        }
    }
    pub fn count(&self) -> usize {
        self.0.iter().map(|w| w.count_ones() as usize).sum()
    }
    pub fn ones(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(k, w)| (0..64).filter(move |b| w >> b & 1 == 1).map(move |b| k * 64 + b))
    }
    fn union(&mut self, o: &Bits) {
        if self.0.len() < o.0.len() {
            self.0.resize(o.0.len(), 0);
        }
        for (a, b) in self.0.iter_mut().zip(&o.0) {
            *a |= *b;
        }
    }
    fn minus(&mut self, o: &Bits) {
        for (a, b) in self.0.iter_mut().zip(&o.0) {
            *a &= !*b;
        }
    }
}

/// Selected (or found) shells and frames. Joints follow when they are drawn (docs/13 §7).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Set {
    pub shells: Bits,
    pub frames: Bits,
}

impl Set {
    pub fn empty(g: &Geometry) -> Self {
        Set { shells: Bits::with_len(g.shells.len()), frames: Bits::with_len(g.frames.len()) }
    }
    pub fn counts(&self) -> (usize, usize) {
        (self.shells.count(), self.frames.count())
    }
    pub fn is_empty(&self) -> bool {
        self.counts() == (0, 0)
    }
}

/// Which kinds the tools take (rule R1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kinds {
    pub shells: bool,
    pub frames: bool,
}

impl Default for Kinds {
    fn default() -> Self {
        Kinds { shells: true, frames: true }
    }
}

/// What every tool needs to know about the view state: shown elements (R2) and kinds (R1).
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub g: &'a Geometry,
    pub shell_shown: &'a [bool],
    pub frame_shown: &'a [bool],
    pub kinds: Kinds,
}

impl Scope<'_> {
    fn shell_ok(&self, e: usize) -> bool {
        self.kinds.shells && self.shell_shown.get(e).copied().unwrap_or(true)
    }
    fn frame_ok(&self, e: usize) -> bool {
        self.kinds.frames && self.frame_shown.get(e).copied().unwrap_or(true)
    }
    /// The elements of the chosen kinds that `shell` / `frame` take, and how many matches were
    /// left out because they are hidden (R2).
    fn collect(&self, shell: impl Fn(usize) -> bool, frame: impl Fn(usize) -> bool) -> Found {
        let mut set = Set::empty(self.g);
        let mut hidden = 0;
        if self.kinds.shells {
            for e in 0..self.g.shells.len() {
                if shell(e) {
                    if self.shell_ok(e) { set.shells.set(e, true) } else { hidden += 1 }
                }
            }
        }
        if self.kinds.frames {
            for e in 0..self.g.frames.len() {
                if frame(e) {
                    if self.frame_ok(e) { set.frames.set(e, true) } else { hidden += 1 }
                }
            }
        }
        Found { set, hidden }
    }
}

/// A tool's result: the elements, and the matches it left out because they are hidden.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub set: Set,
    pub hidden: usize,
}

fn shell_corners(g: &Geometry, e: usize) -> Vec<[f64; 3]> {
    let s = g.shells[e];
    let n = if s[3] == u32::MAX { 3 } else { 4 };
    s[..n].iter().filter_map(|j| g.joints.get(*j as usize).copied()).collect()
}

fn frame_ends(g: &Geometry, e: usize) -> Option<([f64; 3], [f64; 3])> {
    let f = g.frames[e];
    Some((*g.joints.get(f[0] as usize)?, *g.joints.get(f[1] as usize)?))
}

/// A frame's ends as drawn: with Shrink Objects the middle `SHRINK` of its length (rule R4).
pub fn drawn_frame_ends(g: &Geometry, d: &results_view::SnapData, e: usize) -> Option<([f64; 3], [f64; 3])> {
    let (a, b) = frame_ends(g, e)?;
    if !d.view.shrink {
        return Some((a, b));
    }
    let (t0, t1) = ((1.0 - crate::vv::SHRINK) / 2.0, (1.0 + crate::vv::SHRINK) / 2.0);
    let at = |t: f64| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
    Some((at(t0), at(t1)))
}

// ---- T1–T4: screen tools ----------------------------------------------------------------------

/// An area on the screen: a rectangle or a closed polygon (screen points).
#[derive(Clone, Debug, PartialEq)]
pub enum Area {
    Rect { min: [f32; 2], max: [f32; 2] },
    Polygon(Vec<[f32; 2]>),
}

impl Area {
    fn outline(&self) -> Vec<[f32; 2]> {
        match self {
            Area::Rect { min, max } => vec![*min, [max[0], min[1]], *max, [min[0], max[1]]],
            Area::Polygon(p) => p.clone(),
        }
    }
    fn contains(&self, p: [f32; 2]) -> bool {
        match self {
            Area::Rect { min, max } => p[0] >= min[0] && p[0] <= max[0] && p[1] >= min[1] && p[1] <= max[1],
            Area::Polygon(poly) => inside_polygon(p, poly),
        }
    }
}

fn inside_polygon(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

fn segments_cross(p: [f32; 2], q: [f32; 2], r: [f32; 2], s: [f32; 2]) -> bool {
    let o = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let (d1, d2, d3, d4) = (o(r, s, p), o(r, s, q), o(p, q, r), o(p, q, s));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

fn polyline_crosses(a: &[[f32; 2]], b: &[[f32; 2]], close_a: bool, close_b: bool) -> bool {
    let edges = |p: &[[f32; 2]], close: bool| -> Vec<([f32; 2], [f32; 2])> {
        let mut e: Vec<_> = p.windows(2).map(|w| (w[0], w[1])).collect();
        if close && p.len() > 2 {
            e.push((p[p.len() - 1], p[0]));
        }
        e
    };
    let (ea, eb) = (edges(a, close_a), edges(b, close_b));
    ea.iter().any(|(p, q)| eb.iter().any(|(r, s)| segments_cross(*p, *q, *r, *s)))
}

/// T1–T3: elements in `area` as drawn (View Options: shrink, extrude – rule R4). Window: every
/// drawn corner inside; crossing: any part inside, an edge crossing the outline, or the area
/// inside the element. Through the model (R5): elements behind others are taken too.
pub fn in_area(s: Scope<'_>, d: &results_view::SnapData, area: &Area, crossing: bool, project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>) -> Found {
    let outline = area.outline();
    let shell = |e: usize| {
        let n = if s.g.shells[e][3] == u32::MAX { 3 } else { 4 };
        let faces = results_view::drawn_faces(s.g, d, e);
        let mut all_in = true;
        let mut any = false;
        for face in &faces {
            let pts: Vec<[f32; 2]> = face[..n].iter().filter_map(|p| project(p.map(f64::from))).collect();
            if pts.len() < n {
                all_in = false;
                continue;
            }
            let inside = pts.iter().filter(|p| area.contains(**p)).count();
            all_in &= inside == n;
            any |= inside > 0 || polyline_crosses(&pts, &outline, true, true) || outline.first().is_some_and(|o| inside_polygon(*o, &pts));
        }
        if crossing { any } else { all_in && !faces.is_empty() }
    };
    let frame = |e: usize| {
        let Some((a, b)) = drawn_frame_ends(s.g, d, e) else { return false };
        let (Some(pa), Some(pb)) = (project(a), project(b)) else { return false };
        let (ia, ib) = (area.contains(pa), area.contains(pb));
        if crossing { ia || ib || polyline_crosses(&[pa, pb], &outline, false, true) } else { ia && ib }
    };
    s.collect(shell, frame)
}

/// T4: elements a drawn polyline (screen points) crosses.
pub fn crossed_by_line(s: Scope<'_>, d: &results_view::SnapData, line: &[[f32; 2]], project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>) -> Found {
    let shell = |e: usize| {
        let n = if s.g.shells[e][3] == u32::MAX { 3 } else { 4 };
        results_view::drawn_faces(s.g, d, e).iter().any(|face| {
            let pts: Vec<[f32; 2]> = face[..n].iter().filter_map(|p| project(p.map(f64::from))).collect();
            pts.len() == n && (polyline_crosses(line, &pts, false, true) || line.iter().any(|p| inside_polygon(*p, &pts)))
        })
    };
    let frame = |e: usize| {
        let Some((a, b)) = drawn_frame_ends(s.g, d, e) else { return false };
        matches!((project(a), project(b)), (Some(pa), Some(pb)) if polyline_crosses(line, &[pa, pb], false, false))
    };
    s.collect(shell, frame)
}

/// T1 click: the element under the cursor nearest to the viewer (R5) – a shell under it, or a
/// frame within `radius` px; the frame wins when at least as near.
pub fn at_point(s: Scope<'_>, d: &results_view::SnapData, cursor: [f32; 2], project: &dyn Fn([f64; 3]) -> Option<[f32; 2]>, eye: [f64; 3], radius: f32) -> Found {
    let dist_eye = |p: [f64; 3]| ((p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2)).sqrt();
    let mut shell: Option<(f64, usize)> = None;
    if s.kinds.shells {
        // The value snap finds the shell under the cursor as drawn, with the hidden ones skipped.
        let shown = results_view::SnapData { shell_shown: s.shell_shown.to_vec(), view: d.view.clone(), ..Default::default() };
        if let Some(hit) = results_view::snap(s.g, &shown, cursor, project, eye, 0.0)
            && let Some(e) = hit.shell
        {
            shell = Some((dist_eye(hit.at), e));
        }
    }
    let mut frame: Option<(f64, usize)> = None;
    if s.kinds.frames {
        for e in 0..s.g.frames.len() {
            if !s.frame_ok(e) {
                continue;
            }
            let Some((a, b)) = drawn_frame_ends(s.g, d, e) else { continue };
            let (Some(pa), Some(pb)) = (project(a), project(b)) else { continue };
            let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((cursor[0] - pa[0]) * dx + (cursor[1] - pa[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let (cx, cy) = (pa[0] + t * dx - cursor[0], pa[1] + t * dy - cursor[1]);
            if (cx * cx + cy * cy).sqrt() <= radius {
                let p = [0, 1, 2].map(|i| a[i] + f64::from(t) * (b[i] - a[i]));
                let depth = dist_eye(p);
                if frame.is_none_or(|(d0, _)| depth < d0) {
                    frame = Some((depth, e));
                }
            }
        }
    }
    let mut set = Set::empty(s.g);
    match (shell, frame) {
        (sh, Some((fd, e))) if sh.is_none_or(|(sd, _)| fd <= sd + 1e-6) => set.frames.set(e, true),
        (Some((_, e)), _) => set.shells.set(e, true),
        _ => {}
    }
    Found { set, hidden: 0 }
}

// ---- T5–T11: data tools -------------------------------------------------------------------------

/// T5: elements whose every node lies in the ranges (m; `None` = any) – a box or, with min = max
/// and a tolerance, a plane.
pub fn in_coordinates(s: Scope<'_>, ranges: [Option<(f64, f64)>; 3], tolerance: f64) -> Found {
    let ok = |p: &[f64; 3]| (0..3).all(|i| ranges[i].is_none_or(|(lo, hi)| p[i] >= lo.min(hi) - tolerance && p[i] <= lo.max(hi) + tolerance));
    s.collect(
        |e| {
            let c = shell_corners(s.g, e);
            !c.is_empty() && c.iter().all(ok)
        },
        |e| frame_ends(s.g, e).is_some_and(|(a, b)| ok(&a) && ok(&b)),
    )
}

/// Which direction T6 compares with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Parallel {
    /// Along a direction (an axis or a frame's).
    To([f64; 3]),
    /// Lying in planes with this normal (XY: Z, …).
    Plane([f64; 3]),
}

/// T6: frames parallel to a direction or a plane, within `tolerance_deg` (either sense).
pub fn parallel(s: Scope<'_>, to: Parallel, tolerance_deg: f64) -> Found {
    let unit = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        (l > 1e-12).then(|| v.map(|c| c / l))
    };
    let tol = tolerance_deg.to_radians();
    let frame = |e: usize| {
        let Some((a, b)) = frame_ends(s.g, e) else { return false };
        let Some(d) = unit([b[0] - a[0], b[1] - a[1], b[2] - a[2]]) else { return false };
        match to {
            Parallel::To(v) => unit(v).is_some_and(|v| (d[0] * v[0] + d[1] * v[1] + d[2] * v[2]).abs() >= tol.cos()),
            Parallel::Plane(n) => unit(n).is_some_and(|n| (d[0] * n[0] + d[1] * n[1] + d[2] * n[2]).abs() <= tol.sin()),
        }
    };
    let only_frames = Scope { kinds: Kinds { shells: false, frames: s.kinds.frames }, ..s };
    only_frames.collect(|_| false, frame)
}

/// T7: elements with one of these sections (indices into the geometry's section lists), shells
/// optionally within a thickness range (m).
pub fn with_properties(s: Scope<'_>, shell_sections: &[usize], frame_sections: &[usize], thickness: Option<(f64, f64)>) -> Found {
    s.collect(
        |e| {
            let sec = s.g.shells[e][4] as usize;
            let t = s.g.shell_thickness.get(sec).copied().unwrap_or(f64::NAN);
            (shell_sections.is_empty() && thickness.is_some() || shell_sections.contains(&sec)) && thickness.is_none_or(|(lo, hi)| t >= lo.min(hi) - 1e-12 && t <= lo.max(hi) + 1e-12)
        },
        |e| frame_sections.contains(&(s.g.frames[e][2] as usize)),
    )
}

/// Whether `name` matches one `pattern` of T10: `*` / `?` wildcards, a range `101-150` or
/// `A101-A150` (same prefix, numbers between), else the exact name; case-insensitive.
pub fn label_matches(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.trim().to_lowercase(), name.to_lowercase());
    if p.is_empty() {
        return false;
    }
    if p.contains('*') || p.contains('?') {
        return glob(p.as_bytes(), n.as_bytes());
    }
    if let Some((lo, hi)) = p.split_once('-')
        && let (Some((pl, a)), Some((ph, b))) = (split_number(lo), split_number(hi))
        && pl == ph
        && let Some((pn, x)) = split_number(&n)
    {
        return pn == pl && x >= a.min(b) && x <= a.max(b);
    }
    p == n
}

/// A name's text prefix and trailing number ("a101" → ("a", 101)).
fn split_number(s: &str) -> Option<(&str, u64)> {
    let digits = s.len() - s.bytes().rev().take_while(u8::is_ascii_digit).count();
    let number = s.get(digits..).filter(|d| !d.is_empty())?.parse().ok()?;
    Some((s.get(..digits)?, number))
}

fn glob(p: &[u8], n: &[u8]) -> bool {
    match (p.first(), n.first()) {
        (None, None) => true,
        (Some(b'*'), _) => glob(&p[1..], n) || (!n.is_empty() && glob(p, &n[1..])),
        (Some(b'?'), Some(_)) => glob(&p[1..], &n[1..]),
        (Some(a), Some(b)) if a == b => glob(&p[1..], &n[1..]),
        _ => false,
    }
}

/// T10: elements by name – patterns separated by commas, semicolons or spaces.
pub fn with_labels(s: Scope<'_>, text: &str) -> Found {
    let patterns: Vec<&str> = text.split([',', ';', ' ', '\n']).filter(|p| !p.trim().is_empty()).collect();
    let hit = |name: Option<&String>| name.is_some_and(|n| patterns.iter().any(|p| label_matches(p, n)));
    s.collect(|e| hit(s.g.shell_names.get(e)), |e| hit(s.g.frame_names.get(e)))
}

/// T9: the members of groups (element names).
pub fn in_groups(s: Scope<'_>, members: &[String]) -> Found {
    let names: std::collections::HashSet<&str> = members.iter().map(String::as_str).collect();
    let hit = |name: Option<&String>| name.is_some_and(|n| names.contains(n.as_str()));
    s.collect(|e| hit(s.g.shell_names.get(e)), |e| hit(s.g.frame_names.get(e)))
}

/// The shown elements of `set` (Browser rows: a section, Shells, Frames, a group).
pub fn within(s: Scope<'_>, set: &Set) -> Found {
    s.collect(|e| set.shells.get(e), |e| set.frames.get(e))
}

/// T11: every shown element of the chosen kinds.
pub fn all(s: Scope<'_>) -> Found {
    s.collect(|_| true, |_| true)
}

// ---- The selection (R3, R8, C1–C4) --------------------------------------------------------------

/// The current selection, its history and the deselect mode.
#[derive(Default)]
struct State {
    current: Arc<Set>,
    history: Vec<Arc<Set>>,
    deselect: bool,
    kinds: Kinds,
    /// The last tool's report, e.g. "12 shells added (3 hidden matches left out)".
    message: String,
}

/// Steps kept for Previous (R8).
pub const HISTORY: usize = 20;

static STATE: LazyLock<RwLock<State>> = LazyLock::new(|| RwLock::new(State::default()));
static VERSION: AtomicU64 = AtomicU64::new(1);

fn write<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let r = f(&mut STATE.write().unwrap_or_else(PoisonError::into_inner));
    VERSION.fetch_add(1, Ordering::Relaxed);
    r
}

fn read<R>(f: impl FnOnce(&State) -> R) -> R {
    f(&STATE.read().unwrap_or_else(PoisonError::into_inner))
}

/// The selection now.
pub fn current() -> Arc<Set> {
    read(|s| s.current.clone())
}

/// Changes every time the selection, the mode or the kinds change (to redraw the highlight).
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

pub fn deselect_mode() -> bool {
    read(|s| s.deselect)
}

pub fn set_deselect_mode(on: bool) {
    write(|s| s.deselect = on);
}

pub fn kinds() -> Kinds {
    read(|s| s.kinds)
}

pub fn set_kinds(k: Kinds) {
    write(|s| s.kinds = k);
}

pub fn message() -> String {
    read(|s| s.message.clone())
}

fn replace(next: Set, message: String) {
    write(|s| {
        if *s.current != next {
            let previous = std::mem::replace(&mut s.current, Arc::new(next));
            s.history.push(previous);
            if s.history.len() > HISTORY {
                s.history.remove(0);
            }
        }
        s.message = message;
    });
}

fn describe(verb: &str, (shells, frames): (usize, usize), hidden: usize) -> String {
    let mut m = format!("{shells} shells, {frames} frames {verb}");
    if hidden > 0 {
        m.push_str(&format!(" ({hidden} hidden matches left out)"));
    }
    m
}

/// Add what a tool found – or remove it in deselect mode, or when `remove` (Ctrl held) (R3, C1).
pub fn apply(found: Found, remove: bool) {
    let remove = remove || deselect_mode();
    let mut next = (*current()).clone();
    if remove {
        next.shells.minus(&found.set.shells);
        next.frames.minus(&found.set.frames);
    } else {
        next.shells.union(&found.set.shells);
        next.frames.union(&found.set.frames);
    }
    replace(next, describe(if remove { "removed" } else { "added" }, found.set.counts(), found.hidden));
}

/// C2: the shown elements of the chosen kinds swap state.
pub fn invert(s: Scope<'_>) {
    let cur = current();
    let mut next = Set::empty(s.g);
    for e in 0..s.g.shells.len() {
        next.shells.set(e, if s.shell_ok(e) { !cur.shells.get(e) } else { cur.shells.get(e) });
    }
    for e in 0..s.g.frames.len() {
        next.frames.set(e, if s.frame_ok(e) { !cur.frames.get(e) } else { cur.frames.get(e) });
    }
    let counts = next.counts();
    replace(next, describe("selected after invert", counts, 0));
}

/// C3: nothing selected.
pub fn clear() {
    replace(Set::default(), "selection cleared".into());
}

/// C4 / R8: the selection before the last change.
pub fn previous() {
    write(|s| match s.history.pop() {
        Some(p) => {
            s.current = p;
            s.message = "previous selection".into();
        }
        None => s.message = "no previous selection".into(),
    });
}

/// Start over for a new transfer (element numbers change).
pub fn reset() {
    write(|s| {
        s.current = Arc::new(Set::default());
        s.history.clear();
        s.message.clear();
    });
}

/// C5: the selection's element names (shells, then frames), for saving it as a group.
pub fn names(g: &Geometry, set: &Set) -> Vec<String> {
    let name = |list: &Vec<String>, e: usize| list.get(e).cloned().unwrap_or_default();
    set.shells.ones().map(|e| name(&g.shell_names, e)).chain(set.frames.ones().map(|e| name(&g.frame_names, e))).filter(|n| !n.is_empty()).collect()
}
