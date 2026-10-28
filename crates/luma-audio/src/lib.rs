//! Audio playback for music, sound effects and voice.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! # Overview
//!
//! - [`Clip`] and [`decode`]: WAV and Ogg Vorbis files decoded by
//!   Symphonia.
//! - [`Audio`]: the sounds loaded, with the engine's rules for volumes
//!   (per type, per sound, master), panning, looping with a loop start,
//!   pausing and positions, mixed by Kira. With the `device` feature the
//!   output goes to the default device through `cpal`; otherwise, or
//!   when no device is there, the sounds advance by game time, so that
//!   durations and `IsPlaying` behave the same in headless runs and on
//!   continuous integration.
//!
//! Reading the files (through the game's packages) is left to the
//! caller; this crate works on bytes. The behaviour is documented in
//! `docs/audio.md`.

#![forbid(unsafe_code)]

mod audio;
mod clip;
mod error;

pub use audio::{Audio, SoundId, SoundType, gain};
pub use clip::{Clip, decode, silent_wav};
pub use error::{Error, Result};
