//! End-to-end tests of the `luma` binary on synthetic game data.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use luma_archive::PackageBuilder;
use luma_script::Instruction;
use luma_script::asm::Assembler;

fn luma(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_luma"))
        .args(args)
        .output()
        .expect("run luma")
}

fn temp_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-cli-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A game with a base package and a higher-priority patch package in
/// `data/`, sharing one file.
fn sample_game() -> PathBuf {
    let game = temp_dir();
    fs::create_dir_all(game.join("data")).unwrap();

    let mut base = PackageBuilder::new("data");
    base.add_file("default.game", b"GAME { NAME=\"base\" }".to_vec());
    base.add_file(
        "scenes\\Room\\room.scene",
        "SCENE { }".repeat(200).into_bytes(),
    )
    .modified(1_000_000_000);
    base.add_stored_file("sounds/beep.ogg", vec![1, 2, 3]);
    base.add_file("scripts\\room.script", sample_script());
    fs::write(game.join("data.dcp"), base.build()).unwrap();

    let mut patch = PackageBuilder::new("patch");
    patch.priority(1);
    patch.add_file("DEFAULT.GAME", b"GAME { NAME=\"patched\" }".to_vec());
    fs::write(game.join("data/patch.dcp"), patch.build()).unwrap();

    fs::write(game.join("readme.txt"), b"loose file, not a package").unwrap();
    game
}

/// `var x = 1 + 2; on "LeftClick" { Sleep(10); }` as the compiler lays
/// it out.
fn sample_script() -> Vec<u8> {
    let mut a = Assembler::new("scripts\\room.script");
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
    a.bind(end);
    a.emit(Instruction::Ret);
    a.build()
}

#[test]
fn script_disassembles_a_compiled_script() {
    let game = sample_game();
    let out = luma(&["script", game.to_str().unwrap(), "scripts/room.script"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("scripts\\room.script: version 1.2, "),
        "{text}"
    );
    assert!(text.contains("2 symbol(s), 0 function(s), 1 event(s), 0 method(s), 0 external(s)\n"));
    assert!(text.contains("  event LeftClick at "), "{text}");
    assert!(text.contains("event \"LeftClick\":\n"), "{text}");
    assert!(text.contains("  EXTERNAL_CALL Sleep\n"), "{text}");
    assert!(text.contains("  DEF_VAR x\n"), "{text}");
    assert!(text.trim_end().ends_with("RET"), "{text}");

    // Not a script.
    let out = luma(&["script", game.to_str().unwrap(), "default.game"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a compiled script"));

    // Usage.
    let out = luma(&["script", game.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn version_and_help() {
    let out = luma(&["version"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("luma "));

    let out = luma(&[]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("USAGE"));
}

#[test]
fn usage_errors_exit_with_2() {
    for args in [&["list"][..], &["unpack", "x"][..], &["bogus"][..]] {
        let out = luma(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("USAGE"));
    }
}

#[test]
fn list_shows_resolved_files() {
    let game = sample_game();
    let out = luma(&["list", game.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 4);
    // Sorted by normalised name; the patch wins for default.game.
    assert_eq!(lines[0].trim(), "23\tpatch\tDEFAULT.GAME");
    assert_eq!(lines[1].trim(), "1800\tdata\tscenes/Room/room.scene");
    assert!(lines[2].trim().ends_with("\tdata\tscripts/room.script"));
    assert_eq!(lines[3].trim(), "3\tdata\tsounds/beep.ogg");
    assert!(String::from_utf8_lossy(&out.stderr).contains("4 files, "));
    fs::remove_dir_all(&game).unwrap();
}

#[test]
fn list_accepts_a_single_package() {
    let game = sample_game();
    let out = luma(&["list", game.join("data.dcp").to_str().unwrap()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 4);
    assert!(stdout.contains("\tdata\tdefault.game"));
    fs::remove_dir_all(&game).unwrap();
}

#[test]
fn unpack_extracts_everything() {
    let game = sample_game();
    let target = temp_dir().join("out");
    let out = luma(&["unpack", game.to_str().unwrap(), target.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read(target.join("DEFAULT.GAME")).unwrap(),
        b"GAME { NAME=\"patched\" }"
    );
    assert_eq!(
        fs::read(target.join("scenes/Room/room.scene")).unwrap(),
        "SCENE { }".repeat(200).as_bytes()
    );
    assert_eq!(fs::read(target.join("sounds/beep.ogg")).unwrap(), [1, 2, 3]);
    let modified = fs::metadata(target.join("scenes/Room/room.scene"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        modified,
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000)
    );
    fs::remove_dir_all(&game).unwrap();
    fs::remove_dir_all(target.parent().unwrap()).unwrap();
}

#[test]
fn unpack_refuses_names_escaping_the_target() {
    let game = temp_dir();
    let mut evil = PackageBuilder::new("data");
    evil.add_stored_file("..\\..\\escape.txt", b"nope".to_vec());
    fs::write(game.join("evil.dcp"), evil.build()).unwrap();
    let target = game.join("out");
    let out = luma(&["unpack", game.to_str().unwrap(), target.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to extract"));
    assert!(!game.join("escape.txt").exists());
    fs::remove_dir_all(&game).unwrap();
}

#[test]
fn reports_missing_games_and_packages() {
    let out = luma(&["list", "/nonexistent/luma-game"]);
    assert_eq!(out.status.code(), Some(1));

    let empty = temp_dir();
    let out = luma(&["list", empty.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no .dcp package found"));

    fs::write(empty.join("bad.dcp"), b"not a package at all").unwrap();
    let out = luma(&["list", empty.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a WME package"));
    fs::remove_dir_all(&empty).unwrap();
}

#[test]
fn dump_prints_canonical_form_from_disk_and_from_packages() {
    let game = sample_game();
    let out = luma(&["dump", game.to_str().unwrap(), "default.game"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "GAME {\n  NAME = \"patched\"\n}\n"
    );

    let file = game.join("x.sprite");
    fs::write(&file, "SPRITE{LOOPING=TRUE\nFRAME{HOTSPOT{1,2}}}").unwrap();
    let out = luma(&["dump", file.to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "SPRITE {\n  LOOPING = TRUE\n  FRAME {\n    HOTSPOT { 1, 2 }\n  }\n}\n"
    );

    fs::write(&file, "SPRITE {\n  BROKEN = \"\n").unwrap();
    let out = luma(&["dump", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("line 2"));
    fs::remove_dir_all(&game).unwrap();
}

/// A 2x1 RGB PNG: a magenta pixel then a dark one.
const TINY_PNG: [u8; 72] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x7b, 0x40, 0xe8,
    0xdd, 0x00, 0x00, 0x00, 0x0f, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xf0, 0x9f,
    0x4b, 0x44, 0x0e, 0x00, 0x0a, 0x61, 0x02, 0x3b, 0xa5, 0xaa, 0xb2, 0xee, 0x00, 0x00, 0x00, 0x00,
    0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[test]
fn scene_describes_layers_entities_and_regions() {
    let game = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_stored_file("scenes\\bg.png", TINY_PNG.to_vec());
    pkg.add_file(
        "scenes\\room.scene",
        b"SCENE {\n NAME=\"Room\"\n PERSISTENT_STATE=TRUE\n SCALE_LEVEL { Y=10 SCALE=50 }\n LAYER {\n NAME=\"main\"\n MAIN=TRUE\n WIDTH=640\n HEIGHT=480\n ENTITY { NAME=\"bg\" SPRITE=\"scenes\\bg.png\" }\n REGION { NAME=\"floor\" BLOCKED=FALSE POINT {0,0} POINT {10,0} POINT {0,10} }\n ENTITY { NAME=\"door\" X=5 Y=5 SPRITE=\"scenes\\door.sprite\" INTERACTIVE=TRUE REGION { POINT {0,0} } }\n }\n ENTITY { NAME=\"free\" }\n}"
            .to_vec(),
    );
    fs::write(game.join("data.dcp"), pkg.build()).unwrap();

    let out = luma(&["scene", game.to_str().unwrap(), "scenes/ROOM.scene"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"Room\" 640x480"), "{text}");
    assert!(
        text.contains("1 scale level(s), 0 rotation level(s), persistent state"),
        "{text}"
    );
    assert!(
        text.contains("layer 0: \"main\" 640x480 main, 2 entities, 1 regions"),
        "{text}"
    );
    assert!(
        text.contains("entity \"bg\" at 0,0 sprite scenes\\bg.png ["),
        "{text}"
    );
    assert!(text.contains("region \"floor\": 3 points"), "{text}");
    assert!(
        text.contains("sprite scenes\\door.sprite (MISSING) [interactive colorable hotspot]"),
        "{text}"
    );
    assert!(text.contains("free entities: 1"), "{text}");
    assert!(text.contains("sprites: 2 referenced, 1 missing"), "{text}");
    assert!(text.contains("missing: scenes\\door.sprite"), "{text}");

    let out = luma(&["scene", game.to_str().unwrap(), "scenes/missing.scene"]);
    assert_eq!(out.status.code(), Some(1));
    let out = luma(&["scene", game.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    fs::remove_dir_all(&game).unwrap();
}

#[test]
fn sprite_describes_frames_and_images() {
    let game = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_stored_file("sprites\\dot.png", TINY_PNG.to_vec());
    pkg.add_file(
        "sprites\\blink.sprite",
        b"SPRITE {\n NAME=\"blink\"\n LOOPING=TRUE\n FRAME { DELAY=100\n IMAGE=\"sprites\\dot.png\"\n HOTSPOT {1, 0} }\n FRAME { DELAY=50 }\n}"
            .to_vec(),
    );
    fs::write(game.join("data.dcp"), pkg.build()).unwrap();

    let out = luma(&["sprite", game.to_str().unwrap(), "sprites/BLINK.sprite"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"blink\""), "{text}");
    assert!(
        text.contains("2 frame(s), 150 ms total, flags: looping precise"),
        "{text}"
    );
    assert!(
        text.contains("sprites\\dot.png (2x1 color key) hotspot 1,0"),
        "{text}"
    );
    assert!(text.contains("(no image)"), "{text}");

    // An image path is a one-frame sprite.
    let out = luma(&["sprite", game.to_str().unwrap(), "sprites/dot.png"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 frame(s), 0 ms total"));

    let out = luma(&["sprite", game.to_str().unwrap(), "sprites/missing.sprite"]);
    assert_eq!(out.status.code(), Some(1));
    fs::remove_dir_all(&game).unwrap();
}
