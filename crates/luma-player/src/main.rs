//! Desktop player for games running on Luma Engine.
//!
//! Runs a game in a window: its scripts, scenes, actors, sound, mouse and
//! keyboard; shows one scene without scripts (`--scene`); or runs the
//! scripts without a window (`--headless`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use luma_adventure::{Audio, Camera, GameSettings, Runtime, SceneView};
use luma_archive::Vfs;
use luma_formats::Rgba;
use luma_formats::font::TextAlign;
use luma_render::winit::dpi::PhysicalSize;
use luma_render::winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use luma_render::winit::keyboard::{Key, ModifiersState, NamedKey};
use luma_render::{App, DrawList, Flow, Frame, Gpu, Offscreen, Renderer, WindowConfig, letterbox};

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
    --click <node>[:ev]   In headless mode, raise an event (LeftClick by default) on a scene
                          node, e.g. --click Door or --click Freezer:LookAt
    --click-at <x>,<y>    In headless mode, move the mouse there and click
    --right-click-at <x>,<y>
                          In headless mode, move the mouse there and right-click (a verb menu
                          centred on the mouse is then clicked with --click-at at the same point)
    --key <code>          In headless mode, press a key (a Windows virtual key code, e.g. 27 for
                          Escape)
    --hold-key <code>[:<frames>]
                          In headless mode, hold a key down for that many frames (--after by
                          default), for scripts that poll Keyboard.IsKeyDown
    --type <text>         In headless mode, type the text, a key per character (into the
                          focused editor)
    --wait <n>            In headless mode, run n frames, e.g. for a loaded game's sentence to end
    --after <n>           In headless mode, frames between two steps (default 3), e.g. to let
                          a script finish first
    --hover-at <x>,<y>    In headless mode, move the mouse there once the scene is up
    --then-scene <file>   In headless mode, change to that scene --after frames after the clicks;
                          repeatable, to leave a scene and come back to it
    --entities            In headless mode, list the last scene's entities and the states
                          remembered of the scenes left
    --save <slot>         In headless mode, save the game to that slot after the clicks and
                          the scene changes (as Game.SaveGame does)
    --load <slot>         In headless mode, load that slot before the first frame (as
                          Game.LoadGame does); the game's own scripts are replaced by the
                          saved ones
    --help                Show this message

--click, --click-at, --right-click-at, --key, --hold-key, --type and --wait
are steps: they run in the order given, --after frames apart, starting one
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

/// A step of the headless run, in command-line order.
enum Step {
    /// Raise an event on a scene node by name (`node` or `node:event`).
    Node(String),
    ClickAt(i32, i32),
    RightClickAt(i32, i32),
    /// Press and release a key.
    Key(i32),
    /// Hold a key down for that many frames (`--after` when `None`).
    HoldKey(i32, Option<u64>),
    /// Run that many frames.
    Wait(u64),
    Type(String),
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
    steps: Vec<Step>,
    hover_at: Option<(i32, i32)>,
    after: u64,
    then_scenes: Vec<String>,
    entities: bool,
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
    let mut steps = Vec::new();
    let mut hover_at = None;
    let mut after = 3;
    let mut then_scenes = Vec::new();
    let mut entities = false;
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
            "--click-at" => {
                let v = value(&mut i)?;
                let (x, y) = v
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .ok_or_else(|| format!("--click-at expects <x>,<y>, got `{v}`"))?;
                steps.push(Step::ClickAt(x, y));
            }
            "--type" => steps.push(Step::Type(value(&mut i)?.clone())),
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
                let code = v
                    .parse()
                    .map_err(|_| format!("--key expects a key code, got `{v}`"))?;
                steps.push(Step::Key(code));
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
        steps,
        hover_at,
        after,
        then_scenes,
        entities,
        save,
        load,
    })
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
    if vfs.packages().is_empty() && !vfs.exists(luma_adventure::SETTINGS_FILE) {
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
                let (x, y, right) = match step {
                    Step::ClickAt(x, y) => (*x, *y, false),
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
                    Step::Key(code) => {
                        println!("[{tick}] key {code} pressed");
                        runtime.key_down(*code, None, false, false, false);
                        runtime.key_up(*code);
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
                    Step::Type(text) => {
                        println!("[{tick}] typing {text:?}");
                        for ch in text.chars() {
                            let code = i32::from(ch.to_ascii_uppercase() as u8);
                            runtime.key_down(code, Some(ch), false, false, false);
                            runtime.key_up(code);
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
    if options.entities {
        if let Some(view) = runtime.world.scene() {
            for r in view.entity_refs() {
                if let Some(e) = view.entity(r) {
                    println!(
                        "entity {:?}: {}, sprite {:?}, caption {:?}, alpha {:?}",
                        e.name,
                        if e.active { "active" } else { "inactive" },
                        e.sprite.as_deref().unwrap_or("none"),
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
    for id in runtime.world.window_ids() {
        if let Some(control) = runtime.world.control(*id) {
            let mode = control.window().map(|w| w.mode);
            println!(
                "window {:?}: {}{}, at ({}, {}) {}x{}, {} control(s){}",
                control.name,
                if control.visible { "visible" } else { "hidden" },
                match mode {
                    Some(luma_adventure::WindowMode::Exclusive) => ", exclusive",
                    Some(luma_adventure::WindowMode::SystemExclusive) => ", system exclusive",
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
    }
    for channel in 0..luma_adventure::MUSIC_CHANNELS {
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
            sentence.text, sentence.font, sentence.x, sentence.y, sentence.width
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
    luma_render::window::run(
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
    let player = GamePlayer {
        runtime,
        frame_limit,
        size: PhysicalSize::new(settings.width, settings.height),
        modifiers: ModifiersState::empty(),
        started: false,
    };
    luma_render::window::run(
        WindowConfig {
            title: if caption.is_empty() { name } else { caption },
            width: settings.width,
            height: settings.height,
            cursor_visible: false,
            ..WindowConfig::default()
        },
        player,
    )
    .map_err(|e| e.to_string())
}

/// The game running in a window.
struct GamePlayer {
    runtime: Runtime,
    frame_limit: Option<u64>,
    /// The drawable area, for the mouse mapping.
    size: PhysicalSize<u32>,
    modifiers: ModifiersState,
    started: bool,
}

impl GamePlayer {
    /// A window position in game pixels, through the letterbox.
    fn to_game(&self, x: f64, y: f64) -> (i32, i32) {
        let settings = self.runtime.world.settings();
        let (w, h) = (settings.width as f64, settings.height as f64);
        let (vx, vy, vw, vh) = letterbox(w as f32, h as f32, self.size.width, self.size.height);
        if vw <= 0.0 || vh <= 0.0 {
            return (0, 0);
        }
        (
            ((x - f64::from(vx)) * w / f64::from(vw)).floor() as i32,
            ((y - f64::from(vy)) * h / f64::from(vh)).floor() as i32,
        )
    }

    /// The Windows virtual key code of a key, as scripts compare
    /// `Keyboard.KeyCode` (the engine's codes on its home platform):
    /// letters as upper-case ASCII, digits and punctuation as themselves,
    /// named keys by their `VK_` value.
    fn key_code(key: &Key) -> Option<i32> {
        Some(match key {
            Key::Character(text) => {
                let c = text.chars().next()?;
                if c.is_ascii_alphabetic() {
                    i32::from(c.to_ascii_uppercase() as u8)
                } else if c.is_ascii() {
                    i32::from(c as u8)
                } else {
                    return None;
                }
            }
            Key::Named(named) => match named {
                NamedKey::Backspace => 8,
                NamedKey::Tab => 9,
                NamedKey::Enter => 13,
                NamedKey::Shift => 16,
                NamedKey::Control => 17,
                NamedKey::Alt => 18,
                NamedKey::Pause => 19,
                NamedKey::CapsLock => 20,
                NamedKey::Escape => 27,
                NamedKey::Space => 32,
                NamedKey::PageUp => 33,
                NamedKey::PageDown => 34,
                NamedKey::End => 35,
                NamedKey::Home => 36,
                NamedKey::ArrowLeft => 37,
                NamedKey::ArrowUp => 38,
                NamedKey::ArrowRight => 39,
                NamedKey::ArrowDown => 40,
                NamedKey::Insert => 45,
                NamedKey::Delete => 46,
                NamedKey::F1 => 112,
                NamedKey::F2 => 113,
                NamedKey::F3 => 114,
                NamedKey::F4 => 115,
                NamedKey::F5 => 116,
                NamedKey::F6 => 117,
                NamedKey::F7 => 118,
                NamedKey::F8 => 119,
                NamedKey::F9 => 120,
                NamedKey::F10 => 121,
                NamedKey::F11 => 122,
                NamedKey::F12 => 123,
                _ => return None,
            },
            _ => return None,
        })
    }
}

impl App for GamePlayer {
    fn init(&mut self, renderer: &Renderer) -> luma_render::Result<()> {
        eprintln!("adapter: {}", renderer.gpu().describe());
        Ok(())
    }

    fn event(&mut self, event: &WindowEvent) -> Flow {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = self.to_game(position.x, position.y);
                self.runtime.mouse_move(x, y);
            }
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => self.runtime.mouse_left_down(),
                (MouseButton::Left, ElementState::Released) => self.runtime.mouse_left_up(),
                (MouseButton::Right, ElementState::Pressed) => self.runtime.mouse_right_down(),
                (MouseButton::Right, ElementState::Released) => self.runtime.mouse_right_up(),
                _ => {}
            },
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state,
                        text,
                        repeat,
                        ..
                    },
                ..
            } => {
                let Some(code) = GamePlayer::key_code(logical_key) else {
                    return Flow::Continue;
                };
                match state {
                    ElementState::Pressed if !repeat => {
                        let printable = text
                            .as_ref()
                            .and_then(|t| t.chars().next())
                            .filter(|c| !c.is_control());
                        self.runtime.key_down(
                            code,
                            printable,
                            self.modifiers.shift_key(),
                            self.modifiers.alt_key(),
                            self.modifiers.control_key(),
                        );
                    }
                    ElementState::Released => self.runtime.key_up(code),
                    _ => {}
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    fn draw<'a>(&'a mut self, frame: &Frame<'a>, list: &mut DrawList<'a>) -> Flow {
        self.size = frame.size;
        if !self.started {
            self.started = true;
            self.runtime.start();
        }
        // The engine advances its timer by at most a second per frame.
        let dt = (frame.dt.as_millis() as u64).clamp(1, 1000);
        for line in self.runtime.tick(dt) {
            eprintln!("{line}");
        }
        if self.runtime.world.quit_requested() {
            return Flow::Exit;
        }
        self.runtime.world.upload(frame.renderer);
        let settings = self.runtime.world.settings();
        list.width = settings.width as f32;
        list.height = settings.height as f32;
        list.letterbox = true;
        self.runtime.draw(list, Some(frame.renderer.white()));
        match self.frame_limit {
            Some(limit) if frame.frame_index + 1 >= limit => Flow::Exit,
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
    fn init(&mut self, renderer: &Renderer) -> luma_render::Result<()> {
        eprintln!("adapter: {}", renderer.gpu().describe());
        let view = load(&self.vfs, renderer, &self.scene)
            .map_err(|reason| luma_render::Error::Render { reason })?;
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
