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
  load, so that every reference in a script is still good;
- the current scene as the scripts left it (its entities, regions and
  layers, the animation and the sentence of each entity), the previous
  scene's name, a scene change asked and not done yet, and the states
  remembered of the persistent scenes (see
  [game-loop.md](game-loop.md#scene-state-between-visits));
- the view: scroll offset, target, auto-scroll settings;
- the actors with their position, direction, state, path, animation and
  sentence;
- the inventory: the items as the scripts modified them, those taken,
  the selected item, the box's window and scroll;
- the windows and their controls, the focused window, what the mouse
  captured, whether the game is frozen and by how many windows;
- the sounds: each object's sound and the music channels, by file,
  position, looping, volume and whether they play or are paused; the
  crossfade in progress; the speech directories;
- the fader, the cursors chosen, the mouse and the keyboard state;
- the game's own settings a script may change: name, caption,
  subtitles, talk and video skip buttons, `Interactive`, the main
  object; the game timer and the wall clock; the random generator.

What a saved game does **not** hold, and what happens to it:

- a video being played (`Game.PlayTheora`): a game saved from a script
  while a video plays loads without it;
- the speech sound of a sentence: the sentence goes on with its text,
  silently, for the time it had left;
- the string table: the one loaded by the running game stays (the
  original engine does not save it either; a game picks its language
  before loading);
- the caches: sprites, images, textures, fonts and cursors are loaded
  again from the files the state names, like the original engine's
  `InitAfterLoad`. A file that disappeared is logged, as at any load.

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

## File format

The format is Luma's own: the original engine's `.dsv` files serialize
its C++ objects and cannot be read without reproducing their layout. A
Luma saved game is:

| Bytes | Content |
|-------|---------|
| 8 | `LUMASAVE` |
| 4 | layout version, little-endian; `2` today |
| 4 | length of the header, little-endian |
| header | `SaveHeader` (game name, description, timestamp in seconds since the Unix epoch, optional PNG thumbnail), serialized with `postcard` |
| rest | the body, `postcard` serialized and `zlib` compressed |

The body is the engine snapshot and the world's state, serialized with
`serde` through `postcard` (a compact positional format: a saved game
of The White Chamber is a few kilobytes). The header is read alone by
`GetSaveSlotDescription`, without decompressing the body. Because
`postcard` is positional, the fields of the state are versioned by the
layout version: a change of the state's shape bumps `VERSION`, and
older files are then refused rather than misread. Thumbnails
(`Game.StoreSaveThumbnail`) are not stored yet.

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

- Thumbnails in the save file (`StoreSaveThumbnail`,
  `DeleteSaveThumbnail`, the `savegame:` images).
- The saving and loading screens (`SetSavingScreen`,
  `SetLoadingScreen`, `DisplayLoadingIcon`).
- `Game.AutoSaveOnExit` and `AutoSaveSlot`.
- A video being played is not kept.
