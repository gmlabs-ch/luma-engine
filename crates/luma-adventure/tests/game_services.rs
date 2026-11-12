//! The game services scripts reach through `Game`, `Scene` and the items
//! beyond the scene itself: the cursors and the items' images as sprite
//! objects.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, EntityRef, Kind, Runtime};
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

// `new Actor()` and `new Entity()`: empty objects of the game, or
// loaded from a file.
global a = new Actor();
result.actorType = a.Type;
result.actorValid = Game.ValidObject(a);
a.X = 12;
a.Y = 34;
result.actorX = a.X;
global e = new Entity();
result.entityType = e.Type;
result.entityValid = Game.ValidObject(e);
result.entitySpriteSet = e.SetSprite("art\red.png");
result.entitySprite = e.GetSprite();
e.X = 5;
global f = new Entity("art\free.entity");
result.fileEntityName = f.Name;
result.fileEntityX = f.X;
global m = new Actor("actors\mo.actor");
result.fileActorName = m.Name;
var missing = new Entity("nowhere.entity");
result.missingType = missing.Type;
Game.UnloadObject(a);
result.actorGone = Game.ValidObject(a);

// Right-to-left text: a flag scripts and string tables set.
result.rtlBefore = Game.TextRTL;
Game.TextRTL = true;
result.rtlSet = Game.TextRTL;
Game.TextRTL = false;
Game.LoadStringTable("rtl.tab");
result.rtlTable = Game.TextRTL;
result.rtlGreet = Game.ExpandString("/GREET/x");
Game.TextRTL = false;

Game.ChangeScene("scenes\room.scene");

on "Check"
{
  result.checked = true;
  result.entityKept = Game.ValidObject(e);
  result.fileEntityKept = Game.ValidObject(f);
  result.fileActorKept = Game.ValidObject(m);
  result.suspendedBefore = Game.SuspendedRendering;
  Game.SuspendedRendering = true;
  result.suspendedAfter = Game.SuspendedRendering;
}

on "Resume"
{
  Game.SuspendedRendering = false;
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
    pkg.add_file("rtl.tab", b"@right-to-left\r\nGREET\tShalom\r\n".to_vec());
    pkg.add_file(
        "utf8.tab",
        "\u{feff}GREET\té č Ж Ω\r\nTWO\tun|deux\r\n"
            .as_bytes()
            .to_vec(),
    );
    pkg.add_file("ansi.tab", b"GREET\tcaf\xe9\r\n".to_vec());
    pkg.add_file(
        "art\\free.entity",
        b"ENTITY { NAME=\"free\" SPRITE=\"art\\green.png\" X=3 Y=4 }".to_vec(),
    );
    pkg.add_file(
        "actors\\mo.actor",
        b"ACTOR { NAME=\"mo\" X=10 Y=24 STAND { SPRITESET { LEFT=\"art\\green.png\" } } }".to_vec(),
    );
    pkg.add_file(
        "edit.window",
        b"WINDOW { NAME=\"w\" X=0 Y=0 WIDTH=60 HEIGHT=40 EDIT { NAME=\"name\" X=1 Y=1 WIDTH=50 HEIGHT=10 } }".to_vec(),
    );
    pkg.add_file(
        "tt.font",
        b"TTFONT { SIZE=8 FILENAME=\"none.ttf\" COLOR {255,255,255} ALPHA=255 }".to_vec(),
    );
    pkg.add_file(
        "tt.window",
        b"WINDOW { NAME=\"t\" X=0 Y=0 WIDTH=60 HEIGHT=40 FONT=\"tt.font\" EDIT { NAME=\"name\" X=2 Y=2 WIDTH=56 HEIGHT=14 FONT=\"tt.font\" CURSOR_BLINK_RATE=0 } }".to_vec(),
    );
    pkg.add_file(
        "bm.font",
        b"FONT { IMAGE=\"art\\red.png\" COLUMNS=4 TILE_WIDTH=1 TILE_HEIGHT=1 DEFAULT_WIDTH=1 }"
            .to_vec(),
    );
    pkg.add_file(
        "wide.font",
        b"FONT { IMAGE=\"art\\green.png\" COLUMNS=4 TILE_WIDTH=2 TILE_HEIGHT=2 DEFAULT_WIDTH=2 }"
            .to_vec(),
    );
    pkg.add_file(
        "sel.window",
        b"WINDOW { NAME=\"s\" X=0 Y=0 WIDTH=60 HEIGHT=40
          EDIT { NAME=\"name\" X=5 Y=3 WIDTH=20 HEIGHT=6 FONT=\"bm.font\" FONT_SELECTED=\"wide.font\" FRAME_WIDTH=2 TEXT=\"abcdefgh\" }
          EDIT { NAME=\"greeting\" X=5 Y=20 WIDTH=20 HEIGHT=6 FONT=\"bm.font\" TEXT=\"/GREET/Hello\" } }".to_vec(),
    );
    pkg.add_file(
        "tint.window",
        b"WINDOW { NAME=\"tint\" X=0 Y=0 WIDTH=60 HEIGHT=40 ALPHA_COLOR { 10,20,30 } ALPHA=128
          STATIC { NAME=\"bm\" X=0 Y=0 WIDTH=50 HEIGHT=4 FONT=\"bm.font\" TEXT=\"ab\" }
          STATIC { NAME=\"tt\" X=0 Y=10 WIDTH=50 HEIGHT=20 FONT=\"tt.font\" TEXT=\"cd\" } }"
            .to_vec(),
    );
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

#[test]
fn suspended_rendering_is_a_flag_the_front_end_reads() {
    let (dir, mut runtime, _) = start();
    let game = runtime.world.game_id();
    assert!(!runtime.world.suspended_rendering());
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Check", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "checked"), Value::Bool(true));
    assert_eq!(result(&runtime, "suspendedBefore"), Value::Bool(false));
    assert_eq!(result(&runtime, "suspendedAfter"), Value::Bool(true));
    // The game runs on while suspended: the player skips presenting.
    assert!(runtime.world.suspended_rendering());
    let timer = runtime.world.timer();
    runtime.tick(16);
    assert!(runtime.world.timer() > timer);
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Resume", false);
    runtime.tick(16);
    assert!(!runtime.world.suspended_rendering());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn right_to_left_text_is_a_flag_the_fonts_and_editors_follow() {
    let (dir, mut runtime, _) = start();
    assert_eq!(result(&runtime, "rtlBefore"), Value::Bool(false));
    assert_eq!(result(&runtime, "rtlSet"), Value::Bool(true));
    // A string table's `@right-to-left` turns it on
    // (`CBStringTable::AddString`); the entry is not a string.
    assert_eq!(result(&runtime, "rtlTable"), Value::Bool(true));
    assert_eq!(result(&runtime, "rtlGreet"), Value::from("Shalom"));
    assert!(!runtime.world.text_rtl(), "the script turned it off again");

    // An editor mirrors its keys with right-to-left text
    // (`CUIEdit::HandleKeypress`).
    const VK_BACK: i32 = 8;
    const VK_END: i32 = 35;
    const VK_HOME: i32 = 36;
    const VK_DELETE: i32 = 46;
    let game = runtime.world.game_id();
    let window = call_native(
        &mut runtime,
        game,
        "LoadWindow",
        &[Value::from("edit.window")],
    )
    .as_native()
    .unwrap();
    let edit = call_native(&mut runtime, window, "GetWidget", &[Value::from("name")])
        .as_native()
        .unwrap();
    let keys = [
        (65, Some('a')),
        (66, Some('b')),
        (67, Some('c')),
        (VK_BACK, None),
        (VK_END, None),
        (VK_HOME, None),
        (VK_DELETE, None),
        (68, Some('d')),
    ];
    for (code, text) in keys {
        runtime.world.edit_key(edit, code, text);
    }
    assert_eq!(runtime.world.control(edit).unwrap().text, "db");
    {
        use luma_script::Natives;
        runtime.world.set_property(edit, "Text", &Value::from(""));
    }
    runtime.world.set_text_rtl(true);
    for (code, text) in keys {
        runtime.world.edit_key(edit, code, text);
    }
    // Typed backwards in memory ("cba"), Backspace took the character
    // after the caret, Home went to the end, Delete took the one before
    // it, and "d" went in before the caret.
    assert_eq!(runtime.world.control(edit).unwrap().text, "bd");
    std::fs::remove_dir_all(&dir).ok();
}

/// A string table with a byte order mark makes the game's text UTF-8
/// (`CBStringTable::LoadFile`): the text stays as the file holds it and
/// is read as UTF-8 wherever characters matter.
#[test]
fn a_utf8_string_table_switches_the_text_to_utf8() {
    use luma_script::Natives;
    use luma_script::classes::text_to_utf8;
    let (dir, mut runtime, _) = start();
    assert!(runtime.world.load_string_table("utf8.tab", true));
    assert!(runtime.world.text_utf8());
    assert!(runtime.world.fonts().utf8());
    let game = runtime.world.game_id();
    assert_eq!(
        runtime.world.get_property(game, "TextEncoding"),
        Some(Value::Int(1))
    );

    // The table's text, expanded from a reference, as characters.
    let raw = runtime.world.strings().expand("/GREET/Hello");
    assert_eq!(raw, text_to_utf8("é č Ж Ω"));
    assert_eq!(runtime.world.display_text(&raw), "é č Ж Ω");
    assert_eq!(
        runtime
            .world
            .display_text(&runtime.world.strings().expand("/TWO/x")),
        "un\ndeux"
    );
    // A script counts the characters, not the bytes.
    assert_eq!(
        Value::from(raw.clone()).get_property("Length", &runtime.world),
        Some(Value::Int(7))
    );

    // An editor holds characters: the caret and the keys move by whole
    // characters, and scripts read and write its text as UTF-8
    // (`CUIEdit::ScGetProperty`, `ScSetProperty`).
    const VK_BACK: i32 = 8;
    const VK_LEFT: i32 = 37;
    const VK_DELETE: i32 = 46;
    let window = call_native(
        &mut runtime,
        game,
        "LoadWindow",
        &[Value::from("edit.window")],
    )
    .as_native()
    .unwrap();
    let edit = call_native(&mut runtime, window, "GetWidget", &[Value::from("name")])
        .as_native()
        .unwrap();
    for (code, text) in [
        (0, Some('é')),
        (0, Some('Ж')),
        (0, Some('Ω')),
        (0, Some('z')),
        (VK_LEFT, None),
        (VK_LEFT, None),
        (VK_BACK, None),
        (VK_LEFT, None),
        (VK_DELETE, None),
        (0, Some('č')),
    ] {
        runtime.world.edit_key(edit, code, text);
        let control = runtime.world.control(edit).unwrap();
        let caret = control.edit().unwrap().sel_end;
        assert!(
            control.text.is_char_boundary(caret),
            "{:?} at {caret}",
            control.text
        );
    }
    assert_eq!(runtime.world.control(edit).unwrap().text, "čΩz");
    assert_eq!(
        runtime.world.get_property(edit, "Text"),
        Some(Value::from(text_to_utf8("čΩz")))
    );
    runtime
        .world
        .set_property(edit, "Text", &Value::from(text_to_utf8("Łódź")));
    assert_eq!(runtime.world.control(edit).unwrap().text, "Łódź");
    // The frame lays the editor out without a panic, text that is not
    // UTF-8 included.
    runtime.tick(16);
    runtime
        .world
        .set_property(edit, "Text", &Value::from("caf\u{e9}"));
    runtime.tick(16);

    // A table without the mark turns ANSI back on.
    assert!(runtime.world.load_string_table("ansi.tab", true));
    assert!(!runtime.world.text_utf8());
    assert!(!runtime.world.fonts().utf8());
    let raw = runtime.world.strings().expand("/GREET/x");
    assert_eq!(runtime.world.display_text(&raw), "café");
    assert_eq!(
        runtime.world.get_property(edit, "Text"),
        Some(Value::from("caf\u{fffd}"))
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// An editor draws its line without a box (a width of -1): a TrueType
/// font renders it as wide as the text, not into a one-pixel image.
#[test]
fn an_editor_with_a_truetype_font_shows_what_is_typed() {
    use luma_adventure::Font;
    use luma_formats::font::TextAlign;
    let (dir, mut runtime, _) = start();
    let game = runtime.world.game_id();
    let window = call_native(
        &mut runtime,
        game,
        "LoadWindow",
        &[Value::from("tt.window")],
    )
    .as_native()
    .unwrap();
    let edit = call_native(&mut runtime, window, "GetWidget", &[Value::from("name")])
        .as_native()
        .unwrap();
    for ch in "Lab axe".chars() {
        runtime.world.edit_key(edit, 0, Some(ch));
    }
    // The caret goes inside the text: the text before it, the caret and
    // the text after it are prepared, each in the room left to it.
    runtime.world.edit_key(edit, 37, None);
    runtime.world.focus_control(edit);
    runtime.tick(16);
    let Some(Font::TrueType(font)) = runtime.world.fonts().get("tt.font") else {
        eprintln!("skipped: no system font to stand for the game's");
        std::fs::remove_dir_all(&dir).ok();
        return;
    };
    let state = runtime.world.control(edit).unwrap().edit().unwrap().clone();
    let texts: Vec<&str> = state.pieces.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, ["Lab ax", "e"]);
    let caret = state.caret.clone().expect("a focused editor has a caret");
    assert_eq!(caret.text, state.cursor_char);
    assert!(state.line_height >= 8);
    for piece in state.pieces.iter().chain(&state.caret) {
        let (width, height) = font
            .prepared_size(&piece.text, piece.width, TextAlign::Left, state.line_height)
            .unwrap_or_else(|| panic!("{piece:?} is not prepared"));
        assert_eq!(width as i32, piece.width, "as wide as the room left");
        assert!(height >= 8, "{piece:?}: {height}");
    }
    // The parts follow one another, and none passes the right frame.
    assert!(state.pieces[0].x < caret.x && caret.x < state.pieces[1].x);
    assert!(state.pieces.iter().all(|p| p.x + p.width == 56));

    // Drawn, each part is a quad of that size (needs a GPU for the
    // text's texture).
    if let Ok(gpu) = luma_render::Gpu::headless() {
        let renderer = luma_render::Renderer::new(gpu);
        runtime.world.upload(&renderer);
        let mut list = luma_render::DrawList::new(64.0, 48.0);
        runtime.draw(&mut list, None);
        for piece in &state.pieces {
            assert!(
                list.quads()
                    .iter()
                    .any(|q| q.dst.x == (2 + piece.x) as f32 && q.dst.w == piece.width as f32),
                "{piece:?} is drawn"
            );
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// The parts of the editor `sel.window` shows, as (text, x, width,
/// selected), and its caret as (text, x).
type Parts = (Vec<(String, i32, i32, bool)>, Option<(String, i32)>);

fn parts(runtime: &Runtime, edit: luma_script::NativeId) -> Parts {
    let state = runtime.world.control(edit).unwrap().edit().unwrap();
    (
        state
            .pieces
            .iter()
            .map(|p| (p.text.clone(), p.x, p.width, p.selected))
            .collect(),
        state.caret.as_ref().map(|p| (p.text.clone(), p.x)),
    )
}

fn part(text: &str, x: i32, width: i32, selected: bool) -> (String, i32, i32, bool) {
    (text.to_string(), x, width, selected)
}

/// `CUIEdit::Display`: the text before the selection, the selection in
/// `FONT_SELECTED` and the text after it, each cut at the right frame;
/// the caret where the selection ends, keeping its room while it blinks.
#[test]
fn an_editor_draws_its_selection_its_caret_and_stops_at_the_right_frame() {
    use luma_script::Natives;
    let (dir, mut runtime, _) = start();
    assert!(runtime.world.load_string_table("ansi.tab", true));
    let game = runtime.world.game_id();
    let window = call_native(
        &mut runtime,
        game,
        "LoadWindow",
        &[Value::from("sel.window")],
    )
    .as_native()
    .unwrap();
    let widget = |runtime: &mut Runtime, name: &str| {
        call_native(runtime, window, "GetWidget", &[Value::from(name)])
            .as_native()
            .unwrap()
    };
    let edit = widget(&mut runtime, "name");
    let set = |runtime: &mut Runtime, name: &str, value: Value| {
        assert!(runtime.world.set_property(edit, name, &value));
    };
    let get = |runtime: &Runtime, name: &str| runtime.world.get_property(edit, name).unwrap();

    // `TEXT` goes through the string table.
    let greeting = widget(&mut runtime, "greeting");
    assert_eq!(runtime.world.control(greeting).unwrap().text, "café");

    // Before the editor is drawn its selection is where the constructor
    // put it; drawing brings it inside the text.
    assert_eq!(get(&runtime, "SelStart"), Value::Int(10000));
    assert_eq!(get(&runtime, "CursorBlinkRate"), Value::Int(530));
    runtime.world.focus_control(greeting);
    runtime.tick(16);
    assert_eq!(get(&runtime, "SelStart"), Value::Int(8));
    assert_eq!(get(&runtime, "SelEnd"), Value::Int(8));
    // The font is 1 pixel a character, the frame 2: the text starts at
    // 2 and has the room up to 18. Without the focus there is no caret.
    assert_eq!(
        parts(&runtime, edit),
        (vec![part("abcdefgh", 2, 16, false)], None)
    );
    let state = runtime.world.control(edit).unwrap().edit().unwrap();
    assert_eq!(state.line_height, 2, "the taller of the two fonts");

    // With the focus, the caret stands after the text. It is hidden by
    // the first blink and keeps its room.
    runtime.world.focus_control(edit);
    runtime.tick(16);
    let focused = (
        vec![part("abcdefgh", 2, 16, false)],
        Some(("|".to_string(), 10)),
    );
    assert_eq!(parts(&runtime, edit), focused);
    let visible = |runtime: &Runtime| {
        runtime
            .world
            .control(edit)
            .unwrap()
            .edit()
            .unwrap()
            .cursor_visible
    };
    assert!(!visible(&runtime), "the first blink hides it");
    runtime.tick(500);
    assert!(!visible(&runtime));
    runtime.tick(40);
    assert!(visible(&runtime), "530 ms later it shows");
    assert_eq!(parts(&runtime, edit), focused);

    // A selection made from left to right: the caret after it. The
    // selected font is 2 pixels a character.
    set(&mut runtime, "SelStart", Value::Int(2));
    set(&mut runtime, "SelEnd", Value::Int(5));
    runtime.tick(16);
    assert_eq!(
        parts(&runtime, edit),
        (
            vec![
                part("ab", 2, 16, false),
                part("cde", 4, 14, true),
                part("fgh", 11, 7, false)
            ],
            Some(("|".to_string(), 10))
        )
    );
    // From right to left: the caret before it.
    set(&mut runtime, "SelStart", Value::Int(5));
    set(&mut runtime, "SelEnd", Value::Int(2));
    runtime.tick(16);
    assert_eq!(
        parts(&runtime, edit),
        (
            vec![
                part("ab", 2, 16, false),
                part("cde", 5, 13, true),
                part("fgh", 11, 7, false)
            ],
            Some(("|".to_string(), 4))
        )
    );

    // A text longer than the editor, the caret at its start: the text
    // is given the room up to the right frame and no more.
    let long = "abcdefghijklmnopqrstuvwxyz";
    set(&mut runtime, "Text", Value::from(long));
    set(&mut runtime, "SelStart", Value::Int(0));
    set(&mut runtime, "SelEnd", Value::Int(0));
    runtime.tick(16);
    assert_eq!(
        parts(&runtime, edit),
        (vec![part(long, 3, 15, false)], Some(("|".to_string(), 2)))
    );
    if let Ok(gpu) = luma_render::Gpu::headless() {
        let renderer = luma_render::Renderer::new(gpu);
        runtime.world.upload(&renderer);
        let mut list = luma_render::DrawList::new(64.0, 48.0);
        runtime.draw(&mut list, None);
        // The editor is at (5, 3): its line is at y 5, from x 8.
        let drawn: Vec<f32> = list
            .quads()
            .iter()
            .filter(|q| q.dst.y == 5.0 && q.dst.w == 1.0 && q.dst.x >= 8.0)
            .map(|q| q.dst.x)
            .collect();
        assert_eq!(drawn.len(), 15, "cut at the right frame: {drawn:?}");
        assert!(drawn.iter().all(|x| *x < 23.0));
    }
    // The caret at its end: the first characters leave the view until
    // the text up to the caret fits in the room the caret leaves.
    set(&mut runtime, "SelStart", Value::Int(26));
    set(&mut runtime, "SelEnd", Value::Int(26));
    runtime.tick(16);
    assert_eq!(
        parts(&runtime, edit),
        (
            vec![part(&long[11..], 2, 16, false)],
            Some(("|".to_string(), 17))
        )
    );

    // The caret is the whole string a script gives, measured whole and
    // drawn as its first character.
    set(&mut runtime, "CursorChar", Value::from("<>"));
    assert_eq!(get(&runtime, "CursorChar"), Value::from("<>"));
    runtime.tick(16);
    assert_eq!(
        parts(&runtime, edit),
        (
            vec![part(&long[12..], 2, 16, false)],
            Some(("<".to_string(), 16))
        )
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// `SelStart` and `SelEnd` count characters, as the original's count the
/// bytes of its ANSI text (`CUIEdit::ScGetProperty`, `ScSetProperty`).
#[test]
fn a_script_places_the_selection_by_characters() {
    use luma_script::Natives;
    use luma_script::classes::text_to_utf8;
    let (dir, mut runtime, _) = start();
    assert!(runtime.world.load_string_table("utf8.tab", true));
    let edit = load_edit(&mut runtime);
    let set = |runtime: &mut Runtime, name: &str, value: Value| {
        assert!(runtime.world.set_property(edit, name, &value));
    };
    let get = |runtime: &Runtime, name: &str| runtime.world.get_property(edit, name).unwrap();
    set(&mut runtime, "Text", Value::from(text_to_utf8("éàüö")));
    set(&mut runtime, "SelStart", Value::Int(1));
    set(&mut runtime, "SelEnd", Value::Int(3));
    assert_eq!(get(&runtime, "SelStart"), Value::Int(1));
    assert_eq!(get(&runtime, "SelEnd"), Value::Int(3));
    // The selection is the two characters in between.
    runtime.world.edit_key(edit, 0, Some('x'));
    assert_eq!(runtime.world.control(edit).unwrap().text, "éxö");
    assert_eq!(get(&runtime, "SelEnd"), Value::Int(2));
    // Kept inside the text.
    set(&mut runtime, "SelEnd", Value::Int(99));
    assert_eq!(get(&runtime, "SelEnd"), Value::Int(3));
    set(&mut runtime, "SelStart", Value::Int(-4));
    assert_eq!(get(&runtime, "SelStart"), Value::Int(0));
    // A new text leaves the numbers as they are until the editor is
    // drawn.
    set(&mut runtime, "Text", Value::from("ab"));
    assert_eq!(get(&runtime, "SelEnd"), Value::Int(3));
    runtime.tick(16);
    assert_eq!(get(&runtime, "SelEnd"), Value::Int(2));
    set(&mut runtime, "SelStart", Value::Int(2));
    set(&mut runtime, "Text", Value::from(text_to_utf8("ééééé")));
    assert_eq!(get(&runtime, "SelEnd"), Value::Int(2));
    runtime.world.edit_key(edit, 0, Some('z'));
    assert_eq!(runtime.world.control(edit).unwrap().text, "éézééé");
    std::fs::remove_dir_all(&dir).ok();
}

/// An ANSI game made for another system is read in that system's code
/// page (`CBTextUtils::AnsiToUnicode` uses `CP_ACP`): what is shown and
/// spoken, the code of a typed character, an editor's `Text`.
#[test]
fn an_ansi_game_is_read_in_the_code_page_of_its_system() {
    use luma_adventure::code_page::CodePage;
    use luma_script::Natives;
    let (dir, mut runtime, _) = start();
    assert!(!runtime.world.text_utf8());
    assert_eq!(runtime.world.ansi_code_page(), CodePage::WESTERN);
    let raw = "\u{c6}\u{f3}\u{ea} \u{93}x\u{94}";
    assert_eq!(runtime.world.display_text(raw), "Æóê “x”");
    runtime.world.set_ansi_code_page(CodePage::new(1251));
    assert_eq!(runtime.world.display_text(raw), "Жук “x”");

    let edit = load_edit(&mut runtime);
    runtime.key_char('ж', false, false, false);
    assert_eq!(runtime.world.keyboard().key_code, 230);
    runtime.world.edit_key(edit, 0, Some('ж'));
    // The editor keeps the character; a script reads the game's bytes.
    assert_eq!(runtime.world.control(edit).unwrap().text, "ж");
    assert_eq!(
        runtime.world.get_property(edit, "Text"),
        Some(Value::from("\u{e6}"))
    );
    runtime
        .world
        .set_property(edit, "Text", &Value::from("\u{c6}\u{f3}\u{ea}"));
    assert_eq!(runtime.world.control(edit).unwrap().text, "Жук");
    runtime.tick(16);
    std::fs::remove_dir_all(&dir).ok();
}

/// A window's `ALPHA_COLOR` and `ALPHA` reach the texts drawn in it
/// (`m_ForceAlphaColor`): a bitmap font's characters take the colour, a
/// TrueType font keeps its layers' colours and takes the opacity.
#[test]
fn a_windows_alpha_colour_reaches_its_texts() {
    use luma_formats::Rgba;
    let (dir, mut runtime, _) = start();
    let game = runtime.world.game_id();
    call_native(
        &mut runtime,
        game,
        "LoadWindow",
        &[Value::from("tint.window")],
    );
    runtime.tick(16);
    let Ok(gpu) = luma_render::Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        std::fs::remove_dir_all(&dir).ok();
        return;
    };
    let renderer = luma_render::Renderer::new(gpu);
    runtime.world.upload(&renderer);
    let mut list = luma_render::DrawList::new(64.0, 48.0);
    runtime.draw(&mut list, None);
    let forced = Rgba {
        r: 10,
        g: 20,
        b: 30,
        a: 128,
    };
    let tinted = list.quads().iter().filter(|q| q.tint == forced).count();
    assert_eq!(tinted, 2, "the two characters of the bitmap font");
    if runtime.world.fonts().get("tt.font").is_some() {
        let layer = Rgba {
            r: 255,
            g: 255,
            b: 255,
            a: 128,
        };
        assert!(
            list.quads().iter().any(|q| q.tint == layer),
            "the TrueType text keeps its colour and takes the opacity"
        );
    } else {
        eprintln!("skipped: no system font to stand for the game's");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Loads `edit.window` and returns its editor.
fn load_edit(runtime: &mut Runtime) -> luma_script::NativeId {
    let game = runtime.world.game_id();
    let window = call_native(runtime, game, "LoadWindow", &[Value::from("edit.window")])
        .as_native()
        .unwrap();
    call_native(runtime, window, "GetWidget", &[Value::from("name")])
        .as_native()
        .unwrap()
}

/// A script may put the caret in the middle of a multibyte character
/// (`SelStart`, `SelEnd`); the keys must not panic nor cut a character.
#[test]
fn a_caret_set_inside_a_multibyte_character_never_breaks_the_editor() {
    use luma_script::Natives;
    use luma_script::classes::text_to_utf8;
    for utf8 in [true, false] {
        let (dir, mut runtime, _) = start();
        if utf8 {
            assert!(runtime.world.load_string_table("utf8.tab", true));
        }
        let edit = load_edit(&mut runtime);
        let text = if utf8 {
            text_to_utf8("a\u{e9}\u{e9}")
        } else {
            "a\u{e9}\u{e9}".to_string()
        };
        // Keys: Backspace, Delete, Left, Right, Home, End, a typed character.
        let keys: [(i32, Option<char>); 7] = [
            (8, None),
            (46, None),
            (37, None),
            (39, None),
            (36, None),
            (35, None),
            (0, Some('\u{e9}')),
        ];
        for start_at in 0..=8 {
            for end_at in [start_at, 0, 1, 2, 3, 5] {
                for key in keys {
                    runtime
                        .world
                        .set_property(edit, "Text", &Value::from(text.clone()));
                    runtime
                        .world
                        .set_property(edit, "SelStart", &Value::Int(start_at));
                    runtime
                        .world
                        .set_property(edit, "SelEnd", &Value::Int(end_at));
                    runtime.world.edit_key(edit, key.0, key.1);
                    let control = runtime.world.control(edit).unwrap();
                    let e = control.edit().unwrap();
                    assert!(
                        control.text.is_char_boundary(e.sel_start)
                            && control.text.is_char_boundary(e.sel_end),
                        "utf8={utf8} {start_at}/{end_at} key {key:?}: {:?} {}..{}",
                        control.text,
                        e.sel_start,
                        e.sel_end
                    );
                    runtime.tick(16);
                }
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// A shorter `Text` leaves the caret between two characters of it.
#[test]
fn a_shorter_text_keeps_the_caret_between_two_characters() {
    use luma_script::Natives;
    let (dir, mut runtime, _) = start();
    assert!(runtime.world.load_string_table("utf8.tab", true));
    let edit = load_edit(&mut runtime);
    for (long, short) in [
        ("abcdef", "\u{e9}\u{e9}"),
        ("abcdef", "a\u{e9}"),
        ("\u{e9}\u{e9}\u{e9}", "\u{e9}"),
    ] {
        for caret in 0..=long.len() as i32 + 2 {
            runtime.world.set_property(edit, "Text", &Value::from(long));
            runtime
                .world
                .set_property(edit, "SelStart", &Value::Int(caret));
            runtime
                .world
                .set_property(edit, "SelEnd", &Value::Int(caret));
            runtime
                .world
                .set_property(edit, "Text", &Value::from(short));
            // The keys and the next frame bring it inside the text.
            runtime.world.edit_key(edit, 8, None);
            runtime.world.edit_key(edit, 0, Some('z'));
            runtime.tick(16);
            let control = runtime.world.control(edit).unwrap();
            let e = control.edit().unwrap();
            assert!(e.sel_start <= control.text.len() && e.sel_end <= control.text.len());
            assert!(
                control.text.is_char_boundary(e.sel_start),
                "{short:?} {caret}"
            );
            assert!(
                control.text.is_char_boundary(e.sel_end),
                "{short:?} {caret}"
            );
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// A text of a hundred thousand lines is rendered to an image no taller
/// than 4096 pixels.
#[test]
fn a_very_long_truetype_text_renders_to_a_bounded_image() {
    use luma_adventure::Font;
    use luma_formats::font::TextAlign;
    let (dir, mut runtime, _) = start();
    let game = runtime.world.game_id();
    call_native(
        &mut runtime,
        game,
        "LoadWindow",
        &[Value::from("tt.window")],
    );
    let Some(Font::TrueType(font)) = runtime.world.fonts_mut().get_mut("tt.font") else {
        eprintln!("skipped: no system font to stand for the game's");
        std::fs::remove_dir_all(&dir).ok();
        return;
    };
    let text = "line\n".repeat(100_000);
    font.prepare(&text, -1, TextAlign::Left, -1);
    let (_, height) = font
        .prepared_size(&text, -1, TextAlign::Left, -1)
        .expect("prepared");
    assert!((1..=4096).contains(&height), "height {height}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn new_actor_and_new_entity_make_objects_of_the_game() {
    let (dir, mut runtime, log) = start();
    let text = |key: &str| result(&runtime, key).get_string(&());
    assert_eq!(text("actorType"), "actor");
    assert_eq!(result(&runtime, "actorValid"), Value::Bool(true));
    assert_eq!(result(&runtime, "actorX"), Value::Int(12));
    assert_eq!(text("entityType"), "entity");
    assert_eq!(result(&runtime, "entityValid"), Value::Bool(true));
    assert_eq!(result(&runtime, "entitySpriteSet"), Value::Bool(true));
    assert_eq!(text("entitySprite"), "art\\red.png");
    // A file argument loads the file (a Luma extension); one that does
    // not load leaves an empty object.
    assert_eq!(text("fileEntityName"), "free");
    assert_eq!(result(&runtime, "fileEntityX"), Value::Int(3));
    assert_eq!(text("fileActorName"), "mo");
    assert_eq!(text("missingType"), "entity");
    assert!(
        log.iter().any(|l| l.contains("nowhere.entity")),
        "the failed load is logged: {log:?}"
    );
    assert_eq!(result(&runtime, "actorGone"), Value::Bool(false));
    // The objects are the game's: they outlive the scene change and
    // live among the game's own entities and actors.
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Check", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "entityKept"), Value::Bool(true));
    assert_eq!(result(&runtime, "fileEntityKept"), Value::Bool(true));
    assert_eq!(result(&runtime, "fileActorKept"), Value::Bool(true));
    let game_entities = runtime
        .world
        .objects()
        .iter()
        .filter(|(_, n)| matches!(n.kind, Kind::Entity(EntityRef::Game(_))))
        .count();
    assert_eq!(game_entities, 3, "e, f and the one that did not load");
    let actors = runtime
        .world
        .objects()
        .iter()
        .filter(|(_, n)| n.kind == Kind::Actor)
        .count();
    assert_eq!(actors, 1, "m; a was unloaded");
    // The game runs on and draws with them (the empty actor has nothing
    // to show), and a saved game keeps them.
    for _ in 0..3 {
        runtime.tick(16);
        let mut list = luma_render::DrawList::new(64.0, 48.0);
        runtime.draw(&mut list, None);
    }
    let slot = runtime.save_game(1, "with objects", false).unwrap();
    assert!(slot.exists());
    runtime.load_game(1).unwrap();
    runtime.tick(16);
    assert_eq!(result(&runtime, "fileActorKept"), Value::Bool(true));
    let actors = runtime
        .world
        .objects()
        .iter()
        .filter(|(_, n)| n.kind == Kind::Actor)
        .count();
    assert_eq!(actors, 1);
    std::fs::remove_dir_all(&dir).ok();
}
