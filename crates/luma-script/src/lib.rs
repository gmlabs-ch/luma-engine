//! Compiled game scripts: loader, disassembler and (later) virtual machine.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! A game's logic is shipped as bytecode (`*.script` files starting with
//! `DE AD C0 DE`), documented in `docs/formats/script.md`. This crate
//! loads such a file into a [`Program`]: header, symbol, function, event,
//! method and externals tables, and the code, decoded on demand into
//! [`Instruction`]s. The [`asm`] module builds programs for tests.
//!
//! **Status:** loading and disassembly. The virtual machine that executes
//! programs is the next step.
//!
//! Written with reference to the MIT-licensed WME Lite sources; see
//! `NOTICE`.

pub mod asm;
mod error;
mod instruction;
mod program;

pub use error::{Error, Result};
pub use instruction::{Instruction, Opcode, OperandKind};
pub use program::{
    CallType, Entry, External, ExternalType, HEADER_SIZE, Header, MAGIC, Program, VERSION,
};
