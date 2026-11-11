# 3D actors (`.act3d`) and animation sidecars (`.anim`)

A *3D actor* is a skinned, animated [`.X` model](xfile.md) standing on the
painted background of a scene with [3D geometry](geometry.md). Its
definition file names the model (and the files whose animation sets are
merged into it), its speeds, its shadow, and the looping flag and events
of its animation sets. A game loads it like a 2D actor (`Game.LoadActor3D`,
`Scene.LoadActor3D`). The sample projects use the extension `.act3d`; the
engine never looks at it.

Sources: the MIT-licensed Wintermute Engine 1.x sources (`AdActor3DX.cpp`,
`LoadBuffer`, `ParseEffect`, `MergeAnimations`; `AdActor3D.cpp`,
`IsActorX`; `AdObject3D.cpp` and `BObject.cpp` for the defaults;
`XModel.cpp`, `ParseAnim`, `ParseEvent`; `XAnimationSet.cpp`, `AddEvent`)
and the actor files of the Wintermute 3D demo. How the actor behaves at
runtime is in [../3d-actors.md](../3d-actors.md).

Implemented by `luma_3d::actor3d` (`Actor3dDef`, `AnimationDef`,
`ShadowType`), which parses the file into paths and values; the caller
loads the model with `luma_3d::model::Model` and applies the `ANIMATION`
blocks with `AnimationDef::apply`.

## Example

The demo's Trinity:

```
ACTOR3DX
{
  NAME = "trinity"
  CAPTION = "Trinity"
  INTERACTIVE = TRUE
  ACTIVE = TRUE
  SCRIPT = "actors\trinity\trinity.script"
  SHADOW_IMAGE = "actors\trinity\shadow.png"
  SHADOW_SIZE = 12.0
  LIGHT_POSITION { -40, 200, -40 }
  SHADOW_TYPE = "stencil"
  SCALE = 180
  VELOCITY = 70.0
  ANGULAR_VELOCITY = 400.0
  MODEL = "actors\trinity\trinity.x"
  FONT = "fonts\outline_white.font"

  ANIMATION { NAME="idle"  LOOPING=TRUE }
  ANIMATION
  {
    NAME="walk"
    LOOPING=TRUE
    EVENT { FRAME = 4   NAME = "footstep" }
    EVENT { FRAME = 16  NAME = "footstep" }
  }
}
```

## The keyword

The root block is `ACTOR3DX`. The engine reads the **first keyword** of
the file to tell a `.X` actor from a legacy Milkshape one (`ACTOR3D`,
deprecated since WME 1.6, not supported by Luma): `Actor3dDef::is_actor_x`
is that test, and `Actor3dDef::parse` reports a legacy file with a
distinct error. Any other root is an error.

## Keys

Same syntax as every [definition file](definition-files.md). Keys are
case-insensitive; the order matters for the keys marked so. A key the
original does not know **fails the load** (its parser stops on an unknown
token), and Luma does the same.

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `TEMPLATE` | path | | Another `ACTOR3DX` file loaded **in place**: the models, shadow model, `ANIMATION` and `EFFECT` blocks collected so far are dropped, then the template's keys are applied over the current values, then the rest of this file. Put it first: a `MODEL` before it is lost, a `MODEL` after it merges into the template's model. |
| `NAME`, `CAPTION` | string | `<unnamed>`, empty | As for 2D actors. |
| `FONT`, `CURSOR` | path | none | Speech font, cursor sprite. |
| `SCRIPT` | path, repeatable | | Scripts attached to the actor. |
| `PROPERTY` | block, repeatable | | `{ NAME  VALUE }`, a script property. |
| `EDITOR_PROPERTY`, `EVENTS` | | | Accepted and ignored. |
| `ACTIVE` | bool | `TRUE` | Drawn and updated. |
| `INTERACTIVE`, `REGISTRABLE` | bool | `TRUE` | Clickable (one flag, two names). |
| `X`, `Y`, `Z` | float | 0 | Initial 3D position. |
| `ANGLE` | float | 0 | Initial heading in degrees, **as written**: the original calls its angle normalisation and discards the result. |
| `SCALE` | float, percent | 100 | Stored divided by 100 (`Actor3dDef::scale = 1.0`). Scales the model, the walking speed and the shadow. |
| `VELOCITY` | float | 1.0 | Walking speed in world units per second, times the scale. |
| `ANGULAR_VELOCITY` | float | 1.0 | Turning speed in degrees per second. |
| `DROP_TO_FLOOR` | bool | `TRUE` | Re-seat the actor on the walk plane every update. |
| `DRAW_BACKFACES` | bool | `TRUE` | Draw both sides of the triangles. |
| `MODEL` | path, repeatable | | The `.X` file. The first loads the model; each next one merges its animation sets into it (`Model::merge_animations`). **Required**: a file without `MODEL` fails, unless a `TEMPLATE` may supply it (`parse` records the template and lets it pass; `parse_with` loads the template and then insists). |
| `SHADOW_MODEL` | path | none | A low-polygon `.X` with the same bones, used instead of `MODEL` for the shadow. Must **follow** `MODEL`: before it, the key is ignored (the original logs an error). |
| `SHADOW_TYPE` | `none`, `simple`, `flat`, `stencil` (ignoring case) or a number | `stencil` | The actor's own shadow type. A number is read with `atoi` and clamped to `0..=3` (`ShadowType::parse`); a word that is neither a name nor a number gives 0, `none`. |
| `SHADOW` | bool | | Obsolete: `FALSE` sets the type to `none`; `TRUE` changes nothing. |
| `SIMPLE_SHADOW` | bool | | Obsolete: `TRUE` sets the type to `simple`; `FALSE` changes nothing. |
| `SHADOW_COLOR` | `{ r, g, b, a }` | `0, 0, 0, 128` | Colour of flat and stencil shadows; each channel clamped to `0..=255` (the original packs them unclamped). |
| `SHADOW_IMAGE` | path | the game's | Image of the simple shadow. |
| `SHADOW_SIZE` | float | 10 | Half-size of the simple shadow quad in world units; negative values become 0. |
| `LIGHT_POSITION` | `{ x, y, z }` | `-40, 200, -40` | Position of the shadow-casting light **relative to the actor**, scaled by `SCALE`. |
| `ANIMATION` | block, repeatable | | Properties of a named animation set (below). Must **follow** `MODEL`: before it, the block is dropped (the original logs an error). |
| `EFFECT` | block, repeatable | | `{ MATERIAL = "name"  EFFECT_FILE = "file.fx" }`: an HLSL effect on a material, named by its texture file stem ([xfile.md](xfile.md), 2.2.1). Kept only when both keys are present; **parsed and ignored** by the engine (effects are not planned). Must follow `MODEL`. |
| `BLOCKED_REGION` | block | none | A private blocked polygon for the 2D path finder (`2D_PATHFINDING` scenes): the base region keys, as an entity's hotspot. |
| `WAYPOINTS` | block | none | Private waypoints for the 2D path finder. |

`SHADOW_TYPE`, `SHADOW` and `SIMPLE_SHADOW` all write the same field, in
file order (`SHADOW_TYPE = "flat"` then `SHADOW = FALSE` gives `none`).

## `ANIMATION` blocks

An `ANIMATION` block sets properties of **an animation set of the model**,
found by its `NAME` ignoring ASCII case (`GetAnimationSetByName`):

| Key | Meaning |
|-----|---------|
| `NAME` | The set's name. A block without it names no set and is dropped. |
| `LOOPING` | bool; sets the set's looping flag (which the `.X` file's `AnimationOptions` may already have set). Absent, the flag is left alone. |
| `EVENT` | block, repeatable: `{ NAME = "event"  FRAME = n }`. Must come **after** the `NAME` of the animation (earlier events are dropped, the original logs "NAME must precede any EVENT"); an event without a `NAME` is dropped; `FRAME` defaults to 0. |

Other keys inside the block are ignored by Luma (the original's parser
stops at one; in an actor file the block's result is not checked, in an
`.anim` file the whole file fails).

Applying a block (`AnimationDef::apply`): the set's `looping` is set when
the block has `LOOPING`, and each event is added with its frame clamped to
at least 1, then to at most the set's `total_frames` (`total time / frame
time + 1`, 0 when the frame time is 0; see [xfile.md](xfile.md), 2.4).
"Frames" are the set's shortest key gap, so `FRAME = 4` of the demo's walk
(keys 200 ticks apart at 4800 ticks per second) is the pose at 3 x 41 ms.
A block naming a set the model does not have does nothing (the original
logs it).

## The `.anim` sidecar

A `.X` file merged **by script** (`actor.MergeAnims("actors\x\throw.x")`)
may have a sidecar with the same directory and base name and the
extension `.anim` (`Actor3dDef::anim_sidecar`): `actors\x\throw.anim`.
When it exists, it is read after the merge; it holds **top-level
`ANIMATION` blocks** (no root block) and nothing else is read
(`AnimationDef::parse_file`). A syntax error fails the merge. The `MODEL`
keys of an actor file do **not** look for a sidecar; only `MergeAnims`
does.

## What Luma does with the result

`Actor3dDef` holds paths and values; it loads nothing itself. The caller,
`luma_adventure::actor3d::Actor3d::load` (the `3d` feature of the
adventure module; see [../3d-actors.md](../3d-actors.md), section 8):

1. reads the file; with templates, `Actor3dDef::parse_with` and a closure
   that reads a path through the game's file system;
2. loads `models[0]` with `Model::parse(bytes, path)`, then merges each
   further model with `Model::merge_animations`, and loads `shadow_model`
   the same way when present;
3. applies the `ANIMATION` blocks with `Actor3dDef::apply_animations`
   (a block naming no set of the model is logged);
4. uses the other fields as the actor's initial state, attaches the
   `SCRIPT`s and sets the `PROPERTY` values on the script object; the
   `SHADOW_MODEL` path is kept for the shadow drawing step, and the
   `EFFECT` blocks are ignored.

A game loads such a file with `Game.LoadActor3D`, `Scene.LoadActor3D`,
or `LoadActor`: the first keyword of the file decides which actor type
is built, as `CAdActor3D::CreateFromFile` does.
