# 3D actors on 2D backgrounds

Status: **study, implemented.** This note collects what the
Wintermute Engine 1.x sources do for 3D characters standing on painted 2D
backgrounds ("2.5D"), what Luma Engine does to do the same, and the split
into tasks. The parts marked **as implemented** are the specification of
the code in `crates/luma-3d` (the module `luma::three_d`, feature `3d`):
the conventions (section 2), the scene keys (3.1), the geometry file and
its classification (3.2, 3.3, specified in
[formats/geometry.md](formats/geometry.md)), the camera and the 2D/3D
mapping (section 4), the actor definition file (section 5, specified in
[formats/actor3d.md](formats/actor3d.md)), the `.X` model as data
(section 6, specified in [formats/xfile.md](formats/xfile.md)), the
animation and skinning at run time (section 7), the geometry at
runtime (section 9: floor height, segment tests, path finder), the
mesh pass of the renderer (sections 10.1 and 12.3, specified in
[rendering.md](rendering.md#3d-pass)) and, in `crates/luma-adventure`
behind its `3d` feature (which `luma`'s `3d` feature turns on), the actor
logic (section 8), the attachments (8.6), the hit testing (10.3), the
textures set by script (10.5), the scripting surface (11), the saved
games (8.7) and the drawing of the actors with their shadows (10.2,
10.4, 12.4): tasks 10 to 20. What remains is listed in 12.4 (the
`SHADOW_MODEL`, the geometry overlay) and in section 16.

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

**As implemented** in `luma_3d::math`: `Vec3`, `Mat4` (row-major, row
vectors), `Quat`, `Plane`, the D3DX functions listed in 12.2 and the three
helpers of `3DUtils.cpp` (`intersect_triangle`, `pick_get_intersect`,
`decompose_simple`), all in `f32` like D3DX, with no math crate (the types
are a few hundred lines and the row-vector convention is then explicit
everywhere). `Mat4::to_cols_array` gives a matrix to a column-vector GPU
shader: that matrix is the transpose, so its column-major floats are
simply the rows in order.

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

**As implemented** in `luma_formats::scene::Scene` (fields `geometry`,
`camera`, `fov_override`, `near_clipping_plane`, `far_clipping_plane`,
`waypoint_height`, `pathfinding_2d`, `max_shadow_type`,
`scroll_3d_compatibility`, `ambient_light_color`; the optional floats keep
the value as written, negative included, since "negative" and "absent"
mean the same to the consumers) and specified in
[formats/scene.md](formats/scene.md#3d-keys).

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
| `EDITOR_SHOW_GEOMETRY`, `EDITOR_RESOLUTION_WIDTH/HEIGHT` | | | Editor only. The runtime uses the game's resolution, not these (the renderer reads `CBGame::m_EditorResolutionWidth`, which only the editor sets; the scene's own copy of the key is never handed to the game). |

Fog is set by script (`Scene.EnableFog`), not by a key.

### 3.2 The `.3ds` geometry file

**As implemented** in `luma_3d::three_ds` (`File::parse`, with a `Builder`
for fixtures) and specified in
[formats/geometry.md](formats/geometry.md), which is now the reference for
the chunk subset; the table below is kept as the summary. The reader
follows the original on every well-formed file and differs only on
malformed input (bounds-checked reads give an error with the chunk id and
offset; a mesh chunk before any named object is skipped).

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

**As implemented** in `luma_3d::node_patterns` (`NodePatterns::parse`,
`load`, `classify`, the wildcard matcher `matches_pattern`) and
`luma_3d::scene_geometry::SceneGeometry::load_3ds`, specified in
[formats/geometry.md](formats/geometry.md). Two details found while
implementing: the original's sidecar parser **fails on any key it does
not know** and the geometry then falls back to the three built-in nodes
alone (reproduced by `NodePatterns::load`); and a light without a colour
chunk has an undefined colour in the original, white in Luma.

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
  `Scene.EnableNode3D` / `DisableNode3D` by mesh name (case-insensitive;
  every node of that name changes, and `IsNode3DEnabled` reads the first
  match among blocks, then planes, then generics: `SceneGeometry::
  set_node_enabled`, `is_node_enabled`), and saved with the scene.

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

**As implemented** in `luma_3d::camera` (`Projection::new`, `project`,
`project_f32`, `pick_ray`, with `CameraSettings` for the scene's
`FOV_OVERRIDE` and clipping planes and `ViewSetup` for the 2D side) and
`luma_3d::scene_geometry::SceneGeometry` (`convert_2d_to_3d`,
`convert_2d_to_3d_tolerant`). This section is the specification; the
tests of `camera.rs` check the hand-computed values below.

This is the heart of "2.5D": a point of the painted background and a point
of the 3D world are tied by one perspective camera whose image plane is the
**main layer** of the scene.

### 4.1 Projection

Parameters (`BRenderD3D.cpp`, `SetProjection`, `GetProjectionParams`;
`AdGame.cpp`, `GetLayerSize`), all in logical pixels:

- `R = (RW, RH)`: the game's resolution (the runtime ignores the editor
  resolution keys, section 3.1).
- `L = (LW, LH)`: the size of the scene's main layer (`R` instead when
  `SCROLL_3D_COMPATIBILITY`; `ViewSetup::for_scene` applies the rule).
- `V = (VX, VY, VW, VH)`: the viewport the scene is drawn in (the whole
  screen unless the scene's `VIEWPORT` or the game's scene viewport is
  set), with margins `mleft = VX`, `mright = RW - VW - VX`, `mtop = VY`,
  `mbottom = RH - VH - VY`.
- `Scroll = (ScrollX, ScrollY)`: the scene's scroll offset, what a scene
  coordinate loses to become a screen coordinate of the main layer.
  `Off = Scroll - (VX, VY)` is the original's `Game->m_OffsetX/Y` while the
  main layer is drawn.
- `ModW = max(0, (LW - RW) / 2)`, `ModH = max(0, (LH - RH) / 2)`; if
  `LH < RH`: `ModH -= (RH - LH) / 2` and `LH = RH` ("a layer shorter than the
  screen is not scrolled, the camera sees the whole screen"; there is no
  equivalent for the width).
- `S = max(LW / RW, LH / RH) * (RH / VH)`.
- `fov`: the scene's `FOV_OVERRIDE` when it is `>= 0`, else the camera's
  (`1900 / lens` degrees); `near`, `far`: the scene's clipping planes when
  `>= 0`, else **90 and 10000**.

The view matrix is the camera's (section 3.2). The projection matrix is
`PerspectiveFovLH(fov, VW/VH, near, far)` with

```
_11 *= S            _22 *= S
_31  = -(OffX + (mleft - mright)/2 - ModW) / VW * 2
_32  =  (OffY + (mtop - mbottom)/2 - ModH) / VH * 2
```

(`(mleft - mright)/2` is an integer division). These two matrices are what
the GPU draws with; they put a 3D point on the screen exactly where a 2D
object at the same scene position would be drawn, scroll and viewport
included. In the original the window's letterbox offset also enters and
cancels out; Luma's logical coordinates have none.

A world point in camera space `(x, y, z)` lands at **scene (layer) pixel**

```
px = RW/2 + ModW + (VH/2) * S * x / (z * tan(fov/2))
py = RH/2 + ModH - (VH/2) * S * y / (z * tan(fov/2))
```

that is, with `S * VH = max(LW/RW, LH/RH) * RH`, a position that depends on
the resolution and the layer only: **not on the scroll** (the matrix shift
cancels it) and **not on the viewport** (the aspect ratio and the viewport
height cancel). With a layer at least as tall as the screen this is
`px = LW/2 + ...`, `py = LH/2 - ...`: the optical axis passes through the
**centre of the layer**, and the vertical field of view spans the layer's
height when the layer has the screen's aspect ratio (otherwise the larger
ratio decides, `S`). Hand-computed check used by the tests: camera at
`(0, 0, -1000)` looking at the origin, `fov = 90°`, `R = L = 800x600`: the
origin lands at `(400, 300)` and `(100, 0, 0)` at `(430, 300)`; with
`L = 1600x600` they land at `(800, 300)` and `(860, 300)`, scrolled or not.

### 4.2 The inverse and the object's 2D position

- **2D to 3D** (`AdSceneGeometry.cpp`, `Convert2Dto3D`;
  `Projection::pick_ray` then `SceneGeometry::convert_2d_to_3d`): take scene
  pixel `(X, Y)`; subtract `(mleft + mright)/2 + ModW` from `X` and `(mtop +
  mbottom)/2 + ModH` from `Y` (integer division of the margins, the result
  truncated back to an `int` pixel); the ray in camera space is
  `((2X/VW - 1) / P11, -(2Y/VH - 1) / P22, 1)`; rotate it by the inverse
  view matrix (3x3 part), origin at the camera position; the answer is the
  **nearest hit on any walk plane**, enabled or not (by distance from the
  camera to the hit point, so a hit behind the camera counts). It fails
  when the ray misses every walk plane. Hits use the single-sided
  Moeller-Trumbore test of `C3DUtils::IntersectTriangle` (section 9.2), so
  **walk plane triangles must face up toward the ray** (the 3DS loader's
  winding flip makes a plane that faces up in the 3D package face up here).
- **Tolerant** (`Convert2Dto3DTolerant`, `convert_2d_to_3d_tolerant`): if
  the exact conversion fails, scan in steps of 10 pixels (up to 1000) to the
  left, right, up and down for the first pixel that converts. For each axis
  keep the nearer of its two directions (the positive one on a tie); shift
  the point along the axis with the smaller shift (the vertical one when
  equal) and convert that pixel. No hit in any direction fails. Used for
  every walking target.
- **3D to 2D** (`Convert3Dto2D`, `CAdObject3D::Convert3DTo2D`;
  `Projection::project`): project with the same matrices into the viewport
  (`D3DXVec3Project`), add `Scroll - (VX, VY)`, truncate to `int` (toward
  zero; a value such as 199.99998 becomes 199, in Luma as in the original).
  An object's `X`/`Y` are the projection of its **origin** (the feet)
  computed in `CAdObject3D::Update` whenever its 3D position changed; these
  `X`/`Y` drive the drawing order, the region it stands in, auto-scroll and
  speech position.
- Bone positions in 2D (`GetBonePosition2D`, particles) go through the
  same function.
- The scene integration (task 13) owns the `ViewSetup` of the frame: the
  resolution, the main layer, the scene viewport and the scroll of the
  main layer, rebuilt whenever one of them changes, as the original
  rebuilds its projection on each `Setup3D`.

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

**As implemented** in `luma_3d::actor3d` (`Actor3dDef::parse`,
`parse_with` for templates, `is_actor_x`, `AnimationDef::parse_file` for
the sidecar and `AnimationDef::apply` onto a model) and specified in
[formats/actor3d.md](formats/actor3d.md), which is the reference; the
table below is the summary. Found while implementing: an **unknown key
fails the load** (the parser stops on a token it does not know),
reproduced; a `SHADOW_TYPE` word that is neither a name nor a number gives
`none` (`atoi` returns 0); `SHADOW_MODEL`, `ANIMATION` and `EFFECT` before
any `MODEL` are dropped, not errors; the `.anim` sidecar is only looked
for by `MergeAnims`, not by the `MODEL` keys.

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

**As implemented** in `luma_3d::xfile` (the text container, read into a
generic object tree without interpreting the template declarations) and
`luma_3d::model` (`Model::parse`, `merge_animations`, `find_frame`,
`animation_set`, the sampler of 6.4 and 7), specified in
[formats/xfile.md](formats/xfile.md), which is the reference for the
subset; the tables below are kept as the summary with the corrections
found while implementing. Binary and MSZIP-compressed files are read into
the same tree (task 4, `luma_3d::xfile_binary`; `formats/xfile.md` 1.4
and 1.5).

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
| `Animation` (outside a set) | Becomes an anim set of its own, named after the file (two loose animations give two sets with the same name). |
| `AnimTicksPerSecond` `{9E415A43-7BA6-4a73-8743-B73D47E88476}` | One `DWORD`; default **4800**. All animation key times are in these ticks. |
| anything else | Ignored. |

Templates are matched by **name** in Luma (D3DX matches the GUIDs of the
registered templates). A frame answers for its **last** object only, an
accident of the original's child loop that Luma reproduces: an object
that fails before the last one is left out alone, and a frame whose last
object fails or that holds **no object at all** (`Frame x { }`) is
dropped with what is under it and fails in turn, up to the top level,
where a failure rejects the file ([formats/xfile.md](formats/xfile.md),
section 2). The meshes are kept in the order the original walks them: a
frame's own meshes before its child frames'.

Merging a second file (`MergeFromFile`) takes only `AnimationSet` and
`Animation` objects, searching the frames of the file recursively (the
frames themselves are not added); the bones of the new animations are looked
up by name in the existing skeleton (`FindBones`). An unnamed animation set
takes the **file name without path and extension** as its name. A merge
that fails part-way leaves the sets it had read in the model, bound to no
frame ([formats/xfile.md](formats/xfile.md), 2.5).

A model can be loaded **against a parent model** (`LoadFromFile(file,
ParentModel)`, the `SHADOW_MODEL`): its mesh bones and animation targets
are then looked up in the parent's frames, and its skinned meshes are
posed with the parent's combined matrices every update
(`Model::parse_with_parent`, `luma_3d::animation::ShadowModel`;
[formats/xfile.md](formats/xfile.md), 2.6).

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

As implemented (`luma_3d::model::Mesh`, [formats/xfile.md](formats/xfile.md)
2.2): polygons are fan-triangulated; a vertex used with two different
normal indices is duplicated, the file's vertices keeping their indices
and the duplicates appended with a copy of their original's UV and skin
weights (`Mesh::original_vertices` records the origin); a mesh without
`MeshNormals` gets smooth normals computed from its faces (a Luma choice,
the original's vertex format would have none); `VertexDuplicationIndices`
is read and not used, since the sample exporters write the skin weights
against the file's own vertex indices (Trinity's table is the identity);
a material list shorter than the faces repeats its last entry. The
position-welded adjacency is left to the runtime (task 9 / 12b): the
vertex list keeps the positions and origins it needs.

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
  (D3DX attribute table). **As implemented** (`luma_3d::animation`,
  section 7): every vertex a bone weights starts from zero and
  accumulates `weight * (position * palette)` and `weight * (normal *
  palette's 3x3)`, the normals then normalised (what `UpdateSkinnedMesh`
  does with a null inverse-transpose array, **[ext]**; the fixed-function
  pipeline renormalises anyway, 10.1). A bone whose frame the model lacks
  uses the identity for its combined matrix (the original dereferences an
  unset pointer there). A vertex no bone weights keeps its loaded position
  (what D3DX does with it is not known **[ext]**; the sample files weight
  every vertex).
- **Rigid meshes** (no bones) are transformed every frame by their parent
  frame's combined matrix: **positions only**; the normals stay as loaded, so
  the lighting of rigid parts on moving frames is slightly off. Luma does
  the same (`skin_mesh`): a game's models were lit that way.
- **Bounding box.** Recomputed from the skinned vertices every frame
  (`D3DXComputeBoundingBox`, with a manual fallback when NaNs appear). The
  **model's** box (`UpdateBoundingRect`) starts at the origin and grows
  around the meshes' boxes, so it always contains the model's origin; Luma
  does the same (`AnimatedModel::bounding_box`), since the 2D bounding
  rectangle and the hit test of task 16 are built on it.
- **Adjacency.** Face-to-face adjacency for shadow volumes is regenerated on
  the clone with `GenerateAdjacency(0, ...)`: vertices at identical
  positions count as one point, so UV and normal seams do not open edges. A
  Luma loader must weld by position. Nothing in the animation or skinning
  needs it (the skinned vertices keep the loaded indices), so it is left
  to the stencil shadows (task 12b), computed from
  `SkinnedMesh::vertices` positions when the volumes are built.

### 6.4 Animation sets

`AnimationSet` holds `Animation` objects. Each `Animation` has:

- one **frame reference** (a data reference to a `Frame`; only its
  name is kept and later looked up with `FindFrame`). The check that
  rejects a duplicated reference is dead code (it tests the frame pointer,
  which is only set by `FindBones` after loading), so with two references
  the **last wins**; Luma does the same,
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
As implemented (`AnimationSet::total_time_ms`, `frame_time_ms`,
`total_frames`, `add_event`): the divisions are done in single precision
and truncated as in the original, so `4800 / 4.8f` is **999 ms** for a
one-second set at 4800 ticks; the gap scan counts the gap from time 0 to
the first key, and a zero gap met after a positive one resets the minimum
(the next gap replaces it). An event's frame is clamped to `1..=total_frames`
(`total / frame_time + 1`, 0 when the frame time is 0).

## 7. Animation at runtime

**As implemented** in `luma_3d::animation` (`AnimatedModel`, the
`CXModel` of the original at run time, over the data of
`luma_3d::model`), from `XModel.cpp`, `XAnimationChannel.cpp`,
`XActiveAnimation.cpp`, `XAnimation.cpp`, `XAnimationSet.cpp` and
`XFrameNode.cpp`. Times are milliseconds of a timer the caller advances
(`update(delta_ms)`, the game timer in the original); the timer starts at
0 and wraps like the original's `DWORD`.

- **10 channels** (`X_NUM_ANIMATION_CHANNELS`, `animation::NUM_CHANNELS`),
  numbered 0..9. Channel 0 carries the standard animations the actor logic
  selects (idle, walk, turn, talk-stance). Others are for scripts
  (`PlayAnimChannel`). A channel holds up to two *active animations*: the
  current one in slot 0 and, during a transition, the incoming one in
  slot 1.
- **Each update** (`CXModel::Update`, `AnimatedModel::update`): every frame
  starts from its loaded local matrix; the channels update in order 0..9,
  each writing the **sampled transform** (scale, rotation, position; not
  a matrix) of every frame its animation has tracks for into one of the
  frame's two **transformation slots** (`CXFrameNode::SetTransformation`),
  later channels overriding earlier ones on the same frame and slot; then
  the frames build their local matrix from the slots (below), the combined
  matrices are computed from the root, the meshes are skinned (6.3) and
  the boxes recomputed. So a "wave" animation that only has tracks for the
  arm bones, played in channel 1, replaces the arm of the walk cycle.
- **Frame slots** (`CXFrameNode::UpdateMatrices`): a frame whose slot 0 is
  unused keeps its loaded matrix, **whatever slot 1 holds**; with slot 0
  used alone its transform is applied as is; with both used the two
  transforms are blended by **slot 0's** factor (`Lerp` for scale and
  position, `Slerp` for rotation, `Transform::blend`), then `S * R * T`.
  Both slots are cleared after use. Consequences the study missed: during
  a transition a frame only the incoming animation animates stays at rest
  until the transition ends, a frame only the outgoing one animates holds
  the outgoing pose unblended, and a frame animated both by an earlier,
  non-transitioning channel and by a later channel's incoming animation
  only keeps the earlier channel's pose (its slot 0 carries a factor of
  0).
- **Sampling** one track at local time `t` (in ticks): find the first key
  with `time > t`; interpolate it with the previous one
  (`(t - t1) / (t2 - t1)`, `0` when equal times); `t` before the first key
  gives the first key. If *no* key is later than `t` the indices stay at
  **0 and 0**: the pose snaps to the **first** key (not the last). Callers
  keep `t` below the last key time (a finished animation uses
  `total - 1` ms) so this is only reached by degenerate data.
  `Animation::sample` (one frame's `Transform`), `AnimationSet::sample`
  (the transforms of the resolved frames), `Model::rest_pose`,
  `Model::combine` and `Model::sample` (rest pose, one set applied,
  combined matrices) in `luma_3d::model`, [formats/xfile.md](formats/xfile.md)
  2.4; a missing track leaves position `0`, scale `1`, rotation identity,
  so a rotation-only track also moves the frame to its parent's origin, as
  in the original. The millisecond local time becomes ticks by `ms *
  (ticks_per_second / 1000)` truncated (`Model::ms_to_ticks`).
- **Active animation** (`CXActiveAnimation`, `animation::ActiveAnimation`).
  Started with the timer; local time = elapsed ms. If `elapsed > total`:
  looping wraps with `%` (0 when the total is 0), a non-looping one
  becomes *finished* and holds `total - 1` (which wraps to `u32::MAX` for
  an empty set, as the original's `DWORD` does; the pose then snaps to the
  first keys). `Looping` is read from the set when the animation starts
  (`AnimationOptions` or the actor file's `LOOPING`, applied to the set by
  `AnimationDef::apply`).
- **Events.** Frame index `= localTime / frameTime + 1` (0 when the frame
  time is 0; 1-based); when it changes `OnFrameChanged(current, previous)`
  fires every event with `previous < FRAME <= current`, and after a wrap
  (`previous > current`) those with `FRAME > previous` first
  (`AnimationSet::events_between`). The index starts at -1, so the first
  update of a freshly started animation fires the events of frame 1.
  Event frames are clamped to `[1, totalFrames]` when added
  (`AnimationSet::add_event`). No event fires while a pose is **frozen**
  (transitions, below), but the frame index is still tracked, so the
  events of the frame an incoming animation shows during a transition (its
  frame 1) **never fire** once it starts. `AnimatedModel::update` returns
  the event names in firing order; an event runs the owner's event handler
  by name (`ApplyEvent("footstep")`, i.e. `on "footstep" { ... }` in the
  actor's script, task 14).
- **Transitions** (`PlayAnim(set, transitionTime, stopTransitionTime)`,
  `AnimatedModel::play_with`). With transition time 0, or when the channel
  is empty, the new animation replaces the old at once. Otherwise both
  poses are **frozen** for the duration: the old animation at the local
  time it had reached (slot 0) and the new one at its first pose (slot 1,
  because its local time is still 0). While `timer - start < time`, the
  frames blend the two with `lerp = elapsed / time` (through the frame
  slots above). When `elapsed >= time` the new animation becomes the only
  one, its start time is reset to the current timer (the transition time
  is not counted) and it is updated at once from its beginning. Asking for
  the animation the channel already **heads for** (the incoming one during
  a transition, else the current one; names compared ignoring case) does
  nothing unless the request forces a restart; the scripts' `PlayAnim`
  forces it, the actor's own idle/walk/turn requests do not
  (`force_restart`).
- **Finishing and stopping** (`StopAnim(transitionTime)`,
  `AnimatedModel::stop`). When a non-looping animation finishes, the
  channel stops it with the *stop transition time* given when it was
  played; `StopAnim` with a time does the same on demand: the current
  animation is made non-looping, the incoming one (if any) is dropped, and
  the current pose is **held, frozen, for that time** (so no event); then
  the channel empties and the frames fall back to their loaded matrices.
  There is no blend back to the rest pose: the hold only delays the snap.
  With time 0, or an empty channel, the channel empties at once. On the
  update where an animation finishes, the frame still shows its last pose
  (`total - 1`); the stop takes effect from the next update. As in the
  original, a 0 stop time therefore shows the rest pose one update after
  the end.
- **The transition time actually used** when switching from animation `A`
  to `B` on a channel is `Owner.GetAnimTransitionTime(A, B)`: the pair set
  by `SetAnimTransitionTime` (names compared ignoring case; a negative time
  removes the pair), else the actor's `AnimTransitionTime` (default
  **200 ms**, `animation::DEFAULT_TRANSITION_MS`). The
  `AnimStopTransitionTime` (default 200 ms,
  `DEFAULT_STOP_TRANSITION_MS`) is the stop transition time the actor
  passes with every `PlayAnim` and `StopAnim`. In the original the pair
  table lives in the actor and overrides whatever time the caller passed
  whenever the channel has a current animation (so an attachment's
  `PlayAnim`, which passes 0, still uses its owner actor's table and
  default). Luma keeps the table and the two defaults on the
  `AnimatedModel` (`set_transition_time`, `transition_time`,
  `set_default_transition_ms`, `set_default_stop_transition_ms`);
  `play(channel, name, force_restart)` resolves them as the actor does,
  `play_with(channel, name, transition_ms, stop_transition_ms,
  force_restart)` uses the times given.
- `IsAnimPending(channel, name)` (`AnimatedModel::is_playing`) is true
  while the channel has an **unfinished** animation in either slot and,
  when a name is given, the animation it heads for has that name
  (case-insensitive). A pose held after `StopAnim` still counts as playing
  (the animation did not finish); a pose held after a natural end does
  not. The scripts' `IsAnimPlaying` looks at **channel 0 only**;
  `IsAnimPending(name)` over all channels is `is_playing_on_any_channel`.
- **Unloading a set** (`UnloadAnim`, `AnimatedModel::remove_animation_set`)
  drops it from every channel slot that plays it; a transition in progress
  keeps running and ends on whatever is left. When several sets share the
  name (two loose animations of one file, or the same file merged twice),
  `CXModel::UnloadAnimation` removes the first and then **steps over the
  two sets that follow each removed one** (its loop advances the index
  once more after `RemoveAt`), so of three sets of that name in a row only
  the first goes; the call can be repeated. Reproduced
  (`Model::animation_sets_to_unload`), since which sets remain decides
  what `PlayAnim` plays next.
- **Outputs of an update**: the local and combined matrix of every frame
  (`local_matrices`, `combined_matrices`, `bone_matrix(name)` for
  attachments and `GetBonePosition*`), the skinned meshes
  (`meshes()`: one `SkinnedMesh` per model mesh with a contiguous
  `Vec<Vertex>` indexed like the model's, ready for a vertex buffer, and
  its box), the model's box (`bounding_box`, containing the origin as in
  6.3), and the posed-triangle pick (`pick(origin, direction)`,
  `CXMesh::PickPoly` with the single-sided `IntersectTriangle` of 9.2; the
  ray is in model space, the caller inverts the world matrix, task 16).
  A new `AnimatedModel` is posed at rest once, so these are valid before
  the first update.
- **Saving** (`CXModel::Persist`, `CXAnimationChannel::Persist`,
  `CXActiveAnimation::Persist`): `AnimatedModel::state` gives an
  `AnimationState` (the timer, per channel the two active animations
  **by set name** with start time, looping, finished, last local time
  and current frame, the transition in progress, the two default times
  and the pair table) and `AnimatedModel::restore` puts it back on a
  model loaded again from its files, then poses it with an update of 0
  ms, which fires no event. An active animation whose set the reloaded
  model lacks is dropped (the original keeps a pointer into its own
  list, which the reload keeps valid); a transition left without an
  incoming animation ends. The update in which a non-looping animation
  finishes still shows its last pose while the channel is already
  stopped, so a state taken then restores to the pose of the next
  update.

## 8. Actor logic

**As implemented** in `luma_adventure::actor3d` (`Actor3d`, the
`CAdActor3DX` with its `CAdObject3D` and `CAdObject` parts;
`Actor3d::update` is `CAdActor3DX::Update`) and `luma_adventure::scene3d`
(`Scene3d`, the `CAdSceneGeometry` the scene owns, with the path finder;
`World::projection_3d` builds the frame's `Projection` from the game's
resolution, the main layer, the scene's own `VIEWPORT` or the game's scene
viewport, the scroll offset and the scene's camera keys, as 4.1 says).
`World::update_actors3d` runs once per frame after the 2D actors: the
path finder's open request first (`InitLoop`), then every active 3D
actor. A 3D actor is loaded by `Game.LoadActor3D`, `Scene.LoadActor3D`,
and also by `LoadActor`: the file's first keyword decides
(`CAdActor3D::IsActorX`), so a game may use either method. A scene with
`GEOMETRY` loads it with the scene (`Scene3d::load`): a file that is
not a `.3ds` or does not parse is logged and the scene runs in 2D, as
the original drops its geometry. Without the `3d` feature a scene's
geometry is logged and ignored, and a 3D actor file fails to load as a
2D actor.

Differences from the original, chosen while implementing:

- `X` and `Y` of a freshly loaded actor read 0 until its first update
  projects its position, as in the original; a script that needs the
  position at once uses `SkipTo`.
- `GoToObject` on an entity whose `WALK_TO_X/Y` are `0,0` walks to the
  entity's position (what the `2D_PATHFINDING` branch of the original
  does); the original's 3D branch converts the `0,0` itself.
- `TurnTo(object)` whose 2D position converts to no walk plane does not
  turn (the original turns toward an uninitialised vector).
- Direct control that only turns requests the turn animation (or idle)
  and does not run the walking step (8.4); the original walks in place.
- The path finder's open request is not saved (8.7): an actor saved
  while searching or waiting for its path asks again after the load.

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
Luma takes the first reading (`Actor3d::direct_step`): turning alone
requests the turn animation (or idle) and skips the walking step; with a
walk mode on, the walk wins as in the original.

### 8.5 Drop to floor, position, scale

`CAdObject3D::Update`: if `DropToFloor`, `y = GetHeightAt(position, 5)`.
`Scale` is a uniform factor; `Velocity` and the shadow follow it. The
scene's scale levels do **not** apply to the model (`CBObject::GetMatrix`
takes `m_Scale3D` alone); they only scale the actor's private
`BLOCKED_REGION` polygon at its projected `Y`. Setting the
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

**As implemented** (`Attachment3d` in `luma_adventure::actor3d`, its
script members in `natives_actor3d.rs`): an attachment is a script
object of its own (`Kind::Attachment3D(owner)`) that the actor owns and
that goes with it. `AddAttachment(file, name, bone)` needs a model, a
bone that is a frame of it (exact name) and an `.x` file that parses;
otherwise it raises the original's runtime errors (`Bone '...' cannot be
found`, `Error adding attachment`) and returns `false`. The same name
(ignoring ASCII case) replaces the attachment in place, and the replaced
one's script object dies as the original deletes it. `RemoveAttachment`
removes by name; `GetAttachment` finds by name or, a Luma extension, by
index, and `NumAttachments` counts them (in the original the inherited
property counts the 2D attachments of `CAdObject`, always 0 on a 3D
actor). Each update, after the actor's model, every active attachment
computes its full matrix `own GetMatrix * bone combined matrix * actor
world matrix` (the own matrix from its `PosX/Y/Z`, `Angle`, `Scale`),
animates its model on its own channels (the events crossed are raised on
the actor, whose scripts own the attachment's model in the original) and
projects its box. An attachment whose bone is unknown still animates but
is not drawn, as `DisplayAttachments` skips it. The attachment's
`PlayAnim` family passes transition time 0 and forces a restart; when the
channel already plays something the owner actor's pair table and default
transition time decide, as the model's owner does in the original
(`Attachment3d::play_anim`). The attachment is always `Ready`, so the
blocking `PlayAnim` returns at the next tick, as in the original. The
active attachments' projected rectangles are hit areas of the actor, and
the pick through them confirms it (10.3). The drawing step gets them in
`Actor3dDraw::attachments` (`AttachmentDraw`: the posed model, the full
matrix, the backface flag) and draws them after the actor's model, lit as
it is, and extrudes their triangles into the actor's stencil shadow
volume (10.4). `SetTexture` is the actor's only; the original has no
material members on `CAdAttach3DX`.

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

**As implemented** (layout version 14): `Actor3d` serializes with
`serde` like the 2D actor: the definition values scripts may change
(name, caption, flags, speeds, scale, shadow settings, light position,
ambient override, ignored lights), the position and its last value, the
angle, target angle and points, the turning flag, the after-walk angle,
the 3D path with its cursor and the 2D path, the five animation names,
the talk channel, the state channel, the direct-control modes and
parameters, the tolerance, the sentence, and the model's
`AnimationState` (section 7) in place of the model, plus the `MODEL`
files, the files `MergeAnims` added and the sets `UnloadAnim` removed;
the attachments (script object, name, bone, model file, offset, flags
and their model's `AnimationState`), the textures set by script on the
materials (10.5, by material name) and the bone and offset of the
particle emitter. After a load the actor file and its models are read
again, the merges and removals replayed, the attachments' models read
again and every animation state restored (`Actor3d::reload`). `Scene3d` saves the geometry file, the waypoint
height, the active camera, every light's active flag and colour and
every node's active flag by name, and the fog; the geometry is read
again and the state put back (`Scene3d::reload`). The path finder's
request is not saved. The game's `MaxShadowType` and shadow image are
saved with the world.

## 9. Geometry at runtime

All in `AdSceneGeometry.cpp` and `3DUtils.cpp`. Everything is brute force
over all triangles of all planes and blocks (geometry is meant to be a few
hundred triangles).

### 9.1 Floor height

**As implemented** by `SceneGeometry::height_at(pos, tolerance)`, which
returns `None` instead of the unchanged `y` (callers that want the
original's behaviour write `unwrap_or(pos.y)`, as `drop_waypoints` does).

`GetHeightAt(pos, tolerance)`: the start is `pos` raised by `tolerance`. A
walk plane triangle is a candidate when the vertical line through the start
meets it from above (single-sided, the triangle must face up) and the height
`h` of the plane there is not above `start.y + tolerance` (the line is not a
ray: planes above the start count within that margin). The candidate nearest
in height to the **raised** start wins: between a floor 20 below the point
and one 30 above it, a tolerance of 40 picks the upper one. With no
candidate the input `y` is returned unchanged. The `active` flag of walk
planes is **not** consulted here, nor in picking and path tests; only the
debug rendering and the shadow-receiver pass skip inactive planes.

### 9.2 Triangle tests

**As implemented** by `math::intersect_triangle`, `math::pick_get_intersect`,
`SceneGeometry::direct_path_exists` and `SceneGeometry::block_intersection`.

- `IntersectTriangle(origin, direction, v0, v1, v2)` is Moeller-Trumbore
  **without** the `t >= 0` check and **single-sided**: `det < 0.0001` fails,
  i.e. the triangle is hit only when the ray direction opposes the triangle's
  front normal `(v1 - v0) x (v2 - v0)`. On success the three result floats
  are re-purposed to hold the intersection **point** (plane/line
  intersection), not barycentric coordinates.
- `PickGetIntersect(a, b, v0, v1, v2)`: does the segment `a..b` cross the
  **plane** of the triangle (`0 <= t <= 1`, not parallel)? It does not test
  containment. The closed range means a segment that only **touches** the
  plane at one end crosses it.
- `DirectPathExists(p1, p2)` (`direct_path_exists`): false if, for any walk
  plane triangle (active or not) or **active** block triangle, the segment
  crosses the plane **and** `IntersectTriangle(p1, p1 - p2)` or
  `IntersectTriangle(p2, p2 - p1)` hits the triangle: that is, if the
  segment pierces the triangle in either direction (the two single-sided
  tests together make the test two-sided, so the winding of a block does
  not matter). A segment running inside a plane never crosses it, so the
  paths between points lifted over one floor only meet blocks; a segment
  that ends **on** a walk plane coming from above does pierce it (the
  touch counts) and is blocked, which is why path points are lifted; a
  zero-length segment is always free. Staying inside a walk plane's
  outline is **not** checked: it is the author's waypoints that keep paths
  on the floor.
- `GetBlockIntersection(p1, p2)` (`block_intersection`): the same test
  over the **active blocks only**, returning the piercing point of the
  first pierced triangle in file order (blocks in order, faces in order),
  not the one nearest to `p1`: for a segment through a box both side
  faces are pierced and the one listed first wins, from either end. With
  no hit the original returns the origin, which its only caller then
  treats as "no hit" (9.3); Luma returns `None`.

### 9.3 Path finder

**As implemented** by `path3d::PathFinder` (the `m_PF*` state of
`CAdSceneGeometry`: `request` is `GetPath`, `step` is `PathFinderStep`,
`run` is `InitLoop`) and `path3d::Path3D` (`CAdPath3D`).

`GetPath(source, target, path)` opens a request, `PathFinderStep` advances
it, and the scene's `InitLoop` runs steps once per game loop. Only one
request is open at a time (`GetPath` returns false while busy, and the
actor asks again next frame).

**Timing.** `InitLoop` means to spend at most `PFMaxTime = 15` ms per loop,
but it measures with `Game->m_CurrentTime`, which `CBGame::InitLoop` sets
once per frame and nothing refreshes during the loop: the elapsed time is
always zero and the loop only ends when the request is done. A request is
therefore always computed in full during the game loop after the one that
accepted it; `PathFinder::run` does the same, and open question 8 is
closed by this.

1. Both ends are lifted: `y = GetHeightAt(p, waypointHeight) +
   waypointHeight` (a point over no walk plane keeps its own `y`, plus the
   height).
2. Nodes, in this order: source (distance 0), target (infinite), every
   point of every **active** waypoint group (infinite), in file order.
3. A plain Dijkstra with exact position comparisons: take the unmarked node
   with the lowest finite distance (the first one on ties) and mark it. If
   its position **equals** the target's (a waypoint sitting exactly on the
   target counts), build the path by following the origin links and **drop
   the first point** (the start); the path is ready. Otherwise, for every
   unmarked node whose straight segment from the current node passes
   `DirectPathExists`, relax it when strictly shorter (edge length =
   Euclidean distance; ties keep the earlier origin).
4. **Alternate target.** On the first pass, every edge from the current
   node to a node at the target's position that did *not* relax it
   (blocked, or free but not shorter) is a candidate: the shortest
   candidate so far sets the alternate target to `GetBlockIntersection`
   of that edge, moved `30` units back toward the current node
   (`ALTERNATE_TARGET_BACKOFF`). When the edge pierces no active block,
   the intersection is the origin and the alternate target becomes a
   point 30 units from the origin toward the current node. For a free but
   longer edge this is harmless, since the target is reached and the
   alternate never used; for an edge blocked by a **walk plane** (the
   segment to the target dips through the floor, as toward a target below
   the plane's extension) that bogus point becomes the route. Reproduced
   as written, sentinel included: an alternate target that is exactly the
   origin counts as none. The study's earlier worry about the free-edge
   case was unfounded; the walk-plane case is the real quirk.
5. **Fallback.** When no unmarked node with a finite distance is left and
   the target was never marked: on the first pass, if an alternate target
   is set, the request is rerun once (`Rerun = true`) from the same source
   toward it, both ends lifted again by step 1; the rerun computes no
   alternate and, when it fails too, ends ready with an **empty** path.
   With no alternate target the path is ready and empty at once. A target
   inside a block, for instance, yields a walk to a point 30 units short
   of the block along the shortest blocked approach, which may be the
   source itself (empty path) or a point behind the actor.
6. The path is a list of 3D points at waypoint height, the start excluded;
   a free line gives a single point. The actor walks them with 8.3.

**`CAdPath3D`** (`Path3D`): the points, a cursor starting at `-1` and a
`ready` flag. `GetFirst` (`first`) rewinds to the first point, or returns
null for an empty path and leaves the cursor; `GetNext` (`advance`) advances
and returns the point there, or null past the end (from the initial cursor
it returns the first point); `GetCurrent` (`current`) is the point under
the cursor or null; `Reset` clears everything; `SetReady` returns the
previous flag. A saved game stores the points, the cursor and the flag
(`Path3D::from_parts`).

**Ownership in Luma.** The original writes the result into the requester's
own `CAdPath3D`. `PathFinder::request` returns a `Ticket` instead, `run`
computes the open request, and the holder of the ticket collects the
result with `take_path` (an actor in `WAITING_PATH` tries every frame; a
later request supersedes the ticket). `PathFinder::find_path` does the
three at once. The finder is a value of its own that borrows the
`SceneGeometry`, owned by the scene beside its geometry; its state is part
of what the scene saves (8.7).

### 9.4 Other geometry services

- `Convert2Dto3D`, `Convert2Dto3DTolerant`, `Convert3Dto2D`: section 4.
- `EnableLights`: section 3.4 (the renderer's, task 11). `SetLightColor`,
  `EnableLight`, `IsLightEnabled`: **as implemented** by
  `SceneGeometry::set_light_color`, `set_light_enabled` (every light of
  that name, case-insensitive) and `is_light_enabled` (the first); the
  original re-registers all lights with the device after a change, Luma's
  renderer reads the flags when it draws.
- `EnableNode`/`IsNodeEnabled`: **as implemented** by
  `SceneGeometry::set_node_enabled` (every node of that name over blocks,
  planes and generics) and `is_node_enabled` (the first match, blocks
  first).
- `DropWaypoints`: **as implemented** by `SceneGeometry::drop_waypoints`,
  re-lifts every waypoint of every group (at load and when
  `WaypointsHeight` changes, `set_waypoint_height`). Waypoint groups
  (`CAdWaypointGroup3D`, `WaypointGroup`): the loader puts all waypoint
  meshes into one group; a group has an `active` flag that the path
  finder honours and that nothing in the original changes (no script
  method, not saved).
- `Render(showGeometry)`: the debug overlay (wireframes in the colours
  above and the waypoint marker sprite `wpt.sprite`); it also stores the
  matrices and scroll of the frame for the conversions.
- `RenderShadowGeometry`: draws the geometry into the depth buffer only
  (section 10.4).

## 10. Rendering

### 10.1 Render state in 3D

**As implemented** in `luma-render` (the mesh pipelines and `mesh.wgsl`;
see [rendering.md](rendering.md#3d-pass)); the actor's `BlendMode` is a
field of each mesh draw, the lights of a draw are chosen by the caller.

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

**As implemented** (`luma_adventure::draw3d`, `World::draw`,
`SceneView::draw_with_overlays`): the 3D actors are the `DrawItem::
Other` items of the order, sorted and placed exactly like the 2D free
objects (their `Drawable` has no sprite). `World::scene_draw_parts`
puts in the scene's `Overlays` the 3D items of each actor, by its index
among the free objects (`Overlays::three_d`: the shadow, then one mesh
draw per material of each mesh of the model, then the attachments'),
and the shadow receivers (`Overlays::before_main`); `draw_with_overlays`
pushes the receivers when it enters the main layer, before its first
node, and each actor's items in its place, before the actor's particle
overlay, so that the painter's order of the frame holds and the depth
buffer only orders 3D things among themselves (section 10.1). `World::
three_d_draws` gives the data; `World::upload`, called with the renderer
before the frame is drawn, keeps the GPU side (`Render3d`: the meshes
updated with the posed vertices, the textures, the receivers). Without
geometry the scene has no `Scene3d`, `three_d_draws` is `None` and no
3D object is drawn. The debug overlay is not drawn (12.4).

### 10.3 Hit testing

**As implemented**: `Actor3d::update` projects the eight corners of the
model's box through the world matrix into scene pixels
(`bounding_rect`, `CXModel::UpdateBoundingRect`); `World::actor3d_hits`
registers it as a `HitArea` of the actor in the drawing order of the
frame, and `World::object_at` confirms a point inside it with
`Actor3d::is_transparent_at` (the pick ray of the pixel moved to model
space by the inverse world matrix, `AnimatedModel::pick`). `GetHeight`
is `Y - top - 5` of that rectangle. Each active attachment adds its own
projected rectangle as a hit area of the actor (`Actor3d::hit_rects`),
and a point inside one is confirmed by the pick into the attachment's
model, through the inverse of its full matrix.

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

**As implemented** (`luma_adventure::draw3d`, over the shadow items of
[rendering.md](rendering.md#shadow-items)); the effective type comes
with `Actor3dDraw::shadow_type`, the colour, the light position (`actor
.position + LIGHT_POSITION * Scale`), the size and the image with it;
the shadow is pushed before the model and the attachments cast with the
actor (`RenderModel`, `DisplayShadowVol`). Where Luma differs from the
original, on purpose:

- **Simple**: the unit quad of `CreateSimpleShadow` (`-1..1` in X/Z,
  the image mapped once), with `Scaling(size, 1, size) * RotationY(angle)
  * Translation(position)`, the actor's or the game's image decoded with
  the magenta colour key and mipmapped, else a default image generated in
  code (`default_shadow_image`: a 64x64 black blob, alpha 192 at the
  centre falling smoothly to 0 at the edge, standing in for the
  original's `shadow.png`), drawn **unlit** (emissive white, the texture's
  alpha), alpha-blended, depth test on, no depth writes.
- **Flat**: no light-view pre-pass. The posed meshes of the model and of
  the attachments are drawn a second time with the world matrix followed
  by the planar shadow matrix `shadow_matrix(plane y = position.y, light)`
  (`D3DXMatrixShadow` for a point light: every vertex goes to where the
  line from the shadow light through it meets the plane), in a solid
  colour, no texture, no light, no depth writes, with the renderer's
  `stencil_once` (reference `other_index % 255 + 1`) so that the
  overlapping triangles of the silhouette darken the floor once, as the
  original's single textured quad does. The colour is the shadow colour's
  alpha with its RGB channels **squared**, since the original modulates
  the silhouette texture (drawn in the shadow colour) by the floor quad's
  vertex colour (the shadow colour again); for the default black it is
  black. Differences: the silhouette is not cut at 250 units from the
  actor, and it is sharp where the original's 512x512 texture is
  filtered. A keyed texture makes no hole in the original either (the
  alpha stage is disabled for the pre-pass), nor here.
- **Stencil**: the algorithm of `C3DShadowVolume::AddMesh` as written,
  on each posed mesh of the model and of the attachments
  (`draw3d::add_shadow_volume`): the face normal `(v2 - v1) x (v1 - v0)`,
  a face turned toward the light when its dot with the light direction
  (the direction from the actor to its shadow light, moved to the model's
  space by the inverse world matrix, `D3DXVec3TransformNormal`) is
  negative, the position-welded adjacency (`draw3d::Adjacency`, exact
  position equality, computed once per model mesh and again when the mesh
  changes shape), the quads `(v1, v2, v3), (v2, v4, v3)` with `v3, v4 = v1,
  v2 - light * |light - position| * 1.5`. The vertices of every model are
  moved to world space and pushed as one `ShadowVolumeDraw`, followed by
  the `StencilFill` in the shadow colour; the renderer counts them with
  wrapping increments and decrements as Direct3D did (the quads face
  inward, so a shadowed pixel counts to 255; see rendering.md). The
  `SHADOW_MODEL` is not used yet: the display model casts.
- **Receivers**: when `min(Game.MaxShadowType, Scene.MaxShadowType)` is
  stencil (`ThreeDFrame::stencil_shadows`), the active nodes flagged
  `RECEIVE_SHADOWS` (walk planes, blocks and generics alike, `SceneGeometry
  ::nodes`) are uploaded once per scene and geometry and pushed as
  `DepthOnlyDraw`s, counter-clockwise faces culled as `RenderShadowGeometry`
  sets `D3DCULL_CCW`, before the first node of the main layer. They hide
  the actors behind them, as in the original.
- Mixing: a flat shadow leaves its stencil reference where it drew and a
  stencil fill clears only where it passed, so flat and stencil shadows
  in one frame interfere, as the manual warns for the original.

### 10.5 Textures, sprites and videos as textures

Materials keep the `.x` texture, or one replaced by script:
`SetTexture(material, file)` accepts an image **or a `.sprite`** (animated
texture: current frame's image, frame delays honoured, the sprite's other
properties ignored); `SetTheoraTexture(material, file)` plays a Theora video
as a looping texture. Both are looked up by material name (section 6.3).

**As implemented**: the actor keeps the overrides as data
(`MaterialOverride { material, texture: MaterialTexture::Sprite(file) |
Theora(file) }`, one per material name, compared ignoring ASCII case; a
later call on the same name replaces the earlier), and the drawing step
gets them in `Actor3dDraw::materials`. As in the original
(`CXModel::SetMaterialSprite/Theora`) the call fails only when the file
cannot be loaded (Luma checks that it exists) or the actor has no model;
a material name no mesh uses is accepted and has no effect. The model's
data is not changed: when drawing, every mesh material whose name matches
is textured with the override instead of its `.x` texture. The drawing
step loads the image or the `.sprite` and advances its frames with the
game timer, or opens the Theora video, plays it in a loop and hands each
decoded frame to the mesh pass as the texture; the video is not an entity
video and has no sound. The overrides are saved with the actor (8.7) and
the files are opened again after a load. Attachments have no material
members.

The drawing side (`draw3d::Render3d`): a material whose `name` matches
an override (ignoring ASCII case) is textured with it; the texture of a
file that is not a `.sprite` is decoded with the colour key and
mipmapped like a model texture; a `.sprite` is parsed once, every image
of its frames uploaded, and its `AnimState` stepped with the game timer
when the frame is prepared (`World::upload`), the first sub-frame's image
of the current frame being the texture (the sprite's hotspots, moves,
sounds and scripts play no part); a Theora video is opened without sound
and stepped on the game timer with the entity videos' rules, restarted
from its first frame when it ends, its decoded frame uploaded as the
texture (no mipmaps). A file that cannot be opened is logged once and
the material keeps its `.x` texture. An override an actor no longer has
(or that names another file) is dropped with its GPU texture.

## 11. The scripting surface

**As implemented** in `luma_adventure::natives_actor3d` (the actor) and
`luma_adventure::scene3d` (the scene's 3D members), with the game's
shadow members in `natives.rs`; `docs/compatibility.md` has the row of
each. Everything below is implemented but `SetEffect` and the effect
parameters, which return `false` for good (section 13). The `CBObject`
members `SetShadowImage`, `GetShadowImage` and `SetLightPosition` exist
on every object, as in the original (`natives_object.rs`, kept in the
object's state); only a 3D actor's shadow uses them. `Scene.EnableFog` is
stored for the drawing step. `Game.MaxActiveLights` answers 8,
`Game.SupportsRealTimeShadows` true and `Game.IsShadowTypeSupported`
true for the four types: Luma's renderer has what the original asked the
device for. `GetLightColor` packs the light's colour with full alpha,
`SetLightColor` keeps the RGB of its argument.

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
| `AddAttachment(file, name, bone)` (alias `AddMesh`, logs obsolete) | Attach a model to a bone (8.6); a runtime error and `false` when the bone or the file is missing. |
| `RemoveAttachment(name)` (alias `RemoveMesh`) | Remove it; whether there was one. |
| `GetAttachment(name)` | The attachment object or `null`; Luma also accepts an index. |
| `SetTexture(material, file)` | Replace a material's texture (image or sprite). |
| `SetTheoraTexture(material, video)` | Replace it by a video. |
| `SetEffect(material, fxFile)`, `RemoveEffect(material)` | HLSL effect on a material (not planned, section 13). |
| `SetEffectParam(material, name, value)`, `SetEffectParamVector(material, name, x, y, z, w)`, `SetEffectParamColor(material, name, color)` | Effect parameters (not planned). |
| `CreateParticleEmitterBone(bone, ox, oy, oz)` | The actor's particle emitter (created if needed, `CreateParticleEmitter(true)`), placed each frame at the projected position of the bone plus the offset in the bone's space (`UpdatePartEmitter`; an unknown bone leaves it at the actor's position, where the original uses an uninitialised point); returns the emitter. A later `CreateParticleEmitter` forgets the bone. |
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
| `NumAttachments` (RO) | int | The bone attachments (Luma; the original's inherited property counts the 2D attachments, always 0 here). |
| plus the inherited object properties (`Ready`, `Caption`, `Active`, `Registrable`, `Height`, `AlphaColor`, `BlendMode`, `Scale...` of 2D objects where meaningful). |

### 11.3 The attachment object

Returned by `GetAttachment`: `PlayAnim`/`PlayAnimAsync(name)`, `StopAnim()`,
`StopAnimChannel(channel)`, `PlayAnimChannel`/`Async(channel, name)` (forced
restart, channel 0 by default; they pass transition time 0, but the model's
owner is the actor, so when the channel already plays something the actor's
transition table decides, as in section 7), the properties
of `CAdObject3D` (`PosX/Y/Z`, `Angle`, `Scale`, `Active`...), `Type` =
`"attachment"`.

**As implemented** (`natives_actor3d.rs`, `attachment3d_*`): `Type`
(`attachment`), `Name`, `Filename` (the model file), `Active`, `Ready`
(always true), `PosX`, `PosY`, `PosZ`, `Angle`, `DirectionAngle`/`DirAngle`
(normalised on set), `Scale` (percent), `DrawBackfaces`, and `Bone`, the
parent bone, a Luma extension (the original keeps it private); the six
animation methods above (`false` when the model has no such set; the
blocking forms wait for the attachment, which is always ready), `SkipTo3D`
(the offset from the bone), `GetBonePosition2D/3D` (a bone of the
attachment's own model through its full matrix) and the members every
object has (cursor, captions, `SetShadowImage`...). The string form is
`[attachment object]`. The other `CAdObject3D` members (speeds, drop to
floor, the shadow and light settings) have no effect on an attachment in
the original and are not exposed.

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
| Math with the D3D conventions (matrices, quaternions, project/unproject, the D3DX functions listed in 12.2) | `luma-3d` | **Done**: `luma_3d::math`. |
| `.X` reader (text, binary, compressed), `.3ds` reader, `.geometry`, `.act3d`, `.anim` parsers: pure data | `luma-3d` | **Done**: `luma_3d::three_ds`, `node_patterns`, `xfile`, `xfile_binary`, `model`, `actor3d`; the module crate is re-exported by `luma` as `luma::three_d` under the `3d` feature; `luma-formats` only parses the scene keys. |
| Model, skeleton, animation channels, CPU skinning, geometry queries, path finder, camera projection, picking | `luma-3d` | No GPU, no game logic, unit-testable. **Done** for the skeleton, channels, skinning and the model pick (`luma_3d::animation`). |
| The mesh pass on wgpu (depth buffer, pipelines, lighting shader, textures with mipmaps) | `luma-render` | **Done**: a `MeshDraw` item of the `DrawList`, see 12.3. Generic (plain matrices, vertices, lights); knows nothing of actors or scenes. |
| Shadows and the conversion of models and lights into mesh draws | `luma-adventure` (`draw3d`), on the public API of `luma-render` | **Done**: see 12.4; the geometry overlay is not drawn. |
| `Actor3D` object, its state machine, 2D/3D scene glue, scripts | `luma-adventure`, behind a Cargo feature `3d` | Genre behaviour. |
| Feature `3d` in `luma` (off by default) that enables `luma-3d` and `luma-adventure/3d` | `luma` | `docs/modules.md`. |

New third-party dependencies: none beyond `flate2`, already in the workspace. The maths is a few plain types
in `luma_3d::math` rather than a math crate (`glam` was considered): the
row-vector convention stays explicit and the D3DX functions are written
from their documentation. MSZIP-compressed `.X` files are inflated with
`flate2` (raw deflate; the history of the previous block is fed as a stored
block, since the Rust back end has no preset-dictionary call). No other
crate is needed for `.X` or `.3ds`.

### 12.2 Library functions of D3DX that must be re-implemented exactly

`D3DXMatrixLookAtLH`, `PerspectiveFovLH`, `RotationYawPitchRoll` (only the
yaw is used), `RotationZ`, `Scaling`, `Translation`, `Inverse`,
`QuaternionRotationMatrix`, `QuaternionSlerp`, `RotationQuaternion`,
`Vec3Lerp`, `Vec3Project`/`Vec3Transform`/`TransformCoord`/`TransformNormal`,
`PlaneFromPoints`, `PlaneIntersectLine`. Plus the three helpers that WME
wrote itself (`IntersectTriangle`, `PickGetIntersect`,
`DecomposeMatrixSimple`).

### 12.3 `luma-render`

**As implemented**; the specification is the "3D pass" section of
[rendering.md](rendering.md#3d-pass). In short:

1. **A depth and stencil buffer** (`Depth24PlusStencil8`), owned by the
   renderer at the target's size and cleared once per frame with the
   colour (`CBRenderD3D::Fill`). Quads draw with the depth test and the
   writes off (`Setup2D`); meshes test `<=` and write (`Setup3D`). The
   stencil is cleared to zero for the shadow volumes to come.
2. **Meshes are items of the `DrawList`**, interleaved with the quads:
   `DrawList::push_mesh(MeshDraw)` records the draw before the quad with
   the current index, and one render pass draws the list in order (the
   study's option of a custom pass per segment was dropped: a single pass
   with two pipelines is simpler and cheaper on tiled GPUs, and the
   shadow textures of the flat shadows can be rendered before the pass).
   A `MeshDraw` takes a `MeshHandle` (`Renderer::upload_mesh`, vertices
   `position, normal, uv` and `u32` indices, front faces clockwise;
   `update_mesh` for the skinned pose of the frame), an optional index
   range for a material subset, the world and view-projection matrices as
   column-major `[[f32; 4]; 4]` (`Mat4::to_cols_array` then
   `luma_render::matrix`), the texture, the material (diffuse, emissive),
   the ambient colour, up to 8 `Light`s (point, spot, directional, the
   model of 3.4 without attenuation), fog, the culling flag and the blend
   mode.
3. **The viewport** of the meshes is the quads' (letterboxed or not), so
   the projection of 4.1 is computed from the logical resolution and the
   meshes land where the 2D mapping says.
4. **Textures** for meshes are uploaded with a CPU-built mipmap chain
   (`Renderer::mesh_texture`) and sampled trilinearly with repeat
   addressing; the colour key is already alpha `0` and the alpha test cuts
   it (`ALPHAREF 8`).
5. Offscreen tests as for 2D (`crates/luma-render/tests/mesh.rs`): a lit
   textured mesh with directional, ambient, point and spot lights, the
   culling winding, index ranges, meshes between quads with the depth
   test, a vertex update between two frames, the alpha test, fog and the
   letterboxed viewport.

`luma-render` does not depend on `luma-3d`: the conversion of a
`luma_3d::model::Material` into a `MeshMaterial`, of a skinned mesh into
`MeshVertex`es and of `C3DLight`s into `Light`s is the module's.

### 12.4 The 3D pass: `luma_adventure::draw3d`

**As implemented.** The glue between `World::three_d_draws` and the
renderer lives in the adventure module, not in `luma-3d` as first
planned: it needs the world (the files through the `Vfs`, the game
timer, the scene's path) and `luma-3d` stays free of the renderer.

- **`Render3d`**, a field of the world (`World::render3d`), is the GPU
  side kept across frames: per actor or attachment and mesh index the
  uploaded mesh (`Renderer::upload_mesh` with the posed vertices and the
  triangles **sorted by material**, so that each material is one index
  range as the attribute table of a `D3DXMesh` makes it; `update_mesh`
  every frame with the skinned pose; uploaded again when a reload changed
  the triangle count) and its adjacency, the textures by path (decoded
  with the magenta colour key, `Renderer::mesh_texture` for the mipmaps;
  a file that fails is logged and remembered as missing), the script
  textures (10.5), the shadow receivers (10.4), the unit quad and the
  default image of the simple shadows. `World::upload` refreshes it
  (`Render3d::upload`) and frees what belongs to actors and attachments
  that are gone; it is empty without the `3d` feature.
- **Items.** `World::draw` asks `Render3d::overlays` for the frame's
  items (10.2): per material of each mesh a `MeshDraw` with the world
  matrix of section 2 and the frame's view-projection (`Projection::
  view_projection`, both through `Mat4::to_cols_array` and
  `luma_render::matrix`), the material's diffuse and emissive, its
  texture (the override's when a script set one), the ambient (the
  actor's override else the scene's), the lights converted by
  `convert_light` (channels divided by 256, point lights with range
  100000, spots with `theta 0.5`, `phi 1.0`, falloff 1, range `|target -
  position|`, as 3.4 says), the fog (`Scene.EnableFog`, colour channels
  divided by 255), `cull_backfaces = !DrawBackfaces` and the actor's blend
  mode. The attachments follow with their full matrix and their own
  backface flag, lit like the actor. The `AlphaColor` tint is not applied,
  as the original's texture stage ignores it (10.1).
- **Shadows**: 10.4.
- **Picking** uses the skinned triangles on the CPU (10.3), untouched.
- **Not done**: the `SHADOW_MODEL` (the display model casts the shadow)
  and the geometry overlay of `Scene.ShowGeometry` (wireframe in the
  original, which needs a line pipeline the renderer does not have; the
  flag is not stored either).

### 12.5 `luma-adventure`

**Done**, behind the crate's `3d` feature (`luma-3d` is an optional
dependency with its `serde` feature; without the feature the 3D types
are uninhabited stand-ins in `three_d_off.rs`, so the world and the
saved games keep their shape):

- `Actor3d` beside `Actor` in the world (`World::actors3d`),
  `Kind::Actor3D` in `objects.rs`; loaded from `.act3d`; the generic
  object behaviour (name, caption, scripts, sounds, speech, inventory,
  cursor, events, `StickToRegion`, particle emitters) through the same
  dispatch arms as the 2D actor.
- The state machine of 8.2, 8.3, 8.4 over `luma-3d`'s skeleton and
  geometry; `GoTo` with `2D_PATHFINDING` reuses the 2D `path` module
  (`Floor`), the points converted with the tolerant conversion.
- `SceneView::three_d` (`Scene3d`) loads the scene's geometry
  (`GEOMETRY`, `CAMERA`, `WAYPOINT_HEIGHT`), owns the path finder and
  the fog, and keeps the projection of the frame for the drawing step.
- Drawing order: a 3D actor is a free object at its projected position
  (`DrawItem::Other`), with an empty `Drawable`; the 3D pass (12.4) takes
  `World::three_d_draws()`: the frame's `Projection`, the scene's
  ambient and fog and geometry (shadow receivers), whether stencil
  shadows are allowed, and per actor its index among the free objects,
  the posed `AnimatedModel`, the world matrix, position and angle, the
  lights of 3.4 (`Scene3d::lights_for`), the ambient override, the
  backface flag, the shadow settings (the effective type, colour, light
  position, size, image), the material overrides and the attachments.
- Hit areas: 10.3.
- Scripts: `natives_actor3d.rs`, `scene3d.rs`, the game's shadow rows in
  `natives.rs`.
- Attachments (8.6) and the textures set by script (10.5): data on the
  actor, handed to the drawing step in `Actor3dDraw::attachments`
  (`AttachmentDraw`, with the full matrix) and `Actor3dDraw::materials`;
  the bone emitter through the particle system (`natives_particles.rs`).
- Saving: 8.7, layout version 14.
- The player is built with the `3d` feature: the window and the
  headless `--screenshot` draw the 3D actors through `World::upload` and
  `World::draw` like everything else.

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
  triangle, a depth overlap, a shadow darkening): `crates/luma-render/
  tests/shadow.rs` for the stencil items and the mesh options,
  `crates/luma-adventure/src/draw3d.rs` for the silhouette of a cube, the
  adjacency, the planar shadow matrix and the light conversion, and
  `crates/luma-player/tests/three_d.rs` runs the player headless on a
  synthetic 3D game (a lit quad on a painted background, a walk plane, a
  camera, a light) with each `MAX_SHADOW_TYPE` and checks the pixels the
  `Projection` names (the body lit, the floor green, the shadow darker
  where the light casts it). All skip without a graphics adapter.
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
| 1 | **Crate and conventions.** `luma-3d` with a feature in `luma`, `README.md` and `docs/modules.md` rows, the `math` module (D3D conventions, the functions of 12.2). **Done.** | `3DUtils.cpp`, `3DCamera.cpp`, `BObject.cpp` (`GetMatrix`), `dcgf.h` (`DegToRad`) | S | |
| 2 | **`.X` text container.** Tokenizer and template-agnostic reader to a generic object tree (`xof 0303txt`, named templates recorded by name and GUID, nested data, data references, comments; binary and compressed headers rejected). `docs/formats/xfile.md`. **Done** (`luma_3d::xfile`). | `XFile.cpp`, `XModel.cpp` (`LoadFromFile`), `XFrameNode.cpp` (`LoadFromXData`); samples `trinity.x`, `teapot.x` | M | 1 |
| 3 | **`.X` to model.** Frames, `FrameTransformMatrix`, meshes (triangulation, normal vertex splitting, materials, `TextureFilename`), `SkinWeights` and the bind matrices, `AnimTicksPerSecond`, animation sets with the four key types, `AnimationOptions`, merging, material naming, the sampler. **Done** (`luma_3d::model`); the position-welded adjacency moves to task 9 / 12b, where it is consumed. | `XMesh.cpp`, `SkinMeshHelper.cpp`, `XAnimation.cpp` (`Load`, `LoadAnimationKeyData`), `XAnimationSet.cpp`, `3DMaterial.cpp` | L | 2 |
| 4 | **`.X` binary and compressed.** `bin `, `tzip`/`bzip` (MSZIP) with `flate2`, into the same tree as the text reader. **Done** (`luma_3d::xfile_binary`); checked against fixtures built in the tests, not yet against a file from a real game (`formats/xfile.md` 1.4 and 1.5 say what is unverified). | (public DirectX documentation **[ext]**) | M | 2 |
| 5 | **`.3ds` geometry and `.geometry`.** The chunk walker of 3.2 into planes, blocks, generics, waypoint points, cameras, lights; the node patterns; scene keys of 3.1 in `luma_formats::scene`. `docs/formats/geometry.md`. **Done** (`luma_3d::three_ds`, `node_patterns`, `scene_geometry`). | `3DLoader3DS.cpp`, `3DS_chunks.h`, `AdSceneGeometry.cpp` (`LoadFile`), `AdGeomExt*.cpp`, `AdWaypointGroup3D.cpp`, `3DMesh.cpp`, `3DLight.cpp` | M | 1 |
| 6 | **`.act3d` and `.anim`.** Parser to a typed definition, TEMPLATE, defaults, `ANIMATION`/`EVENT`, obsolete keys. `docs/formats/actor3d.md`. **Done** (`luma_3d::actor3d`). | `AdActor3DX.cpp` (`LoadBuffer`, `ParseEffect`, `MergeAnimations2`), `XModel.cpp` (`ParseAnim`, `ParseEvent`), `AdActor3D.cpp` (`IsActorX`) | S | |
| 7 | **Camera and the 2D/3D mapping.** View and projection of 4.1 for a scene (`Camera`, viewport, scroll, layer size, `SCROLL_3D_COMPATIBILITY`, `FOV_OVERRIDE`, clip planes), `convert_2d_to_3d` (exact and tolerant), `convert_3d_to_2d`, the object's 2D position. **Done** (`luma_3d::camera`; the object's 2D position is `Projection::project` of its origin, to be called by task 14). | `BRenderD3D.cpp` (`SetProjection`, `GetProjectionParams`), `AdSceneGeometry.cpp` (`Convert*`, `StoreDrawingParams`), `AdObject3D.cpp`, `AdGame.cpp` (`GetLayerSize`), `AdScene.cpp` (`ScrollTo`) | M | 1, 5 |
| 8 | **Geometry queries and the 3D path finder.** `DirectPathExists`, `GetBlockIntersection`, Dijkstra with the alternate-target rerun, the waypoint groups, the light toggles, the path an actor consumes. Every quirk of 9.2 and 9.3 is reproduced. **Done** (`luma_3d::path3d`, `SceneGeometry::direct_path_exists`, `block_intersection`, `set_light_enabled`, `set_light_color`). | `AdSceneGeometry.cpp`, `3DUtils.cpp` (`IntersectTriangle`, `PickGetIntersect`), `AdPath3D.cpp`, `AdPathPoint3D.cpp` | M | 5 |
| 9 | **Skeleton animation and skinning.** Frame hierarchy and matrices, channels, active animations, blending slots, events, transition-time table, looping and finish rules, CPU skinning, bounding box, ray/triangle picking of a posed model, bone matrices and positions. **Done** (`luma_3d::animation`, section 7); the 2D bone positions need the camera of task 14, the welded adjacency goes to task 12b. | `XFrameNode.cpp`, `XModel.cpp`, `XAnimationChannel.cpp`, `XActiveAnimation.cpp`, `XAnimation.cpp` (`Update`), `XAnimationSet.cpp`, `XMesh.cpp` (`Update`, `PickPoly`) | L | 3 |

### Phase B: drawing

| # | Task | WME files | Size | After |
|---|------|-----------|------|-------|
| 10 | **`luma-render` extension.** Depth/stencil buffer cleared once per frame, quads without depth, `MeshDraw` items of the `DrawList` drawn in order in the same pass, mesh upload and per-frame vertex update, tests of the depth test among meshes and of the viewport. **Done** (12.3, [rendering.md](rendering.md#3d-pass)). | `BRenderD3D.cpp` (`Fill`, `Setup2D`/`Setup3D`) | M | |
| 11 | **Mesh pass.** Pipelines, dynamic skinned buffers, materials and textures with mipmaps and the colour key, lights (model of 3.4), ambient, fog, alpha test and blend mode, culling option. **Done** in `luma-render` (shader `mesh.wgsl`); the light *selection* per object (`EnableLights`) and the geometry debug overlay (`Scene.ShowGeometry`, the validation tool for task 7) move to task 13, which has the scene. | `BRenderD3D.cpp` (`Setup3D`), `XMesh.cpp` (`Render`), `3DMaterial.cpp`, `3DLight.cpp`, `AdSceneGeometry.cpp` (`Render`, `EnableLights`, `CreateLights`) | L | 7, 9, 10 |
| 12a | **Simple shadows.** **Done** (10.4; the default image is generated). | `3DShadowGen.cpp` (`RenderSimple`) | S | 11 |
| 12b | **Stencil shadows.** CPU shadow volumes, stencil passes, receiver depth prepass, attachments, `SHADOW_MODEL`. **Done** but the `SHADOW_MODEL` (10.4, the shadow items of `luma-render`). | `3DShadowVolume.cpp`, `AdActor3DX.cpp` (`DisplayShadowVolume`), `AdSceneGeometry.cpp` (`RenderShadowGeometry`), `XMesh.cpp` (`UpdateShadowVol`), `AdScene.cpp` (`TraverseNodes`) | M | 11, 9 |
| 12c | **Flat shadows.** Light-view pre-pass to texture, projective floor quad. **Done** as a planar projection of the posed triangles with a stencil mask (10.4), not a pre-pass. | `3DShadowGen.cpp` (`Render`, `SetAdjustedProjectionMatrix`, `SetTextureMatrixStage0Stage1`) | M | 11 |

### Phase C: the actor in the game

| # | Task | WME files | Size | After |
|---|------|-----------|------|-------|
| 13 | **Scene integration.** Load geometry with the scene, active camera and light/node state, the 2D position of 3D objects, the draw order hook with `Other` items, geometry scenes in the headless player. **Done** (`luma_adventure::scene3d`, `World::projection_3d`, `World::three_d_draws`, the hookup in `draw3d`). | `AdScene.cpp` (`LoadBuffer`, `TraverseNodes`, `DisplayRegionContent`, `UpdateFreeObjects`, `ScrollTo`) | M | 5, 7, 11 |
| 14 | **`Actor3D` core.** Load, idle, `PlayAnim` family, drop to floor, `SkipTo`, `X`/`Y`, direction, scale; `Game.LoadActor3D`/`Scene.LoadActor3D`/`Unload*`. **Done** (`luma_adventure::actor3d`). | `AdActor3DX.cpp`, `AdObject3D.cpp`, `AdActor3D.cpp` (`CreateFromFile`) | M | 6, 9, 13 |
| 15 | **Walking, turning, direct control, talking.** 8.2 to 8.4 over tasks 8 and the existing 2D path module for `2D_PATHFINDING`; blocked region and waypoints of the actor. **Done**; the actor's own blocked region and waypoints are parsed and not yet handed to the 2D floor of the other actors. | `AdActor3DX.cpp` (`Update`, `GoTo*`, `FollowPath*`, `GetNextStep*`, `PrepareTurn`, `TurnToStep`) | L | 8, 14 |
| 16 | **Hit testing and captions.** Projected rectangle and polygon pick, registration order, `GetHeight`. **Done** (10.3). | `XModel.cpp` (`IsTransparentAt`, `UpdateBoundingRect`), `XMesh.cpp` (`PickPoly`), `BRenderer.cpp` (`GetObjectAt`) | S | 9, 14 |
| 17 | **Attachments and bones.** `AddAttachment`, `GetAttachment`, the attachment object, `GetBonePosition2D/3D`, `CreateParticleEmitterBone`. **Done** (`Attachment3d`, `AttachmentDraw`; drawn and casting with the actor, 12.4, 10.4). | `AdAttach3DX.cpp`, `AdActor3DX.cpp` (`UpdateAttachments`, `DisplayAttachments`, `UpdatePartEmitter`), `AdObject3D.cpp` (`GetBonePosition*`) | S | 14 |
| 18 | **Scripting surface.** The remaining methods and properties of section 11, scene lights/camera/fog/nodes, game shadow properties, the `CBObject` shadow members on every object. **Done**. | `AdActor3DX.cpp`, `AdObject3D.cpp`, `AdScene.cpp`, `AdGame.cpp`, `BGame.cpp`, `BObject.cpp` | M | 14, 15, 17 |
| 19 | **Saved games.** 8.7 into `save.rs`, geometry state, animation channels, attachments, material overrides. **Done** (layout version 14); the open path request is not kept. | `Persist` functions of the files above | M | 18 |
| 20 | **Material scripting.** `SetTexture` with images and sprites, `SetTheoraTexture`; `EFFECT` ignored. **Done** (`MaterialOverride`; the drawing step loads the files and textures the materials with their frames, 10.5). | `XModel.cpp` (`SetMaterial*`), `3DMaterial.cpp` | S | 11, 18 |

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
6. 4 (binary `.X`) is done ahead of need; the first real binary file will
   confirm the details that could not be verified.

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

1. **Crate topology.** Decided: a new `luma-3d` crate, feature `3d` of
   `luma`, re-exported as `luma::three_d`; `luma-formats` only gained the
   scene keys.
2. **Which games to validate against.** The local test games (The White
   Chamber, OpenQuest) are 2D as far as we know. The WME demo is the only 3D data at hand;
   is a real commercial 3D game available for local tests? Are the sample
   models (teapot) acceptable in CI?
3. **Binary `.X`.** The samples are text; real games probably ship both.
   All four forms are read now, from the public documentation and
   fixtures built in the tests; the first real binary or compressed file
   (the opt-in test over `testdata/local/` picks them up) will confirm
   the size words of the MSZIP container.
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
   Decided for now: untextured materials draw their lit colour, the
   simple shadow is drawn unlit (10.4); to be checked against the demo.
7. **Flat shadow pixel details.** Decided: the planar projection of the
   posed vertices onto `y = actor.y` with a stencil mask (10.4), no
   pre-pass; the 250-unit cut and the texture filtering of the original
   are not reproduced. Revisit if a game shows the difference.
8. **Path-finder time budget.** Closed: the original's 15 ms budget is
   dead code (9.3), the request always runs to completion in one game
   loop, and `PathFinder::run` does the same.
9. **Resolution mismatch.** The camera is defined against the runtime
   resolution and the main layer (4.1). A game running at another resolution
   than its scenes were laid out for would shift; check the sample scenes
   (800x600 editor resolution).
10. **Animation time base.** Animations use the game timer clamped to 1000 ms
    per frame; the existing sprite animation uses the same timer, to be
    reused.
11. **Save-game scope.** Decided (8.7): the active camera, the lights'
    flags and colours, the nodes' flags, the waypoint height and the fog
    are saved with the scene and reapplied by name after the geometry is
    read again; a scene change drops them with the scene, as the original
    does (the scene state of `PERSISTENT_STATE` does not cover the
    geometry).
12. **Texture formats.** 3D textures in real games are `.bmp`, `.jpg`, `.tga`,
    `.png`, all loaded by `luma_formats::Image`; power-of-two limits do not
    apply on modern GPUs.
13. **`ShowGeometry` overlay** is a debugging aid; is it wanted in the
    player's UI or only in tests? Not drawn yet (12.4): the original's is
    a wireframe and the renderer has no line pipeline.

## 17. Index of source locations

| Topic | File and function |
|-------|-------------------|
| Actor file keys | `wme_ad/AdActor3DX.cpp`, `LoadBuffer` |
| Actor update and display | `AdActor3DX.cpp`, `Update`, `Display`, `DisplayShadowVolume` |
| Walking | `AdActor3DX.cpp`, `GoTo3D`, `FollowPath3D`, `GetNextStep3D`, `InitLine3D`, `GoTo2D`, `FollowPath2D`, `GetNextStep2D` |
| Turning | `AdActor3DX.cpp`, `PrepareTurn`, `TurnToStep`, `DirToAngle`, `AngleToDir` |
| Scripts | `AdActor3DX.cpp` and `AdObject3D.cpp`, `ScCallMethod`/`ScGetProperty`/`ScSetProperty`; `BObject.cpp` (`SetLightPosition`, `SetShadowImage`) |
| 2D position, drop to floor | `AdObject3D.cpp`, `Update`, `Convert3DTo2D` |
| Attachments | `AdAttach3DX.cpp`; `AdActor3DX.cpp`, `UpdateAttachments`, `DisplayAttachments`, `AddAttachment` in `ScCallMethod` |
| Bone emitter | `AdActor3DX.cpp`, `CreateParticleEmitter`, `UpdatePartEmitter` |
| Material textures | `XModel.cpp`, `SetMaterialSprite`, `SetMaterialTheora`; `XMesh.cpp`, `3DMaterial.cpp` |
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
