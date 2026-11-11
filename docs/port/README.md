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
| `excluded` | Left out by a decision of the maintainer; the note says which and when. Today: the Direct3D 9 HLSL material effects. |

A row is `done` only when the function was read in the original and in
Luma and they agree on every branch, not when a game shows the right
thing. A key is `done` only when Luma reads it **and** acts on it as the
original does. `partial` and `todo` rows are the work that remains.

## Counts

<!-- counts -->
| Table | Functions | Keys | unreviewed | done | partial | todo | replaced | n/a | excluded |
|---|---|---|---|---|---|---|---|---|---|
| [The adventure game: scenes, actors, entities, items, dialogue](adventure.md) | 564 | 358 | 0 | 474 | 184 | 95 | 89 | 76 | 4 |
| [Windows, settings dialogs, hardware detection, plugins, network, debugger](platform.md) | 336 | 0 | 0 | 10 | 10 | 17 | 34 | 265 | 0 |
| [Renderers, surfaces, 3D models and scenes](render.md) | 574 | 13 | 0 | 128 | 49 | 91 | 138 | 106 | 75 |
| [Sound and video](sound-video.md) | 167 | 0 | 0 | 59 | 14 | 20 | 46 | 28 | 0 |
| [Windows and controls, fonts, text](ui-text.md) | 176 | 174 | 0 | 204 | 73 | 2 | 31 | 40 | 0 |
| [The script engine, its classes, saved games](script.md) | 406 | 3 | 0 | 172 | 18 | 6 | 91 | 122 | 0 |
| [Packages, files, the definition-file parser, utilities](files.md) | 127 | 0 | 0 | 15 | 16 | 1 | 68 | 27 | 0 |
| [The game, its objects, sprites, regions, effects](game.md) | 400 | 124 | 0 | 235 | 67 | 15 | 52 | 155 | 0 |
| **Total** | **2750** | **672** | **0** | **1297** | **431** | **247** | **549** | **819** | **79** |
<!-- /counts -->
