//! Textures: decoded images uploaded to the GPU.

use std::sync::atomic::{AtomicU64, Ordering};

use luma_formats::Image;

use crate::gpu::Gpu;

/// The texel format of every texture: 8-bit RGBA, not premultiplied,
/// sampled without sRGB conversion so that blending happens in gamma
/// space like the original engine.
pub const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// An image on the GPU.
#[derive(Debug)]
pub struct Texture {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    pub(crate) id: u64,
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Texture {
    /// Uploads a decoded image.
    pub fn from_image(gpu: &Gpu, label: &str, image: &Image) -> Texture {
        Texture::from_rgba(gpu, label, image.width, image.height, &image.pixels)
    }

    /// Uploads `width * height * 4` bytes of RGBA.
    ///
    /// # Panics
    ///
    /// Panics if the buffer size does not match, or on a zero dimension.
    pub fn from_rgba(gpu: &Gpu, label: &str, width: u32, height: u32, rgba: &[u8]) -> Texture {
        assert!(width > 0 && height > 0, "{label}: empty texture");
        assert_eq!(
            rgba.len(),
            (width as usize) * (height as usize) * 4,
            "{label}: RGBA buffer size does not match {width}x{height}"
        );
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Texture {
            width,
            height,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            texture,
            view,
        }
    }

    /// A 1x1 opaque white texture, for untextured rectangles.
    pub fn white(gpu: &Gpu) -> Texture {
        Texture::from_rgba(gpu, "white", 1, 1, &[255, 255, 255, 255])
    }

    /// A number unique to this texture for the life of the process.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The underlying wgpu texture.
    pub fn raw(&self) -> &wgpu::Texture {
        &self.texture
    }
}
