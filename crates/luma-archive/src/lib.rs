//! Reading of Wintermute Engine game packages (`.dcp`) and a virtual file
//! system that layers packages and loose files.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! The package format is documented in `docs/formats/dcp.md` in the
//! repository. In short, a package is a flat archive: a 128-byte header,
//! the file data (raw or zlib-compressed), and a directory table listing
//! every file with its offset, sizes and timestamp.
//!
//! # Overview
//!
//! - [`Package`] opens a single package and reads its entries.
//! - [`Vfs`] layers several packages and loose directories the way the
//!   original engine does: loose files shadow packaged ones, and when two
//!   packages contain the same file, the one on the lowest disc number or
//!   with the highest priority wins.
//! - [`PackageBuilder`] writes packages in the same layout as the engine's
//!   project manager, for tests and tooling.
//!
//! # Example
//!
//! ```no_run
//! use luma_archive::Vfs;
//!
//! let vfs = Vfs::open_game("path/to/game")?;
//! let game = vfs.read("default.game")?;
//! println!("{} bytes", game.len());
//! # Ok::<(), luma_archive::Error>(())
//! ```

#![forbid(unsafe_code)]

mod builder;
mod error;
mod package;
mod vfs;

pub use builder::PackageBuilder;
pub use error::{Error, Result};
pub use package::{
    CURRENT_VERSION, Directory, Entry, Header, MAGIC_1, MAGIC_2, Package, normalize_name,
};
pub use vfs::{PackagedFile, Vfs};
