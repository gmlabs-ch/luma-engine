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
| `SPRITE` | string | | A sprite instead of an image; its current frame's first sub-frame holds the tiles (an animated font, see below). |
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
| `WIDTHS_FRAME` | int | `0` | With `SPRITE`, the frame measured by `AUTO_WIDTH`. The original lists `WIDTHS` before `WIDTHS_FRAME` in its table, so that it reads this key as `WIDTHS` followed by garbage and crashes; Luma reads it as it was meant. |
| `EDITOR_PROPERTY` | block | | Editor state, ignored. |

A key outside these tables is a syntax error that fails the font; so is
one in a `TTFONT` block, while a `LAYER` with a key of its own that is
not `OFFSET_X`, `OFFSET_Y`, `COLOR` or `ALPHA` is left out alone. The
first statement of the file decides the kind: `TTFONT`, else `FONT`.
Widths are bytes: a value over 255 keeps its low byte.

Character `c` (a code from 0 to 255) is tile number `c`, row `c /
COLUMNS`, column `c % COLUMNS`, drawn from the image at `(column *
TILE_WIDTH, row * TILE_HEIGHT)` with its width and `TILE_HEIGHT`. The
codes are the bytes of the text in an ANSI game; a UTF-8 game has its
text converted to the ANSI code page first (see
[Text encoding](#text-encoding)).

**Automatic widths** (`CBFontBitmap::GetWidths`). For every tile, the
width is one more than the right-most column holding a pixel that is
not fully transparent (alpha 0, or the colour key), scanning each row
from the right: the soft edge of a smoothed character counts, and an
empty tile has width 0. With a `SPRITE` the frame measured is the one
`WIDTHS_FRAME` names; when the sprite has no such frame, or the frame
no image, nothing is measured and every width is 0. `EXPAND_WIDTH` is
then added
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

Bitmap fonts are drawn untinted: their colours are the image's. The
one exception is a window with `ALPHA_COLOR` or `ALPHA`, which forces
its colour on everything drawn in it: the characters of a bitmap font
are tinted with it like any image, and a TrueType font keeps the
colours of its layers and takes the colour's opacity
(`CBFontTT::DrawTextD3D`).

**Animated fonts** (`CBFontBitmap::DrawChar`). With `SPRITE`, each
character is drawn from the image of the first sub-frame of the
sprite's current frame; the sub-frame's own rectangle, hotspot and
mirroring are not used, and a frame without an image draws nothing. The
sprite moves on as any sprite does, by its frames' `DELAY` and its
`LOOPING`, on the clock that never stops (a font is not paused with the
game). It is asked for its frame each time a character is drawn, so
the first character drawn starts the first frame's time, the sprite
stands still while no text is drawn with the font, and a frame whose
delay is 0 lasts a single character.

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
| `SIZE` | int | `12` | Size in points, at 96 dpi: the pixel size is `SIZE * 96 / 72` rounded to a whole pixel (`MulDiv`). |
| `FACE` | string | `Arial` | The family name of the typeface the text is drawn with (see [Choosing the face](#choosing-the-face)). |
| `FILENAME` | string | none | A font file among the game's files, made available to the engine: it is drawn with when `FACE` names its typeface. Without the key no file of the game is registered. |
| `BOLD`, `ITALIC` | bool | `FALSE` | The weight and the slant of the face. |
| `UNDERLINE`, `STRIKE` | bool | `FALSE` | A line under, or through, every line of the text (see [Metrics](#metrics)). |
| `CHARSET` | int | `0` | The Windows character set of the font: in an ANSI game, the code page the text's bytes are read in (see [Code pages](#code-pages)). |
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

Registering is for the whole process (`CBFontStorage::RegisterTTFont`):
once any font has brought a file, the `FACE` of every other font can
name its typeface.

Luma Engine does the same in three steps; the rules for choosing among
the faces found are its own, GDI's font mapper not being documented:

1. **The game's files.** Each `FILENAME` is read once and kept for all
   the fonts. When the font's own file has a face with `FACE` as a
   family name (compared without case), that file is used; else the
   first other file registered that has one. A font loaded before the
   file that holds its typeface is given it when that file arrives.
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
imitates.

**Missing characters.** A character the chosen face lacks is drawn with
another face that has it, where Windows substitutes a font too (which
one is Luma Engine's own rule): the other faces of the font's own file,
then those of the game's other font files, then the fonts of the
system, where the usual text families (Arial, Liberation Sans,
Helvetica, DejaVu Sans, Noto Sans, Segoe UI, Tahoma...) and the large
CJK families are looked at before the rest, the regular upright face of
a family first. In the family found, the face nearest to the font's
`BOLD` and `ITALIC` that has the character is used, and what it lacks is
imitated. The glyph keeps its own advance; no kerning is applied across
two faces. The system's fonts are searched once per character, and a
face found is parsed once for all the fonts. A character no font has is
skipped and takes no room.

### Metrics

The line height is the font's line spacing in pixels, rounded up, plus
2. Text is rendered to an image as wide as the box: each line has its
baseline `line height - descender` below its top, glyphs at their
bearings, advancing by the integer part of each advance plus the integer
part of the kerning with the previous glyph; the image is drawn with its
top left corner at the text's position, moved by each layer's offset.

These metrics are Luma Engine's own (they come from WME Lite) and are
kept on purpose: the original measures and draws with GDI, whose line is
the font's cell height (ascent plus descent, no line gap, no 2 pixels),
whose pixel size is a whole number and whose widths are GDI's. A text is
therefore not placed pixel for pixel as the original places it; what
agrees is the layout: where lines break, which are dropped, how they are
aligned.

`UNDERLINE` and `STRIKE` draw a line across each line of the text, from
its first glyph to the end of its width. Their place and thickness are
the font's own, as for GDI: the underline position and thickness of the
`post` table, the strike-out position and size of the `OS/2` table,
scaled to the pixel size (at least one pixel thick). A font without
them gets an underline half the descent under the baseline and a
strike-through a quarter of the pixel size above it, a fourteenth of the
size thick.

**Line breaking** (`CBFontTT::FormatText`). A line takes characters
while the text up to them fits the width:

- `\n` ends the line;
- when a character does not fit, the line ends before the last space
  that fitted, and that space is dropped; when no space fitted, it ends
  before the character (in the middle of a word, or before a space,
  which then starts the next line and is the first place that line
  breaks at);
- a first character that does not fit ends the text: nothing more is
  drawn;
- a line that would pass the maximum height is dropped with all that
  follows;
- after a line that held a space, one more character is skipped: the
  space the line broke at, or the line break that ended it. A second
  line break right after is then skipped with it, so two line breaks in
  a row leave an empty line after a line without a space and none after
  a line with one. This is how the original's loop behaves, texts were
  laid out with it, and it is reproduced.

The text height is the number of lines times the line height (0 for an
empty text). The text width is that of the whole text on one line
(`CBFontTT::GetTextWidth` gives the text to GDI in one piece); a line
break counts for nothing in it.

One defect of the original is not reproduced: the first line is tested
against a width left over from measuring the letters "Ay", so a text
that starts with a line break, in a box narrower than those two
letters, makes it read before the start of the text. Luma Engine gives
such a text an empty first line.

A negative width means no box, for both kinds of font: the text is one
line of any length (`\n` still breaks it), drawn from the left whatever
the alignment. A TrueType font then renders the text to an image as
wide as its widest line (at most 4096 pixels). This is a convention of
Luma Engine for the callers that place a line themselves, the editors;
the original engine always gives its fonts a width.

Kerning is applied when the font has a `kern` table.

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
  depending on the Windows version). In an ANSI game the bytes are
  read in the code page of the font's `CHARSET` (see
  [Code pages](#code-pages)).
- **Bitmap fonts** have one tile per byte. In an ANSI game the bytes of
  the text are the tile numbers. In a UTF-8 game the text is decoded and
  each character converted to its byte, or bytes, in the game's ANSI
  code page, `?` for the characters the code page lacks
  (`CBTextUtils::Utf8ToAnsi`): a bitmap font laid out for Windows-1251
  shows Cyrillic text when that is the game's code page.
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

### Code pages

The bytes of an ANSI game's text stand for characters through a Windows
code page. The original engine leaves this to Windows in two places:

- **A TrueType font's text.** GDI reads the bytes a font is given in
  the code page of the font's character set, which `CHARSET` chooses
  (`CBFontTT::GetWindowsCharset`):

  | `CHARSET` | Character set | Code page |
  |-----------|---------------|-----------|
  | `1` | default | the system's |
  | `2` | OEM | the system's DOS code page; read as the system's ANSI one |
  | `3` | Baltic | 1257 |
  | `4` | Traditional Chinese (Big5) | 950 |
  | `5` | Eastern Europe | 1250 |
  | `6` | Simplified Chinese (GB2312) | 936 |
  | `7` | Greek | 1253 |
  | `8` | Korean (Hangul) | 949 |
  | `9` | Macintosh | 10000 (Mac Roman) |
  | `10` | Russian | 1251 |
  | `11` | Japanese (Shift-JIS) | 932 |
  | `12` | Symbol | byte `b` from `0x20` is the character U+F000 + `b`, where symbol fonts keep their glyphs |
  | `13` | Turkish | 1254 |
  | `14` | Vietnamese | 1258 |
  | `15` | Korean (Johab) | 1361; not read, taken as the system's |
  | `16` | Arabic | 1256 |
  | `17` | Hebrew | 1255 |
  | `18` | Thai | 874 |
  | any other, `0` | ANSI | 1252 |

- **Everywhere else** (`CBTextUtils::AnsiToUnicode`, `Utf8ToAnsi`): the
  ANSI code page of the system the game runs on (`CP_ACP`). It reads
  the text given to the screen reader, converts a UTF-8 game's text for
  its bitmap fonts and its editors, and is the code page the keyboard's
  characters arrive in (`Keyboard.KeyCode` of a typed `ж` is 230 on a
  Cyrillic system).

Luma Engine has no system code page to ask. The game's ANSI code page
is, in order: the one the front-end sets (`luma-player --code-page
1251`); the one the first TrueType font loaded with a `CHARSET` of a
language names (a game whose fonts say Russian was made for a Cyrillic
system; the Macintosh and symbol sets say nothing); Windows-1252. That
the fonts tell the system's code page is Luma Engine's inference, not
something the original does.

A byte, or pair of bytes, a code page does not define becomes U+FFFD
(the control codes Windows passes through stay). Converting back, a
character the code page lacks is `?`; Windows also replaces some by a
look-alike (`Ā` by `A`), which Luma Engine does not. The original cuts
the converted text to as many bytes as it has characters, losing the
end of a text in a two-byte code page; that defect is not reproduced.

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
  the game's system font. The font is loaded with the object; one that
  does not load leaves the object without a font of its own (`GetFont`
  is then null).
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
  absolute.
- **Stances.** The `stances` argument lists talk sprite names separated
  by commas, one per sprite played in turn; a name that is not a talk
  sprite, or no name, picks a random `TALK` sprite (a `TALK_SPECIAL`
  sprite only by name). Actors use the sprite of their direction. When
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
- **Skipping.** A left click (or right, or both, per `TALK_SKIP_BUTTON`)
  while the game is not interactive ends every sentence older than 300
  milliseconds.
- `Game.Subtitles` switches the text off; the talk sprites still play.
- The text is drawn centred by default; `align` is 0 left, 1 right, 2
  centre.

A script calling `Talk` waits until the sentence ends; `TalkAsync` does
not. `StopTalk` ends it; `IsTalking` says whether one is playing.

## Notes for implementers

- Bitmap fonts index tiles by byte; in a UTF-8 game, characters outside
  the game's ANSI code page draw as `?` (see [Code pages](#code-pages)).
- Right-to-left scripts. The original gives each line of a TrueType
  text to GDI (`ExtTextOut`), which puts Hebrew and Arabic letters in
  the order they are read and joins Arabic letters; with `Game.TextRTL`
  (turned on by a string table's `@right-to-left` entry or by a script)
  it passes `ETO_RTLREADING`, and the whole line is read from the
  right. Luma Engine does this itself, by rules of its own that stand
  for Windows':
  - each line, once broken (lines break by the widths of the characters
    in the order they are written, as in the original), is put in
    drawing order by the Unicode bidirectional algorithm: runs of
    right-to-left letters are reversed and their brackets mirrored,
    numbers and Latin words keep their order; the line is read from
    the right with `Game.TextRTL` and from the left without;
  - Arabic letters take the form their neighbours call for (isolated,
    final, initial, medial, and the single sign of a lam followed by an
    alef), from the Unicode presentation forms, Persian letters
    included; a form the font lacks while it has the plain letter is
    left as the letter. Other scripts that need shaping (Indic, Thai
    marks) are drawn letter by letter.

  Bitmap fonts ignore all of it, as in the original engine, whose
  `CBFontBitmap` never reads the flag and draws one tile per byte. The
  texts a TrueType font has cached are dropped when the direction
  changes.
- A TrueType font's text is cached as a rendered image per (text, width,
  alignment, height limit); the engine keeps 50 such images per font
  (`NUM_CACHED_TEXTS`), a text used again counting one more use and a
  new one replacing the least used. The original renders a text that is
  not cached at the moment it draws it, so a frame with more than 50
  texts in one font shows them all and renders the surplus again every
  frame. Luma Engine renders the texts before the frame is drawn: a
  text the frame asked for is not given up to make room, the cache
  grows for that frame (up to 512 texts) and shrinks back to 50 when a
  later frame no longer asks for them.
- The `.talk` files that time talk sprites to a sound are not supported
  yet.
- A text that does not fit the height it is given is not drawn at all,
  first line included: a window title in a font whose line is higher
  than the `TITLE_RECT` shows nothing, as in the original engine
  (`CBFontTT::FormatText` stops before a line that would pass the
  maximum height; nothing is clipped).
