# Fonts (`.font`)

A font file describes either a **bitmap font** (`FONT`): a grid of
character tiles in an image, one tile per 8-bit character code, or a **TrueType
font** (`TTFONT`): a `.ttf` file, a size, and one or more coloured layers
drawn with an offset (outlines and shadows). The engine picks the kind
from the first keyword of the file.

Sources: the MIT-licensed [WME Lite](https://github.com/MnemonicWME/wmelite)
sources (`BFont.cpp`, `BFontBitmap.cpp`, `BFontTT.cpp`, `BFontStorage.cpp`,
`FontGlyphCache.cpp`, `AdSentence.cpp`, `AdObject.cpp`), the MIT-licensed
Wintermute Engine 1.x sources (`BFontBitmap.cpp`, `BFontTT.cpp`,
`BTextUtils.cpp`, `BStringTable.cpp`, `UIEdit.cpp`, `SXString.cpp`) for
the text encoding, and analysis of game files.

Implemented by `luma_formats::font` (the files, the bitmap metrics and
line breaking) and `luma_adventure::text` (loading, TrueType
rasterisation, drawing).

## Bitmap fonts: `FONT`

```
FONT
{
  IMAGE = "fonts\outline_red.bmp"
  TRANSPARENT = 255,0,255
  COLUMNS = 32
  TILE_WIDTH = 12
  TILE_HEIGHT = 15
  AUTO_WIDTH = TRUE
}
```

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `IMAGE` | string | | The image holding the tiles. |
| `SPRITE` | string | | A sprite instead of an image; its current frame's first sub-frame holds the tiles (an animated font). |
| `TRANSPARENT` | `r,g,b` | the engine's default key | Colour key of the image. |
| `COLUMNS` | int | `0` | Tiles per row; required. |
| `TILE_WIDTH`, `TILE_HEIGHT` | int | `0` | Size of a tile; required. |
| `AUTO_WIDTH` | bool | `FALSE` | Measure each character's width from its tile (see below). |
| `WIDTHS` | int list, repeatable | | Explicit widths, from character 0 on, continuing where the previous `WIDTHS` stopped. |
| `DEFAULT_WIDTH` | int | `8` | Width of the characters not covered by `WIDTHS` (without `AUTO_WIDTH`). |
| `SPACE_WIDTH` | int | `0` | With `AUTO_WIDTH`, the width of the space; `0` means half the sum of `m` and `i`. |
| `EXPAND_WIDTH` | int | `0` | With `AUTO_WIDTH`, added to every measured width. |
| `FONTEXT_FIX` | bool | `FALSE` | The tiles are shifted by one: character `c` is in tile `c - 1`. |
| `PAINT_WHOLE_CELL` | bool | `FALSE` | Draw the full tile instead of the measured width. |
| `WIDTHS_FRAME` | int | `0` | With `SPRITE`, the frame measured by `AUTO_WIDTH`. |
| `EDITOR_PROPERTY` | block | | Editor state, ignored. |

Character `c` (a code from 0 to 255) is tile number `c`, row `c /
COLUMNS`, column `c % COLUMNS`, drawn from the image at `(column *
TILE_WIDTH, row * TILE_HEIGHT)` with its width and `TILE_HEIGHT`. The
codes are the bytes of the text in an ANSI game; a UTF-8 game has its
text converted to the ANSI code page first (see
[Text encoding](#text-encoding)).

**Automatic widths.** For every tile, the width is one more than the
right-most column holding a pixel with alpha above 128, scanning each row
from the right (an empty tile has width 0). `EXPAND_WIDTH` is then added
(not below 0), and the space gets `SPACE_WIDTH`, or, when its measured
width is 0 or equal to `EXPAND_WIDTH`, half the sum of the widths of `m`
and `i`.

**Line breaking** (`CBFontBitmap::TextHeightDraw`). The text is laid out
in a box of a given width:

- the last space before the current character is the candidate break (a
  box whose maximum height holds a single line never breaks at spaces);
- `\n` ends the line;
- a character that would exceed the width ends the line before it; when
  no earlier space was usable the line is cut in the middle of the word;
- a line that ends at a space or a break drops that character;
- lines beyond the maximum height are dropped, and a maximum length
  cuts the text after that many characters.

Each line is `TILE_HEIGHT` high, drawn from the top of the box; the text
height is the number of lines times `TILE_HEIGHT` (one tile for empty
text). Alignment places each line at `x`, at `x + (width - line width) /
2`, or at `x + width - line width`.

Bitmap fonts are drawn untinted: their colours are the image's.

## TrueType fonts: `TTFONT`

```
TTFONT
{
  SIZE = 20
  FACE = "Arial"
  FILENAME = "assets\cyberbit.ttf"
  BOLD = TRUE
  COLOR { 0, 0, 0 }
  ALPHA = 255
  LAYER { OFFSET_X = 1 OFFSET_Y = 1 COLOR { 0, 0, 0 } ALPHA = 255 }
  LAYER { OFFSET_X = 0 OFFSET_Y = 0 COLOR { 255, 255, 255 } ALPHA = 255 }
}
```

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `SIZE` | int | `0` | Size in points, at 96 dpi: the pixel size is `SIZE * 96 / 72`. |
| `FACE` | string | `Arial` | The family name of the typeface the text is drawn with (see [Choosing the face](#choosing-the-face)). |
| `FILENAME` | string | `arial.ttf` | A font file among the game's files, made available to the engine: it is drawn with when `FACE` names its typeface. |
| `BOLD`, `ITALIC` | bool | `FALSE` | The weight and the slant of the face. |
| `UNDERLINE`, `STRIKE` | bool | `FALSE` | `UNDERLINE` draws a line under the text; `STRIKE` is read and ignored. |
| `CHARSET` | int | `0` | The Windows character set of the font, ignored: every face is addressed by Unicode character. |
| `COLOR` | `r,g,b` | `0,0,0` | Colour of the text when no `LAYER` is given. |
| `ALPHA` | int | `0` | Opacity of that colour. |
| `LAYER` | block, repeatable | | A layer: `OFFSET_X`, `OFFSET_Y` (default `0`), `COLOR`, `ALPHA` (default black, transparent). Layers are drawn in file order, each the whole text moved by its offset and tinted with its colour; an outline is four dark layers under a light one. |

Without any `LAYER`, the font has one layer with `COLOR` and `ALPHA`.

### Choosing the face

The original engine registers `FILENAME` with Windows as a private
font, then asks GDI for a font by name: the typeface `FACE`, the weight
`FW_BOLD` or `FW_NORMAL`, the italic flag (`CBFontTT::CreateWinFont`).
The file is therefore not what the text is drawn with unless `FACE` is
its typeface: a font file that asks for `FACE = "Arial"`, `BOLD = TRUE`
and ships another typeface as `FILENAME` is drawn in Arial Bold on any
system that has Arial. Many games rely on it.

Luma Engine does the same in three steps; the rules for choosing among
the faces found are its own, GDI's font mapper not being documented:

1. **The game's file.** When `FILENAME` is among the game's files and
   one of its faces has `FACE` as a family name (compared without
   case), the file is used.
2. **The system's fonts.** Otherwise the fonts installed on the system
   are searched for the family `FACE`; when the system lacks it, for
   the families that stand for it: Liberation Sans, Arimo, Helvetica,
   Helvetica Neue, Nimbus Sans and DejaVu Sans for Arial (and for
   Helvetica, Tahoma, Verdana and MS Sans Serif, after Arial itself),
   Liberation Serif, Tinos, Times, Nimbus Roman and DejaVu Serif for
   Times New Roman, Liberation Mono, Cousine, Courier, Nimbus Mono PS
   and DejaVu Sans Mono for Courier New. The system's font directories
   are listed once, the first time a font needs them.
3. **Whatever can be drawn with.** When no font has the family, the
   game's file is used whatever its typeface, and without a file the
   first of the usual Liberation Sans, DejaVu Sans, Arial and Helvetica
   files of the platform.

In a family, the face chosen is the one whose weight is nearest to the
weight asked for (700 with `BOLD`, 400 without), then one with the
right slant, then the first found. What the face lacks is imitated, as
Windows imitates it, by amounts that are Luma Engine's own: a bold asked of a face lighter than 600 thickens each glyph
to the right by a twenty-fourth of the pixel size (at least one pixel)
and widens its advance as much; an italic asked of an upright face
slants each glyph by a fifth of its height about the baseline. A bold
face is never made lighter nor an italic one upright.

The engine logs the file each TrueType font is drawn with, and what it
imitates. A character the chosen face lacks is skipped: it is not
looked up in the game's file or in another font, where Windows would
substitute one.

**Metrics.** The line height is the font's line spacing in pixels,
rounded up, plus 2. Text is rendered to an image as wide as the box:
each line is placed `line height - descender` below its top, glyphs at
their bearings, advancing by the integer part of each advance plus the
integer part of the kerning with the previous glyph; the whole image is
then drawn `text offset` higher, where the offset is the largest bearing
of the last line (the engine's own quirk, kept so that layouts match).

**Line breaking** (`CBFontTT::WrapText`). Characters are added to the
line until one would exceed the width; the line then breaks at the last
space, or before the current character when there is none, and the
space is dropped. `\n` breaks too. Lines beyond the maximum height are
dropped. The text width is the widest line, the height the number of
lines times the line height.

A negative width means no box, for both kinds of font: the text is one
line of any length (`\n` still breaks it), drawn from the left whatever
the alignment. A TrueType font then renders the text to an image as
wide as its widest line (at most 4096 pixels). This is a convention of
Luma Engine for the callers that place a line themselves, the editors;
the original engine always gives its fonts a width.

Glyphs the font lacks are skipped. Kerning is applied when the font has
a `kern` table.

## Text encoding

The engine holds the game's text as the game's files hold it: bytes,
whatever they encode. This is true of the string table, of the string
constants of compiled scripts, of the captions and texts of definition
files and of video subtitles; in Luma Engine each byte is one `char` of
a string, so a UTF-8 `é` is held as the two characters U+00C3 U+00A9.
The encoding is one setting of the game, `Game.TextEncoding`: `0` for
ANSI, `1` for UTF-8. Loading a string table sets it: UTF-8 when the
file starts with the UTF-8 byte order mark, ANSI otherwise
(`CBStringTable::LoadFile`); a script can set it too. It is applied
where characters matter:

- **TrueType fonts** draw and measure the characters the bytes encode
  (`CBTextUtils::ConvertToNative`). In a UTF-8 game the bytes are
  decoded as UTF-8; a sequence that is not valid UTF-8 becomes U+FFFD
  (the original engine draws such a text as ANSI or not at all,
  depending on the Windows version). In an ANSI game each byte is a
  Windows-1252 character: Latin-1, plus the typographic quotes, dashes,
  `€`, `œ` and the others at `0x80` to `0x9F`. The original engine
  uses the code page of the system it runs on; Luma Engine always uses
  Windows-1252, the one western games are written in.
- **Bitmap fonts** have one tile per byte. In an ANSI game the bytes of
  the text are the tile numbers. In a UTF-8 game the text is decoded and
  each character converted to its Windows-1252 byte, `?` for the
  characters the code page lacks (`CBTextUtils::Utf8ToAnsi`): a bitmap
  font cannot show Cyrillic or Greek text.
- **Scripts** count characters: the `Length` of a string and the
  positions of the `String` class are bytes in an ANSI game, the
  characters the bytes encode in a UTF-8 game (see
  [scripting](../scripting.md)).
- **Editors** (`EDIT` controls) hold characters, not bytes: their text
  and caret are drawn as they are whatever the encoding, and in a UTF-8
  game a script reading `Text` gets UTF-8 and the text it writes is
  decoded (`CUIEdit::ScGetProperty`, `ScSetProperty`, `Display`). The
  original editor only takes the characters of the ANSI code page; Luma
  Engine's takes any character the keyboard gives.
- **The screen reader** is given characters (`CBAccessMgr::Speak`).

A string that holds a character beyond U+00FF cannot be bytes; it is
taken as characters already. The texts a TrueType font has cached are
dropped when the encoding changes.

## Where fonts are used

| Use | Font |
|-----|------|
| Speech (`Talk`) | The object's `FONT`, else the game's `SYSTEM_FONT`. |
| Windows and their controls | The control's `FONT` (later milestone). |
| System messages | `SYSTEM_FONT`. |

## Speech

`Talk(text, sound, duration, stances, align)` on an actor or entity shows
the text above the object and plays its talk sprites until the sentence
ends. The text is expanded through the string table.

- **Font.** The object's own (`FONT` in its file, or `SetFont`), else
  the game's system font. The original engine loads the font with the
  object; Luma loads it when the object first talks.
- **Height.** The object's height is the largest hotspot `y` of the
  frame it shows, of the sprite it shows (a talk sprite or a played
  animation, not only its base sprite), scaled with the object
  (`CAdObject::GetHeight`).
- **Duration.** The `duration` argument when positive; else the sound's
  length when a sound plays; else `max(1000, SUBTITLES_SPEED *
  length)` milliseconds, with the length in characters of the expanded
  text and `SUBTITLES_SPEED` from `default.game` (70 by default).
- **Width.** `SubtitlesWidth` when set; otherwise half the screen, or,
  for an object in the outer quarters of the screen, the smaller of
  twice its distance to either edge (at least a quarter of the screen).
- **Position.** The object's position on screen, raised by the text
  height, the object's height and 5 pixels, moved by `SubtitlesPosX`,
  `SubtitlesPosY` (the mods replace the position when
  `SubtitlesPosRelative` is false), centred on the object when
  `SubtitlesPosXCenter`, and clamped to the screen. The position is kept
  in scene coordinates and follows the scroll, unless the mods are
  absolute. "On screen" counts the scene's viewport: a scene position
  is turned into a screen position by taking off the scroll offset less
  the viewport's origin (`CAdScene::GetOffsetLeft`, `GetOffsetTop`),
  when the sentence is placed and again when it is drawn.
- **Stances.** The `stances` argument lists talk sprite names separated
  by commas, one per sprite played in turn; a name that is not a talk
  sprite, or no name, picks a random `TALK` sprite (a `TALK_SPECIAL`
  sprite only by name). The name of an entity's talk sprite is the
  `NAME` key of its `.sprite` file, compared without case, not its
  file name: a plain image, or a sprite without `NAME`, answers to no
  stance. An actor's is the `NAME` of the sprite set.
  Actors use the sprite of their direction. When
  the sprite finishes before the sentence, the next stance starts.
  `ForceTalkAnim(file)` names a sprite file played as the first stance
  of the next sentence only (the stance list still advances under it);
  a file that does not load is logged and the sets are used. An entity's
  sets change at run time with `AddTalkSprite(file, special)`,
  `RemoveTalkSprite(file, special)` and `SetTalkSprite(file, special)`,
  which replaces the set with one sprite (an entity playing a sprite it
  replaced plays the new one at once). On actors these three methods
  load the file and nothing else: in Wintermute 1.x an actor's talk
  sets (`CAdActor::m_TalkSprites`) hide the list the methods edit.
- **Talk definition.** When the sentence has a speech file and the
  game has a file of the same name with the extension `.talk`, that
  definition times the sprites of the sentence and replaces the stances
  while it gives a sprite: see [talk.md](talk.md). A 3D actor ignores
  it (the original computes the sprite and never shows it).
- **Skipping.** A left click (or right, or both, per `TALK_SKIP_BUTTON`)
  while the game is not interactive ends every sentence older than 300
  milliseconds.
- `Game.Subtitles` switches the text off; the talk sprites still play.
- While the game is frozen (a system-exclusive window) the text of a
  freezable object's sentence is not drawn; a sentence of an object
  that is not freezable still is (`CAdGame::DisplaySentences`).
- A saved game keeps the sentences being said, with their talk
  definition; the speech sound starts again at the position it had.
- The text is drawn centred by default; `align` is 0 left, 1 right, 2
  centre.

A script calling `Talk` waits until the sentence ends; `TalkAsync` does
not. `StopTalk` ends it; `IsTalking` says whether one is playing.

## Notes for implementers

- Bitmap fonts index tiles by byte; in a UTF-8 game, characters outside
  Windows-1252 draw as `?` (see [Text encoding](#text-encoding)).
- Right-to-left text (`Game.TextRTL`, turned on by a string table's
  `@right-to-left` entry or by a script) makes a TrueType font lay each
  line's glyphs out from the last to the first, the reading order GDI
  gives the original engine with `ETO_RTLREADING`; there is no
  bidirectional algorithm, so a run of another direction inside a line
  is reversed with it. Bitmap fonts ignore the flag, as in the original
  engine, whose `CBFontBitmap` never reads it. The texts a TrueType font
  has cached are dropped when the direction changes.
- A TrueType font's text is cached as a rendered image per (text, width,
  alignment, height limit); the engine keeps 30 such images per font.
- The `.talk` files that time talk sprites to a sound are not supported
  yet.
- A text that does not fit the height it is given is not drawn at all,
  first line included: a window title in a font whose line is higher
  than the `TITLE_RECT` shows nothing, as in the original engine
  (`CBFontTT::FormatText` stops before a line that would pass the
  maximum height; nothing is clipped).
