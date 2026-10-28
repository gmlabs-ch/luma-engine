//! Fonts loaded from the game and text drawn with them: bitmap fonts
//! from their tile image, TrueType fonts rasterised with `fontdue` into
//! cached text images (see `docs/formats/font.md`).
//!
//! Written with reference to `BFontBitmap.cpp`, `BFontTT.cpp` and
//! `BFontStorage.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan
//! Nedoma); see `NOTICE`.

use std::collections::HashMap;

use luma_archive::Vfs;
use luma_formats::font::{BitmapFontDef, FontFile, TextAlign, TrueTypeFontDef};
use luma_formats::{DEFAULT_COLOR_KEY, Image, Rgb, Rgba, Sprite};
use luma_render::{DrawList, Quad, Rect, Renderer, Texture};

use crate::scene_view::key;

/// The number of rendered texts a TrueType font keeps.
const CACHED_TEXTS: usize = 30;

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
            Font::TrueType(f) => f.measure(text, width).1,
        }
    }

    /// The width of a text on one line.
    pub fn text_width(&mut self, text: &str) -> i32 {
        match self {
            Font::Bitmap(f) => f.def.text_width(&f.widths, &latin1(text), -1),
            Font::TrueType(f) => f.measure(text, -1).0,
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
        }
    }

    /// The height of a text in a box.
    pub fn text_height(&self, text: &str, width: i32) -> i32 {
        self.def
            .layout(&self.widths, &latin1(text), width, TextAlign::Left, -1, -1)
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
        let layout = self
            .def
            .layout(&self.widths, &latin1(text), width, align, max_height, -1);
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
        })
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

    /// Breaks a text into lines for a box of `max_width`
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
    pub fn render(
        &mut self,
        text: &str,
        width: i32,
        align: TextAlign,
        max_height: i32,
    ) -> (Image, i32) {
        let lines = self.wrap(text, width, max_height);
        let wrapped_height = lines.len() as i32 * self.line_height;
        let mut text_height = lines.len() as i32 * (self.max_char_height + self.ascender as i32);
        if wrapped_height > text_height {
            text_height = wrapped_height;
        }
        let width = width.max(1);
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
            let mut prev: Option<char> = None;
            for ch in line.chars() {
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

    /// Renders a text into the cache if it is not there.
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
        let (image, offset) = self.render(text, width, align, max_height);
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
            Ok(font) => {
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
