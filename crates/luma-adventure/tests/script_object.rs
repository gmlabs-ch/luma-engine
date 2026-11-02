//! `new Object(...)` in a game: a bare native object, its scripts attached
//! from the constructor's arguments, its methods and events answered by
//! them, and `Game.UnloadObject` ending it.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Kind, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_script::Value;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-object-game-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const GAME_SCRIPT: &str = r#"
global result = new Object();
global o = new Object("obj.script");
o.x = 5;
result.type = o.Type;
result.unnamed = o.Name;
o.Name = "thing";
result.name = o.Name;
result.valid = Game.ValidObject(o);
result.text = "" + o;
result.running = o.IsScriptRunning("obj.script");
result.ping = o.Ping();
o.ApplyEvent("Hello");
var plain = new Object();
plain.y = 2;
result.plainType = plain.Type;
result.plainY = plain.y;
result.plainValid = Game.ValidObject(plain);
"#;

const OBJECT_SCRIPT: &str = r#"
global result;
method Ping()
{
  return this.x + 1;
}
on "Hello"
{
  result.hello = self.x;
  result.helloName = self.Name;
}
"#;

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
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 32,32 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Objects\" SCRIPT=\"game.script\" }".to_vec(),
    );
    pkg.add_file("game.script", compile(GAME_SCRIPT, "game.script"));
    pkg.add_file("obj.script", compile(OBJECT_SCRIPT, "obj.script"));
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

#[test]
fn new_object_attaches_its_scripts() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let mut log = runtime.tick(16);
    log.extend(runtime.tick(16));
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
    assert_eq!(result(&runtime, "type"), Value::from("object"));
    assert_eq!(result(&runtime, "unnamed"), Value::from("<unnamed>"));
    assert_eq!(result(&runtime, "name"), Value::from("thing"));
    assert_eq!(result(&runtime, "valid"), Value::Bool(true));
    assert_eq!(result(&runtime, "text"), Value::from("[object]"));
    assert_eq!(result(&runtime, "running"), Value::Bool(true));
    assert_eq!(result(&runtime, "ping"), Value::Int(6), "a script method");
    assert_eq!(result(&runtime, "hello"), Value::Int(5), "an event handler");
    assert_eq!(result(&runtime, "helloName"), Value::from("thing"));
    assert_eq!(result(&runtime, "plainType"), Value::from("object"));
    assert_eq!(result(&runtime, "plainY"), Value::Int(2));
    assert_eq!(result(&runtime, "plainValid"), Value::Bool(true));
    let objects = runtime
        .world
        .objects()
        .iter()
        .filter(|(_, n)| n.kind == Kind::Object)
        .count();
    assert_eq!(objects, 3, "result, o and plain");

    // Unloading ends the object and its scripts.
    let o = runtime.engine.global("o").unwrap().as_native().unwrap();
    assert_eq!(runtime.engine.scripts_of(o).len(), 1);
    let scripts_before = runtime.engine.scripts_of(o);
    assert!(runtime.world.unload_object(&mut runtime.engine, o));
    runtime.tick(16);
    assert!(!runtime.world.objects().contains(o));
    for script in scripts_before {
        assert!(
            runtime.engine.script(script).is_none()
                || runtime.engine.script(script).unwrap().state() == luma_script::State::Finished
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}
