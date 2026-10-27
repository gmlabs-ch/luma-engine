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
| 0 | Workspace, licensing and CI | 🚧 In progress |
| 1 | Read and unpack WME game packages (`luma list`, `luma unpack`) | Planned |
| 2 | Display a static scene | Planned |
| 3 | Run compiled game scripts | Planned |
| 4 | A complete game playable from start to finish on macOS | Planned |
| 5 | Playable in the browser, then on iOS | Planned |

## Workspace

| Crate | Purpose |
|-------|---------|
| `luma-archive` | Game packages (`.dcp`) and a virtual file system |
| `luma-formats` | Definition files, sprites, scenes, fonts and images |
| `luma-script` | Virtual machine for compiled game scripts |
| `luma-render` | 2D/2.5D renderer on wgpu |
| `luma-audio` | Music, sound effects and voice |
| `luma-adventure` | Actors, scenes, inventory, dialogue, pathfinding, saves |
| `luma-cli` | The `luma` command-line tool |
| `luma-player` | The desktop player |

## Building

You need a recent stable Rust toolchain ([rustup](https://rustup.rs)).

```sh
cargo build --release
./target/release/luma help
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

Luma Engine is a [GMLabs](https://gmlabs.ch) project. It is not affiliated
with Wintermute Engine, Dead:Code Software, or the authors of the games it can
run.
