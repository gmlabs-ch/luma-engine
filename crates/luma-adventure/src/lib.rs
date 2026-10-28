//! The adventure layer: actors, scenes, inventory, dialogue, pathfinding
//! and saved games.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! # Status
//!
//! Milestone 2: a scene can be loaded from a game and drawn without
//! scripts.
//!
//! - [`GameSettings`]: the game's resolution from `startup.settings`.
//! - [`SceneView`]: a scene with its sprites and textures, composed into
//!   a `luma_render::DrawList` the way the engine draws it, seen through
//!   a [`Camera`].
//!
//! Actors, inventory, dialogue, pathfinding and saved games come with the
//! script milestone.

#![forbid(unsafe_code)]

mod error;
pub mod scene_view;
pub mod settings;

pub use error::{Error, Result};
pub use scene_view::{Camera, LoadedSprite, SceneView, place};
pub use settings::{GameSettings, SETTINGS_FILE};
