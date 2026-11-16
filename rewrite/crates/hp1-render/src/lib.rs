//! The rewrite's renderer, on wgpu (Metal, Vulkan, D3D12, GLES). It knows nothing about the game's
//! data files: callers hand it vertices, images and draw batches.

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
pub use wgpu;

/// An RGBA8 image.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// Light-map coordinates; negative = use `color` instead of a light map.
    pub lm_uv: [f32; 2],
    /// Light colour when there is no light map (1 = full bright).
    pub color: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    /// Texels with alpha below one half are cut out.
    Masked,
    /// Added on top of what is behind.
    Translucent,
    /// Multiplies what is behind by twice the texture colour.
    Modulated,
    /// Where the sky shows through: writes depth only, so the sky drawn before stays visible.
    Sky,
}

#[derive(Clone, Copy, Debug)]
pub struct DrawBatch {
    pub texture: u32,
    pub lightmap: u32,
    pub blend: Blend,
    pub first_index: u32,
    pub index_count: u32,
}

pub struct SceneDesc<'a> {
    pub vertices: &'a [Vertex],
    pub indices: &'a [u32],
    pub batches: Vec<DrawBatch>,
    pub textures: &'a [Image],
    pub lightmaps: &'a [Image],
}

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The GPU connection.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

pub fn create_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

impl Gpu {
    pub fn new(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Self, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no graphics adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("hp1"),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("no graphics device: {e}"))?;
        Ok(Self { instance, adapter, device, queue })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    params: [f32; 4],
}

/// Draws world geometry into a colour target of one format.
pub struct WorldRenderer {
    pub color_format: wgpu::TextureFormat,
    /// Uniforms for the main view and for the sky view.
    globals: [wgpu::Buffer; 2],
    globals_groups: [wgpu::BindGroup; 2],
    texture_layout: wgpu::BindGroupLayout,
    opaque: wgpu::RenderPipeline,
    masked: wgpu::RenderPipeline,
    translucent: wgpu::RenderPipeline,
    modulated: wgpu::RenderPipeline,
    sky_mask: wgpu::RenderPipeline,
    texture_sampler: wgpu::Sampler,
    lightmap_sampler: wgpu::Sampler,
    pub brightness: f32,
}

/// Vertices, indices and draw batches on the GPU. Can be rewritten every frame.
pub struct Geometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    batches: Vec<DrawBatch>,
}

/// Uploaded images plus any number of geometry sets (the level, the moving objects, …).
pub struct Scene {
    pub geometry: Vec<Geometry>,
    /// Which uploaded texture to draw for each texture index (for animated textures).
    pub texture_map: Vec<u32>,
    textures: Vec<wgpu::BindGroup>,
    texture_handles: Vec<wgpu::Texture>,
    lightmaps: Vec<wgpu::BindGroup>,
}

impl Scene {
    /// Replaces the pixels of texture `index` (same size as uploaded), e.g. a fire texture.
    pub fn update_texture(&self, gpu: &Gpu, index: u32, img: &Image) {
        if let Some(t) = self.texture_handles.get(index as usize)
            && t.width() == img.width
            && t.height() == img.height
        {
            write_chain(gpu, t, &mip_chain(img));
        }
    }
}

impl Geometry {
    pub fn new(gpu: &Gpu, vertices: &[Vertex], indices: &[u32], batches: Vec<DrawBatch>) -> Self {
        let make = |label, bytes: &[u8], usage| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (bytes.len() as u64).max(64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let vb = make("vertices", bytemuck::cast_slice(vertices), wgpu::BufferUsages::VERTEX);
        let ib = make("indices", bytemuck::cast_slice(indices), wgpu::BufferUsages::INDEX);
        let mut g = Self { vertices: vb, indices: ib, batches: Vec::new() };
        g.update(gpu, vertices, indices, batches);
        g
    }

    /// Replaces the contents, growing the buffers when needed.
    pub fn update(&mut self, gpu: &Gpu, vertices: &[Vertex], indices: &[u32], batches: Vec<DrawBatch>) {
        let vbytes: &[u8] = bytemuck::cast_slice(vertices);
        let ibytes: &[u8] = bytemuck::cast_slice(indices);
        if vbytes.len() as u64 > self.vertices.size() || ibytes.len() as u64 > self.indices.size() {
            *self = Self::new(gpu, &[], &[], Vec::new());
            let grow =
                |len: usize| (len as u64 * 3 / 2).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(64);
            self.vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("vertices"),
                size: grow(vbytes.len()),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.indices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("indices"),
                size: grow(ibytes.len()),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let pad = |b: &[u8]| {
            let mut v = b.to_vec();
            v.resize(b.len().next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT as usize), 0);
            v
        };
        if !vbytes.is_empty() {
            gpu.queue.write_buffer(&self.vertices, 0, &pad(vbytes));
        }
        if !ibytes.is_empty() {
            gpu.queue.write_buffer(&self.indices, 0, &pad(ibytes));
        }
        self.batches = batches;
    }
}

fn mip_chain(img: &Image) -> Vec<Image> {
    let mut chain = vec![img.clone()];
    while let Some(last) = chain.last() {
        if last.width == 1 && last.height == 1 {
            break;
        }
        let (w, h) = ((last.width / 2).max(1), (last.height / 2).max(1));
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(last.width - 1);
                    let sy = (y * 2 + dy).min(last.height - 1);
                    let i = ((sy * last.width + sx) * 4) as usize;
                    for k in 0..4 {
                        sum[k] += last.rgba[i + k] as u32;
                    }
                }
                let o = ((y * w + x) * 4) as usize;
                for k in 0..4 {
                    rgba[o + k] = (sum[k] / 4) as u8;
                }
            }
        }
        chain.push(Image { width: w, height: h, rgba });
    }
    chain
}

fn upload_image(gpu: &Gpu, img: &Image, mips: bool, label: &str) -> (wgpu::Texture, wgpu::TextureView) {
    let chain = if mips { mip_chain(img) } else { vec![img.clone()] };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        mip_level_count: chain.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write_chain(gpu, &texture, &chain);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn write_chain(gpu: &Gpu, texture: &wgpu::Texture, chain: &[Image]) {
    for (level, m) in chain.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &m.rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(m.width * 4), rows_per_image: None },
            wgpu::Extent3d { width: m.width, height: m.height, depth_or_array_layers: 1 },
        );
    }
}

impl WorldRenderer {
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world"),
            source: wgpu::ShaderSource::Wgsl(include_str!("world.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<Globals>() as u64),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
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
        let globals = [0, 1].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("globals"),
                size: size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let globals_groups = [0, 1].map(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("globals"),
                layout: &globals_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals[i].as_entire_binding() }],
            })
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2, 3 => Float32x3];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let pipeline = |fs: &str, blend: Option<wgpu::BlendState>, depth_write: bool, write_mask| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: color_format, blend, write_mask })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add,
        };
        let multiply = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::Src,
            operation: wgpu::BlendOperation::Add,
        };
        let all = wgpu::ColorWrites::ALL;
        let opaque = pipeline("fs_opaque", None, true, all);
        let masked = pipeline("fs_masked", None, true, all);
        let translucent = pipeline("fs_color", Some(wgpu::BlendState { color: add, alpha: add }), false, all);
        let modulated =
            pipeline("fs_modulated", Some(wgpu::BlendState { color: multiply, alpha: multiply }), false, all);
        let sky_mask = pipeline("fs_opaque", None, true, wgpu::ColorWrites::empty());
        let texture_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lightmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            color_format,
            globals,
            globals_groups,
            texture_layout,
            opaque,
            masked,
            translucent,
            modulated,
            sky_mask,
            texture_sampler,
            lightmap_sampler,
            brightness: 1.0,
        }
    }

    fn image_group(&self, gpu: &Gpu, view: &wgpu::TextureView, sampler: &wgpu::Sampler) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    pub fn upload(&self, gpu: &Gpu, desc: &SceneDesc) -> Scene {
        let white = Image { width: 1, height: 1, rgba: vec![255; 4] };
        let mut texture_handles = Vec::new();
        let textures: Vec<wgpu::BindGroup> = desc
            .textures
            .iter()
            .chain(desc.textures.is_empty().then_some(&white))
            .map(|img| {
                let (texture, view) = upload_image(gpu, img, true, "texture");
                texture_handles.push(texture);
                self.image_group(gpu, &view, &self.texture_sampler)
            })
            .collect();
        let lightmaps = desc
            .lightmaps
            .iter()
            .chain(desc.lightmaps.is_empty().then_some(&white))
            .map(|img| {
                self.image_group(gpu, &upload_image(gpu, img, false, "lightmap").1, &self.lightmap_sampler)
            })
            .collect();
        let world = Geometry::new(gpu, desc.vertices, desc.indices, desc.batches.clone());
        let texture_map = (0..textures.len() as u32).collect();
        Scene { geometry: vec![world], texture_map, textures, texture_handles, lightmaps }
    }

    /// Records drawing `scene` into `color`/`depth`, clearing both first. With `sky_view`, the
    /// scene is first drawn from the sky's viewpoint, and the sky surfaces of the main view let it
    /// show through; without it, sky surfaces are drawn with their own texture.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        view_proj: Mat4,
        sky_view: Option<Mat4>,
        clear: [f64; 3],
    ) {
        let mut clear_color = Some(clear);
        if let Some(sky) = sky_view {
            self.pass(gpu, encoder, scene, color, depth, sky, 1, clear_color.take(), false);
        }
        self.pass(gpu, encoder, scene, color, depth, view_proj, 0, clear_color, sky_view.is_some());
    }

    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        view_proj: Mat4,
        slot: usize,
        clear: Option<[f64; 3]>,
        sky_as_mask: bool,
    ) {
        let globals =
            Globals { view_proj: view_proj.to_cols_array(), params: [self.brightness, 0.0, 0.0, 0.0] };
        gpu.queue.write_buffer(&self.globals[slot], 0, bytemuck::bytes_of(&globals));
        let load = match clear {
            Some(c) => wgpu::LoadOp::Clear(wgpu::Color { r: c[0], g: c[1], b: c[2], a: 1.0 }),
            None => wgpu::LoadOp::Load,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.globals_groups[slot], &[]);
        // Sky masks first so nothing behind them overwrites the sky, then solid surfaces, then
        // the see-through ones (which don't write depth).
        let phase = |b: Blend| match b {
            Blend::Sky => 0,
            Blend::Opaque | Blend::Masked => 1,
            Blend::Translucent | Blend::Modulated => 2,
        };
        for current in 0..3 {
            for geo in &scene.geometry {
                if geo.batches.iter().all(|b| phase(b.blend) != current) {
                    continue;
                }
                pass.set_vertex_buffer(0, geo.vertices.slice(..));
                pass.set_index_buffer(geo.indices.slice(..), wgpu::IndexFormat::Uint32);
                for b in geo.batches.iter().filter(|b| phase(b.blend) == current) {
                    pass.set_pipeline(match b.blend {
                        Blend::Opaque => &self.opaque,
                        Blend::Masked => &self.masked,
                        Blend::Translucent => &self.translucent,
                        Blend::Modulated => &self.modulated,
                        Blend::Sky if sky_as_mask => &self.sky_mask,
                        Blend::Sky if slot == 1 => continue,
                        Blend::Sky => &self.opaque,
                    });
                    let shown = scene.texture_map.get(b.texture as usize).copied().unwrap_or(b.texture);
                    let tex = scene.textures.get(shown as usize).unwrap_or(&scene.textures[0]);
                    let lm = scene.lightmaps.get(b.lightmap as usize).unwrap_or(&scene.lightmaps[0]);
                    pass.set_bind_group(1, tex, &[]);
                    pass.set_bind_group(2, lm, &[]);
                    pass.draw_indexed(b.first_index..b.first_index + b.index_count, 0, 0..1);
                }
            }
        }
    }
}

pub fn create_depth(gpu: &Gpu, width: u32, height: u32) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// Renders once into an offscreen image (for screenshots and tests without a window).
pub fn render_offscreen(
    gpu: &Gpu,
    renderer: &WorldRenderer,
    scene: &Scene,
    width: u32,
    height: u32,
    view_proj: Mat4,
    sky_view: Option<Mat4>,
) -> Image {
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.color_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = create_depth(gpu, width, height);
    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(gpu, &mut encoder, scene, &view, &depth, view_proj, sky_view, [0.0; 3]);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([encoder.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let Ok(data) = buffer.slice(..).get_mapped_range() else {
        return Image { width, height, rgba: vec![0; (width * height * 4) as usize] };
    };
    let bgra = matches!(
        renderer.color_format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let line = &data[(y * row) as usize..(y * row + width * 4) as usize];
        for px in line.as_chunks::<4>().0 {
            if bgra {
                rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
            } else {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
    }
    Image { width, height, rgba }
}
