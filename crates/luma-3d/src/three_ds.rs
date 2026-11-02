//! The `.3ds` chunk subset that carries a scene's geometry.
//!
//! The reader is a **flat chunk walker**, not a general 3D Studio loader:
//! it walks the chunks in file order, enters the containers it knows,
//! reads the payload of the leaf chunks it knows and skips every other
//! chunk by its length. The subset and its quirks (the Y/Z swap of
//! positions, the winding flip of faces, the camera roll taken from the
//! keyframer) are described in `docs/formats/geometry.md`.
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x sources
//! (`3DLoader3DS.cpp`, `3DS_chunks.h`, Copyright (c) 2003-2013 Jan
//! Nedoma). See `NOTICE` at the root of the repository.

use std::fmt;

use crate::math::Vec3;

/// Chunk identifiers, named as in the original's `3DS_chunks.h`.
pub mod chunk {
    /// Colour as three floats; inside a light, its colour.
    pub const RGB_FLOAT: u16 = 0x0010;
    /// Colour as three bytes; inside a light, its colour.
    pub const RGB_BYTE: u16 = 0x0011;
    /// The file.
    pub const MAIN3DS: u16 = 0x4D4D;
    /// The editor data: objects and materials.
    pub const EDIT3DS: u16 = 0x3D3D;
    /// A named object; its name is a NUL-terminated string.
    pub const NAMED_OBJECT: u16 = 0x4000;
    /// The object is a mesh.
    pub const TRIANGLE_MESH: u16 = 0x4100;
    /// Vertex positions.
    pub const TRIANGLE_VERTEXLIST: u16 = 0x4110;
    /// Texture coordinates.
    pub const TRIANGLE_MAPPINGCOORS: u16 = 0x4140;
    /// Faces.
    pub const TRIANGLE_FACELIST: u16 = 0x4120;
    /// Faces of one material.
    pub const TRIANGLE_MATERIAL: u16 = 0x4130;
    /// The mesh's local axes.
    pub const TRI_LOCAL: u16 = 0x4160;
    /// The object is a camera.
    pub const CAMERA: u16 = 0x4700;
    /// The object is a light.
    pub const LIGHT: u16 = 0x4600;
    /// The light is a spot.
    pub const LIGHT_SPOTLIGHT: u16 = 0x4610;
    /// The light starts off.
    pub const LIGHT_IS_OFF: u16 = 0x4620;
    /// The keyframer data.
    pub const KEYFRAMER: u16 = 0xB000;
    /// Keyframer section: ambient.
    pub const KEYFRAMER_AMBIENT_INFO: u16 = 0xB001;
    /// Keyframer section: a mesh.
    pub const KEYFRAMER_MESH_INFO: u16 = 0xB002;
    /// Keyframer section: a camera.
    pub const KEYFRAMER_CAMERA_INFO: u16 = 0xB003;
    /// Keyframer section: a camera target.
    pub const KEYFRAMER_CAMERA_TARGET_INFO: u16 = 0xB004;
    /// Keyframer section: an omni light.
    pub const KEYFRAMER_OMNI_INFO: u16 = 0xB005;
    /// Keyframer section: a spot light target.
    pub const KEYFRAMER_SPOT_TARGET_INFO: u16 = 0xB006;
    /// Keyframer section: a spot light.
    pub const KEYFRAMER_SPOT_INFO: u16 = 0xB007;
    /// The name of the object a keyframer section describes.
    pub const NODE_HDR: u16 = 0xB010;
    /// The roll track of a camera.
    pub const ROLL_TRACK_TAG: u16 = 0xB024;
}

/// Size of a chunk header: `u16` id and `u32` length.
pub const HEADER_LEN: usize = 6;

/// A parsed `.3ds` file: its named objects, in file order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct File {
    /// The named objects.
    pub objects: Vec<Object>,
}

/// A named object of the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    /// The object's name, as written (3DS names are short, about ten
    /// characters).
    pub name: String,
    /// What the object is.
    pub kind: ObjectKind,
}

/// The kind of a named object.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectKind {
    /// A named object without a mesh, camera or light chunk.
    Unknown,
    /// A triangle mesh.
    Mesh(Mesh),
    /// A camera.
    Camera(Camera),
    /// A light.
    Light(Light),
}

/// A triangle mesh, already in the engine's space (Y up, left-handed).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    /// Positions, with the file's `(x, y, z)` stored as `(x, z, y)`.
    pub vertices: Vec<Vec3>,
    /// Faces as vertex indices, with the file's second and third index
    /// swapped so that the winding matches the axis swap.
    pub faces: Vec<[u16; 3]>,
    /// Texture coordinates, `v` stored as `1 - v`. Read but unused by the
    /// scene geometry.
    pub tex_coords: Vec<[f32; 2]>,
    /// Faces per material, read but unused by the scene geometry.
    pub material_faces: Vec<MaterialFaces>,
    /// The `TRI_LOCAL` 4x3 matrix (three axis rows and an origin row), as
    /// written; it is **not** applied to the vertices.
    pub local_matrix: Option<[[f32; 3]; 4]>,
}

/// The faces of a mesh that use one material.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MaterialFaces {
    /// The material's name.
    pub material: String,
    /// Indices into the mesh's faces.
    pub faces: Vec<u16>,
}

/// A camera.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Camera {
    /// Position, axes swapped like vertices.
    pub position: Vec3,
    /// Target, axes swapped like vertices.
    pub target: Vec3,
    /// Roll about the view axis, degrees. The first key of the keyframer's
    /// roll track replaces the value of the camera chunk when present.
    pub bank: f32,
    /// Lens in millimetres.
    pub lens: f32,
}

impl Camera {
    /// The field of view in degrees the original derives from the lens:
    /// `1900 / lens`, or 45 when the lens is not positive.
    pub fn fov_degrees(&self) -> f32 {
        if self.lens > 0.0 {
            1900.0 / self.lens
        } else {
            45.0
        }
    }
}

/// A light.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Light {
    /// Position, axes swapped like vertices.
    pub position: Vec3,
    /// Colour from the colour chunk inside the light, `None` when the
    /// light has none (the original leaves the value undefined then).
    pub color: Option<[u8; 3]>,
    /// Spot light parameters, `None` for a point light.
    pub spot: Option<Spot>,
    /// The light starts disabled.
    pub off: bool,
}

/// The spot light part of a light.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spot {
    /// Target, axes swapped like vertices.
    pub target: Vec3,
    /// Hotspot angle, degrees (read, unused).
    pub hotspot: f32,
    /// Falloff angle, degrees (read, unused).
    pub falloff: f32,
}

/// Why a file could not be read, with the chunk and the byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// What went wrong.
    pub kind: ErrorKind,
    /// The chunk being read (0 when the header itself is cut).
    pub chunk: u16,
    /// Byte offset of the chunk's header in the file.
    pub offset: usize,
}

/// The kinds of error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// The file ends inside a chunk header.
    TruncatedHeader,
    /// A chunk claims a length shorter than its header.
    BadLength(u32),
    /// A chunk claims to extend past the end of the file.
    PastEnd(u32),
    /// The chunk's payload is cut short.
    TruncatedPayload,
    /// A NUL-terminated string has no terminator inside the chunk.
    UnterminatedString,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "chunk {:#06x} at offset {}: ", self.chunk, self.offset)?;
        match &self.kind {
            ErrorKind::TruncatedHeader => write!(f, "file ends inside the chunk header"),
            ErrorKind::BadLength(len) => write!(f, "chunk length {len} is shorter than its header"),
            ErrorKind::PastEnd(len) => {
                write!(f, "chunk length {len} runs past the end of the file")
            }
            ErrorKind::TruncatedPayload => write!(f, "payload is cut short"),
            ErrorKind::UnterminatedString => write!(f, "string has no terminator"),
        }
    }
}

impl std::error::Error for Error {}

/// A cursor over one chunk's bytes, bounded by the chunk's end.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    end: usize,
    chunk: u16,
    offset: usize,
}

impl<'a> Reader<'a> {
    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            chunk: self.chunk,
            offset: self.offset,
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.pos + n > self.end {
            return Err(self.error(ErrorKind::TruncatedPayload));
        }
        let bytes = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// Three floats `(x, y, z)` of the file, returned as the engine's
    /// `(x, z, y)`.
    fn swapped_vec3(&mut self) -> Result<Vec3, Error> {
        let x = self.f32()?;
        let file_y = self.f32()?;
        let file_z = self.f32()?;
        Ok(Vec3::new(x, file_z, file_y))
    }

    /// A NUL-terminated string; bytes map to the characters of the same
    /// code, as the engine treats names as 8-bit text.
    fn cstr(&mut self) -> Result<String, Error> {
        let rest = &self.data[self.pos..self.end];
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| self.error(ErrorKind::UnterminatedString))?;
        self.pos += len + 1;
        Ok(rest[..len].iter().map(|&b| char::from(b)).collect())
    }
}

impl File {
    /// Parses a `.3ds` file.
    pub fn parse(data: &[u8]) -> Result<File, Error> {
        use chunk::*;

        let mut file = File::default();
        let mut pos = 0usize;
        let mut keyframer_section = 0u16;
        let mut keyframer_object: Option<String> = None;

        while pos < data.len() {
            if data.len() - pos < HEADER_LEN {
                return Err(Error {
                    kind: ErrorKind::TruncatedHeader,
                    chunk: 0,
                    offset: pos,
                });
            }
            let id = u16::from_le_bytes([data[pos], data[pos + 1]]);
            let length =
                u32::from_le_bytes([data[pos + 2], data[pos + 3], data[pos + 4], data[pos + 5]]);
            let error = |kind| Error {
                kind,
                chunk: id,
                offset: pos,
            };
            if (length as usize) < HEADER_LEN {
                return Err(error(ErrorKind::BadLength(length)));
            }
            let next = pos + length as usize;
            if next > data.len() {
                return Err(error(ErrorKind::PastEnd(length)));
            }
            let mut r = Reader {
                data,
                pos: pos + HEADER_LEN,
                end: next,
                chunk: id,
                offset: pos,
            };

            // The current object is the last named one, whatever section
            // of the file we are in (the original keeps the pointer).
            let current = file.objects.last_mut();

            // Chunks that are entered leave `pos` after the payload they
            // read, so that their children are walked next; the others
            // skip to the next chunk.
            let entered = match id {
                MAIN3DS | EDIT3DS | KEYFRAMER => true,
                NAMED_OBJECT => {
                    let name = r.cstr()?;
                    file.objects.push(Object {
                        name,
                        kind: ObjectKind::Unknown,
                    });
                    true
                }
                TRIANGLE_MESH => match current {
                    Some(obj) => {
                        obj.kind = ObjectKind::Mesh(Mesh::default());
                        true
                    }
                    None => false,
                },
                TRIANGLE_VERTEXLIST => match mesh_of(current) {
                    Some(mesh) => {
                        let n = r.u16()?;
                        mesh.vertices.reserve(usize::from(n));
                        for _ in 0..n {
                            mesh.vertices.push(r.swapped_vec3()?);
                        }
                        true
                    }
                    None => false,
                },
                TRIANGLE_MAPPINGCOORS => match mesh_of(current) {
                    Some(mesh) => {
                        let n = r.u16()?;
                        mesh.tex_coords.reserve(usize::from(n));
                        for _ in 0..n {
                            let u = r.f32()?;
                            let v = r.f32()?;
                            mesh.tex_coords.push([u, 1.0 - v]);
                        }
                        true
                    }
                    None => false,
                },
                TRIANGLE_FACELIST => match mesh_of(current) {
                    Some(mesh) => {
                        let n = r.u16()?;
                        mesh.faces.reserve(usize::from(n));
                        for _ in 0..n {
                            let a = r.u16()?;
                            let file_b = r.u16()?;
                            let file_c = r.u16()?;
                            let _flags = r.u16()?;
                            mesh.faces.push([a, file_c, file_b]);
                        }
                        true
                    }
                    None => false,
                },
                TRIANGLE_MATERIAL => match mesh_of(current) {
                    Some(mesh) => {
                        let material = r.cstr()?;
                        let n = r.u16()?;
                        let mut faces = Vec::with_capacity(usize::from(n));
                        for _ in 0..n {
                            faces.push(r.u16()?);
                        }
                        mesh.material_faces.push(MaterialFaces { material, faces });
                        true
                    }
                    None => false,
                },
                TRI_LOCAL => match mesh_of(current) {
                    Some(mesh) => {
                        let mut m = [[0.0f32; 3]; 4];
                        for row in &mut m {
                            for v in row.iter_mut() {
                                *v = r.f32()?;
                            }
                        }
                        mesh.local_matrix = Some(m);
                        true
                    }
                    None => false,
                },
                CAMERA => match current {
                    Some(obj) => {
                        let position = r.swapped_vec3()?;
                        let target = r.swapped_vec3()?;
                        let bank = r.f32()?;
                        let lens = r.f32()?;
                        obj.kind = ObjectKind::Camera(Camera {
                            position,
                            target,
                            bank,
                            lens,
                        });
                        true
                    }
                    None => false,
                },
                LIGHT => match current {
                    Some(obj) => {
                        let position = r.swapped_vec3()?;
                        obj.kind = ObjectKind::Light(Light {
                            position,
                            ..Light::default()
                        });
                        true
                    }
                    None => false,
                },
                LIGHT_SPOTLIGHT => match light_of(current) {
                    Some(light) => {
                        let target = r.swapped_vec3()?;
                        let hotspot = r.f32()?;
                        let falloff = r.f32()?;
                        light.spot = Some(Spot {
                            target,
                            hotspot,
                            falloff,
                        });
                        true
                    }
                    None => false,
                },
                LIGHT_IS_OFF => match light_of(current) {
                    Some(light) => {
                        light.off = true;
                        true
                    }
                    None => false,
                },
                RGB_FLOAT => match light_of(current) {
                    Some(light) => {
                        let mut c = [0u8; 3];
                        for ch in &mut c {
                            // `(int)(value * 255)`: truncation, then the byte.
                            *ch = ((r.f32()? * 255.0) as i32).clamp(0, 255) as u8;
                        }
                        light.color = Some(c);
                        true
                    }
                    None => false,
                },
                RGB_BYTE => match light_of(current) {
                    Some(light) => {
                        light.color = Some([r.u8()?, r.u8()?, r.u8()?]);
                        true
                    }
                    None => false,
                },
                KEYFRAMER_AMBIENT_INFO
                | KEYFRAMER_MESH_INFO
                | KEYFRAMER_CAMERA_INFO
                | KEYFRAMER_CAMERA_TARGET_INFO
                | KEYFRAMER_OMNI_INFO
                | KEYFRAMER_SPOT_TARGET_INFO
                | KEYFRAMER_SPOT_INFO => {
                    keyframer_section = id;
                    true
                }
                NODE_HDR => {
                    keyframer_object = Some(r.cstr()?);
                    // flags1, flags2, hierarchy
                    r.take(6)?;
                    true
                }
                ROLL_TRACK_TAG => match (&keyframer_object, keyframer_section) {
                    (Some(name), KEYFRAMER_CAMERA_INFO) => {
                        // flags, then four unknown words
                        r.take(10)?;
                        let keys = r.u16()?;
                        let _unknown = r.u16()?;
                        if keys > 0 {
                            // frame number, unknown dword, roll
                            r.take(6)?;
                            let roll = r.f32()?;
                            let camera = file.objects.iter_mut().find(|o| {
                                matches!(o.kind, ObjectKind::Camera(_))
                                    && o.name.eq_ignore_ascii_case(name)
                            });
                            if let Some(Object {
                                kind: ObjectKind::Camera(camera),
                                ..
                            }) = camera
                            {
                                camera.bank = roll;
                            }
                        }
                        // Only the first key is used; the rest of the
                        // track is left to its chunk length.
                        false
                    }
                    _ => false,
                },
                _ => false,
            };

            pos = if entered { r.pos } else { next };
        }

        Ok(file)
    }

    /// The meshes, in file order, with their names.
    pub fn meshes(&self) -> impl Iterator<Item = (&str, &Mesh)> {
        self.objects.iter().filter_map(|o| match &o.kind {
            ObjectKind::Mesh(mesh) => Some((o.name.as_str(), mesh)),
            _ => None,
        })
    }

    /// The cameras, in file order, with their names.
    pub fn cameras(&self) -> impl Iterator<Item = (&str, &Camera)> {
        self.objects.iter().filter_map(|o| match &o.kind {
            ObjectKind::Camera(camera) => Some((o.name.as_str(), camera)),
            _ => None,
        })
    }

    /// The lights, in file order, with their names.
    pub fn lights(&self) -> impl Iterator<Item = (&str, &Light)> {
        self.objects.iter().filter_map(|o| match &o.kind {
            ObjectKind::Light(light) => Some((o.name.as_str(), light)),
            _ => None,
        })
    }
}

fn mesh_of(obj: Option<&mut Object>) -> Option<&mut Mesh> {
    match obj {
        Some(Object {
            kind: ObjectKind::Mesh(mesh),
            ..
        }) => Some(mesh),
        _ => None,
    }
}

fn light_of(obj: Option<&mut Object>) -> Option<&mut Light> {
    match obj {
        Some(Object {
            kind: ObjectKind::Light(light),
            ..
        }) => Some(light),
        _ => None,
    }
}

/// A builder of `.3ds` bytes, for tests and fixtures: every chunk gets a
/// correct length, children are nested in the payload. It writes the
/// geometry **in file space**, so that what the reader gives back is what
/// was passed in engine space.
#[derive(Debug, Clone, Copy, Default)]
pub struct Builder;

impl Builder {
    /// A chunk with a payload and children, as bytes.
    pub fn chunk(id: u16, payload: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        let len = HEADER_LEN + payload.len() + children.iter().map(Vec::len).sum::<usize>();
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(payload);
        for child in children {
            out.extend_from_slice(child);
        }
        out
    }

    /// A NUL-terminated string.
    pub fn cstr(s: &str) -> Vec<u8> {
        let mut v: Vec<u8> = s.bytes().collect();
        v.push(0);
        v
    }

    /// Three floats in **file** order `(x, y, z)` with Z up; the reader
    /// swaps them.
    pub fn file_vec3(x: f32, y: f32, z: f32) -> Vec<u8> {
        let mut v = Vec::with_capacity(12);
        v.extend_from_slice(&x.to_le_bytes());
        v.extend_from_slice(&y.to_le_bytes());
        v.extend_from_slice(&z.to_le_bytes());
        v
    }

    /// A named mesh object from engine-space positions and faces (Y up,
    /// clockwise front faces): the builder writes them in file space, so
    /// that the reader gives them back as passed.
    pub fn mesh(name: &str, vertices: &[Vec3], faces: &[[u16; 3]]) -> Vec<u8> {
        let mut verts = (vertices.len() as u16).to_le_bytes().to_vec();
        for v in vertices {
            verts.extend(Self::file_vec3(v.x, v.z, v.y));
        }
        let mut face_list = (faces.len() as u16).to_le_bytes().to_vec();
        for [a, b, c] in faces {
            face_list.extend_from_slice(&a.to_le_bytes());
            face_list.extend_from_slice(&c.to_le_bytes());
            face_list.extend_from_slice(&b.to_le_bytes());
            face_list.extend_from_slice(&0u16.to_le_bytes());
        }
        let mesh = Self::chunk(
            chunk::TRIANGLE_MESH,
            &[],
            &[
                Self::chunk(chunk::TRIANGLE_VERTEXLIST, &verts, &[]),
                Self::chunk(chunk::TRIANGLE_FACELIST, &face_list, &[]),
            ],
        );
        Self::chunk(chunk::NAMED_OBJECT, &Self::cstr(name), &[mesh])
    }

    /// A named camera from engine-space position and target.
    pub fn camera(name: &str, position: Vec3, target: Vec3, bank: f32, lens: f32) -> Vec<u8> {
        let mut payload = Self::file_vec3(position.x, position.z, position.y);
        payload.extend(Self::file_vec3(target.x, target.z, target.y));
        payload.extend_from_slice(&bank.to_le_bytes());
        payload.extend_from_slice(&lens.to_le_bytes());
        let camera = Self::chunk(chunk::CAMERA, &payload, &[]);
        Self::chunk(chunk::NAMED_OBJECT, &Self::cstr(name), &[camera])
    }

    /// A named light from an engine-space position, with a float colour
    /// chunk and an optional spot target.
    pub fn light(
        name: &str,
        position: Vec3,
        color: [f32; 3],
        spot_target: Option<Vec3>,
        off: bool,
    ) -> Vec<u8> {
        let mut children = vec![Self::chunk(
            chunk::RGB_FLOAT,
            &Self::file_vec3(color[0], color[1], color[2]),
            &[],
        )];
        if let Some(target) = spot_target {
            let mut payload = Self::file_vec3(target.x, target.z, target.y);
            payload.extend_from_slice(&30.0f32.to_le_bytes());
            payload.extend_from_slice(&45.0f32.to_le_bytes());
            children.push(Self::chunk(chunk::LIGHT_SPOTLIGHT, &payload, &[]));
        }
        if off {
            children.push(Self::chunk(chunk::LIGHT_IS_OFF, &[], &[]));
        }
        let light = Self::chunk(
            chunk::LIGHT,
            &Self::file_vec3(position.x, position.z, position.y),
            &children,
        );
        Self::chunk(chunk::NAMED_OBJECT, &Self::cstr(name), &[light])
    }

    /// A keyframer camera section with a roll track of one key.
    pub fn camera_roll(name: &str, roll: f32) -> Vec<u8> {
        let mut hdr = Self::cstr(name);
        hdr.extend_from_slice(&[0u8; 6]);
        let mut track = vec![0u8; 10];
        track.extend_from_slice(&1u16.to_le_bytes());
        track.extend_from_slice(&0u16.to_le_bytes());
        track.extend_from_slice(&[0u8; 6]);
        track.extend_from_slice(&roll.to_le_bytes());
        let info = Self::chunk(
            chunk::KEYFRAMER_CAMERA_INFO,
            &[],
            &[
                Self::chunk(chunk::NODE_HDR, &hdr, &[]),
                Self::chunk(chunk::ROLL_TRACK_TAG, &track, &[]),
            ],
        );
        Self::chunk(chunk::KEYFRAMER, &[], &[info])
    }

    /// A whole file: `MAIN3DS { EDIT3DS { objects } extra }`.
    pub fn file(objects: &[Vec<u8>], extra: &[Vec<u8>]) -> Vec<u8> {
        let edit = Self::chunk(chunk::EDIT3DS, &[], objects);
        let mut children = vec![edit];
        children.extend_from_slice(extra);
        Self::chunk(chunk::MAIN3DS, &[], &children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor() -> Vec<u8> {
        Builder::mesh(
            "walk_01",
            &[
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 100.0),
                Vec3::new(100.0, 0.0, 100.0),
                Vec3::new(100.0, 0.0, 0.0),
            ],
            &[[0, 1, 2], [0, 2, 3]],
        )
    }

    #[test]
    fn positions_are_swapped_and_faces_flipped() {
        // Written by hand in file space: (x, y, z) with Z up.
        let mut verts = 1u16.to_le_bytes().to_vec();
        verts.extend(Builder::file_vec3(1.0, 2.0, 3.0));
        let mut faces = 1u16.to_le_bytes().to_vec();
        for w in [7u16, 8, 9, 0] {
            faces.extend_from_slice(&w.to_le_bytes());
        }
        let mesh = Builder::chunk(
            chunk::TRIANGLE_MESH,
            &[],
            &[
                Builder::chunk(chunk::TRIANGLE_VERTEXLIST, &verts, &[]),
                Builder::chunk(chunk::TRIANGLE_FACELIST, &faces, &[]),
            ],
        );
        let obj = Builder::chunk(chunk::NAMED_OBJECT, &Builder::cstr("m"), &[mesh]);
        let file = File::parse(&Builder::file(&[obj], &[])).unwrap();
        let (name, mesh) = file.meshes().next().unwrap();
        assert_eq!(name, "m");
        assert_eq!(mesh.vertices, vec![Vec3::new(1.0, 3.0, 2.0)]);
        assert_eq!(mesh.faces, vec![[7, 9, 8]]);
    }

    #[test]
    fn the_builder_round_trips_engine_space() {
        let file = File::parse(&Builder::file(&[floor()], &[])).unwrap();
        let (_, mesh) = file.meshes().next().unwrap();
        assert_eq!(mesh.vertices[1], Vec3::new(0.0, 0.0, 100.0));
        assert_eq!(mesh.faces, vec![[0, 1, 2], [0, 2, 3]]);
    }

    #[test]
    fn mapping_coordinates_materials_and_local_matrix() {
        let mut uvs = 2u16.to_le_bytes().to_vec();
        for (u, v) in [(0.0f32, 0.25f32), (1.0, 1.0)] {
            uvs.extend_from_slice(&u.to_le_bytes());
            uvs.extend_from_slice(&v.to_le_bytes());
        }
        let mut mat = Builder::cstr("Wood");
        mat.extend_from_slice(&2u16.to_le_bytes());
        mat.extend_from_slice(&0u16.to_le_bytes());
        mat.extend_from_slice(&1u16.to_le_bytes());
        let mut local = Vec::new();
        for i in 0..12 {
            local.extend_from_slice(&(i as f32).to_le_bytes());
        }
        let faces = {
            let mut f = 2u16.to_le_bytes().to_vec();
            for w in [0u16, 1, 2, 0, 0, 2, 3, 0] {
                f.extend_from_slice(&w.to_le_bytes());
            }
            f
        };
        let mesh = Builder::chunk(
            chunk::TRIANGLE_MESH,
            &[],
            &[
                Builder::chunk(chunk::TRIANGLE_MAPPINGCOORS, &uvs, &[]),
                // The material list is a child of the face list.
                Builder::chunk(
                    chunk::TRIANGLE_FACELIST,
                    &faces,
                    &[Builder::chunk(chunk::TRIANGLE_MATERIAL, &mat, &[])],
                ),
                Builder::chunk(chunk::TRI_LOCAL, &local, &[]),
            ],
        );
        let obj = Builder::chunk(chunk::NAMED_OBJECT, &Builder::cstr("m"), &[mesh]);
        let file = File::parse(&Builder::file(&[obj], &[])).unwrap();
        let (_, mesh) = file.meshes().next().unwrap();
        assert_eq!(mesh.tex_coords, vec![[0.0, 0.75], [1.0, 0.0]]);
        assert_eq!(mesh.faces.len(), 2);
        assert_eq!(mesh.material_faces.len(), 1);
        assert_eq!(mesh.material_faces[0].material, "Wood");
        assert_eq!(mesh.material_faces[0].faces, vec![0, 1]);
        let local = mesh.local_matrix.unwrap();
        assert_eq!(local[0], [0.0, 1.0, 2.0]);
        assert_eq!(local[3], [9.0, 10.0, 11.0]);
    }

    #[test]
    fn cameras_lights_and_the_keyframer_roll() {
        let cam = Builder::camera(
            "Camera01",
            Vec3::new(0.0, 100.0, -500.0),
            Vec3::ZERO,
            5.0,
            38.0,
        );
        let spot = Builder::light(
            "Spot01",
            Vec3::new(10.0, 200.0, 10.0),
            [1.0, 0.5, 0.0],
            Some(Vec3::new(0.0, 0.0, 0.0)),
            false,
        );
        let omni = Builder::light(
            "Omni01",
            Vec3::new(1.0, 2.0, 3.0),
            [0.2, 0.2, 0.2],
            None,
            true,
        );
        let roll = Builder::camera_roll("camera01", 12.5);
        let file = File::parse(&Builder::file(&[cam, spot, omni], &[roll])).unwrap();

        let (name, camera) = file.cameras().next().unwrap();
        assert_eq!(name, "Camera01");
        assert_eq!(camera.position, Vec3::new(0.0, 100.0, -500.0));
        assert_eq!(camera.lens, 38.0);
        assert_eq!(camera.fov_degrees(), 50.0);
        // The keyframer's roll replaced the bank, matched ignoring case.
        assert_eq!(camera.bank, 12.5);
        assert_eq!(Camera::default().fov_degrees(), 45.0);

        let lights: Vec<_> = file.lights().collect();
        assert_eq!(lights.len(), 2);
        let (_, spot) = lights[0];
        assert_eq!(spot.position, Vec3::new(10.0, 200.0, 10.0));
        assert_eq!(spot.color, Some([255, 127, 0]));
        assert_eq!(spot.spot.as_ref().unwrap().target, Vec3::ZERO);
        assert_eq!(spot.spot.as_ref().unwrap().hotspot, 30.0);
        assert!(!spot.off);
        let (name, omni) = lights[1];
        assert_eq!(name, "Omni01");
        assert!(omni.spot.is_none());
        assert!(omni.off);
        assert_eq!(omni.color, Some([51, 51, 51]));
    }

    #[test]
    fn byte_colours_and_unknown_chunks_are_skipped_by_length() {
        let light = Builder::chunk(
            chunk::LIGHT,
            &Builder::file_vec3(0.0, 0.0, 0.0),
            &[
                // An attenuation chunk the reader does not know.
                Builder::chunk(0x4659, &[1, 2, 3, 4], &[]),
                Builder::chunk(chunk::RGB_BYTE, &[10, 20, 30], &[]),
            ],
        );
        let obj = Builder::chunk(chunk::NAMED_OBJECT, &Builder::cstr("L"), &[light]);
        // A material block with a colour chunk inside: skipped whole, so the
        // colour does not land on the light.
        let material = Builder::chunk(
            0xAFFF,
            &[],
            &[Builder::chunk(chunk::RGB_BYTE, &[1, 1, 1], &[])],
        );
        let file = File::parse(&Builder::file(&[obj, material], &[])).unwrap();
        let (_, light) = file.lights().next().unwrap();
        assert_eq!(light.color, Some([10, 20, 30]));
    }

    #[test]
    fn colours_outside_a_light_are_ignored() {
        let mesh = floor();
        let stray = Builder::chunk(chunk::RGB_BYTE, &[1, 2, 3], &[]);
        let file = File::parse(&Builder::file(&[mesh, stray], &[])).unwrap();
        assert_eq!(file.objects.len(), 1);
        assert!(file.lights().next().is_none());
    }

    #[test]
    fn errors_carry_the_chunk_and_offset() {
        // A header cut short.
        let err = File::parse(&[0x4D, 0x4D, 0x10]).unwrap_err();
        assert_eq!(err.kind, ErrorKind::TruncatedHeader);
        assert_eq!(err.offset, 0);

        // A chunk longer than the file.
        let mut bytes = Builder::file(&[], &[]);
        bytes[2] = 0xFF;
        let err = File::parse(&bytes).unwrap_err();
        assert_eq!(err.chunk, chunk::MAIN3DS);
        assert!(matches!(err.kind, ErrorKind::PastEnd(_)));

        // A length shorter than the header.
        let mut bytes = Builder::file(&[], &[]);
        bytes[2] = 2;
        let err = File::parse(&bytes).unwrap_err();
        assert_eq!(err.kind, ErrorKind::BadLength(2));

        // A vertex list that promises more vertices than it holds.
        let mut verts = 5u16.to_le_bytes().to_vec();
        verts.extend(Builder::file_vec3(0.0, 0.0, 0.0));
        let mesh = Builder::chunk(
            chunk::TRIANGLE_MESH,
            &[],
            &[Builder::chunk(chunk::TRIANGLE_VERTEXLIST, &verts, &[])],
        );
        let obj = Builder::chunk(chunk::NAMED_OBJECT, &Builder::cstr("m"), &[mesh]);
        let bytes = Builder::file(&[obj], &[]);
        let err = File::parse(&bytes).unwrap_err();
        assert_eq!(err.kind, ErrorKind::TruncatedPayload);
        assert_eq!(err.chunk, chunk::TRIANGLE_VERTEXLIST);
        let expected_offset = HEADER_LEN * 3 + Builder::cstr("m").len() + HEADER_LEN;
        assert_eq!(err.offset, expected_offset);
        assert!(err.to_string().contains("0x4110"));

        // A name without its terminator.
        let obj = Builder::chunk(chunk::NAMED_OBJECT, b"abc", &[]);
        let err = File::parse(&Builder::file(&[obj], &[])).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnterminatedString);
    }

    #[test]
    fn mesh_chunks_without_an_object_are_skipped() {
        let verts = 1u16.to_le_bytes().to_vec();
        let stray = Builder::chunk(chunk::TRIANGLE_VERTEXLIST, &verts, &[]);
        let file = File::parse(&Builder::file(&[stray, floor()], &[])).unwrap();
        assert_eq!(file.meshes().count(), 1);
    }

    #[test]
    fn an_empty_file_has_no_objects() {
        assert_eq!(File::parse(&[]).unwrap(), File::default());
    }
}
