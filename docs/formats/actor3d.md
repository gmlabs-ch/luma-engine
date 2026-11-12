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
`AnimationFile`, `ShadowType`), which parses the file into paths and
values and keeps the keys that act on the model in file order
(`Actor3dDef::steps`); `Actor3dDef::build_models` then loads the models
through a closure that reads a file, in that order.

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
token), and Luma does the same, with the original's table of keys and
its way of matching them (`luma_formats::def::keys`: by prefix, in table
order, so `NAMES` is not `NAME`). What was read before the unknown key
has been applied by then, which matters for a template (below).

**A key that fails does not fail the file.** Where a key loads
something (`TEMPLATE`, `MODEL`, `SHADOW_MODEL`, `CURSOR`,
`BLOCKED_REGION`, `WAYPOINTS`) and that fails, the original sets an error
code and goes on to the next statement, which overwrites the code: the
thing is simply absent. The file fails on an unknown key, and at the end
when **no model is loaded**.

**Values** are read with the engine's scanner: a number that is not one
reads as 0, and an **empty value** (`X { }`) leaves what the actor had
(`SCALE` is still divided by 100 then).

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `TEMPLATE` | path | | Another `ACTOR3DX` file loaded **in place** by the same function (below). Put it first: a `MODEL` before it is lost, a `MODEL` after it merges into the template's model. |
| `NAME`, `CAPTION` | string | `<unnamed>`, empty | As for 2D actors. |
| `FONT`, `CURSOR` | path | none | Speech font, cursor sprite. The cursor is loaded at the key and replaces the one before; when it cannot be loaded the actor has **no cursor**. |
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
| `MODEL` | path, repeatable | | The `.X` file. While the actor has no model the key loads one (a file that fails leaves it without, and the next `MODEL` is tried as the model); once it has one, the key merges the file's animation sets into it (`Model::merge_animations`; a failure is logged and the sets read before it stay, [xfile.md](xfile.md) 2.5). **Required**: an actor without a loaded model at the end fails. |
| `SHADOW_MODEL` | path | none | A low-polygon `.X` skinned to the same bones, used instead of `MODEL` for the shadow. Loaded at the key **with the model as its parent** ([xfile.md](xfile.md) 2.6), replacing the one before; a file that fails leaves the actor without a shadow model. Must **follow** a `MODEL` that loaded: before it, the key is ignored (the original logs an error). |
| `SHADOW_TYPE` | `none`, `simple`, `flat`, `stencil` (ignoring case) or a number | `stencil` | The actor's own shadow type. A number is read with `atoi` and clamped to `0..=3` (`ShadowType::parse`); a word that is neither a name nor a number gives 0, `none`. The name must be the whole value: written in braces (`SHADOW_TYPE { flat }`) it carries blanks and reads as 0. |
| `SHADOW` | bool | | Obsolete: `FALSE` sets the type to `none`; `TRUE` changes nothing. |
| `SIMPLE_SHADOW` | bool | | Obsolete: `TRUE` sets the type to `simple`; `FALSE` changes nothing. |
| `SHADOW_COLOR` | `{ r, g, b, a }` | `0, 0, 0, 128` | Colour of flat and stencil shadows; each channel clamped to `0..=255` (the original packs them unclamped). |
| `SHADOW_IMAGE` | path | the game's | Image of the simple shadow. |
| `SHADOW_SIZE` | float | 10 | Half-size of the simple shadow quad in world units; negative values become 0. |
| `LIGHT_POSITION` | `{ x, y, z }` | `-40, 200, -40` | Position of the shadow-casting light **relative to the actor**, scaled by `SCALE`. With fewer than three numbers the others keep their value. |
| `ANIMATION` | block, repeatable | | Properties of animation sets (below), applied **at the key** to the model as loaded so far: a set that a later `MODEL` brings is not found. Must **follow** a `MODEL` that loaded: before it, the block is dropped (the original logs an error). |
| `EFFECT` | block, repeatable | | `{ MATERIAL = "name"  EFFECT_FILE = "file.fx" }`: an HLSL effect on a material, named by its texture file stem ([xfile.md](xfile.md), 2.2.1). Kept only when both keys are present; **parsed and ignored** by the engine (effects are not planned). Must follow `MODEL`. |
| `BLOCKED_REGION` | block | none | A private blocked polygon for the 2D path finder (`2D_PATHFINDING` scenes): the base region keys, as an entity's hotspot. Replaces the one before; a block that cannot be read leaves none. |
| `WAYPOINTS` | block | none | Private waypoints for the 2D path finder. Replaces the group before; a block that cannot be read leaves none. |

`SHADOW_TYPE`, `SHADOW` and `SIMPLE_SHADOW` all write the same field, in
file order (`SHADOW_TYPE = "flat"` then `SHADOW = FALSE` gives `none`).

## Templates

`TEMPLATE = "file"` calls the actor's own loader on that file, in the
middle of the file being read (`CAdActor3DX::LoadFile`, then
`LoadBuffer`). In order:

1. A file that **cannot be read** is logged and changes nothing: the
   model loaded so far stays.
2. The actor's **file name becomes the template's** (the `Filename`
   property then answers the path of the last template read;
   `Actor3dDef::filename_override`).
3. A file whose root is not `ACTOR3DX` is logged and changes nothing
   more.
4. The **model and the shadow model are dropped** (`Actor3dDef::steps`
   starts again), then the template's keys are applied over the current
   values, its own templates included.
5. A template that stops on an unknown key keeps what it applied before
   it, its model included. A template that ends **without a model**
   fails. Either way the failure is only logged
   (`Error parsing ACTOR3D file '...'`) and the file that named the
   template goes on: its later `MODEL` loads the model, or merges into
   the one the template loaded.

`Actor3dDef::parse_with` does this with a closure that reads a path
(`Actor3dDef::log` holds the lines logged); `Actor3dDef::parse` only
records the last `TEMPLATE` and drops the model keys before it. Templates
may nest 16 deep and one actor file loads 64 template files at most; the
original has no bound and a file that names itself overflows its stack.

## `ANIMATION` blocks

An `ANIMATION` block sets properties of **animation sets of the model**,
found by name ignoring ASCII case (`GetAnimationSetByName`, the first
set of that name). `CXModel::ParseAnim` reads its keys in order and acts
on the model as it goes:

| Key | What it does |
|-----|--------------|
| `NAME` | Names the set for what follows. The set is looked up at once; when the model has none, `Error: Animation '<name>' cannot be found in the model.` is logged. A block may have several: each `EVENT` goes to the set named **so far**. |
| `LOOPING` | bool, remembered; the last one counts (an empty value keeps the one before, false at first). |
| `EVENT` | block, repeatable: `{ NAME = "event"  FRAME = n }` (`ParseEvent`). Before any `NAME` of the animation it is dropped and `Error: NAME filed must precede any EVENT fields in actor definition files.` is logged. Else the event is added to the set named so far, when the model has it. An event without a `NAME`, or whose block has a key other than `NAME` and `FRAME`, is dropped alone. `FRAME` starts at -1. |

At the end of the block the remembered `LOOPING` is applied to the set
of the **last** `NAME`, when the model has it (the looping flag the `.X`
file's `AnimationOptions` gave is left alone without a `LOOPING` key).

A key other than these three **stops the block there**: the events
already added stay, `LOOPING` is **not** applied and the function
fails. Nobody acts on that failure: the actor file ignores the result,
and the `.anim` file's error code is overwritten by its next statement.

`AnimationDef::from_node` reads a block into its ordered operations
(`AnimationDef::ops`, `error`), and `AnimationDef::apply_to(&mut model)`
carries them out and returns the lines logged and the function's
result.

An event is added with its frame clamped to at least 1, then to at most
the set's `total_frames` (`total time / frame time + 1`, 0 when the frame
time is 0; see [xfile.md](xfile.md), 2.4). "Frames" are the set's
shortest key gap, so `FRAME = 4` of the demo's walk (keys 200 ticks
apart at 4800 ticks per second) is the pose at 3 x 41 ms.

## The `.anim` sidecar

A `.X` file merged **by script** (`actor.MergeAnims("actors\x\throw.x")`)
may have a sidecar with the same directory and base name and the
extension `.anim` (`Actor3dDef::anim_sidecar`): `actors\x\throw.anim`.
When it exists, it is read after the merge
(`CAdActor3DX::MergeAnimations2`): it holds **top-level `ANIMATION`
blocks** (no root block), each applied to the model as it is read.
The `MODEL` keys of an actor file do **not** look for a sidecar; only
`MergeAnims` does.

A top-level statement that is not an `ANIMATION` block stops the reading
there with `Syntax error in animation definition file`, and `MergeAnims`
then answers false **although the sets are merged and the blocks before
that statement are applied**. A block that fails inside (an unknown key)
does not fail the file. `AnimationFile::parse` keeps the blocks and the
error; `AnimationFile::apply_to(&mut model)` applies them and returns
the lines logged and whether the file succeeded.
`AnimationDef::parse_file` answers the blocks, or the error alone.

## What Luma does with the result

`Actor3dDef` holds paths and values; it reads no file itself. The
original loads the models while it reads the file, so the order of the
keys matters; `Actor3dDef::steps` keeps the `MODEL`, `ANIMATION` and
`SHADOW_MODEL` keys in file order since the last point where the model
was dropped, and `Actor3dDef::build_models(file_name, read)` carries
them out with a closure that reads a path:

1. `MODEL`: `Model::parse` while there is no model, else
   `Model::merge_animations`;
2. `ANIMATION`: `AnimationDef::apply_to` on the model as it is then;
3. `SHADOW_MODEL`: `ShadowModel::parse` with the model as parent.

It returns the model, the shadow model if any, the files they came
from and the lines the original logs (`BuiltModels`), or, when no model
is loaded at the end, those lines as the error. The fields `models`,
`animations` and `shadow_model` list the same keys without their order,
and `Actor3dDef::apply_animations` applies every block to a finished
model, which is not the original's order.

The caller is `luma_adventure::actor3d` (the `3d` feature of the
adventure module; see [../3d-actors.md](../3d-actors.md), section 8),
which reads the file with `Actor3dDef::parse_with` through the game's
file system, uses the other fields as the actor's initial state,
attaches the `SCRIPT`s and sets the `PROPERTY` values on the script
object. The `EFFECT` blocks are read and ignored: Direct3D 9 material
effects are not ported.

A game loads such a file with `Game.LoadActor3D`, `Scene.LoadActor3D`,
or `LoadActor`: the first keyword of the file decides which actor type
is built, as `CAdActor3D::CreateFromFile` does.
