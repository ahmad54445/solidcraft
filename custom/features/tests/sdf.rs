//! SDF font for GPU text (docs/12 §2.1): atlas built, glyph inside/outside, layout centred.

use custom_features::sdf;

#[test]
fn atlas_has_digits_and_letters_with_inside_and_outside() {
    let f = sdf::font();
    assert_eq!(f.size[0], 512);
    assert_eq!(f.texels.len(), (f.size[0] * f.size[1]) as usize);
    for c in ['0', '9', 'A', 'z', '-', '.', 'E'] {
        let g = f.glyphs.get(&c).unwrap_or_else(|| panic!("glyph {c}"));
        assert!(g.advance > 0.3 && g.advance < 1.0, "{c}: {}", g.advance);
        assert!(g.uv[2] > g.uv[0] && g.uv[3] > g.uv[1]);
    }
    // 'I' (Hack: a bar with serifs): the cell centre is inside, a corner (padding) outside.
    let g = f.glyphs[&'I'];
    let (w, h) = (f.size[0] as f32, f.size[1] as f32);
    let at = |u: f32, v: f32| f.texels[((v * h) as usize) * f.size[0] as usize + (u * w) as usize];
    assert!(at((g.uv[0] + g.uv[2]) / 2.0, (g.uv[1] + g.uv[3]) / 2.0) > 160);
    assert!(at(g.uv[0] + 0.5 / w, g.uv[1] + 0.5 / h) < 60);
    // Space has an advance and no quad.
    assert!(f.glyphs[&' '].advance > 0.3);
    assert!(f.layout(" ", 12.0, 0.0).is_empty());
}

#[test]
fn layout_is_centred_and_scales_with_size() {
    let f = sdf::font();
    let quads = f.layout("123", 20.0, 0.0);
    assert_eq!(quads.len(), 3);
    let (lo, hi) = quads.iter().fold((f32::MAX, f32::MIN), |(lo, hi), (r, _)| (lo.min(r[0]), hi.max(r[2])));
    assert!((lo + hi).abs() < 2.0, "centred: {lo} {hi}");
    let width = f.width("123") * 20.0;
    assert!((hi - lo) > width * 0.8 && (hi - lo) < width + 2.0 * sdf::SPREAD / sdf::EM_PX * 20.0 + 1.0);
    // Line offset moves it down.
    let low = f.layout("1", 20.0, 10.0);
    let high = f.layout("1", 20.0, 0.0);
    assert!((low[0].0[1] - high[0].0[1] - 10.0).abs() < 1e-4);
}
