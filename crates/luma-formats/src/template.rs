//! Reading the files a definition file names with `TEMPLATE`.
//!
//! Written with reference to the MIT-licensed Wintermute Engine sources
//! (the `LoadFile` and `LoadBuffer` methods of its classes, Copyright (c)
//! 2011 Jan Nedoma). See `NOTICE` at the root of the repository.
//!
//! In the original engine a `TEMPLATE` key calls the object's own
//! `LoadFile` on the spot: the keys of the named file are applied to the
//! object where the key stands, so the keys written before it are
//! overridden by the template and the keys after it override the
//! template. The loaders of this crate do the same through [`Files`],
//! which the caller builds over its way of reading a game file.
//!
//! The original has no bound: a file naming itself recurses until the
//! stack is gone. [`Files`] stops a chain at [`TEMPLATE_DEPTH`] files and
//! a whole load at [`TEMPLATE_LOADS`] files, so no game file can make the
//! engine read without end.

use crate::def::Document;
use crate::error::{Error, Result};

/// How many `TEMPLATE` files may be chained (a template naming a
/// template).
pub const TEMPLATE_DEPTH: u32 = 8;

/// How many `TEMPLATE` files one load may read in all.
pub const TEMPLATE_LOADS: u32 = 1024;

/// Reads a game file by name; `None` when there is no such file.
pub type ReadFile<'a> = dyn FnMut(&str) -> Option<Vec<u8>> + 'a;

/// The files a loader may read while it applies a definition: a reader,
/// and what is left of the bounds.
pub struct Files<'a> {
    read: Option<&'a mut ReadFile<'a>>,
    depth: u32,
    left: u32,
}

impl<'a> Files<'a> {
    /// Files read with `read`, which returns `None` for a file that does
    /// not exist.
    pub fn new(read: &'a mut ReadFile<'a>) -> Files<'a> {
        Files {
            read: Some(read),
            depth: 0,
            left: TEMPLATE_LOADS,
        }
    }

    /// No file at all: every `TEMPLATE` fails to load.
    pub fn none() -> Files<'static> {
        Files {
            read: None,
            depth: 0,
            left: 0,
        }
    }

    /// Reads and parses the file `name` (the first half of `LoadFile`).
    /// The error says why there is no document: the bounds are spent, the
    /// file is missing, or it is not a definition file.
    pub fn open(&mut self, name: &str) -> Result<Document> {
        if self.depth >= TEMPLATE_DEPTH || self.left == 0 {
            return Err(Error::Invalid {
                reason: format!("too many TEMPLATE files, '{name}' is not read"),
            });
        }
        let bytes = self
            .read
            .as_mut()
            .and_then(|read| read(name))
            .ok_or_else(|| Error::Invalid {
                reason: format!("TEMPLATE file '{name}' cannot be read"),
            })?;
        self.left -= 1;
        Ok(Document::parse(&bytes)?)
    }

    /// Runs `apply` one level deeper in the chain of templates.
    pub fn nested<T>(&mut self, apply: impl FnOnce(&mut Files<'_>) -> T) -> T {
        self.depth += 1;
        let result = apply(self);
        self.depth -= 1;
        result
    }

    /// Reads and parses the file `name`, and runs `apply` on it one level
    /// deeper in the chain (`LoadFile`). The error is that of the file
    /// (missing, over the bounds, not a definition file) or of `apply`.
    pub fn template<T>(
        &mut self,
        name: &str,
        apply: impl FnOnce(&Document, &mut Files<'_>) -> Result<T>,
    ) -> Result<T> {
        let doc = self.open(name)?;
        self.nested(|files| apply(&doc, files))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_stops_at_the_depth_bound() {
        let mut reads = 0;
        let mut read = |_: &str| {
            reads += 1;
            Some(b"X { TEMPLATE=\"self\" }".to_vec())
        };
        let mut files = Files::new(&mut read);
        fn chain(files: &mut Files<'_>) -> Result<()> {
            files.template("self", |_, files| chain(files))
        }
        assert!(chain(&mut files).is_err());
        assert_eq!(reads, TEMPLATE_DEPTH);
    }

    #[test]
    fn a_load_stops_at_the_file_bound() {
        let mut reads = 0;
        let mut read = |_: &str| {
            reads += 1;
            Some(b"X { }".to_vec())
        };
        let mut files = Files::new(&mut read);
        let mut ok = 0;
        for _ in 0..TEMPLATE_LOADS + 10 {
            if files.template("t", |_, _| Ok(())).is_ok() {
                ok += 1;
            }
        }
        assert_eq!(ok, TEMPLATE_LOADS);
        assert_eq!(reads, TEMPLATE_LOADS);
    }

    #[test]
    fn no_file_reads_nothing() {
        assert!(Files::none().template("t", |_, _| Ok(())).is_err());
    }
}
