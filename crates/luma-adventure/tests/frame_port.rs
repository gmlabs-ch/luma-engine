//! What the engine does every frame and how it draws in 2D, against the
//! original engine's rules, one behaviour per test: viewports and what
//! they clip, the scene-change transition, the clocks of sprites, the
//! cursors, the scene's nodes and the controls.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_formats::{Image, Rgba};
use luma_render::{DrawList, Gpu, Offscreen, Renderer};
use luma_script::{NativeId, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-frame-port-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile(source: &str, name: &str) -> Vec<u8> {
    Compiler::new(Natives::wme())
        .compile(source.as_bytes(), name, &mut |_: &[u8]| None)
        .unwrap()
}

/// An image of one colour.
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

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// A file of the test game.
#[allow(dead_code)]
enum File<'a> {
    /// A script, compiled.
    Script(&'a str, &'a str),
    /// A definition file, as text.
    Text(&'a str, &'a str),
    /// Any other file, as it is.
    Raw(&'a str, Vec<u8>),
}

const GAME_FILE: &str = "GAME { NAME=\"Frame\" SCRIPT=\"game.script\" SYSTEM_FONT=\"sys.font\" }";

/// A 64x48 game whose `game.script` is `source`, with the extra files,
/// started and run for two frames of 16 ms. Returns its directory, the
/// runtime and the log of those frames.
fn start_with(
    game_file: &str,
    source: &str,
    files: &[File<'_>],
) -> (std::path::PathBuf, Runtime, Vec<String>) {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 64,48 } }".to_vec(),
    );
    pkg.add_file("default.game", game_file.as_bytes().to_vec());
    pkg.add_file("game.script", compile(source, "game.script"));
    pkg.add_file("red.png", png(8, 8, RED));
    pkg.add_file("green.png", png(8, 8, GREEN));
    pkg.add_file("blue.png", png(64, 48, BLUE));
    pkg.add_file("font.png", png(128, 128, [255, 255, 255, 255]));
    pkg.add_file(
        "sys.font",
        b"FONT { IMAGE=\"font.png\" COLUMNS=16 TILE_WIDTH=8 TILE_HEIGHT=8 DEFAULT_WIDTH=6 }"
            .to_vec(),
    );
    for file in files {
        match file {
            File::Script(name, source) => pkg.add_file(*name, compile(source, name)),
            File::Text(name, text) => pkg.add_file(*name, text.as_bytes().to_vec()),
            File::Raw(name, bytes) => pkg.add_file(*name, bytes.clone()),
        };
    }
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let mut log = runtime.tick(16);
    log.extend(runtime.tick(16));
    (dir, runtime, log)
}

fn start(source: &str, files: &[File<'_>]) -> (std::path::PathBuf, Runtime, Vec<String>) {
    start_with(GAME_FILE, source, files)
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

/// A native object the script kept in `result`.
#[allow(dead_code)]
fn object(runtime: &Runtime, key: &str) -> NativeId {
    match result(runtime, key) {
        Value::Native(id) => id,
        other => panic!("{key} is {other:?}, not an object"),
    }
}

fn no_errors(log: &[String]) {
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
}

/// Raises an event on the game object and runs a frame, so that the
/// handler of `game.script` has run.
fn raise(runtime: &mut Runtime, event: &str) -> Vec<String> {
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, event, false);
    runtime.tick(16)
}

/// The object under a point of the screen, as the next frame sees it.
fn object_at(runtime: &mut Runtime, x: i32, y: i32) -> Option<NativeId> {
    runtime.mouse_move(x, y);
    runtime.tick(1);
    runtime.world.active_object()
}

/// The frame as it is drawn, or `None` on a machine without a graphics
/// adapter (the test then checks what it can without the pixels).
fn render(runtime: &mut Runtime) -> Option<Image> {
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(err) => {
            eprintln!("skipped: {err}");
            return None;
        }
    };
    let renderer = Renderer::new(gpu);
    runtime.world.upload(&renderer);
    let target = Offscreen::new(renderer.gpu(), 64, 48);
    let mut list = DrawList::new(64.0, 48.0);
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
    Some(target.read(renderer.gpu()).unwrap())
}

fn rgb(image: &Image, x: u32, y: u32) -> (u8, u8, u8) {
    let p = image.pixel(x, y).unwrap();
    (p.r, p.g, p.b)
}

/// A scene whose background fills the screen, with an 8x8 red entity
/// `box` at (28, 20).
const ROOM: &str = "SCENE {
  NAME=\"Room\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=64 HEIGHT=48
    ENTITY { NAME=\"bg\" X=0 Y=0 SPRITE=\"blue.png\" REGISTRABLE=FALSE }
    ENTITY { NAME=\"box\" X=28 Y=20 SPRITE=\"red.png\" }
  }
}";

#[test]
fn a_scene_is_drawn_and_hit_inside_its_viewport_only() {
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
Game.ChangeScene("room.scene");
// Zero or less means the screen's size; the rectangle is kept on the
// screen.
result.first = Game.SetSceneViewport(-5, 0, 0, 0);
on "Second" {
  Game.SetSceneViewport(16, 8, 16, 16);
}
"#,
        &[File::Text("room.scene", ROOM)],
    );
    no_errors(&log);
    assert_eq!(result(&runtime, "first"), Value::Bool(true));
    assert_eq!(runtime.world.viewport(), (0, 0, 59, 48));
    raise(&mut runtime, "Second");
    assert_eq!(runtime.world.viewport(), (16, 8, 16, 16));
    assert_eq!(runtime.world.scene_clip(), Some((16, 8, 32, 24)));
    // The scene is placed at the viewport's corner: the box, at (28, 20)
    // of the scene, is drawn at (44, 28), outside the viewport, and
    // takes no mouse there; the background takes none anywhere.
    assert_eq!(
        object_at(&mut runtime, 46, 30),
        None,
        "outside the viewport"
    );
    // Move the box into the viewport: its left half is inside.
    let scene = runtime.world.scene_mut().unwrap();
    let (layer, index) = scene.find_node("box").unwrap();
    let luma_formats::SceneNode::Entity(entity) = &mut scene.scene.layers[layer].nodes[index]
    else {
        panic!("box is an entity");
    };
    entity.x = 12;
    entity.y = 4;
    // Drawn at (28, 12) to (36, 20): the viewport ends at x 32.
    let hit = object_at(&mut runtime, 30, 14);
    assert!(hit.is_some(), "inside the viewport");
    assert_eq!(object_at(&mut runtime, 33, 14), None, "cut by the viewport");
    assert_eq!(
        runtime.world.hit_rect(hit.unwrap()),
        Some((28, 12, 4, 8)),
        "only the part inside the viewport"
    );
    // Past the fade-in of the scene change.
    for _ in 0..20 {
        runtime.tick(16);
    }
    if let Some(frame) = render(&mut runtime) {
        assert_eq!(rgb(&frame, 30, 14), (255, 0, 0), "the box inside");
        assert_eq!(rgb(&frame, 33, 14), (0, 0, 0), "the box cut at the edge");
        assert_eq!(rgb(&frame, 17, 22), (0, 0, 255), "the background inside");
        assert_eq!(rgb(&frame, 15, 22), (0, 0, 0), "left of the viewport");
        assert_eq!(rgb(&frame, 20, 25), (0, 0, 0), "below the viewport");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_window_that_clips_its_contents_cuts_its_controls() {
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
var w = Game.LoadWindow("clip.window");
result.window = w;
result.button = w.GetControl("wide");
result.clip = w.ClipContents;
var open = Game.LoadWindow("open.window");
result.open = open.GetControl("wide");
"#,
        &[
            File::Text(
                "open.window",
                "WINDOW {
  NAME=\"open\" X=8 Y=28 WIDTH=16 HEIGHT=16 TRANSPARENT=TRUE
  BUTTON { NAME=\"wide\" X=4 Y=4 WIDTH=40 HEIGHT=8 IMAGE=\"green.png\" }
}",
            ),
            File::Text(
                "clip.window",
                "WINDOW {
  NAME=\"clip\" X=8 Y=8 WIDTH=16 HEIGHT=16 CLIP_CONTENTS=TRUE TRANSPARENT=TRUE
  BUTTON { NAME=\"wide\" X=4 Y=4 WIDTH=40 HEIGHT=8 IMAGE=\"green.png\" }
}",
            ),
        ],
    );
    no_errors(&log);
    assert_eq!(result(&runtime, "clip"), Value::Bool(true));
    let button = object(&runtime, "button");
    // The button spans x 12..52 on screen; the window ends at x 24.
    assert_eq!(object_at(&mut runtime, 20, 14), Some(button));
    assert_eq!(object_at(&mut runtime, 30, 14), None, "outside the window");
    assert_eq!(runtime.world.hit_rect(button), Some((12, 12, 12, 8)));
    if let Some(frame) = render(&mut runtime) {
        assert_eq!(rgb(&frame, 14, 14), (0, 255, 0), "the button's image");
        assert_eq!(rgb(&frame, 25, 14), (0, 0, 0), "nothing past the window");
    }
    // Without the switch the button takes the mouse over its whole width.
    let open = object(&runtime, "open");
    assert_eq!(object_at(&mut runtime, 30, 34), Some(open));
    std::fs::remove_dir_all(dir).ok();
}

const HALL: &str = "SCENE {
  NAME=\"Hall\"
  LAYER { NAME=\"main\" MAIN=TRUE WIDTH=64 HEIGHT=48 }
}";

#[test]
fn a_scene_change_fades_out_first_and_fades_in_on_its_own_layer() {
    use luma_adventure::TransitionKind;
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
// The first change of a game happens at once: the script goes on with
// the scene loaded.
Game.ChangeScene("room.scene", false, false);
var scene = Game.Scene;
result.now = scene.Name;
result.changing = Game.ChangingScene;
on "Go" {
  Game.ChangeScene("hall.scene");
  result.changingLater = Game.ChangingScene;
  result.interactive = Game.Interactive;
}
"#,
        &[
            File::Text("room.scene", ROOM),
            File::Text("hall.scene", HALL),
        ],
    );
    no_errors(&log);
    assert_eq!(result(&runtime, "now"), Value::from("Room"));
    assert_eq!(result(&runtime, "changing"), Value::Bool(false));
    assert!(runtime.world.transition().is_ready(), "no fade was asked");

    // Both fades are on by default. The fade to black comes first, with
    // the game not interactive, and the scene changes only when it is
    // over.
    raise(&mut runtime, "Go");
    assert_eq!(result(&runtime, "changingLater"), Value::Bool(true));
    assert_eq!(result(&runtime, "interactive"), Value::Bool(false));
    assert_eq!(
        runtime.world.transition().kind(),
        Some(TransitionKind::FadeOut)
    );
    assert!(!runtime.world.is_interactive());
    runtime.tick(100);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    let half = runtime.world.transition().opacity().unwrap();
    assert!((120..=135).contains(&half), "{half}");
    assert!(
        runtime.world.fader().color().is_none(),
        "the fader scripts use is left alone"
    );
    runtime.tick(101);
    assert_eq!(runtime.world.transition().opacity(), Some(255));
    assert!(runtime.world.transition().is_ready());
    assert!(runtime.world.is_interactive(), "given back with the end");
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    // The next frame changes the scene and starts the fade from black.
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Hall");
    assert_eq!(
        runtime.world.transition().kind(),
        Some(TransitionKind::FadeIn)
    );
    assert_eq!(runtime.world.transition().opacity(), Some(255));
    assert!(runtime.world.is_interactive());
    if let Some(frame) = render(&mut runtime) {
        assert_eq!(rgb(&frame, 30, 20), (0, 0, 0), "black over everything");
    }
    runtime.tick(201);
    assert!(runtime.world.transition().is_ready());
    runtime.tick(16);
    assert_eq!(runtime.world.transition().opacity(), None);
    std::fs::remove_dir_all(dir).ok();
}

/// A window that fills the screen, for a menu.
const MENU: &str = "WINDOW { NAME=\"menu\" X=0 Y=0 WIDTH=64 HEIGHT=48 TRANSPARENT=TRUE }";

#[test]
fn a_frozen_game_hides_the_sentences_and_keeps_the_quick_messages_running() {
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
global menu;
Game.ChangeScene("room.scene", false, false);
on "Say" {
  var scene = Game.Scene;
  var box = scene.GetNode("box");
  box.Talk("A long sentence that stays for a while");
}
on "Menu" {
  Game.Msg("saved");
  menu = Game.LoadWindow("menu.window");
  menu.GoSystemExclusive();
}
on "Close" {
  menu.Close();
}
"#,
        &[
            File::Text("room.scene", ROOM),
            File::Text("menu.window", MENU),
        ],
    );
    no_errors(&log);
    runtime.world.accessibility_mut().keyboard_pause = true;
    no_errors(&raise(&mut runtime, "Say"));
    runtime.tick(16);
    assert_eq!(runtime.world.sentences().len(), 1);
    assert_eq!(runtime.world.visible_sentences().count(), 1);

    // A system-exclusive window freezes the game: the sentence of a
    // freezable speaker is not drawn, and comes back with the game.
    no_errors(&raise(&mut runtime, "Menu"));
    runtime.tick(16);
    assert!(runtime.world.is_frozen());
    assert_eq!(runtime.world.sentences().len(), 1);
    assert_eq!(runtime.world.visible_sentences().count(), 0);
    // A quick message lasts three seconds of the live clock, which the
    // freeze does not stop.
    assert_eq!(
        runtime.world.quick_messages().len(),
        1,
        "{:?}",
        runtime.world.quick_messages()
    );
    let timer = runtime.world.timer();
    for _ in 0..4 {
        runtime.tick(800);
    }
    assert_eq!(runtime.world.timer(), timer, "the game timer stands still");
    assert!(runtime.world.quick_messages().is_empty());
    raise(&mut runtime, "Close");
    runtime.tick(16);
    assert!(!runtime.world.is_frozen());
    assert_eq!(runtime.world.visible_sentences().count(), 1);

    // The accessibility pause freezes the game too, but shows every
    // sentence.
    runtime.key_down(17, None, false, false, true);
    runtime.key_down(32, Some(' '), false, false, true);
    runtime.key_up(32);
    runtime.tick(16);
    assert!(runtime.world.accessibility().paused && runtime.world.is_frozen());
    assert_eq!(runtime.world.visible_sentences().count(), 1);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_scene_file_that_is_missing_leaves_the_scene_and_still_fades_in() {
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
Game.ChangeScene("room.scene", false, false);
on "Go" {
  Game.ChangeScene("nowhere.scene", false, true);
}
"#,
        &[File::Text("room.scene", ROOM)],
    );
    no_errors(&log);
    let scene = runtime.world.scene_id();
    runtime.world.set_sfx_volume(scene, 40);
    let mut log = raise(&mut runtime, "Go");
    log.extend(runtime.tick(16));
    assert!(
        log.iter().any(|l| l.contains("cannot change scene")),
        "{log:?}"
    );
    // The old scene is still there with its nodes, its volume setting
    // went back to 100, and the fade-in started all the same.
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    assert!(runtime.world.find_node("box").is_some());
    assert_eq!(runtime.world.sfx_volume(scene), 100);
    assert_eq!(
        runtime.world.transition().kind(),
        Some(luma_adventure::TransitionKind::FadeIn)
    );
    std::fs::remove_dir_all(dir).ok();
}
