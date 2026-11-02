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

To be written with task 3.
