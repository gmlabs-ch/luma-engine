//! The keyboard and the mouse as the original engine receives them: a
//! key and its character as two messages, key repeats, the editor's
//! keys and its clipboard, the focus a click gives, the mouse buttons'
//! rules (see `docs/game-loop.md`).

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_script::{NativeId, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-input-game-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const VK_BACK: i32 = 8;
const VK_END: i32 = 35;
const VK_HOME: i32 = 36;
const VK_LEFT: i32 = 37;
const VK_DELETE: i32 = 46;
const VK_F1: i32 = 112;

/// Opens a window with two editors, focuses the first and records every
/// `Keypress`.
const GAME_SCRIPT: &str = r#"
global result = new Object();
result.presses = 0;
result.edits = 0;
global win = Game.LoadWindow("w.window");
win.AttachScript("w.script");
win.Visible = true;
var name = win.GetWidget("name");
name.Focus();

on "Keypress"
{
  var kb = Game.Keyboard;
  result.presses = result.presses + 1;
  result.code = kb.KeyCode;
  result.printable = kb.Printable;
  result.key = kb.Key;
  result.shift = kb.IsShift;
  result.text = ToString(kb);
}
"#;

/// Counts the editor's notifications.
const WINDOW_SCRIPT: &str = r#"
global result;

on "name"
{
  result.edits = result.edits + 1;
}
"#;

const WINDOW: &[u8] = b"WINDOW { NAME=\"w\" X=0 Y=0 WIDTH=60 HEIGHT=30
  EDIT { NAME=\"name\" X=1 Y=1 WIDTH=50 HEIGHT=10 PARENT_NOTIFY=TRUE }
  EDIT { NAME=\"code\" X=1 Y=12 WIDTH=50 HEIGHT=10 MAX_LENGTH=4 }
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
        b"GAME { NAME=\"Input\" SCRIPT=\"game.script\" }".to_vec(),
    );
    pkg.add_file("game.script", compile(GAME_SCRIPT, "game.script"));
    pkg.add_file("w.script", compile(WINDOW_SCRIPT, "w.script"));
    pkg.add_file("w.window", WINDOW.to_vec());
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

/// The game started, its window up and the first editor focused.
fn start() -> (Runtime, std::path::PathBuf, NativeId, NativeId) {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let mut log = runtime.tick(16);
    log.extend(runtime.tick(16));
    assert!(
        !log.iter()
            .any(|l| l.contains("error") || l.contains("Error")),
        "{log:?}"
    );
    let window = runtime.world.window_ids()[0];
    let name = runtime.world.widget(window, "name").unwrap();
    let code = runtime.world.widget(window, "code").unwrap();
    assert!(runtime.world.is_focused(name));
    (runtime, dir, name, code)
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

fn text(runtime: &Runtime, edit: NativeId) -> String {
    runtime.world.control(edit).unwrap().text.clone()
}

fn selection(runtime: &Runtime, edit: NativeId) -> (usize, usize) {
    let e = runtime.world.control(edit).unwrap().edit().unwrap();
    (e.sel_start, e.sel_end)
}

/// Types a text as a US keyboard does: each key, then its character.
fn type_text(runtime: &mut Runtime, text: &str) {
    for ch in text.chars() {
        match luma_adventure::input::key_for_char(ch) {
            Some((code, shift)) => {
                runtime.key_down(code, Some(ch), shift, false, false);
                runtime.key_up(code);
            }
            None => runtime.key_char(ch, false, false, false),
        }
    }
}

/// Presses a key with Ctrl held.
fn ctrl(runtime: &mut Runtime, code: i32) {
    runtime.key_down(code, None, false, false, true);
    runtime.key_up(code);
}

#[test]
fn a_key_and_its_character_are_two_messages() {
    let (mut runtime, dir, name, _) = start();
    // The A key: `Keypress` is raised by the key, and its handler runs
    // once the character has replaced the key in the keyboard state.
    runtime.key_down(65, Some('a'), false, false, false);
    runtime.key_up(65);
    runtime.tick(16);
    assert_eq!(result(&runtime, "presses"), Value::Int(1));
    assert_eq!(result(&runtime, "code"), Value::Int(97));
    assert_eq!(result(&runtime, "printable"), Value::Bool(true));
    assert_eq!(result(&runtime, "key"), Value::from("a"));
    assert_eq!(result(&runtime, "text"), Value::from("[keyboard state]"));
    assert_eq!(text(&runtime, name), "a");
    // A key that types nothing keeps its own code.
    runtime.key_down(VK_F1, None, true, false, false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "presses"), Value::Int(2));
    assert_eq!(result(&runtime, "code"), Value::Int(VK_F1));
    assert_eq!(result(&runtime, "printable"), Value::Bool(false));
    assert_eq!(result(&runtime, "key"), Value::from(""));
    assert_eq!(result(&runtime, "shift"), Value::Bool(true));
    // A character alone raises nothing.
    runtime.key_char('b', false, false, false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "presses"), Value::Int(2));
    assert_eq!(text(&runtime, name), "ab");
    assert_eq!(runtime.world.keyboard().key_code, 98);
    // The key message alone raises `Keypress` and types nothing.
    runtime.key_message(67, false, false, false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "presses"), Value::Int(3));
    assert_eq!(result(&runtime, "code"), Value::Int(67));
    assert_eq!(text(&runtime, name), "ab");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn punctuation_and_accents_are_typed_not_taken_for_editing_keys() {
    let (mut runtime, dir, name, _) = start();
    // The ASCII codes of these characters are the virtual key codes of
    // Delete (.), the arrows (% & ' (), End and Home (# $) and the page
    // keys (! ").
    let typed = "ab.%&'(#$!\"";
    type_text(&mut runtime, typed);
    assert_eq!(text(&runtime, name), typed);
    assert_eq!(selection(&runtime, name), (typed.len(), typed.len()));
    // Characters outside ASCII reach the editor; their code is the
    // Windows-1252 byte, or the code point beyond the code page.
    type_text(&mut runtime, "é");
    assert_eq!(runtime.world.keyboard().key_code, 233);
    assert_eq!(runtime.world.keyboard().key, "é");
    type_text(&mut runtime, "€");
    assert_eq!(runtime.world.keyboard().key_code, 128);
    type_text(&mut runtime, "ж");
    assert_eq!(runtime.world.keyboard().key_code, 0x436);
    assert_eq!(text(&runtime, name), format!("{typed}é€ж"));
    // The keys themselves still edit: Delete is a key, not a dot.
    runtime.key_down(VK_HOME, None, false, false, false);
    runtime.key_down(VK_DELETE, None, false, false, false);
    assert_eq!(text(&runtime, name), format!("{}é€ж", &typed[1..]));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_held_key_repeats_its_key_and_its_character() {
    let (mut runtime, dir, name, _) = start();
    for _ in 0..3 {
        runtime.key_down(65, Some('a'), false, false, false);
    }
    runtime.key_up(65);
    runtime.tick(16);
    assert_eq!(text(&runtime, name), "aaa");
    assert_eq!(result(&runtime, "presses"), Value::Int(3));
    // Backspace is a character (8): the key raises `Keypress`, the
    // character deletes; both repeat.
    for _ in 0..2 {
        runtime.key_down(VK_BACK, None, false, false, false);
    }
    runtime.key_up(VK_BACK);
    runtime.tick(16);
    assert_eq!(text(&runtime, name), "a");
    assert_eq!(result(&runtime, "presses"), Value::Int(5));
    assert_eq!(result(&runtime, "code"), Value::Int(8));
    assert_eq!(result(&runtime, "printable"), Value::Bool(true));
    // The editor eats the keys it uses: no `Keypress` for an arrow or
    // for Delete.
    runtime.key_down(VK_LEFT, None, false, false, false);
    runtime.key_down(VK_DELETE, None, false, false, false);
    runtime.tick(16);
    assert_eq!(text(&runtime, name), "");
    assert_eq!(result(&runtime, "presses"), Value::Int(5));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_editor_notifies_its_parent_for_each_deletion_and_insertion() {
    let (mut runtime, dir, name, _) = start();
    type_text(&mut runtime, "abcd");
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "edits"), Value::Int(4));
    // Shift+Home selects to the start; a character typed over the
    // selection deletes, then inserts: two notifications.
    runtime.key_down(VK_HOME, None, true, false, false);
    assert_eq!(selection(&runtime, name), (4, 0));
    type_text(&mut runtime, "x");
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(text(&runtime, name), "x");
    assert_eq!(result(&runtime, "edits"), Value::Int(6));
    // Backspace on an empty text still calls `DeleteChars`.
    runtime.key_down(VK_BACK, None, false, false, false);
    runtime.key_down(VK_BACK, None, false, false, false);
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(text(&runtime, name), "");
    assert_eq!(result(&runtime, "edits"), Value::Int(8));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_editor_copies_cuts_and_pastes() {
    let (mut runtime, dir, name, code) = start();
    type_text(&mut runtime, "hello world");
    // Ctrl+A selects all, Ctrl+C copies, End drops the selection,
    // Ctrl+V pastes at the caret.
    ctrl(&mut runtime, 65);
    assert_eq!(selection(&runtime, name), (0, 11));
    ctrl(&mut runtime, 67);
    assert_eq!(text(&runtime, name), "hello world");
    runtime.key_down(VK_END, None, false, false, false);
    ctrl(&mut runtime, 86);
    assert_eq!(text(&runtime, name), "hello worldhello world");
    assert_eq!(selection(&runtime, name), (22, 22));
    // Ctrl+X cuts the selection; pasting over a selection replaces it.
    ctrl(&mut runtime, 65);
    ctrl(&mut runtime, 88);
    assert_eq!(text(&runtime, name), "");
    type_text(&mut runtime, "abc");
    runtime.key_down(VK_LEFT, None, true, false, false);
    runtime.key_down(VK_LEFT, None, true, false, false);
    assert_eq!(selection(&runtime, name), (3, 1));
    ctrl(&mut runtime, 86);
    assert_eq!(text(&runtime, name), "ahello worldhello world");
    // The other Ctrl letters type nothing, and Ctrl+A is not taken
    // while Alt is held (AltGr).
    ctrl(&mut runtime, 66);
    assert_eq!(text(&runtime, name), "ahello worldhello world");
    runtime.key_down(VK_END, None, false, false, false);
    runtime.key_char('\u{1}', false, true, true);
    assert_eq!(selection(&runtime, name), (23, 23));
    // A paste stops at the editor's `MAX_LENGTH` and drops control
    // characters.
    runtime.world.focus_control(code);
    {
        use luma_adventure::Clipboard;
        let mut clipboard = luma_adventure::MemoryClipboard::default();
        clipboard.set_text("1\r\n23456");
        runtime.world.set_clipboard(Box::new(clipboard));
    }
    ctrl(&mut runtime, 86);
    assert_eq!(text(&runtime, code), "1234");
    assert_eq!(selection(&runtime, code), (4, 4));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn is_key_down_answers_for_the_mouse_buttons() {
    let (mut runtime, dir, _, _) = start();
    let down = |runtime: &Runtime, code: i32| runtime.world.keyboard().is_key_down(code);
    assert!(!down(&runtime, 1) && !down(&runtime, 2) && !down(&runtime, 4));
    runtime.mouse_left_down();
    runtime.mouse_right_down();
    runtime.mouse_middle_down();
    assert!(down(&runtime, 1) && down(&runtime, 2) && down(&runtime, 4));
    // The button is up at once, even when its release waits for a frame
    // to have seen it held.
    runtime.mouse_left_up();
    runtime.mouse_right_up();
    runtime.mouse_middle_up();
    assert!(!down(&runtime, 1) && !down(&runtime, 2) && !down(&runtime, 4));
    std::fs::remove_dir_all(&dir).ok();
}
