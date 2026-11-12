//! The script engine and its classes against the original engine's
//! rules, one behaviour per test: what `Math` computes, what a native
//! object converts to, the clocks of `Sleep`, destroyed objects, scripts
//! shipped as source text, the profiler and the library classes.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_script::Value;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-script-port-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile(source: &str, name: &str) -> Vec<u8> {
    Compiler::new(Natives::wme())
        .compile(source.as_bytes(), name, &mut |_: &[u8]| None)
        .unwrap()
}

/// A file of the test game.
#[allow(dead_code)]
enum File<'a> {
    /// A script, compiled.
    Script(&'a str, &'a str),
    /// Any other file, as it is.
    Raw(&'a str, &'a [u8]),
}

const GAME_FILE: &[u8] = b"GAME { NAME=\"Port\" SCRIPT=\"game.script\" }";

/// A game whose `game.script` is `source`, with the extra files, started
/// and run for two frames. Returns its directory, the runtime and the
/// log of those frames.
fn start_with(
    game_file: &[u8],
    source: &str,
    files: &[File<'_>],
) -> (std::path::PathBuf, Runtime, Vec<String>) {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 64,48 } }".to_vec(),
    );
    pkg.add_file("default.game", game_file.to_vec());
    pkg.add_file("game.script", compile(source, "game.script"));
    for file in files {
        match file {
            File::Script(name, source) => pkg.add_file(*name, compile(source, name)),
            File::Raw(name, bytes) => pkg.add_file(*name, bytes.to_vec()),
        };
    }
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let mut log = runtime.tick(16);
    log.extend(runtime.tick(16));
    (dir, runtime, log)
}

fn start(source: &str, files: &[File<'_>]) -> (std::path::PathBuf, Runtime, Vec<String>) {
    start_with(GAME_FILE, source, files)
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

fn float(runtime: &Runtime, key: &str) -> f64 {
    match result(runtime, key) {
        Value::Float(f) => f,
        other => panic!("{key} is {other:?}, not a float"),
    }
}

fn no_errors(log: &[String]) {
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
}

#[test]
fn math_takes_degrees_and_reads_strings_like_atof() {
    let (dir, runtime, log) = start(
        r#"
global result = new Object();
result.sin90 = Math.Sin(90);
result.cos180 = Math.Cos(180);
result.tan45 = Math.Tan(45);
result.sinh = Math.Sinh(180);
result.cosh = Math.Cosh(0);
result.tanh = Math.Tanh(180);
result.asin = Math.Asin(1);
result.atan2 = Math.Atan2(1, 1);
result.text = Math.Abs("-12abc");
result.pi = Math.PI;
result.rad = Math.DegToRad(180);
result.deg = Math.RadToDeg(1);
result.floor = Math.Floor(2.5);
"#,
        &[],
    );
    no_errors(&log);
    let near = |key: &str, expected: f64| {
        let got = float(&runtime, key);
        assert!((got - expected).abs() < 1e-6, "{key}: {got} != {expected}");
    };
    near("sin90", 1.0);
    near("cos180", -1.0);
    near("tan45", 1.0);
    // The hyperbolic functions convert their argument too.
    near("sinh", std::f64::consts::PI.sinh());
    near("cosh", 1.0);
    near("tanh", std::f64::consts::PI.tanh());
    // The inverse functions answer in radians.
    near("asin", std::f64::consts::FRAC_PI_2);
    near("atan2", std::f64::consts::FRAC_PI_4);
    // `atof`: the leading number of a string.
    assert_eq!(float(&runtime, "text"), 12.0);
    // Single-precision constants, widened.
    assert_eq!(float(&runtime, "pi"), f64::from(std::f32::consts::PI));
    assert_eq!(
        float(&runtime, "rad"),
        180.0 * f64::from(std::f32::consts::PI / 180.0f32)
    );
    assert_eq!(
        float(&runtime, "deg"),
        f64::from(180.0f32 / std::f32::consts::PI)
    );
    assert_eq!(float(&runtime, "floor"), 2.0);
    std::fs::remove_dir_all(&dir).ok();
}

const ROOM: &[u8] = b"SCENE {
  NAME=\"Room\"
  WAYPOINTS { NAME=\"wp\" POINT {1,1} }
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=100
    REGION { NAME=\"reg\" POINT {0,0} POINT {10,0} POINT {10,10} }
    ENTITY { NAME=\"prop\" X=8 Y=12 }
  }
}";

const WINDOW: &[u8] = b"WINDOW { NAME=\"w\" X=0 Y=0 WIDTH=60 HEIGHT=40
  BUTTON { NAME=\"b\" X=1 Y=1 WIDTH=10 HEIGHT=10 }
  EDIT { NAME=\"e\" X=1 Y=12 WIDTH=50 HEIGHT=10 }
  STATIC { NAME=\"s\" X=1 Y=24 WIDTH=50 HEIGHT=10 }
  ENTITY_CONTAINER { NAME=\"c\" X=1 Y=30 WIDTH=5 HEIGHT=5 }
}";

#[test]
fn natives_are_false_and_print_their_class_text() {
    let (dir, mut runtime, mut log) = start_with(
        b"GAME { NAME=\"Port\" SCRIPT=\"game.script\" } AD_GAME { ITEMS=\"items.items\" }",
        r#"
global result = new Object();
var o = new Object();
result.ifObject = false;
if (o) result.ifObject = true;
result.notObject = !o;
result.toBool = ToBool(Game);
result.notNull = (o != null);
result.andObject = (o && true);
result.game = "" + Game;
result.keyboard = "" + Game.Keyboard;
result.math = ToString(Math);
result.directory = ToString(Directory);
result.object = "" + o;
var w = Game.LoadWindow("w.window");
result.window = "" + w;
result.button = "" + w.GetControl("b");
result.edit = "" + w.GetControl("e");
result.static = "" + w.GetControl("s");
result.container = "" + w.GetControl("c");
result.item = "" + Game.QueryItem("key");
Game.ChangeScene("scenes/room.scene");

on "Check"
{
  var scene = Game.Scene;
  result.scene = "" + scene;
  var layer = scene.GetLayer("main");
  result.layer = "" + layer;
  result.region = "" + layer.GetNode("reg");
  var e = layer.GetNode("prop");
  result.entity = "" + e;
  e.CreateRegion();
  result.entityRegion = "" + e.Region;
  result.waypoints = "" + scene.GetWaypointGroup("wp");
  var a = new Actor();
  result.actor = "" + a;
}
"#,
        &[
            File::Raw("scenes\\room.scene", ROOM),
            File::Raw("w.window", WINDOW),
            File::Raw("items.items", b"ITEM { NAME=\"key\" }"),
        ],
    );
    log.extend(runtime.tick(16));
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Check", false);
    log.extend(runtime.tick(16));
    no_errors(&log);
    // `CBScriptable::ScToBool` is false for every class.
    assert_eq!(result(&runtime, "ifObject"), Value::Bool(false));
    assert_eq!(result(&runtime, "notObject"), Value::Bool(true));
    assert_eq!(result(&runtime, "toBool"), Value::Bool(false));
    assert_eq!(result(&runtime, "andObject"), Value::Bool(false));
    assert_eq!(result(&runtime, "notNull"), Value::Bool(true));
    for (key, text) in [
        ("game", "[game object]"),
        ("keyboard", "[keyboard state]"),
        ("math", "[native object]"),
        ("directory", "[native object]"),
        ("object", "[object]"),
        ("window", "[window]"),
        ("button", "[button]"),
        ("edit", "[edit]"),
        ("static", "[static]"),
        ("container", "[entity container]"),
        ("item", "[item]"),
        ("scene", "[scene object]"),
        ("layer", "[layer]"),
        ("region", "[ad region]"),
        ("entity", "[entity object]"),
        ("entityRegion", "[region]"),
        ("waypoints", "[object]"),
        ("actor", "[actor object]"),
    ] {
        assert_eq!(result(&runtime, key), Value::from(text), "{key}: {log:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// A script compiled under another name than the file it is stored as.
fn compile_as(source: &str, recorded: &str) -> Vec<u8> {
    compile(source, recorded)
}

#[test]
fn sleep_runs_on_the_wall_clock_while_the_game_is_frozen() {
    // The script of a system-exclusive window is not freezable: it runs
    // while the game is frozen, and its `Sleep` cannot use the game
    // timer, which is stopped.
    let (dir, mut runtime, mut log) = start(
        r#"
global result = new Object();
global w = Game.LoadWindow("w.window");
w.AttachScript("w.script");
w.GoSystemExclusive();
"#,
        &[
            File::Raw("w.window", WINDOW),
            File::Script(
                "w.script",
                r#"
global result;
on "Nap"
{
  result.frozen = Game.Frozen;
  Sleep(100);
  result.woke = true;
}
"#,
            ),
        ],
    );
    let w = runtime.engine.global("w").unwrap().as_native().unwrap();
    runtime
        .engine
        .apply_event(&mut runtime.world, w, "Nap", false);
    log.extend(runtime.tick(16));
    assert_eq!(result(&runtime, "frozen"), Value::Bool(true), "{log:?}");
    assert_eq!(result(&runtime, "woke"), Value::Null);
    let timer = runtime.world.timer();
    for _ in 0..10 {
        log.extend(runtime.tick(16));
    }
    no_errors(&log);
    assert_eq!(runtime.world.timer(), timer, "the game timer is stopped");
    assert_eq!(
        result(&runtime, "woke"),
        Value::Bool(true),
        "a sleep started while frozen ends on the wall clock"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn scripts_attached_to_a_system_exclusive_window_are_not_freezable() {
    let (dir, mut runtime, log) = start(
        r#"
global result = new Object();
global w = Game.LoadWindow("w.window");
global other = Game.LoadWindow("w.window");
global b = w.GetControl("b");
w.GoSystemExclusive();
"#,
        &[
            File::Raw("w.window", WINDOW),
            File::Script("late.script", "on \"Nothing\" { }"),
        ],
    );
    no_errors(&log);
    // The calling script waits for the window; the scripts are attached
    // behind its back, as a script of the window would.
    for global in ["w", "b", "other"] {
        let id = runtime.engine.global(global).unwrap().as_native().unwrap();
        assert!(runtime.world.attach(&mut runtime.engine, id, "late.script"));
    }
    let freezable = |global: &str| -> Vec<bool> {
        let id = runtime.engine.global(global).unwrap().as_native().unwrap();
        runtime
            .engine
            .scripts_of(id)
            .into_iter()
            .map(|s| runtime.engine.script(s).unwrap().is_freezable())
            .collect()
    };
    // The window went system-exclusive (`MakeFreezable(false)`) before
    // the scripts were attached: they take the flag the window and its
    // controls keep.
    assert_eq!(freezable("w"), [false]);
    assert_eq!(freezable("b"), [false]);
    assert_eq!(freezable("other"), [true]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_destroyed_object_nulls_the_values_that_held_it() {
    let (dir, runtime, log) = start(
        r#"
global result = new Object();
global o = new Object();
var copy = o;
var holder = new Object();
holder.ref = o;
var list = new Array();
list.Push(o);
var plain;
plain.inner = o;
result.kept = o;
result.before = (o == null);
Game.UnloadObject(o);
result.isNull = (o == null);
result.copyNull = (copy == null);
result.propNull = (holder.ref == null);
result.elementNull = (list[0] == null);
result.innerNull = (plain.inner == null);
result.text = ToString(copy);
result.keptNull = (result.kept == null);
"#,
        &[],
    );
    no_errors(&log);
    assert_eq!(result(&runtime, "before"), Value::Bool(false));
    for key in [
        "isNull",
        "copyNull",
        "propNull",
        "elementNull",
        "innerNull",
        "keptNull",
    ] {
        assert_eq!(result(&runtime, key), Value::Bool(true), "{key}");
    }
    assert_eq!(result(&runtime, "text"), Value::from("[null]"));
    assert_eq!(runtime.engine.global("o"), Some(&Value::Null));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn objects_of_a_scene_left_behind_are_null() {
    let (dir, mut runtime, mut log) = start(
        r#"
global result = new Object();
global prop;
Game.ChangeScene("scenes/room.scene");

on "Grab"
{
  var scene = Game.Scene;
  prop = scene.GetNode("prop");
  result.grabbed = (prop != null);
  Game.ChangeScene("scenes/room.scene");
}

on "Check"
{
  result.gone = (prop == null);
}
"#,
        &[File::Raw("scenes\\room.scene", ROOM)],
    );
    let game = runtime.world.game_id();
    log.extend(runtime.tick(16));
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Grab", false);
    // The scene changes once the transitions of the changes have run.
    for _ in 0..40 {
        log.extend(runtime.tick(16));
    }
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Check", false);
    log.extend(runtime.tick(16));
    no_errors(&log);
    assert_eq!(result(&runtime, "grabbed"), Value::Bool(true), "{log:?}");
    assert_eq!(result(&runtime, "gone"), Value::Bool(true));
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(feature = "compiler")]
#[test]
fn a_script_shipped_as_source_is_compiled_when_loaded() {
    let (dir, runtime, log) = start(
        r#"
global result = new Object();
result.attached = Game.AttachScript("src.script");
result.bad = Game.AttachScript("bad.script");
"#,
        &[
            File::Raw(
                "src.script",
                b"#include \"inc\\base.inc\"\r\nresult.fromSource = Twice(21);\r\n",
            ),
            File::Raw(
                "inc\\base.inc",
                b"global result;\r\nfunction Twice(n) { return n * 2; }\r\n",
            ),
            File::Raw("bad.script", b"var a = ;\r\n"),
        ],
    );
    no_errors(&log);
    assert_eq!(result(&runtime, "attached"), Value::Bool(true), "{log:?}");
    assert_eq!(result(&runtime, "fromSource"), Value::Int(42), "{log:?}");
    // A file that does not compile is not attached; the log has the
    // original's two lines and the player is told.
    assert_eq!(result(&runtime, "bad"), Value::Bool(false));
    let at = log
        .iter()
        .position(|l| l == "Compiling script 'bad.script'...")
        .unwrap_or_else(|| panic!("{log:?}"));
    assert!(log[at + 1].starts_with("  Error@line 1: "), "{log:?}");
    assert!(
        runtime
            .world
            .quick_messages()
            .iter()
            .any(|m| m.text == "Script compiler error. View log for details."),
        "{:?}",
        runtime.world.quick_messages()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_runtime_error_tells_the_player_unless_suppressed() {
    let message = "Script runtime error. View log for details.";
    let (dir, runtime, log) = start(
        r#"
global result = new Object();
result.before = Game.SuppressScriptErrors;
var zero = 0;
var x = 1 / zero;
"#,
        &[],
    );
    assert_eq!(result(&runtime, "before"), Value::Bool(false));
    assert!(
        log.iter()
            .any(|l| l == "Runtime error. Script 'game.script', line 5: Division by zero."),
        "{log:?}"
    );
    let shown = |runtime: &Runtime| {
        runtime
            .world
            .quick_messages()
            .iter()
            .filter(|m| m.text == message)
            .count()
    };
    assert_eq!(shown(&runtime), 1);
    std::fs::remove_dir_all(&dir).ok();

    let (dir, runtime, log) = start(
        r#"
global result = new Object();
Game.SuppressScriptErrors = true;
result.after = Game.SuppressScriptErrors;
var zero = 0;
var x = 1 / zero;
var f = new File("nowhere.txt");
f.ReadLine();
"#,
        &[],
    );
    assert_eq!(result(&runtime, "after"), Value::Bool(true));
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("Runtime error."))
            .count(),
        2,
        "still logged: {log:?}"
    );
    assert_eq!(shown(&runtime), 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn scripts_are_known_by_the_path_they_were_attached_with() {
    let recorded = compile_as("on \"Nothing\" { }", "other\\name.script");
    let (dir, runtime, log) = start(
        r#"
global result = new Object();
var o = new Object("obj.script");
result.byPath = o.IsScriptRunning("OBJ.script");
result.byRecorded = o.IsScriptRunning("other\name.script");
result.again = o.AttachScript("obj.script");
result.detachRecorded = o.DetachScript("other\name.script");
result.detach = o.DetachScript("obj.script");
"#,
        &[File::Raw("obj.script", &recorded)],
    );
    no_errors(&log);
    assert_eq!(result(&runtime, "byPath"), Value::Bool(true));
    assert_eq!(result(&runtime, "byRecorded"), Value::Bool(false));
    assert_eq!(result(&runtime, "again"), Value::Bool(true));
    assert!(
        log.iter()
            .any(|l| l.contains("trying to add script 'obj.script' multiple times")),
        "{log:?}"
    );
    assert_eq!(result(&runtime, "detachRecorded"), Value::Bool(false));
    assert_eq!(result(&runtime, "detach"), Value::Bool(true));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn clearing_the_script_cache_reads_a_file_again() {
    let (dir, mut runtime, mut log) = start(
        r#"
global result = new Object();
global a;
global b;
global c;

on "First"
{
  a = new Object("loose.script");
}

on "Second"
{
  b = new Object("loose.script");
}

on "Third"
{
  result.cleared = Game.ClearScriptCache();
  c = new Object("loose.script");
}
"#,
        &[],
    );
    let game = runtime.world.game_id();
    let step = |runtime: &mut Runtime, event: &str, log: &mut Vec<String>| {
        runtime
            .engine
            .apply_event(&mut runtime.world, game, event, false);
        log.extend(runtime.tick(16));
        log.extend(runtime.tick(16));
    };
    let version = |n: i32| {
        compile(
            &format!("global result;\nresult.version = {n};"),
            "loose.script",
        )
    };
    std::fs::write(dir.join("loose.script"), version(1)).unwrap();
    step(&mut runtime, "First", &mut log);
    assert_eq!(result(&runtime, "version"), Value::Int(1), "{log:?}");
    // The file changes on disk: the cached program still runs.
    std::fs::write(dir.join("loose.script"), version(2)).unwrap();
    step(&mut runtime, "Second", &mut log);
    assert_eq!(result(&runtime, "version"), Value::Int(1));
    step(&mut runtime, "Third", &mut log);
    assert_eq!(result(&runtime, "cleared"), Value::Bool(true));
    assert_eq!(result(&runtime, "version"), Value::Int(2));
    no_errors(&log);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_profiler_logs_its_table_and_small_members_answer() {
    let (dir, mut runtime, mut log) = start(
        r#"
global result = new Object();
Game.EnableScriptProfiling();
result.memory = Game.UsedMemory;
var t = new TcpClient("host");
var u = new TcpClient();
result.tcpType = t.Type;
result.tcpText = "" + t;
result.tcpEqual = (t == u);
result.tcpValid = (t != null);
var list = new Array();
list[""] = "zero";
result.emptyIndex = list[0];
result.length = list.Length;
// The turn in which profiling is turned on is not measured, as in the
// original; the next one is.
Sleep(1);
Sleep(1);
Game.DisableScriptProfiling();
"#,
        &[],
    );
    log.extend(runtime.tick(16));
    log.extend(runtime.tick(16));
    no_errors(&log);
    assert_eq!(result(&runtime, "memory"), Value::Int(0));
    assert_eq!(result(&runtime, "tcpType"), Value::from("tcp-client"));
    assert_eq!(
        result(&runtime, "tcpText"),
        Value::from("[tcp-client object]")
    );
    assert_eq!(result(&runtime, "tcpEqual"), Value::Bool(true));
    assert_eq!(result(&runtime, "tcpValid"), Value::Bool(true));
    assert_eq!(result(&runtime, "emptyIndex"), Value::from("zero"));
    assert_eq!(result(&runtime, "length"), Value::Int(1));
    let at = log
        .iter()
        .position(|l| l == "***** Script profiling information: *****")
        .unwrap_or_else(|| panic!("{log:?}"));
    assert!(
        log[at + 1].starts_with("  Total execution time "),
        "{log:?}"
    );
    assert!(log[at + 2].starts_with("  game.script "), "{log:?}");
    assert!(!runtime.engine.is_profiling());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn holders_are_unnamed_and_keep_the_file_they_were_loaded_from() {
    let (dir, runtime, log) = start(
        r#"
global result = new Object();
result.gameFile = Game.Filename;
var e = Game.CreateEntity();
result.createdName = e.Name;
result.createdFile = e.Filename;
var f = Game.LoadEntity("ent/free.entity");
result.loadedName = f.Name;
result.loadedFile = f.Filename;
result.active = f.Active;
result.custom = f.Custom;
var a = new Actor();
result.actorName = a.Name;
var w = Game.LoadWindow("w.window");
result.windowName = w.Name;
var b = w.CreateButton();
result.buttonName = b.Name;
var em = e.CreateParticleEmitter();
result.emitterName = em.Name;
result.emitterFile = em.Filename;
em.Name = 12;
result.emitterNamed = em.Name;
var s = f.GetSpriteObject();
result.spriteName = s.Name;
s.Name = "turn";
result.spriteNamed = s.Name;
result.spriteFile = s.Filename;
"#,
        &[
            File::Raw(
                "ent\\free.entity",
                b"ENTITY { X=3 Y=4 SPRITE=\"spin.sprite\" PROPERTY { NAME=\"Active\" VALUE=\"FALSE\" } PROPERTY { NAME=\"Custom\" VALUE=\"7\" } }",
            ),
            File::Raw("spin.sprite", b"SPRITE { NAME=\"spin\" LOOPING=TRUE }"),
            File::Raw("w.window", WINDOW),
        ],
    );
    no_errors(&log);
    let text = |key: &str| result(&runtime, key);
    assert_eq!(text("gameFile"), Value::from("default.game"));
    // `CBScriptHolder`'s constructor: `<unnamed>`, and no file.
    assert_eq!(text("createdName"), Value::from("<unnamed>"));
    assert_eq!(text("createdFile"), Value::Null);
    assert_eq!(text("loadedName"), Value::from("<unnamed>"), "no NAME key");
    assert_eq!(text("loadedFile"), Value::from("ent/free.entity"));
    assert_eq!(text("actorName"), Value::from("<unnamed>"));
    assert_eq!(text("windowName"), Value::from("w"));
    assert_eq!(text("buttonName"), Value::from("<unnamed>"));
    assert_eq!(text("emitterName"), Value::from("<unnamed>"));
    assert_eq!(text("emitterFile"), Value::Null);
    assert_eq!(
        text("emitterNamed"),
        Value::from("12"),
        "a name is a string"
    );
    // A sprite starts with the name of its file, empty without one.
    assert_eq!(text("spriteName"), Value::from("spin"), "{log:?}");
    assert_eq!(text("spriteNamed"), Value::from("turn"));
    assert_eq!(text("spriteFile"), Value::from("spin.sprite"));
    // A PROPERTY block is an assignment: a built-in property is set, any
    // other name is kept as the string it was written as.
    assert_eq!(text("active"), Value::Bool(false));
    assert_eq!(text("custom"), Value::from("7"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_profiler_left_on_writes_its_table_when_the_game_ends() {
    let (dir, mut runtime, log) = start(
        r#"
Game.EnableScriptProfiling();
Sleep(1);
"#,
        &[],
    );
    no_errors(&log);
    assert!(runtime.engine.is_profiling());
    let lines = runtime.shutdown();
    assert_eq!(lines[0], "***** Script profiling information: *****");
    assert!(lines[1].starts_with("  Total execution time "), "{lines:?}");
    assert!(lines[2].starts_with("  game.script "), "{lines:?}");
    assert!(runtime.shutdown().is_empty(), "the profiler is off now");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_scene_without_a_name_keeps_the_name_of_the_one_before() {
    let (dir, mut runtime, mut log) = start(
        r#"
global result = new Object();
var scene = Game.Scene;
result.first = scene.Name;
Game.ChangeScene("scenes/room.scene");

on "Next"
{
  var scene = Game.Scene;
  result.named = scene.Name;
  Game.ChangeScene("scenes/bare.scene");
}

on "Check"
{
  var scene = Game.Scene;
  result.kept = scene.Name;
  result.file = scene.Filename;
}
"#,
        &[
            File::Raw("scenes\\room.scene", ROOM),
            File::Raw(
                "scenes\\bare.scene",
                b"SCENE { LAYER { MAIN=TRUE WIDTH=64 HEIGHT=48 } }",
            ),
        ],
    );
    let game = runtime.world.game_id();
    for event in ["Next", "Check"] {
        log.extend(runtime.tick(16));
        runtime
            .engine
            .apply_event(&mut runtime.world, game, event, false);
        // The scene changes once the transitions of the changes have
        // run.
        for _ in 0..40 {
            log.extend(runtime.tick(16));
        }
    }
    log.extend(runtime.tick(16));
    no_errors(&log);
    // `CAdGame` names its scene object `""`; only `NAME` changes it.
    assert_eq!(result(&runtime, "first"), Value::from(""));
    assert_eq!(result(&runtime, "named"), Value::from("Room"));
    assert_eq!(result(&runtime, "kept"), Value::from("Room"), "{log:?}");
    assert_eq!(result(&runtime, "file"), Value::from("scenes/bare.scene"));
    std::fs::remove_dir_all(&dir).ok();
}
