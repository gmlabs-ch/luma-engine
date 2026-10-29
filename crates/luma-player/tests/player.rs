//! Runs the player on a synthetic game in screenshot mode.

use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use luma_archive::PackageBuilder;
use luma_formats::Image;

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
    let image = luma_formats::image::decode(
        &std::fs::read(&shot).unwrap(),
        luma_formats::DEFAULT_COLOR_KEY,
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
