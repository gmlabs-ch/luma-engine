//! Error type of the crate.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors raised while reading or decoding a video.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The container (Ogg or AVI) is damaged or ends early.
    Container(String),
    /// The file holds no video stream.
    NoVideoStream,
    /// The decoder rejected the stream or a frame of it.
    Decode(String),
    /// The stream is coded with a codec Luma does not decode; the text
    /// names it (a FourCC for pictures, a format tag for sound).
    UnsupportedCodec(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Container(reason) => write!(f, "cannot read video container: {reason}"),
            Error::NoVideoStream => write!(f, "no video stream in the file"),
            Error::Decode(reason) => write!(f, "cannot decode video: {reason}"),
            Error::UnsupportedCodec(codec) => write!(f, "unsupported codec: {codec}"),
        }
    }
}

impl std::error::Error for Error {}
