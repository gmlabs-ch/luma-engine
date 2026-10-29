//! Fonts (`.font`): bitmap fonts made of character tiles, and TrueType
//! fonts with coloured layers.
//!
//! The format is documented in `docs/formats/font.md` and was
//! established from the MIT-licensed WME Lite sources (`BFont.cpp`,
//! `BFontBitmap.cpp`, `BFontTT.cpp`, Copyright (c) 2011 Jan Nedoma). See
//! `NOTICE` at the root of the repository.

use crate::def::{Document, Node, Nodes};
use crate::error::{Error, Result};
use crate::image::{Image, Rgb, Rgba};

/// How text is placed in its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAlign {
    /// Code 0.
    #[default]
    Left,
    /// Code 1.
    Right,
    /// Code 2.
    Center,
}

impl TextAlign {
    /// The alignment of a script code, clamped to the valid ones.
    pub fn from_code(code: i32) -> TextAlign {
        match code {
            i32::MIN..=0 => TextAlign::Left,
            1 => TextAlign::Right,
            _ => TextAlign::Center,
        }
    }

    /// The script code.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Where a line of `line_width` starts in a box of `width` at `x`.
    pub fn start_x(self, x: i32, width: i32, line_width: i32) -> i32 {
        match self {
            TextAlign::Left => x,
            TextAlign::Right => x + width - line_width,
            TextAlign::Center => x + (width - line_width) / 2,
        }
    }
}

/// A font file: one of the two kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum FontFile {
    /// A `FONT` block.
    Bitmap(BitmapFontDef),
    /// A `TTFONT` block.
    TrueType(TrueTypeFontDef),
}

impl FontFile {
    /// Parses a `.font` file.
    pub fn parse(bytes: &[u8]) -> Result<FontFile> {
        let doc = Document::parse(bytes)?;
        if let Some(root) = doc.get("TTFONT") {
            return Ok(FontFile::TrueType(TrueTypeFontDef::from_node(root)));
        }
        let root = doc.get("FONT").ok_or_else(|| Error::Invalid {
            reason: "'FONT' keyword expected".into(),
        })?;
        Ok(FontFile::Bitmap(BitmapFontDef::from_node(root)?))
    }
}

/// A bitmap font definition (`FONT`).
#[derive(Debug, Clone, PartialEq)]
pub struct BitmapFontDef {
    /// The image holding the tiles.
    pub image: Option<String>,
    /// A sprite holding the tiles instead.
    pub sprite: Option<String>,
    /// Colour key of the image (`TRANSPARENT`), when given.
    pub transparent: Option<Rgb>,
    /// Tiles per row.
    pub columns: i32,
    /// Width of a tile.
    pub tile_width: i32,
    /// Height of a tile, the line height.
    pub tile_height: i32,
    /// Measure the widths from the image.
    pub auto_width: bool,
    /// Explicit widths, from character 0 on.
    pub widths: Vec<u8>,
    /// Width of the characters not listed.
    pub default_width: i32,
    /// With `auto_width`, the width of the space (`0`: computed).
    pub space_width: i32,
    /// With `auto_width`, added to every width.
    pub expand_width: i32,
    /// Character `c` is in tile `c - 1`.
    pub fontext_fix: bool,
    /// Draw whole tiles.
    pub paint_whole_cell: bool,
    /// With `sprite`, the frame measured.
    pub widths_frame: i32,
}

impl Default for BitmapFontDef {
    fn default() -> Self {
        BitmapFontDef {
            image: None,
            sprite: None,
            transparent: None,
            columns: 0,
            tile_width: 0,
            tile_height: 0,
            auto_width: false,
            widths: Vec::new(),
            default_width: 8,
            space_width: 0,
            expand_width: 0,
            fontext_fix: false,
            paint_whole_cell: false,
            widths_frame: 0,
        }
    }
}

impl BitmapFontDef {
    fn from_node(root: &Node) -> Result<BitmapFontDef> {
        let mut def = BitmapFontDef::default();
        for child in root.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "IMAGE" => def.image = child.as_str().map(str::to_string),
                "SPRITE" => def.sprite = child.as_str().map(str::to_string),
                "TRANSPARENT" => {
                    let v = child.as_ints().unwrap_or_default();
                    def.transparent = Some(Rgb {
                        r: channel(&v, 0),
                        g: channel(&v, 1),
                        b: channel(&v, 2),
                    });
                }
                "WIDTHS" => {
                    let v = child.as_ints().unwrap_or_default();
                    for w in v {
                        if def.widths.len() >= 256 {
                            break;
                        }
                        def.widths.push(w.clamp(0, 255) as u8);
                    }
                }
                "DEFAULT_WIDTH" => def.default_width = integer(child),
                "WIDTHS_FRAME" => def.widths_frame = integer(child),
                "COLUMNS" => def.columns = integer(child),
                "TILE_WIDTH" => def.tile_width = integer(child),
                "TILE_HEIGHT" => def.tile_height = integer(child),
                "AUTO_WIDTH" => def.auto_width = boolean(child),
                "FONTEXT_FIX" => def.fontext_fix = boolean(child),
                "PAINT_WHOLE_CELL" => def.paint_whole_cell = boolean(child),
                "SPACE_WIDTH" => def.space_width = integer(child),
                "EXPAND_WIDTH" => def.expand_width = integer(child),
                _ => {}
            }
        }
        if def.image.is_none() && def.sprite.is_none()
            || def.columns == 0
            || def.tile_width == 0
            || def.tile_height == 0
        {
            return Err(Error::Invalid {
                reason: "Incomplete font definition".into(),
            });
        }
        Ok(def)
    }

    /// The widths of the 256 characters: measured from the image when
    /// `auto_width` (`CBFontBitmap::GetWidths`), else the explicit ones
    /// followed by `default_width`.
    pub fn widths(&self, image: Option<&Image>) -> [u8; 256] {
        let mut widths = [0u8; 256];
        if self.auto_width {
            if let Some(image) = image {
                for (i, width) in widths.iter_mut().enumerate() {
                    *width = self.measure_tile(image, i as i32) as u8;
                }
            }
            if self.expand_width != 0 {
                for width in &mut widths {
                    *width = (i32::from(*width) + self.expand_width).clamp(0, 255) as u8;
                }
            }
            let space = if self.fontext_fix { b' ' - 1 } else { b' ' } as usize;
            if self.space_width != 0 {
                widths[space] = self.space_width.clamp(0, 255) as u8;
            } else if i32::from(widths[space]) == self.expand_width || widths[space] == 0 {
                widths[space] = ((u16::from(widths[usize::from(b'm')])
                    + u16::from(widths[usize::from(b'i')]))
                    / 2) as u8;
            }
        } else {
            for (i, width) in widths.iter_mut().enumerate() {
                *width = match self.widths.get(i) {
                    Some(w) => *w,
                    None => self.default_width.clamp(0, 255) as u8,
                };
            }
        }
        widths
    }

    /// The measured width of tile `index`: one more than the right-most
    /// column with a pixel more than half opaque.
    fn measure_tile(&self, image: &Image, index: i32) -> i32 {
        let x0 = (index % self.columns) * self.tile_width;
        let y0 = (index / self.columns) * self.tile_height;
        let mut min_col = -1;
        for row in 0..self.tile_height {
            let mut col = self.tile_width - 1;
            while col > min_col {
                let (x, y) = (x0 + col, y0 + row);
                let opaque =
                    x >= 0 && y >= 0 && image.pixel(x as u32, y as u32).is_some_and(|p| p.a > 128);
                if opaque {
                    min_col = min_col.max(col);
                    break;
                }
                col -= 1;
            }
            if min_col == self.tile_width - 1 {
                break;
            }
        }
        min_col + 1
    }

    /// The tile of a character, in the image: `(x, y, width, height)`
    /// (`CBFontBitmap::DrawChar`).
    pub fn tile(&self, widths: &[u8; 256], c: u8) -> (i32, i32, i32, i32) {
        let c = if self.fontext_fix {
            c.wrapping_sub(1)
        } else {
            c
        };
        let row = i32::from(c) / self.columns;
        let col = i32::from(c) % self.columns;
        let width = if self.paint_whole_cell {
            self.tile_width
        } else {
            i32::from(widths[usize::from(c)])
        };
        (
            col * self.tile_width,
            row * self.tile_height,
            width,
            self.tile_height,
        )
    }

    /// The width of a character (`CBFontBitmap::GetCharWidth`).
    pub fn char_width(&self, widths: &[u8; 256], c: u8) -> i32 {
        let c = if self.fontext_fix {
            c.wrapping_sub(1)
        } else {
            c
        };
        i32::from(widths[usize::from(c)])
    }

    /// The width of a text on one line.
    pub fn text_width(&self, widths: &[u8; 256], text: &[u8], max_length: i32) -> i32 {
        let text = if max_length >= 0 && text.len() > max_length as usize {
            &text[..max_length as usize]
        } else {
            text
        };
        text.iter().map(|&c| self.char_width(widths, c)).sum()
    }

    /// Breaks a Latin-1 text into lines for a box of `width`
    /// (`CBFontBitmap::TextHeightDraw`): the lines with their start `x`,
    /// relative to the box, and the total height. A negative
    /// `max_height` or `max_length` means no limit.
    pub fn layout(
        &self,
        widths: &[u8; 256],
        text: &[u8],
        width: i32,
        align: TextAlign,
        max_height: i32,
        max_length: i32,
    ) -> BitmapLayout {
        let mut layout = BitmapLayout {
            lines: Vec::new(),
            height: 0,
        };
        if max_length == 0 {
            return layout;
        }
        if text.is_empty() {
            layout.height = self.tile_height;
            return layout;
        }
        let ch = |i: i32| -> u8 {
            if i >= 0 && (i as usize) < text.len() {
                text[i as usize]
            } else {
                0
            }
        };
        let (mut line_length, mut real_length, mut num_lines) = (0, 0, 0);
        let mut index: i32 = -1;
        let (mut start, mut end, mut last_end) = (0i32, 0i32, 0i32);
        let (mut done, mut new_line, mut long_line) = (false, false, false);
        let mut y = 0;
        while !done {
            if max_height > 0 && (num_lines + 1) * self.tile_height > max_height {
                layout.height = num_lines * self.tile_height;
                return layout;
            }
            index += 1;
            if ch(index) == b' ' && (max_height < 0 || max_height / self.tile_height > 1) {
                end = index - 1;
                real_length = line_length;
            }
            if ch(index) == b'\n' {
                end = index - 1;
                real_length = line_length;
                new_line = true;
            }
            if line_length + self.char_width(widths, ch(index)) > width && last_end == end {
                end = index - 1;
                real_length = line_length;
                new_line = true;
                long_line = true;
            }
            if ch(index + 1) == 0 || (max_length >= 0 && index == max_length - 1) {
                done = true;
                if !new_line {
                    end = index;
                    line_length += self.char_width(widths, ch(index));
                    real_length = line_length;
                }
            } else {
                line_length += self.char_width(widths, ch(index));
            }
            if line_length > width || done || new_line {
                if end < 0 {
                    done = true;
                }
                let start_x = align.start_x(0, width, real_length);
                let chars: Vec<u8> = (start..=end).filter(|&i| i >= 0).map(ch).collect();
                layout.lines.push(BitmapLine {
                    x: start_x,
                    y,
                    width: real_length,
                    chars,
                });
                y += self.tile_height;
                last_end = end;
                if long_line {
                    end -= 1;
                }
                start = end + 2;
                index = end + 1;
                line_length = 0;
                new_line = false;
                long_line = false;
                num_lines += 1;
            }
        }
        layout.height = num_lines * self.tile_height;
        layout
    }
}

/// A line of a bitmap font layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapLine {
    /// Where the line starts, relative to the box.
    pub x: i32,
    /// The line's top, relative to the box.
    pub y: i32,
    /// The line's width.
    pub width: i32,
    /// The characters drawn.
    pub chars: Vec<u8>,
}

/// The lines of a text in a box, and the text's height.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapLayout {
    /// The lines, top to bottom.
    pub lines: Vec<BitmapLine>,
    /// The height in pixels.
    pub height: i32,
}

/// A layer of a TrueType font: the text drawn with an offset and a
/// colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontLayer {
    /// Horizontal offset in pixels.
    pub offset_x: i32,
    /// Vertical offset in pixels.
    pub offset_y: i32,
    /// Colour and opacity.
    pub color: Rgba,
}

/// A TrueType font definition (`TTFONT`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrueTypeFontDef {
    /// Size in points at 96 dpi.
    pub size: i32,
    /// The font file.
    pub filename: String,
    /// `BOLD`.
    pub bold: bool,
    /// `ITALIC`.
    pub italic: bool,
    /// `UNDERLINE`.
    pub underline: bool,
    /// `STRIKE`.
    pub strike: bool,
    /// The layers, in drawing order; at least one.
    pub layers: Vec<FontLayer>,
}

impl TrueTypeFontDef {
    /// The pixel size of the font.
    pub fn pixel_size(&self) -> f32 {
        self.size as f32 * 96.0 / 72.0
    }

    fn from_node(root: &Node) -> TrueTypeFontDef {
        let mut def = TrueTypeFontDef {
            size: 0,
            filename: "arial.ttf".to_string(),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            layers: Vec::new(),
        };
        let mut base = Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
        for child in root.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "SIZE" => def.size = integer(child),
                "FILENAME" => {
                    if let Some(name) = child.as_str() {
                        def.filename = name.to_string();
                    }
                }
                "BOLD" => def.bold = boolean(child),
                "ITALIC" => def.italic = boolean(child),
                "UNDERLINE" => def.underline = boolean(child),
                "STRIKE" => def.strike = boolean(child),
                "COLOR" => color_of(child, &mut base),
                "ALPHA" => base.a = integer(child).clamp(0, 255) as u8,
                "LAYER" => {
                    let mut layer = FontLayer {
                        offset_x: 0,
                        offset_y: 0,
                        color: Rgba {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 0,
                        },
                    };
                    for key in child.nodes() {
                        match key.key.to_ascii_uppercase().as_str() {
                            "OFFSET_X" => layer.offset_x = integer(key),
                            "OFFSET_Y" => layer.offset_y = integer(key),
                            "COLOR" => color_of(key, &mut layer.color),
                            "ALPHA" => layer.color.a = integer(key).clamp(0, 255) as u8,
                            _ => {}
                        }
                    }
                    def.layers.push(layer);
                }
                _ => {}
            }
        }
        if def.layers.is_empty() {
            def.layers.push(FontLayer {
                offset_x: 0,
                offset_y: 0,
                color: base,
            });
        }
        def
    }
}

fn color_of(node: &Node, color: &mut Rgba) {
    let v = node.as_ints().unwrap_or_default();
    color.r = channel(&v, 0);
    color.g = channel(&v, 1);
    color.b = channel(&v, 2);
}

fn channel(v: &[i64], i: usize) -> u8 {
    v.get(i).copied().unwrap_or(0).clamp(0, 255) as u8
}

fn boolean(node: &Node) -> bool {
    node.as_bool().unwrap_or(false)
}

fn integer(node: &Node) -> i32 {
    node.as_int().unwrap_or(0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed() -> (BitmapFontDef, [u8; 256]) {
        let def = BitmapFontDef {
            image: Some("f.png".into()),
            columns: 16,
            tile_width: 8,
            tile_height: 10,
            default_width: 4,
            ..BitmapFontDef::default()
        };
        let widths = def.widths(None);
        (def, widths)
    }

    #[test]
    fn parses_bitmap_fonts() {
        let FontFile::Bitmap(def) = FontFile::parse(
            b"FONT { IMAGE=\"fonts\\a.bmp\" TRANSPARENT=255,0,255 COLUMNS=32 TILE_WIDTH=12 TILE_HEIGHT=15 AUTO_WIDTH=TRUE EXPAND_WIDTH=1 WIDTHS { 1, 2, 3 } WIDTHS { 4 } }",
        )
        .unwrap() else {
            panic!("bitmap expected");
        };
        assert_eq!(def.image.as_deref(), Some("fonts\\a.bmp"));
        assert_eq!(
            def.transparent,
            Some(Rgb {
                r: 255,
                g: 0,
                b: 255
            })
        );
        assert_eq!((def.columns, def.tile_width, def.tile_height), (32, 12, 15));
        assert!(def.auto_width);
        assert_eq!(def.expand_width, 1);
        assert_eq!(def.widths, [1, 2, 3, 4]);
        assert!(FontFile::parse(b"FONT { IMAGE=\"a\" COLUMNS=4 }").is_err());
        assert!(FontFile::parse(b"SPRITE { }").is_err());
    }

    #[test]
    fn parses_truetype_fonts() {
        let FontFile::TrueType(def) = FontFile::parse(
            b"TTFONT { SIZE=20 FACE=\"Arial\" FILENAME=\"a.ttf\" BOLD=TRUE COLOR { 1, 2, 3 } ALPHA=200 LAYER { OFFSET_X=1 OFFSET_Y=-1 COLOR { 0,0,0 } ALPHA=255 } LAYER { COLOR { 255,255,255 } ALPHA = 255 } }",
        )
        .unwrap() else {
            panic!("truetype expected");
        };
        assert_eq!(def.size, 20);
        assert_eq!(def.filename, "a.ttf");
        assert!(def.bold && !def.italic);
        assert_eq!(def.layers.len(), 2);
        assert_eq!((def.layers[0].offset_x, def.layers[0].offset_y), (1, -1));
        assert_eq!(def.layers[1].color.r, 255);
        assert_eq!(def.pixel_size(), 20.0 * 96.0 / 72.0);
        let FontFile::TrueType(plain) =
            FontFile::parse(b"TTFONT { SIZE=12 COLOR { 9, 8, 7 } ALPHA=100 }").unwrap()
        else {
            panic!()
        };
        assert_eq!(plain.filename, "arial.ttf");
        assert_eq!(plain.layers.len(), 1);
        assert_eq!(
            plain.layers[0].color,
            Rgba {
                r: 9,
                g: 8,
                b: 7,
                a: 100
            }
        );
    }

    #[test]
    fn fixed_widths_and_tiles() {
        let (def, widths) = fixed();
        assert_eq!(widths[b'a' as usize], 4);
        assert_eq!(def.char_width(&widths, b'a'), 4);
        assert_eq!(def.text_width(&widths, b"abc", -1), 12);
        assert_eq!(def.text_width(&widths, b"abc", 2), 8);
        assert_eq!(
            def.tile(&widths, b'A'),
            (8, 40, 4, 10),
            "tile 65: column 1, row 4"
        );
        let whole = BitmapFontDef {
            paint_whole_cell: true,
            fontext_fix: true,
            ..def.clone()
        };
        assert_eq!(
            whole.tile(&widths, b'A'),
            (0, 40, 8, 10),
            "tile 64 with the fix"
        );
    }

    #[test]
    fn measures_widths_from_an_image() {
        // Two columns of 4x4 tiles, two rows; tile 1 has pixels up to
        // column 2, tile 2 is full width, tile 3 is empty.
        let mut pixels = vec![0u8; 8 * 8 * 4];
        let set = |pixels: &mut Vec<u8>, x: usize, y: usize| {
            let i = (y * 8 + x) * 4;
            pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        };
        set(&mut pixels, 4, 0);
        set(&mut pixels, 6, 3);
        set(&mut pixels, 3, 7);
        let image = Image {
            width: 8,
            height: 8,
            pixels,
            has_alpha_channel: true,
        };
        let def = BitmapFontDef {
            image: Some("f.png".into()),
            columns: 2,
            tile_width: 4,
            tile_height: 4,
            auto_width: true,
            ..BitmapFontDef::default()
        };
        let widths = def.widths(Some(&image));
        assert_eq!(&widths[..4], &[0, 3, 4, 0]);
        // The space takes half of m + i: both are outside the image, so 0.
        assert_eq!(widths[b' ' as usize], 0);
        let expanded = BitmapFontDef {
            expand_width: 2,
            space_width: 5,
            ..def
        };
        let widths = expanded.widths(Some(&image));
        assert_eq!(&widths[..4], &[2, 5, 6, 2]);
        assert_eq!(widths[b' ' as usize], 5);
    }

    #[test]
    fn breaks_lines_like_the_engine() {
        let (def, widths) = fixed();
        // 4 px per char: "hello world" in 24 px breaks at the space.
        let layout = def.layout(&widths, b"hello world", 24, TextAlign::Left, -1, -1);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].chars, b"hello");
        assert_eq!(layout.lines[1].chars, b"world");
        assert_eq!((layout.lines[1].x, layout.lines[1].y), (0, 10));
        assert_eq!(layout.height, 20);
        // A long word is cut.
        let layout = def.layout(&widths, b"abcdefgh", 16, TextAlign::Right, -1, -1);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].chars, b"abcd");
        assert_eq!(layout.lines[1].chars, b"efgh");
        // Explicit breaks and centring.
        let layout = def.layout(&widths, b"ab\ncdef", 40, TextAlign::Center, -1, -1);
        assert_eq!(layout.lines[0].chars, b"ab");
        assert_eq!(layout.lines[0].x, (40 - 8) / 2);
        assert_eq!(layout.lines[1].chars, b"cdef");
        assert_eq!(layout.lines[1].x, (40 - 16) / 2);
        // Height limits and lengths.
        let layout = def.layout(&widths, b"a b c d", 4, TextAlign::Left, 20, -1);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.height, 20);
        let layout = def.layout(&widths, b"abcdef", 100, TextAlign::Left, -1, 3);
        assert_eq!(layout.lines[0].chars, b"abc");
        assert_eq!(
            def.layout(&widths, b"", 100, TextAlign::Left, -1, -1)
                .height,
            10
        );
        assert_eq!(
            def.layout(&widths, b"x", 100, TextAlign::Left, -1, 0)
                .height,
            0
        );
        // One line of text fits: width, not height, decides.
        let layout = def.layout(&widths, b"hello", 100, TextAlign::Left, -1, -1);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].width, 20);
    }

    #[test]
    fn alignment_codes() {
        assert_eq!(TextAlign::from_code(0), TextAlign::Left);
        assert_eq!(TextAlign::from_code(1), TextAlign::Right);
        assert_eq!(TextAlign::from_code(2), TextAlign::Center);
        assert_eq!(TextAlign::from_code(9), TextAlign::Center);
        assert_eq!(TextAlign::from_code(-1), TextAlign::Left);
        assert_eq!(TextAlign::Center.start_x(10, 100, 20), 50);
        assert_eq!(TextAlign::Right.start_x(10, 100, 20), 90);
    }
}
