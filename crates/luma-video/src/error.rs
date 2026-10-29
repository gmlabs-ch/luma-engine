//! Error type of the crate.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors raised while reading or decoding a video.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The Ogg container is damaged or ends early.
    Container(String),
    /// The file holds no Theora stream.
    NoVideoStream,
    /// The Theora decoder rejected the stream.
    Decode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Container(reason) => write!(f, "cannot read video container: {reason}"),
            Error::NoVideoStream => write!(f, "no Theora video stream in the file"),
            Error::Decode(reason) => write!(f, "cannot decode video: {reason}"),
        }
    }
}

impl std::error::Error for Error {}
