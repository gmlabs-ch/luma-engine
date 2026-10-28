# Script engine

How Luma Engine runs a game's compiled scripts, as implemented by the
`luma-script` crate. The file format is in
[docs/formats/script.md](formats/script.md); this note is about what the
instructions do, how scripts share time, and what the engine asks of the
program embedding it. The rules follow the original engine (WME Lite's
`ScEngine`, `ScScript`, `ScValue` and `ScStack`, MIT) so that the games
behave as their authors saw them.

## Pieces

| Type | Role |
|------|------|
| `Program` | A loaded script file: tables and code, immutable, shared by the threads running it. |
| `Value` | The dynamic type of the language: `null`, bool, int, float, string, object, native handle. |
| `Script` | One execution: instruction pointer, value stack, `this` stack, scopes, call stack, state. A thread is a `Script` too. |
| `Engine` | Loads programs, keeps the scripts and the engine-wide globals, runs a tick. |
| `Host` | What the embedding program provides: files, time, logging, native objects and their methods, global functions. |

## Values and variables

Values have **value semantics**: assigning an object copies its
properties, and a property set on a copy does not reach the original.
Natives are handles: every copy names the same engine object, and the
properties scripts set on a native live in the host, next to the object.

Variables live in three places, searched in this order by every access:
the **top scope** (the locals of the function or handler being executed,
and only the top one), the **script's globals** (its top-level `var`s,
shared with the threads it spawns, plus `self` and `this`, which name the
owning object), and the **engine's globals** (`global` and `const`
declarations, shared by every script). A variable that is not found is
created as `null` in the top scope, with a warning in the log, as the
engine does; scripts rely on this for `global` declarations that come
after their first use.

`global x` creates `x` only if it does not exist, so the same declaration
repeated by every script (through a shared include) is harmless. `const`
only marks the global as surviving `clear_globals`.

A `PUSH_VAR_REF` puts a **reference** on the stack instead of a copy, so
that `obj.x = 1` and `obj.Method()` reach the variable itself. References
also fill the `this` stack. They are never stored: the engine
dereferences them before writing a value anywhere.

## Calls

For `obj.Method(a, b)` the compiler pushes `b`, then `a`, then the
argument count, then `obj` and the method name, and emits `CALL_BY_EXP`.
The engine pops the arguments and resolves the call in this order:

1. inside the thread running a script method of the same name on the
   same object, the **native** method, so that a script method can wrap
   the engine's (`method Talk() { this.Talk(...) }`);
2. a **script method** defined by a script attached to the object: a
   thread is spawned at the method's position with the arguments on its
   stack, and the caller waits for it (`WaitingScript`); when the thread
   returns a value (`RET`), the caller receives it; when the thread ends
   without one (`RET_EVENT`), the caller receives `null`;
3. a **native method** of the object, answered by the host;
4. otherwise a runtime error, and the call yields `null`.

A method call cannot spawn a thread from an unbreakable script; it is an
error there.

`EXTERNAL_CALL name` is a global function: the host is asked first, then
the engine's built-ins (`Sleep`, `WaitFor`, `Random`, `ToString`, `ToInt`,
`ToFloat`, `ToBool`, `LOG`, `Debug`, `SetScriptTimeSlice`, `MakeRGBA`,
`GetRValue` and the like), otherwise a runtime error and `null`. A
function listed in the script's externals table (a Windows DLL import) is
logged as unsupported and yields `null`.

Script functions (`CALL`) are ordinary jumps with a return address; the
callee opens a scope, fixes the argument count with `CORRECT_STACK`
(surplus arguments dropped, missing ones `null`), pops its parameters and
returns with `RET`, leaving its result on the stack.

## States and threads

A script is in one of these states:

| State | Meaning |
|-------|---------|
| `Running` | Executes until it blocks. |
| `Sleeping` | `Sleep(ms)`: until the game timer, or the wall clock while the game is frozen, passes the wake-up time. |
| `Waiting` | `WaitFor(obj)` or a native method such as `GoTo` or `Talk`: until the host says the object is ready. A vanished object ends the script. |
| `WaitingScript` | Until the method thread it called returns. |
| `Persistent` | Top-level code done; kept for its event handlers and methods. |
| `Finished`, `Error` | Removed at the end of the tick. |
| `ThreadFinished` | A thread that returned a value, kept until its caller takes it. |
| `Paused` | `pause_all` while a system window is open; `resume_all` restores the previous state. Non-freezable and persistent scripts are not paused. |

The top-level code of a script runs once when the script is loaded. Its
`on "Event"` handlers and `method`s run as **threads**: fresh scripts
sharing the parent's program and globals, started at the handler's
position when the host raises the event (`apply_event`) or a script calls
the method. A thread has its own stacks and scopes. Finishing a script
with its threads also ends the threads of the same file on the same
owner.

An **unbreakable** event (`SceneShutdown`, `BeforeSave`, `AfterLoad`) runs
its handlers to completion before `apply_event` returns; they cannot
sleep, wait or call script methods.

`WaitForExclusive`, used by walking and talking, first ends every script
already waiting for the object (and the scripts waiting for those), so a
new order cancels the previous one.

## A tick

Once per frame the host calls `Engine::tick`:

1. every waiting script whose condition is now met (object ready, time
   passed, thread returned) is set to `Running`;
2. every runnable script executes instructions until it blocks or ends;
   threads spawned during the tick run in the same tick; a script that
   asked for a **time slice** (`SetScriptTimeSlice`) is interrupted when
   its slice is used, and continues next tick;
3. finished scripts are removed and returned to the host, which forgets
   them on their owners.

## Errors

The engine never stops the game: a runtime error (division by zero,
undefined method or function, stack corruption) is logged with the
script's path and source line, and the operation yields `null` or
`false`. A corrupt program (an instruction that cannot be decoded, a
symbol index past the table) is fatal for that script only.

## Native objects

The adventure layer (`luma-adventure`) implements the `Host` trait: its
`World` owns the game, the current scene and their parts, and answers
property reads, writes and method calls for them. Scripts hold these
objects by handle; a handle to an object of a previous scene is simply
invalid, so `Game.ValidObject` says no and its properties read as `null`.

| Object | Properties | Methods |
|--------|------------|---------|
| `Game` (the engine global) | `Name`, `Caption`, `ScreenWidth`, `ScreenHeight`, `Interactive`, `MainObject`, `Scene`, `Keyboard`, `PrevScene`, `PrevSceneFilename`, `ChangingScene`, `CurrentTime`, `Subtitles`... | `ChangeScene`, `ExpandString`, `LoadStringTable`, `ValidObject`, `UnloadObject`, `QuitGame`, `FileExists`, `Msg`, `LOG` |
| `Scene` | `Name`, `Width`, `Height`, `NumLayers`, `MainLayer`, `OffsetX`, `OffsetY`, `MouseX`, `MouseY`, `NumFreeNodes` | `GetNode`, `GetLayer`, `GetFreeNode`, `GetRegionAt`, `IsBlockedAt`, `IsWalkableAt`, `GetScaleAt`, `GetRotationAt`, `SkipTo`, `ScrollTo`, `IsPointInViewport` |
| Layers | `Name`, `NumNodes`, `Width`, `Height`, `Main`, `CloseUp`, `Active` | `GetNode` |
| Entities | `Name`, `Caption`, `X`, `Y`, `Height`, `Active`, `Interactive`, `Scalable`, `Scale`, `RelativeScale`, `AlphaColor`, `Colorable`, `Item`, `Subtype`, `WalkToX`, `WalkToY`, `WalkToDirection`, `SaveState`, `IgnoreItems`... | `SkipTo`, `SetSprite`, `GetSprite`, `GetSpriteObject`, `Reset`, `Caption` |
| Regions | `Name`, `Active`, `Blocked`, `Decoration`, `Scale`, `AlphaColor`, `NumPoints` | `GetPoint`, `SkipTo` |
| Sprite objects | `NumFrames`, `CurrentFrame`, `Looping`, `Finished`, `Paused`, `PixelPerfect`, `Owner` | `Reset`, `Pause`, `Play` |
| Every script holder | `Type`, script-set properties | `ApplyEvent`, `CanHandleEvent`, `CanHandleMethod`, `AttachScript`, `DetachScript`, `IsScriptRunning` |

Scripts are attached by the `SCRIPT` keys of `default.game`, `.scene`,
`.entity`, layers and regions, and by `AttachScript`. Changing scene
raises `SceneShutdown` (unbreakable) on the old scene, ends the scripts
of its objects, loads the new scene and attaches its scripts in file
order: the scene's first, then each layer's and its nodes'. The change
itself happens at the start of the next frame, as in the engine.

What later milestones implement is accepted and logged once as "not
implemented": sound and music, `Talk` and `PlayAnim`, inventory,
windows (`LoadWindow` returns an inert object that keeps the properties
scripts set, so that game scripts run through), actors (`LoadActor`
likewise), saving, videos and transitions.

## Not yet

- The `String`, `Array`, `Date`, `File` and `MemBuffer` classes: `new`
  returns an inert object.
- Persistent scene state (`PERSISTENT_STATE`) across scene changes.
- Saving and restoring running scripts.
- The debugger protocol (`DBG_LINE` is read but only used for messages).
