//! Error type of the crate.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors raised while decoding or playing sound.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The file ends before its headers or data do.
    Truncated,
    /// The file is of a kind or variant that is not supported.
    Unsupported(String),
    /// The decoder rejected the file.
    Decode(String),
    /// The audio device could not be opened.
    Device(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated => write!(f, "truncated sound file"),
            Error::Unsupported(what) => write!(f, "unsupported sound: {what}"),
            Error::Decode(reason) => write!(f, "cannot decode sound: {reason}"),
            Error::Device(reason) => write!(f, "audio device: {reason}"),
        }
    }
}

impl std::error::Error for Error {}
