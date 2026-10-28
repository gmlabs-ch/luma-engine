//! Error type of the crate.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors raised while setting up the GPU, a window or a render target.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// No graphics adapter accepts the requested backends.
    NoAdapter {
        /// What the adapter request said.
        reason: String,
    },
    /// The adapter refused to create a device.
    Device {
        /// What the device request said.
        reason: String,
    },
    /// The window system refused to create a window or a surface.
    Window {
        /// What went wrong.
        reason: String,
    },
    /// A frame could not be rendered (surface lost, validation error).
    Render {
        /// What went wrong.
        reason: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoAdapter { reason } => write!(f, "no graphics adapter: {reason}"),
            Error::Device { reason } => write!(f, "cannot create a graphics device: {reason}"),
            Error::Window { reason } => write!(f, "cannot create a window: {reason}"),
            Error::Render { reason } => write!(f, "cannot render: {reason}"),
        }
    }
}

impl std::error::Error for Error {}
