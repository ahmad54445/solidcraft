//! [CUSTOM] Factory's elements in SolidCraft's 3D view (feature `ui`, docs/10-viewport-results.md).
//!
//! Factory hands plain data (triangles and segments with a value per corner); this turns it into
//! SolidCraft's point-cloud records, which the view draws with the model's depth, section cut and
//! colour ramp (blue → cyan → green → yellow → red). Only this file knows those types.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use solidcraft_viewport_gpu::view::{Cell, CloudStyle, ColorMode, FILTERS, GpuCloud, cloud_cell};

/// A triangle with a value at each corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Triangle {
    pub corners: [[f32; 3]; 3],
    pub values: [f32; 3],
}

/// A straight piece of line with a value at each end.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment {
    pub ends: [[f32; 3]; 2],
    pub values: [f32; 2],
}

/// How a layer is coloured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coloring {
    /// One colour (no result shown).
    Plain([u8; 3]),
    /// The ramp over `[low, high]` of the values.
    Ramp([f32; 2]),
}

/// One group of elements drawn together (e.g. all shells, all frames).
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub triangles: Vec<Triangle>,
    pub segments: Vec<Segment>,
    pub coloring: Coloring,
}

/// Ramp colours as the shader has them (`assets/shaders/viewport.wgsl`, `cloud_ramp`), for legends.
pub const RAMP: [[u8; 3]; 5] = [[40, 70, 200], [30, 170, 230], [60, 200, 90], [240, 220, 60], [230, 70, 50]];

/// The ramp colour at `t` in 0..=1, exactly as the shader mixes it.
pub fn ramp(t: f32) -> [u8; 3] {
    let k = t.clamp(0.0, 1.0) * 4.0;
    let j = (k as usize).min(3);
    let f = k - j as f32;
    let mix = |c: usize| (f32::from(RAMP[j][c]) + (f32::from(RAMP[j + 1][c]) - f32::from(RAMP[j][c])) * f).round() as u8;
    [mix(0), mix(1), mix(2)]
}

/// Ids far from the document's point-cloud ids.
const ID_BASE: u64 = 0xFAC7_0000_0000_0000;
static VERSION: AtomicU64 = AtomicU64::new(1);
static SHOWN: Mutex<Vec<GpuCloud>> = Mutex::new(Vec::new());
/// Text drawn as line segments (labels, section names, values), kept apart from the result
/// layers so re-placing the text never rebuilds the results.
static TEXT: Mutex<Vec<GpuCloud>> = Mutex::new(Vec::new());
const TEXT_ID_BASE: u64 = ID_BASE + 0x1_0000;
/// The selection's highlight (docs/13 R6), drawn over the result layers.
static SELECTION: Mutex<Vec<GpuCloud>> = Mutex::new(Vec::new());
const SELECTION_ID: u64 = ID_BASE + 0x2_0000;

/// Result layers + text layers + the selection highlight to the view.
fn publish() {
    let shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let text = TEXT.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let selection = SELECTION.lock().unwrap_or_else(PoisonError::into_inner).clone();
    solidcraft_ui_egui::custom_hook::set_extra_clouds(shown.into_iter().chain(text).chain(selection).collect());
}

/// Show the selection highlight (docs/13 R6): `segments` drawn as beads `width` thick (world
/// units, at least ~1.5 px on screen) in `colour`, around the selected elements; empty clears it.
/// The result layers are not touched.
pub fn show_selection(segments: &[Segment], colour: [u8; 3], width: f32) {
    let mut bytes = Vec::with_capacity(segments.len() * solidcraft_viewport_gpu::view::CELL_SIZE);
    for s in segments {
        let cell = Cell { corners: [s.ends[0], s.ends[1], s.ends[1]], neighbours: s.ends, rgb: colour, size: [width, width], ..Default::default() };
        cloud_cell(&mut bytes, &cell);
    }
    let cloud = GpuCloud {
        id: SELECTION_ID,
        version: VERSION.fetch_add(1, Ordering::Relaxed),
        dots: Arc::new(Vec::new()),
        segments: Arc::new(bytes),
        triangles: Arc::new(Vec::new()),
        style: style(Coloring::Plain(colour)),
    };
    *SELECTION.lock().unwrap_or_else(PoisonError::into_inner) = if segments.is_empty() { Vec::new() } else { vec![cloud] };
    publish();
}

/// A finished box / lasso selection in the 3D view (screen points), see [`take_box_gestures`].
pub use solidcraft_ui_egui::custom_hook::BoxGesture;

/// The box selections the user finished in the 3D view since the last call.
pub fn take_box_gestures() -> Vec<BoxGesture> {
    solidcraft_ui_egui::custom_hook::take_box_gestures()
}

fn clouds(layers: &[Layer], id_base: u64) -> Vec<GpuCloud> {
    layers
        .iter()
        .enumerate()
        .map(|(k, layer)| {
            let has_value = matches!(layer.coloring, Coloring::Ramp(_));
            let rgb = match layer.coloring {
                Coloring::Plain(c) => c,
                Coloring::Ramp(_) => [200, 200, 200],
            };
            let mut triangles = Vec::with_capacity(layer.triangles.len() * solidcraft_viewport_gpu::view::CELL_SIZE);
            for t in &layer.triangles {
                let mut cell = Cell { corners: t.corners, rgb, ..Default::default() };
                if has_value {
                    cell.value = t.values;
                    cell.has |= 1 << FILTERS;
                }
                cloud_cell(&mut triangles, &cell);
            }
            let mut segments = Vec::with_capacity(layer.segments.len() * solidcraft_viewport_gpu::view::CELL_SIZE);
            for s in &layer.segments {
                // A segment repeats its second end; neighbours are its own ends; size 0 = a line.
                let mut cell = Cell { corners: [s.ends[0], s.ends[1], s.ends[1]], neighbours: s.ends, rgb, ..Default::default() };
                if has_value {
                    cell.value = [s.values[0], s.values[1], s.values[1]];
                    cell.has |= 1 << FILTERS;
                }
                cloud_cell(&mut segments, &cell);
            }
            GpuCloud {
                id: id_base + k as u64,
                version: VERSION.fetch_add(1, Ordering::Relaxed),
                dots: Arc::new(Vec::new()),
                segments: Arc::new(segments),
                triangles: Arc::new(triangles),
                style: style(layer.coloring),
            }
        })
        .collect()
}

/// Show these text layers (segments in plain colours), replacing the previous text. The result
/// layers are not touched.
pub fn show_text(layers: &[Layer]) {
    *TEXT.lock().unwrap_or_else(PoisonError::into_inner) = clouds(layers, TEXT_ID_BASE);
    publish();
}

/// Show these layers (replacing what was shown). Builds the GPU records: call it off the UI thread
/// for large models. `bounds` (min, max) makes Fit view include them.
pub fn show(layers: &[Layer], bounds: Option<[[f64; 3]; 2]>) {
    *SHOWN.lock().unwrap_or_else(PoisonError::into_inner) = clouds(layers, ID_BASE);
    publish();
    solidcraft_ui_egui::custom_hook::set_extra_bounds(bounds);
}

/// Change only the ramp ranges of the shown layers (index = layer order): nothing is uploaded again.
pub fn set_ranges(ranges: &[Option<[f32; 2]>]) {
    {
        let mut shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner);
        for (cloud, range) in shown.iter_mut().zip(ranges) {
            if let Some(r) = range {
                cloud.style = style(Coloring::Ramp(*r));
            }
        }
    }
    publish();
}

/// Remove Factory's elements from the view.
pub fn clear() {
    SHOWN.lock().unwrap_or_else(PoisonError::into_inner).clear();
    TEXT.lock().unwrap_or_else(PoisonError::into_inner).clear();
    SELECTION.lock().unwrap_or_else(PoisonError::into_inner).clear();
    solidcraft_ui_egui::custom_hook::set_extra_clouds(Vec::new());
    solidcraft_ui_egui::custom_hook::set_extra_bounds(None);
}

pub use solidcraft_ui_egui::custom_hook::ViewInfo;

/// Draw `draw` over the 3D view every frame (legend, value snap), with the view's projection. Call once.
pub fn set_overlay(draw: solidcraft_ui_egui::custom_hook::OverlayFn) {
    solidcraft_ui_egui::custom_hook::set_viewport_overlay(draw);
}

fn style(coloring: Coloring) -> CloudStyle {
    match coloring {
        Coloring::Plain(_) => CloudStyle { mode: ColorMode::Own, range: [0.0, 1.0], point_size: 0.0, ..Default::default() },
        Coloring::Ramp(r) => CloudStyle { mode: ColorMode::Ramp, range: r, point_size: 0.0, ..Default::default() },
    }
}

/// One screen-facing text glyph (docs/12 §2.1): drawn by the GPU at a world anchor with the
/// model's depth, constant size in pixels; see [`show_sprites`].
pub use solidcraft_viewport_gpu::view::{Sprite, SpriteAtlas};

static SPRITE_VERSION: AtomicU64 = AtomicU64::new(1);

/// Draw `sprites` from `atlas` (an alpha distance field; `atlas_id` changes when it does) in every
/// viewport from now on, every frame, until replaced or cleared.
pub fn show_sprites(atlas: &Arc<SpriteAtlas>, atlas_id: u64, sprites: Vec<Sprite>) {
    let version = SPRITE_VERSION.fetch_add(1, Ordering::Relaxed);
    let set = solidcraft_viewport_gpu::view::SpriteSet { atlas_version: atlas_id, atlas: atlas.clone(), version, sprites: Arc::new(sprites) };
    solidcraft_viewport_gpu::view::set_sprites(Some(Arc::new(set)));
}

/// Remove all sprites.
pub fn clear_sprites() {
    solidcraft_viewport_gpu::view::set_sprites(None);
}

// ---- Display windows (Factory's docs/14-display-windows.md) ----

/// A display window's own drawn layers, texts and labels, kept while another window is active.
#[derive(Clone, Default)]
struct Parked {
    shown: Vec<GpuCloud>,
    text: Vec<GpuCloud>,
    bounds: Option<[[f64; 3]; 2]>,
    sprites: Option<Arc<solidcraft_viewport_gpu::view::SpriteSet>>,
}

static PARKED: Mutex<Vec<(u64, Parked)>> = Mutex::new(Vec::new());

fn parked_mut<T>(f: impl FnOnce(&mut Vec<(u64, Parked)>) -> T) -> T {
    f(&mut PARKED.lock().unwrap_or_else(PoisonError::into_inner))
}

fn current() -> Parked {
    Parked {
        shown: SHOWN.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        text: TEXT.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        bounds: solidcraft_ui_egui::custom_hook::extra_bounds(),
        sprites: solidcraft_viewport_gpu::view::current_sprites(),
    }
}

/// Keep what the view shows now as window `tile`'s (it stops being active, or is a new copy of
/// the active one). Nothing is copied but shared records.
pub fn park(tile: u64) {
    let p = current();
    parked_mut(|v| {
        v.retain(|(id, _)| *id != tile);
        v.push((tile, p));
    });
}

/// Show window `tile`'s kept layers, texts and labels (it becomes active). `false` when none are kept.
pub fn unpark(tile: u64) -> bool {
    let Some(p) = parked_mut(|v| v.iter().position(|(id, _)| *id == tile).map(|k| v.remove(k).1)) else { return false };
    *SHOWN.lock().unwrap_or_else(PoisonError::into_inner) = p.shown;
    *TEXT.lock().unwrap_or_else(PoisonError::into_inner) = p.text;
    publish();
    solidcraft_ui_egui::custom_hook::set_extra_bounds(p.bounds);
    solidcraft_viewport_gpu::view::set_sprites(p.sprites);
    true
}

/// Forget window `tile` (closed).
pub fn forget(tile: u64) {
    parked_mut(|v| v.retain(|(id, _)| *id != tile));
}

/// New layers for an inactive window `tile` (built for it after a transfer); its texts and labels
/// are dropped (chosen again when it is active). `ranges` as [`set_ranges`].
pub fn set_parked(tile: u64, layers: &[Layer], bounds: Option<[[f64; 3]; 2]>, ranges: &[Option<[f32; 2]>]) {
    let mut shown = clouds(layers, ID_BASE);
    for (cloud, range) in shown.iter_mut().zip(ranges) {
        if let Some(r) = range {
            cloud.style = style(Coloring::Ramp(*r));
        }
    }
    let p = Parked { shown, text: Vec::new(), bounds, sprites: None };
    parked_mut(|v| {
        v.retain(|(id, _)| *id != tile);
        v.push((tile, p));
    });
}

fn tile_content(tile: u64) -> solidcraft_ui_egui::custom_tiles::TileContent {
    let p = parked_mut(|v| v.iter().find(|(id, _)| *id == tile).map(|(_, p)| p.clone())).unwrap_or_default();
    // The selection is one for every window (docs/14 §2).
    let selection = SELECTION.lock().unwrap_or_else(PoisonError::into_inner).clone();
    solidcraft_ui_egui::custom_tiles::TileContent { clouds: p.shown.into_iter().chain(p.text).chain(selection).collect(), sprites: p.sprites }
}

pub use solidcraft_ui_egui::custom_tiles::{LayoutFn, TileOverlayFn, TilePlan};

/// Split the 3D view into display windows: `layout` draws their frames and says where each
/// picture goes (`None` = one view); `overlay` draws over an inactive window (its legend). Call once.
pub fn set_tiles(layout: LayoutFn, overlay: TileOverlayFn) {
    solidcraft_ui_egui::custom_tiles::set_tiles(layout, tile_content, overlay);
}
