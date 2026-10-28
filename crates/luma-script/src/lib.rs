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
//! **Status:** loading, disassembly and values. The virtual machine that
//! executes programs is the next step.
//!
//! Written with reference to the MIT-licensed WME Lite sources; see
//! `NOTICE`.

pub mod asm;
mod error;
mod instruction;
mod program;
mod value;

pub use error::{Error, Result};
pub use instruction::{Instruction, Opcode, OperandKind};
pub use program::{
    CallType, Entry, External, ExternalType, HEADER_SIZE, Header, MAGIC, Program, VERSION,
};
pub use value::{DivisionByZero, NativeId, Natives, Properties, Value, atof, atoi, format_float};
