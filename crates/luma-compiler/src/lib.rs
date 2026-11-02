//! Compiler from Wintermute Engine script sources (`*.script`, `*.inc`) to
//! the compiled form `luma-script` runs, producing byte for byte the output
//! of the original compiler.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! The specification is `docs/compiler.md`, written from the MIT-licensed
//! Wintermute Engine 1.x compiler sources (`src/compiler/`) and verified
//! against the compiled scripts of a real game; the file format is
//! `docs/formats/script.md`. The pipeline is the original's: a scanner with
//! `#include` expansion ([`lexer`]), a parser recording the lines the
//! original's LALR(1) parser recorded ([`parser`]), the placement and
//! symbol checks ([`symbols`]), and one code generation pass with
//! back-patching ([`codegen`]).
//!
//! ```
//! use luma_compiler::{Compiler, Natives};
//!
//! let source = b"var x = 1;\non \"LeftClick\" { Game.Msg(\"hi\"); }\n";
//! let bytes = Compiler::new(Natives::wme())
//!     .compile(source, "scripts\\demo.script", &mut |_name: &[u8]| None)
//!     .unwrap();
//! let program = luma_script::Program::parse(bytes).unwrap();
//! assert_eq!(program.symbols(), &["x", "Game"]);
//! assert_eq!(program.events()[0].name, "LeftClick");
//! ```
//!
//! Written with reference to the Wintermute Engine 1.x sources (MIT
//! License, Copyright (c) 2003-2013 Jan Nedoma and contributors); see
//! `NOTICE`.

use std::fs;
use std::path::{Path, PathBuf};

pub mod ast;
mod codegen;
mod error;
pub mod lexer;
mod parser;
mod symbols;

pub use error::{Error, Pos, Result};
pub use lexer::Includes;

/// The names the engine provides to every script: the functions (and
/// classes used with `new`) that compile to `EXTERNAL_CALL`, and the
/// variables that can be read, assigned and used as method receivers
/// without a declaration.
///
/// A call to a name that is neither a native nor declared by the script
/// (or its includes) is an error, as in the original; the games declare
/// the rest (`Scene`, `Keyboard`, their actors) with `global` in an
/// include.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Natives {
    /// Functions and classes.
    pub functions: Vec<String>,
    /// Variables.
    pub variables: Vec<String>,
}

impl Natives {
    /// Nothing predefined.
    pub fn empty() -> Natives {
        Natives::default()
    }

    /// What the Wintermute Engine 1.x adventure runtime publishes
    /// (`CBGame::PublishNatives` and `CAdGame::PublishNatives`); the list
    /// the games were compiled with. Plugins add their classes with
    /// [`Natives::function`].
    pub fn wme() -> Natives {
        const FUNCTIONS: [&str; 30] = [
            "LOG",
            "String",
            "MemBuffer",
            "File",
            "Date",
            "Array",
            "TcpClient",
            "Object",
            "Sleep",
            "WaitFor",
            "Random",
            "SetScriptTimeSlice",
            "MakeRGBA",
            "MakeRGB",
            "MakeHSL",
            "RGB",
            "GetRValue",
            "GetGValue",
            "GetBValue",
            "GetAValue",
            "GetHValue",
            "GetSValue",
            "GetLValue",
            "Debug",
            "ToString",
            "ToInt",
            "ToBool",
            "ToFloat",
            "Actor",
            "Entity",
        ];
        const VARIABLES: [&str; 5] = ["Game", "Math", "Directory", "self", "this"];
        Natives {
            functions: FUNCTIONS.iter().map(|s| s.to_string()).collect(),
            variables: VARIABLES.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Adds a native function or class.
    pub fn function(mut self, name: &str) -> Natives {
        self.functions.push(name.to_string());
        self
    }

    /// Adds a native variable.
    pub fn variable(mut self, name: &str) -> Natives {
        self.variables.push(name.to_string());
        self
    }
}

/// The compiler: the natives it knows, and [`Compiler::compile`].
#[derive(Debug, Clone)]
pub struct Compiler {
    natives: Natives,
}

impl Compiler {
    /// A compiler knowing `natives`.
    pub fn new(natives: Natives) -> Compiler {
        Compiler { natives }
    }

    /// Compiles `source`, writing `name` in the header of the result (the
    /// original tools write the project-relative path with backslashes,
    /// `scripts\scene.script`), and loading `#include`d files through
    /// `includes`. Returns the compiled file, or the first error.
    pub fn compile(
        &self,
        source: &[u8],
        name: &str,
        includes: &mut dyn Includes,
    ) -> Result<Vec<u8>> {
        let mut script = parser::parse(source.to_vec(), name, includes)?;
        symbols::check(&mut script, &self.natives)?;
        Ok(codegen::generate(&script, name))
    }

    /// Parses and checks `source` without generating code; for tools that
    /// want the syntax tree.
    pub fn parse(
        &self,
        source: &[u8],
        name: &str,
        includes: &mut dyn Includes,
    ) -> Result<ast::Script> {
        let mut script = parser::parse(source.to_vec(), name, includes)?;
        symbols::check(&mut script, &self.natives)?;
        Ok(script)
    }
}

/// Resolves `#include` names against one or more directories, the way the
/// engine resolves game files: the name is a path relative to the game
/// root, with `\` or `/`, matched without regard to case. The directories
/// are tried in order; the first file found wins.
#[derive(Debug, Clone)]
pub struct GameDirs {
    roots: Vec<PathBuf>,
}

impl GameDirs {
    /// Resolves against `root`.
    pub fn new(root: impl Into<PathBuf>) -> GameDirs {
        GameDirs {
            roots: vec![root.into()],
        }
    }

    /// Also tries `root`, after the ones given so far.
    pub fn and(mut self, root: impl Into<PathBuf>) -> GameDirs {
        self.roots.push(root.into());
        self
    }

    /// The file `name` names under one of the roots, if it exists.
    pub fn find(&self, name: &[u8]) -> Option<PathBuf> {
        let name = String::from_utf8_lossy(name);
        let components: Vec<&str> = name
            .split(['\\', '/'])
            .filter(|c| !c.is_empty() && *c != ".")
            .collect();
        if components.is_empty() || components.contains(&"..") {
            return None;
        }
        self.roots
            .iter()
            .find_map(|root| find_case_insensitive(root, &components))
    }
}

/// Walks `components` under `dir`, matching each one exactly first, then
/// without regard to case.
fn find_case_insensitive(dir: &Path, components: &[&str]) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    for component in components {
        let exact = current.join(component);
        if exact.exists() {
            current = exact;
            continue;
        }
        let lower = component.to_lowercase();
        let entry = fs::read_dir(&current)
            .ok()?
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().to_lowercase() == lower)?;
        current = entry.path();
    }
    current.is_file().then_some(current)
}

impl Includes for GameDirs {
    fn load(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        fs::read(self.find(name)?).ok()
    }
}

#[cfg(test)]
mod tests;
