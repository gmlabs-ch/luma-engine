//! Error type of the crate.

use std::fmt;

use crate::def::ParseError;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors raised while parsing definition files or decoding images.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The definition file has a syntax error.
    Syntax(ParseError),
    /// The definition file is well-formed but not what was expected
    /// (for example, no `SPRITE` root block).
    Invalid {
        /// What was wrong.
        reason: String,
    },
    /// An image could not be decoded.
    Image {
        /// What was wrong.
        reason: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Syntax(err) => write!(f, "syntax error at {err}"),
            Error::Invalid { reason } => write!(f, "invalid definition: {reason}"),
            Error::Image { reason } => write!(f, "cannot decode image: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Syntax(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ParseError> for Error {
    fn from(err: ParseError) -> Self {
        Error::Syntax(err)
    }
}
