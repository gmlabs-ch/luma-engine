# Port inventory

Luma Engine is a port of the Wintermute Engine 1.x. These tables list
**every function and every definition-file key of the original engine's
core**, one row each, with what Luma does about it. They exist so that
the port is complete by construction, whether or not a game we test
happens to use a function: a row is closed by reading the original
function and Luma's, not by watching a game run.

[compatibility.md](../compatibility.md) is the same idea for the
scripting interface (what a game's scripts can call). It covers one
surface of the engine; these tables cover all of it: how the files are
loaded, what happens every frame, how things are drawn, saved and
played.

## Where the list comes from

`inventory.py` reads the MIT-licensed sources
(`src/engine_core/wme_base` and `src/engine_core/wme_ad` of
<https://github.com/MnemonicWME/wintermute-engine>) and writes one row
per function definition (`Class::Method`) and one per key of a
`TOKEN_TABLE` block (`Class::Method KEY`: a key that function reads from
or writes to a definition file). Nothing is chosen by hand, and the
script keeps the status and the note of the rows already there, so it
can be run again at any time:

```
python3 docs/port/inventory.py <wintermute-engine checkout>
python3 docs/port/inventory.py --check
```

## Statuses

| Status | Meaning |
|---|---|
| `unreviewed` | Nobody has compared it with Luma yet. |
| `done` | Luma does the same; the note says where (crate, file, function). |
| `partial` | Luma does part of it; the note says what is missing. |
| `todo` | Luma does not do it and should. |
| `replaced` | Luma reaches the same result another way (for example the Direct3D renderer by `luma-render`); the note says how. |
| `n/a` | It has no meaning outside the original program (a Windows dialog, the editor's hooks, the debugger); the note says why. |

A row is `done` only when the function was read in the original and in
Luma and they agree on every branch, not when a game shows the right
thing. A key is `done` only when Luma reads it **and** acts on it as the
original does. `partial` and `todo` rows are the work that remains.

## Counts

<!-- counts -->
| Table | Functions | Keys | unreviewed | done | partial | todo | replaced | n/a |
|---|---|---|---|---|---|---|---|---|
| [The adventure game: scenes, actors, entities, items, dialogue](adventure.md) | 564 | 358 | 922 | 0 | 0 | 0 | 0 | 0 |
| [Windows, settings dialogs, hardware detection, plugins, network, debugger](platform.md) | 332 | 0 | 332 | 0 | 0 | 0 | 0 | 0 |
| [Renderers, surfaces, 3D models and scenes](render.md) | 544 | 13 | 557 | 0 | 0 | 0 | 0 | 0 |
| [Sound and video](sound-video.md) | 166 | 0 | 166 | 0 | 0 | 0 | 0 | 0 |
| [Windows and controls, fonts, text](ui-text.md) | 176 | 174 | 350 | 0 | 0 | 0 | 0 | 0 |
| [The script engine, its classes, saved games](script.md) | 406 | 3 | 409 | 0 | 0 | 0 | 0 | 0 |
| [Packages, files, the definition-file parser, utilities](files.md) | 123 | 0 | 123 | 0 | 0 | 0 | 0 | 0 |
| [The game, its objects, sprites, regions, effects](game.md) | 400 | 124 | 524 | 0 | 0 | 0 | 0 | 0 |
| **Total** | **2711** | **672** | **3383** | **0** | **0** | **0** | **0** | **0** |
<!-- /counts -->
