# Game definition and string tables

`startup.settings` names the game definition file (`GAME="default.game"`)
and the resolution; `default.game` describes the game object: its name,
the scripts attached to it, the system font, the cursors, and the
interface files of the adventure layer. Both are
[definition files](definition-files.md).

Sources: the MIT-licensed
[Wintermute Engine 1.x](https://github.com/MnemonicWME/wintermute-engine)
sources (`CBGame::LoadSettings`, `CBGame::LoadBuffer`,
`CAdGame::LoadBuffer`, `BStringTable.cpp`), and analysis of game files.

Implemented by `luma_formats::GameFile` and `luma_formats::StringTable`;
the game object itself lives in `luma-adventure`.

## `startup.settings`

One `SETTINGS` block, which must be the first statement of the file. A
key that is not in this table is a syntax error (`CBGame::LoadSettings`).

| Key | Default | Meaning |
|-----|---------|---------|
| `GAME` | `default.game` | The game definition file. |
| `STRING_TABLE` | | A string table loaded when the game starts (see below). Each `STRING_TABLE` line empties the table before it loads its file, so the last one is the game's table. |
| `RESOLUTION { width, height }` | `800, 600` | The game's resolution. |
| `ALLOW_ACCESSIBILITY_TAB` | `TRUE` | Whether the player may turn the accessibility features on; when false, text-to-speech, keyboard play and the pause stay off whatever the settings say (see [../game-loop.md](../game-loop.md#accessibility)). |
| `SAVED_GAME_EXT` | `dsv` | The extension of the saved game files (`save001.dsv`). Luma only takes a plain extension (letters, digits, `_`, at most 16 characters). |
| `REQUIRE_SOUND` | `FALSE` | The game refuses to start without a sound device. Read (`GameSettings::require_sound`); the player does not act on it yet. |
| `ALLOW_WINDOWED_MODE` | `TRUE` | The player may run the game in a window. Read (`GameSettings::allow_windowed`); the player does not act on it yet. |
| `ALLOW_DESKTOP_RES` | `FALSE` | The player may have the game stretched to the desktop's resolution. Read (`GameSettings::allow_desktop_res`); the player does not act on it yet. |
| `REQUIRE_3D_ACCELERATION`, `HWTL_MODE`, `ALLOW_ABOUT_TAB`, `ALLOW_ADVANCED`, `REGISTRY_PATH`, `RICH_SAVED_GAMES`, `GUID` | | Direct3D, the Windows settings dialog, the registry and the Windows Games Explorer: accepted, no meaning in Luma. |

A game without `startup.settings` starts with the defaults in Luma (the
original refuses to start).

## `default.game`

The file is read one top-level statement at a time: every `GAME` block
and every `AD_GAME` block, in file order (`CAdGame::LoadBuffer`). **A key
that is not in the tables below is a syntax error that fails the game
file**, in either block and at the top level.

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
| | `TEMPLATE` | Another game file, read whole (its `GAME` and `AD_GAME` blocks) [where the key stands](definition-files.md#template). One that cannot be loaded is skipped. |
| | `SYSTEM_FONT` | The font of engine messages. When the key is absent or the font cannot be loaded, the engine's built-in `system_font.fnt` is used. |
| | `SHADOW_IMAGE` | The image of the 3D actors' simple shadows; the built-in `shadow.png` when absent. |
| | `VIDEO_FONT` | The font of video subtitles (see [../video.md](../video.md)); the system font without one. |
| | `CURSOR`, `ACTIVE_CURSOR`, `NONINTERACTIVE_CURSOR` | The mouse cursor sprites: normal, over an interactive object, while the game is not interactive. |
| | `SUBTITLES`, `SUBTITLES_SPEED`, `VIDEO_SUBTITLES` | Whether speech and videos are subtitled (both true by default), and how long a subtitle stays per character (default 70 ms). |
| | `PERSONAL_SAVEGAMES`, `LOCAL_SAVE_DIR` | Where saved games go. |
| | `SAVE_IMAGE`, `SAVE_IMAGE_X`, `SAVE_IMAGE_Y`, `LOAD_IMAGE`, `LOAD_IMAGE_X`, `LOAD_IMAGE_Y` | The images shown while the game saves or loads, and where (see [../saving.md](../saving.md#loading-and-saving-screens-loading-icon)). |
| | `THUMBNAIL_WIDTH`, `THUMBNAIL_HEIGHT` | The size of the picture `Game.StoreSaveThumbnail` keeps for the next saved game; none when absent (see [../saving.md](../saving.md#thumbnails)). |
| | `PROPERTY { NAME VALUE }` | Script properties of the game object, set as a script's assignment would. |
| | `INDICATOR_X`, `INDICATOR_Y`, `INDICATOR_WIDTH`, `INDICATOR_HEIGHT`, `INDICATOR_COLOR { r, g, b, a }` | The progress bar shown while the game saves or loads: by default along the bottom of the screen, 8 pixels high, translucent red. Read into `GameFile`; Luma does not draw the bar. |
| | `COMPAT_KILL_METHOD_THREADS` | Scripts waiting for an object are not reset when the object's scripts are finished (the behaviour before version 1.9). Read into `GameFile`. |
| | `EVENTS`, `EDITOR_PROPERTY` | Accepted and ignored. |
| `AD_GAME` | `RESPONSE_BOX`, `INVENTORY_BOX`, `ITEMS` | The dialogue, inventory and item definition files. A file that is missing or does not parse fails the game file: the game does not start. |
| | `SCENE_VIEWPORT { left, top, right, bottom }` | The part of the screen the scene is drawn in and scrolled for (`Game.SetSceneViewport` changes it); the whole screen when absent. A scene smaller than the viewport sits lower or further right by the difference, as the original engine's scroll clamp gives a negative offset (OpenQuest's 320x170 title in a 320x189 viewport starts 19 pixels down). |
| | `TALK_SKIP_BUTTON` | Which mouse button skips speech: `right`, `both`; anything else is the left button. |
| | `VIDEO_SKIP_BUTTON` | Which mouse button skips a video: `right`, `both`; anything else is the left button. |
| | `STARTUP_SCENE` | A scene name kept for the game's scripts (`Game.StartupScene`, usually `Game.ChangeScene(Game.StartupScene)`). The engine never loads it by itself. |
| | `DEBUG_STARTUP_SCENE` | In debug mode, the scene the first `ChangeScene` loads instead of the one asked for. Read into `GameFile`; Luma has no debug mode. |

The scene viewport is kept inside the screen: a negative left or top is
0, and the right and bottom stop at the screen's.

## String tables

A string table (`string.tab`, or any file named by `STRING_TABLE` in
`startup.settings` or loaded with `Game.LoadStringTable`) holds the
game's text by identifier, one entry per line:

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
- Lines end with CR LF. The engine cuts a line at the line feed and then
  drops the byte before it, which it takes for the carriage return: in a
  file with bare line feeds every text but the last loses its last
  character.
- When the file cannot be read the table has already been emptied (unless
  it was loaded to extend the current one).
- A file starting with the UTF-8 byte order mark switches the game's
  text encoding to UTF-8; a file without it switches it back to ANSI,
  8-bit text in the game's code page. The text is kept as bytes either
  way and read in the game's encoding where it is drawn (see
  [font.md](font.md#text-encoding)).
- The special entry `@right-to-left` turns right-to-left text on
  (`Game.TextRTL`; see [font.md](font.md#notes-for-implementers)) when
  the file holding it is loaded; a table loaded later without it
  leaves the setting as it is.

Scripts and definition files refer to text as `/ID/fallback`: the
engine replaces it with the table's text for `ID`, or with the fallback
when the table has none, so that a game runs with or without a
translation. `Game.ExpandString` does this on demand; strings shown by
the engine (captions, `Talk`) are expanded automatically.
