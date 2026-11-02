# Models: the DirectX `.X` subset

A 3D actor is a skinned, animated model in Microsoft's DirectX `.X` file
format, named by the `MODEL` key of its [actor definition](actor3d.md).
The original engine does not read the format itself: it hands the file to
the DirectX file library and to D3DX (`D3DXLoadSkinMeshFromXof`) and
consumes the objects they return. Luma Engine reads the file directly, so
this document describes two things: the **container** as Luma reads it
(section 1), established from the public DirectX documentation and
checked against the sample models, and the **subset of standard templates**
the engine uses and what it makes of them (section 2), established from the
MIT-licensed Wintermute Engine 1.x sources (`XFile.cpp`, `XModel.cpp`,
`XFrameNode.cpp`, `XMesh.cpp`, `SkinMeshHelper.cpp`, `XAnimation.cpp`,
`XAnimationSet.cpp`, `3DMaterial.cpp`). Statements about what D3DX does
that could not be read in those sources are marked **[ext]**: they come
from the DirectX documentation and from the behaviour the sample files
need.

Implemented by `luma_3d::xfile` (the container) and `luma_3d::model` (the
model). The sample models are those of the Wintermute 3D demo
(`install/source/projects/wme_demo_3d/data/actors/`): `trinity.x` (a
skinned character, 40 bones, seven animation sets), `teapot.x` (a rigid
mesh) and `teapot_taken.x` (a teapot skinned to one bone with a matrix
animation key).

## 1. The container

### 1.1 Header

The file starts with a 16-byte header, four fields of four characters:

| Bytes | Content | Example |
|-------|---------|---------|
| 0..4 | magic `xof ` | `xof ` |
| 4..8 | version, two digits major and two minor | `0303` |
| 8..12 | format: `txt ` text, `bin ` binary, `tzip` compressed text, `bzip` compressed binary | `txt ` |
| 12..16 | float size in bits, `0032` or `0064` | `0032` |

Luma reads the **text** format only. A binary or compressed file is
recognised by its header and rejected with a clear error
(`xfile::ErrorKind::UnsupportedFormat`); the float size of a text file is
recorded and does not change how the numbers are read. Every sample model
is `xof 0303txt 0032`.

### 1.2 Text syntax

After the header the file is a sequence of **template declarations** and
**data objects**. Whitespace separates tokens; `//` and `#` start a comment
that runs to the end of the line. Tokens are:

- identifiers: a letter or `_`, then letters, digits, `_` or `-`;
- numbers: an optional sign, digits, an optional fraction and an optional
  exponent (`-3`, `0.5`, `2E-3`, `.5`); a number without fraction or
  exponent is an integer, the others are floats;
- strings between double quotes, without escapes;
- GUIDs between `<` and `>`;
- the punctuation `{`, `}`, `;` and `,`.

A **template declaration** is `template Name { <guid> members... }`. Luma
records the name and the GUID and skips the body to its matching brace
(the body may contain `[...]` and `[Name <guid>]` restrictions, which are
not tokens of the data section). The member lists are **not interpreted**:
the text form of a data object is self-describing, so the reader does not
need them.

A **data object** is `Template [name] [<guid>] { body }`. The body is a
sequence of:

- scalar values (numbers and strings) separated by `;` or `,`; the
  separators carry no meaning for the reader, which keeps the **flat list
  of values in file order**. Arrays and nested structures (`Vector`,
  `Matrix4x4`, `MeshFace`, `ColorRGBA`...) are therefore just runs of
  values; an empty array is written as a bare `;` and contributes nothing;
- nested data objects, in the same form;
- **data references** `{ name }`, `{ name <guid> }` or `{ <guid> }` to an
  object defined elsewhere in the file (the animations reference the
  frame they drive this way).

`luma_3d::xfile::XFile::parse` returns the header, the templates and the
tree of top-level objects (`DataObject { template, name, guid, values,
children, line }`); `XFile::find(name)` looks an object up by name,
depth-first in file order, case-sensitively. The typed loaders of section
2 read the values of an object in the order its standard template defines
them, through a cursor that checks the kind of each value (`count`,
`float`, `string`...) and reports the object's template and line on a
mismatch. An integer written where a float is expected (`0;` for `0.0;`)
is accepted. An object of an unknown template is kept in the tree and
ignored by the loaders, which is what D3DX does with templates it was not
asked for **[ext]**.

### 1.3 What is not read

- Binary and compressed files (task 4 of [../3d-actors.md](../3d-actors.md);
  MSZIP would use the workspace's `flate2`).
- Template member lists, so a file whose data section disagrees with its
  own template declarations is read as written, not rejected.
- The `[...]` open/closed restrictions on which children an object may
  have: any object may nest any other.

## 2. The model subset

`luma_3d::model::Model::parse(bytes, file_name)` turns the object tree
into a model: a frame hierarchy, meshes, animation sets and the key time
base. The loader walks the top-level objects and, inside a `Frame`, its
children, and acts on the templates below; every other object (`Header`,
`Material` at the top level, `DeclData`, `FVFData`, `EffectInstance`, the
`Effect*` templates, `MeshVertexColors`...) is ignored. The templates are
matched by **name**, not by GUID: the GUIDs of the standard templates are
listed for reference only.

| Object | Where | Handling |
|--------|-------|----------|
| `AnimTicksPerSecond` (`9E415A43-7BA6-4a73-8743-B73D47E88476`) | top level or in a frame | One count; `Model::ticks_per_second`, default **4800**. All key times are in these ticks. |
| `Frame` (`3D82AB46-62DA-11CF-AB39-0020AF71E433`) | top level or in a frame | A named node, children loaded recursively. |
| `FrameTransformMatrix` | in a frame | 16 floats, row-major as written (`Mat4::from_rows`); the frame's loaded local matrix. The last one wins. |
| `Mesh` | top level or in a frame | A mesh attached to the enclosing frame (the root when at the top level); section 2.2. |
| `AnimationSet` | top level or in a frame | Section 2.3. |
| `Animation` outside a set | top level or in a frame | A set of its own, named after the file. |

The model's frame list `Model::frames` starts with a **synthetic root**
(`model::ROOT`, index 0, empty name, identity) that holds the file's
top-level objects, as the original's `CXModel::m_RootFrame`; parents come
before their children, in file order. A file **without a top-level
`Frame`** is rejected (`Error::NoFrame`), as the original rejects it.
`Model::find_frame(name)` is the original's `FindFrame`: exact,
case-sensitive, depth-first in file order; the root never matches.

One place where Luma is more tolerant than the original: an **empty
frame** (`Frame x { }`) fails the whole load in the original (its child
loop leaves the error code at "failed" when there is nothing to load) and
is accepted here.

### 2.1 Paths and names

The model keeps the path it was loaded from (`Model::file_name`) and uses
it for two things, both with the original's `_splitpath` rules where `/`
and `\` are separators:

- an **unnamed `AnimationSet`**, and every loose `Animation`, is named
  after the file: the file name without directory and extension
  (`trinity.x` gives `trinity`; `model::file_stem`);
- a **texture path** is the model file's directory, up to and including
  its last separator, followed by the `TextureFilename` as written
  (`actors\trinity\trinity.x` + `trinitypantsF.bmp` gives
  `actors\trinity\trinitypantsF.bmp`; `model::directory_of`). A model
  path without a directory gives the texture name alone (the original
  reads uninitialised memory in that case).

### 2.2 Meshes

What D3DX produces from a `Mesh` is, for the engine, an indexed triangle
list of vertices (position, normal, UV) with a material per face, the
bones with their weights and offset matrices, and face adjacency. Luma
builds the same from these children **[ext]** for how D3DX splits
vertices and triangulates:

| Object | Content | Handling |
|--------|---------|----------|
| `Mesh` values | `nVertices`, the positions, `nFaces`, then per face `nIndices` and the indices | Faces of any size; a polygon is cut into a **fan** `(0, i, i+1)`. Front faces are clockwise, as in Direct3D. An index past the vertices is an error. |
| `MeshNormals` | `nNormals`, the normals, `nFaceNormals`, then per face the normal index of each corner | Normals are **per corner**. A vertex used with two different normal indices is **duplicated**: the file's vertices keep their indices (each takes the normal of the first corner that uses it) and the duplicates are appended (`Mesh::original_vertices` says which file vertex each vertex comes from). Without `MeshNormals`, Luma computes smooth normals from the faces (the original's vertex format would simply have none; this is a Luma choice). |
| `MeshTextureCoords` | `nCoords`, then `u, v` per vertex | One UV per **file vertex**, `v` downward as in Direct3D; duplicates copy their original's. Missing UVs are 0. |
| `MeshMaterialList` | `nMaterials`, `nFaceIndexes`, the per-face material indices, then the `Material` objects inline or as `{ references }` | `Mesh::triangle_materials` gives each triangle the index of its polygon. A list shorter than the faces repeats its **last** entry (a single entry means "all faces"); an index past the materials is clamped to the last one. |
| `Material` | `faceColor` (r, g, b, a), `power`, `specularColor`, `emissiveColor`; child `TextureFilename { "file"; }` | Section 2.2.1. |
| `XSkinMeshHeader` (`3CF169CE-FF7C-44ab-93C0-F78F62D172E2`) | `nMaxSkinWeightsPerVertex`, `nMaxSkinWeightsPerFace`, `nBones` | Kept in `Mesh::skin`, informational. |
| `SkinWeights` (`6F0D123B-BAD2-4167-A0D0-80224F25FABB`) | `transformNodeName`, `nWeights`, the vertex indices, the weights, `matrixOffset` (16 floats) | One `Bone` per object, in file order: the frame name, the **bone offset matrix** (inverse bind), and the weights on the **file's vertex indices**; a duplicated vertex gets a copy of its original's weight. After loading, each bone's frame is looked up by exact name in the model (`Bone::frame`); a bone whose frame is missing keeps `None` (the original logs a warning and leaves the pointer unset). |
| `VertexDuplicationIndices` (`B8D65549-D7C9-4995-89CF-53A9A8B031E3`) | `nIndices`, `nOriginalVertices`, the original index of each vertex | Read into `Mesh::duplication` and **not used**: the sample exporters write the weights against the vertices as stored in the file (Trinity: 825 vertices, 825 originals, identity table). |
| `DeclData`, `FVFData`, `MeshVertexColors`, `MeshFaceWraps` | | Ignored. |

A mesh **without materials** gets one grey material (diffuse and
specular `0.5, 0.5, 0.5`, no texture) and all its triangles use it. A mesh
**with bones** is skinned at runtime from the bones' frames, so its own
frame's matrix is not applied to it; a mesh **without bones** is rigid and
follows its frame (see [../3d-actors.md](../3d-actors.md), section 6.3).

The runtime also needs the position-welded face adjacency for shadow
volumes (`GenerateAdjacency(0, ...)`: vertices at identical positions are
one point). It is not computed here; the vertex list keeps enough
(positions and `original_vertices`) to build it.

#### 2.2.1 Materials

What the engine keeps of a `Material` (`C3DMaterial`, `XMesh.cpp`):

- `diffuse` from `faceColor`; the **ambient colour is always the diffuse
  colour** (the engine overwrites it), so Luma does not store it; `power`,
  `specular` and `emissive` as written.
- `texture_filename`: the `TextureFilename` as written;
  `texture_path`: resolved as in 2.1. The texture is loaded by the game's
  ordinary image loader, so the 2D rules apply: the magenta colour key
  `(255, 0, 255)` becomes transparent.
- `name`: the **texture file name without path and extension**
  (`C3DMaterial::SetTexture` with "adopt name"), which is how scripts and
  `EFFECT` blocks address a material (`SetTexture("trinityskin3", ...)`).
  An untextured material has **no name** and cannot be addressed.

### 2.3 Animation sets

An `AnimationSet` holds `Animation` objects, one per driven frame. The set
is named by the file (`AnimationSet walk { ... }`) or after the model file
when unnamed (2.1). Several sets may share a name; `Model::animation_set`
finds the first one, ignoring ASCII case, as `GetAnimationSetByName`.

Each `Animation` has:

- a **frame reference** `{ FrameName }`: the name is kept
  (`Animation::target`) and resolved after loading by exact name
  (`Animation::frame`); a name the model does not have leaves `None` and
  the animation then drives nothing. The original accepts a reference only
  if it resolves to a `Frame` object; Luma also takes a reference that
  resolves to nothing as a frame name (D3DX would reject the file). The
  original's check for a duplicated reference is dead code (it tests a
  pointer that is only set later), so with two references the **last
  wins**, as here;
- `AnimationKey` objects: `keyType`, `nKeys`, then per key `time`,
  `nValues`, the values. A key with another number of values than its type
  needs is an error (`Error::KeySize`); an empty track (`nKeys = 0`,
  written `4; 0; ;` by the sample exporter) is accepted and contributes
  nothing;
- an optional `AnimationOptions { openClosed; positionQuality }`: a
  non-zero `openClosed` sets the **whole set** looping; `positionQuality`
  is ignored.

| `keyType` | Values | Stored as |
|-----------|--------|-----------|
| 0 | 4: **w, x, y, z** | `RotationKey { rotation: Quat(x, y, z, w) }`, as written. |
| 1 | 3 | `Vec3Key` on the scale track. |
| 2 | 3 | `Vec3Key` on the position track. |
| 4 | 16, a row-major matrix | Decomposed by `math::decompose_simple` (translation row, scale as the column lengths, rotation quaternion of the matrix as is) into one key on each of the three tracks; the rotation is stored with its **vector part negated**. |
| other | | Ignored (the original logs and continues). |

Each track may appear **once** per animation; a second rotation, scale or
position track, or a matrix track next to any other track, is an error
(`Error::DuplicateTrack`), as in the original.

### 2.4 Sampling

`Animation::sample(ticks)` is `CXAnimation::Update` without the frame
plumbing. For each track present: find the first key with `time > t`;
pair it with the previous key (`(t - t1) / (t2 - t1)`, `0` when the
times are equal); `t` before the first key gives the first key; if **no
key is later** than `t` both indices stay at 0 and the pose snaps to the
**first** key (not the last; callers keep `t` below the last key). Scale
and position are interpolated linearly, the rotation by `slerp` after
**negating the vector part** of both keys (so a file quaternion `q` turns
the frame by `q` conjugated, and a matrix key, negated on load and again
here, by the matrix's own rotation). A missing track leaves its default:
position `0`, scale `1`, rotation identity, so an animation with only a
rotation track also **moves the frame to the origin** of its parent. The
result is `Transform { position, scale, rotation }` and its matrix is
`Scaling * RotationQuaternion * Translation`. An animation with no track
gives `None`: the frame keeps its loaded matrix.

`AnimationSet::apply(ticks, locals)` writes the sampled matrices of the
set's resolved frames into a list of local matrices indexed like
`Model::frames`; `Model::rest_pose()` gives the loaded matrices
(`ResetMatrices`), `Model::combine(locals)` the model-space matrix of
every frame (`local * parent combined`); `Model::sample(set, ms)` chains
the three. `Transform::blend(other, t)` is the blend of
`CXFrameNode::UpdateMatrices` between the two slots of a transition
(linear for scale and position, `slerp` for the rotation).

Timing (`CXAnimationSet`), all in the original's single precision with
truncation to integers:

- milliseconds to ticks: `ms * (ticks_per_second / 1000)`
  (`Model::ms_to_ticks`);
- the **total time** of a set, in ms: the largest last-key time of any
  track of any animation, divided by `ticks_per_second / 1000`. Note that
  `4800 / 4.8f` is just under 1000 in single precision, so a one-second
  animation at 4800 ticks lasts **999 ms**, in the original as here;
- the **frame time** of a set, in ms: the first animation's shortest key
  gap, then the minimum over the animations whose gap is positive, each
  divided by `ticks_per_second / 1000` and truncated. The gap scan
  (`CXAnimation::GetFrameTime`) counts the gap from time 0 to the first
  key, and a zero gap met after a positive one resets the running minimum,
  which the next gap then replaces. "Frames" exist only to place events:
  `total_frames = total / frame_time + 1` (0 when the frame time is 0);
- an **event** added to a set (`add_event`) has its frame clamped to at
  least 1, then to at most `total_frames`;
  `AnimationSet::events_between(current, previous)` lists the events
  raised by a frame change as `OnFrameChanged` does: after a wrap first
  those with `frame > previous`, then those with `previous < frame <=
  current`.

### 2.5 Merging

`Model::merge_animations(bytes, file_name)` is `MergeFromFile`: it takes
the `AnimationSet` and loose `Animation` objects of another file, at the
top level and **inside its frames** (the frames and meshes themselves are
not added), names unnamed sets after that file, and resolves their frame
references against the model's existing frames. Merging into a model that
has no frames is an error. `Model::remove_animation_set(name)` is
`UnloadAnim` (case-insensitive).
