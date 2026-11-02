//! A model read from a `.X` file: the frame hierarchy with its transforms,
//! the meshes (vertices, triangles, material groups, textures), the skin
//! weights per bone and the animation sets with their keys, plus a
//! sampler that evaluates a pose at a time.
//!
//! This is the data side of the original's `CXModel`: what
//! `XModel.cpp`, `XFrameNode.cpp`, `XMesh.cpp` and `XAnimation*.cpp` keep
//! after loading, built here from the generic object tree of
//! [`crate::xfile`] instead of D3DX. The subset of templates read and the
//! rules applied are described in `docs/formats/xfile.md`, section 2. The
//! runtime (animation channels, transitions, events, CPU skinning) builds
//! on these types and is not here.
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x
//! sources (`XModel.cpp`, `XFrameNode.cpp`, `XMesh.cpp`,
//! `SkinMeshHelper.cpp`, `XAnimation.cpp`, `XAnimationSet.cpp`,
//! `3DMaterial.cpp`, Copyright (c) 2003-2013 Jan Nedoma). See `NOTICE` at
//! the root of the repository.

use std::collections::HashMap;
use std::fmt;

use crate::math::{Mat4, Quat, Vec3, decompose_simple};
use crate::xfile::{self, DataObject, XFile};

/// Index of the synthetic root frame in [`Model::frames`].
pub const ROOT: usize = 0;

/// The default `AnimTicksPerSecond` when the file has none.
pub const DEFAULT_TICKS_PER_SECOND: u32 = 4800;

/// A frame (a node of the hierarchy, a bone when skin weights name it).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// The name, empty for an unnamed frame and for the root.
    pub name: String,
    /// The parent frame, `None` for the root.
    pub parent: Option<usize>,
    /// The child frames, in file order.
    pub children: Vec<usize>,
    /// The local transform as loaded (`FrameTransformMatrix`), identity
    /// without one. The pose of an animated frame replaces it.
    pub transform: Mat4,
    /// The meshes attached to the frame, as indices into [`Model::meshes`].
    pub meshes: Vec<usize>,
}

/// A vertex of a mesh: the loaded vertex format (position, normal, UV).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vertex {
    /// Position in the mesh's frame.
    pub position: Vec3,
    /// Normal.
    pub normal: Vec3,
    /// Texture coordinates, `v` downward as in Direct3D.
    pub uv: [f32; 2],
}

/// A material of a mesh, as the fixed-function pipeline sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// Diffuse colour `r, g, b, a` (`faceColor`). The ambient colour is
    /// always equal to it: the original sets `Ambient = Diffuse`.
    pub diffuse: [f32; 4],
    /// Specular exponent (`power`).
    pub power: f32,
    /// Specular colour.
    pub specular: [f32; 3],
    /// Emissive colour.
    pub emissive: [f32; 3],
    /// The texture file as written in the file (`TextureFilename`).
    pub texture_filename: Option<String>,
    /// The texture path resolved against the model file's directory:
    /// the directory part of the model path, up to and including its last
    /// `/` or `\`, followed by `texture_filename` as written.
    pub texture_path: Option<String>,
    /// The name scripts address the material by: the texture file name
    /// without path and extension. An untextured material has none.
    pub name: Option<String>,
}

impl Default for Material {
    /// The grey material a mesh without materials gets: diffuse and
    /// specular `0.5, 0.5, 0.5`, no texture.
    fn default() -> Self {
        Material {
            diffuse: [0.5, 0.5, 0.5, 0.0],
            power: 0.0,
            specular: [0.5, 0.5, 0.5],
            emissive: [0.0, 0.0, 0.0],
            texture_filename: None,
            texture_path: None,
            name: None,
        }
    }
}

/// One weight of a bone on a vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexWeight {
    /// Index into [`Mesh::vertices`].
    pub vertex: u32,
    /// The weight.
    pub weight: f32,
}

/// A bone of a skinned mesh (`SkinWeights`).
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    /// The frame name (`transformNodeName`).
    pub frame_name: String,
    /// The frame, looked up by exact name after loading; `None` when the
    /// model has no frame of that name.
    pub frame: Option<usize>,
    /// The bone offset matrix (`matrixOffset`, the inverse bind matrix).
    pub offset: Mat4,
    /// The weights, with duplicated vertices carrying their original's.
    pub weights: Vec<VertexWeight>,
}

/// The `XSkinMeshHeader` of a skinned mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinHeader {
    /// Most bones influencing one vertex.
    pub max_weights_per_vertex: u32,
    /// Most bones influencing one face.
    pub max_weights_per_face: u32,
    /// Number of bones announced.
    pub bones: u32,
}

/// The `VertexDuplicationIndices` of a mesh: which original vertex each
/// vertex of the file came from. Read and kept, not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VertexDuplication {
    /// Number of vertices before the exporter duplicated any.
    pub original_count: u32,
    /// For each vertex of the file, the original it duplicates.
    pub originals: Vec<u32>,
}

/// A mesh: triangles over a vertex list, grouped by material.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    /// The name, empty for an unnamed mesh.
    pub name: String,
    /// The frame the mesh is attached to.
    pub frame: usize,
    /// The vertices, the file's first, then the vertices Luma duplicated
    /// for corners whose normal differs between faces.
    pub vertices: Vec<Vertex>,
    /// For each vertex, the index of the file vertex it comes from.
    pub original_vertices: Vec<u32>,
    /// The triangles, clockwise front faces as in Direct3D.
    pub triangles: Vec<[u32; 3]>,
    /// For each triangle, its material index.
    pub triangle_materials: Vec<u32>,
    /// The materials; at least one.
    pub materials: Vec<Material>,
    /// The bones; empty for a rigid mesh.
    pub bones: Vec<Bone>,
    /// The skin header when the mesh has one.
    pub skin: Option<SkinHeader>,
    /// The exporter's duplication table when the mesh has one.
    pub duplication: Option<VertexDuplication>,
}

impl Mesh {
    /// Whether the mesh is skinned (has bones). A rigid mesh follows its
    /// frame instead.
    pub fn is_skinned(&self) -> bool {
        !self.bones.is_empty()
    }
}

/// A key of a vector track (scale or position).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3Key {
    /// Time in ticks.
    pub time: u32,
    /// The value.
    pub value: Vec3,
}

/// A key of the rotation track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationKey {
    /// Time in ticks.
    pub time: u32,
    /// The quaternion **as stored**: a file key `w, x, y, z` is kept as
    /// `(x, y, z, w)`; a matrix key's rotation is stored with its vector
    /// part negated. The sampler negates the vector part again before
    /// interpolating, as the original does.
    pub rotation: Quat,
}

/// The animation of one frame: up to one track of each kind.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Animation {
    /// The name of the frame it drives, empty when the file gave none.
    pub target: String,
    /// The frame, looked up by exact name after loading.
    pub frame: Option<usize>,
    /// Scale keys.
    pub scale_keys: Vec<Vec3Key>,
    /// Rotation keys.
    pub rotation_keys: Vec<RotationKey>,
    /// Position keys.
    pub position_keys: Vec<Vec3Key>,
}

/// An event attached to a frame of an animation set (from the actor
/// definition file or the `.anim` sidecar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationEvent {
    /// The script event raised.
    pub name: String,
    /// The 1-based frame, clamped to the set's frames when added.
    pub frame: i32,
}

/// A named animation set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimationSet {
    /// The name; an unnamed set takes its file's name without path and
    /// extension.
    pub name: String,
    /// The animations, one per driven frame.
    pub animations: Vec<Animation>,
    /// Whether the set loops: `AnimationOptions.openClosed != 0` in the
    /// file, or the actor definition's `LOOPING`.
    pub looping: bool,
    /// The events, in the order added.
    pub events: Vec<AnimationEvent>,
}

/// A sampled local transform: scale, rotation and position, combined as
/// `S * R * T`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Position; zero when the animation has no position track.
    pub position: Vec3,
    /// Scale; one when the animation has no scale track.
    pub scale: Vec3,
    /// Rotation; identity when the animation has no rotation track.
    pub rotation: Quat,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            position: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: Quat::IDENTITY,
        }
    }
}

impl Transform {
    /// The local matrix, `Scaling * RotationQuaternion * Translation`
    /// (`CXFrameNode::UpdateMatrices`).
    pub fn to_matrix(&self) -> Mat4 {
        Mat4::scaling(self.scale)
            * self.rotation.to_rotation_matrix()
            * Mat4::translation(self.position)
    }

    /// The blend used while transitioning between two animations: scale
    /// and position linearly, rotation by `slerp`; `t = 0` gives `self`.
    pub fn blend(&self, other: &Transform, t: f32) -> Transform {
        Transform {
            position: self.position.lerp(other.position, t),
            scale: self.scale.lerp(other.scale, t),
            rotation: self.rotation.slerp(other.rotation, t),
        }
    }
}

/// A model: frames, meshes and animation sets.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The frames; `frames[ROOT]` is the synthetic root that holds the
    /// file's top-level objects, parents come before their children.
    pub frames: Vec<Frame>,
    /// The meshes.
    pub meshes: Vec<Mesh>,
    /// The animation sets, in load order (merged files append).
    pub animation_sets: Vec<AnimationSet>,
    /// Key times per second (`AnimTicksPerSecond`, default 4800).
    pub ticks_per_second: u32,
    /// The path the model was loaded from, as given.
    pub file_name: String,
}

/// Why a model could not be built.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The container could not be read or an object lacks values.
    File(xfile::Error),
    /// The file has no top-level `Frame`.
    NoFrame,
    /// A face or a weight names a vertex the mesh does not have.
    VertexOutOfRange {
        /// The mesh's name.
        mesh: String,
        /// The index.
        index: u32,
    },
    /// A `MeshNormals` face list does not match the faces.
    NormalsMismatch {
        /// The mesh's name.
        mesh: String,
    },
    /// An animation has two tracks of the same kind.
    DuplicateTrack {
        /// The animation set.
        set: String,
        /// The track (`rotation`, `scale`, `position`, `matrix`).
        track: &'static str,
    },
    /// An animation key has an unexpected number of values.
    KeySize {
        /// The animation set.
        set: String,
        /// The key type.
        key_type: u32,
        /// The number of values found.
        values: u32,
    },
    /// Animations were merged into a model that has none.
    EmptyModel,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::File(err) => write!(f, "{err}"),
            Error::NoFrame => write!(f, "the file has no top-level Frame"),
            Error::VertexOutOfRange { mesh, index } => {
                write!(f, "mesh '{mesh}': vertex index {index} out of range")
            }
            Error::NormalsMismatch { mesh } => {
                write!(f, "mesh '{mesh}': the normal faces do not match the faces")
            }
            Error::DuplicateTrack { set, track } => {
                write!(f, "animation set '{set}': {track} key duplicated")
            }
            Error::KeySize {
                set,
                key_type,
                values,
            } => write!(
                f,
                "animation set '{set}': a key of type {key_type} has {values} values"
            ),
            Error::EmptyModel => write!(f, "cannot merge animations into an empty model"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::File(err) => Some(err),
            _ => None,
        }
    }
}

impl From<xfile::Error> for Error {
    fn from(err: xfile::Error) -> Self {
        Error::File(err)
    }
}

/// Result of the loaders of this module.
pub type Result<T> = std::result::Result<T, Error>;

/// The file name without directory and extension (`_splitpath`'s
/// `fname`), with `/` and `\` both taken as separators.
pub fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) => &name[..dot],
        None => name,
    }
}

/// The directory part of a path, up to and including its last `/` or
/// `\`; empty when the path has none.
pub fn directory_of(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(sep) => &path[..=sep],
        None => "",
    }
}

impl Model {
    /// Parses a `.X` file into a model. `file_name` is the path the
    /// model is loaded from: it resolves the texture paths and names the
    /// unnamed animation sets.
    pub fn parse(bytes: &[u8], file_name: &str) -> Result<Model> {
        let file = XFile::parse(bytes)?;
        Model::from_xfile(&file, file_name)
    }

    /// Builds a model from a parsed file. Fails when the file has no
    /// top-level `Frame`, as the original does.
    pub fn from_xfile(file: &XFile, file_name: &str) -> Result<Model> {
        let mut model = Model {
            frames: vec![Frame {
                name: String::new(),
                parent: None,
                children: Vec::new(),
                transform: Mat4::IDENTITY,
                meshes: Vec::new(),
            }],
            meshes: Vec::new(),
            animation_sets: Vec::new(),
            ticks_per_second: DEFAULT_TICKS_PER_SECOND,
            file_name: file_name.to_string(),
        };
        for object in &file.objects {
            model.load_object(file, object, ROOT)?;
        }
        if model.frames[ROOT].children.is_empty() {
            return Err(Error::NoFrame);
        }
        model.find_bones(0);
        Ok(model)
    }

    /// Adds the animation sets of another `.X` file (`MergeFromFile`):
    /// its `AnimationSet` and loose `Animation` objects, at the top level
    /// and inside its frames, whose bones are looked up in this model's
    /// frames. Its frames and meshes are not added. Returns how many sets
    /// were added.
    pub fn merge_animations(&mut self, bytes: &[u8], file_name: &str) -> Result<usize> {
        let file = XFile::parse(bytes)?;
        self.merge_animations_from(&file, file_name)
    }

    /// [`Self::merge_animations`] from a parsed file.
    pub fn merge_animations_from(&mut self, file: &XFile, file_name: &str) -> Result<usize> {
        if self.frames[ROOT].children.is_empty() {
            return Err(Error::EmptyModel);
        }
        let first = self.animation_sets.len();
        for object in &file.objects {
            self.merge_object(file, object, file_name)?;
        }
        self.find_bones(first);
        Ok(self.animation_sets.len() - first)
    }

    fn merge_object(&mut self, file: &XFile, object: &DataObject, file_name: &str) -> Result<()> {
        match object.template.as_str() {
            "AnimationSet" => self.load_animation_set(file, object, file_name),
            "Animation" => self.load_loose_animation(file, object, file_name),
            "Frame" => {
                for child in object.objects() {
                    self.merge_object(file, child, file_name)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn load_object(&mut self, file: &XFile, object: &DataObject, frame: usize) -> Result<()> {
        match object.template.as_str() {
            "Frame" => {
                let index = self.frames.len();
                self.frames.push(Frame {
                    name: object.name.clone().unwrap_or_default(),
                    parent: Some(frame),
                    children: Vec::new(),
                    transform: Mat4::IDENTITY,
                    meshes: Vec::new(),
                });
                self.frames[frame].children.push(index);
                for child in object.objects() {
                    self.load_object(file, child, index)?;
                }
                Ok(())
            }
            "FrameTransformMatrix" => {
                self.frames[frame].transform = read_matrix(&mut object.values())?;
                Ok(())
            }
            "Mesh" => {
                let mesh = load_mesh(file, object, frame, &self.file_name)?;
                let index = self.meshes.len();
                self.meshes.push(mesh);
                self.frames[frame].meshes.push(index);
                Ok(())
            }
            "AnimationSet" => {
                let file_name = self.file_name.clone();
                self.load_animation_set(file, object, &file_name)
            }
            "Animation" => {
                let file_name = self.file_name.clone();
                self.load_loose_animation(file, object, &file_name)
            }
            "AnimTicksPerSecond" => {
                self.ticks_per_second = object.values().count()?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn load_animation_set(
        &mut self,
        file: &XFile,
        object: &DataObject,
        file_name: &str,
    ) -> Result<()> {
        let mut set = AnimationSet {
            name: match object.name.as_deref() {
                Some(name) if !name.is_empty() => name.to_string(),
                _ => file_stem(file_name).to_string(),
            },
            ..AnimationSet::default()
        };
        for child in object.objects_of("Animation") {
            let animation = load_animation(file, child, &mut set)?;
            set.animations.push(animation);
        }
        self.animation_sets.push(set);
        Ok(())
    }

    /// An `Animation` outside any set becomes a set named after the file.
    fn load_loose_animation(
        &mut self,
        file: &XFile,
        object: &DataObject,
        file_name: &str,
    ) -> Result<()> {
        let mut set = AnimationSet {
            name: file_stem(file_name).to_string(),
            ..AnimationSet::default()
        };
        let animation = load_animation(file, object, &mut set)?;
        set.animations.push(animation);
        self.animation_sets.push(set);
        Ok(())
    }

    /// Resolves the bone names of the meshes and of the animation sets
    /// from `first_set` on against the frames (`FindBones`).
    fn find_bones(&mut self, first_set: usize) {
        let names: Vec<String> = self.frames.iter().map(|f| f.name.clone()).collect();
        let find = |name: &str| -> Option<usize> { (1..names.len()).find(|&i| names[i] == name) };
        if first_set == 0 {
            for mesh in &mut self.meshes {
                for bone in &mut mesh.bones {
                    bone.frame = find(&bone.frame_name);
                }
            }
        }
        for set in &mut self.animation_sets[first_set..] {
            for animation in &mut set.animations {
                animation.frame = if animation.target.is_empty() {
                    None
                } else {
                    find(&animation.target)
                };
            }
        }
    }

    /// The frame with this exact name, searching the hierarchy depth-first
    /// in file order (`CXFrameNode::FindFrame`). The root never matches.
    pub fn find_frame(&self, name: &str) -> Option<usize> {
        // Frames are stored in depth-first file order.
        (1..self.frames.len()).find(|&i| self.frames[i].name == name)
    }

    /// The animation set with this name, ignoring ASCII case
    /// (`GetAnimationSetByName`); the first when several share a name.
    pub fn animation_set(&self, name: &str) -> Option<&AnimationSet> {
        self.animation_sets
            .iter()
            .find(|set| set.name.eq_ignore_ascii_case(name))
    }

    /// Mutable [`Self::animation_set`].
    pub fn animation_set_mut(&mut self, name: &str) -> Option<&mut AnimationSet> {
        self.animation_sets
            .iter_mut()
            .find(|set| set.name.eq_ignore_ascii_case(name))
    }

    /// Removes an animation set by name, ignoring case (`UnloadAnim`).
    pub fn remove_animation_set(&mut self, name: &str) -> bool {
        match self
            .animation_sets
            .iter()
            .position(|set| set.name.eq_ignore_ascii_case(name))
        {
            Some(index) => {
                self.animation_sets.remove(index);
                true
            }
            None => false,
        }
    }

    /// Milliseconds to key ticks, as the original converts a local time
    /// before sampling: `ms * (ticks_per_second / 1000)`, truncated.
    pub fn ms_to_ticks(&self, ms: u32) -> u32 {
        (ms as f32 * (self.ticks_per_second as f32 / 1000.0)) as u32
    }

    /// The local matrices of the rest pose: every frame's loaded
    /// transform (`ResetMatrices`).
    pub fn rest_pose(&self) -> Vec<Mat4> {
        self.frames.iter().map(|frame| frame.transform).collect()
    }

    /// The combined (model-space) matrix of every frame from its local
    /// matrices: `local * parent combined`, parents first.
    pub fn combine(&self, locals: &[Mat4]) -> Vec<Mat4> {
        let mut combined = vec![Mat4::IDENTITY; self.frames.len()];
        for (index, frame) in self.frames.iter().enumerate() {
            combined[index] = match frame.parent {
                Some(parent) => locals[index] * combined[parent],
                None => locals[index],
            };
        }
        combined
    }

    /// The combined matrices of the model posed by one animation set at a
    /// local time in milliseconds: the rest pose, the set's frames
    /// overwritten by their sampled transforms, then combined.
    pub fn sample(&self, set: &AnimationSet, time_ms: u32) -> Vec<Mat4> {
        let mut locals = self.rest_pose();
        set.apply(self.ms_to_ticks(time_ms), &mut locals);
        self.combine(&locals)
    }
}

impl Animation {
    /// Samples the tracks at a time in ticks (`CXAnimation::Update`):
    /// each track takes the first key later than `time` and the key
    /// before it and interpolates between them (linearly for scale and
    /// position, `slerp` for the rotation, whose vector part is negated
    /// first); a time before the first key gives the first key; when no
    /// key is later than `time` both indices stay 0, so the pose snaps to
    /// the **first** key. `None` when the animation has no track.
    pub fn sample(&self, time: u32) -> Option<Transform> {
        if self.scale_keys.is_empty()
            && self.rotation_keys.is_empty()
            && self.position_keys.is_empty()
        {
            return None;
        }
        let mut result = Transform::default();
        if let Some((a, b, t)) = key_pair(&self.scale_keys, |k| k.time, time) {
            result.scale = a.value.lerp(b.value, t);
        }
        if let Some((a, b, t)) = key_pair(&self.rotation_keys, |k| k.time, time) {
            result.rotation = a.rotation.conjugate().slerp(b.rotation.conjugate(), t);
        }
        if let Some((a, b, t)) = key_pair(&self.position_keys, |k| k.time, time) {
            result.position = a.value.lerp(b.value, t);
        }
        Some(result)
    }

    /// The largest key time of the tracks, in ticks (`GetTotalTime`): the
    /// last key of each track.
    pub fn total_time(&self) -> u32 {
        let last = |times: &[u32]| times.last().copied().unwrap_or(0);
        let rot: Vec<u32> = self.rotation_keys.iter().map(|k| k.time).collect();
        let pos: Vec<u32> = self.position_keys.iter().map(|k| k.time).collect();
        let scale: Vec<u32> = self.scale_keys.iter().map(|k| k.time).collect();
        last(&rot).max(last(&pos)).max(last(&scale))
    }

    /// The shortest gap between consecutive keys of any track, in ticks
    /// (`GetFrameTime`), with the original's scan: the gap before the
    /// first key counts from time 0, and a zero gap found after a
    /// positive one resets the result, which the next gap then replaces.
    pub fn frame_time(&self) -> u32 {
        let mut frame_time: i64 = 0;
        let mut scan = |times: &[u32]| {
            let mut prev: i64 = 0;
            for &time in times {
                let gap = i64::from(time) - prev;
                if frame_time == 0 || gap < frame_time {
                    frame_time = gap;
                }
                prev = i64::from(time);
            }
        };
        let rot: Vec<u32> = self.rotation_keys.iter().map(|k| k.time).collect();
        let pos: Vec<u32> = self.position_keys.iter().map(|k| k.time).collect();
        let scale: Vec<u32> = self.scale_keys.iter().map(|k| k.time).collect();
        scan(&rot);
        scan(&pos);
        scan(&scale);
        frame_time.max(0) as u32
    }
}

/// The two keys around `time` and the interpolation factor between them.
fn key_pair<K>(keys: &[K], time_of: impl Fn(&K) -> u32, time: u32) -> Option<(&K, &K, f32)> {
    if keys.is_empty() {
        return None;
    }
    let (mut i1, mut i2) = (0usize, 0usize);
    if let Some(i) = keys.iter().position(|k| time_of(k) > time) {
        i2 = i;
        i1 = i.saturating_sub(1);
    }
    let (t1, t2) = (time_of(&keys[i1]), time_of(&keys[i2]));
    let t = if t2 == t1 {
        0.0
    } else {
        (time as f32 - t1 as f32) / (t2 as f32 - t1 as f32)
    };
    Some((&keys[i1], &keys[i2], t))
}

impl AnimationSet {
    /// The length of the set in milliseconds (`GetTotalTime`): the largest
    /// total time of its animations divided by `ticks_per_second / 1000`,
    /// truncated.
    pub fn total_time_ms(&self, ticks_per_second: u32) -> u32 {
        let scale = ticks_per_second as f32 / 1000.0;
        self.animations
            .iter()
            .map(|a| (a.total_time() as f32 / scale) as u32)
            .max()
            .unwrap_or(0)
    }

    /// The frame time of the set in milliseconds (`GetFrameTime`): the
    /// first animation's frame time, then the minimum over the animations
    /// whose frame time is positive, each divided by `ticks_per_second /
    /// 1000` and truncated. "Frames" only place events.
    pub fn frame_time_ms(&self, ticks_per_second: u32) -> u32 {
        let scale = ticks_per_second as f32 / 1000.0;
        let mut frame_time: u32 = 0;
        for animation in &self.animations {
            let ticks = animation.frame_time();
            if frame_time == 0 {
                frame_time = (ticks as f32 / scale) as u32;
            } else if ticks > 0 {
                frame_time = frame_time.min((ticks as f32 / scale) as u32);
            }
        }
        frame_time
    }

    /// The number of frames events can name: `total / frame_time + 1`,
    /// or 0 when the frame time is 0.
    pub fn total_frames(&self, ticks_per_second: u32) -> i32 {
        let frame_time = self.frame_time_ms(ticks_per_second);
        match self.total_time_ms(ticks_per_second).checked_div(frame_time) {
            Some(frames) => frames as i32 + 1,
            None => 0,
        }
    }

    /// Adds an event (`AddEvent`): the frame is clamped to at least 1,
    /// then to at most [`Self::total_frames`].
    pub fn add_event(&mut self, name: &str, frame: i32, ticks_per_second: u32) {
        let total = self.total_frames(ticks_per_second);
        let frame = frame.max(1).min(total);
        self.events.push(AnimationEvent {
            name: name.to_string(),
            frame,
        });
    }

    /// The events raised when the frame index goes from `previous` to
    /// `current` (`OnFrameChanged`), in order: after a wrap (`previous >
    /// current`) first those with `frame > previous`, then those with
    /// `previous < frame <= current` (with `previous` taken as -1 after
    /// a wrap).
    pub fn events_between(&self, current: i32, previous: i32) -> Vec<&str> {
        let mut names = Vec::new();
        let mut previous = previous;
        if previous > current {
            names.extend(
                self.events
                    .iter()
                    .filter(|e| e.frame > previous)
                    .map(|e| e.name.as_str()),
            );
            previous = -1;
        }
        names.extend(
            self.events
                .iter()
                .filter(|e| e.frame > previous && e.frame <= current)
                .map(|e| e.name.as_str()),
        );
        names
    }

    /// Samples every animation at a time in ticks and writes the local
    /// matrix of each resolved frame into `locals` (indexed like
    /// [`Model::frames`]). Frames the set does not animate keep what
    /// `locals` holds.
    pub fn apply(&self, time: u32, locals: &mut [Mat4]) {
        for (frame, transform) in self.sample(time) {
            if let Some(local) = locals.get_mut(frame) {
                *local = transform.to_matrix();
            }
        }
    }

    /// The transforms of the resolved frames at a time in ticks.
    pub fn sample(&self, time: u32) -> Vec<(usize, Transform)> {
        self.animations
            .iter()
            .filter_map(|animation| {
                let frame = animation.frame?;
                animation.sample(time).map(|t| (frame, t))
            })
            .collect()
    }
}

fn read_matrix(values: &mut xfile::Values<'_>) -> xfile::Result<Mat4> {
    let mut m = [[0.0f32; 4]; 4];
    for row in &mut m {
        for cell in row.iter_mut() {
            *cell = values.float()?;
        }
    }
    Ok(Mat4::from_rows(m))
}

fn read_vec3(values: &mut xfile::Values<'_>) -> xfile::Result<Vec3> {
    let [x, y, z] = values.vec3()?;
    Ok(Vec3::new(x, y, z))
}

/// Loads an `Animation`: its frame reference, its `AnimationKey` tracks
/// and its `AnimationOptions`, which may set the whole set looping.
fn load_animation(file: &XFile, object: &DataObject, set: &mut AnimationSet) -> Result<Animation> {
    let mut animation = Animation::default();
    for reference in object.references() {
        let Some(name) = reference.name.as_deref() else {
            continue;
        };
        // The reference must be to a frame; an unresolvable one is taken
        // as a frame name (D3DX would reject the file).
        let is_frame = file.find(name).is_none_or(|o| o.template == "Frame");
        if is_frame {
            animation.target = name.to_string();
        }
    }
    for child in object.objects() {
        match child.template.as_str() {
            "AnimationKey" => load_animation_key(child, &mut animation, &set.name)?,
            "AnimationOptions" => {
                let mut values = child.values();
                let open_closed = values.count()?;
                if open_closed != 0 {
                    set.looping = true;
                }
            }
            _ => {}
        }
    }
    Ok(animation)
}

fn load_animation_key(object: &DataObject, animation: &mut Animation, set: &str) -> Result<()> {
    let mut values = object.values();
    let key_type = values.count()?;
    let count = values.count()?;
    let duplicate = |track| Error::DuplicateTrack {
        set: set.to_string(),
        track,
    };
    let expected = match key_type {
        0 => 4,
        1 | 2 => 3,
        4 => 16,
        // Unknown key types are logged and ignored by the original.
        _ => return Ok(()),
    };
    match key_type {
        0 if !animation.rotation_keys.is_empty() => return Err(duplicate("rotation")),
        1 if !animation.scale_keys.is_empty() => return Err(duplicate("scale")),
        2 if !animation.position_keys.is_empty() => return Err(duplicate("position")),
        4 if !animation.rotation_keys.is_empty()
            || !animation.scale_keys.is_empty()
            || !animation.position_keys.is_empty() =>
        {
            return Err(duplicate("matrix"));
        }
        _ => {}
    }
    for _ in 0..count {
        let time = values.count()?;
        let n = values.count()?;
        if n != expected {
            return Err(Error::KeySize {
                set: set.to_string(),
                key_type,
                values: n,
            });
        }
        match key_type {
            0 => {
                // The file writes w, x, y, z.
                let w = values.float()?;
                let x = values.float()?;
                let y = values.float()?;
                let z = values.float()?;
                animation.rotation_keys.push(RotationKey {
                    time,
                    rotation: Quat::new(x, y, z, w),
                });
            }
            1 => animation.scale_keys.push(Vec3Key {
                time,
                value: read_vec3(&mut values)?,
            }),
            2 => animation.position_keys.push(Vec3Key {
                time,
                value: read_vec3(&mut values)?,
            }),
            _ => {
                let matrix = read_matrix(&mut values)?;
                let (position, scale, rotation) = decompose_simple(&matrix);
                animation.position_keys.push(Vec3Key {
                    time,
                    value: position,
                });
                animation.scale_keys.push(Vec3Key { time, value: scale });
                animation.rotation_keys.push(RotationKey {
                    time,
                    rotation: rotation.conjugate(),
                });
            }
        }
    }
    Ok(())
}

/// Loads a `Mesh` and its children into a [`Mesh`].
fn load_mesh(file: &XFile, object: &DataObject, frame: usize, model_path: &str) -> Result<Mesh> {
    let name = object.name.clone().unwrap_or_default();
    let out_of_range = |index: u32| Error::VertexOutOfRange {
        mesh: name.clone(),
        index,
    };

    let mut values = object.values();
    let vertex_count = values.count()?;
    let mut positions = Vec::with_capacity(vertex_count as usize);
    for _ in 0..vertex_count {
        positions.push(read_vec3(&mut values)?);
    }
    let face_count = values.count()?;
    let mut faces: Vec<Vec<u32>> = Vec::with_capacity(face_count as usize);
    for _ in 0..face_count {
        let n = values.count()?;
        let mut face = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let index = values.count()?;
            if index >= vertex_count {
                return Err(out_of_range(index));
            }
            face.push(index);
        }
        faces.push(face);
    }

    // Normals: one list, indexed per face corner.
    let mut normals: Option<(Vec<Vec3>, Vec<Vec<u32>>)> = None;
    if let Some(child) = object.child("MeshNormals") {
        let mut values = child.values();
        let count = values.count()?;
        let mut list = Vec::with_capacity(count as usize);
        for _ in 0..count {
            list.push(read_vec3(&mut values)?);
        }
        let normal_faces = values.count()?;
        if normal_faces != face_count {
            return Err(Error::NormalsMismatch { mesh: name.clone() });
        }
        let mut per_face = Vec::with_capacity(normal_faces as usize);
        for face in &faces {
            let n = values.count()?;
            if n as usize != face.len() {
                return Err(Error::NormalsMismatch { mesh: name.clone() });
            }
            let mut corners = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let index = values.count()?;
                if index >= count {
                    return Err(Error::NormalsMismatch { mesh: name.clone() });
                }
                corners.push(index);
            }
            per_face.push(corners);
        }
        normals = Some((list, per_face));
    }

    // Texture coordinates: one per vertex.
    let mut uvs = vec![[0.0f32; 2]; vertex_count as usize];
    if let Some(child) = object.child("MeshTextureCoords") {
        let mut values = child.values();
        let count = values.count()?;
        for uv in uvs.iter_mut().take(count as usize) {
            *uv = [values.float()?, values.float()?];
        }
        for _ in vertex_count..count {
            values.float()?;
            values.float()?;
        }
    }

    // Build the vertex list, duplicating a vertex whose normal index
    // differs between its faces.
    let mut vertices: Vec<Vertex> = positions
        .iter()
        .zip(&uvs)
        .map(|(&position, &uv)| Vertex {
            position,
            normal: Vec3::ZERO,
            uv,
        })
        .collect();
    let mut original_vertices: Vec<u32> = (0..vertex_count).collect();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut triangle_faces: Vec<usize> = Vec::new();
    match &normals {
        Some((list, per_face)) => {
            let mut assigned: Vec<Option<u32>> = vec![None; vertex_count as usize];
            let mut duplicates: HashMap<(u32, u32), u32> = HashMap::new();
            for (face_index, (face, corners)) in faces.iter().zip(per_face).enumerate() {
                let mut resolved = Vec::with_capacity(face.len());
                for (&position, &normal) in face.iter().zip(corners) {
                    let vertex = match assigned[position as usize] {
                        None => {
                            assigned[position as usize] = Some(normal);
                            vertices[position as usize].normal = list[normal as usize];
                            position
                        }
                        Some(first) if first == normal => position,
                        Some(_) => *duplicates.entry((position, normal)).or_insert_with(|| {
                            let index = vertices.len() as u32;
                            vertices.push(Vertex {
                                normal: list[normal as usize],
                                ..vertices[position as usize]
                            });
                            original_vertices.push(position);
                            index
                        }),
                    };
                    resolved.push(vertex);
                }
                triangulate(&resolved, face_index, &mut triangles, &mut triangle_faces);
            }
        }
        None => {
            for (face_index, face) in faces.iter().enumerate() {
                triangulate(face, face_index, &mut triangles, &mut triangle_faces);
            }
            // No normals in the file: smooth normals from the faces, a
            // Luma choice (the original's vertex format would have none).
            let mut sums = vec![Vec3::ZERO; vertices.len()];
            for tri in &triangles {
                let [a, b, c] = tri.map(|i| vertices[i as usize].position);
                let normal = (b - a).cross(c - a);
                for &i in tri {
                    sums[i as usize] += normal;
                }
            }
            for (vertex, sum) in vertices.iter_mut().zip(sums) {
                vertex.normal = sum.normalized();
            }
        }
    }

    // Materials: per-face indices, materials inline or by reference.
    let mut materials = Vec::new();
    let mut face_materials: Vec<u32> = Vec::new();
    if let Some(child) = object.child("MeshMaterialList") {
        let mut values = child.values();
        // The material count is implied by the materials that follow.
        values.count()?;
        let index_count = values.count()?;
        for _ in 0..index_count {
            face_materials.push(values.count()?);
        }
        for item in &child.children {
            let material = match item {
                xfile::Child::Object(o) if o.template == "Material" => Some(o),
                xfile::Child::Reference(r) => r
                    .name
                    .as_deref()
                    .and_then(|n| file.find(n))
                    .filter(|o| o.template == "Material"),
                xfile::Child::Object(_) => None,
            };
            if let Some(material) = material {
                materials.push(load_material(material, model_path)?);
            }
        }
    }
    if materials.is_empty() {
        materials.push(Material::default());
        face_materials.clear();
    }
    let last_material = materials.len() as u32 - 1;
    let triangle_materials: Vec<u32> = triangle_faces
        .iter()
        .map(|&face| {
            // A list shorter than the faces repeats its last entry (a
            // single entry means "all faces"); indices past the materials
            // are clamped.
            face_materials
                .get(face)
                .or(face_materials.last())
                .copied()
                .unwrap_or(0)
                .min(last_material)
        })
        .collect();

    // Skinning.
    let skin = match object.child("XSkinMeshHeader") {
        Some(child) => {
            let mut values = child.values();
            Some(SkinHeader {
                max_weights_per_vertex: values.count()?,
                max_weights_per_face: values.count()?,
                bones: values.count()?,
            })
        }
        None => None,
    };
    let mut duplicates_of: Vec<Vec<u32>> = vec![Vec::new(); vertex_count as usize];
    for (index, &original) in original_vertices
        .iter()
        .enumerate()
        .skip(vertex_count as usize)
    {
        duplicates_of[original as usize].push(index as u32);
    }
    let mut bones = Vec::new();
    for child in object.objects_of("SkinWeights") {
        let mut values = child.values();
        let frame_name = values.string()?;
        let count = values.count()?;
        let mut indices = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let index = values.count()?;
            if index >= vertex_count {
                return Err(out_of_range(index));
            }
            indices.push(index);
        }
        let mut weights = Vec::with_capacity(count as usize);
        for &vertex in &indices {
            let weight = values.float()?;
            weights.push(VertexWeight { vertex, weight });
            for &duplicate in &duplicates_of[vertex as usize] {
                weights.push(VertexWeight {
                    vertex: duplicate,
                    weight,
                });
            }
        }
        let offset = read_matrix(&mut values)?;
        bones.push(Bone {
            frame_name,
            frame: None,
            offset,
            weights,
        });
    }

    let duplication = match object.child("VertexDuplicationIndices") {
        Some(child) => {
            let mut values = child.values();
            let count = values.count()?;
            let original_count = values.count()?;
            let mut originals = Vec::with_capacity(count as usize);
            for _ in 0..count {
                originals.push(values.count()?);
            }
            Some(VertexDuplication {
                original_count,
                originals,
            })
        }
        None => None,
    };

    Ok(Mesh {
        name,
        frame,
        vertices,
        original_vertices,
        triangles,
        triangle_materials,
        materials,
        bones,
        skin,
        duplication,
    })
}

/// Splits a polygon into a fan of triangles `(0, i, i + 1)`.
fn triangulate(
    face: &[u32],
    face_index: usize,
    triangles: &mut Vec<[u32; 3]>,
    faces: &mut Vec<usize>,
) {
    for i in 1..face.len().saturating_sub(1) {
        triangles.push([face[0], face[i], face[i + 1]]);
        faces.push(face_index);
    }
}

fn load_material(object: &DataObject, model_path: &str) -> Result<Material> {
    let mut values = object.values();
    let diffuse = [
        values.float()?,
        values.float()?,
        values.float()?,
        values.float()?,
    ];
    let power = values.float()?;
    let specular = values.vec3()?;
    let emissive = values.vec3()?;
    let texture_filename = match object.child("TextureFilename") {
        Some(child) => Some(child.values().string()?),
        None => None,
    };
    let texture_path = texture_filename
        .as_deref()
        .map(|file| format!("{}{}", directory_of(model_path), file));
    let name = texture_filename
        .as_deref()
        .map(|file| file_stem(file).to_string());
    Ok(Material {
        diffuse,
        power,
        specular,
        emissive,
        texture_filename,
        texture_path,
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::PI;

    /// A quad in the XZ plane, skinned to two bones: `Root` holds the
    /// left edge (x = 0), its child `Tip` the right edge (x = 2), with
    /// `Tip` placed 2 units along +X. The animation `swing` rotates `Tip`
    /// about Y by 90 degrees between ticks 0 and 4800 (one second) and
    /// moves `Root` up by 10 over the same time.
    const QUAD: &str = r#"xof 0303txt 0032
template Frame {
 <3d82ab46-62da-11cf-ab39-0020af71e433>
 [...]
}

AnimTicksPerSecond {
 4800;
}

Frame Root {
 FrameTransformMatrix {
  1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;;
 }
 Frame Tip {
  FrameTransformMatrix {
   1,0,0,0, 0,1,0,0, 0,0,1,0, 2,0,0,1;;
  }
 }
 Mesh quad {
  4;
  0.0;0.0;0.0;,
  2.0;0.0;0.0;,
  2.0;0.0;1.0;,
  0.0;0.0;1.0;;
  1;
  4;0,1,2,3;;

  MeshNormals {
   1;
   0.0;1.0;0.0;;
   1;
   4;0,0,0,0;;
  }

  MeshTextureCoords {
   4;
   0.0;0.0;,
   1.0;0.0;,
   1.0;1.0;,
   0.0;1.0;;
  }

  MeshMaterialList {
   1;
   1;
   0;

   Material {
    0.800000;0.800000;0.800000;1.000000;;
    0.000000;
    0.000000;0.000000;0.000000;;
    0.000000;0.000000;0.000000;;

    TextureFilename {
     "skin.bmp";
    }
   }
  }

  XSkinMeshHeader {
   1;
   2;
   2;
  }

  SkinWeights {
   "Root";
   2;
   0,
   3;
   1.000000,
   1.000000;
   1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;;
  }

  SkinWeights {
   "Tip";
   2;
   1,
   2;
   1.000000,
   1.000000;
   1,0,0,0, 0,1,0,0, 0,0,1,0, -2,0,0,1;;
  }
 }
}

AnimationSet swing {
 Animation {
  AnimationKey {
   0;
   2;
   0;4;1.0,0.0,0.0,0.0;;,
   4800;4;0.70710678,0.0,0.70710678,0.0;;;
  }
  AnimationKey {
   2;
   2;
   0;3;2.0,0.0,0.0;;,
   4800;3;2.0,0.0,0.0;;;
  }
  AnimationOptions {
   1;
   0;
  }
  { Tip }
 }
 Animation {
  AnimationKey {
   2;
   3;
   0;3;0.0,0.0,0.0;;,
   2400;3;0.0,5.0,0.0;;,
   4800;3;0.0,10.0,0.0;;;
  }
  { Root }
 }
}
"#;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn assert_vec(v: Vec3, x: f32, y: f32, z: f32) {
        assert!(
            close(v.x, x) && close(v.y, y) && close(v.z, z),
            "expected ({x}, {y}, {z}), got {v:?}"
        );
    }

    #[test]
    fn loads_the_frame_hierarchy_and_the_mesh() {
        let model = Model::parse(QUAD.as_bytes(), "actors\\quad\\quad.x").unwrap();
        assert_eq!(model.ticks_per_second, 4800);
        assert_eq!(model.frames.len(), 3);
        assert_eq!(model.frames[ROOT].children, vec![1]);
        assert_eq!(model.frames[1].name, "Root");
        assert_eq!(model.frames[1].parent, Some(ROOT));
        assert_eq!(model.frames[1].children, vec![2]);
        assert_eq!(model.frames[1].meshes, vec![0]);
        assert_eq!(model.frames[2].name, "Tip");
        assert_eq!(model.frames[2].parent, Some(1));
        assert_vec(model.frames[2].transform.translation_part(), 2.0, 0.0, 0.0);
        assert_eq!(model.find_frame("Tip"), Some(2));
        assert_eq!(model.find_frame("tip"), None);
        assert_eq!(model.find_frame(""), None);

        let mesh = &model.meshes[0];
        assert_eq!(mesh.name, "quad");
        assert_eq!(mesh.frame, 1);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.original_vertices, vec![0, 1, 2, 3]);
        assert_eq!(mesh.triangles, vec![[0, 1, 2], [0, 2, 3]]);
        assert_eq!(mesh.triangle_materials, vec![0, 0]);
        assert_vec(mesh.vertices[2].position, 2.0, 0.0, 1.0);
        assert_vec(mesh.vertices[2].normal, 0.0, 1.0, 0.0);
        assert_eq!(mesh.vertices[2].uv, [1.0, 1.0]);
        assert_eq!(mesh.skin.unwrap().bones, 2);
        assert!(mesh.is_skinned());
        assert_eq!(mesh.duplication, None);

        assert_eq!(mesh.materials.len(), 1);
        let material = &mesh.materials[0];
        assert_eq!(material.diffuse, [0.8, 0.8, 0.8, 1.0]);
        assert_eq!(material.texture_filename.as_deref(), Some("skin.bmp"));
        assert_eq!(
            material.texture_path.as_deref(),
            Some("actors\\quad\\skin.bmp")
        );
        assert_eq!(material.name.as_deref(), Some("skin"));

        assert_eq!(mesh.bones.len(), 2);
        assert_eq!(mesh.bones[0].frame_name, "Root");
        assert_eq!(mesh.bones[0].frame, Some(1));
        assert_eq!(
            mesh.bones[0].weights,
            vec![
                VertexWeight {
                    vertex: 0,
                    weight: 1.0
                },
                VertexWeight {
                    vertex: 3,
                    weight: 1.0
                }
            ]
        );
        assert_eq!(mesh.bones[1].frame, Some(2));
        assert_vec(mesh.bones[1].offset.translation_part(), -2.0, 0.0, 0.0);
    }

    #[test]
    fn loads_the_animation_keys_as_stored() {
        let model = Model::parse(QUAD.as_bytes(), "quad.x").unwrap();
        assert_eq!(model.animation_sets.len(), 1);
        let set = &model.animation_sets[0];
        assert_eq!(set.name, "swing");
        assert!(set.looping);
        assert_eq!(set.animations.len(), 2);
        let tip = &set.animations[0];
        assert_eq!(tip.target, "Tip");
        assert_eq!(tip.frame, Some(2));
        assert_eq!(tip.rotation_keys.len(), 2);
        // The file writes w, x, y, z; the key keeps (x, y, z, w).
        let q = tip.rotation_keys[1].rotation;
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!(close(q.x, 0.0) && close(q.y, half) && close(q.z, 0.0));
        assert!(close(q.w, half));
        assert_eq!(tip.rotation_keys[1].time, 4800);
        assert_eq!(tip.position_keys.len(), 2);
        assert!(tip.scale_keys.is_empty());
        let root = &set.animations[1];
        assert_eq!(root.frame, Some(1));
        assert_eq!(root.position_keys.len(), 3);
        assert_eq!(tip.total_time(), 4800);
        assert_eq!(root.frame_time(), 2400);
        assert_eq!(tip.frame_time(), 4800);
        // `4800 ticks / (4800 / 1000)` in single precision: `4.8f32` is
        // slightly above 4.8, so the quotient is just under 1000 and the
        // original's truncation gives 999 ms (and 499 for 2400 ticks).
        assert_eq!(set.total_time_ms(4800), 999);
        assert_eq!(set.frame_time_ms(4800), 499);
        assert_eq!(set.total_frames(4800), 3);
        assert!(model.animation_set("SWING").is_some());
        assert!(model.animation_set("walk").is_none());
    }

    #[test]
    fn samples_a_pose_with_the_negated_quaternion() {
        let model = Model::parse(QUAD.as_bytes(), "quad.x").unwrap();
        let set = &model.animation_sets[0];
        assert_eq!(model.ms_to_ticks(500), 2400);
        assert_eq!(model.ms_to_ticks(1), 4);

        // Halfway: Root has risen by 5, Tip has turned by 45 degrees. The
        // file's quaternion is a rotation of +90 degrees about Y; the
        // original negates its vector part, so the frame turns by -90,
        // and halfway by -45.
        let tip = set.animations[0].sample(2400).unwrap();
        let expected = Mat4::rotation_y(-PI / 4.0) * Mat4::translation(Vec3::new(2.0, 0.0, 0.0));
        for r in 0..4 {
            for c in 0..4 {
                assert!(
                    close(tip.to_matrix().m[r][c], expected.m[r][c]),
                    "cell {r},{c}"
                );
            }
        }
        let root = set.animations[1].sample(2400).unwrap();
        assert_vec(root.position, 0.0, 5.0, 0.0);
        assert_vec(root.scale, 1.0, 1.0, 1.0);

        let combined = model.sample(set, 500);
        assert_vec(combined[1].translation_part(), 0.0, 5.0, 0.0);
        assert_vec(combined[2].translation_part(), 2.0, 5.0, 0.0);
        // A point one unit along Tip's X axis ends up turned by -45
        // degrees about Y, that is toward +Z in a left-handed frame.
        let p = combined[2].transform_coord(Vec3::new(1.0, 0.0, 0.0));
        let s = (PI / 4.0).sin();
        assert_vec(p, 2.0 + s, 5.0, s);

        // Before the first key: the first key. At a time past the last
        // key: both indices stay 0, the first key again.
        let start = set.animations[1].sample(0).unwrap();
        assert_vec(start.position, 0.0, 0.0, 0.0);
        let past = set.animations[1].sample(9000).unwrap();
        assert_vec(past.position, 0.0, 0.0, 0.0);
        // Interpolation within the second segment.
        let late = set.animations[1].sample(3600).unwrap();
        assert_vec(late.position, 0.0, 7.5, 0.0);

        // The rest pose leaves every frame at its loaded matrix.
        let rest = model.combine(&model.rest_pose());
        assert_vec(rest[2].translation_part(), 2.0, 0.0, 0.0);
        assert_eq!(rest[ROOT], Mat4::IDENTITY);
    }

    #[test]
    fn blends_two_transforms() {
        let a = Transform {
            position: Vec3::new(0.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: Quat::IDENTITY,
        };
        let b = Transform {
            position: Vec3::new(10.0, 0.0, 0.0),
            scale: Vec3::new(3.0, 3.0, 3.0),
            rotation: Quat::from_rotation_matrix(&Mat4::rotation_y(PI / 2.0)),
        };
        let half = a.blend(&b, 0.5);
        assert_vec(half.position, 5.0, 0.0, 0.0);
        assert_vec(half.scale, 2.0, 2.0, 2.0);
        let expected = Mat4::rotation_y(PI / 4.0);
        let got = half.rotation.to_rotation_matrix();
        assert!(close(got.m[0][0], expected.m[0][0]) && close(got.m[0][2], expected.m[0][2]));
        assert_eq!(a.blend(&b, 0.0), a);
    }

    #[test]
    fn events_are_clamped_and_reported_in_order() {
        let mut model = Model::parse(QUAD.as_bytes(), "quad.x").unwrap();
        let tps = model.ticks_per_second;
        let set = model.animation_set_mut("swing").unwrap();
        set.add_event("step", 2, tps);
        set.add_event("early", 0, tps);
        set.add_event("late", 99, tps);
        assert_eq!(
            set.events.iter().map(|e| e.frame).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
        assert_eq!(set.events_between(1, 0), vec!["early"]);
        assert_eq!(set.events_between(2, 1), vec!["step"]);
        assert_eq!(set.events_between(3, 2), vec!["late"]);
        assert_eq!(set.events_between(1, 3), vec!["early"]);
        assert_eq!(set.events_between(1, 2), vec!["late", "early"]);
        assert!(set.events_between(2, 2).is_empty());
    }

    #[test]
    fn merges_animation_sets_from_another_file() {
        let mut model = Model::parse(QUAD.as_bytes(), "quad.x").unwrap();
        let extra = r#"xof 0303txt 0032
Frame Root {
 Frame Tip {
  AnimationSet {
   Animation {
    { Tip }
    AnimationKey {
     1;
     1;
     0;3;2.0,2.0,2.0;;;
    }
   }
  }
 }
 Mesh ignored {
  3;
  0;0;0;, 1;0;0;, 0;1;0;;
  1;
  3;0,1,2;;
 }
}
Animation {
 { Nowhere }
 AnimationKey {
  2;
  1;
  0;3;1.0,0.0,0.0;;;
 }
}
"#;
        let added = model
            .merge_animations(extra.as_bytes(), "data/anims/Wave.X")
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(model.frames.len(), 3, "merging adds no frames");
        assert_eq!(model.meshes.len(), 1, "merging adds no meshes");
        assert_eq!(model.animation_sets.len(), 3);
        // Unnamed sets and loose animations take the file's name.
        assert_eq!(model.animation_sets[1].name, "Wave");
        assert_eq!(model.animation_sets[1].animations[0].frame, Some(2));
        assert_eq!(model.animation_sets[2].name, "Wave");
        assert_eq!(model.animation_sets[2].animations[0].target, "Nowhere");
        assert_eq!(model.animation_sets[2].animations[0].frame, None);
        assert!(model.animation_sets[2].sample(0).is_empty());
        // A scale-only animation leaves position and rotation at rest.
        let t = model.animation_sets[1].animations[0].sample(0).unwrap();
        assert_vec(t.scale, 2.0, 2.0, 2.0);
        assert_vec(t.position, 0.0, 0.0, 0.0);
        assert!(model.remove_animation_set("wave"));
        assert_eq!(model.animation_sets.len(), 2);
        assert!(!model.remove_animation_set("none"));
    }

    #[test]
    fn matrix_keys_are_decomposed() {
        let text = r#"xof 0303txt 0032
Frame Bone { }
AnimationSet {
 Animation {
  { Bone }
  AnimationKey {
   4;
   1;
   0;16;0,0,-1,0, 0,1,0,0, 1,0,0,0, 3,4,5,1;;;
  }
 }
}
"#;
        let model = Model::parse(text.as_bytes(), "turn.x").unwrap();
        let set = &model.animation_sets[0];
        assert_eq!(set.name, "turn");
        assert!(!set.looping);
        let animation = &set.animations[0];
        assert_eq!(animation.position_keys[0].value, Vec3::new(3.0, 4.0, 5.0));
        assert_vec(animation.scale_keys[0].value, 1.0, 1.0, 1.0);
        // The matrix is a rotation about Y; negated on load and again on
        // use, the sampled rotation is the matrix's own.
        let sampled = animation.sample(0).unwrap().to_matrix();
        let expected = Mat4::from_rows([
            [0.0, 0.0, -1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [3.0, 4.0, 5.0, 1.0],
        ]);
        for r in 0..4 {
            for c in 0..4 {
                assert!(close(sampled.m[r][c], expected.m[r][c]), "cell {r},{c}");
            }
        }
    }

    #[test]
    fn rejects_duplicate_tracks_and_bad_files() {
        let dup = r#"xof 0303txt 0032
Frame F { }
AnimationSet s {
 Animation {
  { F }
  AnimationKey { 2; 1; 0;3;0,0,0;;; }
  AnimationKey { 2; 1; 0;3;0,0,0;;; }
 }
}
"#;
        assert_eq!(
            Model::parse(dup.as_bytes(), "d.x").unwrap_err(),
            Error::DuplicateTrack {
                set: "s".into(),
                track: "position"
            }
        );
        let matrix_after_pos = dup.replace(
            "AnimationKey { 2; 1; 0;3;0,0,0;;; }\n }",
            "AnimationKey { 4; 0; ; }\n }",
        );
        assert_eq!(
            Model::parse(matrix_after_pos.as_bytes(), "d.x").unwrap_err(),
            Error::DuplicateTrack {
                set: "s".into(),
                track: "matrix"
            }
        );
        let bad_size = "xof 0303txt 0032\nFrame F { }\nAnimationSet s { Animation { AnimationKey { 0; 1; 0;3;0,0,0;;; } } }";
        assert_eq!(
            Model::parse(bad_size.as_bytes(), "d.x").unwrap_err(),
            Error::KeySize {
                set: "s".into(),
                key_type: 0,
                values: 3
            }
        );
        // Unknown key types are ignored.
        let unknown = "xof 0303txt 0032\nFrame F { }\nAnimationSet s { Animation { { F } AnimationKey { 3; 1; 0;1;0;;; } } }";
        let model = Model::parse(unknown.as_bytes(), "d.x").unwrap();
        assert!(model.animation_sets[0].animations[0].sample(0).is_none());

        assert_eq!(
            Model::parse(b"xof 0303txt 0032\nMesh { 0; 0; }", "m.x").unwrap_err(),
            Error::NoFrame
        );
        assert_eq!(
            Model::parse(b"xof 0303txt 0032\nAnimTicksPerSecond { 100; }", "m.x").unwrap_err(),
            Error::NoFrame
        );
        assert!(matches!(
            Model::parse(b"xof 0303bin 0032", "m.x").unwrap_err(),
            Error::File(_)
        ));
        let out_of_range = "xof 0303txt 0032\nFrame F { Mesh { 1; 0;0;0;; 1; 3;0,1,2;; } }";
        assert_eq!(
            Model::parse(out_of_range.as_bytes(), "m.x").unwrap_err(),
            Error::VertexOutOfRange {
                mesh: String::new(),
                index: 1
            }
        );
        let mut empty = Model::parse(QUAD.as_bytes(), "q.x").unwrap();
        empty.frames[ROOT].children.clear();
        assert_eq!(
            empty.merge_animations(QUAD.as_bytes(), "q.x").unwrap_err(),
            Error::EmptyModel
        );
    }

    #[test]
    fn splits_vertices_whose_normals_differ_and_keeps_their_weights() {
        // Two triangles sharing the edge 1-2 with different normals: a
        // fold. Vertices 1 and 2 are duplicated for the second face.
        let text = r#"xof 0303txt 0032
Frame F {
 Mesh fold {
  4;
  0;0;0;, 1;0;0;, 1;0;1;, 2;1;0;;
  2;
  3;0,1,2;,
  3;1,3,2;;
  MeshNormals {
   2;
   0;1;0;, 0.7071;0.7071;0;;
   2;
   3;0,0,0;,
   3;1,1,1;;
  }
  SkinWeights {
   "F";
   2;
   1, 2;
   0.5, 0.25;
   1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;;
  }
 }
}
"#;
        let model = Model::parse(text.as_bytes(), "fold.x").unwrap();
        let mesh = &model.meshes[0];
        assert_eq!(mesh.vertices.len(), 6);
        assert_eq!(mesh.original_vertices, vec![0, 1, 2, 3, 1, 2]);
        assert_eq!(mesh.triangles, vec![[0, 1, 2], [4, 3, 5]]);
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert_vec(mesh.vertices[4].normal, half, half, 0.0);
        assert_vec(mesh.vertices[4].position, 1.0, 0.0, 0.0);
        assert_vec(mesh.vertices[1].normal, 0.0, 1.0, 0.0);
        // The grey default material and one group.
        assert_eq!(mesh.materials.len(), 1);
        assert_eq!(mesh.materials[0].diffuse, [0.5, 0.5, 0.5, 0.0]);
        assert_eq!(mesh.materials[0].name, None);
        assert_eq!(mesh.triangle_materials, vec![0, 0]);
        // The duplicates carry their original's weight.
        let weights: Vec<(u32, f32)> = mesh.bones[0]
            .weights
            .iter()
            .map(|w| (w.vertex, w.weight))
            .collect();
        assert_eq!(weights, vec![(1, 0.5), (4, 0.5), (2, 0.25), (5, 0.25)]);
        assert_eq!(mesh.bones[0].frame, Some(1));
    }

    #[test]
    fn computes_normals_and_triangulates_polygons_without_them() {
        let text = r#"xof 0303txt 0032
Frame F {
 Mesh {
  5;
  0;0;0;, 1;0;0;, 1;0;1;, 0;0;1;, 5;5;5;;
  1;
  4;0,1,2,3;;
  MeshMaterialList {
   2;
   1;
   1;
   Material { 1;0;0;1;; 8; 0;0;0;; 0;0;0;; }
   { Shared }
  }
 }
}
Material Shared {
 0;1;0;1;;
 0;
 0;0;0;;
 0.5;0.5;0.5;;
 TextureFilename { "textures\Wood.tga"; }
}
"#;
        let model = Model::parse(text.as_bytes(), "/game/actors/box.x").unwrap();
        let mesh = &model.meshes[0];
        assert_eq!(mesh.triangles, vec![[0, 1, 2], [0, 2, 3]]);
        // Clockwise (0,0,0) -> (1,0,0) -> (1,0,1) seen from +Y: the
        // left-handed normal points to -Y here; the unused vertex has
        // a zero normal.
        assert_vec(mesh.vertices[0].normal, 0.0, -1.0, 0.0);
        assert_vec(mesh.vertices[4].normal, 0.0, 0.0, 0.0);
        // One index for all faces, resolved through a reference too.
        assert_eq!(mesh.materials.len(), 2);
        assert_eq!(mesh.triangle_materials, vec![1, 1]);
        assert_eq!(mesh.materials[0].power, 8.0);
        assert_eq!(mesh.materials[1].emissive, [0.5, 0.5, 0.5]);
        assert_eq!(
            mesh.materials[1].texture_path.as_deref(),
            Some("/game/actors/textures\\Wood.tga")
        );
        assert_eq!(mesh.materials[1].name.as_deref(), Some("Wood"));
    }

    #[test]
    fn path_helpers_follow_splitpath() {
        assert_eq!(file_stem("actors\\trinity\\trinity.x"), "trinity");
        assert_eq!(file_stem("a/b/Throw.X"), "Throw");
        assert_eq!(file_stem("noext"), "noext");
        assert_eq!(file_stem("dir.d/name"), "name");
        assert_eq!(
            directory_of("actors\\trinity\\trinity.x"),
            "actors\\trinity\\"
        );
        assert_eq!(directory_of("a/b.x"), "a/");
        assert_eq!(directory_of("b.x"), "");
    }

    /// Checks the Wintermute 3D demo's models when they are present under
    /// `testdata/local/wme_demo_3d/` (never in the repository).
    #[test]
    fn loads_the_demo_models_when_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/wme_demo_3d");
        let path = dir.join("trinity.x");
        if !path.is_file() {
            eprintln!("skipped: {} is absent", path.display());
            return;
        }
        let bytes = std::fs::read(&path).unwrap();
        let model = Model::parse(&bytes, "actors\\trinity\\trinity.x").unwrap();
        assert_eq!(model.ticks_per_second, 4800);
        assert_eq!(model.frames.len(), 41, "40 bones and the root");
        assert_eq!(model.frames[1].name, "Bip01");
        assert_eq!(model.find_frame("Bip01_L_Toe02"), Some(8));
        assert_eq!(model.meshes.len(), 1);
        let mesh = &model.meshes[0];
        assert_eq!(mesh.frame, 1);
        assert_eq!(
            mesh.vertices.len(),
            825,
            "the exporter split the vertices already"
        );
        assert_eq!(mesh.triangles.len(), 796);
        assert_eq!(mesh.materials.len(), 5);
        let names: Vec<_> = mesh.materials.iter().map(|m| m.name.as_deref()).collect();
        assert_eq!(
            names,
            vec![
                Some("trinitypantsF"),
                Some("chrome_glass"),
                Some("trinityskin3"),
                Some("chrome_buckle"),
                None
            ]
        );
        assert_eq!(
            mesh.materials[0].texture_path.as_deref(),
            Some("actors\\trinity\\trinitypantsF.bmp")
        );
        assert_eq!(mesh.bones.len(), 40);
        assert!(mesh.bones.iter().all(|b| b.frame.is_some()));
        assert_eq!(mesh.skin.unwrap().max_weights_per_vertex, 1);
        assert_eq!(mesh.duplication.as_ref().unwrap().original_count, 825);

        let sets: Vec<_> = model
            .animation_sets
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            sets,
            vec![
                "all",
                "Idle",
                "walk",
                "take_complete",
                "hold",
                "take_up",
                "take_down"
            ]
        );
        let all = model.animation_set("ALL").unwrap();
        assert_eq!(all.animations.len(), 40);
        assert!(all.animations.iter().all(|a| a.frame.is_some()));
        assert!(!all.looping);
        let first = &all.animations[0];
        assert_eq!(first.target, "Bip01");
        assert_eq!(first.rotation_keys.len(), 66);
        assert_eq!(first.rotation_keys[0].time, 200);
        assert_eq!(first.frame_time(), 200);
        assert_eq!(all.frame_time_ms(4800), 41);
        let walk = model.animation_set("walk").unwrap();
        // Every key of the walk is 200 ticks apart, so the file's events
        // at frames 4 and 16 fall inside the cycle.
        assert!(walk.total_frames(4800) >= 16);
        let pose = model.sample(walk, 0);
        assert_eq!(pose.len(), 41);

        let teapot = dir.join("teapot.x");
        if teapot.is_file() {
            let model = Model::parse(&std::fs::read(&teapot).unwrap(), "teapot.x").unwrap();
            assert_eq!(model.frames.len(), 2);
            assert!(!model.meshes[0].is_skinned());
            assert_eq!(model.animation_sets.len(), 1);
            assert_eq!(model.animation_sets[0].name, "Animation");
            assert!(model.animation_sets[0].animations.is_empty());
        }
        let taken = dir.join("teapot_taken.x");
        if taken.is_file() {
            let model = Model::parse(&std::fs::read(&taken).unwrap(), "teapot_taken.x").unwrap();
            assert_eq!(model.meshes[0].bones.len(), 1);
            let set = &model.animation_sets[0];
            assert!(set.looping);
            assert_eq!(set.animations[0].target, "Scene_Root");
            assert!(
                set.animations[0].sample(0).is_none(),
                "a matrix track with no keys"
            );
            assert_eq!(set.total_time_ms(4800), 0);
        }
    }
}
