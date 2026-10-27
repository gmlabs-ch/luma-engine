//! Sprites: the engine's animated images.
//!
//! The format is documented in `docs/formats/sprite.md` and was established
//! from the MIT-licensed WME Lite sources (`BSprite.cpp`, `BFrame.cpp`,
//! `BSubFrame.cpp`, Copyright (c) 2011 Jan Nedoma). See `NOTICE` at the
//! root of the repository.

use crate::def::{Document, Node, Nodes};
use crate::error::{Error, Result};
use crate::image::{Rgb, Rgba};

/// An animated image: frames shown in sequence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// Shown only for 2D owners.
    pub only_2d: bool,
    /// Shown only for 3D owners.
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

    /// Parses a `.sprite` file.
    pub fn parse(bytes: &[u8]) -> Result<Sprite> {
        Self::from_document(&Document::parse(bytes)?)
    }

    /// Builds a sprite from a parsed definition file.
    pub fn from_document(doc: &Document) -> Result<Sprite> {
        let root = doc.get("SPRITE").ok_or_else(|| Error::Invalid {
            reason: "'SPRITE' keyword expected".into(),
        })?;
        let mut sprite = Sprite {
            precise: true,
            ..Sprite::default()
        };
        for node in root.nodes() {
            match node.key.to_ascii_uppercase().as_str() {
                "NAME" => sprite.name = node.as_str().unwrap_or("").to_string(),
                "LOOPING" => sprite.looping = node.as_bool().unwrap_or(false),
                "CONTINUOUS" => sprite.continuous = node.as_bool().unwrap_or(false),
                "PRECISE" => sprite.precise = node.as_bool().unwrap_or(false),
                "STREAMED" => sprite.streamed = node.as_bool().unwrap_or(false),
                "STREAMED_KEEP_LOADED" => {
                    sprite.streamed_keep_loaded = node.as_bool().unwrap_or(false);
                }
                "SCRIPT" => sprite.scripts.extend(node.as_str().map(str::to_string)),
                "FRAME" => sprite.frames.push(Frame::from_node(node)?),
                _ => {}
            }
        }
        Ok(sprite)
    }
}

impl Frame {
    fn from_node(node: &Node) -> Result<Frame> {
        let mut frame = Frame::default();
        let mut own = SubFrame::default();
        let mut extra = Vec::new();
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "DELAY" => frame.delay_ms = child.as_int().unwrap_or(0).max(0) as u32,
                "KEYFRAME" => frame.keyframe = child.as_bool().unwrap_or(false),
                "MOVE" => {
                    let v = ints(child);
                    frame.move_x = get(&v, 0, 0);
                    frame.move_y = get(&v, 1, 0);
                }
                "SOUND" => frame.sound = child.as_str().map(str::to_string),
                "KILL_SOUND" => frame.kill_sound = child.as_bool().unwrap_or(false),
                "APPLY_EVENT" => frame
                    .apply_events
                    .extend(child.as_str().map(str::to_string)),
                "SUBFRAME" => extra.push(SubFrame::from_node(child)),
                _ => own.apply(child),
            }
        }
        frame.subframes.push(own);
        frame.subframes.extend(extra);
        Ok(frame)
    }
}

impl SubFrame {
    fn from_node(node: &Node) -> SubFrame {
        let mut sub = SubFrame::default();
        for child in node.nodes() {
            sub.apply(child);
        }
        sub
    }

    /// Applies one sub-frame key; unknown keys are ignored like the engine
    /// does.
    fn apply(&mut self, node: &Node) {
        match node.key.to_ascii_uppercase().as_str() {
            "IMAGE" => self.image = node.as_str().map(str::to_string),
            "TRANSPARENT" => {
                let v = ints(node);
                self.color_key = Rgb {
                    r: channel(&v, 0, 255),
                    g: channel(&v, 1, 255),
                    b: channel(&v, 2, 255),
                };
                self.custom_color_key = true;
            }
            "RECT" => {
                let v = ints(node);
                let rect = Rect {
                    left: get(&v, 0, 0),
                    top: get(&v, 1, 0),
                    right: get(&v, 2, 0),
                    bottom: get(&v, 3, 0),
                };
                self.rect = (!rect.is_empty()).then_some(rect);
            }
            "HOTSPOT" => {
                let v = ints(node);
                self.hotspot_x = get(&v, 0, 0);
                self.hotspot_y = get(&v, 1, 0);
            }
            "ALPHA" => self.tint.a = node.as_int().map_or(255, clamp_channel),
            "ALPHA_COLOR" => {
                let v = ints(node);
                self.tint.r = channel(&v, 0, 255);
                self.tint.g = channel(&v, 1, 255);
                self.tint.b = channel(&v, 2, 255);
            }
            "MIRROR_X" => self.mirror_x = node.as_bool().unwrap_or(false),
            "MIRROR_Y" => self.mirror_y = node.as_bool().unwrap_or(false),
            "DECORATION" => self.decoration = node.as_bool().unwrap_or(false),
            "2D_ONLY" => self.only_2d = node.as_bool().unwrap_or(false),
            "3D_ONLY" => self.only_3d = node.as_bool().unwrap_or(false),
            _ => {}
        }
    }
}

fn ints(node: &Node) -> Vec<i64> {
    node.as_ints().unwrap_or_default()
}

fn get(values: &[i64], index: usize, default: i32) -> i32 {
    values.get(index).map_or(default, |&v| {
        v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    })
}

fn channel(values: &[i64], index: usize, default: u8) -> u8 {
    values.get(index).map_or(default, |&v| clamp_channel(v))
}

fn clamp_channel(v: i64) -> u8 {
    v.clamp(0, 255) as u8
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
    fn rejects_files_without_a_sprite_block() {
        let err = Sprite::parse(b"SCENE { }").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
        let err = Sprite::parse(b"SPRITE { FRAME {").unwrap_err();
        assert!(matches!(err, Error::Syntax(_)), "{err}");
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
