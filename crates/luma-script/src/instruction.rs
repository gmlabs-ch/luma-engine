//! The instruction set of compiled scripts: opcodes, operands, encoding.
//!
//! Written with reference to `dcscript.h` and `ScScript.cpp` of WME Lite
//! (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`. The layout
//! is documented in `docs/formats/script.md`.

use std::fmt;

use crate::error::{Error, Result};

/// An operation code, with the numeric value stored in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
#[allow(missing_docs)]
pub enum Opcode {
    DefVar = 0,
    DefGlobVar = 1,
    Ret = 2,
    RetEvent = 3,
    Call = 4,
    CallByExp = 5,
    ExternalCall = 6,
    Scope = 7,
    CorrectStack = 8,
    CreateObject = 9,
    PopEmpty = 10,
    PushVar = 11,
    PushVarRef = 12,
    PopVar = 13,
    PushVarThis = 14,
    PushInt = 15,
    PushBool = 16,
    PushFloat = 17,
    PushString = 18,
    PushNull = 19,
    PushThisFromStack = 20,
    PushThis = 21,
    PopThis = 22,
    PushByExp = 23,
    PopByExp = 24,
    Jmp = 25,
    JmpFalse = 26,
    Add = 27,
    Sub = 28,
    Mul = 29,
    Div = 30,
    Modulo = 31,
    Not = 32,
    And = 33,
    Or = 34,
    CmpEq = 35,
    CmpNe = 36,
    CmpL = 37,
    CmpG = 38,
    CmpLe = 39,
    CmpGe = 40,
    CmpStrictEq = 41,
    CmpStrictNe = 42,
    DbgLine = 43,
    PopReg1 = 44,
    PushReg1 = 45,
    DefConstVar = 46,
}

/// What follows an opcode in the code stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperandKind {
    /// Nothing.
    None,
    /// A 32-bit index into the symbol table.
    Symbol,
    /// A 32-bit offset in the file.
    Address,
    /// A signed 32-bit integer.
    Int,
    /// A 32-bit boolean (zero is false).
    Bool,
    /// A 64-bit float.
    Float,
    /// A NUL-terminated string.
    String,
}

impl Opcode {
    /// Every opcode, in numeric order.
    pub const ALL: [Opcode; 47] = [
        Opcode::DefVar,
        Opcode::DefGlobVar,
        Opcode::Ret,
        Opcode::RetEvent,
        Opcode::Call,
        Opcode::CallByExp,
        Opcode::ExternalCall,
        Opcode::Scope,
        Opcode::CorrectStack,
        Opcode::CreateObject,
        Opcode::PopEmpty,
        Opcode::PushVar,
        Opcode::PushVarRef,
        Opcode::PopVar,
        Opcode::PushVarThis,
        Opcode::PushInt,
        Opcode::PushBool,
        Opcode::PushFloat,
        Opcode::PushString,
        Opcode::PushNull,
        Opcode::PushThisFromStack,
        Opcode::PushThis,
        Opcode::PopThis,
        Opcode::PushByExp,
        Opcode::PopByExp,
        Opcode::Jmp,
        Opcode::JmpFalse,
        Opcode::Add,
        Opcode::Sub,
        Opcode::Mul,
        Opcode::Div,
        Opcode::Modulo,
        Opcode::Not,
        Opcode::And,
        Opcode::Or,
        Opcode::CmpEq,
        Opcode::CmpNe,
        Opcode::CmpL,
        Opcode::CmpG,
        Opcode::CmpLe,
        Opcode::CmpGe,
        Opcode::CmpStrictEq,
        Opcode::CmpStrictNe,
        Opcode::DbgLine,
        Opcode::PopReg1,
        Opcode::PushReg1,
        Opcode::DefConstVar,
    ];

    /// The opcode with this numeric value, if any.
    pub fn from_u32(value: u32) -> Option<Opcode> {
        usize::try_from(value)
            .ok()
            .and_then(|i| Opcode::ALL.get(i).copied())
    }

    /// The numeric value stored in the file.
    pub fn code(self) -> u32 {
        self as u32
    }

    /// The mnemonic used by the engine's sources and by listings.
    pub fn mnemonic(self) -> &'static str {
        match self {
            Opcode::DefVar => "DEF_VAR",
            Opcode::DefGlobVar => "DEF_GLOB_VAR",
            Opcode::Ret => "RET",
            Opcode::RetEvent => "RET_EVENT",
            Opcode::Call => "CALL",
            Opcode::CallByExp => "CALL_BY_EXP",
            Opcode::ExternalCall => "EXTERNAL_CALL",
            Opcode::Scope => "SCOPE",
            Opcode::CorrectStack => "CORRECT_STACK",
            Opcode::CreateObject => "CREATE_OBJECT",
            Opcode::PopEmpty => "POP_EMPTY",
            Opcode::PushVar => "PUSH_VAR",
            Opcode::PushVarRef => "PUSH_VAR_REF",
            Opcode::PopVar => "POP_VAR",
            Opcode::PushVarThis => "PUSH_VAR_THIS",
            Opcode::PushInt => "PUSH_INT",
            Opcode::PushBool => "PUSH_BOOL",
            Opcode::PushFloat => "PUSH_FLOAT",
            Opcode::PushString => "PUSH_STRING",
            Opcode::PushNull => "PUSH_NULL",
            Opcode::PushThisFromStack => "PUSH_THIS_FROM_STACK",
            Opcode::PushThis => "PUSH_THIS",
            Opcode::PopThis => "POP_THIS",
            Opcode::PushByExp => "PUSH_BY_EXP",
            Opcode::PopByExp => "POP_BY_EXP",
            Opcode::Jmp => "JMP",
            Opcode::JmpFalse => "JMP_FALSE",
            Opcode::Add => "ADD",
            Opcode::Sub => "SUB",
            Opcode::Mul => "MUL",
            Opcode::Div => "DIV",
            Opcode::Modulo => "MODULO",
            Opcode::Not => "NOT",
            Opcode::And => "AND",
            Opcode::Or => "OR",
            Opcode::CmpEq => "CMP_EQ",
            Opcode::CmpNe => "CMP_NE",
            Opcode::CmpL => "CMP_L",
            Opcode::CmpG => "CMP_G",
            Opcode::CmpLe => "CMP_LE",
            Opcode::CmpGe => "CMP_GE",
            Opcode::CmpStrictEq => "CMP_STRICT_EQ",
            Opcode::CmpStrictNe => "CMP_STRICT_NE",
            Opcode::DbgLine => "DBG_LINE",
            Opcode::PopReg1 => "POP_REG1",
            Opcode::PushReg1 => "PUSH_REG1",
            Opcode::DefConstVar => "DEF_CONST_VAR",
        }
    }

    /// The opcode with this mnemonic, if any.
    pub fn from_mnemonic(mnemonic: &str) -> Option<Opcode> {
        Opcode::ALL
            .iter()
            .copied()
            .find(|op| op.mnemonic() == mnemonic)
    }

    /// The kind of operand that follows this opcode.
    pub fn operand(self) -> OperandKind {
        match self {
            Opcode::DefVar
            | Opcode::DefGlobVar
            | Opcode::DefConstVar
            | Opcode::ExternalCall
            | Opcode::PushVar
            | Opcode::PushVarRef
            | Opcode::PopVar
            | Opcode::PushThis => OperandKind::Symbol,
            Opcode::Call | Opcode::Jmp | Opcode::JmpFalse => OperandKind::Address,
            Opcode::CorrectStack | Opcode::PushInt | Opcode::DbgLine => OperandKind::Int,
            Opcode::PushBool => OperandKind::Bool,
            Opcode::PushFloat => OperandKind::Float,
            Opcode::PushString => OperandKind::String,
            _ => OperandKind::None,
        }
    }
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.mnemonic())
    }
}

/// A decoded instruction. Symbol operands are indexes into the program's
/// symbol table; addresses are offsets in the file.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum Instruction {
    DefVar(u32),
    DefGlobVar(u32),
    Ret,
    RetEvent,
    Call(u32),
    CallByExp,
    ExternalCall(u32),
    Scope,
    CorrectStack(u32),
    CreateObject,
    PopEmpty,
    PushVar(u32),
    PushVarRef(u32),
    PopVar(u32),
    PushVarThis,
    PushInt(i32),
    PushBool(bool),
    PushFloat(f64),
    PushString(String),
    PushNull,
    PushThisFromStack,
    PushThis(u32),
    PopThis,
    PushByExp,
    PopByExp,
    Jmp(u32),
    JmpFalse(u32),
    Add,
    Sub,
    Mul,
    Div,
    Modulo,
    Not,
    And,
    Or,
    CmpEq,
    CmpNe,
    CmpL,
    CmpG,
    CmpLe,
    CmpGe,
    CmpStrictEq,
    CmpStrictNe,
    DbgLine(u32),
    PopReg1,
    PushReg1,
    DefConstVar(u32),
}

impl Instruction {
    /// The opcode of this instruction.
    pub fn opcode(&self) -> Opcode {
        match self {
            Instruction::DefVar(_) => Opcode::DefVar,
            Instruction::DefGlobVar(_) => Opcode::DefGlobVar,
            Instruction::Ret => Opcode::Ret,
            Instruction::RetEvent => Opcode::RetEvent,
            Instruction::Call(_) => Opcode::Call,
            Instruction::CallByExp => Opcode::CallByExp,
            Instruction::ExternalCall(_) => Opcode::ExternalCall,
            Instruction::Scope => Opcode::Scope,
            Instruction::CorrectStack(_) => Opcode::CorrectStack,
            Instruction::CreateObject => Opcode::CreateObject,
            Instruction::PopEmpty => Opcode::PopEmpty,
            Instruction::PushVar(_) => Opcode::PushVar,
            Instruction::PushVarRef(_) => Opcode::PushVarRef,
            Instruction::PopVar(_) => Opcode::PopVar,
            Instruction::PushVarThis => Opcode::PushVarThis,
            Instruction::PushInt(_) => Opcode::PushInt,
            Instruction::PushBool(_) => Opcode::PushBool,
            Instruction::PushFloat(_) => Opcode::PushFloat,
            Instruction::PushString(_) => Opcode::PushString,
            Instruction::PushNull => Opcode::PushNull,
            Instruction::PushThisFromStack => Opcode::PushThisFromStack,
            Instruction::PushThis(_) => Opcode::PushThis,
            Instruction::PopThis => Opcode::PopThis,
            Instruction::PushByExp => Opcode::PushByExp,
            Instruction::PopByExp => Opcode::PopByExp,
            Instruction::Jmp(_) => Opcode::Jmp,
            Instruction::JmpFalse(_) => Opcode::JmpFalse,
            Instruction::Add => Opcode::Add,
            Instruction::Sub => Opcode::Sub,
            Instruction::Mul => Opcode::Mul,
            Instruction::Div => Opcode::Div,
            Instruction::Modulo => Opcode::Modulo,
            Instruction::Not => Opcode::Not,
            Instruction::And => Opcode::And,
            Instruction::Or => Opcode::Or,
            Instruction::CmpEq => Opcode::CmpEq,
            Instruction::CmpNe => Opcode::CmpNe,
            Instruction::CmpL => Opcode::CmpL,
            Instruction::CmpG => Opcode::CmpG,
            Instruction::CmpLe => Opcode::CmpLe,
            Instruction::CmpGe => Opcode::CmpGe,
            Instruction::CmpStrictEq => Opcode::CmpStrictEq,
            Instruction::CmpStrictNe => Opcode::CmpStrictNe,
            Instruction::DbgLine(_) => Opcode::DbgLine,
            Instruction::PopReg1 => Opcode::PopReg1,
            Instruction::PushReg1 => Opcode::PushReg1,
            Instruction::DefConstVar(_) => Opcode::DefConstVar,
        }
    }

    /// The symbol index this instruction refers to, if it has one.
    pub fn symbol(&self) -> Option<u32> {
        match self {
            Instruction::DefVar(s)
            | Instruction::DefGlobVar(s)
            | Instruction::DefConstVar(s)
            | Instruction::ExternalCall(s)
            | Instruction::PushVar(s)
            | Instruction::PushVarRef(s)
            | Instruction::PopVar(s)
            | Instruction::PushThis(s) => Some(*s),
            _ => None,
        }
    }

    /// The address this instruction jumps or calls to, if it has one.
    pub fn target(&self) -> Option<u32> {
        match self {
            Instruction::Call(a) | Instruction::Jmp(a) | Instruction::JmpFalse(a) => Some(*a),
            _ => None,
        }
    }

    /// Decodes the instruction starting at `offset` in `bytes`, and returns
    /// it with the offset of the next instruction.
    ///
    /// Symbol indexes are not checked here (the decoder does not know the
    /// table); `Program::decode_at` does.
    pub fn decode(bytes: &[u8], offset: usize) -> Result<(Instruction, usize)> {
        let raw = read_u32(bytes, offset, "opcode")?;
        let opcode = Opcode::from_u32(raw).ok_or(Error::UnknownOpcode {
            offset,
            opcode: raw,
        })?;
        let at = offset + 4;
        let (instruction, next) = match opcode.operand() {
            OperandKind::None => (Instruction::without_operand(opcode), at),
            OperandKind::Symbol | OperandKind::Address | OperandKind::Int | OperandKind::Bool => {
                let value = read_u32(bytes, at, "operand")?;
                (Instruction::with_u32(opcode, value), at + 4)
            }
            OperandKind::Float => {
                let end = at + 8;
                let slice = bytes.get(at..end).ok_or(Error::Truncated {
                    offset: at,
                    what: "float operand",
                })?;
                let mut raw = [0u8; 8];
                raw.copy_from_slice(slice);
                (Instruction::PushFloat(f64::from_le_bytes(raw)), end)
            }
            OperandKind::String => {
                let (string, end) = read_string(bytes, at, "string operand")?;
                (Instruction::PushString(string), end)
            }
        };
        Ok((instruction, next))
    }

    /// Appends the encoded instruction to `out`.
    ///
    /// Strings must be Latin-1 (every character below U+0100), as in the
    /// files; other characters are replaced by `?`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.opcode().code().to_le_bytes());
        match self {
            Instruction::DefVar(v)
            | Instruction::DefGlobVar(v)
            | Instruction::DefConstVar(v)
            | Instruction::Call(v)
            | Instruction::ExternalCall(v)
            | Instruction::CorrectStack(v)
            | Instruction::PushVar(v)
            | Instruction::PushVarRef(v)
            | Instruction::PopVar(v)
            | Instruction::PushThis(v)
            | Instruction::Jmp(v)
            | Instruction::JmpFalse(v)
            | Instruction::DbgLine(v) => out.extend_from_slice(&v.to_le_bytes()),
            Instruction::PushInt(v) => out.extend_from_slice(&v.to_le_bytes()),
            Instruction::PushBool(v) => out.extend_from_slice(&u32::from(*v).to_le_bytes()),
            Instruction::PushFloat(v) => out.extend_from_slice(&v.to_le_bytes()),
            Instruction::PushString(s) => {
                out.extend(
                    s.chars()
                        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')),
                );
                out.push(0);
            }
            _ => {}
        }
    }

    fn without_operand(opcode: Opcode) -> Instruction {
        match opcode {
            Opcode::Ret => Instruction::Ret,
            Opcode::RetEvent => Instruction::RetEvent,
            Opcode::CallByExp => Instruction::CallByExp,
            Opcode::Scope => Instruction::Scope,
            Opcode::CreateObject => Instruction::CreateObject,
            Opcode::PopEmpty => Instruction::PopEmpty,
            Opcode::PushVarThis => Instruction::PushVarThis,
            Opcode::PushNull => Instruction::PushNull,
            Opcode::PushThisFromStack => Instruction::PushThisFromStack,
            Opcode::PopThis => Instruction::PopThis,
            Opcode::PushByExp => Instruction::PushByExp,
            Opcode::PopByExp => Instruction::PopByExp,
            Opcode::Add => Instruction::Add,
            Opcode::Sub => Instruction::Sub,
            Opcode::Mul => Instruction::Mul,
            Opcode::Div => Instruction::Div,
            Opcode::Modulo => Instruction::Modulo,
            Opcode::Not => Instruction::Not,
            Opcode::And => Instruction::And,
            Opcode::Or => Instruction::Or,
            Opcode::CmpEq => Instruction::CmpEq,
            Opcode::CmpNe => Instruction::CmpNe,
            Opcode::CmpL => Instruction::CmpL,
            Opcode::CmpG => Instruction::CmpG,
            Opcode::CmpLe => Instruction::CmpLe,
            Opcode::CmpGe => Instruction::CmpGe,
            Opcode::CmpStrictEq => Instruction::CmpStrictEq,
            Opcode::CmpStrictNe => Instruction::CmpStrictNe,
            Opcode::PopReg1 => Instruction::PopReg1,
            Opcode::PushReg1 => Instruction::PushReg1,
            other => unreachable!("{other} takes an operand"),
        }
    }

    fn with_u32(opcode: Opcode, value: u32) -> Instruction {
        match opcode {
            Opcode::DefVar => Instruction::DefVar(value),
            Opcode::DefGlobVar => Instruction::DefGlobVar(value),
            Opcode::DefConstVar => Instruction::DefConstVar(value),
            Opcode::Call => Instruction::Call(value),
            Opcode::ExternalCall => Instruction::ExternalCall(value),
            Opcode::CorrectStack => Instruction::CorrectStack(value),
            Opcode::PushVar => Instruction::PushVar(value),
            Opcode::PushVarRef => Instruction::PushVarRef(value),
            Opcode::PopVar => Instruction::PopVar(value),
            Opcode::PushThis => Instruction::PushThis(value),
            Opcode::Jmp => Instruction::Jmp(value),
            Opcode::JmpFalse => Instruction::JmpFalse(value),
            Opcode::DbgLine => Instruction::DbgLine(value),
            Opcode::PushInt => Instruction::PushInt(value as i32),
            Opcode::PushBool => Instruction::PushBool(value != 0),
            other => unreachable!("{other} does not take a 32-bit operand"),
        }
    }
}

impl fmt::Display for Instruction {
    /// The mnemonic and the raw operand: symbols as `#index`, strings
    /// quoted. `Program::format_instruction` resolves symbol names.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opcode = self.opcode();
        f.write_str(opcode.mnemonic())?;
        match self {
            Instruction::PushInt(v) => write!(f, " {v}"),
            Instruction::PushBool(v) => write!(f, " {v}"),
            Instruction::PushFloat(v) => write!(f, " {v:?}"),
            Instruction::PushString(s) => write!(f, " {s:?}"),
            Instruction::CorrectStack(v) | Instruction::DbgLine(v) => write!(f, " {v}"),
            Instruction::Call(a) | Instruction::Jmp(a) | Instruction::JmpFalse(a) => {
                write!(f, " {a}")
            }
            _ => match self.symbol() {
                Some(index) => write!(f, " #{index}"),
                None => Ok(()),
            },
        }
    }
}

/// Reads a little-endian `u32` at `offset`.
pub(crate) fn read_u32(bytes: &[u8], offset: usize, what: &'static str) -> Result<u32> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or(Error::Truncated { offset, what })?;
    let mut raw = [0u8; 4];
    raw.copy_from_slice(slice);
    Ok(u32::from_le_bytes(raw))
}

/// Reads a NUL-terminated Latin-1 string at `offset`, returning it with the
/// offset following the terminator.
pub(crate) fn read_string(
    bytes: &[u8],
    offset: usize,
    what: &'static str,
) -> Result<(String, usize)> {
    let rest = bytes
        .get(offset..)
        .ok_or(Error::Truncated { offset, what })?;
    let len = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or(Error::Truncated { offset, what })?;
    let string = rest[..len].iter().map(|&b| char::from(b)).collect();
    Ok((string, offset + len + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcodes_are_numbered_in_order() {
        for (i, op) in Opcode::ALL.iter().enumerate() {
            assert_eq!(op.code() as usize, i);
            assert_eq!(Opcode::from_u32(op.code()), Some(*op));
            assert_eq!(Opcode::from_mnemonic(op.mnemonic()), Some(*op));
        }
        assert_eq!(Opcode::from_u32(47), None);
        assert_eq!(Opcode::from_u32(u32::MAX), None);
    }

    #[test]
    fn every_instruction_round_trips() {
        let all = vec![
            Instruction::DefVar(1),
            Instruction::DefGlobVar(2),
            Instruction::Ret,
            Instruction::RetEvent,
            Instruction::Call(1234),
            Instruction::CallByExp,
            Instruction::ExternalCall(3),
            Instruction::Scope,
            Instruction::CorrectStack(2),
            Instruction::CreateObject,
            Instruction::PopEmpty,
            Instruction::PushVar(4),
            Instruction::PushVarRef(5),
            Instruction::PopVar(6),
            Instruction::PushVarThis,
            Instruction::PushInt(-42),
            Instruction::PushBool(true),
            Instruction::PushBool(false),
            Instruction::PushFloat(-1.5e-3),
            Instruction::PushString("héllo \"quoted\"".to_string()),
            Instruction::PushString(String::new()),
            Instruction::PushNull,
            Instruction::PushThisFromStack,
            Instruction::PushThis(7),
            Instruction::PopThis,
            Instruction::PushByExp,
            Instruction::PopByExp,
            Instruction::Jmp(99),
            Instruction::JmpFalse(100),
            Instruction::Add,
            Instruction::Sub,
            Instruction::Mul,
            Instruction::Div,
            Instruction::Modulo,
            Instruction::Not,
            Instruction::And,
            Instruction::Or,
            Instruction::CmpEq,
            Instruction::CmpNe,
            Instruction::CmpL,
            Instruction::CmpG,
            Instruction::CmpLe,
            Instruction::CmpGe,
            Instruction::CmpStrictEq,
            Instruction::CmpStrictNe,
            Instruction::DbgLine(12),
            Instruction::PopReg1,
            Instruction::PushReg1,
            Instruction::DefConstVar(8),
        ];
        let mut bytes = Vec::new();
        let mut offsets = Vec::new();
        for instruction in &all {
            offsets.push(bytes.len());
            instruction.encode(&mut bytes);
        }
        let mut offset = 0;
        for (expected, start) in all.iter().zip(&offsets) {
            assert_eq!(offset, *start);
            let (decoded, next) = Instruction::decode(&bytes, offset).unwrap();
            assert_eq!(&decoded, expected);
            assert_eq!(
                decoded.opcode().operand() == OperandKind::None,
                next == offset + 4
            );
            offset = next;
        }
        assert_eq!(offset, bytes.len());
        let seen: std::collections::HashSet<Opcode> = all.iter().map(|i| i.opcode()).collect();
        assert_eq!(seen.len(), Opcode::ALL.len());
    }

    #[test]
    fn decoding_errors_name_the_offset() {
        assert_eq!(
            Instruction::decode(&[1, 2], 0),
            Err(Error::Truncated {
                offset: 0,
                what: "opcode"
            })
        );
        assert_eq!(
            Instruction::decode(&47u32.to_le_bytes(), 0),
            Err(Error::UnknownOpcode {
                offset: 0,
                opcode: 47
            })
        );
        let mut bytes = Vec::new();
        Instruction::PushInt(1).encode(&mut bytes);
        bytes.truncate(6);
        assert_eq!(
            Instruction::decode(&bytes, 0),
            Err(Error::Truncated {
                offset: 4,
                what: "operand"
            })
        );
        let mut bytes = Vec::new();
        Instruction::PushString("abc".to_string()).encode(&mut bytes);
        bytes.pop();
        assert_eq!(
            Instruction::decode(&bytes, 0),
            Err(Error::Truncated {
                offset: 4,
                what: "string operand"
            })
        );
    }

    #[test]
    fn display_shows_raw_operands() {
        assert_eq!(Instruction::PushVar(3).to_string(), "PUSH_VAR #3");
        assert_eq!(Instruction::PushInt(-1).to_string(), "PUSH_INT -1");
        assert_eq!(Instruction::PushFloat(2.0).to_string(), "PUSH_FLOAT 2.0");
        assert_eq!(
            Instruction::PushString("a\"b".to_string()).to_string(),
            "PUSH_STRING \"a\\\"b\""
        );
        assert_eq!(Instruction::JmpFalse(10).to_string(), "JMP_FALSE 10");
        assert_eq!(Instruction::Add.to_string(), "ADD");
    }
}
