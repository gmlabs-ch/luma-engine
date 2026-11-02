//! The quad renderer: turns a [`DrawList`] into GPU work.
//!
//! The rotation of a quad around its pivot and the blend factors of each
//! mode follow `BRenderD3D.cpp` of Wintermute Engine 1.x (MIT License,
//! Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use std::cell::RefCell;
use std::collections::HashMap;

use luma_formats::Rgba;
use wgpu::util::DeviceExt;

use crate::draw::{Attributes, BlendMode, DrawList, Quad};
use crate::error::{Error, Result};
use crate::gpu::Gpu;
use crate::texture::Texture;

/// One vertex of a quad.
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Globals {
    projection: [[f32; 4]; 4],
}

/// A texture or surface to render into.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    /// The view to draw on.
    pub view: &'a wgpu::TextureView,
    /// Its format, which selects the pipeline.
    pub format: wgpu::TextureFormat,
    /// Its size in physical pixels.
    pub width: u32,
    /// See `width`.
    pub height: u32,
}

/// Draws textured quads into any target, blended as their attributes
/// say.
///
/// Quads are batched: consecutive quads with the same texture and blend
/// mode become one draw call, so sort them by texture when the order
/// does not matter.
#[derive(Debug)]
pub struct Renderer {
    gpu: Gpu,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    /// One pipeline per target format and blend mode.
    pipelines: RefCell<HashMap<(wgpu::TextureFormat, BlendMode), wgpu::RenderPipeline>>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    vertices: RefCell<wgpu::Buffer>,
    indices: RefCell<(wgpu::Buffer, u32)>,
    white: Texture,
    bind_groups: RefCell<HashMap<u64, wgpu::BindGroup>>,
}

impl Renderer {
    /// Creates a renderer on a device.
    pub fn new(gpu: Gpu) -> Renderer {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture layout"),
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
            label: Some("quad pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bilinear clamp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad vertices"),
            size: 4 * std::mem::size_of::<Vertex>() as u64 * 256,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let indices = index_buffer(device, 256);
        let white = Texture::white(&gpu);
        Renderer {
            gpu,
            shader,
            pipeline_layout,
            pipelines: RefCell::new(HashMap::new()),
            texture_layout,
            sampler,
            globals,
            globals_group,
            vertices: RefCell::new(vertices),
            indices: RefCell::new(indices),
            white,
            bind_groups: RefCell::new(HashMap::new()),
        }
    }

    /// The device the renderer draws with.
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// A 1x1 white texture for untextured rectangles.
    pub fn white(&self) -> &Texture {
        &self.white
    }

    /// Uploads an image as a texture.
    pub fn texture(&self, label: &str, image: &luma_formats::Image) -> Texture {
        Texture::from_image(&self.gpu, label, image)
    }

    /// Uploads RGBA pixels as a texture.
    pub fn texture_from_rgba(&self, label: &str, width: u32, height: u32, rgba: &[u8]) -> Texture {
        Texture::from_rgba(&self.gpu, label, width, height, rgba)
    }

    /// Replaces the texels of a texture with RGBA pixels of the same
    /// size (a video frame).
    pub fn write_texture(&self, texture: &Texture, rgba: &[u8]) {
        texture.write(&self.gpu, rgba);
    }

    /// Renders a draw list into a target, clearing it first, and submits
    /// the work. The GPU runs it asynchronously; see [`Gpu::wait`].
    pub fn render(&self, target: Target<'_>, clear: Rgba, list: &DrawList<'_>) -> Result<()> {
        if target.width == 0 || target.height == 0 {
            return Err(Error::Render {
                reason: "empty target".into(),
            });
        }
        let vertices = build_vertices(list);
        let quad_count = list.quads().len();
        self.ensure_capacity(quad_count);
        let vertex_buffer = self.vertices.borrow();
        let index_buffer = self.indices.borrow();
        if !vertices.is_empty() {
            self.gpu
                .queue
                .write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        }
        let (x, y, w, h) = viewport(list, target.width, target.height);
        self.gpu.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals {
                projection: orthographic(list.width, list.height),
            }),
        );
        for quad in list.quads() {
            self.ensure_bind_group(quad.texture);
        }
        self.ensure_pipeline(target.format, BlendMode::Normal);
        for attributes in list.attributes() {
            self.ensure_pipeline(target.format, attributes.blend);
        }
        let pipelines = self.pipelines.borrow();
        let bind_groups = self.bind_groups.borrow();

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("luma frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("quads"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(clear.r) / 255.0,
                            g: f64::from(clear.g) / 255.0,
                            b: f64::from(clear.b) / 255.0,
                            a: f64::from(clear.a) / 255.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
            pass.set_pipeline(&pipelines[&(target.format, BlendMode::Normal)]);
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.set_index_buffer(index_buffer.0.slice(..), wgpu::IndexFormat::Uint32);
            let mut first = 0;
            let quads = list.quads();
            let attributes = list.attributes();
            let blend_of = |i: usize| attributes.get(i).map_or(BlendMode::Normal, |a| a.blend);
            let mut blend = BlendMode::Normal;
            while first < quads.len() {
                let texture = quads[first].texture;
                let batch_blend = blend_of(first);
                let mut last = first + 1;
                while last < quads.len()
                    && quads[last].texture.id == texture.id
                    && blend_of(last) == batch_blend
                {
                    last += 1;
                }
                if batch_blend != blend {
                    blend = batch_blend;
                    pass.set_pipeline(&pipelines[&(target.format, blend)]);
                }
                let group = &bind_groups[&texture.id];
                pass.set_bind_group(1, group, &[]);
                pass.draw_indexed((first as u32 * 6)..(last as u32 * 6), 0, 0..1);
                first = last;
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Forgets the cached binding of a texture that is no longer used.
    /// Optional: bindings are small, but a game that streams many images
    /// should release them.
    pub fn forget(&self, texture: &Texture) {
        self.bind_groups.borrow_mut().remove(&texture.id);
    }

    fn ensure_capacity(&self, quads: usize) {
        let needed = (quads.max(1) * 4 * std::mem::size_of::<Vertex>()) as u64;
        let mut vertices = self.vertices.borrow_mut();
        if vertices.size() < needed {
            *vertices = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad vertices"),
                size: needed.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let mut indices = self.indices.borrow_mut();
        if (indices.1 as usize) < quads * 6 {
            *indices = index_buffer(&self.gpu.device, quads.next_power_of_two());
        }
    }

    fn ensure_bind_group(&self, texture: &Texture) {
        let id = texture.id;
        if self.bind_groups.borrow().contains_key(&id) {
            return;
        }
        let group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("texture"),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
        self.bind_groups.borrow_mut().insert(id, group);
    }

    fn ensure_pipeline(&self, format: wgpu::TextureFormat, blend: BlendMode) {
        if self.pipelines.borrow().contains_key(&(format, blend)) {
            return;
        }
        let pipeline = self
            .gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("quads"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend_state(blend)),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        self.pipelines
            .borrow_mut()
            .insert((format, blend), pipeline);
    }
}

/// The blend factors of a mode, as `CBRenderD3D::SetSpriteBlendMode` sets
/// them for the colour channels: normal is `src_alpha,
/// one_minus_src_alpha`; additive adds the source weighted by its alpha
/// to the destination; subtractive keeps `1 - src` of the destination.
/// Direct3D 8 applied the same factors to the alpha channel, which the
/// screen ignores; Luma keeps the target's alpha covered instead, so that
/// a screenshot of an additive or subtractive sprite is not transparent
/// there.
fn blend_state(blend: BlendMode) -> wgpu::BlendState {
    let component = |src: wgpu::BlendFactor, dst: wgpu::BlendFactor| wgpu::BlendComponent {
        src_factor: src,
        dst_factor: dst,
        operation: wgpu::BlendOperation::Add,
    };
    match blend {
        BlendMode::Normal => wgpu::BlendState::ALPHA_BLENDING,
        BlendMode::Additive => wgpu::BlendState {
            color: component(wgpu::BlendFactor::SrcAlpha, wgpu::BlendFactor::One),
            alpha: component(wgpu::BlendFactor::One, wgpu::BlendFactor::One),
        },
        BlendMode::Subtractive => wgpu::BlendState {
            color: component(wgpu::BlendFactor::Zero, wgpu::BlendFactor::OneMinusSrc),
            alpha: component(wgpu::BlendFactor::Zero, wgpu::BlendFactor::One),
        },
    }
}

/// Six indices per quad: two triangles.
fn index_buffer(device: &wgpu::Device, quads: usize) -> (wgpu::Buffer, u32) {
    let mut indices = Vec::with_capacity(quads * 6);
    for q in 0..quads as u32 {
        let v = q * 4;
        indices.extend_from_slice(&[v, v + 1, v + 2, v, v + 2, v + 3]);
    }
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("quad indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    (buffer, indices.len() as u32)
}

/// Four vertices per quad: top-left, top-right, bottom-right, bottom-left.
fn build_vertices(list: &DrawList<'_>) -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(list.quads().len() * 4);
    let attributes = list.attributes();
    for (i, quad) in list.quads().iter().enumerate() {
        let attrs = attributes.get(i).copied().unwrap_or_default();
        vertices.extend_from_slice(&quad_vertices(quad, &attrs));
    }
    vertices
}

/// The corners of a quad turned by its attributes' rotation around the
/// pivot, as `CBRenderD3D::TransformVertices` does: the angle is in
/// degrees, positive clockwise on a screen whose `y` grows downwards.
pub fn rotated_corners(dst: crate::draw::Rect, attrs: &Attributes) -> [(f32, f32); 4] {
    let (x0, y0, x1, y1) = (dst.x, dst.y, dst.x + dst.w, dst.y + dst.h);
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    if !attrs.is_rotated() {
        return corners;
    }
    let (sin, cos) = attrs.rotation.to_radians().sin_cos();
    let (px, py) = attrs.pivot;
    corners.map(|(x, y)| {
        let (dx, dy) = (x - px, y - py);
        (px + dx * cos - dy * sin, py + dx * sin + dy * cos)
    })
}

fn quad_vertices(quad: &Quad<'_>, attrs: &Attributes) -> [Vertex; 4] {
    let (tw, th) = (quad.texture.width as f32, quad.texture.height as f32);
    let (mut u0, mut v0, mut u1, mut v1) = match quad.src {
        Some(src) => (
            src.x / tw,
            src.y / th,
            (src.x + src.w) / tw,
            (src.y + src.h) / th,
        ),
        None => (0.0, 0.0, 1.0, 1.0),
    };
    if quad.flip_x {
        std::mem::swap(&mut u0, &mut u1);
    }
    if quad.flip_y {
        std::mem::swap(&mut v0, &mut v1);
    }
    let color = [
        f32::from(quad.tint.r) / 255.0,
        f32::from(quad.tint.g) / 255.0,
        f32::from(quad.tint.b) / 255.0,
        f32::from(quad.tint.a) / 255.0,
    ];
    let [tl, tr, br, bl] = rotated_corners(quad.dst, attrs);
    [
        Vertex {
            position: [tl.0, tl.1],
            uv: [u0, v0],
            color,
        },
        Vertex {
            position: [tr.0, tr.1],
            uv: [u1, v0],
            color,
        },
        Vertex {
            position: [br.0, br.1],
            uv: [u1, v1],
            color,
        },
        Vertex {
            position: [bl.0, bl.1],
            uv: [u0, v1],
            color,
        },
    ]
}

/// Column-major orthographic projection mapping `(0,0)` to the top-left
/// corner and `(width, height)` to the bottom-right one.
fn orthographic(width: f32, height: f32) -> [[f32; 4]; 4] {
    let w = width.max(1.0);
    let h = height.max(1.0);
    [
        [2.0 / w, 0.0, 0.0, 0.0],
        [0.0, -2.0 / h, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0, 1.0],
    ]
}

/// The physical rectangle the logical size maps onto: the whole target,
/// or the largest centred rectangle with the list's aspect ratio.
pub(crate) fn viewport(list: &DrawList<'_>, width: u32, height: u32) -> (f32, f32, f32, f32) {
    if !list.letterbox {
        return (0.0, 0.0, width as f32, height as f32);
    }
    letterbox(list.width, list.height, width, height)
}

/// The largest centred rectangle of a target of `width` by `height`
/// physical pixels with the aspect ratio of a logical size, as
/// `(x, y, w, h)`: where a letterboxed draw list lands, and the mapping
/// to turn a window position into a logical one.
pub fn letterbox(
    logical_width: f32,
    logical_height: f32,
    width: u32,
    height: u32,
) -> (f32, f32, f32, f32) {
    let (tw, th) = (width as f32, height as f32);
    if logical_width <= 0.0 || logical_height <= 0.0 {
        return (0.0, 0.0, tw, th);
    }
    let scale = (tw / logical_width).min(th / logical_height);
    let w = (logical_width * scale).floor();
    let h = (logical_height * scale).floor();
    (((tw - w) / 2.0).floor(), ((th - h) / 2.0).floor(), w, h)
}
