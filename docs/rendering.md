# Rendering

How Luma Engine draws a frame, as implemented by the `luma-render` crate.
This is a design note for the engine itself, not a description of a
Wintermute format; the formats are in [docs/formats](formats/README.md).

## Goals

- Draw what a 2D Wintermute game draws: textured quads (sprite
  sub-frames) in a fixed order, with alpha blending, tinting, mirroring,
  scaling and scrolling layers. Nothing else is needed for milestone 2.
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
| `Texture` | A decoded `luma_formats::Image` uploaded as `Rgba8Unorm`. One texture per image; no atlas yet. |
| `DrawList` | The quads of one frame, back to front, in *logical* pixels, with a logical size and a scroll offset applied to the quads pushed after `set_offset`. |
| `Renderer` | Owns the pipeline, the sampler, the vertex and index buffers and a bind group per texture; renders a `DrawList` into a `Target`. |
| `Target` | Any texture view with its format and size: the window's swap chain texture or an `Offscreen`. |
| `Offscreen` | A render target whose pixels can be read back as an `Image`. |
| `window::run` | A winit window with a swap chain; calls an `App` for events and for the draw list of every frame. |

## A frame

1. The application fills a `DrawList`: for each layer, set the layer's
   scroll offset, then push the quads of its nodes in order; free objects
   go where the scene's drawing rules put them (see
   [scene.md](formats/scene.md#drawing)).
2. `Renderer::render` builds four vertices per quad (position, texture
   coordinates, colour), uploads them, and records one render pass that
   clears the target and draws the quads with `ALPHA_BLENDING`
   (`src_alpha, one_minus_src_alpha`), in list order. Consecutive quads
   with the same texture are one draw call.
3. The window presents the swap chain texture; an `Offscreen` can be
   read back.

The composition of a scene into a draw list (layers, parallax, free
entities sorted into the floor regions, sprite frames, hotspots, scale and
tint) lives in `luma-adventure` (`SceneView`), not in the renderer.

## Coordinates

The draw list has a logical size, usually the game's resolution
(`1024x768`). An orthographic projection maps `(0,0)` to the top-left
corner and `(width, height)` to the bottom-right one, so sprite positions
from the scene files are used as they are. The projection covers the
whole target, or, with `letterbox`, the largest centred rectangle of the
same aspect ratio; the GPU does the scaling, so HiDPI windows get a sharp
bilinear upscale for free.

Scrolling is the draw list's `offset`, added to the position of every
quad pushed after it (a layer scrolled by `s` pixels uses an offset of
`-s`): the renderer does not know about layers or cameras, the scene code
computes each layer's parallax offset (see [scene.md](formats/scene.md#camera)).

## Colour

Textures are `Rgba8Unorm` and the surface uses a non-sRGB format when
the platform offers one (`Bgra8Unorm` on Metal): blending happens in
gamma space, as it did in Direct3D 8 and SDL, so translucent sprites look
the way the game's artists saw them. Tint and alpha are a per-vertex
colour multiplied with the texel.

Sampling is bilinear with clamping. A quad drawn at its texture's size at
an integer position samples texel centres and is pixel-exact; a scaled
quad is smoothed like the original engine's stretched sprites. The engine
keeps images whole and uses the sub-frame `RECT` as the source
rectangle; the texels just outside a source rectangle bleed into its last
row and column when scaled, which the original engine does as well.

## Not yet

- Texture atlases: one texture per image is enough for the games of the
  era and keeps streaming simple; an atlas can come when draw-call counts
  matter.
- Rotation (`ROTATABLE` entities) and 3D actors.
- Mouse hit testing from pixels (`PRECISE` sprites): the adventure layer
  will do it on the CPU from the decoded alpha channel.
- Render-to-texture effects, lighting, particles.
