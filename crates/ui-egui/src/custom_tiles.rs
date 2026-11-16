//! [CUSTOM] Factory fork hook: display windows (Factory's docs/14-display-windows.md).
//!
//! The fork splits the 3D view area into tiles ([`set_tiles`]). The **active** tile is SolidCraft's
//! own view, drawn into the tile's rectangle with `app.cam`; every tool works there as before. The
//! other tiles keep their own camera here and are drawn by the renderer's tile path
//! (`solidcraft_viewport_gpu::view::TileFrame`) from the active frame with that camera. When the
//! active tile changes, its camera goes into `app.cam` and the old one is kept: SolidCraft's one
//! camera path (`navigation::handle`) is not changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use egui_wgpu::wgpu;
use solidcore_record::Vec3;
use solidcraft_theme::Tokens;
use solidcraft_viewport::Camera;
use solidcraft_viewport_gpu::view::{Frame, GpuCloud, Renderer, SpriteSet, TileFrame};

use crate::SolveApp;
use crate::custom_hook::ViewInfo;

/// The tiles this frame: (id, rectangle for its picture) and the active one.
pub struct TilePlan {
    pub tiles: Vec<(u64, egui::Rect)>,
    pub active: u64,
}

/// What an inactive tile shows.
#[derive(Default)]
pub struct TileContent {
    pub clouds: Vec<GpuCloud>,
    pub sprites: Option<Arc<SpriteSet>>,
}

/// Draws the tiles' frames (title bars, borders) in the view area and says where each picture
/// goes; `None` = one view as usual.
pub type LayoutFn = fn(&mut egui::Ui, egui::Rect) -> Option<TilePlan>;
/// The clouds and texts of an inactive tile.
pub type ContentFn = fn(u64) -> TileContent;
/// Drawn over an inactive tile (legend), with its projection.
pub type TileOverlayFn = fn(&mut egui::Ui, u64, egui::Rect, &ViewInfo<'_>);

static HOOKS: OnceLock<(LayoutFn, ContentFn, TileOverlayFn)> = OnceLock::new();

/// Turn display windows on. First call wins.
pub fn set_tiles(layout: LayoutFn, content: ContentFn, overlay: TileOverlayFn) {
    let _ = HOOKS.set((layout, content, overlay));
}

#[derive(Default)]
struct State {
    active: Option<u64>,
    /// The inactive tiles' cameras.
    cams: HashMap<u64, Camera>,
    /// The active view's last frame (cloned, its records are shared).
    last: Option<Frame>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    f(STATE.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_with(State::default))
}

/// The active view's frame, for the inactive tiles drawn after it.
pub(crate) fn remember(frame: &Frame) {
    if HOOKS.get().is_some() {
        with(|s| s.last = Some(frame.clone()));
    }
}

/// The view area: one view, or the tiles.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let full = ui.max_rect();
    let Some(&(layout, content, overlay)) = HOOKS.get() else {
        crate::viewport::show(app, ui);
        return;
    };
    let plan = layout(ui, full).filter(|p| p.tiles.iter().any(|(id, _)| *id == p.active));
    let Some(plan) = plan else {
        with(|s| *s = State::default());
        crate::viewport::show(app, ui);
        return;
    };
    // The active tile's camera is `app.cam`; the one it replaces is kept.
    with(|s| {
        if s.active != Some(plan.active) {
            if let Some(old) = s.active {
                s.cams.insert(old, app.cam);
            }
            if let Some(cam) = s.cams.remove(&plan.active) {
                app.cam = cam;
            }
            s.active = Some(plan.active);
        }
        s.cams.retain(|id, _| plan.tiles.iter().any(|(t, _)| t == id) && *id != plan.active);
        for (id, _) in &plan.tiles {
            if *id != plan.active {
                // A new tile starts with the active one's camera (docs/14 W7).
                s.cams.entry(*id).or_insert(app.cam);
            }
        }
        s.last = None;
    });
    let active_rect = plan.tiles.iter().find(|(id, _)| *id == plan.active).map_or(full, |(_, r)| *r);
    ui.scope_builder(egui::UiBuilder::new().max_rect(active_rect), |ui| crate::viewport::show(app, ui));
    let last = with(|s| s.last.clone());
    let t = Tokens::get();
    let ppp = ui.ctx().pixels_per_point();
    // A tile with no room (another one maximised) keeps its camera but is not drawn.
    let drawn = |r: &egui::Rect| r.width() >= 2.0 && r.height() >= 2.0;
    let all: Vec<u64> = plan.tiles.iter().filter(|(id, r)| *id != plan.active && drawn(r)).map(|(id, _)| *id).collect();
    for &(id, rect) in plan.tiles.iter().filter(|(id, r)| *id != plan.active && drawn(r)) {
        let painter = ui.painter_at(rect);
        solidcraft_viewport_egui::paint_background(&painter, rect, t.viewport_top, t.viewport_bottom);
        let Some(cam) = with(|s| s.cams.get(&id).copied()) else { continue };
        let saved = std::mem::replace(&mut app.cam, cam);
        let proj = crate::viewport::projection(app, rect);
        app.cam = saved;
        if let Some(main) = &last {
            let mut frame = main.clone();
            frame.view_proj = proj.vp.to_f32();
            frame.turned_depth = Some(proj.vp.turned_depth_f32());
            frame.back = proj.cam.back().to_f32();
            let e = proj.cam.eye().to_f32();
            frame.eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
            frame.size_px = [rect.width() * ppp, rect.height() * ppp];
            frame.ssaa = 1.0;
            frame.moving = None;
            let c = content(id);
            let tile = TileFrame { id, frame, clouds: c.clouds, sprites: c.sprites, all: all.clone() };
            painter.add(egui_wgpu::Callback::new_paint_callback(rect, TileCallback(tile)));
        }
        let eye = proj.cam.eye();
        let view = ViewInfo { project: &|p: [f64; 3]| proj.to_screen(Vec3::new(p[0], p[1], p[2])), eye: [eye.x, eye.y, eye.z], pointer_on_view: false };
        overlay(ui, id, rect, &view);
    }
}

/// An inactive tile's picture: drawn offscreen in `prepare`, copied in `paint`.
struct TileCallback(TileFrame);

impl egui_wgpu::CallbackTrait for TileCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = resources.get_mut::<Renderer>() {
            r.prepare_tile(device, queue, encoder, &self.0);
        }
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        let Some(r) = resources.get::<Renderer>() else { return };
        let vp = info.viewport_in_pixels();
        r.paint_tile(self.0.id, pass, [vp.left_px as f32, vp.top_px as f32, vp.width_px as f32, vp.height_px as f32]);
    }
}
