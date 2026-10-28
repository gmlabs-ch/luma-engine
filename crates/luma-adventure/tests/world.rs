//! A synthetic game whose scripts drive the world: the game script
//! changes scene, the scene script inspects and modifies entities, an
//! entity script handles events.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Kind, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_script::asm::Assembler;
use luma_script::{Instruction, Natives, State, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-world-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An argument of a call in the assembled scripts.
enum Arg {
    Int(i32),
    Str(&'static str),
}

fn push_arg(a: &mut Assembler, arg: &Arg) {
    match arg {
        Arg::Int(i) => a.push_int(*i),
        Arg::Str(s) => a.push_string(s),
    }
}

/// `object.Method(args)`, leaving the result on the stack.
fn call(a: &mut Assembler, object: &str, method: &str, args: &[Arg]) {
    for arg in args.iter().rev() {
        push_arg(a, arg);
    }
    a.push_int(args.len() as i32);
    a.push_var(object);
    a.push_string(method);
    a.emit(Instruction::CallByExp);
}

/// `object.Prop`, pushed.
fn get(a: &mut Assembler, object: &str, prop: &str) {
    a.push_var_ref(object);
    a.push_string(prop);
    a.emit(Instruction::PushByExp);
}

/// `object.Prop = <top of stack>`.
fn set(a: &mut Assembler, object: &str, prop: &str) {
    a.push_var_ref(object);
    a.push_string(prop);
    a.emit(Instruction::PopByExp);
}

/// `result.key = <top of stack>` (`result` is an engine global object).
fn store(a: &mut Assembler, key: &str) {
    set(a, "result", key);
}

fn game_script() -> Vec<u8> {
    let mut a = Assembler::new("game.script");
    a.def_glob_var("result");
    a.emit(Instruction::CreateObject);
    a.pop_var("result");
    call(&mut a, "Game", "ExpandString", &[Arg::Str("/GREET/x")]);
    store(&mut a, "greet");
    get(&mut a, "Game", "ScreenWidth");
    store(&mut a, "width");
    get(&mut a, "Game", "Name");
    store(&mut a, "name");
    get(&mut a, "self", "Name");
    store(&mut a, "selfName");
    // var win = Game.LoadWindow("w.window"); win.Visible = false;
    a.def_var("win");
    call(&mut a, "Game", "LoadWindow", &[Arg::Str("w.window")]);
    a.pop_var("win");
    a.push_bool(false);
    set(&mut a, "win", "Visible");
    get(&mut a, "win", "Visible");
    store(&mut a, "winVisible");
    // Game.Interactive = false; result.interactive = Game.Interactive
    a.push_bool(false);
    set(&mut a, "Game", "Interactive");
    get(&mut a, "Game", "Interactive");
    store(&mut a, "interactive");
    // Scene = Game.Scene (a global the scene scripts use)
    a.def_glob_var("Scene");
    get(&mut a, "Game", "Scene");
    a.pop_var("Scene");
    call(&mut a, "Game", "PlaySound", &[Arg::Str("a.ogg")]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "Game", "PlaySound", &[Arg::Str("b.ogg")]);
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "ChangeScene",
        &[Arg::Str("scenes\\room.scene")],
    );
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Game", "ChangingScene");
    store(&mut a, "changing");
    a.emit(Instruction::Ret);
    a.build()
}

fn scene_init_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room\\scene_init.script");
    a.def_glob_var("result");
    a.def_glob_var("Scene");
    get(&mut a, "Scene", "Name");
    store(&mut a, "scene");
    get(&mut a, "self", "Name");
    store(&mut a, "sceneSelf");
    // var p = Scene.GetNode("PROP"); p.Active = false; ...
    a.def_var("p");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("PROP")]);
    a.pop_var("p");
    get(&mut a, "p", "X");
    store(&mut a, "propX");
    a.push_bool(false);
    set(&mut a, "p", "Active");
    get(&mut a, "p", "Active");
    store(&mut a, "propActive");
    call(&mut a, "p", "SkipTo", &[Arg::Int(5), Arg::Int(6)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "p", "Y");
    store(&mut a, "propY");
    get(&mut a, "p", "Type");
    store(&mut a, "propType");
    // Script properties on natives persist.
    a.push_int(7);
    set(&mut a, "p", "xResult");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("prop")]);
    a.pop_var("p");
    get(&mut a, "p", "xResult");
    store(&mut a, "xResult");
    // var s = p.GetSpriteObject(); result.frames = s.NumFrames
    a.def_var("s");
    call(&mut a, "p", "GetSpriteObject", &[]);
    a.pop_var("s");
    get(&mut a, "s", "NumFrames");
    store(&mut a, "frames");
    get(&mut a, "s", "Finished");
    store(&mut a, "finishedAtStart");
    // Scene.SkipTo(100, 0); result.offset = Scene.OffsetX
    call(&mut a, "Scene", "SkipTo", &[Arg::Int(100), Arg::Int(0)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Scene", "OffsetX");
    store(&mut a, "offset");
    call(
        &mut a,
        "Scene",
        "IsWalkableAt",
        &[Arg::Int(10), Arg::Int(20)],
    );
    store(&mut a, "walkable");
    call(
        &mut a,
        "Scene",
        "IsWalkableAt",
        &[Arg::Int(10), Arg::Int(2)],
    );
    store(&mut a, "walkableAbove");
    // result.region = Scene.GetRegionAt(10, 20).Name
    a.def_var("r");
    call(
        &mut a,
        "Scene",
        "GetRegionAt",
        &[Arg::Int(10), Arg::Int(20)],
    );
    a.pop_var("r");
    get(&mut a, "r", "Name");
    store(&mut a, "region");
    get(&mut a, "r", "Blocked");
    store(&mut a, "regionBlocked");
    // Layers.
    get(&mut a, "Scene", "MainLayer");
    a.pop_var("r");
    get(&mut a, "r", "NumNodes");
    store(&mut a, "nodes");
    // The lamp handles LookAt; raise it.
    a.def_var("lamp");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("lamp")]);
    a.pop_var("lamp");
    call(&mut a, "lamp", "CanHandleEvent", &[Arg::Str("lookat")]);
    store(&mut a, "lampHandles");
    call(&mut a, "lamp", "CanHandleEvent", &[Arg::Str("Take")]);
    store(&mut a, "lampTakes");
    call(&mut a, "lamp", "ApplyEvent", &[Arg::Str("LookAt")]);
    store(&mut a, "applied");
    call(
        &mut a,
        "lamp",
        "IsScriptRunning",
        &[Arg::Str("scenes\\room\\lamp.script")],
    );
    store(&mut a, "lampRunning");
    // A missing node and an undefined method.
    call(&mut a, "Scene", "GetNode", &[Arg::Str("nobody")]);
    store(&mut a, "nobody");
    call(&mut a, "p", "Explode", &[]);
    store(&mut a, "explode");
    get(&mut a, "Game", "PrevScene");
    store(&mut a, "prev");
    a.emit(Instruction::Ret);
    a.build()
}

fn scene_events_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room\\scene_events.script");
    let end = a.label();
    a.jmp(end);
    a.event("SceneShutdown");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.push_bool(true);
    store(&mut a, "shutdown");
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    a.build()
}

fn lamp_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room\\lamp.script");
    let end = a.label();
    a.jmp(end);
    a.event("LookAt");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    get(&mut a, "this", "Name");
    store(&mut a, "looked");
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    a.build()
}

fn game() -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 32,32 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Test\" SCRIPT=\"game.script\" STRING_TABLE=\"string.tab\" } AD_GAME { }"
            .to_vec(),
    );
    pkg.add_file("string.tab", b"GREET\tHello\r\n".to_vec());
    pkg.add_file("game.script", game_script());
    pkg.add_file("scenes\\room\\scene_init.script", scene_init_script());
    pkg.add_file("scenes\\room\\scene_events.script", scene_events_script());
    pkg.add_file("scenes\\room\\lamp.script", lamp_script());
    pkg.add_file(
        "art\\prop.sprite",
        b"SPRITE { LOOPING=FALSE FRAME { DELAY=100 IMAGE=\"art\\red.png\" } FRAME { DELAY=100 IMAGE=\"art\\green.png\" } }".to_vec(),
    );
    pkg.add_file(
        "scenes\\room.scene",
        b"SCENE {
  NAME=\"Room\"
  SCRIPT=\"scenes\\room\\scene_init.script\"
  SCRIPT=\"scenes\\room\\scene_events.script\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=32
    ENTITY { NAME=\"prop\" X=8 Y=12 SPRITE=\"art\\prop.sprite\" }
    REGION { NAME=\"floor\" POINT {0,16} POINT {200,16} POINT {200,32} POINT {0,32} }
    ENTITY { NAME=\"lamp\" X=20 Y=20 SCRIPT=\"scenes\\room\\lamp.script\" }
  }
}"
        .to_vec(),
    );
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

fn result(runtime: &Runtime, key: &str) -> Value {
    runtime
        .engine
        .global("result")
        .and_then(|r| r.get_property(key, &()))
        .unwrap_or(Value::Null)
}

#[test]
fn scripts_drive_the_game_and_its_scene() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::new(vfs).unwrap();
    assert_eq!(runtime.world.strings().get("greet"), Some("Hello"));
    runtime.start();
    assert_eq!(
        runtime.engine.counts().total,
        1,
        "the game script is attached"
    );

    // Tick 1: the game script runs and schedules the scene change.
    let log = runtime.tick(16);
    assert_eq!(result(&runtime, "greet"), Value::from("Hello"));
    assert_eq!(result(&runtime, "width"), Value::Int(32));
    assert_eq!(result(&runtime, "name"), Value::from("Test"));
    assert_eq!(result(&runtime, "selfName"), Value::from("Test"));
    assert_eq!(result(&runtime, "winVisible"), Value::Bool(false));
    assert_eq!(result(&runtime, "interactive"), Value::Bool(false));
    assert!(!runtime.world.is_interactive());
    assert_eq!(result(&runtime, "changing"), Value::Bool(true));
    assert_eq!(runtime.world.scheduled_scene(), Some("scenes\\room.scene"));
    assert!(runtime.world.scene().is_none());
    let stubs: Vec<&String> = log
        .iter()
        .filter(|l| l.contains("not implemented"))
        .collect();
    assert_eq!(
        stubs.len(),
        2,
        "LoadWindow and PlaySound, once each: {log:?}"
    );
    assert!(
        log.iter().all(|l| !l.starts_with("Runtime error")),
        "{log:?}"
    );

    // Tick 2: the scene loads, its scripts run, the lamp handler runs.
    let log = runtime.tick(16);
    assert!(
        log.iter().any(|l| l.contains("scene 'Room' loaded")),
        "{log:?}"
    );
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    assert_eq!(result(&runtime, "scene"), Value::from("Room"));
    assert_eq!(result(&runtime, "sceneSelf"), Value::from("Room"));
    assert_eq!(result(&runtime, "propX"), Value::Int(8));
    assert_eq!(result(&runtime, "propActive"), Value::Bool(false));
    assert_eq!(result(&runtime, "propY"), Value::Int(6));
    assert_eq!(result(&runtime, "propType"), Value::from("entity"));
    assert_eq!(result(&runtime, "xResult"), Value::Int(7));
    assert_eq!(result(&runtime, "frames"), Value::Int(2));
    assert_eq!(result(&runtime, "finishedAtStart"), Value::Bool(false));
    assert_eq!(result(&runtime, "offset"), Value::Int(84), "100 - 32/2");
    assert_eq!(runtime.world.offset(), (84, 0));
    assert_eq!(result(&runtime, "walkable"), Value::Bool(true));
    assert_eq!(result(&runtime, "walkableAbove"), Value::Bool(false));
    assert_eq!(result(&runtime, "region"), Value::from("floor"));
    assert_eq!(result(&runtime, "regionBlocked"), Value::Bool(false));
    assert_eq!(result(&runtime, "nodes"), Value::Int(3));
    assert_eq!(result(&runtime, "lampHandles"), Value::Bool(true));
    assert_eq!(result(&runtime, "lampTakes"), Value::Bool(false));
    assert_eq!(result(&runtime, "applied"), Value::Bool(true));
    assert_eq!(result(&runtime, "lampRunning"), Value::Bool(true));
    assert_eq!(result(&runtime, "looked"), Value::from("lamp"));
    assert_eq!(result(&runtime, "nobody"), Value::Null);
    assert_eq!(result(&runtime, "explode"), Value::Null);
    assert_eq!(result(&runtime, "prev"), Value::from(""));
    assert!(
        log.iter()
            .any(|l| l.contains("Call to undefined method 'Explode'")),
        "{log:?}"
    );
    let errors = log
        .iter()
        .filter(|l| l.starts_with("Runtime error"))
        .count();
    assert_eq!(errors, 1, "{log:?}");

    // The scene's state is what the scripts left: the prop is inactive at
    // (5, 6), the lamp's script and the scene's event script persist.
    let view = runtime.world.scene().unwrap();
    let prop = view
        .entity(luma_adventure::EntityRef::Node { layer: 0, index: 0 })
        .unwrap();
    assert!(!prop.active);
    assert_eq!((prop.x, prop.y), (5, 6));
    let persistent = runtime
        .engine
        .scripts()
        .iter()
        .filter(|s| s.state() == State::Persistent)
        .count();
    assert_eq!(persistent, 2, "scene_events and lamp");
    let lamp = runtime.world.find_node("LAMP").unwrap();
    assert!(matches!(
        runtime.world.objects().get(lamp).unwrap().kind,
        Kind::Entity(_)
    ));
    assert!(runtime.engine.can_handle_event(lamp, "LookAt"));

    // Changing scene again: SceneShutdown runs, the old objects are gone,
    // PrevScene names the old scene.
    runtime.change_scene("scenes\\room.scene").unwrap();
    assert_eq!(result(&runtime, "shutdown"), Value::Bool(true));
    assert!(
        !runtime.world.objects().contains(lamp),
        "old nodes are invalid"
    );
    assert!(runtime.world.objects().contains(runtime.world.scene_id()));
    runtime.tick(16);
    assert_eq!(result(&runtime, "prev"), Value::from("Room"));
    assert_eq!(runtime.world.offset(), (84, 0), "scene_init ran again");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn sprite_objects_follow_the_animation() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::new(vfs).unwrap();
    runtime.change_scene("scenes\\room.scene").unwrap();
    // Let the scene scripts run (they deactivate the prop); reactivate it.
    runtime.tick(1);
    let prop_ref = luma_adventure::EntityRef::Node { layer: 0, index: 0 };
    runtime
        .world
        .scene_mut()
        .unwrap()
        .entity_mut(prop_ref)
        .unwrap()
        .active = true;
    let prop = runtime.world.node_id(prop_ref).unwrap();
    let sprite = runtime.world.sprite_object_id(prop).unwrap();
    let finished = |runtime: &Runtime| {
        runtime
            .world
            .get_property(sprite, "Finished")
            .unwrap_or(Value::Null)
    };
    let frame = |runtime: &Runtime| {
        runtime
            .world
            .get_property(sprite, "CurrentFrame")
            .unwrap_or(Value::Null)
    };
    runtime.tick(1);
    assert_eq!(frame(&runtime), Value::Int(0));
    assert_eq!(finished(&runtime), Value::Bool(false));
    runtime.tick(150);
    assert_eq!(frame(&runtime), Value::Int(1));
    runtime.tick(150);
    assert_eq!(frame(&runtime), Value::Int(1));
    assert_eq!(finished(&runtime), Value::Bool(true));
    // Reset through the sprite object.
    runtime
        .world
        .set_property(sprite, "CurrentFrame", &Value::Int(0));
    assert_eq!(frame(&runtime), Value::Int(0));
    assert_eq!(finished(&runtime), Value::Bool(false));
    assert_eq!(
        runtime.world.get_property(sprite, "Owner"),
        Some(Value::Native(prop))
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn quitting_and_missing_scenes() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::new(vfs).unwrap();
    assert!(!runtime.world.quit_requested());
    assert!(runtime.change_scene("scenes\\nope.scene").is_err());
    runtime.world.schedule_scene("scenes\\nope.scene", true);
    let log = runtime.tick(16);
    assert!(
        log.iter().any(|l| l.contains("cannot change scene")),
        "{log:?}"
    );
    assert!(runtime.world.scene().is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The local test game: its game script runs, then a real scene with
/// its entities and scripts, without a fatal error.
#[test]
#[ignore = "needs the local test game in testdata/local"]
fn runs_the_local_game() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/local/white-chamber");
    if !root.is_dir() {
        eprintln!("skipped: {} is absent", root.display());
        return;
    }
    let vfs = Vfs::open_game(&root).unwrap();
    let mut runtime = Runtime::new(vfs).unwrap();
    runtime.start();
    let mut log = Vec::new();
    for _ in 0..30 {
        log.extend(runtime.tick(16));
    }
    assert!(
        runtime.world.scene().is_some(),
        "the game script picked a scene"
    );
    runtime
        .change_scene("scenes/storeroom/storeroom.scene")
        .unwrap();
    for _ in 0..120 {
        log.extend(runtime.tick(16));
    }
    assert_eq!(runtime.world.scene().unwrap().scene.name, "StoreRoom");
    assert!(log.iter().all(|l| !l.starts_with("Fatal")), "{log:?}");
    let errors: Vec<&String> = log
        .iter()
        .filter(|l| l.starts_with("Runtime error"))
        .collect();
    eprintln!("{} log lines, {} runtime errors", log.len(), errors.len());
    for line in &log {
        eprintln!("{line}");
    }
    let counts = runtime.engine.counts();
    eprintln!("{counts:?}");
}
