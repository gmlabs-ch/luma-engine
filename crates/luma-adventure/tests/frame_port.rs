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
use luma_script::{NativeId, Natives as _, Value};

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

/// A sprite of two frames, 50 ms each, looping.
const BLINK: &str = "SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"red.png\" } FRAME { DELAY=50 IMAGE=\"green.png\" } }";

#[test]
fn sprites_without_a_freezable_owner_animate_while_the_game_is_frozen() {
    let (dir, mut runtime, log) = start_with(
        "GAME { NAME=\"Frame\" SCRIPT=\"game.script\" CURSOR=\"blink.sprite\" }",
        r#"
global result = new Object();
global menu;
Game.ChangeScene("anim.scene", false, false);
on "Menu" {
  menu = Game.LoadWindow("anim.window");
  result.button = menu.GetControl("b");
  menu.GoSystemExclusive();
}
"#,
        &[
            File::Text("blink.sprite", BLINK),
            File::Text(
                "anim.scene",
                "SCENE { NAME=\"Anim\" LAYER { NAME=\"main\" MAIN=TRUE WIDTH=64 HEIGHT=48
  ENTITY { NAME=\"lamp\" X=40 Y=30 SPRITE=\"blink.sprite\" } } }",
            ),
            File::Text(
                "anim.window",
                "WINDOW { NAME=\"menu\" X=0 Y=0 WIDTH=20 HEIGHT=20 TRANSPARENT=TRUE
  BUTTON { NAME=\"b\" X=2 Y=2 WIDTH=8 HEIGHT=8 IMAGE=\"blink.sprite\" } }",
            ),
        ],
    );
    no_errors(&log);
    no_errors(&raise(&mut runtime, "Menu"));
    runtime.tick(16);
    assert!(runtime.world.is_frozen());
    let button = object(&runtime, "button");
    let lamp = luma_adventure::EntityRef::Node { layer: 0, index: 0 };
    let frames = |runtime: &Runtime| {
        (
            runtime.world.cursors().frame(),
            runtime
                .world
                .control(button)
                .and_then(|c| c.anim.as_ref())
                .map_or(0, |a| a.current_frame),
            runtime.world.scene().unwrap().frame_of(lamp),
        )
    };
    // Every 50 ms of the live timer the cursor and the button's image
    // change frame; the entity, whose owner is freezable, stands still
    // with the game timer.
    let mut cursor_changes = 0;
    let mut button_changes = 0;
    let mut last = frames(&runtime);
    for _ in 0..6 {
        runtime.tick(50);
        let now = frames(&runtime);
        cursor_changes += usize::from(now.0 != last.0);
        button_changes += usize::from(now.1 != last.1);
        assert_eq!(now.2, last.2, "the entity is frozen");
        last = now;
    }
    assert!(cursor_changes >= 5, "{cursor_changes}");
    assert!(button_changes >= 5, "{button_changes}");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_frames_move_is_scaled_by_the_scene_zoom_at_a_scalable_entity() {
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
Game.ChangeScene("half.scene", false, false);
"#,
        &[
            File::Text(
                "step.sprite",
                "SPRITE { LOOPING=FALSE FRAME { DELAY=50 IMAGE=\"red.png\" }
  FRAME { DELAY=5000 IMAGE=\"green.png\" MOVE {10,10} } }",
            ),
            File::Text(
                "half.scene",
                "SCENE { NAME=\"Half\"
  LAYER { NAME=\"main\" MAIN=TRUE WIDTH=64 HEIGHT=48 }
  SCALE_LEVEL { Y=0 SCALE=50 }
  SCALE_LEVEL { Y=48 SCALE=50 }
  ENTITY { NAME=\"scaled\" X=10 Y=20 SCALABLE=TRUE SPRITE=\"step.sprite\" }
  ENTITY { NAME=\"plain\" X=30 Y=20 SCALABLE=FALSE SPRITE=\"step.sprite\" }
}",
            ),
        ],
    );
    no_errors(&log);
    for _ in 0..5 {
        runtime.tick(20);
    }
    let view = runtime.world.scene().unwrap();
    let at = |name: &str| {
        let entity = view
            .entity(view.free_ref(view.find_free(name).unwrap()))
            .unwrap();
        (entity.x, entity.y)
    };
    // The X move takes the zoom, the Y move does not (the original
    // passes the zoom for X only).
    assert_eq!(at("scaled"), (15, 30));
    assert_eq!(at("plain"), (40, 30));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_control_sized_from_an_animated_image_takes_the_largest_frame() {
    let (dir, runtime, log) = start(
        r#"
global result = new Object();
var w = Game.LoadWindow("grow.window");
var b = w.GetControl("b");
result.width = b.Width;
result.height = b.Height;
"#,
        &[
            File::Raw("wide.png", png(20, 4, GREEN)),
            File::Text(
                "grow.sprite",
                "SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"red.png\" } FRAME { DELAY=50 IMAGE=\"wide.png\" } }",
            ),
            File::Text(
                "grow.window",
                "WINDOW { NAME=\"w\" X=0 Y=0 WIDTH=40 HEIGHT=40
  BUTTON { NAME=\"b\" X=2 Y=2 IMAGE=\"grow.sprite\" } }",
            ),
        ],
    );
    no_errors(&log);
    // 8x8 and 20x4 frames: the rectangle around both.
    assert_eq!(result(&runtime, "width"), Value::Int(20));
    assert_eq!(result(&runtime, "height"), Value::Int(8));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn every_object_has_a_cursor_of_its_own_and_the_games_is_the_normal_one() {
    let (dir, mut runtime, log) = start_with(
        "GAME { NAME=\"Frame\" SCRIPT=\"game.script\" CURSOR=\"red.png\" }",
        r#"
global result = new Object();
Game.ChangeScene("room.scene", false, false);
// The game's own cursor is the normal cursor.
result.had = Game.HasCursor();
result.name = Game.GetCursor();
result.set = Game.SetCursor("green.png");
result.now = Game.GetCursor();
var sprite = Game.GetCursorObject();
result.object = (sprite != null);
// A sprite that does not load leaves no cursor.
result.bad = Game.SetCursor("missing.sprite");
result.afterBad = Game.HasCursor();
result.objectAfterBad = Game.GetCursorObject();
Game.SetCursor("red.png");
Game.RemoveCursor();
result.removed = Game.HasCursor();
Game.SetCursor("green.png");

// The scene, a layer and an entity's region are objects too.
var scene = Game.Scene;
result.sceneHad = scene.HasCursor();
result.sceneSet = scene.SetCursor("red.png");
result.sceneName = scene.GetCursor();
var layer = scene.MainLayer;
result.layerSet = layer.SetCursor("red.png");
result.layerHas = layer.HasCursor();
layer.RemoveCursor();
result.layerRemoved = layer.HasCursor();

// The active and the wait cursor go when their sprite does not load.
Game.SetActiveCursor("red.png");
result.activeBad = Game.SetActiveCursor("missing.sprite");
result.activeLeft = Game.HasActiveCursor();
Game.SetWaitCursor("red.png");
result.waitBad = Game.SetWaitCursor("missing.sprite");
result.waitLeft = Game.GetWaitCursor();

on "Busy" {
  Game.Interactive = false;
}
"#,
        &[File::Text("room.scene", ROOM)],
    );
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
    let flag = |key: &str| result(&runtime, key);
    assert_eq!(flag("had"), Value::Bool(true));
    assert_eq!(flag("name"), Value::from("red.png"));
    assert_eq!(flag("set"), Value::Bool(true));
    assert_eq!(flag("now"), Value::from("green.png"));
    assert_eq!(flag("object"), Value::Bool(true));
    assert_eq!(flag("bad"), Value::Bool(false));
    assert_eq!(flag("afterBad"), Value::Bool(false));
    assert_eq!(flag("objectAfterBad"), Value::Null);
    assert_eq!(flag("removed"), Value::Bool(false));
    assert_eq!(flag("sceneHad"), Value::Bool(false));
    assert_eq!(flag("sceneSet"), Value::Bool(true));
    assert_eq!(flag("sceneName"), Value::from("red.png"));
    assert_eq!(flag("layerSet"), Value::Bool(true));
    assert_eq!(flag("layerHas"), Value::Bool(true));
    assert_eq!(flag("layerRemoved"), Value::Bool(false));
    assert_eq!(flag("activeBad"), Value::Bool(false));
    assert_eq!(flag("activeLeft"), Value::Bool(false));
    assert_eq!(flag("waitBad"), Value::Bool(false));
    assert_eq!(flag("waitLeft"), Value::Null);
    // The cursor the game ended with is the one shown.
    runtime.tick(16);
    assert_eq!(runtime.world.cursors().shown(), Some("green.png"));
    // Not interactive and running, without a wait cursor: no cursor.
    raise(&mut runtime, "Busy");
    runtime.tick(16);
    assert_eq!(runtime.world.cursors().shown(), None);
    std::fs::remove_dir_all(dir).ok();
}

/// A scene 200 wide: a back layer with an entity and an inactive layer
/// with an animated one, the main layer with an entity, a region that
/// is switched off, a second entity and a close-up layer over it all;
/// two free entities, one standing in the region, and waypoints.
const STAGE: &str = "SCENE {
  NAME=\"Stage\"
  LAYER {
    NAME=\"back\" WIDTH=200 HEIGHT=48
    ENTITY { NAME=\"far\" X=4 Y=4 SPRITE=\"red.png\" }
  }
  LAYER {
    NAME=\"off\" WIDTH=200 HEIGHT=48 ACTIVE=FALSE
    ENTITY { NAME=\"hidden\" X=4 Y=20 SPRITE=\"blink.sprite\" }
  }
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=48
    ENTITY { NAME=\"first\" X=20 Y=4 SPRITE=\"red.png\"
      BLOCKED_REGION { POINT {0,0} POINT {8,0} POINT {8,8} POINT {0,8} }
      WAYPOINTS { POINT {1,1} } }
    REGION { NAME=\"floor\" ACTIVE=FALSE POINT {0,24} POINT {200,24} POINT {200,48} POINT {0,48} }
    ENTITY { NAME=\"second\" X=40 Y=4 SPRITE=\"red.png\" }
  }
  LAYER {
    NAME=\"close\" WIDTH=200 HEIGHT=48 CLOSE_UP=TRUE
    ENTITY { NAME=\"near\" X=50 Y=30 SPRITE=\"green.png\" }
  }
  WAYPOINTS { NAME=\"route\" POINT {100,30} }
  ENTITY { NAME=\"walker\" X=30 Y=40 SPRITE=\"red.png\"
    BLOCKED_REGION { POINT {-2,-2} POINT {2,-2} POINT {2,2} POINT {-2,2} }
    WAYPOINTS { POINT {5,0} } }
  ENTITY { NAME=\"flyer\" X=30 Y=10 SPRITE=\"red.png\" }
}";

fn stage(script: &str) -> (std::path::PathBuf, Runtime) {
    let (dir, runtime, log) = start(
        script,
        &[
            File::Text("stage.scene", STAGE),
            File::Text("blink.sprite", BLINK),
            File::Text("room.scene", ROOM),
            File::Text(
                "bob.actor",
                "ACTOR { NAME=\"bob\" X=60 Y=44 SCALABLE=FALSE STAND { SPRITESET { LEFT=\"red.png\" } } }",
            ),
        ],
    );
    no_errors(&log);
    (dir, runtime)
}

#[test]
fn a_close_up_layer_shields_the_layers_below_it() {
    let (dir, mut runtime) = stage(
        r#"
global result = new Object();
Game.ChangeScene("stage.scene", false, false);
var scene = Game.Scene;
result.near = scene.GetNode("near");
result.far = scene.GetNode("far");
"#,
    );
    let near = object(&runtime, "near");
    let far = object(&runtime, "far");
    assert_eq!(object_at(&mut runtime, 52, 32), Some(near));
    // Over the entity of the back layer the mouse finds the shield, an
    // object of its own that is a window to scripts, not the entity.
    let found = object_at(&mut runtime, 6, 6).expect("the shield");
    assert_ne!(found, far);
    assert_eq!(
        runtime.world.get_property(found, "Type"),
        Some(Value::from("window"))
    );
    // Anywhere else on the screen too.
    assert_eq!(object_at(&mut runtime, 60, 2), Some(found));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_region_switched_off_keeps_its_place_and_an_inactive_layer_stands_still() {
    use luma_adventure::{DrawItem, EntityRef};
    let (dir, mut runtime) = stage(
        r#"
global result = new Object();
Game.ChangeScene("stage.scene", false, false);
"#,
    );
    // The walker stands in the floor region, which is switched off: it
    // is still drawn at the region's place, between the two entities of
    // the main layer; the flyer, in no region, ends the layer.
    let order = runtime.world.draw_order();
    let main: Vec<DrawItem> = order.into_iter().filter(|i| i.layer() == 2).collect();
    assert_eq!(
        main,
        vec![
            DrawItem::Node { layer: 2, index: 0 },
            DrawItem::Free { index: 0, layer: 2 },
            DrawItem::Node { layer: 2, index: 2 },
            DrawItem::Free { index: 1, layer: 2 },
        ]
    );
    // The entity of the inactive layer is not updated: its sprite does
    // not animate.
    let hidden = EntityRef::Node { layer: 1, index: 0 };
    let frame = runtime.world.scene().unwrap().frame_of(hidden);
    for _ in 0..6 {
        runtime.tick(50);
    }
    assert_eq!(runtime.world.scene().unwrap().frame_of(hidden), frame);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn only_free_objects_block_the_floor_and_lend_their_waypoints() {
    let (dir, mut runtime) = stage(
        r#"
global result = new Object();
Game.ChangeScene("stage.scene", false, false);
result.bob = Game.LoadActor("bob.actor");
"#,
    );
    runtime.tick(16);
    let bob = object(&runtime, "bob");
    let (blockers, waypoints) = runtime.world.obstacles_for(bob);
    let point = |x, y| luma_formats::Point { x, y };
    // The scene's waypoint group and the free entity's waypoint; the
    // `WAYPOINTS` and `BLOCKED_REGION` of the entity that is a node of
    // the main layer are not looked at.
    assert_eq!(waypoints, vec![point(100, 30), point(35, 40)]);
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0][0], point(28, 38));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn the_scene_answers_for_its_free_objects_waypoint_groups_and_viewport() {
    let (dir, runtime) = stage(
        r#"
global result = new Object();
Game.ChangeScene("stage.scene", false, false);
var scene = Game.Scene;
var bob = scene.LoadActor("bob.actor");
// The free entities and the actors loaded into the scene.
result.count = scene.NumFreeNodes;
var node = scene.GetFreeNode(2);
result.third = node.Name;
node = scene.GetFreeNode("BOB");
result.byName = (node == bob);
result.past = scene.GetFreeNode(3);
// A waypoint group is found by name after the nodes and free entities.
var group = scene.GetNode("route");
result.groupType = group.Type;
// The viewport's right and bottom edges are inside.
result.corner = scene.IsPointInViewport(64, 48);
result.beyond = scene.IsPointInViewport(65, 48);
// A point to centre on, kept inside the scene (200 wide, 64 shown).
scene.OffsetX = 100;
result.centred = scene.OffsetX;
scene.OffsetX = 500;
result.clampedRight = scene.OffsetX;
scene.OffsetX = 10;
result.clampedLeft = scene.OffsetX;
scene.OffsetY = 40;
result.vertical = scene.OffsetY;
"#,
    );
    let flag = |key: &str| result(&runtime, key);
    assert_eq!(flag("count"), Value::Int(3));
    assert_eq!(flag("third"), Value::from("bob"));
    assert_eq!(flag("byName"), Value::Bool(true));
    assert_eq!(flag("past"), Value::Null);
    assert_eq!(flag("groupType"), Value::from("waypoint-group"));
    assert_eq!(flag("corner"), Value::Bool(true));
    assert_eq!(flag("beyond"), Value::Bool(false));
    assert_eq!(flag("centred"), Value::Int(68));
    assert_eq!(flag("clampedRight"), Value::Int(136));
    assert_eq!(flag("clampedLeft"), Value::Int(0));
    assert_eq!(
        flag("vertical"),
        Value::Int(0),
        "max(0, 40 - 24), at most 48 - 48"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_scene_load_puts_the_scroll_settings_back() {
    let (dir, mut runtime) = stage(
        r#"
global result = new Object();
Game.ChangeScene("stage.scene", false, false);
var scene = Game.Scene;
scene.AutoScroll = false;
scene.ScrollPixelsX = 7;
scene.ScrollSpeedY = 99;
on "Next" {
  Game.ChangeScene("room.scene", false, false);
}
on "Read" {
  var s = Game.Scene;
  result.auto = s.AutoScroll;
  result.pixels = s.ScrollPixelsX;
  result.speed = s.ScrollSpeedY;
}
"#,
    );
    raise(&mut runtime, "Next");
    runtime.tick(16);
    raise(&mut runtime, "Read");
    assert_eq!(result(&runtime, "auto"), Value::Bool(true));
    assert_eq!(result(&runtime, "pixels"), Value::Int(1));
    assert_eq!(result(&runtime, "speed"), Value::Int(10));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn the_keyboard_walks_the_scenes_objects_in_the_order_they_are_drawn() {
    let (dir, mut runtime) = stage(
        r#"
global result = new Object();
Game.ChangeScene("stage.scene", false, false);
var scene = Game.Scene;
var layer = scene.GetLayer("close");
layer.CloseUp = false;
result.far = scene.GetNode("far");
result.hidden = scene.GetNode("hidden");
result.first = scene.GetNode("first");
result.second = scene.GetNode("second");
result.near = scene.GetNode("near");
result.walker = scene.GetFreeNode("walker");
result.flyer = scene.GetFreeNode("flyer");
on "Close" {
  var s = Game.Scene;
  var l = s.GetLayer("close");
  l.CloseUp = true;
}
"#,
    );
    let id = |key: &str| object(&runtime, key);
    // Layer by layer, the inactive one included; at the region the free
    // entity standing in it; the one outside every region at the end.
    assert_eq!(
        runtime.world.access_objects(),
        vec![
            id("far"),
            id("hidden"),
            id("first"),
            id("walker"),
            id("second"),
            id("near"),
            id("flyer"),
        ]
    );
    // A close-up layer drops what was gathered under it; the free
    // objects then all come at the end, by `y`.
    let (near, flyer, walker) = (id("near"), id("flyer"), id("walker"));
    raise(&mut runtime, "Close");
    assert_eq!(runtime.world.access_objects(), vec![near, flyer, walker]);
    std::fs::remove_dir_all(dir).ok();
}

const ZONES: &str = "SCENE {
  NAME=\"Zones\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=64 HEIGHT=48
    REGION { NAME=\"floor\" SCRIPT=\"floor.script\" POINT {0,24} POINT {64,24} POINT {64,48} POINT {0,48} }
    ENTITY { NAME=\"spot\" X=7 Y=9 REGION { POINT {10,4} POINT {30,4} POINT {30,20} POINT {10,20} } }
    ENTITY { NAME=\"mover\" X=5 Y=5 SPRITE=\"red.png\" }
  }
}";

#[test]
fn moving_an_entity_or_an_actor_by_script_raises_the_region_events() {
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
result.entries = 0;
result.leaves = 0;
Game.ChangeScene("zones.scene", false, false);
global bob = Game.LoadActor("bob.actor");
on "Enter" {
  var scene = Game.Scene;
  var mover = scene.GetNode("mover");
  mover.Y = 30;
}
on "Leave" {
  var scene = Game.Scene;
  var mover = scene.GetNode("mover");
  mover.SkipTo(5, 5);
}
on "ActorOut" {
  bob.Y = 2;
}
on "ActorIn" {
  bob.Y = 40;
}
"#,
        &[
            File::Text("zones.scene", ZONES),
            File::Script(
                "floor.script",
                r#"
global result;
on "ActorEntry" {
  result.entries = result.entries + 1;
}
on "ActorLeave" {
  result.leaves = result.leaves + 1;
}
"#,
            ),
            File::Text(
                "bob.actor",
                "ACTOR { NAME=\"bob\" X=60 Y=44 SCALABLE=FALSE STAND { SPRITESET { LEFT=\"red.png\" } } }",
            ),
        ],
    );
    no_errors(&log);
    let counts = |runtime: &Runtime| (result(runtime, "entries"), result(runtime, "leaves"));
    let step = |runtime: &mut Runtime, event: &str| {
        no_errors(&raise(runtime, event));
        runtime.tick(16);
        runtime.tick(16);
    };
    // The actor, loaded on the floor, entered it with its first frame.
    runtime.tick(16);
    assert_eq!(counts(&runtime), (Value::Int(1), Value::Int(0)));
    // An entity whose `Y` a script sets enters the floor region...
    step(&mut runtime, "Enter");
    assert_eq!(counts(&runtime), (Value::Int(2), Value::Int(0)));
    // ...and `SkipTo` takes it out again.
    step(&mut runtime, "Leave");
    assert_eq!(counts(&runtime), (Value::Int(2), Value::Int(1)));
    // The same for an actor whose `Y` is set.
    step(&mut runtime, "ActorOut");
    assert_eq!(counts(&runtime), (Value::Int(2), Value::Int(2)));
    step(&mut runtime, "ActorIn");
    assert_eq!(counts(&runtime), (Value::Int(3), Value::Int(2)));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn an_entity_with_a_region_and_no_sprite_stands_at_the_regions_foot() {
    let (dir, runtime, log) = start(
        r#"
global result = new Object();
Game.ChangeScene("zones.scene", false, false);
var scene = Game.Scene;
var spot = scene.GetNode("spot");
result.x = spot.X;
result.y = spot.Y;
result.height = spot.Height;
var mover = scene.GetNode("mover");
result.moverX = mover.X;
"#,
        &[
            File::Text("zones.scene", ZONES),
            File::Script("floor.script", "global result;"),
        ],
    );
    no_errors(&log);
    // The bottom centre of the region's rectangle (10..30, 4..20), not
    // the X and Y of the file; the height is the rectangle's.
    assert_eq!(result(&runtime, "x"), Value::Int(20));
    assert_eq!(result(&runtime, "y"), Value::Int(20));
    assert_eq!(result(&runtime, "height"), Value::Int(16));
    // An entity with a sprite keeps the position of its file.
    assert_eq!(result(&runtime, "moverX"), Value::Int(5));
    std::fs::remove_dir_all(dir).ok();
}

/// A window at (2, 2): a pixel-perfect button whose 8x8 image is centred
/// in its 20x20 rectangle, an animated button with a hover image, and an
/// entity container at (30, 4) whose entity file says X=50 Y=50.
const PANEL: &str = "WINDOW {
  NAME=\"panel\" X=2 Y=2 WIDTH=60 HEIGHT=44 SCRIPT=\"panel.script\"
  BUTTON { NAME=\"exact\" X=0 Y=0 WIDTH=20 HEIGHT=20 IMAGE=\"red.png\" CENTER_IMAGE=TRUE PIXEL_PERFECT=TRUE PARENT_NOTIFY=TRUE }
  BUTTON { NAME=\"anim\" X=0 Y=24 WIDTH=8 HEIGHT=8 IMAGE=\"blink.sprite\" IMAGE_HOVER=\"green.png\" }
  ENTITY_CONTAINER { NAME=\"box\" X=30 Y=4 ENTITY=\"badge.entity\" }
}";

fn panel(script: &str) -> (std::path::PathBuf, Runtime) {
    let (dir, runtime, log) = start(
        script,
        &[
            File::Text("panel.window", PANEL),
            File::Text("blink.sprite", BLINK),
            File::Text(
                "badge.entity",
                "ENTITY { NAME=\"badge\" X=50 Y=50 SPRITE=\"green.png\" SCRIPT=\"badge.script\" }",
            ),
            File::Script("badge.script", "on \"Never\" { }"),
            File::Script(
                "panel.script",
                r#"
global result;
on "exact" {
  result.presses = result.presses + 1;
}
"#,
            ),
        ],
    );
    no_errors(&log);
    (dir, runtime)
}

const PANEL_SCRIPT: &str = r#"
global result = new Object();
result.presses = 0;
global panel = Game.LoadWindow("panel.window");
result.window = panel;
result.exact = panel.GetControl("exact");
result.anim = panel.GetControl("anim");
global box = panel.GetControl("box");
global badge = box.GetEntity();
result.badge = badge;
result.independent = badge.SceneIndependent;
"#;

#[test]
fn a_pixel_perfect_button_takes_the_mouse_where_its_image_is_drawn() {
    let (dir, mut runtime) = panel(PANEL_SCRIPT);
    let exact = object(&runtime, "exact");
    let window = object(&runtime, "window");
    // The 8x8 image is centred in the 20x20 button at (2, 2): drawn at
    // (8, 8). The corner of the button's rectangle is the window's.
    assert_eq!(object_at(&mut runtime, 12, 12), Some(exact));
    assert_eq!(object_at(&mut runtime, 4, 4), Some(window));
    assert_eq!(object_at(&mut runtime, 17, 12), Some(window));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_buttons_hidden_images_start_again_and_press_needs_a_live_button() {
    let script = format!(
        "{PANEL_SCRIPT}{}",
        r#"
on "PressDisabled" {
  var b = panel.GetControl("exact");
  b.Disabled = true;
  b.Press();
}
on "PressEnabled" {
  var b = panel.GetControl("exact");
  b.Disabled = false;
  b.Press();
}
on "Align" {
  var b = panel.GetControl("exact");
  b.TextAlign = 7;
  result.align = b.TextAlign;
  var made = panel.CreateButton("made");
  result.madeAlign = made.TextAlign;
}
"#
    );
    let (dir, mut runtime) = panel(&script);
    let anim = object(&runtime, "anim");
    let frame = |runtime: &Runtime| {
        runtime
            .world
            .control(anim)
            .and_then(|c| c.anim.as_ref())
            .map_or(0, |a| a.current_frame)
    };
    // The image runs while it shows; under the mouse the hover image
    // takes its place and the plain one is put back to its first frame.
    runtime.mouse_move(60, 44);
    runtime.tick(16);
    runtime.tick(50);
    assert_eq!(frame(&runtime), 1);
    runtime.mouse_move(4, 28);
    runtime.tick(1);
    runtime.tick(1);
    assert_eq!(frame(&runtime), 0, "reset while the hover image shows");

    // `Press()` does nothing on a disabled button, and fires an enabled
    // one a tenth of a second later.
    no_errors(&raise(&mut runtime, "PressDisabled"));
    for _ in 0..3 {
        runtime.tick(60);
    }
    assert_eq!(result(&runtime, "presses"), Value::Int(0));
    no_errors(&raise(&mut runtime, "PressEnabled"));
    for _ in 0..3 {
        runtime.tick(60);
    }
    assert_eq!(result(&runtime, "presses"), Value::Int(1));

    // An alignment outside 0..2 is left; a button made by a script is
    // centred.
    no_errors(&raise(&mut runtime, "Align"));
    assert_eq!(result(&runtime, "align"), Value::Int(0));
    assert_eq!(result(&runtime, "madeAlign"), Value::Int(2));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_containers_entity_stands_at_the_container_and_goes_with_it() {
    let script = format!(
        "{PANEL_SCRIPT}{}",
        r#"
on "Read" {
  result.x = badge.X;
  result.y = badge.Y;
}
on "Bad" {
  result.bad = box.SetEntity("missing.entity");
  result.left = box.GetEntity();
  result.old = Game.ValidObject(badge);
}
on "Reload" {
  result.reloaded = box.SetEntity("badge.entity");
  badge = box.GetEntity();
  result.badge = badge;
}
on "Unload" {
  Game.UnloadObject(panel);
  result.gone = Game.ValidObject(badge);
}
"#
    );
    let (dir, mut runtime) = panel(&script);
    assert_eq!(result(&runtime, "independent"), Value::Bool(true));
    let badge = object(&runtime, "badge");
    // The container is at (30, 4) of the window at (2, 2): the entity
    // stands there, not 50 pixels further as its file says.
    runtime.tick(16);
    no_errors(&raise(&mut runtime, "Read"));
    assert_eq!(result(&runtime, "x"), Value::Int(32));
    assert_eq!(result(&runtime, "y"), Value::Int(6));
    assert_eq!(object_at(&mut runtime, 34, 8), Some(badge));
    // A file that does not load leaves the container empty: the old
    // entity went first.
    no_errors(&raise(&mut runtime, "Bad"));
    assert_eq!(result(&runtime, "bad"), Value::Bool(false));
    assert_eq!(result(&runtime, "left"), Value::Null);
    assert_eq!(result(&runtime, "old"), Value::Bool(false));
    // The window takes its container's entity and its scripts with it.
    no_errors(&raise(&mut runtime, "Reload"));
    assert_eq!(result(&runtime, "reloaded"), Value::Bool(true));
    let badge = object(&runtime, "badge");
    assert!(!runtime.engine.scripts_of(badge).is_empty());
    no_errors(&raise(&mut runtime, "Unload"));
    assert_eq!(result(&runtime, "gone"), Value::Bool(false));
    assert!(runtime.engine.scripts_of(badge).is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_window_shows_its_title_only_with_a_font_of_its_own() {
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
Game.LoadWindow("plain.window");
Game.LoadWindow("titled.window");
"#,
        &[
            File::Text(
                "plain.window",
                "WINDOW { NAME=\"plain\" X=0 Y=0 WIDTH=30 HEIGHT=20 TITLE=\"MMM\" TITLE_RECT { 0, 0, 30, 10 } }",
            ),
            File::Text(
                "titled.window",
                "WINDOW { NAME=\"titled\" X=0 Y=24 WIDTH=30 HEIGHT=20 FONT=\"sys.font\" TITLE=\"MMM\" TITLE_RECT { 0, 0, 30, 10 } }",
            ),
        ],
    );
    no_errors(&log);
    runtime.tick(16);
    if let Some(frame) = render(&mut runtime) {
        // The font's image is white: a glyph is a white block.
        assert_eq!(rgb(&frame, 2, 2), (0, 0, 0), "no font, no title");
        assert_eq!(rgb(&frame, 2, 26), (255, 255, 255), "the title");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn buttons_are_dead_while_the_game_is_not_interactive_and_runs() {
    let script = format!(
        "{PANEL_SCRIPT}{}",
        r#"
on "Busy" {
  Game.Interactive = false;
}
"#
    );
    let (dir, mut runtime) = panel(&script);
    let click = |runtime: &mut Runtime| {
        runtime.mouse_move(12, 12);
        runtime.tick(16);
        runtime.mouse_left_down();
        runtime.tick(16);
        runtime.mouse_left_up();
        runtime.tick(16);
        runtime.tick(16);
        runtime.tick(16);
    };
    click(&mut runtime);
    assert_eq!(result(&runtime, "presses"), Value::Int(1));
    // Not interactive: the button neither hovers nor fires
    // (`m_Interactive || m_State == GAME_SEMI_FROZEN`).
    no_errors(&raise(&mut runtime, "Busy"));
    click(&mut runtime);
    assert_eq!(result(&runtime, "presses"), Value::Int(1));
    std::fs::remove_dir_all(dir).ok();
}
