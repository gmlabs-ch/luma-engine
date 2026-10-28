//! Audio playback for music, sound effects and voice.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! # Overview
//!
//! - [`decode`]: WAV and Ogg Vorbis files decoded to 16-bit PCM
//!   [`Clip`]s.
//! - [`Audio`]: the sounds loaded, with the engine's rules for volumes
//!   (per type, per sound, master), panning, looping with a loop start,
//!   pausing and positions. Output goes to the default device through
//!   `cpal`; without a device, the sounds advance by game time, so that
//!   durations and `IsPlaying` behave the same in headless runs and on
//!   continuous integration.
//!
//! Reading the files (through the game's packages) is left to the
//! caller; this crate works on bytes. The behaviour is documented in
//! `docs/audio.md`.

#![forbid(unsafe_code)]

mod audio;
pub mod decode;
mod error;
mod mixer;

pub use audio::Audio;
pub use decode::{Clip, decode, encode_wav};
pub use error::{Error, Result};
pub use mixer::{SoundId, SoundType, gain};
