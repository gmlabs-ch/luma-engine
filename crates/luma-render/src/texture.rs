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

    /// Replaces the texels with `width * height * 4` bytes of RGBA of the
    /// same size (a video frame).
    ///
    /// # Panics
    ///
    /// Panics when the buffer does not match the texture's size.
    pub fn write(&self, gpu: &Gpu, rgba: &[u8]) {
        assert_eq!(
            rgba.len(),
            (self.width as usize) * (self.height as usize) * 4,
            "RGBA buffer size does not match {}x{}",
            self.width,
            self.height
        );
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Uploads `width * height * 4` bytes of RGBA.
    ///
    /// # Panics
    ///
    /// Panics if the buffer size does not match, or on a zero dimension.
    pub fn from_rgba(gpu: &Gpu, label: &str, width: u32, height: u32, rgba: &[u8]) -> Texture {
        Texture::from_levels(gpu, label, width, height, rgba, false)
    }

    /// Uploads a decoded image with its whole mipmap chain, for meshes
    /// sampled trilinearly. Each level halves the previous one with a box
    /// filter whose colours are weighted by alpha (see
    /// [`mip_level`]).
    pub fn from_image_mipmapped(gpu: &Gpu, label: &str, image: &Image) -> Texture {
        Texture::from_levels(gpu, label, image.width, image.height, &image.pixels, true)
    }

    fn from_levels(
        gpu: &Gpu,
        label: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        mipmaps: bool,
    ) -> Texture {
        assert!(width > 0 && height > 0, "{label}: empty texture");
        assert_eq!(
            rgba.len(),
            (width as usize) * (height as usize) * 4,
            "{label}: RGBA buffer size does not match {width}x{height}"
        );
        let mip_level_count = if mipmaps {
            32 - width.max(height).leading_zeros()
        } else {
            1
        };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut level = (width, height, std::borrow::Cow::Borrowed(rgba));
        for mip in 0..mip_level_count {
            if mip > 0 {
                let (w, h, pixels) = mip_level(level.0, level.1, &level.2);
                level = (w, h, std::borrow::Cow::Owned(pixels));
            }
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &level.2,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(level.0 * 4),
                    rows_per_image: Some(level.1),
                },
                wgpu::Extent3d {
                    width: level.0,
                    height: level.1,
                    depth_or_array_layers: 1,
                },
            );
        }
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

/// The next mipmap level of an RGBA image: half the size (at least 1,
/// rounded down), each texel the box average of the 2x2 texels it
/// covers (clamped at an odd edge). Colours are weighted by alpha, so a
/// transparent texel (a colour-keyed one, say) contributes no colour and
/// the opaque neighbours keep theirs; the alpha is the plain average.
pub fn mip_level(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    let texel = |x: u32, y: u32| {
        let x = x.min(width - 1);
        let y = y.min(height - 1);
        let i = ((y * width + x) * 4) as usize;
        &rgba[i..i + 4]
    };
    for y in 0..h {
        for x in 0..w {
            let samples = [
                texel(x * 2, y * 2),
                texel(x * 2 + 1, y * 2),
                texel(x * 2, y * 2 + 1),
                texel(x * 2 + 1, y * 2 + 1),
            ];
            let alpha_sum: u32 = samples.iter().map(|s| u32::from(s[3])).sum();
            if alpha_sum == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            for channel in 0..3 {
                let weighted: u32 = samples
                    .iter()
                    .map(|s| u32::from(s[channel]) * u32::from(s[3]))
                    .sum();
                out.push(((weighted + alpha_sum / 2) / alpha_sum) as u8);
            }
            out.push(((alpha_sum + 2) / 4) as u8);
        }
    }
    (w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_level_averages_opaque_texels_and_ignores_transparent_ones() {
        // 2x2: red, red / transparent magenta, blue.
        let rgba = [
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 255, 0, 0, 0, 255, 255,
        ];
        let (w, h, out) = mip_level(2, 2, &rgba);
        assert_eq!((w, h), (1, 1));
        // Three opaque texels: two red, one blue; alpha 3/4.
        assert_eq!(out, vec![170, 0, 85, 191]);
    }

    #[test]
    fn mip_level_clamps_odd_sizes() {
        let rgba = [10, 20, 30, 255, 50, 60, 70, 255, 90, 100, 110, 255];
        let (w, h, out) = mip_level(3, 1, &rgba);
        assert_eq!((w, h), (1, 1));
        // The 2x2 block covers texels 0 and 1 twice each (rows clamp).
        assert_eq!(out, vec![30, 40, 50, 255]);
    }
}
