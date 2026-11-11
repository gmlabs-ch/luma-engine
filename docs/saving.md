# Saved games

How Luma Engine saves a game and loads it back, as implemented by
`luma-adventure` (`save.rs`) and `luma-script` (`snapshot.rs`). The
rules follow the original engine (Wintermute Engine 1.x's `BGame`,
`BPersistMgr`, `ScScript` and `ScEngine`, MIT) so that games save and
load where their scripts expect; the file format is Luma's own.

## What a saved game is

As in the original engine, a saved game is the **whole state of the
engine**, running scripts included. The script that calls
`Game.SaveGame` is in the file, stopped right after the call: when the
game loads, that script goes on from there, as if the call had just
returned. This is how a game's save window closes itself after saving
and after loading.

A saved game holds:

- the script engine: every script and thread with its instruction
  pointer, value stack, `this` stack, scopes, call stack and state
  (running, sleeping until a time, waiting for an object or a thread,
  paused by a frozen game, persistent), the globals a script shares with
  its threads, and the engine-wide globals; programs are named by the
  file they were loaded from;
- the native objects and their handles, which stay the same after a
  load, so that every reference in a script is still good, with the
  script properties set on each and its runtime state (`ObjectState`:
  the `Movable`, `SceneIndependent` and `NonIntMouseEvents` flags, the
  extra caption slots, the forced talk animation);
  load, so that every reference in a script is still good; the script
  library objects (`new String`, `Array`, `Date`, `File`, `MemBuffer`)
  with their contents: a file open for reading keeps its content and
  position, one open for writing is closed, as in the original;
- the current scene as the scripts left it (its entities, regions and
  layers, those the scripts created at run time included, the
  animation and the sentence of each entity), the previous
  scene's name, a scene change asked and not done yet, and the states
  remembered of the persistent scenes (see
  [game-loop.md](game-loop.md#scene-state-between-visits));
- the sprites scripts edited through their sprite, frame and sub-frame
  objects (a delay, a hotspot, a frame added...), whole, in the bank
  they belong to: the scene's, an actor's or the windows'. The original
  engine writes every sprite of every object (`CBSprite::Persist`); Luma
  writes only the edited ones and reads the others from their files;
- the view: scroll offset, target, auto-scroll settings;
- the game's own entities and the attachments (`Game.CreateEntity`,
  `Game.LoadEntity`, `AddAttachment`) with their sprites' state;
- the actors with their position, direction, state, path, animation and
  sentence;
- with the `3d` feature, the 3D actors (their 3D position and angle,
  targets, paths, speeds and settings, animation names, direct-control
  modes, sentence, and their model's animation channels by set name, as
  WME's `CAdActor3DX::Persist` and `CXModel::Persist` keep them; the
  models are read again from the actor file, the files `MergeAnims`
  added merged again and the sets `UnloadAnim` removed again), their
  attachments (script object, name, bone, model file, offset and their
  model's animation channels; the model is read again), the textures
  set by script on their materials and the bone of their particle
  emitter, and the
  scene's 3D side (the geometry file, the waypoint height, the active
  camera, the lights' flags and colours and the nodes' flags by name,
  the fog; the geometry is read again); the game's `MaxShadowType` and
  shadow image. The scene's open path request is not kept: an actor
  saved while waiting for its path asks again. A build without the
  feature reads a 3D build's saved games as long as they hold no 3D
  actor and no geometry, and refuses the others with a clear message
  (see [3d-actors.md](3d-actors.md), section 8.7);
- the particle emitters of the entities and actors, with their
  parameters, sprite files, forces and every particle, live or dead
  (see [particles.md](particles.md)); their sprites are loaded again
  from the files;
- the inventory: the items as the scripts modified them, those taken
  (the game's and each object's own), the selected item, the object
  whose inventory the box shows, the box's window and scroll;
- the windows and their controls, the focused window, what the mouse
  captured, whether the game is frozen and by how many windows;
- the sounds: each object's sound and the music channels, by file,
  position, looping, volume, start position and whether they play or
  are paused, with each object's pending sound event; a sound held by a
  frozen game is saved as playing, with its position (the original
  engine saves it as stopped and it starts over after the load; see
  [audio.md](audio.md)); the crossfade in progress; the speech
  directories;
- the game's fader and the scene's, the cursors chosen, the mouse and
  the keyboard state;
- the game's own settings a script may change: name, caption,
  subtitles, talk and video skip buttons, `Interactive`, the main
  object, `TextEncoding`, `TextRTL`, `SuspendedRendering`,
  `StartupScene`, the mouse lock rectangle,
  the loading and saving screens and whether `Game.Reset` asked not to
  remember the scene left next; the game timer and the wall clock; the
  random generator.

What a saved game does **not** hold, and what happens to it:

- a video being played (`Game.PlayTheora`): a game saved from a script
  while a video plays loads without it;
- the speech sound of a sentence: the sentence goes on with its text,
  silently, for the time it had left;
- the sounds of sprite frames that were playing (a frame's `SOUND`):
  the next frame that becomes current plays its own;
- the string table: the one loaded by the running game stays (the
  original engine does not save it either; a game picks its language
  before loading);
- the caches: sprites (except the edited ones), images, textures, fonts
  and cursors are loaded again from the files the state names, like the
  original engine's `InitAfterLoad`. A file that disappeared is logged,
  as at any load.

## Saving

`Game.SaveGame(slot, description, quick)` returns `true` to the script
at once, and the file is written **after the value is on the script's
stack** (the host's `after_call` hook of the script engine): the saved
script resumes right after the call. Before the snapshot, `BeforeSave`
is raised on the game and runs to completion. When the file cannot be
written, the error is logged and the script's result becomes `false`,
as the original engine pops its `true`.

`quick` is accepted and ignored: the original engine only uses it to
skip the saving screen.

## Loading

`Game.LoadGame(slot)` schedules the load and puts the calling script to
sleep (`Sleep(0)`); the load happens at the start of the next frame,
before anything else of it, as the original engine does between two
frames. Everything the file holds replaces the world and the scripts,
including the script that called `LoadGame`, which is therefore gone.
The files the state names are loaded again, the sounds start where they
were (held by the frozen game when it was frozen), and `AfterLoad` is
raised on the game and runs to completion.

A script whose program cannot be read any more is logged and dropped,
as the original engine ends it; a script waiting for it gets `null`.

A file of another game (its `NAME` differs), a file that is not a Luma
Engine saved game, or one written by a newer layout version is refused
with an error in the log and leaves the game as it was.

## Slots and the save directory

Slots are files `save000.dsv` to `save999.dsv` (`Game.SaveGame` takes
any integer, formatted on three digits as `GetSaveSlotFilename` does)
in the **save directory**, which `Game.SaveDirectory` reads:

- the game's own, `LOCAL_SAVE_DIR` of `default.game` under the game's
  directory, `saves` by default;
- the player's own when `PERSONAL_SAVEGAMES` is on, or when the game's
  directory turns out not to be writable at the first save or slot
  query (logged as "Save directory is not writable. Switching to
  personal savegames."): the platform's application data directory
  (`~/Library/Application Support` on macOS, `%APPDATA%` on Windows,
  `$XDG_DATA_HOME` or `~/.local/share` elsewhere), then `Luma Engine`,
  then the game's caption or name with the characters `\|*?:"<>/`
  replaced by `_`, as the original engine names it.

`Game.IsSaveSlotUsed(slot)` is whether the file exists;
`Game.GetSaveSlotDescription(slot)` reads the description from the
file's header (empty when the slot is free or holds another game's
file); `Game.EmptySaveSlot(slot)` deletes the file;
`Game.MostRecentSaveSlot` is the slot saved last, kept in `recent.slot`
in the save directory (the original engine keeps it in the registry),
`-1` before any save.

## Thumbnails

`Game.StoreSaveThumbnail()` keeps a small picture of the screen for the
next saved game, and `DeleteSaveThumbnail()` drops it; a game calls the
first before opening its save menu, so that the menu is not in the
picture. The size comes from `THUMBNAIL_WIDTH` and `THUMBNAIL_HEIGHT`
of `default.game`; a game that sets none gets no thumbnail, as in the
original engine. The picture goes into the header of the next save
file only (`BPersistMgr::InitSave` destroys the cached one once used),
as a PNG, where `read_save_header` returns it.

The world does not draw by itself, so the picture is taken from the
**next frame the front-end renders**: the call records the request,
`World::wants_frame` tells the front-end to render that frame where it
can be read back, and `World::deliver_frame` receives the pixels
(`luma-player` renders the frame once more into an offscreen texture
when a request is pending). The original engine reads the screen at
the call instead, and takes a fresh picture at save time when none was
stored; Luma cannot, so a save without `StoreSaveThumbnail` before it
carries no thumbnail. A headless run renders nothing and fulfils no
request.

`Game.Screenshot(name)` and `Game.ScreenshotEx(file, width, height)`
work the same way, writing the frame to a file: `Screenshot` to
`<name>000.bmp` with the first free number (the game's name when no
name is given), `ScreenshotEx` to the file named, scaled to the size
given (the screen's when zero). A relative path is under the game's
directory, the original engine's working directory; a `.png` name
gives a PNG, any other a 24-bit BMP as the original engine writes.
Both return `true` once the request is recorded; a file that cannot be
written is logged when the frame comes.

## Loading and saving screens, loading icon

`Game.SetLoadingScreen(file, x, y)` names an image drawn while a saved
game loads, `SetSavingScreen(file, x, y)` one drawn while the game
saves; `null` removes it. The game file sets them first (`LOAD_IMAGE`,
`LOAD_IMAGE_X`, `LOAD_IMAGE_Y`, `SAVE_IMAGE`, `SAVE_IMAGE_X`,
`SAVE_IMAGE_Y`, see [formats/game.md](formats/game.md)), and both
travel with a saved game, as in the original engine. The original
engine draws them over black while it writes or reads the file, with
its progress indicator (`INDICATOR_*`, which Luma reads and ignores).
In Luma:

- the **loading screen** is drawn over black in the frame between
  `Game.LoadGame` and the load, which happens at the start of the next
  frame: the player sees it for one frame, longer when the load takes
  time;
- the **saving screen** is never seen: a save completes inside the
  frame that asks for it, with no frame drawn in between. The value is
  kept, read back and saved all the same.

`Game.DisplayLoadingIcon(sprite, x, y, persistent)` shows a sprite over
everything else (after the fader, before the messages of `Game.Msg`),
for the one frame that follows the call, or until
`Game.HideLoadingIcon()` when `persistent` is true, animating
meanwhile; the original engine deletes a non-persistent icon after its
first display too. A sprite that does not load shows nothing. The icon
is not part of a saved game.

## Private settings

`Game.RegWriteNumber(key, value)`, `RegWriteString(key, value)`,
`RegReadNumber(key, default)` and `RegReadString(key, default)` keep
values between runs of the game: its options, the language picked, a
"seen the intro" flag. The original engine stores them in the
`PrivateSettings` section of its `wme.ini`, or in the Windows registry
under the game's key. Luma stores them in `settings.ini` in the save
directory (the one of `Game.SaveDirectory`), in a `[PrivateSettings]`
section, one `key=value` per line:

```ini
[PrivateSettings]
Volume=80
Lang=fr
```

Keys compare without case, as Windows reads them. A read of a key never
written returns the default; `RegReadNumber` takes the leading integer
of the stored text (`GetPrivateProfileInt`), `0` when there is none. A
value is written to the file at once; a file that cannot be written is
logged and the value is lost, as the original engine reports nothing
to the script either. The section name is the original engine's, so a
game's values can be copied from its `wme.ini` by hand.

### Accessibility settings

The same file holds the player's accessibility switches, in an
`[Accessibility]` section with the keys the original engine's settings
dialog writes (`TTSEnabled`, `TTSCaptions`, `TTSTalk`, `TTSKeypress`,
`KeyboardEnabled`, `KeyboardCursorSkip`, `KeyboardPause`; `1`/`0`,
`true`/`false`, `yes`/`no`), read when the game starts; see
[game-loop.md](game-loop.md#accessibility). A key absent keeps the
default: everything off but the three text-to-speech kinds and the
cursor skip, which apply once their master switch is on.

```ini
[Accessibility]
TTSEnabled=1
KeyboardEnabled=1
```

The `AccCaption` scripts set on objects is saved with the game, like
the original's `m_AccessCaption`; the switches are not.

## File format

The format is Luma's own: the original engine's `.dsv` files serialize
its C++ objects and cannot be read without reproducing their layout. A
Luma saved game is:

| Bytes | Content |
|-------|---------|
| 8 | `LUMASAVE` |
| 4 | layout version, little-endian; `15` today |
| 4 | length of the header, little-endian |
| header | `SaveHeader` (game name, description, timestamp in seconds since the Unix epoch, optional PNG thumbnail), serialized with `postcard` |
| rest | the body, `postcard` serialized and `zlib` compressed |

The body is the engine snapshot and the world's state, serialized with
`serde` through `postcard` (a compact positional format: a saved game
of The White Chamber is a few kilobytes). The header is read alone by
`GetSaveSlotDescription`, without decompressing the body. Because
`postcard` is positional, the fields of the state are versioned by the
layout version: a change of the state's shape bumps `VERSION`, and
files of another version are then refused rather than misread (the
header still reads, so a slot of an older file keeps its description).
Version 7 added the particle emitters; version 8 the per-axis scale,
rotation and blend mode of objects and emitters, and the scene's fader;
version 9, after 8, the objects' runtime state and the content created
at run time (entities, layers, nodes, attachments, the game's own
entities, per-object inventories). Version 10, after 9, added the
animations of the controls' state images, the container entities'
`Freezable` flag and the windows' pending mode changes, the right click
kept for double clicks, and the objects' sound effects. Version 11,
after 10, added the accessibility captions (`AccCaption`) to the
objects' runtime state. Version 12, after 11, changed the cursor sprite
objects (which cursor of the game, an item or an object they stand
for) and added the items' hover image objects, `TextRTL` and
`SuspendedRendering`. Version 13, after 12, added the 3D actors, the
scene's 3D side and the game's shadow settings (the lists are empty and
the side absent in a 2D game). Version 14, after 13, added the 3D
actors' attachments, material overrides and bone emitters, and the
shadow image and light position every object keeps (`SetShadowImage`,
`SetLightPosition`). Version 15, after 14, added the start position a
sound keeps for its later plays and moved the sound event from the
sound to its object.

## Checking a game

The headless player saves and loads for validation:

```sh
# Play, open the cupboard, save to slot 5
luma-player path/to/game --headless --mute --ticks 400 \
  --scene scenes/room/room.scene --click Cupboard:Take --after 250 --save 5 --entities

# Load slot 5 before the first frame and list what came back
luma-player path/to/game --headless --mute --ticks 60 --load 5 --entities
```

`--click-at` and `--type` run in the order given, so a game's own save
window can be driven: open the menu with `--key 27`, click its "Save"
button, a slot, "Save" again, type a name and click "OK".

## Not yet

- The `savegame:` images (a saved game's thumbnail shown by a window).
- `Game.AutoSaveOnExit` and `AutoSaveSlot`.
- A video being played is not kept.
