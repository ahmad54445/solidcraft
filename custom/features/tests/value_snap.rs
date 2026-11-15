//! Value snap (docs/11-visualization-options.md, docs/10 §1.1): a value wherever the cursor is
//! on a shown shell – the corner near a corner, else interpolated – and frame stations.

use custom_features::results_view::{SnapData, snap};
use custom_features::transfer::Geometry;

/// Plan view, 100 px per metre, y up on the model = up on the screen; camera above (z = 10 m).
fn project(p: [f64; 3]) -> Option<[f32; 2]> {
    Some([(p[0] * 100.0) as f32, (-p[1] * 100.0) as f32])
}
const EYE: [f64; 3] = [0.0, 0.0, 10.0];

fn model() -> (Geometry, SnapData) {
    let g = Geometry {
        // shell 0: 1 m square at z = 0; shell 1: same square raised to z = 1 (closer to the eye)
        joints: vec![
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0],
        ],
        shells: vec![[0, 1, 2, 3, 0], [4, 5, 6, 7, 0]],
        ..Default::default()
    };
    let d = SnapData {
        shell_values: vec![[1.0, 2.0, 3.0, 4.0], [10.0, 20.0, 30.0, 40.0]],
        frame_points: Vec::new(),
        shell_unit: "kN·m/m".into(),
        frame_unit: "kN·m".into(),
        ..Default::default()
    };
    (g, d)
}

#[test]
fn snaps_to_the_nearest_corner_of_the_shell_closest_to_the_viewer() {
    let (g, d) = model();
    // cursor near corner n2 (1, 0) on screen: (100, 0) – 7 px inside the square (radius 10)
    let hit = snap(&g, &d, [95.0, -5.0], &project, EYE, 10.0).unwrap();
    // both squares are under the cursor; the raised one (shell 1) is closer to the eye
    assert_eq!(hit.value, 20.0);
    assert_eq!(hit.at, [1.0, 0.0, 1.0]);
    assert_eq!(hit.unit, "kN·m/m");
}

#[test]
fn nothing_outside_the_elements() {
    let (g, d) = model();
    assert!(snap(&g, &d, [300.0, 300.0], &project, EYE, 10.0).is_none());
}

#[test]
fn a_frame_station_within_the_radius_wins_when_closer() {
    let (g, mut d) = model();
    d.frame_points = vec![([0.5, 0.5, 2.0], 7.5)]; // above both squares
    let hit = snap(&g, &d, [52.0, -48.0], &project, EYE, 10.0).unwrap();
    assert_eq!((hit.value, hit.unit.as_str()), (7.5, "kN·m"));
    // outside the radius: back to the shell
    let hit = snap(&g, &d, [80.0, -80.0], &project, EYE, 10.0).unwrap();
    assert_eq!(hit.unit, "kN·m/m");
}

// ---- docs/10 §1.1 (rules S1–S5): a number on every shell ------------------------------------

/// Point (0.6, 0.2) of the unit square: in triangle [n1 n2 n3], weights n1 0.4, n2 0.4, n3 0.2;
/// 40 px and more from every corner.
const INSIDE: [f32; 2] = [60.0, -20.0];

#[test]
fn inside_a_shell_the_value_is_interpolated_like_the_contour() {
    let (g, d) = model();
    let hit = snap(&g, &d, INSIDE, &project, EYE, 10.0).unwrap();
    // raised shell 1 (10, 20, 30, 40): 0.4·10 + 0.4·20 + 0.2·30 = 18
    assert!((hit.value - 18.0).abs() < 1e-4, "{}", hit.value);
    assert!(!hit.corner);
    assert_eq!(hit.shell, Some(1));
    // the point under the cursor, on the shell's plane
    assert!((hit.at[0] - 0.6).abs() < 1e-6 && (hit.at[1] - 0.2).abs() < 1e-6 && (hit.at[2] - 1.0).abs() < 1e-9, "{:?}", hit.at);
    // the centre lies on the diagonal: both triangles give (10 + 30) / 2
    let hit = snap(&g, &d, [50.0, -50.0], &project, EYE, 10.0).unwrap();
    assert!((hit.value - 20.0).abs() < 1e-4);
}

#[test]
fn on_an_edge_and_in_perspective_the_value_follows_the_surface() {
    let (g, d) = model();
    // the edge n1–n2 at x = 0.3: 0.7·10 + 0.3·20 = 13
    let hit = snap(&g, &d, [30.0, 0.0], &project, EYE, 10.0).unwrap();
    assert!((hit.value - 13.0).abs() < 1e-3, "{}", hit.value);
    // A perspective camera above the squares: the point is found on the plane, not in screen space.
    let eye = [0.2, 0.3, 5.0];
    let persp = move |p: [f64; 3]| {
        let z = eye[2] - p[2];
        (z > 1e-9).then(|| [((p[0] - eye[0]) / z * 800.0) as f32, (-(p[1] - eye[1]) / z * 800.0) as f32])
    };
    let target = [0.6, 0.2, 1.0];
    let cursor = persp(target).unwrap();
    let hit = snap(&g, &d, cursor, &persp, eye, 10.0).unwrap();
    assert!((hit.at[0] - 0.6).abs() < 1e-4 && (hit.at[1] - 0.2).abs() < 1e-4, "{:?}", hit.at);
    assert!((hit.value - 18.0).abs() < 1e-3, "{}", hit.value);
}

#[test]
fn a_shell_without_result_still_answers_with_the_reason() {
    let (g, mut d) = model();
    d.shell_values[1] = [f32::NAN; 4];
    let hit = snap(&g, &d, INSIDE, &project, EYE, 10.0).unwrap();
    assert_eq!(hit.shell, Some(1));
    assert!(hit.value.is_nan());
    assert_eq!(hit.note, Some("no result"));
    // halftone / colour override: not contoured
    d.shell_contoured = vec![true, false];
    assert_eq!(snap(&g, &d, INSIDE, &project, EYE, 10.0).unwrap().note, Some("not contoured"));
    // no result chosen at all (contour off): the shell, no value, no note
    d.shell_values.clear();
    let hit = snap(&g, &d, INSIDE, &project, EYE, 10.0).unwrap();
    assert_eq!((hit.shell, hit.note), (Some(1), None));
    assert!(hit.value.is_nan());
}

#[test]
fn a_hidden_shell_is_looked_through() {
    let (g, mut d) = model();
    d.shell_shown = vec![true, false]; // the raised shell is hidden (rule G5)
    let hit = snap(&g, &d, INSIDE, &project, EYE, 10.0).unwrap();
    assert_eq!(hit.shell, Some(0));
    // shell 0 (1, 2, 3, 4): 0.4·1 + 0.4·2 + 0.2·3 = 1.8
    assert!((hit.value - 1.8).abs() < 1e-4, "{}", hit.value);
}

#[test]
fn extruded_the_snap_is_on_the_top_face() {
    let (mut g, mut d) = model();
    g.shell_thickness = vec![0.4];
    d.shell_shown = vec![true, false];
    d.view.view_type = custom_features::vv::ViewType::Extrude;
    let hit = snap(&g, &d, INSIDE, &project, EYE, 10.0).unwrap();
    // faces at z = ±0.2; the eye is above, so the top one
    assert!((hit.at[2] - 0.2).abs() < 1e-6, "{:?}", hit.at);
    assert!((hit.value - 1.8).abs() < 1e-4);
}

#[test]
fn the_index_finds_the_same_shell_as_testing_all() {
    use custom_features::results_view::{SnapIndex, snap_indexed};
    let (g, d) = model();
    let index = SnapIndex::new(&g, &d, &project);
    for cursor in [INSIDE, [50.0, -50.0], [5.0, -95.0], [300.0, 300.0]] {
        assert_eq!(snap_indexed(&g, &d, &index, cursor, &project, EYE, 10.0), snap(&g, &d, cursor, &project, EYE, 10.0));
    }
}
