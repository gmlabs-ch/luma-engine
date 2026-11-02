//! Accessibility in a game: the `Game.Acc*` switches, `AccOutputText`,
//! captions and sentences read out through a recording speech backend,
//! `AccCaption` on objects (kept in saved games), keyboard picking of
//! the clickable objects with Ctrl+Tab and the Ctrl+Space pause.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, RecordingSpeech, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_script::Value;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-access-game-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const VK_TAB: i32 = 9;
const VK_CONTROL: i32 = 17;
const VK_SPACE: i32 = 32;

const GAME_SCRIPT: &str = r#"
global result = new Object();
result.tts = Game.AccTTSEnabled;
result.talk = Game.AccTTSTalk;
result.captions = Game.AccTTSCaptions;
result.keypress = Game.AccTTSKeypress;
result.kb = Game.AccKeyboardEnabled;
result.skip = Game.AccKeyboardCursorSkip;
result.pause = Game.AccKeyboardPause;
Game.AccOutputText("Welcome", 0);
Game.ChangeScene("scenes\acc.scene");

on "Check"
{
  var scene = Game.Scene;
  var door = scene.GetNode("door");
  result.before = door.AccCaption;
  door.AccCaption = "A heavy door";
  result.after = door.AccCaption;
  var lamp = scene.GetNode("lamp");
  lamp.AccCaption = "x";
  lamp.AccCaption = null;
  result.cleared = lamp.AccCaption;
  Game.AccCaption = "The game";
  result.game = Game.AccCaption;
  door.TalkAsync("Knock knock");
}
"#;

const SCENE: &[u8] = b"SCENE {
  NAME=\"Acc\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=64 HEIGHT=32
    ENTITY { NAME=\"door\" CAPTION=\"Door\" X=8 Y=8 REGION { POINT {0,0} POINT {16,0} POINT {16,16} POINT {0,16} } }
    ENTITY { NAME=\"lamp\" CAPTION=\"Lamp\" X=40 Y=8 REGION { POINT {32,0} POINT {48,0} POINT {48,16} POINT {32,16} } }
    ENTITY { NAME=\"hidden\" CAPTION=\"Hidden\" X=2 Y=18 ACTIVE=FALSE REGION { POINT {0,16} POINT {4,16} POINT {4,20} POINT {0,20} } }
  }
}";

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
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 64,32 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Access\" SCRIPT=\"game.script\" }".to_vec(),
    );
    pkg.add_file("game.script", compile(GAME_SCRIPT, "game.script"));
    pkg.add_file("scenes\\acc.scene", SCENE.to_vec());
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

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

fn node(runtime: &Runtime, layer: usize, index: usize) -> luma_script::NativeId {
    runtime
        .world
        .node_id(luma_adventure::EntityRef::Node { layer, index })
        .unwrap()
}

/// Presses a key with Ctrl held.
fn ctrl_key(runtime: &mut Runtime, code: i32, text: Option<char>, shift: bool) {
    runtime.key_down(VK_CONTROL, None, shift, false, true);
    runtime.key_down(code, text, shift, false, true);
    runtime.key_up(code);
}

#[test]
fn speech_captions_and_keyboard_play() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    let recording = RecordingSpeech::default();
    {
        let access = runtime.world.accessibility_mut();
        access.tts_enabled = true;
        access.keyboard_enabled = true;
        access.keyboard_pause = true;
        access.set_speech(Some(Box::new(recording.clone())));
    }
    runtime.start();
    let mut log = runtime.tick(16);
    log.extend(runtime.tick(16));
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
    for key in ["tts", "talk", "captions", "keypress", "kb", "skip", "pause"] {
        assert_eq!(result(&runtime, key), Value::Bool(true), "{key}");
    }
    assert_eq!(recording.spoken(), ["Welcome"]);
    assert!(runtime.world.scene().is_some(), "{log:?}");

    // The caption of the object the mouse comes over is read out.
    runtime.world.skip_to(0, 0);
    runtime.mouse_move(4, 4);
    runtime.tick(16);
    let door = node(&runtime, 0, 0);
    let lamp = node(&runtime, 0, 1);
    assert_eq!(runtime.world.active_object(), Some(door));
    assert_eq!(recording.spoken(), ["Welcome", "Door"]);

    // `AccCaption` replaces the caption; a sentence without a speech
    // file is read out, and lets another finish.
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Check", false);
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "before"), Value::Null);
    assert_eq!(result(&runtime, "after"), Value::from("A heavy door"));
    assert_eq!(result(&runtime, "cleared"), Value::Null);
    assert_eq!(result(&runtime, "game"), Value::from("The game"));
    assert_eq!(runtime.world.access_caption(door), Some("A heavy door"));
    assert_eq!(runtime.world.spoken_caption(lamp), "Lamp");
    let record = recording.record();
    assert_eq!(
        record.spoken.last(),
        Some(&("Knock knock".to_string(), true))
    );
    runtime.mouse_move(30, 30);
    runtime.tick(16);
    runtime.mouse_move(4, 4);
    runtime.tick(16);
    assert_eq!(
        recording.record().spoken.last(),
        Some(&("A heavy door".to_string(), false)),
        "a caption does not cut a sentence short"
    );
    runtime.world.skip_sentences();
    assert_eq!(recording.record().stops, 1);

    // Ctrl+Tab picks the clickable objects in turn (the inactive one is
    // skipped), the cursor jumping to each; Ctrl+Shift+Tab goes back.
    ctrl_key(&mut runtime, VK_TAB, None, false);
    runtime.tick(16);
    assert_eq!(runtime.world.accessibility().active, Some(door));
    assert_eq!(runtime.world.accessibility().hint, Some((0, 0, 16, 16)));
    assert_eq!((runtime.world.mouse().x, runtime.world.mouse().y), (8, 8));
    ctrl_key(&mut runtime, VK_TAB, None, false);
    runtime.tick(16);
    assert_eq!(runtime.world.accessibility().active, Some(lamp));
    assert_eq!((runtime.world.mouse().x, runtime.world.mouse().y), (40, 8));
    runtime.tick(16);
    assert_eq!(
        runtime.world.active_object(),
        Some(lamp),
        "the object under the moved cursor, from the next frame"
    );
    ctrl_key(&mut runtime, VK_TAB, None, false);
    assert_eq!(
        runtime.world.accessibility().active,
        Some(door),
        "wraps around"
    );
    ctrl_key(&mut runtime, VK_TAB, None, true);
    assert_eq!(runtime.world.accessibility().active, Some(lamp));
    assert_eq!(runtime.world.access_objects(), [door, lamp]);
    runtime.key_up(VK_CONTROL);

    // Ctrl+Space pauses the game under a shield window, and resumes it.
    let windows = runtime.world.window_ids().len();
    ctrl_key(&mut runtime, VK_SPACE, Some(' '), false);
    runtime.tick(16);
    assert!(runtime.world.accessibility().paused);
    assert!(runtime.world.is_frozen());
    let shield = runtime.world.accessibility().shield.unwrap();
    assert_eq!(runtime.world.window_ids().len(), windows + 1);
    let label = runtime.world.widgets_of(shield)[0];
    assert_eq!(
        runtime.world.control(label).unwrap().text,
        "Game paused. Press Ctrl+Space to resume."
    );
    ctrl_key(&mut runtime, VK_SPACE, Some(' '), false);
    runtime.tick(16);
    assert!(!runtime.world.accessibility().paused);
    assert!(!runtime.world.is_frozen());
    assert_eq!(runtime.world.accessibility().shield, None);
    assert_eq!(runtime.world.window_ids().len(), windows);

    // The captions are saved with the game.
    runtime.save_game(3, "access", false).unwrap();
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(3).unwrap();
    loaded.tick(16);
    let door = node(&loaded, 0, 0);
    assert_eq!(loaded.world.access_caption(door), Some("A heavy door"));
    assert_eq!(loaded.world.spoken_caption(door), "A heavy door");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn switches_come_from_the_settings_file() {
    let dir = game();
    let saves = dir.join("saves");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(
        saves.join("settings.ini"),
        "[Accessibility]\nTTSEnabled=0\nTTSTalk=false\nKeyboardEnabled=1\nKeyboardCursorSkip=0\n",
    )
    .unwrap();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    assert_eq!(result(&runtime, "tts"), Value::Bool(false));
    assert_eq!(result(&runtime, "talk"), Value::Bool(false));
    assert_eq!(result(&runtime, "captions"), Value::Bool(true));
    assert_eq!(result(&runtime, "kb"), Value::Bool(true));
    assert_eq!(result(&runtime, "skip"), Value::Bool(false));
    assert_eq!(result(&runtime, "pause"), Value::Bool(false));
    assert!(
        !runtime.world.accessibility().tts_available(),
        "no backend is started while text-to-speech is off"
    );
    std::fs::remove_dir_all(&dir).ok();
}
