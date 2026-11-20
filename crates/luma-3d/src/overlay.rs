//! The geometry overlay of `Scene.ShowGeometry`: the walk planes, the
//! blocks and the generic meshes of a scene's geometry as translucent
//! wireframes over the finished frame, and a marker at every waypoint
//! (`CAdSceneGeometry::Render(true)`).
//!
//! The original gives each mesh a vertex buffer of its triangles with one
//! colour (`C3DMesh::FillVertexBuffer(Color)`, after
//! `C3DMesh::ComputeNormals`, which [`TriangleMesh`] does when it is
//! built) and draws them in wireframe, unlit, without a depth test, with
//! alpha blending and `D3DCULL_CCW`: a triangle that is counter-clockwise
//! on the screen is not drawn, so a plane or a block shows only the faces
//! turned to the camera. This module does the geometry of it: which
//! edges, clipped to the view, where on the screen, in which colour. The
//! caller draws the lines.
//!
//! Written with reference to `AdSceneGeometry.cpp` (`Render`,
//! `LoadFile`), `3DMesh.cpp` (`FillVertexBuffer`, `ComputeNormals`) of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan Nedoma);
//! see `NOTICE`.

use crate::math::Mat4;
use crate::scene_geometry::{MeshNode, SceneGeometry};

/// The colour of the walk planes, `0xAARRGGBB`: blue, 44 % opaque.
pub const WALKPLANE_COLOR: u32 = 0x7000_00FF;
/// The colour of the blocks: red.
pub const BLOCK_COLOR: u32 = 0x70FF_0000;
/// The colour of the generic meshes: green.
pub const GENERIC_COLOR: u32 = 0x7000_FF00;

/// The sprite the original puts on every waypoint (`LoadFile`: loaded
/// from the editor's resources, else from the game's files).
pub const WAYPOINT_MARKER: &str = "wpt.sprite";

/// One edge of the overlay on the screen, in pixels of the screen the
/// projection draws on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WireLine {
    /// Start.
    pub from: (f32, f32),
    /// End.
    pub to: (f32, f32),
    /// The colour of the mesh, `0xAARRGGBB`.
    pub color: u32,
}

/// The edges of the overlay for a view, drawn the way the original draws
/// it: the active walk planes, then the active blocks, then the active
/// generic meshes, each edge of each front-facing triangle clipped to
/// the view volume. `view_projection` is the camera's view times its
/// projection (what the meshes are drawn with) and `size` the screen the
/// result maps onto, in pixels.
pub fn geometry_wireframe(
    geometry: &SceneGeometry,
    view_projection: &Mat4,
    size: (f32, f32),
) -> Vec<WireLine> {
    let mut lines = Vec::new();
    for (nodes, color) in [
        (&geometry.walkplanes, WALKPLANE_COLOR),
        (&geometry.blocks, BLOCK_COLOR),
        (&geometry.generics, GENERIC_COLOR),
    ] {
        for node in nodes.iter().filter(|n| n.active) {
            node_wireframe(node, view_projection, size, color, &mut lines);
        }
    }
    lines
}

/// The screen position of every waypoint of every group, active or not,
/// in file order (`D3DXVec3Project` of each point): where the original
/// displays the marker sprite. A point behind the camera or out of the
/// view is left out.
pub fn waypoint_positions(
    geometry: &SceneGeometry,
    view_projection: &Mat4,
    size: (f32, f32),
) -> Vec<(f32, f32)> {
    geometry
        .waypoints()
        .filter_map(|point| {
            let clip = view_projection.transform(*point);
            inside(clip).then(|| screen(clip, size))
        })
        .collect()
}

fn node_wireframe(
    node: &MeshNode,
    view_projection: &Mat4,
    size: (f32, f32),
    color: u32,
    lines: &mut Vec<WireLine>,
) {
    let clip: Vec<[f32; 4]> = node
        .mesh
        .vertices
        .iter()
        .map(|v| view_projection.transform(*v))
        .collect();
    for face in &node.mesh.faces {
        let corner = |i: usize| clip.get(usize::from(face[i])).copied();
        let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
            continue;
        };
        if counter_clockwise(a, b, c) {
            continue;
        }
        for (from, to) in [(a, b), (b, c), (c, a)] {
            if let Some((from, to)) = clip_segment(from, to) {
                lines.push(WireLine {
                    from: screen(from, size),
                    to: screen(to, size),
                    color,
                });
            }
        }
    }
}

/// Whether a triangle is counter-clockwise on the screen, from its clip
/// coordinates: the sign of the area of the projected triangle, taken
/// from the homogeneous coordinates so that a corner behind the camera
/// does not turn it over. Direct3D's front faces are clockwise.
fn counter_clockwise(a: [f32; 4], b: [f32; 4], c: [f32; 4]) -> bool {
    let det = a[0] * (b[1] * c[3] - b[3] * c[1]) - a[1] * (b[0] * c[3] - b[3] * c[0])
        + a[3] * (b[0] * c[1] - b[1] * c[0]);
    let area = f64::from(det) / (f64::from(a[3]) * f64::from(b[3]) * f64::from(c[3]));
    area > 0.0
}

/// Whether a clip-space point is inside the view volume (`-w <= x <= w`,
/// `-w <= y <= w`, `0 <= z <= w`).
fn inside(p: [f32; 4]) -> bool {
    planes(p).iter().all(|d| *d >= 0.0)
}

/// The distances of a clip-space point to the six planes of the view
/// volume; all are `>= 0` inside.
fn planes(p: [f32; 4]) -> [f32; 6] {
    let [x, y, z, w] = p;
    [w + x, w - x, w + y, w - y, z, w - z]
}

/// The part of a clip-space segment inside the view volume
/// (Liang-Barsky in homogeneous coordinates), `None` when it is outside.
fn clip_segment(p0: [f32; 4], p1: [f32; 4]) -> Option<([f32; 4], [f32; 4])> {
    let (d0, d1) = (planes(p0), planes(p1));
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for i in 0..6 {
        if d0[i] < 0.0 && d1[i] < 0.0 {
            return None;
        }
        if d0[i] < 0.0 {
            t0 = t0.max(d0[i] / (d0[i] - d1[i]));
        } else if d1[i] < 0.0 {
            t1 = t1.min(d0[i] / (d0[i] - d1[i]));
        }
    }
    if t0 > t1 {
        return None;
    }
    let at = |t: f32| {
        let mut p = [0.0; 4];
        for k in 0..4 {
            p[k] = p0[k] + (p1[k] - p0[k]) * t;
        }
        p
    };
    Some((at(t0), at(t1)))
}

/// A clip-space point on the screen: `x = (1 + x/w) * width / 2`,
/// `y = (1 - y/w) * height / 2`.
fn screen(p: [f32; 4], (width, height): (f32, f32)) -> (f32, f32) {
    (
        (1.0 + p[0] / p[3]) * width / 2.0,
        (1.0 - p[1] / p[3]) * height / 2.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{CameraSettings, Projection, ViewSetup};
    use crate::math::Vec3;
    use crate::scene_geometry::{Camera, TriangleMesh, WaypointGroup};

    fn node(vertices: Vec<Vec3>, faces: Vec<[u16; 3]>, active: bool) -> MeshNode {
        MeshNode {
            name: "n".to_string(),
            mesh: TriangleMesh::new(vertices, faces),
            receive_shadows: false,
            active,
        }
    }

    /// A camera 1000 units in front of the origin looking at it, 90
    /// degrees wide, on an 800x600 screen.
    fn view() -> (Mat4, (f32, f32)) {
        let camera = Camera {
            name: "c".to_string(),
            position: Vec3::new(0.0, 0.0, -1000.0),
            target: Vec3::ZERO,
            bank: 0.0,
            fov: std::f32::consts::FRAC_PI_2,
        };
        let setup = ViewSetup::full_screen((800, 600), (800, 600));
        let projection = Projection::new(&camera, &CameraSettings::default(), setup);
        (projection.view_projection(), (800.0, 600.0))
    }

    /// Clockwise seen from the camera at `-z` (x to the right, y up).
    fn clockwise() -> (Vec<Vec3>, Vec<[u16; 3]>) {
        (
            vec![
                Vec3::new(-100.0, -100.0, 0.0),
                Vec3::new(-100.0, 100.0, 0.0),
                Vec3::new(100.0, -100.0, 0.0),
            ],
            vec![[0, 1, 2]],
        )
    }

    fn geometry(
        planes: Vec<MeshNode>,
        blocks: Vec<MeshNode>,
        generics: Vec<MeshNode>,
    ) -> SceneGeometry {
        SceneGeometry {
            walkplanes: planes,
            blocks,
            generics,
            ..SceneGeometry::default()
        }
    }

    #[test]
    fn a_front_facing_triangle_is_three_edges_in_its_colour() {
        let (view_projection, size) = view();
        let (vertices, faces) = clockwise();
        let g = geometry(vec![node(vertices, faces, true)], vec![], vec![]);
        let lines = geometry_wireframe(&g, &view_projection, size);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.color == 0x7000_00FF));
        // (-100,-100) is left and below the centre (400, 300): 100 units
        // at 1000 away is 0.1 of half the height (300) either way.
        let (x, y) = lines[0].from;
        assert!(
            (x - 370.0).abs() < 0.5 && (y - 330.0).abs() < 0.5,
            "{x} {y}"
        );
        let (x, y) = lines[0].to;
        assert!(
            (x - 370.0).abs() < 0.5 && (y - 270.0).abs() < 0.5,
            "{x} {y}"
        );
    }

    #[test]
    fn a_counter_clockwise_triangle_is_culled() {
        let (view_projection, size) = view();
        let (vertices, mut faces) = clockwise();
        faces[0].swap(1, 2);
        let g = geometry(vec![], vec![node(vertices, faces, true)], vec![]);
        assert!(geometry_wireframe(&g, &view_projection, size).is_empty());
    }

    #[test]
    fn inactive_nodes_are_left_out_and_the_kinds_have_their_colours_in_order() {
        let (view_projection, size) = view();
        let (vertices, faces) = clockwise();
        let make = |active| node(vertices.clone(), faces.clone(), active);
        let g = geometry(
            vec![make(false), make(true)],
            vec![make(true)],
            vec![make(true)],
        );
        let colors: Vec<u32> = geometry_wireframe(&g, &view_projection, size)
            .iter()
            .map(|l| l.color)
            .collect();
        assert_eq!(
            colors,
            [[WALKPLANE_COLOR; 3], [BLOCK_COLOR; 3], [GENERIC_COLOR; 3]].concat()
        );
        assert_eq!(BLOCK_COLOR, 0x70FF0000);
        assert_eq!(GENERIC_COLOR, 0x7000FF00);
    }

    #[test]
    fn an_edge_is_cut_where_it_leaves_the_view_and_one_behind_the_camera_is_not_a_loop() {
        let (view_projection, size) = view();
        // A long floor edge running out of the screen on the right and
        // back past the camera: every end stays on the screen.
        let vertices = vec![
            Vec3::new(0.0, -100.0, 0.0),
            Vec3::new(0.0, 100.0, 0.0),
            Vec3::new(5000.0, -100.0, -2000.0),
        ];
        let g = geometry(vec![node(vertices, vec![[0, 1, 2]], true)], vec![], vec![]);
        for line in geometry_wireframe(&g, &view_projection, size) {
            for (x, y) in [line.from, line.to] {
                assert!(
                    (-0.5..=800.5).contains(&x) && (-0.5..=600.5).contains(&y),
                    "{x} {y}"
                );
            }
        }
        // Wholly outside: nothing.
        let far = vec![
            Vec3::new(9000.0, 0.0, 0.0),
            Vec3::new(9000.0, 100.0, 0.0),
            Vec3::new(9100.0, 0.0, 0.0),
        ];
        let g = geometry(vec![node(far, vec![[0, 1, 2]], true)], vec![], vec![]);
        assert!(geometry_wireframe(&g, &view_projection, size).is_empty());
    }

    #[test]
    fn a_face_with_an_index_out_of_range_is_skipped() {
        let (view_projection, size) = view();
        let (vertices, _) = clockwise();
        let g = geometry(vec![node(vertices, vec![[0, 1, 9]], true)], vec![], vec![]);
        assert!(geometry_wireframe(&g, &view_projection, size).is_empty());
    }

    #[test]
    fn the_waypoints_of_every_group_get_a_position_when_in_view() {
        let (view_projection, size) = view();
        let mut g = geometry(vec![], vec![], vec![]);
        g.waypoint_groups = vec![
            WaypointGroup {
                points: vec![Vec3::ZERO, Vec3::new(100.0, 100.0, 0.0)],
                active: true,
            },
            WaypointGroup {
                points: vec![Vec3::new(0.0, 0.0, -5000.0), Vec3::new(-100.0, 0.0, 0.0)],
                active: false,
            },
        ];
        let positions = waypoint_positions(&g, &view_projection, size);
        assert_eq!(
            positions.len(),
            3,
            "the point behind the camera is left out"
        );
        assert!((positions[0].0 - 400.0).abs() < 0.5 && (positions[0].1 - 300.0).abs() < 0.5);
        assert!((positions[1].0 - 430.0).abs() < 0.5 && (positions[1].1 - 270.0).abs() < 0.5);
        assert!((positions[2].0 - 370.0).abs() < 0.5);
    }
}
