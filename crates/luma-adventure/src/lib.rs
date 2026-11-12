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
//! - [`Item`] and [`Inventory`]: the items, those taken, the box that
//!   shows them and the cursor of the item in use.
//! - [`VideoPlayer`]: a video played by `Game.PlayTheora` over the frozen
//!   game, with its sound and subtitles.
//! - [`SceneState`]: what a persistent scene remembers of its nodes
//!   between visits.
//! - [`Emitter`]: a particle emitter attached to an entity or an actor
//!   (`CreateParticleEmitter`), its particles and forces.
//! - [`World`]: the game as scripts see it (`Game`, `Scene`, layers,
//!   entities, regions, sprites, actors), the script engine's host.
//! - [`Runtime`]: a script engine and a world, driven once per frame.
//! - [`save`]: saved games, the whole engine written to a slot and read
//!   back (`Game.SaveGame`, `Game.LoadGame`).
//! - [`registry`]: the private settings a game keeps between runs
//!   (`Game.RegReadNumber`, `RegWriteString`...).
//! - [`capture`]: screenshots and save thumbnails, captured from the
//!   frame the front-end renders next.
//! - [`accessibility`]: text-to-speech of captions, speech and typed
//!   keys, and keyboard-only play (`Game.Acc*`, `AccCaption`).

#![forbid(unsafe_code)]

pub mod accessibility;
pub mod actor;
#[cfg(feature = "3d")]
pub mod actor3d;
pub mod capture;
#[cfg(feature = "3d")]
pub mod draw3d;
mod error;
pub mod fader;
pub mod font_face;
pub mod input;
pub mod inventory;
mod natives;
mod natives_actor;
#[cfg(feature = "3d")]
mod natives_actor3d;
mod natives_item;
mod natives_object;
mod natives_particles;
mod natives_region;
mod natives_sound;
mod natives_sprite;
mod natives_ui;
pub mod objects;
pub mod particles;
pub mod path;
pub mod registry;
pub mod responses;
pub mod save;
#[cfg(feature = "3d")]
pub mod scene3d;
mod scene_edit;
pub mod scene_state;
pub mod scene_view;
pub mod settings;
pub mod sound;
pub mod talk;
#[cfg(test)]
mod test_font;
pub mod text;
#[cfg(not(feature = "3d"))]
mod three_d_off;
pub mod ui;
pub mod video;
pub mod world;

#[cfg(not(feature = "3d"))]
use three_d_off::{actor3d, draw3d, scene3d};

pub use accessibility::{
    Accessibility, CommandSpeech, RecordingSpeech, Speech, SpeechKind, SpeechRecord,
};
pub use actor::{Actor, ActorEvent, ActorState, WalkContext};
#[cfg(feature = "3d")]
pub use actor3d::{
    Actor3d, Actor3dDraw, Actor3dState, Attachment3d, AttachmentDraw, MaterialOverride,
    MaterialTexture, ThreeDFrame,
};
pub use error::{Error, Result};
pub use fader::{Fader, SCENE_FADE_MS};
pub use input::{Clipboard, Cursors, Keyboard, MemoryClipboard, Mouse};
pub use inventory::{Inventory, Item};
pub use luma_audio::Audio;
pub use objects::{Attachment, CursorSlot, Kind, Native, ObjectInventory, ObjectState, Objects};
pub use particles::Emitter;
pub use path::Floor;
pub use responses::{Response, ResponseKind, Responses};
pub use save::{SAVE_EXTENSION, SaveHeader, read_save_header};
pub use scene_state::{NodeState, SceneState, state_key};
pub use scene_view::{
    AnimState, AttachedDraws, Camera, DrawItem, Drawable, EntityRef, EntityTalk, FreeObject,
    LoadedSprite, Mask, Overlays, SceneView, SpriteBank, Transform, draw_object, drawn_subframes,
    frame_height, is_drawn, place, place_transformed,
};
#[cfg(feature = "3d")]
pub use scene3d::{Fog, Scene3d};
pub use settings::{GameSettings, SETTINGS_FILE};
pub use sound::{MUSIC_CHANNELS, MusicChannel, ObjectSfx, ObjectSound, SfxEffect, Sounds};
pub use talk::{Sentence, SentenceDraw, SubtitleMods, TalkSkipButton};
pub use text::{BitmapFont, Font, FontBank, TrueTypeFont};
pub use ui::{
    ButtonListener, ButtonState, Control, ControlKind, EditState, ImageSlot, StaticState, Ui,
    WindowMode, WindowState,
};
pub use video::{Subtitle, Subtitler, VideoPlayback, VideoPlayer, parse_subtitles};
pub use world::{FIXED_SEED, LoadingIcon, QuickMessage, Runtime, ScreenImage, World};
