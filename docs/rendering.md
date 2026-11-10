# Rendering

How Luma Engine draws a frame, as implemented by the `luma-render` crate.
This is a design note for the engine itself, not a description of a
Wintermute format; the formats are in [docs/formats](formats/README.md).

## Goals

- Draw what a 2D Wintermute game draws: textured quads (sprite
  sub-frames) in a fixed order, with alpha, additive and subtractive
  blending, tinting, mirroring, scaling per axis, rotation and scrolling
  layers. Nothing else is needed for milestone 2.
- Draw the lit, textured meshes of 3D actors *between* those quads, in
  the painter's order, as the original's fixed-function pipeline did
  (see [3D pass](#3d-pass)), without the 2D path paying for it.
- Run on Metal first (macOS, iOS), and on Vulkan, DirectX 12, OpenGL ES and
  WebGPU through [wgpu](https://wgpu.rs) without platform code.
- Be testable without a window: every test renders into a texture and
  reads the pixels back.
- Leave room for the modern features of later milestones (lighting,
  particles, post-processing) without designing them now.

## Pieces

| Type | Role |
|------|------|
| `Gpu` | Instance, adapter, device and queue. `Gpu::headless()` for tools and tests, `Gpu::with_surface()` for a window. |
| `Texture` | A decoded `luma_formats::Image` uploaded as `Rgba8Unorm`. One texture per image; no atlas yet. `Renderer::mesh_texture` uploads it with a mipmap chain for meshes. |
| `DrawList` | The quads of one frame, back to front, in *logical* pixels, with a logical size and a scroll offset applied to the quads pushed after `set_offset`; each quad has `Attributes` (rotation around a pivot, blend mode), the default ones unless pushed with `push_with`. `push_mesh` puts a `MeshDraw` at the current place in the order. |
| `MeshHandle`, `MeshVertex`, `MeshDraw`, `Light`, `Lights`, `MeshMaterial`, `Fog` | A mesh uploaded once (`Renderer::upload_mesh`, `update_mesh` for skinning) and one draw of it: matrices, texture, material, lights, fog, culling, blend mode. |
| `Renderer` | Owns the pipelines (quads: one per target format and blend mode; meshes: per format, blend mode and culling), the samplers, the vertex and index buffers, the uploaded meshes, the depth and stencil buffer and a bind group per texture; renders a `DrawList` into a `Target`. |
| `Target` | Any texture view with its format and size: the window's swap chain texture or an `Offscreen`. The renderer keeps a depth and stencil buffer of the target's size. |
| `Offscreen` | A render target whose pixels can be read back as an `Image`. |
| `window::run` | A winit window with a swap chain; calls an `App` for events and for the draw list of every frame. |

## A frame

1. The application fills a `DrawList`: for each layer, set the layer's
   scroll offset, then push the quads of its nodes in order; free objects
   go where the scene's drawing rules put them (see
   [scene.md](formats/scene.md#drawing)).
2. `Renderer::render` builds four vertices per quad (position, texture
   coordinates, colour), turned around the pivot when the quad's
   attributes rotate it, uploads them with the uniforms of every mesh
   draw, and records one render pass that clears the target (colour,
   depth and stencil) and draws the list in order: the quads with the
   pipeline of each quad's blend mode (see [Transforms](#transforms)),
   the meshes where they were pushed (see [3D pass](#3d-pass)).
   Consecutive quads with the same texture and blend mode are one draw
   call; a mesh between them ends the batch.
3. The window presents the swap chain texture, unless the application
   answered `Flow::Skip` for this frame, which keeps the previous one on
   screen; an `Offscreen` can be read back.

The composition of a scene into a draw list (layers, parallax, free
entities sorted into the floor regions, sprite frames, hotspots, scale and
tint) lives in `luma-adventure` (`SceneView`), not in the renderer.

## Coordinates

The draw list has a logical size, usually the game's resolution
(`1024x768`). An orthographic projection maps `(0,0)` to the top-left
corner and `(width, height)` to the bottom-right one, so sprite positions
from the scene files are used as they are. The projection covers the
whole target, or, with `letterbox`, the largest centred rectangle of the
same aspect ratio.

A letterboxed list whose size is not the target's is drawn in two steps:
the frame is rendered into a texture of the list's own size, as the game
was made to be seen (the original engine drew at the game's resolution
and let the display stretch it), then that texture is scaled onto the
letterbox rectangle (`blit.wgsl`). The scaling keeps the game's pixels
sharp: each output pixel takes one source pixel, and only the output
pixels that straddle two source pixels blend them. At a whole multiple
of the resolution this is exact nearest-neighbour scaling; at any other
scale the pixels stay crisp without the uneven rows of plain nearest
sampling. Scaled sprites, text and 3D actors are therefore smoothed at
the game's resolution, as in the original, not at the window's.

Scrolling is the draw list's `offset`, added to the position of every
quad pushed after it (a layer scrolled by `s` pixels uses an offset of
`-s`): the renderer does not know about layers or cameras, the scene code
computes each layer's parallax offset (see [scene.md](formats/scene.md#camera)).

## Transforms

A sprite's sub-frame is placed with its hotspot at the owner's position
and scaled around it, by a separate factor on each axis when the object
has a `ScaleX` or `ScaleY` (`CAdObject::GetScale`); the adventure layer
computes the destination rectangle, with the engine's integer
truncation, and the renderer draws it as it is.

Rotation is a per-quad attribute: an angle in degrees and a pivot in
logical pixels. The renderer turns the four corners of the destination
rectangle around the pivot before projecting them, exactly as
`CBRenderD3D::TransformVertices` does (`x' = px + dx cos θ - dy sin θ`,
`y' = py + dx sin θ + dy cos θ`), so a positive angle turns clockwise on
a screen whose `y` grows downwards. The pivot of a sprite is where its
hotspot is, the object's position (`CBSubFrame::Draw` passes the hotspot
as the centre to `DisplayTransform`); the draw list's offset moves the
pivot with the quad. Mirroring stays in the texture coordinates, so a
mirrored sprite turns the same way. `rotated_corners` is public so that
the adventure layer can hit-test with the same corners.

Blend modes are pipeline states, the factors `CBRenderD3D::
SetSpriteBlendMode` sets for the colour channels:

| Mode (`BlendMode`) | Source factor | Destination factor | Effect |
|--------------------|---------------|--------------------|--------|
| `Normal` (0) | `src_alpha` | `one_minus_src_alpha` | Alpha blending. |
| `Additive` (1) | `src_alpha` | `one` | Adds the sprite to what is under it (glows, light). |
| `Subtractive` (2) | `zero` | `one_minus_src_color` | Keeps `1 - src` of what is under it (shadows, smoke). |

Direct3D 8 used the same factors for the alpha channel, which the screen
ignores; Luma keeps the target's alpha covered in the additive and
subtractive modes (`one, one` and `zero, one`), so that a screenshot of a
glow or a shadow is not transparent there.

## Colour

Textures are `Rgba8Unorm` and the surface uses a non-sRGB format when
the platform offers one (`Bgra8Unorm` on Metal): blending happens in
gamma space, as it did in Direct3D 8 and SDL, so translucent sprites look
the way the game's artists saw them. Tint and alpha are a per-vertex
colour multiplied with the texel.

Sampling is bilinear with clamping, and textures are not premultiplied:
the transparent pixels of a colour-keyed image carry the colour of their
opaque neighbours (see [sprite.md](formats/sprite.md#images)), so that a
scaled sprite's edge fades out instead of showing the key. A quad drawn at its texture's size at
an integer position samples texel centres and is pixel-exact; a scaled
quad is smoothed like the original engine's stretched sprites. The engine
keeps images whole and uses the sub-frame `RECT` as the source
rectangle; the texels just outside a source rectangle bleed into its last
row and column when scaled, which the original engine does as well.

## 3D pass

The 3D actors of a 2.5D scene are drawn in the middle of the 2D order
(after the layers behind them, before the ones in front; see
[3d-actors.md](3d-actors.md), section 10.2). The renderer therefore
has no separate 3D stage: a `MeshDraw` is an item of the `DrawList`
like a quad, pushed with `push_mesh` and drawn at that place. The
state it is drawn with is the one `CBRenderD3D::Setup3D` set up in the
original, so that a model lit by the fixed-function pipeline in 2003
looks the same here.

**Depth and stencil.** Every target gets a `Depth24PlusStencil8` buffer
of its size, owned by the renderer and recreated when the size changes
(a window resize). It is cleared with the colour, once per frame
(`CBRenderD3D::Fill` clears target, depth and stencil together).
Quads are drawn with the depth test off and no depth writes
(`Setup2D`: `ZENABLE` false), so 2D drawing never touches the buffer;
meshes test `<=` and write (`Setup3D`), unless the draw turns the
writes off (`depth_write`, for the shadows laid on the floor). The
depth therefore only orders 3D things among themselves in a frame: an
actor behind another, the shadow-receiving geometry; a 2D quad drawn
after a mesh covers it regardless of depth, as a foreground layer must.
The stencil is cleared to zero for the shadow items below.

**Meshes.** `Renderer::upload_mesh(vertices, indices)` returns a
`MeshHandle`; `MeshVertex` is position, normal and one UV in model
space (`D3DFVF_XYZ | NORMAL | TEX1`), indices are `u32`, three per
triangle, **front faces clockwise on screen** (the Direct3D convention,
`wgpu::FrontFace::Cw`). `update_mesh(handle, vertices)` replaces the
vertices (the CPU-skinned pose of this frame), growing the buffer when
there are more; `free_mesh` releases it. A `MeshDraw` names the handle
and, optionally, an index range (a material's subset of the mesh), the
model-to-world `Matrix`, the world-to-clip `Matrix` (the camera's view
times its projection), the texture or none, the `MeshMaterial`, the
ambient colour, up to `MAX_LIGHTS` (8) `Light`s, optional `Fog`, whether
to cull back faces and the blend mode. Matrices are plain
`[[f32; 4]; 4]` in column-major order, as a WGSL `mat4x4<f32>` reads
them; `luma_3d::math::Mat4::to_cols_array` produces the sixteen floats
from a row-vector Direct3D matrix and `luma_render::matrix` groups them.
Direct3D's clip space (depth `0..1`, `y` up, `w` the view depth for a
perspective projection) is wgpu's, so the matrices of
`D3DXMatrixLookAtLH` and `D3DXMatrixPerspectiveFovLH` are handed over
as they are. Meshes use the quads' viewport, letterboxed or not.

**Lighting** is per vertex (Gouraud), in the vertex shader `mesh.wgsl`,
the fixed-function model with the values Wintermute set:

```
lit = emissive + ambient * diffuse + sum over lights (color * diffuse * max(n . l, 0) * cone)
```

- `diffuse` and `emissive` are the material's (`.X` `faceColor`,
  `emissiveColor`); the material's ambient colour *is* its diffuse one
  (`CXMesh` sets `Ambient = Diffuse`), and the lights carry no ambient or
  specular colour (`C3DLight::SetLight` zeroes them), so the specular
  colour and power of the material play no role although
  `SPECULARENABLE` was on. A mesh without materials is grey `0.5`.
- `ambient` is the scene's (`D3DRS_AMBIENT`), black by default: a model
  in a scene without lights is black.
- Normals are transformed by the upper 3x3 of the world matrix and
  normalised (`NORMALIZENORMALS`); an actor's world matrix is a rotation,
  a uniform scale and a translation, for which this is exact.
- A `Light::Point` lights everything within `range` with **no distance
  attenuation** (`Attenuation0 = 1`, the others `0`; the original's range
  is `100000`); beyond the range it contributes nothing, a hard cut.
- A `Light::Spot` adds the cone: with `rho` the cosine between the axis
  and the direction to the vertex, full inside the inner cone (`rho >
  cos(inner_angle / 2)`), nothing outside the outer one (`rho <=
  cos(outer_angle / 2)`), `((rho - cos_outer) / (cos_inner -
  cos_outer)) ^ falloff` between. The original's spots have `inner_angle
  = 0.5`, `outer_angle = 1.0` radians, `falloff = 1` and `range = |target
  - position|`.
- A `Light::Directional` has no position or range; the original never
  creates one, it is there for tools.
- The result is clamped to `0..1` per channel before the texture
  multiplies it. Light colours arrive as floats; the original divides
  the `0..255` channels by **256**, the caller does the same.

Which lights a draw gets (the nearest `MaxLights` to the object, the
ignored names) is the scene's business (`AdSceneGeometry::EnableLights`),
not the renderer's: `luma-3d` or the adventure module fills the
`Lights`.

**Texturing.** Stage 0 as `Setup3D` set it: colour = texture *
lit colour (`MODULATE`), alpha = texture alpha **only** (`SELECTARG1`),
so the material's alpha does not show through a texture; without a
texture the lit colour and the material's alpha are used (the white
texture is bound, so the shader is the same). The alpha test discards
fragments below `8/255` (`ALPHAREF 8`, `GREATEREQUAL`); the magenta
colour key is already alpha `0` in the decoded image, so keyed texels
are cut out and the blend mode then composites the rest. Mesh textures
are sampled with repeat addressing and trilinear filtering, the Direct3D
defaults with `MIPFILTER LINEAR`; `Renderer::mesh_texture` builds the
mipmap chain on the CPU (a box filter whose colours are weighted by
alpha, so that transparent texels do not bleed a fringe into the smaller
levels; `mip_level` is public for tests). 2D textures have no mipmaps
and the 2D sampler clamps, as before.

**Fog** is linear on the view depth (`D3DFOG_LINEAR` vertex fog):
`f = clamp((end - depth) / (end - start), 0, 1)` per vertex, the
depth being the clip `w`, interpolated and applied after texturing as
`mix(fog_color, color, f)`.

**Blending and culling.** The blend mode of a mesh draw uses the same
pipeline states as the quads (an actor's `BlendMode` property). Back
faces (counter-clockwise on screen) are culled only when the draw asks,
as `CXModel::Render` sets `D3DCULL_CCW` only when `DrawBackfaces` is
false; by default both sides are drawn.

**Uniforms.** Each mesh draw has a 720-byte slot (two matrices, five
`vec4`s, eight lights of four `vec4`s) in one uniform buffer, bound with
a dynamic offset at the device's alignment (256 bytes); the buffer grows
with the number of draws in a frame. The position-only draws below
share a second buffer of 80-byte slots (one matrix and a colour).

### Shadow items

The stencil shadows of the 3D actors ([3d-actors.md](3d-actors.md),
section 10.4) need three more kinds of item, all 3D items of the
`DrawList` like the meshes (`Draw3d`, `push_3d`, and the helpers
`push_shadow_volume`, `push_depth_only`, `push_stencil_fill`), drawn at
their place in the order with a position-only shader (`shadow.wgsl`).
The renderer knows nothing of actors: it counts triangles into the
stencil and fills where the count says.

- **`ShadowVolumeDraw`** (`C3DShadowVolume::RenderToStencilBuffer`):
  triangles in model space (`Vec<[f32; 3]>`, built by the caller every
  frame), a world and a view-projection matrix. Drawn twice with the
  depth test `<=` against the depth buffer as it is and no depth or
  colour writes: once culling the counter-clockwise faces with stencil
  `IncrementWrap` on depth pass, once culling the clockwise ones with
  `DecrementWrap` (`D3DSTENCILOP_INCR` and `DECR` wrap too). The quads
  `C3DShadowVolume::AddMesh` builds face **inward**, so the side of the
  volume away from the camera counts up and the near side counts down:
  a pixel of a surface already in the depth buffer and inside the
  volume ends at 255 (the far side fails the depth test, the near side
  passes and wraps), a pixel with nothing inside the volume at zero
  (both pass), and the fill's `1 <= stencil` accepts the first. A
  volume wound the other way counts 1 and 0 instead; the test is the
  same. The vertices of a frame's volumes are concatenated into one
  dynamic vertex buffer.
- **`DepthOnlyDraw`** (`CAdSceneGeometry::RenderShadowGeometry`): an
  uploaded mesh written to the depth buffer only (colour writes off,
  depth writes on, test `<=`), its counter-clockwise faces culled when
  asked, as the original sets `D3DCULL_CCW` for the receivers. The
  shadow receivers of a scene go in before the actors so that the
  volumes are tested against them; a receiver also hides the meshes
  drawn later behind it, which authors use on purpose.
- **`StencilFill`** (`RenderToScene`): a viewport-wide triangle of one
  colour, alpha-blended, no depth test, drawn where `1 <= stencil`
  (`D3DCMP_LESSEQUAL` with reference 1), whose stencil pass operation
  is `Zero`: the stencil is cleared where the fill passed, as the
  original clears it after each actor, without a clear call.

Two options of `MeshDraw` serve the other shadow types: `depth_write`
(off for the floor shadows, `ZWRITEENABLE` false in `C3DShadowGen`) and
`stencil_once: Option<u8>`, which draws a fragment only where the
stencil differs from the reference and sets it to the reference
(`NotEqual` / `Replace`), so that the overlapping triangles of a
translucent silhouette darken the floor once (the flat shadows).

Not in the renderer: the geometry debug overlay (`Scene.ShowGeometry`,
wireframe in the original), which needs a line pipeline.

## Not yet

- Texture atlases: one texture per image is enough for the games of the
  era and keeps streaming simple; an atlas can come when draw-call counts
  matter.
- The geometry overlay of 3D scenes (see above).
- Render-to-texture effects. Particles are quads pushed by
  `luma-adventure` with their rotation and their owner's blend mode (see
  [particles.md](particles.md)); they need no renderer feature of their
  own.
