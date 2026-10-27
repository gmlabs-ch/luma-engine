# Sprites (`.sprite`) and images

A *sprite* is the engine's animated image: a list of frames, each shown
for a delay and made of one or more sub-frames (an image, or a rectangle
of one, drawn at an offset). Everything visible in a game is a sprite:
scene backgrounds and props, actors, cursors, inventory items, buttons.

Sources: the MIT-licensed [WME Lite](https://github.com/MnemonicWME/wmelite)
sources (`BSprite.cpp`, `BFrame.cpp`, `BSubFrame.cpp`, `BSurfaceSDL.cpp`)
and analysis of game files.

Implemented by `luma_formats::sprite` and `luma_formats::image`, and shown
by `luma sprite <game> <file>`.

## Image files as sprites

Wherever a sprite is expected, the file name may point directly at an
image (`.png`, `.bmp`, `.tga`, `.jpg`, compared ignoring case). The engine
then builds a sprite with a single frame, a zero delay, the whole image as
its rectangle, a hotspot of `0,0` and the default colour key.

## Syntax

A `.sprite` file is a [definition file](definition-files.md) with one
`SPRITE` root block:

```
SPRITE {
  NAME="walk"
  LOOPING=TRUE
  CONTINUOUS=FALSE
  PRECISE=TRUE
  FRAME {
    DELAY = 100
    IMAGE = "actors\droid\walk_01.png"
    HOTSPOT {50, 120}
    MOVE {0, -5}
    SOUND="audio\sounds\step.wav"
    SUBFRAME {
      IMAGE = "actors\droid\shadow.png"
      HOTSPOT {50, 110}
      DECORATION=TRUE
    }
  }
  FRAME {
    DELAY = 100
    IMAGE = "actors\droid\walk_02.png"
    HOTSPOT {50, 120}
  }
}
```

### `SPRITE`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `NAME` | string | `""` | Name used by scripts. |
| `LOOPING` | bool | false | Restart from the first frame after the last one. Otherwise the animation stops on the last frame and is *finished*. |
| `CONTINUOUS` | bool | false | The animation may only be interrupted on its last frame (used for talk and walk cycles that must complete). |
| `PRECISE` | bool | true | Mouse hit-testing uses the pixels' alpha instead of the bounding rectangle. |
| `STREAMED` | bool | false | Images are loaded on demand and unloaded after a while, instead of staying in memory. |
| `STREAMED_KEEP_LOADED` | bool | false | With `STREAMED`, keep the images once loaded. |
| `SCRIPT` | string, repeatable | | Script attached to the sprite. |
| `FRAME` | block, repeatable | | The frames, in order. A sprite with no frame draws nothing. |
| `EDITOR_MUTED`, `EDITOR_BG_FILE`, `EDITOR_BG_OFFSET_X`, `EDITOR_BG_OFFSET_Y`, `EDITOR_BG_ALPHA`, `EDITOR_PROPERTY` | | | Editor state, ignored by the engine. |

### `FRAME`

A frame is a sub-frame of its own (its image, rectangle, hotspot and
flags) plus any number of extra `SUBFRAME` blocks drawn after it. The
frame's own sub-frame is always first, even when it has no `IMAGE` (an
empty frame that shows nothing for `DELAY` milliseconds).

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `DELAY` | int | 0 | Time the frame stays on screen, in milliseconds. |
| `KEYFRAME` | bool | false | Marks the frame as a key frame for scripts (`Sprite.Keyframe` events). |
| `MOVE` | `x,y` | `0,0` | Offset applied to the owner's position when this frame becomes current, for animations that move the actor. |
| `SOUND` | string | | Sound played when the frame becomes current. |
| `KILL_SOUND` | bool | false | Stop the previous frame's sound first. |
| `APPLY_EVENT` | string, repeatable | | Script event raised on the owner when the frame becomes current. |
| `SUBFRAME` | block, repeatable | | Extra sub-frames, see below. |
| `EDITOR_SELECTED`, `EDITOR_EXPANDED`, `EDITOR_PROPERTY` | | | Editor state, ignored. |

plus every sub-frame key, which describes the frame's own sub-frame.

### `SUBFRAME` (and the frame itself)

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `IMAGE` | string | | Image file, relative to the game root. Without it the sub-frame draws nothing. |
| `TRANSPARENT` | `r,g,b` | `255,0,255` | Colour key: pixels of exactly this colour are transparent. Only used for images without an alpha channel, see [Images](#images). |
| `RECT` | `left,top,right,bottom` | whole image | Part of the image to draw, right and bottom exclusive. An empty rectangle means the whole image. |
| `HOTSPOT` | `x,y` | `0,0` | Point of the sub-frame placed at the owner's position: the sub-frame is drawn at `(X - x, Y - y)`. Actors put it at their feet. |
| `ALPHA` | int 0–255 | 255 | Opacity. |
| `ALPHA_COLOR` | `r,g,b` | `255,255,255` | Tint multiplied with the image. Together with `ALPHA` they form the modulation colour; `255,255,255,255` means "unchanged" and lets the owner's own alpha apply instead. |
| `MIRROR_X`, `MIRROR_Y` | bool | false | Flip horizontally / vertically. |
| `DECORATION` | bool | false | The sub-frame is not part of the clickable area. |
| `2D_ONLY`, `3D_ONLY` | bool | false | Shown only when the owner is a 2D, respectively 3D, actor. |
| `EDITOR_SELECTED`, `EDITOR_PROPERTY` | | | Editor state, ignored. |

Numbers written as lists (`HOTSPOT {50, 25}`) and as unquoted values
(`TRANSPARENT = 255,0,0`) are both accepted; the editors write lists.

### Animation

The engine keeps a current frame and the time it was entered. When the
frame's `DELAY` has elapsed it moves to the next frame; after the last
one it either restarts (`LOOPING`) or stops and marks the sprite finished.
A delay of `0` advances on every update. When the current frame changes,
its `MOVE` offset is applied to the owner, its `SOUND` is played and its
`APPLY_EVENT`s are raised.

## Images

Images are `.png`, `.bmp`, `.jpg` or `.tga` files. The engine decodes them
with FreeImage and converts them to 32-bit RGBA as follows:

- An image with an **alpha channel** (32-bit PNG, TGA, ...) keeps it; the
  colour key is ignored.
- A **BMP** never keeps its alpha channel, even a 32-bit one, because
  Windows tools rarely write it correctly.
- Every other image (24-bit, palette, grayscale) is opaque, except that
  pixels equal to the **colour key** (`TRANSPARENT`, default magenta
  `255,0,255`) become fully transparent.

The engine also derives a one-bit mask from the alpha channel for
pixel-precise hit testing (`PRECISE`), and stretches images with bilinear
filtering when scaled.

## Notes for implementers

- Keep the colour key's RGB in transparent pixels or clear it consistently;
  with premultiplied alpha and bilinear filtering, clearing it avoids
  magenta fringes.
- Image paths use `\` and are case-insensitive; resolve them through the
  virtual file system, loose files first.
- A missing image is not fatal for the engine: it substitutes
  `invalid.bmp`. Report it, but keep loading the sprite.
