//! Runs the player on a synthetic game in screenshot mode.

use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use luma::archive::PackageBuilder;
use luma::formats::Image;

fn player(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_luma-player"))
        .args(args)
        .output()
        .expect("run luma-player")
}

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-player-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn usage_errors() {
    let out = player(&[]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("USAGE"));
    let out = player(&["game", "--screenshot", "x.png"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--scene"));
    let out = player(&["game", "--scene", "x", "--scroll", "abc"]);
    assert_eq!(out.status.code(), Some(2));
    let out = player(&["game", "--headless", "--wheel", "sideways"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--wheel expects"));
    let out = player(&["game", "--scale", "0"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--scale expects"));
    let out = player(&["game", "--headless", "--savegame", "notes.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--savegame expects"));
    let out = player(&["/nonexistent", "--scene", "x", "--screenshot", "x.png"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn renders_a_screenshot() {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { RESOLUTION { 16,8 } }".to_vec(),
    );
    pkg.add_stored_file(
        "bg.png",
        Image {
            width: 16,
            height: 8,
            pixels: [0u8, 255, 0, 255].repeat(16 * 8),
            has_alpha_channel: true,
        }
        .to_png()
        .unwrap(),
    );
    pkg.add_file(
        "room.scene",
        b"SCENE { LAYER { MAIN=TRUE WIDTH=16 HEIGHT=8 ENTITY { SPRITE=\"bg.png\" } } }".to_vec(),
    );
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    let shot = dir.join("shot.png");

    let out = player(&[
        dir.to_str().unwrap(),
        "--scene",
        "room.scene",
        "--screenshot",
        shot.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("no graphics adapter") {
        eprintln!("skipped: {stderr}");
        return;
    }
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains("16x8, 1 layer(s), 1 entities, 1 texture(s)"),
        "{stderr}"
    );
    let image = luma::formats::image::decode(
        &std::fs::read(&shot).unwrap(),
        luma::formats::DEFAULT_COLOR_KEY,
    )
    .unwrap();
    assert_eq!((image.width, image.height), (16, 8));
    let p = image.pixel(8, 4).unwrap();
    assert_eq!((p.r, p.g, p.b), (0, 255, 0));

    let out = player(&[
        dir.to_str().unwrap(),
        "--scene",
        "nope.scene",
        "--screenshot",
        shot.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The headless keyboard steps: a text typed key by key with its
/// punctuation and an accented letter, a held key repeating, the
/// editing keys and the clipboard shortcuts.
#[test]
fn headless_steps_type_into_the_focused_editor() {
    const SCRIPT: &str = r#"
var win = Game.LoadWindow("w.window");
win.Visible = true;
var name = win.GetWidget("name");
name.Focus();
Game.ChangeScene("room.scene");
"#;
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 64,32 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Typing\" SCRIPT=\"game.script\" }".to_vec(),
    );
    pkg.add_file(
        "game.script",
        luma_compiler::Compiler::new(luma_compiler::Natives::wme())
            .compile(SCRIPT.as_bytes(), "game.script", &mut |_: &[u8]| None)
            .unwrap(),
    );
    pkg.add_file(
        "w.window",
        b"WINDOW { NAME=\"w\" X=0 Y=0 WIDTH=60 HEIGHT=30 EDIT { NAME=\"name\" X=1 Y=1 WIDTH=50 HEIGHT=10 } }"
            .to_vec(),
    );
    pkg.add_file(
        "room.scene",
        b"SCENE { NAME=\"Room\" LAYER { NAME=\"main\" MAIN=TRUE WIDTH=64 HEIGHT=32 } }".to_vec(),
    );
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    let run = |steps: &[&str]| -> String {
        let mut args = vec![
            dir.to_str().unwrap(),
            "--headless",
            "--mute",
            "--after",
            "1",
        ];
        args.extend_from_slice(steps);
        let out = player(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    // Punctuation whose ASCII codes are those of Delete and the arrows,
    // and a letter no key of a US keyboard types.
    let out = run(&["--type", "a.b'(é"]);
    assert!(out.contains("text \"a.b'(é\""), "{out}");
    // A held key repeats; Backspace, Home and Delete edit.
    let out = run(&[
        "--type",
        "xy",
        "--repeat-key",
        "65:2",
        "--key",
        "8",
        "--key",
        "36",
        "--key",
        "46",
    ]);
    assert!(out.contains("text \"yaa\""), "{out}");
    // Select all, copy, go to the end, paste.
    let out = run(&[
        "--type", "ab", "--key", "ctrl+65", "--key", "ctrl+67", "--key", "35", "--key", "ctrl+86",
    ]);
    assert!(out.contains("text \"abab\""), "{out}");
    let out = player(&["game", "--headless", "--key", "meta+65"]);
    assert_eq!(out.status.code(), Some(2));
    let out = player(&["game", "--headless", "--repeat-key", "65"]);
    assert_eq!(out.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}
