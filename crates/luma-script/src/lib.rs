//! Compiled game scripts: loader, disassembler and (later) virtual machine.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! A game's logic is shipped as bytecode (`*.script` files starting with
//! `DE AD C0 DE`), documented in `docs/formats/script.md`. This crate
//! loads such a file into a [`Program`]: header, symbol, function, event,
//! method and externals tables, and the code, decoded on demand into
//! [`Instruction`]s. The [`asm`] module builds programs for tests.
//! [`Value`] is the dynamic type of the language, with the engine's
//! conversion, comparison and property rules; native objects of the
//! engine are reached through the [`Natives`] trait.
//!
//! The [`Engine`] runs scripts: it executes their instructions, spawns
//! threads for events and methods, and resolves sleeps and waits once per
//! frame through the [`Host`] trait, which the embedding program
//! implements. The rules are summarised in `docs/scripting.md`. With the
//! `serde` feature, an [`EngineSnapshot`] captures every script and
//! global for a saved game, and puts them back.
//!
//! Written with reference to the MIT-licensed WME Lite sources; see
//! `NOTICE`.

pub mod asm;
mod engine;
mod error;
mod host;
mod instruction;
mod program;
mod script;
#[cfg(feature = "serde")]
mod snapshot;
mod stack;
mod value;

pub use engine::{Counts, Engine, Global, Removed, rgba};
pub use error::{Error, Result};
pub use host::{Context, Effect, Host, Return};
pub use instruction::{Instruction, Opcode, OperandKind};
pub use program::{
    CallType, Entry, External, ExternalType, HEADER_SIZE, Header, MAGIC, Program, VERSION,
};
pub use script::{Script, ScriptId, State};
#[cfg(feature = "serde")]
pub use snapshot::{EngineSnapshot, ScriptSnapshot, SlotSnapshot, VarRefSnapshot};
pub use stack::{Globals, Slot, Stack, VarRef};
pub use value::{DivisionByZero, NativeId, Natives, Properties, Value, atof, atoi, format_float};
