//! Desktop player for games running on Luma Engine.
//!
//! Runs a game in a window: its scripts, scenes, actors, sound, mouse and
//! keyboard; shows one scene without scripts (`--scene`); or runs the
//! scripts without a window (`--headless`).

mod keys;
mod view;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use luma::adventure::{Audio, Camera, GameSettings, Runtime, SceneView};
use luma::archive::Vfs;
use luma::formats::Rgba;
use luma::formats::font::TextAlign;
use luma::render::winit::dpi::{PhysicalPosition, PhysicalSize};
use luma::render::winit::event::{
    ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent,
};
use luma::render::winit::keyboard::{Key, ModifiersState, NamedKey};
use luma::render::{App, DrawList, Flow, Frame, Gpu, Offscreen, Renderer, WindowConfig};

const USAGE: &str = "\
luma-player - desktop player for Luma Engine

USAGE:
    luma-player <game> [OPTIONS]
    luma-player <game> --scene <file> [OPTIONS]
    luma-player <game> --headless [--scene <file>] [OPTIONS]

Without --scene or --headless, the game runs in a window: its scripts
pick the scenes, the mouse and the keyboard go to them.

OPTIONS:
    --scene <file>        Show one scene without running the game (a path inside the game,
                          e.g. scenes/room/room.scene); in headless mode, the scene to jump
                          to after the game script ran
    --scroll <x>,<y>      Initial scroll offset in scene pixels (default 0,0)
    --screenshot <png>    Render one frame to a PNG file and exit, without a window
    --animate             Play sprite animations (the default shows every first frame)
    --headless            Run the game's scripts without a window and print the log
    --ticks <n>           Frames to run in headless mode (default 60)
    --dt <ms>             Milliseconds per frame in headless mode (default 16)
    --at <x>,<y>          In headless mode, place the main actor there once the scene is up
    --say <text>          In headless mode, make the main actor say a sentence once the scene is up
    --font <file>         With --say, the font the actor talks with
    --mute                Play no sound (the sounds still run, silently, on the game clock)
    --seed <n|clock>      Seed of the random numbers (Random, particles): a number for a run
                          that draws the same numbers every time, or `clock` for the clock,
                          as the original engine does. By default the clock in a window, a
                          fixed seed in headless mode, so that headless runs can be compared
    --code-page <n>       The ANSI code page of the system the game was made for, when its
                          text is not UTF-8: 1250 Central European, 1251 Cyrillic, 1252
                          Western (the default), 1253 Greek, 1254 Turkish, 1255 Hebrew, 1256
                          Arabic, 1257 Baltic, 1258 Vietnamese, 874 Thai, 932 Japanese, 936
                          and 950 Chinese, 949 Korean. By default what the game's TrueType
                          fonts say with CHARSET, else 1252
    --scale <n>           Open the window at n times the game's resolution (by default the
                          largest whole number that keeps it within 1280 points wide)
    --fullscreen          Run the game full screen (the frame fills the desktop, letterboxed)
    --windowed            Run the game in a window (the default, unless the settings say
                          otherwise or the game forbids it with ALLOW_WINDOWED_MODE=FALSE)
    --click <node>[:ev]   In headless mode, raise an event (LeftClick by default) on a scene
                          node, e.g. --click Door or --click Freezer:LookAt
    --click-at <x>,<y>    In headless mode, move the mouse there and click; always a click of
                          its own, never the second of a double click, however close the
                          step before
    --double-click-at <x>,<y>
                          In headless mode, move the mouse there and double-click: a click,
                          then a second press that is the double click (LeftDoubleClick)
    --click-on <node>     In headless mode, move the mouse over a scene node where it is
                          now (the centre of its hit area) and click, e.g. a wandering
                          character
    --right-click-at <x>,<y>
                          In headless mode, move the mouse there and right-click (a verb menu
                          centred on the mouse is then clicked with --click-at at the same point)
    --wheel <up|down>     In headless mode, turn the mouse wheel a notch (MouseWheelUp or
                          MouseWheelDown on the focused window, else on the game)
    --key [ctrl+][shift+][alt+]<code>
                          In headless mode, press a key (a Windows virtual key code, e.g. 27 for
                          Escape, 46 for Delete, ctrl+86 to paste), then the character the key
                          types on a US keyboard, if any
    --repeat-key <code>:<n>
                          In headless mode, hold a key until it has repeated n times, as a held
                          key does: n+1 presses, then one release
    --hold-key <code>[:<frames>]
                          In headless mode, hold a key down for that many frames (--after by
                          default), for scripts that poll Keyboard.IsKeyDown
    --type <text>         In headless mode, type the text, a key per character (into the
                          focused editor): each key as a US keyboard has it, then its
                          character; a character the layout lacks (an accented letter) comes
                          without a key
    --drag <x>,<y>:<x>,<y>
                          In headless mode, press the left button at the first point, move to
                          the second over --after frames and release there (a puzzle piece)
    --wait <n>            In headless mode, run n frames, e.g. for a loaded game's sentence to end
    --after <n>           In headless mode, frames between two steps (default 3), e.g. to let
                          a script finish first
    --hover-at <x>,<y>    In headless mode, move the mouse there once the scene is up
    --then-scene <file>   In headless mode, change to that scene --after frames after the clicks;
                          repeatable, to leave a scene and come back to it
    --entities            In headless mode, list the last scene's entities and the states
                          remembered of the scenes left
    --global <name>       In headless mode, print a script global's value at the end
                          (repeatable)
    --save <slot>         In headless mode, save the game to that slot after the clicks and
                          the scene changes (as Game.SaveGame does)
    --load <slot>         Load that slot before the first frame (as Game.LoadGame does);
                          the game's own scripts are replaced by the saved ones
    --savegame <file>     Load that saved game before the first frame, by its file name
                          in the save directory (save001.dsv) or its path
    --help                Show this message

--click, --click-at, --double-click-at, --click-on, --right-click-at, --drag, --wheel, --key, --hold-key,
--repeat-key, --type and --wait are steps: they run in the order given, --after frames apart, starting one
frame after the scene is up, once its scripts have placed the actor, as
for a player who acts when the scene shows.

With --headless and --screenshot, the scene and the actors are rendered
once after the last frame.

<game> is a game directory (its *.dcp packages and loose files are read
the way the engine does).

With --scene: arrow keys scroll the scene, Escape quits. In the game, the
keys and the mouse belong to the game; close the window to quit.
";

const BLACK: Rgba = Rgba {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};

/// What `--seed` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seed {
    /// The clock, as the original engine seeds.
    Clock,
    /// A number: the same draws at every run.
    Fixed(u64),
}

/// A step of the headless run, in command-line order.
enum Step {
    /// Raise an event on a scene node by name (`node` or `node:event`).
    Node(String),
    ClickAt(i32, i32),
    /// A click, then the second press of a double click.
    DoubleClickAt(i32, i32),
    /// Click a scene node where it is at that moment.
    ClickOn(String),
    RightClickAt(i32, i32),
    /// Press and release a key, with modifiers held.
    Key {
        code: i32,
        shift: bool,
        alt: bool,
        control: bool,
    },
    /// Press a key, let it repeat that many times, release it.
    RepeatKey(i32, u32),
    /// Turn the wheel a notch: `1` up, `-1` down.
    Wheel(i32),
    /// Hold a key down for that many frames (`--after` when `None`).
    HoldKey(i32, Option<u64>),
    /// Run that many frames.
    Wait(u64),
    Type(String),
    /// Press the left button at one point, move to another over the
    /// `--after` frames, hold a few frames there and release.
    Drag(i32, i32, i32, i32),
}

struct Options {
    game: PathBuf,
    scene: Option<String>,
    scroll: (f32, f32),
    screenshot: Option<PathBuf>,
    animate: bool,
    headless: bool,
    ticks: u64,
    dt: u64,
    at: Option<(i32, i32)>,
    say: Option<String>,
    font: Option<String>,
    mute: bool,
    /// Window size as a multiple of the game's resolution (`--scale`).
    scale: Option<u32>,
    /// The game's ANSI code page (`--code-page`).
    code_page: Option<luma::adventure::code_page::CodePage>,
    /// `--windowed` (true) or `--fullscreen` (false).
    windowed: Option<bool>,
    /// `--seed`: a number, or `Seed::Clock`.
    seed: Option<Seed>,
    steps: Vec<Step>,
    hover_at: Option<(i32, i32)>,
    after: u64,
    then_scenes: Vec<String>,
    entities: bool,
    /// Globals of the scripts to print at the end (`--global`).
    globals: Vec<String>,
    save: Option<i32>,
    load: Option<i32>,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut game = None;
    let mut scene = None;
    let mut scroll = (0.0, 0.0);
    let mut screenshot = None;
    let mut animate = false;
    let mut headless = false;
    let mut ticks = 60;
    let mut dt = 16;
    let mut at = None;
    let mut say = None;
    let mut font = None;
    let mut mute = false;
    let mut scale = None;
    let mut code_page = None;
    let mut windowed = None;
    let mut seed = None;
    let mut steps = Vec::new();
    let mut hover_at = None;
    let mut after = 3;
    let mut then_scenes = Vec::new();
    let mut entities = false;
    let mut globals = Vec::new();
    let mut save = None;
    let mut load = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let value = |i: &mut usize| -> Result<&String, String> {
            *i += 1;
            args.get(*i).ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--scene" => scene = Some(value(&mut i)?.clone()),
            "--scroll" => {
                let v = value(&mut i)?;
                let (x, y) = v
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .ok_or_else(|| format!("--scroll expects <x>,<y>, got `{v}`"))?;
                scroll = (x, y);
            }
            "--screenshot" => screenshot = Some(PathBuf::from(value(&mut i)?)),
            "--animate" => animate = true,
            "--then-scene" => then_scenes.push(value(&mut i)?.clone()),
            "--entities" => entities = true,
            "--global" => globals.push(value(&mut i)?.clone()),
            "--save" | "--load" => {
                let v = value(&mut i)?;
                let slot = v
                    .parse()
                    .map_err(|_| format!("{arg} expects a slot number, got `{v}`"))?;
                if arg == "--save" {
                    save = Some(slot);
                } else {
                    load = Some(slot);
                }
            }
            "--headless" => headless = true,
            "--click" => steps.push(Step::Node(value(&mut i)?.clone())),
            "--say" => say = Some(value(&mut i)?.clone()),
            "--font" => font = Some(value(&mut i)?.clone()),
            "--mute" => mute = true,
            "--seed" => {
                let v = value(&mut i)?;
                seed =
                    Some(if v.eq_ignore_ascii_case("clock") {
                        Seed::Clock
                    } else {
                        Seed::Fixed(v.parse().map_err(|_| {
                            format!("--seed expects a number or `clock`, got `{v}`")
                        })?)
                    });
            }
            "--fullscreen" => windowed = Some(false),
            "--windowed" => windowed = Some(true),
            "--savegame" => {
                let v = value(&mut i)?;
                load = Some(save_slot_of(v).ok_or_else(|| {
                    format!("--savegame expects a saved game named save<slot>.dsv, got `{v}`")
                })?);
            }
            "--code-page" => {
                let v = value(&mut i)?;
                code_page = Some(
                    v.parse::<u16>()
                        .ok()
                        .and_then(luma::adventure::code_page::CodePage::new)
                        .ok_or_else(|| {
                            format!("--code-page expects a Windows code page, got `{v}`")
                        })?,
                );
            }
            "--scale" => {
                let v = value(&mut i)?;
                scale = Some(
                    v.parse::<u32>()
                        .ok()
                        .filter(|n| (1..=16).contains(n))
                        .ok_or_else(|| {
                            format!("--scale expects a number from 1 to 16, got `{v}`")
                        })?,
                );
            }
            "--click-at" => {
                let v = value(&mut i)?;
                let (x, y) = v
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .ok_or_else(|| format!("--click-at expects <x>,<y>, got `{v}`"))?;
                steps.push(Step::ClickAt(x, y));
            }
            "--double-click-at" => {
                let v = value(&mut i)?;
                let (x, y) = v
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .ok_or_else(|| format!("--double-click-at expects <x>,<y>, got `{v}`"))?;
                steps.push(Step::DoubleClickAt(x, y));
            }
            "--click-on" => steps.push(Step::ClickOn(value(&mut i)?.clone())),
            "--type" => steps.push(Step::Type(value(&mut i)?.clone())),
            "--drag" => {
                let v = value(&mut i)?;
                let point = |p: &str| -> Option<(i32, i32)> {
                    let (x, y) = p.split_once(',')?;
                    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
                };
                let (from, to) = v
                    .split_once(':')
                    .and_then(|(from, to)| Some((point(from)?, point(to)?)))
                    .ok_or_else(|| format!("--drag expects <x>,<y>:<x>,<y>, got `{v}`"))?;
                steps.push(Step::Drag(from.0, from.1, to.0, to.1));
            }
            "--wait" => {
                let v = value(&mut i)?;
                let frames = v
                    .parse()
                    .map_err(|_| format!("--wait expects a number of frames, got `{v}`"))?;
                steps.push(Step::Wait(frames));
            }
            "--hover-at" => {
                let v = value(&mut i)?;
                let (x, y) = v
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .ok_or_else(|| format!("--hover-at expects <x>,<y>, got `{v}`"))?;
                hover_at = Some((x, y));
            }
            "--right-click-at" => {
                let v = value(&mut i)?;
                let (x, y) = v
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .ok_or_else(|| format!("--right-click-at expects <x>,<y>, got `{v}`"))?;
                steps.push(Step::RightClickAt(x, y));
            }
            "--after" => {
                let v = value(&mut i)?;
                after = v
                    .parse()
                    .map_err(|_| format!("--after expects a number of frames, got `{v}`"))?;
            }
            "--key" => {
                let v = value(&mut i)?;
                let (mut shift, mut alt, mut control) = (false, false, false);
                let mut parts: Vec<&str> = v.split('+').collect();
                let code = parts
                    .pop()
                    .and_then(|code| code.trim().parse().ok())
                    .ok_or_else(|| format!("--key expects a key code, got `{v}`"))?;
                for modifier in parts {
                    match modifier.trim().to_ascii_lowercase().as_str() {
                        "shift" => shift = true,
                        "alt" => alt = true,
                        "ctrl" | "control" => control = true,
                        _ => {
                            return Err(format!(
                                "--key expects [ctrl+][shift+][alt+]<code>, got `{v}`"
                            ));
                        }
                    }
                }
                steps.push(Step::Key {
                    code,
                    shift,
                    alt,
                    control,
                });
            }
            "--repeat-key" => {
                let v = value(&mut i)?;
                let (code, count) = v
                    .split_once(':')
                    .and_then(|(code, count)| {
                        Some((code.trim().parse().ok()?, count.trim().parse::<u32>().ok()?))
                    })
                    .filter(|(_, count)| *count <= 10_000)
                    .ok_or_else(|| format!("--repeat-key expects <code>:<n>, got `{v}`"))?;
                steps.push(Step::RepeatKey(code, count));
            }
            "--wheel" => {
                let v = value(&mut i)?;
                let delta = match v.as_str() {
                    "up" => 1,
                    "down" => -1,
                    _ => return Err(format!("--wheel expects up or down, got `{v}`")),
                };
                steps.push(Step::Wheel(delta));
            }
            "--hold-key" => {
                let v = value(&mut i)?;
                let (code, frames) = match v.split_once(':') {
                    Some((code, frames)) => (code, Some(frames)),
                    None => (v.as_str(), None),
                };
                let code = code
                    .parse()
                    .map_err(|_| format!("--hold-key expects a key code, got `{v}`"))?;
                let frames = frames
                    .map(|f| {
                        f.parse()
                            .map_err(|_| format!("--hold-key expects <code>[:<frames>], got `{v}`"))
                    })
                    .transpose()?;
                steps.push(Step::HoldKey(code, frames));
            }
            "--at" => {
                let v = value(&mut i)?;
                let (x, y) = v
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .ok_or_else(|| format!("--at expects <x>,<y>, got `{v}`"))?;
                at = Some((x, y));
            }
            "--ticks" => {
                let v = value(&mut i)?;
                ticks = v
                    .parse()
                    .map_err(|_| format!("--ticks expects a number, got `{v}`"))?;
            }
            "--dt" => {
                let v = value(&mut i)?;
                dt = v
                    .parse()
                    .map_err(|_| format!("--dt expects a number, got `{v}`"))?;
            }
            other if other.starts_with('-') => return Err(format!("unknown option `{other}`")),
            _ if game.is_none() => game = Some(PathBuf::from(arg)),
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }
    if scene.is_none() && !headless && screenshot.is_some() {
        return Err("--screenshot needs --scene or --headless".to_string());
    }
    Ok(Options {
        game: game.ok_or("missing <game>")?,
        scene,
        scroll,
        screenshot,
        animate,
        headless,
        ticks,
        dt,
        at,
        say,
        font,
        mute,
        scale,
        code_page,
        windowed,
        seed,
        steps,
        hover_at,
        after,
        then_scenes,
        entities,
        globals,
        save,
        load,
    })
}

/// The slot of a saved game's file (`save007.dsv`, with or without a
/// directory): the original's `-savegame <file>` names the file, Luma's
/// saved games are found by slot in the game's save directory.
fn save_slot_of(file: &str) -> Option<i32> {
    let name = Path::new(file).file_name()?.to_str()?.to_ascii_lowercase();
    let digits = name
        .strip_prefix("save")?
        .strip_suffix(&format!(".{}", luma::adventure::SAVE_EXTENSION))?;
    if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let options = match parse_args(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("luma-player: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("luma-player: {message}");
            ExitCode::FAILURE
        }
    }
}

fn open(game: &Path) -> Result<(Vfs, GameSettings), String> {
    let vfs = Vfs::open_game(game).map_err(|e| format!("{}: {e}", game.display()))?;
    if vfs.packages().is_empty() && !vfs.exists(luma::adventure::SETTINGS_FILE) {
        return Err(format!("{}: not a game directory", game.display()));
    }
    let settings = GameSettings::load(&vfs).map_err(|e| e.to_string())?;
    Ok((vfs, settings))
}

fn load(vfs: &Vfs, renderer: &Renderer, scene: &str) -> Result<SceneView, String> {
    let mut view = SceneView::load(vfs, scene).map_err(|e| e.to_string())?;
    view.upload(vfs, renderer);
    eprintln!(
        "scene {:?}: {}x{}, {} layer(s), {} entities, {} texture(s)",
        view.scene.name,
        view.scene.width(),
        view.scene.height(),
        view.scene.layers.len(),
        view.scene.entities().count(),
        view.texture_count()
    );
    for (path, reason) in view.missing() {
        eprintln!("  missing: {path}: {reason}");
    }
    Ok(view)
}

/// Runs the game's scripts without a window, printing the log.
fn headless(vfs: Vfs, options: &Options) -> Result<(), String> {
    let audio = if options.mute {
        Audio::null()
    } else {
        Audio::new()
    };
    let mut runtime = Runtime::with_audio(vfs, audio).map_err(|e| e.to_string())?;
    runtime.world.set_ansi_code_page(options.code_page);
    // Headless runs are compared with one another: the same random
    // numbers at every run unless the clock is asked for.
    match options.seed {
        Some(Seed::Clock) => {}
        Some(Seed::Fixed(seed)) => runtime.world.seed_random(seed),
        None => runtime.world.seed_random(luma::adventure::FIXED_SEED),
    }
    let game = runtime.world.game_file();
    println!(
        "game {:?}: {} script(s), {} strings, audio {}",
        game.name,
        game.scripts.len(),
        runtime.world.strings().len(),
        runtime.world.audio().device().unwrap_or("off")
    );
    runtime.start();
    if let Some(slot) = options.load {
        runtime.world.schedule_load(slot);
        println!("load scheduled from slot {slot}");
    }
    let mut clicked = false;
    let mut scene_up = false;
    for tick in 1..=options.ticks {
        for line in runtime.tick(options.dt) {
            println!("[{tick}] {line}");
        }
        if tick == 1 {
            if let Some(scene) = &options.scene {
                runtime
                    .change_scene(scene)
                    .map_err(|e| format!("{scene}: {e}"))?;
                for line in runtime.world.take_log() {
                    println!("[{tick}] {line}");
                }
            }
        }
        // The scene's own scripts get one frame before the steps, so that
        // its init script has placed the actor, as for a player who acts
        // once the scene shows.
        let act = scene_up && !clicked;
        scene_up = runtime.world.scene().is_some();
        if act {
            clicked = true;
            if let Some((x, y)) = options.at {
                if let Some(main) = runtime.world.main_object() {
                    runtime.world.place_actor(main, x, y);
                    println!("[{tick}] main actor placed at ({x}, {y})");
                }
            }
            if let Some(text) = &options.say {
                match runtime.world.main_object() {
                    Some(main) => {
                        if let Some(font) = &options.font {
                            let ok = runtime.world.set_font(main, Some(font));
                            println!(
                                "[{tick}] font {font:?}: {}",
                                if ok { "loaded" } else { "failed" }
                            );
                        }
                        let ok = runtime
                            .world
                            .talk(main, text, None, 0, None, TextAlign::Center);
                        println!(
                            "[{tick}] main actor says {text:?}: {}",
                            if ok { "started" } else { "no main actor" }
                        );
                    }
                    None => println!("[{tick}] --say: no main actor"),
                }
            }
            if let Some((x, y)) = options.hover_at {
                runtime.mouse_move(x, y);
                println!("[{tick}] mouse moved to ({x}, {y})");
            }
            for (index, step) in options.steps.iter().enumerate() {
                // A few frames between the steps, so that a menu opened by a
                // click or a key is in place, or that a script ran. The first
                // step comes at once, as the scene has just come up.
                if index > 0 {
                    for _ in 0..options.after {
                        for line in runtime.tick(options.dt) {
                            println!("[{tick}] {line}");
                        }
                    }
                }
                // Each step is a click of its own: two steps on the same
                // point a few frames apart are not a double click.
                runtime.world.forget_clicks();
                let double = matches!(step, Step::DoubleClickAt(..));
                let (x, y, right) = match step {
                    Step::ClickAt(x, y) | Step::DoubleClickAt(x, y) => (*x, *y, false),
                    Step::ClickOn(node) => {
                        let point = runtime
                            .world
                            .find_node(node)
                            .and_then(|id| runtime.world.hit_point(id));
                        match point {
                            Some((x, y)) => (x, y, false),
                            None => {
                                println!("[{tick}] click on {node:?}: not on screen");
                                continue;
                            }
                        }
                    }
                    Step::RightClickAt(x, y) => (*x, *y, true),
                    Step::Node(click) => {
                        let (node, event) = click.split_once(':').unwrap_or((click, "LeftClick"));
                        match runtime.world.find_node(node) {
                            Some(id) => {
                                let handled = runtime.engine.apply_event(
                                    &mut runtime.world,
                                    id,
                                    event,
                                    false,
                                );
                                println!(
                                    "[{tick}] {event} on {node:?}: {}",
                                    if handled { "handled" } else { "no handler" }
                                );
                            }
                            None => println!("[{tick}] {event} on {node:?}: no such node"),
                        }
                        continue;
                    }
                    Step::Key {
                        code,
                        shift,
                        alt,
                        control,
                    } => {
                        println!("[{tick}] key {code} pressed");
                        let text = luma::adventure::input::char_for_key(*code, *shift);
                        runtime.key_down(*code, text, *shift, *alt, *control);
                        runtime.key_up(*code);
                        continue;
                    }
                    Step::RepeatKey(code, count) => {
                        println!("[{tick}] key {code} held, repeated {count} time(s)");
                        let text = luma::adventure::input::char_for_key(*code, false);
                        for _ in 0..=*count {
                            runtime.key_down(*code, text, false, false, false);
                        }
                        runtime.key_up(*code);
                        continue;
                    }
                    Step::Wheel(delta) => {
                        println!("[{tick}] wheel {}", if *delta > 0 { "up" } else { "down" });
                        runtime.mouse_wheel(*delta);
                        continue;
                    }
                    Step::HoldKey(code, frames) => {
                        let frames = frames.unwrap_or(options.after);
                        println!("[{tick}] key {code} held down for {frames} frames");
                        runtime.key_down(*code, None, false, false, false);
                        for _ in 0..frames {
                            for line in runtime.tick(options.dt) {
                                println!("[{tick}] {line}");
                            }
                        }
                        runtime.key_up(*code);
                        continue;
                    }
                    Step::Wait(frames) => {
                        for _ in 0..*frames {
                            for line in runtime.tick(options.dt) {
                                println!("[{tick}] {line}");
                            }
                        }
                        continue;
                    }
                    Step::Drag(x1, y1, x2, y2) => {
                        runtime.mouse_move(*x1, *y1);
                        for line in runtime.tick(options.dt) {
                            println!("[{tick}] {line}");
                        }
                        let over = runtime
                            .world
                            .active_object()
                            .map(|id| runtime.world.describe_object(id))
                            .unwrap_or_else(|| "nothing".to_string());
                        println!(
                            "[{tick}] mouse at ({x1}, {y1}) over {over}; drag to ({x2}, {y2})"
                        );
                        runtime.mouse_left_down();
                        let frames = options.after.max(1);
                        for frame in 1..=frames {
                            let t = frame as f64 / frames as f64;
                            let x = x1 + ((x2 - x1) as f64 * t).round() as i32;
                            let y = y1 + ((y2 - y1) as f64 * t).round() as i32;
                            runtime.mouse_move(x, y);
                            for line in runtime.tick(options.dt) {
                                println!("[{tick}] {line}");
                            }
                        }
                        // Hold the piece at the end point so that a script
                        // that follows the mouse (`Sleep` loop) catches up.
                        for _ in 0..10 {
                            for line in runtime.tick(options.dt) {
                                println!("[{tick}] {line}");
                            }
                        }
                        runtime.mouse_left_up();
                        continue;
                    }
                    Step::Type(text) => {
                        println!("[{tick}] typing {text:?}");
                        for ch in text.chars() {
                            match luma::adventure::input::key_for_char(ch) {
                                Some((code, shift)) => {
                                    runtime.key_down(code, Some(ch), shift, false, false);
                                    runtime.key_up(code);
                                }
                                // No key of the layout types it: the
                                // character alone, as a composed one.
                                None => runtime.key_char(ch, false, false, false),
                            }
                        }
                        continue;
                    }
                };
                runtime.mouse_move(x, y);
                // A frame so that the object under the mouse is known.
                for line in runtime.tick(options.dt) {
                    println!("[{tick}] {line}");
                }
                let over = runtime
                    .world
                    .active_object()
                    .map(|id| runtime.world.describe_object(id))
                    .unwrap_or_else(|| "nothing".to_string());
                let button = if right { "right" } else { "left" };
                println!("[{tick}] mouse at ({x}, {y}) over {over}; {button} click");
                if right {
                    runtime.mouse_right_down();
                } else {
                    runtime.mouse_left_down();
                }
                // A frame between the press and the release, as a player's
                // click spans frames (a button notes it is held).
                for line in runtime.tick(options.dt) {
                    println!("[{tick}] {line}");
                }
                if right {
                    runtime.mouse_right_up();
                } else {
                    runtime.mouse_left_up();
                }
                if double {
                    // The second press, a frame later: the double click.
                    for line in runtime.tick(options.dt) {
                        println!("[{tick}] {line}");
                    }
                    println!("[{tick}] second press: double click");
                    runtime.mouse_left_down();
                    for line in runtime.tick(options.dt) {
                        println!("[{tick}] {line}");
                    }
                    runtime.mouse_left_up();
                }
            }
            for scene in &options.then_scenes {
                // Frames for the scene's scripts to run before leaving it.
                for _ in 0..options.after {
                    for line in runtime.tick(options.dt) {
                        println!("[{tick}] {line}");
                    }
                }
                runtime
                    .change_scene(scene)
                    .map_err(|e| format!("{scene}: {e}"))?;
                for line in runtime.world.take_log() {
                    println!("[{tick}] {line}");
                }
            }
            if let Some(slot) = options.save {
                for _ in 0..options.after {
                    for line in runtime.tick(options.dt) {
                        println!("[{tick}] {line}");
                    }
                }
                match runtime.save_game(slot, "luma-player", false) {
                    Ok(path) => println!("[{tick}] saved to {}", path.display()),
                    Err(err) => println!("[{tick}] save to slot {slot} failed: {err}"),
                }
                for line in runtime.world.take_log() {
                    println!("[{tick}] {line}");
                }
            }
        }
        if runtime.world.quit_requested() {
            println!("[{tick}] quit requested");
            break;
        }
    }
    for line in runtime.shutdown() {
        println!("[{}] {line}", options.ticks);
    }
    let counts = runtime.engine.counts();
    println!(
        "after {} ms: scene {:?}, offset {:?}, {} script(s) ({} running, {} waiting, {} persistent), {} native object(s)",
        runtime.world.timer(),
        runtime
            .world
            .scene()
            .map(|s| s.scene.name.as_str())
            .unwrap_or("none"),
        runtime.world.offset(),
        counts.total,
        counts.running,
        counts.waiting,
        counts.persistent,
        runtime.world.objects().len()
    );
    for name in &options.globals {
        println!(
            "global {name}: {}",
            runtime
                .engine
                .global(name)
                .map_or("undefined".to_string(), |v| format!("{v:?}"))
        );
    }
    if options.entities {
        if let Some(view) = runtime.world.scene() {
            for r in view.entity_refs() {
                if let Some(e) = view.entity(r) {
                    let shown = view.shown_sprite(r);
                    println!(
                        "entity {:?}: {}, at ({}, {}), sprite {:?}{}, caption {:?}, alpha {:?}",
                        e.name,
                        if e.active { "active" } else { "inactive" },
                        e.x,
                        e.y,
                        e.sprite.as_deref().unwrap_or("none"),
                        match &shown {
                            Some(s) if Some(s.as_str()) != e.sprite.as_deref() =>
                                format!(", showing {s:?}"),
                            _ => String::new(),
                        },
                        e.caption,
                        e.tint
                    );
                }
            }
        }
        for (file, state) in runtime.world.scene_states() {
            println!("scene state {file:?}: {} node(s)", state.nodes.len());
            for (name, node) in &state.nodes {
                println!(
                    "  {name}: {}, sprite {:?}, caption {:?}, alpha {:?}",
                    if node.active { "active" } else { "inactive" },
                    node.sprite.as_deref().unwrap_or("none"),
                    node.caption.as_deref().unwrap_or(""),
                    node.alpha
                );
            }
        }
    }
    for id in runtime.world.actor_ids() {
        if let Some(actor) = runtime.world.actor(id) {
            println!(
                "actor {:?}: at ({}, {}), facing {:?}, {:?}{}, sprite {:?}",
                actor.def.name,
                actor.x,
                actor.y,
                actor.dir,
                actor.state,
                if actor.def.active { "" } else { ", inactive" },
                actor.current_sprite().unwrap_or("none")
            );
        }
    }
    // The game's windows, then the response box's when it is up.
    let mut windows: Vec<_> = runtime.world.window_ids().to_vec();
    if let Some(responses) = runtime.world.responses_window() {
        if runtime.world.responses_visible() && !windows.contains(&responses) {
            windows.push(responses);
        }
    }
    for id in &windows {
        if let Some(control) = runtime.world.control(*id) {
            let mode = control.window().map(|w| w.mode);
            println!(
                "window {:?}: {}{}, at ({}, {}) {}x{}, {} control(s){}",
                control.name,
                if control.visible { "visible" } else { "hidden" },
                match mode {
                    Some(luma::adventure::WindowMode::Exclusive) => ", exclusive",
                    Some(luma::adventure::WindowMode::SystemExclusive) => ", system exclusive",
                    _ => "",
                },
                control.x,
                control.y,
                control.width,
                control.height,
                runtime.world.widgets_of(*id).len(),
                if runtime.world.focused_window() == Some(*id) {
                    ", focused"
                } else {
                    ""
                }
            );
            if control.visible {
                for widget in runtime.world.widgets_of(*id) {
                    if let Some(c) = runtime.world.control(widget) {
                        println!(
                            "  {} {:?}: {}, at ({}, {}) {}x{}, text {:?}",
                            c.type_name(),
                            c.name,
                            if c.visible { "visible" } else { "hidden" },
                            c.x,
                            c.y,
                            c.width,
                            c.height,
                            c.text
                        );
                    }
                }
            }
        }
    }
    if runtime.world.is_frozen() {
        println!("game frozen by a system-exclusive window or a video");
    }
    if let Some(video) = runtime.world.video() {
        println!(
            "video {:?}: {}x{} at {}/{} fps, frame {} shown, at ({}, {}) zoom {}%, {}{}",
            video.filename,
            video.info.width,
            video.info.height,
            video.info.fps_num,
            video.info.fps_den,
            video
                .frame_index()
                .map_or("none".to_string(), |i| i.to_string()),
            video.x,
            video.y,
            video.zoom,
            if video.is_playing() {
                "playing"
            } else {
                "finished"
            },
            if video.sound.is_some() {
                ", with sound"
            } else {
                ""
            }
        );
    }
    let inventory = runtime.world.inventory();
    if !inventory.items.is_empty() {
        let taken: Vec<String> = runtime
            .world
            .taken_items()
            .iter()
            .filter_map(|id| runtime.world.item(*id).map(|i| i.def.name.clone()))
            .collect();
        println!(
            "inventory: {} item(s), {} taken {:?}, box {}, selected {}",
            inventory.items.len(),
            taken.len(),
            taken,
            if inventory.visible {
                "visible"
            } else {
                "hidden"
            },
            runtime
                .world
                .selected_item()
                .and_then(|id| runtime.world.item(id))
                .map_or("none".to_string(), |i| format!("{:?}", i.def.name))
        );
        if options.entities {
            for (_, item) in &inventory.items {
                println!(
                    "  item {:?}: caption {:?}, sprite {:?}",
                    item.def.name,
                    item.def.caption,
                    item.def.sprite.as_deref().unwrap_or("none")
                );
            }
        }
        let responses = runtime.world.response_texts();
        if !responses.is_empty() {
            println!("responses: {responses:?}");
        }
    }
    for channel in 0..luma::adventure::MUSIC_CHANNELS {
        if let Some(music) = runtime.world.music(channel) {
            println!(
                "music channel {channel}: {} ({}, at {} ms)",
                music.filename,
                if runtime.world.is_music_playing(channel) {
                    "playing"
                } else {
                    "stopped"
                },
                runtime.world.music_position(channel)
            );
        }
    }
    println!(
        "mouse at ({}, {}) over {}, cursor {:?}{}",
        runtime.world.mouse().x,
        runtime.world.mouse().y,
        runtime
            .world
            .active_object()
            .map(|id| runtime.world.describe_object(id))
            .unwrap_or_else(|| "nothing".to_string()),
        runtime.world.cursors().shown().unwrap_or("none"),
        runtime
            .world
            .cursors()
            .item_cursor
            .as_deref()
            .map(|c| format!(", item cursor {c:?}"))
            .unwrap_or_default()
    );
    for sentence in runtime.world.sentences() {
        println!(
            "sentence {:?} with {} at ({}, {}), width {}",
            runtime.world.display_text(&sentence.text),
            sentence.font,
            sentence.x,
            sentence.y,
            sentence.width
        );
    }
    if let Some(output) = &options.screenshot {
        let gpu = Gpu::headless().map_err(|e| e.to_string())?;
        let renderer = Renderer::new(gpu);
        runtime.world.upload(&renderer);
        let settings = runtime.world.settings();
        let target = Offscreen::new(renderer.gpu(), settings.width, settings.height);
        let mut list = DrawList::new(settings.width as f32, settings.height as f32);
        runtime.draw(&mut list, Some(renderer.white()));
        renderer
            .render(target.target(), BLACK, &list)
            .map_err(|e| e.to_string())?;
        let image = target.read(renderer.gpu()).map_err(|e| e.to_string())?;
        let png = image.to_png().map_err(|e| e.to_string())?;
        std::fs::write(output, png).map_err(|e| format!("{}: {e}", output.display()))?;
        println!(
            "wrote {} ({}x{})",
            output.display(),
            image.width,
            image.height
        );
    }
    Ok(())
}

fn run(options: Options) -> Result<(), String> {
    let (vfs, settings) = open(&options.game)?;
    if options.headless {
        return headless(vfs, &options);
    }
    let Some(scene) = options.scene.clone() else {
        return play(vfs, settings, &options);
    };
    let mut camera = Camera::new(settings.width as f32, settings.height as f32);
    camera.offset_x = options.scroll.0;
    camera.offset_y = options.scroll.1;

    if let Some(output) = &options.screenshot {
        let gpu = Gpu::headless().map_err(|e| e.to_string())?;
        eprintln!("adapter: {}", gpu.describe());
        let renderer = Renderer::new(gpu);
        let view = load(&vfs, &renderer, &scene)?;
        camera.clamp_to(&view.scene);
        let target = Offscreen::new(renderer.gpu(), settings.width, settings.height);
        let mut list = DrawList::new(camera.width, camera.height);
        view.draw(&mut list, &camera);
        renderer
            .render(target.target(), BLACK, &list)
            .map_err(|e| e.to_string())?;
        let image = target.read(renderer.gpu()).map_err(|e| e.to_string())?;
        let png = image.to_png().map_err(|e| e.to_string())?;
        std::fs::write(output, png).map_err(|e| format!("{}: {e}", output.display()))?;
        eprintln!(
            "wrote {} ({}x{})",
            output.display(),
            image.width,
            image.height
        );
        return Ok(());
    }

    // Undocumented, for smoke tests: exit after that many frames.
    let frame_limit = std::env::var("LUMA_PLAYER_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok());
    let player = Player {
        frame_limit,
        vfs,
        scene,
        animate: options.animate,
        camera,
        view: None,
        started: Instant::now(),
        keys: Keys::default(),
    };
    luma::render::window::run(
        WindowConfig {
            title: format!("Luma Engine - {}", options.scene.unwrap_or_default()),
            width: settings.width,
            height: settings.height,
            ..WindowConfig::default()
        },
        player,
    )
    .map_err(|e| e.to_string())
}

/// Runs the game in a window.
fn play(vfs: Vfs, settings: GameSettings, options: &Options) -> Result<(), String> {
    let audio = if options.mute {
        Audio::null()
    } else {
        Audio::new()
    };
    let runtime = Runtime::with_audio(vfs, audio).map_err(|e| e.to_string())?;
    let caption = runtime.world.game_file().caption.clone();
    let name = runtime.world.game_file().name.clone();
    eprintln!(
        "game {:?}: audio {}",
        name,
        runtime.world.audio().device().unwrap_or("off")
    );
    let frame_limit = std::env::var("LUMA_PLAYER_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok());
    let mut runtime = runtime;
    runtime.world.set_ansi_code_page(options.code_page);
    // The world is seeded from the clock, as the original; a number
    // makes the run repeatable.
    if let Some(Seed::Fixed(seed)) = options.seed {
        runtime.world.seed_random(seed);
    }
    // Window or full screen (`CHWManager`): what the command line asks,
    // else the choice kept in the settings (`Video/Windowed`), else a
    // window; full screen only when the game forbids the window
    // (`ALLOW_WINDOWED_MODE`, which `Debug/AllowWindowed` overrides).
    let allow_windowed = runtime
        .world
        .read_engine_int("Debug", "AllowWindowed")
        .map_or(settings.allow_windowed, |v| v != 0);
    let saved = runtime
        .world
        .read_engine_int("Video", "Windowed")
        .map(|v| v != 0);
    let windowed = view::windowed_mode(allow_windowed, options.windowed, saved);
    if options.windowed == Some(true) && !windowed {
        eprintln!("this game runs full screen only (ALLOW_WINDOWED_MODE=FALSE)");
    }
    runtime.world.set_windowed(windowed);
    // The window opens where it was left (`Video/WindowPosX`, `WindowPosY`).
    let position = match (
        runtime.world.read_engine_int("Video", "WindowPosX"),
        runtime.world.read_engine_int("Video", "WindowPosY"),
    ) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    // The editors copy to and paste from the system's clipboard; when
    // it cannot be reached, they keep the game's own.
    match arboard::Clipboard::new() {
        Ok(clipboard) => runtime
            .world
            .set_clipboard(Box::new(SystemClipboard(clipboard))),
        Err(err) => eprintln!("no system clipboard: {err}"),
    }
    // Games of the time are 320 to 1024 pixels wide: open the window at a
    // whole multiple of that, so that a small game is not a stamp on a
    // modern screen. The frame is letterboxed into whatever the window is.
    let scale = options
        .scale
        .unwrap_or_else(|| (1280 / settings.width.max(1)).max(1));
    let player = GamePlayer {
        runtime,
        frame_limit,
        size: PhysicalSize::new(settings.width, settings.height),
        modifiers: ModifiersState::empty(),
        started: false,
        load: options.load,
        windowed,
        window_position: None,
        warp_failed: false,
        focused: true,
        occluded: false,
    };
    luma::render::window::run(
        WindowConfig {
            title: if caption.is_empty() { name } else { caption },
            width: settings.width.saturating_mul(scale).clamp(1, 16384),
            height: settings.height.saturating_mul(scale).clamp(1, 16384),
            cursor_visible: false,
            fullscreen: !windowed,
            position,
            ..WindowConfig::default()
        },
        player,
    )
    .map_err(|e| e.to_string())
}

/// The system's clipboard, for the game's editors (Ctrl+C, Ctrl+X,
/// Ctrl+V).
struct SystemClipboard(arboard::Clipboard);

impl luma::adventure::Clipboard for SystemClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.0.get_text().ok()
    }

    fn set_text(&mut self, text: &str) -> bool {
        self.0.set_text(text).is_ok()
    }
}

/// The game running in a window.
struct GamePlayer {
    runtime: Runtime,
    frame_limit: Option<u64>,
    /// The drawable area, for the mouse mapping.
    size: PhysicalSize<u32>,
    modifiers: ModifiersState,
    started: bool,
    /// A saved game to load before the first frame (`--load`,
    /// `--savegame`).
    load: Option<i32>,
    /// Whether the game runs in a window, else full screen.
    windowed: bool,
    /// Where the window's top-left corner is on the desktop, once
    /// known, to open it there next time.
    window_position: Option<(i32, i32)>,
    /// The system refused to move its cursor (said once).
    warp_failed: bool,
    /// Whether the window has the keyboard focus.
    focused: bool,
    /// Whether the window is minimised or wholly hidden.
    occluded: bool,
}

impl GamePlayer {
    /// The window got or lost the focus, was hidden or shown again:
    /// the game is active while its window is focused and visible
    /// (`WM_ACTIVATE`, `WM_SIZE` in `CBPlatform::WindowProc`).
    fn set_window_state(&mut self, focused: bool, occluded: bool) {
        self.focused = focused;
        self.occluded = occluded;
        let active = focused && !occluded;
        if active != self.runtime.is_active() {
            self.runtime.activate(active, false);
        }
    }

    /// A window position in game pixels, through the letterbox.
    fn to_game(&self, x: f64, y: f64) -> (i32, i32) {
        let settings = self.runtime.world.settings();
        view::to_game(
            (settings.width, settings.height),
            (self.size.width, self.size.height),
            (x, y),
        )
    }
}

impl Drop for GamePlayer {
    /// The game ends: the four volumes go to the settings file for the
    /// next run (`CBSoundMgr::~CBSoundMgr`), the window's position too
    /// (`CBPlatform::MessageLoop` on exit) unless the game ran full
    /// screen, and a script profiler still on writes its table, as the
    /// original engine does when it shuts down.
    fn drop(&mut self) {
        if let (true, Some((x, y))) = (self.windowed, self.window_position) {
            let world = &mut self.runtime.world;
            world.write_engine_setting("Video", "WindowPosX", &x.to_string());
            world.write_engine_setting("Video", "WindowPosY", &y.to_string());
        }
        if self.started {
            self.runtime.world.save_volume_settings();
        }
        for line in self.runtime.shutdown() {
            eprintln!("{line}");
        }
    }
}

impl App for GamePlayer {
    fn init(&mut self, renderer: &Renderer) -> luma::render::Result<()> {
        eprintln!("adapter: {}", renderer.gpu().describe());
        Ok(())
    }

    fn event(&mut self, event: &WindowEvent) -> Flow {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let (mut x, mut y) = self.to_game(position.x, position.y);
                if !self.windowed {
                    // Full screen keeps the cursor on the game's frame
                    // (`CBRenderer::ClipCursor`).
                    let settings = self.runtime.world.settings();
                    x = x.clamp(0, settings.width as i32 - 1);
                    y = y.clamp(0, settings.height as i32 - 1);
                }
                self.runtime.mouse_move(x, y);
            }
            WindowEvent::Moved(position) => {
                // A minimised window reports a place far off the desktop
                // on some systems: not one to come back to.
                if self.windowed && !self.occluded && position.x > -30000 && position.y > -30000 {
                    self.window_position = Some((position.x, position.y));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => self.runtime.mouse_left_down(),
                (MouseButton::Left, ElementState::Released) => self.runtime.mouse_left_up(),
                (MouseButton::Right, ElementState::Pressed) => self.runtime.mouse_right_down(),
                (MouseButton::Right, ElementState::Released) => self.runtime.mouse_right_up(),
                (MouseButton::Middle, ElementState::Pressed) => self.runtime.mouse_middle_down(),
                (MouseButton::Middle, ElementState::Released) => self.runtime.mouse_middle_up(),
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
                if steps != 0.0 {
                    self.runtime.mouse_wheel(if steps > 0.0 { 1 } else { -1 });
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Focused(focused) => self.set_window_state(*focused, self.occluded),
            WindowEvent::Occluded(occluded) => self.set_window_state(self.focused, *occluded),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        physical_key,
                        location,
                        state,
                        text,
                        ..
                    },
                ..
            } => {
                // A held key repeats: the game gets the key and its
                // character again each time, as Windows sends them.
                let code = keys::virtual_key(logical_key, *physical_key);
                let sided = code.and_then(|code| keys::sided_key(code, *location));
                if *state == ElementState::Released {
                    for code in code.into_iter().chain(sided) {
                        self.runtime.key_up(code);
                    }
                    return Flow::Continue;
                }
                let shift = self.modifiers.shift_key();
                let alt = self.modifiers.alt_key();
                // The Command key of a Mac stands for Control, so that
                // Cmd+C, Cmd+X, Cmd+V and Cmd+A edit as Ctrl does.
                let control = self.modifiers.control_key()
                    || (cfg!(target_os = "macos") && self.modifiers.super_key());
                // The key is held whether or not the game is told.
                for code in code.into_iter().chain(sided) {
                    self.runtime.world.key_held(code);
                }
                // On a Mac, Option types the third character of a key.
                if !keys::reaches_game(code, alt, control, cfg!(target_os = "macos")) {
                    return Flow::Continue;
                }
                let mut chars = text
                    .iter()
                    .flat_map(|t| t.chars())
                    .filter(|c| !c.is_control());
                if let Some(code) = code {
                    self.runtime
                        .key_down(code, chars.next(), shift, alt, control);
                }
                // A key without a code, or a second character (a dead
                // key that did not combine): the characters alone.
                for ch in chars {
                    self.runtime.key_char(ch, shift, alt, control);
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    fn active(&self) -> bool {
        self.runtime.is_active()
    }

    fn close_requested(&mut self) -> Flow {
        // A game script handling `QuitGame` decides; the window stays
        // open while it runs.
        if self.runtime.window_close() {
            Flow::Continue
        } else {
            Flow::Exit
        }
    }

    fn draw<'a>(&'a mut self, frame: &Frame<'a>, list: &mut DrawList<'a>) -> Flow {
        self.size = frame.size;
        if !self.started {
            self.started = true;
            self.runtime.start();
            if let Some(slot) = self.load {
                // As the original's `-savegame`: the saved game loads
                // right after the start-up.
                self.runtime.world.schedule_load(slot);
            }
            if self.windowed && self.window_position.is_none() {
                self.window_position = frame
                    .window
                    .outer_position()
                    .ok()
                    .map(|position| (position.x, position.y));
            }
        }
        if !self.runtime.is_active() {
            // The system asked for a repaint while the game sleeps: the
            // frame as it is, without running the game (`OnPaint`).
            let settings = self.runtime.world.settings();
            list.width = settings.width as f32;
            list.height = settings.height as f32;
            list.letterbox = true;
            self.runtime.draw(list, Some(frame.renderer.white()));
            return Flow::Continue;
        }
        // The engine advances its timer by at most a second per frame
        // (also for the first frame after the window was inactive).
        let dt = (frame.dt.as_millis() as u64).clamp(1, 1000);
        for line in self.runtime.tick(dt) {
            eprintln!("{line}");
        }
        if self.runtime.world.quit_requested() {
            return Flow::Exit;
        }
        if let Some(point) = self.runtime.world.take_cursor_warp() {
            // The game moved the mouse (`Game.SetMousePos`, the mouse
            // lock...): the system cursor follows, where the window
            // system allows it.
            let settings = self.runtime.world.settings();
            let (x, y) = view::to_window(
                (settings.width, settings.height),
                (frame.size.width, frame.size.height),
                point,
            );
            if let Err(err) = frame
                .window
                .set_cursor_position(PhysicalPosition::new(x, y))
            {
                if !self.warp_failed {
                    self.warp_failed = true;
                    eprintln!("cannot move the system cursor: {err}");
                }
            }
        }
        self.runtime.world.upload(frame.renderer);
        if self.runtime.world.wants_frame() {
            // A script asked for a screenshot or a save thumbnail: the
            // frame is rendered once more where it can be read back.
            let (width, height) = {
                let settings = self.runtime.world.settings();
                (settings.width, settings.height)
            };
            let target = Offscreen::new(frame.renderer.gpu(), width, height);
            let image = {
                let mut shot = DrawList::new(width as f32, height as f32);
                self.runtime.draw(&mut shot, Some(frame.renderer.white()));
                frame
                    .renderer
                    .render(target.target(), BLACK, &shot)
                    .and_then(|()| target.read(frame.renderer.gpu()))
            };
            match image {
                Ok(image) => self.runtime.world.deliver_frame(&image),
                Err(err) => eprintln!("cannot capture the frame: {err}"),
            }
        }
        let settings = self.runtime.world.settings();
        list.width = settings.width as f32;
        list.height = settings.height as f32;
        list.letterbox = true;
        self.runtime.draw(list, Some(frame.renderer.white()));
        match self.frame_limit {
            Some(limit) if frame.frame_index + 1 >= limit => Flow::Exit,
            // `Game.SuspendedRendering`: the game ran its frame, the
            // window keeps showing the last one presented.
            _ if self.runtime.world.suspended_rendering() => Flow::Skip,
            _ => Flow::Continue,
        }
    }
}

#[derive(Default)]
struct Keys {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

struct Player {
    frame_limit: Option<u64>,
    vfs: Vfs,
    scene: String,
    animate: bool,
    camera: Camera,
    view: Option<SceneView>,
    started: Instant,
    keys: Keys,
}

impl App for Player {
    fn init(&mut self, renderer: &Renderer) -> luma::render::Result<()> {
        eprintln!("adapter: {}", renderer.gpu().describe());
        let view = load(&self.vfs, renderer, &self.scene)
            .map_err(|reason| luma::render::Error::Render { reason })?;
        self.camera.clamp_to(&view.scene);
        self.view = Some(view);
        self.started = Instant::now();
        Ok(())
    }

    fn event(&mut self, event: &WindowEvent) -> Flow {
        if let WindowEvent::KeyboardInput {
            event: KeyEvent {
                logical_key, state, ..
            },
            ..
        } = event
        {
            let pressed = *state == ElementState::Pressed;
            match logical_key {
                Key::Named(NamedKey::Escape) => return Flow::Exit,
                Key::Named(NamedKey::ArrowLeft) => self.keys.left = pressed,
                Key::Named(NamedKey::ArrowRight) => self.keys.right = pressed,
                Key::Named(NamedKey::ArrowUp) => self.keys.up = pressed,
                Key::Named(NamedKey::ArrowDown) => self.keys.down = pressed,
                _ => {}
            }
        }
        Flow::Continue
    }

    fn draw<'a>(&'a mut self, frame: &Frame<'a>, list: &mut DrawList<'a>) -> Flow {
        let Some(view) = &mut self.view else {
            return Flow::Exit;
        };
        let step = 400.0 * frame.dt.as_secs_f32();
        if self.keys.left {
            self.camera.offset_x -= step;
        }
        if self.keys.right {
            self.camera.offset_x += step;
        }
        if self.keys.up {
            self.camera.offset_y -= step;
        }
        if self.keys.down {
            self.camera.offset_y += step;
        }
        self.camera.clamp_to(&view.scene);
        list.width = self.camera.width;
        list.height = self.camera.height;
        list.letterbox = true;
        if self.animate {
            // The engine's timer never reads 0 on a shown frame.
            view.advance(self.started.elapsed().as_millis() as u64 + 1);
        }
        view.draw(list, &self.camera);
        match self.frame_limit {
            Some(limit) if frame.frame_index + 1 >= limit => Flow::Exit,
            _ => Flow::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_game_file_names_its_slot() {
        assert_eq!(save_slot_of("save007.dsv"), Some(7));
        assert_eq!(save_slot_of("SAVE120.DSV"), Some(120));
        assert_eq!(save_slot_of("/games/saves/save001.dsv"), Some(1));
        assert_eq!(save_slot_of("save.dsv"), None);
        assert_eq!(save_slot_of("save-1.dsv"), None);
        assert_eq!(save_slot_of("save1234567.dsv"), None);
        assert_eq!(save_slot_of("notes.txt"), None);
        assert_eq!(save_slot_of(""), None);
    }

    #[test]
    fn window_options_are_parsed() {
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|a| a.to_string()).collect() };
        let options = parse_args(&args(&["game"])).unwrap();
        assert_eq!((options.windowed, options.load), (None, None));
        let options = parse_args(&args(&[
            "game",
            "--fullscreen",
            "--savegame",
            "save003.dsv",
        ]))
        .unwrap();
        assert_eq!((options.windowed, options.load), (Some(false), Some(3)));
        let options = parse_args(&args(&["game", "--windowed", "--load", "2"])).unwrap();
        assert_eq!((options.windowed, options.load), (Some(true), Some(2)));
        assert!(parse_args(&args(&["game", "--savegame", "x.txt"])).is_err());
        assert_eq!(options.seed, None);
        let options = parse_args(&args(&["game", "--seed", "42"])).unwrap();
        assert_eq!(options.seed, Some(Seed::Fixed(42)));
        let options = parse_args(&args(&["game", "--seed", "Clock"])).unwrap();
        assert_eq!(options.seed, Some(Seed::Clock));
        assert!(parse_args(&args(&["game", "--seed", "dice"])).is_err());
        assert_eq!(options.code_page, None);
        let options = parse_args(&args(&["game", "--code-page", "1251"])).unwrap();
        assert_eq!(options.code_page.map(|page| page.number()), Some(1251));
        assert!(parse_args(&args(&["game", "--code-page", "437"])).is_err());
    }
}
