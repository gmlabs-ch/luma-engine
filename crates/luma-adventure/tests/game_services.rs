//! The game services scripts reach through `Game`, `Scene` and the items
//! beyond the scene itself: the cursors and the items' images as sprite
//! objects.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Kind, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_formats::Image;
use luma_script::Value;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-services-game-{}-{}",
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

const GAME_SCRIPT: &str = r#"
global result = new Object();

// The game's cursors as sprite objects.
var ac = Game.GetActiveCursorObject();
result.activeType = ac.Type;
result.activeFile = ac.Filename;
result.activeFrames = ac.NumFrames;
result.activeOwnerIsGame = (ac.Owner == Game);
result.activeSame = (ac == Game.GetActiveCursorObject());
var wc = Game.GetWaitCursorObject();
result.waitFile = wc.Filename;
Game.RemoveWaitCursor();
result.waitGone = Game.GetWaitCursorObject();

// An item's hover image and cursors as sprite objects.
var key = Game.QueryItem("key");
var hover = key.GetHoverSpriteObject();
result.hoverSprite = hover.Filename;
result.hoverSpriteType = hover.Type;
result.hoverSpriteOwnerIsItem = (hover.Owner == key);
result.hoverSpriteSame = (hover == key.GetHoverSpriteObject());
var normal = key.GetNormalCursorObject();
var hoverCursor = key.GetHoverCursorObject();
result.normalCursor = normal.Filename;
result.hoverCursor = hoverCursor.Filename;
result.cursorsDiffer = (normal != hoverCursor);
var coin = Game.QueryItem("coin");
result.coinHoverSprite = coin.GetHoverSpriteObject();
result.coinHoverCursor = coin.GetHoverCursorObject();
var coinNormal = coin.GetNormalCursorObject();
result.coinNormalCursor = coinNormal.Filename;

Game.ChangeScene("scenes\room.scene");

on "Check"
{
  result.checked = true;
}
"#;

const SCENE: &[u8] = b"SCENE {
  NAME=\"Room\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=100
    ENTITY { NAME=\"prop\" X=8 Y=12 SPRITE=\"art\\green.png\" }
  }
}";

const ITEMS: &[u8] = b"ITEM { NAME=\"key\" IMAGE=\"art\\green.png\" SPRITE_HOVER=\"art\\red.png\" CURSOR=\"art\\red.png\" CURSOR_HOVER=\"sprites\\cursor.sprite\" }
ITEM { NAME=\"coin\" IMAGE=\"art\\red.png\" CURSOR=\"art\\green.png\" }";

fn compile(source: &str, name: &str) -> Vec<u8> {
    Compiler::new(Natives::wme())
        .compile(source.as_bytes(), name, &mut |_: &[u8]| None)
        .unwrap()
}

fn game() -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 64,48 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Services\" SCRIPT=\"game.script\" CURSOR=\"art\\red.png\" ACTIVE_CURSOR=\"sprites\\cursor.sprite\" NONINTERACTIVE_CURSOR=\"art\\green.png\" } AD_GAME { ITEMS=\"items.items\" }".to_vec(),
    );
    pkg.add_file("game.script", compile(GAME_SCRIPT, "game.script"));
    pkg.add_file("scenes\\room.scene", SCENE.to_vec());
    pkg.add_file("items.items", ITEMS.to_vec());
    pkg.add_file("art\\red.png", png(4, 4, [255, 0, 0, 255]));
    pkg.add_file("art\\green.png", png(4, 4, [0, 255, 0, 255]));
    pkg.add_file(
        "sprites\\cursor.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" } FRAME { DELAY=50 IMAGE=\"art\\green.png\" } }".to_vec(),
    );
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

/// Calls a method of a native object from the game's script.
fn call_native(
    runtime: &mut Runtime,
    target: luma_script::NativeId,
    name: &str,
    args: &[Value],
) -> Value {
    use luma_script::Host;
    let game = runtime.world.game_id();
    let script = runtime.engine.scripts_of(game)[0];
    let ctx = luma_script::Context {
        engine: &mut runtime.engine,
        script,
        owner: Some(game),
        line: 0,
    };
    runtime
        .world
        .call_method(target, name, args, ctx)
        .map(|r| r.value)
        .unwrap_or(Value::Null)
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

/// Starts the game and runs its script and the first frames of its
/// scene; returns the log of those frames too.
fn start() -> (std::path::PathBuf, Runtime, Vec<String>) {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let mut log = Vec::new();
    for _ in 0..3 {
        log.extend(runtime.tick(16));
    }
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
    assert!(runtime.world.scene().is_some(), "{log:?}");
    (dir, runtime, log)
}

#[test]
fn cursors_and_item_images_are_sprite_objects() {
    let (dir, runtime, log) = start();
    let text = |key: &str| result(&runtime, key).get_string(&());

    // The game's active and wait cursors (`CBGame::m_ActiveCursor`,
    // `m_CursorNoninteractive`): the same object on every request, gone
    // with the cursor.
    assert_eq!(text("activeType"), "sprite");
    assert_eq!(text("activeFile"), "sprites\\cursor.sprite");
    assert_eq!(result(&runtime, "activeFrames"), Value::Int(2));
    assert_eq!(result(&runtime, "activeOwnerIsGame"), Value::Bool(true));
    assert_eq!(result(&runtime, "activeSame"), Value::Bool(true));
    assert_eq!(text("waitFile"), "art\\green.png");
    assert_eq!(result(&runtime, "waitGone"), Value::Null);

    // An item's hover image lives in the windows' bank like its image;
    // its cursors are cursor sprites of their own.
    assert_eq!(text("hoverSprite"), "art\\red.png");
    assert_eq!(text("hoverSpriteType"), "sprite");
    assert_eq!(
        result(&runtime, "hoverSpriteOwnerIsItem"),
        Value::Bool(true)
    );
    assert_eq!(result(&runtime, "hoverSpriteSame"), Value::Bool(true));
    assert_eq!(text("normalCursor"), "art\\red.png");
    assert_eq!(text("hoverCursor"), "sprites\\cursor.sprite");
    assert_eq!(result(&runtime, "cursorsDiffer"), Value::Bool(true));
    assert_eq!(result(&runtime, "coinHoverSprite"), Value::Null);
    assert_eq!(result(&runtime, "coinHoverCursor"), Value::Null);
    assert_eq!(text("coinNormalCursor"), "art\\green.png");

    // The hover image is a sprite object of its own kind, and no stub
    // was logged for any of these.
    let hover_sprites = runtime
        .world
        .objects()
        .iter()
        .filter(|(_, n)| matches!(n.kind, Kind::HoverSprite(_)))
        .count();
    assert_eq!(hover_sprites, 1);
    assert!(
        !log.iter().any(|l| l.starts_with("not implemented")),
        "{log:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_scenes_own_viewport_comes_before_the_games() {
    let (dir, mut runtime, _) = start();
    let scene = runtime.world.scene_id();
    let game = runtime.world.game_id();
    let ints = |v: &[i32]| v.iter().map(|&i| Value::Int(i)).collect::<Vec<_>>();
    assert_eq!(runtime.world.viewport(), (0, 0, 64, 48));
    assert_eq!(
        call_native(&mut runtime, scene, "SetViewport", &ints(&[10, 5, 40, 30])),
        Value::Bool(true)
    );
    assert_eq!(runtime.world.viewport(), (10, 5, 40, 30));
    // The game's viewport stands behind the scene's own.
    call_native(
        &mut runtime,
        game,
        "SetSceneViewport",
        &ints(&[1, 1, 10, 10]),
    );
    assert_eq!(runtime.world.viewport(), (10, 5, 40, 30));
    assert_eq!(
        call_native(&mut runtime, scene, "IsPointInViewport", &ints(&[9, 5])),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(&mut runtime, scene, "IsPointInViewport", &ints(&[10, 5])),
        Value::Bool(true)
    );
    // A width or height of zero means the screen's; the rectangle is
    // kept on the screen (`CBViewport::SetRect`).
    call_native(&mut runtime, scene, "SetViewport", &ints(&[-4, 8, 0, 0]));
    assert_eq!(runtime.world.viewport(), (0, 8, 60, 40));
    // The viewport is the scene's: the next scene starts without one and
    // sees the game's.
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    assert_eq!(runtime.world.viewport(), (1, 1, 10, 10));
    std::fs::remove_dir_all(&dir).ok();
}
