# Windows (`.window`) and the inventory (`.def`, `.items`)

A *window* is a rectangle drawn over the scene: an image or a tiled
background, a title, and *controls* laid inside it: buttons, static
texts, text editors and nested windows. Games build their menus,
captions, dialogs and the inventory box with them. A script loads one
with `Game.LoadWindow` (or creates an empty one with
`Game.CreateWindow`) and drives it through its properties and methods;
the window and each of its controls can carry scripts of their own.

Sources: the MIT-licensed [WME Lite](https://github.com/MnemonicWME/wmelite)
sources (`UIWindow.cpp`, `UIObject.cpp`, `UIButton.cpp`, `UIText.cpp`,
`UIEdit.cpp`, `UITiledImage.cpp`, `AdInventoryBox.cpp`, `AdItem.cpp`,
`AdInventory.cpp`, `BGame.cpp`) and analysis of game files.

Implemented by `luma_formats::window` (the files) and
`luma_adventure::ui` (display, focus, input, the inventory).

## Example

```
WINDOW
{
  NAME = "MainMenu"
  X = 200  Y = 200  WIDTH = 520  HEIGHT = 450
  TITLE = "/INTERF0004/Main Menu"
  TITLE_RECT { 60, 35, 260, 70 }
  TITLE_ALIGN = left
  DRAG_RECT { 58, 30, 420, 70 }
  IMAGE = "interface\system\artwork\guibackground.png"
  FONT = "primaryttf.font"
  SCRIPT = "interface\system\mainmenu.script"

  BUTTON
  {
    NAME = "close"
    TEXT = "/INTERF0005/Resume Game"
    TEXT_ALIGN = "center"
    FONT = "secondaryttf.font"
    FONT_HOVER = "secondaryinvertttf.font"
    X = 115  Y = 130  WIDTH = 300
    PARENT_NOTIFY = TRUE
  }

  STATIC
  {
    NAME = "scanlines"
    X = 0  Y = 0
    IMAGE = "interface\system\artwork\guiscanlines.png"
  }
}
```

Every control shares the keys of the generic control (`UIObject`), then
adds its own. Texts (`TEXT`, `TITLE`) go through the string table
(`/ID/fallback`). A `|` in a text is a line break.

## Keys common to every control

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `TEMPLATE` | string | | Another file of the same kind loaded first; the keys that follow override it. |
| `NAME` | string | `""` | Name used by scripts (`GetWidget`, `PARENT_NOTIFY`). |
| `CAPTION` | string | `""` | Free caption. |
| `X`, `Y` | int | `0` | Position, relative to the parent window. |
| `WIDTH`, `HEIGHT` | int | `0` | Size; `0` is corrected after loading (see each control). |
| `BACK` | string | | A `.tiled` file (`TILED_IMAGE`) drawn stretched to the control's size. |
| `IMAGE` | string | | A sprite or image drawn at the control's position. |
| `FONT` | string | | The font of the control's text; the game's `SYSTEM_FONT` without one. |
| `CURSOR` | string | | The cursor shown over the control. |
| `VISIBLE` | bool | `TRUE` | Drawn and clickable. |
| `DISABLED` | bool | `FALSE` | Drawn in its disabled state, ignored by the mouse and the focus. |
| `PARENT_NOTIFY` | bool | `FALSE` | A press (of a button) or an edit of the text (of an editor) raises an event named after the control on its parent. |
| `SCRIPT` | string | | A script attached to the control; repeatable. |
| `EDITOR_PROPERTY` | block | | Editor data, ignored. |

The size correction (`CorrectSize`) runs after a control is read: a
width of `0` becomes the width of the control's image (its bounding
rectangle at origin), else `100`; a height of `0` becomes the image's
height. A `BACK` tiled image then rounds the size down to whole tiles.

## `WINDOW`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `BACK_INACTIVE`, `IMAGE_INACTIVE`, `FONT_INACTIVE` | string | | Used instead of `BACK`, `IMAGE`, `FONT` while the window is not the focused one. |
| `TITLE` | string | `""` | Text drawn in `TITLE_RECT` with the window's font; also the window's `Text` property. |
| `TITLE_ALIGN` | `left`, `right`, `center` | `left` | Alignment of the title. |
| `TITLE_RECT` | `{ left, top, right, bottom }` | empty | Where the title is drawn, relative to the window; no title without it. |
| `DRAG_RECT` | `{ left, top, right, bottom }` | empty | The area by which the mouse drags the window. |
| `TRANSPARENT` | bool | `FALSE` | The window itself is not an active object: only its controls catch the mouse. |
| `MENU` | bool | `FALSE` | A menu: a click anywhere outside the window closes it. |
| `IN_GAME` | bool | `FALSE` | Drawn with the scene, under the sentences and the other windows. |
| `CLIP_CONTENTS` | bool | `FALSE` | The controls are clipped to the window. |
| `PAUSE_MUSIC` | bool | `TRUE` | `GoSystemExclusive` pauses the music too. |
| `FADE_COLOR` | `r, g, b` | | The screen is tinted with this colour under the window. |
| `FADE_ALPHA` | int | `255` | The tint's opacity. |
| `ALPHA_COLOR` | `r, g, b` | | A colour multiplied with the window and its controls. |
| `ALPHA` | int | `255` | Its opacity. |
| `BUTTON`, `STATIC`, `EDIT`, `WINDOW` | block | | A control, in drawing order. |
| `ENTITY_CONTAINER` | block | | A control showing an entity: `NAME`, `X`, `Y`, `DISABLED`, `VISIBLE`, `SCRIPT`, and `ENTITY`, an `.entity` file (an `ENTITY` block as in a scene). The entity is drawn at the container's position with its own sprite and animation, takes the mouse through its sprite when it is active and registrable and the container is enabled, and its own scripts get the mouse events; `GetEntity` hands it to scripts, `SetEntity` loads another file. OpenQuest's verb bar is made of them. |

A window with a `TITLE` but no `TITLE_RECT` shows no title. A window
whose size is `0` after the correction has the width of its image or
`100` and the height of its image, as any control.

## `BUTTON`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `BACK_HOVER`, `BACK_PRESS`, `BACK_DISABLE`, `BACK_FOCUS` | string | | Tiled backgrounds of the states. |
| `IMAGE_HOVER`, `IMAGE_PRESS`, `IMAGE_DISABLE`, `IMAGE_FOCUS` | string | | Images of the states. |
| `FONT_HOVER`, `FONT_PRESS`, `FONT_DISABLE`, `FONT_FOCUS` | string | | Fonts of the states. |
| `TEXT` | string | | The label, drawn centred vertically. |
| `TEXT_ALIGN` | `left`, `right`, `center` | `center` | Alignment of the label. |
| `CENTER_IMAGE` | bool | `FALSE` | The image is centred in the button instead of drawn at its corner. |
| `PRESSED` | bool | `FALSE` | Stays drawn pressed. |
| `FOCUSABLE` | bool | `FALSE` | Can take the keyboard focus. |
| `PIXEL_PERFECT` | bool | `FALSE` | Only the opaque pixels of the image catch the mouse; else the whole rectangle does. |

The state shown, in order: disabled; pressed (the mouse is held on it,
or `PRESSED`); hovered (it is the active object, while the game is
interactive); focused; normal. A state without its own image, background
or font uses the normal one. Pressed, the image and the text shift by
one pixel. The height of a button with a text is at least the text's
height; a button without image nor text is 100 by 100.

## `STATIC`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `TEXT` | string | | The text, wrapped to the width. |
| `TEXT_ALIGN` | `left`, `right`, `center` | `left` | Horizontal alignment. |
| `VERTICAL_ALIGN` | `top`, `center`, `bottom` | `center` | Vertical placement of the text in the height. |

A static never catches the mouse. `SizeToFit` sets its size to its
text; `HeightToFit` only its height.

## `EDIT`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `TEXT` | string | | The text being edited. |
| `FONT_SELECTED` | string | | The font of the selected part of the text. |
| `MAX_LENGTH` | int | `-1` | Longest text accepted; `-1` for no limit. |
| `FRAME_WIDTH` | int | `0` | Margin between the frame and the text. |
| `CURSOR_BLINK_RATE` | int | `600` | Milliseconds between blinks of the caret. |

An editor can take the focus (`Tab` moves it between the focusable
controls of a window) and reads the keys: printable characters are
inserted at the caret, `Backspace`, `Delete`, the arrows, `Home` and
`End` edit and move. `Escape`, `Tab` and `Return` are left to the
window.

## `TILED_IMAGE` (`.tiled`)

An image cut into nine tiles: the corners are drawn once, the edges
repeated along their side, the middle repeated in both directions, so
that the image fills any size.

| Key | Type | Meaning |
|-----|------|---------|
| `TEMPLATE` | string | Another `.tiled` file loaded first. |
| `IMAGE` | string | The image. |
| `UP_LEFT`, `UP_MIDDLE`, `UP_RIGHT`, `MIDDLE_LEFT`, `MIDDLE_MIDDLE`, `MIDDLE_RIGHT`, `DOWN_LEFT`, `DOWN_MIDDLE`, `DOWN_RIGHT` | `left, top, right, bottom` | The tiles in the image. |
| `HORIZONTAL_TILES` | `w1, w2, w3` | With `VERTICAL_TILES`, the widths of the three columns, from which the nine tiles follow. |
| `VERTICAL_TILES` | `h1, h2, h3` | The heights of the three rows. |

A tile left empty is a third of the image. A tiled image of width `W`
and height `H` draws `(W - left - right) / middle` columns and
`(H - top - bottom) / middle` rows of the middle tile, rounding down
(the engine advances the rows by the tile's width, which only matters
for non-square middle tiles). `CorrectSize` reduces a control to the size
those tiles cover.

## Display and input

Windows are kept in a list in the order they were loaded; the **focused
window** is the one a script focused last (`GoExclusive`,
`GoSystemExclusive`, `Focus`), moved to the end of the list, or the
topmost visible and enabled one when that one vanished. The windows
with `IN_GAME` are drawn right after the scene; the others after the
inventory, before the sentences and the cursor. A window shows its
`_INACTIVE` background, image and font while it is not the focused one.

A visible window that is not `TRANSPARENT` is an active object over its
whole rectangle (through its image's pixels when it has one); its
controls are drawn after it, offset by its position, and a control
drawn later is found first under the mouse. A button is an active
object over its rectangle unless `PIXEL_PERFECT`. A click on a button
holds it pressed while the mouse stays on it; releasing it there raises
`Press` on the button, then the event named after the button on its
parent when `PARENT_NOTIFY` is on; an editor with `PARENT_NOTIFY` raises
the event named after it on its parent at each key that edits its text
(a character, Backspace, Delete), as `CUIEdit::InsertChars` and
`DeleteChars` do, which is how a save window enables its OK button once
a name is typed. A `MENU` window registers a button
named `close` under the whole screen (and under the window) whose press
closes the window.

Modes (`Window.Exclusive`, `Window.SystemExclusive`):

- **normal**: the game goes on around the window;
- **exclusive** (`GoExclusive`): the window becomes visible and focused;
  a shield window under it takes the mouse so that the scene gets no
  click; the game runs;
- **system exclusive** (`GoSystemExclusive`): as exclusive, and the game
  is **frozen** (`Game.Freeze`): the scripts are paused but the window's
  own and those of its controls, the sounds are paused (the music too
  unless `PAUSE_MUSIC = FALSE`), the game timer stops, the game is
  interactive whatever it was. `Close` resumes everything. Freezing
  nests: only the last `Unfreeze` resumes.

The script calling `GoExclusive` or `GoSystemExclusive` waits until the
window closes (`Close`, or the `close` button of a menu).

Keys go to the focused window first: `Tab` moves the focus among its
focusable controls, else the focused control takes the key (an editor),
else the window's `Keypress` handler, else the game's. The mouse wheel
goes to the focused window too.

A window can be dragged by its `DRAG_RECT` while the left button is held
there.

## The inventory

The game names its inventory box (`INVENTORY_BOX` in `default.game`, a
`.def` file) and its items (`ITEMS`, a `.items` file).

```
INVENTORY_BOX
{
  ITEM_WIDTH = 100  ITEM_HEIGHT = 100  SPACING = 0  SCROLL_BY = 1
  HIDE_SELECTED = TRUE
  AREA { 12, 0, 1012, 100 }
  EXCLUSIVE = FALSE

  WINDOW
  {
    X = 0  Y = 0  WIDTH = 1024  HEIGHT = 100
    IMAGE = "interface\Inventory.png"
    BUTTON { NAME = "prev" IMAGE = "interface\left.png" X = 0 Y = 0 WIDTH = 12 HEIGHT = 100 PARENT_NOTIFY = TRUE }
    BUTTON { NAME = "next" IMAGE = "interface\right.png" X = 1012 Y = 0 WIDTH = 12 HEIGHT = 100 PARENT_NOTIFY = TRUE }
  }
}
```

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `TEMPLATE` | string | | Another `.def` file loaded first. |
| `WINDOW` | block | | The box's window, drawn under the items; its `prev` and `next` buttons scroll, its `close` button hides the box. |
| `AREA` | `left, top, right, bottom` | empty | Where the items are laid out, relative to the window. |
| `ITEM_WIDTH`, `ITEM_HEIGHT` | int | `50` | The cell of one item. |
| `SPACING` | int | `0` | Pixels between cells. |
| `SCROLL_BY` | int | `1` | Items scrolled by `prev` and `next`. |
| `HIDE_SELECTED` | bool | `FALSE` | The selected item is not drawn in the box. |
| `EXCLUSIVE` | bool | `FALSE` | A button under the whole screen closes the box when clicked outside. |
| `ALWAYS_VISIBLE` | bool | `FALSE` | The box starts visible. |

The box shows `floor((width + spacing) / (item width + spacing))`
columns by the same count of rows of the area, from the scroll offset;
`prev` is enabled while the offset is positive, `next` while items
remain after the page. Each item is drawn at its cell's top-left corner
(its sprite's hotspot there) and is an active object through its sprite.

The items file is a list of `ITEM` blocks:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `NAME` | string | | Name used by `TakeItem`, `DropItem`, `HasItem` and as the event an item raises when used on an object. |
| `CAPTION` | string | | Caption shown by the game's scripts. |
| `IMAGE`, `SPRITE` | string | | The item's sprite in the box. |
| `IMAGE_HOVER`, `SPRITE_HOVER` | string | | Shown instead while the mouse is over the item. |
| `CURSOR` | string | | The cursor while the item is selected. |
| `CURSOR_HOVER` | string | | The cursor while the selected item is over an object that can use it. |
| `CURSOR_COMBINED` | bool | `TRUE` | The item's cursor is drawn over the normal cursor instead of replacing it. |
| `FONT` | string | | Font of the item's speech and amount. |
| `AMOUNT`, `DISPLAY_AMOUNT`, `AMOUNT_OFFSET_X`, `AMOUNT_OFFSET_Y`, `AMOUNT_ALIGN`, `AMOUNT_STRING` | | | A count drawn on the item. |
| `TALK`, `TALK_SPECIAL` | string | | Talk sprites, as for entities. |
| `SCRIPT` | string | | A script attached to the item; repeatable. |
| `PROPERTY` | block | | A script property. |
| `ALPHA_COLOR`, `ALPHA` | | | Tint and opacity. |

`Game.TakeItem(name, after)` puts the item in the inventory (again, if
it was there, after the named item or at the end); `DropItem` takes it
out and deselects it; `HasItem` says whether it is there; `GetItem`
finds an item by name or by index in the inventory; `Game.SelectedItem`
is the item in use, which the cursor shows. `Game.InventoryVisible`
shows and hides the box.
