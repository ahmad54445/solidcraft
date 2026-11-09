//! Signed-distance-field font for texts drawn by the GPU in the 3D view (docs/12 §2.1, labels).
//!
//! The glyphs of Hack Regular (egui's monospace font, `epaint_default_fonts`) are turned once into
//! a distance field: each texel stores how far it lies inside (> 0.5) or outside (< 0.5) the glyph
//! outline, `SPREAD` px of the rasterised em mapping to 0.5. The GPU draws a glyph at any size
//! from it, sharp, with a halo for contrast.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Pixels per em in the atlas.
pub const EM_PX: f32 = 32.0;
/// Distance (atlas px) mapped onto 0 … 0.5 on each side of the outline.
pub const SPREAD: f32 = 6.0;
/// Atlas width (px).
const WIDTH: u32 = 512;

/// A glyph in the atlas: its rectangle in em (x right, y down, origin on the baseline at the pen),
/// its place in the atlas (u0, v0, u1, v1) and its advance (em).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub advance: f32,
}

/// The atlas and its glyphs.
pub struct SdfFont {
    pub size: [u32; 2],
    pub texels: Vec<u8>,
    pub glyphs: HashMap<char, Glyph>,
    /// Advance (em) for characters without a glyph.
    pub fallback: f32,
}

/// The font, built at first use (~95 glyphs, a few ms).
pub fn font() -> &'static SdfFont {
    static FONT: OnceLock<SdfFont> = OnceLock::new();
    FONT.get_or_init(|| build(epaint_default_fonts::HACK_REGULAR, (' '..='~').chain(['°', '±', '²', '³', 'µ', '·'])))
}

/// One quad of laid-out text: rectangle (px, y down, around the anchor) and atlas rectangle.
pub type Quad = ([f32; 4], [f32; 4]);

impl SdfFont {
    /// Width of `text` in em.
    pub fn width(&self, text: &str) -> f32 {
        text.chars().map(|c| self.glyphs.get(&c).map_or(self.fallback, |g| g.advance)).sum()
    }

    /// `text` at `px` pixels per em, centred horizontally on the anchor and with its capitals
    /// centred vertically at `dy` px below it.
    pub fn layout(&self, text: &str, px: f32, dy: f32) -> Vec<Quad> {
        let mut x = -self.width(text) * px / 2.0;
        let baseline = dy + 0.36 * px;
        let mut out = Vec::new();
        for c in text.chars() {
            match self.glyphs.get(&c) {
                Some(g) => {
                    if g.uv[2] > g.uv[0] {
                        out.push(([x + g.rect[0] * px, baseline + g.rect[1] * px, x + g.rect[2] * px, baseline + g.rect[3] * px], g.uv));
                    }
                    x += g.advance * px;
                }
                None => x += self.fallback * px,
            }
        }
        out
    }
}

/// Flattened outline: closed polygons of line segments (atlas px, y up).
#[derive(Default)]
struct Outline {
    segments: Vec<[[f32; 2]; 2]>,
    start: [f32; 2],
    at: [f32; 2],
    scale: f32,
}

impl Outline {
    fn line(&mut self, to: [f32; 2]) {
        if to != self.at {
            self.segments.push([self.at, to]);
        }
        self.at = to;
    }
    fn p(&self, x: f32, y: f32) -> [f32; 2] {
        [x * self.scale, y * self.scale]
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.p(x, y);
        self.at = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.line(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, b, c) = (self.at, self.p(x1, y1), self.p(x, y));
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let m = 1.0 - t;
            self.line([0, 1].map(|k| m * m * a[k] + 2.0 * m * t * b[k] + t * t * c[k]));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b, c, d) = (self.at, self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let m = 1.0 - t;
            self.line([0, 1].map(|k| m * m * m * a[k] + 3.0 * m * m * t * b[k] + 3.0 * m * t * t * c[k] + t * t * t * d[k]));
        }
    }
    fn close(&mut self) {
        let s = self.start;
        self.line(s);
    }
}

/// Signed distance (px, > 0 inside, non-zero winding) from `p` to the outline.
fn signed_distance(segments: &[[[f32; 2]; 2]], p: [f32; 2]) -> f32 {
    let mut best = f32::MAX;
    let mut winding = 0i32;
    for [a, b] in segments {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let (ex, ey) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
        best = best.min(ex * ex + ey * ey);
        // Winding: crossings of the ray to +x.
        if (a[1] <= p[1]) != (b[1] <= p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * dx;
            if x > p[0] {
                winding += if b[1] > a[1] { 1 } else { -1 };
            }
        }
    }
    let d = best.sqrt();
    if winding != 0 { d } else { -d }
}

/// Build the atlas for `chars` from a TrueType font.
pub fn build(ttf: &[u8], chars: impl Iterator<Item = char>) -> SdfFont {
    let Ok(face) = ttf_parser::Face::parse(ttf, 0) else {
        return SdfFont { size: [1, 1], texels: vec![0], glyphs: HashMap::new(), fallback: 0.6 };
    };
    let upem = f32::from(face.units_per_em());
    let scale = EM_PX / upem;
    let pad = SPREAD.ceil() as i32 + 1;
    let fallback = face.glyph_index('0').and_then(|id| face.glyph_hor_advance(id)).map_or(0.6, |a| f32::from(a) / upem);
    // Rasterise each glyph into its own cell.
    struct Cell {
        c: char,
        w: u32,
        h: u32,
        texels: Vec<u8>,
        rect: [f32; 4],
        advance: f32,
    }
    let mut cells = Vec::new();
    for c in chars {
        let Some(id) = face.glyph_index(c) else { continue };
        let advance = face.glyph_hor_advance(id).map_or(fallback, |a| f32::from(a) / upem);
        let mut o = Outline { scale, ..Default::default() };
        let Some(bb) = face.outline_glyph(id, &mut o) else {
            cells.push(Cell { c, w: 0, h: 0, texels: Vec::new(), rect: [0.0; 4], advance });
            continue;
        };
        let x0 = (f32::from(bb.x_min) * scale).floor() as i32 - pad;
        let y1 = (f32::from(bb.y_max) * scale).ceil() as i32 + pad;
        let x1 = (f32::from(bb.x_max) * scale).ceil() as i32 + pad;
        let y0 = (f32::from(bb.y_min) * scale).floor() as i32 - pad;
        let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let mut texels = Vec::with_capacity((w * h) as usize);
        for row in 0..h {
            for col in 0..w {
                let p = [x0 as f32 + col as f32 + 0.5, y1 as f32 - row as f32 - 0.5];
                let d = signed_distance(&o.segments, p);
                texels.push(((0.5 + d / (2.0 * SPREAD)).clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
        let rect = [x0 as f32 / EM_PX, -y1 as f32 / EM_PX, x1 as f32 / EM_PX, -y0 as f32 / EM_PX];
        cells.push(Cell { c, w, h, texels, rect, advance });
    }
    // Shelf packing, one texel apart.
    let mut places = Vec::with_capacity(cells.len());
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    for cell in &cells {
        if x + cell.w > WIDTH {
            x = 0;
            y += shelf + 1;
            shelf = 0;
        }
        places.push((x, y));
        x += cell.w + 1;
        shelf = shelf.max(cell.h);
    }
    let height = (y + shelf).max(1).next_power_of_two();
    let mut texels = vec![0u8; (WIDTH * height) as usize];
    let mut glyphs = HashMap::new();
    for (cell, (px, py)) in cells.iter().zip(places) {
        for row in 0..cell.h {
            let src = (row * cell.w) as usize;
            let dst = ((py + row) * WIDTH + px) as usize;
            texels[dst..dst + cell.w as usize].copy_from_slice(&cell.texels[src..src + cell.w as usize]);
        }
        let uv = if cell.w == 0 {
            [0.0; 4]
        } else {
            [px as f32 / WIDTH as f32, py as f32 / height as f32, (px + cell.w) as f32 / WIDTH as f32, (py + cell.h) as f32 / height as f32]
        };
        glyphs.insert(cell.c, Glyph { rect: cell.rect, uv, advance: cell.advance });
    }
    SdfFont { size: [WIDTH, height], texels, glyphs, fallback }
}
