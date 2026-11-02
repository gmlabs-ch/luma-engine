# Scene geometry: `.3ds` and `.geometry`

The hidden 3D geometry of a scene: the walk planes actors stand on, the
blocked meshes they avoid, the waypoints their path finder links, extra
meshes that receive shadows, and the cameras and lights the painted
background was rendered with. A scene names it with its `GEOMETRY` key
([scene.md](scene.md#3d-keys)); only `.3ds` files are accepted. Next to
the `.3ds` file, an optional `<same base name>.geometry` file says which
mesh plays which role.

Implemented by `luma_3d::three_ds` (the chunks), `luma_3d::node_patterns`
(the sidecar) and `luma_3d::scene_geometry` (the classified result).
Established from the MIT-licensed Wintermute Engine 1.x sources
(`3DLoader3DS.cpp`, `3DS_chunks.h`, `AdSceneGeometry.cpp`, `AdGeomExt*.cpp`,
`AdWaypointGroup3D.cpp`, `3DMesh.cpp`, `3DCamera.cpp`, `3DLight.cpp`) and
checked against the geometry of the Wintermute 3D demo
(`install/source/projects/wme_demo_3d/data/scenes/Front/`). How the
geometry is used at runtime (camera mapping, floor height, paths) is in
[../3d-actors.md](../3d-actors.md).

## The `.3ds` file

A 3D Studio file is a tree of chunks. Each chunk is a header of six bytes,
a `u16` identifier and a `u32` length **including the header**, both little
endian, followed by its payload and then its child chunks up to the length.
Positions are `f32` triples, counts `u16`.

The original's loader is a **flat chunk walker**: it reads the chunks in
file order, *enters* the ones it knows (continues right after the payload
it read, so that the children come next) and skips every other chunk by
its length, children included. The object a chunk belongs to is simply the
**last `NAMED_OBJECT` seen**, in any section of the file. Luma does the
same. The chunks read:

| Id | Name | Payload read | Effect |
|----|------|--------------|--------|
| `0x4D4D` | `MAIN3DS` | none | Entered. |
| `0x3D3D` | `EDIT3DS` | none | Entered. |
| `0x4000` | `NAMED_OBJECT` | NUL-terminated name | Starts an object of unknown kind; entered. |
| `0x4100` | `TRIANGLE_MESH` | none | The current object is a mesh; entered. |
| `0x4110` | `TRIANGLE_VERTEXLIST` | `u16 n`, `n` x `(x, y, z)` | Vertices. The file is Z-up right-handed; a position is stored as **`(x, z, y)`**, which makes it Y-up left-handed. |
| `0x4120` | `TRIANGLE_FACELIST` | `u16 n`, `n` x `(u16 a, b, c, flags)` | Faces, stored as **`(a, c, b)`**: the winding flip that goes with the axis swap, so a face that is counter-clockwise seen from above in the 3D package is clockwise (a front face) here. The flags are dropped. Entered: the material lists are its children. |
| `0x4130` | `TRIANGLE_MATERIAL` | NUL-terminated material name, `u16 n`, `n` x `u16` face index | Kept on the mesh, unused by the geometry. |
| `0x4140` | `TRIANGLE_MAPPINGCOORS` | `u16 n`, `n` x `(u, v)` | Texture coordinates, `v` stored as `1 - v`. Unused by the geometry. |
| `0x4160` | `TRI_LOCAL` | 12 x `f32`: three axis rows and an origin row | Kept, **not applied**: vertices are used as stored, exporters are expected to bake transforms. |
| `0x4700` | `CAMERA` | position `(x, y, z)`, target `(x, y, z)`, `bank`, `lens` | The current object is a camera. Position and target are swapped like vertices; `bank` is in degrees, `lens` in millimetres. |
| `0x4600` | `LIGHT` | position `(x, y, z)` | The current object is a light (swapped). Entered: its children follow. |
| `0x4610` | `LIGHT_SPOTLIGHT` | target `(x, y, z)`, `hotspot`, `falloff` | Inside a light: it is a spot light aimed at the target (swapped). Hotspot and falloff are kept, unused. |
| `0x4620` | `LIGHT_IS_OFF` | none | Inside a light: it starts disabled. |
| `0x0010` | `RGB_FLOAT` | `r, g, b` floats | Inside a light: its colour, each channel `trunc(value * 255)`. Elsewhere skipped. |
| `0x0011` | `RGB_BYTE` | `r, g, b` bytes | Inside a light: its colour. Elsewhere skipped. |
| `0xB000` | `KEYFRAMER` | none | Entered. |
| `0xB001`..`0xB007` | `KEYFRAMER_*_INFO` | none | Entered; the section kind is remembered (`0xB003` is a camera). |
| `0xB010` | `NODE_HDR` | NUL-terminated object name, three `u16` | The name of the object the keyframer section describes. |
| `0xB024` | `ROLL_TRACK_TAG` | `u16 flags`, 8 unknown bytes, `u16 keys`, `u16`, then per key `u16 frame`, `u32`, `f32 roll` | Only in a camera section: the roll of the **first key** replaces the `bank` of the camera of that name (case-insensitive). |

Everything else, materials (`0xAFFF` and its tree), the version chunks,
smoothing groups, light attenuation, camera ranges, the other keyframer
tracks, is skipped. A mesh may have any number of vertices up to 65535;
the counts are 16-bit.

Deviations from the original, all on malformed input only: a mesh, camera
or light chunk before any `NAMED_OBJECT` is skipped by its length (the
original would read its payload as chunks); a payload that runs past its
chunk, a chunk that runs past the file, a chunk shorter than its header or
a name without terminator is an error with the chunk id and offset
(`luma_3d::three_ds::Error`) where the original reads out of bounds. On a
well-formed file the two agree byte for byte in what they take.

### Derived values

- **Camera.** The field of view in degrees is `1900 / lens`, or 45 when
  `lens <= 0`; it is stored in radians. The view matrix is
  `LookAtLH(position, target, up)` with `up = (0, 1, 0)` rotated about Z by
  `bank` degrees (`3DCamera.cpp`). Cinema 4D is known to export a wrong
  lens, which is what the scene's `FOV_OVERRIDE` is for.
- **Light.** Colour from the colour chunk (white when there is none: the
  original leaves it undefined), `active = !LIGHT_IS_OFF`, spot and target
  from `LIGHT_SPOTLIGHT`. Every other light property of the file is
  ignored; the light model is in [../3d-actors.md](../3d-actors.md#34-lights).
- **Mesh normals.** Smooth, per vertex: the normalised sum, over the faces
  sharing the vertex, of the normalised face normal `(v1 - v0) x (v2 - v1)`
  (`C3DMesh::ComputeNormals`). They only serve the debug overlay and the
  shadow pass.
- 3DS object names are limited to about ten characters by the common
  exporters (the demo has `blk_wareho`, `shadow_Bo0`); scripts name nodes
  exactly as the file does.

## The `.geometry` file

A [definition file](definition-files.md) with one `GEOMETRY` root block
holding `NODE` blocks:

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

| Key of `NODE` | Type | Default | Meaning |
|---------------|------|---------|---------|
| `NAME` | string | `""` | A pattern on mesh names: `*` matches any run of characters, `?` exactly one, letters match ignoring ASCII case. A `.` in the pattern follows the original's DOS file-name rules (a trailing `.` matches a name without extension, `*.ext` a name with that extension); mesh names have no dots in practice. |
| `WALKPLANE`, `BLOCKED`, `WAYPOINT` | bool | false | The role. Each key that is `TRUE` sets the role, so when several are true the last one in the block wins; none means a **generic** mesh. |
| `RECEIVE_SHADOWS` | bool | false | The meshes receive stencil shadows. |

A `NODE` without `NAME` only matches an empty name (the original would
dereference a null pattern).

After the file's nodes come three **built-in** nodes, in this order:
`walk_*` (walk plane, receives shadows), `blk_*` (blocked, does not),
`wpt_*` (waypoint, does not). Without a `.geometry` file only these three
exist, so a `.3ds` of `walk_`, `blk_` and `wpt_` meshes plus cameras and
lights needs no sidecar.

The original's parser only knows the keys above: a key it does not know,
in `GEOMETRY` or in a `NODE`, fails the whole file, and the geometry is
then classified with the built-in nodes alone, as if the file were absent.
`luma_3d::NodePatterns::parse` fails the same way and
`NodePatterns::load` applies the same fallback.

## Classification

Each mesh of the `.3ds` file, in file order, is matched against the node
list; the **first** matching node gives its role and its `RECEIVE_SHADOWS`
flag; a mesh matching nothing is **dropped**.

| Role | Result | Overlay colour |
|------|--------|----------------|
| Walk plane | `SceneGeometry::walkplanes`: triangles an actor may stand on, used for picking a 3D point under a 2D point and for the floor height; also a barrier in segment tests. | `0x700000FF` |
| Blocked | `SceneGeometry::blocks`: triangles that stop segments while the node is enabled. | `0x70FF0000` |
| Waypoint | One point per mesh, the centre of its bounding box (`min + (max - min) / 2`), appended to `SceneGeometry::waypoints` (the original keeps a single group). After loading every waypoint is lifted to `floor height + waypoint height` (10 by default; the scene's `WAYPOINT_HEIGHT` re-lifts them). | the `wpt.sprite` marker |
| Generic | `SceneGeometry::generics`: only useful as a shadow receiver. | `0x7000FF00` |

Walk planes, blocks and generics keep their mesh name and an `active`
flag, switched by scripts (`Scene.EnableNode3D` / `DisableNode3D`,
case-insensitive; every node of that name changes). Only blocks honour the
flag in the geometry queries: walk planes are picked and measured whether
enabled or not, as in the original, which only consults a plane's flag in
the debug overlay and the shadow pass.

Cameras and lights are kept in file order with their names. The first
camera is active after loading; the scene's `CAMERA` key selects another
by name (case-insensitive; an unknown name keeps the first). The lights'
`active` flag starts as the file says and is switched by scripts.

## The demo geometry

`geometry.3DS` of the demo's `Front` scene (30 KB) holds one `walk_01`,
five `blk_*`, ten `wpt_*` and twelve `shadow_*` boxes (receivers through
the sidecar above), four lights (`Spot01`, `Spot02`, `Omni02`, `Omni03`)
and two cameras (`Camera01`, `Camera02`); the scene selects `Camera01`.
`crates/luma-3d/tests/demo_geometry.rs` checks this when the files are
copied under `testdata/local/wme_demo_3d/scenes/front/` (git-ignored) and
skips otherwise.
