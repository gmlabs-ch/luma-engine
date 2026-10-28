# Game definition and string tables

`startup.settings` names the game definition file (`GAME="default.game"`)
and the resolution; `default.game` describes the game object: its name,
the scripts attached to it, the system font, the cursors, and the
interface files of the adventure layer. Both are
[definition files](definition-files.md).

Sources: the MIT-licensed [WME Lite](https://github.com/MnemonicWME/wmelite)
sources (`CBGame::LoadBuffer`, `CAdGame::LoadBuffer`, `BStringTable.cpp`),
and analysis of game files.

Implemented by `luma_formats::GameFile` and `luma_formats::StringTable`;
the game object itself lives in `luma-adventure`.

## `default.game`

```
GAME {
  NAME="WhiteChamber"
  CAPTION="WhiteChamber"
  SCRIPT="scripts\game.script"
  SYSTEM_FONT="fonts\outline_green.font"
  CURSOR="sprites\system\cur_arrow.sprite"
  ACTIVE_CURSOR="sprites\system\cur_arrow_h.sprite"
  NONINTERACTIVE_CURSOR="sprites\system\cur_wait.sprite"
  SUBTITLES=TRUE
  VIDEO_SUBTITLES=TRUE
  SUBTITLES_SPEED=100
}
AD_GAME {
  RESPONSE_BOX="interface\responses.def"
  INVENTORY_BOX="interface\inventory.def"
  ITEMS="items\items.items"
  TALK_SKIP_BUTTON="left"
}
```

| Block | Key | Meaning |
|-------|-----|---------|
| `GAME` | `NAME`, `CAPTION` | The game's name (`Game.Name`) and window title. |
| | `SCRIPT` | A script attached to the game object when it starts; repeatable. The game's logic starts there: the first script picks the first scene with `Game.ChangeScene`. |
| | `STRING_TABLE` | A string table loaded at startup (see below). |
| | `SYSTEM_FONT` | The font of engine messages. |
| | `CURSOR`, `ACTIVE_CURSOR`, `NONINTERACTIVE_CURSOR` | The mouse cursor sprites: normal, over an interactive object, while the game is not interactive. |
| | `SUBTITLES`, `SUBTITLES_SPEED`, `VIDEO_SUBTITLES` | Whether speech and videos are subtitled, and how long a subtitle stays per character (default 70 ms). |
| | `PERSONAL_SAVEGAMES`, `LOCAL_SAVE_DIR` | Where saved games go. |
| | `PROPERTY { NAME VALUE }` | Script properties of the game object. |
| `AD_GAME` | `RESPONSE_BOX`, `INVENTORY_BOX`, `ITEMS` | The dialogue, inventory and item definition files. |
| | `TALK_SKIP_BUTTON` | Which mouse button skips speech: `left`, `right`, `both`, `none`. |
| | `STARTUP_SCENE` | The first scene, when no script picks one. |

Other keys (`INDICATOR_*`, `GUID`, `EDITOR_*`, `COMPAT_*`) describe the
loading indicator, the editors' state and compatibility switches; they
are read and ignored.

## String tables

A string table (`string.tab`, or any file named by `STRING_TABLE` or
loaded with `Game.LoadStringTable`) holds the game's text by identifier,
one entry per line:

```
; comments start with a semicolon
CONFIG0001	true
SYSENG0001	Game settings
INTERF0015	< empty >
STORER0041	I'm back!|How was that possible?
```

- The identifier and the text are separated by a tab, or by the first
  space when there is no tab. Identifiers are case-insensitive.
- A `|` in the text is a line break.
- A file starting with the UTF-8 byte order mark switches the game's
  text encoding to UTF-8; otherwise text is 8-bit in the game's code page.
- The special entry `@right-to-left` enables right-to-left text.

Scripts and definition files refer to text as `/ID/fallback`: the
engine replaces it with the table's text for `ID`, or with the fallback
when the table has none, so that a game runs with or without a
translation. `Game.ExpandString` does this on demand; strings shown by
the engine (captions, `Talk`) are expanded automatically.
