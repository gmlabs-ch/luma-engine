# Luma Engine

[![CI](https://github.com/gmlabs-ch/luma-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/gmlabs-ch/luma-engine/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**A modern narrative game engine, written in Rust, that plays Wintermute's 2D adventures and its 3D characters on painted backgrounds.**

Luma Engine plays point-and-click adventure games made with
[Wintermute Engine](https://github.com/MnemonicWME/wintermute-engine) (WME),
natively on today's machines, and aims to become a solid foundation for new
narrative games.

> **Status: alpha.** The two test games play from start to finish and every
> member of the Wintermute 1.x scripting interface is implemented. Both games
> were replayed whole with the headless player and checked frame by frame
> (every scene, menu, dialogue and video), and what that showed is fixed; the
> sound has not been checked by ear yet, nor a whole game played by hand in a
> window. Follow the milestones below.

## Why

Wintermute Engine powered hundreds of independent adventure games from 2003
onwards. Its original runtime targets Windows and Direct3D, and its portable
runtime, WME Lite, has not been maintained since 2015.

Luma Engine reads the same game data with a new engine built for today:

- **Native everywhere** — macOS (Apple Silicon first), the web, iOS, Windows
  and Linux, from a single codebase.
- **Modern rendering** — [wgpu](https://wgpu.rs), which runs on Metal on Apple
  platforms and on Vulkan, DirectX 12 or WebGPU elsewhere.
- **Memory-safe** — written in Rust, with no unsafe code outside of thin
  platform layers.
- **Ready for new games** — HD art, 2.5D characters, lighting and particles,
  beyond what 2005-era games needed.

## Milestones

| # | Milestone | Status |
|---|-----------|--------|
| 0 | Workspace, licensing and CI | ✅ Done |
| 1 | Read and unpack WME game packages (`luma list`, `luma unpack`) | ✅ Done |
| 2 | Display a static scene (`luma-player <game> --scene ...`) | ✅ Done |
| 3 | Run compiled game scripts (`luma-player <game>` plays a game) | ✅ Done |
| 4 | A complete game playable from start to finish on macOS | ✅ Done (frames checked; sound by ear and a hand-played windowed run still to do) |
| 5 | Playable in the browser, then on iOS | Planned |

## Workspace

| Crate | Purpose | Depends on |
|-------|---------|------------|
| `luma-archive` | Game packages (`.dcp`) and a virtual file system | `flate2` |
| `luma-formats` | Definition files, sprites, scenes, actors, fonts and images | `image` (`serde` with the `serde` feature) |
| `luma-script` | Virtual machine for compiled game scripts | nothing (`serde` with the `serde` feature) |
| `luma-compiler` | Compiler from script sources to the bytecode `luma-script` runs, byte for byte as the original | `luma-script` |
| `luma-render` | 2D/2.5D renderer on wgpu | `luma-formats`, `wgpu`, `winit` |
| `luma-audio` | Music, sound effects and voice | `kira` (Symphonia for decoding, `cpal` with the `device` feature) |
| `luma-video` | Ogg Theora videos decoded to frames | `ogg`, `oxideav-theora`, `oxideav-core` |
| `luma-adventure` | Actors, scenes, speech, inventory, dialogue, pathfinding, saves; 3D actors on scene geometry with its `3d` feature | the six run-time core crates, `fontdue`, `fontdb`, `serde`, `postcard`, `flate2`, `luma-3d` with the `3d` feature (`luma-compiler` for its tests) |
| `luma-lumen` | Lumen, the scenario format for games made for Luma: parser, checker, text runner | nothing |
| `luma-3d` | 3D characters on painted backgrounds: `.3ds` scene geometry, the camera mapping, `.x` models with their skin and animations, the Direct3D maths conventions | `luma-formats`, `flate2`, `serde` with the `serde` feature |
| `luma` | The engine in one crate: the core re-exported, the modules behind features | the six core crates, `luma-adventure` with the `adventure` feature, `luma-lumen` with `lumen`, `luma-3d` and `luma-adventure/3d` with `3d` |
| `luma-cli` | The `luma` command-line tool | `luma-archive`, `luma-formats`, `luma-script`, `luma-compiler`, `luma` (feature `lumen`) |
| `luma-player` | The desktop player | `luma` (features `adventure`, `audio-device`, `3d`) |

The first six are the run-time core and `luma-compiler` a build tool (games
ship compiled scripts, so `luma` does not re-export it); `luma-adventure`,
`luma-lumen` and `luma-3d` are the optional modules. A game or a front-end depends on `luma` and
enables the modules it needs through its features (`adventure` is on by
default, `lumen` and `3d` are off, `audio-device` plays the sounds,
`serde` serializes the core); see [docs/modules.md](docs/modules.md).

## Building

You need a recent stable Rust toolchain ([rustup](https://rustup.rs)).
On Linux, sound output (`luma-player`, the `device` feature of
`luma-audio`) builds against ALSA: install `libasound2-dev` and
`pkg-config` (Debian and Ubuntu names) first.

```sh
cargo build --release
./target/release/luma help
```

## Using the tools

`luma` reads a game's `.dcp` packages the way the engine does, including
the priority rules between packages:

```sh
# List every file, with its size and the package it comes from
luma list path/to/game

# Extract everything into a directory
luma unpack path/to/game path/to/output

# Build a package from a directory's files, the way the project manager does
luma pack path/to/files path/to/game/data.dcp

# Parse a definition file (scene, sprite, window, ...) and print it
luma dump path/to/game scenes/intro/intro.scene

# Describe a sprite: frames, images, sizes, hotspots
luma sprite path/to/game actors/hero/walk.sprite

# Describe a scene: layers, entities, regions, and whether their sprites exist
luma scene path/to/game scenes/room/room.scene

# Disassemble a compiled script: tables, then one line per instruction
luma script path/to/game scenes/room/scr/scene_init.script
luma script path/to/compiled.script

# Compile a script source to the engine's bytecode, exactly as the original
# compiler would (see docs/compiler.md); --game is the project root that
# #include names are relative to, and the script's name in the output
luma compile path/to/project/scripts/scene.script --game path/to/project -o scene.script
```

`luma compile` knows the functions and variables the Wintermute 1.x
adventure runtime gives every script (`Sleep`, `Random`, `Game`, `Math`,
`new Object`, ...); a game built with a plugin that adds classes names
them with `--native <name>`, once per class. Everything else a script
uses must be declared by the script or its includes, as the original
required.

For games written for Luma, `luma lumen` checks and plays a story in the
[Lumen](docs/lumen.md) scenario format, as text, without the player:

```sh
# Parse and check every .lumen file of a story, with file:line:column reports
luma lumen check story/

# Play it in the terminal, choices by number (--choose answers the first ones)
luma lumen play story/
luma lumen play story/ --start flamme --choose 1,1
```

`list` and `unpack` also accept a single `.dcp` file, and `dump` a file on
disk. The formats are documented in [docs/formats](docs/formats/README.md),
the renderer in [docs/rendering.md](docs/rendering.md), the script
engine in [docs/scripting.md](docs/scripting.md), saved games in
[docs/saving.md](docs/saving.md) and the crate layout in
[docs/modules.md](docs/modules.md).

The player shows one scene of a game, without running its scripts yet:

```sh
# Open a window on a scene (arrow keys scroll, Escape quits)
# Play a game: its scripts pick the scenes, the mouse and the keyboard go to them
luma-player path/to/game

# Play full screen, or start from a saved game
luma-player path/to/game --fullscreen
luma-player path/to/game --load 3

# Show one scene without running the game
luma-player path/to/game --scene scenes/room/room.scene

# Render the same scene to a PNG file, without a window
luma-player path/to/game --scene scenes/room/room.scene --screenshot room.png
```

The scene is given on the command line because `default.game` names no
starting scene: the game script picks it. The scripts already run, without
a window for now:

```sh
# Run the game's scripts for 60 frames and print what they did
luma-player path/to/game --headless

# The same, jumping to a scene after the game script ran
luma-player path/to/game --headless --scene scenes/room/room.scene --ticks 200

# Place the actor, raise an event on a node (here the look-at handler, which walks
# the actor there and makes it talk), then render the result
luma-player path/to/game --headless --scene scenes/room/room.scene --at 120,420 --click Scanner:LookAt --ticks 300 --screenshot talk.png

# Make the main actor say something with a given font
luma-player path/to/game --headless --scene scenes/room/room.scene --say "Hello" --font fonts/big.font --ticks 10 --screenshot hello.png

# The same without sound (the sounds still run on the game clock)
luma-player path/to/game --headless --mute

# Play in a window three times the game's resolution (by default the
# largest whole multiple that keeps the window at most 1280 points wide)
luma-player path/to/game --scale 3

# Open the game's main menu with Escape and render it, or right-click an
# object and click the verb menu that opens
luma-player path/to/game --headless --key 27 --screenshot menu.png
luma-player path/to/game --headless --scene scenes/room/room.scene --right-click-at 300,240 --click-at 300,240

# Type into the focused editor, punctuation and accents included, hold a
# key until it has repeated five times, paste with Ctrl+V
luma-player path/to/game --headless --type "Zoé, 3.5%" --repeat-key 8:5 --key ctrl+86

# Hold the right arrow down for a scene driven by Keyboard.IsKeyDown, or
# turn the mouse wheel (MouseWheelUp / MouseWheelDown on the focused window, else the game)
luma-player path/to/game --headless --scene scenes/minigame/minigame.scene --hold-key 39 --ticks 300 --entities
luma-player path/to/game --headless --key 27 --wheel down --wheel down --entities

# Move the mouse and click on the floor: the main actor walks there
luma-player path/to/game --headless --scene scenes/room/room.scene --click-at 500,520 --ticks 300 --screenshot walk.png
luma-player path/to/game --headless --scene scenes/room/room.scene --click Cupboard:Take --after 250 --then-scene scenes/hall/hall.scene --then-scene scenes/room/room.scene --entities

# Click a character where he stands at that moment, and drag a puzzle piece
luma-player path/to/game --headless --click-at 101,176 --click-on Zif --wait 700
luma-player path/to/game --headless --load 36 --drag 61,100:261,109

# Save to slot 5 after the clicks, then load it back before the first frame
luma-player path/to/game --headless --scene scenes/room/room.scene --click Cupboard:Take --after 250 --save 5
luma-player path/to/game --headless --load 5 --entities
```

## Compatibility

Luma implements the whole scripting interface of Wintermute Engine 1.x, as
inventoried in [docs/compatibility.md](docs/compatibility.md): every method,
property, function, constructor and event. Of the 1153 members, 1076 are
implemented and 77 are deliberately without effect, because they are
Windows-, debugger- or editor-only, belong to unfinished stubs of WME itself,
or belong to the legacy WME 1.0 3D actors with MS3D models, which Luma refuses
with a clear message. The behaviour follows the MIT-licensed WME 1.x sources,
quirks included.

The scripting interface is one surface of the engine. The rest (how the
definition files are loaded, what happens every frame, drawing, saving,
sound) is being checked function by function against the original sources:
[docs/port/](docs/port/README.md) lists every function and definition-file
key of the engine core with its status.

The two test games play from start to finish with the headless player, and
their frames were checked along the way. Still to do: listen to the sound,
and play a game through by hand in a window.

## Games

Luma Engine does not ship any commercial game. To play a game, you need its
data files, obtained legally from its authors. Some Wintermute games are
freeware; check each game's license before redistributing anything.

The repository does include one small game, [OpenQuest](testdata/openquest),
a two-room adventure released under CC BY 4.0: the tests and CI play it
from the opening to the ending with the headless player, and you can too:

```sh
luma-player testdata/openquest
```

## Contributing

Issues and bug reports are welcome. Please open an issue to discuss a change
before sending a pull request, and read [CONTRIBUTING.md](CONTRIBUTING.md),
in particular the clean-room rule.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Third-party credits are listed in [NOTICE](NOTICE).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

---

Luma Engine is a [GMLabs](https://gmlabs.ch) project. Contact:
[contact@gmlabs.ch](mailto:contact@gmlabs.ch). It is not affiliated
with Wintermute Engine, Dead:Code Software, or the authors of the games it can
run.
