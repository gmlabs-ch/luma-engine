//! The adventure layer: the game world scripts run in, scenes, and (in
//! later milestones) actors, inventory, dialogue, pathfinding and saved
//! games.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! # Status
//!
//! Milestone 3, in progress: a game's scripts run against the objects of
//! the engine.
//!
//! - [`GameSettings`]: the game's resolution from `startup.settings`.
//! - [`SceneView`]: a scene with its sprites and the animation state of
//!   its entities, composed into a `luma_render::DrawList` the way the
//!   engine draws it, seen through a [`Camera`].
//! - [`Actor`]: a walking character, with its path finding in [`path`].
//! - [`World`]: the game as scripts see it (`Game`, `Scene`, layers,
//!   entities, regions, sprites, actors), the script engine's host.
//! - [`Runtime`]: a script engine and a world, driven once per frame.
//!
//! Text, sound, inventory, windows and saved games come with the later
//! tickets of the milestone.

#![forbid(unsafe_code)]

pub mod actor;
mod error;
mod natives;
mod natives_actor;
pub mod objects;
pub mod path;
pub mod scene_view;
pub mod settings;
pub mod world;

pub use actor::{Actor, ActorEvent, ActorState, WalkContext};
pub use error::{Error, Result};
pub use objects::{Kind, Native, Objects};
pub use path::Floor;
pub use scene_view::{
    AnimState, Camera, Drawable, EntityRef, LoadedSprite, SceneView, SpriteBank, draw_object,
    frame_height, place,
};
pub use settings::{GameSettings, SETTINGS_FILE};
pub use world::{Runtime, World};
