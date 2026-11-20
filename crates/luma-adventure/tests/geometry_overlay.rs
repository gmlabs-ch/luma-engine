//! The geometry overlay of `Scene.ShowGeometry`: a scene with a floor, a
//! block and waypoints, a script that turns the overlay on, and the lines
//! the frame then holds. Runs only with the `3d` feature; the tests that
//! build a frame are skipped on a machine without a graphics adapter.

#![cfg(feature = "3d")]

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_3d::Vec3;
use luma_3d::three_ds::Builder;
use luma_adventure::{Audio, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_formats::{Image, Rgba};
use luma_render::{DrawList, Gpu, Offscreen, Renderer};
use luma_script::Value;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-geometry-overlay-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile(source: &str) -> Vec<u8> {
    Compiler::new(Natives::wme())
        .compile(source.as_bytes(), "game.script", &mut |_: &[u8]| None)
        .unwrap()
}

fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    Image {
        width,
        height,
        pixels: color.repeat((width * height) as usize),
        has_alpha_channel: true,
    }
    .to_png()
    .unwrap()
}

const SCRIPT: &str = r#"
global result = new Object();
Game.ChangeScene("room.scene", false, false);
on "Show" {
  var scene = Game.Scene;
  result.before = scene.ShowGeometry;
  scene.ShowGeometry = true;
  result.after = scene.ShowGeometry;
}
on "Hide" {
  var scene = Game.Scene;
  scene.ShowGeometry = false;
}
"#;

/// A floor facing up, a wall block, two waypoints, a camera on the `-z`
/// side looking down at the floor.
fn geometry() -> Vec<u8> {
    let floor = Builder::mesh(
        "walk_01",
        &[
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 100.0),
            Vec3::new(100.0, 0.0, 100.0),
            Vec3::new(100.0, 0.0, 0.0),
        ],
        &[[0, 1, 2], [0, 2, 3]],
    );
    let corners = |lo: Vec3, hi: Vec3| {
        [
            Vec3::new(lo.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, hi.z),
            Vec3::new(lo.x, lo.y, hi.z),
            Vec3::new(lo.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, hi.z),
            Vec3::new(lo.x, hi.y, hi.z),
        ]
    };
    let faces: [[u16; 3]; 12] = [
        [0, 1, 2],
        [0, 2, 3],
        [4, 6, 5],
        [4, 7, 6],
        [0, 4, 5],
        [0, 5, 1],
        [3, 6, 7],
        [3, 2, 6],
        [0, 7, 4],
        [0, 3, 7],
        [1, 5, 6],
        [1, 6, 2],
    ];
    let wall = Builder::mesh(
        "blk_wall",
        &corners(Vec3::new(40.0, 0.0, 20.0), Vec3::new(60.0, 30.0, 80.0)),
        &faces,
    );
    let waypoint = |name: &str, x: f32| {
        Builder::mesh(
            name,
            &corners(Vec3::new(x - 1.0, 0.0, 9.0), Vec3::new(x + 1.0, 2.0, 11.0)),
            &faces,
        )
    };
    let camera = Builder::camera(
        "Camera01",
        Vec3::new(50.0, 120.0, -120.0),
        Vec3::new(50.0, 0.0, 50.0),
        0.0,
        38.0,
    );
    Builder::file(
        &[
            floor,
            wall,
            waypoint("wpt_a", 30.0),
            waypoint("wpt_b", 70.0),
            camera,
        ],
        &[],
    )
}

fn start(marker: bool) -> Runtime {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 200,150 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Geometry\" SCRIPT=\"game.script\" } AD_GAME { }".to_vec(),
    );
    pkg.add_file("game.script", compile(SCRIPT));
    pkg.add_file(
        "room.scene",
        b"SCENE {
  NAME=\"Room\"
  GEOMETRY=\"geo.3ds\"
  CAMERA=\"Camera01\"
  LAYER { NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=150 }
}"
        .to_vec(),
    );
    pkg.add_file("geo.3ds", geometry());
    if marker {
        pkg.add_file("wpt.png", png(4, 4, [255, 255, 0, 255]));
        pkg.add_file(
            "wpt.sprite",
            b"SPRITE { FRAME { DELAY=100 IMAGE=\"wpt.png\" } }".to_vec(),
        );
    }
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    runtime
}

/// A property of the `result` global, a `new Object()` the world owns.
fn result(runtime: &Runtime, key: &str) -> Value {
    runtime
        .engine
        .global("result")
        .and_then(Value::as_native)
        .and_then(|id| runtime.world.objects().get(id))
        .and_then(|native| native.props.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

fn raise(runtime: &mut Runtime, event: &str) {
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, event, false);
    runtime.tick(16);
}

/// The quads of a frame, with the white texture standing in for the
/// renderer's when there is none.
fn quads(runtime: &mut Runtime, renderer: &Renderer) -> Vec<(luma_render::Rect, Rgba)> {
    runtime.world.upload(renderer);
    let mut list = DrawList::new(200.0, 150.0);
    runtime.draw(&mut list, Some(renderer.white()));
    list.quads().iter().map(|q| (q.dst, q.tint)).collect()
}

fn renderer() -> Option<Renderer> {
    match Gpu::headless() {
        Ok(gpu) => Some(Renderer::new(gpu)),
        Err(err) => {
            eprintln!("skipped: {err}");
            None
        }
    }
}

#[test]
fn the_property_is_stored_and_read_back_by_scripts() {
    let mut runtime = start(false);
    assert!(!runtime.world.scene().unwrap().scene.show_geometry);
    raise(&mut runtime, "Show");
    assert_eq!(result(&runtime, "before"), Value::Bool(false));
    assert_eq!(result(&runtime, "after"), Value::Bool(true));
    assert!(runtime.world.scene().unwrap().scene.show_geometry);
    raise(&mut runtime, "Hide");
    assert!(!runtime.world.scene().unwrap().scene.show_geometry);
}

#[test]
fn the_overlay_draws_blue_floor_and_red_block_lines_only_when_asked() {
    let Some(renderer) = renderer() else { return };
    let mut runtime = start(false);
    let plain = quads(&mut runtime, &renderer);
    assert!(
        plain.iter().all(|(_, tint)| tint.a == 255),
        "no line without the switch: {plain:?}"
    );
    raise(&mut runtime, "Show");
    let shown = quads(&mut runtime, &renderer);
    let lines = |color: [u8; 3]| {
        shown
            .iter()
            .filter(|(_, t)| t.a == 0x70 && [t.r, t.g, t.b] == color)
            .count()
    };
    assert!(lines([0, 0, 255]) > 0, "walk plane edges are blue");
    assert!(lines([255, 0, 0]) > 0, "block edges are red");
    assert_eq!(lines([0, 255, 0]), 0, "no generic mesh in the file");
    // The waypoint meshes are not drawn as geometry.
    raise(&mut runtime, "Hide");
    let hidden = quads(&mut runtime, &renderer);
    assert!(hidden.iter().all(|(_, tint)| tint.a == 255));
}

#[test]
fn a_marker_the_game_ships_is_shown_at_each_waypoint() {
    let Some(renderer) = renderer() else { return };
    let mut runtime = start(true);
    raise(&mut runtime, "Show");
    // The marker is loaded and uploaded on the next upload.
    quads(&mut runtime, &renderer);
    let shown = quads(&mut runtime, &renderer);
    let markers = shown
        .iter()
        .filter(|(rect, tint)| tint.a == 255 && rect.w == 4.0 && rect.h == 4.0)
        .count();
    assert_eq!(markers, 2, "one per waypoint: {shown:?}");
}

#[test]
fn the_lines_land_on_the_screen_where_the_floor_is() {
    let Some(renderer) = renderer() else { return };
    let mut runtime = start(false);
    raise(&mut runtime, "Show");
    runtime.world.upload(&renderer);
    let target = Offscreen::new(renderer.gpu(), 200, 150);
    let mut list = DrawList::new(200.0, 150.0);
    runtime.draw(&mut list, Some(renderer.white()));
    renderer
        .render(
            target.target(),
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            &list,
        )
        .unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    // Some pixel is bluish, none of them has a red component without a
    // block edge being there: the first is enough to tell that the
    // translucent lines reached the target.
    let bluish = (0..150)
        .flat_map(|y| (0..200).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = image.pixel(x, y).unwrap();
            p.b > 60 && p.r == 0 && p.g == 0
        })
        .count();
    assert!(bluish > 20, "{bluish} blue pixels");
}

#[test]
fn a_saved_game_keeps_the_switch() {
    let mut runtime = start(false);
    raise(&mut runtime, "Show");
    runtime.save_game(1, "overlay", true).unwrap();
    raise(&mut runtime, "Hide");
    assert!(!runtime.world.scene().unwrap().scene.show_geometry);
    runtime.load_game(1).unwrap();
    runtime.tick(16);
    assert!(runtime.world.scene().unwrap().scene.show_geometry);
}
