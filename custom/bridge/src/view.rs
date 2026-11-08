//! [CUSTOM] Factory's elements in SolidCraft's 3D view (feature `ui`, docs/10-viewport-results.md).
//!
//! Factory hands plain data (triangles and segments with a value per corner); this turns it into
//! SolidCraft's point-cloud records, which the view draws with the model's depth, section cut and
//! colour ramp (blue → cyan → green → yellow → red). Only this file knows those types.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use solidcraft_gpu::view::{Cell, CloudStyle, ColorMode, FILTERS, GpuCloud, cloud_cell};

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

/// Result layers + text layers to the view.
fn publish() {
    let shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let text = TEXT.lock().unwrap_or_else(PoisonError::into_inner).clone();
    solidcraft_ui_egui::custom_hook::set_extra_clouds(shown.into_iter().chain(text).collect());
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
            let mut triangles = Vec::with_capacity(layer.triangles.len() * solidcraft_gpu::view::CELL_SIZE);
            for t in &layer.triangles {
                let mut cell = Cell { corners: t.corners, rgb, ..Default::default() };
                if has_value {
                    cell.value = t.values;
                    cell.has |= 1 << FILTERS;
                }
                cloud_cell(&mut triangles, &cell);
            }
            let mut segments = Vec::with_capacity(layer.segments.len() * solidcraft_gpu::view::CELL_SIZE);
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
