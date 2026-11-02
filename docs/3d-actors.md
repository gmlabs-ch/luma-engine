# 3D actors on 2D backgrounds (study)

Status: **study for planning, no code yet.** This note collects what the
Wintermute Engine 1.x sources do for 3D characters standing on painted 2D
backgrounds ("2.5D"), what Luma Engine would need to do the same, and a
proposed split into tasks. It is not a format specification yet; the
specifications (`docs/formats/actor3d.md`, `geometry.md`, `xfile.md`) come
with the tasks that implement them.

## 0. Sources and rules

Read for this study, all MIT-licensed, under `src/engine_core/` of
Wintermute Engine 1.x (`https://github.com/MnemonicWME/wintermute-engine`;
paths below are relative to that directory unless they start with `doc/` or
`install/`):

- Actors and objects: `wme_ad/AdActor3DX.cpp`, `AdActor3D.cpp` (legacy MS3D
  actors and the file sniffing), `AdObject3D.cpp`, `AdAttach3DX.cpp`,
  `AdPath3D.cpp`, `AdPathPoint3D.cpp`.
- Scene geometry: `wme_ad/AdSceneGeometry.cpp`, `AdGeomExt.cpp`,
  `AdGeomExtNode.cpp`, `AdWalkplane.*`, `AdBlock.*`, `AdGeneric.*`,
  `AdWaypointGroup3D.cpp`, `wme_base/3DLoader3DS.cpp`, `3DS_chunks.h`,
  `3DMesh.cpp`, `3DCamera.cpp`, `3DLight.cpp`, `3DUtils.cpp`.
- Models and animation: `wme_base/XModel.cpp`, `XFrameNode.cpp`, `XMesh.cpp`,
  `SkinMeshHelper.cpp`, `XAnimation.cpp`, `XAnimationSet.cpp`,
  `XAnimationChannel.cpp`, `XActiveAnimation.cpp`, `XFile.cpp`,
  `3DMaterial.cpp`.
- Rendering and shadows: `wme_base/BRenderD3D.cpp`, `3DShadowGen.cpp`,
  `3DShadowVolume.cpp`, `BRenderer.cpp` (picking), `BObject.cpp`.
- Compositing and game integration: `wme_ad/AdScene.cpp` (`TraverseNodes`,
  `DisplayRegionContent`, `UpdateFreeObjects`, the scene keys and the 3D
  script methods), `AdGame.cpp` (`GetLayerSize`, `GetMaxShadowType`,
  `LoadActor3D`).
- Documentation shipped in the same repository: `doc/html/wme/3D_*.html`,
  `doc/source/eng/scripting_ref_actor3d.txt`, `scripting_ref_scene.txt`,
  `scripting_ref_game.txt`.
- Sample project data in the same repository, used to understand the
  formats: `install/source/projects/wme_demo_3d/` (a scene with a `.3DS`
  geometry, a `.geometry` file, `.act3d` files, `.x` models, scripts).

Not read, by the clean-room rule: ScummVM or any other GPL code. The
`.X` loading in WME is done by Microsoft's DirectX file and D3DX
libraries (`D3DXLoadSkinMeshFromXof`, `rmxftmpl.h`), which are not in the
WME sources; what those libraries do is described here from the
public DirectX documentation and from the sample files. Such statements
are marked **[ext]** and need a test against real files before they are
trusted.

Luma Engine already documents and implements the 2D side; this study only
adds the 3D side. `docs/formats/sprite.md` has one correction to make (see
section 14).

## 1. Overview

A 3D scene is an ordinary 2D scene (layers, entities, regions, scroll
offset, painted background) plus **hidden scene geometry**: a `.3ds` file
that carries the cameras and lights the background was rendered with, the
**walk planes** where actors may stand, the **blocked objects** they avoid,
the **waypoints** their path finder links, and extra meshes that only
receive shadows. A **3D actor** is a skinned, animated `.X` model described
by an `.act3d` file. It is loaded by a script (`Game.LoadActor3D`,
`Scene.LoadActor3D`) like a 2D actor and behaves like one for the rest of
the game: it has a name, a caption, a script, a position `X`/`Y` in the
scene, it is clickable, it walks to a point (`GoTo`), turns, talks, plays
animations and sits among the other free objects in the drawing order.

What is different is only the middle layer:

```
 script (GoTo x,y / PlayAnim / TurnTo ...)
        |
 actor logic (state machine, same states as a 2D actor)
        |   x,y in scene pixels  <--->  X,Y,Z in the geometry's world units
 scene geometry (camera projection, walk planes, blocks, waypoints)
        |
 model (frames, skin, animation channels)  --->  triangles, lit, textured
        |
 frame: 2D layers ... 3D actor drawn in the middle of the 2D order ... 2D
```

The 2D side stays authoritative for **ordering and occlusion**: the 3D actor
is drawn at its place in the painter's order of the scene's free objects
(sorted by the `y` of its projected position, inside the region it
stands in), exactly where a 2D actor would be drawn. Anything that must
hide the actor (a foreground pillar) is a 2D entity, region or layer drawn
later. The only depth buffer interplay is between 3D things: actors with
each other, and with the shadow-receiving geometry when stencil shadows
are on (section 10.4).

The 3D features need Direct3D in WME 1.x: with the software renderer
`LoadActor3D` fails and `GEOMETRY` is ignored (`AdScene.cpp`,
`AdGame.cpp`). Luma always draws with the GPU, so the compatibility mode is
not a concern.

### 1.1 What is in the sources, by size

| Area | WME files | Lines (approx.) |
|------|-----------|-----------------|
| Actor logic and scripts | `AdActor3DX.cpp`, `AdObject3D.cpp`, `AdAttach3DX.cpp` | 3400 |
| Scene geometry, path finding, picking | `AdSceneGeometry.cpp`, `AdGeomExt*.cpp`, `3DUtils.cpp`, `3DLoader3DS.cpp` | 3200 |
| Model, skinning, animation | `XModel`, `XFrameNode`, `XMesh`, `XAnimation*`, `XActiveAnimation` | 4000 |
| Shadows and the 3D parts of the renderer | `3DShadow*.cpp`, `BRenderD3D.cpp` (3D parts) | 1000 |
| Compositing in the scene | `AdScene.cpp` (3D parts) | 600 |
| Not needed: HLSL effects, MS3D actors | `3DEffect*.cpp`, `AdActor3D.cpp`, `3DModel.cpp`, `3DLoaderMS3D.cpp`, `3DSkelAnim.cpp`, `3DJoint.cpp` | 4900 |

## 2. Conventions of the original (read this first)

Everything in WME 3D is plain Direct3D 8/9 fixed-function, and numbers only
come out right with the same conventions:

- **Left-handed**, **Y up**, matrices are **row-major and used with row
  vectors** (`v' = v * M`, translation in `_41.._43`). `A * B` means "A,
  then B".
- Perspective is `D3DXMatrixPerspectiveFovLH` (vertical field of view,
  depth range 0..1), view is `D3DXMatrixLookAtLH`. These match wgpu's
  clip space exactly (z in 0..1, y up in NDC, framebuffer origin top-left),
  so the matrices can be handed to the GPU as they are, transposed for
  column-vector shaders.
- Front faces are **clockwise** (`D3DCULL_CCW` culls counter-clockwise ones).
  Actors draw both sides by default (`m_DrawBackfaces = true`), so culling
  rarely matters for display, but picking and shadow volumes depend on the
  winding (below).
- Angles are **degrees** in scripts and files, radians in the matrices. The
  model matrix of an object is `RotationYawPitchRoll(angle, 0, 0) * Scale
  * Translation` (`BObject.cpp`, `GetMatrix`): a rotation about Y, then a
  uniform scale (`SCALE` / 100), then the position. Walking at heading
  `angle` moves by `dx = -sin(angle)`, `dz = -cos(angle)`, so angle 0 walks
  toward `-Z` (it is the direction constant `DOWN`, toward a camera that looks
  along `+Z`); a model's front is therefore expected to point to `-Z`
  (inferred from the code).
- Colours are `0xAARRGGBB`; the scripts' `MakeRGBA`.
- Scene geometry and models share one unit (the author's), `SCALE` and the
  clipping planes are in it; nothing converts to pixels except the camera.

## 3. The scene side

### 3.1 Scene keys

Parsed in `AdScene.cpp` (`CAdScene::LoadBuffer`). These are the 3D keys of a
`SCENE { ... }` block; the 2D keys are in
[formats/scene.md](formats/scene.md).

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `GEOMETRY` | path | none | The `.3ds` geometry file. Only `.3ds` is accepted (`AdSceneGeometry.cpp`, `LoadFile`). |
| `CAMERA` | string | first camera | Name of the active camera (case-insensitive); an unknown name logs a warning and keeps the first. |
| `FOV_OVERRIDE` | float | -1 | Field of view in **radians** (SceneEdit shows degrees, writes radians) replacing the camera's. A negative value keeps the file's. |
| `NEAR_CLIPPING_PLANE`, `FAR_CLIPPING_PLANE` | float | -1 | World-unit distances. Negative means the renderer's defaults: **near 90, far 10000** (`BRenderD3D.cpp`). |
| `WAYPOINT_HEIGHT` | float | 10 | How far above the floor the waypoints are dropped. |
| `2D_PATHFINDING` | bool | false | Actors path-find on the scene's 2D regions and waypoints exactly like 2D actors; the geometry is then only used to map 2D to 3D. |
| `MAX_SHADOW_TYPE` | int 0..3 | 2 (flat) | Highest shadow type allowed in the scene. |
| `SCROLL_3D_COMPATIBILITY` | bool | false | Pre-1.7 behaviour: the camera sees only the top-left screenful of the scene, whatever the layer size (section 4.1). |
| `AMBIENT_LIGHT_COLOR` | `{ r,g,b }` | `0,0,0` | The scene's ambient light. |
| `EDITOR_SHOW_GEOMETRY`, `EDITOR_RESOLUTION_WIDTH/HEIGHT` | | | Editor only. The runtime uses the game's resolution, not these. |

Fog is set by script (`Scene.EnableFog`), not by a key.

### 3.2 The `.3ds` geometry file

`3DLoader3DS.cpp` is a flat chunk walker, not a general 3DS reader. Chunk
header: `u16 id`, `u32 length` (including the 6 bytes of the header), little
endian. Known **container** chunks are entered (the walker simply continues
into the payload); every other chunk is skipped by its length.

| Id | Name in `3DS_chunks.h` | What the loader does |
|----|------------------------|----------------------|
| `0x4D4D` | `MAIN3DS` | Entered. |
| `0x3D3D` | `EDIT3DS` | Entered. |
| `0x4000` | `NAMED_OBJECT` | Starts an object: NUL-terminated name, then entered. |
| `0x4100` | `TRIANGLE_MESH` | Marks the current object as a mesh; entered. |
| `0x4110` | `TRIANGLE_VERTEXLIST` | `u16 n`, then `n` x (x, y, z) floats. **Stored as (x, z, y)**: the Y/Z swap turns the Z-up right-handed 3DS space into the Y-up left-handed space. |
| `0x4140` | `TRIANGLE_MAPPINGCOORS` | `u16 n`, `n` x (u, v), `v` becomes `1 - v`. Read but not used by the geometry. |
| `0x4120` | `TRIANGLE_FACELIST` | `u16 n`, `n` x (`u16` a, b, c, flags). The second and third index are swapped on reading, which reverses the winding to match the axis swap. |
| `0x4130` | `TRIANGLE_MATERIAL` | Parsed (name, face list) and kept; not used for geometry. |
| `0x4160` | `TRI_LOCAL` | 4x3 local matrix, read but not applied by `LoadMesh`: vertices are used as stored (the exporter is expected to bake transforms). |
| `0x4700` | `CHUNK_CAMERA` | Marks the object as a camera. Payload: position (x, z, y), target (x, z, y), `bank` (degrees), `lens` (mm). |
| `0x4600` | `CHUNK_LIGHT` | Marks the object as a light. Payload: position (x, z, y); children follow. |
| `0x4610` | `LIGHT_SPOTLIGHT` | Target (x, z, y), hotspot, falloff; makes the light a spot. |
| `0x4620` | `LIGHT_IS_OFF` | The light starts disabled. |
| `0x0010`, `0x0011` | `RGB_FLOAT`, `RGB_BYTE` | Inside a light: its colour (floats x 255 truncated, or bytes). Elsewhere skipped. |
| `0xB000`, `0xB002`..`0xB007` | `KEYFRAMER` and `KEYFRAMER_*_INFO` | Entered; the section kind is remembered. |
| `0xB010` | `NODE_HDR` | Reads the keyframer object name (three `u16` follow). |
| `0xB024` | `ROLL_TRACK_TAG` | Only inside the camera-info section: the first key's roll replaces the camera's `bank`. |

Derived values:

- **Camera.** `fov_degrees = 1900 / lens` (45 when `lens <= 0`),
  `fov = radians(fov_degrees)`. The view matrix is `LookAtLH(position,
  target, up)` with `up = (0,1,0)` rotated about Z by `bank` degrees
  (`3DCamera.cpp`). The first camera is active after loading; a scene's
  `CAMERA` selects another. Cinema 4D exports a wrong lens, hence
  `FOV_OVERRIDE`.
- **Light.** `m_DiffuseColor` from the colour chunk, `active = !LIGHT_IS_OFF`,
  `spotlight` and `target` from `LIGHT_SPOTLIGHT`. Light multipliers and
  every other 3DS light chunk are ignored.
- **Mesh.** Smooth vertex normals are computed by summing the face normals
  of every face sharing a vertex index (`C3DMesh::ComputeNormals`), face
  normal `(v2 - v1) x (v3 - v2)`.
- 3DS names are limited to about ten characters by common exporters (the
  sample has `blk_wareho`, `shadow_Bo0`); scripts name nodes as the file does.

### 3.3 Naming: which mesh is what

After parsing, each mesh is matched by name against the **node list** of
the optional sidecar `<geometry name>.geometry` (same directory, same base
name, extension `.geometry`) followed by three built-in nodes. First match
wins; a mesh matching nothing is **dropped**.

```
GEOMETRY
{
  NODE
  {
    NAME = "shadow_*"
    RECEIVE_SHADOWS = TRUE
  }
}
```

| Key of `NODE` | Meaning |
|---------------|---------|
| `NAME` | Pattern, case-insensitive, `*` any run, `?` any one character (`CBUtils::MatchesPattern`). |
| `WALKPLANE` / `BLOCKED` / `WAYPOINT` | bool; the node's role. Each key that is `TRUE` sets the role, so if several are true the last one in the file wins; none means a **generic** mesh. |
| `RECEIVE_SHADOWS` | bool, default false. |

Built-in nodes, added after the file's: `walk_*` (walk plane, receives
shadows), `blk_*` (blocked, does not), `wpt_*` (waypoint, does not). Without
a `.geometry` file only these three exist, so a `.3ds` of only `walk_`,
`blk_`, `wpt_` meshes plus cameras and lights is complete.

- **Walk plane** (`CAdWalkplane`): triangles an actor may stand on; used for
  picking a 3D point under a 2D point, for the floor height, and (by
  piercing) as a barrier in segment tests. Vertex colour for the debug
  overlay `0x700000FF`.
- **Blocked** (`CAdBlock`): triangles that stop segments; overlay colour
  `0x70FF0000`. Also receives shadows when the node says so.
- **Waypoint**: every `wpt_*` mesh contributes **one point**, the centre of
  its bounding box, to a single group (`AdWaypointGroup3D::AddFromMesh`);
  all are dropped to `floorHeight + WAYPOINT_HEIGHT` after loading.
- **Generic** (`CAdGeneric`): any other matched mesh; only useful as a
  shadow receiver (and for the debug overlay, colour `0x7000FF00`).
- Walk planes, blocks and generics have an `active` flag, switched by
  `Scene.EnableNode3D` / `DisableNode3D` by mesh name (case-insensitive),
  and saved with the scene.

The sample geometry (`install/source/projects/wme_demo_3d/data/scenes/
Front/geometry.3DS`, 30 KB): one `walk_01`, five `blk_*`, ten `wpt_*`, twelve
`shadow_*` boxes (receivers, via the `.geometry` above), four lights
(`Spot01`, `Spot02`, `Omni02`, `Omni03`) and two cameras (`Camera01`,
`Camera02`); the scene file selects `Camera01` and `MAX_SHADOW_TYPE=3`.

### 3.4 Lights

`C3DLight` (`3DLight.cpp`) maps to a fixed-function light:

| Property | Value |
|----------|-------|
| Diffuse | colour channels divided by **256** (not 255), alpha 0 |
| Ambient, specular | 0 |
| Point light | position; range 100000; attenuation `Atten0 = 1` (no falloff by distance) |
| Spot light | position and direction `target - position` (range = its length), theta 0.5 rad, phi 1.0 rad, falloff 1.0, `Atten0 = 1` |

Only as many lights as the hardware offers are enabled per object
(`GetMaxActiveLights`, 8 typical, `-1` means 100). When more are active in
the scene, per object the active lights are sorted by distance to the
object's position (for a spot the point 75% of the way to its target) and
the nearest `MaxLights` are used (`AdSceneGeometry.cpp`, `EnableLights`). An
object can exclude lights by name (`AddIgnoredLight`).

The scene's ambient colour is `D3DRS_AMBIENT` and is **black by default**:
without lights a lit model renders black (plus its emissive colour). An
object can override the ambient colour (`AmbientLightColor` property).

## 4. Camera and the 2D/3D mapping

This is the heart of "2.5D": a point of the painted background and a point
of the 3D world are tied by one perspective camera whose image plane is the
**main layer** of the scene.

### 4.1 Projection

Parameters (`BRenderD3D.cpp`, `GetProjectionParams`; `AdGame.cpp`,
`GetLayerSize`):

- `R = (RW, RH)`: the game's resolution (the runtime ignores the editor
  resolution keys).
- `L = (LW, LH)`: the size of the scene's main layer (`R` instead when
  `SCROLL_3D_COMPATIBILITY`).
- `V = (VW, VH)` and margins `mleft, mright, mtop, mbottom`: the current
  viewport (the whole screen unless `SCENE_VIEWPORT` is set).
- `Off = (OffX, OffY)`: the scroll offset of the layer being drawn (for the
  main layer, the scene offset).
- `ModW = max(0, (LW - RW) / 2)`, `ModH = max(0, (LH - RH) / 2)`; if
  `LH < RH`: `ModH -= (RH - LH) / 2` and `LH = RH` ("a layer shorter than the
  screen is not scrolled, the camera sees the whole screen").
- `S = max(LW / RW, LH / RH) * (RH / VH)`.

The projection matrix is `PerspectiveFovLH(fov, VW/VH, near, far)` with

```
_11 *= S            _22 *= S
_31  = -(OffX + (mleft - mright)/2 - ModW) / VW * 2
_32  =  (OffY + (mtop - mbottom)/2 - ModH) / VH * 2
```

With the typical `V = R`, no margins, the main layer, a world point in
camera space `(x, y, z)` lands at **scene (layer) pixel**

```
px = LW/2 + (RW/2) * S * x / (z * aspect * tan(fov/2))      aspect = RW/RH
py = LH/2 - (RH/2) * S * y / (z *          tan(fov/2))
```

(`px, py` are screen pixels plus the scroll offset; the matrix shift cancels
the scroll, so an actor's scene position does not depend on it.) So the
optical axis passes through the **centre of the layer**, and the vertical
field of view spans the layer's height when the layer has the screen's
aspect ratio (otherwise the larger ratio decides, `S`).

### 4.2 The inverse and the object's 2D position

- **2D to 3D** (`AdSceneGeometry.cpp`, `Convert2Dto3D`): take scene pixel
  `(X, Y)`; subtract `(mleft + mright)/2 + ModW` from `X` and `(mtop +
  mbottom)/2 + ModH` from `Y`; the ray in camera space is
  `((2X/VW - 1) / P11, -(2Y/VH - 1) / P22, 1)`; rotate it by the inverse
  view matrix, origin at the camera position; the answer is the **nearest
  hit on any walk plane** (by distance from the camera). It fails (false)
  when the ray misses every walk plane. Hits use the single-sided
  Moeller-Trumbore test of `C3DUtils::IntersectTriangle` (section 9.2), so
  **walk plane triangles must face up toward the ray** (the 3DS loader's
  winding flip makes a plane that faces up in the 3D package face up here).
- **Tolerant** (`Convert2Dto3DTolerant`): if the exact conversion fails,
  scan in steps of 10 pixels (up to 1000) to the left, right, up and down for
  the first pixel that converts. For each axis keep the nearer of its two
  directions; shift the point along the axis with the smaller shift (the
  vertical one when equal) and convert that pixel. No hit in any direction
  fails. Used for every walking target.
- **3D to 2D** (`Convert3Dto2D`, `CAdObject3D::Convert3DTo2D`): project with
  the same matrices, add the scroll offset, truncate to `int`. An object's
  `X`/`Y` are the projection of its **origin** (the feet) computed in
  `CAdObject3D::Update` whenever its 3D position changed; these `X`/`Y`
  drive the drawing order, the region it stands in, auto-scroll and speech
  position.
- Bone positions in 2D (`GetBonePosition2D`, particles) go through the
  same function.

### 4.3 Scrolling

The scene scrolls toward the main object as for 2D actors (`ScrollToObject`
on `X`, `Y - height/2`). For a 3D main object a change smaller than 5 pixels
is ignored (`AdScene.cpp`, `ScrollTo`) to avoid jitter from the projected
position. The actor's `GetHeight()` is `Y - boundingRect.top - 5` (the
projected height of its box).

The projection is rebuilt each time the renderer enters 3D state
(`Setup3D`), from the scroll offset in effect for the layer currently being
drawn. For an actor drawn in the main layer this is the scene scroll. An
actor standing in a region of another layer would use that layer's
(parallax) offset; this is a corner case.

## 5. The actor definition file (`.act3d`)

Same syntax as every definition file ([definition-files.md](formats/
definition-files.md)). The keyword is `ACTOR3DX` for `.X` actors;
`AdActor3D.cpp` (`IsActorX`) reads the first keyword and takes the legacy
Milkshape route (`ACTOR3D`, deprecated since WME 1.6, see section 13) for
anything else. The file extension used by the sample projects is `.act3d`;
the loader never looks at it.

```
ACTOR3DX
{
  NAME = "trinity"
  CAPTION = "Trinity"
  INTERACTIVE = TRUE
  SCRIPT = "actors\trinity\trinity.script"
  SCALE = 180
  VELOCITY = 70.0
  ANGULAR_VELOCITY = 400.0
  MODEL = "actors\trinity\trinity.x"
  SHADOW_TYPE = "stencil"
  LIGHT_POSITION { -40, 200, -40 }
  ANIMATION { NAME = "walk"  LOOPING = TRUE  EVENT { FRAME = 4  NAME = "footstep" } }
}
```

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `TEMPLATE` | path | | Another actor file, loaded in place. Put it first: each load drops the model loaded so far, so a `MODEL` before the `TEMPLATE` is lost, while a `MODEL` after it is merged into the template's. |
| `NAME`, `CAPTION`, `FONT`, `CURSOR`, `SCRIPT` (repeatable), `PROPERTY`, `EDITOR_PROPERTY` | | | As for 2D actors. |
| `ACTIVE` | bool | true | |
| `INTERACTIVE`, `REGISTRABLE` | bool | true | Clickable. |
| `X`, `Y`, `Z` | float | 0 | Initial 3D position. |
| `ANGLE` | float | 0 | Initial heading (the call that should normalise it discards its result, so the value is stored as written). |
| `SCALE` | float, percent | 100 | Stored as `/ 100` (`m_Scale3D`). It scales the model, the walking speed and the shadow. |
| `VELOCITY` | float | 1.0 | Walking speed in world units per second, times the scale. |
| `ANGULAR_VELOCITY` | float | 1.0 | Turning speed in degrees per second. |
| `DROP_TO_FLOOR` | bool | true | Re-seat the actor on the walk plane every update. |
| `DRAW_BACKFACES` | bool | true | Draw both sides of triangles. |
| `MODEL` | path | | The `.X` file. **Repeatable**: the first loads the model, each next one *merges its animation sets* into it (`MergeFromFile`). Required: without a model the load fails. |
| `SHADOW_MODEL` | path | | Low-polygon `.X` with the same bones, used instead of `MODEL` for shadow volumes and flat-shadow silhouettes. Must follow `MODEL`. |
| `SHADOW_TYPE` | `none`, `simple`, `flat`, `stencil` or 0..3 | `stencil` | The actor's own shadow type. |
| `SHADOW` | bool | | Obsolete: `FALSE` means `none`. |
| `SIMPLE_SHADOW` | bool | | Obsolete: `TRUE` means `simple`. |
| `SHADOW_COLOR` | `{ r,g,b,a }` | `0,0,0,128` | Colour of flat and stencil shadows. |
| `SHADOW_IMAGE` | path | the game's | Image for simple shadows. |
| `SHADOW_SIZE` | float | 10 | Half-size of the simple shadow quad, in world units. |
| `LIGHT_POSITION` | `{ x,y,z }` | `-40,200,-40` | Position of the shadow-casting light **relative to the actor**, scaled by `SCALE`. |
| `ANIMATION` | block, repeatable | | Properties of a named animation set of the model: `NAME`, `LOOPING`, `EVENT { NAME, FRAME }` (repeatable). Must follow `MODEL`. Animations without a block keep the defaults. |
| `EFFECT` | block | | `{ MATERIAL = "...", EFFECT_FILE = "...fx" }`, HLSL effect on a material. See section 13. Must follow `MODEL`. |
| `BLOCKED_REGION`, `WAYPOINTS` | blocks | | As for 2D actors: a private polygon and waypoints for the 2D path finder (`2D_PATHFINDING` scenes only). The polygon is scaled by the scene's scale levels at the actor's `Y`. |

`ANIMATION` blocks may also be placed in a sidecar `<name>.anim` next to a
`.x` that is merged by script (`MergeAnims("...\throw.x")` loads
`...\throw.anim` if it exists; only `ANIMATION` blocks are read).

The old key `START_FRAME`, `END_FRAME` and per-animation `MODEL` belong to
the legacy `ACTOR3D` format only.

## 6. The model: the `.X` subset WME uses

### 6.1 Who parses what

`CXFile::OpenFile` reads the whole file, creates a DirectX file object,
registers the **standard `d3drm` templates** (`rmxftmpl.h`) and enumerates
the top-level data objects. `CXModel::LoadFromFile` hands each object to
`CXFrameNode::LoadFromXData` and `CXMesh::LoadFromXData` calls
`D3DXLoadSkinMeshFromXof`. That means the binary, text and compressed
container variants (`xof 0303txt 0032`, `bin `, `tzip`, `bzip`), template
declarations inside the file, data references and the triangulation of
polygons are all done by Microsoft's libraries **[ext]**. A Luma loader has
to reproduce the useful part. Every sample file (`trinity.x`, `teapot.x`,
`teapot_taken.x`) is **text, 32-bit floats** (`xof 0303txt 0032`), declares
its non-standard templates at the top, and has no data references except
the animation targets.

### 6.2 Top-level objects

`CXFrameNode::LoadFromXData`, recursive. The root of the model is a
synthetic empty frame holding the file's top-level objects; the model is
rejected if no top-level frame was found.

| Object (template) | Handling |
|-------------------|----------|
| `Frame` `{ ... }` (3D82AB46-62DA-11CF-AB39-0020AF71E433 [ext]) | A named node; children are loaded recursively. Names are matched **case-sensitively** (`strcmp`) by `FindFrame`. |
| `FrameTransformMatrix` | 16 floats, row-major, stored as both the *original* and the current local matrix. |
| `Mesh` | A mesh attached to the frame; loaded by D3DX (section 6.3). |
| `AnimationSet` | Loaded into the model (section 6.4); also accepted inside a frame. |
| `Animation` (outside a set) | Becomes an anim set named after the file. |
| `AnimTicksPerSecond` `{9E415A43-7BA6-4a73-8743-B73D47E88476}` | One `DWORD`; default **4800**. All animation key times are in these ticks. |
| anything else | Ignored. |

Merging a second file (`MergeFromFile`) takes only `AnimationSet` and
`Animation` objects, searching the frames of the file recursively (the
frames themselves are not added); the bones of the new animations are looked
up by name in the existing skeleton (`FindBones`). An unnamed animation set
takes the **file name without path and extension** as its name.

### 6.3 Meshes (what D3DX does, **[ext]**)

For a `Mesh` WME wants a *skin mesh* with its adjacency, materials and bone
information. The data comes from these templates, all standard:

| Template | Content used |
|----------|--------------|
| `Mesh` | vertex positions, faces (any polygon, triangulated) |
| `MeshNormals` | normals indexed per face corner; where the normal or the UV of a vertex differs between faces, D3DX duplicates the vertex |
| `MeshTextureCoords` | one (u, v) per vertex, V downward as in Direct3D |
| `MeshMaterialList` | per-face material index; materials listed inline or as references |
| `Material` | `faceColor` (RGBA, used as diffuse), `power`, `specularColor`, `emissiveColor`; child `TextureFilename` |
| `XSkinMeshHeader` (3cf169ce-ff7c-44ab-93c0-f78f62d172e2) | max weights per vertex and per face, bone count |
| `SkinWeights` (6f0d123b-bad2-4167-a0d0-80224f25fabb) | per bone: frame name, vertex indices, weights, 4x4 `matrixOffset` (the inverse bind matrix) |
| `VertexDuplicationIndices` (b8d65549-d7c9-4995-89cf-53a9a8b031e3) | maps duplicated vertices to originals, so weights follow the duplicates |

The three GUIDs are those of the template declarations at the head of the
sample `.x` files, which also declare `AnimTicksPerSecond`, `FVFData`,
`DeclData`, `EffectInstance` and the `Effect*` templates. WME ignores the
effect templates (it asks D3DX for no effect instances); a loader should skip
every template it does not know.

What WME makes of the result:

- **Materials.** `Diffuse` from `faceColor`, `Ambient = Diffuse` (WME sets it
  explicitly), specular and emissive from the file. A mesh without
  materials gets one grey material (`0.5, 0.5, 0.5`).
- **Texture path.** The `TextureFilename` is resolved **relative to the
  `.x` file's directory**; the texture is loaded as an ordinary image
  surface, so the 2D image rules apply, notably the **magenta colour key
  (255, 0, 255) becomes transparent** by default.
- **Material name.** The name by which scripts address a material
  (`SetTexture`, `SetEffect`, `EFFECT { MATERIAL }`) is the **texture file
  name without path and extension** (`C3DMaterial::SetTexture`, "adopt
  name"). Untextured materials have no name and cannot be addressed.
- **Skinning.** Weighted bone skinning, performed **on the CPU** each frame
  (`XMesh.cpp`, `Update`; `SkinMeshHelper.cpp`): `palette[i] = boneOffset[i]
  * boneCombined[i]` (the bone's combined matrix from the root, so the
  mesh's own frame matrix is not applied), then D3DX `UpdateSkinnedMesh`
  rewrites the positions and normals of a clone of the loaded mesh (the
  blended-mesh conversion is commented out). The result keeps the loaded
  vertex format (position, normal, UV) and is drawn per material subset
  (D3DX attribute table).
- **Rigid meshes** (no bones) are transformed every frame by their parent
  frame's combined matrix: **positions only**; the normals stay as loaded, so
  the lighting of rigid parts on moving frames is slightly off. A faithful
  port can keep this or transform the normals as well.
- **Bounding box.** Recomputed from the skinned vertices every frame
  (`D3DXComputeBoundingBox`, with a manual fallback when NaNs appear).
- **Adjacency.** Face-to-face adjacency for shadow volumes is regenerated on
  the clone with `GenerateAdjacency(0, ...)`: vertices at identical
  positions count as one point, so UV and normal seams do not open edges. A
  Luma loader must weld by position.

### 6.4 Animation sets

`AnimationSet` holds `Animation` objects. Each `Animation` has:

- exactly one **frame reference** (a data reference to a `Frame`; only its
  name is kept and later looked up with `FindFrame`),
- `AnimationKey` objects, each `{ keyType; nKeys; keys }` with each key
  `{ time; nValues; values }`:

| `keyType` | Content | Handling in `XAnimation.cpp` |
|-----------|---------|------------------------------|
| 0 | rotation, 4 values **w, x, y, z** | kept as quaternion; at use the **xyz are negated** (conjugate) before `Slerp` |
| 1 | scale, 3 values | `Lerp` |
| 2 | position, 3 values | `Lerp` |
| 4 | 16-value matrix | decomposed into position, scale and a quaternion (`DecomposeMatrixSimple`: translation row, scale = row lengths, rotation from the matrix, xyz negated on load and again on use) and stored as the three tracks above |
| other | | logged and ignored |

A track may appear once per animation (duplicates are an error).

- an optional `AnimationOptions { openClosed; positionQuality }`: a non-zero
  `openClosed` sets the whole set to **looping**. `positionQuality` is
  ignored.

A frame with no scale/rotation/position track in a given animation is left
at its original matrix (the pose is rebuilt every frame from
`FrameTransformMatrix`, then overwritten for animated frames only; this is
what makes channels layerable, section 7).

Timing derived from the keys: the *total time* of a set is the largest key
time of any track divided by `ticksPerSecond / 1000` (milliseconds); the
*frame time* is the smallest gap between two consecutive keys of any track
(milliseconds). "Frames" exist only to place events (`FRAME` is 1-based).

## 7. Animation at runtime

All in `XModel.cpp`, `XAnimationChannel.cpp`, `XActiveAnimation.cpp`,
`XAnimation.cpp`, `XFrameNode.cpp`.

- **10 channels** (`X_NUM_ANIMATION_CHANNELS`), numbered 0..9. Channel 0
  carries the standard animations the actor logic selects (idle, walk,
  turn, talk-stance). Others are for scripts (`PlayAnimChannel`).
- Each frame (`CXModel::Update`): reset every frame to its original local
  matrix; update every channel in order 0..9, each of which **sets the local
  transform of the frames its animation has tracks for**, later channels
  overriding earlier ones on the same frame; compute combined matrices from
  the root; skin the meshes. So a "wave" animation that only has tracks for
  the arm bones, played in channel 1, replaces the arm of the walk cycle.
- **Sampling** one track at local time `t` (in ticks): find the first key
  with `time > t`; interpolate it with the previous one
  (`(t - t1) / (t2 - t1)`, `0` when equal times); `t` before the first key
  gives the first key. If *no* key is later than `t` the indices stay at
  **0 and 0**: the pose snaps to the **first** key (not the last). Callers
  keep `t` below the last key time (a finished animation uses
  `total - 1` ms) so this is only reached by degenerate data.
- **Active animation.** Started with the game timer; local time = elapsed
  ms. If `elapsed > total`: looping wraps with `%`, a non-looping one
  becomes *finished* and holds `total - 1`. `Looping` comes from the set
  (`AnimationOptions` or the actor file's `LOOPING`).
- **Events.** Frame index `= localTime / frameTime + 1`; when it changes
  (not during a transition) `OnFrameChanged(current, previous)` fires every
  event with `previous < FRAME <= current`, and after a wrap those with
  `FRAME > previous` first. Event frames are clamped to `[1, totalFrames]`
  when added. An event runs the owner's event handler by name
  (`ApplyEvent("footstep")`, i.e. `on "footstep" { ... }` in the actor's
  script).
- **Transitions** (`PlayAnim(set, transitionTime, stopTransitionTime)`).
  A channel holds up to two active animations. With transition time 0, or
  when the channel is empty, the new one replaces the old at once. Otherwise
  both poses are **frozen** for the duration: the old animation at the local
  time it had reached (slot 0) and the new one at its first pose (slot 1,
  because its local time is still 0). While `Game.timer - start < time`,
  `UpdateMatrices` blends the two with `lerp = elapsed / time` (`Lerp` for
  scale and position, `Slerp` for rotation, then `S * R * T`). At the end the
  new animation becomes the only one, its start time is reset and it starts
  playing from its beginning. No events fire during a transition. Asking for
  the animation that already plays on the channel (same name) does nothing
  unless the request forces a restart; the scripts' `PlayAnim` forces it, the
  actor's own idle/walk/turn requests do not.
- **The transition time actually used** when switching from animation `A`
  to `B` on a channel is `Owner.GetAnimTransitionTime(A, B)`: the pair set
  by `SetAnimTransitionTime`, else the actor's `AnimTransitionTime` (default
  200 ms). The `AnimStopTransitionTime` (default 200 ms) is used when an
  animation finishes or is stopped (`StopAnim`): the animation is made
  non-looping, held for that time, then the channel empties and the frames
  fall back to their original matrices (there is no blend back to the rest
  pose in the code; the fade-out only delays the removal).
- `IsAnimPending(channel, name)` is true while the channel's active
  animation (the incoming one during a transition) is not finished and, when
  a name is given, has that name (case-insensitive). The scripts'
  `IsAnimPlaying` looks at **channel 0 only**.

## 8. Actor logic

### 8.1 Update order, per frame

`CAdActor3DX::Update` (`AdActor3DX.cpp`):

1. Return if the game is frozen or the actor has no model.
2. In `READY`, forget the state-animation channel. If a sentence exists but
   the state is not `TALKING`, finish it and stop the talk channel.
3. The state machine below (animation requests go to channel 0).
4. If the state is `PLAYING_ANIM` and its channel no longer has a pending
   animation, go to the next state (`READY`).
5. `UpdateBlockRegion` (the private 2D polygon), `Ready = (state == READY)`.
6. `CAdObject3D::Update`: drop to floor, world matrix, 2D position
   (section 4.2); `AfterMove` hook if `X`/`Y` changed.
7. `Model.Update()` (animation, skinning, boxes), the shadow model likewise.
8. Attachments' `Update`, particle emitter position.

`CAdActor3DX::Display`: set up lights (`SetupLights`: pick the lights for the
actor's position); ambient override; shadow (section 10.4); world matrix;
`Model.Render()`; register the clickable rectangle; attachments (each
rendered with `attachment.worldMatrix * boneCombined * actorWorld`);
particles in 2D; restore ambient.

### 8.2 States

The actor uses the same states as 2D actors (`AdTypes.h`): `READY`,
`SEARCHING_PATH`, `WAITING_PATH`, `FOLLOWING_PATH`, `TURNING`, `TALKING`,
`PLAYING_ANIM`, `DIRECT_CONTROL`. In every state not mentioned the idle
animation is requested each frame (the animation name list is
`IdleAnimName` = `idle`, `WalkAnimName` = `walk`, `TalkAnimName` = `talk`,
`TurnLeftAnimName` = `turnleft`, `TurnRightAnimName` = `turnright`, all
settable). A request for an animation the model does not have simply does
nothing.

| State | Per frame |
|-------|-----------|
| `READY` | request idle |
| `SEARCHING_PATH` | request idle; ask the path finder (3D: `Geom.GetPath`, 2D mode: `Scene.GetPath`) every frame until it accepts the request, then `WAITING_PATH` |
| `WAITING_PATH` | request idle; when the path is ready go to `FOLLOWING_PATH` (`FollowPath3D`/`2D`) |
| `FOLLOWING_PATH` | step along the path (below); request walk |
| `TURNING` | request turn-left or turn-right; `TurnToStep(AngularVelocity)`; when done go to the next state |
| `TALKING` | stance animations of the sentence on channel 0 (forced restart), else the `talk` animation on `TalkAnimChannel`, else idle; ends with the sentence |
| `PLAYING_ANIM` | idle on channel 0 unless the script's animation is on channel 0; ends when the script's channel finishes |
| `DIRECT_CONTROL` | see 8.4 |

### 8.3 Walking and turning

- **Target.** `GoTo(x, y)`: skipped if both `|x - X| <= GoToTolerance` and the
  same for `y` (default 2 pixels). In a geometry scene it converts `(x, y)`
  with the tolerant conversion and calls `GoTo3D`; in a `2D_PATHFINDING`
  scene it corrects the target with the scene's 2D rules and walks the 2D
  path. `GoToObject(entity)` uses the entity's `WALK_TO_X/Y` and
  `WALK_TO_DIR` (turn to that direction at the end), or its position.
  Scripts block on `GoTo` and continue on `GoToAsync`.
- **Path in 3D.** The geometry's path finder returns a list of points
  (section 9). The actor first **turns in place** toward the first point
  (`TurnTo`, then `FOLLOWING_PATH`), then walks.
- **Step.** Heading angle `a = degrees(-atan2(dz, dx)) - 90` toward a point.
  Each frame `TurnToStep` rotates toward `a` at `AngularVelocity` degrees per
  second (turning while walking along every point after the first), and the
  position moves along the **target heading** (not the current one):
  `x += -sin(target) * v * dt`, `z += -cos(target) * v * dt`, with
  `v = Velocity * Scale` and `dt` in seconds. When the step would
  not get closer than before (`|P - pos| < |P - newPos|`) the actor snaps to
  the point and takes the next one; after the last it turns to
  `AfterWalkAngle` if there is one, else becomes `READY`.
- **Turn.** `PrepareTurn` normalises both angles into 0..360 and chooses the
  shorter way. Normalisation is `while (a > 360) a -= 360; while (a < 0)
  a += 360`, so 360 stays 360.
- **Directions.** `DirToAngle` for the eight direction constants: `UP 180`,
  `UPRIGHT 225`, `RIGHT 270`, `DOWNRIGHT 315`, `DOWN 0`, `DOWNLEFT 45`,
  `LEFT 90`, `UPLEFT 135`. `AngleToDir` (the `Direction` property) uses
  eight 45-degree sectors centred on those, boundaries at 22, 67, 112, 157,
  202, 247, 292, 337. `TurnTo(object)` turns toward the object's 2D position
  converted to 3D (no tolerance; if the conversion fails the target is
  garbage in the original).
- **2D path mode.** Each path point `(x, y)` is converted with the tolerant
  conversion and walked as a 3D point the same way.
- **Waypoint height.** Path points carry the waypoint height; the actor takes
  that `y` when snapping to a point and `DropToFloor` puts it back on the
  floor in the same update. With `DROP_TO_FLOOR = FALSE` the actor therefore
  floats at waypoint height while following a path.

### 8.4 Direct control

`DirectWalk(velocity, anim)`, `DirectWalkBack`, `DirectTurnLeft/Right`
(`DirectWalkStop`, `DirectTurnStop`) put the actor into `DIRECT_CONTROL`
until both modes are off. Each frame: turn at the given (or the actor's)
angular velocity (`Left` = `angle -= ...`, `Right` = `angle += ...`); a
walking step in the heading direction (back: opposite) of
`velocity * scale * dt`; the step is accepted if (2D mode) the scene is not
blocked at the projected new position, else if the straight segment from
the old to the new position does not hit the geometry
(`DirectPathExists`); on success the walk (or given) animation is requested,
else idle. Turning without walking first requests the turn animation (or
idle), but the movement code that follows runs with a zero-length step, which
is always allowed, and requests the walk animation over it: as written the
actor walks in place while turning and the turn animation is overridden in the
same frame (and, being a different name, restarts a transition each frame).
Reproduce or choose one, after looking at the demo.

### 8.5 Drop to floor, position, scale

`CAdObject3D::Update`: if `DropToFloor`, `y = GetHeightAt(position, 5)`.
`Scale` is a uniform factor; `Velocity` and the shadow follow it. Setting the
scripts' `X`/`Y` stores the 2D value and converts the point to 3D with the
exact conversion (no tolerance); if the conversion fails the 3D position is
left alone and the two disagree until the next 3D move. `SkipTo(x, y)` does
the same conversion and stores the 2D position only on success.
`SkipTo3D(x, y, z)` sets the 3D position; the 2D position follows at the next
update.

### 8.6 Attachments

`AddAttachment(file, name, bone)` loads a second `.x` model as an object of
its own (`CAdAttach3DX`, an `CAdObject3D` with `DropToFloor = false`), owned
by the actor: same name replaces. The bone must exist (`GetBoneMatrix`,
a combined matrix looked up by exact frame name) or the call raises a script
error. Each frame the attachment's model animates (its own channels, owner
for events is the actor) and is drawn with `attachment world matrix *
bone combined matrix * actor world matrix`; the attachment's own `PosX/Y/Z`,
`Angle`, `Scale` therefore offset it relative to the bone. It registers the
actor's clickable rectangle too (the actor is clickable through its
attachments) and takes part in the actor's stencil shadow. Persisted with
the actor.

### 8.7 Persistence (what a saved game stores)

`Persist` of the actor, object and model: position vector and its last
value, angle, target angle and point, 2D target point, turning flag,
after-walk angle, path points (3D and 2D), velocity, angular velocity, drop
flag, all five animation names, direct-control modes and parameters, default
and stop transition times, the pair table, talk channel, go-to tolerance,
attachments (name, bone, model file, state), ignored lights, ambient
override, bone/offset of the particle emitter, the model's merged file
names and per channel the two active animations (name, start time, looping,
finished, last local time, current frame) and the transition state. The
scene's geometry saves the active camera and light/node states by name, plus
the pending path request. Luma has its own save format
([saving.md](saving.md)), which needs these fields, not WME's layout.

## 9. Geometry at runtime

All in `AdSceneGeometry.cpp` and `3DUtils.cpp`. Everything is brute force
over all triangles of all planes and blocks (geometry is meant to be a few
hundred triangles).

### 9.1 Floor height

`GetHeightAt(pos, tolerance)`: the start is `pos` raised by `tolerance`. A
walk plane triangle is a candidate when the vertical line through the start
meets it from above (single-sided, the triangle must face up) and the height
`h` of the plane there is not above `start.y + tolerance` (the line is not a
ray: planes above the start count within that margin). The candidate nearest
in height to the start wins. With no candidate the input `y` is returned
unchanged. The `active` flag of walk planes is **not** consulted here, nor in
picking and path tests; only the debug rendering and the shadow-receiver
pass skip inactive planes.

### 9.2 Triangle tests

- `IntersectTriangle(origin, direction, v0, v1, v2)` is Moeller-Trumbore
  **without** the `t >= 0` check and **single-sided**: `det < 0.0001` fails,
  i.e. the triangle is hit only when the ray direction opposes the triangle's
  front normal `(v1 - v0) x (v2 - v0)`. On success the three result floats
  are re-purposed to hold the intersection **point** (plane/line
  intersection), not barycentric coordinates.
- `PickGetIntersect(a, b, v0, v1, v2)`: does the segment `a..b` cross the
  **plane** of the triangle (`0 <= t <= 1`, not parallel)? It does not test
  containment.
- `DirectPathExists(p1, p2)`: false if, for any walk plane triangle (active
  or not) or **active** block triangle, the segment crosses the plane **and**
  `IntersectTriangle(p1, p1 - p2)` or `IntersectTriangle(p2, p2 - p1)` hits
  the triangle: that is, if the segment pierces the triangle in either
  direction. Free segments between floor-height points do not pierce planes,
  so walk planes mostly matter through picking and height, and blocks
  through piercing. Staying inside a walk plane's outline is **not**
  checked: it is the author's waypoints that keep paths on the floor.

### 9.3 Path finder

`GetPath(source, target, path)` starts a request, `PathFinderStep` advances
it; the scene's `InitLoop` runs steps for up to `PFMaxTime = 15` ms per game
loop. Only one request is open at a time (`GetPath` returns false while
busy, and the actor asks again next frame).

1. Both ends are lifted: `y = GetHeightAt(p, waypointHeight) +
   waypointHeight`.
2. Nodes: source (distance 0), target (infinite), and every point of every
   active waypoint group (infinite).
3. A plain Dijkstra: take the unmarked node with the lowest distance; if it
   is the target, build the path by following the origin links and **drop the
   first point** (the start); otherwise relax every unmarked node whose
   straight segment passes `DirectPathExists` (edge length = Euclidean
   distance).
4. **Fallback.** If the target was never reached, the request is rerun once
   (`Rerun = true`) with an *alternate target*: while running, for every
   edge from the current node to the target found blocked, the shortest such
   edge supplies `GetBlockIntersection` (the first piercing point on the
   blocks, in file order), moved back `30` units toward the node. The
   alternate is also recomputed for edges that were visible but not
   shorter; the original does this unconditionally, and when no block is
   hit it produces a bogus point. A faithful port should keep the "blocked
   edge" condition only and say so.
5. The path is a list of 3D points; the actor walks them with 8.3.

### 9.4 Other geometry services

- `Convert2Dto3D`, `Convert2Dto3DTolerant`, `Convert3Dto2D`: section 4.
- `EnableLights`: section 3.4. `SetLightColor`, `EnableLight`: change the
  light and re-register all lights.
- `EnableNode`/`IsNodeEnabled`: by mesh name over blocks, planes and
  generics.
- `DropWaypoints`: re-lifts every waypoint (at load and when
  `WaypointsHeight` changes).
- `Render(showGeometry)`: the debug overlay (wireframes in the colours
  above and the waypoint marker sprite `wpt.sprite`); it also stores the
  matrices and scroll of the frame for the conversions.
- `RenderShadowGeometry`: draws the geometry into the depth buffer only
  (section 10.4).

## 10. Rendering

### 10.1 Render state in 3D

`CBRenderD3D::Setup3D` (called whenever a 3D object is about to update or
draw, `Setup2D` for the others; the renderer tracks which one is current).
The effect on a frame:

- Depth test **on** (`ZENABLE`, depth function `<=`), depth writes on;
  `Setup2D` turns the depth test off, so 2D drawing never touches depth.
  **The depth buffer is cleared once per frame** (`Fill`: colour, depth and
  stencil), not per actor.
- Fixed-function **per-vertex (Gouraud) lighting**, normals normalised;
  specular enabled, but lights have no specular component.
- Texture stage 0: colour = texture x lit diffuse (`MODULATE`), **alpha =
  texture alpha only** (`SELECTARG1`); the material's diffuse alpha and the
  actor's tint are not used. Alpha test on, reference `8` (`>= 8/255`); alpha
  blending on with `SRCALPHA/INVSRCALPHA` (so translucent textures
  composite over what is behind). The actor's `BlendMode` property changes
  blending (additive, subtractive).
- Filtering: linear, **mip filter linear** (trilinear), repeat addressing.
  Untextured materials render with the lit colour **[ext]**.
- Linear **vertex fog** when the scene has fog (`Scene.EnableFog(color,
  start, end)`), in world units of view depth.
- Ambient: the scene's, or the actor's override; default black.
- Culling: none unless `DrawBackfaces` is false (then back faces,
  counter-clockwise, are culled).
- Projection: section 4.1, and the view is the active camera's.
- Vertex format of a model: position, normal, one UV (`D3DFVF_XYZ | NORMAL |
  TEX1`, as loaded; skinning rewrites positions and normals in place); the
  geometry overlay uses position, normal, two colours and a UV.
- `D3DRS_COLORVERTEX` is off: the colour comes from the material.

### 10.2 Draw order with 2D (the compositing)

Per frame `CAdScene::TraverseNodes(false)` (`AdScene.cpp`) walks the layers
in order. For each layer it sets the scroll offset (parallax), then walks the
nodes in order: an entity is drawn (2D); a region node draws the **free
objects standing in it** sorted by `y` (`DisplayRegionContent`), 2D objects
with `Setup2D`, 3D objects with `Setup3D(activeCamera)`. After the nodes of
the **main** layer, the remaining free objects are drawn the same way. So:

- A 3D actor is placed exactly like a 2D actor: by its projected `Y` and the
  region its projected `(X, Y)` falls in (or `StickRegion`). The stable order
  among equal `Y` is that of a `qsort`, i.e. unspecified.
- Without geometry, 3D entity nodes of the layers are skipped; free 3D
  objects are not guarded in the original (undefined). Luma should draw no
  3D object in a scene without geometry.
- The debug overlay (`Scene.ShowGeometry`) is drawn after all layers.

### 10.3 Hit testing

Each drawn, registrable 3D actor adds a **precise** clickable rectangle (the
screen-space bounds of the 8 corners of the model's current bounding box,
plus its attachments' own) to the frame's rectangle list, in draw order;
lookups go from the last to the first (`CBRenderer::GetObjectAt`). A point
inside the rectangle is confirmed by casting a ray through the pixel into the
model and testing **every triangle** (`CXModel::IsTransparentAt`, `CXMesh::
PickPoly`, single-sided as above, so only front faces are clickable): the
ray is built as in 4.2, with the layer offset added, then moved to model
space by the inverse world matrix. The rectangle of an actor is that of the
frame it was drawn in. The caption and the cursor then follow the 2D rules.

### 10.4 Shadows

`TShadowType`: 0 none, 1 simple, 2 flat, 3 stencil. The type used for an
actor is `min(actor.ShadowType, Game.MaxShadowType, Scene.MaxShadowType)`.
Defaults: actor 3, game 3, scene 2, so a scene that does not say otherwise
gets **flat** shadows. Flat falls back to simple when the hardware lacks
projected-texture support (never in Luma). Shadows are drawn **before** the
model, inside the actor's `Display`. `ShadowColor` defaults to
`0x80000000` (50% black). The light position is `actor.position +
LIGHT_POSITION * Scale`.

- **Simple.** A horizontal quad `(-1..1)` in X/Z, scaled by `SHADOW_SIZE *
  Scale`, rotated by the actor's angle, placed at the actor's position, with
  the actor's `SHADOW_IMAGE` or the game's (`Game.SetShadowImage`, default
  engine resource `shadow.png`, a soft dark blob that Luma must supply),
  alpha-blended without depth writes (`C3DShadowGen::RenderSimple`). It is
  drawn with lighting still on **[ext]**; a plain unlit draw is the safe
  reading.
- **Flat.** Two steps (`C3DShadowGen::Render`). (1) Render the model (the
  `SHADOW_MODEL` if any) from the light's point of view into a 512x512 (or
  the device maximum) offscreen texture cleared to transparent white, with a
  solid colour (the shadow colour), `PerspectiveFov(45 degrees, aspect 1,
  near 1, far 10000)` nudged by half a texel, `LookAtLH(lightPos, actorPos,
  up)`. (2) Draw a horizontal floor quad of **500 x 500 units** (`+-250`)
  centred on the actor at its feet height, textured by that image through a
  projective texture matrix (`InvCamView * LightView * LightProj * texBias`),
  clamped, modulated by the shadow colour, no depth writes. The shadow is
  therefore a silhouette on the plane `y = actor.y`, **not** on the walk
  planes, cut off at 250 units.
- **Stencil** (`C3DShadowVolume`). Per actor each frame:
  1. For each mesh of the model (`SHADOW_MODEL` if any) and each attachment,
     take the skinned triangles and classify each face as facing the light
     or not (the sign of the dot product of the face normal with the
     direction to the light, taken into model space). For each edge of a
     light-facing face whose neighbour (by the position-welded adjacency)
     does not face the light, or that has no neighbour, emit a quad extruded
     away from the light by `|light - target| * 1.5`. The volume is **open**
     (no caps), the depth-pass variant.
  2. Draw the volume into the stencil buffer with the depth test on and no
     depth or colour writes: one pass with `CULL_CCW` **increments** the
     stencil, one with `CULL_CW` **decrements** it, both on depth pass (the
     source comments name the passes the other way round; the render-state
     calls are what counts).
  3. Draw a screen-size quad of the shadow colour where `stencil >= 1`,
     alpha-blended, depth test off. Clear the stencil.
  The depth buffer for step 2 is whatever is there: the shadow-receiving
  geometry (below), earlier 3D actors. Shadows therefore fall on walls and
  furniture proxies and bend over them.
- **Receivers.** When the effective type is stencil, before drawing the
  nodes of the **main layer** the scene draws all active walk planes, blocks
  and generics flagged `RECEIVE_SHADOWS` into the depth buffer with colour
  writes off (`RenderShadowGeometry`). Side effect: those invisible meshes
  also **occlude actors** (an actor behind a receiver mesh is hidden), which
  authors can use to hide an actor behind an invisible proxy of a painted
  object; it only works in stencil mode.
- Stencil and flat/simple must not be mixed in one scene (the manual says
  artifacts appear).

### 10.5 Textures, sprites and videos as textures

Materials keep the `.x` texture, or one replaced by script:
`SetTexture(material, file)` accepts an image **or a `.sprite`** (animated
texture: current frame's image, frame delays honoured, the sprite's other
properties ignored); `SetTheoraTexture(material, file)` plays a Theora video
as a looping texture. Both are looked up by material name (section 6.3).

## 11. The scripting surface

All names are exact. "Blocking" methods wait for the object (`Ready`); the
`...Async` twins return at once. Unless said, a method returns `true` on
success and `false` on failure.

### 11.1 Methods of a 3D actor (`CAdActor3DX::ScCallMethod`)

| Method | Meaning |
|--------|---------|
| `PlayAnim(name)`, `PlayAnimAsync(name)` | Play an animation on channel 0, restarting it, and set the actor's state to "playing animation". The async twin sets the state too (a commented-out `!Async` in the code suggests it once did not); only the blocking form waits for the end. |
| `PlayAnimChannel(channel, name)`, `PlayAnimChannelAsync(channel, name)` | Same on a channel 0..9; only the blocking form sets the state. |
| `StopAnim([transition ms])` | Stop channel 0 (default stop transition time). |
| `StopAnimChannel(channel, [transition ms])` | Stop a channel (transition defaults to 0). |
| `IsAnimPlaying([name])` | Channel 0 plays (that animation). |
| `IsAnimChannelPlaying(channel, [name])` | Same for a channel. |
| `SetAnimTransitionTime(from, to, ms)` | Per-pair blend time; negative removes the pair. |
| `GetAnimTransitionTime(from, to)` | The pair's time or `-1`. |
| `MergeAnims(file)` | Merge animation sets from another `.x`, then its `.anim` options if present. |
| `UnloadAnim(name)` | Remove an animation set (and stop it on channels). |
| `GoTo(x, y)`, `GoToAsync` | Walk to a scene point (ignored within `GoToTolerance`). |
| `GoTo3D(x, y, z)`, `GoTo3DAsync` | Walk to a raw 3D point. |
| `GoToObject(entity)`, `GoToObjectAsync` | Walk to an entity's walk-to point and turn to its direction. Errors if not an entity. |
| `TurnTo(direction or object)`, `TurnToAsync` | Turn to one of 8 directions or toward an object. |
| `TurnToAngle(degrees)`, `TurnToAngleAsync` | Turn to an angle. |
| `IsWalking()` | State is following a path. |
| `DirectWalk([velocity, [anim]])`, `DirectWalkBack(...)`, `DirectWalkStop()` | Direct control (8.4); `null` parameters mean the defaults. |
| `DirectTurnLeft([velocity, [anim]])`, `DirectTurnRight(...)`, `DirectTurnStop()` | Same for turning. |
| `AddAttachment(file, name, bone)` (alias `AddMesh`, logs obsolete) | Attach a model to a bone. |
| `RemoveAttachment(name)` (alias `RemoveMesh`) | Remove it. |
| `GetAttachment(name)` | The attachment object or `null`. |
| `SetTexture(material, file)` | Replace a material's texture (image or sprite). |
| `SetTheoraTexture(material, video)` | Replace it by a video. |
| `SetEffect(material, fxFile)`, `RemoveEffect(material)` | HLSL effect on a material (not planned, section 13). |
| `SetEffectParam(material, name, value)`, `SetEffectParamVector(material, name, x, y, z, w)`, `SetEffectParamColor(material, name, color)` | Effect parameters (not planned). |
| `CreateParticleEmitterBone(bone, ox, oy, oz)` | Particle emitter following a bone; returns the emitter. Needs the particle system. |
| inherited from `CAdObject3D`: `SkipTo(x, y)` | Place at the scene point (exact conversion; fails silently off the walk planes). |
| `SkipTo3D(x, y, z)` | Place at a 3D point. |
| `GetBonePosition2D(bone)` | Object `{X, Y}` in scene pixels, or no members on failure. |
| `GetBonePosition3D(bone)` | Object `{X, Y, Z}` in world units. |
| `AddIgnoredLight(name)`, `RemoveIgnoredLight(name)` | Lights that do not affect this actor. |
| inherited from `CBObject`: `SetShadowImage(file)`, `GetShadowImage()`, `SetLightPosition(x, y, z)` | Simple-shadow image; shadow light position relative to the actor. |
| inherited from `CAdObject`/`CBObject` | `Talk`, sounds, cursors, inventory, `SetCaption`, ... as for 2D actors (already in `luma-adventure`). |

### 11.2 Properties of a 3D actor

| Property | Type | Meaning |
|----------|------|---------|
| `Type` (RO) | string | `"actor3dx"` (the manual says `actor3d`). |
| `X`, `Y` | int | Scene position; setting converts to 3D (exact). |
| `PosX`, `PosY`, `PosZ` | float | 3D position. |
| `Angle` | float | Heading (`CAdObject3D`). |
| `DirectionAngle` (`DirAngle`) | float | Heading, normalised on set. |
| `Direction` | int | The 8-sector direction; setting snaps the angle. |
| `Velocity`, `AngularVelocity` (`AngVelocity`) | float | Speeds. |
| `Scale` | float | Percent (stored `/ 100`). |
| `DropToFloor` | bool | |
| `DrawBackfaces` | bool | |
| `ShadowType` | int | 0..3, clamped on set. |
| `ShadowColor` | int | ARGB. |
| `Shadow`, `SimpleShadow` | bool | Obsolete aliases of `ShadowType`. |
| `AmbientLightColor` | int or `null` | Per-actor ambient override; `null` clears it. |
| `TalkAnimName`, `IdleAnimName`, `WalkAnimName`, `TurnLeftAnimName`, `TurnRightAnimName` | string | Standard animation names; `null` restores the default. |
| `TalkAnimChannel` | int | Channel of talk animations (default 0). |
| `AnimTransitionTime`, `AnimStopTransitionTime` | int ms | Defaults 200, 200. |
| `GoToTolerance` | int px | Default 2. |
| plus the inherited object properties (`Ready`, `Caption`, `Active`, `Registrable`, `Height`, `AlphaColor`, `BlendMode`, `Scale...` of 2D objects where meaningful). |

### 11.3 The attachment object

Returned by `GetAttachment`: `PlayAnim`/`PlayAnimAsync(name)`, `StopAnim()`,
`StopAnimChannel(channel)`, `PlayAnimChannel`/`Async(channel, name)` (forced
restart, channel 0 by default; they pass transition time 0, but the model's
owner is the actor, so when the channel already plays something the actor's
transition table decides, as in section 7), the properties
of `CAdObject3D` (`PosX/Y/Z`, `Angle`, `Scale`, `Active`...), `Type` =
`"attachment"`.

### 11.4 Scene and game methods for 3D

| Where | Member | Meaning |
|-------|--------|---------|
| `Scene` | `LoadActor3D(file)` | Load a 3D actor that goes away with the scene. |
| `Game` | `LoadActor3D(file)` | Load a 3D actor that stays. |
| `Scene` | `EnableNode3D(name)`, `DisableNode3D(name)`, `IsNode3DEnabled(name)` | Walk plane/block/generic by mesh name. |
| `Scene` | `EnableLight(name)`, `DisableLight(name)`, `IsLightEnabled(name)` | Lights by name. |
| `Scene` | `SetLightColor(name, color)`, `GetLightColor(name)`, `GetLightPosition(name)` (object `{X,Y,Z}`), `GetLightName(index)`, property `NumLights` | |
| `Scene` | `SetActiveCamera(name)` | Switch camera (`FOV_OVERRIDE`, clipping planes reapplied). |
| `Scene` | `EnableFog(color, start, end)`, `DisableFog()` | Linear fog for 3D actors. |
| `Scene` | properties `ShowGeometry`, `GeometryFile` (RO), `WaypointsHeight`, `MaxShadowType`, `AmbientLightColor` | |
| `Game` | `SetShadowImage(file)`, `GetShadowImage()`, `IsShadowTypeSupported(type)`, properties `MaxShadowType` (0..3), obsolete `Shadows`, `SimpleShadows`, `SupportsRealTimeShadows` | |

Animation events arrive as script events named by `EVENT { NAME }` on the
actor (`ApplyEvent`).

## 12. What Luma needs

### 12.1 Where the code goes

`CLAUDE.md` asks for a small core and optional modules. 3D actors are a
capability only some games need, so they should be a **module**, not code in
the core crates:

| Piece | Home | Why |
|-------|------|-----|
| Math with the D3D conventions (matrices, quaternions, project/unproject, the D3DX functions listed in 12.2) | `luma-3d` | New crate. |
| `.X` reader (text, optional binary), `.3ds` reader, `.geometry`, `.act3d`, `.anim` parsers: pure data | `luma-3d` (a `formats` module) or `luma-formats` | They are file formats, like sprites. `luma-formats` is the natural home by the existing rule, but it should not grow 3D-only code that every game compiles; recommended: the module crate, re-exported by `luma` under the `3d` feature. To decide (open question 1). |
| Model, skeleton, animation channels, CPU skinning, geometry queries, path finder, camera projection, picking | `luma-3d` | No GPU, no game logic, unit-testable. |
| The 3D pass on wgpu (pipelines, lights, shadows) | `luma-3d::render`, depending on `luma-render` for `Gpu`/`Target` | The 2D `luma-render` stays small. |
| A small generic extension point in `luma-render` | `luma-render` | See 12.3. |
| `Actor3D` object, its state machine, 2D/3D scene glue, scripts | `luma-adventure`, behind a Cargo feature `3d` | Genre behaviour. |
| Feature `3d` in `luma` (off by default) that enables `luma-3d` and `luma-adventure/3d` | `luma` | `docs/modules.md`. |

New third-party dependencies to pick (licences to state in the commit):
a math crate, **`glam`** (MIT OR Apache-2.0) with `default-features =
false`, would give `Mat4`, `Quat`, `perspective_lh`, `look_at_lh`; the row
versus column vector difference is handled once, in `luma-3d::math`, by
transposing at the boundary. Decompression for MSZIP-compressed `.X`:
`flate2` is already in the workspace (raw deflate with a preset dictionary).
No crate is needed for `.X` or `.3ds`.

### 12.2 Library functions of D3DX that must be re-implemented exactly

`D3DXMatrixLookAtLH`, `PerspectiveFovLH`, `RotationYawPitchRoll` (only the
yaw is used), `RotationZ`, `Scaling`, `Translation`, `Inverse`,
`QuaternionRotationMatrix`, `QuaternionSlerp`, `RotationQuaternion`,
`Vec3Lerp`, `Vec3Project`/`Vec3Transform`/`TransformCoord`/`TransformNormal`,
`PlaneFromPoints`, `PlaneIntersectLine`. Plus the three helpers that WME
wrote itself (`IntersectTriangle`, `PickGetIntersect`,
`DecomposeMatrixSimple`).

### 12.3 `luma-render`

Today it draws a `DrawList` of textured quads in one pass, with no depth
attachment. A 3D pass needs:

1. **A depth (and stencil) attachment**, `Depth24PlusStencil8` or
   `Depth32FloatStencil8`, created with the target, cleared once per frame.
   2D quads draw with depth test and writes off.
2. **A way to interleave 3D with the quads**, because a 3D actor is in the
   middle of the 2D order. Options:
   (a) the `DrawList` gets a second kind of item, an opaque **custom pass**
   (`Box<dyn FnOnce(&mut CustomPassContext)>` or an index into a registry
   the module owns), and the renderer splits the list into segments: a
   render pass for each run of quads, and a callback for each custom item,
   all with `LoadOp::Load` on colour and depth. A frame has a few passes,
   which is fine for a 2D game (but each `Load`/`Store` costs bandwidth on
   tiled GPUs).
   (b) one render pass, the module records its draws into the same pass
   through a callback receiving the `RenderPass`. Fewer passes, but the flat
   shadow's render-to-texture must then be done *before* the pass.
   Recommended: (a), with the module free to render its offscreen shadow
   textures at the beginning of the callback of the first item (they are
   independent of the visible frame).
3. **The viewport of the 3D pass** equals the letterboxed rectangle and the
   scene viewport (scissor) used by the quads, with the projection of 4.1
   computed from the logical resolution.
4. Offscreen tests as for 2D (`Offscreen`, read back).

Everything else (pipelines, vertex layouts, uniforms, shadow geometry) can
live in `luma-3d`.

### 12.4 The 3D pass in `luma-3d`

- **Mesh pipeline.** Vertex: position, normal, UV (CPU-skinned each frame
  into a dynamic buffer); index buffer per mesh; one draw per material
  subset. Uniforms: world, view-projection (the 4.1 matrix), the light list
  (up to 8: kind, position, direction, range, spot angles, colour/256),
  ambient, material (diffuse, emissive), fog, alpha reference, tint, blend
  mode. Texture: repeat sampler, **trilinear with generated mipmaps** (the
  2D textures have none), RGBA8 with the magenta key already applied by
  the image loader.
- **Lighting shader.** Per-vertex, to match the originals: `colour =
  emissive + globalAmbient*ambientMat + sum(light terms)` with the
  light model of 3.4 (no distance attenuation for point lights, spot cone
  and range as above), `clamp`, then `texture * colour`, alpha from the
  texture, `discard` below 8/255, then blend. Gouraud interpolation of the
  vertex colour (do the lighting in the vertex shader). A per-pixel mode is
  an option later.
- **Depth.** `LessEqual`, writes on, with the stencil state for shadows.
- **Fog:** linear on view depth.
- **Stencil shadows:** shadow volumes built on the CPU every frame from
  the skinned triangles (the algorithm of 10.4) into a dynamic buffer; two
  draws with `IncrementClamp`/`DecrementClamp` on depth pass (cull-mode
  switch), then a full-screen quad with stencil test `>= 1` and stencil pass
  op `Zero`, which clears the stencil without a clear call. The depth prepass
  of receiver meshes is a depth-only pipeline.
- **Flat shadows:** a pre-pass renders the actor into a 512x512 texture
  (own depth buffer) before the main pass; the floor quad samples it with a
  projective matrix.
- **Picking** needs the skinned triangles on the CPU: same buffers, kept.

### 12.5 `luma-adventure`

- `Actor3D` beside `Actor` in the world (`actors3d`), `Kind::Actor3D` in
  `objects.rs`; load from `.act3d`; share the generic object behaviour
  (name, caption, scripts, sounds, speech, inventory, events).
- The state machine of 8.2, 8.3, 8.4 over `luma-3d`'s skeleton and geometry;
  `GoTo` with `2D_PATHFINDING` reuses the existing 2D `path` module.
- `SceneView` loads the scene's geometry (`GEOMETRY`, `CAMERA`, ...), owns its
  runtime state (node and light flags, active camera), exposes
  `Convert2Dto3D`/`Convert3Dto2D` with the current `Camera`, and drives the
  path finder.
- Drawing: `draw_order` already takes `others` with positions; a 3D actor is
  an `Other` whose draw callback appends the custom item (12.3) of the
  actor, shadow included; hit areas come from the projected rectangle and the
  polygon test.
- Scripts: the tables of section 11 in `natives_actor.rs`-style modules
  (`natives_actor3d.rs`).
- Saving: section 8.7 into `save.rs`.
- The headless player and Lumen runner need a 3D switch only if they render.

### 12.6 Testing

- All tests are built in code (`CLAUDE.md`): a generator for tiny text `.X`
  files (a quad, a two-bone box with weights and an animation), a `.3ds`
  builder (a floor, a block, a camera, a light), act3d strings. Numbers to
  check: the projection (section 4.1) against hand-computed pixels, the
  inverse mapping round trip, `GetHeightAt`, `DirectPathExists` and a
  path around a block, the animation sampler at known times (including the
  ticks and the quaternion conjugate), skinning of the two-bone box,
  blending halfway, events.
- Rendering tests use the offscreen path and compare a few pixels (a lit
  triangle, a depth overlap, a shadow darkening).
- Real files are opt-in under `testdata/local/` and skip when absent. The
  WME repository carries a demo (`install/source/projects/wme_demo_3d`) that
  is the only 3D sample known to us. It is loose files (no `.dcp`); its
  Trinity model comes from a fan mod with unclear redistribution terms
  (`original files/TRINITYrage.txt`), so it must not enter the repository;
  the teapot (`teapot.x`, a standard test model) is the better candidate if
  a real file is wanted in CI. The demo is the visual oracle: the geometry
  overlay of `Front` must sit on the painted floor and crates, and the walk
  plane picks must match what the manual shows.

## 13. Not planned (and why)

| Item | WME | Decision |
|------|-----|----------|
| HLSL effect files (`.fx`) on materials, `SetEffect*`, `EFFECT` blocks | `3DEffect*.cpp`, `3DEffectScript.cpp`, 1400 lines on top of the Direct3D 9 effect framework | Parse and ignore `EFFECT` blocks, make `SetEffect*` return `false` (the original does when the compile fails). A shader transpiler is out of scope until a game needs it. |
| Legacy Milkshape actors (`ACTOR3D`, `.ms3d`, `HideMesh`, `ShowMesh`) | `AdActor3D.cpp`, `3DModel.cpp`, `3DLoaderMS3D.cpp`, `3DSkelAnim.cpp`, `3DJoint.cpp` (about 3500 lines), deprecated by WME itself since 1.6 and slated for removal in 2.0 | Detect the keyword and report a clear error. Revisit if a game turns up. |
| Software (non-Direct3D) mode | | Not applicable. |
| Mirror code (`CAdMirror`), the frustum class, render targets | commented out or unused in `AdScene.cpp` | Ignore. |
| `SetTheoraTexture`, `CreateParticleEmitterBone`, `GetBonePosition2D` for particles | | Later, after the video and particle work exists; the model side must still keep bone matrices accessible. |

## 14. Finding about the existing documentation

`docs/formats/sprite.md` says `2D_ONLY` / `3D_ONLY` sub-frames are "shown
only when the owner is a 2D, respectively 3D, actor", and
`luma_formats::sprite` says the same in its field docs. In WME 1.x
(`BFrame.cpp`, `Draw`) the test is on the **renderer mode**:
`(subframe.2DOnly && Game->m_UseD3D) || (subframe.3DOnly && !Game->m_UseD3D)`
skips the sub-frame. `2D_ONLY` therefore means "only in the software 2D
renderer", `3D_ONLY` "only in the Direct3D renderer". Luma always draws with
the GPU: **`2D_ONLY` sub-frames should never be drawn and `3D_ONLY` ones
always**. The fields are parsed and currently ignored at runtime, so
nothing misbehaves yet, but the documentation and the field comments should
be corrected when this is touched (it is independent of the 3D actor work).

## 15. Proposed tasks

Sizes: **S** is one short session (about a day or less, a few hundred
lines), **M** two to four sessions, **L** more than that. Every task is a
set of small commits that build and pass the checks, updates its document in
the same commit, and has tests built in code. "WME files" are the ones to
read first.

### Phase A: data and maths (no GPU)

| # | Task | WME files to read | Size | After |
|---|------|-------------------|------|-------|
| 1 | **Crate and conventions.** `luma-3d` with a feature in `luma`, `README.md` and `docs/modules.md` rows, the `math` module (D3D conventions, the functions of 12.2), `docs/3d.md` skeleton. | `3DUtils.cpp`, `3DCamera.cpp`, `BObject.cpp` (`GetMatrix`), `dcgf.h` (`DegToRad`) | S | |
| 2 | **`.X` text container.** Tokenizer and template-aware reader to a generic object tree (`xof 0303txt`, 32- and 64-bit floats, named templates, nested data, data references, comments). `docs/formats/xfile.md`. | `XFile.cpp`, `XModel.cpp` (`LoadFromFile`), `XFrameNode.cpp` (`LoadFromXData`); samples `trinity.x`, `teapot.x` | M | 1 |
| 3 | **`.X` to model.** Frames, `FrameTransformMatrix`, meshes (triangulation, normal and UV vertex splitting, materials, `TextureFilename`), `SkinWeights` and the bind matrices, position-welded adjacency, `AnimTicksPerSecond`, animation sets with the four key types, `AnimationOptions`, merging, material naming. | `XMesh.cpp`, `SkinMeshHelper.cpp`, `XAnimation.cpp` (`Load`, `LoadAnimationKeyData`), `XAnimationSet.cpp`, `3DMaterial.cpp` | L | 2 |
| 4 | **`.X` binary and compressed.** `bin `, `tzip`/`bzip` (MSZIP) with `flate2`. Only if a real game needs it; check first which variant the games use. | (public DirectX documentation **[ext]**) | M | 2 |
| 5 | **`.3ds` geometry and `.geometry`.** The chunk walker of 3.2 into planes, blocks, generics, waypoint points, cameras, lights; the node patterns; scene keys of 3.1 in `luma_formats::scene`. `docs/formats/geometry.md`. | `3DLoader3DS.cpp`, `3DS_chunks.h`, `AdSceneGeometry.cpp` (`LoadFile`), `AdGeomExt*.cpp`, `AdWaypointGroup3D.cpp`, `3DMesh.cpp`, `3DLight.cpp` | M | 1 |
| 6 | **`.act3d` and `.anim`.** Parser to a typed definition, TEMPLATE, defaults, `ANIMATION`/`EVENT`, obsolete keys. `docs/formats/actor3d.md`. | `AdActor3DX.cpp` (`LoadBuffer`, `ParseEffect`, `MergeAnimations2`), `XModel.cpp` (`ParseAnim`, `ParseEvent`), `AdActor3D.cpp` (`IsActorX`) | S | |
| 7 | **Camera and the 2D/3D mapping.** View and projection of 4.1 for a scene (`Camera`, viewport, scroll, layer size, `SCROLL_3D_COMPATIBILITY`, `FOV_OVERRIDE`, clip planes), `convert_2d_to_3d` (exact and tolerant), `convert_3d_to_2d`, the object's 2D position. | `BRenderD3D.cpp` (`SetProjection`, `GetProjectionParams`), `AdSceneGeometry.cpp` (`Convert*`, `StoreDrawingParams`), `AdObject3D.cpp`, `AdGame.cpp` (`GetLayerSize`), `AdScene.cpp` (`ScrollTo`) | M | 1, 5 |
| 8 | **Geometry queries and the 3D path finder.** `GetHeightAt`, `DirectPathExists`, `GetBlockIntersection`, `DropWaypoints`, Dijkstra with the alternate-target rerun, node enabling. Decide per quirk (9.2, 9.3) whether to reproduce. | `AdSceneGeometry.cpp`, `3DUtils.cpp` (`IntersectTriangle`, `PickGetIntersect`), `AdPath3D.cpp`, `AdPathPoint3D.cpp` | M | 5 |
| 9 | **Skeleton animation and skinning.** Frame hierarchy and matrices, channels, active animations, blending slots, events, transition-time table, looping and finish rules, CPU skinning, bounding box, ray/triangle picking of a posed model, bone matrices and positions. | `XFrameNode.cpp`, `XModel.cpp`, `XAnimationChannel.cpp`, `XActiveAnimation.cpp`, `XAnimation.cpp` (`Update`), `XAnimationSet.cpp`, `XMesh.cpp` (`Update`, `PickPoly`) | L | 3 |

### Phase B: drawing

| # | Task | WME files | Size | After |
|---|------|-----------|------|-------|
| 10 | **`luma-render` extension.** Depth/stencil attachment, `DrawList` custom items with the segment splitting of 12.3, tests that a custom pass sees the right depth and viewport. | `BRenderD3D.cpp` (`Fill`, `Setup2D`/`Setup3D`) | M | |
| 11 | **Mesh pass.** Pipelines, dynamic skinned buffers, materials and textures with mipmaps and the colour key, lights (selection, model of 3.4), ambient, fog, alpha test and blend mode, culling option; the geometry debug overlay (`Scene.ShowGeometry`) as the validation tool for task 7. | `BRenderD3D.cpp` (`Setup3D`), `XMesh.cpp` (`Render`), `3DMaterial.cpp`, `3DLight.cpp`, `AdSceneGeometry.cpp` (`Render`, `EnableLights`, `CreateLights`) | L | 7, 9, 10 |
| 12a | **Simple shadows.** | `3DShadowGen.cpp` (`RenderSimple`) | S | 11 |
| 12b | **Stencil shadows.** CPU shadow volumes, stencil passes, receiver depth prepass, attachments, `SHADOW_MODEL`. | `3DShadowVolume.cpp`, `AdActor3DX.cpp` (`DisplayShadowVolume`), `AdSceneGeometry.cpp` (`RenderShadowGeometry`), `XMesh.cpp` (`UpdateShadowVol`), `AdScene.cpp` (`TraverseNodes`) | M | 11, 9 |
| 12c | **Flat shadows.** Light-view pre-pass to texture, projective floor quad. | `3DShadowGen.cpp` (`Render`, `SetAdjustedProjectionMatrix`, `SetTextureMatrixStage0Stage1`) | M | 11 |

### Phase C: the actor in the game

| # | Task | WME files | Size | After |
|---|------|-----------|------|-------|
| 13 | **Scene integration.** Load geometry with the scene, active camera and light/node state, the 2D position of 3D objects, the draw order hook with `Other` items, geometry scenes in the headless player. | `AdScene.cpp` (`LoadBuffer`, `TraverseNodes`, `DisplayRegionContent`, `UpdateFreeObjects`, `ScrollTo`) | M | 5, 7, 11 |
| 14 | **`Actor3D` core.** Load, idle, `PlayAnim` family, drop to floor, `SkipTo`, `X`/`Y`, direction, scale; `Game.LoadActor3D`/`Scene.LoadActor3D`/`Unload*`. | `AdActor3DX.cpp`, `AdObject3D.cpp`, `AdActor3D.cpp` (`CreateFromFile`) | M | 6, 9, 13 |
| 15 | **Walking, turning, direct control, talking.** 8.2 to 8.4 over tasks 8 and the existing 2D path module for `2D_PATHFINDING`; blocked region and waypoints of the actor. | `AdActor3DX.cpp` (`Update`, `GoTo*`, `FollowPath*`, `GetNextStep*`, `PrepareTurn`, `TurnToStep`) | L | 8, 14 |
| 16 | **Hit testing and captions.** Projected rectangle and polygon pick, registration order, `GetHeight`. | `XModel.cpp` (`IsTransparentAt`, `UpdateBoundingRect`), `XMesh.cpp` (`PickPoly`), `BRenderer.cpp` (`GetObjectAt`) | S | 9, 14 |
| 17 | **Attachments and bones.** `AddAttachment`, `GetAttachment`, the attachment object, `GetBonePosition2D/3D`. | `AdAttach3DX.cpp`, `AdObject3D.cpp` (`GetBonePosition*`) | S | 14 |
| 18 | **Scripting surface.** The remaining methods and properties of section 11, scene lights/camera/fog/nodes, game shadow properties. | `AdActor3DX.cpp`, `AdObject3D.cpp`, `AdScene.cpp`, `AdGame.cpp`, `BGame.cpp`, `BObject.cpp` | M | 14, 15, 17 |
| 19 | **Saved games.** 8.7 into `save.rs`, geometry state, animation channels. | `Persist` functions of the files above | M | 18 |
| 20 | **Material scripting.** `SetTexture` with images and sprites; later `SetTheoraTexture`; ignore `EFFECT`. | `XModel.cpp` (`SetMaterial*`), `3DMaterial.cpp` | S | 11, 18 |

### Order

1. Phase A first, in the order 1, 5, 7 (the projection is the riskiest piece
   to get exactly right and is testable without a GPU), then 2, 3, 6, 9, 8.
2. 10 and 11 as soon as 3, 5 and 7 exist: the first visible result is the
   **demo scene with its geometry overlay drawn over the painted background**
   and a static, lit teapot standing where `Convert2Dto3D` says. This
   validates the camera before any game logic exists.
3. 13 to 16: idle actor, walking actor, clickable actor.
4. 12b, then 12a, then 12c (the demo uses stencil; the default for scenes
   without `MAX_SHADOW_TYPE` is flat; simple is trivial).
5. 17, 18, 19, 20.
6. 4 (binary `.X`) as soon as a real game shows it is needed; it may move
   up to the start of phase A if the first real file is binary.

### Size of the whole effort

**Large.** Roughly 8000 to 10000 lines of Rust plus tests and three format
documents, roughly half the size of `luma-adventure` today; about thirty
short sessions. The risky parts, in order, are the exact
camera mapping, the `.X` loader (because the original delegates it to
DirectX), the render-pass integration in `luma-render`, and getting shadows
to look like the originals. The logic of walking and animation is long but
well specified by the sources. The software renderer, effects and Milkshape
models are not part of it.

## 16. Open questions

1. **Crate topology.** A new `luma-3d` crate (recommended) or the formats
   in `luma-formats` and the pass in `luma-render`? The rule "core stays
   small" suggests the first, the maintainer decides.
2. **Which games to validate against.** The local test games (The White
   Chamber, OpenQuest) are 2D as far as we know. The WME demo is the only 3D data at hand;
   is a real commercial 3D game available for local tests? Are the sample
   models (teapot) acceptable in CI?
3. **Binary `.X`.** The samples are text; real games probably ship both.
   Which does the target game use?
4. **Quirk policy.** Where the original is arguably wrong (single-sided
   picking, walk planes ignoring `active` in queries, the alternate-target
   path, the first-key pose when the time is past the last key, `PlayAnimAsync`
   setting the state, `ANGLE` not normalised) reproduce as written (safe for
   existing games, recommended for anything that affects walkability and
   timing) or fix (visual-only issues such as mipmaps, MSAA)?
5. **Lighting model.** Per-vertex Gouraud with `colour/256` and no
   attenuation (faithful) or per-pixel? Black default ambient makes unlit
   actors black; verify on the demo (it has four lights).
6. **Untextured materials and the simple shadow's lighting.** What the
   fixed-function pipeline did exactly is not visible in the sources.
7. **Flat shadow pixel details** (alpha of the silhouette, the 0.5-texel
   nudge): an approximation by a planar projection of the vertices onto
   `y = actor.y` is much simpler (no pre-pass) and probably close; is the
   difference acceptable?
8. **Path-finder time budget.** 15 ms per frame in the original; Luma can
   run it to completion at once. Behaviour differs only in timing when
   several actors path-find in one frame.
9. **Resolution mismatch.** The camera is defined against the runtime
   resolution and the main layer (4.1). A game running at another resolution
   than its scenes were laid out for would shift; check the sample scenes
   (800x600 editor resolution).
10. **Animation time base.** Animations use the game timer clamped to 1000 ms
    per frame; the existing sprite animation uses the same timer, to be
    reused.
11. **Save-game scope.** What of the geometry (active camera, light colours,
    node flags) must survive a load, and whether it also survives a scene
    change (the original saves it with the scene; the role of
    `PERSISTENT_STATE` was not checked here).
12. **Texture formats.** 3D textures in real games are `.bmp`, `.jpg`, `.tga`,
    `.png`, all loaded by `luma_formats::Image`; power-of-two limits do not
    apply on modern GPUs.
13. **`ShowGeometry` overlay** is a debugging aid; is it wanted in the
    player's UI or only in tests?

## 17. Index of source locations

| Topic | File and function |
|-------|-------------------|
| Actor file keys | `wme_ad/AdActor3DX.cpp`, `LoadBuffer` |
| Actor update and display | `AdActor3DX.cpp`, `Update`, `Display`, `DisplayShadowVolume` |
| Walking | `AdActor3DX.cpp`, `GoTo3D`, `FollowPath3D`, `GetNextStep3D`, `InitLine3D`, `GoTo2D`, `FollowPath2D`, `GetNextStep2D` |
| Turning | `AdActor3DX.cpp`, `PrepareTurn`, `TurnToStep`, `DirToAngle`, `AngleToDir` |
| Scripts | `AdActor3DX.cpp` and `AdObject3D.cpp`, `ScCallMethod`/`ScGetProperty`/`ScSetProperty`; `BObject.cpp` (`SetLightPosition`, `SetShadowImage`) |
| 2D position, drop to floor | `AdObject3D.cpp`, `Update`, `Convert3DTo2D` |
| Attachments | `AdAttach3DX.cpp` |
| Scene geometry | `AdSceneGeometry.cpp` (all) |
| Geometry sidecar | `AdGeomExt.cpp`, `AdGeomExtNode.cpp` |
| 3DS chunks | `3DLoader3DS.cpp`, `3DS_chunks.h` |
| Cameras, lights | `3DCamera.cpp`, `3DLight.cpp` |
| Projection | `BRenderD3D.cpp`, `SetProjection`, `GetProjectionParams`; `AdGame.cpp`, `GetLayerSize` |
| Compositing | `AdScene.cpp`, `TraverseNodes`, `DisplayRegionContent`, `UpdateFreeObjects`, `Display3DContent` (unused mirror code) |
| Shadows | `3DShadowGen.cpp`, `3DShadowVolume.cpp`, `AdGame.cpp` (`GetMaxShadowType`), `BGame.cpp` (`SetMaxShadowType`) |
| Model loading | `XModel.cpp`, `XFrameNode.cpp`, `XMesh.cpp`, `XFile.cpp` |
| Animation | `XAnimation.cpp`, `XAnimationSet.cpp`, `XAnimationChannel.cpp`, `XActiveAnimation.cpp` |
| Picking | `BRenderer.cpp` (`GetObjectAt`), `XModel.cpp` (`IsTransparentAt`) |
| Manual | `doc/html/wme/3D_intro.html`, `3D_principles.html`, `3D_actors_x.html`, `3D_scripting.html`, `3D_shadows.html`, `3D_additional.html` |
