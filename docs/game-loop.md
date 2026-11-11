# The game loop, the mouse and the keyboard

How Luma Engine runs a frame of a game, and how the player's mouse and
keyboard reach the scripts. The rules follow the original engine (WME
Lite's `BGame`, `AdGame`, `BRenderer`, `BKeyboardState`, `BFader`,
`BTransitionMgr`, MIT) so that games react as their authors expect.

## A frame

`luma-player <game>` opens a window at a whole multiple of the game's
resolution (the largest that keeps the window at most 1280 points wide,
or `--scale <n>`; the frame is letterboxed when the window has another
shape, see [rendering.md](rendering.md#coordinates)), loads `default.game`, attaches the
game's scripts and schedules its `STARTUP_SCENE`. Every frame then runs,
in this order (`Runtime::tick`):

1. a game load asked last frame (`Game.LoadGame`) happens, replacing
   everything below with the saved state (see [saving.md](saving.md));
   the timer advances by the frame's duration, at most a second;
2. a scene change asked last frame happens (`SceneShutdown`, the old
   scene's state noted, the new scene loaded and its remembered state
   put back, its scripts attached), with a fade from black of 200 ms
   when asked;
3. speech sounds that ended end their sentences; the fader advances;
4. the scripts run (`Engine::tick`);
5. the scene's entities and the game's own (see
   [formats/scene.md](formats/scene.md#entities-of-the-game-and-attachments))
   animate and talk; the actors walk, turn, talk; a sprite frame that
   becomes current plays its `SOUND`, stops the sprite's earlier frame
   sounds when marked `KILL_SOUND`, raises `FrameChanged` on the sprite
   object and its `APPLY_EVENT`s on the object showing the sprite; in a
   scene with 3D geometry (the `3d` feature), the scene's path finder
   computes the path asked last frame, then the 3D actors walk, turn,
   talk and animate, their model's animation events reaching their
   scripts by name (see [3d-actors.md](3d-actors.md), section 8); the
   particle emitters of the entities and actors follow their owner and
   emit (see [particles.md](particles.md));
6. the view scrolls towards the main object; the objects' sounds follow
   them;
7. the object under the mouse is found and becomes the **active
   object**, raising `MouseLeave` and `MouseEntry`;
8. the windows note the mouse: a button held under it is pressed, a
   press released on it fires, a dragged window moves, the focused
   window is checked; the cursor is chosen and animates;
9. the events raised during the frame reach their scripts (they run
   next frame).

The mouse reaches the runtime between frames (`mouse_move`,
`mouse_left_down`, `mouse_left_up`). A button needs one frame that sees
it held and a later one that sees it released, as in the original
engine, but a click can be shorter than a frame (a tap on a trackpad, a
slow frame). So a press that follows a move finds the object under the
mouse again before it is dispatched, and a release that arrives before
any frame saw the press waits for the end of the next frame: the
`LeftRelease` event is raised then, and the button fires on the frame
after. This is Luma's own; the original engine lost such clicks.

The frame is then drawn: the scene with the actors (each object's
particles right after it; a 3D actor takes its place in the order at
its projected position, where its shadow, its model and its attachments
are pushed as 3D items of the draw list, and the scene's shadow
receivers are written to the depth buffer before the main layer's
nodes, see [3d-actors.md](3d-actors.md) section 10), the windows marked
`IN_GAME`, the inventory box, the response box while a dialogue waits
for a choice, the other windows (so a menu opened during a dialogue
covers the responses, and takes the clicks over them; `CAdGame::
DisplayContent`), the sentences (not while the game is frozen by a
system-exclusive window such as a menu: `DisplaySentences`; they come
back when it closes), the cursor, the fader's
colour over everything, the loading icon (see
[saving.md](saving.md#loading-and-saving-screens-loading-icon)), and
last the messages of `Game.Msg`: short
texts in the system font from 20 pixels down the left edge, each for
three seconds of the game timer, five at most (the oldest goes), as the
original engine's `QuickMessage`; nothing shows without a system font.

While a window is **system exclusive** (see
[formats/window.md](formats/window.md)) the game is frozen: the timer
stops, so the scene, the actors and the sentences stand still, the
freezable scripts and the sounds pause, and only the window's own
scripts run.

While a video plays (see [video.md](video.md)), the frame is the
video's alone: the game is frozen, no script runs, and only the video
and its subtitles are drawn.

Without a window (`--headless`), the same loop runs with a fixed frame
duration, and the sounds run on the game clock.

While a script holds `Game.SuspendedRendering` true (the original
engine's way of hiding a heavy loading sequence), the loop runs as
above but the player presents no frame: the window keeps the last one
shown until the script sets it back to false. The original engine draws
no sprite and skips the flip; the headless player ignores the flag, so
its frames are still rendered.

Scripts read the loop's clocks and counters on the game object:
`CurrentTime` is the game timer (stopped while frozen), `WindowsTime`
the wall clock since the game started (the original engine gives the
system's `timeGetTime`; both only make sense as differences), `FPS` the
number of frames of the last whole second, counted as the original
engine does (`m_FramesRendered` moved to `m_Fps` when `m_FpsTime`
passes 1000 ms, so the value is `0` during the first second).
`WindowedMode` is whether a front-end opened a window (the player says
so; headless runs read `false`), `SoundAvailable` whether an audio
device was opened, `AcceleratedMode` (and `Accelerated`) always true
with wgpu. `TextEncoding` is `0` (ANSI) or `1` (UTF-8): set by every
string table loaded (a byte order mark means UTF-8, as in the original
engine) and by the script; the fonts, the `String` class, the editors
and the screen reader read the game's text in that encoding (see
[formats/font.md](formats/font.md#text-encoding)). `TextRTL` turns right-to-left text on, for
the TrueType fonts and the editors (set by a string table's
`@right-to-left` entry or by the script; see
[formats/font.md](formats/font.md#notes-for-implementers)). `Game.GetFileChecksum(file, hex)` is the
CRC-32 of a file of the game (IEEE polynomial, as the original engine's
`crc.cpp`), as an integer or a lowercase hex string, `null` for a
missing file. `Game.OpenDocument(target)` opens a file or an address
with the system (`open` on macOS, `xdg-open` elsewhere, `start` on
Windows); the original engine also minimizes a fullscreen window
first, which Luma does not do.

## The active object

While drawing, the engine notes where each object that reacts to the
mouse lands (`CBActiveRect`): an entity or actor whose `INTERACTIVE` is
on (and not ignoring the mouse while an inventory item is selected),
through its `REGION` when it has one, and through every sub-frame of the
sprite it shows that is not a `DECORATION`; then the windows, drawn
after the scene: a window over its rectangle (or its image's pixels)
unless `TRANSPARENT`, its buttons over their rectangles (or their
image's pixels when `PIXEL_PERFECT`), its editors, and under an
exclusive window or a menu a shield covering the whole screen. The
object under the mouse is the **last one drawn** whose area contains the
point: a region by its polygon (in scene coordinates), a sprite by its
rectangle on screen, and, when the sprite is `PRECISE` (the default),
only where the pixel's alpha is above 128, with the sprite's scale (per
axis, `ScaleX`/`ScaleY`) and mirroring applied. A rotated sprite
(`Rotatable`, see [scene.md](formats/scene.md#rotation-of-an-object)) is
hit where it is seen: the point is turned back around the hotspot before
the tests. The original engine registers the unrotated rectangle and
hits a turned sprite where it would be unrotated; Luma follows the
transform instead, deliberately.

While the game is not interactive there is no active object, so a
click skips the sentences and nothing else (per `TALK_SKIP_BUTTON`).
The exception is an object whose `NonIntMouseEvents` is on
(`CBObject::m_NonIntMouseEvents`): the windows, their controls and the
entities of entity containers start with it on, so that a menu works
while the actors talk; any object may have it set by a script.

## Mouse events

A button raises its event (`LeftClick`, `LeftRelease`, `RightClick`,
`RightRelease`, `LeftDoubleClick`, a double click being a second click
within 400 ms and 4 pixels) **on the game first**. When no game script
handles it, it goes to the active object, else to the scene when the
mouse is over it. A game script that handles `LeftClick` is therefore in
charge of forwarding it (The White Chamber's dispatches to the active
object or the scene itself, and its scenes walk the actor to
`Scene.MouseX`, `Scene.MouseY`).

`RightDoubleClick`, `MiddleClick` and `MiddleRelease` follow the same
path but stop at the active object: the engine's adventure layer only
forwards the left-button events and the right click to the scene
(`CAdGame` against `CBGame`). A right double click replaces the second
`RightClick`, as the system's double-click message did; a middle click
or a right double click does nothing while the game is not interactive
(a right click then skips the sentence, as before).

The wheel raises `MouseWheelUp` or `MouseWheelDown` on the **focused
window** when its scripts handle that event, else on the game
(`CBGame::HandleMouseWheel`); a notch of the wheel is one event, in
either direction. The headless player turns it with `--wheel up` or
`--wheel down`.

Closing the game's window raises `QuitGame` on the game when a game
script handles it, and the window stays open: the script confirms and
calls `Game.QuitGame()` itself (nothing is raised while the game is
frozen). Without a handler, the window closes and the game ends
(`CBGame::OnWindowClose`).

While the game is frozen by a system-exclusive window, the game and the
scene get no mouse event: only the active object does, a control of the
window or its shield.

`Game.MouseX` and `Game.MouseY` are the mouse in game pixels
(writable, as is `Game.SetMousePos`). `Game.LockMouseRect(left, top,
right, bottom)` keeps the position the game sees inside a rectangle
(the edges are put in order; a rectangle with a zero edge unlocks,
since the original engine applies the lock only while all four edges
are non-zero); the original engine also moves the system cursor back
into the rectangle, which Luma does not. `Scene.MouseX` and `Scene.MouseY`
add the scene's scroll offset and take off the scene viewport's origin
(`SCENE_VIEWPORT` in the game file, `Game.SetSceneViewport`, see
[formats/game.md](formats/game.md); a scene's own `VIEWPORT` or
`Scene.SetViewport` comes before it, see
[formats/scene.md](formats/scene.md)). The viewport also sizes the view
the scroll centres on the main object, and `Scene.IsPointInViewport`
answers for it. The scene is not clipped to it yet: a scene taller than
its viewport draws over whatever lies below, which the games seen so far
cover with a window. `Game.ActiveObject` is the active object,
`Game.SelectedItem` the inventory item in use. The inventory box, when
`Game.InventoryVisible`, is drawn between the `IN_GAME` windows and the
others, and its items are active objects through their sprites.

## Cursors

The cursor drawn (`CBGame::ShowCursor`), in order: nothing while
`Game.HideCursor`; the `NONINTERACTIVE_CURSOR` while the game is not
interactive; the active object's own `CURSOR`; the `ACTIVE_CURSOR` over
an active object that can be used (an entity, an actor or an item, the
engine's `usable` flag; not a window or a button); the `CURSOR`. A cursor that was not shown last frame
restarts its animation. `Game.SetActiveCursor`, `RemoveActiveCursor`,
`SetWaitCursor`, `RemoveWaitCursor`, `HasActiveCursor`,
`GetActiveCursor` and `GetWaitCursor` change and read them;
`GetActiveCursorObject` and `GetWaitCursorObject` return them as
sprite objects (see below), `null` without one. The system cursor is
hidden over the window.

An object's own cursor comes from its `CURSOR` key, or from `SetCursor`
on the object (entities, actors and controls; `false` when the sprite
does not load, and the old cursor stays). `RemoveCursor` takes it off,
`HasCursor` and `GetCursor` read it, and `GetCursorObject` returns it as
a sprite object (frames, `CurrentFrame`, `Looping`, `Reset`, `Pause`,
`Play`), sharing the animation the cursor shows. Scripts find the
entity under the mouse change cursor from the next frame on, as in the
original engine.

While an inventory item is selected and the game is interactive, the
item's `CURSOR` is drawn at the mouse, over the cursor above when the
item's `CURSOR_COMBINED` is on, instead of it otherwise; over an entity,
an actor or an item its `CURSOR_HOVER` is drawn instead (only over
objects that handle the item's event when `Game.SmartItemCursor` is
on). The item's `GetNormalCursorObject` and `GetHoverCursorObject`
return those two cursors as sprite objects, and `GetHoverSpriteObject`
its hover image (`SPRITE_HOVER`), the one the inventory box shows under
the mouse, with the animation it plays there.

## Keyboard

A key press reaches the game as **two messages**, as Windows delivers
it to the original engine (`CBPlatform::WindowProc`, `CBGame::
HandleKeypress`):

1. the **key** going down (`WM_KEYDOWN`), named by its Windows virtual
   key code: letters as upper-case ASCII (`A` is 65), digits as
   themselves, the other keys by their `VK_` value (`VK_ESCAPE` 27,
   `VK_RETURN` 13, `VK_SPACE` 32, arrows 37 to 40, `VK_DELETE` 46,
   `VK_F1` 112, the period key 190...), as the games' `keys.inc` define
   them. A key code says which key, never what it types: the period key
   is 190, not the 46 of its character, which is `VK_DELETE`;
2. the **character** the key types, if any (`WM_CHAR`): `a` or `A`,
   `.`, `é`. Backspace, Tab, Enter and Escape send the control
   characters 8, 9, 13 and 27, Ctrl with a letter the control character
   of that letter (Ctrl+C is 3, Ctrl+V 22), Ctrl+Enter 10, Ctrl+Backspace
   127; Ctrl+Tab sends none.

Each message, in this order (`Runtime::key_down` sends both,
`Runtime::key_message` and `Runtime::key_char` one):

- while a video plays, Escape stops it and nothing else happens;
- the [accessibility](#accessibility) shortcuts take it when their
  switches are on (Ctrl+Tab on the key, Ctrl+Space on the character);
- it becomes the state of `Game.Keyboard` (`CBKeyboardState::ReadKey`);
- the **focused window** gets it when there is one: the `Tab` key moves
  its focus between its focusable controls, an editor with the focus
  edits its text (see below);
- a **key** the window did not use raises `Keypress` on the window when
  its scripts handle that event, else on the game; without a focused
  window, on the game. A **character** never raises `Keypress`.

A held key repeats: the front-end sends the key and its character again
for each repeat, so `Keypress` is raised again and an editor keeps
deleting or typing, as on Windows.

| Property | Value |
|----------|-------|
| `KeyCode` | The code of the last message: a key's virtual key code, or a character's code (its Windows-1252 byte: `a` is 97, `é` 233; the Unicode code point for a character the code page lacks). |
| `Key` | The character of the last message when it was a character, else empty. |
| `Printable` | Whether the last message was a character. |
| `IsShift`, `IsAlt`, `IsControl` | The modifiers held at the last message. |

A `Keypress` handler runs with the next frame's scripts, after both
messages, so it sees the **character** of a key that types one and the
**key code** of a key that does not: `KeyCode` is 112 for F1 and 27 for
Escape (whose character is 27 too), but 97 for the A key, with `Key`
`"a"` and `Printable` true. This is the original's behaviour, read from
its sources (the handler is a script thread started at the next frame,
the character message is handled before that frame): a script that
compares `KeyCode` with a letter's virtual key code never matches, and
a character whose code is a key's (`p` is 112, `VK_F1`; `.` is 46,
`VK_DELETE`; `$` is 36, `VK_HOME`) is taken for that key by a script
that does not test `Printable`.

`Keyboard.IsKeyDown(code)` says whether a key is held; a string gives
its first character, in either case. It also answers for the mouse
buttons (1 left, 2 right, 4 middle: `VK_LBUTTON`, `VK_RBUTTON`,
`VK_MBUTTON`) and for the left and right Shift, Control and Alt (160 to
165), as the system call the original asks does. Luma answers from the
keys and buttons its front-end reported. `ToString(Game.Keyboard)` is
`[keyboard state]`.

### The editor's keys

An editor with the focus (`CUIEdit::HandleKeypress`) takes:

- the **keys** Left and Up, Right and Down (the caret moves a
  character), Home and End, each extending the selection while Shift is
  held, and Delete; any other key is left to `Keypress`;
- the **characters**: Backspace (8) deletes, a printable character is
  typed over the selection, and with Ctrl held and Alt up, the control
  characters of A (select all), C (copy the selection), X (cut it) and
  V (paste over the selection, up to `MAX_LENGTH`). Escape, Tab and
  Enter are never taken, as key or as character.

So Backspace both raises `Keypress` (its key) and deletes (its
character), while Delete and the arrows are eaten by the editor.

Every deletion and every insertion raises the editor's event on its
parent when `PARENT_NOTIFY` is on: typing over a selection raises it
twice (`DeleteChars`, then `InsertChars`), and Backspace at the start of
the text still raises it once.

Copy and paste go through a clipboard the front-end provides
(`World::set_clipboard`; `luma-adventure` has no platform code).
`luma-player` gives the system's in a window (the `arboard` crate);
without one (the headless player, tests) the game keeps a clipboard of
its own, so that what one editor copies another pastes.

Luma differs from the original on purpose in three places, all defects
of the original that no game could rely on:

- typing or pasting over a selection made from left to right inserts
  where the selection was; the original inserts further right, after as
  many characters as the selection had;
- a control character that is no command (Ctrl+B, Ctrl+Backspace) or
  that comes with a pasted text (a line break) is dropped; the original
  inserts it in the text;
- Ctrl+C when the clipboard cannot be written types nothing; the
  original inserts the control character.

Mirrored keys for right-to-left text are described in
[formats/window.md](formats/window.md#edit).

### In `luma-player`

The window's key events are translated in `crates/luma-player/src/
keys.rs`: letters by the character they type (so a French keyboard's A
is `A`), the other keys by their place on a US keyboard (the key right
of M is 188 whatever it types), the numeric keypad by its own codes
with Num Lock on (`VK_NUMPAD0` 96 to `VK_DIVIDE` 111) and as the arrows
it stands for with Num Lock off; F13 to F24, Print Screen, Num Lock,
Scroll Lock and the menu key have their codes. A character of any
alphabet is delivered, with or without a key code. This table is
inferred from the public list of virtual key codes: the original gets
them from Windows.

With Alt held and Control up, only F10 reaches the game, as on Windows,
where the other keys are system keys the engine leaves to the system
(AltGr, which Windows reports as Control and Alt, types). On macOS the
rule is lifted, since Option is the key that types the third character
of a key (`@`, `#`, `{` on many layouts), and Command stands for
Control, so that Cmd+C, Cmd+X, Cmd+V and Cmd+A edit.

The headless player presses keys with `--key [ctrl+][shift+][alt+]
<code>` (the key, then the character it types on a US keyboard),
`--repeat-key <code>:<n>` (a held key repeating n times), `--hold-key`
(held without repeating, for `IsKeyDown`) and `--type <text>` (each
character's key, then the character; a character no US key types comes
alone).

## Fades

`Game.FadeOut(duration, r, g, b, alpha)` fades the screen to a colour
over `duration` milliseconds (500, black, opaque by default) and makes
the script wait; `FadeIn` fades the colour away; the `Async` forms do
not wait and the `System` forms run on the wall clock, so that a frozen
game still fades. `GetFadeColor` reads the colour laid over the frame.
A scene change fades in from black over 200 ms unless `ChangeScene` is
told not to.

The scene has a fader of its own (`Scene.FadeOut`, `FadeIn`, their
`Async` forms and `Scene.GetFadeColor`, same arguments): its colour
covers the scene viewport only, drawn after the scene's objects and
under the windows, the inventory, the sentences and the cursor
(`CAdScene::TraverseNodes` displays it last). It runs on the game timer
(there is no `System` form) and every scene gets a fresh one, as the
engine creates a `CBFader` per `CAdScene`.

## Scene state between visits

A scene marked `PERSISTENT_STATE` (see
[formats/scene.md](formats/scene.md)) is remembered when the player
leaves it (`CAdScene::SaveState`, right after `SceneShutdown`) and put
back when it loads again (`CAdScene::LoadState`, before its scripts
run). The states are kept by scene file name, without case; a scene the
player never left has none. Per named node marked `SAVE_STATE` (the
default) the engine keeps:

- for an entity: `Active`, the caption, `AlphaColor`, the cursor, and
  the sprite it was switched to, unless the entity is a hotspot (a
  `REGION` without sprite) or the scene says
  `PERSISTENT_STATE_SPRITES=FALSE`;
- for a region: `Active`.

Nothing else comes back: positions, script variables and the scripts
themselves start over from the file, and a scene's init script runs on
every visit, so it may override what was restored. `Game.Reset`
(`CAdGame::ResetContent`) forgets every remembered scene, along with the
inventory, the dialogue branches and the "once" responses, loads the
items file again, and sets the non-native script globals back to
`null`; the scene the player is in stays, and is not remembered when
left. The flag that skips that one save travels with a saved game, as
in the original engine. A remembered sprite
that cannot be loaded is logged and the entity keeps the file's. The
states are part of a saved game (see [saving.md](saving.md)). Whether
a waypoint group is active (`WaypointGroup.Active`) is not remembered
between visits, in either engine.

## Dialogue responses

A conversation's choices go through the **response box**
(`RESPONSE_BOX` in the game file, a `.def` file with `FONT`,
`FONT_HOVER`, `AREA`, `HORIZONTAL`, `SPACING`, `TEXT_ALIGN`,
`VERTICAL_ALIGN`, `CURSOR` and a `WINDOW` with `prev` and `next`
buttons), as the original engine's `CAdResponseBox` does. The script
adds its choices with `Game.AddResponse(id, text)`,
`AddResponseOnce` (not offered again in the current dialogue branch) or
`AddResponseOnceGame` (never again), then `Game.GetResponse()` shows
them and waits: one text button per choice in the box's fonts (or its
icons), laid out in the area from the scroll offset, `prev` and `next`
shown when they do not fit, the window exclusive meanwhile. The pick
returns the choice's id to the script, sets `Game.LastResponse` and
`LastResponseOrig`, and closes the box; without any choice left
`GetResponse` returns `null` at once, and with a single one it returns
it at once when asked to (`GetResponse(true)`). `StartDlgBranch` and
`EndDlgBranch` name the branch the once-only choices are remembered in
(qualified by the script file and event, as the original does; the
innermost without a name); when the last branch ends they are
forgotten. `ResetResponse(id)` forgets one, `ClearResponses` drops the
pending ones, `GetNumResponses` counts them after weeding,
`Game.ResponsesVisible` says whether the box waits, and
`GetResponsesWindow` and `LoadResponseBox` serve the box itself.

While the box waits (the original's semi-frozen state), and while an
exclusive window is up, the windows and their controls take the mouse
even when `Game.Interactive` is off, so a choice can be picked during
the actors' lines; the scene's objects do not.

## Accessibility

The original engine's settings dialog has an accessibility tab with
seven switches, kept in the `Accessibility` section of its registry
(`CBAccessMgr`). Luma reads the same keys from the `[Accessibility]`
section of `settings.ini` in the save directory (see
[saving.md](saving.md#accessibility-settings)) when the game starts,
and a front-end can set them through `World::accessibility_mut`.
Scripts read them as `Game.AccTTSEnabled`, `AccTTSTalk`,
`AccTTSCaptions`, `AccTTSKeypress`, `AccKeyboardEnabled`,
`AccKeyboardCursorSkip` and `AccKeyboardPause`, read only, as in the
original. A game whose `default.game` says `ALLOW_ACCESSIBILITY_TAB =
FALSE` keeps speech, keyboard play and the pause off whatever the file
says, as the original forces them off.

**Text-to-speech** (`TTSEnabled`, off by default) reads out, through a
speech backend:

- the caption of the object the mouse comes over (`TTSCaptions`): its
  `AccCaption` when a script set one, else a control's text, else its
  caption, expanded through the string table;
- a sentence said without a speech file (`TTSTalk`): the text of
  `Talk` when no sound was found for it;
- each key typed in an editor (`TTSKeypress`);
- whatever a script gives to `Game.AccOutputText(text, type)`, `type`
  being 0 for a caption, 1 for speech, 2 for a key.

A text interrupts what is being said, unless the last text spoken was
a sentence: nothing cuts a sentence short, and what follows it waits
for its end, as the original purges the voice queue before speaking
except after speech. Skipping the sentences (a click while the
game is not interactive) stops the speech. The backend is the `Speech`
trait of `luma_adventure::accessibility`: on macOS the system's `say`
command, spawned without blocking the game loop and killed when a text
must interrupt it; elsewhere there is none and the text-to-speech is
unavailable, as the original's SAPI voice is without a voice installed.
Tests use a recording backend.

**Keyboard play** (`KeyboardEnabled`, off by default): Ctrl+Tab picks
the next clickable object and Ctrl+Shift+Tab the previous one, cycling
through the buttons and editors of the focused window (into its child
windows, disabled ones skipped), else the response buttons while the
dialogue waits for a pick, else the scene's active, interactive
entities in layer order (a close-up layer hides what is below it) and
its active, interactive actors. While Ctrl is held, the object picked
is framed in red, four pixels wide, over the windows; with
`KeyboardCursorSkip` (on by default) the mouse cursor jumps to the
centre of its area the frame it is picked, so that the next click goes
to it and `MouseEntry` and the caption follow. A window going exclusive
forgets the pick. Two differences from the original: the cursor jumps
whether or not Ctrl is still held (the original repositions only while
it is), and objects standing in a region node are not listed through
the region.

**Pause** (`KeyboardPause`, off by default): Ctrl+Space pauses the game
under a shield window covering the screen, system-exclusive so that
every script but the window's stops, with "Game paused. Press
Ctrl+Space to resume." (`/SYSENG0040/`) in the system font at the
bottom centre; Ctrl+Space again closes it.

## Not yet

- Transitions other than the fade.
