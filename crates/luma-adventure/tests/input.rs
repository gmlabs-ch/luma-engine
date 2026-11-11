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
result.clicks = 0;
result.doubles = 0;
result.releases = 0;
result.rights = 0;
result.middles = 0;
result.fired = 0;
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

on "OpenNote"
{
  global note = Game.LoadWindow("note.window");
  note.Visible = true;
}

on "LeftClick"
{
  result.clicks = result.clicks + 1;
}

on "LeftDoubleClick"
{
  result.doubles = result.doubles + 1;
}

on "LeftRelease"
{
  result.releases = result.releases + 1;
}

on "RightClick"
{
  result.rights = result.rights + 1;
}

on "MiddleClick"
{
  result.middles = result.middles + 1;
}

on "MouseWheelDown"
{
  result.wheel = "game down";
}

on "Music"
{
  Game.PlayMusic("audio\long.wav");
}

on "Warp"
{
  Game.SetMousePos(500, 0 - 3);
}

on "WarpX"
{
  Game.MouseX = 10;
}

on "Lock"
{
  Game.LockMouseRect(20, 20, 10, 10);
}

on "Reorder"
{
  var name = win.GetWidget("name");
  result.moved = name.MoveToTop();
}
"#;

/// Counts the editor's notifications.
const WINDOW_SCRIPT: &str = r#"
global result;

on "name"
{
  result.edits = result.edits + 1;
}

on "ok"
{
  result.fired = result.fired + 1;
}

on "MouseWheelUp"
{
  result.wheel = "window up";
}
"#;

const WINDOW: &[u8] = b"WINDOW { NAME=\"w\" X=0 Y=0 WIDTH=40 HEIGHT=30
  EDIT { NAME=\"name\" X=1 Y=1 WIDTH=30 HEIGHT=8 PARENT_NOTIFY=TRUE }
  EDIT { NAME=\"code\" X=1 Y=10 WIDTH=30 HEIGHT=8 MAX_LENGTH=4 }
  BUTTON { NAME=\"ok\" X=1 Y=20 WIDTH=10 HEIGHT=8 PARENT_NOTIFY=TRUE }
}";

/// A second window, right of the first.
const NOTE_WINDOW: &[u8] = b"WINDOW { NAME=\"note\" X=44 Y=0 WIDTH=20 HEIGHT=16
  EDIT { NAME=\"text\" X=1 Y=1 WIDTH=18 HEIGHT=8 }
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
    pkg.add_file("note.window", NOTE_WINDOW.to_vec());
    pkg.add_file("audio\\long.wav", luma_audio::silent_wav(8000, 1, 5000));
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

/// Moves the mouse, lets a frame find what is under it, then presses
/// and releases the left button a frame apart.
fn click(runtime: &mut Runtime, x: i32, y: i32) {
    runtime.mouse_move(x, y);
    runtime.tick(16);
    runtime.mouse_left_down();
    runtime.tick(16);
    runtime.mouse_left_up();
    runtime.tick(16);
}

/// Lets the double-click delay pass.
fn wait(runtime: &mut Runtime) {
    for _ in 0..30 {
        runtime.tick(16);
    }
}

#[test]
fn a_click_gives_the_focus_and_the_topmost_window_takes_the_keys() {
    let (mut runtime, dir, name, code) = start();
    let window = runtime.world.window_ids()[0];
    // Clicking the second editor lets the player type in it.
    click(&mut runtime, 5, 12);
    assert!(runtime.world.is_focused(code));
    assert!(!runtime.world.is_focused(name));
    type_text(&mut runtime, "42");
    assert_eq!(text(&runtime, code), "42");
    assert_eq!(text(&runtime, name), "");
    // A window shown above the others takes the keys from the next
    // frame, without a click: each frame focuses the topmost visible
    // window.
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "OpenNote", false);
    runtime.tick(16);
    runtime.tick(16);
    let note = runtime.world.window_ids()[1];
    assert_eq!(runtime.world.focused_window(), Some(note));
    // Clicking the first window raises it: it is now the last of the
    // list, so it keeps the keys on the following frames.
    wait(&mut runtime);
    click(&mut runtime, 20, 25);
    assert_eq!(runtime.world.window_ids(), [note, window]);
    runtime.tick(16);
    assert_eq!(runtime.world.focused_window(), Some(window));
    type_text(&mut runtime, "7");
    assert_eq!(text(&runtime, code), "427");
    // And clicking the other window's editor raises that one.
    wait(&mut runtime);
    click(&mut runtime, 50, 4);
    assert_eq!(runtime.world.window_ids(), [window, note]);
    let note_text = runtime.world.widget(note, "text").unwrap();
    assert!(runtime.world.is_focused(note_text));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_double_click_is_not_a_click() {
    let (mut runtime, dir, _, _) = start();
    let window = runtime.world.window_ids()[0];
    let ok = runtime.world.widget(window, "ok").unwrap();
    click(&mut runtime, 5, 24);
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "clicks"), Value::Int(1));
    assert_eq!(result(&runtime, "releases"), Value::Int(1));
    assert_eq!(result(&runtime, "fired"), Value::Int(1));
    // A second press within 400 ms at the same point: the system's
    // double-click message replaces the click. The button is not held,
    // nothing is captured, the button under the mouse does not fire.
    runtime.mouse_left_down();
    assert!(!runtime.world.mouse().left_down);
    runtime.tick(16);
    assert!(!runtime.world.control(ok).unwrap().button().unwrap().press);
    runtime.mouse_left_up();
    for _ in 0..3 {
        runtime.tick(16);
    }
    assert_eq!(result(&runtime, "clicks"), Value::Int(1));
    assert_eq!(result(&runtime, "doubles"), Value::Int(1));
    assert_eq!(result(&runtime, "releases"), Value::Int(2));
    assert_eq!(result(&runtime, "fired"), Value::Int(1));
    // A third press starts over.
    click(&mut runtime, 5, 24);
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "clicks"), Value::Int(2));
    assert_eq!(result(&runtime, "fired"), Value::Int(2));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn buttons_pressed_outside_the_frame_are_ignored() {
    let (mut runtime, dir, _, _) = start();
    // In the bars around a letterboxed frame the pointer is outside the
    // screen (64x32 here; the edges count as inside).
    for (x, y) in [(-1, 10), (70, 10), (10, -3), (10, 33)] {
        runtime.mouse_move(x, y);
        runtime.tick(16);
        runtime.mouse_left_down();
        runtime.mouse_left_up();
        runtime.mouse_right_down();
        runtime.mouse_right_up();
        wait(&mut runtime);
    }
    assert_eq!(result(&runtime, "clicks"), Value::Int(0));
    assert_eq!(result(&runtime, "rights"), Value::Int(0));
    runtime.mouse_move(64, 32);
    runtime.tick(16);
    runtime.mouse_left_down();
    runtime.mouse_left_up();
    runtime.mouse_right_down();
    runtime.mouse_right_up();
    wait(&mut runtime);
    assert_eq!(result(&runtime, "clicks"), Value::Int(1));
    assert_eq!(result(&runtime, "rights"), Value::Int(1));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_game_that_is_not_interactive_gets_no_click() {
    let (mut runtime, dir, _, _) = start();
    let game = runtime.world.game_id();
    {
        use luma_script::Natives;
        runtime
            .world
            .set_property(game, "Interactive", &Value::Bool(false));
    }
    // Over the scene (right of the window): nothing is under the mouse.
    runtime.mouse_move(50, 20);
    runtime.tick(16);
    runtime.mouse_left_down();
    runtime.mouse_left_up();
    runtime.mouse_right_down();
    runtime.mouse_right_up();
    runtime.mouse_middle_down();
    runtime.mouse_middle_up();
    wait(&mut runtime);
    assert_eq!(result(&runtime, "clicks"), Value::Int(0));
    assert_eq!(result(&runtime, "rights"), Value::Int(0));
    assert_eq!(result(&runtime, "middles"), Value::Int(0));
    // The button was never held for the game.
    assert!(!runtime.world.mouse().left_down);
    {
        use luma_script::Natives;
        runtime
            .world
            .set_property(game, "Interactive", &Value::Bool(true));
    }
    runtime.mouse_middle_down();
    runtime.mouse_middle_up();
    wait(&mut runtime);
    assert_eq!(result(&runtime, "middles"), Value::Int(1));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_wheel_goes_up_on_a_window_that_only_handles_up() {
    let (mut runtime, dir, _, _) = start();
    // The focused window handles `MouseWheelUp` only: as in the
    // original, a wheel turned down raises `MouseWheelUp` on it, and
    // the game's `MouseWheelDown` handler is not reached.
    runtime.mouse_wheel(-1);
    runtime.tick(16);
    assert_eq!(result(&runtime, "wheel"), Value::from("window up"));
    // Without a focused window the game gets the direction turned.
    let window = runtime.world.window_ids()[0];
    {
        use luma_script::Natives;
        runtime
            .world
            .set_property(window, "Visible", &Value::Bool(false));
    }
    runtime.tick(16);
    runtime.mouse_wheel(-1);
    runtime.tick(16);
    assert_eq!(result(&runtime, "wheel"), Value::from("game down"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn move_to_top_puts_a_control_last() {
    let (mut runtime, dir, name, code) = start();
    let window = runtime.world.window_ids()[0];
    let ok = runtime.world.widget(window, "ok").unwrap();
    assert_eq!(runtime.world.widgets_of(window), [name, code, ok]);
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Reorder", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "moved"), Value::Bool(true));
    assert_eq!(runtime.world.widgets_of(window), [code, ok, name]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_inactive_window_pauses_the_sounds() {
    let (mut runtime, dir, _, _) = start();
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Music", false);
    runtime.tick(16);
    runtime.tick(16);
    let music = runtime.world.music(0).expect("the music started").id;
    assert!(runtime.is_active());
    assert!(runtime.world.is_music_playing(0));
    assert!(!runtime.world.audio().is_freeze_paused(music));
    // The window loses the focus: every playing sound is paused, and
    // the front-end runs no frame while `is_active` is false.
    runtime.activate(false, false);
    assert!(!runtime.is_active());
    assert!(runtime.world.audio().is_freeze_paused(music));
    // Back: the sounds go on, and the object under the mouse is found
    // again when a click activated the window.
    runtime.world.set_mouse(5, 24);
    runtime.activate(true, true);
    assert!(runtime.is_active());
    assert!(!runtime.world.audio().is_freeze_paused(music));
    assert!(runtime.world.is_music_playing(0));
    let window = runtime.world.window_ids()[0];
    assert_eq!(
        runtime.world.active_object(),
        runtime.world.widget(window, "ok")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_game_moves_the_system_cursor() {
    let (mut runtime, dir, _, _) = start();
    let game = runtime.world.game_id();
    let raise = |runtime: &mut Runtime, event: &str| {
        runtime
            .engine
            .apply_event(&mut runtime.world, game, event, false);
        runtime.tick(16);
    };
    let at = |runtime: &Runtime| (runtime.world.mouse().x, runtime.world.mouse().y);
    // A mouse movement is the system's: nothing to move.
    runtime.mouse_move(30, 5);
    assert_eq!(runtime.world.take_cursor_warp(), None);
    // `SetMousePos` keeps the point on the screen (64x32, far edges
    // included) and asks the front-end to put the system cursor there.
    raise(&mut runtime, "Warp");
    assert_eq!(at(&runtime), (64, 0));
    assert_eq!(runtime.world.take_cursor_warp(), Some((64, 0)));
    assert_eq!(runtime.world.take_cursor_warp(), None, "asked once");
    // `Game.MouseX = ...` moves it too (`ResetMousePos`).
    raise(&mut runtime, "WarpX");
    assert_eq!(at(&runtime), (10, 0));
    assert_eq!(runtime.world.take_cursor_warp(), Some((10, 0)));
    // The mouse lock, set while the mouse is outside the rectangle,
    // brings it back at the next frame, and the system cursor with it.
    raise(&mut runtime, "Lock");
    assert_eq!(runtime.world.take_cursor_warp(), None);
    runtime.tick(16);
    assert_eq!(at(&runtime), (10, 10));
    assert_eq!(runtime.world.take_cursor_warp(), Some((10, 10)));
    // A movement out of the rectangle is put back on its edge; one
    // inside is left alone.
    runtime.mouse_move(40, 15);
    assert_eq!(at(&runtime), (20, 15));
    assert_eq!(runtime.world.take_cursor_warp(), Some((20, 15)));
    runtime.mouse_move(12, 18);
    assert_eq!(at(&runtime), (12, 18));
    assert_eq!(runtime.world.take_cursor_warp(), None);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_engine_keeps_its_own_settings_beside_the_games() {
    let (mut runtime, dir, _, _) = start();
    assert_eq!(runtime.world.read_engine_int("Video", "WindowPosX"), None);
    assert!(runtime.world.write_setting("Lang", "fr"));
    assert!(
        runtime
            .world
            .write_engine_setting("Video", "WindowPosX", "-120")
    );
    assert!(
        runtime
            .world
            .write_engine_setting("Video", "WindowPosY", "48")
    );
    assert!(runtime.world.write_engine_setting("Video", "Windowed", "0"));
    assert_eq!(
        runtime.world.read_engine_int("video", "windowposx"),
        Some(-120)
    );
    assert_eq!(
        runtime.world.read_engine_int("Video", "WindowPosY"),
        Some(48)
    );
    assert_eq!(runtime.world.read_engine_int("Video", "Windowed"), Some(0));
    // As `GetPrivateProfileInt`: the leading integer, 0 without one.
    assert!(
        runtime
            .world
            .write_engine_setting("Video", "Windowed", "true")
    );
    assert_eq!(runtime.world.read_engine_int("Video", "Windowed"), Some(0));
    // The game's private settings are a section of the same file.
    assert_eq!(runtime.world.read_setting("Lang").as_deref(), Some("fr"));
    assert_eq!(runtime.world.read_setting("WindowPosX"), None);
    let text = std::fs::read_to_string(runtime.world.settings_ini_path()).unwrap();
    assert!(text.contains("[Video]\nWindowPosX=-120\n"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn random_numbers_are_seeded_from_the_clock_and_can_be_fixed() {
    use luma_script::Host;
    let draws = |runtime: &mut Runtime| -> Vec<i32> {
        (0..8).map(|_| runtime.world.random(0, 30000)).collect()
    };
    // Two games started one after the other draw different numbers, as
    // two launches of the original do.
    let (mut first, dir1, _, _) = start();
    let (mut second, dir2, _, _) = start();
    assert_ne!(draws(&mut first), draws(&mut second));
    // Under a seed the draws are the same at every run.
    first.world.seed_random(5);
    second.world.seed_random(5);
    let seeded = draws(&mut first);
    assert_eq!(seeded, draws(&mut second));
    first.world.seed_random(6);
    assert_ne!(draws(&mut first), seeded);
    first.world.seed_random(luma_adventure::FIXED_SEED);
    second.world.seed_random(0);
    assert_eq!(draws(&mut first), draws(&mut second), "0 is the fixed seed");
    // As the original's `rand()` gives 15 bits, a span wider than 32768
    // values is never covered; reversed bounds are put in order.
    for _ in 0..2000 {
        let value = first.world.random(1000, 2_000_000);
        assert!((1000..=1000 + 32767).contains(&value), "{value}");
        let value = first.world.random(6, 1);
        assert!((1..=6).contains(&value), "{value}");
    }
    assert_eq!(first.world.random(7, 7), 7);
    assert_eq!(first.world.random(i32::MIN, i32::MAX), i32::MIN);
    std::fs::remove_dir_all(&dir1).ok();
    std::fs::remove_dir_all(&dir2).ok();
}
