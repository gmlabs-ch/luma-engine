//! An assembler producing compiled scripts, for tests and tools.
//!
//! It writes the layout the compiler writes (header, file name, code, then
//! the function, symbol, event, externals and method tables), so a program
//! built here loads like one from a game. It is not a compiler: it takes
//! instructions, not source.
//!
//! ```
//! use luma_script::asm::Assembler;
//! use luma_script::{Instruction, Program};
//!
//! let mut a = Assembler::new("test.script");
//! a.def_var("x");
//! a.push_int(42);
//! a.pop_var("x");
//! a.emit(Instruction::Ret);
//! let program = Program::parse(a.build()).unwrap();
//! assert_eq!(program.symbols(), &["x"]);
//! ```

use crate::instruction::Instruction;
use crate::program::{CallType, ExternalType, HEADER_SIZE, MAGIC, VERSION};

/// A position in the code, bound with [`Assembler::bind`] and used as a
/// jump or call target before or after it is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label(usize);

/// Builds a compiled script instruction by instruction.
#[derive(Debug, Clone)]
pub struct Assembler {
    filename: String,
    code: Vec<u8>,
    symbols: Vec<String>,
    labels: Vec<Option<u32>>,
    fixups: Vec<(usize, Label)>,
    functions: Vec<(String, u32)>,
    events: Vec<(String, u32)>,
    methods: Vec<(String, u32)>,
    externals: Vec<Vec<u8>>,
}

impl Assembler {
    /// Starts a script that records `filename` as its own path.
    pub fn new(filename: &str) -> Self {
        Assembler {
            filename: filename.to_string(),
            code: Vec::new(),
            symbols: Vec::new(),
            labels: Vec::new(),
            fixups: Vec::new(),
            functions: Vec::new(),
            events: Vec::new(),
            methods: Vec::new(),
            externals: Vec::new(),
        }
    }

    /// Offset of the first instruction in the built file.
    fn code_base(&self) -> u32 {
        (HEADER_SIZE + self.filename.len() + 1) as u32
    }

    /// The file offset the next instruction will have.
    pub fn position(&self) -> u32 {
        self.code_base() + self.code.len() as u32
    }

    /// The index of a symbol, adding it to the table if needed.
    pub fn symbol(&mut self, name: &str) -> u32 {
        if let Some(index) = self.symbols.iter().position(|s| s == name) {
            return index as u32;
        }
        self.symbols.push(name.to_string());
        (self.symbols.len() - 1) as u32
    }

    /// Creates an unbound label.
    pub fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    /// Binds a label to the current position.
    pub fn bind(&mut self, label: Label) {
        self.labels[label.0] = Some(self.position());
    }

    /// Declares a function starting at the current position.
    pub fn function(&mut self, name: &str) {
        self.functions.push((name.to_string(), self.position()));
    }

    /// Declares an event handler starting at the current position.
    pub fn event(&mut self, name: &str) {
        self.events.push((name.to_string(), self.position()));
    }

    /// Declares a method starting at the current position.
    pub fn method(&mut self, name: &str) {
        self.methods.push((name.to_string(), self.position()));
    }

    /// Declares an external DLL function.
    pub fn external(
        &mut self,
        dll: &str,
        name: &str,
        call_type: CallType,
        returns: ExternalType,
        params: &[ExternalType],
    ) {
        let mut entry = Vec::new();
        push_string(&mut entry, dll);
        push_string(&mut entry, name);
        entry.extend_from_slice(&(call_type as u32).to_le_bytes());
        entry.extend_from_slice(&(returns as u32).to_le_bytes());
        entry.extend_from_slice(&(params.len() as u32).to_le_bytes());
        for param in params {
            entry.extend_from_slice(&(*param as u32).to_le_bytes());
        }
        self.externals.push(entry);
    }

    /// Appends an instruction. Addresses in `Call`, `Jmp` and `JmpFalse`
    /// are taken as given; use [`Assembler::call`], [`Assembler::jmp`] and
    /// [`Assembler::jmp_false`] for labels.
    pub fn emit(&mut self, instruction: Instruction) {
        instruction.encode(&mut self.code);
    }

    fn emit_with_label(&mut self, instruction: Instruction, label: Label) {
        let operand = self.code.len() + 4;
        self.emit(instruction);
        self.fixups.push((operand, label));
    }

    /// `JMP` to a label.
    pub fn jmp(&mut self, label: Label) {
        self.emit_with_label(Instruction::Jmp(0), label);
    }

    /// `JMP_FALSE` to a label.
    pub fn jmp_false(&mut self, label: Label) {
        self.emit_with_label(Instruction::JmpFalse(0), label);
    }

    /// `CALL` a label.
    pub fn call(&mut self, label: Label) {
        self.emit_with_label(Instruction::Call(0), label);
    }

    /// `DEF_VAR name`.
    pub fn def_var(&mut self, name: &str) {
        let s = self.symbol(name);
        self.emit(Instruction::DefVar(s));
    }

    /// `DEF_GLOB_VAR name`.
    pub fn def_glob_var(&mut self, name: &str) {
        let s = self.symbol(name);
        self.emit(Instruction::DefGlobVar(s));
    }

    /// `DEF_CONST_VAR name`.
    pub fn def_const_var(&mut self, name: &str) {
        let s = self.symbol(name);
        self.emit(Instruction::DefConstVar(s));
    }

    /// `PUSH_VAR name`.
    pub fn push_var(&mut self, name: &str) {
        let s = self.symbol(name);
        self.emit(Instruction::PushVar(s));
    }

    /// `PUSH_VAR_REF name`.
    pub fn push_var_ref(&mut self, name: &str) {
        let s = self.symbol(name);
        self.emit(Instruction::PushVarRef(s));
    }

    /// `POP_VAR name`.
    pub fn pop_var(&mut self, name: &str) {
        let s = self.symbol(name);
        self.emit(Instruction::PopVar(s));
    }

    /// `PUSH_THIS name`.
    pub fn push_this(&mut self, name: &str) {
        let s = self.symbol(name);
        self.emit(Instruction::PushThis(s));
    }

    /// `EXTERNAL_CALL name`.
    pub fn external_call(&mut self, name: &str) {
        let s = self.symbol(name);
        self.emit(Instruction::ExternalCall(s));
    }

    /// `PUSH_INT value`.
    pub fn push_int(&mut self, value: i32) {
        self.emit(Instruction::PushInt(value));
    }

    /// `PUSH_BOOL value`.
    pub fn push_bool(&mut self, value: bool) {
        self.emit(Instruction::PushBool(value));
    }

    /// `PUSH_FLOAT value`.
    pub fn push_float(&mut self, value: f64) {
        self.emit(Instruction::PushFloat(value));
    }

    /// `PUSH_STRING value`.
    pub fn push_string(&mut self, value: &str) {
        self.emit(Instruction::PushString(value.to_string()));
    }

    /// Writes the file.
    ///
    /// # Panics
    ///
    /// If a label used as a target was never bound.
    pub fn build(mut self) -> Vec<u8> {
        for (operand, label) in &self.fixups {
            let target = self.labels[label.0]
                .unwrap_or_else(|| panic!("label {} used but never bound", label.0));
            self.code[*operand..*operand + 4].copy_from_slice(&target.to_le_bytes());
        }

        let mut out = Vec::with_capacity(HEADER_SIZE + self.code.len() + 64);
        out.resize(HEADER_SIZE, 0);
        push_string(&mut out, &self.filename);
        let code = out.len() as u32;
        out.extend_from_slice(&self.code);

        let functions = out.len() as u32;
        push_entries(&mut out, &self.functions);

        let symbols = out.len() as u32;
        out.extend_from_slice(&(self.symbols.len() as u32).to_le_bytes());
        for (index, name) in self.symbols.iter().enumerate() {
            out.extend_from_slice(&(index as u32).to_le_bytes());
            push_string(&mut out, name);
        }

        let events = out.len() as u32;
        push_entries(&mut out, &self.events);

        let externals = out.len() as u32;
        out.extend_from_slice(&(self.externals.len() as u32).to_le_bytes());
        for entry in &self.externals {
            out.extend_from_slice(entry);
        }

        let methods = out.len() as u32;
        push_entries(&mut out, &self.methods);

        for (at, value) in [
            (0, MAGIC),
            (4, VERSION),
            (8, code),
            (12, functions),
            (16, symbols),
            (20, events),
            (24, externals),
            (28, methods),
        ] {
            out[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
}

fn push_string(out: &mut Vec<u8>, s: &str) {
    out.extend(
        s.chars()
            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')),
    );
    out.push(0);
}

fn push_entries(out: &mut Vec<u8>, entries: &[(String, u32)]) {
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (name, position) in entries {
        out.extend_from_slice(&position.to_le_bytes());
        push_string(out, name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::Program;

    #[test]
    fn labels_resolve_forwards_and_backwards() {
        let mut a = Assembler::new("loop.script");
        let top = a.label();
        let exit = a.label();
        a.bind(top);
        a.push_var("i");
        a.push_int(3);
        a.emit(Instruction::CmpL);
        a.jmp_false(exit);
        a.jmp(top);
        a.bind(exit);
        a.emit(Instruction::Ret);
        let program = Program::parse(a.build()).unwrap();
        let code = program.disassemble().unwrap();
        let start = program.code_start() as u32;
        assert_eq!(code[0].0 as u32, start);
        assert_eq!(code[3].1, Instruction::JmpFalse(code[5].0 as u32));
        assert_eq!(code[4].1, Instruction::Jmp(start));
        assert_eq!(code[5].1, Instruction::Ret);
    }

    #[test]
    fn symbols_are_interned() {
        let mut a = Assembler::new("s.script");
        a.push_var("a");
        a.push_var("b");
        a.pop_var("a");
        assert_eq!(a.symbol("b"), 1);
        assert_eq!(a.symbol("c"), 2);
        a.emit(Instruction::Ret);
        let program = Program::parse(a.build()).unwrap();
        assert_eq!(program.symbols(), &["a", "b", "c"]);
    }

    #[test]
    #[should_panic(expected = "never bound")]
    fn unbound_label_panics() {
        let mut a = Assembler::new("bad.script");
        let l = a.label();
        a.jmp(l);
        a.build();
    }
}
