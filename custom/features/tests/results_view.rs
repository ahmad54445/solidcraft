//! Colouring by results (docs/10-viewport-results.md) on a small hand-made model.

use custom_features::results_view::{Choice, Step, build, frame_unit, shell_unit};
use custom_features::transfer::{Geometry, ResultBlock};
use custom_features::units::Units;

/// Little-endian columns in the order the agent sends them.
fn columns(u32s: &[Vec<u32>], f64s: &[Vec<f64>]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in u32s {
        out.extend(c.iter().flat_map(|v| v.to_le_bytes()));
    }
    for c in f64s {
        out.extend(c.iter().flat_map(|v| v.to_le_bytes()));
    }
    out
}

fn block(dir: &std::path::Path, name: &str, family: &str, rows: usize, cols: &[&str], bytes: Vec<u8>) -> ResultBlock {
    let file = dir.join(format!("{name}_{family}.bin"));
    std::fs::write(&file, bytes).unwrap();
    ResultBlock { name: name.into(), combo: false, family: family.into(), rows, columns: cols.iter().map(|s| s.to_string()).collect(), file }
}

const SHELL_COLS: [&str; 12] = ["element:u32", "joint:u32", "step_type:u32", "step_num:f64", "F11:f64", "F22:f64", "F12:f64", "M11:f64", "M22:f64", "M12:f64", "V13:f64", "V23:f64"];
const FRAME_COLS: [&str; 10] = ["element:u32", "step_type:u32", "station:f64", "step_num:f64", "P:f64", "V2:f64", "V3:f64", "T:f64", "M2:f64", "M3:f64"];

fn geometry() -> Geometry {
    Geometry {
        // a 2 m × 1 m quad (joints 0..3) and a 4 m frame (joints 4, 5)
        joints: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [4.0, 0.0, 1.0]],
        shells: vec![[0, 1, 2, 3, 0]],
        frames: vec![[4, 5, 0]],
        ..Default::default()
    }
}

fn dir(test: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("factory_results_view_{test}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn shell_values_land_on_their_corners_and_frames_follow_stations() {
    let d = dir("corners");
    // M11 (column 7) = 1000, 2000, 3000, 4000 N·m/m at joints 3, 2, 1, 0 (shuffled order)
    let joints = vec![3, 2, 1, 0];
    let m11 = vec![1000.0, 2000.0, 3000.0, 4000.0];
    let zero = vec![0.0; 4];
    let shells = columns(
        &[vec![0; 4], joints, vec![0; 4]],
        &[zero.clone(), zero.clone(), zero.clone(), zero.clone(), m11, zero.clone(), zero.clone(), zero.clone(), zero.clone()],
    );
    // M3 (frame column 9) = 0 at 0 m, 8000 N·m at 2 m, 0 at 4 m; stations given out of order
    let frames = columns(&[vec![0; 3], vec![0; 3]], &[vec![4.0, 0.0, 2.0], vec![0.0; 3], vec![0.0; 3], vec![0.0; 3], vec![0.0; 3], vec![0.0; 3], vec![0.0; 3], vec![0.0, 0.0, 8000.0]]);
    let blocks = vec![block(&d, "DC", "shell_results", 4, &SHELL_COLS, shells), block(&d, "DC", "frame_results", 3, &FRAME_COLS, frames)];
    let choice = Choice { load: Some("DC".into()), shell: 3, frame: 5, step: Step::Max, ..Default::default() };
    let units = Units::default(); // kN, m
    let b = build(&geometry(), &blocks, &choice, units).unwrap();
    // corners n1..n4 = joints 0, 1, 2, 3 → 4, 3, 2, 1 kN·m/m
    assert_eq!(b.shell_values, [[4.0, 3.0, 2.0, 1.0]]);
    assert_eq!(b.shell.as_ref().unwrap().range, [1.0, 4.0]);
    assert_eq!(b.shell.as_ref().unwrap().unit, "kN·m/m");
    // stations sorted, in kN·m
    assert_eq!(b.frame_values, [vec![(0.0, 0.0), (2.0, 8.0), (4.0, 0.0)]]);
    assert_eq!(b.frame.as_ref().unwrap().unit, "kN·m");
    assert!(!b.envelope);
    assert_eq!(b.bounds, Some([[0.0, 0.0, 0.0], [4.0, 1.0, 1.0]]));
}

#[test]
fn envelopes_show_the_chosen_side() {
    let d = dir("envelope");
    // one frame station twice: Max (1) = 5000 N, Min (2) = -3000 N for P (column 4)
    let frames = columns(&[vec![0, 0], vec![1, 2]], &[vec![0.0, 0.0], vec![0.0, 0.0], vec![5000.0, -3000.0], vec![0.0; 2], vec![0.0; 2], vec![0.0; 2], vec![0.0; 2], vec![0.0; 2]]);
    let blocks = vec![block(&d, "ENV", "frame_results", 2, &FRAME_COLS, frames)];
    let mut choice = Choice { load: Some("ENV".into()), frame: 0, ..Default::default() };
    let max = build(&geometry(), &blocks, &choice, Units::default()).unwrap();
    assert!(max.envelope);
    assert_eq!(max.frame_values, [vec![(0.0, 5.0)]]);
    choice.step = Step::Min;
    let min = build(&geometry(), &blocks, &choice, Units::default()).unwrap();
    assert_eq!(min.frame_values, [vec![(0.0, -3.0)]]);
}

#[test]
fn no_load_means_plain_elements() {
    let b = build(&geometry(), &[], &Choice::default(), Units::default()).unwrap();
    assert!(b.shell.is_none() && b.frame.is_none());
}

#[test]
fn units_of_components() {
    let u = Units::default();
    assert_eq!(shell_unit(0, u), (1000.0, "kN/m".to_string())); // F11
    assert_eq!(shell_unit(3, u), (1000.0, "kN·m/m".to_string())); // M11
    assert_eq!(frame_unit(0, u), (1000.0, "kN".to_string())); // P
    assert_eq!(frame_unit(5, u), (1000.0, "kN·m".to_string())); // M3
}
