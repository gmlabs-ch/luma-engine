//! A loaded compiled script: header, tables and code.
//!
//! Written with reference to `ScScript.cpp` and `ScScript.h` of WME Lite
//! (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`. The layout
//! is documented in `docs/formats/script.md`.

use std::fmt::Write as _;

use crate::error::{Error, Result};
use crate::instruction::{Instruction, read_string, read_u32};

/// The magic at the start of a compiled script, as a little-endian value:
/// the bytes `DE AD C0 DE`.
pub const MAGIC: u32 = 0xDEC0_ADDE;

/// The newest script version understood, and the one every known game
/// uses: `1.2`.
pub const VERSION: u32 = 0x0102;

/// The first version with an externals table.
const FIRST_VERSION_WITH_EXTERNALS: u32 = 0x0101;

/// Size of the fixed header, before the file name.
pub const HEADER_SIZE: usize = 32;

/// The fixed header of a script: its version and the offsets of its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Version, major in the high byte and minor in the low byte.
    pub version: u32,
    /// Offset of the first instruction.
    pub code: u32,
    /// Offset of the function table.
    pub functions: u32,
    /// Offset of the symbol table.
    pub symbols: u32,
    /// Offset of the event table.
    pub events: u32,
    /// Offset of the externals table (meaningful from version 1.1).
    pub externals: u32,
    /// Offset of the method table.
    pub methods: u32,
}

/// A named position in the code: a function, an event handler or a method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The name, as declared in the source.
    pub name: String,
    /// Offset of the first instruction of the body.
    pub position: u32,
}

/// Calling convention of an external DLL function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum CallType {
    StdCall,
    CDecl,
    ThisCall,
}

/// Parameter or return type of an external DLL function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ExternalType {
    Void,
    Bool,
    Long,
    Byte,
    String,
    Float,
    Double,
    MemBuffer,
}

/// An external DLL function declared by the script. Luma Engine never
/// calls these; they are kept so that tools can show them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    /// The DLL file name.
    pub dll: String,
    /// The exported function name.
    pub name: String,
    /// The calling convention.
    pub call_type: CallType,
    /// The return type.
    pub returns: ExternalType,
    /// The parameter types.
    pub params: Vec<ExternalType>,
}

/// A compiled script, loaded and checked: the raw bytes plus the decoded
/// header and tables. Immutable, so that it can be shared by the threads
/// executing it.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    bytes: Vec<u8>,
    header: Header,
    filename: String,
    symbols: Vec<String>,
    functions: Vec<Entry>,
    events: Vec<Entry>,
    methods: Vec<Entry>,
    externals: Vec<External>,
    code_end: usize,
}

/// A little-endian reader over the file, with bounds checks.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], pos: u32, what: &'static str) -> Result<Self> {
        let pos = usize::try_from(pos).map_err(|_| Error::Truncated { offset: 0, what })?;
        if pos >= bytes.len() {
            return Err(Error::Truncated { offset: pos, what });
        }
        Ok(Cursor { bytes, pos })
    }

    fn u32(&mut self, what: &'static str) -> Result<u32> {
        let value = read_u32(self.bytes, self.pos, what)?;
        self.pos += 4;
        Ok(value)
    }

    fn string(&mut self, what: &'static str) -> Result<String> {
        let (string, next) = read_string(self.bytes, self.pos, what)?;
        self.pos = next;
        Ok(string)
    }

    fn count(&mut self, what: &'static str) -> Result<usize> {
        let offset = self.pos;
        let count = self.u32(what)?;
        // Every entry takes at least five bytes; a count larger than the
        // file is corrupt and must not be used for an allocation.
        let count = usize::try_from(count).unwrap_or(usize::MAX);
        if count > self.bytes.len() / 5 {
            return Err(Error::Invalid {
                offset,
                reason: format!("{what} count {count} exceeds the file size"),
            });
        }
        Ok(count)
    }

    fn entries(&mut self, what: &'static str) -> Result<Vec<Entry>> {
        let count = self.count(what)?;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let position = self.u32(what)?;
            let name = self.string(what)?;
            entries.push(Entry { name, position });
        }
        Ok(entries)
    }
}

impl Program {
    /// Loads a compiled script.
    pub fn parse(bytes: impl Into<Vec<u8>>) -> Result<Program> {
        let bytes: Vec<u8> = bytes.into();
        let magic = read_u32(&bytes, 0, "magic")?;
        if magic != MAGIC {
            return Err(Error::NotAScript { magic });
        }
        let version = read_u32(&bytes, 4, "version")?;
        if version > VERSION {
            return Err(Error::UnsupportedVersion { version });
        }
        let header = Header {
            version,
            code: read_u32(&bytes, 8, "header")?,
            functions: read_u32(&bytes, 12, "header")?,
            symbols: read_u32(&bytes, 16, "header")?,
            events: read_u32(&bytes, 20, "header")?,
            externals: read_u32(&bytes, 24, "header")?,
            methods: read_u32(&bytes, 28, "header")?,
        };
        let (filename, _) = read_string(&bytes, HEADER_SIZE, "file name")?;

        // Symbols: explicit indexes, all below the count.
        let mut cursor = Cursor::new(&bytes, header.symbols, "symbol table")?;
        let count = cursor.count("symbol table")?;
        let mut symbols = vec![String::new(); count];
        for _ in 0..count {
            let offset = cursor.pos;
            let index = cursor.u32("symbol table")?;
            let name = cursor.string("symbol table")?;
            let slot = usize::try_from(index)
                .ok()
                .and_then(|i| symbols.get_mut(i))
                .ok_or(Error::SymbolIndex {
                    offset,
                    index,
                    count,
                })?;
            *slot = name;
        }

        let functions =
            Cursor::new(&bytes, header.functions, "function table")?.entries("function table")?;
        let events = Cursor::new(&bytes, header.events, "event table")?.entries("event table")?;
        let methods =
            Cursor::new(&bytes, header.methods, "method table")?.entries("method table")?;

        let externals = if version >= FIRST_VERSION_WITH_EXTERNALS {
            let mut cursor = Cursor::new(&bytes, header.externals, "externals table")?;
            let count = cursor.count("externals table")?;
            let mut externals = Vec::with_capacity(count);
            for _ in 0..count {
                let dll = cursor.string("externals table")?;
                let name = cursor.string("externals table")?;
                let offset = cursor.pos;
                let call_type = match cursor.u32("externals table")? {
                    0 => CallType::StdCall,
                    1 => CallType::CDecl,
                    2 => CallType::ThisCall,
                    other => {
                        return Err(Error::Invalid {
                            offset,
                            reason: format!("unknown calling convention {other}"),
                        });
                    }
                };
                let returns = external_type(&mut cursor)?;
                let num_params = cursor.count("externals table")?;
                let mut params = Vec::with_capacity(num_params);
                for _ in 0..num_params {
                    params.push(external_type(&mut cursor)?);
                }
                externals.push(External {
                    dll,
                    name,
                    call_type,
                    returns,
                    params,
                });
            }
            externals
        } else {
            Vec::new()
        };

        let code_start = usize::try_from(header.code).unwrap_or(usize::MAX);
        if code_start < HEADER_SIZE || code_start > bytes.len() {
            return Err(Error::Invalid {
                offset: 8,
                reason: format!("code offset {code_start} is outside the file"),
            });
        }
        // The code runs up to the first table placed after it.
        let code_end = [
            header.functions,
            header.symbols,
            header.events,
            header.externals,
            header.methods,
        ]
        .iter()
        .filter_map(|&offset| usize::try_from(offset).ok())
        .filter(|&offset| offset > code_start && offset <= bytes.len())
        .min()
        .unwrap_or(bytes.len());

        Ok(Program {
            bytes,
            header,
            filename,
            symbols,
            functions,
            events,
            methods,
            externals,
            code_end,
        })
    }

    /// The raw bytes of the file.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The decoded header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The version as `major.minor`, for example `1.2`.
    pub fn version(&self) -> String {
        format!(
            "{}.{}",
            self.header.version >> 8,
            self.header.version & 0xff
        )
    }

    /// The script's own path, as recorded by the compiler.
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// The symbol table, by index.
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    /// The symbol with this index, if it exists.
    pub fn symbol(&self, index: u32) -> Option<&str> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.symbols.get(i))
            .map(String::as_str)
    }

    /// The functions declared by the script.
    pub fn functions(&self) -> &[Entry] {
        &self.functions
    }

    /// The event handlers declared by the script.
    pub fn events(&self) -> &[Entry] {
        &self.events
    }

    /// The methods declared by the script.
    pub fn methods(&self) -> &[Entry] {
        &self.methods
    }

    /// The external DLL functions declared by the script.
    pub fn externals(&self) -> &[External] {
        &self.externals
    }

    /// Offset of the first instruction.
    pub fn code_start(&self) -> usize {
        usize::try_from(self.header.code).unwrap_or(usize::MAX)
    }

    /// Offset just past the last instruction.
    pub fn code_end(&self) -> usize {
        self.code_end
    }

    /// Whether the script stays loaded after its top-level code returns,
    /// because it has event handlers or methods to invoke later.
    pub fn is_persistent(&self) -> bool {
        !self.events.is_empty() || !self.methods.is_empty()
    }

    /// Position of a function: exact name, first match.
    pub fn function_position(&self, name: &str) -> Option<u32> {
        self.functions
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.position)
    }

    /// Position of an event handler: case-insensitive, last match.
    pub fn event_position(&self, name: &str) -> Option<u32> {
        self.events
            .iter()
            .rev()
            .find(|e| e.name.eq_ignore_ascii_case(name))
            .map(|e| e.position)
    }

    /// Position of a method: exact name, first match.
    pub fn method_position(&self, name: &str) -> Option<u32> {
        self.methods
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.position)
    }

    /// Decodes the instruction at `offset`, checking that it lies in the
    /// code and that its symbol index, if any, exists. Returns the
    /// instruction and the offset of the next one.
    pub fn decode_at(&self, offset: usize) -> Result<(Instruction, usize)> {
        if offset < self.code_start() || offset >= self.code_end {
            return Err(Error::Invalid {
                offset,
                reason: "instruction pointer outside the code".to_string(),
            });
        }
        let (instruction, next) = Instruction::decode(&self.bytes[..self.code_end], offset)?;
        if let Some(index) = instruction.symbol() {
            if self.symbol(index).is_none() {
                return Err(Error::SymbolIndex {
                    offset,
                    index,
                    count: self.symbols.len(),
                });
            }
        }
        Ok((instruction, next))
    }

    /// Decodes the whole code, in order, with the offset of each
    /// instruction.
    pub fn disassemble(&self) -> Result<Vec<(usize, Instruction)>> {
        let mut out = Vec::new();
        let mut offset = self.code_start();
        while offset < self.code_end {
            let (instruction, next) = self.decode_at(offset)?;
            out.push((offset, instruction));
            offset = next;
        }
        Ok(out)
    }

    /// The instruction with its operand resolved: symbol names instead of
    /// indexes.
    pub fn format_instruction(&self, instruction: &Instruction) -> String {
        match instruction.symbol() {
            Some(index) => format!(
                "{} {}",
                instruction.opcode().mnemonic(),
                self.symbol(index).unwrap_or("?")
            ),
            None => instruction.to_string(),
        }
    }

    /// The entry points at `offset`: functions, events and methods whose
    /// body starts there, as labels such as `event "LeftClick"`.
    pub fn labels_at(&self, offset: usize) -> Vec<String> {
        let mut labels = Vec::new();
        let at = |e: &Entry| usize::try_from(e.position).ok() == Some(offset);
        for e in self.functions.iter().filter(|e| at(e)) {
            labels.push(format!("function {}", e.name));
        }
        for e in self.events.iter().filter(|e| at(e)) {
            labels.push(format!("event {:?}", e.name));
        }
        for e in self.methods.iter().filter(|e| at(e)) {
            labels.push(format!("method {}", e.name));
        }
        labels
    }

    /// A listing of the whole code: one line per instruction with its
    /// offset, preceded by a label line at each entry point.
    pub fn listing(&self) -> Result<String> {
        let mut out = String::new();
        for (offset, instruction) in self.disassemble()? {
            for label in self.labels_at(offset) {
                let _ = writeln!(out, "{label}:");
            }
            let _ = writeln!(
                out,
                "{offset:>6}  {}",
                self.format_instruction(&instruction)
            );
        }
        Ok(out)
    }
}

fn external_type(cursor: &mut Cursor<'_>) -> Result<ExternalType> {
    let offset = cursor.pos;
    Ok(match cursor.u32("externals table")? {
        0 => ExternalType::Void,
        1 => ExternalType::Bool,
        2 => ExternalType::Long,
        3 => ExternalType::Byte,
        4 => ExternalType::String,
        5 => ExternalType::Float,
        6 => ExternalType::Double,
        7 => ExternalType::MemBuffer,
        other => {
            return Err(Error::Invalid {
                offset,
                reason: format!("unknown external type {other}"),
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::Assembler;

    /// `var x = 1 + 2; on "LeftClick" { Sleep(10); }` and a method, more
    /// or less as the compiler would lay it out.
    fn sample() -> Vec<u8> {
        let mut a = Assembler::new("scripts\\sample.script");
        a.emit(Instruction::DbgLine(1));
        a.def_var("x");
        a.push_int(2);
        a.push_int(1);
        a.emit(Instruction::Add);
        a.pop_var("x");
        let end = a.label();
        a.jmp(end);
        a.event("LeftClick");
        a.emit(Instruction::Scope);
        a.push_int(10);
        a.push_int(1);
        a.external_call("Sleep");
        a.emit(Instruction::PopEmpty);
        a.emit(Instruction::RetEvent);
        a.event("leftclick");
        a.emit(Instruction::Scope);
        a.emit(Instruction::RetEvent);
        a.method("Reset");
        a.emit(Instruction::Scope);
        a.emit(Instruction::CorrectStack(0));
        a.push_string("done");
        a.emit(Instruction::RetEvent);
        a.bind(end);
        a.emit(Instruction::Ret);
        a.build()
    }

    #[test]
    fn parses_header_and_tables() {
        let bytes = sample();
        let program = Program::parse(bytes.clone()).unwrap();
        assert_eq!(program.version(), "1.2");
        assert_eq!(program.filename(), "scripts\\sample.script");
        assert_eq!(
            program.code_start(),
            HEADER_SIZE + "scripts\\sample.script".len() + 1
        );
        assert_eq!(program.header().functions as usize, program.code_end());
        assert_eq!(program.symbols(), &["x", "Sleep"]);
        assert!(program.functions().is_empty());
        assert_eq!(program.events().len(), 2);
        assert_eq!(program.methods().len(), 1);
        assert!(program.externals().is_empty());
        assert!(program.is_persistent());
        assert_eq!(program.bytes(), &bytes[..]);
    }

    #[test]
    fn looks_up_entry_points_like_the_engine() {
        let program = Program::parse(sample()).unwrap();
        // Events: case-insensitive, last match wins.
        let last = program.events()[1].position;
        assert_eq!(program.event_position("LEFTCLICK"), Some(last));
        assert_eq!(program.event_position("LeftClick"), Some(last));
        assert_eq!(program.event_position("RightClick"), None);
        // Methods: exact.
        assert_eq!(
            program.method_position("Reset"),
            Some(program.methods()[0].position)
        );
        assert_eq!(program.method_position("reset"), None);
        assert_eq!(program.function_position("Reset"), None);
    }

    #[test]
    fn disassembles_the_whole_code() {
        let program = Program::parse(sample()).unwrap();
        let code = program.disassemble().unwrap();
        assert_eq!(code.first().unwrap().1, Instruction::DbgLine(1));
        assert_eq!(code.last().unwrap().1, Instruction::Ret);
        assert_eq!(code[1], (program.code_start() + 8, Instruction::DefVar(0)));
        let jump = code.iter().find(|(_, i)| matches!(i, Instruction::Jmp(_)));
        assert_eq!(
            jump.unwrap().1.target().map(|t| t as usize),
            Some(code.last().unwrap().0)
        );
        let listing = program.listing().unwrap();
        assert!(listing.contains("  DEF_VAR x\n"), "{listing}");
        assert!(listing.contains("  EXTERNAL_CALL Sleep\n"), "{listing}");
        assert!(listing.contains("event \"LeftClick\":\n"), "{listing}");
        assert!(listing.contains("event \"leftclick\":\n"), "{listing}");
        assert!(listing.contains("method Reset:\n"), "{listing}");
        assert!(listing.contains("  PUSH_STRING \"done\"\n"), "{listing}");
        assert_eq!(listing.lines().count(), code.len() + 3);
    }

    #[test]
    fn decode_at_checks_bounds_and_symbols() {
        let program = Program::parse(sample()).unwrap();
        assert!(matches!(
            program.decode_at(0),
            Err(Error::Invalid { offset: 0, .. })
        ));
        assert!(matches!(
            program.decode_at(program.code_end()),
            Err(Error::Invalid { .. })
        ));
        // Patch the DEF_VAR symbol index to point past the table.
        let mut bytes = sample();
        let at = Program::parse(bytes.clone()).unwrap().code_start() + 8 + 4;
        bytes[at..at + 4].copy_from_slice(&7u32.to_le_bytes());
        let program = Program::parse(bytes).unwrap();
        assert_eq!(
            program.decode_at(program.code_start() + 8),
            Err(Error::SymbolIndex {
                offset: program.code_start() + 8,
                index: 7,
                count: 2
            })
        );
        assert!(program.listing().is_err());
    }

    #[test]
    fn rejects_foreign_or_damaged_files() {
        assert_eq!(
            Program::parse(b"SCENE { }".to_vec()),
            Err(Error::NotAScript { magic: 0x4E45_4353 })
        );
        assert_eq!(
            Program::parse(vec![0xDE, 0xAD]),
            Err(Error::Truncated {
                offset: 0,
                what: "magic"
            })
        );
        let mut bytes = sample();
        bytes[4..8].copy_from_slice(&0x0103u32.to_le_bytes());
        assert_eq!(
            Program::parse(bytes),
            Err(Error::UnsupportedVersion { version: 0x0103 })
        );
        // Every truncation of the sample is an error, never a panic.
        let bytes = sample();
        for len in 0..bytes.len() {
            assert!(Program::parse(bytes[..len].to_vec()).is_err(), "len {len}");
        }
        // A symbol count larger than the file.
        let mut bytes = sample();
        let at = Program::parse(bytes.clone()).unwrap().header().symbols as usize;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            Program::parse(bytes),
            Err(Error::Invalid { offset, .. }) if offset == at
        ));
    }

    #[test]
    fn reads_an_externals_table() {
        let mut a = Assembler::new("ext.script");
        a.external(
            "user32.dll",
            "MessageBeep",
            CallType::StdCall,
            ExternalType::Bool,
            &[ExternalType::Long],
        );
        a.emit(Instruction::Ret);
        let program = Program::parse(a.build()).unwrap();
        assert_eq!(
            program.externals(),
            &[External {
                dll: "user32.dll".to_string(),
                name: "MessageBeep".to_string(),
                call_type: CallType::StdCall,
                returns: ExternalType::Bool,
                params: vec![ExternalType::Long],
            }]
        );
    }

    #[test]
    #[ignore = "needs the local test game in testdata/local"]
    fn decodes_every_local_script() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/white-chamber/unpacked");
        if !root.is_dir() {
            eprintln!("skipped: {} is absent", root.display());
            return;
        }
        let mut scripts = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("script"))
                {
                    scripts.push(path);
                }
            }
        }
        assert!(!scripts.is_empty());
        let mut histogram = std::collections::BTreeMap::new();
        let mut instructions = 0usize;
        for path in &scripts {
            let bytes = std::fs::read(path).unwrap();
            let program =
                Program::parse(bytes).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            assert_eq!(program.version(), "1.2", "{}", path.display());
            let code = program
                .disassemble()
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            // The code decodes exactly up to the first table.
            let (last, _) = code.last().unwrap();
            let (_, next) = program.decode_at(*last).unwrap();
            assert_eq!(next, program.code_end(), "{}", path.display());
            // Every entry point and jump target is an instruction boundary.
            let starts: std::collections::HashSet<usize> =
                code.iter().map(|(offset, _)| *offset).collect();
            for entry in program
                .functions()
                .iter()
                .chain(program.events())
                .chain(program.methods())
            {
                assert!(
                    starts.contains(&(entry.position as usize)),
                    "{}: {} at {}",
                    path.display(),
                    entry.name,
                    entry.position
                );
            }
            for (_, instruction) in &code {
                if let Some(target) = instruction.target() {
                    assert!(starts.contains(&(target as usize)), "{}", path.display());
                }
                *histogram
                    .entry(instruction.opcode().mnemonic())
                    .or_insert(0usize) += 1;
            }
            instructions += code.len();
            assert!(program.listing().is_ok());
        }
        eprintln!(
            "{} scripts, {instructions} instructions, {} distinct opcodes",
            scripts.len(),
            histogram.len()
        );
        for (mnemonic, count) in &histogram {
            eprintln!("{count:>8}  {mnemonic}");
        }
    }
}
