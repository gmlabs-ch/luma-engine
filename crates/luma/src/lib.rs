//! Luma Engine, in one crate.
//!
//! The engine is a light core and optional modules, in the manner of
//! Bevy: a game depends on this crate and enables the modules it needs
//! through Cargo features, so that it only pays for what it uses. The
//! core is always there, as the modules below:
//!
//! - [`archive`]: game packages (`.dcp`) and the virtual file system;
//! - [`formats`]: the definition files, sprites, scenes, actors, fonts
//!   and images;
//! - [`script`]: the virtual machine for compiled game scripts;
//! - [`render`]: the 2D renderer on wgpu and the window;
//! - [`audio`]: music, sound effects and voice;
//! - [`video`]: Ogg Theora videos.
//!
//! The optional modules, each behind a feature of the same name:
//!
//! - `adventure` (on by default): [`adventure`], the point-and-click
//!   adventure game as Wintermute Engine runs it, with actors, scenes,
//!   speech, inventory, pathfinding and saved games.
//!
//! Two more features configure the core: `audio-device` plays the sounds
//! on the default output device (without it they run silently, on the
//! game clock, which the tests and the headless player rely on), and
//! `serde` serializes the core's definitions and the script engine.
//!
//! A front-end such as `luma-player` depends on `luma` with the features
//! it needs; a module of another genre is a crate that depends on `luma`
//! without `adventure` and implements the script engine's [`script::Host`]
//! over its own world, as `luma-adventure` does. See `docs/modules.md`.

pub use luma_archive as archive;
pub use luma_audio as audio;
pub use luma_formats as formats;
pub use luma_render as render;
pub use luma_script as script;
pub use luma_video as video;

#[cfg(feature = "adventure")]
pub use luma_adventure as adventure;

#[cfg(test)]
mod tests {
    /// A type of each core crate, by its path through this crate.
    #[allow(dead_code)]
    fn core_types(
        _: crate::archive::Vfs,
        _: crate::formats::Rgba,
        _: crate::script::Value,
        _: crate::render::DrawList,
        _: crate::audio::SoundType,
        _: crate::video::Video,
    ) {
    }

    #[test]
    fn the_core_is_reachable_through_the_crate() {
        assert!(matches!(
            crate::script::Value::Null,
            crate::script::Value::Null
        ));
    }

    #[cfg(feature = "adventure")]
    #[test]
    fn the_adventure_module_is_reachable_with_its_feature() {
        assert_eq!(crate::adventure::SAVE_EXTENSION, "dsv");
    }
}
