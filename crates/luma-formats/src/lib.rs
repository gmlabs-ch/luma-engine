//! Parsers for game data formats: text definition files, sprites, scenes,
//! entities, fonts, windows and images.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! # Overview
//!
//! - [`def`]: the generic text syntax shared by every definition file
//!   (`default.game`, `.scene`, `.sprite`, `.window`, `.font`, ...),
//!   documented in `docs/formats/definition-files.md`.
//! - [`sprite`]: animated images (`.sprite`), documented in
//!   `docs/formats/sprite.md`.
//! - [`scene`]: rooms made of layers, entities and regions (`.scene`,
//!   `.entity`), documented in `docs/formats/scene.md`.
//! - [`actor`]: walking characters with a sprite per direction
//!   (`.actor`), documented in `docs/formats/actor.md`.
//! - [`font`]: bitmap and TrueType fonts (`.font`), documented in
//!   `docs/formats/font.md`.
//! - [`window`]: windows and their controls (`.window`), tiled images,
//!   the inventory box and the items, documented in
//!   `docs/formats/window.md`.
//! - [`game`] and [`strings`]: the game definition and its string tables,
//!   documented in `docs/formats/game.md`.
//! - [`image`]: decoding of PNG, BMP, JPEG and TGA files to RGBA with the
//!   engine's colour key rules.
//!
//! Resolving file names (through packages and loose files) is left to the
//! caller; this crate works on bytes.

#![forbid(unsafe_code)]

pub mod actor;
pub mod def;
mod error;
pub mod font;
pub mod game;
pub mod image;
pub mod scene;
pub mod sprite;
pub mod strings;
pub mod window;

pub use actor::{Actor, SpriteSet};
pub use def::{Document, Node, Nodes, ParseError, Scalar, Value};
pub use error::{Error, Result};
pub use font::{
    BitmapFontDef, BitmapLayout, BitmapLine, FontFile, FontLayer, TextAlign, TrueTypeFontDef,
};
pub use game::GameFile;
pub use image::{DEFAULT_COLOR_KEY, Image, Rgb, Rgba};
pub use scene::{
    Direction, Entity, EntityKind, Layer, Point, Property, Region, RotationLevel, ScaleLevel,
    Scene, SceneNode, WaypointGroup,
};
pub use sprite::{Frame, Rect, Sprite, SubFrame};
pub use strings::StringTable;
pub use window::{
    ButtonDef, ControlDef, EditDef, InventoryBoxDef, ItemDef, StaticDef, TiledImage, VerticalAlign,
    WidgetDef, WindowDef,
};
