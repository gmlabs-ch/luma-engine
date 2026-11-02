//! The oracle: every script source of The White Chamber (a local, git-ignored
//! test game) compiled by this crate must be byte for byte the compiled
//! script the game ships. Skips when the game is absent.
//!
//! Layout under `testdata/local/white-chamber/`: `unpacked/` holds the
//! game's packages as `luma unpack` extracts them (the compiled scripts,
//! `data.dcp` and `language.dcp` flattened together), and `source/` the
//! WME project of the source pack (`WhiteChamber/data`, `WhiteChamber/
//! language`, with the clear-text scripts and includes).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use luma_compiler::{Compiler, GameDirs, Natives};
use luma_script::Program;

fn white_chamber() -> Option<(PathBuf, PathBuf)> {
    let game = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/local/white-chamber");
    let unpacked = game.join("unpacked");
    let source = ["source/WhiteChamber", "source"]
        .iter()
        .map(|s| game.join(s))
        .find(|s| s.join("data").is_dir())?;
    if !unpacked.is_dir() {
        return None;
    }
    Some((source, unpacked))
}

fn scripts_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            scripts_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "script") {
            out.push(path);
        }
    }
}

/// The first differing offset, with the instructions around it in both
/// files when they decode.
fn describe_difference(expected: &[u8], actual: &[u8]) -> String {
    let first = expected
        .iter()
        .zip(actual)
        .position(|(a, b)| a != b)
        .unwrap_or(expected.len().min(actual.len()));
    let mut out = format!(
        "first difference at offset {first} (expected {} bytes, got {})\n",
        expected.len(),
        actual.len()
    );
    for (label, bytes) in [("expected", expected), ("actual", actual)] {
        let _ = writeln!(out, "  {label}:");
        match Program::parse(bytes.to_vec()) {
            Ok(program) => {
                let _ = writeln!(
                    out,
                    "    header: code {} functions {} symbols {} events {} externals {} methods {}",
                    program.header().code,
                    program.header().functions,
                    program.header().symbols,
                    program.header().events,
                    program.header().externals,
                    program.header().methods
                );
                match program.disassemble() {
                    Ok(code) => {
                        let index = code
                            .iter()
                            .position(|(offset, _)| *offset >= first)
                            .unwrap_or(code.len());
                        let from = index.saturating_sub(6);
                        let to = (index + 6).min(code.len());
                        for (offset, instruction) in &code[from..to] {
                            let mark = if *offset <= first
                                && code
                                    .get(code.iter().position(|(o, _)| o == offset).unwrap() + 1)
                                    .is_none_or(|(next, _)| *next > first)
                            {
                                ">>"
                            } else {
                                "  "
                            };
                            let _ = writeln!(
                                out,
                                "    {mark} {offset:>6}  {}",
                                program.format_instruction(instruction)
                            );
                        }
                        if first >= program.code_end() {
                            let _ = writeln!(
                                out,
                                "    (in the tables: {} symbols {:?}...)",
                                program.symbols().len(),
                                program.symbols().iter().take(8).collect::<Vec<_>>()
                            );
                        }
                    }
                    Err(err) => {
                        let _ = writeln!(out, "    cannot disassemble: {err}");
                    }
                }
            }
            Err(err) => {
                let _ = writeln!(out, "    cannot parse: {err}");
            }
        }
        let from = first.saturating_sub(16);
        let to = (first + 16).min(bytes.len());
        let hex: Vec<String> = bytes[from..to].iter().map(|b| format!("{b:02x}")).collect();
        let _ = writeln!(out, "    bytes {from}..{to}: {}", hex.join(" "));
    }
    out
}

#[test]
fn every_white_chamber_script_compiles_identically() {
    let Some((source, unpacked)) = white_chamber() else {
        eprintln!("skipped: The White Chamber sources are not in testdata/local");
        return;
    };
    let data = source.join("data");
    let language = source.join("language");
    let compiler = Compiler::new(Natives::wme());

    let mut total = 0;
    let mut identical = 0;
    let mut report = String::new();
    for root in [&data, &language] {
        let mut scripts = Vec::new();
        scripts_under(root, &mut scripts);
        scripts.sort();
        for path in scripts {
            let relative = path.strip_prefix(root).unwrap();
            let compiled = unpacked.join(relative);
            let Ok(expected) = fs::read(&compiled) else {
                continue;
            };
            total += 1;
            let name = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\\");
            let text = fs::read(&path).unwrap();
            let mut includes = GameDirs::new(&data).and(&language);
            match compiler.compile(&text, &name, &mut includes) {
                Ok(actual) if actual == expected => identical += 1,
                Ok(actual) => {
                    let _ = writeln!(
                        report,
                        "{}: {}",
                        relative.display(),
                        describe_difference(&expected, &actual)
                    );
                }
                Err(err) => {
                    let _ = writeln!(report, "{}: error: {err}", relative.display());
                }
            }
        }
    }
    eprintln!("The White Chamber: {identical} of {total} scripts identical");
    assert!(
        report.is_empty(),
        "{} of {total} scripts differ:\n{report}",
        total - identical
    );
    assert!(total >= 276, "expected at least 276 scripts, found {total}");
}
