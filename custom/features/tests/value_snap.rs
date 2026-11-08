//! Value snap (docs/11-visualization-options.md): the corner / station under the cursor.

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
    };
    (g, d)
}

#[test]
fn snaps_to_the_nearest_corner_of_the_shell_closest_to_the_viewer() {
    let (g, d) = model();
    // cursor near corner n2 (1, 0) on screen: (100, 0) – a bit inside the square
    let hit = snap(&g, &d, [90.0, -10.0], &project, EYE, 10.0).unwrap();
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
