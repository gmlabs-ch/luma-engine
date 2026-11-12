# Talk definitions (`.talk`)

A talk definition says which sprite an object shows at which moment of
**one sentence**: mouth shapes timed on a recorded line (lip-sync), a
gesture at a given second. It is a definition file (see
[definition-files.md](definition-files.md)) that sits **next to the
speech sound** of the sentence, with the same name and the extension
`.talk`: the sentence whose sound is `speech\en\line01.ogg` uses
`speech\en\line01.talk` when the game has that file.

This document was written from the MIT-licensed Wintermute Engine 1.x
sources (`AdTalkDef.cpp`, `AdTalkNode.cpp`, `AdSentence.cpp`,
`AdSpriteSet.cpp`); the parser is `crates/luma-formats/src/talk.rs`, the
playback `crates/luma-adventure/src/talk.rs`.

## Example

```
TALK
{
  DEFAULT_SPRITE = "actors\molly\talk\closed.sprite"

  ACTION
  {
    START_TIME = 120
    END_TIME = 340
    SPRITE = "actors\molly\talk\open.sprite"
  }
  ACTION
  {
    START_TIME = 900
    SPRITESET_FILE = "actors\molly\talk\shrug.spriteset"
    PRECACHE = TRUE
  }
}
```

## `TALK`

The root block.

| Key | Type | Meaning |
|-----|------|---------|
| `ACTION` | block, repeatable | One timed sprite; the actions are kept in file order. |
| `DEFAULT_SPRITE` | string | The sprite shown while no action is active. It wins over the default set. |
| `DEFAULT_SPRITESET_FILE` | string | A `.spriteset` file (see [actor.md](actor.md#spriteset)): the default, one sprite per direction. |
| `DEFAULT_SPRITESET` | block | An inline `SPRITESET`. It is read, and a bad one fails the file, but the engine drops it at the end of the block: it has **no effect**. Use `DEFAULT_SPRITESET_FILE`. |
| `TEMPLATE` | string | Another `.talk` file, read into the same definition where the key stands: its actions are added, and the defaults it names replace those named before it and are replaced by those named after it. |
| `EDITOR_PROPERTY` | block | Editor state, ignored. |

## `ACTION`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `START_TIME` | int | `0` | Milliseconds from the start of the sentence at which the action begins. |
| `END_TIME` | int | `0` | The last millisecond of the action (inclusive). `0`, or no key, means **until its sprite has played to its end**. |
| `SPRITE` | string | | The sprite of the action. It wins over the set. |
| `SPRITESET_FILE` | string | | A `.spriteset` file: the sprite of the action for each direction. |
| `SPRITESET` | block | | An inline `SPRITESET` block. |
| `PRECACHE` | bool | `FALSE` | Load the sprite and the set file with the definition instead of at first use. |
| `COMMENT` | string | | For the editor, ignored. |
| `EDITOR_PROPERTY` | block | | Editor state, ignored. |

The times are counted on the game timer from the moment `Talk` was
called, not on the position of the sound, so they stop with the game
when a menu freezes it.

## What fails a definition

A definition that fails is logged (`Error parsing TALK file '...'`) and
the sentence is said without it, with the ordinary talk sprites.

- The root block is not `TALK`, or a block holds a key that is not in
  its table (a syntax error in the original).
- A `TEMPLATE` file that cannot be read or fails itself.
- An inline `SPRITESET` or `DEFAULT_SPRITESET` that fails (see
  [actor.md](actor.md#spriteset): an unknown key, a template or a sprite
  that does not load).
- `DEFAULT_SPRITE` or `DEFAULT_SPRITESET_FILE` that does not load: both
  are loaded with the definition.
- With `PRECACHE`, the action's `SPRITE` or `SPRITESET_FILE` that does
  not load.

Without `PRECACHE` the engine loads an action's sprite the first time
the action is reached: `SPRITE` if there is one; the `SPRITESET_FILE`
only when the action has neither `SPRITE` nor an inline `SPRITESET`. A
file that does not load then leaves the action without a sprite, and the
original tries again on every frame. Luma reads these files when the
sentence starts, with the same result. With `PRECACHE` the set file
replaces an inline set.

"Does not load" means what `CBSprite::LoadFile` means: a sprite file
that exists and does not parse. A file that is **missing** loads, in the
original as its `invalid.bmp` placeholder; Luma shows nothing in its
place.

Luma bounds a chain of templates at 8 files and a definition at 4096
actions; the original has no limit.

## Playing a definition

Each frame of the sentence (`CAdSentence::Update`), with `t` the
milliseconds since it started and the object's direction (an actor's
own; an entity or an item faces down):

1. The actions are visited in file order. An action is **active** when
   `t >= START_TIME` and
   - it has an end time and `t <= END_TIME`; or
   - it has none and its sprite (its own, else the set's sprite for the
     direction) has not played to its end yet. An action whose sprite
     file or set file did not load stays active and has no sprite.
2. The sprite of an active action becomes the definition's current
   sprite, restarted from its first frame when it was not the current
   one. A timed action ends the search; an action without an end time
   lets the following active actions replace it.
3. With no active action, the default sprite is the current one
   (restarted likewise), or none without a default.

The object shows the current sprite in place of its talk stance. When
the definition gives none, the object keeps the sprite it was showing
(the last one the definition gave, or a stance) until that sprite has
played to its end, and then goes on with its talk stances as without a
definition (see [font.md](font.md#speech)). A looping sprite given by
an action therefore stays after the action when there is no default
sprite to replace it.

Three consequences of these rules are reproduced as they are, because a
game's files can depend on them:

- **A sprite that has played to its end is not shown again.** A timed
  action keeps naming it until `END_TIME`, but the object sees a
  finished sprite and falls back to its talk stances, restarting one on
  every frame (so the stance stays on its first frame). A still image,
  or a sprite that does not loop, therefore shows for the time it takes
  to play, not for the whole interval: use a looping sprite to hold a
  pose.
- **Two actions without an end time that are active together** fight:
  the earlier one takes the place for a moment on every frame, so the
  later one is restarted every frame and stays on its first frame until
  the earlier sprite has played.
- An action without an end time whose sprite loops never ends: the
  default is never shown again. A timed action before it in the file
  still wins while it is active; one after it is shown too, but
  restarted on every frame, for the same reason as above.

The sentence itself ends as without a definition: with its sound, else
after its duration.

## In a saved game

A sentence being said keeps its definition, the current sprite and
which sprites have played, so the same sprites follow after loading.
