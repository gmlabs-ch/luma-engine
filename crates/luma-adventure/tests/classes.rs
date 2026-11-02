//! The script library classes in a game: `new` creates them through the
//! world, files read from the packages and write under the save
//! directory, and the objects survive a saved game.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Kind, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_script::asm::Assembler;
use luma_script::classes::ClassObject;
use luma_script::{Instruction, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-classes-game-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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

/// `var name = new Class(args);`, as the compiler emits `new`.
fn var_new(a: &mut Assembler, name: &str, class: &str, args: &[Arg]) {
    a.def_var(name);
    a.emit(Instruction::CreateObject);
    a.emit(Instruction::PushThisFromStack);
    for arg in args.iter().rev() {
        push_arg(a, arg);
    }
    a.push_int(args.len() as i32);
    a.external_call(class);
    a.emit(Instruction::PopThis);
    a.emit(Instruction::PopEmpty);
    a.pop_var(name);
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

/// `result.key = <top of stack>`.
fn store(a: &mut Assembler, key: &str) {
    a.push_var_ref("result");
    a.push_string(key);
    a.emit(Instruction::PopByExp);
}

/// `result.key = object.Prop`.
fn get_store(a: &mut Assembler, object: &str, prop: &str, key: &str) {
    a.push_var_ref(object);
    a.push_string(prop);
    a.emit(Instruction::PushByExp);
    store(a, key);
}

fn game_script() -> Vec<u8> {
    let mut a = Assembler::new("game.script");
    a.def_glob_var("result");
    a.emit(Instruction::CreateObject);
    a.pop_var("result");
    var_new(&mut a, "s", "String", &[Arg::Str("a,b")]);
    get_store(&mut a, "s", "Length", "len");
    call(&mut a, "s", "Substring", &[Arg::Int(0), Arg::Int(0)]);
    store(&mut a, "first");
    a.def_var("parts");
    call(&mut a, "s", "Split", &[]);
    a.pop_var("parts");
    get_store(&mut a, "parts", "Length", "parts");
    var_new(
        &mut a,
        "d",
        "Date",
        &[Arg::Int(2001), Arg::Int(2), Arg::Int(3)],
    );
    call(&mut a, "d", "GetWeekday", &[]);
    store(&mut a, "weekday");
    var_new(&mut a, "m", "MemBuffer", &[Arg::Int(4)]);
    call(&mut a, "m", "SetInt", &[Arg::Int(0), Arg::Int(42)]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "m", "GetInt", &[Arg::Int(0)]);
    store(&mut a, "mem");
    // A file of the game's packages.
    var_new(&mut a, "r", "File", &[Arg::Str("data\\readme.txt")]);
    call(&mut a, "r", "OpenAsText", &[Arg::Int(1)]);
    store(&mut a, "ropen");
    call(&mut a, "r", "ReadLine", &[]);
    store(&mut a, "line");
    // A file written, then read back.
    var_new(&mut a, "w", "File", &[Arg::Str("notes\\out.txt")]);
    call(&mut a, "w", "OpenAsText", &[Arg::Int(2)]);
    store(&mut a, "wopen");
    call(&mut a, "w", "WriteLine", &[Arg::Str("saved")]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "w", "Close", &[]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "w", "OpenAsText", &[Arg::Int(1)]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "w", "ReadLine", &[]);
    store(&mut a, "back");
    call(&mut a, "w", "Close", &[]);
    a.emit(Instruction::PopEmpty);
    // Conversions go through the world.
    a.push_string("");
    a.push_var("s");
    a.emit(Instruction::Add);
    store(&mut a, "text");
    a.push_var("s");
    a.push_string("a,b");
    a.emit(Instruction::CmpEq);
    store(&mut a, "eq");
    a.push_var("m");
    a.emit(Instruction::Not);
    store(&mut a, "notBuffer");
    a.emit(Instruction::Ret);
    // on "Check": the objects after a load.
    a.event("Check");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    get_store(&mut a, "s", "Length", "afterLen");
    get_store(&mut a, "parts", "Length", "afterParts");
    call(&mut a, "r", "ReadLine", &[]);
    store(&mut a, "afterLine");
    get_store(&mut a, "w", "AccessMode", "afterMode");
    call(&mut a, "m", "GetInt", &[Arg::Int(0)]);
    store(&mut a, "afterMem");
    call(&mut a, "d", "GetYear", &[]);
    store(&mut a, "afterYear");
    a.emit(Instruction::RetEvent);
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
        b"GAME { NAME=\"Classes\" SCRIPT=\"game.script\" }".to_vec(),
    );
    pkg.add_file("game.script", game_script());
    pkg.add_file("data\\readme.txt", b"hello\r\nworld\r\n".to_vec());
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

fn result(runtime: &Runtime, key: &str) -> Value {
    runtime
        .engine
        .global("result")
        .and_then(|r| r.properties())
        .and_then(|p| p.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn scripts_use_the_library_classes() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let log = runtime.tick(16);
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
    assert_eq!(result(&runtime, "len"), Value::Int(3));
    assert_eq!(result(&runtime, "first"), Value::from("a"));
    assert_eq!(result(&runtime, "parts"), Value::Int(2));
    assert_eq!(result(&runtime, "weekday"), Value::Int(6), "a Saturday");
    assert_eq!(result(&runtime, "mem"), Value::Int(42));
    assert_eq!(result(&runtime, "ropen"), Value::Bool(true));
    assert_eq!(result(&runtime, "line"), Value::from("hello"));
    assert_eq!(result(&runtime, "wopen"), Value::Bool(true));
    assert_eq!(result(&runtime, "back"), Value::from("saved"));
    assert_eq!(result(&runtime, "text"), Value::from("a,b"));
    assert_eq!(result(&runtime, "eq"), Value::Bool(true));
    assert_eq!(
        result(&runtime, "notBuffer"),
        Value::Bool(true),
        "a library object converts to false"
    );
    let written = runtime.world.save_directory().join("notes/out.txt");
    assert_eq!(std::fs::read(&written).unwrap(), b"saved\n");
    let classes = runtime
        .world
        .objects()
        .iter()
        .filter(|(_, n)| matches!(n.kind, Kind::Class(_)))
        .count();
    assert_eq!(
        classes, 7,
        "s, parts (the split array), d, m, r, w, and the Directory global"
    );

    // The objects are saved with the game and work after a load.
    runtime.save_game(5, "classes", false).unwrap();
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(5).unwrap();
    loaded.tick(16);
    let game = loaded.engine.global("Game").unwrap().as_native().unwrap();
    loaded
        .engine
        .apply_event(&mut loaded.world, game, "Check", false);
    let log = loaded.tick(16);
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
    assert_eq!(result(&loaded, "afterLen"), Value::Int(3));
    assert_eq!(result(&loaded, "afterParts"), Value::Int(2));
    assert_eq!(
        result(&loaded, "afterLine"),
        Value::from("world"),
        "the read position is kept"
    );
    assert_eq!(result(&loaded, "afterMode"), Value::Int(0));
    assert_eq!(result(&loaded, "afterMem"), Value::Int(42));
    assert_eq!(result(&loaded, "afterYear"), Value::Int(2001));
    let strings = loaded
        .world
        .objects()
        .iter()
        .filter(|(_, n)| matches!(&n.kind, Kind::Class(ClassObject::String(_))))
        .count();
    assert_eq!(strings, 1);
    std::fs::remove_dir_all(&dir).ok();
}
