# The core and the modules

How Luma Engine is cut into crates, and how a game, a front-end or a
module of another genre builds on it. The model is Bevy's: a light core
that every game needs, and optional modules that a compiled game only
includes when it enables them.

## One crate to depend on: `luma`

`crates/luma` is the engine as a game sees it. It re-exports the six run-time
core crates under short names and the modules behind Cargo features:

| Path | Crate | Always there? |
|------|-------|---------------|
| `luma::archive` | `luma-archive`: `.dcp` packages, the virtual file system | yes |
| `luma::formats` | `luma-formats`: definition files, sprites, scenes, actors, fonts, images | yes |
| `luma::script` | `luma-script`: the virtual machine for compiled scripts | yes |
| `luma::render` | `luma-render`: the 2D renderer on wgpu, the window | yes |
| `luma::audio` | `luma-audio`: music, sound effects, voice | yes |
| `luma::video` | `luma-video`: Ogg Theora and AVI videos | yes |
| `luma::adventure` | `luma-adventure`: the point-and-click adventure game as Wintermute runs it | feature `adventure`, on by default |
| `luma::lumen` | `luma-lumen`: the Lumen scenario format for games made for Luma (parser, checker, text runner; see [lumen.md](lumen.md)) | feature `lumen`, off by default |
| `luma::three_d` | `luma-3d`: 3D characters on painted backgrounds, as Wintermute's 3D actors (the `.3ds` scene geometry, the camera that maps scene pixels to the 3D world, the `.x` models with their skin and animation sets, the Direct3D maths conventions; see [3d-actors.md](3d-actors.md)). The feature is `3d`; the module path needs a name that can start an identifier. With `adventure` on, the feature also turns on `luma-adventure/3d`: the scenes load their `GEOMETRY`, `LoadActor3D` builds 3D actors, and their state goes into the saved games. | feature `3d`, off by default |

One more core crate is not re-exported: `luma-compiler`, the compiler
from script sources to the bytecode `luma::script` runs (see
[compiler.md](compiler.md)). It is a build tool, used by `luma compile`
and by tests; games ship compiled scripts and do not need it at run time,
so a game does not pay for it. The original engine also runs a script
file that is still source text, compiling it when it is loaded: the
feature `compiler` of `luma` (off by default, on in `luma-player`) turns
on `luma-adventure/compiler`, which answers the script engine's
`Host::compile_script` with `luma-compiler`. The script engine itself
never depends on the compiler; without the feature such a file is
refused with the original's message, `needs to be compiled but compiler
is not available`.

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
on. `luma-player` is built this way, with `3d` as well so that it plays
the games with 3D actors. A game that needs none of the
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
  `luma` (usually without `adventure`); one that only adds script
  classes or reacts to events uses the module API below, with
  `adventure`. It gets everything through the public API: a module of another genre implements the script engine's
  `luma::script::Host` over its own world, as `luma-adventure` does, and
  drives the renderer, the sounds and the videos through their public
  types. When a module needs something the core does not expose, the
  answer is a public API in the core, not access to internals.

## Adding script classes and receiving engine events: the module API

The original engine let a native Windows DLL (`wme_*.dll`) add script
classes and receive engine events. Luma cannot load those and does not
try (the plugin ABI is out of scope); what a plugin gave a game is a
**module**: Rust code in the same build, registered on the world, that
implements the `Module` trait (`luma::adventure::Module`, in
`crates/luma-adventure/src/module.rs`). The trait belongs to the
adventure crate because the engine events are the adventure game's (a
scene, a draw of the scene) and the objects live in its object table.
The script engine only gained a generic hook, `Host::call_extension`.

### How it works

- `World::register_module(Box<dyn Module>)` adds a module (through
  `Runtime::world`). It is refused when its name or one of its classes is
  malformed (a class is an identifier a script can write after `new`),
  when another module has the name or the class, or when the class is
  one the engine constructs itself (`String`, `Array`, `Date`, `File`,
  `MemBuffer`, `TcpClient`, `Object`, `Actor`, `Entity`, `Directory`,
  `Math`). At most 64 modules.
- A script runs `new Counter(5)`. The engine tries its own functions and
  built-ins first; a name nobody knows goes to `Host::call_extension`,
  which offers it to the module that provides the class, as
  `CBGame::ExternalCall` ends with `CBPluginMgr::HandleExternalCall`.
  `Module::construct` makes the object, and the script gets a native
  handle like any other (`Game.ValidObject`, `WaitFor`, `Game.UnloadObject`
  work on it). An object that nobody unloads lives as long as the world
  (Luma has no reference counts); a module has at most 4096.
- Method calls reach `Module::call_method`, property reads
  `Module::get_property` and writes `Module::set_property`. What the
  module does not answer falls to the engine's handling of an object
  (methods and properties of a plain object, script properties the
  script sets, `Ready`). `Ready` is the object's own, as in
  `CSXPluginObject::ScSetProperty`; it reads `[module object]` as a string.
- The compiler must be told the class names, as the original declares
  its plugins' classes to the compiler (`CBPluginMgr::RegisterClasses`):
  `World::module_classes()` lists them, `luma compile --native Counter`
  and `Natives::wme().function("Counter")` accept them.

### Events

An object subscribes with `ModuleContext::subscribe` (in `construct`,
`restore` or any later call; `unsubscribe` undoes it) and `Module::event`
then receives the engine's seven events (`EWmeEvent` of `wme_plugin.h`,
`ModuleEvent`), in object order:

| Event | When |
|-------|------|
| `Update` | once per frame, right after the scripts ran; not while a video plays |
| `SceneDrawBegin`, `SceneDrawEnd` | with a scene loaded, at the end of the frame's update, one after the other |
| `SceneInit` | a scene was loaded and its remembered state restored |
| `SceneShutdown` | before the scene is left, before the scripts' `SceneShutdown` |
| `GameBeforeSave` | before a game is saved, before the script's `BeforeSave` |
| `GameAfterLoad` | after a game was loaded, before the script's `AfterLoad` |

The scene events carry the scene's path in `ModuleContext::scene`; the
original hands the scene object. The draw events are not around the
drawing: Luma's `Runtime::draw` takes `&self` and draws what the update
left, so they come together at the end of the update, and a module has
nothing to draw with yet (see "Not yet"). The answer of `event` says
whether the module handled it, as the original's.

### What a module cannot do to the world

A module never receives the world. A call gets a `ModuleContext` that
holds copies (the clocks, the scene path) and collects what the module
asks for (log lines, subscriptions), applied by the engine after the
call. Across the boundary go script values with bounds: at most 32
arguments, scalars and native handles (a script object is passed as
`null` and refused as a return value), strings of at most 64 KiB,
saved state of at most 16 MiB per object. An error a module returns
becomes a script runtime error, with the script and line of the call
and the call yielding `null`; a panic does the same and **stops the
module** (its own state cannot be trusted), so every later call to it
is an error, and a save that needs it fails rather than write a wrong
state. The world's state is never half-changed by a module call, since
the module has no way to change it.

A module object is a native like the engine's: there are no reference
counts, so what a script does with a handle is its own business, and a
module that wants to know an object is unreachable must wait for
`Game.UnloadObject` (`Module::destruct`) or for its own drop.

### Saved games

A saved game keeps each module object's class, its subscriptions,
`Ready` and the bytes of `Module::save`; see [saving.md](saving.md). A
load makes the objects again with `Module::restore`, before it replaces
the world, so a class nobody provides, or a module that refuses its
bytes, refuses the load and leaves the game as it was. The module's
number for an object (`ObjectId`) is not kept: the module chooses a new
one when it restores, and must not reuse a number while the module
lives, because the objects of the world being replaced are destroyed
after the new ones were made. A module that does not support saved games
returns an error from `restore`, and games that use it cannot be loaded.
Register the modules before the scripts run, and the same modules in
the game that loads a save as in the game that wrote it.

### Writing a module

A community module is a crate that depends on `luma` and implements
the trait; the game's front-end registers it. A small one, a class
`Counter` that survives saves and counts the frames:

```rust
use luma::adventure::{
    Module, ModuleContext, ModuleError, ModuleEvent, ModuleResult, ObjectId,
};
use luma::script::Value;
use std::collections::HashMap;

#[derive(Default)]
struct Counters {
    next: ObjectId,
    values: HashMap<ObjectId, i32>,
}

impl Module for Counters {
    fn name(&self) -> &str {
        "counters"
    }

    fn classes(&self) -> Vec<String> {
        vec!["Counter".to_string()]
    }

    fn construct(&mut self, _class: &str, args: &[Value], cx: &mut ModuleContext)
        -> ModuleResult<ObjectId>
    {
        let start = match args.first() {
            Some(Value::Int(n)) => *n,
            None => 0,
            Some(_) => return Err(ModuleError::new("Counter takes a number")),
        };
        self.next += 1;
        self.values.insert(self.next, start);
        cx.subscribe(ModuleEvent::Update);
        Ok(self.next)
    }

    fn restore(&mut self, _class: &str, state: &[u8], _cx: &mut ModuleContext)
        -> ModuleResult<ObjectId>
    {
        let bytes: [u8; 4] = state.try_into().map_err(|_| ModuleError::new("bad state"))?;
        self.next += 1;
        self.values.insert(self.next, i32::from_le_bytes(bytes));
        Ok(self.next) // the subscriptions came back with the object
    }

    fn call_method(&mut self, object: ObjectId, name: &str, _args: &[Value],
                   _cx: &mut ModuleContext) -> ModuleResult<Option<Value>>
    {
        match name {
            "Get" => Ok(Some(Value::Int(self.values[&object]))),
            _ => Ok(None), // no such method: the engine reports it
        }
    }

    fn destruct(&mut self, object: ObjectId) {
        self.values.remove(&object);
    }

    fn save(&mut self, object: ObjectId) -> ModuleResult<Vec<u8>> {
        Ok(self.values[&object].to_le_bytes().to_vec())
    }

    fn event(&mut self, object: ObjectId, event: ModuleEvent, _cx: &mut ModuleContext) -> bool {
        if event == ModuleEvent::Update {
            *self.values.get_mut(&object).unwrap() += 1;
        }
        true
    }
}

// In the front-end, once the runtime is open:
// runtime.world.register_module(Box::new(Counters::default()))?;
```

A script then writes `var c = new Counter(5); ... c.Get()`. The
complete test module (properties, errors, panics, every event, saved
games) is `crates/luma-adventure/tests/modules.rs`.

## Not yet

- A module of another genre, to prove the public API carries one without
  touching the core (`luma-lumen` is a second module, but it needs nothing
  from the core yet; `luma-3d` is a second one that reaches the renderer
  without touching it: it depends on `luma-formats` only, and the adventure
  module draws its models through the public mesh API of `luma-render`,
  the 3D pass of [3d-actors.md](3d-actors.md) section 12.4).
- A way for a module to draw: the module API receives the draw events
  but has no draw list to add to, and `Runtime::draw` is read-only. When
  a module needs it, the answer is a public hook in the draw path, not
  access to the world.
- The rest of the original's plugin game interface (`IWmeGame`: reading
  and writing the world's objects from a plugin). `ModuleContext` holds
  only what the events and the tests needed; it grows when a module
  needs something, through the public API.
- Features for the parts of the core a game may not need (the video
  decoder, the TrueType rasterizer).
