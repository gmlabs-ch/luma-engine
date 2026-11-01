//! Lumen, the scenario format of Luma Engine: a plain-text file that reads
//! like a screenplay, with knots and stitches for the flow (`== knot`,
//! `= stitch`, `-> target`), bracketed speakers for dialogue
//! (`[arfwedson] text`), `@` stage directions, `?` choices, `~` logic and
//! `if` blocks.
//!
//! This crate is the first slice of the toolchain: the [`parse`]r, the
//! [`check`]er and a [`run`]ner that plays a story as text. The format is
//! described in `docs/lumen.md`.

pub mod ast;
pub mod check;
pub mod diag;
pub mod expr;
pub mod parse;
pub mod run;
pub mod text;

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

pub use ast::Story;
pub use diag::{Diagnostic, Severity};
pub use run::{ChoiceView, Event, Runner, RuntimeError};

/// The file extension, without the dot.
pub const EXTENSION: &str = "lumen";

/// The manifest's file name, next to the story's files.
pub const MANIFEST: &str = "lumen.toml";

/// Why a story could not be loaded at all (problems inside the files are
/// diagnostics, not errors).
#[derive(Debug)]
pub enum LoadError {
    /// A file or folder could not be read.
    Io(PathBuf, io::Error),
    /// The folder holds no `.lumen` file.
    NoFiles(PathBuf),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(path, err) => write!(f, "{}: {err}", path.display()),
            LoadError::NoFiles(path) => write!(f, "{}: no .lumen file found", path.display()),
        }
    }
}

impl std::error::Error for LoadError {}

impl Story {
    /// Loads a story from a folder (every `.lumen` file under it, in path
    /// order, and the manifest `lumen.toml` when there is one) or from a
    /// single file and the files it includes. The diagnostics are the
    /// parser's; run [`check::check`] for the rest.
    pub fn load(path: &Path) -> Result<(Story, Vec<Diagnostic>), LoadError> {
        let root = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        };
        let mut files: Vec<(String, String)> = Vec::new();
        if path.is_dir() {
            let mut found = Vec::new();
            collect(path, &root, &mut found)?;
            found.sort();
            for relative in found {
                let text = read(&root.join(&relative))?;
                files.push((relative, text));
            }
            if files.is_empty() {
                return Err(LoadError::NoFiles(path.to_path_buf()));
            }
        } else {
            let relative = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push((relative, read(path)?));
        }
        let manifest = root.join(MANIFEST);
        let start = if manifest.is_file() {
            manifest_start(&read(&manifest)?)
        } else {
            None
        };
        let (mut story, diags) = Story::from_sources(files, |include| {
            let full = root.join(include);
            full.is_file().then(|| read(&full).ok()).flatten()
        });
        story.start = start;
        Ok((story, diags))
    }

    /// Builds a story from `(path, text)` pairs, the way [`Story::load`]
    /// does from disk; `include` fetches an included file that is not
    /// among them, by the path the `include` line gives.
    pub fn from_sources(
        files: Vec<(String, String)>,
        include: impl Fn(&str) -> Option<String>,
    ) -> (Story, Vec<Diagnostic>) {
        let mut story = Story::default();
        let mut diags = Vec::new();
        let mut queue: Vec<(String, String)> = files.into_iter().rev().collect();
        while let Some((path, text)) = queue.pop() {
            if story.files.iter().any(|f| f.path == path) {
                continue;
            }
            let index = story.files.len();
            story.files.push(ast::SourceFile {
                path: path.clone(),
                text: text.clone(),
            });
            let ast = parse::parse_file(index, &text, &mut diags);
            for (included, span) in ast.includes {
                let included = normalize(&included);
                if story.files.iter().any(|f| f.path == included)
                    || queue.iter().any(|(p, _)| *p == included)
                {
                    continue;
                }
                match include(&included) {
                    Some(text) => queue.push((included, text)),
                    None => diags.push(Diagnostic::error(
                        span,
                        format!("included file `{included}` not found"),
                    )),
                }
            }
            story.characters.extend(ast.characters);
            story.vars.extend(ast.vars);
            story.knots.extend(ast.knots);
            story.notes.extend(ast.notes);
        }
        (story, diags)
    }
}

fn read(path: &Path) -> Result<String, LoadError> {
    std::fs::read_to_string(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<(), LoadError> {
    let entries = std::fs::read_dir(dir).map_err(|e| LoadError::Io(dir.to_path_buf(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| LoadError::Io(dir.to_path_buf(), e))?;
        let path = entry.path();
        if path.is_dir() {
            collect(&path, root, out)?;
        } else if path.extension().is_some_and(|e| e == EXTENSION) {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            out.push(normalize(&relative.to_string_lossy()));
        }
    }
    Ok(())
}

/// Paths with `/`, as the `include` lines write them.
fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

/// `start = "knot"` in the manifest.
fn manifest_start(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "start").then(|| value.trim().trim_matches('"').to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_are_loaded_once_and_missing_ones_reported() {
        let (story, diags) = Story::from_sources(
            vec![(
                "a.lumen".to_string(),
                "include \"common/chars.lumen\"\ninclude \"missing.lumen\"\n== start\nHi.\n"
                    .to_string(),
            )],
            |path| {
                (path == "common/chars.lumen")
                    .then(|| "include \"a.lumen\"\ncharacter zif \"Zif\"\n".to_string())
            },
        );
        assert_eq!(story.files.len(), 2);
        assert_eq!(story.characters.len(), 1);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("missing.lumen"));
    }

    #[test]
    fn manifest_names_the_start() {
        assert_eq!(
            manifest_start("# story\nlang = \"fr\"\nstart = \"labo_nuit\"\n").as_deref(),
            Some("labo_nuit")
        );
        assert_eq!(manifest_start("lang = \"fr\"\n"), None);
    }
}
