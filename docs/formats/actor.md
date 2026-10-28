# Actors (`.actor`)

An *actor* is a character that walks: a sprite for each of eight
directions in each of its states (standing, walking, turning, talking),
a position in the scene, and the same hotspot, scale and script keys as
an entity. A game loads its main actor once (`Game.LoadActor`) and keeps
it across scenes; a scene can load its own (`Scene.LoadActor`), which go
with the scene.

Sources: the MIT-licensed [WME Lite](https://github.com/MnemonicWME/wmelite)
sources (`AdActor.cpp`, `AdSpriteSet.cpp`, `AdPath.cpp`,
`AdWaypointGroup.cpp`, `AdObject.cpp`, `AdTalkHolder.cpp`, `BObject.cpp`
and the path finder in `AdScene.cpp`) and analysis of game files.

Implemented by `luma_formats::actor` (the file) and
`luma_adventure::actor` (walking, turning, animation).

## Example

```
ACTOR {
  NAME = "theGirl"
  CAPTION = ""
  SCALABLE = TRUE
  INTERACTIVE = FALSE
  X = 400
  Y = 460
  SCRIPT = "actors\thegirl\theGirl.script"
  FONT = "fonts\outline_red.font"

  WALK {
    SPRITESET {
      LEFT       = "actors\thegirl\ll\walk.sprite"
      RIGHT      = "actors\thegirl\rr\walk.sprite"
      UP         = "actors\thegirl\uu\walk.sprite"
      DOWN       = "actors\thegirl\dd\walk.sprite"
      UP_LEFT    = "actors\thegirl\ll\walk.sprite"
      UP_RIGHT   = "actors\thegirl\rr\walk.sprite"
      DOWN_LEFT  = "actors\thegirl\ll\walk.sprite"
      DOWN_RIGHT = "actors\thegirl\rr\walk.sprite"
    }
  }
  STAND      { SPRITESET { LEFT = "..." RIGHT = "..." UP = "..." DOWN = "..." } }
  TURN_LEFT  { SPRITESET { ... } }
  TURN_RIGHT { SPRITESET { ... } }
  TALK       { SPRITESET { NAME = "talk1" ... } }
  TALK       { SPRITESET { NAME = "talk2" ... } }
}
```

## `ACTOR`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `TEMPLATE` | string | | Another `.actor` file loaded first; the keys that follow override it. |
| `NAME` | string | `""` | Name used by scripts. |
| `CAPTION` | string | `""` | Caption shown when the mouse is over the actor. |
| `X`, `Y` | int | `0` | Position: where the hotspot of the current sprite is placed. |
| `ACTIVE` | bool | `TRUE` | Drawn, updated and clickable. |
| `SCALABLE` | bool | `TRUE` | Follows the scene's scale levels and regions. |
| `SCALE` | int | | Fixed scale in percent, overriding the scene's. |
| `RELATIVE_SCALE` | int | `0` | Added to the scene's scale. |
| `ROTATABLE`, `ROTABLE` | bool | `FALSE` | Follows the scene's rotation levels. |
| `INTERACTIVE`, `REGISTRABLE` | bool | `TRUE` | Reacts to the mouse. |
| `COLORABLE`, `SHADOWABLE` | bool | `TRUE` | Takes the tint of the region it stands in. |
| `ALPHA_COLOR` | `r,g,b` | | Fixed tint; with `ALPHA` as opacity. Like entities, `ALPHA` alone means white. |
| `ALPHA` | int | `0` | Opacity of the fixed tint. |
| `IGNORE_ITEMS` | bool | `FALSE` | Not clickable while an inventory item is selected. |
| `STOP_ON_BLOCKED` | bool | `FALSE` | Stop when something blocks the way instead of looking for another path. |
| `WALK` | block | | The walking sprites, one `SPRITESET`. |
| `STAND` | block | | The standing sprites. |
| `TURN_LEFT`, `TURN_RIGHT` | block | | The sprites shown while turning, one per direction passed. |
| `TALK` | block, repeatable | | A set of talking sprites; a random set is used for each sentence unless the script names one by `NAME`. |
| `TALK_SPECIAL` | block, repeatable | | Talking sprites only used when named. |
| `ANIMATION` | block, repeatable | | A named sprite set (the keys of `SPRITESET` directly in the block) for `PlayAnim` and the newer `WalkAnimName`-style lookups. |
| `FONT` | string | | Font of the actor's speech. |
| `CURSOR` | string | | Cursor sprite shown over the actor. |
| `SCRIPT` | string, repeatable | | Scripts attached to the actor. |
| `PROPERTY` | block, repeatable | | Script properties. |
| `BLOCKED_REGION` | block | | Polygon, relative to the position, that blocks other actors. |
| `WAYPOINTS` | block | | Waypoints, relative to the position, that other actors can walk through. |
| `SOUND_VOLUME` | int | `100` | Volume of the actor's sounds. |
| `SOUND_PANNING` | bool | `TRUE` | Pan the actor's sounds with its position on screen. |
| `EDITOR_PROPERTY` | block | | Editor state, ignored. |

The blocked region and the waypoints are stored relative to the actor's
position and scaled with it: the polygon used at a given moment is the
file's polygon scaled by the scene's scale level at the actor's `y`
(100 % when the actor is not scalable) and moved to its position.

## `SPRITESET`

| Key | Type | Meaning |
|-----|------|---------|
| `NAME` | string | Name of the set (talk stances, animations). |
| `UP`, `UP_RIGHT`, `RIGHT`, `DOWN_RIGHT`, `DOWN`, `DOWN_LEFT`, `LEFT`, `UP_LEFT` | string | The sprite for each direction. |
| `TEMPLATE` | string | A `.spriteset` file loaded first. |

Directions are numbered clockwise from `UP`: `0` up, `1` up-right, `2`
right, `3` down-right, `4` down, `5` down-left, `6` left, `7` up-left.
Scripts see them as `Direction` and pass them to `TurnTo`.

A set need not define every direction. The sprite for a missing one is
the **nearest defined direction by index**: the engine looks downwards
from the wanted index and upwards from it, and takes the closer of the
two (the lower one when equally close). The search does not wrap
around: for `UP_LEFT` (7) with only `UP` (0) and `LEFT` (6) defined, the
answer is `LEFT`.

## Walking

`GoTo(x, y)` corrects the target when it is not walkable, looks for a
path, then walks it one segment at a time.

**Target correction.** From the target, the engine scans right, left, up
and down for the first walkable point that still has a walkable point 5
pixels further in the same direction, and moves the target by the
shortest of the offsets found (horizontal wins a tie); if the point is
still not walkable, it walks back from the target towards the actor,
along the longer axis, to the first walkable point.

**Path.** The graph's vertices are the actor's position (moved to the
nearest walkable point within 2 pixels when it stands on a blocked one),
the target, and every active waypoint of the scene and of the other
active objects, except those on blocked points. Two vertices are joined
when the straight line between them, sampled one pixel at a time along
its longer axis, crosses no blocked point; the edge's length is the
longer of the two axis distances. Dijkstra's algorithm from the actor to
the target gives the path, or an empty path when the target cannot be
reached (the actor then stays and is ready again).

The original engine computes the path a few milliseconds per frame and
the actor waits for it; Luma Engine computes it in one go, which only
changes when the walk starts by at most a frame.

**Following.** Each segment is walked in `max(|dx|, |dy|)` equal steps.
The actor first turns towards the segment's direction (see below), then,
each time the walking sprite shows a new frame, advances
`max(|MOVE x|, |MOVE y|, 1) + 1` steps along the line (`MOVE` is the
frame's key, see [sprite.md](sprite.md)). A step that lands on a point
blocked by another object (the floor regions do not count here) raises
`ActorIsBlocked` on the actor; the actor then looks for a new path, or
stops with `ActorIsBlocked` set when `STOP_ON_BLOCKED` is on. At the
end of the last segment the actor is placed exactly on the target and,
when the walk came from `GoToObject`, turns to the entity's
`WALK_TO_DIR`.

Every move raises `ActorEntry` on the regions of the main layer the
actor enters and `ActorLeave` on those it leaves.

**Direction of a segment** from its angle in degrees (`atan2`, y down,
truncated to an integer): up for `(-112, -67]`, up-right for
`(-67, -22]`, right for `(-22, 22]`, down-right for `(22, 67]`, down for
`(67, 112]`, down-left for `(112, 157]`, left beyond `157` or at or
below `-157`, up-left for `(-157, -112]`; `down` otherwise.

## Turning

`TurnTo(direction)` turns by the shorter way, one direction index per
turn sprite: the actor shows the `TURN_LEFT` or `TURN_RIGHT` sprite of
each intermediate direction, played once (looping is switched off), and
moves to the next direction when it finishes. A difference of one step
is not a turn: the actor faces the new direction at once. Without turn
sprites each step takes one frame.

## Animation

The sprite shown is, in order: the sprite being played by `PlayAnim`, the
turn sprite, the walk sprite of the current direction while walking, the
sprite set by `SetSprite`, else the `STAND` sprite of the current
direction (or the `ANIMATION` named by `IdleAnimName`, `idle` by
default). Each sprite keeps its own frame timing (see
[sprite.md](sprite.md), "Animation"); a frame's `MOVE` offset is applied
to the actor when the frame becomes current, except while walking, where
it sets the step length instead.

`PlayAnim(name)` plays the `ANIMATION` set of that name in the current
direction if there is one, else the sprite file of that name; the actor
is not ready until it ends. `Reset` ends it and makes the actor ready.

## Scripts

The actor object has every property and method of entities, plus:

| Property | Meaning |
|----------|---------|
| `Type` | `"actor"`. |
| `Direction` | The direction faced, `0` to `7`; setting it turns at once. |
| `ActorIsBlocked` | Whether the last walk stopped on a block. |
| `TalkAnimName`, `IdleAnimName`, `WalkAnimName`, `TurnLeftAnimName`, `TurnRightAnimName` | Names of the `ANIMATION` sets used for each state; `null` restores the defaults (`talk`, `idle`, `walk`, `turnleft`, `turnright`). |
| `Ready` | Whether no walk, turn or animation is in progress. |

| Method | Meaning |
|--------|---------|
| `GoTo(x, y)`, `GoToAsync(x, y)` | Walk to a point; the synchronous form makes the script wait, and cancels the scripts already waiting for the actor. |
| `GoToObject(entity)`, `GoToObjectAsync(entity)` | Walk to the entity's `WALK_TO_X`, `WALK_TO_Y` (its position when both are 0) and turn to its `WALK_TO_DIR`. |
| `TurnTo(direction)`, `TurnTo(object)`, `TurnToAsync(...)` | Turn to a direction, or towards an object. |
| `IsWalking()` | Whether the actor is following a path. |
| `SkipTo(x, y)` | Jump to a point. |
| `Reset()` | End the current walk, turn or animation. |
| `PlayAnim(name)`, `PlayAnimAsync(name)` | See above. |
| `SetSprite(file)`, `GetSprite()` | Replace the standing sprite. |
| `HasAnim(name)`, `MergeAnims(file)`, `UnloadAnim(name)` | Query, add from a file of `ANIMATION` blocks, remove named animations. |
| `SetStopOnBlocked(bool)`, `GetStopOnBlocked()` | See `STOP_ON_BLOCKED`. |

`Scene.SkipTo(actor)` and `ScrollTo(actor)` centre the view on the
actor's position raised by half its height; with `Game.MainObject` set
and the scene's `AutoScroll` on, the view follows that object by
`ScrollPixelsX` pixels every `ScrollSpeedX` milliseconds (1 and 10 by
default), and likewise vertically.

## Notes for implementers

- A sprite path repeated for several directions (the example uses the
  same `walk.sprite` for `LEFT`, `UP_LEFT` and `DOWN_LEFT`) is one file;
  the original engine loads it once per direction, each with its own
  frame timing. Luma Engine keeps one timing per path, so switching
  between aliases continues the animation instead of restarting it.
- The actor's height, for `Height`, speech placement and scrolling, is
  the largest `HOTSPOT` `y` of the current frame's sub-frames, scaled by
  the scene's scale at the actor's position when it is scalable.
- A scene's `LoadActor` objects are removed with the scene; the game's
  keep their position and direction, are `Reset` on every scene change,
  and forget which regions they stood in.
