//! Tests of whole compilations, construct by construct, against the
//! output `docs/compiler.md` derives from the original compiler.

use luma_script::{Instruction, Program};

use crate::{Compiler, Natives};

fn compile(source: &str) -> Vec<u8> {
    Compiler::new(Natives::wme())
        .compile(source.as_bytes(), "demo.script", &mut |_: &[u8]| None)
        .unwrap_or_else(|err| panic!("{err}"))
}

/// The code as `luma script` lists it, one instruction per line, with
/// symbol names and absolute targets.
fn listing(bytes: &[u8]) -> Vec<String> {
    let program = Program::parse(bytes.to_vec()).unwrap();
    program
        .disassemble()
        .unwrap()
        .into_iter()
        .map(|(_, instruction)| program.format_instruction(&instruction))
        .collect()
}

#[test]
fn the_complete_example_of_the_specification() {
    let source = "global G = 1;\nvar x = 2;\nfunction add(a, b)\n{\n  return a + b;\n}\non \"LeftClick\"\n{\n  x = add(1, 2);\n  Game.Msg(\"hi\");\n}\n";
    let bytes = compile(source);
    assert_eq!(bytes.len(), 450);
    let program = Program::parse(bytes.clone()).unwrap();
    let header = program.header();
    assert_eq!(header.code, 44);
    assert_eq!(header.functions, 375);
    assert_eq!(header.symbols, 387);
    assert_eq!(header.events, 424);
    assert_eq!(header.externals, 442);
    assert_eq!(header.methods, 446);
    assert_eq!(program.symbols(), &["G", "x", "a", "b", "Game"]);
    assert_eq!(program.functions()[0].position, 124);
    assert_eq!(program.events()[0].position, 240);
    let code = program.disassemble().unwrap();
    let at = |offset: usize| {
        code.iter()
            .find(|(o, _)| *o == offset)
            .map(|(_, i)| i.clone())
            .unwrap_or_else(|| panic!("no instruction at {offset}"))
    };
    assert_eq!(at(44), Instruction::DbgLine(1));
    assert_eq!(at(108), Instruction::DbgLine(6));
    assert_eq!(at(116), Instruction::Jmp(224));
    assert_eq!(at(124), Instruction::Scope);
    assert_eq!(at(176), Instruction::DbgLine(5));
    assert_eq!(at(208), Instruction::DbgLine(6));
    assert_eq!(at(224), Instruction::DbgLine(11));
    assert_eq!(at(232), Instruction::Jmp(363));
    assert_eq!(at(244), Instruction::DbgLine(9));
    assert_eq!(at(351), Instruction::DbgLine(11));
    assert_eq!(at(363), Instruction::DbgLine(12));
    assert_eq!(at(371), Instruction::Ret);
    assert_eq!(
        listing(&bytes),
        [
            "DBG_LINE 1",
            "DEF_GLOB_VAR G",
            "PUSH_INT 1",
            "POP_VAR G",
            "DBG_LINE 2",
            "DEF_VAR x",
            "PUSH_INT 2",
            "POP_VAR x",
            "DBG_LINE 6",
            "JMP 224",
            "SCOPE",
            "CORRECT_STACK 2",
            "DBG_LINE 3",
            "DEF_VAR a",
            "POP_VAR a",
            "DEF_VAR b",
            "POP_VAR b",
            "DBG_LINE 5",
            "PUSH_VAR a",
            "PUSH_VAR b",
            "ADD",
            "RET",
            "DBG_LINE 6",
            "PUSH_NULL",
            "RET",
            "DBG_LINE 11",
            "JMP 363",
            "SCOPE",
            "DBG_LINE 9",
            "PUSH_INT 2",
            "PUSH_INT 1",
            "PUSH_INT 2",
            "CALL 124",
            "POP_VAR x",
            "DBG_LINE 10",
            "PUSH_THIS Game",
            "PUSH_STRING \"hi\"",
            "PUSH_INT 1",
            "PUSH_VAR Game",
            "PUSH_STRING \"Msg\"",
            "CALL_BY_EXP",
            "POP_EMPTY",
            "POP_THIS",
            "DBG_LINE 11",
            "RET_EVENT",
            "DBG_LINE 12",
            "RET",
        ]
    );
}
