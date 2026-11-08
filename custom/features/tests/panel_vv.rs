//! Results panel computations (docs/10 §7) and Visibility/Graphics looks (docs/12).

use custom_features::results_view::{Averaging, Choice, build_with, shell_component};
use custom_features::transfer::{Geometry, ResultBlock};
use custom_features::units::Units;
use custom_features::vv::{Action, Filter, Look, Rule, Settings, looks};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn principal_values_and_vmax() {
    // Fxx = 3, Fyy = -1, Fxy = 0 → principal 3 and -1; |F|max = 3
    let v = [3.0, -1.0, 0.0, 0.0, 0.0, 0.0, 3.0, 4.0];
    assert!(close(shell_component(8, v), 3.0));
    assert!(close(shell_component(9, v), -1.0));
    assert!(close(shell_component(10, v), 3.0));
    // pure shear Fxy = 2 → ±2
    let s = [0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    assert!(close(shell_component(8, s), 2.0) && close(shell_component(9, s), -2.0));
    // |M|max keeps the sign of the larger magnitude: Mxx = -5, Myy = 1 → -5
    let m = [0.0, 0.0, 0.0, -5.0, 1.0, 0.0, 0.0, 0.0];
    assert!(close(shell_component(13, m), -5.0));
    // VMax = √(3² + 4²)
    assert!(close(shell_component(14, v), 5.0));
    // stored components pass through
    assert!(close(shell_component(3, m), -5.0));
}

/// Two quads side by side sharing the edge joints 1 and 2; Mxx (component 3) per element joint.
fn two_quads(dir: &std::path::Path) -> (Geometry, Vec<ResultBlock>) {
    let g = Geometry {
        joints: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0]],
        shells: vec![[0, 1, 2, 3, 0], [1, 4, 5, 2, 1]],
        shell_sections: vec!["A".into(), "B".into()],
        shell_thickness: vec![0.3, 0.7],
        ..Default::default()
    };
    // element 0: all corners 1000; element 1: all corners 3000 (N·m/m)
    let rows = 8usize;
    let element: Vec<u32> = vec![0, 0, 0, 0, 1, 1, 1, 1];
    let joint: Vec<u32> = vec![0, 1, 2, 3, 1, 4, 5, 2];
    let m11: Vec<f64> = vec![1000.0, 1000.0, 1000.0, 1000.0, 3000.0, 3000.0, 3000.0, 3000.0];
    let mut bytes = Vec::new();
    for c in [&element, &joint, &vec![0u32; rows]] {
        bytes.extend(c.iter().flat_map(|v| v.to_le_bytes()));
    }
    let zero = vec![0.0f64; rows];
    for c in [&zero, &zero, &zero, &zero, &m11, &zero, &zero, &zero, &zero] {
        bytes.extend(c.iter().flat_map(|v| v.to_le_bytes()));
    }
    let file = dir.join("DC_shell.bin");
    std::fs::write(&file, bytes).unwrap();
    let columns = ["element:u32", "joint:u32", "step_type:u32", "step_num:f64", "F11:f64", "F22:f64", "F12:f64", "M11:f64", "M22:f64", "M12:f64", "V13:f64", "V23:f64"];
    let block = ResultBlock { name: "DC".into(), combo: false, family: "shell_results".into(), rows, columns: columns.iter().map(|s| s.to_string()).collect(), file };
    (g, vec![block])
}

fn dir(test: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("factory_panel_vv_{test}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn avg_nodal_smooths_shared_joints_and_active_only_ignores_hidden() {
    let d = dir("avg");
    let (g, blocks) = two_quads(&d);
    let mut choice = Choice { load: Some("DC".into()), shell: 3, ..Default::default() };
    let units = Units::default(); // kN·m/m
    let element = build_with(&g, &blocks, &choice, units, &Settings::default()).unwrap();
    assert_eq!(element.shell_values, [[1.0; 4], [3.0; 4]]);

    choice.averaging = Averaging::Nodal;
    let nodal = build_with(&g, &blocks, &choice, units, &Settings::default()).unwrap();
    // shared joints 1 and 2 → (1 + 3) / 2 = 2
    assert_eq!(nodal.shell_values, [[1.0, 2.0, 2.0, 1.0], [2.0, 3.0, 3.0, 2.0]]);

    // hide section B (element 1) with a filter; Active Only averages over element 0 alone
    let mut settings = Settings::default();
    settings.filters.push(Filter { enabled: true, rule: Rule::SectionIs("B".into()), action: Action::Hide });
    choice.active_only = true;
    let active = build_with(&g, &blocks, &choice, units, &settings).unwrap();
    assert_eq!(active.shell_values[0], [1.0; 4]);
    // the hidden element is out of the legend range and out of value snap
    assert_eq!(active.shell.as_ref().unwrap().range, [0.5, 1.5]);
    assert!(active.snap.shell_values[1][0].is_nan());
}

#[test]
fn looks_follow_category_then_filters_in_order() {
    let d = dir("looks");
    let (g, _) = two_quads(&d);
    let mut s = Settings::default();
    assert_eq!(looks(&g, &s).0, [Look::Normal, Look::Normal]);
    s.filters.push(Filter { enabled: true, rule: Rule::ThicknessBelow(0.5), action: Action::Halftone });
    s.filters.push(Filter { enabled: true, rule: Rule::ThicknessAbove(0.5), action: Action::Colour([1, 2, 3]) });
    assert_eq!(looks(&g, &s).0, [Look::Halftone, Look::Colour([1, 2, 3])]);
    // a later filter wins
    s.filters.push(Filter { enabled: true, rule: Rule::SectionIs("A".into()), action: Action::Hide });
    assert_eq!(looks(&g, &s).0, [Look::Hidden, Look::Colour([1, 2, 3])]);
    // disabled filters do nothing; a hidden category stays hidden
    s.filters.iter_mut().for_each(|f| f.enabled = false);
    s.shells.visible = false;
    s.filters.push(Filter { enabled: true, rule: Rule::SectionIs("A".into()), action: Action::Colour([9, 9, 9]) });
    assert_eq!(looks(&g, &s).0, [Look::Hidden, Look::Hidden]);
}

#[test]
fn contour_off_draws_plain_and_has_no_legend() {
    let d = dir("plain");
    let (g, blocks) = two_quads(&d);
    let choice = Choice { load: Some("DC".into()), shell: 3, ..Default::default() };
    let settings = Settings { contour: false, ..Default::default() };
    let b = build_with(&g, &blocks, &choice, Units::default(), &settings).unwrap();
    assert!(b.shell.is_none());
    assert!(b.snap.shell_values.is_empty());
}

#[test]
fn a_hidden_category_hides_its_subcategories_which_keep_their_setting() {
    use custom_features::vv::{self, Settings};
    let mut s = Settings::default();
    s.frames.labels.visible = true;
    s.frames.sections.visible = true;
    s.frames.sections.halftone = true;
    vv::apply(s.clone());
    let o = vv::overlay();
    assert_eq!(o.frame_labels, Some(s.frames.labels.colour));
    assert_eq!(o.frame_sections, Some(vv::halftone(s.frames.sections.colour)), "halftone fades the text");
    assert_eq!(o.shell_labels, None);
    // category off: subcategories not drawn, but their own setting is kept
    s.frames.visible = false;
    vv::apply(s.clone());
    assert_eq!(vv::overlay().frame_labels, None);
    assert!(vv::settings().frames.labels.visible);
    vv::apply(Settings::default());
}

#[test]
fn view_options_shape_the_drawn_shells() {
    use custom_features::vv::{SHRINK, ViewType};
    let d = dir("view");
    let (g, blocks) = two_quads(&d);
    let choice = Choice { load: Some("DC".into()), shell: 3, ..Default::default() };
    let count = |s: &Settings| {
        let b = build_with(&g, &blocks, &choice, Units::default(), s).unwrap();
        let tris: usize = b.layers.iter().map(|l| l.triangles.len()).sum();
        let segs: usize = b.layers.iter().map(|l| l.segments.len()).sum();
        (tris, segs, b.layers)
    };
    // Standard: 2 triangles per quad
    let (t, s, _) = count(&Settings::default());
    assert_eq!((t, s), (4, 0));
    // Extrude: bottom + top (2 + 2) and 4 sides (8) per quad
    let mut ext = Settings::default();
    ext.view.view_type = ViewType::Extrude;
    let (t, _, layers) = count(&ext);
    assert_eq!(t, 24);
    // the slab of element 0 (thickness 0.3 m, normal +z from its corners) spans z = ±0.15
    let zs: Vec<f32> = layers[0].triangles.iter().flat_map(|tr| tr.corners.map(|c| c[2])).collect();
    let (lo, hi) = zs.iter().fold((f32::MAX, f32::MIN), |(l, h), z| (l.min(*z), h.max(*z)));
    assert!((lo + 0.35).abs() < 1e-6 && (hi - 0.35).abs() < 1e-6, "thicker element 1 (0.7 m) sets the extremes: {lo} {hi}");
    // Fill off: outlines only, 4 edges per quad
    let mut outline = Settings::default();
    outline.view.fill = false;
    assert_eq!(count(&outline).0, 0);
    assert_eq!(count(&outline).1, 8);
    // Show Edges: an extra plain layer with 4 edges per quad
    let mut edges = Settings::default();
    edges.view.edges = true;
    let (t, s, layers) = count(&edges);
    assert_eq!((t, s), (4, 8));
    assert_eq!(layers.last().unwrap().segments.len(), 8);
    // Shrink: corners move toward the centre (quad 0 centre (0.5, 0.5))
    let mut shrink = Settings::default();
    shrink.view.shrink = true;
    let (_, _, layers) = count(&shrink);
    let first = layers[0].triangles[0].corners[0];
    let expected = 0.5 - 0.5 * SHRINK as f32;
    assert!((first[0] - expected).abs() < 1e-6 && (first[1] - expected).abs() < 1e-6);
}

#[test]
fn local_axes_are_scene_lines_with_depth_one_triad_per_element() {
    use custom_bridge::view::Coloring;
    use custom_features::results_view::AXIS_COLOURS;
    let d = dir("axes");
    let (mut g, blocks) = two_quads(&d);
    // element 0 has axes from CSiBridge (identity: 1 = x, 2 = y, 3 = z), element 1 none (NaN)
    g.shell_axes = vec![[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], [f64::NAN; 9]];
    let choice = Choice { load: Some("DC".into()), shell: 3, ..Default::default() };
    let mut s = Settings::default();
    s.shells.axes.visible = true;
    let b = build_with(&g, &blocks, &choice, Units::default(), &s).unwrap();
    let is_axis = |l: &&custom_bridge::view::Layer| AXIS_COLOURS.iter().any(|c| l.coloring == Coloring::Plain(*c));
    let axis_layers: Vec<_> = b.layers.iter().filter(is_axis).collect();
    assert_eq!(axis_layers.len(), 3, "one layer per axis colour");
    assert!(axis_layers.iter().all(|l| l.segments.len() == 1), "one triad: element 1 has no axes");
    // axis 3 (blue) points up from just above the centre (0.5, 0.5, lift)
    let blue = &axis_layers[2].segments[0];
    assert_eq!(blue.ends[0][0], 0.5);
    assert!(blue.ends[0][2] > 0.0 && blue.ends[1][2] > blue.ends[0][2]);
    // off: no axis layers
    let none = build_with(&g, &blocks, &choice, Units::default(), &Settings::default()).unwrap();
    assert!(none.layers.iter().filter(is_axis).count() == 0);
}
