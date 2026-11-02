//! The drawing of the 3D actors: their models as mesh draws at their
//! place in the scene's order, and their shadows before them, from what
//! [`World::three_d_draws`] hands out (see `docs/3d-actors.md`, sections
//! 10 and 12.4).
//!
//! [`Render3d`] is the GPU side the world keeps across frames: the
//! uploaded meshes of the models (one per `.X` mesh, updated with the
//! skinned pose each frame), the textures of their materials, the shadow
//! receivers of the scene and the unit quad and default image of the
//! simple shadows. [`World::upload`] refreshes it; [`World::draw`] then
//! turns the frame into draw-list items without the renderer.
//!
//! Written with reference to `AdActor3DX.cpp` (`Display`,
//! `DisplayShadowVolume`), `3DShadowGen.cpp` (`RenderSimple`, `Render`),
//! `3DShadowVolume.cpp` (`AddMesh`), `3DLight.cpp` (`SetLight`),
//! `AdSceneGeometry.cpp` (`RenderShadowGeometry`) and `AdScene.cpp`
//! (`TraverseNodes`) of Wintermute Engine 1.x (MIT License, Copyright (c)
//! 2003-2013 Jan Nedoma); see `NOTICE`.

use std::collections::HashMap;
use std::ops::Range;

use luma_3d::math::deg_to_rad;
use luma_3d::{AnimatedModel, Mat4, Material, Mesh, Model, Plane, ShadowType, Vec3};
use luma_formats::image::DEFAULT_COLOR_KEY;
use luma_formats::{Image, Rgb, Rgba, Sprite};
use luma_render::{
    DepthOnlyDraw, Draw3d, Fog, Light, Lights, Matrix, MeshDraw, MeshHandle, MeshMaterial,
    MeshVertex, Renderer, ShadowVolumeDraw, StencilFill, Texture, matrix,
};
use luma_script::NativeId;
use luma_video::Video;

use crate::actor3d::{Actor3dDraw, AttachmentDraw, MaterialOverride, MaterialTexture, ThreeDFrame};
use crate::scene_view::{AnimState, Overlays};
use crate::video::{VideoPlayer, step_player, upload_frame};
use crate::world::World;

/// How far a shadow volume is extruded, as a multiple of the distance
/// from the actor to its shadow light (`DisplayShadowVolume`).
pub const EXTRUSION_FACTOR: f32 = 1.5;

/// The range of the point lights of the geometry (`C3DLight::SetLight`).
pub const POINT_LIGHT_RANGE: f32 = 100_000.0;

/// The cone of a spot light of the geometry, radians: `Theta` and `Phi`.
pub const SPOT_INNER_ANGLE: f32 = 0.5;
/// See [`SPOT_INNER_ANGLE`].
pub const SPOT_OUTER_ANGLE: f32 = 1.0;
/// The falloff exponent of a spot light.
pub const SPOT_FALLOFF: f32 = 1.0;

/// The size of the default simple shadow image, a soft dark blob that
/// stands in for the original's `shadow.png`.
const DEFAULT_SHADOW_SIZE: u32 = 64;

/// A model mesh on the GPU.
#[derive(Debug, Clone)]
struct UploadedMesh {
    handle: MeshHandle,
    /// The index range of each material, in the order of the mesh's
    /// materials (the triangles are uploaded sorted by material, as the
    /// attribute table of `D3DXMesh` sorts them).
    ranges: Vec<Range<u32>>,
    /// The number of triangles the index buffer was built from.
    triangles: usize,
}

/// The neighbours of each triangle's three edges in a model mesh,
/// welded by position: the adjacency `D3DXMesh` carries
/// (`GenerateAdjacency`), on which the silhouette edges are found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjacency {
    /// For triangle `t`, `neighbours[t][i]` is the triangle sharing the
    /// edge from its vertex `i` to vertex `(i + 1) % 3`, if any.
    pub neighbours: Vec<[Option<u32>; 3]>,
    /// The vertex and triangle counts it was built from.
    built_from: (usize, usize),
}

impl Adjacency {
    /// The adjacency of a mesh's triangles over its vertices welded by
    /// exact position (the vertices Luma duplicated for split normals
    /// share their position with the original).
    pub fn of(mesh: &Mesh) -> Adjacency {
        let mut welded: HashMap<[u32; 3], u32> = HashMap::new();
        let mut weld = |index: u32| -> u32 {
            let p = mesh.vertices[index as usize].position;
            let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
            let next = welded.len() as u32;
            *welded.entry(key).or_insert(next)
        };
        let welded_triangles: Vec<[u32; 3]> = mesh
            .triangles
            .iter()
            .map(|t| [weld(t[0]), weld(t[1]), weld(t[2])])
            .collect();
        let mut by_edge: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
        for (t, tri) in welded_triangles.iter().enumerate() {
            for i in 0..3 {
                let (a, b) = (tri[i], tri[(i + 1) % 3]);
                by_edge
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push(t as u32);
            }
        }
        let neighbours = welded_triangles
            .iter()
            .enumerate()
            .map(|(t, tri)| {
                std::array::from_fn(|i| {
                    let (a, b) = (tri[i], tri[(i + 1) % 3]);
                    by_edge[&(a.min(b), a.max(b))]
                        .iter()
                        .copied()
                        .find(|&other| other != t as u32)
                })
            })
            .collect();
        Adjacency {
            neighbours,
            built_from: (mesh.vertices.len(), mesh.triangles.len()),
        }
    }

    /// Whether the adjacency was built from a mesh of this shape.
    fn fits(&self, mesh: &Mesh) -> bool {
        self.built_from == (mesh.vertices.len(), mesh.triangles.len())
    }
}

/// The shadow volume of a posed mesh (`C3DShadowVolume::AddMesh`): for
/// every edge of a triangle turned toward the light whose neighbour is
/// not, or that has none, a quad from the edge to the edge pushed away
/// from the light by `depth`. `light` is the direction from the model
/// to its shadow light, in model space; the triangles are appended to
/// `out`, in model space, two per edge.
pub fn add_shadow_volume(
    mesh: &Mesh,
    positions: &[Vec3],
    adjacency: &Adjacency,
    light: Vec3,
    depth: f32,
    out: &mut Vec<[f32; 3]>,
) {
    let position = |i: u32| positions[i as usize];
    // `IsFront`: the original's face normal is `(v2 - v1) x (v1 - v0)`,
    // the opposite of the outward normal of a clockwise face, and a face
    // is turned toward the light when its dot with the light is
    // negative.
    let front: Vec<bool> = mesh
        .triangles
        .iter()
        .map(|t| {
            let (v0, v1, v2) = (position(t[0]), position(t[1]), position(t[2]));
            (v2 - v1).cross(v1 - v0).dot(light) < 0.0
        })
        .collect();
    let push = |out: &mut Vec<[f32; 3]>, v: Vec3| out.push([v.x, v.y, v.z]);
    for (t, tri) in mesh.triangles.iter().enumerate() {
        if !front[t] {
            continue;
        }
        for i in 0..3 {
            let neighbour = adjacency.neighbours[t][i];
            if neighbour.is_some_and(|n| front[n as usize]) {
                continue;
            }
            let (v1, v2) = (position(tri[i]), position(tri[(i + 1) % 3]));
            let (v3, v4) = (v1 - light * depth, v2 - light * depth);
            push(out, v1);
            push(out, v2);
            push(out, v3);
            push(out, v2);
            push(out, v4);
            push(out, v3);
        }
    }
}

/// The matrix that flattens points onto a plane as seen from a point
/// light (`D3DXMatrixShadow`, row-vector convention): a point `p`
/// becomes the intersection of the line from the light through `p` with
/// the plane.
pub fn shadow_matrix(plane: Plane, light: Vec3) -> Mat4 {
    let n = plane.normal.normalized();
    let d = plane.d / plane.normal.length();
    let l = [light.x, light.y, light.z, 1.0];
    let p = [n.x, n.y, n.z, d];
    let dot = p[0] * l[0] + p[1] * l[1] + p[2] * l[2] + p[3] * l[3];
    let mut m = [[0.0f32; 4]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = -p[r] * l[c];
            if r == c {
                *cell += dot;
            }
        }
    }
    Mat4::from_rows(m)
}

/// A light of the geometry as the renderer takes it (`C3DLight::
/// SetLight`): the colour channels divided by 256, a point light with
/// the original's range and no attenuation, or a spot with its cone.
pub fn convert_light(light: &luma_3d::Light) -> Light {
    let c = light.color;
    let color = [c.r, c.g, c.b].map(|v| f32::from(v) / 256.0);
    let position = [light.position.x, light.position.y, light.position.z];
    if light.spotlight {
        let direction = light.target - light.position;
        Light::Spot {
            position,
            direction: [direction.x, direction.y, direction.z],
            range: direction.length(),
            inner_angle: SPOT_INNER_ANGLE,
            outer_angle: SPOT_OUTER_ANGLE,
            falloff: SPOT_FALLOFF,
            color,
        }
    } else {
        Light::Point {
            position,
            range: POINT_LIGHT_RANGE,
            color,
        }
    }
}

/// The `r, g, b` of a `0xAARRGGBB` colour as floats.
fn rgb_of(color: i32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((color >> shift) & 0xff) as f32 / 255.0)
}

/// A `0xAARRGGBB` colour as an [`Rgba`].
fn rgba_of(color: i32) -> Rgba {
    Rgba {
        r: ((color >> 16) & 0xff) as u8,
        g: ((color >> 8) & 0xff) as u8,
        b: (color & 0xff) as u8,
        a: ((color >> 24) & 0xff) as u8,
    }
}

/// The renderer's matrix of a row-vector one.
fn gpu_matrix(m: &Mat4) -> Matrix {
    matrix(m.to_cols_array())
}

/// The vertices of a posed mesh as the renderer takes them.
fn mesh_vertices(positions: &[luma_3d::model::Vertex]) -> Vec<MeshVertex> {
    positions
        .iter()
        .map(|v| MeshVertex {
            position: [v.position.x, v.position.y, v.position.z],
            normal: [v.normal.x, v.normal.y, v.normal.z],
            uv: v.uv,
        })
        .collect()
}

/// The index buffer of a mesh, its triangles sorted by material, and
/// the range of each material.
fn sorted_indices(mesh: &Mesh) -> (Vec<u32>, Vec<Range<u32>>) {
    let mut indices = Vec::with_capacity(mesh.triangles.len() * 3);
    let mut ranges = Vec::with_capacity(mesh.materials.len());
    for material in 0..mesh.materials.len().max(1) as u32 {
        let start = indices.len() as u32;
        for (t, tri) in mesh.triangles.iter().enumerate() {
            if mesh.triangle_materials.get(t).copied().unwrap_or(0) == material {
                indices.extend_from_slice(tri);
            }
        }
        ranges.push(start..indices.len() as u32);
    }
    (indices, ranges)
}

/// The default image of the simple shadows: a soft black blob, opaque
/// at the centre and fading to nothing at the edge of the quad.
pub fn default_shadow_image() -> Image {
    let size = DEFAULT_SHADOW_SIZE;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let radius = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 + 0.5 - radius, y as f32 + 0.5 - radius);
            let distance = (dx * dx + dy * dy).sqrt() / radius;
            let fade = (1.0 - distance).clamp(0.0, 1.0);
            // A smooth step keeps the centre dark and the edge soft.
            let alpha = fade * fade * (3.0 - 2.0 * fade);
            pixels.extend_from_slice(&[0, 0, 0, (alpha * 192.0).round() as u8]);
        }
    }
    Image {
        width: size,
        height: size,
        pixels,
        has_alpha_channel: true,
    }
}

/// The unit quad of the simple shadows (`C3DShadowGen::
/// CreateSimpleShadow`): `-1..1` in `x` and `z` at `y = 0`, the image
/// mapped once over it.
fn shadow_quad() -> (Vec<MeshVertex>, Vec<u32>) {
    let v = |x: f32, z: f32, u: f32, vv: f32| MeshVertex {
        position: [x, 0.0, z],
        normal: [0.0, 1.0, 0.0],
        uv: [u, vv],
    };
    (
        vec![
            v(-1.0, 1.0, 0.0, 1.0),
            v(1.0, 1.0, 1.0, 1.0),
            v(-1.0, -1.0, 0.0, 0.0),
            v(1.0, -1.0, 1.0, 0.0),
        ],
        vec![0, 1, 2, 2, 1, 3],
    )
}

/// The shadow receivers of the scene on the GPU.
#[derive(Debug)]
struct Receivers {
    /// The scene and geometry file they were built from, and how many
    /// nodes the geometry had.
    key: (String, usize),
    /// One per node of the geometry, in the order of
    /// `SceneGeometry::nodes`, for the nodes that receive shadows.
    nodes: Vec<(usize, MeshHandle)>,
}

/// A texture a script put on a material (`MaterialOverride`), as the
/// drawing step keeps it between frames.
// A video player is large; there are a few of these per actor at most.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum ScriptTexture {
    /// A still image, in `Render3d::textures`.
    Still(String),
    /// A `.sprite`: its frames' images are in `Render3d::textures` and
    /// the state advances on the game timer, frame delays honoured
    /// (`CXModel::SetMaterialSprite`).
    Sprite {
        file: String,
        sprite: Sprite,
        state: AnimState,
    },
    /// A Theora video played in a loop, without sound; each decoded
    /// frame becomes the texture (`CXModel::SetMaterialTheora`).
    Video {
        file: String,
        player: VideoPlayer,
        texture: Option<Texture>,
        uploaded: u64,
    },
    /// A file that could not be opened; not tried again.
    Failed(String),
}

impl ScriptTexture {
    fn file(&self) -> &str {
        match self {
            ScriptTexture::Still(file)
            | ScriptTexture::Sprite { file, .. }
            | ScriptTexture::Video { file, .. }
            | ScriptTexture::Failed(file) => file,
        }
    }

    /// Whether this is the texture `wanted` asks for.
    fn is(&self, wanted: &MaterialTexture) -> bool {
        match (self, wanted) {
            (ScriptTexture::Video { file, .. }, MaterialTexture::Theora(f)) => file == f,
            (ScriptTexture::Video { .. }, MaterialTexture::Sprite(_)) => false,
            (
                ScriptTexture::Still(_) | ScriptTexture::Sprite { .. },
                MaterialTexture::Theora(_),
            ) => false,
            (other, MaterialTexture::Sprite(f) | MaterialTexture::Theora(f)) => other.file() == f,
        }
    }
}

/// A model drawn this frame: the actor's or an attachment's.
struct ModelDraw<'a> {
    id: NativeId,
    model: &'a AnimatedModel,
    world: Mat4,
    draw_backfaces: bool,
}

impl<'a> ModelDraw<'a> {
    fn of_actor(draw: &Actor3dDraw<'a>) -> ModelDraw<'a> {
        ModelDraw {
            id: draw.id,
            model: draw.model,
            world: draw.world,
            draw_backfaces: draw.draw_backfaces,
        }
    }

    fn of_attachment(attachment: &AttachmentDraw<'a>) -> ModelDraw<'a> {
        ModelDraw {
            id: attachment.id,
            model: attachment.model,
            world: attachment.world,
            draw_backfaces: attachment.draw_backfaces,
        }
    }

    /// The actor's model, then its attachments', as they are drawn
    /// (`Display`, then `DisplayAttachments`) and cast shadows
    /// (`RenderModel`, `DisplayShadowVolume`).
    fn all(draw: &Actor3dDraw<'a>) -> Vec<ModelDraw<'a>> {
        std::iter::once(ModelDraw::of_actor(draw))
            .chain(draw.attachments.iter().map(ModelDraw::of_attachment))
            .collect()
    }
}

/// The GPU side of the 3D actors, kept by the world across frames.
#[derive(Debug, Default)]
pub struct Render3d {
    /// By actor or attachment, and mesh index in its model.
    meshes: HashMap<(NativeId, usize), UploadedMesh>,
    adjacency: HashMap<(NativeId, usize), Adjacency>,
    /// By path; `None` when the file could not be read or decoded, so
    /// that it is not tried again every frame.
    textures: HashMap<String, Option<Texture>>,
    /// The textures scripts put on materials, by actor and material name
    /// in lower case.
    overrides: HashMap<(NativeId, String), ScriptTexture>,
    receivers: Option<Receivers>,
    quad: Option<MeshHandle>,
    default_shadow: Option<Texture>,
    /// What went wrong, for the world's log.
    log: Vec<String>,
}

impl Render3d {
    /// Uploads and updates what the frame's 3D actors need: their meshes
    /// and their attachments' with the pose of this frame, the textures
    /// of their materials, the textures scripts put on materials (a
    /// sprite's frame of this moment, a video's), their shadow images,
    /// the scene's shadow receivers; frees what belongs to actors that
    /// are gone.
    pub(crate) fn upload(&mut self, world: &World, renderer: &Renderer) {
        let Some((frame, draws)) = world.three_d_draws() else {
            self.free_gone(renderer, &[], &[]);
            return;
        };
        let live: Vec<NativeId> = draws
            .iter()
            .flat_map(|d| std::iter::once(d.id).chain(d.attachments.iter().map(|a| a.id)))
            .collect();
        let overrides: Vec<(NativeId, &MaterialOverride)> = draws
            .iter()
            .flat_map(|d| d.materials.iter().map(move |m| (d.id, m)))
            .collect();
        self.free_gone(renderer, &live, &overrides);
        let mut need_quad = false;
        for draw in &draws {
            for model in ModelDraw::all(draw) {
                self.upload_model(
                    renderer,
                    model.id,
                    model.model.model(),
                    model.model.meshes(),
                );
                for mesh in &model.model.model().meshes {
                    for material in &mesh.materials {
                        if let Some(path) = texture_path(material) {
                            self.ensure_texture(world, renderer, path, DEFAULT_COLOR_KEY);
                        }
                    }
                }
            }
            for material in draw.materials {
                self.update_override(world, renderer, draw.id, material);
            }
            if draw.shadow_type == ShadowType::Simple {
                need_quad = true;
                match draw.shadow_image {
                    Some(path) => self.ensure_texture(world, renderer, path, DEFAULT_COLOR_KEY),
                    None => {
                        if self.default_shadow.is_none() {
                            self.default_shadow =
                                Some(renderer.mesh_texture("shadow", &default_shadow_image()));
                        }
                    }
                }
            }
        }
        if need_quad && self.quad.is_none() {
            let (vertices, indices) = shadow_quad();
            self.quad = Some(renderer.upload_mesh(&vertices, &indices));
        }
        if frame.stencil_shadows {
            self.upload_receivers(world, renderer, &frame);
        }
    }

    /// What the upload logged, once.
    pub(crate) fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    /// Frees the meshes of the models not in `live` and the script
    /// textures no actor asks for any more.
    fn free_gone(
        &mut self,
        renderer: &Renderer,
        live: &[NativeId],
        overrides: &[(NativeId, &MaterialOverride)],
    ) {
        self.meshes.retain(|(id, _), mesh| {
            let keep = live.contains(id);
            if !keep {
                renderer.free_mesh(mesh.handle);
            }
            keep
        });
        self.adjacency.retain(|(id, _), _| live.contains(id));
        self.overrides.retain(|(id, material), texture| {
            let keep = overrides.iter().any(|(owner, o)| {
                owner == id && o.material.eq_ignore_ascii_case(material) && texture.is(&o.texture)
            });
            if !keep {
                if let ScriptTexture::Video {
                    texture: Some(texture),
                    ..
                } = texture
                {
                    renderer.forget(texture);
                }
            }
            keep
        });
    }

    /// Opens a script texture when it is new, and advances it: the
    /// sprite's frame on the game timer, the video's frames.
    fn update_override(
        &mut self,
        world: &World,
        renderer: &Renderer,
        id: NativeId,
        material: &MaterialOverride,
    ) {
        let key = (id, material.material.to_ascii_lowercase());
        if !self.overrides.contains_key(&key) {
            let texture = self.open_override(world, renderer, &material.texture);
            self.overrides.insert(key.clone(), texture);
        }
        let Some(texture) = self.overrides.get_mut(&key) else {
            return;
        };
        match texture {
            ScriptTexture::Still(_) | ScriptTexture::Failed(_) => {}
            ScriptTexture::Sprite { sprite, state, .. } => {
                state.step(sprite, world.timer);
            }
            ScriptTexture::Video {
                file,
                player,
                texture,
                uploaded,
            } => {
                if !world.is_frozen() && step_player(player, None, world.timer, &mut self.log) {
                    if let Err(err) = player.restart() {
                        self.log
                            .push(format!("Error restarting video '{file}': {err}"));
                        if let Some(texture) = texture.take() {
                            renderer.forget(&texture);
                        }
                        *self.overrides.get_mut(&key).expect("present") =
                            ScriptTexture::Failed(file.clone());
                        return;
                    }
                }
                upload_frame(renderer, player, texture, uploaded);
            }
        }
    }

    /// Reads the file of a script texture: a `.sprite` with its frames'
    /// images, a video, or a still image.
    fn open_override(
        &mut self,
        world: &World,
        renderer: &Renderer,
        texture: &MaterialTexture,
    ) -> ScriptTexture {
        match texture {
            MaterialTexture::Theora(file) => match world.vfs.read(file) {
                Ok(bytes) => match Video::open(bytes) {
                    Ok(video) => ScriptTexture::Video {
                        file: file.clone(),
                        player: VideoPlayer::texture_loop(file, video),
                        texture: None,
                        uploaded: 0,
                    },
                    Err(err) => {
                        self.log
                            .push(format!("Error opening video file '{file}': {err}"));
                        ScriptTexture::Failed(file.clone())
                    }
                },
                Err(err) => {
                    self.log
                        .push(format!("Error opening video file '{file}': {err}"));
                    ScriptTexture::Failed(file.clone())
                }
            },
            MaterialTexture::Sprite(file) if is_sprite_file(file) => {
                let sprite = world
                    .vfs
                    .read(file)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| Sprite::parse(&bytes).map_err(|e| e.to_string()));
                match sprite {
                    Ok(sprite) => {
                        for sub in sprite.frames.iter().flat_map(|f| f.subframes.iter()) {
                            if let Some(image) = &sub.image {
                                self.ensure_texture(world, renderer, image, sub.color_key);
                            }
                        }
                        ScriptTexture::Sprite {
                            file: file.clone(),
                            sprite,
                            state: AnimState::new(file),
                        }
                    }
                    Err(err) => {
                        self.log
                            .push(format!("Error loading sprite '{file}': {err}"));
                        ScriptTexture::Failed(file.clone())
                    }
                }
            }
            MaterialTexture::Sprite(file) => {
                self.ensure_texture(world, renderer, file, DEFAULT_COLOR_KEY);
                ScriptTexture::Still(file.clone())
            }
        }
    }

    /// The texture a script put on a material of an actor, if it shows
    /// one this frame.
    fn override_texture(&self, id: NativeId, material: &Material) -> Option<&Texture> {
        let name = material.name.as_deref()?;
        let texture = self.overrides.get(&(id, name.to_ascii_lowercase()))?;
        match texture {
            ScriptTexture::Still(file) => self.texture(file),
            ScriptTexture::Sprite { sprite, state, .. } => sprite
                .frames
                .get(state.current_frame)
                .and_then(|f| f.subframes.first())
                .and_then(|s| s.image.as_deref())
                .and_then(|image| self.texture(image)),
            ScriptTexture::Video { texture, .. } => texture.as_ref(),
            ScriptTexture::Failed(_) => None,
        }
    }

    /// Uploads the meshes of a model once and writes the posed vertices
    /// each frame; a model whose meshes changed shape (a reload) is
    /// uploaded again.
    fn upload_model(
        &mut self,
        renderer: &Renderer,
        id: NativeId,
        model: &Model,
        posed: &[luma_3d::animation::SkinnedMesh],
    ) {
        for (index, (mesh, skinned)) in model.meshes.iter().zip(posed).enumerate() {
            if mesh.triangles.is_empty() || skinned.vertices.is_empty() {
                continue;
            }
            let vertices = mesh_vertices(&skinned.vertices);
            match self.meshes.get(&(id, index)) {
                Some(uploaded) if uploaded.triangles == mesh.triangles.len() => {
                    // The vertex count can only grow through `update_mesh`
                    // if a reload changed it; the handle grows with it.
                    let _ = renderer.update_mesh(uploaded.handle, &vertices);
                }
                existing => {
                    if let Some(old) = existing {
                        renderer.free_mesh(old.handle);
                    }
                    let (indices, ranges) = sorted_indices(mesh);
                    let handle = renderer.upload_mesh(&vertices, &indices);
                    self.meshes.insert(
                        (id, index),
                        UploadedMesh {
                            handle,
                            ranges,
                            triangles: mesh.triangles.len(),
                        },
                    );
                }
            }
            let fits = self
                .adjacency
                .get(&(id, index))
                .is_some_and(|a| a.fits(mesh));
            if !fits {
                self.adjacency.insert((id, index), Adjacency::of(mesh));
            }
        }
    }

    /// Decodes and uploads a texture once, with a colour key and a
    /// mipmap chain.
    fn ensure_texture(&mut self, world: &World, renderer: &Renderer, path: &str, key: Rgb) {
        if self.textures.contains_key(path) {
            return;
        }
        let texture = match world
            .vfs
            .read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| luma_formats::image::decode(&bytes, key).map_err(|e| e.to_string()))
        {
            Ok(image) => Some(renderer.mesh_texture(path, &image)),
            Err(err) => {
                self.log
                    .push(format!("Error loading texture '{path}': {err}"));
                None
            }
        };
        self.textures.insert(path.to_string(), texture);
    }

    /// Uploads the shadow-receiving nodes of the geometry once per scene
    /// and geometry.
    fn upload_receivers(&mut self, world: &World, renderer: &Renderer, frame: &ThreeDFrame<'_>) {
        let key = (
            world
                .scene
                .as_ref()
                .map(|v| {
                    format!(
                        "{}|{}",
                        v.path,
                        v.three_d.as_ref().map(|t| t.file.as_str()).unwrap_or("")
                    )
                })
                .unwrap_or_default(),
            frame.geometry.nodes().count(),
        );
        if self.receivers.as_ref().is_some_and(|r| r.key == key) {
            return;
        }
        if let Some(old) = self.receivers.take() {
            for (_, handle) in old.nodes {
                renderer.free_mesh(handle);
            }
        }
        let mut nodes = Vec::new();
        for (index, node) in frame.geometry.nodes().enumerate() {
            if !node.receive_shadows || node.mesh.faces.is_empty() || node.mesh.vertices.is_empty()
            {
                continue;
            }
            let vertices: Vec<MeshVertex> = node
                .mesh
                .vertices
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let n = node
                        .mesh
                        .normals
                        .get(i)
                        .copied()
                        .unwrap_or(Vec3::new(0.0, 1.0, 0.0));
                    MeshVertex {
                        position: [p.x, p.y, p.z],
                        normal: [n.x, n.y, n.z],
                        uv: [0.0, 0.0],
                    }
                })
                .collect();
            let indices: Vec<u32> = node
                .mesh
                .faces
                .iter()
                .flat_map(|f| f.iter().map(|i| u32::from(*i)))
                .collect();
            nodes.push((index, renderer.upload_mesh(&vertices, &indices)));
        }
        self.receivers = Some(Receivers { key, nodes });
    }

    /// The texture of a path, when it was uploaded.
    fn texture(&self, path: &str) -> Option<&Texture> {
        self.textures.get(path).and_then(|t| t.as_ref())
    }

    /// The 3D items of the frame: the receivers to draw before the main
    /// layer's nodes, and, per 3D actor (by its index among the free
    /// objects), its shadow, then its model and its attachments.
    pub(crate) fn overlays<'a>(&'a self, world: &'a World, overlays: &mut Overlays<'a>) {
        let Some((frame, draws)) = world.three_d_draws() else {
            return;
        };
        let view_projection = gpu_matrix(&frame.projection.view_projection());
        let fog = frame.fog.map(|f| Fog {
            color: rgb_of(f.color),
            start: f.start,
            end: f.end,
        });
        if frame.stencil_shadows {
            if let Some(receivers) = &self.receivers {
                let nodes: Vec<_> = frame.geometry.nodes().collect();
                for (index, handle) in &receivers.nodes {
                    if nodes.get(*index).is_some_and(|n| n.active) {
                        overlays.before_main.push(Draw3d::DepthOnly(DepthOnlyDraw {
                            mesh: *handle,
                            world: luma_render::IDENTITY,
                            view_projection,
                            cull_backfaces: true,
                        }));
                    }
                }
            }
        }
        for draw in &draws {
            let mut items = Vec::new();
            self.shadow_items(draw, view_projection, &mut items);
            self.model_items(draw, view_projection, fog, &mut items);
            if !items.is_empty() {
                overlays.three_d.insert(draw.other_index, items);
            }
        }
    }

    /// The shadow of an actor, drawn before its model
    /// (`CAdActor3DX::Display`); the attachments cast with it.
    fn shadow_items<'a>(
        &'a self,
        draw: &Actor3dDraw<'_>,
        view_projection: Matrix,
        items: &mut Vec<Draw3d<'a>>,
    ) {
        match draw.shadow_type {
            ShadowType::None => {}
            ShadowType::Simple => {
                // `RenderSimple`: the quad scaled by the shadow size,
                // turned with the actor, at its feet, with the shadow
                // image, no depth writes. Drawn unlit (section 10.4).
                let (Some(quad), Some(texture)) = (
                    self.quad,
                    draw.shadow_image
                        .and_then(|path| self.texture(path))
                        .or(self.default_shadow.as_ref()),
                ) else {
                    return;
                };
                let world = Mat4::scaling(Vec3::new(draw.shadow_size, 1.0, draw.shadow_size))
                    * Mat4::rotation_y(deg_to_rad(draw.angle))
                    * Mat4::translation(draw.position);
                items.push(Draw3d::Mesh(MeshDraw {
                    texture: Some(texture),
                    material: MeshMaterial {
                        diffuse: [0.0, 0.0, 0.0, 1.0],
                        emissive: [1.0, 1.0, 1.0],
                    },
                    depth_write: false,
                    ..MeshDraw::new(quad, gpu_matrix(&world), view_projection)
                }));
            }
            ShadowType::Flat => {
                // `C3DShadowGen::Render`: the model and its attachments
                // seen from the shadow light, in the shadow colour, laid
                // on the plane of the actor's feet. Luma projects the
                // posed triangles onto that plane instead of rendering
                // them to a texture; the stencil keeps the overlapping
                // triangles from darkening twice. The original modulates
                // the silhouette texture by the floor quad's colour, both
                // the shadow colour, hence the squared channels.
                let color = rgba_of(draw.shadow_color);
                let channel = |v: u8| (f32::from(v) / 255.0).powi(2);
                let material = MeshMaterial {
                    diffuse: [0.0, 0.0, 0.0, f32::from(color.a) / 255.0],
                    emissive: [channel(color.r), channel(color.g), channel(color.b)],
                };
                let plane = Plane {
                    normal: Vec3::new(0.0, 1.0, 0.0),
                    d: -draw.position.y,
                };
                let flatten = shadow_matrix(plane, draw.shadow_light);
                let reference = (draw.other_index % 255 + 1) as u8;
                for model in ModelDraw::all(draw) {
                    let world = gpu_matrix(&(model.world * flatten));
                    for index in 0..model.model.model().meshes.len() {
                        let Some(uploaded) = self.meshes.get(&(model.id, index)) else {
                            continue;
                        };
                        items.push(Draw3d::Mesh(MeshDraw {
                            material,
                            depth_write: false,
                            stencil_once: Some(reference),
                            ..MeshDraw::new(uploaded.handle, world, view_projection)
                        }));
                    }
                }
            }
            ShadowType::Stencil => {
                // `DisplayShadowVolume`: the silhouette edges of every
                // posed mesh of the model and of the attachments, extruded
                // away from the light in each model's space, counted into
                // the stencil in world space, then the fill in the shadow
                // colour.
                let light_vector = draw.shadow_light - draw.position;
                let depth = light_vector.length() * EXTRUSION_FACTOR;
                let direction = light_vector.normalized();
                let mut vertices = Vec::new();
                for model in ModelDraw::all(draw) {
                    let Some(inverse) = model.world.inverse() else {
                        continue;
                    };
                    let light = inverse.transform_normal(direction);
                    let start = vertices.len();
                    for (index, (mesh, posed)) in model
                        .model
                        .model()
                        .meshes
                        .iter()
                        .zip(model.model.meshes())
                        .enumerate()
                    {
                        let Some(adjacency) = self.adjacency.get(&(model.id, index)) else {
                            continue;
                        };
                        if !adjacency.fits(mesh) || posed.vertices.len() < mesh.vertices.len() {
                            continue;
                        }
                        let positions: Vec<Vec3> =
                            posed.vertices.iter().map(|v| v.position).collect();
                        add_shadow_volume(mesh, &positions, adjacency, light, depth, &mut vertices);
                    }
                    for v in &mut vertices[start..] {
                        let w = model.world.transform_coord(Vec3::new(v[0], v[1], v[2]));
                        *v = [w.x, w.y, w.z];
                    }
                }
                if vertices.is_empty() {
                    return;
                }
                items.push(Draw3d::ShadowVolume(ShadowVolumeDraw {
                    vertices,
                    world: luma_render::IDENTITY,
                    view_projection,
                }));
                items.push(Draw3d::StencilFill(StencilFill {
                    color: rgba_of(draw.shadow_color),
                }));
            }
        }
    }

    /// The model of an actor, then its attachments: one draw per material
    /// of each mesh (`CXMesh::Render`), the attachments lit like the
    /// actor (`DisplayAttachments`).
    fn model_items<'a>(
        &'a self,
        draw: &Actor3dDraw<'_>,
        view_projection: Matrix,
        fog: Option<Fog>,
        items: &mut Vec<Draw3d<'a>>,
    ) {
        let lights = Lights::from_slice(
            &draw
                .lights
                .iter()
                .map(|l| convert_light(l))
                .collect::<Vec<_>>(),
        );
        let ambient = rgb_of(draw.ambient);
        for model in ModelDraw::all(draw) {
            let world = gpu_matrix(&model.world);
            // Only the actor's own materials can be replaced by script.
            let overrides = model.id == draw.id;
            for (index, mesh) in model.model.model().meshes.iter().enumerate() {
                let Some(uploaded) = self.meshes.get(&(model.id, index)) else {
                    continue;
                };
                for (material_index, range) in uploaded.ranges.iter().enumerate() {
                    if range.is_empty() {
                        continue;
                    }
                    let material = mesh.materials.get(material_index);
                    let texture = material.and_then(|m| {
                        overrides
                            .then(|| self.override_texture(draw.id, m))
                            .flatten()
                            .or_else(|| texture_path(m).and_then(|path| self.texture(path)))
                    });
                    items.push(Draw3d::Mesh(MeshDraw {
                        indices: Some(range.clone()),
                        texture,
                        material: material.map_or_else(MeshMaterial::default, |m| MeshMaterial {
                            diffuse: m.diffuse,
                            emissive: m.emissive,
                        }),
                        ambient,
                        lights,
                        fog,
                        cull_backfaces: !model.draw_backfaces,
                        blend: draw.blend_mode,
                        ..MeshDraw::new(uploaded.handle, world, view_projection)
                    }));
                }
            }
        }
    }
}

/// The texture file of a material: the path resolved against the model's
/// directory, else the name as written.
fn texture_path(material: &Material) -> Option<&str> {
    material
        .texture_path
        .as_deref()
        .or(material.texture_filename.as_deref())
}

/// Whether a script texture file is a `.sprite` rather than an image.
fn is_sprite_file(file: &str) -> bool {
    file.rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sprite"))
}

impl World {
    /// Refreshes the GPU side of the 3D actors for this frame (see
    /// [`Render3d::upload`]).
    pub(crate) fn upload_3d(&mut self, renderer: &Renderer) {
        let mut render = std::mem::take(&mut self.render3d);
        render.upload(self, renderer);
        for message in render.take_log() {
            self.log(message);
        }
        self.render3d = render;
    }

    /// Adds the 3D items of the frame to the scene's overlays.
    pub(crate) fn three_d_overlays<'a>(&'a self, overlays: &mut Overlays<'a>) {
        self.render3d.overlays(self, overlays);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luma_3d::model::Vertex;

    /// A unit cube of twelve clockwise triangles, each face with its own
    /// four vertices (so that welding matters).
    fn cube() -> Mesh {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        // Each face: its normal, and its corners clockwise seen from
        // outside.
        let faces: [(Vec3, [Vec3; 4]); 6] = [
            (
                Vec3::new(0.0, 1.0, 0.0),
                [
                    Vec3::new(-1.0, 1.0, -1.0),
                    Vec3::new(-1.0, 1.0, 1.0),
                    Vec3::new(1.0, 1.0, 1.0),
                    Vec3::new(1.0, 1.0, -1.0),
                ],
            ),
            (
                Vec3::new(0.0, -1.0, 0.0),
                [
                    Vec3::new(-1.0, -1.0, 1.0),
                    Vec3::new(-1.0, -1.0, -1.0),
                    Vec3::new(1.0, -1.0, -1.0),
                    Vec3::new(1.0, -1.0, 1.0),
                ],
            ),
            (
                Vec3::new(0.0, 0.0, -1.0),
                [
                    Vec3::new(-1.0, -1.0, -1.0),
                    Vec3::new(-1.0, 1.0, -1.0),
                    Vec3::new(1.0, 1.0, -1.0),
                    Vec3::new(1.0, -1.0, -1.0),
                ],
            ),
            (
                Vec3::new(0.0, 0.0, 1.0),
                [
                    Vec3::new(1.0, -1.0, 1.0),
                    Vec3::new(1.0, 1.0, 1.0),
                    Vec3::new(-1.0, 1.0, 1.0),
                    Vec3::new(-1.0, -1.0, 1.0),
                ],
            ),
            (
                Vec3::new(-1.0, 0.0, 0.0),
                [
                    Vec3::new(-1.0, -1.0, 1.0),
                    Vec3::new(-1.0, 1.0, 1.0),
                    Vec3::new(-1.0, 1.0, -1.0),
                    Vec3::new(-1.0, -1.0, -1.0),
                ],
            ),
            (
                Vec3::new(1.0, 0.0, 0.0),
                [
                    Vec3::new(1.0, -1.0, -1.0),
                    Vec3::new(1.0, 1.0, -1.0),
                    Vec3::new(1.0, 1.0, 1.0),
                    Vec3::new(1.0, -1.0, 1.0),
                ],
            ),
        ];
        for (normal, corners) in faces {
            let base = vertices.len() as u32;
            for corner in corners {
                vertices.push(Vertex {
                    position: corner,
                    normal,
                    uv: [0.0, 0.0],
                });
            }
            triangles.push([base, base + 1, base + 2]);
            triangles.push([base, base + 2, base + 3]);
        }
        let count = triangles.len();
        Mesh {
            name: "cube".into(),
            frame: 0,
            original_vertices: (0..vertices.len() as u32).collect(),
            vertices,
            triangles,
            triangle_materials: vec![0; count],
            materials: vec![luma_3d::Material::default()],
            bones: Vec::new(),
            skin: None,
            duplication: None,
        }
    }

    #[test]
    fn the_cube_faces_are_clockwise_seen_from_outside() {
        let cube = cube();
        for tri in &cube.triangles {
            let [v0, v1, v2] = tri.map(|i| cube.vertices[i as usize].position);
            // The outward normal of a clockwise face in a left-handed
            // system is `(v1 - v0) x (v2 - v0)`; the face centre points
            // the same way from the origin.
            let outward = (v1 - v0).cross(v2 - v0);
            let centre = (v0 + v1 + v2) * (1.0 / 3.0);
            assert!(outward.dot(centre) > 0.0, "{tri:?}");
        }
    }

    #[test]
    fn adjacency_welds_the_shared_corners() {
        let cube = cube();
        let adjacency = Adjacency::of(&cube);
        assert_eq!(adjacency.neighbours.len(), 12);
        // Every edge of a closed cube has a neighbour.
        for (t, edges) in adjacency.neighbours.iter().enumerate() {
            for (i, n) in edges.iter().enumerate() {
                assert!(n.is_some(), "triangle {t} edge {i}");
                assert_ne!(*n, Some(t as u32));
            }
        }
        // The diagonal of a face (corners 0 and 2) joins its two
        // triangles; the other edges go to the neighbouring faces.
        assert_eq!(adjacency.neighbours[0][2], Some(1));
        assert_eq!(adjacency.neighbours[1][0], Some(0));
        assert!(adjacency.neighbours[0][1].is_some_and(|n| n >= 2));
    }

    #[test]
    fn the_silhouette_of_a_cube_lit_from_above_is_its_top_rim() {
        let cube = cube();
        let adjacency = Adjacency::of(&cube);
        let positions: Vec<Vec3> = cube.vertices.iter().map(|v| v.position).collect();
        let mut out = Vec::new();
        let light = Vec3::new(0.0, 1.0, 0.0);
        add_shadow_volume(&cube, &positions, &adjacency, light, 10.0, &mut out);
        // Only the top face turns toward the light: its four edges give
        // four quads, two triangles each.
        assert_eq!(out.len(), 4 * 6);
        for v in &out {
            // Every vertex is on the top rim or pushed 10 down from it.
            assert!(v[1] == 1.0 || v[1] == -9.0, "{v:?}");
            assert!(v[0].abs() == 1.0 && v[2].abs() == 1.0, "{v:?}");
        }
        assert_eq!(out.iter().filter(|v| v[1] == -9.0).count(), 12);

        // Lit from the front-top corner, three faces are turned toward
        // the light; the silhouette is the hexagon of six edges.
        let mut out = Vec::new();
        add_shadow_volume(
            &cube,
            &positions,
            &adjacency,
            Vec3::new(1.0, 1.0, -1.0),
            10.0,
            &mut out,
        );
        assert_eq!(out.len(), 6 * 6);
    }

    #[test]
    fn the_shadow_matrix_flattens_onto_the_plane_along_the_light() {
        let plane = Plane {
            normal: Vec3::new(0.0, 1.0, 0.0),
            d: -2.0,
        };
        let light = Vec3::new(0.0, 10.0, 0.0);
        let m = shadow_matrix(plane, light);
        // A point at height 6 under the light, 4 to the side: the line
        // from the light reaches the plane `y = 2` twice as far out.
        let p = m.transform_coord(Vec3::new(4.0, 6.0, 0.0));
        assert!((p.y - 2.0).abs() < 1e-4, "{p:?}");
        assert!((p.x - 8.0).abs() < 1e-4, "{p:?}");
        assert!(p.z.abs() < 1e-4, "{p:?}");
        // A point on the plane stays.
        let q = m.transform_coord(Vec3::new(3.0, 2.0, -5.0));
        assert!((q - Vec3::new(3.0, 2.0, -5.0)).length() < 1e-4, "{q:?}");
    }

    #[test]
    fn lights_are_converted_as_the_fixed_function_took_them() {
        let point = luma_3d::Light {
            name: "Omni".into(),
            position: Vec3::new(1.0, 2.0, 3.0),
            target: Vec3::ZERO,
            spotlight: false,
            color: luma_formats::Rgb {
                r: 255,
                g: 128,
                b: 0,
            },
            falloff: 0.0,
            active: true,
        };
        match convert_light(&point) {
            Light::Point {
                position,
                range,
                color,
            } => {
                assert_eq!(position, [1.0, 2.0, 3.0]);
                assert_eq!(range, POINT_LIGHT_RANGE);
                assert_eq!(color, [255.0 / 256.0, 0.5, 0.0]);
            }
            other => panic!("{other:?}"),
        }
        let spot = luma_3d::Light {
            spotlight: true,
            target: Vec3::new(1.0, 2.0, 13.0),
            ..point
        };
        match convert_light(&spot) {
            Light::Spot {
                direction,
                range,
                inner_angle,
                outer_angle,
                ..
            } => {
                assert_eq!(direction, [0.0, 0.0, 10.0]);
                assert_eq!(range, 10.0);
                assert_eq!((inner_angle, outer_angle), (0.5, 1.0));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn indices_are_sorted_by_material() {
        let mut cube = cube();
        cube.materials.push(luma_3d::Material::default());
        cube.triangle_materials = (0..12).map(|t| (t % 2) as u32).collect();
        let (indices, ranges) = sorted_indices(&cube);
        assert_eq!(indices.len(), 36);
        assert_eq!(ranges, vec![0..18, 18..36]);
        assert_eq!(&indices[0..3], &cube.triangles[0]);
        assert_eq!(&indices[3..6], &cube.triangles[2]);
        assert_eq!(&indices[18..21], &cube.triangles[1]);
    }

    #[test]
    fn the_default_shadow_is_dark_in_the_middle_and_clear_at_the_corners() {
        let image = default_shadow_image();
        let centre = image.pixel(32, 32).unwrap();
        assert!(centre.a > 180 && centre.r == 0);
        assert_eq!(image.pixel(0, 0).unwrap().a, 0);
    }
}
