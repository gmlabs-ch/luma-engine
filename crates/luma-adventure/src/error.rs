//! Error type of the crate.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors raised while loading a game or a scene.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The game's packages or files could not be read.
    Archive(luma_archive::Error),
    /// A definition file or image is invalid.
    Format {
        /// The file concerned.
        path: String,
        /// The parser's error.
        source: luma_formats::Error,
    },
    /// The renderer failed.
    Render(luma_render::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Archive(err) => write!(f, "{err}"),
            Error::Format { path, source } => write!(f, "{path}: {source}"),
            Error::Render(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Archive(err) => Some(err),
            Error::Format { source, .. } => Some(source),
            Error::Render(err) => Some(err),
        }
    }
}

impl From<luma_archive::Error> for Error {
    fn from(err: luma_archive::Error) -> Self {
        Error::Archive(err)
    }
}

impl From<luma_render::Error> for Error {
    fn from(err: luma_render::Error) -> Self {
        Error::Render(err)
    }
}
