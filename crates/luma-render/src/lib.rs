//! 2D renderer built on wgpu (Metal on Apple platforms, Vulkan, DirectX 12
//! or OpenGL ES elsewhere).
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! # Overview
//!
//! - [`Gpu`]: the device and its queue, with or without a window.
//! - [`Texture`]: a decoded image uploaded to the GPU.
//! - [`DrawList`]: the textured quads of a frame, back to front, in logical
//!   pixels, with a per-layer offset for scrolling.
//! - [`Renderer`]: draws a list into any [`Target`] with alpha blending,
//!   batching consecutive quads of the same texture.
//! - [`Offscreen`]: a target whose pixels can be read back (tests,
//!   screenshots).
//! - [`window::run`]: a winit window with a swap chain, HiDPI aware, that
//!   calls an [`App`] every frame.
//!
//! Blending happens in gamma space, like the original engine: textures are
//! `Rgba8Unorm` and the surface uses a non-sRGB format when one exists.
//!
//! See `docs/rendering.md` for the design.

#![forbid(unsafe_code)]

pub mod draw;
mod error;
pub mod gpu;
pub mod offscreen;
pub mod renderer;
pub mod texture;
pub mod window;

pub use draw::{DrawList, Quad, Rect};
pub use error::{Error, Result};
pub use gpu::Gpu;
pub use offscreen::Offscreen;
pub use renderer::{Renderer, Target, letterbox};
pub use texture::{TEXTURE_FORMAT, Texture};
pub use window::{App, Flow, Frame, WindowConfig};

/// Re-export of the windowing crate, for event types.
pub use winit;
