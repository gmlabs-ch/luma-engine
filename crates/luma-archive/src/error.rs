//! Error type of the crate.

use std::fmt;
use std::io;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors raised while opening packages or reading files.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An I/O error while reading a package or a loose file.
    Io(io::Error),
    /// The file does not start with the package signature.
    NotAPackage {
        /// Path or label of the file.
        source: String,
    },
    /// The package uses a format version newer than this crate supports.
    UnsupportedVersion {
        /// Path or label of the package.
        source: String,
        /// Version number found in the header.
        version: u32,
    },
    /// The package is truncated or its directory table is inconsistent.
    Corrupt {
        /// Path or label of the package.
        source: String,
        /// What was wrong.
        reason: String,
    },
    /// A compressed file could not be inflated, or inflated to the wrong size.
    Decompress {
        /// Name of the file being read.
        name: String,
        /// What was wrong.
        reason: String,
    },
    /// The requested file exists in no package and no directory.
    NotFound {
        /// Name of the requested file.
        name: String,
    },
    /// The file name is not acceptable (empty, absolute or containing `..`).
    InvalidName {
        /// The offending name.
        name: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "I/O error: {err}"),
            Error::NotAPackage { source } => write!(f, "{source}: not a WME package"),
            Error::UnsupportedVersion { source, version } => {
                write!(f, "{source}: unsupported package version {version:#x}")
            }
            Error::Corrupt { source, reason } => write!(f, "{source}: corrupt package: {reason}"),
            Error::Decompress { name, reason } => {
                write!(f, "{name}: cannot decompress: {reason}")
            }
            Error::NotFound { name } => write!(f, "{name}: file not found"),
            Error::InvalidName { name } => write!(f, "{name:?}: invalid file name"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}
