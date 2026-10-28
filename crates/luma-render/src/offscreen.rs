//! Rendering into a texture and reading the pixels back.

use luma_formats::Image;

use crate::error::{Error, Result};
use crate::gpu::Gpu;
use crate::renderer::Target;
use crate::texture::TEXTURE_FORMAT;

/// A texture to render into, whose pixels can be read back: for tests,
/// screenshots and tools.
#[derive(Debug)]
pub struct Offscreen {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Offscreen {
    /// Creates a target of the given size in pixels.
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Offscreen {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Offscreen {
            width: width.max(1),
            height: height.max(1),
            texture,
            view,
        }
    }

    /// The target to pass to [`Renderer::render`](crate::Renderer::render).
    pub fn target(&self) -> Target<'_> {
        Target {
            view: &self.view,
            format: TEXTURE_FORMAT,
            width: self.width,
            height: self.height,
        }
    }

    /// Copies the pixels back to the CPU, waiting for the GPU.
    pub fn read(&self, gpu: &Gpu) -> Result<Image> {
        const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded = self.width * 4;
        let padded = unpadded.div_ceil(ALIGN) * ALIGN;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(self.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        gpu.wait()?;
        receiver
            .recv()
            .map_err(|_| Error::Render {
                reason: "readback callback never ran".into(),
            })?
            .map_err(|err| Error::Render {
                reason: err.to_string(),
            })?;
        let mut pixels = Vec::with_capacity((unpadded * self.height) as usize);
        {
            let data = slice.get_mapped_range().map_err(|err| Error::Render {
                reason: err.to_string(),
            })?;
            for row in data.chunks_exact(padded as usize) {
                pixels.extend_from_slice(&row[..unpadded as usize]);
            }
        }
        buffer.unmap();
        Ok(Image {
            width: self.width,
            height: self.height,
            pixels,
            has_alpha_channel: true,
        })
    }
}
