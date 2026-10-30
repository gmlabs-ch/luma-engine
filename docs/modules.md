# The core and the modules

How Luma Engine is cut into crates, and how a game, a front-end or a
module of another genre builds on it. The model is Bevy's: a light core
that every game needs, and optional modules that a compiled game only
includes when it enables them.

## One crate to depend on: `luma`

`crates/luma` is the engine as a game sees it. It re-exports the six
core crates under short names and the modules behind Cargo features:

| Path | Crate | Always there? |
|------|-------|---------------|
| `luma::archive` | `luma-archive`: `.dcp` packages, the virtual file system | yes |
| `luma::formats` | `luma-formats`: definition files, sprites, scenes, actors, fonts, images | yes |
| `luma::script` | `luma-script`: the virtual machine for compiled scripts | yes |
| `luma::render` | `luma-render`: the 2D renderer on wgpu, the window | yes |
| `luma::audio` | `luma-audio`: music, sound effects, voice | yes |
| `luma::video` | `luma-video`: Ogg Theora videos | yes |
| `luma::adventure` | `luma-adventure`: the point-and-click adventure game as Wintermute runs it | feature `adventure`, on by default |

Two more features configure the core rather than add a module:
`audio-device` plays the sounds on the default output device (without
it they run silently on the game clock, which the tests and the headless
player rely on); `serde` serializes the core's definitions and the
script engine (the `adventure` module turns it on, for saved games).

A front-end writes, in its `Cargo.toml`:

```toml
luma = { version = "0.0.1", features = ["adventure", "audio-device"] }
```

and uses `luma::adventure::Runtime`, `luma::render::window::run` and so
on. `luma-player` is built this way. A game that needs none of the
adventure behaviour depends on `luma` with `default-features = false`
and gets the core alone; the CI checks that this builds.

## Where code goes

- The **core** implements the engine itself: the file formats, the
  script semantics, rendering, sound, video. It knows nothing of a kind
  of game: no walking, inventory, dialogue or scene-change logic there.
- A **module** implements the behaviour of a kind of game or a
  capability that only some games need. `luma-adventure` is the first;
  MIDI input, a management or simulation layer, mini-games would each be
  their own crate. A module depends on the core, never the reverse, and
  the core crates never depend on each other in a cycle.
- A new **official module** is a workspace crate plus a feature of
  `luma` that enables it, off by default unless every game needs it, a
  row in the workspace table of `README.md` and a line in the table
  above.
- A **community module** lives in its own repository and depends on
  `luma` (usually without `adventure`). It gets everything through the
  public API: a module of another genre implements the script engine's
  `luma::script::Host` over its own world, as `luma-adventure` does, and
  drives the renderer, the sounds and the videos through their public
  types. When a module needs something the core does not expose, the
  answer is a public API in the core, not access to internals.

## Not yet

- A second module, to prove the public API carries a genre other than
  the adventure game without touching the core.
- Features for the parts of the core a game may not need (the video
  decoder, the TrueType rasterizer).
