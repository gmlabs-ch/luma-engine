# Compatibility with the Wintermute Engine 1.x scripting interface

This is the checklist for porting the whole of the scripting interface of Wintermute Engine (WME) 1.x to Luma Engine. It lists every method, property, global function, constructor and event that WME 1.x exposes to game scripts, and says for each one whether Luma implements it.

Reference: the MIT-licensed Wintermute Engine 1.x sources (https://github.com/MnemonicWME/wintermute-engine), under `src/engine_core/` (`wme_base`, `wme_ad`). Source paths of the form `src/engine_core/...` are relative to the root of that repository; paths starting with `luma-` are in `crates/` of this repository. No GPL code was consulted (clean-room rule in `CLAUDE.md`).

How the list was made. For every class with a script interface, the names tested by `strcmp(Name, "...")` in `ScCallMethod`, `ScGetProperty` and `ScSetProperty` were extracted (code inside comments excluded, since WME 1.x leaves some dead blocks in `SXFile`, `SXMemBuffer`, `SXTcpClient`, `AdTalkHolder`, `AdRegion` and others). The global functions and constructors come from `CBGame::ExternalCall` and `CAdGame::ExternalCall`, the extra window method from `CAdGame::WindowScriptMethodHook`. Luma was then read: the match arms of `crates/luma-adventure/src/natives*.rs`, the `*_STUBS` lists (names accepted, logged once as "not implemented" and ignored), the built-ins of `crates/luma-script/src/engine.rs`, and `World::call_function`. Luma state as of commit `c35949d`.

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
- Count: 16 done, 4 todo, 1 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `LOG` | function | **done** | Writes its argument to the game log (`sc: ...`). |
| `Sleep` | function | **done** | Suspends the calling script for N milliseconds. |
| `WaitFor` | function | **done** | Suspends the script until an object is ready (finished walking, talking, animating). |
| `Random` | function | **done** | Random integer in [min, max]. Luma uses its own seeded generator, so sequences differ from WME's. |
| `SetScriptTimeSlice` | function | **done** | Limits how long the calling script runs per frame before yielding. |
| `MakeRGBA`, `MakeRGB`, `RGB` | function | **done** | Packs r, g, b (and a) into the engine's 0xAARRGGBB integer. |
| `MakeHSL` | function | **todo** | Builds a colour integer from hue, saturation, lightness. |
| `GetRValue`, `GetGValue`, `GetBValue`, `GetAValue` | function | **done** | Extracts one channel of a colour integer. |
| `GetHValue`, `GetSValue`, `GetLValue` | function | **todo** | Extracts hue, saturation or lightness of a colour integer. |
| `Debug` | function | **no-op** | Hits a breakpoint in the WME script debugger when one is attached, otherwise does nothing. Luma has no debugger protocol, so it does nothing either way. |
| `ToString`, `ToInt`, `ToFloat`, `ToBool` | function | **done** | Explicit conversions with the engine's coercion rules. |

### Constructors (`new X(...)`)

- WME source: src/engine_core/wme_base/BGame.cpp, src/engine_core/wme_ad/AdGame.cpp (CAdGame::ExternalCall)
- Hierarchy and Luma: luma-adventure/src/world.rs (`call_function` returns an inert object)
- Count: 0 done, 8 todo, 1 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Object` | constructor | **todo** | Generic object, optionally with script files to attach as arguments. Luma returns an inert property bag: properties work, the attached scripts and the CBObject members do not. |
| `String` | constructor | **todo** | `new String(text)`: string object with methods (see CSXString). Luma returns an inert object. |
| `Array` | constructor | **todo** | `new Array(...)`: array object (see CSXArray). Luma returns an inert object. |
| `Date` | constructor | **todo** | `new Date()`: date/time object (see CSXDate). Luma returns an inert object. |
| `File` | constructor | **todo** | `new File(name)`: file access object (see CSXFile). Luma returns an inert object. |
| `MemBuffer` | constructor | **todo** | `new MemBuffer(size)`: raw byte buffer (see CSXMemBuffer). Luma returns an inert object. |
| `TcpClient` | constructor | **no-op** | Network client; the class is an unfinished stub in WME 1.x itself (its methods are commented out), and games cannot rely on it. |
| `Actor` | constructor | **todo** | `new Actor()`: creates an empty 2D actor not yet in a scene. Luma returns an inert object; actors only come from `LoadActor`. |
| `Entity` | constructor | **todo** | `new Entity()`: creates an empty entity. Luma returns an inert object; entities only come from the scene files. |

### Global variables

- WME source: src/engine_core/wme_base/BGame.cpp (RegisterExternals, ExtDefineVariable)
- Hierarchy and Luma: luma-script/src/engine.rs, luma-adventure/src/world.rs (Runtime::with_audio)
- Count: 4 done, 1 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Game` | var | **done** | The game object (CAdGame). |
| `Math` | var | **done** | The math object (CSXMath). |
| `self`, `this` | var | **done** | The object owning the running script / the current method's target. |
| `Directory` | var | **todo** | The directory object (CSXDirectory); not published to scripts in Luma. |

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
| `AttachScript` | method | **done** | Attaches a script file to the object and runs it. |
| `DetachScript` | method | **done** | Detaches a script (optionally killing its threads). |
| `IsScriptRunning` | method | **done** | True when the script file is attached and running. |
| `Type` | get | **done** | Class name string (`game`, `scene`, `entity`, ...). Luma returns the right value for each object it has. |
| `Name` | get | **done** | Object name. |
| `Name` | set | **done** | Renames the object. |
| `Filename` | get | **done** | Source file of the object. Luma: scenes, actors and windows only; entities and others return null. |

### CBObject (base of the game, scenes, layers, entities, actors, windows, controls, regions)

- WME source: src/engine_core/wme_base/BObject.cpp
- Hierarchy and Luma: CBScriptHolder → CBObject; Luma: game, entities, actors (sound, position, flags); controls, regions, layers have their own subsets
- Count: 45 done, 27 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SkipTo` | method | **done** | Moves the object to (x, y) immediately, or to another object's position. |
| `Caption` | method | **done** | Returns the object's caption (argument selects a stored caption). Luma: entities and actors; missing on `Game` and items. |
| `SetCaption` | method | **todo** | Sets the caption text (second argument selects the caption slot). Stubbed on entities and actors; implemented on items only. |
| `SetCursor` | method | **todo** | Sets the cursor shown when the mouse is over the object. Stubbed. |
| `RemoveCursor` | method | **todo** | Removes the object's own cursor. Stubbed. |
| `GetCursor` | method | **todo** | Returns the object's cursor file. Luma returns null. |
| `GetCursorObject` | method | **todo** | Returns the object's cursor as a sprite object. Luma returns null. |
| `HasCursor` | method | **todo** | True when the object has its own cursor. Luma always returns false. |
| `LoadSound` | method | **done** | Loads the object's sound effect without playing it. |
| `PlaySound` | method | **done** | Plays the object's sound (file, looping, loop start). |
| `PlaySoundEvent` | method | **done** | Plays a sound and raises a script event on the object when it ends. |
| `StopSound`, `PauseSound`, `ResumeSound`, `IsSoundPlaying` | method | **done** | Controls the object's sound effect. |
| `SetSoundPosition`, `GetSoundPosition` | method | **done** | Seeks / reads the sound's position in milliseconds. |
| `SetSoundVolume`, `GetSoundVolume` | method | **done** | Sets / reads the sound's volume (percent). |
| `SetShadowImage`, `GetShadowImage` | method | **todo** | Sets / reads the image used for the object's 3D simple shadow. |
| `SetLightPosition` | method | **todo** | Sets the position of the light attached to a 3D object. |
| `SoundFXNone` | method | **done** | Removes sound effects from the object's sound (nothing to do in Luma). |
| `SoundFXEcho`, `SoundFXReverb` | method | **todo** | Applies an echo / reverb effect to the object's sound. Logged and ignored. |
| `Type` | get | **done** | Class name of the object. |
| `Caption` | get/set | **done** | Object caption (expanded through the string table on set). |
| `X`, `Y` | get/set | **done** | Position in scene coordinates. |
| `Height` | get | **done** | Height of the current frame, scaled. |
| `Ready` | get | **done** | False while the object is talking, walking or playing an animation a script waits for. |
| `Movable` | get/set | **todo** | Whether the object can be moved by the engine (e.g. dragged). Luma reads true, ignores the set. |
| `Registrable` | get/set | **done** | Whether the object is registered for mouse interaction (same as `Interactive`). |
| `Interactive` | get/set | **done** | Whether the object reacts to the mouse. |
| `Zoomable`, `Scalable` | get/set | **done** | Whether the scene's scale levels apply to the object. |
| `Rotatable` | get/set | **done** | Whether the scene's rotation levels apply. The flag is stored; rotation is not rendered. |
| `AlphaColor` | get/set | **done** | Tint colour and alpha (0xAARRGGBB). |
| `BlendMode` | get/set | **todo** | Blend mode (normal / additive / subtractive). Luma reads 0, ignores the set; no blend modes in the renderer. |
| `Scale` | get/set | **done** | Absolute scale in percent (null = use the scene's scale). |
| `ScaleX`, `ScaleY` | get/set | **todo** | Independent horizontal / vertical scale. Luma reads null, ignores the set. |
| `RelativeScale` | get/set | **done** | Scale adjustment in percent added to the scene's scale. |
| `Rotate` | get/set | **todo** | Absolute rotation in degrees. Luma reads null, ignores the set. |
| `RelativeRotate` | get/set | **todo** | Rotation added to the scene's rotation level. Luma reads 0, ignores the set. |
| `Colorable` | get/set | **done** | Whether the scene's ambient colour tints the object. |
| `SoundPanning` | get/set | **done** | Whether the object's sound pans with its screen position. |
| `SaveState` | get/set | **done** | Whether the object is saved with the scene state (entities; actors always read true and ignore the set). |
| `NonIntMouseEvents` | get/set | **todo** | Whether the object receives mouse events while the game is non-interactive. Luma reads false, ignores the set. |
| `AccCaption` | get/set | **todo** | Caption read out by the accessibility text-to-speech. Luma reads null, ignores the set. |

## 3. The Game object

### CBGame (the `Game` object, base part)

- WME source: src/engine_core/wme_base/BGame.cpp
- Hierarchy and Luma: CBObject → CBGame → CAdGame; Luma: World (`game_call`, `game_get`, `game_set`, `natives_sound.rs`)
- Count: 90 done, 58 todo, 17 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `LOG` | method | **done** | Writes to the log (same as the global `LOG`). |
| `Caption` | method | **todo** | Inherited CBObject.Caption plus refreshes the window title. Not handled on `Game` in Luma (the `Caption` property exists). |
| `Msg` | method | **todo** | Shows a short on-screen debug message. Luma only writes it to the log. |
| `RunScript` | method | **todo** | Obsolete alias of `AttachScript` on the game (logs a warning). |
| `LoadStringTable` | method | **done** | Loads a string table, optionally keeping the old strings. |
| `ValidObject` | method | **done** | True when a handle still refers to a live object. |
| `Reset` | method | **todo** | Resets all game content (scene, objects, scripts, inventory) to the startup state. Stubbed. |
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
| `LockMouseRect` | method | **todo** | Confines the mouse to a rectangle. Stubbed. |
| `PlayVideo` | method | **todo** | Deprecated AVI player (DirectShow). Luma aliases it to `PlayTheora`; AVI files cannot play. |
| `PlayTheora` | method | **done** | Plays an Ogg Theora video full-frame (file, type, x, y, freeze music, drop frames, subtitles). |
| `QuitGame` | method | **done** | Ends the game. |
| `RegWriteNumber`, `RegWriteString` | method | **todo** | Stores a value in the game's private settings (the `PrivateSettings` section of `wme.ini` in WME). Logged and ignored. |
| `RegReadNumber`, `RegReadString` | method | **todo** | Reads a stored private setting, with a default. Luma returns the type's zero value, never the stored one. |
| `SaveGame` | method | **done** | Saves to a slot (slot, description, quick save); the script resumes after loading. |
| `LoadGame` | method | **done** | Loads a slot at the start of the next frame. |
| `IsSaveSlotUsed`, `GetSaveSlotDescription`, `EmptySaveSlot` | method | **done** | Inspects / clears a save slot. |
| `SetGlobalSFXVolume`, `SetGlobalSpeechVolume`, `SetGlobalMusicVolume`, `SetGlobalMasterVolume` | method | **done** | Sets the global volume of a sound class (percent). |
| `GetGlobalSFXVolume`, `GetGlobalSpeechVolume`, `GetGlobalMusicVolume`, `GetGlobalMasterVolume` | method | **done** | Reads the global volume of a sound class. |
| `SetActiveCursor`, `GetActiveCursor`, `RemoveActiveCursor`, `HasActiveCursor` | method | **done** | Sets, reads, removes the cursor used for "usable" objects (the item cursor). |
| `GetActiveCursorObject` | method | **todo** | Returns the active cursor as a sprite object. Stubbed. |
| `FileExists` | method | **done** | True when a file exists in the game's virtual file system. |
| `FadeOut`, `FadeOutAsync`, `FadeIn`, `FadeInAsync` | method | **done** | Fades the game view to / from a colour; `Async` does not wait. |
| `SystemFadeOut`, `SystemFadeOutAsync`, `SystemFadeIn`, `SystemFadeInAsync` | method | **done** | Same, but the fade also covers system windows and keeps running while the game is frozen. |
| `GetFadeColor` | method | **done** | Colour of the last fade. |
| `Screenshot`, `ScreenshotEx` | method | **todo** | Saves the screen to an image file (ScreenshotEx: with size and format). Stubbed. |
| `CreateWindow` | method | **done** | Creates an empty window at runtime. |
| `DeleteWindow` | method | **done** | Destroys a window created by `CreateWindow` or `LoadWindow`. |
| `OpenDocument` | method | **todo** | Opens a file or URL with the system's default application. Stubbed. |
| `DEBUG_DumpClassRegistry` | method | **no-op** | Writes the class registry to the log; debugging aid. |
| `SetLoadingScreen` | method | **todo** | Sets the image shown while a scene loads (file, x, y). Stubbed. |
| `SetSavingScreen` | method | **todo** | Sets the image shown while saving (file, x, y). Stubbed. |
| `SetWaitCursor`, `RemoveWaitCursor`, `GetWaitCursor` | method | **done** | Sets, removes, reads the cursor shown while the game is not interactive. |
| `GetWaitCursorObject` | method | **todo** | Returns the wait cursor as a sprite object. Stubbed. |
| `ClearScriptCache` | method | **no-op** | Empties the compiled-script cache; Luma keeps no cache and accepts the call. |
| `DisplayLoadingIcon` | method | **todo** | Shows a loading icon sprite (file, x, y, persistent). Stubbed. |
| `HideLoadingIcon` | method | **todo** | Hides the loading icon. Stubbed. |
| `DumpTextureStats` | method | **no-op** | Writes texture memory statistics to a file (D3D only); debugging aid. |
| `AccOutputText` | method | **todo** | Speaks text through the accessibility text-to-speech. Stubbed. |
| `IsShadowTypeSupported` | method | **todo** | True when the renderer supports a 3D shadow type. |
| `StoreSaveThumbnail`, `DeleteSaveThumbnail` | method | **todo** | Captures / discards the screenshot stored in the next save file. Stubbed. |
| `GetFileChecksum` | method | **todo** | CRC-32 of a file in the virtual file system, as integer or hex string. |
| `EnableScriptProfiling`, `DisableScriptProfiling` | method | **no-op** | Turns the script profiler on / off; Luma has no profiler and accepts the calls. |
| `Type` | get | **done** | `game`. |
| `Name` | get/set | **done** | Game name. |
| `Hwnd` | get | **no-op** | Windows window handle (HWND); meaningless outside Windows. |
| `CurrentTime` | get | **done** | Game timer in milliseconds (stops while frozen). |
| `WindowsTime` | get | **todo** | Real-time clock in milliseconds (timeGetTime). Luma returns the game timer, which stops while frozen. |
| `WindowedMode` | get | **todo** | True when running windowed. Luma always reads false. |
| `MouseX`, `MouseY` | get/set | **done** | Mouse position in game coordinates. |
| `MainObject` | get/set | **done** | The object the player controls and the camera follows. |
| `ActiveObject` | get | **done** | The object under the mouse. |
| `ScreenWidth`, `ScreenHeight` | get | **done** | Game resolution. |
| `Interactive` | get/set | **done** | Whether the player can interact (false in cutscenes). |
| `DebugMode` | get | **no-op** | True in debug mode (`-debug`); Luma has none and reads false. |
| `SoundAvailable` | get | **todo** | True when a sound device is present. Luma always reads false. |
| `SFXVolume`, `SpeechVolume`, `MusicVolume`, `MasterVolume` | get/set | **todo** | Global volumes (percent), the property form of the `SetGlobal...Volume` methods. Luma reads 100 and ignores the set. |
| `Keyboard` | get | **done** | The keyboard state object. |
| `Subtitles` | get/set | **done** | Show subtitles for speech. |
| `SubtitlesSpeed` | get/set | **done** | Subtitle reading speed (characters per second basis). |
| `VideoSubtitles` | get/set | **done** | Show subtitles during videos. |
| `FPS` | get | **todo** | Current frame rate. Luma reads 0. |
| `Shadows` | get/set | **todo** | Enables real-time 3D shadows (3D renderer). |
| `SimpleShadows` | get/set | **todo** | Enables simple blob shadows for 3D objects. |
| `SupportsRealTimeShadows` | get | **todo** | True when the renderer can draw 3D real-time shadows. |
| `MaxShadowType` | get/set | **todo** | Highest shadow quality allowed (none, simple, flat, stencil). |
| `AcceleratedMode` | get | **done** | True when GPU accelerated; Luma reads true. |
| `Accelerated` | get | **todo** | Alias of `AcceleratedMode`. |
| `TextEncoding` | get/set | **todo** | Text encoding of the game (ANSI code page / UTF-8). Luma reads 0 and draws UTF-8 tables as Latin-1. |
| `TextRTL` | get/set | **todo** | Right-to-left text layout. Luma reads false, ignores the set. |
| `SoundBufferSize` | get/set | **no-op** | DirectSound buffer length in seconds; the audio backend manages its buffers. |
| `SuspendedRendering` | get/set | **todo** | Pauses drawing (used around heavy loading). Luma reads false, ignores the set. |
| `SuppressScriptErrors` | get/set | **no-op** | Hides the script error dialog; Luma only logs errors and never shows one. |
| `Frozen` | get | **done** | True while the game is frozen by a system-exclusive window. |
| `Direct3DDevice`, `DirectDrawInterface` | get | **no-op** | Raw Direct3D / DirectDraw interface pointers (Windows only). |
| `AccTTSEnabled`, `AccTTSTalk`, `AccTTSCaptions`, `AccTTSKeypress` | get | **todo** | Accessibility: text-to-speech master switch and which events it reads out (speech, captions, keys). Not implemented. |
| `AccKeyboardEnabled`, `AccKeyboardCursorSkip`, `AccKeyboardPause` | get | **todo** | Accessibility: keyboard-only play, cursor skipping, pause on focus. Not implemented. |
| `UsedMemory` | get | **no-op** | Memory use statistic. |
| `MaxActiveLights` | get | **todo** | Number of simultaneous 3D lights the device supports. |
| `HardwareTL` | get | **no-op** | True when the Direct3D device does hardware transform and lighting; no meaning with wgpu. |
| `AutorunDisabled` | get/set | **no-op** | Windows CD autorun switch; no meaning on macOS. |
| `SaveDirectory` | get | **done** | Folder holding the save files. |
| `MostRecentSaveSlot` | get | **done** | Slot number of the newest save (-1 if none). |

### CAdGame (the `Game` object, adventure part)

- WME source: src/engine_core/wme_ad/AdGame.cpp
- Hierarchy and Luma: CBGame → CAdGame; Luma: World (`game_call`, `game_get`, `game_set`, `responses.rs`, `inventory.rs`)
- Count: 56 done, 9 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `ChangeScene` | method | **done** | Changes scene at the start of the next frame (scene file, fade-in flag). |
| `LoadActor` | method | **done** | Loads a 2D actor from an `.actor` file and returns it. |
| `LoadActor3D` | method | **todo** | Loads a 3D actor (`.actor3d`, X model). 3D is not implemented. |
| `LoadEntity` | method | **todo** | Loads an entity from an `.entity` file at runtime. Stubbed. |
| `UnloadObject`, `UnloadActor`, `UnloadEntity`, `DeleteEntity` | method | **done** | Unloads an actor or entity loaded at runtime. |
| `UnloadActor3D` | method | **todo** | Unloads a 3D actor. 3D is not implemented. |
| `CreateEntity` | method | **todo** | Creates an empty entity at runtime. Stubbed. |
| `CreateItem` | method | **done** | Creates an inventory item at runtime. |
| `DeleteItem` | method | **done** | Deletes an inventory item. |
| `QueryItem` | method | **done** | Returns an item by name or index. |
| `AddResponse`, `AddResponseOnce`, `AddResponseOnceGame` | method | **done** | Adds a dialogue choice (id, text, icons, font); `Once` hides it after use in this scene / in the whole game. |
| `ResetResponse`, `ClearResponses` | method | **done** | Re-enables a used choice / clears all choices. |
| `GetResponse` | method | **done** | Shows the choices and waits for the player's pick. |
| `GetNumResponses` | method | **done** | Number of choices currently available. |
| `StartDlgBranch`, `EndDlgBranch`, `GetCurrentDlgBranch` | method | **done** | Dialogue-branch bookkeeping used by "once" responses. |
| `TakeItem`, `DropItem`, `GetItem`, `HasItem` | method | **done** | Inventory handling for the game object (one shared inventory). |
| `IsItemTaken` | method | **done** | True when the named item is in the inventory. |
| `GetInventoryWindow` | method | **done** | Returns the inventory window. |
| `GetResponsesWindow`, `GetResponseWindow` | method | **done** | Returns the dialogue responses window. |
| `LoadResponseBox` | method | **done** | Loads a different responses window definition. |
| `LoadInventoryBox` | method | **todo** | Loads a different inventory window definition at runtime. Stubbed (the one named in `default.game` is loaded at startup). |
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
| `InventoryObject` | get/set | **todo** | The object whose inventory is shown (each object has its own in WME). Luma has one shared inventory; the set is ignored. |
| `TalkSkipButton` | get/set | **done** | Mouse button that skips speech. |
| `VideoSkipButton` | get/set | **done** | Mouse button that skips videos. |
| `ChangingScene` | get | **done** | True while a scene change is pending. |
| `StartupScene` | get/set | **todo** | Scene loaded at startup (from `default.game`). |

### Window hook defined in CAdGame (`CAdGame::WindowScriptMethodHook`)

- WME source: src/engine_core/wme_ad/AdGame.cpp
- Hierarchy and Luma: adds a method to every window
- Count: 0 done, 1 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `CreateEntityContainer` | method | **todo** | Window method: creates an entity container control (UIEntity) at runtime. Stubbed. |

## 4. Scenes and adventure objects

### CAdScene

- WME source: src/engine_core/wme_ad/AdScene.cpp
- Hierarchy and Luma: CBObject → CAdScene; Luma: scene (`scene_call`, `scene_get`, `scene_set`, `scene_view.rs`)
- Count: 46 done, 35 todo, 2 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `LoadActor` | method | **done** | Loads a 2D actor into the scene. |
| `LoadActor3D` | method | **todo** | Loads a 3D actor. 3D is not implemented. |
| `LoadEntity` | method | **todo** | Loads an entity into the scene. Stubbed. |
| `CreateEntity` | method | **todo** | Creates an empty entity in the scene. Stubbed. |
| `UnloadObject`, `UnloadActor`, `UnloadEntity`, `DeleteEntity` | method | **done** | Removes a runtime-loaded object. |
| `UnloadActor3D` | method | **todo** | Removes a 3D actor. 3D is not implemented. |
| `SkipTo` | method | **done** | Jumps the view to a position or object. |
| `ScrollTo`, `ScrollToAsync` | method | **done** | Scrolls the view to a position or object, waiting or not. |
| `GetLayer` | method | **done** | Layer by index or name. |
| `GetWaypointGroup` | method | **todo** | Waypoint group by index. Stubbed (every group is used). |
| `GetNode` | method | **done** | Any node (entity or region) by name. |
| `GetFreeNode` | method | **done** | Free (layer-less) entity by index or name. |
| `GetRegionAt` | method | **done** | Region under a point (optionally including decorations). |
| `IsBlockedAt`, `IsWalkableAt` | method | **done** | Walkplane tests at a point. |
| `GetScaleAt`, `GetRotationAt` | method | **done** | Scale / rotation level at a point. |
| `IsScrolling` | method | **done** | True while the view scrolls. |
| `FadeOut`, `FadeOutAsync`, `FadeIn`, `FadeInAsync` | method | **todo** | Fades only the scene (not the UI). Stubbed. |
| `GetFadeColor` | method | **todo** | Colour of the scene fade. |
| `IsPointInViewport` | method | **done** | True when a point is inside the scene viewport. |
| `EnableNode3D`, `DisableNode3D`, `IsNode3DEnabled` | method | **todo** | Shows / hides / queries a mesh node of the scene's 3D geometry. |
| `EnableLight`, `DisableLight`, `IsLightEnabled`, `GetLightName`, `SetLightColor`, `GetLightColor`, `GetLightPosition` | method | **todo** | Controls the lights of the scene's 3D geometry. |
| `SetActiveCamera` | method | **todo** | Selects a camera of the 3D geometry. |
| `SetViewport` | method | **todo** | Sets the scene's viewport rectangle. Stubbed. |
| `AddLayer`, `InsertLayer`, `DeleteLayer` | method | **todo** | Adds, inserts or deletes a layer at runtime. Stubbed. |
| `EnableFog`, `DisableFog` | method | **todo** | 3D fog (colour, start, end). |
| `Type` | get | **done** | `scene`. |
| `NumLayers`, `NumWaypointGroups`, `MainLayer`, `NumFreeNodes` | get | **done** | Counts and the main layer. |
| `MouseX`, `MouseY` | get | **done** | Mouse position in scene coordinates. |
| `AutoScroll` | get/set | **done** | Whether the view follows the main object. |
| `ShowGeometry` | get/set | **no-op** | Draws the walkplane / geometry as a debug overlay (editor aid). |
| `PersistentState` | get/set | **done** | Whether the scene keeps its state between visits. |
| `PersistentStateSprites` | get/set | **done** | Whether sprite state is kept too. |
| `ScrollPixelsX`, `ScrollPixelsY`, `ScrollSpeedX`, `ScrollSpeedY` | get/set | **done** | Auto-scroll step and speed. |
| `OffsetX`, `OffsetY` | get/set | **done** | Current view offset. |
| `GeometryFile` | get | **todo** | The 3D geometry file. 3D is not implemented. |
| `WaypointsHeight` | get/set | **todo** | Height of 3D waypoints. |
| `Width`, `Height` | get | **done** | Scene size in pixels. |
| `MaxShadowType` | get/set | **todo** | Highest 3D shadow quality for the scene. |
| `AmbientLightColor` | get/set | **todo** | 3D ambient colour. |
| `NumLights` | get | **todo** | Number of 3D lights. |
| `Name` | set | **done** | Renames the scene. |

### CAdLayer

- WME source: src/engine_core/wme_ad/AdLayer.cpp
- Hierarchy and Luma: CBObject → CAdLayer; Luma: layers (`layer_call`)
- Count: 13 done, 5 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetNode` | method | **done** | Node by index or name. |
| `AddRegion`, `InsertRegion`, `AddEntity`, `InsertEntity` | method | **todo** | Adds a new region / entity to the layer. Stubbed. |
| `DeleteNode` | method | **todo** | Deletes a node. Stubbed. |
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
- Count: 7 done, 4 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `AddPoint`, `InsertPoint`, `SetPoint`, `RemovePoint` | method | **todo** | Edits the region's polygon. Stubbed. |
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
- Hierarchy and Luma: CBObject → CAdWaypointGroup; Luma: not exposed (all groups always active)
- Count: 0 done, 3 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Type` | get | **todo** | `waypoint-group`. |
| `Active` | get/set | **todo** | Enables or disables the group for path finding. |

### CAdObject (entities, actors, items: adventure objects)

- WME source: src/engine_core/wme_ad/AdObject.cpp
- Hierarchy and Luma: CBObject → CAdObject → CAdTalkHolder / CAdObject3D; Luma: entities, actors (`entity_call`, `actor_call`)
- Count: 30 done, 11 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `PlayAnim`, `PlayAnimAsync` | method | **done** | Plays a sprite once in place of the object's own, optionally waiting. |
| `Reset` | method | **done** | Returns to the idle state; stops talking and animations. |
| `IsTalking` | method | **done** | True while a sentence plays. |
| `StopTalk`, `StopTalking` | method | **done** | Cuts the current sentence. |
| `ForceTalkAnim` | method | **todo** | Forces a named talk animation for the next sentence. Stubbed. |
| `Talk`, `TalkAsync` | method | **done** | Says a sentence (text, sound, duration, stances, align). |
| `StickToRegion` | method | **todo** | Constrains a free object to move only inside a region. Stubbed. |
| `SetFont`, `GetFont` | method | **done** | Sets / reads the font used for the object's speech. |
| `TakeItem`, `DropItem`, `GetItem`, `HasItem` | method | **done** | Inventory handling (shared inventory in Luma). |
| `CreateParticleEmitter` | method | **todo** | Creates a particle emitter attached to the object. Particles are not implemented. |
| `DeleteParticleEmitter` | method | **todo** | Deletes the object's emitter. |
| `AddAttachment` | method | **todo** | Attaches a sprite or 3D model to the object (file, bone/offset, ...). Stubbed. |
| `RemoveAttachment` | method | **todo** | Removes an attachment. |
| `GetAttachment` | method | **todo** | Returns an attachment by index or name. Luma returns null. |
| `Type` | get | **done** | Class name of the object. |
| `Active` | get/set | **done** | Whether the object is shown and active. |
| `IgnoreItems` | get/set | **done** | Whether a selected item may be used on the object. |
| `SceneIndependent` | get/set | **todo** | Object stays in the scene tree across scene changes. Luma reads false, ignores the set. |
| `SubtitlesWidth`, `SubtitlesPosRelative`, `SubtitlesPosX`, `SubtitlesPosY`, `SubtitlesPosXCenter` | get/set | **done** | Placement of the object's subtitles. |
| `NumItems` | get | **done** | Number of items in the object's inventory. |
| `ParticleEmitter` | get | **todo** | The object's particle emitter. Luma reads null. |
| `NumAttachments` | get | **todo** | Number of attachments. Luma reads 0. |

### CAdTalkHolder (base of actors, entities, items)

- WME source: src/engine_core/wme_ad/AdTalkHolder.cpp
- Hierarchy and Luma: CAdObject → CAdTalkHolder; Luma: entities, actors, items
- Count: 3 done, 4 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetSprite` | method | **done** | Sets the object's default sprite. |
| `GetSprite` | method | **done** | File of the current default sprite. |
| `GetSpriteObject` | method | **todo** | The default sprite as a script object. Done for entities, stubbed for actors and items. |
| `AddTalkSprite` | method | **todo** | Adds a sprite to the talk animation set (file, "ex" flag). Stubbed. |
| `RemoveTalkSprite` | method | **todo** | Removes a talk sprite. Stubbed. |
| `SetTalkSprite` | method | **todo** | Replaces the talk sprite set with one sprite. Stubbed. |
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
- Count: 11 done, 9 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `StopSound` | method | **done** | Stops the entity's sound. |
| `PlayTheora` | method | **todo** | Plays an Ogg Theora video inside the entity's rectangle (file, looping, ...). |
| `StopTheora`, `IsTheoraPlaying`, `PauseTheora`, `ResumeTheora`, `IsTheoraPaused` | method | **todo** | Controls the entity-bound video. |
| `CreateRegion` | method | **todo** | Creates the entity's own clickable region. Stubbed. |
| `DeleteRegion` | method | **todo** | Deletes that region. Stubbed. |
| `Type` | get | **done** | `entity`. |
| `Item` | get/set | **done** | Item the entity represents. |
| `Subtype` | get | **done** | `normal` or `sound`. |
| `WalkToX`, `WalkToY`, `WalkToDirection` | get/set | **done** | Where actors stand when walking to the entity, and which way they face. |
| `Region` | get | **todo** | The entity's region object. Luma reads null. |

### CAdItem (inventory item)

- WME source: src/engine_core/wme_ad/AdItem.cpp
- Hierarchy and Luma: CAdTalkHolder → CAdItem; Luma: items (`natives_item.rs`)
- Count: 23 done, 3 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetHoverSprite` | method | **done** | Sets the image shown while hovering the item in the inventory. |
| `GetHoverSprite` | method | **done** | File of that image. |
| `GetHoverSpriteObject` | method | **todo** | Hover sprite as a script object. Stubbed. |
| `SetNormalCursor`, `GetNormalCursor`, `SetHoverCursor`, `GetHoverCursor` | method | **done** | Sets / reads the cursor used when the item is selected (normal / over a target). |
| `GetNormalCursorObject`, `GetHoverCursorObject` | method | **todo** | Cursor sprites as script objects. Stubbed. |
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
- Count: 37 done, 3 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetWidget`, `GetControl` | method | **done** | A child control by name or index. |
| `SetInactiveFont`, `SetInactiveImage`, `GetInactiveImage` | method | **done** | Font / image used while the window is not focused. |
| `GetInactiveImageObject` | method | **todo** | The inactive image as a sprite object. Stubbed. |
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
| `Exclusive`, `SystemExclusive` | set | **todo** | Switches the window into / out of modal mode. Luma stores the value as a plain script property. |
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
- Count: 22 done, 4 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetDisabledFont`, `SetHoverFont`, `SetPressedFont`, `SetFocusedFont` | method | **done** | Fonts for each button state. |
| `SetDisabledImage`, `SetHoverImage`, `SetPressedImage`, `SetFocusedImage` | method | **done** | Images for each button state. |
| `GetDisabledImage`, `GetHoverImage`, `GetPressedImage`, `GetFocusedImage` | method | **done** | Image files of each state. |
| `GetDisabledImageObject`, `GetHoverImageObject`, `GetPressedImageObject`, `GetFocusedImageObject` | method | **todo** | Images of each state as sprite objects. Stubbed. |
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
- Count: 3 done, 2 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetEntity` | method | **done** | The entity hosted by the container. |
| `SetEntity` | method | **done** | Loads an entity file into the container. |
| `Type` | get | **done** | `entity container`. |
| `Freezable` | get/set | **todo** | Whether the entity freezes with the game. Luma reads false, ignores the set. |

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
| `Abs`, `Acos`, `Asin`, `Atan`, `Atan2`, `Ceil`, `Cos`, `Cosh`, `Exp`, `Floor`, `Log`, `Log10`, `Pow`, `Sin`, `Sinh`, `Tan`, `Tanh`, `Sqrt`, `DegToRad`, `RadToDeg` | method | **done** | The C math function of that name on floats; every result is a float (Floor and Ceil included). |
| `Type` | get | **done** | `math`. |
| `PI` | get | **done** | The constant pi. |

### CSXString (`new String`)

- WME source: src/engine_core/wme_base/SXString.cpp
- Hierarchy and Luma: CBScriptable → CSXString; Luma: inert object
- Count: 0 done, 10 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Substring`, `Substr` | method | **todo** | Part of the string by (start, end) or (start, length). |
| `ToUpperCase`, `ToLowerCase` | method | **todo** | Case conversion. |
| `IndexOf` | method | **todo** | Position of a substring from an index. |
| `Split` | method | **todo** | Splits on a separator and returns an array. |
| `Type` | get | **todo** | `string`. |
| `Length` | get | **todo** | Length of the string object. (Length of a plain string value is done; see Script values.) |
| `Capacity` | get/set | **todo** | Allocated buffer size. |

### CSXArray (`new Array`)

- WME source: src/engine_core/wme_base/SXArray.cpp
- Hierarchy and Luma: CBScriptable → CSXArray; Luma: inert object (numeric properties work as a bag, no `Length` tracking)
- Count: 0 done, 5 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Push` | method | **todo** | Appends one or more values; returns the new length. |
| `Pop` | method | **todo** | Removes and returns the last value. |
| `Type` | get | **todo** | `array`. |
| `Length` | get/set | **todo** | Number of elements; setting it truncates. Element access `a[n]` is a numeric property. |

### CSXDate (`new Date`)

- WME source: src/engine_core/wme_base/SXDate.cpp
- Hierarchy and Luma: CBScriptable → CSXDate; Luma: inert object
- Count: 0 done, 15 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `GetYear`, `GetMonth`, `GetDate`, `GetHours`, `GetMinutes`, `GetSeconds`, `GetWeekday` | method | **todo** | Reads a field of the date (local time). |
| `SetYear`, `SetMonth`, `SetDate`, `SetHours`, `SetMinutes`, `SetSeconds` | method | **todo** | Writes a field of the date. |
| `SetCurrentTime` | method | **todo** | Sets the object to the current date and time. |
| `Type` | get | **todo** | `date`. |

### CSXFile (`new File`)

- WME source: src/engine_core/wme_base/SXFile.cpp
- Hierarchy and Luma: CBScriptable → CSXFile; Luma: inert object. Needs a policy for where scripts may read and write (saves, settings).
- Count: 0 done, 33 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetFilename` | method | **todo** | Sets the file name. |
| `OpenAsText`, `OpenAsBinary` | method | **todo** | Opens the file for read / write / append as text or binary. |
| `Close` | method | **todo** | Closes the file. |
| `SetPosition` | method | **todo** | Seeks. |
| `Delete`, `Copy` | method | **todo** | Deletes the file / copies it to a new name. |
| `ReadLine`, `ReadText` | method | **todo** | Reads a line / N characters of text. |
| `WriteLine`, `WriteText` | method | **todo** | Writes text with / without a line break. |
| `ReadBool`, `ReadByte`, `ReadShort`, `ReadInt`, `ReadLong`, `ReadFloat`, `ReadDouble`, `ReadString` | method | **todo** | Reads a binary value. |
| `WriteBool`, `WriteByte`, `WriteShort`, `WriteInt`, `WriteLong`, `WriteFloat`, `WriteDouble`, `WriteString` | method | **todo** | Writes a binary value. |
| `Type` | get | **todo** | `file`. |
| `Filename`, `Position`, `Length`, `TextMode`, `AccessMode` | get | **todo** | Current file state. |

### CSXDirectory (the global `Directory`)

- WME source: src/engine_core/wme_base/SXDirectory.cpp
- Hierarchy and Luma: CBScriptable → CSXDirectory; Luma: not published
- Count: 0 done, 9 todo, 1 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Create`, `Delete` | method | **todo** | Creates / deletes a directory. |
| `GetFiles`, `GetDirectories` | method | **todo** | Lists files / sub-directories matching a pattern. |
| `GetDrives` | method | **no-op** | Lists Windows drive letters; no meaning on macOS. |
| `Type` | get | **todo** | `directory`. |
| `PathSeparator` | get | **todo** | The path separator character. |
| `CurrentDirectory` | get/set | **todo** | Working directory. |
| `TempDirectory` | get | **todo** | Temporary files directory. |

### CSXMemBuffer (`new MemBuffer`)

- WME source: src/engine_core/wme_base/SXMemBuffer.cpp
- Hierarchy and Luma: CBScriptable → CSXMemBuffer; Luma: inert object
- Count: 0 done, 19 todo, 3 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetSize` | method | **todo** | Resizes the buffer. |
| `GetBool`, `GetByte`, `GetShort`, `GetInt`, `GetLong`, `GetFloat`, `GetDouble`, `GetString` | method | **todo** | Reads a value at a byte offset. |
| `SetBool`, `SetByte`, `SetShort`, `SetInt`, `SetLong`, `SetFloat`, `SetDouble`, `SetString` | method | **todo** | Writes a value at a byte offset. |
| `GetPointer`, `SetPointer` | method | **no-op** | Reads / writes a raw memory address, for calling Windows DLL functions. |
| `DEBUG_Dump` | method | **no-op** | Dumps the buffer to the log; debugging aid. |
| `Type` | get | **todo** | `membuffer`. |
| `Size` | get | **todo** | Buffer size in bytes. |

### CSXTcpClient (`new TcpClient`)

- WME source: src/engine_core/wme_base/SXTcpClient.cpp
- Hierarchy and Luma: CBScriptable → CSXTcpClient; unfinished in WME 1.x (methods commented out)
- Count: 0 done, 0 todo, 1 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `Type` | get | **no-op** | `tcp-client`. No other member is active in WME 1.x. |

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
- Hierarchy and Luma: CBObject → CPartEmitter; Luma: absent (no particles)
- Count: 0 done, 79 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SetBorder`, `SetBorderThickness` | method | **todo** | Sets the emitter's bounding rectangle and its soft border. |
| `AddSprite`, `RemoveSprite` | method | **todo** | Adds / removes a particle image. |
| `Start`, `Stop`, `Pause`, `Resume` | method | **todo** | Controls emission. |
| `AddGlobalForce`, `AddPointForce`, `RemoveForce` | method | **todo** | Adds a wind-like force or point attractor / removes it. |
| `Type` | get | **todo** | `particle-emitter`. |
| `X`, `Y`, `Width`, `Height` | get/set | **todo** | Emission rectangle. |
| `Scale1`, `Scale2`, `ScaleZBased` | get/set | **todo** | Particle scale range, optionally by depth. |
| `Velocity1`, `Velocity2`, `VelocityZBased` | get/set | **todo** | Speed range. |
| `LifeTime1`, `LifeTime2`, `LifeTimeZBased` | get/set | **todo** | Lifetime range. |
| `Angle1`, `Angle2`, `AngVelocity1`, `AngVelocity2`, `Rotation1`, `Rotation2` | get/set | **todo** | Direction, spin and initial rotation ranges. |
| `Alpha1`, `Alpha2`, `AlphaTimeBased` | get/set | **todo** | Opacity range. |
| `MaxParticles`, `GenerationInterval`, `GenerationAmount`, `MaxBatches` | get/set | **todo** | Emission rate and caps. |
| `NumLiveParticles` | get | **todo** | Particles alive now. |
| `FadeInTime`, `FadeOutTime` | get/set | **todo** | Fade durations. |
| `GrowthRate1`, `GrowthRate2`, `ExponentialGrowth` | get/set | **todo** | Size growth. |
| `UseRegion` | get/set | **todo** | Emits inside a region instead of the rectangle. |
| `EmitEvent` | get/set | **todo** | Script event raised on the owner at each emission. |

## 10. 3D objects

### CAdObject3D (base of 3D actors)

- WME source: src/engine_core/wme_ad/AdObject3D.cpp
- Hierarchy and Luma: CAdObject → CAdObject3D → CAdActor3D / CAdActor3DX / CAdAttach3DX; Luma: absent (no 3D)
- Count: 0 done, 38 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `SkipTo` | method | **todo** | Moves the object to (x, y) on the 3D floor immediately. |
| `SkipTo3D` | method | **todo** | Moves the object to (x, y, z) immediately. |
| `GetBonePosition2D` | method | **todo** | Screen position of a skeleton bone. |
| `GetBonePosition3D` | method | **todo** | World position of a skeleton bone. |
| `AddIgnoredLight`, `RemoveIgnoredLight` | method | **todo** | Excludes / re-includes a scene light when lighting the object. |
| `Angle` | get/set | **todo** | Heading in degrees. |
| `PosX`, `PosY`, `PosZ` | get/set | **todo** | World position. |
| `X`, `Y` | set | **todo** | 2D alias of the world position. |
| `Velocity` | get/set | **todo** | Walking speed. |
| `AngularVelocity`, `AngVelocity` | get/set | **todo** | Turning speed. |
| `DropToFloor` | get/set | **todo** | Keeps the object on the floor mesh. |
| `ShadowType`, `Shadow` | get/set | **todo** | Shadow kind (none, simple, flat, stencil). |
| `SimpleShadow` | get/set | **todo** | Uses the simple blob shadow. |
| `ShadowColor` | get/set | **todo** | Shadow colour. |
| `Scale` | get/set | **todo** | Uniform scale of the model. |
| `DrawBackfaces` | get/set | **todo** | Draws back faces of the mesh. |
| `AmbientLightColor` | get/set | **todo** | Per-object ambient colour. |

### CAdActor3D (legacy 3D actor, MS3D/3DS models)

- WME source: src/engine_core/wme_ad/AdActor3D.cpp
- Hierarchy and Luma: CAdObject3D → CAdActor3D; Luma: absent
- Count: 0 done, 40 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `PlayAnim`, `PlayAnimAsync` | method | **todo** | Plays a named animation, optionally waiting. |
| `StopAnim` | method | **todo** | Stops the current animation. |
| `StopAnimChannel`, `PlayAnimChannel`, `PlayAnimChannelAsync` | method | **todo** | Stops / plays an animation on a numbered blend channel. |
| `HideMesh`, `ShowMesh` | method | **todo** | Hides / shows a named mesh of the model. |
| `AddMesh`, `RemoveMesh` | method | **todo** | Adds / removes a mesh from another model file. |
| `GoTo3D`, `GoTo3DAsync` | method | **todo** | Walks to (x, y, z). |
| `GoTo`, `GoToAsync` | method | **todo** | Walks to (x, y) on the floor. |
| `GoToObject`, `GoToObjectAsync` | method | **todo** | Walks to an object. |
| `TurnTo`, `TurnToAsync` | method | **todo** | Turns toward a point or object. |
| `TurnToAngle`, `TurnToAngleAsync` | method | **todo** | Turns to an absolute angle. |
| `IsWalking` | method | **todo** | True while walking. |
| `DirectWalk`, `DirectWalkBack`, `DirectWalkStop` | method | **todo** | Free keyboard-style walking forward / backward / stop. |
| `DirectTurnLeft`, `DirectTurnRight`, `DirectTurnStop` | method | **todo** | Free turning left / right / stop. |
| `Type` | get | **todo** | `actor3d`. |
| `TalkAnimName`, `WalkAnimName`, `IdleAnimName` | get/set | **todo** | Animation names for talk, walk and idle. |
| `DirectionAngle`, `DirAngle`, `Direction` | get/set | **todo** | Facing angle. |

### CAdActor3DX (3D actor, X-file models with skeletal animation)

- WME source: src/engine_core/wme_ad/AdActor3DX.cpp
- Hierarchy and Luma: CAdObject3D → CAdActor3DX; Luma: absent
- Count: 0 done, 67 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `PlayAnim`, `PlayAnimAsync` | method | **todo** | Plays a named animation, optionally waiting. |
| `StopAnim` | method | **todo** | Stops an animation with a transition time. |
| `StopAnimChannel`, `PlayAnimChannel`, `PlayAnimChannelAsync` | method | **todo** | Controls animations on numbered blend channels. |
| `IsAnimPlaying`, `IsAnimChannelPlaying` | method | **todo** | True when an animation (on a channel) plays. |
| `AddAttachment`, `RemoveAttachment`, `GetAttachment` | method | **todo** | Attaches another model to a bone, removes it, or returns it. |
| `AddMesh`, `RemoveMesh` | method | **todo** | Adds / removes meshes from another model file. |
| `GoTo3D`, `GoTo3DAsync`, `GoTo`, `GoToAsync`, `GoToObject`, `GoToObjectAsync` | method | **todo** | Walking orders (3D point, floor point, object). |
| `TurnTo`, `TurnToAsync`, `TurnToAngle`, `TurnToAngleAsync` | method | **todo** | Turning orders. |
| `IsWalking` | method | **todo** | True while walking. |
| `DirectWalk`, `DirectWalkBack`, `DirectWalkStop`, `DirectTurnLeft`, `DirectTurnRight`, `DirectTurnStop` | method | **todo** | Free walking / turning. |
| `SetTexture` | method | **todo** | Replaces a material's texture. |
| `SetTheoraTexture` | method | **todo** | Uses a playing video as a material's texture. |
| `SetEffect`, `RemoveEffect` | method | **todo** | Applies / removes a shader effect on a material. |
| `SetEffectParam`, `SetEffectParamVector`, `SetEffectParamColor` | method | **todo** | Sets shader effect parameters. |
| `MergeAnims` | method | **todo** | Merges animation sets from another file. |
| `UnloadAnim` | method | **todo** | Unloads a named animation set. |
| `SetAnimTransitionTime`, `GetAnimTransitionTime` | method | **todo** | Sets / reads the blend time between two animations. |
| `CreateParticleEmitterBone` | method | **todo** | Creates a particle emitter attached to a bone. |
| `Type` | get | **todo** | `actor3dx`. |
| `TalkAnimName`, `TalkAnimChannel`, `WalkAnimName`, `IdleAnimName`, `TurnLeftAnimName`, `TurnRightAnimName` | get/set | **todo** | Animation names / channel for talk, walk, idle, turn. |
| `DirectionAngle`, `DirAngle`, `Direction` | get/set | **todo** | Facing angle. |
| `AnimTransitionTime`, `AnimStopTransitionTime` | get/set | **todo** | Default blend times. |
| `GoToTolerance` | get/set | **todo** | Distance at which a walk counts as arrived. |

### CAdAttach3DX (model attached to a 3D actor bone)

- WME source: src/engine_core/wme_ad/AdAttach3DX.cpp
- Hierarchy and Luma: CAdObject3D → CAdAttach3DX; Luma: absent
- Count: 0 done, 7 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `PlayAnim`, `PlayAnimAsync`, `StopAnim`, `StopAnimChannel`, `PlayAnimChannel`, `PlayAnimChannelAsync` | method | **todo** | Animation control of the attached model. |
| `Type` | get | **todo** | `attachment`. |

## 11. Events

### Events raised on objects (`on "Name" { ... }` handlers)

- WME source: src/engine_core/wme_base/BGame.cpp, wme_ad/AdGame.cpp, wme_ad/AdObject.cpp, wme_base/UIButton.cpp, wme_base/BSprite.cpp (search `ApplyEvent("`)
- Hierarchy and Luma: luma-adventure/src/world.rs (`mouse_event`, `key_down`), ui.rs, scene_state.rs
- Count: 15 done, 9 todo, 0 no-op

| WME name | Kind | State | Note |
|----------|------|-------|------|
| `LeftClick`, `LeftRelease`, `RightClick`, `RightRelease` | event | **done** | Mouse button events: raised on the game, then on the object under the mouse or the scene. |
| `LeftDoubleClick` | event | **done** | Double click of the left button. |
| `RightDoubleClick` | event | **todo** | Double click of the right button (BGame.cpp, same dispatch as the left one). |
| `MiddleClick`, `MiddleRelease` | event | **todo** | Middle button events. |
| `MouseWheelUp`, `MouseWheelDown` | event | **todo** | Wheel events: raised on the focused window if it handles them, else on the game. |
| `MouseEntry`, `MouseLeave` | event | **done** | The object under the mouse changes. |
| `Keypress` | event | **done** | A non-printable key, on the focused window if it handles the event, else on the game. |
| `Press` | event | **done** | A button was released over itself; with `ParentNotify`, also an event named after the button on its window. |
| `QuitGame` | event | **todo** | The player closed the window; raised on the game if a handler exists (otherwise the game quits). |
| `SceneShutdown` | event | **done** | The scene is about to be left (unbreakable). |
| `BeforeSave`, `AfterLoad` | event | **done** | Around saving and loading (unbreakable). |
| `ActorEntry`, `ActorLeave` | event | **done** | An actor enters / leaves a region of the main layer (raised on the region). |
| (sound event name) | event | **done** | The name given to `PlaySoundEvent`, raised on the object when the sound ends. |
| `FrameChanged` | event | **todo** | Raised on a sprite when its current frame changes. |
| (frame event names) | event | **todo** | Events attached to a frame (`APPLY_EVENT` in sprite files, `AddEvent`), raised on the sprite's owner when the frame shows. The sprite loader reads them; the adventure layer does not raise them. |
| (emit event name) | event | **todo** | The particle emitter's `EmitEvent`, raised on its owner at each emission. |

## Summary per class

| Class | done | todo | no-op | total |
|-------|-----:|-----:|------:|------:|
| Global functions | 16 | 4 | 1 | 21 |
| Constructors | 0 | 8 | 1 | 9 |
| Global variables | 4 | 1 | 0 | 5 |
| Script values | 1 | 0 | 0 | 1 |
| CBScriptHolder | 10 | 0 | 1 | 11 |
| CBObject | 45 | 27 | 0 | 72 |
| CBGame | 90 | 58 | 17 | 165 |
| CAdGame | 56 | 9 | 0 | 65 |
| Window hook defined in CAdGame | 0 | 1 | 0 | 1 |
| CAdScene | 46 | 35 | 2 | 83 |
| CAdLayer | 13 | 5 | 0 | 18 |
| CBRegion | 7 | 4 | 0 | 11 |
| CAdRegion | 11 | 0 | 0 | 11 |
| CAdWaypointGroup | 0 | 3 | 0 | 3 |
| CAdObject | 30 | 11 | 0 | 41 |
| CAdTalkHolder | 3 | 4 | 0 | 7 |
| CAdActor | 23 | 0 | 0 | 23 |
| CAdEntity | 11 | 9 | 0 | 20 |
| CAdItem | 23 | 3 | 0 | 26 |
| CBSprite | 18 | 0 | 0 | 18 |
| CBFrame | 21 | 0 | 0 | 21 |
| CBSubFrame | 20 | 0 | 0 | 20 |
| CUIObject | 27 | 0 | 0 | 27 |
| CUIWindow | 37 | 3 | 0 | 40 |
| CUIButton | 22 | 4 | 0 | 26 |
| CUIText | 7 | 0 | 0 | 7 |
| CUIEdit | 16 | 0 | 0 | 16 |
| CUIEntity | 3 | 2 | 0 | 5 |
| CUIMarkup | 0 | 0 | 1 | 1 |
| CBKeyboardState | 8 | 0 | 0 | 8 |
| CSXMath | 22 | 0 | 0 | 22 |
| CSXString | 0 | 10 | 0 | 10 |
| CSXArray | 0 | 5 | 0 | 5 |
| CSXDate | 0 | 15 | 0 | 15 |
| CSXFile | 0 | 33 | 0 | 33 |
| CSXDirectory | 0 | 9 | 1 | 10 |
| CSXMemBuffer | 0 | 19 | 3 | 22 |
| CSXTcpClient | 0 | 0 | 1 | 1 |
| CSXPluginObject | 0 | 0 | 1 | 1 |
| CPartEmitter | 0 | 79 | 0 | 79 |
| CAdObject3D | 0 | 38 | 0 | 38 |
| CAdActor3D | 0 | 40 | 0 | 40 |
| CAdActor3DX | 0 | 67 | 0 | 67 |
| CAdAttach3DX | 0 | 7 | 0 | 7 |
| Events raised on objects | 15 | 9 | 0 | 24 |
| **All** | **605** | **522** | **29** | **1156** |

Of the total, 24 are events (15 done, 9 todo) and the rest are methods, properties, functions, constructors and global variables: 590 done, 513 todo, 29 no-op, 1132 in all.

## WME classes Luma lacks entirely

These classes have no done member in Luma: the object does not exist for scripts, or is only an inert placeholder returned by `new`.

| Class | What it is | Members still todo |
|-------|-----------|-------------------:|
| `CAdWaypointGroup` | A waypoint group of a scene (`Scene.GetWaypointGroup`) | 3 |
| `CSXString` | `new String` | 10 |
| `CSXArray` | `new Array` | 5 |
| `CSXDate` | `new Date` | 15 |
| `CSXFile` | `new File` | 33 |
| `CSXDirectory` | `Directory` | 9 |
| `CSXMemBuffer` | `new MemBuffer` | 19 |
| `CPartEmitter` | Particle emitter | 79 |
| `CAdObject3D` | Base of 3D objects: position, velocity, shadows, bones, ignored lights | 38 |
| `CAdActor3D` | Legacy 3D actor (MS3D/3DS models) | 40 |
| `CAdActor3DX` | 3D actor with X-file models and skeletal animation, effects, attachments | 67 |
| `CAdAttach3DX` | Model attached to a bone of a 3D actor | 7 |

Classes that exist in WME 1.x with a script interface but that are no-op by design: `CSXTcpClient` (unfinished in WME itself), `CSXPluginObject` (Windows DLL plugins), `CUIMarkup` (never instantiated), and the Windows-only parts of `CSXDirectory`/`CSXMemBuffer`.

WME classes with no script interface of their own (nothing to port; scripts reach them only through the owner's methods): `CBSound`, `CBFont`/`CBFontBitmap`/`CBFontTT`, `CAdInventory`, `CAdInventoryBox`, `CAdResponseBox`, `CAdResponse`, `CAdSceneNode`, `CAdSentence`, `CAdSpriteSet`, `CAdTalkDef`, `CAdScaleLevel`, `CAdRotLevel`, `CAdWalkplane`, `CAdBlock`, `CAdGeneric`, `CBViewport`, `CBFader`, `CUITiledImage`, `CBPersistMgr`, `CBEventSet`, the video players (`VidTheoraPlayer`), `C3DLight`, `C3DModel`. The inventory, responses, fader and viewport are driven through `Game` methods and properties (`GetInventoryWindow`, `AddResponse`, `FadeOut`, `SetSceneViewport`), which the tables above cover.

## Groups the work falls into

Counts are the todo members (no-op excluded). Groups are ordered by what a game port is most likely to need first, not by size.

| Group | todo | What it covers |
|-------|-----:|----------------|
| Game services | 46 | Persistent private settings (`RegRead*`/`RegWrite*`), screenshots and thumbnails, loading/saving screens and icons, `Reset`, `LockMouseRect`, `OpenDocument`, `GetFileChecksum`, real values for the volume, sound, FPS and encoding properties, `StartupScene`, the game-level halves of runtime object creation. Mostly small and independent. |
| Script library | 103 | `String`, `Array`, `Date`, `File`, `MemBuffer` and `Directory` objects, `new Object` with scripts, `new Actor` and `new Entity`, and the HSL colour helpers. Needed by scripts that parse text, keep arrays, read dates or write settings files; the `File` object needs a policy for where a script may read and write. |
| Object model (2D) | 47 | Runtime creation and editing (`CreateEntity`, `LoadEntity`, `AddLayer`, `AddRegion`, region points, waypoint groups, entity `Region`), per-object cursors, `SetCaption`, `StickToRegion`, `ForceTalkAnim`, talk sprite sets, attachments, `Movable`, `SceneIndependent`, `NonIntMouseEvents`, per-object inventories, `GetSpriteObject` on actors and items. |
| Rendering features | 15 | Rotation (`Rotate`, `RelativeRotate`, `ROTATABLE` entities), per-axis scale (`ScaleX`, `ScaleY`), blend modes, scene-only fades. Needs renderer work first (see `docs/rendering.md`). |
| Sprites and frames | 2 | The frame events (`FrameChanged`, the frames' `APPLY_EVENT` names); the frame and sub-frame objects are done. |
| Windows and controls | 10 | State-image objects (`Get...ImageObject`), `CreateEntityContainer`, settable `Exclusive`, `Freezable`. |
| Video | 7 | Entity-bound Theora video (`PlayTheora` on an entity and its pause/stop family), AVI for the deprecated `PlayVideo`. |
| Sound effects | 2 | `SoundFXEcho`, `SoundFXReverb` (also listed in `docs/audio.md`). |
| Input and system events | 6 | Wheel, middle and right-double-click events, the `QuitGame` event. |
| Accessibility | 12 | `AccTTS*`, `AccKeyboard*`, `AccOutputText`, `AccCaption`. On macOS this would sit on the system speech synthesizer. |
| Particles | 83 | The `CPartEmitter` class and the object methods that create emitters (`CreateParticleEmitter`, `DeleteParticleEmitter`, `ParticleEmitter`). |
| 3D | 189 | `CAdActor3D`, `CAdActor3DX`, `CAdAttach3DX`, `CAdObject3D`, the scene's geometry, lights, fog, cameras, shadows, effects and textures on 3D models. A separate module in the sense of `docs/modules.md`: renderer, loaders (X files, MS3D/3DS), animation, lighting and shadows, all missing. |
| **All** | **522** | |

## Members in Luma that WME 1.x does not define

These are accepted by Luma and not counted above; some come from later WME versions, some from Luma itself.

| Object | Members | Note |
|--------|---------|------|
| Game | `HideCursor`, `ShowCursor` | Hide / show the mouse cursor. |
| Game | `IsVideoPlaying`, `StopVideo` | Query / stop the running video (`StopVideo` is an alias of `StopTheora`). |
| Game | `FocusedWindow`, `CursorHidden`, `AutoSaveOnExit`, `AutoSaveSlot`, `Platform`, `DeviceType` | Properties Luma reads that WME 1.x does not define in its script interface (some come from later WME versions or from Luma itself). |
| Actor | `SetStopOnBlocked`, `GetStopOnBlocked`, `ActorIsBlocked` | Stop-on-blocked behaviour and the matching event / property. |
| Sprite | `Filename` | The file the sprite was loaded from. |

## Maintenance

Update this file in the same commit as any change to `natives*.rs`, to the `*_STUBS` lists or to the built-ins of `luma-script`: flip the row to **done**, and delete its stub entry. Then recount the summary tables: the per-class numbers are computed from the rows (a name counts once, a `get/set` row twice).
