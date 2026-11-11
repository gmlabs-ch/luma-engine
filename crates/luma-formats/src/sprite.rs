//! Sprites: the engine's animated images.
//!
//! The format is documented in `docs/formats/sprite.md` and was established
//! from the MIT-licensed WME Lite sources (`BSprite.cpp`, `BFrame.cpp`,
//! `BSubFrame.cpp`, Copyright (c) 2011 Jan Nedoma). See `NOTICE` at the
//! root of the repository.

use crate::def::{Document, Node, keys};
use crate::error::{Error, Result};
use crate::image::{Rgb, Rgba};

/// An animated image: frames shown in sequence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sprite {
    /// Name used by scripts.
    pub name: String,
    /// Restart after the last frame.
    pub looping: bool,
    /// May only be interrupted on the last frame.
    pub continuous: bool,
    /// Pixel-precise hit testing.
    pub precise: bool,
    /// Images are loaded on demand.
    pub streamed: bool,
    /// With `streamed`, keep images once loaded.
    pub streamed_keep_loaded: bool,
    /// Scripts attached to the sprite.
    pub scripts: Vec<String>,
    /// The frames, in order.
    pub frames: Vec<Frame>,
}

/// One frame of a sprite.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frame {
    /// Time on screen, in milliseconds.
    pub delay_ms: u32,
    /// Key frame for scripts.
    pub keyframe: bool,
    /// Offset applied to the owner when the frame becomes current.
    pub move_x: i32,
    /// See `move_x`.
    pub move_y: i32,
    /// Sound played when the frame becomes current.
    pub sound: Option<String>,
    /// Stop the previous frame's sound first.
    pub kill_sound: bool,
    /// Script events raised on the owner when the frame becomes current.
    pub apply_events: Vec<String>,
    /// Sub-frames, drawn in order. The first one is the frame's own.
    pub subframes: Vec<SubFrame>,
}

/// A rectangle with exclusive right and bottom edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rect {
    /// Left edge.
    pub left: i32,
    /// Top edge.
    pub top: i32,
    /// Right edge, exclusive.
    pub right: i32,
    /// Bottom edge, exclusive.
    pub bottom: i32,
}

impl Rect {
    /// Width, zero when the rectangle is inverted.
    pub fn width(&self) -> u32 {
        (self.right - self.left).max(0) as u32
    }

    /// Height, zero when the rectangle is inverted.
    pub fn height(&self) -> u32 {
        (self.bottom - self.top).max(0) as u32
    }

    /// Whether the rectangle has no area, which the engine treats as
    /// "the whole image".
    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
}

/// An image, or part of one, drawn at an offset.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SubFrame {
    /// Image file, relative to the game root; `None` draws nothing.
    pub image: Option<String>,
    /// Colour key for images without an alpha channel.
    pub color_key: Rgb,
    /// Whether `color_key` was set explicitly (`TRANSPARENT`).
    pub custom_color_key: bool,
    /// Part of the image to draw; `None` means the whole image.
    pub rect: Option<Rect>,
    /// Point of the sub-frame placed at the owner's position.
    pub hotspot_x: i32,
    /// See `hotspot_x`.
    pub hotspot_y: i32,
    /// Tint and opacity; `255,255,255,255` means unchanged.
    pub tint: Rgba,
    /// Flip horizontally.
    pub mirror_x: bool,
    /// Flip vertically.
    pub mirror_y: bool,
    /// Not part of the clickable area.
    pub decoration: bool,
    /// Drawn only by the engine's software renderer (`2D_ONLY`); an
    /// accelerated renderer skips it.
    pub only_2d: bool,
    /// Drawn only by the engine's accelerated renderer (`3D_ONLY`); the
    /// software renderer skips it.
    pub only_3d: bool,
}

impl Default for SubFrame {
    fn default() -> Self {
        SubFrame {
            image: None,
            color_key: crate::image::DEFAULT_COLOR_KEY,
            custom_color_key: false,
            rect: None,
            hotspot_x: 0,
            hotspot_y: 0,
            tint: Rgba::OPAQUE_WHITE,
            mirror_x: false,
            mirror_y: false,
            decoration: false,
            only_2d: false,
            only_3d: false,
        }
    }
}

impl SubFrame {
    /// A sub-frame showing a whole image with default settings.
    pub fn with_image(image: impl Into<String>) -> Self {
        SubFrame {
            image: Some(image.into()),
            ..SubFrame::default()
        }
    }

    /// Whether the tint leaves the image unchanged, in which case the
    /// owner's own alpha applies.
    pub fn is_untinted(&self) -> bool {
        self.tint == Rgba::OPAQUE_WHITE
    }
}

/// Image extensions the engine accepts in place of a `.sprite` file.
const IMAGE_EXTENSIONS: [&str; 4] = ["bmp", "tga", "png", "jpg"];

impl Sprite {
    /// Whether a sprite path refers directly to an image file.
    pub fn is_image_path(path: &str) -> bool {
        path.rfind('.').is_some_and(|dot| {
            let ext = &path[dot + 1..];
            IMAGE_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext))
        })
    }

    /// The single-frame sprite the engine builds for an image path.
    pub fn from_image(path: impl Into<String>) -> Sprite {
        Sprite {
            precise: true,
            frames: vec![Frame {
                subframes: vec![SubFrame::with_image(path)],
                ..Frame::default()
            }],
            ..Sprite::default()
        }
    }

    /// Total duration of one pass through the frames, in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.frames.iter().map(|f| u64::from(f.delay_ms)).sum()
    }

    /// The frame shown `elapsed_ms` after the animation started: frames
    /// follow each other after their delays, the animation restarts when
    /// `looping` and stays on the last frame otherwise. A sprite whose
    /// delays are all zero stays on its first frame. `None` without
    /// frames.
    pub fn frame_index_at(&self, elapsed_ms: u64) -> Option<usize> {
        if self.frames.is_empty() {
            return None;
        }
        let total = self.duration_ms();
        if total == 0 {
            return Some(0);
        }
        let mut t = if self.looping {
            elapsed_ms % total
        } else if elapsed_ms >= total {
            return Some(self.frames.len() - 1);
        } else {
            elapsed_ms
        };
        for (i, frame) in self.frames.iter().enumerate() {
            let delay = u64::from(frame.delay_ms);
            if t < delay {
                return Some(i);
            }
            t -= delay;
        }
        Some(self.frames.len() - 1)
    }

    /// Parses a `.sprite` file.
    pub fn parse(bytes: &[u8]) -> Result<Sprite> {
        Self::from_document(&Document::parse(bytes)?)
    }

    /// Builds a sprite from a parsed definition file
    /// (`CBSprite::LoadBuffer`): the first statement must be the `SPRITE`
    /// block; a key the engine does not know, or a frame that cannot be
    /// read, fails the sprite.
    pub fn from_document(doc: &Document) -> Result<Sprite> {
        let root = doc.root("SPRITE").ok_or_else(|| Error::Invalid {
            reason: "'SPRITE' keyword expected".into(),
        })?;
        let mut sprite = Sprite {
            precise: true,
            ..Sprite::default()
        };
        for item in keys(root, SPRITE_KEYS, "SPRITE") {
            let (key, node) = item?;
            match key {
                "NAME" => sprite.name = node.text(),
                "LOOPING" => node.read_bool(&mut sprite.looping),
                "CONTINUOUS" => node.read_bool(&mut sprite.continuous),
                "PRECISE" => node.read_bool(&mut sprite.precise),
                "STREAMED" => node.read_bool(&mut sprite.streamed),
                "STREAMED_KEEP_LOADED" => node.read_bool(&mut sprite.streamed_keep_loaded),
                "SCRIPT" => sprite.scripts.push(node.text()),
                "FRAME" => sprite.frames.push(Frame::from_node(node)?),
                // The editor's keys, and `SPRITE` itself, are accepted
                // and mean nothing to the engine.
                _ => {}
            }
        }
        Ok(sprite)
    }
}

/// The keys of `CBSprite::LoadBuffer`, in the order of its table.
const SPRITE_KEYS: &[&str] = &[
    "CONTINUOUS",
    "SPRITE",
    "LOOPING",
    "FRAME",
    "NAME",
    "PRECISE",
    "EDITOR_MUTED",
    "STREAMED_KEEP_LOADED",
    "STREAMED",
    "SCRIPT",
    "EDITOR_BG_FILE",
    "EDITOR_BG_OFFSET_X",
    "EDITOR_BG_OFFSET_Y",
    "EDITOR_BG_ALPHA",
    "EDITOR_PROPERTY",
];

/// The keys of `CBFrame::LoadBuffer`, in the order of its table.
const FRAME_KEYS: &[&str] = &[
    "DELAY",
    "IMAGE",
    "TRANSPARENT",
    "RECT",
    "HOTSPOT",
    "2D_ONLY",
    "3D_ONLY",
    "MIRROR_X",
    "MIRROR_Y",
    "MOVE",
    "ALPHA_COLOR",
    "ALPHA",
    "SUBFRAME",
    "SOUND",
    "KEYFRAME",
    "DECORATION",
    "APPLY_EVENT",
    "EDITOR_SELECTED",
    "EDITOR_EXPANDED",
    "EDITOR_PROPERTY",
    "KILL_SOUND",
];

/// The keys of `CBSubFrame::LoadBuffer`, in the order of its table.
const SUBFRAME_KEYS: &[&str] = &[
    "IMAGE",
    "TRANSPARENT",
    "RECT",
    "HOTSPOT",
    "2D_ONLY",
    "3D_ONLY",
    "DECORATION",
    "ALPHA_COLOR",
    "ALPHA",
    "MIRROR_X",
    "MIRROR_Y",
    "EDITOR_SELECTED",
    "EDITOR_PROPERTY",
];

/// What a frame or a sub-frame block says about its image, read into
/// locals first and put together after the block, as the original does.
struct ImageKeys {
    image: Option<String>,
    key: (i32, i32, i32),
    custom_key: bool,
    rect: Rect,
    tint: (i32, i32, i32, i32),
}

impl ImageKeys {
    fn new() -> ImageKeys {
        ImageKeys {
            image: None,
            key: (255, 255, 255),
            custom_key: false,
            rect: Rect::default(),
            tint: (255, 255, 255, 255),
        }
    }

    /// Reads one of the image keys; `false` when the key is another one.
    fn read(&mut self, key: &str, node: &Node) -> bool {
        match key {
            "IMAGE" => self.image = Some(node.text()),
            "TRANSPARENT" => {
                let (r, g, b) = &mut self.key;
                node.read_ints(&mut [r, g, b]);
                self.custom_key = true;
            }
            "RECT" => {
                let rect = &mut self.rect;
                node.read_ints(&mut [
                    &mut rect.left,
                    &mut rect.top,
                    &mut rect.right,
                    &mut rect.bottom,
                ]);
            }
            "ALPHA_COLOR" => {
                let (r, g, b, _) = &mut self.tint;
                node.read_ints(&mut [r, g, b]);
            }
            "ALPHA" => node.read_int(&mut self.tint.3),
            _ => return false,
        }
        true
    }

    fn rect(&self) -> Option<Rect> {
        (!self.rect.is_empty()).then_some(self.rect)
    }

    fn tint(&self) -> Rgba {
        let (r, g, b, a) = self.tint;
        Rgba::from_ints(r, g, b, a)
    }

    fn color_key(&self) -> Rgb {
        let (r, g, b) = self.key;
        Rgb::from_ints(r, g, b)
    }
}

impl Frame {
    /// `CBFrame::LoadBuffer`: the frame's keys, then its own sub-frame
    /// built from them and put before the `SUBFRAME`s. A `SUBFRAME` block
    /// that cannot be read is dropped and the frame goes on (the
    /// original's error code is overwritten by the next statement).
    fn from_node(node: &Node) -> Result<Frame> {
        let mut frame = Frame::default();
        let mut own = SubFrame::default();
        let mut image = ImageKeys::new();
        let mut delay = 0;
        for item in keys(node, FRAME_KEYS, "FRAME") {
            let (key, child) = item?;
            if image.read(key, child) {
                continue;
            }
            match key {
                "DELAY" => child.read_int(&mut delay),
                "KEYFRAME" => child.read_bool(&mut frame.keyframe),
                "MOVE" => child.read_ints(&mut [&mut frame.move_x, &mut frame.move_y]),
                "SOUND" => frame.sound = Some(child.text()),
                "KILL_SOUND" => child.read_bool(&mut frame.kill_sound),
                "APPLY_EVENT" => frame.apply_events.push(child.text()),
                "SUBFRAME" => frame.subframes.extend(SubFrame::from_node(child).ok()),
                "HOTSPOT" => child.read_ints(&mut [&mut own.hotspot_x, &mut own.hotspot_y]),
                "2D_ONLY" => child.read_bool(&mut own.only_2d),
                "3D_ONLY" => child.read_bool(&mut own.only_3d),
                "MIRROR_X" => child.read_bool(&mut own.mirror_x),
                "MIRROR_Y" => child.read_bool(&mut own.mirror_y),
                "DECORATION" => child.read_bool(&mut own.decoration),
                _ => {}
            }
        }
        frame.delay_ms = delay.max(0) as u32;
        // The colour key and the tint of the frame only reach its own
        // sub-frame when it has an image.
        if image.image.is_some() {
            own.tint = image.tint();
            if image.custom_key {
                own.color_key = image.color_key();
                own.custom_color_key = true;
            }
        }
        own.image = image.image.take();
        own.rect = image.rect();
        frame.subframes.insert(0, own);
        Ok(frame)
    }
}

impl SubFrame {
    /// `CBSubFrame::LoadBuffer`. A sub-frame without image is allowed.
    fn from_node(node: &Node) -> Result<SubFrame> {
        let mut sub = SubFrame::default();
        let mut image = ImageKeys::new();
        for item in keys(node, SUBFRAME_KEYS, "SUBFRAME") {
            let (key, child) = item?;
            if image.read(key, child) {
                continue;
            }
            match key {
                "HOTSPOT" => child.read_ints(&mut [&mut sub.hotspot_x, &mut sub.hotspot_y]),
                "2D_ONLY" => child.read_bool(&mut sub.only_2d),
                "3D_ONLY" => child.read_bool(&mut sub.only_3d),
                "MIRROR_X" => child.read_bool(&mut sub.mirror_x),
                "MIRROR_Y" => child.read_bool(&mut sub.mirror_y),
                "DECORATION" => child.read_bool(&mut sub.decoration),
                _ => {}
            }
        }
        sub.tint = image.tint();
        if image.custom_key {
            sub.color_key = image.color_key();
            sub.custom_color_key = true;
        }
        sub.rect = image.rect();
        sub.image = image.image;
        Ok(sub)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const WALK: &str = r#"; generated by SpriteEdit
SPRITE {
  NAME="walk"
  LOOPING=TRUE
  CONTINUOUS=TRUE
  PRECISE=FALSE
  STREAMED=TRUE
  SCRIPT="a.script"
  FRAME {
    DELAY = 100
    KEYFRAME=TRUE
    IMAGE = "actors\droid\walk_01.png"
    HOTSPOT {50, 120}
    MOVE {0, -5}
    SOUND="audio\step.wav"
    KILL_SOUND=TRUE
    APPLY_EVENT="Step"
    APPLY_EVENT="Dust"
    ALPHA = 128
    ALPHA_COLOR { 255,200,100 }
    MIRROR_X=TRUE
    EDITOR_SELECTED=TRUE
    SUBFRAME {
      IMAGE = "actors\droid\shadow.png"
      HOTSPOT {50, 110}
      DECORATION=TRUE
      RECT { 10, 20, 30, 60 }
      TRANSPARENT { 166,202,240 }
      2D_ONLY=TRUE
    }
  }
  FRAME {
    DELAY = 50
  }
}
"#;

    #[test]
    fn parses_a_full_sprite() {
        let s = Sprite::parse(WALK.as_bytes()).unwrap();
        assert_eq!(s.name, "walk");
        assert!(s.looping && s.continuous && s.streamed && !s.precise);
        assert_eq!(s.scripts, ["a.script"]);
        assert_eq!(s.frames.len(), 2);

        let f = &s.frames[0];
        assert_eq!(f.delay_ms, 100);
        assert!(f.keyframe && f.kill_sound);
        assert_eq!((f.move_x, f.move_y), (0, -5));
        assert_eq!(f.sound.as_deref(), Some("audio\\step.wav"));
        assert_eq!(f.apply_events, ["Step", "Dust"]);
        assert_eq!(f.subframes.len(), 2);

        let own = &f.subframes[0];
        assert_eq!(own.image.as_deref(), Some("actors\\droid\\walk_01.png"));
        assert_eq!((own.hotspot_x, own.hotspot_y), (50, 120));
        assert_eq!(
            own.tint,
            Rgba {
                r: 255,
                g: 200,
                b: 100,
                a: 128
            }
        );
        assert!(!own.is_untinted());
        assert!(own.mirror_x && !own.mirror_y && !own.decoration);
        assert_eq!(own.color_key, crate::image::DEFAULT_COLOR_KEY);
        assert!(!own.custom_color_key);
        assert_eq!(own.rect, None);

        let shadow = &f.subframes[1];
        assert_eq!(shadow.image.as_deref(), Some("actors\\droid\\shadow.png"));
        assert_eq!((shadow.hotspot_x, shadow.hotspot_y), (50, 110));
        assert!(shadow.decoration && shadow.only_2d && !shadow.only_3d);
        let rect = shadow.rect.unwrap();
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (10, 20, 30, 60)
        );
        assert_eq!((rect.width(), rect.height()), (20, 40));
        assert_eq!(
            shadow.color_key,
            Rgb {
                r: 166,
                g: 202,
                b: 240
            }
        );
        assert!(shadow.custom_color_key);
        assert!(shadow.is_untinted());

        // An empty frame still has its own, imageless sub-frame.
        let empty = &s.frames[1];
        assert_eq!(empty.delay_ms, 50);
        assert_eq!(empty.subframes.len(), 1);
        assert_eq!(empty.subframes[0].image, None);
    }

    #[test]
    fn defaults_match_the_engine() {
        let s = Sprite::parse(b"SPRITE { FRAME { IMAGE=\"a.png\" } }").unwrap();
        assert!(s.precise, "PRECISE defaults to true");
        assert!(!s.looping && !s.continuous && !s.streamed);
        let sub = &s.frames[0].subframes[0];
        assert_eq!(*sub, SubFrame::with_image("a.png"));
        assert_eq!(
            sub.color_key,
            Rgb {
                r: 255,
                g: 0,
                b: 255
            }
        );
        assert!(sub.is_untinted());
    }

    #[test]
    fn tolerates_partial_and_odd_values() {
        let s = Sprite::parse(
            b"SPRITE { FRAME { DELAY=-5\n HOTSPOT {7}\n RECT {5,5,5,9}\n TRANSPARENT = 300,-1\n ALPHA = 999 } }",
        )
        .unwrap();
        let f = &s.frames[0];
        assert_eq!(f.delay_ms, 0);
        let sub = &f.subframes[0];
        assert_eq!((sub.hotspot_x, sub.hotspot_y), (7, 0));
        assert_eq!(sub.rect, None, "zero-width rect means whole image");
        assert_eq!(
            sub.color_key,
            Rgb {
                r: 255,
                g: 0,
                b: 255
            }
        );
        assert_eq!(sub.tint.a, 255);
    }

    #[test]
    fn picks_frames_by_elapsed_time() {
        let mut sprite = Sprite::parse(WALK.as_bytes()).unwrap();
        assert_eq!(sprite.frames.len(), 2);
        let d0 = u64::from(sprite.frames[0].delay_ms);
        let total = sprite.duration_ms();
        assert!(d0 > 0 && total > d0);
        sprite.looping = false;
        assert_eq!(sprite.frame_index_at(0), Some(0));
        assert_eq!(sprite.frame_index_at(d0), Some(1));
        assert_eq!(
            sprite.frame_index_at(total + 1000),
            Some(1),
            "stays on the last frame"
        );
        sprite.looping = true;
        assert_eq!(sprite.frame_index_at(total), Some(0), "restarts");
        assert_eq!(sprite.frame_index_at(total + d0), Some(1));

        let still = Sprite::from_image("a.png");
        assert_eq!(still.frame_index_at(12345), Some(0));
        assert_eq!(Sprite::default().frame_index_at(0), None);
    }

    #[test]
    fn rejects_files_without_a_sprite_block() {
        let err = Sprite::parse(b"SCENE { }").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
        // The root block must be the first statement.
        let err = Sprite::parse(b"NAME=\"x\"\nSPRITE { }").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
    }

    /// `CBSprite::LoadBuffer`, `CBFrame::LoadBuffer`: a key that is not
    /// in the loader's table is a syntax error that fails the sprite; a
    /// `SUBFRAME` that cannot be read is dropped alone.
    #[test]
    fn an_unknown_key_fails_the_sprite_or_drops_the_subframe() {
        let err = Sprite::parse(b"SPRITE { LOOPING=TRUE\n BOGUS=1 }").unwrap_err();
        assert!(matches!(err, Error::Syntax(_)), "{err}");
        let err = Sprite::parse(b"SPRITE { FRAME { DELAY=1\n BOGUS=1 } }").unwrap_err();
        assert!(matches!(err, Error::Syntax(_)), "{err}");
        // The editor's keys are in the table.
        let s = Sprite::parse(
            b"SPRITE { EDITOR_MUTED=TRUE\n EDITOR_BG_ALPHA=3\n FRAME { EDITOR_EXPANDED=TRUE\n SUBFRAME { IMAGE=\"a.png\"\n MOVE {1,2} }\n SUBFRAME { IMAGE=\"b.png\" } } }",
        )
        .unwrap();
        // `MOVE` is a frame key, not a sub-frame key: that sub-frame goes.
        let images: Vec<_> = s.frames[0]
            .subframes
            .iter()
            .map(|sub| sub.image.as_deref())
            .collect();
        assert_eq!(images, [None, Some("b.png")]);
    }

    /// Values read as the original's `ScanStr` and `DRGBA` do.
    #[test]
    fn frame_values_follow_the_original() {
        let s = Sprite::parse(
            b"SPRITE { LOOPING { TRUE }\n PRECISE=\"\"\n FRAME { IMAGE=\"a.png\"\n ALPHA { 300 }\n TRANSPARENT = 300,-1\n HOTSPOT=3,4\n MIRROR_X=T } }",
        )
        .unwrap();
        assert!(s.looping, "a scalar in braces");
        assert!(s.precise, "an empty value keeps the default");
        let sub = &s.frames[0].subframes[0];
        assert_eq!(sub.tint.a, 44, "300 cut to its low byte");
        assert_eq!(
            sub.color_key,
            Rgb {
                r: 44,
                g: 255,
                b: 255
            }
        );
        assert_eq!((sub.hotspot_x, sub.hotspot_y), (3, 4));
        assert!(sub.mirror_x, "a prefix of TRUE is true");
    }

    /// Parses every sprite of the local reference game and decodes every
    /// image they reference. Slow in debug builds: run with
    /// `cargo test --release -p luma-formats -- --ignored`.
    #[test]
    #[ignore = "needs testdata/local and a release build"]
    fn loads_every_local_sprite_and_image() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/white-chamber/unpacked");
        if !root.is_dir() {
            eprintln!("skipped: {} is absent", root.display());
            return;
        }
        let mut sprites = 0;
        let mut images = std::collections::BTreeMap::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("sprite"))
                {
                    let sprite = Sprite::parse(&std::fs::read(&path).unwrap())
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    assert!(!sprite.frames.is_empty(), "{}: no frame", path.display());
                    for sub in sprite.frames.iter().flat_map(|f| &f.subframes) {
                        if let Some(image) = &sub.image {
                            images
                                .entry(image.replace('\\', "/").to_ascii_lowercase())
                                .or_insert(sub.color_key);
                        }
                    }
                    sprites += 1;
                }
            }
        }
        // Lower-case index of the files on disk, since names in sprites
        // do not always match the case on disk.
        let mut on_disk = std::collections::HashMap::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .to_ascii_lowercase();
                    on_disk.insert(rel, path);
                }
            }
        }
        let mut decoded = 0;
        let mut missing = Vec::new();
        for (name, color_key) in &images {
            let Some(path) = on_disk.get(name) else {
                missing.push(name.clone());
                continue;
            };
            let img = crate::image::decode(&std::fs::read(path).unwrap(), *color_key)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(img.width > 0 && img.height > 0, "{name}: empty image");
            assert_eq!(img.pixels.len(), (img.width * img.height * 4) as usize);
            decoded += 1;
        }
        eprintln!(
            "parsed {sprites} sprites, decoded {decoded} distinct images, {} missing: {missing:?}",
            missing.len()
        );
        assert!(sprites > 0 && decoded > 0);
    }

    #[test]
    fn image_paths_become_single_frame_sprites() {
        assert!(Sprite::is_image_path("a\\b.PNG"));
        assert!(Sprite::is_image_path("x.jpg"));
        assert!(!Sprite::is_image_path("x.sprite"));
        assert!(!Sprite::is_image_path("png"));
        let s = Sprite::from_image("items\\axe.png");
        assert_eq!(s.frames.len(), 1);
        assert_eq!(s.frames[0].delay_ms, 0);
        assert_eq!(
            s.frames[0].subframes,
            [SubFrame::with_image("items\\axe.png")]
        );
        assert!(s.precise);
    }
}
