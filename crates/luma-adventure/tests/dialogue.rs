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
on "askIcons"
{
  Game.AddResponse(1, "one", "art\missing.png", "art\nohover.png", "art\nopress.png", "fonts\missing.font");
  Game.AddResponse(2, "two", "art\red.png", "art\blue.png");
  Game.AddResponse(3, "three", null, null, null, "fonts\big.font");
  result.asked = Game.GetResponse();
}
on "loadBox"
{
  result.loaded = Game.LoadResponseBox(result.boxFile);
  result.window = Game.GetResponsesWindow();
}
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

/// The response buttons of the waiting box, in order.
fn response_buttons(runtime: &Runtime) -> Vec<NativeId> {
    let window = runtime.world.responses_window().unwrap();
    runtime
        .world
        .widgets_of(window)
        .into_iter()
        .filter(|w| runtime.world.control(*w).unwrap().name == "response")
        .collect()
}

fn click(runtime: &mut Runtime, x: i32, y: i32) {
    runtime.mouse_move(x, y);
    runtime.tick(16);
    runtime.mouse_left_down();
    runtime.tick(16);
    runtime.mouse_left_up();
    runtime.tick(16);
    runtime.tick(16);
}

#[test]
fn a_response_box_without_a_window_shows_its_choices_in_the_area() {
    let (dir, mut runtime, log) = start(&[(
        "responses.def",
        b"RESPONSE_BOX { FONT=\"fonts\\sys.font\" VERTICAL_ALIGN=\"top\" AREA { 4, 6, 60, 40 } }"
            .to_vec(),
    )]);
    assert!(!log.iter().any(|l| l.contains("RESPONSE_BOX")), "{log:?}");
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(true));
    // Scripts see no window.
    assert_eq!(
        call_native(&mut runtime, game, "GetResponsesWindow", &[]),
        Value::Null
    );
    event(&mut runtime, "ask");
    assert_eq!(
        runtime.world.get_property(game, "ResponsesVisible"),
        Some(Value::Bool(true))
    );
    let buttons = response_buttons(&runtime);
    assert_eq!(buttons.len(), 2);
    let first = runtime.world.control(buttons[0]).unwrap().clone();
    let second = runtime.world.control(buttons[1]).unwrap().clone();
    assert!(first.visible && second.visible);
    assert_eq!(
        (first.x, first.y),
        (4, 6),
        "placed in the area on the screen"
    );
    assert_eq!(second.y, 6 + first.height);
    click(&mut runtime, second.x + 2, second.y + second.height / 2);
    assert_eq!(result(&runtime, "asked"), Value::Int(2));
    assert_eq!(
        runtime.world.get_property(game, "ResponsesVisible"),
        Some(Value::Bool(false))
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_choice_higher_than_the_area_makes_the_area_higher() {
    let (dir, mut runtime, _) = start(&[(
        "responses.def",
        b"RESPONSE_BOX { FONT=\"fonts\\sys.font\" VERTICAL_ALIGN=\"top\" AREA { 2, 2, 60, 6 } WINDOW { X=0 Y=0 WIDTH=64 HEIGHT=48 } }"
            .to_vec(),
    )]);
    let log = event(&mut runtime, "ask");
    assert!(
        log.iter().any(|l| l
            == "Warning: Response 'one' is too high to be displayed within response box. Correcting."),
        "{log:?}"
    );
    let buttons = response_buttons(&runtime);
    let first = runtime.world.control(buttons[0]).unwrap();
    assert!(first.height > 4);
    assert!(first.visible, "the first choice fits the corrected area");
    assert_eq!(first.y, 2);
    // The area grew by the difference and stays so: the second choice,
    // as high, does not correct it again and waits behind `next`.
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("Warning: Response"))
            .count(),
        1
    );
    assert!(!runtime.world.control(buttons[1]).unwrap().visible);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn response_buttons_take_the_fonts_icons_and_cursor_the_original_gives_them() {
    let (dir, mut runtime, _) = start(&[]);
    let log = event(&mut runtime, "askIcons");
    for line in [
        "CAdResponse::SetIcon failed for file 'art\\missing.png'",
        "CAdResponse::SetIconHover failed for file 'art\\nohover.png'",
        "CAdResponse::SetIconPressed failed for file 'art\\nopress.png'",
        "CAdResponse::SetFont failed for file 'fonts\\missing.font'",
    ] {
        assert!(log.iter().any(|l| l == line), "{line}: {log:?}");
    }
    let buttons = response_buttons(&runtime);
    assert_eq!(buttons.len(), 3);
    // An icon that did not load makes a text choice, in the box's font
    // when its own did not load; the hover font is the system font when
    // the box names none; a text choice has no cursor of its own.
    let one = runtime.world.control(buttons[0]).unwrap();
    assert_eq!(one.text, "one");
    assert_eq!(one.image, None);
    assert_eq!(one.font.as_deref(), Some("fonts\\sys.font"));
    assert_eq!(one.cursor, None);
    let state = one.button().unwrap();
    assert_eq!(state.font_hover.as_deref(), Some("fonts\\sys.font"));
    assert_eq!(state.font_press.as_deref(), Some("fonts\\sys.font"));
    // An icon choice: the images, the text as caption, and the game's
    // active cursor when the box has no `CURSOR`.
    let two = runtime.world.control(buttons[1]).unwrap();
    assert_eq!(two.image.as_deref(), Some("art\\red.png"));
    assert_eq!(two.text, "");
    assert_eq!(two.caption, "two");
    assert_eq!(two.cursor.as_deref(), Some("art\\green.png"));
    assert_eq!(
        two.button().unwrap().image_hover.as_deref(),
        Some("art\\blue.png")
    );
    // A response's own font replaces the normal font only.
    let three = runtime.world.control(buttons[2]).unwrap();
    assert_eq!(three.font.as_deref(), Some("fonts\\big.font"));
    assert_eq!(
        three.button().unwrap().font_hover.as_deref(),
        Some("fonts\\sys.font")
    );
    // Each choice is listed once for the keyboard navigation, before
    // the window's own buttons.
    let objects = runtime.world.access_objects();
    assert_eq!(&objects[..3], &buttons[..]);
    assert_eq!(objects.len(), 5, "three choices, prev and next");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_response_box_that_does_not_load_leaves_the_game_without_one() {
    let (dir, mut runtime, _) = start(&[
        (
            "bad.def",
            b"RESPONSE_BOX { AREA { 0, 0, 10, 10 } COLOUR=1 }".to_vec(),
        ),
        (
            "cursor.def",
            b"RESPONSE_BOX { CURSOR=\"art\\blue.png\" AREA { 0, 0, 30, 30 } WINDOW { NAME=\"w\" } }"
                .to_vec(),
        ),
    ]);
    // The box of the game file has a window.
    let game = runtime.world.game_id();
    assert_ne!(
        call_native(&mut runtime, game, "GetResponsesWindow", &[]),
        Value::Null
    );
    let old = runtime.world.responses_window().unwrap();
    set_result(&mut runtime, "boxFile", Value::from("cursor.def"));
    event(&mut runtime, "loadBox");
    assert_eq!(result(&runtime, "loaded"), Value::Bool(true));
    assert!(runtime.world.control(old).is_none(), "the old window went");
    // The box's cursor goes to icon choices.
    event(&mut runtime, "askIcons");
    let buttons = response_buttons(&runtime);
    assert_eq!(
        runtime.world.control(buttons[1]).unwrap().cursor.as_deref(),
        Some("art\\blue.png")
    );
    assert_eq!(runtime.world.control(buttons[0]).unwrap().cursor, None);
    // A key the box does not know is a syntax error: no box is left.
    set_result(&mut runtime, "boxFile", Value::from("bad.def"));
    let log = event(&mut runtime, "loadBox");
    assert_eq!(result(&runtime, "loaded"), Value::Bool(false));
    assert_eq!(result(&runtime, "window"), Value::Null);
    assert!(
        log.iter()
            .any(|l| l.starts_with("Error parsing RESPONSE_BOX file 'bad.def'")),
        "{log:?}"
    );
    let log = event(&mut runtime, "offerGame");
    assert!(
        log.iter()
            .any(|l| l.contains("Game.GetResponse: response box is not defined")),
        "{log:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
