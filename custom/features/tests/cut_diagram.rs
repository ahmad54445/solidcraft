//! Cut Diagram (docs/16-cut-diagram.md §7): crossings, values, jumps, cut axes, resultant, table,
//! the cuts kept per model and per view. The cuts are global: one test at a time.

use std::collections::HashMap;

use custom_features::cut::{self, CUT_AXES, Cut, Direction};
use custom_features::results_view::{Averaging, Choice};
use custom_features::transfer::{Geometry, ResultBlock};
use custom_features::units::Units;
use custom_features::{model, vv, windows};

static GLOBAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn start() -> std::sync::MutexGuard<'static, ()> {
    let g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cut::set_all(Vec::new());
    model::write(|f| f.model.file = "test.bdb".into());
    vv::set_quiet(vv::Settings::default());
    g
}

const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// Two 1 m squares side by side at z = 0: A from x 0..1, B from x 1..2 (y 0..1), local axes = global.
fn model2() -> Geometry {
    Geometry {
        joints: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        shells: vec![[0, 1, 4, 5, 0], [1, 2, 3, 4, 0]],
        shell_names: vec!["A".into(), "B".into()],
        shell_axes: vec![IDENTITY, IDENTITY],
        ..Default::default()
    }
}

/// A cut along x at y = 0.5 from x = 0 to 2.
fn along_x() -> Cut {
    Cut { name: "t".into(), p1: [0.0, 0.5, 0.0], p2: [2.0, 0.5, 0.0], ..Cut::default() }
}

/// 8 values per corner from a function of the corner's joint position (SI).
fn tensors(g: &Geometry, f: impl Fn([f64; 3], usize) -> [f64; 8]) -> HashMap<usize, [[f64; 8]; 4]> {
    (0..g.shells.len()).map(|e| (e, std::array::from_fn(|c| f(g.joints[g.shells[e][c] as usize], e)))).collect()
}

fn diagram(g: &Geometry, c: &Cut, t: &HashMap<usize, [[f64; 8]; 4]>, component: usize, shown: &[bool]) -> cut::Diagram {
    let f = c.frame().expect("a valid cut");
    let x = cut::crossing(g, &f, shown);
    cut::evaluate(g, c, &f, &x, t, component, Units::default())
}

const M11: usize = 3;

#[test]
fn c1_linear_values_give_a_straight_line_with_exact_ends() {
    let _g = start();
    let g = model2();
    // M11 = x kN·m/m (stored in N·m/m).
    let t = tensors(&g, |p, _| [0.0, 0.0, 0.0, 1000.0 * p[0], 0.0, 0.0, 0.0, 0.0]);
    let d = diagram(&g, &along_x(), &t, M11, &[]);
    assert_eq!(d.pieces.len(), 4, "two triangles per square");
    assert_eq!(d.unit, "kN·m/m");
    for st in d.pieces.iter().flatten() {
        assert!((st.value - st.s).abs() < 1e-9, "M11 = x at station {}: {}", st.s, st.value);
    }
    assert!((d.pieces[0][0].value - 0.0).abs() < 1e-9 && (d.pieces[3][1].value - 2.0).abs() < 1e-9);
}

#[test]
fn c2_jumps_without_averaging_none_with_nodal() {
    let _g = start();
    let g = model2();
    // Element values: A = 1, B = 3 kN·m/m at every corner.
    let t = tensors(&g, |_, e| [0.0, 0.0, 0.0, if e == 0 { 1000.0 } else { 3000.0 }, 0.0, 0.0, 0.0, 0.0]);
    let d = diagram(&g, &along_x(), &t, M11, &[]);
    let at1: Vec<f64> = d.pieces.iter().flatten().filter(|s| (s.s - 1.0).abs() < 1e-9).map(|s| s.value).collect();
    assert!(at1.contains(&1.0) && at1.contains(&3.0), "a jump at x = 1 (R3): {at1:?}");

    // The same from a stored result block, averaged at the joints: 2 on both sides of x = 1.
    let block = write_block(&g, |e| if e == 0 { 1000.0 } else { 3000.0 });
    let choice = Choice { load: Some("L".into()), averaging: Averaging::Nodal, ..Choice::default() };
    let t = cut::tensors(&g, std::slice::from_ref(&block), &choice, &[], &[0, 1]).expect("read");
    let d = diagram(&g, &along_x(), &t, M11, &[]);
    for st in d.pieces.iter().flatten().filter(|s| (s.s - 1.0).abs() < 1e-9) {
        assert!((st.value - 2.0).abs() < 1e-9, "nodal: no jump, {}", st.value);
    }
    // Element averaging from the block: the jump again.
    let t = cut::tensors(&g, &[block], &Choice { averaging: Averaging::Element, ..choice }, &[], &[0, 1]).expect("read");
    assert_eq!(t[&0][1][M11], 1000.0);
    assert_eq!(t[&1][0][M11], 3000.0);
}

/// A shell result block as the transfer stores it: element, joint, step_type, step_num, F11 … V23.
fn write_block(g: &Geometry, m11: impl Fn(usize) -> f64) -> ResultBlock {
    let mut rows: Vec<(u32, u32, [f64; 8])> = Vec::new();
    for (e, s) in g.shells.iter().enumerate() {
        for j in &s[..4] {
            rows.push((e as u32, *j, [0.0, 0.0, 0.0, m11(e), 0.0, 0.0, 0.0, 0.0]));
        }
    }
    let mut bytes = Vec::new();
    for r in &rows {
        bytes.extend(r.0.to_le_bytes());
    }
    for r in &rows {
        bytes.extend(r.1.to_le_bytes());
    }
    for _ in &rows {
        bytes.extend(0u32.to_le_bytes());
    }
    for _ in &rows {
        bytes.extend(0f64.to_le_bytes());
    }
    for c in 0..8 {
        for r in &rows {
            bytes.extend(r.2[c].to_le_bytes());
        }
    }
    let file = std::env::temp_dir().join(format!("factory_cut_test_{}.bin", std::process::id()));
    std::fs::write(&file, bytes).expect("temp file");
    let mut columns: Vec<String> = ["element:u32", "joint:u32", "step_type:u32", "step_num:f64"].map(String::from).to_vec();
    columns.extend(["F11", "F22", "F12", "M11", "M22", "M12", "V13", "V23"].map(|c| format!("{c}:f64")));
    ResultBlock { name: "L".into(), combo: false, family: "shell_results".into(), rows: rows.len(), columns, file }
}

#[test]
fn c3_shells_in_the_plane_and_hidden_shells_are_not_cut() {
    let _g = start();
    let g = model2();
    let t = tensors(&g, |_, _| [0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    // The plane z = 0 itself: the line along x, extruded along y.
    let flat = Cut { direction: Direction::Along([0.0, 1.0, 0.0]), ..along_x() };
    let d = diagram(&g, &flat, &t, M11, &[]);
    assert!(d.pieces.is_empty());
    assert!(d.notes.iter().any(|n| n.contains("2 shells lie in the cut plane")), "{:?}", d.notes);
    // Shell A hidden: only B is cut.
    let d = diagram(&g, &along_x(), &t, M11, &[false, true]);
    assert!(!d.pieces.is_empty() && d.pieces.iter().all(|p| p[0].shell == 1));
}

#[test]
fn c4_resultant_of_a_constant_value() {
    let _g = start();
    let g = model2();
    let t = tensors(&g, |_, _| [0.0, 0.0, 0.0, 10_000.0, 0.0, 0.0, 0.0, 0.0]); // 10 kN·m/m
    let d = diagram(&g, &along_x(), &t, M11, &[]);
    let (r, unit) = d.resultant.expect("M11 is per length");
    assert!((r - 20.0).abs() < 1e-9, "10 kN·m/m × 2 m = 20 kN·m, got {r}");
    assert_eq!(unit, "kN·m");
    // Principal values are not integrated (C7).
    assert!(diagram(&g, &along_x(), &t, 11, &[]).resultant.is_none());
}

#[test]
fn c5_principal_values_after_interpolation() {
    let _g = start();
    let g = model2();
    // F12 = +1 kN/m at x = 0, −1 at x = 1: in the middle F12 = 0, so Fmax = 0 there (interpolated
    // principal values would give 1).
    let t = tensors(&g, |p, _| [0.0, 0.0, 1000.0 * (1.0 - 2.0 * p[0]), 0.0, 0.0, 0.0, 0.0, 0.0]);
    let d = diagram(&g, &along_x(), &t, 8, &[]);
    let mid = d.pieces.iter().flatten().find(|s| (s.s - 0.5).abs() < 1e-9).expect("a station at x = 0.5 (the diagonal)");
    assert!(mid.value.abs() < 1e-9, "Fmax at x = 0.5: {}", mid.value);
}

#[test]
fn c5b_cut_axes_follow_the_cut_direction() {
    let _g = start();
    let g = model2();
    // M11 = 1, M22 = 5, M12 = 0 kN·m/m.
    let t = tensors(&g, |_, _| [0.0, 0.0, 0.0, 1000.0, 5000.0, 0.0, 0.0, 0.0]);
    let mn = CUT_AXES + 2;
    // Cut along x: its normal is along y, so Mn = M22.
    let d = diagram(&g, &along_x(), &t, mn, &[]);
    assert!(d.pieces.iter().flatten().all(|s| (s.value - 5.0).abs() < 1e-9));
    // Cut along y: Mn = M11.
    let across = Cut { p1: [0.5, 0.0, 0.0], p2: [0.5, 1.0, 0.0], ..along_x() };
    let d = diagram(&g, &across, &t, mn, &[]);
    assert!(!d.pieces.is_empty() && d.pieces.iter().flatten().all(|s| (s.value - 1.0).abs() < 1e-9));
    // At 45°: Mn = (M11 + M22) / 2 = 3, Mnt = (M22 − M11) / 2 · sign.
    let skew = Cut { p1: [0.0, 0.0, 0.0], p2: [1.0, 1.0, 0.0], ..along_x() };
    let d = diagram(&g, &skew, &t, mn, &[]);
    assert!(!d.pieces.is_empty() && d.pieces.iter().flatten().all(|s| (s.value - 3.0).abs() < 1e-9));
    let d = diagram(&g, &skew, &t, CUT_AXES + 3, &[]);
    assert!(d.pieces.iter().flatten().all(|s| (s.value.abs() - 2.0).abs() < 1e-9));
    // No local axes: left out with a note.
    let mut bare = model2();
    bare.shell_axes = vec![[f64::NAN; 9]; 2];
    let d = diagram(&bare, &along_x(), &t, mn, &[]);
    assert!(d.pieces.is_empty() && d.notes.iter().any(|n| n.contains("no local axes")));
}

#[test]
fn c6_the_table() {
    let _g = start();
    let g = model2();
    let t = tensors(&g, |p, _| [0.0, 0.0, 0.0, 1000.0 * p[0], 0.0, 0.0, 0.0, 0.0]);
    let d = diagram(&g, &along_x(), &t, M11, &[]);
    let rows = cut::table(&g, &d, Units::default());
    assert_eq!(rows.len(), 8, "entry and exit of 4 pieces");
    assert!(rows.windows(2).all(|w| w[0].0 <= w[1].0 + 1e-12), "stations increase");
    assert_eq!(rows[0].2, "A");
    assert_eq!(rows[7].2, "B");
    assert!((d.max().expect("max").value - 2.0).abs() < 1e-9 && d.min().expect("min").value.abs() < 1e-9);
    let text = cut::table_text(&g, &d, Units::default(), '\t');
    assert!(text.starts_with("Station [m]\tX [m]") && text.contains("Resultant [kN·m]"));
}

#[test]
fn c7_hostile_input_gives_no_cut() {
    let _g = start();
    assert!(cut::add(None, [1.0; 3], [1.0; 3], Direction::Vertical).is_none(), "equal points");
    assert!(cut::add(None, [0.0; 3], [f64::NAN, 0.0, 0.0], Direction::Vertical).is_none(), "NaN");
    assert!(cut::add(None, [0.0; 3], [0.0, 0.0, 5.0], Direction::Vertical).is_none(), "line along the extrusion");
    assert!(cut::add(None, [0.0; 3], [1.0, 0.0, 0.0], Direction::Along([0.0; 3])).is_none(), "no direction");
    assert!(!cut::rename("nothing", "x"));
    assert!(!cut::delete("nothing"));
    let a = cut::add(None, [0.0; 3], [1.0, 0.0, 0.0], Direction::Vertical).expect("a cut");
    assert_eq!(a, "Cut 1");
    assert!(cut::add(Some("Cut 1"), [0.0; 3], [2.0, 0.0, 0.0], Direction::Vertical).is_none(), "name taken");
    assert!(!cut::rename("Cut 1", "  "), "empty name");
    // The mode: two clicks make a cut and leave the mode.
    cut::set_mode(Some(cut::Mode::default()));
    assert_eq!(cut::click([0.0; 3]), None);
    assert_eq!(cut::click([0.0, 3.0, 0.0]).as_deref(), Some("Cut 2"));
    assert!(cut::mode().is_none());
    // 3 points: a third click in line with the first two makes no cut; the mode waits for another.
    cut::set_mode(Some(cut::Mode { kind: cut::Kind::ThreePoints, ..cut::Mode::default() }));
    assert_eq!(cut::click([0.0; 3]), None);
    assert_eq!(cut::click([1.0, 0.0, 0.0]), None);
    assert_eq!(cut::click([2.0, 0.0, 0.0]), None, "in one line");
    assert_eq!(cut::mode().map(|m| m.points.len()), Some(2));
    assert_eq!(cut::click([0.0, 0.0, 1.0]).as_deref(), Some("Cut 3"));
    assert_eq!(cut::get("Cut 3").map(|c| c.direction), Some(Direction::Plane3([0.0, 0.0, 1.0])));
    // Redraw keeps the cut's type.
    assert_eq!(cut::Mode::redraw("Cut 3").kind, cut::Kind::ThreePoints);
}

#[test]
fn c8_kept_per_model_and_shown_per_view() {
    let _g = start();
    let a = cut::add(Some("Deck"), [0.0; 3], [1.0, 0.0, 0.0], Direction::Vertical).expect("a cut");
    // Another model does not show it.
    model::write(|f| f.model.file = "other.bdb".into());
    assert!(cut::cuts().is_empty());
    model::write(|f| f.model.file = "test.bdb".into());
    assert_eq!(cut::cuts().len(), 1);
    // Hidden in this view: in its V/G settings (a template rules it, R6).
    cut::show_in_view(&a, false);
    assert!(!cut::shown_in_view(&a));
    assert_eq!(vv::settings().cuts_hidden, vec!["Deck".to_string()]);
    cut::show_in_view(&a, true);
    assert!(cut::shown_in_view(&a));
    // Positive drawn (§13): Up by default, also for V/G settings kept before it existed; a view
    // template carries it with Visibility/Graphics.
    assert!(!vv::Settings::default().cut_flip);
    let old: vv::Settings = serde_json::from_str(r#"{"contour":true,"cut_plain":true}"#).expect("old V/G");
    assert!(old.cut_plain && !old.cut_flip);
    let mut s = vv::settings();
    s.cut_flip = true;
    vv::set_quiet(s);
    assert!(windows::save_template("Flipped", windows::current().active));
    assert_eq!(windows::template("Flipped").map(|t| t.state.settings.cut_flip), Some(true));
    windows::delete_template("Flipped");
    vv::set_quiet(vv::Settings::default());
    // Kept in Factory's file with the windows (§4).
    let text = serde_json::to_string(&windows::stored()).expect("JSON");
    cut::set_all(Vec::new());
    let back: windows::Stored = serde_json::from_str(&text).expect("read back");
    windows::restore(&back);
    assert_eq!(cut::get("Deck").map(|c| c.p2), Some([1.0, 0.0, 0.0]));
    // A file without cuts reads as none.
    let old: windows::Stored = serde_json::from_str(r#"{"layout":"1"}"#).expect("old file");
    assert!(old.cuts.is_empty());
    // Properties: a fixed component and a new name.
    let mut c = cut::get("Deck").expect("there");
    c.component = Some(CUT_AXES + 2);
    c.name = "Deck M".into();
    assert!(cut::replace("Deck", c));
    assert_eq!(cut::get("Deck M").and_then(|c| c.component), Some(CUT_AXES + 2));
    assert!(cut::get("Deck").is_none());
}

// ---- §12: cut planes of any direction ----

/// A vertical web in the plane y = `y`: x 0..2, z 0..1; local axes 1 = X, 2 = Z, 3 = −Y.
const WEB_AXES: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0];

fn webs(ys: &[f64]) -> Geometry {
    let mut g = Geometry::default();
    for (k, y) in ys.iter().enumerate() {
        let b = (k * 4) as u32;
        g.joints.extend([[0.0, *y, 0.0], [2.0, *y, 0.0], [2.0, *y, 1.0], [0.0, *y, 1.0]]);
        g.shells.push([b, b + 1, b + 2, b + 3, 0]);
        g.shell_names.push(format!("W{k}"));
        g.shell_axes.push(WEB_AXES);
    }
    g
}

#[test]
fn c9_horizontal_cut_through_a_web() {
    let _g = start();
    let g = webs(&[0.0]);
    let t = tensors(&g, |p, _| [0.0, 0.0, 0.0, 1000.0 * p[0], 0.0, 0.0, 0.0, 0.0]);
    // Point 2 at another level: the plane is at point 1's level.
    let c = Cut { name: "h".into(), p1: [0.0, 0.0, 0.5], p2: [2.0, 0.0, 0.9], direction: Direction::Horizontal, ..Cut::default() };
    let f = c.frame().expect("a plane");
    assert_eq!(f.n, [0.0, 0.0, 1.0]);
    assert!((f.length - 2.0).abs() < 1e-12);
    let d = diagram(&g, &c, &t, M11, &[]);
    assert!(!d.pieces.is_empty());
    for st in d.pieces.iter().flatten() {
        assert!((st.at[2] - 0.5).abs() < 1e-9, "on the level of point 1");
        assert!((st.value - st.s).abs() < 1e-9, "M11 = x");
    }
    // A vertical cut along the same web lies in it (skipped): why the horizontal type is needed.
    let v = Cut { direction: Direction::Vertical, ..c };
    assert!(diagram(&g, &v, &t, M11, &[]).pieces.is_empty());
}

#[test]
fn c10_three_points_through_an_inclined_plate() {
    let _g = start();
    // A plate rising 1 in 1 along x.
    let g = Geometry {
        joints: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 0.0]],
        shells: vec![[0, 1, 2, 3, 0]],
        shell_names: vec!["P".into()],
        shell_axes: vec![IDENTITY],
        ..Default::default()
    };
    let t = tensors(&g, |_, _| [0.0, 0.0, 0.0, 2000.0, 0.0, 0.0, 0.0, 0.0]);
    // Line up the slope at y = 0.5; point 3 straight above point 1: the plane y = 0.5.
    let c = Cut { name: "s".into(), p1: [0.0, 0.5, 0.0], p2: [1.0, 0.5, 1.0], direction: Direction::Plane3([0.0, 0.5, 3.0]), ..Cut::default() };
    let f = c.frame().expect("a plane");
    assert!((f.n[1].abs() - 1.0).abs() < 1e-12, "normal along y: {:?}", f.n);
    assert!((f.length - 2f64.sqrt()).abs() < 1e-12, "along the slope");
    let d = diagram(&g, &c, &t, M11, &[]);
    let last = d.pieces.last().expect("cut")[1].s;
    assert!((last - 2f64.sqrt()).abs() < 1e-9);
    let (r, _) = d.resultant.expect("integrable");
    assert!((r - 2.0 * 2f64.sqrt()).abs() < 1e-9, "2 kN·m/m over √2 m");
    // 3 points in one line: no plane.
    assert!(Cut { direction: Direction::Plane3([2.0, 0.5, 2.0]), ..c.clone() }.frame().is_none());
    assert!(Cut { direction: Direction::Plane3([f64::NAN, 0.0, 0.0]), ..c }.frame().is_none());
}

#[test]
fn c11_global_plane_equals_the_drawn_cut() {
    let _g = start();
    let (p1, p2, d) = cut::global("XZ", 0.5, 'X', 0.0, 2.0, 0.0).expect("XZ");
    assert_eq!((p1, p2, d), (along_x().p1, along_x().p2, Direction::Vertical));
    let (p1, p2, d) = cut::global("yz", 1.5, 'X', 0.0, 1.0, 0.0).expect("YZ");
    assert_eq!((p1, p2, d), ([1.5, 0.0, 0.0], [1.5, 1.0, 0.0], Direction::Vertical));
    let (p1, p2, d) = cut::global("XY", 0.5, 'Y', -1.0, 4.0, 2.0).expect("XY along Y");
    assert_eq!((p1, p2, d), ([2.0, -1.0, 0.5], [2.0, 4.0, 0.5], Direction::Horizontal));
    assert!(cut::global("XX", 0.0, 'X', 0.0, 1.0, 0.0).is_none());
    assert!(cut::global("XY", 0.0, 'Z', 0.0, 1.0, 0.0).is_none());
    // The XY form gives the same diagram as c9's horizontal cut.
    let g = webs(&[0.0]);
    let t = tensors(&g, |p, _| [0.0, 0.0, 0.0, 1000.0 * p[0], 0.0, 0.0, 0.0, 0.0]);
    let (p1, p2, direction) = cut::global("XY", 0.5, 'X', 0.0, 2.0, 0.0).expect("XY");
    let d = diagram(&g, &Cut { name: "g".into(), p1, p2, direction, ..Cut::default() }, &t, M11, &[]);
    assert!((d.pieces.last().expect("cut")[1].value - 2.0).abs() < 1e-9);
}

#[test]
fn c12_mn_on_a_horizontal_cut_uses_the_plane_normal() {
    let _g = start();
    let g = webs(&[0.0]);
    // M11 = 1 (about the web's axis 1 = X), M22 = 5 (axis 2 = Z).
    let t = tensors(&g, |_, _| [0.0, 0.0, 0.0, 1000.0, 5000.0, 0.0, 0.0, 0.0]);
    let c = Cut { name: "h".into(), p1: [0.0, 0.0, 0.5], p2: [2.0, 0.0, 0.5], direction: Direction::Horizontal, ..Cut::default() };
    let d = diagram(&g, &c, &t, CUT_AXES + 2, &[]);
    assert!(!d.pieces.is_empty() && d.pieces.iter().flatten().all(|s| (s.value - 5.0).abs() < 1e-9), "normal Z = local axis 2: Mn = M22");
}

#[test]
fn c13_only_near_the_line() {
    let _g = start();
    let g = webs(&[0.0, 3.0]);
    let t = tensors(&g, |_, _| [0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, 0.0, 0.0]);
    let c = Cut { name: "h".into(), p1: [0.0, 0.0, 0.5], p2: [2.0, 0.0, 0.5], direction: Direction::Horizontal, ..Cut::default() };
    let both = diagram(&g, &c, &t, M11, &[]);
    assert_eq!(both.pieces.iter().map(|p| p[0].shell).collect::<std::collections::BTreeSet<_>>().len(), 2, "the whole plane: both webs");
    assert!((both.resultant.expect("sum").0 - 4.0).abs() < 1e-9, "1 × 2 m per web, summed");
    let near = diagram(&g, &Cut { band: Some(1.0), ..c }, &t, M11, &[]);
    assert!(!near.pieces.is_empty() && near.pieces.iter().all(|p| p[0].shell == 0), "only the web at the line");
}

#[test]
fn c14_kept_phase_1_cuts_read_back() {
    let _g = start();
    let old: Cut = serde_json::from_str(r#"{"name":"a","model":"m","p1":[0,0,0],"p2":[1,0,0],"direction":"Vertical","component":null,"scale_px":60.0,"text":true,"all_values":false,"fill":true}"#).expect("phase 1 cut");
    assert_eq!((old.direction, old.draw_normal, old.band), (Direction::Vertical, false, None));
    let along: Cut = serde_json::from_str(r#"{"name":"b","p1":[0,0,0],"p2":[1,0,0],"direction":{"Along":[0.0,0.0,1.0]}}"#).expect("along");
    assert_eq!(along.direction, Direction::Along([0.0, 0.0, 1.0]));
    // And the new types go and come back.
    for d in [Direction::Horizontal, Direction::Plane3([1.0, 2.0, 3.0])] {
        let c = Cut { direction: d, band: Some(0.5), draw_normal: true, ..old.clone() };
        let back: Cut = serde_json::from_str(&serde_json::to_string(&c).expect("JSON")).expect("back");
        assert_eq!(back, c);
    }
}
