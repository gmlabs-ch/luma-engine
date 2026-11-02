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
| `Host` | What the embedding program provides: files, time, logging, native objects and their methods, global functions, and a hook after each native call. |
| `ClassHost` | What the script library classes (`new String`, `Array`, `Date`, `File`, `MemBuffer`) need from the program: files to read and a place to write, the clock, new handles. |

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

`new Class(args)` compiles to `CREATE_OBJECT`, `PUSH_THIS_FROM_STACK`, the
arguments and their count, `EXTERNAL_CALL Class`, `POP_THIS`, `POP_EMPTY`:
a placeholder object is made `this`, the constructor fills it, and the
call's own result is dropped, so that the object stays on the stack as
the value of the expression. A constructor answers with
`Return::constructed`, which writes the object into the `this` slot and
yields `null`; `new Object()` is answered by the engine with nothing to
add, the placeholder being the object. A constructor nobody knows is an
undefined function: an error is logged and the placeholder stays.

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
| `Game` (the engine global) | `Name`, `Caption`, `ScreenWidth`, `ScreenHeight`, `Interactive`, `MainObject`, `ActiveObject`, `MouseX`, `MouseY`, `Scene`, `Keyboard`, `PrevScene`, `PrevSceneFilename`, `ChangingScene`, `CurrentTime`, `Subtitles`, `SubtitlesSpeed`, `TalkSkipButton`... | `ChangeScene`, `ExpandString`, `LoadStringTable`, `LoadItems`, `CreateItem`, `DeleteItem`, `LoadActor`, `ValidObject`, `UnloadObject`, `QuitGame`, `FileExists`, `Msg`, `LOG` |
| `Scene` | `Name`, `Width`, `Height`, `NumLayers`, `MainLayer`, `OffsetX`, `OffsetY`, `MouseX`, `MouseY`, `NumFreeNodes`, `AutoScroll`, `ScrollPixelsX`, `ScrollSpeedX`... | `GetNode`, `GetLayer`, `GetFreeNode`, `GetRegionAt`, `IsBlockedAt`, `IsWalkableAt`, `GetScaleAt`, `GetRotationAt`, `SkipTo`, `ScrollTo`, `ScrollToAsync`, `IsScrolling`, `IsPointInViewport`, `LoadActor`, `UnloadObject` |
| Layers | `Name`, `NumNodes`, `Width`, `Height`, `Main`, `CloseUp`, `Active` | `GetNode` |
| Entities | `Name`, `Caption`, `X`, `Y`, `Height`, `Active`, `Interactive`, `Scalable`, `Scale`, `RelativeScale`, `AlphaColor`, `Colorable`, `Item`, `Subtype`, `WalkToX`, `WalkToY`, `WalkToDirection`, `SaveState`, `IgnoreItems`... | `SkipTo`, `SetSprite`, `GetSprite`, `GetSpriteObject`, `Reset`, `Caption`, `PlayAnim`, `PlayAnimAsync` (a sprite shown in place of the entity's: one that does not loop gives the entity its own back at its end, a looping one plays on, moving the entity by its frames' `MOVE`; `PlayAnim` waits for the end, `Reset` or a sentence cuts it: talking is the entity's state then, and it is ready when the sentence ends) |
| Actors (`Game.LoadActor`, `Scene.LoadActor`) | The entities' properties, plus `Direction`, `Ready`, `ActorIsBlocked`, `TalkAnimName`, `IdleAnimName`, `WalkAnimName`, `TurnLeftAnimName`, `TurnRightAnimName` | `GoTo`, `GoToAsync`, `GoToObject`, `GoToObjectAsync`, `TurnTo`, `TurnToAsync`, `IsWalking`, `SkipTo`, `Reset`, `PlayAnim`, `PlayAnimAsync`, `SetSprite`, `GetSprite`, `HasAnim`, `MergeAnims`, `UnloadAnim`, `SetStopOnBlocked`, `GetStopOnBlocked` |
| Actors and entities, speech | `SubtitlesWidth`, `SubtitlesPosX`, `SubtitlesPosY`, `SubtitlesPosRelative`, `SubtitlesPosXCenter` | `Talk`, `TalkAsync`, `StopTalk`, `IsTalking`, `SetFont`, `GetFont` |
| `Game`, actors and entities, sound | `SoundPanning` | `LoadSound`, `PlaySound`, `PlaySoundEvent`, `StopSound`, `PauseSound`, `ResumeSound`, `IsSoundPlaying`, `SetSoundPosition`, `GetSoundPosition`, `SetSoundVolume`, `GetSoundVolume` |
| `Game`, input and fades | `Keyboard.KeyCode`, `Key`, `Printable`, `IsShift`, `IsAlt`, `IsControl` | `Keyboard.IsKeyDown`, `SetMousePos`, `FadeOut`, `FadeIn` (and their `Async` and `System` forms), `GetFadeColor`, `SetActiveCursor`, `RemoveActiveCursor`, `SetWaitCursor`, `RemoveWaitCursor`, `HasActiveCursor`, `GetActiveCursor`, `GetWaitCursor`, `HideCursor`, `ShowCursor` |
| `Math` (a global) | `PI` | `Abs`, `Acos`, `Asin`, `Atan`, `Atan2`, `Ceil`, `Cos`, `Cosh`, `Exp`, `Floor`, `Log`, `Log10`, `Pow`, `Sin`, `Sinh`, `Tan`, `Tanh`, `Sqrt`, `DegToRad`, `RadToDeg`; every one returns a float, `Floor` and `Ceil` included, as in the original engine |
| `Game`, music and volumes | | `PlayMusic`, `PlayMusicChannel`, `StopMusic`, `StopMusicChannel`, `PauseMusic`, `ResumeMusic`, `GetMusic`, `SetMusicPosition`, `GetMusicPosition`, `IsMusicPlaying`, `SetMusicVolume`, `GetMusicVolume` (and their `...Channel` forms), `MusicCrossfade`, `GetSoundLength`, `SetGlobalSFXVolume`, `SetGlobalSpeechVolume`, `SetGlobalMusicVolume`, `SetGlobalMasterVolume` and their getters, `AddSpeechDir`, `RemoveSpeechDir` |
| Windows (`Game.LoadWindow`, `Game.CreateWindow`) | `Name`, `X`, `Y`, `Width`, `Height`, `Visible`, `Disabled`, `Text`, `Parent`, `ParentNotify`, `NumControls`, `Exclusive`, `SystemExclusive`, `Menu`, `InGame`, `PauseMusic`, `Transparent`, `FadeColor`, `NextSibling`, `PrevSibling` | `GetWidget`, `GetControl`, `Close`, `GoExclusive`, `GoSystemExclusive`, `Center`, `Focus`, `LoadFromFile`, `SetImage`, `GetImage`, `SetFont`, `SetInactiveImage`, `SetInactiveFont`, `CreateButton`, `CreateStatic`, `CreateEditor`, `CreateWindow`, `DeleteControl`, `MoveAfter`, `MoveBefore`, `MoveToTop`, `MoveToBottom` |
| Buttons | The controls' properties, plus `TextAlign`, `Focusable`, `Pressed`, `PixelPerfect` | `Press`, `SetHoverImage`, `SetPressedImage`, `SetDisabledImage`, `SetFocusedImage` and their getters, `SetHoverFont`, `SetPressedFont`, `SetDisabledFont`, `SetFocusedFont` |
| Static texts and editors | `TextAlign`, `VerticalAlign`; `SelStart`, `SelEnd`, `MaxLength`, `FrameWidth`, `CursorChar`, `CursorBlinkRate` | `SizeToFit`, `HeightToFit`; `SetSelectedFont` |
| Entity containers | The controls' properties, plus `Freezable` | `GetEntity`, `SetEntity`; the entity itself has the entities' properties and methods, `SetSprite` and `Caption` included |
| Items (`ITEMS` of `default.game`) | `Name`, `Caption`, `X`, `Y`, `Amount`, `DisplayAmount`, `AmountOffsetX`, `AmountOffsetY`, `AmountAlign`, `AmountString`, `CursorCombined` | `SetSprite`, `GetSprite`, `SetHoverSprite`, `GetHoverSprite`, `SetNormalCursor`, `GetNormalCursor`, `SetHoverCursor`, `GetHoverCursor`, `SetFont`, `GetFont`, `SetCaption` |
| `Game`, dialogue | `ResponsesVisible`, `LastResponse`, `LastResponseOrig` | `AddResponse`, `AddResponseOnce`, `AddResponseOnceGame`, `GetResponse`, `GetNumResponses`, `ResetResponse`, `ClearResponses`, `StartDlgBranch`, `EndDlgBranch`, `GetCurrentDlgBranch`, `GetResponsesWindow`, `LoadResponseBox` (see [game-loop.md](game-loop.md#dialogue-responses)) |
| `Game`, videos | `VideoSubtitles`, `VideoSkipButton` | `PlayTheora`, `PlayVideo`, `StopTheora`, `StopVideo`, `IsVideoPlaying` |
| `Game`, system | `FPS`, `CurrentTime`, `WindowsTime`, `SoundAvailable`, `WindowedMode`, `AcceleratedMode`, `TextEncoding`, `SFXVolume`, `SpeechVolume`, `MusicVolume`, `MasterVolume`, `StartupScene` | `Caption`, `RunScript`, `GetFileChecksum`, `OpenDocument`, `Reset`, `LockMouseRect`, `LoadInventoryBox` (see [game-loop.md](game-loop.md#a-frame)) |
| `Game`, saved games and settings | `SaveDirectory`, `MostRecentSaveSlot` | `SaveGame`, `LoadGame`, `IsSaveSlotUsed`, `GetSaveSlotDescription`, `EmptySaveSlot`, `StoreSaveThumbnail`, `DeleteSaveThumbnail`, `Screenshot`, `ScreenshotEx`, `SetLoadingScreen`, `SetSavingScreen`, `DisplayLoadingIcon`, `HideLoadingIcon`, `RegReadNumber`, `RegReadString`, `RegWriteNumber`, `RegWriteString` (see [saving.md](saving.md)) |
| `Game`, actors and entities, inventory | `Game.SelectedItem`, `InventoryVisible`, `InventoryScrollOffset`, `NumItems`, `TotalNumItems`, `SmartItemCursor` | `TakeItem`, `DropItem`, `HasItem`, `GetItem`, `Game.IsItemTaken`, `Game.QueryItem`, `Game.GetInventoryWindow` |
| Regions | `Name`, `Active`, `Blocked`, `Decoration`, `Scale`, `AlphaColor`, `NumPoints` | `GetPoint`, `SkipTo` |
| Sprite objects (`GetSpriteObject` of an entity, in a scene or in an entity container) | `NumFrames`, `CurrentFrame`, `Looping`, `Finished`, `Paused`, `PixelPerfect`, `Owner`, `Filename` | `Reset`, `Pause`, `Play`, `GetFrame`, `AddFrame`, `InsertFrame`, `DeleteFrame` (by index or by frame object) |
| Frames (`Sprite.GetFrame`, `AddFrame`, `InsertFrame`) | `Delay`, `Keyframe`, `KillSounds`, `MoveX`, `MoveY`, `NumSubframes`, `NumEvents` | `GetSound`, `SetSound`, `GetSubframe`, `AddSubframe`, `InsertSubframe`, `DeleteSubframe`, `AddEvent`, `DeleteEvent`; a frame with a single sub-frame answers for it (`frame.MirrorX`, `frame.SetImage(...)`) |
| Particle emitters (`CreateParticleEmitter`, `ParticleEmitter` of an entity or an actor; `DeleteParticleEmitter` removes it) | `X`, `Y`, `Width`, `Height`, `Angle1`, `Angle2`, `Velocity1`, `Velocity2`, `VelocityZBased`, `Scale1`, `Scale2`, `ScaleZBased`, `LifeTime1`, `LifeTime2`, `LifeTimeZBased`, `Rotation1`, `Rotation2`, `AngVelocity1`, `AngVelocity2`, `GrowthRate1`, `GrowthRate2`, `ExponentialGrowth`, `Alpha1`, `Alpha2`, `AlphaTimeBased`, `MaxParticles`, `GenerationInterval`, `GenerationAmount`, `MaxBatches`, `FadeInTime`, `FadeOutTime`, `UseRegion`, `EmitEvent`, `NumLiveParticles` | `AddSprite`, `RemoveSprite`, `Start`, `Stop`, `Pause`, `Resume`, `SetBorder`, `SetBorderThickness`, `AddGlobalForce`, `AddPointForce`, `RemoveForce` (see [docs/particles.md](particles.md)) |
| Sub-frames (`Frame.GetSubframe`, `AddSubframe`, `InsertSubframe`) | `HotspotX`, `HotspotY`, `MirrorX`, `MirrorY`, `Decoration`, `AlphaColor`, `TransparentColor` (read only), `Is2DOnly`, `Is3DOnly` | `GetImage`, `SetImage` |
| Every script holder | `Type`, script-set properties | `ApplyEvent`, `CanHandleEvent`, `CanHandleMethod`, `AttachScript`, `DetachScript`, `IsScriptRunning` |
| `String`, `Array`, `Date`, `File`, `MemBuffer` objects | see [Script library classes](#script-library-classes) | |

When a frame of an object's sprite becomes current, its `SOUND` plays,
`FrameChanged` is raised on the sprite object (a script attached to it
with `AttachScript` can handle it) and the frame's `APPLY_EVENT`s are
raised on the object showing the sprite: an entity, an actor or a
control; the handlers run on the next frame with the other events
([docs/game-loop.md](game-loop.md#a-frame)).

Sprite, frame and sub-frame objects are handles into the sprite
definition the owner shows (see [docs/formats/sprite.md](formats/sprite.md)):
what a script changes through them, a frame's delay, a sub-frame's
hotspot or mirroring, a frame added with an image, is drawn from the
next frame on, and an edited sprite is kept in saved games
([docs/saving.md](saving.md)). A frame object keeps pointing at its frame
when frames before it are inserted or deleted; the object of a deleted
frame or sub-frame is invalid, like the engine's pointer to it. Two
differences from the original engine: a sprite file shown by several
entities of the same scene (or several controls) is one definition in
Luma, so an edit through one entity's sprite object shows on all of them,
where the engine gives each object its own copy; and
`DeleteFrame(index)` / `DeleteSubframe(index)` delete, where WME 1.x only
checks the index and deletes with a frame or sub-frame object.

Scripts are attached by the `SCRIPT` keys of `default.game`, `.scene`,
`.entity`, layers and regions, and by `AttachScript`. Changing scene
raises `SceneShutdown` (unbreakable) on the old scene, ends the scripts
of its objects, loads the new scene and attaches its scripts in file
order: the scene's first, then each layer's and its nodes'. The change
itself happens at the start of the next frame, as in the engine.

Actors (see [docs/formats/actor.md](formats/actor.md)) are updated once
per frame after the scripts ran: walks, turns and animations progress,
the view follows `Game.MainObject` when the scene's `AutoScroll` is on,
and the events the updates raise (`ActorIsBlocked` on the actor,
`ActorEntry` and `ActorLeave` on the regions of the main layer) reach
their scripts on the next frame. A script that called `GoTo`, `TurnTo`
or `PlayAnim` waits until the actor is `Ready`; `ScrollTo` waits until
the view reached its target. A scene change removes the actors the scene
loaded and resets the game's.

Speech (`Talk`, see [docs/formats/font.md](formats/font.md)) is a state
of the actor or entity: the sentence lasts its duration, the talk
sprites play stance after stance, and the text is drawn above the
object with its font (or the game's `SYSTEM_FONT`) while
`Game.Subtitles` is on. The scripts waiting for the object resume when
the sentence ends, or when a click skips it while the game is not
interactive.

Input (see [docs/game-loop.md](game-loop.md)): the mouse raises
`LeftClick`, `LeftRelease`, `RightClick`, `RightRelease` and
`LeftDoubleClick` on the game first, then on the object under the
mouse or on the scene; the object under the mouse gets `MouseEntry`
and `MouseLeave`; a key raises `Keypress` on the focused window when
its scripts handle it, else on the game. `Game.FadeOut` and `FadeIn`
make the script wait for the fade.

Windows (see [docs/formats/window.md](formats/window.md)) and their
controls are script holders of their own: a window's `SCRIPT` and its
buttons' run as any other, a button raises `Press` on itself when
released and, with `PARENT_NOTIFY`, an event named after it on its
window. `GoExclusive` and `GoSystemExclusive` make the script wait until
the window closes; the system-exclusive form freezes the game (`Frozen`),
pausing every script but the window's.

Sound (see [docs/audio.md](audio.md)) is owned by the objects: each has
one sound effect, the game has five music channels, and a sentence plays
its speech file, which then times it. The sounds advance with the game
clock when no audio device is there, so scripts behave the same in
headless runs.

The inventory (see [docs/formats/window.md](formats/window.md)) is one
list of taken items shared by the game, the actors and the entities
(the engine gives each object its own, switched with
`Game.InventoryObject`); taking an item hides the scene entities whose
`ITEM` names it, dropping it shows them again, and the selected item
draws its cursor. `Game.LoadItems(file, merge)` loads another items
file: without `merge` the items are replaced and what the inventory
held is lost, as in the original engine (a language menu does this
before anything is taken, so that the captions expand with the string
table it just loaded); with `merge` an item of the file replaces the
one of the same name. `Game.CreateItem` and `Game.DeleteItem` add and
remove single items the same way.

Videos (see [docs/video.md](video.md)) take the frame over: while one
plays, the game is frozen and no script runs, so the script that called
`PlayTheora` goes on when the video ends or is skipped.

Saved games (see [docs/saving.md](saving.md)) keep every running
script: `Game.SaveGame` writes the file once its `true` is on the
caller's stack, through the host's `after_call` hook, so that the script
resumes after the call when the game loads; `Game.LoadGame` schedules
the load for the next frame and replaces every script with the saved
ones. With the `serde` feature, `Engine::snapshot` and `Engine::restore`
do this for any host.

What later milestones implement is accepted and logged once as "not
implemented": transitions.

## Script library classes

`new String(...)`, `new Array(...)`, `new Date(...)`, `new File(...)` and
`new MemBuffer(...)` create the objects of the script library,
implemented in `luma_script::classes` after WME 1.x's `SXString`,
`SXArray`, `SXDate`, `SXFile` and `SXMemBuffer`. The host owns them like
its other natives (the adventure layer keeps them in its object table,
so they are saved with the game and never freed, where the original
frees them by reference count) and forwards property reads, writes and
method calls to the class; what the classes need from the program is the
`ClassHost` trait. Like every native they convert to `0`, `0.0` and
`false` (so `if (obj)` is false and `obj + 1` is `1.000000`); objects of
different classes compare by class name. Assigning a plain value to a
variable holding one replaces the object, as `POP_VAR` does in the
original (it copies anything but a native), so `s = "text"` leaves a
plain string.

Strings are sequences of characters: every position, length and index
counts characters, which is what the original does with a UTF-8 game
and, each byte being one character, with a Latin-1 game. Text read from
and written to files is Latin-1, as the engine reads the game's files.

| Class | Properties | Methods | Notes |
|-------|------------|---------|-------|
| `String` | `Type` (`string`), `Length`, `Capacity` (settable, never below the text and its terminator: a warning is logged) | `Substring(start, end)` (inclusive positions, swapped when reversed, clamped to the text), `Substr(start, length)` (to the end without a length; empty for a length of 0 or less), `ToUpperCase`, `ToLowerCase`, `IndexOf(text, from)` (-1 when absent; an empty text is found at `from`), `Split(separators)` (every character of the argument separates, `,` by default; an empty text gives one empty part; returns an `Array`) | `new String(text)`; `new String()` holds `[null]`, the string form of its missing argument; `new String(n)` is empty with a capacity of `n`. The string form is the text, and two `String` objects compare by text. Unknown properties read `null` and are not kept. |
| `Array` | `Type` (`array`), `Length` (settable: truncates, or grows with unset elements), `[index]` (a name of digits only: `a["007"]` is `a[7]`; `a[1.0]` or `a[-1]` is nothing) | `Push(...)` (returns the new length), `Pop()` (`null` when empty) | `new Array()`; `new Array(n)`, `n` unset elements, any single argument being a length; `new Array(a, b, ...)` holds the arguments. Elements are copies. The string form joins the elements with commas, an unset element printing nothing and a `null` one `[null]`. Unknown properties are not kept. |
| `Date` | `Type` (`date`) | `GetYear`, `GetMonth` (1 to 12), `GetDate`, `GetHours`, `GetMinutes`, `GetSeconds`, `GetWeekday` (0 is Sunday), the matching setters, `SetCurrentTime` | `new Date(y, m, d, h, mi, s)`, a missing field being 0 and every field normalised as `mktime` does, so `new Date(2020)` is 30 November 2019 and `SetMonth(13)` moves to January of the next year; `new Date()` is the host's clock. Dates compare by time. The string form is `asctime`'s, trailing newline included: `Wed Dec 31 13:30:25 2003\n`. |
| `File` | `Type` (`file`), `Filename`, `Position`, `Length`, `TextMode`, `AccessMode` (0 closed, 1 read, 2 write, 3 append); other properties are kept | `SetFilename`, `OpenAsText(mode)`, `OpenAsBinary(mode)` (mode 1 when missing; another value logs an error and reads), `Close`, `SetPosition`, `Delete`, `Copy(dest, overwrite = true)`, `ReadLine`, `ReadText(n)`, `WriteLine`, `WriteText`, `ReadBool`, `ReadByte`, `ReadShort`, `ReadInt`, `ReadLong`, `ReadFloat`, `ReadDouble`, `ReadString` and the matching `Write...` | Reading finds a file a script wrote first, then the game's files (packages and loose files); writing, deleting and copying happen under the save directory, whose sub-directories are created as needed, and a name leaving it (`..`, an absolute path) fails. A method used in the wrong mode logs a runtime error and yields `null` (reads) or `false` (writes); reading past the end yields `null`. Text is Latin-1, `\r` is dropped when reading and lines are written with `\n`; binary values are little-endian, `ReadString`/`WriteString` a 32-bit length then the bytes, and `ReadShort` is 65536 plus the signed value, as in the original. The string form is the file name, `[file object]` without one. |
| `MemBuffer` | `Type` (`membuffer`), `Size`; other properties are kept | `SetSize`, `GetBool`, `GetByte`, `GetShort` (65536 plus the signed value, as in the original), `GetInt`, `GetLong`, `GetFloat`, `GetDouble`, `GetString(pos, length)` (to the terminating zero without a length; the text stops at a zero either way), the matching setters (`SetString` writes the zero too), `GetPointer`, `SetPointer`, `DEBUG_Dump` | `new MemBuffer(n)` is `n` zero bytes, little-endian. An access outside the buffer logs `Set/Get method call is out of bounds` and yields `null` or `false`; an empty buffer has room for nothing (the original reads past its zero-length allocation). `GetPointer` and `SetPointer` deal in raw addresses for DLL plugins: they log a runtime error and yield `null` and `false`. `DEBUG_Dump` does nothing. |

Deliberate differences from the original:

- `new Date()` and `SetCurrentTime` take the host's clock, which the
  adventure layer gives in UTC: the engine carries no time-zone data.
- `ToUpperCase` and `ToLowerCase` use Unicode's simple case mapping and
  keep a character whose mapping is several characters (`ß`); the
  original uses the C library's locale.
- Files are written with `\n` line ends; the original, on Windows, wrote
  `\r\n` in text mode. Writes stay under the save directory.
- An array's string form is not capped at 32 KB, and `new Array(n)` with
  a negative `n` is empty where the original keeps the negative length.

## Not yet

- `Scene.GetWaypointGroup` and `WaypointGroup.Active`: every waypoint
  group of a scene is used.
- `GetSpriteObject` on actors.
- `.talk` files timing talk sprites to a sound, `ForceTalkAnim`, and
  `AddTalkSprite`; text in UTF-8 string tables is drawn as Latin-1.
- The echo and reverb sound effects, and streaming of long sounds.
- The debugger protocol (`DBG_LINE` is read but only used for messages).
