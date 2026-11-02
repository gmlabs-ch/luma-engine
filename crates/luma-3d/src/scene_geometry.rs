//! The hidden geometry of a scene: walk planes, blocks, generic meshes,
//! waypoints, cameras and lights, classified from a `.3ds` file by the
//! node patterns of its `.geometry` sidecar.
//!
//! The runtime queries that only need the geometry live here too: the
//! floor height under a point, the conversion of a scene pixel to the
//! nearest point on a walk plane (with a [`Projection`]), the segment
//! tests against the planes and blocks, node and light toggles by name.
//! The path finder over the waypoints is [`crate::path3d`].
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x sources
//! (`AdSceneGeometry.cpp`, `AdWaypointGroup3D.cpp`, `3DMesh.cpp`,
//! `3DCamera.cpp`, `3DLight.cpp`, Copyright (c) 2003-2013 Jan Nedoma). See
//! `NOTICE` at the root of the repository.

use luma_formats::Rgb;

use crate::camera::Projection;
use crate::math::{self, Mat4, Vec3, deg_to_rad};
use crate::node_patterns::{NodeKind, NodePatterns};
use crate::three_ds;

/// A triangle mesh of the geometry, in world units.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TriangleMesh {
    /// Positions.
    pub vertices: Vec<Vec3>,
    /// Triangles as vertex indices; front faces are clockwise, as the
    /// `.3ds` reader leaves them.
    pub faces: Vec<[u16; 3]>,
    /// Smooth normals, one per vertex: the normalised sum of the normals
    /// `(v1 - v0) × (v2 - v1)` of the faces sharing the vertex.
    pub normals: Vec<Vec3>,
}

impl TriangleMesh {
    /// A mesh from positions and faces, with its normals computed.
    pub fn new(vertices: Vec<Vec3>, faces: Vec<[u16; 3]>) -> TriangleMesh {
        let mut mesh = TriangleMesh {
            vertices,
            faces,
            normals: Vec::new(),
        };
        mesh.compute_normals();
        mesh
    }

    /// A mesh from a parsed `.3ds` mesh.
    pub fn from_3ds(mesh: &three_ds::Mesh) -> TriangleMesh {
        TriangleMesh::new(mesh.vertices.clone(), mesh.faces.clone())
    }

    /// Recomputes the smooth normals (`C3DMesh::ComputeNormals`). Faces
    /// with an index out of range are ignored.
    pub fn compute_normals(&mut self) {
        let mut sums = vec![Vec3::ZERO; self.vertices.len()];
        for face in &self.faces {
            let Some([v0, v1, v2]) = self.corners(face) else {
                continue;
            };
            let n = (v1 - v0).cross(v2 - v1).normalized();
            for &i in face {
                sums[usize::from(i)] += n;
            }
        }
        self.normals = sums.into_iter().map(Vec3::normalized).collect();
    }

    /// The three corners of a face, `None` when an index is out of range.
    pub fn corners(&self, face: &[u16; 3]) -> Option<[Vec3; 3]> {
        Some([
            *self.vertices.get(usize::from(face[0]))?,
            *self.vertices.get(usize::from(face[1]))?,
            *self.vertices.get(usize::from(face[2]))?,
        ])
    }

    /// The triangles with valid indices, as corner positions.
    pub fn triangles(&self) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.faces.iter().filter_map(|face| self.corners(face))
    }

    /// The axis-aligned bounds `(min, max)`, `None` for an empty mesh.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let first = *self.vertices.first()?;
        Some(
            self.vertices
                .iter()
                .fold((first, first), |(lo, hi), &v| (lo.min(v), hi.max(v))),
        )
    }

    /// The centre of the bounds (`min + (max - min) / 2`), the origin for
    /// an empty mesh; a waypoint mesh contributes this point.
    pub fn center(&self) -> Vec3 {
        match self.bounds() {
            Some((lo, hi)) => lo + (hi - lo) * 0.5,
            None => Vec3::ZERO,
        }
    }
}

/// A walk plane, a block or a generic mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshNode {
    /// The mesh's name in the `.3ds` file; scripts use it to enable and
    /// disable the node (case-insensitive).
    pub name: String,
    /// The triangles.
    pub mesh: TriangleMesh,
    /// Whether the mesh receives stencil shadows (from its node pattern).
    pub receive_shadows: bool,
    /// Whether the node is enabled. Blocks only stop paths while enabled;
    /// walk planes are picked and measured whether enabled or not (the
    /// original only honours the flag of a plane in the debug overlay and
    /// the shadow pass).
    pub active: bool,
}

/// A camera of the geometry, as the background was rendered.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// The camera's name in the `.3ds` file (case-insensitive lookups).
    pub name: String,
    /// Eye position.
    pub position: Vec3,
    /// The point looked at.
    pub target: Vec3,
    /// Roll about the view axis, degrees.
    pub bank: f32,
    /// Vertical field of view, radians, as derived from the file's lens.
    /// A scene's `FOV_OVERRIDE` replaces it through
    /// [`CameraSettings`](crate::camera::CameraSettings), not here.
    pub fov: f32,
}

impl Camera {
    /// A camera from a parsed `.3ds` camera.
    pub fn from_3ds(name: &str, camera: &three_ds::Camera) -> Camera {
        Camera {
            name: name.to_string(),
            position: camera.position,
            target: camera.target,
            bank: camera.bank,
            fov: deg_to_rad(camera.fov_degrees()),
        }
    }

    /// The view matrix (`C3DCamera::GetViewMatrix`): `LookAtLH(position,
    /// target, up)` with `up = (0, 1, 0)` rotated about Z by the bank.
    pub fn view_matrix(&self) -> Mat4 {
        let mut up = Vec3::UNIT_Y;
        if self.bank != 0.0 {
            up = Mat4::rotation_z(deg_to_rad(self.bank)).transform_coord(up);
        }
        Mat4::look_at_lh(self.position, self.target, up)
    }
}

/// A light of the geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    /// The light's name in the `.3ds` file (case-insensitive lookups).
    pub name: String,
    /// Position.
    pub position: Vec3,
    /// The target of a spot light; the origin for a point light.
    pub target: Vec3,
    /// A spot light rather than a point light.
    pub spotlight: bool,
    /// Diffuse colour. White when the file gives none.
    pub color: Rgb,
    /// Falloff angle of a spot, degrees (kept, unused by the original).
    pub falloff: f32,
    /// Whether the light is on; scripts switch it.
    pub active: bool,
}

impl Light {
    /// A light from a parsed `.3ds` light.
    pub fn from_3ds(name: &str, light: &three_ds::Light) -> Light {
        let [r, g, b] = light.color.unwrap_or([255, 255, 255]);
        Light {
            name: name.to_string(),
            position: light.position,
            target: light.spot.as_ref().map_or(Vec3::ZERO, |s| s.target),
            spotlight: light.spot.is_some(),
            color: Rgb { r, g, b },
            falloff: light.spot.as_ref().map_or(0.0, |s| s.falloff),
            active: !light.off,
        }
    }
}

/// A group of waypoints (`CAdWaypointGroup3D`). The loader puts every
/// waypoint mesh of the file into one group; the path finder links the
/// points of the active groups.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WaypointGroup {
    /// The points: the bounding-box centre of each waypoint mesh, lifted
    /// to `floor + waypoint_height` by [`SceneGeometry::drop_waypoints`].
    pub points: Vec<Vec3>,
    /// Whether the path finder uses the group. The original starts it
    /// `true` and has no script or file that changes it.
    pub active: bool,
}

/// The classified geometry of a scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneGeometry {
    /// The walk planes, in file order.
    pub walkplanes: Vec<MeshNode>,
    /// The blocked meshes, in file order.
    pub blocks: Vec<MeshNode>,
    /// The generic meshes, in file order.
    pub generics: Vec<MeshNode>,
    /// The waypoint groups: empty for a file without waypoint meshes,
    /// otherwise one group holding them all, in file order (the original
    /// never creates a second one).
    pub waypoint_groups: Vec<WaypointGroup>,
    /// The cameras, in file order.
    pub cameras: Vec<Camera>,
    /// The lights, in file order.
    pub lights: Vec<Light>,
    /// How far above the floor the waypoints sit (the scene's
    /// `WAYPOINT_HEIGHT`, default 10).
    pub waypoint_height: f32,
    /// Index of the active camera: the first one after loading, changed
    /// by name with [`Self::set_active_camera`].
    pub active_camera: Option<usize>,
}

impl SceneGeometry {
    /// The waypoint height a geometry starts with.
    pub const DEFAULT_WAYPOINT_HEIGHT: f32 = 10.0;

    /// Loads a `.3ds` geometry and classifies its meshes with the node
    /// patterns; the waypoints are dropped to the floor with the default
    /// height. Only `.3ds` is a geometry format: the caller checks the
    /// extension ([`crate::GEOMETRY_EXTENSION`]).
    pub fn load_3ds(
        bytes: &[u8],
        patterns: &NodePatterns,
    ) -> Result<SceneGeometry, three_ds::Error> {
        Ok(SceneGeometry::from_file(
            &three_ds::File::parse(bytes)?,
            patterns,
        ))
    }

    /// Classifies a parsed `.3ds` file. A mesh matching no node is
    /// dropped; a waypoint mesh contributes its bounding-box centre.
    pub fn from_file(file: &three_ds::File, patterns: &NodePatterns) -> SceneGeometry {
        let mut geometry = SceneGeometry {
            walkplanes: Vec::new(),
            blocks: Vec::new(),
            generics: Vec::new(),
            waypoint_groups: Vec::new(),
            cameras: Vec::new(),
            lights: Vec::new(),
            waypoint_height: Self::DEFAULT_WAYPOINT_HEIGHT,
            active_camera: None,
        };
        for (name, mesh) in file.meshes() {
            let Some(pattern) = patterns.classify(name) else {
                continue;
            };
            let node = || MeshNode {
                name: name.to_string(),
                mesh: TriangleMesh::from_3ds(mesh),
                receive_shadows: pattern.receive_shadows,
                active: true,
            };
            match pattern.kind {
                NodeKind::Walkplane => geometry.walkplanes.push(node()),
                NodeKind::Blocked => geometry.blocks.push(node()),
                NodeKind::Generic => geometry.generics.push(node()),
                NodeKind::Waypoint => {
                    if geometry.waypoint_groups.is_empty() {
                        geometry.waypoint_groups.push(WaypointGroup {
                            points: Vec::new(),
                            active: true,
                        });
                    }
                    geometry.waypoint_groups[0]
                        .points
                        .push(TriangleMesh::from_3ds(mesh).center());
                }
            }
        }
        geometry.cameras = file
            .cameras()
            .map(|(name, camera)| Camera::from_3ds(name, camera))
            .collect();
        geometry.lights = file
            .lights()
            .map(|(name, light)| Light::from_3ds(name, light))
            .collect();
        if !geometry.cameras.is_empty() {
            geometry.active_camera = Some(0);
        }
        geometry.drop_waypoints();
        geometry
    }

    /// Lifts every waypoint to `floor height + waypoint_height`
    /// (`DropWaypoints`): the floor is measured from the waypoint's
    /// current position with no tolerance, and a waypoint over no walk
    /// plane keeps its height plus the waypoint height.
    pub fn drop_waypoints(&mut self) {
        let height = self.waypoint_height;
        let planes = std::mem::take(&mut self.walkplanes);
        for point in self
            .waypoint_groups
            .iter_mut()
            .flat_map(|group| group.points.iter_mut())
        {
            let floor = height_at(&planes, *point, 0.0).unwrap_or(point.y);
            point.y = floor + height;
        }
        self.walkplanes = planes;
    }

    /// Every waypoint of every group, active or not, in file order.
    pub fn waypoints(&self) -> impl Iterator<Item = &Vec3> {
        self.waypoint_groups
            .iter()
            .flat_map(|group| group.points.iter())
    }

    /// Sets the waypoint height and drops the waypoints again, as the
    /// scene does for `WAYPOINT_HEIGHT` and the `WaypointsHeight` property.
    pub fn set_waypoint_height(&mut self, height: f32) {
        self.waypoint_height = height;
        self.drop_waypoints();
    }

    /// The floor height under a point (`GetHeightAt`). The vertical line
    /// through `pos` raised by `tolerance` is tested against every walk
    /// plane triangle, enabled or not, from above (single-sided); a hit
    /// higher than `pos.y + 2 * tolerance` is skipped; of the rest, the
    /// height nearest to the raised start wins. `None` without a hit.
    pub fn height_at(&self, pos: Vec3, tolerance: f32) -> Option<f32> {
        height_at(&self.walkplanes, pos, tolerance)
    }

    /// Whether the straight segment `p1..p2` is free (`DirectPathExists`):
    /// it is not when it pierces a triangle of any walk plane, enabled or
    /// not, or of an **enabled** block. "Pierces" is the original's pair
    /// of tests: the segment crosses the triangle's plane
    /// ([`math::pick_get_intersect`], so a segment that only touches the
    /// plane at one end counts) and the line through it meets the
    /// triangle in one direction or the other ([`math::intersect_triangle`]
    /// from `p1` along `p1 - p2`, then from `p2` along `p2 - p1`). A
    /// segment that runs inside a plane never crosses it, so paths between
    /// points lifted over one floor only meet blocks; a zero-length
    /// segment is always free.
    pub fn direct_path_exists(&self, p1: Vec3, p2: Vec3) -> bool {
        let pierced = |nodes: &[MeshNode], enabled_only: bool| {
            nodes
                .iter()
                .filter(|node| node.active || !enabled_only)
                .flat_map(|node| node.mesh.triangles())
                .any(|tri| segment_pierces(p1, p2, tri).is_some())
        };
        !(pierced(&self.walkplanes, false) || pierced(&self.blocks, true))
    }

    /// Where the segment `p1..p2` first pierces an enabled block
    /// (`GetBlockIntersection`): the blocks in file order, their triangles
    /// in face order, the first pierced one wins, whichever end of the
    /// segment it is nearer to. `None` when no enabled block is pierced
    /// (the original returns the origin, which the path finder treats as
    /// "none").
    pub fn block_intersection(&self, p1: Vec3, p2: Vec3) -> Option<Vec3> {
        self.blocks
            .iter()
            .filter(|block| block.active)
            .flat_map(|block| block.mesh.triangles())
            .find_map(|tri| segment_pierces(p1, p2, tri))
    }

    /// The point of the walk planes under a scene pixel (`Convert2Dto3D`):
    /// the pick ray of the projection is tested against every walk plane
    /// triangle, enabled or not, and the hit nearest to the camera wins.
    /// `None` when the ray misses every walk plane.
    pub fn convert_2d_to_3d(&self, projection: &Projection, x: i32, y: i32) -> Option<Vec3> {
        let ray = projection.pick_ray(x, y);
        let mut best: Option<(f32, Vec3)> = None;
        for plane in &self.walkplanes {
            for [v0, v1, v2] in plane.mesh.triangles() {
                if let Some(hit) = math::intersect_triangle(ray.origin, ray.direction, v0, v1, v2) {
                    let dist = (hit - ray.origin).length();
                    if best.is_none_or(|(d, _)| dist < d) {
                        best = Some((dist, hit));
                    }
                }
            }
        }
        best.map(|(_, hit)| hit)
    }

    /// [`Self::convert_2d_to_3d`] with the original's tolerance
    /// (`Convert2Dto3DTolerant`): when the pixel misses, scan in steps of
    /// 10 pixels, up to 1000, to the left, right, up and down for the
    /// first pixel that converts; per axis keep the nearer direction, then
    /// shift along the axis with the smaller shift (the vertical one when
    /// equal) and convert that pixel. `None` when no direction finds a
    /// walk plane.
    pub fn convert_2d_to_3d_tolerant(
        &self,
        projection: &Projection,
        x: i32,
        y: i32,
    ) -> Option<Vec3> {
        if let Some(hit) = self.convert_2d_to_3d(projection, x, y) {
            return Some(hit);
        }
        let scan = |dx: i32, dy: i32| {
            (0..1000)
                .step_by(10)
                .find(|i| {
                    self.convert_2d_to_3d(projection, x + dx * i, y + dy * i)
                        .is_some()
                })
                .unwrap_or(0)
        };
        let (left, right, up, down) = (scan(-1, 0), scan(1, 0), scan(0, -1), scan(0, 1));
        if left == 0 && right == 0 && up == 0 && down == 0 {
            return None;
        }
        let pick = |neg: i32, pos: i32| -> i32 {
            if pos != 0 {
                if neg != 0 && neg < pos { -neg } else { pos }
            } else {
                -neg
            }
        };
        let offset_x = if left != 0 || right != 0 {
            pick(left, right)
        } else {
            i32::MAX
        };
        let offset_y = if up != 0 || down != 0 {
            pick(up, down)
        } else {
            i32::MAX
        };
        if offset_x.abs() < offset_y.abs() {
            self.convert_2d_to_3d(projection, x + offset_x, y)
        } else {
            self.convert_2d_to_3d(projection, x, y + offset_y)
        }
    }

    /// The index of a camera by name, ignoring ASCII case.
    pub fn camera_index(&self, name: &str) -> Option<usize> {
        self.cameras
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Makes the named camera active. An unknown name keeps the current
    /// one and returns `false` (the original logs a warning).
    pub fn set_active_camera(&mut self, name: &str) -> bool {
        match self.camera_index(name) {
            Some(i) => {
                self.active_camera = Some(i);
                true
            }
            None => false,
        }
    }

    /// The active camera.
    pub fn active_camera(&self) -> Option<&Camera> {
        self.active_camera.and_then(|i| self.cameras.get(i))
    }

    /// The walk planes, blocks and generics, in that order.
    pub fn nodes(&self) -> impl Iterator<Item = &MeshNode> {
        self.walkplanes
            .iter()
            .chain(&self.blocks)
            .chain(&self.generics)
    }

    /// Enables or disables every node with this name, ignoring ASCII case
    /// (`EnableNode`). Returns whether a node matched.
    pub fn set_node_enabled(&mut self, name: &str, enabled: bool) -> bool {
        let mut found = false;
        for node in self
            .walkplanes
            .iter_mut()
            .chain(&mut self.blocks)
            .chain(&mut self.generics)
        {
            if node.name.eq_ignore_ascii_case(name) {
                node.active = enabled;
                found = true;
            }
        }
        found
    }

    /// Whether the node of this name is enabled (`IsNodeEnabled`): the
    /// first match among blocks, then walk planes, then generics; `false`
    /// for an unknown name.
    pub fn is_node_enabled(&self, name: &str) -> bool {
        self.blocks
            .iter()
            .chain(&self.walkplanes)
            .chain(&self.generics)
            .find(|node| node.name.eq_ignore_ascii_case(name))
            .is_some_and(|node| node.active)
    }

    /// A light by name, ignoring ASCII case.
    pub fn light(&self, name: &str) -> Option<&Light> {
        self.lights
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// A light by name, mutable.
    pub fn light_mut(&mut self, name: &str) -> Option<&mut Light> {
        self.lights
            .iter_mut()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Switches every light of this name on or off (`EnableLight`),
    /// ignoring ASCII case. Returns whether a light matched. The original
    /// re-registers all lights with the device afterwards
    /// (`CreateLights`); here the renderer reads the flags when it draws.
    pub fn set_light_enabled(&mut self, name: &str, enabled: bool) -> bool {
        let mut found = false;
        for light in self.lights.iter_mut() {
            if light.name.eq_ignore_ascii_case(name) {
                light.active = enabled;
                found = true;
            }
        }
        found
    }

    /// Whether the first light of this name is on (`IsLightEnabled`);
    /// `false` for an unknown name.
    pub fn is_light_enabled(&self, name: &str) -> bool {
        self.light(name).is_some_and(|light| light.active)
    }

    /// Sets the diffuse colour of every light of this name
    /// (`SetLightColor`), ignoring ASCII case. Returns whether a light
    /// matched.
    pub fn set_light_color(&mut self, name: &str, color: Rgb) -> bool {
        let mut found = false;
        for light in self.lights.iter_mut() {
            if light.name.eq_ignore_ascii_case(name) {
                light.color = color;
                found = true;
            }
        }
        found
    }
}

/// Where the segment `p1..p2` pierces a triangle, as the original tests
/// it in `DirectPathExists` and `GetBlockIntersection`; see
/// [`SceneGeometry::direct_path_exists`].
fn segment_pierces(p1: Vec3, p2: Vec3, [v0, v1, v2]: [Vec3; 3]) -> Option<Vec3> {
    math::pick_get_intersect(p1, p2, v0, v1, v2)?;
    math::intersect_triangle(p1, p1 - p2, v0, v1, v2)
        .or_else(|| math::intersect_triangle(p2, p2 - p1, v0, v1, v2))
}

/// `GetHeightAt` over a list of walk planes; see
/// [`SceneGeometry::height_at`].
fn height_at(planes: &[MeshNode], mut pos: Vec3, tolerance: f32) -> Option<f32> {
    let down = Vec3::new(0.0, -1.0, 0.0);
    pos.y += tolerance;
    let mut best: Option<f32> = None;
    for plane in planes {
        for [v0, v1, v2] in plane.mesh.triangles() {
            if let Some(hit) = math::intersect_triangle(pos, down, v0, v1, v2) {
                if hit.y > pos.y + tolerance {
                    continue; // only fall down
                }
                if best.is_none_or(|b| (b - pos.y).abs() > (hit.y - pos.y).abs()) {
                    best = Some(hit.y);
                }
            }
        }
    }
    best
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::three_ds::Builder;

    /// A 100x100 floor at height 0, facing up (clockwise seen from above
    /// in a left-handed Y-up space: (0,0,0) -> (0,0,100) -> (100,0,100)).
    pub(crate) fn floor(name: &str, height: f32) -> Vec<u8> {
        Builder::mesh(
            name,
            &[
                Vec3::new(0.0, height, 0.0),
                Vec3::new(0.0, height, 100.0),
                Vec3::new(100.0, height, 100.0),
                Vec3::new(100.0, height, 0.0),
            ],
            &[[0, 1, 2], [0, 2, 3]],
        )
    }

    /// A closed box of twelve triangles, faces in the order bottom, top,
    /// `-z`, `+z`, `-x`, `+x` (the order matters to
    /// `block_intersection`).
    pub(crate) fn box_mesh(name: &str, lo: Vec3, hi: Vec3) -> Vec<u8> {
        let v = [
            Vec3::new(lo.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, hi.z),
            Vec3::new(lo.x, lo.y, hi.z),
            Vec3::new(lo.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, hi.z),
            Vec3::new(lo.x, hi.y, hi.z),
        ];
        Builder::mesh(
            name,
            &v,
            &[
                [0, 1, 2],
                [0, 2, 3],
                [4, 6, 5],
                [4, 7, 6],
                [0, 4, 5],
                [0, 5, 1],
                [3, 6, 7],
                [3, 2, 6],
                [0, 7, 4],
                [0, 3, 7],
                [1, 5, 6],
                [1, 6, 2],
            ],
        )
    }

    /// A floor with a wall across its middle: the block `blk_wall` spans
    /// `x` 40..60, `z` 20..80, 30 high.
    pub(crate) fn wall_scene() -> SceneGeometry {
        let bytes = Builder::file(
            &[
                floor("walk_01", 0.0),
                box_mesh(
                    "blk_wall",
                    Vec3::new(40.0, 0.0, 20.0),
                    Vec3::new(60.0, 30.0, 80.0),
                ),
            ],
            &[],
        );
        SceneGeometry::load_3ds(&bytes, &NodePatterns::standard()).unwrap()
    }

    fn cube(name: &str, lo: Vec3, hi: Vec3) -> Vec<u8> {
        let v = [
            Vec3::new(lo.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, hi.z),
            Vec3::new(lo.x, lo.y, hi.z),
            Vec3::new(lo.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, hi.z),
            Vec3::new(lo.x, hi.y, hi.z),
        ];
        Builder::mesh(name, &v, &[[0, 1, 2], [0, 2, 3], [4, 6, 5], [4, 7, 6]])
    }

    fn demo() -> SceneGeometry {
        let bytes = Builder::file(
            &[
                floor("walk_01", 0.0),
                cube(
                    "blk_box",
                    Vec3::new(40.0, 0.0, 40.0),
                    Vec3::new(60.0, 30.0, 60.0),
                ),
                cube(
                    "wpt_01",
                    Vec3::new(10.0, 50.0, 10.0),
                    Vec3::new(20.0, 60.0, 20.0),
                ),
                cube(
                    "wpt_off",
                    Vec3::new(200.0, 50.0, 200.0),
                    Vec3::new(210.0, 60.0, 210.0),
                ),
                cube(
                    "shadow_1",
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(1.0, 1.0, 1.0),
                ),
                cube(
                    "ignored",
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(1.0, 1.0, 1.0),
                ),
                Builder::camera(
                    "Camera01",
                    Vec3::new(50.0, 100.0, -200.0),
                    Vec3::new(50.0, 0.0, 50.0),
                    0.0,
                    38.0,
                ),
                Builder::camera(
                    "Camera02",
                    Vec3::new(0.0, 100.0, -200.0),
                    Vec3::ZERO,
                    0.0,
                    0.0,
                ),
                Builder::light(
                    "Omni01",
                    Vec3::new(50.0, 300.0, 50.0),
                    [1.0, 1.0, 1.0],
                    None,
                    false,
                ),
                Builder::light(
                    "Spot01",
                    Vec3::new(0.0, 300.0, 0.0),
                    [0.5, 0.0, 0.0],
                    Some(Vec3::ZERO),
                    true,
                ),
            ],
            &[],
        );
        let patterns = NodePatterns::parse(
            b"GEOMETRY { NODE { NAME = \"shadow_*\" RECEIVE_SHADOWS = TRUE } }",
        )
        .unwrap();
        SceneGeometry::load_3ds(&bytes, &patterns).unwrap()
    }

    #[test]
    fn meshes_are_classified_by_the_node_patterns() {
        let g = demo();
        assert_eq!(g.walkplanes.len(), 1);
        assert_eq!(g.walkplanes[0].name, "walk_01");
        assert!(g.walkplanes[0].receive_shadows);
        assert_eq!(g.blocks.len(), 1);
        assert!(!g.blocks[0].receive_shadows);
        assert_eq!(g.generics.len(), 1);
        assert_eq!(g.generics[0].name, "shadow_1");
        assert!(g.generics[0].receive_shadows);
        assert_eq!(g.waypoint_groups.len(), 1);
        assert!(g.waypoint_groups[0].active);
        assert_eq!(g.waypoints().count(), 2);
        assert_eq!(g.cameras.len(), 2);
        assert_eq!(g.lights.len(), 2);
        assert_eq!(g.active_camera, Some(0));
    }

    #[test]
    fn waypoints_are_box_centres_dropped_to_the_floor() {
        let g = demo();
        let points = &g.waypoint_groups[0].points;
        // Centre (15, 55, 15) dropped to the floor at 0 plus the height 10.
        assert_eq!(points[0], Vec3::new(15.0, 10.0, 15.0));
        // Off the floor: keeps its own height, plus the waypoint height.
        assert_eq!(points[1], Vec3::new(205.0, 65.0, 205.0));
        let mut g = g;
        g.set_waypoint_height(25.0);
        let points = &g.waypoint_groups[0].points;
        assert_eq!(points[0], Vec3::new(15.0, 25.0, 15.0));
        assert_eq!(points[1], Vec3::new(205.0, 90.0, 205.0));
    }

    #[test]
    fn a_file_without_waypoint_meshes_has_no_group() {
        let bytes = Builder::file(&[floor("walk_01", 0.0)], &[]);
        let g = SceneGeometry::load_3ds(&bytes, &NodePatterns::standard()).unwrap();
        assert!(g.waypoint_groups.is_empty());
        assert_eq!(g.waypoints().count(), 0);
    }

    #[test]
    fn normals_point_up_on_the_floor() {
        let g = demo();
        for n in &g.walkplanes[0].mesh.normals {
            assert_eq!(*n, Vec3::UNIT_Y);
        }
        let mesh = TriangleMesh::new(vec![Vec3::ZERO], vec![[0, 1, 2]]);
        assert_eq!(mesh.normals, vec![Vec3::ZERO]);
        assert!(mesh.triangles().next().is_none());
        assert_eq!(TriangleMesh::default().center(), Vec3::ZERO);
    }

    #[test]
    fn height_at_finds_the_floor_from_above_only() {
        let g = demo();
        assert_eq!(g.height_at(Vec3::new(50.0, 80.0, 50.0), 0.0), Some(0.0));
        assert_eq!(g.height_at(Vec3::new(50.0, 80.0, 50.0), 5.0), Some(0.0));
        // Below the floor, no tolerance: the single-sided test still hits
        // the plane along the line, but the hit is above the start.
        assert_eq!(g.height_at(Vec3::new(50.0, -5.0, 50.0), 0.0), None);
        // Within the tolerance margin a plane slightly above counts.
        assert_eq!(g.height_at(Vec3::new(50.0, -5.0, 50.0), 5.0), Some(0.0));
        // Off the floor.
        assert_eq!(g.height_at(Vec3::new(500.0, 80.0, 50.0), 0.0), None);
    }

    #[test]
    fn the_nearest_floor_in_height_wins() {
        let bytes = Builder::file(&[floor("walk_low", 0.0), floor("walk_high", 50.0)], &[]);
        let g = SceneGeometry::load_3ds(&bytes, &NodePatterns::standard()).unwrap();
        assert_eq!(g.height_at(Vec3::new(10.0, 60.0, 10.0), 0.0), Some(50.0));
        assert_eq!(g.height_at(Vec3::new(10.0, 20.0, 10.0), 0.0), Some(0.0));
        // Start between the floors with a tolerance that reaches the upper
        // one: the height nearest to the *raised* start (20 + 40 = 60)
        // wins, so the upper floor does, although the point is below it.
        assert_eq!(g.height_at(Vec3::new(10.0, 20.0, 10.0), 40.0), Some(50.0));
        // With a small tolerance the upper floor is above the margin.
        assert_eq!(g.height_at(Vec3::new(10.0, 20.0, 10.0), 10.0), Some(0.0));
        assert_eq!(g.height_at(Vec3::new(10.0, 45.0, 10.0), 40.0), Some(50.0));
    }

    #[test]
    fn cameras_and_lights_are_converted() {
        let g = demo();
        let cam = &g.cameras[0];
        assert_eq!(cam.name, "Camera01");
        assert!((cam.fov - deg_to_rad(50.0)).abs() < 1e-6);
        assert_eq!(g.cameras[1].fov, deg_to_rad(45.0));
        assert_eq!(g.camera_index("CAMERA02"), Some(1));
        let omni = g.light("omni01").unwrap();
        assert!(!omni.spotlight && omni.active);
        assert_eq!(
            omni.color,
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        let spot = g.light("Spot01").unwrap();
        assert!(spot.spotlight && !spot.active);
        assert_eq!(spot.color, Rgb { r: 127, g: 0, b: 0 });
        assert_eq!(spot.target, Vec3::ZERO);
        let unnamed = three_ds::Light::default();
        assert_eq!(
            Light::from_3ds("x", &unnamed).color,
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
    }

    #[test]
    fn the_view_matrix_uses_the_bank() {
        let cam = Camera {
            name: "c".into(),
            position: Vec3::new(0.0, 0.0, -100.0),
            target: Vec3::ZERO,
            bank: 0.0,
            fov: 1.0,
        };
        let p = cam.view_matrix().transform_coord(Vec3::new(0.0, 10.0, 0.0));
        assert!((p.y - 10.0).abs() < 1e-4 && p.x.abs() < 1e-4);
        let banked = Camera { bank: 90.0, ..cam };
        // The up vector rotated by 90 degrees about Z is (-1, 0, 0); the
        // view's x axis becomes up x z = (0, 1, 0), so the world's +Y now
        // shows as the view's +X.
        let p = banked
            .view_matrix()
            .transform_coord(Vec3::new(0.0, 10.0, 0.0));
        assert!((p.x - 10.0).abs() < 1e-3 && p.y.abs() < 1e-3, "{p:?}");
    }

    #[test]
    fn nodes_and_cameras_by_name() {
        let mut g = demo();
        assert!(g.is_node_enabled("BLK_BOX"));
        assert!(g.set_node_enabled("blk_box", false));
        assert!(!g.is_node_enabled("blk_box"));
        assert!(!g.set_node_enabled("nothing", true));
        assert!(!g.is_node_enabled("nothing"));
        assert_eq!(g.nodes().count(), 3);
        assert!(g.set_active_camera("camera02"));
        assert_eq!(g.active_camera().unwrap().name, "Camera02");
        assert!(!g.set_active_camera("camera03"));
        assert_eq!(g.active_camera, Some(1));
        g.light_mut("omni01").unwrap().active = false;
        assert!(!g.light("Omni01").unwrap().active);
        assert!(g.light("none").is_none());
    }

    #[test]
    fn direct_paths_are_stopped_by_enabled_blocks() {
        let mut g = wall_scene();
        let s = Vec3::new(10.0, 10.0, 50.0);
        let t = Vec3::new(90.0, 10.0, 50.0);
        let a = Vec3::new(30.0, 10.0, 10.0);
        assert!(!g.direct_path_exists(s, t));
        assert!(!g.direct_path_exists(t, s));
        assert!(g.direct_path_exists(s, a));
        assert!(g.direct_path_exists(s, s));
        // Ending inside the wall still pierces its near face.
        assert!(!g.direct_path_exists(s, Vec3::new(50.0, 10.0, 50.0)));
        // Over the wall.
        assert!(g.direct_path_exists(Vec3::new(10.0, 40.0, 50.0), Vec3::new(90.0, 40.0, 50.0)));
        // The first pierced face in face order wins, from either end: the
        // -x face is listed before the +x face.
        assert_eq!(
            g.block_intersection(s, t),
            Some(Vec3::new(40.0, 10.0, 50.0))
        );
        assert_eq!(
            g.block_intersection(t, s),
            Some(Vec3::new(40.0, 10.0, 50.0))
        );
        assert_eq!(g.block_intersection(s, a), None);
        g.set_node_enabled("blk_wall", false);
        assert!(g.direct_path_exists(s, t));
        assert_eq!(g.block_intersection(s, t), None);
    }

    #[test]
    fn walk_planes_stop_segments_that_pierce_them_whether_enabled_or_not() {
        let mut g = wall_scene();
        let above = Vec3::new(20.0, 10.0, 10.0);
        let below = Vec3::new(20.0, -10.0, 10.0);
        assert!(!g.direct_path_exists(above, below));
        assert!(!g.direct_path_exists(below, above));
        // Touching the floor at one end counts as piercing it.
        assert!(!g.direct_path_exists(above, Vec3::new(20.0, 0.0, 10.0)));
        // Running inside the floor's plane does not.
        assert!(g.direct_path_exists(Vec3::new(20.0, 0.0, 10.0), Vec3::new(30.0, 0.0, 10.0)));
        g.set_node_enabled("walk_01", false);
        assert!(!g.direct_path_exists(above, below));
    }

    #[test]
    fn lights_are_switched_by_name() {
        let mut g = demo();
        assert!(g.is_light_enabled("OMNI01"));
        assert!(g.set_light_enabled("omni01", false));
        assert!(!g.is_light_enabled("Omni01"));
        assert!(!g.set_light_enabled("none", true));
        assert!(!g.is_light_enabled("none"));
        let red = Rgb { r: 255, g: 0, b: 0 };
        assert!(g.set_light_color("spot01", red));
        assert_eq!(g.light("Spot01").unwrap().color, red);
        assert!(!g.set_light_color("none", red));
    }

    #[test]
    fn a_file_without_cameras_has_no_active_camera() {
        let bytes = Builder::file(&[floor("walk_01", 0.0)], &[]);
        let g = SceneGeometry::load_3ds(&bytes, &NodePatterns::standard()).unwrap();
        assert_eq!(g.active_camera, None);
        assert!(g.active_camera().is_none());
        assert!(SceneGeometry::load_3ds(&[1, 2, 3], &NodePatterns::standard()).is_err());
    }
}
