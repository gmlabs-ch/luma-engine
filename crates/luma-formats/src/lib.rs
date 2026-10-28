//! Parsers for game data formats: text definition files, sprites, scenes,
//! entities, fonts and images.
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
//! - [`image`]: decoding of PNG, BMP, JPEG and TGA files to RGBA with the
//!   engine's colour key rules.
//!
//! Resolving file names (through packages and loose files) is left to the
//! caller; this crate works on bytes.

#![forbid(unsafe_code)]

pub mod def;
mod error;
pub mod image;
pub mod scene;
pub mod sprite;

pub use def::{Document, Node, Nodes, ParseError, Scalar, Value};
pub use error::{Error, Result};
pub use image::{DEFAULT_COLOR_KEY, Image, Rgb, Rgba};
pub use scene::{
    Direction, Entity, EntityKind, Layer, Point, Property, Region, RotationLevel, ScaleLevel,
    Scene, SceneNode, WaypointGroup,
};
pub use sprite::{Frame, Rect, Sprite, SubFrame};
