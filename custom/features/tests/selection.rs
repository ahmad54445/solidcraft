//! Selection tools (docs/13-selection.md): T1–T7, T9–T11, C1–C4 and rules R1–R4, R8.

use custom_features::results_view::SnapData;
use custom_features::selection::{self, Area, Kinds, Parallel, Scope, label_matches};
use custom_features::transfer::Geometry;

/// Plan view, 100 px per metre, y up = up on screen; eye above.
fn project(p: [f64; 3]) -> Option<[f32; 2]> {
    Some([(p[0] * 100.0) as f32, (-p[1] * 100.0) as f32])
}
const EYE: [f64; 3] = [0.0, 0.0, 10.0];

/// Two unit shells side by side (x 0–1 and 1–2), frame F1 along x at y = 2, frame F2 along z at
/// (3, 0), frame F3 along y at x = 4.
fn model() -> Geometry {
    Geometry {
        joints: vec![
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0],
            [0.0, 2.0, 0.0], [2.0, 2.0, 0.0], [3.0, 0.0, 0.0], [3.0, 0.0, 3.0], [4.0, 0.0, 0.0], [4.0, 1.0, 0.0],
        ],
        shells: vec![[0, 1, 2, 3, 0], [1, 4, 5, 2, 1]],
        frames: vec![[6, 7, 0], [8, 9, 1], [10, 11, 0]],
        shell_names: vec!["A101".into(), "A102".into()],
        frame_names: vec!["F1".into(), "F2".into(), "F3".into()],
        shell_sections: vec!["SLAB30".into(), "SLAB40".into()],
        shell_thickness: vec![0.3, 0.4],
        frame_sections: vec!["IPE300".into(), "COL".into()],
        ..Default::default()
    }
}

fn scope(g: &Geometry) -> Scope<'_> {
    Scope { g, shell_shown: &[], frame_shown: &[], kinds: Kinds::default() }
}

fn ids(f: &selection::Found) -> (Vec<usize>, Vec<usize>) {
    (f.set.shells.ones().collect(), f.set.frames.ones().collect())
}

#[test]
fn t1_window_takes_whole_elements_crossing_takes_touched_ones() {
    let g = model();
    let d = SnapData::default();
    // box over shell 0 entirely and half of shell 1: x 0–150 px, y -110–10 px
    let area = Area::Rect { min: [-10.0, -110.0], max: [150.0, 10.0] };
    assert_eq!(ids(&selection::in_area(scope(&g), &d, &area, false, &project)), (vec![0], vec![]));
    assert_eq!(ids(&selection::in_area(scope(&g), &d, &area, true, &project)), (vec![0, 1], vec![]));
    // a small box inside shell 0: crossing takes the shell around it, window nothing
    let inner = Area::Rect { min: [40.0, -60.0], max: [60.0, -40.0] };
    assert_eq!(ids(&selection::in_area(scope(&g), &d, &inner, true, &project)).0, vec![0]);
    assert!(selection::in_area(scope(&g), &d, &inner, false, &project).set.is_empty());
    // a crossing box over the middle of frame F1 (no end inside)
    let mid = Area::Rect { min: [90.0, -210.0], max: [110.0, -190.0] };
    assert_eq!(ids(&selection::in_area(scope(&g), &d, &mid, true, &project)).1, vec![0]);
}

#[test]
fn t1_click_takes_the_shell_or_the_frame_under_the_cursor() {
    let g = model();
    let d = SnapData::default();
    assert_eq!(ids(&selection::at_point(scope(&g), &d, [150.0, -50.0], &project, EYE, 6.0)), (vec![1], vec![]));
    assert_eq!(ids(&selection::at_point(scope(&g), &d, [100.0, -203.0], &project, EYE, 6.0)), (vec![], vec![0]));
    assert!(selection::at_point(scope(&g), &d, [900.0, 900.0], &project, EYE, 6.0).set.is_empty());
}

#[test]
fn t2_t3_t4_polygons_and_lines() {
    let g = model();
    let d = SnapData::default();
    // a pentagon round shell 0 only (shell 1 has two corners on its edge x = 120 px … inside)
    let poly = Area::Polygon(vec![[-20.0, 20.0], [120.0, 20.0], [130.0, -50.0], [120.0, -120.0], [-20.0, -120.0]]);
    let window = selection::in_area(scope(&g), &d, &poly, false, &project);
    assert!(window.set.shells.get(0) && !window.set.shells.get(1));
    let crossing = selection::in_area(scope(&g), &d, &poly, true, &project);
    assert!(crossing.set.shells.get(0) && crossing.set.shells.get(1));
    // a line across both shells at y = 0.5
    let line = [[-10.0, -50.0], [250.0, -50.0]];
    assert_eq!(ids(&selection::crossed_by_line(scope(&g), &d, &line, &project)), (vec![0, 1], vec![]));
}

#[test]
fn t5_coordinates_box_and_plane() {
    let g = model();
    let found = selection::in_coordinates(scope(&g), [Some((0.0, 1.0)), None, None], 1e-6);
    assert_eq!(ids(&found), (vec![0], vec![]));
    // plane z = 0: both shells, F1 and F3 (F2 goes up)
    let plane = selection::in_coordinates(scope(&g), [None, None, Some((0.0, 0.0))], 1e-3);
    assert_eq!(ids(&plane), (vec![0, 1], vec![0, 2]));
}

#[test]
fn t6_parallel_to_an_axis_or_a_plane() {
    let g = model();
    assert_eq!(ids(&selection::parallel(scope(&g), Parallel::To([1.0, 0.0, 0.0]), 1.0)).1, vec![0]);
    assert_eq!(ids(&selection::parallel(scope(&g), Parallel::To([0.0, 0.0, -1.0]), 1.0)).1, vec![1]);
    assert_eq!(ids(&selection::parallel(scope(&g), Parallel::Plane([0.0, 0.0, 1.0]), 1.0)).1, vec![0, 2]);
}

#[test]
fn t7_properties_sections_and_thickness() {
    let g = model();
    assert_eq!(ids(&selection::with_properties(scope(&g), &[1], &[1], None)), (vec![1], vec![1]));
    assert_eq!(ids(&selection::with_properties(scope(&g), &[], &[], Some((0.25, 0.35)))), (vec![0], vec![]));
}

#[test]
fn t9_t10_groups_and_labels() {
    let g = model();
    assert!(label_matches("101-150", "101") && label_matches("A101-A150", "a120") && !label_matches("A101-A150", "B120"));
    assert!(label_matches("A1*", "A102") && label_matches("F?", "F3") && !label_matches("F?", "F33"));
    assert_eq!(ids(&selection::with_labels(scope(&g), "A102, F1-F2")), (vec![1], vec![0, 1]));
    assert_eq!(ids(&selection::in_groups(scope(&g), &["A101".into(), "F3".into()])), (vec![0], vec![2]));
}

#[test]
fn r1_r2_kinds_and_hidden_elements() {
    let g = model();
    let hidden = [true, false];
    let s = Scope { g: &g, shell_shown: &hidden, frame_shown: &[], kinds: Kinds { shells: true, frames: false } };
    let found = selection::all(s);
    assert_eq!(ids(&found), (vec![0], vec![]));
    assert_eq!(found.hidden, 1);
}

/// The global selection: one test, so no other test changes it meanwhile.
#[test]
fn r3_r8_c1_c4_add_remove_invert_clear_previous() {
    let g = model();
    selection::reset();
    selection::apply(selection::with_labels(scope(&g), "A101"), false);
    selection::apply(selection::with_labels(scope(&g), "F1"), false);
    assert_eq!(selection::current().counts(), (1, 1), "adds (R3)");
    selection::apply(selection::with_labels(scope(&g), "A101"), true);
    assert_eq!(selection::current().counts(), (0, 1), "Ctrl removes (C1)");
    selection::set_deselect_mode(true);
    selection::apply(selection::with_labels(scope(&g), "F1"), false);
    selection::set_deselect_mode(false);
    assert_eq!(selection::current().counts(), (0, 0), "deselect mode removes (C1)");
    selection::invert(scope(&g));
    assert_eq!(selection::current().counts(), (2, 3), "invert (C2)");
    selection::previous();
    assert_eq!(selection::current().counts(), (0, 0), "previous (C4)");
    selection::apply(selection::all(scope(&g)), false);
    selection::clear();
    assert!(selection::current().is_empty(), "clear (C3)");
    assert_eq!(selection::names(&g, &selection::with_labels(scope(&g), "A1*").set), vec!["A101", "A102"]);
    selection::reset();
}

#[test]
fn r4_a_shrunk_frame_is_selected_as_drawn() {
    let g = model();
    let mut d = SnapData::default();
    d.view.shrink = true;
    // F1 runs 0–2 m in x at y = 2; drawn shrunk it covers 0.15–1.85 m (15–185 px)
    let inner = Area::Rect { min: [10.0, -210.0], max: [190.0, -190.0] };
    assert_eq!(ids(&selection::in_area(scope(&g), &d, &inner, false, &project)).1, vec![0]);
    d.view.shrink = false;
    assert!(selection::in_area(scope(&g), &d, &inner, false, &project).set.frames.ones().next().is_none());
}
