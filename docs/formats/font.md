# Fonts (`.font`)

A font file describes either a **bitmap font** (`FONT`): a grid of
character tiles in an image, one tile per Latin-1 code, or a **TrueType
font** (`TTFONT`): a `.ttf` file, a size, and one or more coloured layers
drawn with an offset (outlines and shadows). The engine picks the kind
from the first keyword of the file.

Sources: the MIT-licensed [WME Lite](https://github.com/MnemonicWME/wmelite)
sources (`BFont.cpp`, `BFontBitmap.cpp`, `BFontTT.cpp`, `BFontStorage.cpp`,
`FontGlyphCache.cpp`, `AdSentence.cpp`, `AdObject.cpp`) and analysis of
game files.

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

Character `c` (a Latin-1 code, 0 to 255) is tile number `c`, row `c /
COLUMNS`, column `c % COLUMNS`, drawn from the image at `(column *
TILE_WIDTH, row * TILE_HEIGHT)` with its width and `TILE_HEIGHT`. Text is
Latin-1; a game whose string table is UTF-8 has its text converted to
Latin-1 first.

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
| `FILENAME` | string | `arial.ttf` | The font file, in the game's files. |
| `FACE`, `CHARSET` | | | Windows font selection, ignored. |
| `BOLD`, `ITALIC`, `UNDERLINE`, `STRIKE` | bool | `FALSE` | Styles; the file is expected to carry bold and italic, `UNDERLINE` draws a line, `STRIKE` is read and ignored. |
| `COLOR` | `r,g,b` | `0,0,0` | Colour of the text when no `LAYER` is given. |
| `ALPHA` | int | `0` | Opacity of that colour. |
| `LAYER` | block, repeatable | | A layer: `OFFSET_X`, `OFFSET_Y` (default `0`), `COLOR`, `ALPHA` (default black, transparent). Layers are drawn in file order, each the whole text moved by its offset and tinted with its colour; an outline is four dark layers under a light one. |

Without any `LAYER`, the font has one layer with `COLOR` and `ALPHA`.

When the font file is not among the game's files the engine loads a
system font instead (it logs which one); Luma Engine looks for the usual
Liberation Sans, DejaVu Sans and Arial files of the platform.

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

Glyphs the font lacks are skipped. Kerning is applied when the font has
a `kern` table.

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
- **Skipping.** A left click (or right, or both, per `TALK_SKIP_BUTTON`)
  while the game is not interactive ends every sentence older than 300
  milliseconds.
- `Game.Subtitles` switches the text off; the talk sprites still play.
- The text is drawn centred by default; `align` is 0 left, 1 right, 2
  centre.

A script calling `Talk` waits until the sentence ends; `TalkAsync` does
not. `StopTalk` ends it; `IsTalking` says whether one is playing.

## Notes for implementers

- Bitmap fonts index tiles by Latin-1 code; characters outside Latin-1
  draw as `?`.
- A TrueType font's text is cached as a rendered image per (text, width,
  alignment); the engine keeps 30 such images per font.
- The `.talk` files that time talk sprites to a sound are not supported
  yet.
