//! Fonts loaded from the game and text drawn with them: bitmap fonts
//! from their tile image, TrueType fonts rasterised with `fontdue` into
//! cached text images (see `docs/formats/font.md`).
//!
//! Written with reference to `BFontBitmap.cpp`, `BFontTT.cpp` and
//! `BFontStorage.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan
//! Nedoma) and to `BFontBitmap.cpp`, `BFontTT.cpp`, `BTextUtils.cpp` and
//! `UIEdit.cpp` of Wintermute Engine 1.x (MIT License, Copyright (c)
//! 2003-2013 Jan Nedoma); see `NOTICE`.
//!
//! The text a font is given is the game's, held as its files hold it:
//! one `char` per byte. A font reads it in the game's encoding
//! (`Game.TextEncoding`, see [`FontBank::set_utf8`]): the bytes are
//! Windows-1252 characters in an ANSI game, UTF-8 in a UTF-8 game.

use std::collections::HashMap;

use luma_archive::Vfs;
use luma_formats::font::{BitmapFontDef, FontFile, TextAlign, TrueTypeFontDef};
use luma_formats::{DEFAULT_COLOR_KEY, Image, Rgb, Rgba, Sprite};
use luma_render::{DrawList, Quad, Rect, Renderer, Texture};
use luma_script::classes::utf8_to_text;

use crate::scene_view::key;

/// The number of rendered texts a TrueType font keeps.
const CACHED_TEXTS: usize = 30;

/// The widest image a text without a box is rendered to: wider than any
/// screen a game draws on, so nothing visible is lost.
const MAX_UNBOUNDED_WIDTH: i32 = 4096;

/// System font files tried when a TrueType font is not in the game.
const SYSTEM_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/liberation-sans/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "C:\\Windows\\Fonts\\arial.ttf",
];

/// A font the game can draw text with.
#[derive(Debug)]
pub enum Font {
    /// A bitmap font.
    Bitmap(Box<BitmapFont>),
    /// A TrueType font.
    TrueType(Box<TrueTypeFont>),
}

/// The Latin-1 bytes of a text, `?` for other characters.
pub fn latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

/// The characters Windows-1252 puts at `0x80..=0x9F`, where Latin-1 has
/// control codes; U+FFFD for the five bytes it leaves unused.
const WINDOWS_1252: [char; 32] = [
    '€', '\u{fffd}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{fffd}', 'Ž',
    '\u{fffd}', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{fffd}',
    'ž', 'Ÿ',
];

/// The characters of an ANSI game's text, for a TrueType font: each
/// byte is a Windows-1252 character, the code page the original engine
/// draws ANSI text with on a western system. Characters beyond a byte
/// are kept.
fn ansi_to_text(raw: &str) -> String {
    raw.chars()
        .map(|c| match u32::from(c) {
            code @ 0x80..=0x9F => match WINDOWS_1252[(code - 0x80) as usize] {
                '\u{fffd}' => c,
                mapped => mapped,
            },
            _ => c,
        })
        .collect()
}

/// The Windows-1252 bytes of a text, `?` for the characters the code
/// page lacks (`CBTextUtils::Utf8ToAnsi`): what a bitmap font draws of
/// a UTF-8 game's text.
fn text_to_ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match u32::from(c) {
            code @ (0..=0x7F | 0xA0..=0xFF) => code as u8,
            _ => WINDOWS_1252
                .iter()
                .position(|&w| w == c && w != '\u{fffd}')
                .map_or(b'?', |at| 0x80 + at as u8),
        })
        .collect()
}

impl Font {
    /// Loads a `.font` file and what it needs (the tile image, the
    /// TrueType file).
    pub fn load(vfs: &Vfs, path: &str) -> Result<Font, String> {
        let bytes = vfs.read(path).map_err(|e| e.to_string())?;
        match FontFile::parse(&bytes).map_err(|e| e.to_string())? {
            FontFile::Bitmap(def) => BitmapFont::load(vfs, def).map(|f| Font::Bitmap(Box::new(f))),
            FontFile::TrueType(def) => {
                TrueTypeFont::load(vfs, def).map(|f| Font::TrueType(Box::new(f)))
            }
        }
    }

    /// The height of a line.
    pub fn line_height(&self) -> i32 {
        match self {
            Font::Bitmap(f) => f.def.tile_height,
            Font::TrueType(f) => f.line_height,
        }
    }

    /// The height of a text laid out in a box of `width`.
    pub fn text_height(&mut self, text: &str, width: i32) -> i32 {
        match self {
            Font::Bitmap(f) => f.text_height(text, width),
            Font::TrueType(f) => f.measure(&f.characters(text), width).1,
        }
    }

    /// The width of a text on one line.
    pub fn text_width(&mut self, text: &str) -> i32 {
        match self {
            Font::Bitmap(f) => f.def.text_width(&f.widths, &f.codes(text), -1),
            Font::TrueType(f) => f.measure(&f.characters(text), -1).0,
        }
    }

    /// Lays out and renders a text so that [`Font::draw`] can show it
    /// after the next [`Font::upload`]. Bitmap fonts need nothing.
    pub fn prepare(&mut self, text: &str, width: i32, align: TextAlign, max_height: i32) {
        if let Font::TrueType(f) = self {
            f.prepare(text, width, align, max_height);
        }
    }

    /// Uploads the textures the font needs: the tile image, or the
    /// texts prepared since the last upload.
    pub fn upload(&mut self, renderer: &Renderer) {
        match self {
            Font::Bitmap(f) => f.upload(renderer),
            Font::TrueType(f) => f.upload(renderer),
        }
    }

    /// Reads the text as UTF-8 (`Game.TextEncoding`), or as ANSI.
    pub fn set_utf8(&mut self, utf8: bool) {
        match self {
            Font::Bitmap(f) => f.utf8 = utf8,
            Font::TrueType(f) => f.set_utf8(utf8),
        }
    }

    /// Lays TrueType text out right to left (`Game.TextRTL`); bitmap
    /// fonts are unaffected, as in the original engine, where only the
    /// GDI text of `CBFontTT` reads the flag.
    pub fn set_rtl(&mut self, rtl: bool) {
        if let Font::TrueType(f) = self {
            f.set_rtl(rtl);
        }
    }

    /// Draws a text in a box at `(x, y)` of `width`, from the textures
    /// uploaded (`CBFont::DrawText`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        text: &str,
        x: i32,
        y: i32,
        width: i32,
        align: TextAlign,
        max_height: i32,
    ) {
        match self {
            Font::Bitmap(f) => f.draw(list, text, x, y, width, align, max_height),
            Font::TrueType(f) => f.draw(list, text, x, y, width, align, max_height),
        }
    }
}

/// A negative width means a single line of any length, for the callers
/// that lay the text out themselves (an editor's line). The engine's
/// layout takes the width as it is: with a negative one every character
/// would be too wide and nothing would be drawn.
fn unbounded(width: i32) -> i32 {
    if width < 0 { i32::MAX / 2 } else { width }
}

/// A bitmap font with its tile image.
#[derive(Debug)]
pub struct BitmapFont {
    /// The definition.
    pub def: BitmapFontDef,
    /// The width of each character.
    pub widths: [u8; 256],
    /// The tile image's path.
    pub image_path: String,
    image: Image,
    texture: Option<Texture>,
    /// Whether the text is UTF-8 (`Game.TextEncoding`).
    utf8: bool,
}

impl BitmapFont {
    /// Loads the tile image and measures the widths.
    pub fn load(vfs: &Vfs, def: BitmapFontDef) -> Result<BitmapFont, String> {
        let (image_path, color_key) = match (&def.image, &def.sprite) {
            (Some(image), _) => (image.clone(), def.transparent.unwrap_or(DEFAULT_COLOR_KEY)),
            (None, Some(sprite_path)) => {
                let bytes = vfs.read(sprite_path).map_err(|e| e.to_string())?;
                let sprite = Sprite::parse(&bytes).map_err(|e| e.to_string())?;
                let sub = sprite
                    .frames
                    .first()
                    .and_then(|f| f.subframes.first())
                    .ok_or_else(|| format!("{sprite_path}: no frame"))?;
                let image = sub
                    .image
                    .clone()
                    .ok_or_else(|| format!("{sprite_path}: no image"))?;
                (image, sub.color_key)
            }
            (None, None) => return Err("Incomplete font definition".to_string()),
        };
        let bytes = vfs.read(&image_path).map_err(|e| e.to_string())?;
        let image = luma_formats::image::decode(&bytes, color_key).map_err(|e| e.to_string())?;
        let widths = def.widths(Some(&image));
        Ok(BitmapFont {
            def,
            widths,
            image_path,
            image,
            texture: None,
            utf8: false,
        })
    }

    /// A font from a definition and an already decoded image (tests).
    pub fn from_image(def: BitmapFontDef, image_path: &str, image: Image) -> BitmapFont {
        let widths = def.widths(Some(&image));
        BitmapFont {
            def,
            widths,
            image_path: image_path.to_string(),
            image,
            texture: None,
            utf8: false,
        }
    }

    /// The tile codes of a text: its bytes in an ANSI game; in a UTF-8
    /// game its characters converted to the ANSI code page, `?` for
    /// those it lacks (`CBFontBitmap::TextHeightDraw` calls
    /// `Utf8ToAnsi`).
    fn codes(&self, text: &str) -> Vec<u8> {
        if self.utf8 {
            text_to_ansi(&utf8_to_text(text))
        } else {
            latin1(text)
        }
    }

    /// The height of a text in a box.
    pub fn text_height(&self, text: &str, width: i32) -> i32 {
        self.def
            .layout(
                &self.widths,
                &self.codes(text),
                unbounded(width),
                TextAlign::Left,
                -1,
                -1,
            )
            .height
    }

    fn upload(&mut self, renderer: &Renderer) {
        if self.texture.is_none() {
            self.texture = Some(renderer.texture(&self.image_path, &self.image));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        text: &str,
        x: i32,
        y: i32,
        width: i32,
        align: TextAlign,
        max_height: i32,
    ) {
        let Some(texture) = &self.texture else {
            return;
        };
        let layout = self.def.layout(
            &self.widths,
            &self.codes(text),
            unbounded(width),
            // Without a width there is nothing to centre the line in.
            if width < 0 { TextAlign::Left } else { align },
            max_height,
            -1,
        );
        for line in &layout.lines {
            let mut cx = x + line.x;
            for &c in &line.chars {
                let (tx, ty, tw, th) = self.def.tile(&self.widths, c);
                if tw > 0 {
                    list.push(Quad {
                        texture,
                        dst: Rect::new(cx as f32, (y + line.y) as f32, tw as f32, th as f32),
                        src: Some(Rect::new(tx as f32, ty as f32, tw as f32, th as f32)),
                        tint: Rgba::OPAQUE_WHITE,
                        flip_x: false,
                        flip_y: false,
                    });
                }
                cx += self.def.char_width(&self.widths, c);
            }
        }
    }
}

/// A rasterised glyph.
#[derive(Debug)]
struct Glyph {
    metrics: fontdue::Metrics,
    coverage: Vec<u8>,
}

impl Glyph {
    /// The glyph's top above the baseline (FreeType's `bitmap_top`).
    fn bearing_y(&self) -> i32 {
        self.metrics.ymin + self.metrics.height as i32
    }
}

/// A text rendered to an image, kept for reuse.
#[derive(Debug)]
struct CachedText {
    text: String,
    width: i32,
    align: TextAlign,
    max_height: i32,
    image: Image,
    offset: i32,
    texture: Option<Texture>,
    priority: u32,
}

/// A TrueType font: the rasteriser, its glyphs, and the texts rendered.
#[derive(Debug)]
pub struct TrueTypeFont {
    /// The definition.
    pub def: TrueTypeFontDef,
    /// The file the glyphs come from (the game's, or a system font).
    pub source: String,
    font: fontdue::Font,
    px: f32,
    /// The height of a line.
    pub line_height: i32,
    ascender: f32,
    descender: f32,
    max_char_height: i32,
    glyphs: HashMap<char, Option<Glyph>>,
    texts: Vec<CachedText>,
    /// Whether lines are laid out right to left (`Game.TextRTL`).
    rtl: bool,
    /// Whether the text is UTF-8 (`Game.TextEncoding`).
    utf8: bool,
}

impl TrueTypeFont {
    /// Loads the font file from the game, or a system font when the
    /// game has none.
    pub fn load(vfs: &Vfs, def: TrueTypeFontDef) -> Result<TrueTypeFont, String> {
        let (source, bytes) = match vfs.read(&def.filename) {
            Ok(bytes) => (def.filename.clone(), bytes),
            Err(_) => {
                let fallback = SYSTEM_FONTS
                    .iter()
                    .find_map(|p| std::fs::read(p).ok().map(|b| (p.to_string(), b)));
                match fallback {
                    Some(found) => found,
                    None => {
                        return Err(format!(
                            "Error loading TrueType font '{}': no system font found",
                            def.filename
                        ));
                    }
                }
            }
        };
        TrueTypeFont::from_bytes(def, &source, &bytes)
    }

    /// A font from the bytes of a `.ttf` file.
    pub fn from_bytes(
        def: TrueTypeFontDef,
        source: &str,
        bytes: &[u8],
    ) -> Result<TrueTypeFont, String> {
        let px = def.pixel_size();
        let settings = fontdue::FontSettings {
            scale: px,
            ..fontdue::FontSettings::default()
        };
        let font =
            fontdue::Font::from_bytes(bytes, settings).map_err(|e| format!("{source}: {e}"))?;
        let metrics = font
            .horizontal_line_metrics(px)
            .ok_or_else(|| format!("{source}: no horizontal metrics"))?;
        let ascender = metrics.ascent;
        let descender = -metrics.descent;
        let line_height = metrics.new_line_size.ceil() as i32 + 2;
        let max_char_height = (metrics.ascent - metrics.descent).ceil() as i32;
        Ok(TrueTypeFont {
            def,
            source: source.to_string(),
            font,
            px,
            line_height,
            ascender,
            descender,
            max_char_height,
            glyphs: HashMap::new(),
            texts: Vec::new(),
            rtl: false,
            utf8: false,
        })
    }

    /// Reads the text as UTF-8 or as ANSI; the texts rendered so far go
    /// when the encoding changes.
    pub fn set_utf8(&mut self, utf8: bool) {
        if self.utf8 != utf8 {
            self.utf8 = utf8;
            self.texts.clear();
        }
    }

    /// The characters of a game text in the game's encoding
    /// (`CBTextUtils::ConvertToNative`): what [`wrap`](Self::wrap),
    /// [`measure`](Self::measure) and [`render`](Self::render) take.
    pub fn characters(&self, text: &str) -> String {
        if self.utf8 {
            utf8_to_text(text)
        } else {
            ansi_to_text(text)
        }
    }

    /// Lays lines out right to left when `rtl`; the texts rendered so
    /// far go when the direction changes.
    pub fn set_rtl(&mut self, rtl: bool) {
        if self.rtl != rtl {
            self.rtl = rtl;
            self.texts.clear();
        }
    }

    fn glyph(&mut self, ch: char) -> Option<&Glyph> {
        let font = &self.font;
        let px = self.px;
        self.glyphs
            .entry(ch)
            .or_insert_with(|| {
                if font.lookup_glyph_index(ch) == 0 {
                    return None;
                }
                let (metrics, coverage) = font.rasterize(ch, px);
                Some(Glyph { metrics, coverage })
            })
            .as_ref()
    }

    fn kerning(&self, left: char, right: char) -> f32 {
        self.font
            .horizontal_kern(left, right, self.px)
            .unwrap_or(0.0)
    }

    /// The advance of a character after `prev`, as the engine counts
    /// it: the integer parts of the advance and of the kerning.
    fn advance(&mut self, prev: Option<char>, ch: char) -> Option<i32> {
        let advance = self.glyph(ch)?.metrics.advance_width as i32;
        let kerning = prev.map_or(0.0, |p| self.kerning(p, ch)) as i32;
        Some(advance + kerning)
    }

    /// Breaks a text (its characters) into lines for a box of `max_width`
    /// (`CBFontTT::WrapText`): each line with its width. Negative limits
    /// mean none.
    pub fn wrap(&mut self, text: &str, max_width: i32, max_height: i32) -> Vec<(String, i32)> {
        let chars: Vec<char> = text.chars().collect();
        let mut lines: Vec<(String, i32)> = Vec::new();
        let mut curr_width = 0;
        let mut prev_char: Option<char> = None;
        let mut prev_space_index: Option<usize> = None;
        let mut prev_space_width = 0;
        let mut line_start = 0;
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if ch == ' ' {
                prev_space_index = Some(i);
                prev_space_width = curr_width;
            }
            let mut char_width = 0;
            if ch != '\n' {
                match self.advance(prev_char, ch) {
                    Some(advance) => {
                        char_width = advance;
                        prev_char = Some(ch);
                    }
                    None => {
                        i += 1;
                        continue;
                    }
                }
            }
            let line_too_long = max_width >= 0 && curr_width + char_width > max_width;
            if line_too_long && curr_width == 0 {
                break;
            }
            if ch == '\n' || i == chars.len() - 1 || line_too_long {
                let (mut break_point, mut break_width, break_on_space);
                match prev_space_index {
                    Some(space) if line_too_long => {
                        break_point = space;
                        break_width = prev_space_width;
                        break_on_space = true;
                    }
                    _ => {
                        break_point = i;
                        break_width = curr_width;
                        break_on_space = ch == '\n';
                        if i == chars.len() - 1 {
                            break_point += 1;
                            break_width += char_width;
                        }
                    }
                }
                if max_height >= 0 && (lines.len() as i32 + 1) * self.line_height > max_height {
                    break;
                }
                let line: String = chars[line_start..break_point.max(line_start)]
                    .iter()
                    .collect();
                lines.push((line, break_width));
                curr_width = 0;
                prev_char = None;
                prev_space_index = None;
                if break_on_space {
                    break_point += 1;
                }
                line_start = break_point;
                i = break_point;
                continue;
            }
            curr_width += char_width;
            i += 1;
        }
        lines
    }

    /// The width and height of a text in a box.
    pub fn measure(&mut self, text: &str, max_width: i32) -> (i32, i32) {
        let lines = self.wrap(text, max_width, -1);
        let height = lines.len() as i32 * self.line_height;
        let width = lines.iter().map(|l| l.1).max().unwrap_or(0);
        (width, height)
    }

    /// Renders a text to an image (`CBFontTT::RenderTextToTexture`):
    /// white glyphs with their coverage as alpha, to be tinted by each
    /// layer. Returns the image and the offset the engine draws it
    /// higher by.
    ///
    /// A negative width means no box, as for [`wrap`](Self::wrap) and
    /// for the bitmap fonts: the lines are never broken for their
    /// length, start at the left whatever the alignment, and the image
    /// is as wide as the widest of them (an editor's line).
    pub fn render(
        &mut self,
        text: &str,
        width: i32,
        align: TextAlign,
        max_height: i32,
    ) -> (Image, i32) {
        let boxed = width >= 0;
        let lines = self.wrap(text, if boxed { width } else { -1 }, max_height);
        let wrapped_height = lines.len() as i32 * self.line_height;
        let mut text_height = lines.len() as i32 * (self.max_char_height + self.ascender as i32);
        if wrapped_height > text_height {
            text_height = wrapped_height;
        }
        let (width, align) = if boxed {
            (width.max(1), align)
        } else {
            // The widest line, with room for a last glyph that reaches
            // past its advance (an italic).
            let widest = lines.iter().map(|l| l.1).max().unwrap_or(0);
            let width = widest.saturating_add(self.px.ceil() as i32);
            (width.clamp(1, MAX_UNBOUNDED_WIDTH), TextAlign::Left)
        };
        let text_height = text_height.max(1);
        let mut image = Image {
            width: width as u32,
            height: text_height as u32,
            pixels: vec![0; (width * text_height * 4) as usize],
            has_alpha_channel: true,
        };
        let mut pos_y = self.line_height - self.descender as i32;
        let mut text_offset = 0;
        for (line, line_width) in &lines {
            let mut pos_x = align.start_x(0, width, *line_width);
            let orig_x = pos_x;
            text_offset = 0;
            for ch in line.chars() {
                if let Some(glyph) = self.glyph(ch) {
                    text_offset = text_offset.max(glyph.bearing_y());
                }
            }
            // Right-to-left text lays the line's glyphs out from the last
            // to the first: the reading order GDI gives the original
            // engine with `ETO_RTLREADING`, without a bidirectional
            // algorithm.
            let visual: Vec<char> = if self.rtl {
                line.chars().rev().collect()
            } else {
                line.chars().collect()
            };
            let mut prev: Option<char> = None;
            for ch in visual {
                let (metrics, bearing_y, coverage) = match self.glyph(ch) {
                    Some(glyph) => (glyph.metrics, glyph.bearing_y(), glyph.coverage.clone()),
                    None => continue,
                };
                let kerning = prev.map_or(0.0, |p| self.kerning(p, ch)) as i32;
                pos_x += kerning;
                let gx = pos_x + metrics.xmin;
                let gy = pos_y - bearing_y + text_offset;
                blit(&mut image, &coverage, metrics.width, metrics.height, gx, gy);
                prev = Some(ch);
                pos_x += metrics.advance_width as i32;
            }
            if self.def.underline {
                let y = (-self.font_underline() + self.ascender) as i32;
                for x in orig_x..orig_x + line_width {
                    set_pixel(&mut image, x, y);
                }
            }
            pos_y += self.line_height;
        }
        (image, text_offset)
    }

    fn font_underline(&self) -> f32 {
        // fontdue exposes no underline position; a line just below the
        // baseline, as most fonts place it.
        -(self.descender / 2.0)
    }

    /// Renders a game text into the cache if it is not there.
    pub fn prepare(&mut self, text: &str, width: i32, align: TextAlign, max_height: i32) {
        if text.is_empty() {
            return;
        }
        let found = self.texts.iter().position(|t| {
            t.text == text && t.width == width && t.align == align && t.max_height == max_height
        });
        let next_priority = self.texts.iter().map(|t| t.priority).max().unwrap_or(0) + 1;
        if let Some(i) = found {
            self.texts[i].priority = next_priority;
            return;
        }
        let characters = self.characters(text);
        let (image, offset) = self.render(&characters, width, align, max_height);
        let cached = CachedText {
            text: text.to_string(),
            width,
            align,
            max_height,
            image,
            offset,
            texture: None,
            priority: next_priority,
        };
        if self.texts.len() >= CACHED_TEXTS {
            let oldest = (0..self.texts.len())
                .min_by_key(|&i| self.texts[i].priority)
                .unwrap_or(0);
            self.texts[oldest] = cached;
        } else {
            self.texts.push(cached);
        }
    }

    /// The size of the image a text was prepared to, if it was: what
    /// [`Font::draw`] shows of it once uploaded.
    pub fn prepared_size(
        &self,
        text: &str,
        width: i32,
        align: TextAlign,
        max_height: i32,
    ) -> Option<(u32, u32)> {
        self.texts
            .iter()
            .find(|t| {
                t.text == text && t.width == width && t.align == align && t.max_height == max_height
            })
            .map(|t| (t.image.width, t.image.height))
    }

    fn upload(&mut self, renderer: &Renderer) {
        for text in &mut self.texts {
            if text.texture.is_none() {
                text.texture = Some(renderer.texture("text", &text.image));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        text: &str,
        x: i32,
        y: i32,
        width: i32,
        align: TextAlign,
        max_height: i32,
    ) {
        let Some(cached) = self.texts.iter().find(|t| {
            t.text == text && t.width == width && t.align == align && t.max_height == max_height
        }) else {
            return;
        };
        let Some(texture) = &cached.texture else {
            return;
        };
        for layer in &self.def.layers {
            list.push(Quad {
                texture,
                dst: Rect::new(
                    (x + layer.offset_x) as f32,
                    (y - cached.offset + layer.offset_y) as f32,
                    cached.image.width as f32,
                    cached.image.height as f32,
                ),
                src: None,
                tint: layer.color,
                flip_x: false,
                flip_y: false,
            });
        }
    }
}

/// Blits a coverage bitmap into an image as white pixels with that
/// alpha, keeping the higher alpha where glyphs overlap.
fn blit(image: &mut Image, coverage: &[u8], w: usize, h: usize, x: i32, y: i32) {
    for row in 0..h {
        let py = y + row as i32;
        if py < 0 || py >= image.height as i32 {
            continue;
        }
        for col in 0..w {
            let px = x + col as i32;
            if px < 0 || px >= image.width as i32 {
                continue;
            }
            let a = coverage[row * w + col];
            if a == 0 {
                continue;
            }
            let i = ((py as u32 * image.width + px as u32) * 4) as usize;
            image.pixels[i] = 255;
            image.pixels[i + 1] = 255;
            image.pixels[i + 2] = 255;
            image.pixels[i + 3] = image.pixels[i + 3].max(a);
        }
    }
}

fn set_pixel(image: &mut Image, x: i32, y: i32) {
    if x < 0 || y < 0 || x >= image.width as i32 || y >= image.height as i32 {
        return;
    }
    let i = ((y as u32 * image.width + x as u32) * 4) as usize;
    image.pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
}

/// The fonts a game has loaded, by path.
#[derive(Debug, Default)]
pub struct FontBank {
    fonts: HashMap<String, Font>,
    missing: Vec<(String, String)>,
    /// Whether text is laid out right to left (`Game.TextRTL`).
    rtl: bool,
    /// Whether text is UTF-8 (`Game.TextEncoding`).
    utf8: bool,
}

impl FontBank {
    /// An empty bank.
    pub fn new() -> FontBank {
        FontBank::default()
    }

    /// Loads a font if it is not loaded yet (`CBFontStorage::AddFont`).
    /// Returns whether it is available.
    pub fn load(&mut self, vfs: &Vfs, path: &str) -> bool {
        let k = key(path);
        if self.fonts.contains_key(&k) {
            return true;
        }
        if self.missing.iter().any(|(p, _)| key(p) == k) {
            return false;
        }
        match Font::load(vfs, path) {
            Ok(mut font) => {
                font.set_rtl(self.rtl);
                font.set_utf8(self.utf8);
                self.fonts.insert(k, font);
                true
            }
            Err(reason) => {
                self.missing.push((path.to_string(), reason));
                false
            }
        }
    }

    /// A loaded font.
    pub fn get(&self, path: &str) -> Option<&Font> {
        self.fonts.get(&key(path))
    }

    /// A loaded font, mutable.
    pub fn get_mut(&mut self, path: &str) -> Option<&mut Font> {
        self.fonts.get_mut(&key(path))
    }

    /// Lays the fonts' text out right to left (`Game.TextRTL`); fonts
    /// loaded later follow.
    pub fn set_rtl(&mut self, rtl: bool) {
        self.rtl = rtl;
        for font in self.fonts.values_mut() {
            font.set_rtl(rtl);
        }
    }

    /// Whether text is laid out right to left.
    pub fn rtl(&self) -> bool {
        self.rtl
    }

    /// Reads the text the fonts are given as UTF-8 (`Game.TextEncoding`
    /// 1, what a string table with a byte order mark turns on) or as
    /// ANSI; fonts loaded later follow.
    pub fn set_utf8(&mut self, utf8: bool) {
        self.utf8 = utf8;
        for font in self.fonts.values_mut() {
            font.set_utf8(utf8);
        }
    }

    /// Whether text is read as UTF-8.
    pub fn utf8(&self) -> bool {
        self.utf8
    }

    /// Fonts that could not be loaded, with the reason.
    pub fn missing(&self) -> &[(String, String)] {
        &self.missing
    }

    /// Takes the reasons of the fonts that failed since the last call.
    pub fn take_missing(&mut self) -> Vec<(String, String)> {
        let taken = self.missing.clone();
        self.missing.clear();
        taken
    }

    /// Uploads what every font needs.
    pub fn upload(&mut self, renderer: &Renderer) {
        for font in self.fonts.values_mut() {
            font.upload(renderer);
        }
    }
}

/// The colour key a bitmap font uses when the file gives none.
pub fn default_color_key() -> Rgb {
    DEFAULT_COLOR_KEY
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system_font() -> Option<(String, Vec<u8>)> {
        SYSTEM_FONTS
            .iter()
            .find_map(|p| std::fs::read(p).ok().map(|b| (p.to_string(), b)))
    }

    #[test]
    fn bitmap_fonts_measure_and_lay_out_text() {
        let def = BitmapFontDef {
            image: Some("f.png".into()),
            columns: 16,
            tile_width: 8,
            tile_height: 10,
            default_width: 4,
            ..BitmapFontDef::default()
        };
        let image = Image {
            width: 128,
            height: 160,
            pixels: vec![0; 128 * 160 * 4],
            has_alpha_channel: true,
        };
        let mut font = Font::Bitmap(Box::new(BitmapFont::from_image(def, "f.png", image)));
        assert_eq!(font.line_height(), 10);
        assert_eq!(font.text_width("abc"), 12);
        assert_eq!(font.text_height("hello world", 24), 20);
        assert_eq!(font.text_width("é€"), 8, "Latin-1 and a replacement");
    }

    #[test]
    fn truetype_fonts_wrap_and_render() {
        let Some((source, bytes)) = system_font() else {
            eprintln!("skipped: no system font");
            return;
        };
        let FontFile::TrueType(def) =
            FontFile::parse(b"TTFONT { SIZE=12 LAYER { OFFSET_X=1 OFFSET_Y=1 COLOR {0,0,0} ALPHA=255 } LAYER { COLOR {255,255,255} ALPHA=255 } }")
                .unwrap()
        else {
            panic!()
        };
        let mut font = TrueTypeFont::from_bytes(def, &source, &bytes).unwrap();
        assert!(
            font.line_height > 12 && font.line_height < 40,
            "{}",
            font.line_height
        );
        let (w, h) = font.measure("Hello", -1);
        assert!(w > 10 && h == font.line_height);
        let lines = font.wrap("Hello wide world", w + 2, -1);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert_eq!(lines[0].0, "Hello");
        assert_eq!(lines[1].0, "wide");
        assert_eq!(lines[2].0, "world");
        let lines = font.wrap("a\nb", 1000, -1);
        assert_eq!(lines.len(), 2);
        assert_eq!(font.wrap("a\nb\nc", 1000, font.line_height).len(), 1);
        let (image, offset) = font.render("Hi", 100, TextAlign::Center, -1);
        assert_eq!(image.width, 100);
        assert!(offset > 0);
        assert!(image.pixels.chunks(4).any(|p| p[3] > 128), "some ink");
        font.prepare("Hi", 100, TextAlign::Center, -1);
        font.prepare("Hi", 100, TextAlign::Center, -1);
        assert_eq!(font.texts.len(), 1);
        for i in 0..40 {
            font.prepare(&format!("t{i}"), 50, TextAlign::Left, -1);
        }
        assert_eq!(font.texts.len(), CACHED_TEXTS);

        // Right-to-left lays the glyphs out from the last: "ab" comes out
        // as "ba" does left to right; the cache goes with the direction.
        let (ltr, _) = font.render("ba", 100, TextAlign::Left, -1);
        font.set_rtl(true);
        assert!(font.texts.is_empty());
        let (rtl, _) = font.render("ab", 100, TextAlign::Left, -1);
        assert_eq!(rtl.pixels, ltr.pixels);
        let (same, _) = font.render("ab", 100, TextAlign::Left, -1);
        font.set_rtl(false);
        let (back, _) = font.render("ab", 100, TextAlign::Left, -1);
        assert_eq!(same.pixels, rtl.pixels);
        assert_ne!(back.pixels, rtl.pixels);
    }

    /// A negative width is a single line of any length, as for the
    /// bitmap fonts: the image is as wide as the line, not one pixel.
    #[test]
    fn truetype_fonts_render_one_unbounded_line_with_a_negative_width() {
        let Some((source, bytes)) = system_font() else {
            eprintln!("skipped: no system font");
            return;
        };
        let FontFile::TrueType(def) =
            FontFile::parse(b"TTFONT { SIZE=12 COLOR {255,255,255} ALPHA=255 }").unwrap()
        else {
            panic!()
        };
        let mut font = TrueTypeFont::from_bytes(def, &source, &bytes).unwrap();
        let text = "Lab with axe, a long name no box holds";
        let (line_width, height) = font.measure(text, -1);
        assert_eq!(height, font.line_height, "one line");
        let (image, _) = font.render(text, -1, TextAlign::Center, -1);
        assert!(
            image.width as i32 >= line_width,
            "{} < {line_width}",
            image.width
        );
        assert!((image.width as i32) < line_width + 2 * font.line_height);
        // The ink reaches the end of the line, and starts at the left
        // whatever the alignment asked.
        let column_has_ink = |image: &Image, x: u32| {
            (0..image.height).any(|y| image.pixels[((y * image.width + x) * 4 + 3) as usize] > 0)
        };
        let last_ink = (0..image.width)
            .rev()
            .find(|&x| column_has_ink(&image, x))
            .unwrap();
        let first_ink = (0..image.width)
            .find(|&x| column_has_ink(&image, x))
            .unwrap();
        assert!(
            last_ink as i32 > line_width - font.line_height,
            "{last_ink}"
        );
        assert!((first_ink as i32) < font.line_height, "{first_ink}");
        let (left, _) = font.render(text, -1, TextAlign::Left, -1);
        assert_eq!(left.pixels, image.pixels);

        // What is prepared is what is drawn: same arguments, same image.
        font.prepare(text, -1, TextAlign::Left, -1);
        assert_eq!(
            font.prepared_size(text, -1, TextAlign::Left, -1),
            Some((image.width, image.height))
        );
        assert_eq!(font.prepared_size(text, 50, TextAlign::Left, -1), None);

        // A line breaks only where the text says so, and an absurd
        // length does not make an absurd image.
        assert_eq!(
            font.render("a\nb", -1, TextAlign::Left, -1).0.height as i32,
            {
                let lines = 2;
                (lines * font.line_height)
                    .max(lines * (font.max_char_height + font.ascender as i32))
            }
        );
        let endless = "wide ".repeat(4000);
        let (image, _) = font.render(&endless, -1, TextAlign::Left, -1);
        assert_eq!(image.width as i32, MAX_UNBOUNDED_WIDTH);
        // A box still wraps and keeps its width.
        let (image, _) = font.render(text, 60, TextAlign::Left, -1);
        assert_eq!(image.width, 60);
        assert!(image.height as i32 >= 2 * font.line_height);
    }

    /// A game's text as its files hold it: one `char` per UTF-8 byte.
    fn raw(text: &str) -> String {
        luma_script::classes::text_to_utf8(text)
    }

    #[test]
    fn bitmap_fonts_read_utf8_text_through_the_ansi_code_page() {
        let def = BitmapFontDef {
            image: Some("f.png".into()),
            columns: 16,
            tile_width: 8,
            tile_height: 10,
            default_width: 4,
            ..BitmapFontDef::default()
        };
        let image = Image {
            width: 128,
            height: 160,
            pixels: vec![0; 128 * 160 * 4],
            has_alpha_channel: true,
        };
        let mut font = Font::Bitmap(Box::new(BitmapFont::from_image(def, "f.png", image)));
        // ANSI: a tile per byte, so the two bytes of a UTF-8 `é` are two.
        assert_eq!(font.text_width(&raw("été")), 20);
        font.set_utf8(true);
        assert_eq!(font.text_width(&raw("été")), 12, "a tile per character");
        assert_eq!(
            font.text_width(&raw("ЖΩ")),
            8,
            "`?` for what the page lacks"
        );
        assert_eq!(font.text_height(&raw("été été"), 14), 20);
        let Font::Bitmap(bitmap) = &font else {
            panic!()
        };
        assert_eq!(bitmap.codes(&raw("é’€œЖ")), b"\xe9\x92\x80\x9c?");
        // Bytes that are not UTF-8 are replaced, not a panic.
        assert_eq!(bitmap.codes("caf\u{e9}"), b"caf?");
        assert_eq!(text_to_ansi("\u{fffd}"), b"?");
    }

    #[test]
    fn ansi_text_is_windows_1252_for_truetype_fonts() {
        assert_eq!(
            ansi_to_text("caf\u{e9} \u{84}x\u{93} \u{80}\u{81}"),
            "café „x“ €\u{81}"
        );
        assert_eq!(text_to_ansi("café „x“ €"), b"caf\xe9 \x84x\x93 \x80");
    }

    #[test]
    fn truetype_fonts_read_utf8_text_one_glyph_per_character() {
        let Some((source, bytes)) = system_font() else {
            eprintln!("skipped: no system font");
            return;
        };
        let FontFile::TrueType(def) =
            FontFile::parse(b"TTFONT { SIZE=12 COLOR {255,255,255} ALPHA=255 }").unwrap()
        else {
            panic!()
        };
        let mut font = Font::TrueType(Box::new(
            TrueTypeFont::from_bytes(def, &source, &bytes).unwrap(),
        ));
        let sentence = "L'été déjà fini à Noël";
        // What the characters measure, laid out directly.
        let expected = match &mut font {
            Font::TrueType(f) => f.measure(sentence, -1).0,
            Font::Bitmap(_) => unreachable!(),
        };
        let ansi = font.text_width(&raw(sentence));
        font.set_utf8(true);
        let utf8 = font.text_width(&raw(sentence));
        assert_eq!(utf8, expected, "one glyph per character");
        assert!(ansi > utf8, "read as ANSI, each accent is two glyphs");
        let Font::TrueType(f) = &mut font else {
            unreachable!()
        };
        assert_eq!(f.characters(&raw("é č Ж Ω")), "é č Ж Ω");
        let lines = f.wrap(&f.characters(&raw(sentence)), -1, -1);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0.chars().count(), sentence.chars().count());
        // The cache is keyed by the text as given and goes with the
        // encoding; text that is not UTF-8 renders without a panic.
        f.prepare(&raw(sentence), 300, TextAlign::Left, -1);
        f.prepare("caf\u{e9}\u{ff}", 300, TextAlign::Left, -1);
        assert_eq!(f.texts.len(), 2);
        f.set_utf8(false);
        assert!(f.texts.is_empty());
    }

    /// Every `.font` of the local test game loads and measures text.
    #[test]
    #[ignore = "needs the local test game in testdata/local"]
    fn loads_every_local_font() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/white-chamber");
        if !root.is_dir() {
            eprintln!("skipped: {} is absent", root.display());
            return;
        }
        let vfs = Vfs::open_game(&root).unwrap();
        let fonts: Vec<String> = vfs
            .packages()
            .iter()
            .flat_map(|p| p.entries().map(|e| e.name.clone()))
            .filter(|f| f.to_ascii_lowercase().ends_with(".font"))
            .collect();
        assert!(!fonts.is_empty());
        let mut bank = FontBank::new();
        for path in &fonts {
            assert!(bank.load(&vfs, path), "{path}: {:?}", bank.missing());
            let font = bank.get_mut(path).unwrap();
            let height = font.text_height("The quick brown fox jumps over the lazy dog", 300);
            let width = font.text_width("Hello");
            let line = font.line_height();
            let kind = match font {
                Font::Bitmap(_) => "bitmap".to_string(),
                Font::TrueType(f) => f.source.clone(),
            };
            eprintln!("{path}: {kind}, line {line}, 'Hello' {width} px, paragraph {height} px");
            assert!(height >= line && width > 0, "{path}");
        }
        eprintln!("{} fonts loaded", fonts.len());
    }
}
