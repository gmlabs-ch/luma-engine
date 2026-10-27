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
//!
//! Typed views of each file kind (sprites, scenes, fonts) will build on
//! [`def`] in later milestones.

#![forbid(unsafe_code)]

pub mod def;

pub use def::{Document, Node, Nodes, ParseError, Scalar, Value};
