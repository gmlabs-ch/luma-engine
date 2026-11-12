//! Speech and dialogue: the response box and its bookkeeping, the
//! sentences, the talk sprites and the `.talk` definitions, on a small
//! game built in code.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, EntityRef, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_formats::{Image, TextAlign};
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

/// The scene's `prop` entity and its native object.
fn prop(runtime: &Runtime) -> (EntityRef, NativeId) {
    let (layer, index) = runtime.world.scene().unwrap().find_node("prop").unwrap();
    let r = EntityRef::Node { layer, index };
    (r, runtime.world.node_id(r).unwrap())
}

/// The files of a sentence with a talk definition: a long speech sound
/// and, next to it, timed sprites.
fn talk_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("voices\\long.wav", luma_audio::silent_wav(8000, 1, 1200)),
        (
            "voices\\long.talk",
            b"TALK {
  DEFAULT_SPRITE=\"art\\rest.sprite\"
  ACTION { START_TIME=100 END_TIME=200 SPRITE=\"art\\blue.png\" }
  ACTION { START_TIME=300 SPRITE=\"art\\once.sprite\" }
}"
            .to_vec(),
        ),
        (
            "art\\rest.sprite",
            b"SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" } }".to_vec(),
        ),
        (
            "art\\once.sprite",
            b"SPRITE { LOOPING=FALSE FRAME { DELAY=40 IMAGE=\"art\\red.png\" } FRAME { DELAY=40 IMAGE=\"art\\blue.png\" } }"
                .to_vec(),
        ),
    ]
}

/// Runs frames while the entity talks and returns the sprites it
/// showed, each once per run of frames.
fn sprites_while_talking(runtime: &mut Runtime, r: EntityRef, out: &mut Vec<String>) {
    for _ in 0..400 {
        runtime.tick(10);
        let view = runtime.world.scene().unwrap();
        if !view.is_talking(r) {
            return;
        }
        let shown = view.shown_sprite(r).unwrap_or_default();
        if out.last() != Some(&shown) {
            out.push(shown);
        }
    }
    panic!("the sentence never ended: {out:?}");
}

#[test]
fn a_talk_definition_times_the_sprites_of_a_sentence() {
    let (dir, mut runtime, _) = start(&talk_files());
    let (r, id) = prop(&runtime);
    assert!(runtime.world.talk(
        id,
        "Hello",
        Some("voices\\long.wav"),
        0,
        None,
        TextAlign::Center
    ));
    let mut shown = Vec::new();
    sprites_while_talking(&mut runtime, r, &mut shown);
    // The default sprite outside the nodes; the timed node, whose
    // sprite is a still image and so has played at once: the entity's
    // talk stance (its own sprite here) fills the rest of the node, as
    // in the original; then the node without an end until its sprite
    // has played.
    assert_eq!(
        shown,
        [
            "art\\rest.sprite",
            "art\\blue.png",
            "art\\green.png",
            "art\\rest.sprite",
            "art\\once.sprite",
            "art\\rest.sprite"
        ]
    );
    // The sentence over, the entity shows its own sprite again.
    runtime.tick(10);
    assert_eq!(
        runtime.world.scene().unwrap().shown_sprite(r).as_deref(),
        Some("art\\green.png")
    );
    // A sentence without a `.talk` file beside its sound keeps the
    // entity's sprite, and one whose definition does not load is logged.
    assert!(runtime.world.talk(
        id,
        "Hello",
        Some("voices\\line.wav"),
        0,
        None,
        TextAlign::Center
    ));
    let mut shown = Vec::new();
    sprites_while_talking(&mut runtime, r, &mut shown);
    assert_eq!(shown, ["art\\green.png"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_talk_definition_and_its_speech_survive_a_saved_game() {
    let (dir, mut runtime, _) = start(&talk_files());
    let (r, id) = prop(&runtime);
    assert!(runtime.world.talk(
        id,
        "Hello",
        Some("voices\\long.wav"),
        0,
        None,
        TextAlign::Center
    ));
    // Save while the node without an end plays its sprite.
    let mut before = Vec::new();
    for _ in 0..33 {
        runtime.tick(10);
    }
    assert_eq!(
        runtime.world.scene().unwrap().shown_sprite(r).as_deref(),
        Some("art\\once.sprite")
    );
    runtime.save_game(4, "talking", false).unwrap();
    sprites_while_talking(&mut runtime, r, &mut before);

    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(4).unwrap();
    {
        let view = loaded.world.scene_mut().unwrap();
        assert!(view.is_talking(r));
        let sentence = view.sentences_mut().next().unwrap();
        assert!(sentence.sound.is_some(), "the speech plays again");
        assert!(sentence.talk.is_some(), "the definition came back");
        assert!(sentence.sound_position > 100, "{}", sentence.sound_position);
    }
    let mut after = Vec::new();
    sprites_while_talking(&mut loaded, r, &mut after);
    assert_eq!(after, before, "the same sprites as without the save");
    assert_eq!(after, ["art\\once.sprite", "art\\rest.sprite"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_bad_talk_definition_is_logged_and_the_sentence_goes_without() {
    let mut files = talk_files();
    files.push((
        "voices\\line.talk",
        b"TALK { ACTION { START_TIME=0 LOOP=TRUE } }".to_vec(),
    ));
    let (dir, mut runtime, _) = start(&files);
    let (r, id) = prop(&runtime);
    runtime.world.talk(
        id,
        "Hello",
        Some("voices\\line.wav"),
        0,
        None,
        TextAlign::Center,
    );
    let log = runtime.tick(10);
    assert!(
        log.iter()
            .any(|l| l.starts_with("Error parsing TALK file 'voices\\line.talk'")),
        "{log:?}"
    );
    let view = runtime.world.scene().unwrap();
    assert!(view.is_talking(r));
    assert_eq!(view.shown_sprite(r).as_deref(), Some("art\\green.png"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stance_names_a_talk_sprite_by_its_name_key() {
    let (dir, mut runtime, _) = start(&[
        (
            "scenes\\room.scene",
            b"SCENE { NAME=\"Room\" LAYER { NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=100
              ENTITY { NAME=\"prop\" X=40 Y=60 SPRITE=\"art\\green.png\"
                TALK=\"art\\wave.sprite\" TALK_SPECIAL=\"art\\angry.sprite\" } } }"
                .to_vec(),
        ),
        (
            "art\\wave.sprite",
            b"SPRITE { NAME=\"angry\" LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" } }"
                .to_vec(),
        ),
        (
            "art\\angry.sprite",
            b"SPRITE { NAME=\"Shout\" LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\blue.png\" } }"
                .to_vec(),
        ),
    ]);
    let (r, id) = prop(&runtime);
    // "shout" is the `NAME` of the special sprite; "angry" is the name
    // of the generic sprite, not the file name of the special one.
    for (stance, sprite) in [
        ("shout", "art\\angry.sprite"),
        ("angry", "art\\wave.sprite"),
    ] {
        runtime.world.talk(
            id,
            "Hello",
            None,
            300,
            Some(stance.to_string()),
            TextAlign::Center,
        );
        runtime.tick(10);
        runtime.tick(10);
        assert_eq!(
            runtime.world.scene().unwrap().shown_sprite(r).as_deref(),
            Some(sprite),
            "{stance}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_sentence_is_placed_in_the_scene_viewport_and_hidden_by_a_freeze() {
    let (dir, mut runtime, _) = start(&[]);
    let (_, id) = prop(&runtime);
    let say = |runtime: &mut Runtime| {
        runtime
            .world
            .talk(id, "Hi", None, 5000, None, TextAlign::Center);
        runtime.tick(10);
        runtime.tick(10);
        let draw = runtime.world.sentences()[0].clone();
        (draw.x, draw.width)
    };
    // The entity stands at x = 40 on a screen 64 wide: a box of half
    // the screen centred on it.
    assert_eq!(say(&mut runtime), (24, 32));
    // In a scene viewport that starts at (10, 5) the entity is drawn 10
    // pixels to the right, and its sentence with it (`GetOffsetLeft`).
    let scene = runtime.world.scene_id();
    call_native(
        &mut runtime,
        scene,
        "SetViewport",
        &[
            Value::Int(10),
            Value::Int(5),
            Value::Int(40),
            Value::Int(30),
        ],
    );
    assert_eq!(say(&mut runtime), (36, 28));
    // A frozen game hides the sentences of freezable objects only.
    assert_eq!(runtime.world.displayed_sentences().count(), 1);
    runtime.world.freeze(&mut runtime.engine, false);
    assert_eq!(runtime.world.displayed_sentences().count(), 0);
    runtime.world.unfreeze(&mut runtime.engine);
    assert_eq!(runtime.world.displayed_sentences().count(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_actor_shows_the_talk_definition_sprite_of_its_direction() {
    let (dir, mut runtime, _) = start(&[
        ("voices\\act.wav", luma_audio::silent_wav(8000, 1, 600)),
        (
            "voices\\act.talk",
            b"TALK { ACTION { START_TIME=0 END_TIME=300 SPRITESET_FILE=\"art\\mouth.spriteset\" } }"
                .to_vec(),
        ),
        (
            "art\\mouth.spriteset",
            b"SPRITESET { TEMPLATE=\"art\\base.spriteset\" RIGHT=\"art\\right.sprite\" }".to_vec(),
        ),
        (
            "art\\base.spriteset",
            b"SPRITESET { LEFT=\"art\\left.sprite\" RIGHT=\"art\\blue.png\" }".to_vec(),
        ),
        (
            "art\\left.sprite",
            b"SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" } }".to_vec(),
        ),
        (
            "art\\right.sprite",
            b"SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\blue.png\" } }".to_vec(),
        ),
        (
            "actors\\mo.actor",
            b"ACTOR { NAME=\"mo\" X=20 Y=40 STAND { SPRITESET { LEFT=\"art\\green.png\" } } }".to_vec(),
        ),
    ]);
    let actor = runtime
        .world
        .load_actor(&mut runtime.engine, "actors\\mo.actor", true)
        .unwrap();
    runtime.tick(10);
    let shown = |runtime: &Runtime| {
        runtime
            .world
            .actor(actor)
            .unwrap()
            .current_sprite()
            .map(str::to_string)
    };
    assert_eq!(shown(&runtime).as_deref(), Some("art\\green.png"));
    let say = |runtime: &mut Runtime| {
        assert!(runtime.world.talk(
            actor,
            "Hello",
            Some("voices\\act.wav"),
            0,
            None,
            TextAlign::Center
        ));
        runtime.tick(10);
        runtime.tick(10);
    };
    // The set of the action, through its template: the sprite of the
    // actor's direction.
    say(&mut runtime);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\left.sprite"));
    call_native(&mut runtime, actor, "TurnTo", &[Value::Int(2)]);
    for _ in 0..5 {
        runtime.tick(10);
    }
    say(&mut runtime);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\right.sprite"));
    // Past the action, without a default, the definition gives nothing
    // and the actor keeps the sprite it had, a looping one, as in the
    // original; the sentence over, it stands again.
    for _ in 0..32 {
        runtime.tick(10);
    }
    assert!(runtime.world.is_talking(actor));
    assert_eq!(shown(&runtime).as_deref(), Some("art\\right.sprite"));
    for _ in 0..40 {
        runtime.tick(10);
    }
    assert!(!runtime.world.is_talking(actor));
    assert_eq!(shown(&runtime).as_deref(), Some("art\\green.png"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_item_talks_and_plays_animations_while_the_box_shows_it() {
    let (dir, mut runtime, _) = start(&[
        (
            "default.game",
            b"GAME { NAME=\"Dialogue\" SCRIPT=\"game.script\" SYSTEM_FONT=\"fonts\\sys.font\" } AD_GAME { ITEMS=\"items.items\" INVENTORY_BOX=\"inv.def\" }".to_vec(),
        ),
        (
            "items.items",
            b"ITEM { NAME=\"key\" IMAGE=\"art\\green.png\" TALK=\"art\\wave.sprite\" TALK_SPECIAL=\"art\\shout.sprite\" }".to_vec(),
        ),
        (
            "inv.def",
            b"INVENTORY_BOX { ITEM_WIDTH=8 ITEM_HEIGHT=8 AREA { 0, 0, 32, 8 } WINDOW { X=0 Y=0 WIDTH=64 HEIGHT=8 } }".to_vec(),
        ),
        (
            "art\\wave.sprite",
            b"SPRITE { NAME=\"wave\" LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" } }".to_vec(),
        ),
        (
            "art\\shout.sprite",
            b"SPRITE { NAME=\"Shout\" LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\blue.png\" } }".to_vec(),
        ),
        (
            "art\\once.sprite",
            b"SPRITE { LOOPING=FALSE FRAME { DELAY=40 IMAGE=\"art\\red.png\" } FRAME { DELAY=40 IMAGE=\"art\\blue.png\" } }".to_vec(),
        ),
    ]);
    let game = runtime.world.game_id();
    call_native(&mut runtime, game, "TakeItem", &[Value::from("key")]);
    let key = call_native(&mut runtime, game, "QueryItem", &[Value::from("key")])
        .as_native()
        .unwrap();
    runtime
        .world
        .set_property(game, "InventoryVisible", &Value::Bool(true));
    runtime.tick(10);
    let shown = |runtime: &Runtime| runtime.world.item(key).unwrap().current_sprite.clone();
    assert_eq!(shown(&runtime).as_deref(), Some("art\\green.png"));

    // A sentence: the special talk sprite the stance names, by the
    // `NAME` of its sprite file; the text is shown; the item is busy.
    assert!(runtime.world.talk(
        key,
        "Hi",
        None,
        300,
        Some("shout".into()),
        TextAlign::Center
    ));
    runtime.tick(10);
    runtime.tick(10);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\shout.sprite"));
    assert!(runtime.world.is_talking(key));
    assert_eq!(runtime.world.sentences().len(), 1);
    assert_eq!(runtime.world.sentences()[0].text, "Hi");
    assert_eq!(
        call_native(&mut runtime, key, "IsTalking", &[]),
        Value::Bool(true)
    );
    for _ in 0..32 {
        runtime.tick(10);
    }
    assert!(!runtime.world.is_talking(key));
    assert_eq!(shown(&runtime).as_deref(), Some("art\\green.png"));
    assert!(runtime.world.sentences().is_empty());

    // Without a stance, a generic talk sprite. An item is updated only
    // while the inventory box draws it: hidden, its sentence is neither
    // shown nor ended.
    assert!(
        runtime
            .world
            .talk(key, "Hi", None, 100, None, TextAlign::Center)
    );
    runtime.tick(10);
    runtime.tick(10);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\wave.sprite"));
    runtime
        .world
        .set_property(game, "InventoryVisible", &Value::Bool(false));
    for _ in 0..20 {
        runtime.tick(10);
    }
    assert!(runtime.world.is_talking(key));
    assert!(runtime.world.sentences().is_empty());
    runtime
        .world
        .set_property(game, "InventoryVisible", &Value::Bool(true));
    runtime.tick(10);
    runtime.tick(10);
    assert!(!runtime.world.is_talking(key));

    // `PlayAnim`: the sprite plays to its end, then the item's own.
    assert_eq!(
        call_native(
            &mut runtime,
            key,
            "PlayAnimAsync",
            &[Value::from("art\\once.sprite")]
        ),
        Value::Bool(true)
    );
    runtime.tick(10);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\once.sprite"));
    assert!(!runtime.world.item(key).unwrap().ready);
    for _ in 0..15 {
        runtime.tick(10);
    }
    assert_eq!(shown(&runtime).as_deref(), Some("art\\green.png"));
    assert!(runtime.world.item(key).unwrap().ready);
    assert_eq!(
        call_native(
            &mut runtime,
            key,
            "PlayAnim",
            &[Value::from("art\\none.sprite")]
        ),
        Value::Bool(false)
    );

    // The talk sprite sets are the item's, as an entity's.
    let sets = |runtime: &Runtime| {
        let def = &runtime.world.item(key).unwrap().def;
        (def.talk.clone(), def.talk_special.clone())
    };
    let file = |name: &str| Value::from(name);
    assert_eq!(
        call_native(
            &mut runtime,
            key,
            "AddTalkSprite",
            &[file("art\\once.sprite"), Value::Bool(false)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            key,
            "SetTalkSprite",
            &[file("art\\wave.sprite"), Value::Bool(true)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            key,
            "RemoveTalkSprite",
            &[file("ART\\WAVE.sprite"), Value::Bool(false)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        sets(&runtime),
        (
            vec!["art\\once.sprite".to_string()],
            vec!["art\\wave.sprite".to_string()]
        )
    );
    assert_eq!(
        call_native(
            &mut runtime,
            key,
            "AddTalkSprite",
            &[file("art\\none.sprite"), Value::Bool(false)]
        ),
        Value::Bool(false)
    );
    // `SetSprite(null)` takes the sprite away and answers true.
    assert_eq!(
        call_native(&mut runtime, key, "SetSprite", &[Value::Null]),
        Value::Bool(true)
    );
    assert_eq!(runtime.world.item(key).unwrap().def.sprite, None);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_entitys_sprite_and_talk_sprites_change_as_in_the_original() {
    let (dir, mut runtime, _) = start(&[
        (
            "art\\loop.sprite",
            b"SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" } FRAME { DELAY=50 IMAGE=\"art\\blue.png\" } }".to_vec(),
        ),
        ("art\\broken.sprite", b"SPRITE { FRAME {".to_vec()),
    ]);
    let (r, id) = prop(&runtime);
    let shown = |runtime: &Runtime| runtime.world.scene().unwrap().shown_sprite(r);
    let own = |runtime: &Runtime| {
        runtime
            .world
            .scene()
            .unwrap()
            .entity(r)
            .unwrap()
            .sprite
            .clone()
    };
    // A sprite `PlayAnim` plays goes on when the entity's own sprite is
    // replaced; the new one shows once it is over.
    let file = |name: &str| Value::from(name);
    call_native(
        &mut runtime,
        id,
        "PlayAnimAsync",
        &[file("art\\loop.sprite")],
    );
    runtime.tick(10);
    assert_eq!(
        call_native(&mut runtime, id, "SetSprite", &[file("art\\blue.png")]),
        Value::Bool(true)
    );
    runtime.tick(10);
    assert!(runtime.world.scene().unwrap().is_playing_anim(r));
    assert_eq!(shown(&runtime).as_deref(), Some("art\\loop.sprite"));
    call_native(&mut runtime, id, "Reset", &[]);
    runtime.tick(10);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\blue.png"));
    // A talk sprite removed while it plays: the entity's own sprite
    // takes its place.
    call_native(
        &mut runtime,
        id,
        "AddTalkSprite",
        &[file("art\\loop.sprite"), Value::Bool(false)],
    );
    runtime
        .world
        .talk(id, "Hi", None, 2000, None, TextAlign::Center);
    runtime.tick(10);
    runtime.tick(10);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\loop.sprite"));
    call_native(
        &mut runtime,
        id,
        "RemoveTalkSprite",
        &[file("art\\loop.sprite"), Value::Bool(false)],
    );
    assert_eq!(shown(&runtime).as_deref(), Some("art\\blue.png"));
    runtime.tick(10);
    assert!(runtime.world.is_talking(id));
    assert_eq!(shown(&runtime).as_deref(), Some("art\\blue.png"));
    runtime.world.stop_talk(id);
    runtime.tick(10);
    // A file that does not load leaves the entity without a sprite: the
    // old one went first.
    assert_eq!(
        call_native(&mut runtime, id, "SetSprite", &[file("art\\broken.sprite")]),
        Value::Bool(false)
    );
    assert_eq!(own(&runtime), None);
    std::fs::remove_dir_all(&dir).ok();
}
