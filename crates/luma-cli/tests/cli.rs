//! End-to-end tests of the `luma` binary on synthetic game data.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use luma_archive::PackageBuilder;

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
    fs::write(game.join("data.dcp"), base.build()).unwrap();

    let mut patch = PackageBuilder::new("patch");
    patch.priority(1);
    patch.add_file("DEFAULT.GAME", b"GAME { NAME=\"patched\" }".to_vec());
    fs::write(game.join("data/patch.dcp"), patch.build()).unwrap();

    fs::write(game.join("readme.txt"), b"loose file, not a package").unwrap();
    game
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
    assert_eq!(lines.len(), 3);
    // Sorted by normalised name; the patch wins for default.game.
    assert_eq!(lines[0].trim(), "23\tpatch\tDEFAULT.GAME");
    assert_eq!(lines[1].trim(), "1800\tdata\tscenes/Room/room.scene");
    assert_eq!(lines[2].trim(), "3\tdata\tsounds/beep.ogg");
    assert!(String::from_utf8_lossy(&out.stderr).contains("3 files, 1826 bytes, in 2 package(s)"));
    fs::remove_dir_all(&game).unwrap();
}

#[test]
fn list_accepts_a_single_package() {
    let game = sample_game();
    let out = luma(&["list", game.join("data.dcp").to_str().unwrap()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 3);
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
