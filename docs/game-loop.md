# The game loop, the mouse and the keyboard

How Luma Engine runs a frame of a game, and how the player's mouse and
keyboard reach the scripts. The rules follow the original engine (WME
Lite's `BGame`, `AdGame`, `BRenderer`, `BKeyboardState`, `BFader`,
`BTransitionMgr`, MIT) so that games react as their authors expect.

## A frame

`luma-player <game>` opens a window at the game's resolution (letterboxed
when the window has another shape), loads `default.game`, attaches the
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
5. the scene's entities animate and talk; the actors walk, turn, talk;
6. the view scrolls towards the main object; the objects' sounds follow
   them;
7. the object under the mouse is found and becomes the **active
   object**, raising `MouseLeave` and `MouseEntry`;
8. the windows note the mouse: a button held under it is pressed, a
   press released on it fires, a dragged window moves, the focused
   window is checked; the cursor is chosen and animates;
9. the events raised during the frame reach their scripts (they run
   next frame).

The frame is then drawn: the scene with the actors, the windows marked
`IN_GAME`, the other windows, the sentences, the cursor, and the fader's
colour over everything.

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
only where the pixel's alpha is above 128, with the sprite's scale and
mirroring applied.

While the game is not interactive there is no active object, so a
click skips the sentences and nothing else (per `TALK_SKIP_BUTTON`).

## Mouse events

A button raises its event (`LeftClick`, `LeftRelease`, `RightClick`,
`RightRelease`, `LeftDoubleClick`, a double click being a second click
within 400 ms and 4 pixels) **on the game first**. When no game script
handles it, it goes to the active object, else to the scene when the
mouse is over it. A game script that handles `LeftClick` is therefore in
charge of forwarding it (The White Chamber's dispatches to the active
object or the scene itself, and its scenes walk the actor to
`Scene.MouseX`, `Scene.MouseY`).

While the game is frozen by a system-exclusive window, the game and the
scene get no mouse event: only the active object does, a control of the
window or its shield.

`Game.MouseX` and `Game.MouseY` are the mouse in game pixels
(writable, as is `Game.SetMousePos`); `Scene.MouseX` and `Scene.MouseY`
add the scene's scroll offset. `Game.ActiveObject` is the active object,
`Game.SelectedItem` the inventory item in use. The inventory box, when
`Game.InventoryVisible`, is drawn between the `IN_GAME` windows and the
others, and its items are active objects through their sprites.

## Cursors

The cursor drawn (`CBGame::ShowCursor`), in order: nothing while
`Game.HideCursor`; the `NONINTERACTIVE_CURSOR` while the game is not
interactive; the active object's own `CURSOR`; the `ACTIVE_CURSOR` over
any active object; the `CURSOR`. A cursor that was not shown last frame
restarts its animation. `Game.SetActiveCursor`, `RemoveActiveCursor`,
`SetWaitCursor`, `RemoveWaitCursor`, `HasActiveCursor`,
`GetActiveCursor` and `GetWaitCursor` change and read them. The system
cursor is hidden over the window.

While an inventory item is selected and the game is interactive, the
item's `CURSOR` is drawn at the mouse, over the cursor above when the
item's `CURSOR_COMBINED` is on, instead of it otherwise; over an entity,
an actor or an item its `CURSOR_HOVER` is drawn instead (only over
objects that handle the item's event when `Game.SmartItemCursor` is
on).

## Keyboard

A key going down updates `Game.Keyboard`, then goes to the **focused
window** when there is one: `Tab` moves its focus between its focusable
controls, an editor with the focus edits its text; a key the window
did not use raises `Keypress` on the window when its scripts handle
that event, else on the game. Without a focused window, `Keypress`
goes to the game.

| Property | Value |
|----------|-------|
| `KeyCode` | The key's code: letters as upper-case ASCII, digits and punctuation as themselves, named keys by their Windows virtual key code (`VK_ESCAPE` 27, `VK_RETURN` 13, `VK_SPACE` 32, arrows 37 to 40, `VK_F1` 112...), as the games' `keys.inc` define them. |
| `Key` | The character typed when the key is printable, else empty. |
| `Printable` | Whether the key produced a character. |
| `IsShift`, `IsAlt`, `IsControl` | The modifiers held. |

`Keyboard.IsKeyDown(code)` says whether a key is held; a string gives
its first character, in either case.

## Fades

`Game.FadeOut(duration, r, g, b, alpha)` fades the screen to a colour
over `duration` milliseconds (500, black, opaque by default) and makes
the script wait; `FadeIn` fades the colour away; the `Async` forms do
not wait and the `System` forms run on the wall clock, so that a frozen
game still fades. `GetFadeColor` reads the colour laid over the frame.
A scene change fades in from black over 200 ms unless `ChangeScene` is
told not to.

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
every visit, so it may override what was restored. A remembered sprite
that cannot be loaded is logged and the entity keeps the file's. The
states are part of a saved game (see [saving.md](saving.md)). The
original engine also remembers whether each waypoint group is active;
Luma uses every group, so this is not kept yet.

## Not yet

- Dialogue responses (`AddResponse`, `GetResponse`).
- Each object's own inventory (`Game.InventoryObject`): one inventory
  serves every object.
- The mouse wheel, which the engine gives to the focused window.
- Transitions other than the fade.
