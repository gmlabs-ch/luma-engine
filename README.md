# Luma Engine

[![CI](https://github.com/gmlabs-ch/luma-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/gmlabs-ch/luma-engine/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**A modern 2D narrative game engine, written in Rust.**

Luma Engine plays point-and-click adventure games made with
[Wintermute Engine](https://github.com/MnemonicWME/wintermute-engine) (WME),
natively on today's machines, and aims to become a solid foundation for new
narrative games.

> **Status: pre-alpha.** Nothing is playable yet. Follow the milestones below.

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
| 3 | Run compiled game scripts (`luma-player <game>` plays a game) | In progress |
| 4 | A complete game playable from start to finish on macOS | Planned |
| 5 | Playable in the browser, then on iOS | Planned |

## Workspace

| Crate | Purpose | Depends on |
|-------|---------|------------|
| `luma-archive` | Game packages (`.dcp`) and a virtual file system | `flate2` |
| `luma-formats` | Definition files, sprites, scenes, actors, fonts and images | `image` |
| `luma-script` | Virtual machine for compiled game scripts | nothing |
| `luma-render` | 2D/2.5D renderer on wgpu | `luma-formats`, `wgpu`, `winit` |
| `luma-audio` | Music, sound effects and voice | `kira` (Symphonia for decoding, `cpal` with the `device` feature) |
| `luma-video` | Ogg Theora videos decoded to frames | `ogg`, `oxideav-theora`, `oxideav-core` |
| `luma-adventure` | Actors, scenes, speech, inventory, dialogue, pathfinding, saves | the six core crates, `fontdue` |
| `luma-cli` | The `luma` command-line tool | `luma-archive`, `luma-formats`, `luma-script` |
| `luma-player` | The desktop player | `luma-adventure`, `luma-archive`, `luma-formats`, `luma-render` |

The first six are the core; `luma-adventure` is the first optional
module, and the front-ends depend on the modules they use.

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

# Parse a definition file (scene, sprite, window, ...) and print it
luma dump path/to/game scenes/intro/intro.scene

# Describe a sprite: frames, images, sizes, hotspots
luma sprite path/to/game actors/hero/walk.sprite

# Describe a scene: layers, entities, regions, and whether their sprites exist
luma scene path/to/game scenes/room/room.scene

# Disassemble a compiled script: tables, then one line per instruction
luma script path/to/game scenes/room/scr/scene_init.script
```

`list` and `unpack` also accept a single `.dcp` file, and `dump` a file on
disk. The formats are documented in [docs/formats](docs/formats/README.md),
the renderer in [docs/rendering.md](docs/rendering.md) and the script
engine in [docs/scripting.md](docs/scripting.md).

The player shows one scene of a game, without running its scripts yet:

```sh
# Open a window on a scene (arrow keys scroll, Escape quits)
# Play a game: its scripts pick the scenes, the mouse and the keyboard go to them
luma-player path/to/game

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

# Open the game's main menu with Escape and render it, or right-click an
# object and click the verb menu that opens
luma-player path/to/game --headless --key 27 --screenshot menu.png
luma-player path/to/game --headless --scene scenes/room/room.scene --right-click-at 300,240 --click-at 300,240

# Move the mouse and click on the floor: the main actor walks there
luma-player path/to/game --headless --scene scenes/room/room.scene --click-at 500,520 --ticks 300 --screenshot walk.png
luma-player path/to/game --headless --scene scenes/room/room.scene --click Cupboard:Take --after 250 --then-scene scenes/hall/hall.scene --then-scene scenes/room/room.scene --entities
```

## Games

Luma Engine does not ship any game. To play a game, you need its data files,
obtained legally from its authors. Some Wintermute games are freeware; check
each game's license before redistributing anything.

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
