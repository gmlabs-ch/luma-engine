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
