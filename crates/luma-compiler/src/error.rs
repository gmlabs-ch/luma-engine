//! Compile errors, located in a file.

use std::fmt;

/// A position in a source file, for error messages: the file (the main
/// file or an include, by the name it was opened with), a 1-based line
/// and a 1-based column in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pos {
    /// Index of the file in [`crate::Script::files`].
    pub file: usize,
    /// Line, from 1.
    pub line: u32,
    /// Column, from 1, counted in bytes.
    pub column: u32,
}

/// A compile error: the first one met, with where it was met. The
/// messages are those of the original compiler where it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The file: the name given to the compiler for the main file, the
    /// name written in the `#include` directive for an included one.
    pub file: String,
    /// Line, from 1 (0 when the error has no position).
    pub line: u32,
    /// Column, from 1, in bytes (0 when unknown).
    pub column: u32,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.file, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for Error {}

/// The result of a compiler phase.
pub type Result<T> = std::result::Result<T, Error>;
