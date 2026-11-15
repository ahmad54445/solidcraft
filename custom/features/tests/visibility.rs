//! Hide / Isolate (docs/13 §8): the hidden set, the eye toggle and the looks the view is built from.

use custom_features::selection::{Bits, Set};
use custom_features::transfer::Geometry;
use custom_features::visibility;
use custom_features::vv::Look;

fn model() -> Geometry {
    Geometry {
        joints: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0]],
        shells: vec![[0, 1, 2, 3, 0], [1, 4, 5, 2, 0]],
        frames: vec![[0, 1, 0], [3, 2, 0]],
        ..Default::default()
    }
}

fn set(shells: &[usize], frames: &[usize]) -> Set {
    let mut s = Set { shells: Bits::with_len(2), frames: Bits::with_len(2) };
    shells.iter().for_each(|e| s.shells.set(*e, true));
    frames.iter().for_each(|e| s.frames.set(*e, true));
    s
}

/// One test: the hidden set is global.
#[test]
fn hide_isolate_toggle_show_all_and_the_looks() {
    let g = model();
    visibility::show_all();
    visibility::hide(&set(&[0], &[]));
    assert_eq!(visibility::hidden().counts(), (1, 0));

    // The view: hidden elements become Look::Hidden, the others keep their look.
    let (mut shells, mut frames) = (vec![Look::Normal; 2], vec![Look::Halftone; 2]);
    visibility::apply(&mut shells, &mut frames);
    assert_eq!(shells, vec![Look::Hidden, Look::Normal]);
    assert_eq!(frames, vec![Look::Halftone, Look::Halftone]);

    // Isolate frame 1: everything else hidden.
    visibility::isolate(&g, &set(&[], &[1]));
    assert_eq!(visibility::hidden().counts(), (2, 1));
    assert!(!visibility::hidden().frames.get(1));

    // The eye of a row: some shown → hide all; all hidden → show all.
    let both_frames = set(&[], &[0, 1]);
    assert_eq!(visibility::hidden_of(&both_frames), (1, 2));
    visibility::toggle(&both_frames);
    assert_eq!(visibility::hidden_of(&both_frames), (2, 2));
    visibility::toggle(&both_frames);
    assert_eq!(visibility::hidden_of(&both_frames), (0, 2));

    visibility::show_all();
    assert_eq!(visibility::hidden().counts(), (0, 0));
}
