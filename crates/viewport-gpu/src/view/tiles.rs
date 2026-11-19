//! [CUSTOM] Factory fork hook: display windows (Factory's docs/14-display-windows.md).
//!
//! The 3D view can be split into tiles. The active tile is the ordinary viewport; every other
//! tile is drawn here, from the **same renderer**, with its own camera: its own uniform (group 0),
//! an offscreen frame of its own size, and the clouds and sprites it shows. A cloud the active view
//! has uploaded at the same version and style is drawn from that upload – the geometry is on the
//! GPU once (rule S1/S2); only a cloud the active view does not show is uploaded for the tile.
//! Bodies, sketches and ambient occlusion are not drawn in a tile (Factory shows clouds only).
//!
//! egui-wgpu prepares every callback before it paints any, so a tile cannot reuse the main
//! frame's uniform or picture: each has its own.

use std::sync::Arc;

use super::{
    GpuCloud, Renderer, SpriteSet, Supersampled, Tally, allocated, clear_resolve_targets, clouds::CloudDraws, far_depth, material_texture,
    palette_texture, sprites::SpriteDraws, uniform_bytes,
};

/// What one tile draws this frame.
#[derive(Clone)]
pub struct TileFrame {
    /// The tile (stable while it exists).
    pub id: u64,
    /// The active view's frame with this tile's camera (`view_proj`, `turned_depth`, `back`,
    /// `eye`) and size (`size_px`); its `clouds` are the active view's (to share uploads).
    pub frame: super::Frame,
    /// The clouds this tile shows.
    pub clouds: Vec<GpuCloud>,
    /// The texts (sprites) this tile shows.
    pub sprites: Option<Arc<SpriteSet>>,
    /// Every tile shown this frame: the others' GPU resources are freed.
    pub all: Vec<u64>,
}

/// A tile on the GPU.
struct Tile {
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// Kept for the bind group.
    _palette: wgpu::TextureView,
    _materials: wgpu::TextureView,
    frame: Option<Supersampled>,
    /// Clouds the active view does not have uploaded.
    clouds: CloudDraws,
    sprites: SpriteDraws,
    /// Ids of the active view's uploads this tile draws.
    shared: Vec<u64>,
    drawn: bool,
}

/// The tiles on the GPU, by id.
#[derive(Default)]
pub(super) struct TileDraws(std::collections::HashMap<u64, Tile>);

impl Renderer {
    /// Draw tile `t` offscreen (called from the tile's paint callback `prepare`).
    pub fn prepare_tile(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, t: &TileFrame) {
        self.tiles.0.retain(|id, _| t.all.contains(id));
        let [w, h] = t.frame.size_px;
        let max = self.max_texture.max(1);
        let size = [(w.round().max(1.0) as u32).min(max), (h.round().max(1.0) as u32).min(max)];
        if !self.tiles.0.contains_key(&t.id) {
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("factory_tile_u"),
                size: super::UNIFORM_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let palette = palette_texture(device, 1).create_view(&Default::default());
            let materials = material_texture(device, 1).create_view(&Default::default());
            let [e3, e4, e5, e6] = self.uniform_bgl.environment.entries();
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("factory_tile"),
                layout: &self.uniform_bgl.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&palette) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&materials) },
                    e3,
                    e4,
                    e5,
                    e6,
                ],
            });
            let tile = Tile {
                uniform,
                bind,
                _palette: palette,
                _materials: materials,
                frame: None,
                clouds: CloudDraws::default(),
                sprites: SpriteDraws::default(),
                shared: Vec::new(),
                drawn: false,
            };
            self.tiles.0.insert(t.id, tile);
        }
        // The offscreen frame at the tile's own size (kept while the size stays).
        let needs_frame = self.tiles.0.get(&t.id).is_some_and(|tile| tile.frame.as_ref().is_none_or(|f| f.size != size));
        if needs_frame {
            if let Some(tile) = self.tiles.0.get_mut(&t.id) {
                tile.frame = None;
            }
            match allocated(device, || self.supersampled(device, size)) {
                Ok(frame) => {
                    clear_resolve_targets(encoder, frame.resolve.as_ref());
                    if let Some(tile) = self.tiles.0.get_mut(&t.id) {
                        tile.frame = Some(frame);
                    }
                }
                Err(e) => log::warn!("a display window is not drawn: the GPU refused its {} × {} frame: {e}", size[0], size[1]),
            }
        }
        let reversed = self.target.reversed_depth();
        let stencil = self.target.depth.is_some_and(|f| f.has_stencil_aspect());
        let linear = self.linear_out;
        let Renderer { tiles, clouds, cloud_pipes, sprite_pipes, .. } = self;
        let Some(tile) = tiles.0.get_mut(&t.id) else { return };
        tile.drawn = false;
        let Some(frame) = &tile.frame else { return };
        queue.write_buffer(&tile.uniform, 0, &uniform_bytes(&t.frame, size[0] as f32, size[1] as f32, linear, reversed));
        // Shared: uploaded by the active view at the same version and drawn in the same style.
        let same_style = |c: &GpuCloud| t.frame.clouds.iter().any(|m| m.id == c.id && m.version == c.version && m.style == c.style);
        let (shared, own): (Vec<&GpuCloud>, Vec<&GpuCloud>) = t.clouds.iter().partition(|c| clouds.has(c.id, c.version) && same_style(c));
        tile.shared = shared.iter().map(|c| c.id).collect();
        let own: Vec<GpuCloud> = own.into_iter().cloned().collect();
        tile.clouds.prepare(device, queue, cloud_pipes, &own);
        tile.sprites.prepare_with(device, queue, sprite_pipes, t.sprites.clone());
        let tally = Tally::default();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("factory_tile"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.color,
                depth_slice: None,
                resolve_target: frame.resolve.as_ref(),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: frame.depth.as_ref().map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(far_depth(reversed)), store: wgpu::StoreOp::Discard }),
                stencil_ops: stencil.then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
        pass.set_bind_group(0, &tile.bind, &[]);
        clouds.paint_only(&mut pass, cloud_pipes, &tally, |id| tile.shared.contains(&id));
        tile.clouds.paint(&mut pass, cloud_pipes, &tally);
        tile.sprites.paint(&mut pass, sprite_pipes, &tally);
        drop(pass);
        tile.drawn = true;
    }

    /// Copy tile `id`'s picture into the window's pass at `viewport` (physical pixels).
    pub fn paint_tile(&self, id: u64, pass: &mut wgpu::RenderPass<'static>, viewport: [f32; 4]) {
        let [x, y, w, h] = viewport;
        let Some(tile) = self.tiles.0.get(&id).filter(|t| t.drawn) else { return };
        let Some(frame) = &tile.frame else { return };
        if !(w > 0.0 && h > 0.0) {
            return;
        }
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        pass.set_pipeline(&self.copy_blit);
        pass.set_bind_group(0, &frame.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
