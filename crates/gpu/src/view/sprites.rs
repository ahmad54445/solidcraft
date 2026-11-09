//! [CUSTOM] Factory fork hook: screen-facing sprites from one alpha atlas (e.g. SDF text).
//!
//! Each sprite is a rectangle of constant size in pixels, placed around a world anchor and
//! drawn in the model's main pass with its depth: whatever lies in front of the anchor hides it,
//! every frame, while the camera moves. The atlas holds a signed distance field (0.5 = edge),
//! so glyphs stay sharp at any size and get a contrasting halo. The fork sets the sprites with
//! [`set_sprites`]; nothing here knows about fonts.

use std::sync::{Arc, Mutex};

use super::{Batch, Tally, upload};

/// One sprite: a world anchor, a lift toward the eye (world units, added to 1 % of the distance),
/// its rectangle around the anchor in physical pixels (x right, y down: x0, y0, x1, y1), its
/// rectangle in the atlas (u0, v0, u1, v1) and its colour (sRGB, alpha).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sprite {
    pub anchor: [f32; 3],
    pub lift: f32,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub rgba: [u8; 4],
}

/// Bytes per [`Sprite`] on the GPU.
pub const SPRITE_SIZE: usize = 52;

/// An alpha atlas: one byte per texel, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteAtlas {
    pub size: [u32; 2],
    pub texels: Vec<u8>,
}

/// What is drawn: the atlas (uploaded again when its version changes) and the sprites.
#[derive(Clone, Debug)]
pub struct SpriteSet {
    pub atlas_version: u64,
    pub atlas: Arc<SpriteAtlas>,
    pub version: u64,
    pub sprites: Arc<Vec<Sprite>>,
}

static SPRITES: Mutex<Option<Arc<SpriteSet>>> = Mutex::new(None);

/// Set (or clear) the sprites every viewport draws from now on.
pub fn set_sprites(set: Option<Arc<SpriteSet>>) {
    *SPRITES.lock().unwrap_or_else(|e| e.into_inner()) = set;
}

fn current() -> Option<Arc<SpriteSet>> {
    SPRITES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn sprite_bytes(sprites: &[Sprite]) -> Vec<u8> {
    let mut out = Vec::with_capacity(sprites.len() * SPRITE_SIZE);
    for s in sprites {
        for v in s.anchor.iter().chain([&s.lift]).chain(&s.rect).chain(&s.uv) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&s.rgba);
    }
    out
}

const SHADER: &str = r#"
struct U {
    vp: mat4x4<f32>,
    back: vec4<f32>,
    screen: vec4<f32>,
    clip: vec4<f32>,
    cap: vec4<f32>,
    ana: vec4<f32>,
    pull: vec4<f32>,
    acc_u: vec4<f32>,
    acc_v: vec4<f32>,
    eye: vec4<f32>,
    ao: vec4<f32>,
    ana_c: array<vec4<f32>, 8>,
    ana_f: vec4<f32>,
    depth: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;

struct SIn {
    @location(0) anchor: vec3<f32>,
    @location(1) lift: f32,
    @location(2) rect: vec4<f32>,
    @location(3) uv: vec4<f32>,
    @location(4) rgba: vec4<f32>,
};

struct SOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) rgba: vec4<f32>,
};

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}

@vertex
fn vs_sprite(@builtin(vertex_index) vi: u32, i: SIn) -> SOut {
    var xs = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var ys = array<f32, 6>(0.0, 0.0, 1.0, 0.0, 1.0, 1.0);
    let t = vec2<f32>(xs[vi], ys[vi]);
    let reversed = u.depth.x > 0.5;
    var p = i.anchor;
    if (u.eye.w > 0.5) {
        let d = u.eye.xyz - i.anchor;
        let dist = length(d);
        p = i.anchor + d / max(dist, 1e-9) * (i.lift + 0.01 * dist);
    } else {
        p = i.anchor + u.back.xyz * i.lift;
    }
    let c = u.vp * vec4<f32>(p, 1.0);
    let px = mix(i.rect.xy, i.rect.zw, t);
    let off = vec2<f32>(px.x, -px.y) * 2.0 / u.screen.xy * c.w;
    var z = c.z;
    if (u.eye.w < 0.5) {
        // Orthographic: depth is linear, a fixed step toward the near plane.
        z = z + select(-0.002, 0.002, reversed) * c.w;
    }
    var o: SOut;
    o.pos = vec4<f32>(c.xy + off, z, c.w);
    let near = select(c.z, c.w - c.z, reversed);
    if (near < 0.0) { o.pos = vec4<f32>(0.0, 0.0, 0.0, -1.0); }
    o.uv = mix(i.uv.xy, i.uv.zw, t);
    o.rgba = i.rgba;
    return o;
}

@fragment
fn fs_sprite(o: SOut) -> @location(0) vec4<f32> {
    let d = textureSample(atlas, smp, o.uv).r;
    let w = max(fwidth(d), 1e-4) * 0.7;
    let ink = smoothstep(0.5 - w, 0.5 + w, d);
    let halo = smoothstep(0.36 - w, 0.36 + w, d);
    let lum = dot(o.rgba.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let back = select(vec3<f32>(0.0), vec3<f32>(1.0), lum < 0.5);
    var rgb = mix(back, o.rgba.rgb, ink);
    let a = max(ink, halo * 0.8) * o.rgba.a;
    if (a < 0.02) { discard; }
    if (u.back.w > 0.5) { rgb = to_linear(rgb); }
    return vec4<f32>(rgb, a);
}
"#;

/// The pipeline that draws sprites.
pub(super) struct SpritePipes {
    layout: wgpu::BindGroupLayout,
    pipe: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

impl SpritePipes {
    pub(super) fn new(device: &wgpu::Device, viewport: &wgpu::BindGroupLayout, target: super::GpuTarget, depth: Option<wgpu::DepthStencilState>) -> SpritePipes {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_sprites"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_sprites"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_sprites"),
            bind_group_layouts: &[Some(viewport), Some(&layout)],
            immediate_size: 0,
        });
        const A: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x4, 3 => Float32x4, 4 => Unorm8x4];
        let buffer = wgpu::VertexBufferLayout { array_stride: SPRITE_SIZE as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &A };
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_sprites"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_sprite"), buffers: &[Some(buffer)], compilation_options: Default::default() },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: depth,
            multisample: wgpu::MultisampleState { count: target.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_sprite"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_sprites"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        SpritePipes { layout, pipe, sampler }
    }
}

/// The sprites on the GPU.
#[derive(Default)]
pub(super) struct SpriteDraws {
    atlas: Option<(u64, wgpu::BindGroup)>,
    sprites: Option<(u64, Option<Batch>)>,
    shown: bool,
}

impl SpriteDraws {
    /// Upload a new atlas or new sprites.
    pub(super) fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pipes: &SpritePipes) {
        let Some(set) = current() else {
            self.shown = false;
            return;
        };
        self.shown = true;
        if self.atlas.as_ref().is_none_or(|(v, _)| *v != set.atlas_version) {
            let [w, h] = set.atlas.size.map(|v| v.max(1));
            let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sc_sprites_atlas"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let mut texels = set.atlas.texels.clone();
            texels.resize((w * h) as usize, 0);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &texels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w), rows_per_image: Some(h) },
                size,
            );
            let view = texture.create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sc_sprites_atlas"),
                layout: &pipes.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&pipes.sampler) },
                ],
            });
            self.atlas = Some((set.atlas_version, bind));
        }
        if self.sprites.as_ref().is_none_or(|(v, _)| *v != set.version) {
            self.sprites = Some((set.version, upload(device, "sc_sprites", &sprite_bytes(&set.sprites), SPRITE_SIZE)));
        }
    }

    pub(super) fn paint(&self, pass: &mut wgpu::RenderPass<'_>, pipes: &SpritePipes, tally: &Tally) {
        if !self.shown {
            return;
        }
        let (Some((_, bind)), Some((_, Some(batch)))) = (&self.atlas, &self.sprites) else { return };
        pass.set_bind_group(1, bind, &[]);
        pass.set_pipeline(&pipes.pipe);
        for (buffer, count) in &batch.parts {
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..6, 0..*count);
            tally.triangles(6, *count);
        }
    }
}

/// What the sprites add to a picture's key.
pub(super) fn hash_sprites(h: &mut impl std::hash::Hasher) {
    use std::hash::Hash as _;
    match current() {
        Some(s) => (s.atlas_version, s.version).hash(h),
        None => 0u64.hash(h),
    }
}
