# Compatibility with the Wintermute Engine 1.x scripting interface

This is the checklist for porting the whole of the scripting interface of Wintermute Engine (WME) 1.x to Luma Engine. It lists every method, property, global function, constructor and event that WME 1.x exposes to game scripts, and says for each one whether Luma implements it.

Reference: the MIT-licensed Wintermute Engine 1.x sources (https://github.com/MnemonicWME/wintermute-engine), under `src/engine_core/` (`wme_base`, `wme_ad`). Source paths of the form `src/engine_core/...` are relative to the root of that repository; paths starting with `luma-` are in `crates/` of this repository. No GPL code was consulted (clean-room rule in `CLAUDE.md`).

How the list was made. For every class with a script interface, the names tested by `strcmp(Name, "...")` in `ScCallMethod`, `ScGetProperty` and `ScSetProperty` were extracted (code inside comments excluded, since WME 1.x leaves some dead blocks in `SXFile`, `SXMemBuffer`, `SXTcpClient`, `AdTalkHolder`, `AdRegion` and others). The global functions and constructors come from `CBGame::ExternalCall` and `CAdGame::ExternalCall`, the extra window method from `CAdGame::WindowScriptMethodHook`. Luma was then read: the match arms of `crates/luma-adventure/src/natives*.rs`, the `*_STUBS` lists (names accepted, logged once as "not implemented" and ignored), the built-ins of `crates/luma-script/src/engine.rs`, and `World::call_function`. Luma state as of the current commit (recounted at each merge).

## States

| State | Meaning |
|-------|---------|
| **done** | Luma implements it with WME's behavior on the Luma objects named in the section header. Small known differences are in the note. |
| **todo** | Missing, stubbed (accepted and ignored), or answered with a constant. The note says what WME does, so that it can be implemented later. A property whose get works but whose set is ignored is split into two rows, one per half. |
| **no-op** | Meaningless on macOS or for a player (Windows registry, window handles, profiling, texture statistics, debugger, DLL plugins, unfinished WME stubs). The note justifies each. Luma may still accept the name so that scripts do not fail. |

## Reading the tables

- One table per WME class, in the class where WME defines the member. A derived class inherits its base class's members: `Game` has the members of `CBScriptHolder`, `CBObject`, `CBGame` and `CAdGame`; an actor those of `CBObject`, `CAdObject`, `CAdTalkHolder` and `CAdActor`; a button those of `CBObject`, `CUIObject` and `CUIButton`.
- The "Kind" column is `method`, `get`, `set`, `get/set` (a property both read and written by scripts), `event` or `var`. A row with several names counts each name separately in the totals, and a `get/set` row counts twice (one read, one write).
- A "Luma" line under each heading says which Luma objects the section covers. The base classes (`CBObject`, `CBScriptHolder`) are judged on the objects scripts use them on most: the game, entities and actors; the note says when windows, controls, regions or layers differ.
- Luma also gives scripts a few members WME 1.x does not define; they are listed at the end and not counted.

## 1. Script environment

### Global functions

- WME source: src/engine_core/wme_base/BGame.cpp (CBGame::ExternalCall, CBGame::RegisterExternals)
- Hierarchy and Luma: luma-script/src/engine.rs (`builtin`), luma-adventure/src/world.rs (`call_function`)
- Count: 20 done, 0 todo, 1 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `LOG` | function | **done** | Writes its argument to the game log (`sc: ...`). |
| `Sleep` | function | **done** | Suspends the calling script for N milliseconds. |
| `WaitFor` | function | **done** | Suspends the script until an object is ready (finished walking, talking, animating). |
| `Random` | function | **done** | Random integer in [min, max]. Luma uses its own seeded generator, so sequences differ from WME's. |
| `SetScriptTimeSlice` | function | **done** | Limits how long the calling script runs per frame before yielding. |
| `MakeRGBA`, `MakeRGB`, `RGB` | function | **done** | Packs r, g, b (and a) into the engine's 0xAARRGGBB integer. |
| `MakeHSL` | function | **done** | Builds an opaque colour integer from hue, saturation and lightness in 0..=255 (`CBUtils::HSLtoRGB`), single-precision arithmetic and byte truncation included. |
| `GetRValue`, `GetGValue`, `GetBValue`, `GetAValue` | function | **done** | Extracts one channel of a colour integer. |
| `GetHValue`, `GetSValue`, `GetLValue` | function | **done** | Extracts the hue, saturation or lightness of a colour integer, in 0..=255 (`CBUtils::RGBtoHSL`). |
| `Debug` | function | **no-op** | Hits a breakpoint in the WME script debugger when one is attached, otherwise does nothing. Luma has no debugger protocol, so it does nothing either way. |
| `ToString`, `ToInt`, `ToFloat`, `ToBool` | function | **done** | Explicit conversions with the engine's coercion rules. |

### Constructors (`new X(...)`)

- WME source: src/engine_core/wme_base/BGame.cpp, src/engine_core/wme_ad/AdGame.cpp (CAdGame::ExternalCall)
- Hierarchy and Luma: luma-adventure/src/world.rs (`call_function`: the library classes become `Kind::Class` objects answered by `luma-script/src/classes/`, `Object` a `Kind::Object` native with its scripts attached, `Actor` and `Entity` objects of the game like `Game.LoadActor`'s and `Game.CreateEntity`'s)
- Count: 9 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Object` | constructor | **done** | Generic object (CSXObject), the script files given as arguments attached to it; `Type` is `object`, `Name` reads `<unnamed>` until set, and `Game.UnloadObject` removes it. The CBObject members beyond the script holder's (`X`, `Caption`...) are plain properties on it. |
| `String` | constructor | **done** | `new String(text)`: string object with methods (see CSXString). `new String()` holds `[null]`, `new String(n)` an empty text of capacity n, as in WME. |
| `Array` | constructor | **done** | `new Array(...)`: array object (see CSXArray). Empty, n unset elements, or the arguments. |
| `Date` | constructor | **done** | `new Date()`: date/time object (see CSXDate). The current time is the host's clock, UTC in the adventure layer. |
| `File` | constructor | **done** | `new File(name)`: file access object (see CSXFile). |
| `MemBuffer` | constructor | **done** | `new MemBuffer(size)`: raw byte buffer (see CSXMemBuffer). |
| `TcpClient` | constructor | **done** | Network client; the class is an unfinished stub in WME 1.x itself (its methods are commented out). `new TcpClient(x)` gives an object as WME does (see CSXTcpClient); no socket is opened, where WME opens one it never uses. |
| `Actor` | constructor | **done** | `new Actor()`: an empty 2D actor (no file, no sprites). WME's belongs to no list and is never drawn or updated; Luma's is one of the game's own actors, like `Game.LoadActor`'s, so a sprite given to it shows and `Game.UnloadObject` removes it. `new Actor(file)` loads the file (a Luma extension; WME discards the argument). |
| `Entity` | constructor | **done** | `new Entity()`: an empty entity. WME's belongs to no list and is never drawn; Luma's is one of the game's own entities, like `Game.CreateEntity`'s, kept across scenes. `new Entity(file)` loads the file as `Game.LoadEntity` (a Luma extension; WME discards the argument). |

### Global variables

- WME source: src/engine_core/wme_base/BGame.cpp (RegisterExternals, ExtDefineVariable)
- Hierarchy and Luma: luma-script/src/engine.rs, luma-adventure/src/world.rs (Runtime::with_audio)
- Count: 5 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Game` | var | **done** | The game object (CAdGame). |
| `Math` | var | **done** | The math object (CSXMath). |
| `self`, `this` | var | **done** | The object owning the running script / the current method's target. |
| `Directory` | var | **done** | The directory object (CSXDirectory). |

### Script values (CScValue)

- WME source: src/engine_core/wme_base/ScValue.cpp
- Hierarchy and Luma: built into the VM (`luma-script/src/value.rs`)
- Count: 1 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Length (string value)` | get | **done** | Number of characters of a string value (honours the game's text encoding in WME). |

## 2. Base classes of every object

### CBScriptHolder (every script-attachable object)

- WME source: src/engine_core/wme_base/BScriptHolder.cpp
- Hierarchy and Luma: CBScriptable → CBScriptHolder; Luma: all native kinds (`holder_call`)
- Count: 10 done, 0 todo, 1 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `DEBUG_CrashMe` | method | **no-op** | Deliberately crashes the engine to test crash reporting. |
| `ApplyEvent` | method | **done** | Runs the object's handlers for a named event; returns whether one ran. |
| `CanHandleEvent` | method | **done** | True when a script attached to the object has a handler for the event. |
| `CanHandleMethod` | method | **done** | True when a script attached to the object defines the method. |
| `AttachScript` | method | **done** | Attaches a script file to the object and runs it; a file already attached under the same path (compared without case) and not finished is logged and left alone. The script takes the object's freezable flag. |
| `DetachScript` | method | **done** | Detaches the script attached under that path (optionally killing its threads). |
| `IsScriptRunning` | method | **done** | True when a script is attached under that path and neither finished nor failed. |
| `Type` | get | **done** | Class name string (`game`, `scene`, `entity`, ...). Luma returns the right value for each object it has. |
| `Name` | get | **done** | Object name: `<unnamed>` until the file's `NAME` or a script gives one (empty for a sprite and for the scene object). |
| `Name` | set | **done** | Renames the object; the value is kept as a string. |
| `Filename` | get | **done** | The file the object itself was loaded from (the game file, a scene, an actor, a window, a sprite, the response box, an entity loaded by `LoadEntity`, `AddAttachment` or a container); null for an object written inside another file or created empty. |

### CBObject (base of the game, scenes, layers, entities, actors, windows, controls, regions)

- WME source: src/engine_core/wme_base/BObject.cpp
- Hierarchy and Luma: CBScriptHolder → CBObject; Luma: game, entities, actors (sound, position, flags); controls, regions, layers have their own subsets
- Count: 72 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SkipTo` | method | **done** | Moves the object to (x, y) immediately, or to another object's position. |
| `Caption` | method | **done** | Returns a caption slot (1 to 7; 0 is 1) of any object: game, scene objects, controls, items. |
| `SetCaption` | method | **done** | Sets a caption slot (text, slot), expanded through the string table. |
| `SetCursor` | method | **done** | Sets the cursor shown when the mouse is over the object (entities, actors, controls); false when the sprite does not load. Items keep their normal / hover cursors (`SetNormalCursor`). |
| `RemoveCursor` | method | **done** | Removes the object's own cursor. |
| `GetCursor` | method | **done** | Returns the object's cursor file. |
| `GetCursorObject` | method | **done** | Returns the object's cursor as a sprite object sharing the cursor's animation. |
| `HasCursor` | method | **done** | True when the object has its own cursor. |
| `LoadSound` | method | **done** | Loads the object's sound effect without playing it. |
| `PlaySound` | method | **done** | Plays the object's sound (file, looping, loop start). |
| `PlaySoundEvent` | method | **done** | Plays a sound and raises a script event on the object when it ends. |
| `StopSound`, `PauseSound`, `ResumeSound`, `IsSoundPlaying` | method | **done** | Controls the object's sound effect. |
| `SetSoundPosition`, `GetSoundPosition` | method | **done** | Seeks / reads the sound's position in milliseconds. |
| `SetSoundVolume`, `GetSoundVolume` | method | **done** | Sets / reads the sound's volume (percent). |
| `SetShadowImage`, `GetShadowImage` | method | **done** | Sets / reads the image used for the object's 3D simple shadow; `null` clears it, a missing file fails. On every object, kept in its state and saved; only a 3D actor's shadow (and the game's default) uses it. |
| `SetLightPosition` | method | **done** | Sets the position of the shadow light relative to the object. On every object; only a 3D actor's shadow uses it. |
| `SoundFXNone` | method | **done** | Removes the effect from the object's sounds. |
| `SoundFXEcho`, `SoundFXReverb` | method | **done** | Applies an echo / reverb effect to the object's sounds, now and next, with the engine's parameters mapped onto Kira's delay and reverb (`docs/audio.md`); an approximation of the DirectSound effects. |
| `Type` | get | **done** | Class name of the object. |
| `Caption` | get/set | **done** | Object caption (expanded through the string table on set). |
| `X`, `Y` | get/set | **done** | Position in scene coordinates. |
| `Height` | get | **done** | Height of the current frame, scaled. |
| `Ready` | get | **done** | False while the object is talking, walking or playing an animation a script waits for. |
| `Movable` | get/set | **done** | Whether the engine may move the object. Kept per object; WME 1.x only reads it in `CBSprite::Draw`, which no scene object goes through, so it changes nothing on screen in either engine. |
| `Registrable` | get/set | **done** | Whether the object is registered for mouse interaction (same as `Interactive`). |
| `Interactive` | get/set | **done** | Whether the object reacts to the mouse. |
| `Zoomable`, `Scalable` | get/set | **done** | Whether the scene's scale levels apply to the object. |
| `Rotatable` | get/set | **done** | Whether the scene's rotation levels (and `Rotate`, `RelativeRotate`) apply; the sprite turns around its hotspot. |
| `AlphaColor` | get/set | **done** | Tint colour and alpha (0xAARRGGBB). |
| `BlendMode` | get/set | **done** | Blend mode (0 normal, 1 additive, 2 subtractive; another value is normal). Pipeline states of the renderer, see `docs/rendering.md`. |
| `Scale` | get/set | **done** | Absolute scale in percent (null = use the scene's scale). |
| `ScaleX`, `ScaleY` | get/set | **done** | Independent horizontal / vertical scale in percent (null = unset); when either is set they replace `Scale` and the scene's, the other axis at 100 (`CAdObject::GetScale`). The hit area follows. |
| `RelativeScale` | get/set | **done** | Scale adjustment in percent added to the scene's scale. |
| `Rotate` | get/set | **done** | Absolute rotation in degrees (null = use the scene's), applied when `Rotatable`. Luma also hit-tests the sprite where it is drawn turned; WME tests it where it would be unrotated. |
| `RelativeRotate` | get/set | **done** | Rotation added to the scene's rotation level, when `Rotatable` and no `Rotate` is set. |
| `Colorable` | get/set | **done** | Whether the scene's ambient colour tints the object. |
| `SoundPanning` | get/set | **done** | Whether the object's sound pans with its screen position. |
| `SaveState` | get/set | **done** | Whether the object is saved with the scene state (entities; actors always read true and ignore the set). |
| `NonIntMouseEvents` | get/set | **done** | Whether the object stays under the mouse while the game is non-interactive (on by default for windows, controls and container entities). |
| `AccCaption` | get/set | **done** | Caption read out by the accessibility text-to-speech in place of the caption; `null` clears it. On the game, entities, actors, items, windows, controls and `new Object`s; saved with the game. |

## 3. The Game object

### CBGame (the `Game` object, base part)

- WME source: src/engine_core/wme_base/BGame.cpp
- Hierarchy and Luma: CBObject → CBGame → CAdGame; Luma: World (`game_call`, `game_get`, `game_set`, `natives_sound.rs`, `accessibility.rs`)
- Count: 152 done, 0 todo, 13 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `LOG` | method | **done** | Writes to the log (same as the global `LOG`). |
| `Caption` | method | **done** | Returns the caption (CBObject.Caption; the game has one). WME also refreshes the window title; Luma's player sets the title once at start. |
| `Msg` | method | **done** | Shows a short on-screen message for three seconds in the system font (and logs it). |
| `RunScript` | method | **done** | Obsolete alias of `AttachScript` on the game (logs a warning). |
| `LoadStringTable` | method | **done** | Loads a string table, optionally keeping the old strings. |
| `ValidObject` | method | **done** | True when a handle still refers to a live object. |
| `Reset` | method | **done** | `ResetContent`: clears the dialogue branches, the inventory, the scene states and the "once" responses, reloads the items, resets the non-native globals; the current scene stays and is not remembered when left. |
| `UnloadObject` | method | **done** | Unloads an object created at runtime. |
| `LoadWindow` | method | **done** | Loads a window definition and returns the window. |
| `ExpandString` | method | **done** | Expands `/string-id/` references through the string table. |
| `PlayMusic`, `PlayMusicChannel` | method | **done** | Plays a music file on a channel (file, looping, loop start, volume). |
| `StopMusic`, `StopMusicChannel`, `PauseMusic`, `PauseMusicChannel`, `ResumeMusic`, `ResumeMusicChannel` | method | **done** | Stops, pauses or resumes a music channel. |
| `GetMusic`, `GetMusicChannel` | method | **done** | Returns the file playing on a channel. |
| `SetMusicPosition`, `SetMusicChannelPosition`, `SetMusicPositionChannel`, `GetMusicPosition`, `GetMusicChannelPosition` | method | **done** | Seeks / reads the playback position of a channel. |
| `IsMusicPlaying`, `IsMusicChannelPlaying` | method | **done** | True when the channel is playing. |
| `SetMusicVolume`, `SetMusicChannelVolume`, `GetMusicVolume`, `GetMusicChannelVolume` | method | **done** | Sets / reads a channel's volume. |
| `MusicCrossfade` | method | **done** | Crossfades between two music channels over a duration. |
| `GetSoundLength` | method | **done** | Duration in milliseconds of a sound file. |
| `SetMousePos` | method | **done** | Moves the mouse cursor. |
| `LockMouseRect` | method | **done** | Confines the mouse to a rectangle (edges put in order; applies while all four are non-zero, as in WME). Luma clamps the position the game sees and does not move the system cursor. |
| `PlayVideo` | method | **no-op** | Deprecated AVI player (DirectShow, Windows codecs). Luma plays an Ogg Theora file given to it as `PlayTheora` does and refuses an `.avi` with a log line and `false`; AVI decoding is out of scope (see `docs/video.md`). |
| `PlayTheora` | method | **done** | Plays an Ogg Theora video full-frame (file, type, x, y, freeze music, drop frames, subtitles). |
| `QuitGame` | method | **done** | Ends the game. |
| `RegWriteNumber`, `RegWriteString` | method | **done** | Stores a value in the game's private settings: the `PrivateSettings` section of `wme.ini` in WME, of `settings.ini` in the save directory in Luma (see `docs/saving.md`). |
| `RegReadNumber`, `RegReadString` | method | **done** | Reads a stored private setting, with a default; the number is the leading integer of the stored text, as `GetPrivateProfileInt` reads it. |
| `SaveGame` | method | **done** | Saves to a slot (slot, description, quick save); the script resumes after loading. |
| `LoadGame` | method | **done** | Loads a slot at the start of the next frame. |
| `IsSaveSlotUsed`, `GetSaveSlotDescription`, `EmptySaveSlot` | method | **done** | Inspects / clears a save slot. |
| `SetGlobalSFXVolume`, `SetGlobalSpeechVolume`, `SetGlobalMusicVolume`, `SetGlobalMasterVolume` | method | **done** | Sets the global volume of a sound class (percent). |
| `GetGlobalSFXVolume`, `GetGlobalSpeechVolume`, `GetGlobalMusicVolume`, `GetGlobalMasterVolume` | method | **done** | Reads the global volume of a sound class. |
| `SetActiveCursor`, `GetActiveCursor`, `RemoveActiveCursor`, `HasActiveCursor` | method | **done** | Sets, reads, removes the cursor used for "usable" objects (the item cursor). |
| `GetActiveCursorObject` | method | **done** | The active cursor as a sprite object, sharing the animation the cursor shows; `null` without one. |
| `FileExists` | method | **done** | True when a file exists in the game's virtual file system. |
| `FadeOut`, `FadeOutAsync`, `FadeIn`, `FadeInAsync` | method | **done** | Fades the game view to / from a colour; `Async` does not wait. |
| `SystemFadeOut`, `SystemFadeOutAsync`, `SystemFadeIn`, `SystemFadeInAsync` | method | **done** | Same, but the fade also covers system windows and keeps running while the game is frozen. |
| `GetFadeColor` | method | **done** | Colour of the last fade. |
| `Screenshot`, `ScreenshotEx` | method | **done** | Saves the screen to an image file (ScreenshotEx: at a size). Luma writes the next frame the front-end renders, not the screen at the call, and returns `true` once the request is recorded (see `docs/saving.md`). |
| `CreateWindow` | method | **done** | Creates an empty window at runtime. |
| `DeleteWindow` | method | **done** | Destroys a window created by `CreateWindow` or `LoadWindow`. |
| `OpenDocument` | method | **done** | Opens a file or URL with the system's default application (`open`, `xdg-open`, `start`). WME also minimizes a fullscreen window first. |
| `DEBUG_DumpClassRegistry` | method | **no-op** | Writes the class registry to the log; debugging aid. |
| `SetLoadingScreen` | method | **done** | Sets the image shown while a saved game loads (file, x, y; `null` removes it). Luma draws it over black in the frame between `LoadGame` and the load (see `docs/saving.md`). |
| `SetSavingScreen` | method | **no-op** | Sets the image shown while saving (file, x, y). Luma keeps and saves the value, but a save completes inside the frame that asks for it, so no frame can show the image. |
| `SetWaitCursor`, `RemoveWaitCursor`, `GetWaitCursor` | method | **done** | Sets, removes, reads the cursor shown while the game is not interactive. |
| `GetWaitCursorObject` | method | **done** | The wait cursor as a sprite object; `null` without one. |
| `ClearScriptCache` | method | **done** | Empties the compiled-script cache and returns true: running scripts keep their program, the next script started from a file reads the file again. |
| `DisplayLoadingIcon` | method | **done** | Shows a loading icon sprite (file, x, y, persistent) over the frame: one frame, or until hidden when persistent. |
| `HideLoadingIcon` | method | **done** | Hides the loading icon. |
| `DumpTextureStats` | method | **no-op** | Writes texture memory statistics to a file (D3D only); debugging aid. |
| `AccOutputText` | method | **done** | Speaks text through the accessibility text-to-speech (text, type: 0 caption, 1 talk, 2 keypress), when it is on and the type's switch allows it. The backend is macOS's `say`; elsewhere the text-to-speech is unavailable, as WME's is without a SAPI voice. |
| `IsShadowTypeSupported` | method | **done** | True when the renderer supports a 3D shadow type: the four types (0..3) with wgpu, where WME asks the device. |
| `StoreSaveThumbnail`, `DeleteSaveThumbnail` | method | **done** | Captures / discards the screenshot stored in the next save file (size from `THUMBNAIL_WIDTH/HEIGHT`). The capture is the next frame rendered; a save without a stored thumbnail carries none, where WME takes one at save time. |
| `GetFileChecksum` | method | **done** | CRC-32 of a file in the virtual file system, as integer or hex string; `null` for a missing file. |
| `EnableScriptProfiling`, `DisableScriptProfiling` | method | **done** | Turns the script profiler on / off: the real time each script file runs is added up, and turning it off writes WME's table to the log (see `docs/scripting.md`, The profiler). |
| `Type` | get | **done** | `game`. |
| `Name` | get/set | **done** | Game name. |
| `Hwnd` | get | **no-op** | Windows window handle (HWND); meaningless outside Windows. |
| `CurrentTime` | get | **done** | Game timer in milliseconds (stops while frozen). |
| `WindowsTime` | get | **done** | Real-time clock in milliseconds (timeGetTime). Luma returns the wall clock since the game started, which runs while frozen. |
| `WindowedMode` | get | **done** | True when running windowed: the front-end says so (`World::set_windowed`); the player does, headless runs read false. |
| `MouseX`, `MouseY` | get/set | **done** | Mouse position in game coordinates. |
| `MainObject` | get/set | **done** | The object the player controls and the camera follows. |
| `ActiveObject` | get | **done** | The object under the mouse. |
| `ScreenWidth`, `ScreenHeight` | get | **done** | Game resolution. |
| `Interactive` | get/set | **done** | Whether the player can interact (false in cutscenes). |
| `DebugMode` | get | **no-op** | True in debug mode (`-debug`); Luma has none and reads false. |
| `SoundAvailable` | get | **done** | True when a sound device is present (an audio device was opened). |
| `SFXVolume`, `SpeechVolume`, `MusicVolume`, `MasterVolume` | get/set | **done** | Global volumes (percent), the property form of the `SetGlobal...Volume` methods. WME logs an "obsolete" warning at every access; Luma once, on write. |
| `Keyboard` | get | **done** | The keyboard state object. |
| `Subtitles` | get/set | **done** | Show subtitles for speech. |
| `SubtitlesSpeed` | get/set | **done** | Subtitle reading speed (characters per second basis). |
| `VideoSubtitles` | get/set | **done** | Show subtitles during videos. |
| `FPS` | get | **done** | Frames of the last whole second, counted as WME does (0 during the first second). |
| `Shadows` | get/set | **done** | Obsolete alias of `MaxShadowType`: true when shadows are on; setting true means stencil, false none. |
| `SimpleShadows` | get/set | **done** | Obsolete alias of `MaxShadowType`: true when it is simple; setting true means simple, false stencil. |
| `SupportsRealTimeShadows` | get | **done** | True when the renderer can draw 3D real-time shadows; Luma reads true. |
| `MaxShadowType` | get/set | **done** | Highest shadow quality allowed (none, simple, flat, stencil), clamped to 0..3; the shadow drawn for a 3D actor is the lowest of its own, this and the scene's. Saved with the game. |
| `AcceleratedMode` | get | **done** | True when GPU accelerated; Luma reads true. |
| `Accelerated` | get | **done** | Alias of `AcceleratedMode`. |
| `TextEncoding` | get/set | **done** | Text encoding of the game: 0 ANSI, 1 UTF-8, set by each string table loaded (byte order mark) and by the script, clamped as WME does. The fonts, the `String` class, the editors and the screen reader read the game's text in this encoding (see `docs/formats/font.md`, Text encoding). |
| `TextRTL` | get/set | **done** | Right-to-left text, set by a string table's `@right-to-left` and by scripts, saved. TrueType fonts lay each line's glyphs out from the last (WME passes `ETO_RTLREADING` to GDI; Luma has no bidirectional algorithm, so mixed-direction runs are not reordered), bitmap fonts are unaffected as in WME, and editors mirror `Backspace`, `Delete`, `Home` and `End`, keep the caret before a typed character and set their text against the right edge. |
| `SoundBufferSize` | get/set | **no-op** | DirectSound buffer length in seconds; the audio backend manages its buffers. |
| `SuspendedRendering` | get/set | **done** | While true the front-end presents no frame and the window keeps the last one, as WME's `CBSubFrame::Display` draws nothing and its loop skips the flip; the game runs on. `luma-player` honours it, headless runs ignore it. Saved. |
| `SuppressScriptErrors` | get/set | **done** | While true a script runtime error only goes to the log; otherwise the quick message `Script runtime error. View log for details.` is shown too, as in WME. Saved. |
| `Frozen` | get | **done** | True while the game is frozen by a system-exclusive window. |
| `Direct3DDevice`, `DirectDrawInterface` | get | **no-op** | Raw Direct3D / DirectDraw interface pointers (Windows only). |
| `AccTTSEnabled`, `AccTTSTalk`, `AccTTSCaptions`, `AccTTSKeypress` | get | **done** | Accessibility: text-to-speech master switch and which texts it reads out (sentences without a speech file, captions of the object under the mouse, keys typed). From the `Accessibility` section of `settings.ini` (WME: of the registry, through its settings dialog). |
| `AccKeyboardEnabled`, `AccKeyboardCursorSkip`, `AccKeyboardPause` | get | **done** | Accessibility: Ctrl+Tab cycling through the clickable objects with a red frame while Ctrl is held, the cursor jumping to the object picked, Ctrl+Space pausing under a shield window. Same source. Luma moves the cursor whether or not Ctrl is still held, and does not list objects through region nodes (see `docs/game-loop.md`). |
| `UsedMemory` | get | **done** | Memory use statistic: 0, as in the release builds of WME (only its debug build measures it). |
| `MaxActiveLights` | get | **done** | Number of simultaneous 3D lights the device supports; Luma reads 8, the number its renderer lights an actor with. |
| `HardwareTL` | get | **no-op** | True when the Direct3D device does hardware transform and lighting; no meaning with wgpu. |
| `AutorunDisabled` | get/set | **no-op** | Windows CD autorun switch; no meaning on macOS. |
| `SaveDirectory` | get | **done** | Folder holding the save files. |
| `MostRecentSaveSlot` | get | **done** | Slot number of the newest save (-1 if none). |

### CAdGame (the `Game` object, adventure part)

- WME source: src/engine_core/wme_ad/AdGame.cpp
- Hierarchy and Luma: CBGame → CAdGame; Luma: World (`game_call`, `game_get`, `game_set`, `responses.rs`, `inventory.rs`)
- Count: 65 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `ChangeScene` | method | **done** | Changes scene at the start of the next frame (scene file, fade-in flag). |
| `LoadActor` | method | **done** | Loads a 2D actor from an `.actor` file and returns it. |
| `LoadActor3D` | method | **done** | Loads a 3D actor (`.act3d`, X model) kept across scenes (the `3d` feature; see `docs/3d-actors.md`). `LoadActor` does the same for an `ACTOR3DX` file, as WME sniffs the keyword. |
| `LoadEntity` | method | **done** | Loads an entity from an `.entity` file as an entity of the game, kept across scene changes (`TEMPLATE` is not followed). |
| `UnloadObject`, `UnloadActor`, `UnloadEntity`, `DeleteEntity` | method | **done** | Unloads an actor or entity loaded at runtime. |
| `UnloadActor3D` | method | **done** | Unloads a 3D actor (an alias of `UnloadObject`). |
| `CreateEntity` | method | **done** | Creates an empty entity of the game, kept across scene changes. |
| `CreateItem` | method | **done** | Creates an inventory item at runtime. |
| `DeleteItem` | method | **done** | Deletes an inventory item. |
| `QueryItem` | method | **done** | Returns an item by name or index. |
| `AddResponse`, `AddResponseOnce`, `AddResponseOnceGame` | method | **done** | Adds a dialogue choice (id, text, icons, font); `Once` hides it after use in this scene / in the whole game. |
| `ResetResponse`, `ClearResponses` | method | **done** | Re-enables a used choice / clears all choices. |
| `GetResponse` | method | **done** | Shows the choices and waits for the player's pick. |
| `GetNumResponses` | method | **done** | Number of choices currently available. |
| `StartDlgBranch`, `EndDlgBranch`, `GetCurrentDlgBranch` | method | **done** | Dialogue-branch bookkeeping used by "once" responses. |
| `TakeItem`, `DropItem`, `GetItem`, `HasItem` | method | **done** | Inventory handling for the game's own inventory. |
| `IsItemTaken` | method | **done** | True when the named item is in any object's inventory. |
| `GetInventoryWindow` | method | **done** | Returns the inventory window. |
| `GetResponsesWindow`, `GetResponseWindow` | method | **done** | Returns the dialogue responses window. |
| `LoadResponseBox` | method | **done** | Loads a different responses window definition. |
| `LoadInventoryBox` | method | **done** | Loads a different inventory box definition at runtime, replacing the current box and its window. |
| `LoadItems` | method | **done** | Loads an items file, replacing or merging. |
| `AddSpeechDir`, `RemoveSpeechDir` | method | **done** | Adds / removes a folder searched for speech files. |
| `SetSceneViewport` | method | **done** | Sets the rectangle where the scene is drawn. |
| `Type` | get | **done** | `game`. |
| `Scene` | get | **done** | The current scene. |
| `SelectedItem` | get/set | **done** | Item selected as cursor. |
| `NumItems` | get | **done** | Number of items in the inventory. |
| `TotalNumItems` | get | **done** | Number of items defined. |
| `SmartItemCursor` | get/set | **done** | Item cursor shows the item's image only over interactive objects. |
| `InventoryVisible` | get/set | **done** | Inventory window visibility. |
| `InventoryScrollOffset` | get/set | **done** | Scroll position of the inventory. |
| `ResponsesVisible` | get | **done** | True while the responses window is shown. |
| `PrevScene`, `PreviousScene`, `PrevSceneFilename`, `PreviousSceneFilename` | get | **done** | Name / file of the previous scene. |
| `LastResponse`, `LastResponseOrig` | get | **done** | Text (expanded / original) of the last chosen response. |
| `InventoryObject` | get/set | **done** | The object whose inventory the box shows (the game itself by default); the box's scroll position is kept per inventory. |
| `TalkSkipButton` | get/set | **done** | Mouse button that skips speech. |
| `VideoSkipButton` | get/set | **done** | Mouse button that skips videos. |
| `ChangingScene` | get | **done** | True while a scene change is pending. |
| `StartupScene` | get/set | **done** | Scene loaded at startup (from `default.game`); `null` when none, settable, saved with the game. |

### Window hook defined in CAdGame (`CAdGame::WindowScriptMethodHook`)

- WME source: src/engine_core/wme_ad/AdGame.cpp
- Hierarchy and Luma: adds a method to every window
- Count: 1 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `CreateEntityContainer` | method | **done** | Window method: creates an empty entity container control (UIEntity) at runtime; `SetEntity` then loads its entity. |

## 4. Scenes and adventure objects

### CAdScene

- WME source: src/engine_core/wme_ad/AdScene.cpp
- Hierarchy and Luma: CBObject → CAdScene; Luma: scene (`scene_call`, `scene_get`, `scene_set`, `scene_view.rs`)
- Count: 81 done, 0 todo, 2 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `LoadActor` | method | **done** | Loads a 2D actor into the scene. |
| `LoadActor3D` | method | **done** | Loads a 3D actor into the scene (the `3d` feature); `LoadActor` does the same for an `ACTOR3DX` file. |
| `LoadEntity` | method | **done** | Loads an entity file into the scene as a free entity (`TEMPLATE` is not followed). |
| `CreateEntity` | method | **done** | Creates an empty free entity in the scene. |
| `UnloadObject`, `UnloadActor`, `UnloadEntity`, `DeleteEntity` | method | **done** | Removes a runtime-loaded object. |
| `UnloadActor3D` | method | **done** | Removes a 3D actor (an alias of `UnloadObject`). |
| `SkipTo` | method | **done** | Jumps the view to a position or object. |
| `ScrollTo`, `ScrollToAsync` | method | **done** | Scrolls the view to a position or object, waiting or not. |
| `GetLayer` | method | **done** | Layer by index or name. |
| `GetWaypointGroup` | method | **done** | Waypoint group by index, as an object. |
| `GetNode` | method | **done** | Any node (entity or region) by name. |
| `GetFreeNode` | method | **done** | Free (layer-less) entity by index or name. |
| `GetRegionAt` | method | **done** | Region under a point (optionally including decorations). |
| `IsBlockedAt`, `IsWalkableAt` | method | **done** | Walkplane tests at a point. |
| `GetScaleAt`, `GetRotationAt` | method | **done** | Scale / rotation level at a point. |
| `IsScrolling` | method | **done** | True while the view scrolls. |
| `FadeOut`, `FadeOutAsync`, `FadeIn`, `FadeInAsync` | method | **done** | Fades only the scene viewport (not the UI), on the game timer; the scene's own fader, new with every scene. |
| `GetFadeColor` | method | **done** | Colour of the scene fade (0xAARRGGBB, the alpha as it is now). |
| `IsPointInViewport` | method | **done** | True when a point is inside the scene viewport. |
| `EnableNode3D`, `DisableNode3D`, `IsNode3DEnabled` | method | **done** | Shows / hides / queries a mesh node of the scene's 3D geometry. |
| `EnableLight`, `DisableLight`, `IsLightEnabled`, `GetLightName`, `SetLightColor`, `GetLightColor`, `GetLightPosition` | method | **done** | Controls the lights of the scene's 3D geometry, by name; `GetLightColor` packs the colour with full alpha. |
| `SetActiveCamera` | method | **done** | Selects a camera of the 3D geometry by name (a runtime error when it does not exist). |
| `SetViewport` | method | **done** | Sets the scene's own viewport (x, y, width, height; a width or height of 0 or less means the screen's), which comes before the game's `SCENE_VIEWPORT`; kept on the screen, saved with the scene, gone with it. |
| `AddLayer`, `InsertLayer`, `DeleteLayer` | method | **done** | Adds, inserts or deletes a layer at runtime (the main layer cannot be deleted); handles to the other layers and nodes stay valid. |
| `EnableFog`, `DisableFog` | method | **done** | 3D fog (colour, start, end), stored with the scene for the 3D pass, which does not draw it yet. |
| `Type` | get | **done** | `scene`. |
| `NumLayers`, `NumWaypointGroups`, `MainLayer`, `NumFreeNodes` | get | **done** | Counts and the main layer. |
| `MouseX`, `MouseY` | get | **done** | Mouse position in scene coordinates. |
| `AutoScroll` | get/set | **done** | Whether the view follows the main object. |
| `ShowGeometry` | get/set | **no-op** | Draws the walkplane / geometry as a debug overlay (editor aid). |
| `PersistentState` | get/set | **done** | Whether the scene keeps its state between visits. |
| `PersistentStateSprites` | get/set | **done** | Whether sprite state is kept too. |
| `ScrollPixelsX`, `ScrollPixelsY`, `ScrollSpeedX`, `ScrollSpeedY` | get/set | **done** | Auto-scroll step and speed. |
| `OffsetX`, `OffsetY` | get/set | **done** | Current view offset. |
| `GeometryFile` | get | **done** | The 3D geometry file, `null` without one. |
| `WaypointsHeight` | get/set | **done** | Height of the 3D waypoints over the floor; setting it drops them again. |
| `Width`, `Height` | get | **done** | Scene size in pixels. |
| `MaxShadowType` | get/set | **done** | Highest 3D shadow quality for the scene (`MAX_SHADOW_TYPE`, 2 by default), clamped to 0..3. |
| `AmbientLightColor` | get/set | **done** | 3D ambient colour (`AMBIENT_LIGHT_COLOR`); the alpha of a set value is dropped. |
| `NumLights` | get | **done** | Number of 3D lights of the geometry. |
| `Name` | set | **done** | Renames the scene. |

### CAdLayer

- WME source: src/engine_core/wme_ad/AdLayer.cpp
- Hierarchy and Luma: CBObject → CAdLayer; Luma: layers (`layer_call`)
- Count: 18 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetNode` | method | **done** | Node by index or name. |
| `AddRegion`, `InsertRegion`, `AddEntity`, `InsertEntity` | method | **done** | Adds a new region / entity to the layer, at the end or at an index. |
| `DeleteNode` | method | **done** | Deletes a node (by object or index), ending its scripts. |
| `Type` | get | **done** | `layer`. |
| `NumNodes` | get | **done** | Number of nodes. |
| `Width`, `Height` | get/set | **done** | Layer size. |
| `Main` | get | **done** | True for the main (walk) layer. |
| `CloseUp` | get/set | **done** | Close-up layers hide the layers below. |
| `Active` | get/set | **done** | Layer visibility. |
| `Name` | set | **done** | Renames the layer. |

### CBRegion

- WME source: src/engine_core/wme_base/BRegion.cpp
- Hierarchy and Luma: CBObject → CBRegion → CAdRegion; Luma: regions (`region_call`)
- Count: 11 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `AddPoint`, `InsertPoint`, `SetPoint`, `RemovePoint` | method | **done** | Edits the region's polygon; an index outside it answers false. |
| `GetPoint` | method | **done** | Returns a polygon point as `{X, Y}`. |
| `Type` | get | **done** | `ad region` (or `region`). |
| `Name` | get/set | **done** | Region name. |
| `Active` | get/set | **done** | Whether the region reacts to the mouse. |
| `NumPoints` | get | **done** | Number of polygon points. |

### CAdRegion

- WME source: src/engine_core/wme_ad/AdRegion.cpp
- Hierarchy and Luma: CBRegion → CAdRegion; Luma: regions
- Count: 11 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Type` | get | **done** | `ad region`. |
| `Name` | get/set | **done** | Region name. |
| `Blocked` | get/set | **done** | Blocks walking inside the region. |
| `Decoration` | get/set | **done** | Region ignored for mouse hits. |
| `Scale` | get/set | **done** | Fixed scale of actors inside (0 = none). |
| `AlphaColor` | get/set | **done** | Region tint. |

### CAdWaypointGroup

- WME source: src/engine_core/wme_ad/AdWaypointGroup.cpp
- Hierarchy and Luma: CBObject → CAdWaypointGroup; Luma: waypoint groups (`natives_region.rs`)
- Count: 3 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Type` | get | **done** | `waypoint-group`. |
| `Active` | get/set | **done** | Enables or disables the group for path finding. |

### CAdObject (entities, actors, items: adventure objects)

- WME source: src/engine_core/wme_ad/AdObject.cpp
- Hierarchy and Luma: CBObject → CAdObject → CAdTalkHolder / CAdObject3D; Luma: entities, actors (`entity_call`, `actor_call`)
- Count: 41 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `PlayAnim`, `PlayAnimAsync` | method | **done** | Plays a sprite once in place of the object's own, optionally waiting. |
| `Reset` | method | **done** | Returns to the idle state; stops talking and animations. |
| `IsTalking` | method | **done** | True while a sentence plays. |
| `StopTalk`, `StopTalking` | method | **done** | Cuts the current sentence. |
| `ForceTalkAnim` | method | **done** | Plays a sprite file as the first stance of the next sentence. |
| `Talk`, `TalkAsync` | method | **done** | Says a sentence (text, sound, duration, stances, align). |
| `StickToRegion` | method | **done** | Draws a free object in the slot of a region of the main layer (by object or name) whatever its position; null unsticks. |
| `SetFont`, `GetFont` | method | **done** | Sets / reads the font used for the object's speech. |
| `TakeItem`, `DropItem`, `GetItem`, `HasItem` | method | **done** | Inventory handling on the object's own inventory, created on first use. |
| `CreateParticleEmitter` | method | **done** | Creates the object's particle emitter (one per object; a second call only changes how it follows), optionally following the object at a scaled offset. See `docs/particles.md`. |
| `DeleteParticleEmitter` | method | **done** | Deletes the object's emitter; its handle becomes invalid. |
| `AddAttachment` | method | **done** | Attaches an entity file to the object (file, pre-display flag, offset), drawn before or after it, scaled and tinted with it, hit as it. |
| `RemoveAttachment` | method | **done** | Removes an attachment, by object or by name. |
| `GetAttachment` | method | **done** | Returns an attachment by index (those drawn before first) or by name. |
| `Type` | get | **done** | Class name of the object. |
| `Active` | get/set | **done** | Whether the object is shown and active. |
| `IgnoreItems` | get/set | **done** | Whether a selected item may be used on the object. |
| `SceneIndependent` | get/set | **done** | The object's position is on the screen, not in the scene: its speech is not moved by the scroll offset (the only use WME 1.x makes of it). |
| `SubtitlesWidth`, `SubtitlesPosRelative`, `SubtitlesPosX`, `SubtitlesPosY`, `SubtitlesPosXCenter` | get/set | **done** | Placement of the object's subtitles. |
| `NumItems` | get | **done** | Number of items in the object's inventory. |
| `ParticleEmitter` | get | **done** | The object's particle emitter, `null` without one. |
| `NumAttachments` | get | **done** | Number of attachments. |

### CAdTalkHolder (base of actors, entities, items)

- WME source: src/engine_core/wme_ad/AdTalkHolder.cpp
- Hierarchy and Luma: CAdObject → CAdTalkHolder; Luma: entities, actors, items
- Count: 7 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetSprite` | method | **done** | Sets the object's default sprite. |
| `GetSprite` | method | **done** | File of the current default sprite. |
| `GetSpriteObject` | method | **done** | The default sprite as a script object: an entity's sprite, the sprite `SetSprite` gave an actor, an item's image. |
| `AddTalkSprite` | method | **done** | Adds a sprite to the talk animation set (file, "ex" flag). On actors and items the file is loaded and nothing else, as in WME (an actor's own sprite sets hide the list; items do not talk). |
| `RemoveTalkSprite` | method | **done** | Removes a talk sprite (entities). |
| `SetTalkSprite` | method | **done** | Replaces the talk sprite set with one sprite (entities). |
| `Type` | get | **done** | Class name of the object. |

### CAdActor (2D walking character)

- WME source: src/engine_core/wme_ad/AdActor.cpp
- Hierarchy and Luma: CAdTalkHolder → CAdActor; Luma: actors (`natives_actor.rs`, `actor.rs`, `path.rs`)
- Count: 23 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GoTo`, `GoToAsync` | method | **done** | Walks to (x, y) along the walkplane. |
| `GoToObject`, `GoToObjectAsync` | method | **done** | Walks to another object's walk-to point. |
| `TurnTo`, `TurnToAsync` | method | **done** | Turns to a direction or toward an object. |
| `IsWalking` | method | **done** | True while walking. |
| `MergeAnims` | method | **done** | Merges the animations of another `.actor` file. |
| `UnloadAnim` | method | **done** | Unloads a named animation. |
| `HasAnim` | method | **done** | True when the actor defines a named animation. |
| `Direction` | get/set | **done** | Facing direction (0..7, 8 = none). |
| `Type` | get | **done** | `actor`. |
| `TalkAnimName`, `WalkAnimName`, `IdleAnimName`, `TurnLeftAnimName`, `TurnRightAnimName` | get/set | **done** | Names of the animation sets used for talking, walking, standing and turning. |

### CAdEntity (scene entity)

- WME source: src/engine_core/wme_ad/AdEntity.cpp
- Hierarchy and Luma: CAdTalkHolder → CAdEntity; Luma: entities (`entity_call`)
- Count: 20 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `StopSound` | method | **done** | Stops the entity's sound. |
| `PlayTheora` | method | **done** | Plays an Ogg Theora video in the entity's place (file, looping, alpha image, start time), at the entity's position, scale and sound volume; the game runs on. Not kept in saved games. |
| `StopTheora`, `IsTheoraPlaying`, `PauseTheora`, `ResumeTheora`, `IsTheoraPaused` | method | **done** | Controls the entity-bound video; the pause holds the sound too. |
| `CreateRegion` | method | **done** | Creates the entity's own clickable region (empty until points are added) and returns it. |
| `DeleteRegion` | method | **done** | Deletes that region. |
| `Type` | get | **done** | `entity`. |
| `Item` | get/set | **done** | Item the entity represents. |
| `Subtype` | get | **done** | `normal` or `sound`. |
| `WalkToX`, `WalkToY`, `WalkToDirection` | get/set | **done** | Where actors stand when walking to the entity, and which way they face. |
| `Region` | get | **done** | The entity's region object (a `CBRegion`: name, points, `Active`). |

### CAdItem (inventory item)

- WME source: src/engine_core/wme_ad/AdItem.cpp
- Hierarchy and Luma: CAdTalkHolder → CAdItem; Luma: items (`natives_item.rs`)
- Count: 26 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetHoverSprite` | method | **done** | Sets the image shown while hovering the item in the inventory. |
| `GetHoverSprite` | method | **done** | File of that image. |
| `GetHoverSpriteObject` | method | **done** | The hover image as a sprite object (frames, `CurrentFrame`, `Reset`...), with the animation the inventory box shows; `null` without one. |
| `SetNormalCursor`, `GetNormalCursor`, `SetHoverCursor`, `GetHoverCursor` | method | **done** | Sets / reads the cursor used when the item is selected (normal / over a target). |
| `GetNormalCursorObject`, `GetHoverCursorObject` | method | **done** | The item's cursors as sprite objects, sharing the animation the cursor shows; `null` without one. |
| `Type` | get | **done** | `item`. |
| `Name` | get/set | **done** | Item name. |
| `DisplayAmount` | get/set | **done** | Whether the amount is drawn on the item. |
| `Amount` | get/set | **done** | Item quantity. |
| `AmountOffsetX`, `AmountOffsetY`, `AmountAlign`, `AmountString` | get/set | **done** | Placement and text of the amount. |
| `CursorCombined` | get/set | **done** | Whether the item cursor is the item image combined with the normal cursor. |

## 5. Sprites and frames

### CBSprite (animated sprite)

- WME source: src/engine_core/wme_base/BSprite.cpp
- Hierarchy and Luma: CBScriptHolder → CBSprite; Luma: the sprite of an entity, in a scene or in an entity container (`natives_sprite.rs`); the sprite objects of actors, items and controls are in their classes' groups
- Count: 18 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetFrame` | method | **done** | Returns a frame object by index; out of range logs a runtime error and returns `null`. |
| `DeleteFrame` | method | **done** | Deletes a frame, by index or by frame object; the other frame objects follow their frames. WME 1.x only deletes when given a frame object and merely checks an index; Luma deletes in both cases. |
| `AddFrame`, `InsertFrame` | method | **done** | Adds / inserts a frame from an image file (no sub-frame without a file, or when it is missing), and returns its object. |
| `Reset` | method | **done** | Restarts the animation. |
| `Pause`, `Play` | method | **done** | Pauses / resumes the animation. |
| `Type` | get | **done** | `sprite`. |
| `NumFrames` | get | **done** | Number of frames. |
| `CurrentFrame` | get/set | **done** | Current frame index. |
| `PixelPerfect` | get/set | **done** | Whether clicks test the sprite's pixels. |
| `Looping` | get/set | **done** | Whether the animation loops. |
| `Owner` | get | **done** | The object the sprite belongs to. |
| `Finished` | get | **done** | True when a non-looping animation reached its end. |
| `Paused` | get | **done** | True while paused. |

### CBFrame (sprite frame)

- WME source: src/engine_core/wme_base/BFrame.cpp
- Hierarchy and Luma: CBScriptable → CBFrame; Luma: the frame objects of a sprite object (`natives_sprite.rs`), handles by position that follow their frame when others are inserted or deleted
- Count: 21 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetSound`, `SetSound` | method | **done** | Reads / sets the sound played when the frame shows; `SetSound` returns `false` and leaves no sound when the file cannot be loaded. |
| `GetSubframe` | method | **done** | Returns a sub-frame by index; out of range logs a runtime error and returns `null`. |
| `DeleteSubframe` | method | **done** | Deletes a sub-frame, by index or by object (WME 1.x only deletes by object, see `DeleteFrame`). |
| `AddSubframe`, `InsertSubframe` | method | **done** | Adds / inserts a sub-frame from an image (added even when the image is missing, as in WME). |
| `AddEvent`, `DeleteEvent` | method | **done** | Adds / removes a script event raised when the frame shows (names compared ignoring case, no duplicates). |
| `Type` | get | **done** | `frame`. |
| `Delay` | get/set | **done** | Frame duration in milliseconds. |
| `Keyframe` | get/set | **done** | Marks the frame as a keyframe (movement / talk sync). |
| `KillSounds` | get/set | **done** | Stops running sounds when the frame shows. |
| `MoveX`, `MoveY` | get/set | **done** | Movement applied to the owner when the frame shows. |
| `NumSubframes`, `NumEvents` | get | **done** | Counts. |

A frame with a single sub-frame answers that sub-frame's properties and methods too (`frame.MirrorX`, `frame.SetImage`), as in WME.

### CBSubFrame (image part of a frame)

- WME source: src/engine_core/wme_base/BSubFrame.cpp
- Hierarchy and Luma: CBScriptable → CBSubFrame; Luma: the sub-frame objects of a frame object (`natives_sprite.rs`)
- Count: 20 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetImage`, `SetImage` | method | **done** | Reads / replaces the image file (`null` removes it); a new image shows whole. `SetImage` returns `false` when the file is missing. |
| `Type` | get | **done** | `subframe`. |
| `AlphaColor` | get/set | **done** | Tint and alpha, `0xAARRGGBB`. |
| `TransparentColor` | get | **done** | Colour key, `0xFFRRGGBB`. |
| `Is2DOnly`, `Is3DOnly` | get/set | **done** | Which renderer draws the sub-frame; Luma never draws `2D_ONLY` ones (see `docs/formats/sprite.md`). |
| `MirrorX`, `MirrorY` | get/set | **done** | Flips the image. |
| `Decoration` | get/set | **done** | Ignored for mouse hits. |
| `HotspotX`, `HotspotY` | get/set | **done** | Hotspot (draw origin). |

## 6. Windows and controls

### CUIObject (base of windows and controls)

- WME source: src/engine_core/wme_base/UIObject.cpp
- Hierarchy and Luma: CBObject → CUIObject → CUIWindow / CUIButton / CUIText / CUIEdit / CUIEntity / CUIMarkup; Luma: `Control` (`natives_ui.rs`, `ui.rs`)
- Count: 27 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetFont` | method | **done** | Sets the control's font file. |
| `SetImage`, `GetImage` | method | **done** | Sets / reads the background image file. |
| `GetImageObject` | method | **done** | The background image as a sprite object. |
| `Focus` | method | **done** | Gives the control keyboard focus. |
| `MoveAfter`, `MoveBefore` | method | **done** | Reorders the control among its siblings relative to another one. |
| `MoveToBottom`, `MoveToTop` | method | **done** | Moves the control to the back / front of its siblings. |
| `Type` | get | **done** | `window`, `button`, `static`, `editor`, `entity container`. |
| `Name` | get/set | **done** | Control name. |
| `Parent` | get | **done** | The containing window. |
| `ParentNotify` | get/set | **done** | Raise an event named after the control on the parent when pressed or changed. |
| `Width`, `Height` | get/set | **done** | Size. |
| `Visible` | get/set | **done** | Visibility. |
| `Disabled` | get/set | **done** | Disabled state. |
| `Text` | get/set | **done** | Caption text. |
| `NextSibling`, `PrevSibling` | get | **done** | Neighbouring control in the parent's order. |

### CUIWindow

- WME source: src/engine_core/wme_base/UIWindow.cpp
- Hierarchy and Luma: CUIObject → CUIWindow; Luma: windows (`natives_ui.rs`, `ui.rs`)
- Count: 40 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetWidget`, `GetControl` | method | **done** | A child control by name or index. |
| `SetInactiveFont`, `SetInactiveImage`, `GetInactiveImage` | method | **done** | Font / image used while the window is not focused. |
| `GetInactiveImageObject` | method | **done** | The inactive image as a sprite object (`null` without one). |
| `Close` | method | **done** | Closes the window. |
| `GoExclusive` | method | **done** | Opens the window modally; the script waits until it closes. |
| `GoSystemExclusive` | method | **done** | Opens the window modally and freezes the game. |
| `Center` | method | **done** | Centers the window on screen. |
| `LoadFromFile` | method | **done** | Loads a window definition into the window. |
| `CreateButton`, `CreateStatic`, `CreateEditor`, `CreateWindow` | method | **done** | Creates a child control at runtime. |
| `DeleteControl`, `DeleteButton`, `DeleteStatic`, `DeleteEditor`, `DeleteWindow` | method | **done** | Deletes a child control. |
| `Type` | get | **done** | `window`. |
| `NumWidgets`, `NumControls` | get | **done** | Number of child controls. |
| `Exclusive`, `SystemExclusive` | get | **done** | True while modal / system-modal. |
| `Exclusive`, `SystemExclusive` | set | **done** | `true` opens the window modally (as `GoExclusive` / `GoSystemExclusive`, without making the script wait), `false` closes it and leaves it visible. The freeze or unfreeze of the system-exclusive form happens at the next frame's window update. |
| `Menu` | get/set | **done** | Window is a menu (closes when the game resumes). |
| `InGame` | get/set | **done** | Window is drawn in the game view (not as system UI). |
| `PauseMusic` | get/set | **done** | Pauses music while the window is open. |
| `ClipContents` | get/set | **done** | Clips children to the window rectangle. |
| `Transparent` | get/set | **done** | Window ignores mouse input where transparent. |
| `FadeColor` | get/set | **done** | Colour the game is faded to behind the window. |
| `Name` | set | **done** | Renames the window. |

### CUIButton

- WME source: src/engine_core/wme_base/UIButton.cpp
- Hierarchy and Luma: CUIObject → CUIButton; Luma: buttons
- Count: 26 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetDisabledFont`, `SetHoverFont`, `SetPressedFont`, `SetFocusedFont` | method | **done** | Fonts for each button state. |
| `SetDisabledImage`, `SetHoverImage`, `SetPressedImage`, `SetFocusedImage` | method | **done** | Images for each button state. |
| `GetDisabledImage`, `GetHoverImage`, `GetPressedImage`, `GetFocusedImage` | method | **done** | Image files of each state. |
| `GetDisabledImageObject`, `GetHoverImageObject`, `GetPressedImageObject`, `GetFocusedImageObject` | method | **done** | Images of each state as sprite objects (`null` for a state without one); each state image animates while shown, as the engine's sprites do. |
| `Press` | method | **done** | Presses the button from script. |
| `Type` | get | **done** | `button`. |
| `TextAlign` | get/set | **done** | Horizontal text alignment. |
| `Focusable` | get/set | **done** | Whether the button can take focus. |
| `Pressed` | get/set | **done** | Latched pressed state. |
| `PixelPerfect` | get/set | **done** | Hit-tests the button image's pixels. |

### CUIText (static text)

- WME source: src/engine_core/wme_base/UIText.cpp
- Hierarchy and Luma: CUIObject → CUIText; Luma: static controls
- Count: 7 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SizeToFit` | method | **done** | Resizes the control to fit its text. |
| `HeightToFit` | method | **done** | Resizes only the height. |
| `Type` | get | **done** | `static`. |
| `TextAlign`, `VerticalAlign` | get/set | **done** | Text alignment. |

### CUIEdit (text editor)

- WME source: src/engine_core/wme_base/UIEdit.cpp
- Hierarchy and Luma: CUIObject → CUIEdit; Luma: editor controls
- Count: 16 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetSelectedFont` | method | **done** | Font for the selected text. |
| `Type` | get | **done** | `editor`. |
| `SelStart`, `SelEnd` | get/set | **done** | Selection range. |
| `CursorBlinkRate`, `CursorChar` | get/set | **done** | Caret blink time and caret character. |
| `FrameWidth` | get/set | **done** | Width of the border around the text. |
| `MaxLength` | get/set | **done** | Maximum text length. |
| `Text` | get/set | **done** | Edited text. |

### CUIEntity (entity container inside a window)

- WME source: src/engine_core/wme_ad/UIEntity.cpp
- Hierarchy and Luma: CUIObject → CUIEntity; Luma: `ControlKind::EntityContainer`
- Count: 5 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetEntity` | method | **done** | The entity hosted by the container. |
| `SetEntity` | method | **done** | Loads an entity file into the container. |
| `Type` | get | **done** | `entity container`. |
| `Freezable` | get/set | **done** | Whether the entity's scripts pause when the game freezes; off when the container loads its entity (`MakeFreezable(false)`). |

### CUIMarkup

- WME source: src/engine_core/wme_base/UIMarkup.cpp
- Hierarchy and Luma: CUIObject → CUIMarkup; unreachable: no window file keyword creates one
- Count: 0 done, 0 todo, 1 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Type` | get | **no-op** | `html`. The class is an unfinished stub in WME 1.x; nothing in the engine core instantiates it. |

## 7. Input

### CBKeyboardState (`Game.Keyboard`)

- WME source: src/engine_core/wme_base/BKeyboardState.cpp
- Hierarchy and Luma: CBScriptable → CBKeyboardState; Luma: Kind::Keyboard
- Count: 8 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `IsKeyDown` | method | **done** | True while a key (character or code) is held. |
| `Type` | get | **done** | `keyboard`. |
| `Key` | get | **done** | Last key as text. |
| `Printable` | get | **done** | Whether the last key produced a character. |
| `KeyCode` | get | **done** | Last key's code. |
| `IsShift`, `IsAlt`, `IsControl` | get | **done** | Modifier state. |

## 8. Script library objects

### CSXMath (the global `Math`)

- WME source: src/engine_core/wme_base/SXMath.cpp
- Hierarchy and Luma: CBScriptable → CSXMath; Luma: Kind::Math (`math_call`)
- Count: 22 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Abs`, `Acos`, `Asin`, `Atan`, `Atan2`, `Ceil`, `Cos`, `Cosh`, `Exp`, `Floor`, `Log`, `Log10`, `Pow`, `Sin`, `Sinh`, `Tan`, `Tanh`, `Sqrt`, `DegToRad`, `RadToDeg` | method | **done** | The C math function of that name on floats; every result is a float (Floor and Ceil included). `Sin`, `Cos`, `Tan`, `Sinh`, `Cosh` and `Tanh` take degrees, as in WME; the inverse functions return radians. A string argument is read like `atof`. |
| `Type` | get | **done** | `math`. |
| `PI` | get | **done** | The constant pi in single precision, as in WME (3.1415927410125732). |

### CSXString (`new String`)

- WME source: src/engine_core/wme_base/SXString.cpp
- Hierarchy and Luma: CBScriptable → CSXString; Luma: Kind::Class (luma-script/src/classes/string.rs)
- Count: 10 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Substring`, `Substr` | method | **done** | Part of the string by (start, end) or (start, length), with WME's clamping and swapping; positions count characters. |
| `ToUpperCase`, `ToLowerCase` | method | **done** | Case conversion: the letters A to Z only in an ANSI game, as WME's `_strupr`/`_strlwr` in the C locale; Unicode's simple mapping in a UTF-8 game (WME: the wide-character functions in the user's locale). |
| `IndexOf` | method | **done** | Position of a substring from an index, -1 when absent. |
| `Split` | method | **done** | Splits on any of the separator characters and returns an array. |
| `Type` | get | **done** | `string`. |
| `Length` | get | **done** | Length of the string object, in characters. |
| `Capacity` | get/set | **done** | Allocated buffer size; cannot shrink below the text (warning logged). |

### CSXArray (`new Array`)

- WME source: src/engine_core/wme_base/SXArray.cpp
- Hierarchy and Luma: CBScriptable → CSXArray; Luma: Kind::Class (luma-script/src/classes/array.rs)
- Count: 5 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Push` | method | **done** | Appends one or more values; returns the new length. |
| `Pop` | method | **done** | Removes and returns the last value, `null` when empty. |
| `Type` | get | **done** | `array`. |
| `Length` | get/set | **done** | Number of elements; setting it truncates. Element access `a[n]` is a numeric property (digits only, as WME's `ValidNumber`). |

### CSXDate (`new Date`)

- WME source: src/engine_core/wme_base/SXDate.cpp
- Hierarchy and Luma: CBScriptable → CSXDate; Luma: Kind::Class (luma-script/src/classes/date.rs)
- Count: 15 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetYear`, `GetMonth`, `GetDate`, `GetHours`, `GetMinutes`, `GetSeconds`, `GetWeekday` | method | **done** | Reads a field of the date. |
| `SetYear`, `SetMonth`, `SetDate`, `SetHours`, `SetMinutes`, `SetSeconds` | method | **done** | Writes a field of the date, normalised as `mktime` does. |
| `SetCurrentTime` | method | **done** | Sets the object to the current date and time (the host's clock: UTC in the adventure layer, where WME uses local time). |
| `Type` | get | **done** | `date`. |

### CSXFile (`new File`)

- WME source: src/engine_core/wme_base/SXFile.cpp
- Hierarchy and Luma: CBScriptable → CSXFile; Luma: Kind::Class (luma-script/src/classes/file.rs). Reads come from files scripts wrote, then the game's files; writes, deletions and copies stay under the save directory.
- Count: 33 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetFilename` | method | **done** | Sets the file name (and closes the file). |
| `OpenAsText`, `OpenAsBinary` | method | **done** | Opens the file for read / write / append as text or binary. |
| `Close` | method | **done** | Closes the file. |
| `SetPosition` | method | **done** | Seeks. |
| `Delete`, `Copy` | method | **done** | Deletes the file / copies it to a new name, under the save directory. The source of `Copy` is a file on disk (one a script wrote, or a loose file of the game), as WME's `CopyFile`. |
| `ReadLine`, `ReadText` | method | **done** | Reads a line / N characters of text (`\r` skipped). |
| `WriteLine`, `WriteText` | method | **done** | Writes text with / without a line break; every `\n` is written as `\r\n`, as WME's text-mode stream does. |
| `ReadBool`, `ReadByte`, `ReadShort`, `ReadInt`, `ReadLong`, `ReadFloat`, `ReadDouble`, `ReadString` | method | **done** | Reads a binary value, little-endian; `ReadShort` is 65536 plus the signed value, as in WME. |
| `WriteBool`, `WriteByte`, `WriteShort`, `WriteInt`, `WriteLong`, `WriteFloat`, `WriteDouble`, `WriteString` | method | **done** | Writes a binary value. |
| `Type` | get | **done** | `file`. |
| `Filename`, `Position`, `Length`, `TextMode`, `AccessMode` | get | **done** | Current file state; `Filename` is null without a name. |

### CSXDirectory (the global `Directory`)

- WME source: src/engine_core/wme_base/SXDirectory.cpp
- Hierarchy and Luma: CBScriptable → CSXDirectory; Luma: Kind::Class (luma-script/src/classes/directory.rs), one object published as the global. Paths resolve under the save directory, as `File`'s do; listings add the game's packaged and loose files.
- Count: 8 done, 0 todo, 2 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Create`, `Delete` | method | **done** | Creates every level of a directory / deletes an empty one, under the save directory. `Create` answers as WME's final `mkdir` does: true when the call made the directory, false when it existed or when the path ends with a separator. |
| `GetFiles`, `GetDirectories` | method | **done** | Lists the files / sub-directories matching a Windows wildcard mask (`*.*` by default, without case): the save directory's, then the game's own under the same path. Names, not paths, sorted; directories starting with `.` skipped as in WME. |
| `GetDrives` | method | **no-op** | Lists Windows drive letters; no meaning on macOS. Answers an empty array. |
| `Type` | get | **done** | `directory`. |
| `PathSeparator` | get | **done** | `\`, as in WME; every file name in Luma accepts it along with `/`. |
| `CurrentDirectory` | get | **done** | The save directory, the root every path a script gives resolves under (WME: the process's working directory). |
| `CurrentDirectory` | set | **no-op** | Changes the process's working directory in WME; Luma's scripts cannot leave the save directory, so the set is accepted and ignored. |
| `TempDirectory` | get | **done** | `temp` under the save directory, created on demand, with a trailing separator as `GetTempPath` gives (WME: the system's temporary directory). |

### CSXMemBuffer (`new MemBuffer`)

- WME source: src/engine_core/wme_base/SXMemBuffer.cpp
- Hierarchy and Luma: CBScriptable → CSXMemBuffer; Luma: Kind::Class (luma-script/src/classes/membuffer.rs)
- Count: 19 done, 0 todo, 3 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetSize` | method | **done** | Resizes the buffer, zero-filling growth. |
| `GetBool`, `GetByte`, `GetShort`, `GetInt`, `GetLong`, `GetFloat`, `GetDouble`, `GetString` | method | **done** | Reads a value at a byte offset (little-endian; `GetShort` is 65536 plus the signed value, as in WME); out of bounds is a runtime error and `null`. |
| `SetBool`, `SetByte`, `SetShort`, `SetInt`, `SetLong`, `SetFloat`, `SetDouble`, `SetString` | method | **done** | Writes a value at a byte offset; out of bounds is a runtime error and `false`. |
| `GetPointer`, `SetPointer` | method | **no-op** | Reads / writes a raw memory address, for calling Windows DLL functions. Luma logs a runtime error and yields `null` / `false`. |
| `DEBUG_Dump` | method | **no-op** | Dumps the buffer to a file on drive C; debugging aid. Luma yields `null`. |
| `Type` | get | **done** | `membuffer`. |
| `Size` | get | **done** | Buffer size in bytes. |

### CSXTcpClient (`new TcpClient`)

- WME source: src/engine_core/wme_base/SXTcpClient.cpp
- Hierarchy and Luma: CBScriptable → CSXTcpClient; unfinished in WME 1.x (methods commented out); Luma: Kind::Class (`ClassObject::TcpClient` in luma-script/src/classes/mod.rs)
- Count: 1 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Type` | get | **done** | `tcp-client`. No other member is active in WME 1.x: every method call is an undefined method, the string form is `[tcp-client object]`, any two objects compare equal, and the object is saved with the game. |

### CSXPluginObject (objects of Windows DLL plugins)

- WME source: src/engine_core/wme_base/SXPluginObject.cpp
- Hierarchy and Luma: CBObject → CSXPluginObject; created by `BPluginMgr::RegisterClasses` for classes exported by plugin DLLs
- Count: 0 done, 0 todo, 1 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Ready` | set | **no-op** | Marks a plugin object ready. Plugins are Windows DLLs; Luma does not load them. |

## 9. Particles

### CPartEmitter (particle emitter)

- WME source: src/engine_core/wme_base/PartEmitter.cpp
- Hierarchy and Luma: CBObject → CPartEmitter; Luma: luma-adventure/src/particles.rs, natives_particles.rs (`emitter_call`, `emitter_get`, `emitter_set`); see `docs/particles.md`. Of the inherited CBObject members, `BlendMode` is implemented on the emitter (its particles blend with it); the others are not exposed on emitters.
- Count: 79 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetBorder`, `SetBorderThickness` | method | **done** | The rectangle particles fade out on leaving, and how much it shrinks for the nearest particles. |
| `AddSprite`, `RemoveSprite` | method | **done** | Adds / removes a particle sprite file; `AddSprite` fails on a missing file. |
| `Start`, `Stop`, `Pause`, `Resume` | method | **done** | Controls emission; `Start(overhead)` runs ahead in steps of 500 ms. |
| `AddGlobalForce`, `AddPointForce`, `RemoveForce` | method | **done** | Adds a wind-like force / removes it. As in WME 1.x, `AddPointForce` creates a global force (its `ScCallMethod` passes `FORCE_GLOBAL`); Luma reproduces this. |
| `Type` | get | **done** | `particle-emitter`. |
| `X`, `Y`, `Width`, `Height` | get/set | **done** | Emission rectangle. |
| `Scale1`, `Scale2`, `ScaleZBased` | get/set | **done** | Particle scale range, optionally by depth. |
| `Velocity1`, `Velocity2`, `VelocityZBased` | get/set | **done** | Speed range. |
| `LifeTime1`, `LifeTime2`, `LifeTimeZBased` | get/set | **done** | Lifetime range. |
| `Angle1`, `Angle2`, `AngVelocity1`, `AngVelocity2`, `Rotation1`, `Rotation2` | get/set | **done** | Direction, spin and initial rotation ranges; each particle turns around its position. |
| `Alpha1`, `Alpha2`, `AlphaTimeBased` | get/set | **done** | Opacity range, clamped to 0..255 on set. |
| `MaxParticles`, `GenerationInterval`, `GenerationAmount`, `MaxBatches` | get/set | **done** | Emission rate and caps. |
| `NumLiveParticles` | get | **done** | Particles alive now. |
| `FadeInTime`, `FadeOutTime` | get/set | **done** | Fade durations. |
| `GrowthRate1`, `GrowthRate2`, `ExponentialGrowth` | get/set | **done** | Size growth. |
| `UseRegion` | get/set | **done** | Only the particles inside the owner's region are drawn. |
| `EmitEvent` | get/set | **done** | Script event raised on the owner at each emission. |

## 10. 3D objects

### CAdObject3D (base of 3D actors)

- WME source: src/engine_core/wme_ad/AdObject3D.cpp
- Hierarchy and Luma: CAdObject → CAdObject3D → CAdActor3D / CAdActor3DX / CAdAttach3DX; Luma: `Actor3d` (`actor3d.rs`, `natives_actor3d.rs`, the `3d` feature)
- Count: 38 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SkipTo` | method | **done** | Moves the object to (x, y) on the 3D floor immediately (the exact conversion; nothing moves off the walk planes). |
| `SkipTo3D` | method | **done** | Moves the object to (x, y, z) immediately; the 2D position follows at the next update. |
| `GetBonePosition2D` | method | **done** | Screen position of a skeleton bone, as `{ X, Y }`; an empty object for an unknown bone. |
| `GetBonePosition3D` | method | **done** | World position of a skeleton bone, as `{ X, Y, Z }`. |
| `AddIgnoredLight`, `RemoveIgnoredLight` | method | **done** | Excludes / re-includes a scene light when lighting the object (the list goes to the drawing step). |
| `Angle` | get/set | **done** | Heading in degrees, as set (not normalised). |
| `PosX`, `PosY`, `PosZ` | get/set | **done** | World position. |
| `X`, `Y` | set | **done** | 2D alias of the world position: the pixel converts to the walk planes with the exact conversion when it can. |
| `Velocity` | get/set | **done** | Walking speed, world units per second, times the scale. |
| `AngularVelocity`, `AngVelocity` | get/set | **done** | Turning speed, degrees per second. |
| `DropToFloor` | get/set | **done** | Keeps the object on the floor mesh every update. |
| `ShadowType`, `Shadow` | get/set | **done** | Shadow kind (none, simple, flat, stencil), clamped; `Shadow` true means stencil, false none. Stored for the shadow drawing step. |
| `SimpleShadow` | get/set | **done** | Obsolete alias: true means simple, false stencil. |
| `ShadowColor` | get/set | **done** | Shadow colour (`0xAARRGGBB`), stored for the drawing step. |
| `Scale` | get/set | **done** | Uniform scale of the model, in percent; the walking speed follows it. |
| `DrawBackfaces` | get/set | **done** | Draws back faces of the mesh (handed to the drawing step). |
| `AmbientLightColor` | get/set | **done** | Per-object ambient colour; `null` clears the override. |

### CAdActor3D (legacy 3D actor, MS3D/3DS models)

- WME source: src/engine_core/wme_ad/AdActor3D.cpp
- Hierarchy and Luma: CAdObject3D → CAdActor3D; Luma: refused by design (deprecated by WME 1.6; `LoadActor3D` on an `ACTOR3D` file logs a clear error and returns `null`, see `docs/3d-actors.md` section 13)
- Count: 0 done, 0 todo, 40 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `PlayAnim`, `PlayAnimAsync` | method | **no-op** | Plays a named animation, optionally waiting. |
| `StopAnim` | method | **no-op** | Stops the current animation. |
| `StopAnimChannel`, `PlayAnimChannel`, `PlayAnimChannelAsync` | method | **no-op** | Stops / plays an animation on a numbered blend channel. |
| `HideMesh`, `ShowMesh` | method | **no-op** | Hides / shows a named mesh of the model. |
| `AddMesh`, `RemoveMesh` | method | **no-op** | Adds / removes a mesh from another model file. |
| `GoTo3D`, `GoTo3DAsync` | method | **no-op** | Walks to (x, y, z). |
| `GoTo`, `GoToAsync` | method | **no-op** | Walks to (x, y) on the floor. |
| `GoToObject`, `GoToObjectAsync` | method | **no-op** | Walks to an object. |
| `TurnTo`, `TurnToAsync` | method | **no-op** | Turns toward a point or object. |
| `TurnToAngle`, `TurnToAngleAsync` | method | **no-op** | Turns to an absolute angle. |
| `IsWalking` | method | **no-op** | True while walking. |
| `DirectWalk`, `DirectWalkBack`, `DirectWalkStop` | method | **no-op** | Free keyboard-style walking forward / backward / stop. |
| `DirectTurnLeft`, `DirectTurnRight`, `DirectTurnStop` | method | **no-op** | Free turning left / right / stop. |
| `Type` | get | **no-op** | `actor3d`. |
| `TalkAnimName`, `WalkAnimName`, `IdleAnimName` | get/set | **no-op** | Animation names for talk, walk and idle. |
| `DirectionAngle`, `DirAngle`, `Direction` | get/set | **no-op** | Facing angle. |

### CAdActor3DX (3D actor, X-file models with skeletal animation)

- WME source: src/engine_core/wme_ad/AdActor3DX.cpp
- Hierarchy and Luma: CAdObject3D → CAdActor3DX; Luma: `Actor3d` (`actor3d.rs`, `natives_actor3d.rs`, the `3d` feature; see `docs/3d-actors.md`)
- Count: 62 done, 0 todo, 5 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `PlayAnim`, `PlayAnimAsync` | method | **done** | Plays a named animation on channel 0, restarted; both forms set the actor's state, only the blocking one waits. |
| `StopAnim` | method | **done** | Stops channel 0 with a transition time (the stop transition time by default). |
| `StopAnimChannel`, `PlayAnimChannel`, `PlayAnimChannelAsync` | method | **done** | Controls animations on numbered blend channels 0..9 (`StopAnimChannel` defaults to no transition). |
| `IsAnimPlaying`, `IsAnimChannelPlaying` | method | **done** | True when an animation (on a channel) plays, by name or any. |
| `AddAttachment`, `RemoveAttachment`, `GetAttachment` | method | **done** | Attaches another `.x` model to a bone (`file, name, bone`; a runtime error and `false` when the bone or the file is missing; the same name replaces), removes it by name, or returns the attachment object (by name; Luma also accepts an index). See `docs/3d-actors.md` section 8.6. |
| `AddMesh`, `RemoveMesh` | method | **done** | Obsolete aliases of the attachment methods; they log the original's warning. |
| `GoTo3D`, `GoTo3DAsync`, `GoTo`, `GoToAsync`, `GoToObject`, `GoToObjectAsync` | method | **done** | Walking orders (3D point, floor point with the tolerant conversion, entity's walk-to point and direction); `GoTo` is skipped within `GoToTolerance`. |
| `TurnTo`, `TurnToAsync`, `TurnToAngle`, `TurnToAngleAsync` | method | **done** | Turning orders: a direction constant, an object (its 2D position converted to 3D) or an angle, the shorter way. |
| `IsWalking` | method | **done** | True while following a path. |
| `DirectWalk`, `DirectWalkBack`, `DirectWalkStop`, `DirectTurnLeft`, `DirectTurnRight`, `DirectTurnStop` | method | **done** | Free walking / turning, stopped by the geometry (or the 2D floor in a `2D_PATHFINDING` scene). |
| `SetTexture` | method | **done** | Replaces a material's texture by an image or a `.sprite`, by material name; `false` when the file is missing (a material no mesh has is accepted, as in WME). Kept on the actor and handed to the drawing step (`docs/3d-actors.md` section 10.5). |
| `SetTheoraTexture` | method | **done** | Uses a looping Theora video as a material's texture; recorded like `SetTexture`, the drawing step plays the video. |
| `SetEffect`, `RemoveEffect` | method | **no-op** | Applies / removes an HLSL shader effect on a material; not planned (see `docs/3d-actors.md` section 13), the calls return `false` as WME does when an effect fails to compile. |
| `SetEffectParam`, `SetEffectParamVector`, `SetEffectParamColor` | method | **no-op** | Sets shader effect parameters; not planned, `false`. |
| `MergeAnims` | method | **done** | Merges animation sets from another `.x` file, with its `.anim` sidecar. |
| `UnloadAnim` | method | **done** | Unloads a named animation set (and stops it on its channels). |
| `SetAnimTransitionTime`, `GetAnimTransitionTime` | method | **done** | Sets / reads the blend time between two animations (`-1` for an unset pair; a negative time removes the pair). |
| `CreateParticleEmitterBone` | method | **done** | Creates (or re-aims) the actor's particle emitter so that it follows the projected position of a bone plus an offset; returns the emitter. `CreateParticleEmitter` forgets the bone. |
| `Type` | get | **done** | `actor3dx`. |
| `TalkAnimName`, `TalkAnimChannel`, `WalkAnimName`, `IdleAnimName`, `TurnLeftAnimName`, `TurnRightAnimName` | get/set | **done** | Animation names / channel for talk, walk, idle, turn; `null` restores a default name. |
| `DirectionAngle`, `DirAngle`, `Direction` | get/set | **done** | Facing angle (normalised on set) and its eight-sector direction code. |
| `AnimTransitionTime`, `AnimStopTransitionTime` | get/set | **done** | Default blend times (200 ms). |
| `GoToTolerance` | get/set | **done** | Distance in pixels within which a `GoTo` is not started (2). |

### CAdAttach3DX (model attached to a 3D actor bone)

- WME source: src/engine_core/wme_ad/AdAttach3DX.cpp
- Hierarchy and Luma: CAdObject3D → CAdAttach3DX; Luma: `Attachment3d` (`actor3d.rs`, `natives_actor3d.rs`, the `3d` feature; see `docs/3d-actors.md` sections 8.6 and 11.3)
- Count: 7 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `PlayAnim`, `PlayAnimAsync`, `StopAnim`, `StopAnimChannel`, `PlayAnimChannel`, `PlayAnimChannelAsync` | method | **done** | Animation control of the attached model: restarted, no stop transition, the owner actor's transition table when the channel plays something; the blocking forms return at the next tick (the attachment is always ready, as in WME). |
| `Type` | get | **done** | `attachment`. The `CAdObject3D` part it inherits answers `PosX`, `PosY`, `PosZ`, `Angle`, `Scale`, `Active`, `DrawBackfaces`, `SkipTo3D`, `GetBonePosition2D`, `GetBonePosition3D`. |

## 11. Events

### Events raised on objects (`on "Name" { ... }` handlers)

- WME source: src/engine_core/wme_base/BGame.cpp, wme_ad/AdGame.cpp, wme_ad/AdObject.cpp, wme_base/UIButton.cpp, wme_base/BSprite.cpp (search `ApplyEvent("`)
- Hierarchy and Luma: luma-adventure/src/world.rs (`mouse_event`, `key_down`), ui.rs, scene_state.rs, natives_particles.rs
- Count: 21 done, 0 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `LeftClick`, `LeftRelease`, `RightClick`, `RightRelease` | event | **done** | Mouse button events: raised on the game, then on the object under the mouse or the scene. |
| `LeftDoubleClick` | event | **done** | Double click of the left button. |
| `RightDoubleClick` | event | **done** | Double click of the right button: on the game, else on the active object (not the scene, as `CBGame` dispatches it); replaces the second `RightClick`. |
| `MiddleClick`, `MiddleRelease` | event | **done** | Middle button events, dispatched as the right double click. |
| `MouseWheelUp`, `MouseWheelDown` | event | **done** | Wheel events: raised on the focused window if it handles them, else on the game. |
| `MouseEntry`, `MouseLeave` | event | **done** | The object under the mouse changes. |
| `Keypress` | event | **done** | A non-printable key, on the focused window if it handles the event, else on the game. |
| `Press` | event | **done** | A button was released over itself; with `ParentNotify`, also an event named after the button on its window. |
| `QuitGame` | event | **done** | The player closed the window; raised on the game if a handler exists and the game is not frozen, and the window stays open (otherwise the game quits). |
| `SceneShutdown` | event | **done** | The scene is about to be left (unbreakable). |
| `BeforeSave`, `AfterLoad` | event | **done** | Around saving and loading (unbreakable). |
| `ActorEntry`, `ActorLeave` | event | **done** | An actor enters / leaves a region of the main layer (raised on the region). |
| (sound event name) | event | **done** | The name given to `PlaySoundEvent`, raised on the object when the sound ends. |
| `FrameChanged` | event | **done** | Raised on a sprite object when its current frame changes (the sprite of an entity, an actor or a control); only scripts attached to the sprite object with `AttachScript` can handle it, since Luma does not attach a sprite file's own `SCRIPT`s yet. |
| (frame event names) | event | **done** | Events attached to a frame (`APPLY_EVENT` in sprite files, `AddEvent`), raised on the sprite's owner (entity, actor, control) when the frame becomes current; the frame's `SOUND` plays and `KILL_SOUND` stops the sprite's earlier frame sounds at the same time. |
| (emit event name) | event | **done** | The particle emitter's `EmitEvent`, raised on its owner each frame a batch is born (not during the run-ahead of `Start`). |

## Summary per class

| Class | done | todo | no-op | total |
|-------|-----:|-----:|------:|------:|
| Global functions | 20 | 0 | 1 | 21 |
| Constructors | 9 | 0 | 0 | 9 |
| Global variables | 5 | 0 | 0 | 5 |
| Script values | 1 | 0 | 0 | 1 |
| CBScriptHolder | 10 | 0 | 1 | 11 |
| CBObject | 72 | 0 | 0 | 72 |
| CBGame | 152 | 0 | 13 | 165 |
| CAdGame | 65 | 0 | 0 | 65 |
| Window hook defined in CAdGame | 1 | 0 | 0 | 1 |
| CAdScene | 81 | 0 | 2 | 83 |
| CAdLayer | 18 | 0 | 0 | 18 |
| CBRegion | 11 | 0 | 0 | 11 |
| CAdRegion | 11 | 0 | 0 | 11 |
| CAdWaypointGroup | 3 | 0 | 0 | 3 |
| CAdObject | 41 | 0 | 0 | 41 |
| CAdTalkHolder | 7 | 0 | 0 | 7 |
| CAdActor | 23 | 0 | 0 | 23 |
| CAdEntity | 20 | 0 | 0 | 20 |
| CAdItem | 26 | 0 | 0 | 26 |
| CBSprite | 18 | 0 | 0 | 18 |
| CBFrame | 21 | 0 | 0 | 21 |
| CBSubFrame | 20 | 0 | 0 | 20 |
| CUIObject | 27 | 0 | 0 | 27 |
| CUIWindow | 40 | 0 | 0 | 40 |
| CUIButton | 26 | 0 | 0 | 26 |
| CUIText | 7 | 0 | 0 | 7 |
| CUIEdit | 16 | 0 | 0 | 16 |
| CUIEntity | 5 | 0 | 0 | 5 |
| CUIMarkup | 0 | 0 | 1 | 1 |
| CBKeyboardState | 8 | 0 | 0 | 8 |
| CSXMath | 22 | 0 | 0 | 22 |
| CSXString | 10 | 0 | 0 | 10 |
| CSXArray | 5 | 0 | 0 | 5 |
| CSXDate | 15 | 0 | 0 | 15 |
| CSXFile | 33 | 0 | 0 | 33 |
| CSXDirectory | 8 | 0 | 2 | 10 |
| CSXMemBuffer | 19 | 0 | 3 | 22 |
| CSXTcpClient | 1 | 0 | 0 | 1 |
| CSXPluginObject | 0 | 0 | 1 | 1 |
| CPartEmitter | 79 | 0 | 0 | 79 |
| CAdObject3D | 38 | 0 | 0 | 38 |
| CAdActor3D | 0 | 0 | 40 | 40 |
| CAdActor3DX | 62 | 0 | 5 | 67 |
| CAdAttach3DX | 7 | 0 | 0 | 7 |
| Events raised on objects | 21 | 0 | 0 | 21 |
| **All** | **1084** | **0** | **69** | **1153** |

Of the total, 21 are events (21 done, 0 todo) and the rest are methods, properties, functions, constructors and global variables: 1063 done, 0 todo, 69 no-op.

## WME classes Luma lacks entirely

These classes have no done member in Luma: the object does not exist for scripts, or is only an inert placeholder returned by `new`.

| Class | What it is | Members still todo |
|-------|-----------|-------------------:|
| `CBFrame` | A frame of a sprite (delay, sound, move, sub-frames, events) | 21 |
| `CBSubFrame` | The image part of a frame (hotspot, mirror, alpha) | 20 |

Classes that exist in WME 1.x with a script interface but that are no-op by design: `CAdActor3D` (the legacy Milkshape 3D actor WME deprecated in 1.6; `LoadActor3D` refuses its files with a clear error), `CSXPluginObject` (Windows DLL plugins), `CUIMarkup` (never instantiated), the Windows-only parts of `CSXDirectory` (`GetDrives`, the working directory) and `CSXMemBuffer`, and the AVI side of `Game.PlayVideo` (DirectShow).

WME classes with no script interface of their own (nothing to port; scripts reach them only through the owner's methods): `CBSound`, `CBFont`/`CBFontBitmap`/`CBFontTT`, `CAdInventory`, `CAdInventoryBox`, `CAdResponseBox`, `CAdResponse`, `CAdSceneNode`, `CAdSentence`, `CAdSpriteSet`, `CAdTalkDef`, `CAdScaleLevel`, `CAdRotLevel`, `CAdWalkplane`, `CAdBlock`, `CAdGeneric`, `CBViewport`, `CBFader`, `CUITiledImage`, `CBPersistMgr`, `CBEventSet`, the video players (`VidTheoraPlayer`), `C3DLight`, `C3DModel`. The inventory, responses, fader and viewport are driven through `Game` methods and properties (`GetInventoryWindow`, `AddResponse`, `FadeOut`, `SetSceneViewport`), which the tables above cover.

## Groups the work falls into

Counts are the todo members (no-op excluded). Everything is done: the script library (`String`, `Array`, `Date`, `File`, `MemBuffer`, `Directory`, `new Object`, `new Actor`, `new Entity`, the HSL colour helpers), the game services (private settings, screenshots and thumbnails, loading screens, `Reset`, `LockMouseRect`, `OpenDocument`, `GetFileChecksum`, the volume, sound, FPS and encoding properties, `StartupScene`, `TextRTL`, `SuspendedRendering`, `Scene.SetViewport`, the game's and the items' cursor objects, the items' hover image objects), the 2D object model (per-object cursors, `SetCaption`, `Movable`, `SceneIndependent`, `NonIntMouseEvents`, `ForceTalkAnim`, talk sprite sets, `GetSpriteObject` on actors and items, region points, waypoint groups, entity regions, scene and game content created at run time, `StickToRegion`, attachments, per-object inventories) the rendering features (rotation, per-axis scale, blend modes, scene-only fades; see `docs/rendering.md`) and the 3D side (the scene geometry, the 3D actor (`CAdObject3D`, `CAdActor3DX`), its walking, turning, animation and scripting, its attachments (`CAdAttach3DX`) and bone emitter, the textures set by script on its materials, the scene's lights, nodes, cameras and fog, the game's shadow settings and the shadow members of every object; `docs/3d-actors.md`, the `3d` feature). The drawing of the models and their shadows is the 3D pass, not a script member.

| Group | todo | What it covers |
|-------|-----:|----------------|
| **All** | **0** | |

## Members in Luma that WME 1.x does not define

These are accepted by Luma and not counted above; some come from later WME versions, some from Luma itself.

| Object | Members | Note |
|--------|---------|------|
| Game | `HideCursor`, `ShowCursor` | Hide / show the mouse cursor. |
| Game | `IsVideoPlaying`, `StopVideo` | Query / stop the running video (`StopVideo` is an alias of `StopTheora`). |
| Game | `FocusedWindow`, `CursorHidden`, `AutoSaveOnExit`, `AutoSaveSlot`, `Platform`, `DeviceType` | Properties Luma reads that WME 1.x does not define in its script interface (some come from later WME versions or from Luma itself). |
| `new Actor`, `new Entity` | a file argument | `new Actor(file)` and `new Entity(file)` load the file, as `Game.LoadActor` and `Game.LoadEntity` do; WME's constructors take no argument. |
| Actor | `SetStopOnBlocked`, `GetStopOnBlocked`, `ActorIsBlocked` | Stop-on-blocked behaviour and the matching event / property. |
| 3D actor | `NumAttachments`, `GetAttachment(index)` | The bone attachments counted and found by index; WME's `GetAttachment` on a 3D actor takes a name, and its inherited `NumAttachments` counts the 2D attachments of `CAdObject`, always 0 on a 3D actor. |
| Attachment of a 3D actor | `Bone`, `Filename` | The parent bone and the model file, which WME keeps private. |

## Maintenance

Update this file in the same commit as any change to `natives*.rs`, to the `*_STUBS` lists or to the built-ins of `luma-script`: flip the row to **done**, and delete its stub entry. Then recount the summary tables: the per-class numbers are computed from the rows (a name counts once, a `get/set` row twice).

The test `crates/luma-adventure/tests/compatibility_doc.rs` enforces this: it recomputes the counts of every section, the summary table, the `Of the total` sentence and the total of the groups table from the rows, and fails when a number written here differs or a row has a state other than **done**, **todo** or **no-op**.
