//! The renderer: turns a [`DrawList`] into GPU work, quads and meshes in
//! the list's order.
//!
//! The rotation of a quad around its pivot, the blend factors of each
//! mode and the 3D render state (depth test, alpha test, lighting)
//! follow `BRenderD3D.cpp` of Wintermute Engine 1.x, the stencil states
//! of the shadow volumes `3DShadowVolume.cpp` and the depth-only
//! receivers `AdSceneGeometry.cpp` (MIT License, Copyright (c) 2003-2013
//! Jan Nedoma); see `NOTICE`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use luma_formats::Rgba;
use wgpu::util::DeviceExt;

use crate::draw::{
    Attributes, BlendMode, Draw3d, DrawList, Light, MAX_LIGHTS, Matrix, MeshDraw, MeshHandle,
    MeshVertex, Quad, multiply,
};
use crate::error::{Error, Result};
use crate::gpu::Gpu;
use crate::texture::Texture;

/// The format of the depth and stencil buffer every target gets: 24-bit
/// depth and 8-bit stencil (for the shadow volumes), as the original
/// asked of Direct3D.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

/// The alpha test reference of the 3D state (`D3DRS_ALPHAREF` 8): a
/// fragment of a mesh with less alpha is discarded.
pub const MESH_ALPHA_REFERENCE: f32 = 8.0 / 255.0;

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

/// A light as the mesh shader reads it; see `mesh.wgsl`.
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct GpuLight {
    /// `xyz` position, `w` kind (0 none, 1 directional, 2 point, 3 spot).
    position: [f32; 4],
    /// `xyz` direction, `w` range.
    direction: [f32; 4],
    color: [f32; 4],
    /// Cosine of the half inner angle, of the half outer angle, falloff.
    cone: [f32; 4],
}

/// The uniforms of one mesh draw; see `mesh.wgsl`.
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct MeshUniforms {
    world: [[f32; 4]; 4],
    view_projection: [[f32; 4]; 4],
    diffuse: [f32; 4],
    emissive: [f32; 4],
    ambient: [f32; 4],
    fog_color: [f32; 4],
    fog_params: [f32; 4],
    lights: [GpuLight; MAX_LIGHTS],
}

/// The uniforms of one position-only draw; see `shadow.wgsl`.
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct SimpleUniforms {
    mvp: [[f32; 4]; 4],
    color: [f32; 4],
}

/// The position-only pipelines of the stencil shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SimpleKind {
    /// A shadow volume's faces turned toward the camera: the stencil is
    /// incremented where they pass the depth test.
    VolumeIncrement,
    /// The faces turned away: decremented.
    VolumeDecrement,
    /// A mesh written to the depth buffer only.
    DepthOnly,
    /// The fill where the stencil count is one or more.
    Fill,
}

/// The mesh pipeline variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MeshVariant {
    format: wgpu::TextureFormat,
    blend: BlendMode,
    cull: bool,
    depth_write: bool,
    stencil_once: bool,
}

impl MeshVariant {
    fn of(format: wgpu::TextureFormat, draw: &MeshDraw<'_>) -> MeshVariant {
        MeshVariant {
            format,
            blend: draw.blend,
            cull: draw.cull_backfaces,
            depth_write: draw.depth_write,
            stencil_once: draw.stencil_once.is_some(),
        }
    }
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

/// The depth and stencil buffer of the targets of a size.
#[derive(Debug)]
struct DepthBuffer {
    width: u32,
    height: u32,
    view: wgpu::TextureView,
}

/// A mesh on the GPU.
#[derive(Debug)]
struct GpuMesh {
    vertices: wgpu::Buffer,
    vertex_capacity: usize,
    indices: wgpu::Buffer,
    index_count: u32,
}

/// Draws textured quads and lit meshes into any target, blended as
/// their attributes say.
///
/// Quads are batched: consecutive quads with the same texture and blend
/// mode become one draw call, so sort them by texture when the order
/// does not matter. A mesh draw between two quads ends a batch.
#[derive(Debug)]
pub struct Renderer {
    gpu: Gpu,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    /// One quad pipeline per target format and blend mode.
    pipelines: RefCell<HashMap<(wgpu::TextureFormat, BlendMode), wgpu::RenderPipeline>>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    vertices: RefCell<wgpu::Buffer>,
    indices: RefCell<(wgpu::Buffer, u32)>,
    white: Texture,
    bind_groups: RefCell<HashMap<u64, wgpu::BindGroup>>,
    depth: RefCell<Option<DepthBuffer>>,

    mesh_shader: wgpu::ShaderModule,
    mesh_pipeline_layout: wgpu::PipelineLayout,
    /// One mesh pipeline per target format, blend mode, culling, depth
    /// write and stencil variant.
    mesh_pipelines: RefCell<HashMap<MeshVariant, wgpu::RenderPipeline>>,
    mesh_uniform_layout: wgpu::BindGroupLayout,
    /// The uniforms of every mesh draw of the frame, one aligned slot
    /// each, bound with a dynamic offset.
    mesh_uniforms: RefCell<(wgpu::Buffer, wgpu::BindGroup)>,
    mesh_uniform_stride: u64,
    mesh_sampler: wgpu::Sampler,
    /// Texture bindings with the mesh sampler.
    mesh_bind_groups: RefCell<HashMap<u64, wgpu::BindGroup>>,
    meshes: RefCell<HashMap<u64, GpuMesh>>,
    next_mesh: Cell<u64>,

    simple_shader: wgpu::ShaderModule,
    simple_pipeline_layout: wgpu::PipelineLayout,
    /// The position-only pipelines, per target format and kind.
    simple_pipelines: RefCell<HashMap<(wgpu::TextureFormat, SimpleKind), wgpu::RenderPipeline>>,
    simple_uniform_layout: wgpu::BindGroupLayout,
    /// The uniforms of every position-only draw of the frame, one
    /// aligned slot each.
    simple_uniforms: RefCell<(wgpu::Buffer, wgpu::BindGroup)>,
    simple_uniform_stride: u64,
    /// The vertices of the frame's shadow volumes, concatenated.
    volume_vertices: RefCell<wgpu::Buffer>,

    blit_shader: wgpu::ShaderModule,
    blit_pipeline_layout: wgpu::PipelineLayout,
    /// The pipeline that scales a native frame onto a target, per
    /// target format.
    blit_pipelines: RefCell<HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>>,
    /// The frame at the list's own resolution, when a letterboxed list
    /// is scaled onto its target.
    native: RefCell<Option<NativeFrame>>,
}

/// The texture a letterboxed frame is drawn into before it is scaled.
#[derive(Debug)]
struct NativeFrame {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

impl Renderer {
    /// Creates a renderer on a device.
    pub fn new(gpu: Gpu) -> Renderer {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });
        let simple_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()),
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
        let mesh_uniform_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("mesh uniforms layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<MeshUniforms>() as u64,
                        ),
                    },
                    count: None,
                }],
            });
        let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh pipeline layout"),
            bind_group_layouts: &[Some(&mesh_uniform_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let simple_uniform_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shadow uniforms layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<SimpleUniforms>() as u64,
                        ),
                    },
                    count: None,
                }],
            });
        let simple_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow pipeline layout"),
                bind_group_layouts: &[Some(&simple_uniform_layout)],
                immediate_size: 0,
            });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit pipeline layout"),
            bind_group_layouts: &[Some(&texture_layout)],
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
        // `Setup3D`: linear magnification and minification, linear mip
        // filter, repeat addressing (the Direct3D default).
        let mesh_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("trilinear repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
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
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let mesh_uniform_stride =
            (std::mem::size_of::<MeshUniforms>() as u64).div_ceil(alignment) * alignment;
        let mesh_uniforms =
            mesh_uniform_buffer(device, &mesh_uniform_layout, mesh_uniform_stride, 16);
        let simple_uniform_stride =
            (std::mem::size_of::<SimpleUniforms>() as u64).div_ceil(alignment) * alignment;
        let simple_uniforms = uniform_buffer(
            device,
            "shadow uniforms",
            &simple_uniform_layout,
            simple_uniform_stride,
            std::mem::size_of::<SimpleUniforms>() as u64,
            16,
        );
        let volume_vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow volume vertices"),
            size: 12 * 1024,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
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
            depth: RefCell::new(None),
            mesh_shader,
            mesh_pipeline_layout,
            mesh_pipelines: RefCell::new(HashMap::new()),
            mesh_uniform_layout,
            mesh_uniforms: RefCell::new(mesh_uniforms),
            mesh_uniform_stride,
            mesh_sampler,
            mesh_bind_groups: RefCell::new(HashMap::new()),
            meshes: RefCell::new(HashMap::new()),
            next_mesh: Cell::new(1),
            simple_shader,
            simple_pipeline_layout,
            simple_pipelines: RefCell::new(HashMap::new()),
            simple_uniform_layout,
            simple_uniforms: RefCell::new(simple_uniforms),
            simple_uniform_stride,
            volume_vertices: RefCell::new(volume_vertices),
            blit_shader,
            blit_pipeline_layout,
            blit_pipelines: RefCell::new(HashMap::new()),
            native: RefCell::new(None),
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

    /// Uploads an image as a texture with its mipmap chain, for meshes:
    /// the original engine let D3DX build mipmaps for the textures of
    /// models and sampled them trilinearly. The chain is built on the CPU
    /// with a box filter whose colours are weighted by alpha, so that the
    /// transparent texels of a colour-keyed image do not bleed into their
    /// neighbours.
    pub fn mesh_texture(&self, label: &str, image: &luma_formats::Image) -> Texture {
        Texture::from_image_mipmapped(&self.gpu, label, image)
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

    /// Uploads a mesh: vertices and the indices of its triangles (three
    /// per triangle, front faces clockwise on screen). The vertices can be
    /// replaced later with [`Renderer::update_mesh`], for skinning.
    ///
    /// # Panics
    ///
    /// Panics when there is no vertex or no index.
    pub fn upload_mesh(&self, vertices: &[MeshVertex], indices: &[u32]) -> MeshHandle {
        assert!(!vertices.is_empty(), "a mesh needs vertices");
        assert!(!indices.is_empty(), "a mesh needs indices");
        let device = &self.gpu.device;
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let id = self.next_mesh.get();
        self.next_mesh.set(id + 1);
        self.meshes.borrow_mut().insert(
            id,
            GpuMesh {
                vertices: vertex_buffer,
                vertex_capacity: vertices.len(),
                indices: index_buffer,
                index_count: indices.len() as u32,
            },
        );
        MeshHandle(id)
    }

    /// Replaces the vertices of a mesh (the skinned positions and normals
    /// of this frame). The indices stay; more vertices than uploaded grow
    /// the buffer, fewer leave the rest as they were.
    pub fn update_mesh(&self, mesh: MeshHandle, vertices: &[MeshVertex]) -> Result<()> {
        let mut meshes = self.meshes.borrow_mut();
        let gpu_mesh = meshes.get_mut(&mesh.0).ok_or_else(|| Error::Render {
            reason: format!("unknown mesh {}", mesh.0),
        })?;
        if vertices.is_empty() {
            return Ok(());
        }
        if vertices.len() > gpu_mesh.vertex_capacity {
            gpu_mesh.vertices =
                self.gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("mesh vertices"),
                        contents: bytemuck::cast_slice(vertices),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    });
            gpu_mesh.vertex_capacity = vertices.len();
        } else {
            self.gpu
                .queue
                .write_buffer(&gpu_mesh.vertices, 0, bytemuck::cast_slice(vertices));
        }
        Ok(())
    }

    /// Releases a mesh. Drawing it afterwards is an error.
    pub fn free_mesh(&self, mesh: MeshHandle) {
        self.meshes.borrow_mut().remove(&mesh.0);
    }

    /// Renders a draw list into a target, clearing it first (colour,
    /// depth and stencil), and submits the work. The GPU runs it
    /// asynchronously; see [`Gpu::wait`].
    ///
    /// A letterboxed list whose size is not the target's is drawn at its
    /// own size first, as the game was made to be seen, and that frame is
    /// then scaled onto the target with its pixels kept sharp.
    pub fn render(&self, target: Target<'_>, clear: Rgba, list: &DrawList<'_>) -> Result<()> {
        if target.width == 0 || target.height == 0 {
            return Err(Error::Render {
                reason: "empty target".into(),
            });
        }
        if list.letterbox && list.width.is_finite() && list.height.is_finite() {
            let width = list.width.round().clamp(1.0, 8192.0) as u32;
            let height = list.height.round().clamp(1.0, 8192.0) as u32;
            if (width, height) != (target.width, target.height) {
                return self.render_scaled(target, clear, list, width, height);
            }
        }
        self.render_direct(target, clear, list)
    }

    /// Draws the list into a texture of its own size, then that texture
    /// into the letterbox rectangle of the target.
    fn render_scaled(
        &self,
        target: Target<'_>,
        clear: Rgba,
        list: &DrawList<'_>,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.ensure_native(width, height, target.format);
        self.ensure_blit_pipeline(target.format);
        let native = self.native.borrow();
        let native = native.as_ref().expect("native frame created above");
        self.render_direct(
            Target {
                view: &native.view,
                format: target.format,
                width,
                height,
            },
            clear,
            list,
        )?;
        let (x, y, w, h) = letterbox(width as f32, height as f32, target.width, target.height);
        let pipelines = self.blit_pipelines.borrow();
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("luma frame scale"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scale"),
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
            if w >= 1.0 && h >= 1.0 {
                pass.set_viewport(x, y, w, h, 0.0, 1.0);
                pass.set_pipeline(&pipelines[&target.format]);
                pass.set_bind_group(0, &native.group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        Ok(())
    }

    fn ensure_native(&self, width: u32, height: u32, format: wgpu::TextureFormat) {
        let mut native = self.native.borrow_mut();
        if native
            .as_ref()
            .is_some_and(|n| (n.width, n.height, n.format) == (width, height, format))
        {
            return;
        }
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("native frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("native frame"),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
        *native = Some(NativeFrame {
            width,
            height,
            format,
            view,
            group,
        });
    }

    fn ensure_blit_pipeline(&self, format: wgpu::TextureFormat) {
        if self.blit_pipelines.borrow().contains_key(&format) {
            return;
        }
        let pipeline = self
            .gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("scale"),
                layout: Some(&self.blit_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.blit_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.blit_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        self.blit_pipelines.borrow_mut().insert(format, pipeline);
    }

    fn render_direct(&self, target: Target<'_>, clear: Rgba, list: &DrawList<'_>) -> Result<()> {
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
        let draws_3d = list.draws_3d();
        // The uniform slot of each 3D draw: meshes in the mesh buffer,
        // the position-only draws in the other; the volumes' vertices
        // in one buffer, each with its range.
        let mut mesh_slots = 0usize;
        let mut simple_slots = 0usize;
        let mut volume_vertices: Vec<[f32; 3]> = Vec::new();
        let mut volume_ranges: Vec<std::ops::Range<u32>> = Vec::new();
        for placed in draws_3d {
            match &placed.draw {
                Draw3d::Mesh(_) => mesh_slots += 1,
                Draw3d::ShadowVolume(volume) => {
                    simple_slots += 1;
                    let start = volume_vertices.len() as u32;
                    volume_vertices.extend_from_slice(&volume.vertices);
                    volume_ranges.push(start..volume_vertices.len() as u32);
                }
                Draw3d::DepthOnly(_) | Draw3d::StencilFill(_) => simple_slots += 1,
            }
        }
        self.ensure_mesh_uniform_capacity(mesh_slots);
        self.ensure_simple_uniform_capacity(simple_slots);
        self.ensure_volume_capacity(volume_vertices.len());
        {
            let meshes = self.meshes.borrow();
            let mut uniforms = vec![0u8; mesh_slots * self.mesh_uniform_stride as usize];
            let mut simple = vec![0u8; simple_slots * self.simple_uniform_stride as usize];
            let (mut mesh_index, mut simple_index) = (0usize, 0usize);
            let mut write_simple = |index: usize, slot: SimpleUniforms| {
                let start = index * self.simple_uniform_stride as usize;
                let bytes = bytemuck::bytes_of(&slot);
                simple[start..start + bytes.len()].copy_from_slice(bytes);
            };
            for placed in draws_3d {
                match &placed.draw {
                    Draw3d::Mesh(draw) => {
                        if !meshes.contains_key(&draw.mesh.0) {
                            return Err(Error::Render {
                                reason: format!("unknown mesh {}", draw.mesh.0),
                            });
                        }
                        self.ensure_mesh_bind_group(draw.texture.unwrap_or(&self.white));
                        self.ensure_mesh_pipeline(MeshVariant::of(target.format, draw));
                        let start = mesh_index * self.mesh_uniform_stride as usize;
                        let slot = mesh_uniforms(draw);
                        let bytes = bytemuck::bytes_of(&slot);
                        uniforms[start..start + bytes.len()].copy_from_slice(bytes);
                        mesh_index += 1;
                    }
                    Draw3d::ShadowVolume(volume) => {
                        self.ensure_simple_pipeline(target.format, SimpleKind::VolumeIncrement);
                        self.ensure_simple_pipeline(target.format, SimpleKind::VolumeDecrement);
                        write_simple(
                            simple_index,
                            simple_uniforms(&volume.view_projection, &volume.world, [0.0; 4]),
                        );
                        simple_index += 1;
                    }
                    Draw3d::DepthOnly(draw) => {
                        if !meshes.contains_key(&draw.mesh.0) {
                            return Err(Error::Render {
                                reason: format!("unknown mesh {}", draw.mesh.0),
                            });
                        }
                        self.ensure_simple_pipeline(target.format, SimpleKind::DepthOnly);
                        write_simple(
                            simple_index,
                            simple_uniforms(&draw.view_projection, &draw.world, [0.0; 4]),
                        );
                        simple_index += 1;
                    }
                    Draw3d::StencilFill(fill) => {
                        self.ensure_simple_pipeline(target.format, SimpleKind::Fill);
                        let c = fill.color;
                        let color = [c.r, c.g, c.b, c.a].map(|v| f32::from(v) / 255.0);
                        write_simple(
                            simple_index,
                            simple_uniforms(&crate::draw::IDENTITY, &crate::draw::IDENTITY, color),
                        );
                        simple_index += 1;
                    }
                }
            }
            if !uniforms.is_empty() {
                self.gpu
                    .queue
                    .write_buffer(&self.mesh_uniforms.borrow().0, 0, &uniforms);
            }
            if !simple.is_empty() {
                self.gpu
                    .queue
                    .write_buffer(&self.simple_uniforms.borrow().0, 0, &simple);
            }
            if !volume_vertices.is_empty() {
                self.gpu.queue.write_buffer(
                    &self.volume_vertices.borrow(),
                    0,
                    bytemuck::cast_slice(&volume_vertices),
                );
            }
        }
        self.ensure_depth(target.width, target.height);
        let pipelines = self.pipelines.borrow();
        let bind_groups = self.bind_groups.borrow();
        let mesh_pipelines = self.mesh_pipelines.borrow();
        let mesh_bind_groups = self.mesh_bind_groups.borrow();
        let mesh_uniforms = self.mesh_uniforms.borrow();
        let simple_pipelines = self.simple_pipelines.borrow();
        let simple_uniforms = self.simple_uniforms.borrow();
        let volume_buffer = self.volume_vertices.borrow();
        let meshes = self.meshes.borrow();
        let depth = self.depth.borrow();
        let depth_view = &depth.as_ref().expect("depth buffer created above").view;

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("luma frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
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
                // `CBRenderD3D::Fill`: depth and stencil are cleared once
                // per frame, with the colour.
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
            let quads = list.quads();
            let attributes = list.attributes();
            let blend_of = |i: usize| attributes.get(i).map_or(BlendMode::Normal, |a| a.blend);
            // The quad state; `None` after 3D items were drawn, so that
            // it is set again before the next batch.
            let mut quad_state: Option<BlendMode> = None;
            let mut next_3d = 0;
            let (mut mesh_index, mut simple_index, mut volume_index) = (0usize, 0usize, 0usize);
            let mut first = 0;
            loop {
                // The 3D items due before the quad `first` (all the
                // remaining ones once the quads are done).
                while next_3d < draws_3d.len() && draws_3d[next_3d].before_quad <= first {
                    match &draws_3d[next_3d].draw {
                        Draw3d::Mesh(draw) => {
                            let mesh = &meshes[&draw.mesh.0];
                            pass.set_pipeline(
                                &mesh_pipelines[&MeshVariant::of(target.format, draw)],
                            );
                            let offset = (mesh_index as u64 * self.mesh_uniform_stride) as u32;
                            pass.set_bind_group(0, &mesh_uniforms.1, &[offset]);
                            let texture = draw.texture.unwrap_or(&self.white);
                            pass.set_bind_group(1, &mesh_bind_groups[&texture.id], &[]);
                            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                            pass.set_index_buffer(
                                mesh.indices.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            if let Some(reference) = draw.stencil_once {
                                pass.set_stencil_reference(u32::from(reference));
                            }
                            let range = match &draw.indices {
                                Some(range) => {
                                    range.start.min(mesh.index_count)
                                        ..range.end.min(mesh.index_count)
                                }
                                None => 0..mesh.index_count,
                            };
                            if !range.is_empty() {
                                pass.draw_indexed(range, 0, 0..1);
                            }
                            mesh_index += 1;
                        }
                        Draw3d::ShadowVolume(_) => {
                            let offset = (simple_index as u64 * self.simple_uniform_stride) as u32;
                            let range = volume_ranges[volume_index].clone();
                            if !range.is_empty() {
                                pass.set_bind_group(0, &simple_uniforms.1, &[offset]);
                                pass.set_vertex_buffer(0, volume_buffer.slice(..));
                                for kind in
                                    [SimpleKind::VolumeIncrement, SimpleKind::VolumeDecrement]
                                {
                                    pass.set_pipeline(&simple_pipelines[&(target.format, kind)]);
                                    pass.draw(range.clone(), 0..1);
                                }
                            }
                            simple_index += 1;
                            volume_index += 1;
                        }
                        Draw3d::DepthOnly(draw) => {
                            let mesh = &meshes[&draw.mesh.0];
                            let offset = (simple_index as u64 * self.simple_uniform_stride) as u32;
                            pass.set_pipeline(
                                &simple_pipelines[&(target.format, SimpleKind::DepthOnly)],
                            );
                            pass.set_bind_group(0, &simple_uniforms.1, &[offset]);
                            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                            pass.set_index_buffer(
                                mesh.indices.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                            simple_index += 1;
                        }
                        Draw3d::StencilFill(_) => {
                            let offset = (simple_index as u64 * self.simple_uniform_stride) as u32;
                            pass.set_pipeline(
                                &simple_pipelines[&(target.format, SimpleKind::Fill)],
                            );
                            pass.set_bind_group(0, &simple_uniforms.1, &[offset]);
                            pass.set_stencil_reference(1);
                            pass.draw(0..3, 0..1);
                            simple_index += 1;
                        }
                    }
                    quad_state = None;
                    next_3d += 1;
                }
                if first >= quads.len() {
                    break;
                }
                // A batch of quads with one texture and blend mode, ending
                // before the next 3D item.
                let limit = draws_3d
                    .get(next_3d)
                    .map_or(quads.len(), |m| m.before_quad.min(quads.len()));
                let texture = quads[first].texture;
                let batch_blend = blend_of(first);
                let mut last = first + 1;
                while last < limit
                    && quads[last].texture.id == texture.id
                    && blend_of(last) == batch_blend
                {
                    last += 1;
                }
                if quad_state.is_none() {
                    pass.set_bind_group(0, &self.globals_group, &[]);
                    pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                    pass.set_index_buffer(index_buffer.0.slice(..), wgpu::IndexFormat::Uint32);
                }
                if quad_state != Some(batch_blend) {
                    pass.set_pipeline(&pipelines[&(target.format, batch_blend)]);
                    quad_state = Some(batch_blend);
                }
                pass.set_bind_group(1, &bind_groups[&texture.id], &[]);
                pass.draw_indexed((first as u32 * 6)..(last as u32 * 6), 0, 0..1);
                first = last;
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Forgets the cached bindings of a texture that is no longer used.
    /// Optional: bindings are small, but a game that streams many images
    /// should release them.
    pub fn forget(&self, texture: &Texture) {
        self.bind_groups.borrow_mut().remove(&texture.id);
        self.mesh_bind_groups.borrow_mut().remove(&texture.id);
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

    fn ensure_mesh_uniform_capacity(&self, draws: usize) {
        let needed = draws.max(1) as u64 * self.mesh_uniform_stride;
        let mut uniforms = self.mesh_uniforms.borrow_mut();
        if uniforms.0.size() < needed {
            *uniforms = mesh_uniform_buffer(
                &self.gpu.device,
                &self.mesh_uniform_layout,
                self.mesh_uniform_stride,
                draws.next_power_of_two(),
            );
        }
    }

    fn ensure_simple_uniform_capacity(&self, draws: usize) {
        let needed = draws.max(1) as u64 * self.simple_uniform_stride;
        let mut uniforms = self.simple_uniforms.borrow_mut();
        if uniforms.0.size() < needed {
            *uniforms = uniform_buffer(
                &self.gpu.device,
                "shadow uniforms",
                &self.simple_uniform_layout,
                self.simple_uniform_stride,
                std::mem::size_of::<SimpleUniforms>() as u64,
                draws.next_power_of_two(),
            );
        }
    }

    fn ensure_volume_capacity(&self, vertices: usize) {
        let needed = (vertices.max(1) * std::mem::size_of::<[f32; 3]>()) as u64;
        let mut buffer = self.volume_vertices.borrow_mut();
        if buffer.size() < needed {
            *buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shadow volume vertices"),
                size: needed.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
    }

    /// Recreates the depth and stencil buffer when the target size
    /// changes.
    fn ensure_depth(&self, width: u32, height: u32) {
        let mut depth = self.depth.borrow_mut();
        if depth
            .as_ref()
            .is_some_and(|d| d.width == width && d.height == height)
        {
            return;
        }
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth stencil"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        *depth = Some(DepthBuffer {
            width,
            height,
            view,
        });
    }

    fn ensure_bind_group(&self, texture: &Texture) {
        let id = texture.id;
        if self.bind_groups.borrow().contains_key(&id) {
            return;
        }
        let group = self.texture_bind_group(texture, &self.sampler);
        self.bind_groups.borrow_mut().insert(id, group);
    }

    fn ensure_mesh_bind_group(&self, texture: &Texture) {
        let id = texture.id;
        if self.mesh_bind_groups.borrow().contains_key(&id) {
            return;
        }
        let group = self.texture_bind_group(texture, &self.mesh_sampler);
        self.mesh_bind_groups.borrow_mut().insert(id, group);
    }

    fn texture_bind_group(&self, texture: &Texture, sampler: &wgpu::Sampler) -> wgpu::BindGroup {
        self.gpu
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
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
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
                // `Setup2D`: no depth test, no depth writes; quads never
                // touch the depth buffer.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
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

    fn ensure_mesh_pipeline(&self, variant: MeshVariant) {
        if self.mesh_pipelines.borrow().contains_key(&variant) {
            return;
        }
        let MeshVariant {
            format,
            blend,
            cull,
            depth_write,
            stencil_once,
        } = variant;
        // `stencil_once`: draw where the stencil differs from the
        // reference and set it to the reference.
        let stencil = if stencil_once {
            let face = wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::NotEqual,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Replace,
            };
            wgpu::StencilState {
                front: face,
                back: face,
                read_mask: 0xff,
                write_mask: 0xff,
            }
        } else {
            wgpu::StencilState::default()
        };
        let pipeline = self
            .gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("meshes"),
                layout: Some(&self.mesh_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.mesh_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                    })],
                },
                // Direct3D: front faces are clockwise on screen;
                // `D3DCULL_CCW` drops the counter-clockwise (back) ones.
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: cull.then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                // `Setup3D`: depth test `<=` with writes (the floor
                // shadows turn the writes off).
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil,
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.mesh_shader,
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
        self.mesh_pipelines.borrow_mut().insert(variant, pipeline);
    }

    /// The position-only pipelines of the stencil shadows
    /// (`C3DShadowVolume::RenderToStencilBuffer`, `RenderToScene`,
    /// `CAdSceneGeometry::RenderShadowGeometry`).
    fn ensure_simple_pipeline(&self, format: wgpu::TextureFormat, kind: SimpleKind) {
        if self.simple_pipelines.borrow().contains_key(&(format, kind)) {
            return;
        }
        let keep = wgpu::StencilOperation::Keep;
        let count = |pass_op| wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Always,
            fail_op: keep,
            depth_fail_op: keep,
            pass_op,
        };
        let (face, cull, depth_compare, depth_write, color_writes, blend, entry) = match kind {
            // The clockwise faces (`D3DCULL_CCW` keeps them) count up on
            // depth pass, the others count down, both wrapping
            // (`D3DSTENCILOP_INCR`/`DECR` wrap): the quads of
            // `C3DShadowVolume` face inward, so the far side counts up
            // and the near side down, and a pixel of a surface inside
            // the volume ends at 255, which `1 <= stencil` accepts.
            SimpleKind::VolumeIncrement => (
                count(wgpu::StencilOperation::IncrementWrap),
                Some(wgpu::Face::Back),
                wgpu::CompareFunction::LessEqual,
                false,
                wgpu::ColorWrites::empty(),
                None,
                "vs_main",
            ),
            SimpleKind::VolumeDecrement => (
                count(wgpu::StencilOperation::DecrementWrap),
                Some(wgpu::Face::Front),
                wgpu::CompareFunction::LessEqual,
                false,
                wgpu::ColorWrites::empty(),
                None,
                "vs_main",
            ),
            // The receivers: depth only, back faces culled as the original
            // sets `D3DCULL_CCW` for them.
            SimpleKind::DepthOnly => (
                wgpu::StencilFaceState::IGNORE,
                Some(wgpu::Face::Back),
                wgpu::CompareFunction::LessEqual,
                true,
                wgpu::ColorWrites::empty(),
                None,
                "vs_main",
            ),
            // Where `1 <= stencil`, blended, no depth test; the stencil is
            // zeroed where the fill passes.
            SimpleKind::Fill => (
                wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::LessEqual,
                    fail_op: keep,
                    depth_fail_op: keep,
                    pass_op: wgpu::StencilOperation::Zero,
                },
                None,
                wgpu::CompareFunction::Always,
                false,
                wgpu::ColorWrites::ALL,
                Some(wgpu::BlendState::ALPHA_BLENDING),
                "vs_fill",
            ),
        };
        // A shadow volume's `[f32; 3]` vertices, or the position at the
        // start of a `MeshVertex`.
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: if kind == SimpleKind::DepthOnly {
                std::mem::size_of::<MeshVertex>() as u64
            } else {
                std::mem::size_of::<[f32; 3]>() as u64
            },
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        })];
        let pipeline = self
            .gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow"),
                layout: Some(&self.simple_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.simple_shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers: if kind == SimpleKind::Fill {
                        &[]
                    } else {
                        &buffers
                    },
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: cull,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(depth_compare),
                    stencil: wgpu::StencilState {
                        front: face,
                        back: face,
                        read_mask: 0xff,
                        write_mask: 0xff,
                    },
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.simple_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: color_writes,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        self.simple_pipelines
            .borrow_mut()
            .insert((format, kind), pipeline);
    }
}

/// The uniform buffer of `slots` mesh draws and its bind group.
fn mesh_uniform_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    stride: u64,
    slots: usize,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    uniform_buffer(
        device,
        "mesh uniforms",
        layout,
        stride,
        std::mem::size_of::<MeshUniforms>() as u64,
        slots,
    )
}

/// A uniform buffer of `slots` aligned slots of `size` bytes, bound
/// with a dynamic offset.
fn uniform_buffer(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    stride: u64,
    size: u64,
    slots: usize,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: stride * slots.max(1) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buffer,
                offset: 0,
                size: wgpu::BufferSize::new(size),
            }),
        }],
    });
    (buffer, group)
}

/// The uniforms of a position-only draw: the world matrix followed by
/// the view-projection, and the fill colour.
fn simple_uniforms(view_projection: &Matrix, world: &Matrix, color: [f32; 4]) -> SimpleUniforms {
    SimpleUniforms {
        mvp: multiply(view_projection, world),
        color,
    }
}

/// The uniforms of a mesh draw as the shader reads them.
fn mesh_uniforms(draw: &MeshDraw<'_>) -> MeshUniforms {
    let mut lights = [GpuLight::default(); MAX_LIGHTS];
    for (slot, light) in lights.iter_mut().zip(draw.lights.as_slice()) {
        *slot = gpu_light(light);
    }
    let material = draw.material;
    let [er, eg, eb] = material.emissive;
    let [ar, ag, ab] = draw.ambient;
    let (fog_color, fog_params) = match draw.fog {
        Some(fog) => {
            let [r, g, b] = fog.color;
            (
                [r, g, b, 1.0],
                [fog.start, fog.end, MESH_ALPHA_REFERENCE, 0.0],
            )
        }
        None => ([0.0; 4], [0.0, 0.0, MESH_ALPHA_REFERENCE, 0.0]),
    };
    MeshUniforms {
        world: draw.world,
        view_projection: draw.view_projection,
        diffuse: material.diffuse,
        emissive: [er, eg, eb, if draw.texture.is_some() { 1.0 } else { 0.0 }],
        ambient: [ar, ag, ab, draw.lights.len() as f32],
        fog_color,
        fog_params,
        lights,
    }
}

fn gpu_light(light: &Light) -> GpuLight {
    let with_w = |v: [f32; 3], w: f32| [v[0], v[1], v[2], w];
    match *light {
        Light::Directional { direction, color } => GpuLight {
            position: [0.0, 0.0, 0.0, 1.0],
            direction: with_w(direction, 0.0),
            color: with_w(color, 0.0),
            cone: [0.0; 4],
        },
        Light::Point {
            position,
            range,
            color,
        } => GpuLight {
            position: with_w(position, 2.0),
            direction: [0.0, 0.0, 1.0, range],
            color: with_w(color, 0.0),
            cone: [0.0; 4],
        },
        Light::Spot {
            position,
            direction,
            range,
            inner_angle,
            outer_angle,
            falloff,
            color,
        } => GpuLight {
            position: with_w(position, 3.0),
            direction: with_w(direction, range),
            color: with_w(color, 0.0),
            cone: [
                (inner_angle / 2.0).cos(),
                (outer_angle / 2.0).cos(),
                falloff,
                0.0,
            ],
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_uniforms_match_the_shader_layout() {
        // Two matrices, five vec4s and eight lights of four vec4s.
        assert_eq!(std::mem::size_of::<GpuLight>(), 64);
        assert_eq!(std::mem::size_of::<MeshUniforms>(), 128 + 80 + 8 * 64);
    }

    #[test]
    fn spot_cones_are_half_angle_cosines() {
        let light = gpu_light(&Light::Spot {
            position: [1.0, 2.0, 3.0],
            direction: [0.0, -1.0, 0.0],
            range: 10.0,
            inner_angle: 0.5,
            outer_angle: 1.0,
            falloff: 1.0,
            color: [1.0, 0.5, 0.25],
        });
        assert_eq!(light.position, [1.0, 2.0, 3.0, 3.0]);
        assert_eq!(light.direction, [0.0, -1.0, 0.0, 10.0]);
        assert!((light.cone[0] - 0.25f32.cos()).abs() < 1e-6);
        assert!((light.cone[1] - 0.5f32.cos()).abs() < 1e-6);
        assert_eq!(light.cone[2], 1.0);
    }
}
