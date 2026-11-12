//! Speech and dialogue: the response box and its bookkeeping, the
//! sentences, the talk sprites and the `.talk` definitions, on a small
//! game built in code.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_formats::Image;
use luma_script::Natives as _;
use luma_script::{NativeId, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-dialogue-game-{}-{}",
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

fn compile(source: &str, name: &str) -> Vec<u8> {
    Compiler::new(Natives::wme())
        .compile(source.as_bytes(), name, &mut |_: &[u8]| None)
        .unwrap()
}

const GAME_SCRIPT: &str = r#"
global result = new Object();
result.branch = null;
Game.ChangeScene("scenes\room.scene");

on "open"   { Game.StartDlgBranch(result.branch); }
on "close"  { Game.EndDlgBranch(); }
on "offerGame"
{
  Game.AddResponseOnceGame(7, "seven");
  result.num = Game.GetNumResponses();
  result.picked = Game.GetResponse(true);
}
on "offerOnce"
{
  Game.AddResponseOnce(5, "five");
  result.num = Game.GetNumResponses();
  result.picked = Game.GetResponse(true);
}
on "reset"  { Game.ResetResponse(result.resetId); }
on "ask"
{
  Game.AddResponse(1, "one");
  Game.AddResponse(2, "two");
  result.asked = Game.GetResponse();
}
"#;

const SCENE: &[u8] = b"SCENE {
  NAME=\"Room\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=100
    ENTITY { NAME=\"prop\" X=40 Y=60 SPRITE=\"art\\green.png\" }
  }
}";

const RESPONSE_BOX: &[u8] = b"RESPONSE_BOX {
  FONT=\"fonts\\sys.font\" SPACING=1 VERTICAL_ALIGN=\"top\"
  AREA { 2, 2, 60, 40 }
  WINDOW {
    X=0 Y=0 WIDTH=64 HEIGHT=48
    BUTTON { NAME=\"prev\" X=0 Y=0 WIDTH=2 HEIGHT=2 }
    BUTTON { NAME=\"next\" X=62 Y=0 WIDTH=2 HEIGHT=2 }
  }
}";

/// The test game, with `extra` files added to (or replacing) its own.
fn game(extra: &[(&str, Vec<u8>)]) -> std::path::PathBuf {
    let dir = temp_dir();
    let mut files: Vec<(String, Vec<u8>)> = vec![
        (
            "startup.settings".into(),
            b"SETTINGS { GAME=\"default.game\" RESOLUTION { 64,48 } }".to_vec(),
        ),
        (
            "default.game".into(),
            b"GAME { NAME=\"Dialogue\" SCRIPT=\"game.script\" SYSTEM_FONT=\"fonts\\sys.font\" CURSOR=\"art\\red.png\" ACTIVE_CURSOR=\"art\\green.png\" } AD_GAME { RESPONSE_BOX=\"responses.def\" }".to_vec(),
        ),
        ("game.script".into(), compile(GAME_SCRIPT, "game.script")),
        ("scenes\\room.scene".into(), SCENE.to_vec()),
        ("responses.def".into(), RESPONSE_BOX.to_vec()),
        ("art\\font.png".into(), png(128, 128, [255, 255, 255, 255])),
        (
            "fonts\\sys.font".into(),
            b"FONT { IMAGE=\"art\\font.png\" COLUMNS=16 TILE_WIDTH=8 TILE_HEIGHT=8 DEFAULT_WIDTH=6 }".to_vec(),
        ),
        (
            "fonts\\big.font".into(),
            b"FONT { IMAGE=\"art\\font.png\" COLUMNS=16 TILE_WIDTH=8 TILE_HEIGHT=10 DEFAULT_WIDTH=4 }".to_vec(),
        ),
        ("art\\red.png".into(), png(4, 4, [255, 0, 0, 255])),
        ("art\\green.png".into(), png(4, 4, [0, 255, 0, 255])),
        ("art\\blue.png".into(), png(4, 4, [0, 0, 255, 255])),
        (
            "voices\\line.wav".into(),
            luma_audio::silent_wav(8000, 1, 400),
        ),
    ];
    for (name, bytes) in extra {
        files.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        files.push(((*name).to_string(), bytes.clone()));
    }
    let mut pkg = PackageBuilder::new("data");
    for (name, bytes) in files {
        pkg.add_file(&name, bytes);
    }
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
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

fn set_result(runtime: &mut Runtime, key: &str, value: Value) {
    let id = runtime
        .engine
        .global("result")
        .and_then(Value::as_native)
        .unwrap();
    runtime.world.set_property(id, key, &value);
}

/// Starts the game and runs the first frames of its scene.
fn start(extra: &[(&str, Vec<u8>)]) -> (std::path::PathBuf, Runtime, Vec<String>) {
    let dir = game(extra);
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let mut log = Vec::new();
    for _ in 0..3 {
        log.extend(runtime.tick(16));
    }
    assert!(runtime.world.scene().is_some(), "{log:?}");
    (dir, runtime, log)
}

/// Raises an event on the game and runs two frames; returns their log.
fn event(runtime: &mut Runtime, name: &str) -> Vec<String> {
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, name, false);
    let mut log = runtime.tick(16);
    log.extend(runtime.tick(16));
    log
}

/// Calls a method of a native object from the game's script.
#[allow(dead_code, reason = "not every test calls natives directly")]
fn call_native(runtime: &mut Runtime, target: NativeId, name: &str, args: &[Value]) -> Value {
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

fn offer(runtime: &mut Runtime, which: &str) -> (Value, Value) {
    event(runtime, which);
    (result(runtime, "num"), result(runtime, "picked"))
}

#[test]
fn once_responses_are_remembered_with_their_dialogue_branch() {
    let (dir, mut runtime, _) = start(&[]);
    let game = runtime.world.game_id();
    // Before any response was chosen the last response is an empty
    // string, not null.
    assert_eq!(
        runtime.world.get_property(game, "LastResponse"),
        Some(Value::from(""))
    );
    // Outside any branch: offered once, then never again.
    assert_eq!(
        offer(&mut runtime, "offerGame"),
        (Value::Int(1), Value::Int(7))
    );
    assert_eq!(
        offer(&mut runtime, "offerGame"),
        (Value::Int(0), Value::Null)
    );
    // In a branch the same id is another response (`GameResponseUsed`
    // compares the dialogue-branch context too).
    set_result(&mut runtime, "branch", Value::from("Chat"));
    event(&mut runtime, "open");
    assert_eq!(
        offer(&mut runtime, "offerGame"),
        (Value::Int(1), Value::Int(7))
    );
    assert_eq!(
        offer(&mut runtime, "offerGame"),
        (Value::Int(0), Value::Null)
    );
    // `ResetResponse` forgets the use noted in the current branch only.
    set_result(&mut runtime, "resetId", Value::Int(7));
    event(&mut runtime, "reset");
    assert_eq!(
        offer(&mut runtime, "offerGame"),
        (Value::Int(1), Value::Int(7))
    );
    event(&mut runtime, "close");
    assert_eq!(
        offer(&mut runtime, "offerGame"),
        (Value::Int(0), Value::Null),
        "the use outside any branch was not reset"
    );
    // The branch names are compared without regard to case.
    set_result(&mut runtime, "branch", Value::from("CHAT"));
    event(&mut runtime, "open");
    assert_eq!(
        offer(&mut runtime, "offerGame"),
        (Value::Int(0), Value::Null)
    );
    // A once-per-branch response the same way: used in "CHAT", it is
    // still used in "chat" while the dialogue lasts.
    assert_eq!(
        offer(&mut runtime, "offerOnce"),
        (Value::Int(1), Value::Int(5))
    );
    set_result(&mut runtime, "branch", Value::from("inner"));
    event(&mut runtime, "open");
    assert_eq!(
        offer(&mut runtime, "offerOnce"),
        (Value::Int(1), Value::Int(5)),
        "another branch, another response"
    );
    event(&mut runtime, "close");
    assert_eq!(
        offer(&mut runtime, "offerOnce"),
        (Value::Int(0), Value::Null)
    );

    // The contexts survive a saved game.
    runtime.save_game(3, "branches", false).unwrap();
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(3).unwrap();
    loaded.tick(16);
    assert_eq!(
        offer(&mut loaded, "offerGame"),
        (Value::Int(0), Value::Null)
    );
    assert_eq!(
        offer(&mut loaded, "offerOnce"),
        (Value::Int(0), Value::Null)
    );
    event(&mut loaded, "close");
    // The dialogue is over: the once-per-branch uses are forgotten, the
    // once-per-game ones stay.
    assert_eq!(
        offer(&mut loaded, "offerOnce"),
        (Value::Int(1), Value::Int(5))
    );
    assert_eq!(
        offer(&mut loaded, "offerGame"),
        (Value::Int(0), Value::Null)
    );
    // No id given resets -1, not 0 (`GetInt(-1)`): nothing to forget.
    set_result(&mut loaded, "resetId", Value::Null);
    event(&mut loaded, "reset");
    assert_eq!(
        offer(&mut loaded, "offerGame"),
        (Value::Int(0), Value::Null)
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn responses_need_a_response_box() {
    let (dir, mut runtime, _) = start(&[(
        "default.game",
        b"GAME { NAME=\"Dialogue\" SCRIPT=\"game.script\" SYSTEM_FONT=\"fonts\\sys.font\" }"
            .to_vec(),
    )]);
    set_result(&mut runtime, "num", Value::Int(-1));
    let log = event(&mut runtime, "offerGame");
    assert!(
        log.iter()
            .any(|l| l.contains("Game.AddResponse: response box is not defined")),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|l| l.contains("Game.GetNumResponses: response box is not defined")),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|l| l.contains("Game.GetResponse: response box is not defined")),
        "{log:?}"
    );
    assert_eq!(result(&runtime, "num"), Value::Null);
    assert_eq!(result(&runtime, "picked"), Value::Null);
    std::fs::remove_dir_all(&dir).ok();
}
