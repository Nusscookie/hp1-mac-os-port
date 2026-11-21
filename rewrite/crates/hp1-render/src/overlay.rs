//! Screen-space tiles drawn over the world (the HUD), with crisp unfiltered texels.

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};

use crate::{Gpu, Image, upload_image};

/// A texture rectangle on the screen. Positions are in the units given to `draw`; `uv` is the
/// source rectangle as fractions of the image: left, top, right, bottom.
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub image: u32,
    pub dst: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    /// See-through where the texture's alpha is below one half.
    pub masked: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OverlayVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

pub struct Overlay {
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    normal: wgpu::RenderPipeline,
    masked: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    images: Vec<(wgpu::Texture, wgpu::BindGroup)>,
}

impl Overlay {
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(16),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay texture"),
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
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<OverlayVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let pipeline = |fs: &str| {
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
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let normal = pipeline("fs_normal");
        let masked = pipeline("fs_masked");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("overlay"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Self { globals, globals_group, texture_layout, normal, masked, sampler, images: Vec::new() }
    }

    /// Uploads an image and returns its number for `Quad::image`.
    pub fn add_image(&mut self, gpu: &Gpu, img: &Image) -> u32 {
        let (texture, view) = upload_image(gpu, img, false, "overlay");
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.images.push((texture, group));
        self.images.len() as u32 - 1
    }

    /// Records drawing `quads` over what `target` holds; the screen is `size` units big.
    pub fn draw(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [f32; 2],
        quads: &[Quad],
    ) {
        let quads: Vec<&Quad> = quads
            .iter()
            .filter(|q| (q.image as usize) < self.images.len() && q.dst[2] > 0.0 && q.dst[3] > 0.0)
            .collect();
        if quads.is_empty() {
            return;
        }
        gpu.queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[size[0], size[1], 0.0, 0.0]));
        let mut vertices = Vec::with_capacity(quads.len() * 6);
        for q in &quads {
            let [x, y, w, h] = q.dst;
            let [u0, v0, u1, v1] = q.uv;
            let v = |px, py, u, vv| OverlayVertex { pos: [px, py], uv: [u, vv], color: q.color };
            vertices.extend([
                v(x, y, u0, v0),
                v(x + w, y, u1, v0),
                v(x, y + h, u0, v1),
                v(x + w, y, u1, v0),
                v(x + w, y + h, u1, v1),
                v(x, y + h, u0, v1),
            ]);
        }
        let bytes: &[u8] = bytemuck::cast_slice(&vertices);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay vertices"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&buffer, 0, bytes);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.globals_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for (i, q) in quads.iter().enumerate() {
            pass.set_pipeline(if q.masked { &self.masked } else { &self.normal });
            pass.set_bind_group(1, &self.images[q.image as usize].1, &[]);
            let first = i as u32 * 6;
            pass.draw(first..first + 6, 0..1);
        }
    }
}
