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
//! - [`Font`] and [`FontBank`]: bitmap and TrueType fonts, and text drawn
//!   with them; [`Sentence`]: what an object says (`Talk`).
//! - [`Sounds`]: the objects' sound effects, the music channels and the
//!   speech of sentences, played through `luma_audio`.
//! - [`Mouse`], [`Keyboard`], [`Cursors`] and [`Fader`]: the input scripts
//!   see, the object under the cursor, the cursor drawn, and the screen
//!   fades; the [`Runtime`] routes the events to the scripts.
//! - [`Control`] and [`Ui`]: windows, buttons, static texts and editors
//!   drawn over the scene, their focus, modes and freezing of the game.
//! - [`World`]: the game as scripts see it (`Game`, `Scene`, layers,
//!   entities, regions, sprites, actors), the script engine's host.
//! - [`Runtime`]: a script engine and a world, driven once per frame.
//!
//! The inventory and saved games come with the later tickets of the
//! milestone.

#![forbid(unsafe_code)]

pub mod actor;
mod error;
pub mod fader;
pub mod input;
mod natives;
mod natives_actor;
mod natives_sound;
mod natives_ui;
pub mod objects;
pub mod path;
pub mod scene_view;
pub mod settings;
pub mod sound;
pub mod talk;
pub mod text;
pub mod ui;
pub mod world;

pub use actor::{Actor, ActorEvent, ActorState, WalkContext};
pub use error::{Error, Result};
pub use fader::{Fader, SCENE_FADE_MS};
pub use input::{Cursors, Keyboard, Mouse};
pub use luma_audio::Audio;
pub use objects::{Kind, Native, Objects};
pub use path::Floor;
pub use scene_view::{
    AnimState, Camera, DrawItem, Drawable, EntityRef, EntityTalk, LoadedSprite, Mask, SceneView,
    SpriteBank, draw_object, frame_height, place,
};
pub use settings::{GameSettings, SETTINGS_FILE};
pub use sound::{MUSIC_CHANNELS, MusicChannel, ObjectSfx, ObjectSound, Sounds};
pub use talk::{Sentence, SentenceDraw, SubtitleMods, TalkSkipButton};
pub use text::{BitmapFont, Font, FontBank, TrueTypeFont};
pub use ui::{
    ButtonState, Control, ControlKind, EditState, StaticState, Ui, WindowMode, WindowState,
};
pub use world::{Runtime, World};
