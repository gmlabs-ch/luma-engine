//! The draw list: what to draw this frame, in order.

use luma_formats::Rgba;

use crate::texture::Texture;

/// An axis-aligned rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle from its position and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
}

/// One textured quad.
#[derive(Debug, Clone, Copy)]
pub struct Quad<'a> {
    /// The texture.
    pub texture: &'a Texture,
    /// Where to draw, in logical pixels, after the current offset.
    pub dst: Rect,
    /// Part of the texture to draw, in texels; `None` for the whole
    /// texture.
    pub src: Option<Rect>,
    /// Colour multiplied with the texels; opaque white leaves them
    /// unchanged.
    pub tint: Rgba,
    /// Flip horizontally.
    pub flip_x: bool,
    /// Flip vertically.
    pub flip_y: bool,
}

/// A list of quads to draw back to front.
///
/// Coordinates are *logical* pixels: the list has a logical size (the
/// game's resolution) that the renderer maps onto the whole target, or
/// onto the largest centred rectangle of the same aspect ratio when
/// `letterbox` is set.
#[derive(Debug)]
pub struct DrawList<'a> {
    /// Logical width.
    pub width: f32,
    /// Logical height.
    pub height: f32,
    /// Keep the aspect ratio and pad with the clear colour.
    pub letterbox: bool,
    /// Added to every quad position (scroll offset of the current layer).
    pub offset: (f32, f32),
    quads: Vec<Quad<'a>>,
}

impl<'a> DrawList<'a> {
    /// An empty list with the given logical size.
    pub fn new(width: f32, height: f32) -> DrawList<'a> {
        DrawList {
            width,
            height,
            letterbox: false,
            offset: (0.0, 0.0),
            quads: Vec::new(),
        }
    }

    /// Removes every quad, keeping the size and offset.
    pub fn clear(&mut self) {
        self.quads.clear();
    }

    /// The quads, in drawing order.
    pub fn quads(&self) -> &[Quad<'a>] {
        &self.quads
    }

    /// Sets the offset added to the following quads.
    pub fn set_offset(&mut self, x: f32, y: f32) {
        self.offset = (x, y);
    }

    /// Appends a quad, applying the current offset to its position.
    pub fn push(&mut self, mut quad: Quad<'a>) {
        quad.dst.x += self.offset.0;
        quad.dst.y += self.offset.1;
        self.quads.push(quad);
    }

    /// Draws a whole texture at `(x, y)` at its own size.
    pub fn image(&mut self, texture: &'a Texture, x: f32, y: f32) {
        self.push(Quad {
            texture,
            dst: Rect::new(x, y, texture.width as f32, texture.height as f32),
            src: None,
            tint: Rgba::OPAQUE_WHITE,
            flip_x: false,
            flip_y: false,
        });
    }

    /// Draws a part of a texture into a rectangle.
    pub fn sprite(&mut self, texture: &'a Texture, dst: Rect, src: Option<Rect>, tint: Rgba) {
        self.push(Quad {
            texture,
            dst,
            src,
            tint,
            flip_x: false,
            flip_y: false,
        });
    }

    /// Fills a rectangle with a colour, using a white texture.
    pub fn fill(&mut self, white: &'a Texture, dst: Rect, color: Rgba) {
        self.sprite(white, dst, None, color);
    }
}
