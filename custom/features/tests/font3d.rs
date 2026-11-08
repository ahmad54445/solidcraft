//! Stroke font for 3D labels (labels, sections, values as scene lines).

use custom_features::font3d::{ADVANCE, HEIGHT, has_glyph, text_segments};

#[test]
fn text_is_centred_on_its_anchor_and_as_high_as_asked() {
    // "1" alone: strokes (1,5)-(2,6)-(2,0) on the 4 × 6 grid; height 6 → one unit per grid step
    let segs = text_segments("1", [10.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], HEIGHT);
    assert_eq!(segs.len(), 2);
    let ys: Vec<f64> = segs.iter().flat_map(|s| [s[0][1], s[1][1]]).collect();
    let (lo, hi) = ys.iter().fold((f64::MAX, f64::MIN), |(l, h), y| (l.min(*y), h.max(*y)));
    assert_eq!((lo, hi), (-3.0, 3.0), "centred vertically, 6 high");
    // the vertical stroke sits at x = 2 of a 4-wide glyph → on the anchor
    assert!(segs.iter().any(|s| s[0][0] == 10.0 && s[1][0] == 10.0));
}

#[test]
fn width_grows_with_the_characters_and_planes_follow_right_and_up() {
    let one = text_segments("11", [0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], HEIGHT);
    let xs: Vec<f64> = one.iter().flat_map(|s| [s[0][0], s[1][0]]).collect();
    let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
    // '1' spans x = 1 … 2 of its cell: from the first's left edge to the second's right edge
    // is one advance plus the glyph's own width (1)
    assert!((hi - lo - (ADVANCE + 1.0)).abs() < 1e-9, "two '1' one advance apart: {lo} … {hi}");
    // up = +z: all y stay 0
    assert!(one.iter().all(|s| s[0][1] == 0.0 && s[1][1] == 0.0));
}

#[test]
fn section_names_and_numbers_have_glyphs() {
    for c in "TS 70CM Column 0.6x1.7 ASEC1 -12.5e3 (kN·m/m) DC_ULS".chars() {
        assert!(has_glyph(c), "missing glyph {c:?}");
    }
}
