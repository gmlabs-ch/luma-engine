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

Luma reads the four formats into the same object tree (1.2 for the text
syntax, 1.4 for the binary tokens, 1.5 for the compressed container), so
the model loader does not know which one a file came in; `XFile::header`
records it. The float size only matters to the binary forms, where it is
the size of each entry of a float list; a text file's numbers are read as
written. Any other format tag is rejected
(`xfile::ErrorKind::UnsupportedFormat`). Every sample model is
`xof 0303txt 0032`.

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

A **data object** is `Template [name] [<guid>] { body }`. The DirectX
grammar puts the optional GUID first **inside** the braces (`Template
[name] { [<guid>] body }`); the sample files write it before the opening
brace, and Luma accepts both places. The body is a sequence of:

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

- Template member lists, so a file whose data section disagrees with its
  own template declarations is read as written, not rejected.
- The `[...]` open/closed restrictions on which children an object may
  have: any object may nest any other.
- In a binary file, the type keywords of a template body (1.4) are
  decoded and dropped with the rest of the body.

### 1.4 Binary form

A `bin ` file (`xof 0302bin 0032` or `0064`) follows the header with a
stream of **16-bit little-endian tokens**, from the public DirectX
documentation. Six tokens carry a record; the others stand alone:

| Code | Token | Record |
|------|-------|--------|
| 1 | `TOKEN_NAME` | `DWORD count`, `count` bytes: a template or object name |
| 2 | `TOKEN_STRING` | `DWORD count`, `count` bytes, then the terminating `TOKEN_SEMICOLON` or `TOKEN_COMMA` |
| 3 | `TOKEN_INTEGER` | `DWORD value` |
| 5 | `TOKEN_GUID` | `DWORD data1`, `WORD data2`, `WORD data3`, `BYTE data4[8]` |
| 6 | `TOKEN_INTEGER_LIST` | `DWORD count`, `count` DWORDs |
| 7 | `TOKEN_FLOAT_LIST` | `DWORD count`, `count` floats of the header's float size (32 or 64 bits) |
| 10, 11 | `TOKEN_OBRACE`, `TOKEN_CBRACE` | |
| 12, 13 | `TOKEN_OPAREN`, `TOKEN_CPAREN` | |
| 14, 15 | `TOKEN_OBRACKET`, `TOKEN_CBRACKET` | |
| 16, 17 | `TOKEN_OANGLE`, `TOKEN_CANGLE` | |
| 18 | `TOKEN_DOT` | |
| 19, 20 | `TOKEN_COMMA`, `TOKEN_SEMICOLON` | |
| 31 | `TOKEN_TEMPLATE` | |
| 40..52 | `TOKEN_WORD`, `DWORD`, `FLOAT`, `DOUBLE`, `CHAR`, `UCHAR`, `SWORD`, `SDWORD`, `VOID`, `LPSTR`, `UNICODE`, `CSTRING`, `TOKEN_ARRAY` | the type keywords of template members |

Any other code is an error (`UnexpectedToken` naming the code and its
offset); a record cut by the end of the file is `UnexpectedEnd`.

The data section has the same shape as in text and is read the same way
(`luma_3d::xfile_binary`, a private module behind `XFile::parse`):

- a **template** is `TOKEN_TEMPLATE`, its name, `TOKEN_OBRACE`, the GUID,
  the members and restrictions, `TOKEN_CBRACE`. Luma records the name and
  the first GUID and skips the rest token by token (the records carry
  their lengths, so no byte-level guessing is needed) to the matching
  brace;
- a **data object** is the template name, an optional object name, an
  optional GUID before or right after `TOKEN_OBRACE` (the binary grammar
  puts the class GUID after the brace), then integers (`TOKEN_INTEGER`
  or `TOKEN_INTEGER_LIST`), floats (`TOKEN_FLOAT_LIST`), strings, nested
  objects and references, to `TOKEN_CBRACE`. Separators are skipped. The
  lists are flattened: an object's `values` are the same flat list as the
  text form produces, a DWORD becoming `Value::Int` (unsigned: a binary
  writer stores a negative `SDWORD` member as its 32-bit two's complement
  and Luma does not know the member type) and a float `Value::Float`,
  widened from 32 bits when the header says `0032`. A `Matrix4x4` written
  with integral entries is therefore `Int` in a text file that writes `1`
  and `Float` in a binary file, which the typed loaders accept alike
  (`Values::float` takes both);
- a **reference** is `TOKEN_OBRACE`, a name and/or a GUID,
  `TOKEN_CBRACE`.

A binary GUID is rendered in the text form, lowercase, 8-4-4-4-12 hex
digits, so templates and references compare by the same string whatever
the file form. Objects of a binary file have `line = 0`: errors of the
typed loaders name the object's template without a line.

What was verified: the token codes and record layouts are those of the
public DirectX documentation, and a binary token stream of the two-bone
quad used by the model tests reads to exactly the tree of its text twin
(and to the same `Model`). What was not: no binary `.x` file from a real
game was available (the local test games are 2D and the Wintermute demo
models are text), so the reader has not met a file written by the
DirectX SDK. The opt-in test
`xfile_binary::tests::reads_the_local_binary_models_when_present` parses
every binary or compressed `.x` under `testdata/local/` and skips when
there is none; the first real file will confirm or correct this section.

### 1.5 Compressed container (MSZIP)

A `tzip` or `bzip` file keeps the 16-byte header in clear; the rest is
**MSZIP**, the cabinet compression Microsoft's tools use: a sequence of
blocks of at most 32 KiB of data, each a raw **deflate** stream preceded
by the two-byte signature `CK`. Luma reads, after the header:

| Bytes | Content |
|-------|---------|
| 0..4 | `DWORD`: total uncompressed size |
| then, per block | `WORD` size, `CK`, a raw deflate stream |

The decompressed data is the data section in the form the tag names
(`tzip`: text, 1.2; `bzip`: binary tokens, 1.4), read as if it had
followed the header directly; lines in a `tzip` file count from the
start of the decompressed text. Should the decompressed data start with
its own `xof ` header (a writer that compressed the whole file), that
inner header names the form and is skipped; it must not be compressed
itself.

Inflation uses `flate2` (`Decompress::new(false)`, the raw deflate
decoder). Each block's stream ends with its own final deflate block, so
its compressed length does not need to be known in advance: the decoder
reports where the stream ended and the next block starts there. The
**history window** carries across blocks as MSZIP allows: the last 32
KiB of output so far are fed to the decoder of the next block as a
dictionary (as a non-final stored deflate block, since `flate2`'s Rust
back end has no preset-dictionary call), so a writer that compressed the
blocks as one continuous stream and one that reset its compressor on each
block are both read.

What could and could not be verified, with no compressed `.x` file at
hand (the layout above is from the public description of MSZIP and of
the `.X` compressed variants, and from the `tzip`/`bzip` fixtures the
tests build with `flate2`):

- the **size word** of a block is believed to be the block's
  **uncompressed** size. Luma accepts it as that, or as the compressed
  size with or without the signature, and also accepts **two size words**
  before `CK` (uncompressed then compressed, the cabinet layout); a word
  matching none of these is an error (`BadCompression`), as is a missing
  signature or a stream that does not inflate;
- the **total size** after the header is believed to count the
  decompressed data, perhaps with the 16 header bytes; it is used only to
  pre-size the output buffer (capped at 256 MiB) and never checked;
- whether real writers carry the deflate history across blocks is
  unknown; both cases are read;
- trailing bytes too short to be a block are an `UnexpectedEnd`
  (a truncated file), not padding.

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

**What a failing object does** (`CXFrameNode::LoadFromXData`). At the
top level the first object that fails fails the file. Inside a `Frame`
the original keeps the result of the **last** object only, and starts
from "failed" when the frame holds no object at all, so:

- an object that fails (a mesh whose face names a missing vertex, an
  animation set with a duplicated track) is left out alone when another
  object follows it in the frame;
- a frame whose last object fails, and a frame with **no object at all**
  (`Frame x { }`), is dropped with the frames and meshes under it, and
  fails in turn: its parent drops it and goes on, unless it was the
  parent's last object, and so on up to the top level, where it fails the
  file (`Error::EmptyFrame` for an empty frame);
- the animation sets and the tick rate read inside a dropped frame stay:
  they belong to the model, not to the frame.

Luma reproduces this (`Model::load_object`): a model the original loads
has the same frames and meshes here, and `FindFrame` misses the same
names. Every failure of an object is treated this way, including one
D3DX would have refused when it parsed the file (an object with too few
values): Luma then loads a file the original rejects.

A **data reference** among the objects of a frame (`{ Name }`) counts as
an object that loaded and is not followed. This point is **[ext]**: the
Direct3D 9 build hands the reference to the same loader, which asks D3DX
for the type and data of the object referred to, and the Direct3D 8
build skips references (and fails the frame when one is its last
object); no sample file has one.

**Mesh order.** `Model::meshes` lists the meshes in the order the
original walks them when it updates, draws and builds shadow volumes
(`CXFrameNode::Render`): a frame's own meshes, then its child frames in
file order. A mesh written after a child frame in the file therefore
comes before that child frame's meshes. `Frame::meshes` and
`Mesh::frame` index accordingly.

**Bounds.** Objects may nest 256 deep (a skeleton is a chain of a few
dozen frames), every list is allocated from the values the object really
has, not from the count it announces, and a bone cannot weight more
vertices than the mesh has (`Error::TooManyWeights`).

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
  resolves to nothing in the file as a frame name, which is what a file
  of animations alone, merged into a model, needs (**[ext]**: whether
  the DirectX file library accepts a reference to an object the file
  does not define is not documented). The
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
(linear for scale and position, `slerp` for the rotation). The channels,
transitions, events and the CPU skinning that drive these at run time are
`luma_3d::animation`, specified in [../3d-actors.md](../3d-actors.md),
section 7.

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
has no frames is an error.

A frame of the merged file follows the rule of section 2
(`CXFrameNode::MergeFromXData`): it answers what its last object
answered, and fails when it is empty. The first top-level object that
fails ends the merge with an error; the sets read before it **stay in the
model, bound to no frame** (they play and move nothing), because the
original looks the bones up only after a merge that succeeded. A later
merge that succeeds binds every set of the model, those included.

`Model::remove_animation_set(name)` is `CXModel::UnloadAnimation`
(case-insensitive). When several sets share the name, the original's
loop steps over the **two sets that follow each removed one**
(`Model::animation_sets_to_unload`), so of three such sets in a row only
the first is removed by one call.

### 2.6 A model with a parent (the shadow model)

`CXModel::LoadFromFile(file, ParentModel)` loads a second model whose
bones are those of another: the actor's `SHADOW_MODEL`, a low-polygon
stand-in that casts the shadows. `Model::parse_with_parent(bytes,
file_name, &parent)` is that load: the file is read as any model, then
the frame of every mesh bone and of every animation target is looked up
by name in the **parent's** frames instead of its own (`Bone::frame` and
`Animation::frame` then index `parent.frames`). The file's own frames
are kept and only place its meshes without bones.

At run time (`luma_3d::animation::ShadowModel`, `update(&parent)` after
the parent's own update, as `CAdActor3DX::Update` calls
`m_ShadowModel->Update()` right after the model's):

- a skinned mesh is posed with `offset * the parent's combined matrix`
  of each bone, so it follows the parent's animation;
- a mesh without bones is moved by the combined matrix of its own frame
  **at rest**: nothing animates the shadow model's own frames;
- a bone the parent has no frame for uses the identity (the original
  reads a pointer it never set).

## 3. What is inferred

The engine leaves these to Direct3D (`D3DXLoadSkinMeshFromXof`,
`ID3DXSkinInfo::UpdateSkinnedMesh`, the DirectX file library), whose
code is not available. Luma's behaviour there comes from the public
description of the format and from what the sample files need:

- the container: text, binary and MSZIP forms (section 1);
- templates matched by name instead of by GUID, and template
  declarations skipped (1.3);
- polygons cut into fans, vertices duplicated where the normal of a
  corner differs, duplicates carrying the UV and weights of their
  original (2.2);
- smooth normals computed for a mesh without `MeshNormals` (2.2);
- a material list shorter than the faces repeating its last entry, an
  index past the materials clamped (2.2);
- a face or a weight naming a vertex the mesh does not have, and a
  normal list that does not match the faces, failing the mesh (2.2);
- `VertexDuplicationIndices` read and not used (2.2);
- skinning: a vertex no bone weights keeps its loaded position, and the
  normals are transformed by the bone matrices themselves and then
  normalised (D3DX takes inverse transposes when it is given them; the
  engine gives none) ([../3d-actors.md](../3d-actors.md), 6.3);
- a reference among the objects of a frame not followed (section 2);
- an animation's reference to a frame the file does not define taken as
  a frame name (2.3);
- an object with too few values failing alone instead of failing the
  file at parse time (section 2).

A missing texture file is not part of the format: the original then uses
its `invalid.bmp` placeholder (`CBSurfaceStorage::AddSurface`), which is
the business of whoever loads the texture.
