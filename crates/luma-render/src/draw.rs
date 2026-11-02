//! The draw list: what to draw this frame, in order: textured quads and,
//! between them, lit 3D meshes.

use std::ops::Range;

pub use luma_formats::BlendMode;
use luma_formats::Rgba;

use crate::texture::Texture;

/// A 4x4 matrix as a column-vector GPU shader reads it: column-major,
/// `m[c][r]`, the translation in `m[3]`. A row-vector matrix in the
/// Direct3D convention (`v' = v * M`) becomes one by taking its rows as
/// the columns, which is what `luma_3d::math::Mat4::to_cols_array`
/// does; [`matrix`] regroups those sixteen floats.
pub type Matrix = [[f32; 4]; 4];

/// Groups the sixteen floats of a column-major matrix into a [`Matrix`].
pub fn matrix(cols: [f32; 16]) -> Matrix {
    let mut m = [[0.0; 4]; 4];
    for (c, column) in m.iter_mut().enumerate() {
        column.copy_from_slice(&cols[c * 4..c * 4 + 4]);
    }
    m
}

/// The identity [`Matrix`].
pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// The most lights one mesh draw uses, the Direct3D fixed-function limit
/// (`GetMaxActiveLights`, 8 on every card of the era).
pub const MAX_LIGHTS: usize = 8;

/// A mesh uploaded with [`Renderer::upload_mesh`](crate::Renderer::upload_mesh).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshHandle(pub(crate) u64);

/// One vertex of a mesh: position, normal and texture coordinates
/// (`D3DFVF_XYZ | NORMAL | TEX1`), in model space.
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct MeshVertex {
    /// Position.
    pub position: [f32; 3],
    /// Normal; normalised by the shader after the world transform.
    pub normal: [f32; 3],
    /// Texture coordinates, `v` downward.
    pub uv: [f32; 2],
}

/// A light of the fixed-function model: its colour is multiplied with
/// the material's diffuse colour and the cosine of the angle between the
/// normal and the direction to the light; no distance attenuation, as
/// `C3DLight::SetLight` sets `Attenuation0 = 1` and nothing else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Light {
    /// Light from one direction, everywhere.
    Directional {
        /// The direction the light travels in; any length.
        direction: [f32; 3],
        /// Colour.
        color: [f32; 3],
    },
    /// Light from a point, in every direction, up to `range`.
    Point {
        /// Position in world units.
        position: [f32; 3],
        /// Beyond this distance the light has no effect (`D3DLIGHT::Range`).
        range: f32,
        /// Colour.
        color: [f32; 3],
    },
    /// Light from a point inside a cone.
    Spot {
        /// Position in world units.
        position: [f32; 3],
        /// The axis of the cone, from the light; any length.
        direction: [f32; 3],
        /// Beyond this distance the light has no effect.
        range: f32,
        /// Full angle of the inner cone, radians (`Theta`): full
        /// intensity inside.
        inner_angle: f32,
        /// Full angle of the outer cone, radians (`Phi`): nothing
        /// outside.
        outer_angle: f32,
        /// Exponent of the fade between the cones (`Falloff`).
        falloff: f32,
        /// Colour.
        color: [f32; 3],
    },
}

/// The lights of one mesh draw, at most [`MAX_LIGHTS`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lights {
    lights: [Light; MAX_LIGHTS],
    count: usize,
}

impl Default for Lights {
    fn default() -> Self {
        Lights::NONE
    }
}

impl Lights {
    /// No light: a mesh shows its emissive colour and the ambient term.
    pub const NONE: Lights = Lights {
        lights: [Light::Directional {
            direction: [0.0, 0.0, 1.0],
            color: [0.0, 0.0, 0.0],
        }; MAX_LIGHTS],
        count: 0,
    };

    /// The first [`MAX_LIGHTS`] of a slice.
    pub fn from_slice(lights: &[Light]) -> Lights {
        let mut out = Lights::NONE;
        for light in lights {
            if !out.push(*light) {
                break;
            }
        }
        out
    }

    /// Adds a light; `false` when the list is full.
    pub fn push(&mut self, light: Light) -> bool {
        if self.count == MAX_LIGHTS {
            return false;
        }
        self.lights[self.count] = light;
        self.count += 1;
        true
    }

    /// The lights, in order.
    pub fn as_slice(&self) -> &[Light] {
        &self.lights[..self.count]
    }

    /// How many lights there are.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether there is no light.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// The colours of a material (`D3DMATERIAL`), as the `.X` file gives
/// them. The ambient colour is the diffuse one (`CXMesh` sets
/// `Ambient = Diffuse`) and the lights have no specular part, so the
/// specular colour and power play no role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshMaterial {
    /// Diffuse colour and alpha; the alpha is used only when there is no
    /// texture.
    pub diffuse: [f32; 4],
    /// Emissive colour, added unlit.
    pub emissive: [f32; 3],
}

impl Default for MeshMaterial {
    /// The grey of a mesh without materials: diffuse `0.5`, opaque, no
    /// emission.
    fn default() -> Self {
        MeshMaterial {
            diffuse: [0.5, 0.5, 0.5, 1.0],
            emissive: [0.0, 0.0, 0.0],
        }
    }
}

/// Linear fog on the view depth (`D3DFOG_LINEAR` vertex fog): a vertex
/// at depth `start` or nearer keeps its colour, one at `end` or farther
/// is the fog colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
    /// Fog colour.
    pub color: [f32; 3],
    /// Depth where the fog starts, in world units along the view axis.
    pub start: f32,
    /// Depth where the fog is complete.
    pub end: f32,
}

/// One draw of a mesh (or of one material's part of it), lit and
/// textured. Pushed between quads with [`DrawList::push_mesh`], it is
/// drawn at that place in the order, with the depth test against the
/// meshes drawn before it in the frame.
#[derive(Debug, Clone)]
pub struct MeshDraw<'a> {
    /// The mesh.
    pub mesh: MeshHandle,
    /// The indices to draw, a material's subset; `None` for the whole
    /// mesh.
    pub indices: Option<Range<u32>>,
    /// Model to world.
    pub world: Matrix,
    /// World to clip space: the camera's view times its projection
    /// (`D3DXMatrixLookAtLH`, `D3DXMatrixPerspectiveFovLH`: depth `0..1`,
    /// `w` the view depth).
    pub view_projection: Matrix,
    /// The texture, sampled with repeat addressing and trilinear
    /// filtering; `None` draws the lit material colour.
    pub texture: Option<&'a Texture>,
    /// The material.
    pub material: MeshMaterial,
    /// The scene's ambient colour (black by default in the original).
    pub ambient: [f32; 3],
    /// The lights.
    pub lights: Lights,
    /// Fog, when the scene has some.
    pub fog: Option<Fog>,
    /// Cull the back faces (counter-clockwise on screen); the original
    /// draws both sides unless `DrawBackfaces` is false.
    pub cull_backfaces: bool,
    /// How the mesh is blended over what is under it.
    pub blend: BlendMode,
}

impl<'a> MeshDraw<'a> {
    /// A whole mesh with the default material, no texture, no light, no
    /// fog, both faces, alpha blending, at the given matrices.
    pub fn new(mesh: MeshHandle, world: Matrix, view_projection: Matrix) -> MeshDraw<'a> {
        MeshDraw {
            mesh,
            indices: None,
            world,
            view_projection,
            texture: None,
            material: MeshMaterial::default(),
            ambient: [0.0, 0.0, 0.0],
            lights: Lights::NONE,
            fog: None,
            cull_backfaces: false,
            blend: BlendMode::Normal,
        }
    }
}

/// A mesh draw and its place in the order: it is drawn before the quad
/// with this index.
#[derive(Debug, Clone)]
pub struct PlacedMesh<'a> {
    /// Index of the first quad drawn after the mesh (the number of quads
    /// pushed before it).
    pub before_quad: usize,
    /// The draw.
    pub draw: MeshDraw<'a>,
}

/// An axis-aligned rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle from its position and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
}

/// One textured quad.
#[derive(Debug, Clone, Copy)]
pub struct Quad<'a> {
    /// The texture.
    pub texture: &'a Texture,
    /// Where to draw, in logical pixels, after the current offset.
    pub dst: Rect,
    /// Part of the texture to draw, in texels; `None` for the whole
    /// texture.
    pub src: Option<Rect>,
    /// Colour multiplied with the texels; opaque white leaves them
    /// unchanged.
    pub tint: Rgba,
    /// Flip horizontally.
    pub flip_x: bool,
    /// Flip vertically.
    pub flip_y: bool,
}

/// How a quad is drawn beyond its rectangle: turned around a pivot and
/// blended with what is under it. The default draws the rectangle as it
/// is, with alpha blending.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attributes {
    /// Rotation in degrees, positive clockwise on screen (the engine's
    /// `Rotate`); `0` draws the rectangle as it is.
    pub rotation: f32,
    /// The point the quad turns around, in logical pixels before the
    /// current offset (an object's position, where the sprite's hotspot
    /// is).
    pub pivot: (f32, f32),
    /// How the quad is blended.
    pub blend: BlendMode,
}

impl Default for Attributes {
    fn default() -> Self {
        Attributes {
            rotation: 0.0,
            pivot: (0.0, 0.0),
            blend: BlendMode::Normal,
        }
    }
}

impl Attributes {
    /// Draws the rectangle as it is, with alpha blending.
    pub const NONE: Attributes = Attributes {
        rotation: 0.0,
        pivot: (0.0, 0.0),
        blend: BlendMode::Normal,
    };

    /// Whether the quad is turned.
    pub fn is_rotated(&self) -> bool {
        self.rotation != 0.0
    }
}

/// A list of quads to draw back to front, with 3D meshes between them.
///
/// Coordinates are *logical* pixels: the list has a logical size (the
/// game's resolution) that the renderer maps onto the whole target, or
/// onto the largest centred rectangle of the same aspect ratio when
/// `letterbox` is set.
///
/// Every quad has [`Attributes`]; [`DrawList::push`] and the helpers
/// give the default ones, [`DrawList::push_with`] takes them. A
/// [`MeshDraw`] pushed with [`DrawList::push_mesh`] is drawn where it
/// was pushed, in clip space, with the same viewport as the quads.
#[derive(Debug)]
pub struct DrawList<'a> {
    /// Logical width.
    pub width: f32,
    /// Logical height.
    pub height: f32,
    /// Keep the aspect ratio and pad with the clear colour.
    pub letterbox: bool,
    /// Added to every quad position (scroll offset of the current layer).
    pub offset: (f32, f32),
    quads: Vec<Quad<'a>>,
    /// One per quad.
    attributes: Vec<Attributes>,
    /// The mesh draws, each before the quad whose index it carries;
    /// `before_quad` never decreases along the list.
    meshes: Vec<PlacedMesh<'a>>,
}

impl<'a> DrawList<'a> {
    /// An empty list with the given logical size.
    pub fn new(width: f32, height: f32) -> DrawList<'a> {
        DrawList {
            width,
            height,
            letterbox: false,
            offset: (0.0, 0.0),
            quads: Vec::new(),
            attributes: Vec::new(),
            meshes: Vec::new(),
        }
    }

    /// Removes every quad and mesh, keeping the size and offset.
    pub fn clear(&mut self) {
        self.quads.clear();
        self.attributes.clear();
        self.meshes.clear();
    }

    /// The quads, in drawing order.
    pub fn quads(&self) -> &[Quad<'a>] {
        &self.quads
    }

    /// The attributes of the quads, in the same order as
    /// [`DrawList::quads`].
    pub fn attributes(&self) -> &[Attributes] {
        &self.attributes
    }

    /// The mesh draws with their places among the quads, in drawing
    /// order.
    pub fn meshes(&self) -> &[PlacedMesh<'a>] {
        &self.meshes
    }

    /// Sets the offset added to the following quads.
    pub fn set_offset(&mut self, x: f32, y: f32) {
        self.offset = (x, y);
    }

    /// Appends a quad, applying the current offset to its position.
    pub fn push(&mut self, quad: Quad<'a>) {
        self.push_with(quad, Attributes::NONE);
    }

    /// Appends a quad with its attributes, applying the current offset
    /// to its position and to the pivot.
    pub fn push_with(&mut self, mut quad: Quad<'a>, mut attributes: Attributes) {
        quad.dst.x += self.offset.0;
        quad.dst.y += self.offset.1;
        attributes.pivot.0 += self.offset.0;
        attributes.pivot.1 += self.offset.1;
        self.quads.push(quad);
        self.attributes.push(attributes);
    }

    /// Appends a mesh draw at this place in the order: it is drawn after
    /// the quads pushed so far and before the following ones. The offset
    /// does not apply: a mesh is placed by its matrices (the scroll of a
    /// 3D scene is in its camera, see `docs/3d-actors.md`).
    pub fn push_mesh(&mut self, draw: MeshDraw<'a>) {
        self.meshes.push(PlacedMesh {
            before_quad: self.quads.len(),
            draw,
        });
    }

    /// Draws a whole texture at `(x, y)` at its own size.
    pub fn image(&mut self, texture: &'a Texture, x: f32, y: f32) {
        self.push(Quad {
            texture,
            dst: Rect::new(x, y, texture.width as f32, texture.height as f32),
            src: None,
            tint: Rgba::OPAQUE_WHITE,
            flip_x: false,
            flip_y: false,
        });
    }

    /// Draws a part of a texture into a rectangle.
    pub fn sprite(&mut self, texture: &'a Texture, dst: Rect, src: Option<Rect>, tint: Rgba) {
        self.push(Quad {
            texture,
            dst,
            src,
            tint,
            flip_x: false,
            flip_y: false,
        });
    }

    /// Fills a rectangle with a colour, using a white texture.
    pub fn fill(&mut self, white: &'a Texture, dst: Rect, color: Rgba) {
        self.sprite(white, dst, None, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_regroups_columns() {
        let cols: [f32; 16] = std::array::from_fn(|i| i as f32);
        let m = matrix(cols);
        assert_eq!(m[0], [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(m[3], [12.0, 13.0, 14.0, 15.0]);
    }

    #[test]
    fn lights_hold_at_most_the_limit() {
        let light = Light::Point {
            position: [0.0; 3],
            range: 1.0,
            color: [1.0; 3],
        };
        let mut lights = Lights::NONE;
        assert!(lights.is_empty());
        for _ in 0..MAX_LIGHTS {
            assert!(lights.push(light));
        }
        assert!(!lights.push(light));
        assert_eq!(lights.len(), MAX_LIGHTS);
        assert_eq!(Lights::from_slice(&[light; 10]).len(), MAX_LIGHTS);
        assert_eq!(Lights::from_slice(&[light; 3]).as_slice().len(), 3);
    }
}
