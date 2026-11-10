//! Decoding of game images into RGBA pixels, with the engine's colour key
//! rules.
//!
//! The rules are documented in `docs/formats/sprite.md` and were
//! established from the MIT-licensed WME Lite sources (`BSurfaceSDL.cpp`,
//! Copyright (c) 2011 Jan Nedoma). See `NOTICE` at the root of the
//! repository.

use image::{ColorType, ImageEncoder, ImageFormat, ImageReader};
use std::io::Cursor;

use crate::error::{Error, Result};

/// An opaque colour; the default is black.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

/// A colour with opacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rgba {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha, 255 opaque.
    pub a: u8,
}

impl Rgba {
    /// Opaque white, the tint that leaves an image unchanged.
    pub const OPAQUE_WHITE: Rgba = Rgba {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
}

/// How a sprite is blended with what is under it (`TSpriteBlendMode`,
/// the `BlendMode` property of objects): the values are the engine's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum BlendMode {
    /// Alpha blending: `src * a + dst * (1 - a)`.
    #[default]
    Normal = 0,
    /// Added to what is under: `src * a + dst`.
    Additive = 1,
    /// Darkens what is under by the sprite's colour: `dst * (1 - src)`.
    Subtractive = 2,
}

impl BlendMode {
    /// The mode of the engine's integer; an unknown value is `Normal`,
    /// as in `CBObject::ScSetProperty`.
    pub fn from_int(value: i32) -> BlendMode {
        match value {
            1 => BlendMode::Additive,
            2 => BlendMode::Subtractive,
            _ => BlendMode::Normal,
        }
    }

    /// The engine's integer for the mode.
    pub fn to_int(self) -> i32 {
        self as i32
    }
}

/// The engine's default colour key: magenta.
pub const DEFAULT_COLOR_KEY: Rgb = Rgb {
    r: 255,
    g: 0,
    b: 255,
};

/// A decoded image: 8-bit RGBA, rows top to bottom, not premultiplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    /// Whether transparency came from the file's alpha channel (`true`)
    /// or from the colour key (`false`).
    pub has_alpha_channel: bool,
}

impl Image {
    /// Encodes the image as a PNG file (for screenshots and tools).
    pub fn to_png(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(
                &self.pixels,
                self.width,
                self.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|err| Error::Image {
                reason: err.to_string(),
            })?;
        Ok(out)
    }

    /// Encodes the image as a 24-bit BMP file, the original engine's
    /// screenshot format (`CBImage::SaveBMPFile`); the alpha channel is
    /// dropped.
    pub fn to_bmp(&self) -> Result<Vec<u8>> {
        let rgb: Vec<u8> = self
            .pixels
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let mut out = Vec::new();
        image::codecs::bmp::BmpEncoder::new(&mut out)
            .write_image(
                &rgb,
                self.width,
                self.height,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|err| Error::Image {
                reason: err.to_string(),
            })?;
        Ok(out)
    }

    /// The image scaled to a size, with a bilinear filter (for
    /// thumbnails and scaled screenshots).
    pub fn resized(&self, width: u32, height: u32) -> Image {
        let (width, height) = (width.max(1), height.max(1));
        let Some(source) = image::RgbaImage::from_raw(self.width, self.height, self.pixels.clone())
        else {
            return Image {
                width,
                height,
                pixels: vec![0; (width * height * 4) as usize],
                has_alpha_channel: self.has_alpha_channel,
            };
        };
        let scaled = image::imageops::resize(
            &source,
            width,
            height,
            image::imageops::FilterType::Triangle,
        );
        Image {
            width,
            height,
            pixels: scaled.into_raw(),
            has_alpha_channel: self.has_alpha_channel,
        }
    }

    /// The pixel at `(x, y)`, or `None` outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        let p = &self.pixels[i..i + 4];
        Some(Rgba {
            r: p[0],
            g: p[1],
            b: p[2],
            a: p[3],
        })
    }
}

/// Decodes an image file (PNG, BMP, JPEG or TGA) the way the engine does:
/// an image with an alpha channel keeps it, except BMP; any other image is
/// opaque with pixels of the colour key made transparent.
pub fn decode(bytes: &[u8], color_key: Rgb) -> Result<Image> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| Error::Image {
            reason: err.to_string(),
        })?;
    let format = reader.format();
    let decoded = reader.decode().map_err(|err| Error::Image {
        reason: err.to_string(),
    })?;
    let keep_alpha = decoded.color().has_alpha() && format != Some(ImageFormat::Bmp);
    let (width, height) = (decoded.width(), decoded.height());
    let pixels = if keep_alpha {
        decoded.into_rgba8().into_raw()
    } else {
        let mut rgba = decoded.into_rgb8().into_raw().chunks_exact(3).fold(
            Vec::with_capacity((width * height * 4) as usize),
            |mut out, p| {
                out.extend_from_slice(p);
                out.push(255);
                out
            },
        );
        for p in rgba.chunks_exact_mut(4) {
            if p[0] == color_key.r && p[1] == color_key.g && p[2] == color_key.b {
                p[3] = 0;
            }
        }
        clear_keyed_color(&mut rgba, width as usize, height as usize);
        rgba
    };
    Ok(Image {
        width,
        height,
        pixels,
        has_alpha_channel: keep_alpha,
    })
}

/// Replaces the colour of the transparent pixels of a colour-keyed image:
/// the average of the opaque pixels around each one, or black when it has
/// none. Bilinear filtering blends a sprite's edge with its transparent
/// neighbours, so the key (magenta, usually) would show as a fringe
/// around every scaled sprite; with the neighbours' colour the edge only
/// fades out.
fn clear_keyed_color(rgba: &mut [u8], width: usize, height: usize) {
    let source = rgba.to_vec();
    for y in 0..height {
        for x in 0..width {
            let at = (y * width + x) * 4;
            if source[at + 3] != 0 {
                continue;
            }
            let (mut sum, mut count) = ([0u32; 3], 0u32);
            for ny in y.saturating_sub(1)..(y + 2).min(height) {
                for nx in x.saturating_sub(1)..(x + 2).min(width) {
                    let n = (ny * width + nx) * 4;
                    if source[n + 3] != 0 {
                        for c in 0..3 {
                            sum[c] += u32::from(source[n + c]);
                        }
                        count += 1;
                    }
                }
            }
            for c in 0..3 {
                rgba[at + c] = sum[c].checked_div(count).unwrap_or(0) as u8;
            }
        }
    }
}

/// Whether the decoder would keep the alpha channel of an image with this
/// colour type and format. Exposed for tests and tooling.
pub fn keeps_alpha(color: ColorType, format: Option<ImageFormat>) -> bool {
    color.has_alpha() && format != Some(ImageFormat::Bmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb as IRgb, Rgba as IRgba};

    fn png_rgb() -> Vec<u8> {
        let img = ImageBuffer::from_fn(2, 2, |x, y| match (x, y) {
            (0, 0) => IRgb([255u8, 0, 255]),
            (1, 0) => IRgb([10, 20, 30]),
            (0, 1) => IRgb([166, 202, 240]),
            _ => IRgb([0, 0, 0]),
        });
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn png_rgba() -> Vec<u8> {
        let img = ImageBuffer::from_fn(2, 1, |x, _| match x {
            0 => IRgba([255u8, 0, 255, 255]),
            _ => IRgba([10, 20, 30, 77]),
        });
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn bmp_rgba() -> Vec<u8> {
        let img = ImageBuffer::from_fn(2, 1, |x, _| match x {
            0 => IRgba([255u8, 0, 255, 255]),
            _ => IRgba([10, 20, 30, 77]),
        });
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Bmp).unwrap();
        out.into_inner()
    }

    #[test]
    fn color_key_applies_to_images_without_alpha() {
        let img = decode(&png_rgb(), DEFAULT_COLOR_KEY).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert!(!img.has_alpha_channel);
        // The transparent pixel takes the average colour of its opaque
        // neighbours instead of the key: (10+166+0)/3, (20+202+0)/3,
        // (30+240+0)/3.
        assert_eq!(
            img.pixel(0, 0),
            Some(Rgba {
                r: 58,
                g: 74,
                b: 90,
                a: 0
            }),
            "magenta is transparent and gone"
        );
        assert_eq!(
            img.pixel(1, 0),
            Some(Rgba {
                r: 10,
                g: 20,
                b: 30,
                a: 255
            })
        );
        assert_eq!(img.pixel(0, 1).unwrap().a, 255);
        assert_eq!(img.pixel(2, 0), None);

        let custom = decode(
            &png_rgb(),
            Rgb {
                r: 166,
                g: 202,
                b: 240,
            },
        )
        .unwrap();
        assert_eq!(
            custom.pixel(0, 0).unwrap().a,
            255,
            "magenta stays with a custom key"
        );
        assert_eq!(custom.pixel(0, 1).unwrap().a, 0);
    }

    #[test]
    fn keyed_pixels_without_opaque_neighbours_turn_black() {
        let img = ImageBuffer::from_fn(2, 2, |_, _| IRgb([255u8, 0, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        let img = decode(&out.into_inner(), DEFAULT_COLOR_KEY).unwrap();
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(
                img.pixel(x, y),
                Some(Rgba {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0
                }),
                "pixel {x},{y}"
            );
        }
    }

    #[test]
    fn keyed_clearing_leaves_opaque_pixels_and_rgba_images_alone() {
        let img = decode(&png_rgb(), DEFAULT_COLOR_KEY).unwrap();
        let rgb = |r, g, b| Rgba { r, g, b, a: 255 };
        assert_eq!(img.pixel(1, 0), Some(rgb(10, 20, 30)));
        assert_eq!(img.pixel(0, 1), Some(rgb(166, 202, 240)));
        assert_eq!(img.pixel(1, 1), Some(rgb(0, 0, 0)));

        // A real alpha channel keeps every colour, the key included.
        let img = decode(&png_rgba(), DEFAULT_COLOR_KEY).unwrap();
        assert_eq!(img.pixel(0, 0), Some(rgb(255, 0, 255)));
        assert_eq!(
            img.pixel(1, 0),
            Some(Rgba {
                r: 10,
                g: 20,
                b: 30,
                a: 77
            })
        );
    }

    #[test]
    fn alpha_channel_wins_over_the_color_key() {
        let img = decode(&png_rgba(), DEFAULT_COLOR_KEY).unwrap();
        assert!(img.has_alpha_channel);
        assert_eq!(img.pixel(0, 0).unwrap().a, 255, "magenta is kept");
        assert_eq!(img.pixel(1, 0).unwrap().a, 77);
    }

    #[test]
    fn bmp_alpha_is_ignored() {
        let img = decode(&bmp_rgba(), DEFAULT_COLOR_KEY).unwrap();
        assert!(!img.has_alpha_channel);
        assert_eq!(img.pixel(0, 0).unwrap().a, 0, "magenta is keyed out");
        assert_eq!(img.pixel(1, 0).unwrap().a, 255, "file alpha is dropped");
        assert!(keeps_alpha(ColorType::Rgba8, Some(ImageFormat::Png)));
        assert!(!keeps_alpha(ColorType::Rgba8, Some(ImageFormat::Bmp)));
        assert!(!keeps_alpha(ColorType::Rgb8, Some(ImageFormat::Png)));
    }

    #[test]
    fn encodes_png_round_trip() {
        let img = decode(&png_rgba(), DEFAULT_COLOR_KEY).unwrap();
        let png = img.to_png().unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let back = decode(&png, DEFAULT_COLOR_KEY).unwrap();
        assert_eq!(back, img);
    }

    #[test]
    fn rejects_garbage() {
        let err = decode(b"not an image", DEFAULT_COLOR_KEY).unwrap_err();
        assert!(matches!(err, Error::Image { .. }), "{err}");
    }
}
