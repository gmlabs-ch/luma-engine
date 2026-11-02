//! 3D characters on painted 2D backgrounds, as Wintermute Engine 1.x
//! does them: a scene keeps hidden **scene geometry** (walk planes,
//! blocked meshes, waypoints, the cameras and lights the background was
//! rendered with) in a `.3ds` file, and one perspective camera ties a pixel
//! of the painted background to a point of the 3D world.
//!
//! This crate is the data and maths side of that feature, testable without
//! a GPU:
//!
//! - [`math`]: vectors, matrices, quaternions and planes with the
//!   **Direct3D conventions** of the original (left-handed, Y up, row
//!   vectors, `v' = v * M`), and the D3DX functions the engine relies on;
//! - [`three_ds`]: the `.3ds` chunk subset the original reads (named
//!   meshes, cameras, lights, the camera roll of the keyframer);
//! - [`node_patterns`]: the `.geometry` sidecar that says which mesh is a
//!   walk plane, a block, a waypoint or a generic shadow receiver;
//! - [`scene_geometry`]: the classified geometry of a scene and the
//!   queries on it;
//! - [`camera`]: the scene camera, the projection that maps the layer onto
//!   the 3D world, and the conversions between scene pixels and 3D points;
//! - [`xfile`]: the DirectX `.X` text container the actor models are
//!   stored in, read into a generic object tree.
//!
//! The behaviour follows `docs/3d-actors.md`, which is the specification;
//! `docs/formats/geometry.md` and `docs/formats/xfile.md` describe the
//! files. The crate is an optional
//! module of the engine: `luma` re-exports it as `luma::three_d` behind its
//! `3d` feature.

pub mod camera;
pub mod math;
pub mod node_patterns;
pub mod scene_geometry;
pub mod three_ds;
pub mod xfile;

pub use camera::{CameraSettings, Projection, Ray, ViewSetup, Viewport};
pub use math::{Mat4, Plane, Quat, Vec3};
pub use node_patterns::{NodeKind, NodePattern, NodePatterns};
pub use scene_geometry::{Camera, Light, MeshNode, SceneGeometry, TriangleMesh};

/// The file extension of scene geometry files, without the dot. The
/// original accepts no other geometry format.
pub const GEOMETRY_EXTENSION: &str = "3ds";

/// The extension of the sidecar that classifies the meshes, without the
/// dot. It sits next to the geometry file, with the same base name.
pub const NODE_PATTERNS_EXTENSION: &str = "geometry";
