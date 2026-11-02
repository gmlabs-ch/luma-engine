//! The global `Directory` object in a game: directories are created,
//! listed and deleted under the save directory, listings add the game's
//! own files, and a path spelled from `Game.SaveDirectory` works.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_script::Value;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-directory-game-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const GAME_SCRIPT: &str = r#"
global result = new Object();
var shots = Game.SaveDirectory + Directory.PathSeparator + "shots";
result.created = Directory.Create(shots);
var f = new File(shots + "\one.png");
f.OpenAsBinary(2);
f.WriteByte(1);
f.Close();
var files = Directory.GetFiles(shots, "*.png");
result.shots = files.Length;
result.first = files[0];
var game = Directory.GetFiles("", "*.game");
result.gameFiles = game.Length;
result.gameFile = game[0];
var data = Directory.GetFiles("data", null);
result.dataFiles = data.Length;
result.dataFile = data[0];
var dirs = Directory.GetDirectories("");
result.dirs = dirs.Length;
result.dir0 = dirs[0];
result.dir1 = dirs[1];
result.type = Directory.Type;
result.cwd = Directory.CurrentDirectory;
result.temp = Directory.TempDirectory;
result.deletedFull = Directory.Delete("shots");
result.escaped = Directory.Create("..\outside");
"#;

fn compile(source: &str) -> Vec<u8> {
    Compiler::new(Natives::wme())
        .compile(source.as_bytes(), "game.script", &mut |_: &[u8]| None)
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
        b"GAME { NAME=\"Directory\" SCRIPT=\"game.script\" }".to_vec(),
    );
    pkg.add_file("game.script", compile(GAME_SCRIPT));
    pkg.add_file("data\\readme.txt", b"hello\r\n".to_vec());
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
fn scripts_use_the_directory_object_under_the_save_directory() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let log = runtime.tick(16);
    assert!(
        !log.iter().any(|l| l.starts_with("Runtime error")),
        "{log:?}"
    );
    let saves = runtime.world.save_directory();
    assert_eq!(result(&runtime, "created"), Value::Bool(true));
    assert!(saves.join("shots/one.png").is_file());
    assert_eq!(result(&runtime, "shots"), Value::Int(1));
    assert_eq!(result(&runtime, "first"), Value::from("one.png"));
    assert_eq!(
        result(&runtime, "gameFiles"),
        Value::Int(1),
        "the game's packaged files are listed too"
    );
    assert_eq!(result(&runtime, "gameFile"), Value::from("default.game"));
    assert_eq!(result(&runtime, "dataFiles"), Value::Int(1));
    assert_eq!(result(&runtime, "dataFile"), Value::from("readme.txt"));
    // The game's `data` package directory, the loose `saves` directory
    // and the save directory's own `shots`; `temp` is made later.
    assert_eq!(result(&runtime, "dirs"), Value::Int(3));
    assert_eq!(result(&runtime, "dir0"), Value::from("data"));
    assert_eq!(result(&runtime, "dir1"), Value::from("saves"));
    assert_eq!(result(&runtime, "type"), Value::from("directory"));
    let root = saves.to_string_lossy().into_owned();
    assert_eq!(result(&runtime, "cwd"), Value::from(format!("{root}/")));
    assert_eq!(
        result(&runtime, "temp"),
        Value::from(format!("{root}/temp/"))
    );
    assert!(saves.join("temp").is_dir());
    assert_eq!(result(&runtime, "deletedFull"), Value::Bool(false));
    assert_eq!(result(&runtime, "escaped"), Value::Bool(false));
    assert!(!dir.join("outside").exists());
    std::fs::remove_dir_all(&dir).ok();
}
