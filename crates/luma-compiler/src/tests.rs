//! Tests of whole compilations, construct by construct, against the
//! output `docs/compiler.md` derives from the original compiler. The
//! constructs the White Chamber corpus does not exercise (`continue`,
//! `new`, floats, `*`, `%`, `===`, `const`, `external`, string escapes,
//! casts, multi-name declarations, a `break` after a nested loop) were
//! also checked against a port of the original on the same snippets.

use luma_script::{CallType, ExternalType, Instruction, Program};

use crate::{Compiler, Error, Natives};

fn compile_with(source: &str, includes: &mut dyn crate::Includes) -> Result<Vec<u8>, Error> {
    Compiler::new(Natives::wme()).compile(source.as_bytes(), "demo.script", includes)
}

fn compile(source: &str) -> Vec<u8> {
    compile_with(source, &mut |_: &[u8]| None).unwrap_or_else(|err| panic!("{err}"))
}

fn error(source: &str) -> Error {
    compile_with(source, &mut |_: &[u8]| None).expect_err("should not compile")
}

/// The code, one instruction per line, with symbol names, and jump and
/// call targets as `@n`, the index of the instruction they land on (a
/// target that is no instruction, such as the header, stays a number).
fn code(bytes: &[u8]) -> Vec<String> {
    let program = Program::parse(bytes.to_vec()).unwrap();
    let decoded = program.disassemble().unwrap();
    let index_of = |target: u32| {
        decoded
            .iter()
            .position(|(offset, _)| *offset == target as usize)
            .map_or_else(|| target.to_string(), |i| format!("@{i}"))
    };
    decoded
        .iter()
        .map(|(_, instruction)| match instruction {
            Instruction::Jmp(t) => format!("JMP {}", index_of(*t)),
            Instruction::JmpFalse(t) => format!("JMP_FALSE {}", index_of(*t)),
            Instruction::Call(t) => format!("CALL {}", index_of(*t)),
            other => program.format_instruction(other),
        })
        .collect()
}

fn names(entries: &[luma_script::Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
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
    let offsets: Vec<usize> = program
        .disassemble()
        .unwrap()
        .into_iter()
        .map(|(o, _)| o)
        .collect();
    assert_eq!(offsets[0], 44);
    assert_eq!(offsets[8], 108);
    assert_eq!(offsets[10], 124);
    assert_eq!(offsets[25], 224);
    assert_eq!(offsets[27], 240);
    assert_eq!(offsets[45], 363);
    assert_eq!(
        code(&bytes),
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
            "JMP @25",
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
            "JMP @45",
            "SCOPE",
            "DBG_LINE 9",
            "PUSH_INT 2",
            "PUSH_INT 1",
            "PUSH_INT 2",
            "CALL @10",
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

#[test]
fn the_worked_example_of_line_numbers() {
    let source = "var a = 1 +\n   2\n   ;\nvar x;\nif (a == 3)\n  a = 4;\n\n\n// comment\nvar y;\nSleep(a,\n  a\n);\n";
    assert_eq!(
        code(&compile(source)),
        [
            "DBG_LINE 1",
            "DEF_VAR a",
            "PUSH_INT 1",
            "DBG_LINE 2",
            "PUSH_INT 2",
            "DBG_LINE 3",
            "ADD",
            "POP_VAR a",
            "DBG_LINE 4",
            "DEF_VAR x",
            "DBG_LINE 5",
            "PUSH_VAR a",
            "PUSH_INT 3",
            "CMP_EQ",
            "DBG_LINE 10",
            "JMP_FALSE @19",
            "DBG_LINE 6",
            "PUSH_INT 4",
            "POP_VAR a",
            "DBG_LINE 10",
            "DEF_VAR y",
            "DBG_LINE 13",
            "PUSH_VAR a",
            "DBG_LINE 11",
            "PUSH_VAR a",
            "DBG_LINE 13",
            "PUSH_INT 2",
            "EXTERNAL_CALL Sleep",
            "POP_EMPTY",
            "DBG_LINE 14",
            "RET",
        ]
    );
}

#[test]
fn continue_in_a_while_jumps_to_the_condition() {
    let source =
        "var i = 0;\nwhile (i < 3) {\n  i = i + 1;\n  if (i == 2) continue;\n  Sleep(1);\n}\n";
    assert_eq!(
        code(&compile(source)),
        [
            "DBG_LINE 1",
            "DEF_VAR i",
            "PUSH_INT 0",
            "POP_VAR i",
            "DBG_LINE 2",
            "PUSH_VAR i",
            "PUSH_INT 3",
            "CMP_L",
            "JMP_FALSE @29",
            "DBG_LINE 3",
            "PUSH_VAR i",
            "PUSH_INT 1",
            "ADD",
            "POP_VAR i",
            "DBG_LINE 4",
            "PUSH_VAR i",
            "PUSH_INT 2",
            "CMP_EQ",
            "DBG_LINE 5",
            "JMP_FALSE @22",
            "DBG_LINE 4",
            "JMP @4",
            "DBG_LINE 5",
            "PUSH_INT 1",
            "PUSH_INT 1",
            "EXTERNAL_CALL Sleep",
            "POP_EMPTY",
            "DBG_LINE 6",
            "JMP @4",
            "DBG_LINE 7",
            "RET",
        ]
    );
}

#[test]
fn continue_in_a_for_jumps_to_the_update() {
    let source = "for (var i = 0; i < 2; i = i + 1) {\n  if (i) continue;\n  Sleep(1);\n}\n";
    assert_eq!(
        code(&compile(source)),
        [
            "DBG_LINE 1",
            "DEF_VAR i",
            "PUSH_INT 0",
            "POP_VAR i",
            "PUSH_VAR i",
            "PUSH_INT 2",
            "CMP_L",
            "JMP_FALSE @26",
            "DBG_LINE 2",
            "PUSH_VAR i",
            "DBG_LINE 3",
            "JMP_FALSE @14",
            "DBG_LINE 2",
            "JMP @19",
            "DBG_LINE 3",
            "PUSH_INT 1",
            "PUSH_INT 1",
            "EXTERNAL_CALL Sleep",
            "POP_EMPTY",
            "DBG_LINE 1",
            "PUSH_VAR i",
            "PUSH_INT 1",
            "ADD",
            "POP_VAR i",
            "DBG_LINE 4",
            "JMP @4",
            "DBG_LINE 5",
            "RET",
        ]
    );
}

#[test]
fn switch_fall_through_continue_and_the_label_bug() {
    // `continue` in a `switch` restarts the switch; the `break` after the
    // switch has a `loop_nest` no loop matches, so it is `JMP 0`.
    let source = "var x = 1;\nwhile (true) {\n  switch (x) {\n    case 1: x = 2; continue;\n    case 2: break;\n  }\n  break;\n}\n";
    assert_eq!(
        code(&compile(source)),
        [
            "DBG_LINE 1",
            "DEF_VAR x",
            "PUSH_INT 1",
            "POP_VAR x",
            "DBG_LINE 2",
            "PUSH_BOOL true",
            "JMP_FALSE @36",
            "DBG_LINE 3",
            "PUSH_VAR x",
            "DBG_LINE 6",
            "POP_REG1",
            "DBG_LINE 4",
            "PUSH_INT 1",
            "DBG_LINE 5",
            "PUSH_REG1",
            "CMP_EQ",
            "JMP_FALSE @23",
            "DBG_LINE 4",
            "PUSH_INT 2",
            "POP_VAR x",
            "JMP @7",
            "DBG_LINE 5",
            "JMP @28",
            "PUSH_INT 2",
            "DBG_LINE 6",
            "PUSH_REG1",
            "CMP_EQ",
            "JMP_FALSE @32",
            "DBG_LINE 5",
            "JMP @32",
            "DBG_LINE 6",
            "JMP @32",
            "DBG_LINE 7",
            "JMP 0",
            "DBG_LINE 8",
            "JMP @4",
            "DBG_LINE 9",
            "RET",
        ]
    );
}

#[test]
fn break_after_a_nested_loop_is_jmp_0() {
    let source = "var a = 1;\nwhile (a) {\n  while (a) { a = 0; }\n  if (a) break;\n}\n";
    let listing = code(&compile(source));
    assert_eq!(listing[listing.len() - 5], "JMP 0");
    // The inner loop's own jumps resolve.
    assert_eq!(
        &listing[4..16],
        [
            "DBG_LINE 2",
            "PUSH_VAR a",
            "JMP_FALSE @21",
            "DBG_LINE 3",
            "PUSH_VAR a",
            "JMP_FALSE @13",
            "PUSH_INT 0",
            "POP_VAR a",
            "JMP @7",
            "DBG_LINE 4",
            "PUSH_VAR a",
            "DBG_LINE 5",
        ]
    );
}

#[test]
fn default_only_and_empty_switches() {
    let source = "var a = 1;\nswitch (a) { default: Sleep(1); }\nswitch (a) { }\n";
    assert_eq!(
        code(&compile(source)),
        [
            "DBG_LINE 1",
            "DEF_VAR a",
            "PUSH_INT 1",
            "POP_VAR a",
            "DBG_LINE 2",
            "PUSH_VAR a",
            "POP_REG1",
            "PUSH_INT 1",
            "PUSH_INT 1",
            "EXTERNAL_CALL Sleep",
            "POP_EMPTY",
            "DBG_LINE 3",
            "PUSH_VAR a",
            "POP_REG1",
            "DBG_LINE 4",
            "RET",
        ]
    );
}

#[test]
fn new_objects_native_and_script_constructors() {
    let source = "var o = new Object();\nvar p = new Object;\nnew Object(1);\n";
    let bytes = compile(source);
    assert_eq!(
        code(&bytes),
        [
            "DBG_LINE 1",
            "DEF_VAR o",
            "CREATE_OBJECT",
            "PUSH_THIS_FROM_STACK",
            "PUSH_INT 0",
            "EXTERNAL_CALL Object",
            "POP_THIS",
            "POP_EMPTY",
            "POP_VAR o",
            "DBG_LINE 2",
            "DEF_VAR p",
            "CREATE_OBJECT",
            "PUSH_THIS_FROM_STACK",
            "PUSH_INT 0",
            "EXTERNAL_CALL Object",
            "POP_THIS",
            "POP_EMPTY",
            "POP_VAR p",
            "DBG_LINE 3",
            "CREATE_OBJECT",
            "PUSH_THIS_FROM_STACK",
            "PUSH_INT 1",
            "PUSH_INT 1",
            "EXTERNAL_CALL Object",
            "POP_THIS",
            "POP_EMPTY",
            "POP_EMPTY",
            "DBG_LINE 4",
            "RET",
        ]
    );
    assert_eq!(
        Program::parse(bytes).unwrap().symbols(),
        &["o", "Object", "p"]
    );

    let source = "function C(a) { this.a = a; }\nvar o = new C(5);\n";
    assert_eq!(
        code(&compile(source)),
        [
            "DBG_LINE 2",
            "DEF_VAR o",
            "CREATE_OBJECT",
            "PUSH_THIS_FROM_STACK",
            "PUSH_INT 5",
            "PUSH_INT 1",
            "CALL @12",
            "POP_THIS",
            "POP_EMPTY",
            "POP_VAR o",
            // line 1 again: the function body comes after the top level.
            "DBG_LINE 1",
            "JMP @22",
            "SCOPE",
            "CORRECT_STACK 1",
            "DEF_VAR a",
            "POP_VAR a",
            "PUSH_VAR a",
            "PUSH_VAR_REF this",
            "PUSH_STRING \"a\"",
            "POP_BY_EXP",
            "PUSH_NULL",
            "RET",
            "DBG_LINE 3",
            "RET",
        ]
    );
}

#[test]
fn floats_and_arithmetic_precedence() {
    // `%` binds tighter than `*` and `/`: `1.5 * 2 / 4 % 3 - -1` is
    // `((1.5 * 2) / (4 % 3)) - (-1)`.
    let source = "var f = 1.5 * 2 / 4 % 3 - -1;\n";
    let bytes = compile(source);
    assert_eq!(
        code(&bytes),
        [
            "DBG_LINE 1",
            "DEF_VAR f",
            "PUSH_FLOAT 1.5",
            "PUSH_INT 2",
            "MUL",
            "PUSH_INT 4",
            "PUSH_INT 3",
            "MODULO",
            "DIV",
            "PUSH_INT 0",
            "PUSH_INT 1",
            "SUB",
            "SUB",
            "POP_VAR f",
            "DBG_LINE 2",
            "RET",
        ]
    );
    let program = Program::parse(bytes).unwrap();
    let (_, float) = &program.disassemble().unwrap()[2];
    assert_eq!(float, &Instruction::PushFloat(1.5));
    let listing = code(&compile("var g = 5.;\nvar h = 0.25;\n"));
    assert_eq!(listing[2], "PUSH_FLOAT 5.0");
    assert_eq!(listing[6], "PUSH_FLOAT 0.25");
}

#[test]
fn comparisons_and_logic_without_short_circuit() {
    let source = "var a = 1;\nvar b = a === 1 !== false;\nvar c = a && Random(0, 1) || false;\nvar d = a < b == c > a;\n";
    assert_eq!(
        code(&compile(source))[4..],
        [
            "DBG_LINE 2",
            "DEF_VAR b",
            "PUSH_VAR a",
            "PUSH_INT 1",
            "CMP_STRICT_EQ",
            "PUSH_BOOL false",
            "CMP_STRICT_NE",
            "POP_VAR b",
            "DBG_LINE 3",
            "DEF_VAR c",
            "PUSH_VAR a",
            "PUSH_INT 1",
            "PUSH_INT 0",
            "PUSH_INT 2",
            "EXTERNAL_CALL Random",
            "AND",
            "PUSH_BOOL false",
            "OR",
            "POP_VAR c",
            "DBG_LINE 4",
            "DEF_VAR d",
            "PUSH_VAR a",
            "PUSH_VAR b",
            "CMP_L",
            "PUSH_VAR c",
            "CMP_EQ",
            "PUSH_VAR a",
            "CMP_G",
            "POP_VAR d",
            "DBG_LINE 5",
            "RET",
        ]
    );
}

#[test]
fn const_global_and_multi_name_declarations() {
    let source = "const K = 1;\nglobal G;\nglobal H = K;\n";
    assert_eq!(
        code(&compile(source)),
        [
            "DBG_LINE 1",
            "DEF_CONST_VAR K",
            "PUSH_INT 1",
            "POP_VAR K",
            "DBG_LINE 2",
            "DEF_GLOB_VAR G",
            "DBG_LINE 3",
            "DEF_GLOB_VAR H",
            "PUSH_VAR K",
            "POP_VAR H",
            "DBG_LINE 4",
            "RET",
        ]
    );
    // One `DEF_VAR` per name, the initializer generated once per name, and
    // the second `POP_VAR` on the final line of the file (the split
    // declaration is created after parsing).
    let source = "var a, b = Random(1, 2);\n\n";
    let bytes = compile(source);
    assert_eq!(
        code(&bytes),
        [
            "DBG_LINE 1",
            "DEF_VAR a",
            "PUSH_INT 2",
            "PUSH_INT 1",
            "PUSH_INT 2",
            "EXTERNAL_CALL Random",
            "POP_VAR a",
            "DEF_VAR b",
            "PUSH_INT 2",
            "PUSH_INT 1",
            "PUSH_INT 2",
            "EXTERNAL_CALL Random",
            "DBG_LINE 3",
            "POP_VAR b",
            "RET",
        ]
    );
    assert_eq!(
        Program::parse(bytes).unwrap().symbols(),
        &["a", "Random", "b"]
    );
    assert_eq!(
        code(&compile("var a, b;\n")),
        ["DBG_LINE 1", "DEF_VAR a", "DEF_VAR b", "DBG_LINE 2", "RET"]
    );
}

#[test]
fn externals_fill_the_table_and_call_externally() {
    let source = "external \"kernel32.dll\" stdcall long GetTickCount();\nexternal \"user32.dll\" cdecl Beep(long t, string);\nvar t = GetTickCount();\nBeep(1, \"x\");\n";
    let bytes = compile(source);
    assert_eq!(
        code(&bytes),
        [
            "DBG_LINE 3",
            "DEF_VAR t",
            "PUSH_INT 0",
            "EXTERNAL_CALL GetTickCount",
            "POP_VAR t",
            "DBG_LINE 4",
            "PUSH_STRING \"x\"",
            "PUSH_INT 1",
            "PUSH_INT 2",
            "EXTERNAL_CALL Beep",
            "POP_EMPTY",
            "DBG_LINE 5",
            "RET",
        ]
    );
    let program = Program::parse(bytes).unwrap();
    assert_eq!(program.symbols(), &["t", "GetTickCount", "Beep"]);
    let externals = program.externals();
    assert_eq!(externals.len(), 2);
    // Reverse source order.
    assert_eq!(externals[0].dll, "user32.dll");
    assert_eq!(externals[0].name, "Beep");
    assert_eq!(externals[0].call_type, CallType::CDecl);
    assert_eq!(externals[0].returns, ExternalType::Void);
    assert_eq!(
        externals[0].params,
        vec![ExternalType::Long, ExternalType::String]
    );
    assert_eq!(externals[1].dll, "kernel32.dll");
    assert_eq!(externals[1].name, "GetTickCount");
    assert_eq!(externals[1].call_type, CallType::StdCall);
    assert_eq!(externals[1].returns, ExternalType::Long);
    assert!(externals[1].params.is_empty());
    // All the types; `short` and `int` are `long`.
    let bytes = compile(
        "external \"x.dll\" thiscall membuffer F(bool, byte, float, double, short, int, string);\n",
    );
    let program = Program::parse(bytes).unwrap();
    assert_eq!(program.externals()[0].call_type, CallType::ThisCall);
    assert_eq!(program.externals()[0].returns, ExternalType::MemBuffer);
    assert_eq!(
        program.externals()[0].params,
        vec![
            ExternalType::Bool,
            ExternalType::Byte,
            ExternalType::Float,
            ExternalType::Double,
            ExternalType::Long,
            ExternalType::Long,
            ExternalType::String,
        ]
    );
    // An unused external is in the table but not among the symbols.
    assert!(program.symbols().is_empty());
}

#[test]
fn string_escapes_and_bytes() {
    let bytes = Compiler::new(Natives::wme())
        .compile(
            b"var s = \"a~nb~~c~\"d~x\\e \xe9\";\n",
            "demo.script",
            &mut |_: &[u8]| None,
        )
        .unwrap();
    let program = Program::parse(bytes).unwrap();
    let (_, push) = &program.disassemble().unwrap()[2];
    assert_eq!(
        push,
        &Instruction::PushString("a\nb~c\"d~x\\e \u{e9}".to_string())
    );
}

#[test]
fn a_cast_generates_nothing_at_all() {
    assert_eq!(
        code(&compile("var a = (var) 5;\nvar b = 1 + (global) a;\n")),
        [
            "DBG_LINE 1",
            "DEF_VAR a",
            "POP_VAR a",
            "DBG_LINE 2",
            "DEF_VAR b",
            "PUSH_INT 1",
            "ADD",
            "POP_VAR b",
            "DBG_LINE 3",
            "RET",
        ]
    );
}

#[test]
fn members_and_method_calls() {
    let source = "var o;\no.x = 1;\no[\"y\"] = o.x;\no.M(1, 2);\nvar r = o[\"M\"]();\n";
    assert_eq!(
        code(&compile(source)),
        [
            "DBG_LINE 1",
            "DEF_VAR o",
            "DBG_LINE 2",
            "PUSH_INT 1",
            "PUSH_VAR_REF o",
            "PUSH_STRING \"x\"",
            "POP_BY_EXP",
            "DBG_LINE 3",
            "PUSH_VAR_REF o",
            "PUSH_STRING \"x\"",
            "PUSH_BY_EXP",
            "PUSH_VAR_REF o",
            "PUSH_STRING \"y\"",
            "POP_BY_EXP",
            "DBG_LINE 4",
            "PUSH_THIS o",
            "PUSH_INT 2",
            "PUSH_INT 1",
            "PUSH_INT 2",
            "PUSH_VAR o",
            "PUSH_STRING \"M\"",
            "CALL_BY_EXP",
            "POP_EMPTY",
            "POP_THIS",
            "DBG_LINE 5",
            "DEF_VAR r",
            "PUSH_THIS o",
            "PUSH_INT 0",
            "PUSH_VAR o",
            "PUSH_STRING \"M\"",
            "CALL_BY_EXP",
            "POP_THIS",
            "POP_VAR r",
            "DBG_LINE 6",
            "RET",
        ]
    );
}

#[test]
fn chained_assignment_and_assignment_in_a_condition() {
    assert_eq!(
        code(&compile(
            "var x; var y;\nx = y = 1;\nif (x = y) Sleep(1);\n"
        )),
        [
            "DBG_LINE 1",
            "DEF_VAR x",
            "DEF_VAR y",
            "DBG_LINE 2",
            "PUSH_INT 1",
            "POP_VAR y",
            "POP_VAR x",
            "DBG_LINE 3",
            "PUSH_VAR y",
            "POP_VAR x",
            "DBG_LINE 4",
            "JMP_FALSE @17",
            "DBG_LINE 3",
            "PUSH_INT 1",
            "PUSH_INT 1",
            "EXTERNAL_CALL Sleep",
            "POP_EMPTY",
            "DBG_LINE 4",
            "RET",
        ]
    );
}

#[test]
fn returns_and_events_with_any_statement() {
    let source = "function f(a) {\n  if (a) return;\n  return a;\n}\non \"E\" Sleep(1);\non \"F\" { if (true) return; }\n";
    let bytes = compile(source);
    assert_eq!(
        code(&bytes),
        [
            "DBG_LINE 4",
            "JMP @20",
            "SCOPE",
            "CORRECT_STACK 1",
            "DBG_LINE 1",
            "DEF_VAR a",
            "POP_VAR a",
            "DBG_LINE 2",
            "PUSH_VAR a",
            "DBG_LINE 3",
            "JMP_FALSE @14",
            "DBG_LINE 2",
            "PUSH_NULL",
            "RET",
            "DBG_LINE 3",
            "PUSH_VAR a",
            "RET",
            "DBG_LINE 4",
            "PUSH_NULL",
            "RET",
            // `on "E" Sleep(1);`: the line is the statement's `;`.
            "DBG_LINE 5",
            "JMP @28",
            "SCOPE",
            "PUSH_INT 1",
            "PUSH_INT 1",
            "EXTERNAL_CALL Sleep",
            "POP_EMPTY",
            "RET_EVENT",
            "DBG_LINE 6",
            "JMP @35",
            "SCOPE",
            "PUSH_BOOL true",
            "JMP_FALSE @34",
            "RET_EVENT",
            "RET_EVENT",
            "DBG_LINE 7",
            "RET",
        ]
    );
    let program = Program::parse(bytes).unwrap();
    assert_eq!(names(program.events()), ["F", "E"]);
}

#[test]
fn tables_are_in_reverse_source_order_and_symbols_in_first_use_order() {
    let source = "function f() { }\nmethod m() { }\nfunction g() { f(); }\nmethod n() { }\non \"A\" { }\non \"B\" { }\nvar x;\non \"A\" { x = 1; }\n";
    let program = Program::parse(compile(source)).unwrap();
    assert_eq!(names(program.functions()), ["g", "f"]);
    assert_eq!(names(program.methods()), ["n", "m"]);
    assert_eq!(names(program.events()), ["A", "B", "A"]);
    // Positions decrease along each table; the engine's "last match"
    // lookup therefore finds the first handler in the source.
    assert!(program.events()[0].position > program.events()[2].position);
    assert_eq!(
        program.event_position("A"),
        Some(program.events()[2].position)
    );
    assert_eq!(program.symbols(), &["x"]);
}

#[test]
fn includes_carry_the_directive_line_and_resolve_through_the_host() {
    let mut includes = |name: &[u8]| -> Option<Vec<u8>> {
        match name {
            b"scripts\\base.inc" => {
                Some(b"global G;\n\nglobal H = 1;\n#include \"scripts\\const.inc\"\n".to_vec())
            }
            b"scripts\\const.inc" => Some(b"\xEF\xBB\xBFconst K = 2;".to_vec()),
            _ => None,
        }
    };
    let source = "\n#include \"scripts\\base.inc\"\nvar y = G + K;\n";
    let bytes = compile_with(source, &mut includes).unwrap();
    assert_eq!(
        code(&bytes),
        [
            "DBG_LINE 2",
            "DEF_GLOB_VAR G",
            "DEF_GLOB_VAR H",
            "PUSH_INT 1",
            "POP_VAR H",
            "DEF_CONST_VAR K",
            "PUSH_INT 2",
            "POP_VAR K",
            "DBG_LINE 3",
            "DEF_VAR y",
            "PUSH_VAR G",
            "PUSH_VAR K",
            "ADD",
            "POP_VAR y",
            "DBG_LINE 4",
            "RET",
        ]
    );
    let err = compile_with("#include \"missing.inc\"", &mut includes).unwrap_err();
    assert_eq!(err.message, "Error opening include file missing.inc");
    assert_eq!(
        (err.file.as_str(), err.line, err.column),
        ("demo.script", 1, 1)
    );
    // No include guard: a double include declares twice.
    let err = compile_with(
        "#include \"scripts\\const.inc\"\n#include \"scripts\\const.inc\"\n",
        &mut includes,
    )
    .unwrap_err();
    assert_eq!(err.message, "Duplicate declaration of variable 'K'");
    assert_eq!(err.file, "scripts\\const.inc");
    assert_eq!((err.line, err.column), (1, 7));
}

#[test]
fn the_header_and_an_empty_script() {
    let bytes = compile("");
    assert_eq!(bytes.len(), 44 + 12 + 4 * 5);
    let program = Program::parse(bytes).unwrap();
    assert_eq!(program.version(), "1.2");
    assert_eq!(program.filename(), "demo.script");
    assert_eq!(program.header().code, 44);
    assert_eq!(code(program.bytes()), ["DBG_LINE 1", "RET"]);
    assert!(program.symbols().is_empty());
    // A file with only newlines ends on its last line.
    assert_eq!(code(&compile("\n\n\n")), ["DBG_LINE 4", "RET"]);
    // Unterminated last line, comments and regions at the end.
    assert_eq!(code(&compile("var a;")), ["DBG_LINE 1", "DEF_VAR a", "RET"]);
    assert_eq!(
        code(&compile("var a;\n// end\n#region x")),
        ["DBG_LINE 1", "DEF_VAR a", "DBG_LINE 3", "RET"]
    );
}

#[test]
fn big_integers_saturate() {
    let listing = code(&compile("var a = 2147483647;\nvar b = 99999999999;\n"));
    assert_eq!(listing[2], "PUSH_INT 2147483647");
    assert_eq!(listing[6], "PUSH_INT 2147483647");
}

#[test]
fn placement_and_symbol_errors() {
    let cases: [(&str, &str, u32, u32); 18] = [
        ("break;", "Invalid break", 1, 1),
        ("continue;", "Invalid continue", 1, 1),
        (
            "switch (1) { case 1: continue; }",
            "Invalid continue",
            1,
            22,
        ),
        (
            "on \"X\" { return 1; }",
            "Event handler cannot return a value",
            1,
            10,
        ),
        (
            "function f() { }\nfunction f() { }",
            "Duplicate declaration of function 'f'",
            2,
            10,
        ),
        (
            "function Sleep() { }",
            "Duplicate declaration of function 'Sleep'",
            1,
            10,
        ),
        (
            "var f;\nexternal \"x.dll\" f();",
            "Duplicate declaration of function 'f'",
            2,
            18,
        ),
        ("x = 1;", "Variable 'x' is referenced but not defined", 1, 1),
        (
            "function f() { return g; }\nvar g;",
            "Variable 'g' is referenced but not defined",
            1,
            23,
        ),
        (
            "var i = i;",
            "Variable 'i' is referenced but not defined",
            1,
            9,
        ),
        ("Sleep = 2;", "Identifier 'Sleep' is not a variable", 1, 1),
        ("f();", "Function 'f' is referenced but not defined", 1, 1),
        (
            "var f; f();",
            "Trying to use variable f as a function",
            1,
            8,
        ),
        (
            "new X;",
            "Object 'X' is referenced but no constuctor is defined",
            1,
            5,
        ),
        (
            "var X; new X;",
            "Trying to use variable X as an object",
            1,
            12,
        ),
        (
            "o.M();",
            "Method is called for 'o' which is not defined",
            1,
            1,
        ),
        (
            "function o() { }\no.M();",
            "Trying to call method of a non-variable o",
            2,
            1,
        ),
        (
            "var a; var a;",
            "Duplicate declaration of variable 'a'",
            1,
            12,
        ),
    ];
    for (source, message, line, column) in cases {
        let err = error(source);
        assert_eq!(err.message, message, "{source}");
        assert_eq!((err.line, err.column), (line, column), "{source}: {err}");
        assert_eq!(err.file, "demo.script");
    }
    assert_eq!(
        error("function f(a, a) { }").message,
        "Duplicate declaration of formal 'a'"
    );
    // Allowed: forward calls, shadowing, later receivers, `break` in a
    // switch, `continue` in a switch inside a loop.
    for source in [
        "f();\nfunction f() { }",
        "var x;\nfunction f() { var x; return x; }",
        "on \"X\" { o.M(); }\nvar o;",
        "switch (1) { case 1: break; }",
        "while (true) { switch (1) { case 1: continue; } }",
        "var x;\non \"X\" { x = 1; }",
    ] {
        compile(source);
    }
}

#[test]
fn natives_are_a_parameter() {
    let plugin = Natives::wme().function("Plugin").variable("Shell");
    let bytes = Compiler::new(plugin)
        .compile(
            b"var p = new Plugin();\nShell.Run(\"x\");\n",
            "demo.script",
            &mut |_: &[u8]| None,
        )
        .unwrap();
    let listing = code(&bytes);
    assert!(listing.contains(&"EXTERNAL_CALL Plugin".to_string()));
    assert!(listing.contains(&"PUSH_THIS Shell".to_string()));
    let err = Compiler::new(Natives::empty())
        .compile(b"Sleep(1);", "demo.script", &mut |_: &[u8]| None)
        .unwrap_err();
    assert_eq!(
        err.message,
        "Function 'Sleep' is referenced but not defined"
    );
}
