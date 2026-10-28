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

/// An opaque colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        rgba
    };
    Ok(Image {
        width,
        height,
        pixels,
        has_alpha_channel: keep_alpha,
    })
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
        assert_eq!(img.pixel(0, 0).unwrap().a, 0, "magenta is transparent");
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
