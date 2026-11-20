//! The debug overlay (`CBGame::DisplayDebugInfo` and its overrides):
//! which lines it draws, where, and when.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::debug_info::{DebugLine, ScriptCounts};
use luma_adventure::{Audio, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_formats::Image;
use luma_formats::font::TextAlign;
use luma_render::{DrawList, Gpu, Renderer};
use luma_script::Value;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-debug-overlay-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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
Game.ChangeScene("room.scene", false, false);
on "Scroll" {
  var scene = Game.Scene;
  scene.OffsetX = 42;
  scene.OffsetY = 28;
}
"#;

const ROOM: &str = "SCENE {
  NAME=\"Room\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=128 HEIGHT=96
    ENTITY { NAME=\"bg\" X=0 Y=0 SPRITE=\"blue.png\" REGISTRABLE=FALSE }
    ENTITY { NAME=\"box\" X=28 Y=20 SPRITE=\"red.png\" }
  }
}";

/// A 64x48 game, started with the `wme.ini` given (the game's own
/// presets), and run for two frames.
fn start(ini: Option<&str>) -> Runtime {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 64,48 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Debug\" SCRIPT=\"game.script\" SYSTEM_FONT=\"sys.font\" }".to_vec(),
    );
    let script = Compiler::new(Natives::wme())
        .compile(SCRIPT.as_bytes(), "game.script", &mut |_: &[u8]| None)
        .unwrap();
    pkg.add_file("game.script", script);
    pkg.add_file("room.scene", ROOM.as_bytes().to_vec());
    pkg.add_file("red.png", png(8, 8, [255, 0, 0, 255]));
    pkg.add_file("blue.png", png(64, 48, [0, 0, 255, 255]));
    pkg.add_file("font.png", png(128, 128, [255, 255, 255, 255]));
    pkg.add_file(
        "sys.font",
        b"FONT { IMAGE=\"font.png\" COLUMNS=16 TILE_WIDTH=8 TILE_HEIGHT=8 DEFAULT_WIDTH=6 }"
            .to_vec(),
    );
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    if let Some(ini) = ini {
        std::fs::write(dir.join("wme.ini"), ini).unwrap();
    }
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    runtime
}

fn texts(runtime: &Runtime) -> Vec<&str> {
    runtime
        .world
        .debug_lines()
        .iter()
        .map(|l| l.text.as_str())
        .collect()
}

fn line<'a>(runtime: &'a Runtime, prefix: &str) -> &'a DebugLine {
    runtime
        .world
        .debug_lines()
        .iter()
        .find(|l| l.text.starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix:?} in {:?}", texts(runtime)))
}

#[test]
fn nothing_is_drawn_without_a_switch() {
    let runtime = start(None);
    assert!(!runtime.world.debug_mode());
    assert!(!runtime.world.show_fps());
    assert!(runtime.world.debug_lines().is_empty());
}

#[test]
fn the_frame_rate_alone_is_a_line_at_the_top_left() {
    let mut runtime = start(Some("[Debug]\nShowFPS=1\n"));
    assert!(runtime.world.show_fps());
    assert!(!runtime.world.debug_mode());
    runtime.tick(16);
    // `DrawText("FPS: n", 0, 0, 100, TAL_LEFT)`, and nothing else.
    let fps = runtime.world.fps();
    assert_eq!(
        runtime.world.debug_lines(),
        &[DebugLine {
            text: format!("FPS: {fps}"),
            x: 0,
            y: 0,
            width: 100,
            align: TextAlign::Left,
        }]
    );
}

#[test]
fn debug_mode_lists_the_engine_state_right_aligned_in_the_original_rows() {
    let mut runtime = start(Some("[Debug]\nDebugMode=1\n"));
    assert!(runtime.world.debug_mode());
    runtime.mouse_move(30, 22);
    runtime.tick(16);
    let rows: Vec<(i32, i32)> = runtime
        .world
        .debug_lines()
        .iter()
        .map(|l| (l.y, l.width))
        .collect();
    assert_eq!(
        rows,
        [
            (90, 64),
            (110, 64),
            (0, 64),
            (20, 64),
            (70, 64),
            (130, 64),
            (150, 64)
        ]
    );
    assert!(
        runtime
            .world
            .debug_lines()
            .iter()
            .all(|l| l.x == 0 && l.align == TextAlign::Right)
    );
    assert_eq!(
        line(&runtime, "Mouse").text,
        "Mouse: 30, 22 (scene: 30, 22)"
    );
    assert_eq!(line(&runtime, "Scene").text, "Scene: Room (prev: ???)");
    assert_eq!(line(&runtime, "Mode").text, "Mode: 64x48x32 (wgpu)");
    assert_eq!(
        line(&runtime, "Timer").text,
        format!("Timer: {}", runtime.world.timer())
    );
    // The mouse is over the entity `box`: its name is the last line.
    assert_eq!(line(&runtime, "box").text, "box");
}

#[test]
fn the_mode_line_says_windowed_and_the_scene_offset_is_added_to_the_mouse() {
    let mut runtime = start(Some("[Debug]\nDebugMode=1\n"));
    runtime.world.set_windowed(true);
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Scroll", false);
    runtime.tick(16);
    assert_eq!(runtime.world.offset(), (10, 4));
    runtime.mouse_move(1, 2);
    runtime.tick(16);
    assert_eq!(line(&runtime, "Mode").text, "Mode: 64x48 windowed (wgpu)");
    assert_eq!(line(&runtime, "Mouse").text, "Mouse: 1, 2 (scene: 11, 6)");
}

#[test]
fn a_switch_set_by_the_front_end_turns_the_overlay_on() {
    let mut runtime = start(None);
    runtime.world.set_debug_mode(true);
    runtime.tick(16);
    assert!(texts(&runtime).iter().any(|t| t.starts_with("PolyCount")));
    runtime.world.set_debug_mode(false);
    runtime.tick(16);
    assert!(runtime.world.debug_lines().is_empty());
}

#[test]
fn the_script_line_counts_the_scripts_as_the_engine_does() {
    let mut runtime = start(Some("[Debug]\nDebugMode=1\n"));
    let counts = ScriptCounts::of(&runtime.engine);
    // game.script stays attached for its handlers: persistent.
    assert!(counts.total >= 1);
    assert_eq!(
        counts.total,
        counts.running + counts.waiting + counts.persistent
    );
    runtime.tick(16);
    assert_eq!(
        line(&runtime, "Running scripts").text,
        format!(
            "Running scripts: {} (r:{} w:{} p:{})",
            counts.total, counts.running, counts.waiting, counts.persistent
        )
    );
}

#[test]
fn game_debug_mode_reads_the_switch() {
    let mut runtime = start(Some("[Debug]\nDebugMode=1\n"));
    let game = runtime.world.game_id();
    let value = luma_script::Natives::get_property(&runtime.world, game, "DebugMode");
    assert_eq!(value, Some(Value::Bool(true)));
    runtime.world.set_debug_mode(false);
    let value = luma_script::Natives::get_property(&runtime.world, game, "DebugMode");
    assert_eq!(value, Some(Value::Bool(false)));
}

#[test]
fn the_overlay_is_drawn_over_the_frame_when_a_renderer_is_there() {
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(err) => {
            eprintln!("skipped: {err}");
            return;
        }
    };
    let renderer = Renderer::new(gpu);
    let mut runtime = start(None);
    let quads = |runtime: &mut Runtime| {
        runtime.world.upload(&renderer);
        let mut list = DrawList::new(64.0, 48.0);
        runtime.draw(&mut list, Some(renderer.white()));
        (list.quads().len(), list.polygon_count())
    };
    let (plain, plain_polygons) = quads(&mut runtime);
    assert_eq!(plain_polygons as usize, plain * 2);
    runtime.world.set_debug_mode(true);
    runtime.tick(16);
    let (with, _) = quads(&mut runtime);
    assert!(with > plain, "{with} quads over {plain}");
    // The polygons of the last frame are shown by the next one.
    runtime.tick(16);
    let shown = line(&runtime, "PolyCount").text.clone();
    assert_eq!(shown, format!("PolyCount: {}", with * 2));
}
