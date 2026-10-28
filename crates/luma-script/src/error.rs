//! Error type of the crate.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors raised while loading or decoding a compiled script.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The data does not start with the script magic.
    NotAScript {
        /// The first four bytes, as a little-endian value.
        magic: u32,
    },
    /// The script's version is newer than the one this crate understands.
    UnsupportedVersion {
        /// The version found in the header.
        version: u32,
    },
    /// The data ends before the item at `offset` is complete.
    Truncated {
        /// Where the item starts.
        offset: usize,
        /// What was being read.
        what: &'static str,
    },
    /// A value is out of range or otherwise inconsistent.
    Invalid {
        /// Where the value is.
        offset: usize,
        /// What was wrong.
        reason: String,
    },
    /// An instruction has an opcode this crate does not know.
    UnknownOpcode {
        /// Where the instruction starts.
        offset: usize,
        /// The opcode found.
        opcode: u32,
    },
    /// An instruction refers to a symbol outside the symbol table.
    SymbolIndex {
        /// Where the instruction starts.
        offset: usize,
        /// The index found.
        index: u32,
        /// The number of symbols in the table.
        count: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotAScript { magic } => {
                write!(f, "not a compiled script (magic {magic:#010x})")
            }
            Error::UnsupportedVersion { version } => write!(
                f,
                "unsupported script version {}.{}",
                version >> 8,
                version & 0xff
            ),
            Error::Truncated { offset, what } => {
                write!(f, "truncated script: {what} at offset {offset}")
            }
            Error::Invalid { offset, reason } => {
                write!(f, "invalid script at offset {offset}: {reason}")
            }
            Error::UnknownOpcode { offset, opcode } => {
                write!(f, "unknown opcode {opcode} at offset {offset}")
            }
            Error::SymbolIndex {
                offset,
                index,
                count,
            } => write!(
                f,
                "symbol index {index} out of range (the table has {count}) at offset {offset}"
            ),
        }
    }
}

impl std::error::Error for Error {}
