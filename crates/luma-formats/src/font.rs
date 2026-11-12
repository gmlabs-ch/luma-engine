//! Fonts (`.font`): bitmap fonts made of character tiles, and TrueType
//! fonts with coloured layers.
//!
//! The format is documented in `docs/formats/font.md` and was
//! established from the MIT-licensed WME Lite sources (`BFont.cpp`,
//! `BFontBitmap.cpp`, `BFontTT.cpp`, Copyright (c) 2011 Jan Nedoma). See
//! `NOTICE` at the root of the repository.

use crate::def::{Document, Node, keys, scan_int_list, token};
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
    /// Parses a `.font` file. The first statement decides the kind
    /// (`CBFont::IsTrueType`): `TTFONT` is a TrueType font, anything else
    /// must be `FONT`.
    pub fn parse(bytes: &[u8]) -> Result<FontFile> {
        let doc = Document::parse(bytes)?;
        let first = doc.nodes.first().map(|node| node.key.as_str());
        let kind = first.and_then(|key| token(&["FONT", "TTFONT"], key));
        if kind == Some("TTFONT") {
            return Ok(FontFile::TrueType(TrueTypeFontDef::from_node(
                &doc.nodes[0],
            )?));
        }
        let root = doc.root("FONT").ok_or_else(|| Error::Invalid {
            reason: "'FONT' keyword expected".into(),
        })?;
        Ok(FontFile::Bitmap(BitmapFontDef::from_node(root)?))
    }
}

/// The keys of `CBFontBitmap::LoadBuffer`. The original lists `WIDTHS`
/// before `WIDTHS_FRAME`, so that `WIDTHS_FRAME` is read as `WIDTHS`
/// followed by garbage and crashes the engine; here the longer key comes
/// first and is read as it was meant to be.
const BITMAP_FONT_KEYS: &[&str] = &[
    "FONTEXT_FIX",
    "FONT",
    "IMAGE",
    "TRANSPARENT",
    "COLUMNS",
    "TILE_WIDTH",
    "TILE_HEIGHT",
    "DEFAULT_WIDTH",
    "WIDTHS_FRAME",
    "WIDTHS",
    "AUTO_WIDTH",
    "SPACE_WIDTH",
    "EXPAND_WIDTH",
    "EDITOR_PROPERTY",
    "SPRITE",
    "PAINT_WHOLE_CELL",
];

/// The keys of `CBFontTT::LoadBuffer`, in the order of its table.
const TRUETYPE_FONT_KEYS: &[&str] = &[
    "TTFONT",
    "SIZE",
    "FACE",
    "FILENAME",
    "BOLD",
    "ITALIC",
    "UNDERLINE",
    "STRIKE",
    "CHARSET",
    "COLOR",
    "ALPHA",
    "LAYER",
];

/// The keys of `CBFontTT::ParseLayer`, in the order of its table.
const FONT_LAYER_KEYS: &[&str] = &["OFFSET_X", "OFFSET_Y", "COLOR", "ALPHA"];

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
    /// `CBFontBitmap::LoadBuffer`: the keys of the `FONT` block. A key
    /// that is not in the table is a syntax error that fails the font.
    /// Whether the image or the sprite can be loaded is the caller's
    /// part of the 'Incomplete font definition' test.
    fn from_node(root: &Node) -> Result<BitmapFontDef> {
        let mut def = BitmapFontDef::default();
        let (mut r, mut g, mut b) = (255, 255, 255);
        for item in keys(root, BITMAP_FONT_KEYS, "FONT") {
            let (key, child) = item?;
            match key {
                "IMAGE" => def.image = Some(child.text()),
                "SPRITE" => def.sprite = Some(child.text()),
                "TRANSPARENT" => {
                    child.read_ints(&mut [&mut r, &mut g, &mut b]);
                    def.transparent = Some(Rgb::from_ints(r, g, b));
                }
                "WIDTHS" => {
                    // The original writes past its 256 widths when the
                    // lists are longer; the extra ones are dropped here.
                    for w in scan_int_list(child.as_str().unwrap_or_default()) {
                        if def.widths.len() >= 256 {
                            break;
                        }
                        def.widths.push((w & 0xff) as u8);
                    }
                }
                "DEFAULT_WIDTH" => child.read_int(&mut def.default_width),
                "WIDTHS_FRAME" => child.read_int(&mut def.widths_frame),
                "COLUMNS" => child.read_int(&mut def.columns),
                "TILE_WIDTH" => child.read_int(&mut def.tile_width),
                "TILE_HEIGHT" => child.read_int(&mut def.tile_height),
                "AUTO_WIDTH" => child.read_bool(&mut def.auto_width),
                "FONTEXT_FIX" => child.read_bool(&mut def.fontext_fix),
                "PAINT_WHOLE_CELL" => child.read_bool(&mut def.paint_whole_cell),
                "SPACE_WIDTH" => child.read_int(&mut def.space_width),
                "EXPAND_WIDTH" => child.read_int(&mut def.expand_width),
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
            // The original stores every width in a byte: a value over 255
            // keeps its low byte.
            if self.expand_width != 0 {
                for width in &mut widths {
                    *width = ((i32::from(*width) + self.expand_width).max(0) & 0xff) as u8;
                }
            }
            let space = if self.fontext_fix { b' ' - 1 } else { b' ' } as usize;
            if self.space_width != 0 {
                widths[space] = (self.space_width & 0xff) as u8;
            } else if i32::from(widths[space]) == self.expand_width || widths[space] == 0 {
                widths[space] = ((u16::from(widths[usize::from(b'm')])
                    + u16::from(widths[usize::from(b'i')]))
                    / 2) as u8;
            }
        } else {
            for (i, width) in widths.iter_mut().enumerate() {
                *width = match self.widths.get(i) {
                    Some(w) => *w,
                    None => (self.default_width & 0xff) as u8,
                };
            }
        }
        widths
    }

    /// The measured width of tile `index`: one more than the right-most
    /// column with a pixel that is not fully transparent.
    fn measure_tile(&self, image: &Image, index: i32) -> i32 {
        let x0 = (index % self.columns) * self.tile_width;
        let y0 = (index / self.columns) * self.tile_height;
        let mut min_col = -1;
        for row in 0..self.tile_height {
            let mut col = self.tile_width - 1;
            while col > min_col {
                let (x, y) = (x0 + col, y0 + row);
                // Ink is any pixel that is not fully transparent
                // (`IsTransparentAtLite`), the soft edge of an
                // antialiased letter included.
                let opaque =
                    x >= 0 && y >= 0 && image.pixel(x as u32, y as u32).is_some_and(|p| p.a != 0);
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
    /// `FACE`: the family name of the typeface, `Arial` by default.
    pub face: String,
    /// `FILENAME`: a font file of the game, made available to the
    /// engine; it is the one drawn with when `FACE` names it.
    pub filename: String,
    /// `BOLD`.
    pub bold: bool,
    /// `ITALIC`.
    pub italic: bool,
    /// `UNDERLINE`.
    pub underline: bool,
    /// `STRIKE`.
    pub strike: bool,
    /// `CHARSET`: the character set the text's bytes are read in when the
    /// game is not UTF-8 (0 and 1: the system's, read as Windows-1252;
    /// see `charset_code_page`).
    pub charset: i32,
    /// The layers, in drawing order; at least one.
    pub layers: Vec<FontLayer>,
}

impl TrueTypeFontDef {
    /// The pixel size of the font: the point size at 96 dots per inch,
    /// rounded to a whole pixel (`MulDiv(size, 96, 72)`).
    pub fn pixel_size(&self) -> f32 {
        let scaled = i64::from(self.size) * 96;
        let rounded = if scaled >= 0 {
            (scaled + 36) / 72
        } else {
            (scaled - 36) / 72
        };
        rounded as f32
    }

    /// `CBFontTT::LoadBuffer`: the keys of the `TTFONT` block. A key that
    /// is not in the table is a syntax error that fails the font; a
    /// `LAYER` that cannot be read is dropped alone (the original's error
    /// code is overwritten by the next statement).
    fn from_node(root: &Node) -> Result<TrueTypeFontDef> {
        let mut def = TrueTypeFontDef {
            size: 12,
            face: "Arial".to_string(),
            filename: "arial.ttf".to_string(),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            charset: 0,
            layers: Vec::new(),
        };
        let mut base = Rgba::from_ints(0, 0, 0, 0);
        for item in keys(root, TRUETYPE_FONT_KEYS, "TTFONT") {
            let (key, child) = item?;
            match key {
                "SIZE" => child.read_int(&mut def.size),
                "FACE" => def.face = child.text(),
                "FILENAME" => def.filename = child.text(),
                "BOLD" => child.read_bool(&mut def.bold),
                "ITALIC" => child.read_bool(&mut def.italic),
                "UNDERLINE" => child.read_bool(&mut def.underline),
                "STRIKE" => child.read_bool(&mut def.strike),
                "CHARSET" => child.read_int(&mut def.charset),
                "COLOR" => color_of(child, &mut base),
                "ALPHA" => alpha_of(child, &mut base),
                "LAYER" => def.layers.extend(FontLayer::from_node(child).ok()),
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
        Ok(def)
    }
}

impl FontLayer {
    /// `CBFontTT::ParseLayer`: any key outside its table fails the layer.
    fn from_node(node: &Node) -> Result<FontLayer> {
        let mut layer = FontLayer {
            offset_x: 0,
            offset_y: 0,
            color: Rgba::from_ints(0, 0, 0, 0),
        };
        for item in keys(node, FONT_LAYER_KEYS, "LAYER") {
            let (key, child) = item?;
            match key {
                "OFFSET_X" => child.read_int(&mut layer.offset_x),
                "OFFSET_Y" => child.read_int(&mut layer.offset_y),
                "COLOR" => color_of(child, &mut layer.color),
                "ALPHA" => alpha_of(child, &mut layer.color),
                _ => {}
            }
        }
        Ok(layer)
    }
}

/// `COLOR { r, g, b }` over a colour, whose alpha stays. The original's
/// three integers are uninitialised; a short list leaves the others at 0.
fn color_of(node: &Node, color: &mut Rgba) {
    let (mut r, mut g, mut b) = (0, 0, 0);
    node.read_ints(&mut [&mut r, &mut g, &mut b]);
    *color = Rgba::from_ints(r, g, b, i32::from(color.a));
}

/// `ALPHA = a` over a colour, whose red, green and blue stay.
fn alpha_of(node: &Node, color: &mut Rgba) {
    let mut a = 0;
    node.read_int(&mut a);
    color.a = (a & 0xff) as u8;
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
            b"TTFONT { SIZE=20 FACE=\"Comic Sans MS\" FILENAME=\"a.ttf\" BOLD=TRUE COLOR { 1, 2, 3 } ALPHA=200 LAYER { OFFSET_X=1 OFFSET_Y=-1 COLOR { 0,0,0 } ALPHA=255 } LAYER { COLOR { 255,255,255 } ALPHA = 255 } }",
        )
        .unwrap() else {
            panic!("truetype expected");
        };
        assert_eq!(def.size, 20);
        assert_eq!(def.filename, "a.ttf");
        assert_eq!(def.face, "Comic Sans MS");
        assert!(def.bold && !def.italic);
        assert_eq!(def.layers.len(), 2);
        assert_eq!((def.layers[0].offset_x, def.layers[0].offset_y), (1, -1));
        assert_eq!(def.layers[1].color.r, 255);
        assert_eq!(def.pixel_size(), 27.0, "20 points at 96 dpi, rounded");
        let FontFile::TrueType(plain) =
            FontFile::parse(b"TTFONT { SIZE=12 COLOR { 9, 8, 7 } ALPHA=100 }").unwrap()
        else {
            panic!()
        };
        assert_eq!(plain.filename, "arial.ttf");
        assert_eq!(plain.face, "Arial", "the typeface without `FACE`");
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

    /// `CBFontBitmap::LoadBuffer`: the tables, the defaults and what a
    /// key outside them does.
    #[test]
    fn font_keys_follow_the_original() {
        // A key outside the table fails the font.
        assert!(
            FontFile::parse(b"FONT { IMAGE=\"a\" COLUMNS=4 TILE_WIDTH=4 TILE_HEIGHT=4 BOGUS=1 }")
                .is_err()
        );
        assert!(FontFile::parse(b"TTFONT { SIZE=10 BOGUS=1 }").is_err());
        // The root block is the first statement.
        assert!(
            FontFile::parse(
                b"NAME=\"x\"\nFONT { IMAGE=\"a\" COLUMNS=4 TILE_WIDTH=4 TILE_HEIGHT=4 }"
            )
            .is_err()
        );
        // `WIDTHS_FRAME` is read as meant (the original takes it for
        // `WIDTHS` and crashes), next to `WIDTHS`.
        let FontFile::Bitmap(def) = FontFile::parse(
            b"FONT { SPRITE=\"f.sprite\" COLUMNS=4 TILE_WIDTH=4 TILE_HEIGHT=4 WIDTHS_FRAME=2 WIDTHS { 300, 2 } DEFAULT_WIDTH=260 }",
        )
        .unwrap() else {
            panic!("bitmap expected");
        };
        assert_eq!(def.widths_frame, 2);
        // Widths are bytes: 300 keeps its low byte, 44.
        assert_eq!(def.widths, [44, 2]);
        assert_eq!(def.widths(None)[5], 4, "260 as a byte");

        // TrueType: `SIZE` is 12 by default, `CHARSET` is read, a layer
        // with a key of its own that is not known is dropped alone.
        let FontFile::TrueType(def) = FontFile::parse(
            b"TTFONT { CHARSET=5 COLOR { 1,2,3 } ALPHA=9 LAYER { BOGUS=1 } LAYER { OFFSET_X=2 COLOR { 4,5,6 } } }",
        )
        .unwrap() else {
            panic!("truetype expected");
        };
        assert_eq!(def.size, 12);
        assert_eq!(def.charset, 5);
        assert_eq!(def.layers.len(), 1);
        assert_eq!(def.layers[0].offset_x, 2);
        assert_eq!(def.layers[0].color, Rgba::from_ints(4, 5, 6, 0));
    }

    /// `CBFontBitmap::GetWidths`: a pixel is ink unless it is fully
    /// transparent.
    #[test]
    fn a_faint_pixel_counts_for_the_width() {
        let mut pixels = vec![0u8; 4 * 4 * 4];
        // One pixel of alpha 1 in column 2 of the only tile.
        pixels[(4 + 2) * 4 + 3] = 1;
        let image = Image {
            width: 4,
            height: 4,
            pixels,
            has_alpha_channel: true,
        };
        let def = BitmapFontDef {
            image: Some("f.png".into()),
            columns: 1,
            tile_width: 4,
            tile_height: 4,
            auto_width: true,
            ..BitmapFontDef::default()
        };
        assert_eq!(def.widths(Some(&image))[0], 3);
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
