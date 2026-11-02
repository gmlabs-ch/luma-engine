//! Opt-in test on the geometry of the Wintermute 3D demo
//! (`install/source/projects/wme_demo_3d` of the Wintermute Engine 1.x
//! repository), copied by hand under `testdata/local/wme_demo_3d/`, which
//! is git-ignored. The test skips when the files are absent.

use std::path::PathBuf;

use luma_3d::camera::{CameraSettings, Projection, ViewSetup};
use luma_3d::math::deg_to_rad;
use luma_3d::{NodeKind, NodePatterns, PathFinder, SceneGeometry, Vec3};

fn demo_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/local/wme_demo_3d/scenes/front");
    if dir.join("geometry.3DS").is_file() {
        Some(dir)
    } else {
        eprintln!("skipped: {} is absent", dir.display());
        None
    }
}

#[test]
fn the_front_scene_of_the_demo_loads_as_the_study_describes() {
    let Some(dir) = demo_dir() else { return };
    let bytes = std::fs::read(dir.join("geometry.3DS")).unwrap();
    let sidecar = std::fs::read(dir.join("geometry.geometry")).ok();
    let patterns = NodePatterns::load(sidecar.as_deref());
    assert_eq!(patterns.nodes.len(), 4);
    assert_eq!(patterns.nodes[0].pattern, "shadow_*");
    assert_eq!(patterns.nodes[0].kind, NodeKind::Generic);
    assert!(patterns.nodes[0].receive_shadows);

    let geometry = SceneGeometry::load_3ds(&bytes, &patterns).unwrap();
    let names =
        |nodes: &[luma_3d::MeshNode]| nodes.iter().map(|n| n.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&geometry.walkplanes), vec!["walk_01"]);
    assert_eq!(geometry.blocks.len(), 5, "{:?}", names(&geometry.blocks));
    assert!(geometry.blocks.iter().all(|b| b.name.starts_with("blk_")));
    assert_eq!(geometry.waypoint_groups.len(), 1);
    assert!(geometry.waypoint_groups[0].active);
    assert_eq!(geometry.waypoint_groups[0].points.len(), 10);
    assert_eq!(
        geometry.generics.len(),
        12,
        "{:?}",
        names(&geometry.generics)
    );
    assert!(
        geometry
            .generics
            .iter()
            .all(|g| g.name.starts_with("shadow_") && g.receive_shadows)
    );
    let lights: Vec<_> = geometry.lights.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(lights, vec!["Spot01", "Spot02", "Omni02", "Omni03"]);
    assert!(geometry.lights[0].spotlight && !geometry.lights[2].spotlight);
    let cameras: Vec<_> = geometry.cameras.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cameras, vec!["Camera01", "Camera02"]);
    assert_eq!(geometry.active_camera().unwrap().name, "Camera01");

    // Every waypoint sits 10 units over the walk plane.
    for point in geometry.waypoints() {
        let floor = geometry.height_at(*point, 0.0).unwrap();
        assert!(
            (point.y - floor - 10.0).abs() < 1e-3,
            "{point:?} over {floor}"
        );
    }

    // The scene is 800x600 and selects Camera01: the walk-to points of
    // its entities land on the walk plane, and the hit projects back to
    // the same pixel (give or take the truncation).
    let projection = Projection::new(
        geometry.active_camera().unwrap(),
        &CameraSettings::default(),
        ViewSetup::full_screen((800, 600), (800, 600)),
    );
    let fov = projection.fov();
    assert!(
        fov > deg_to_rad(10.0) && fov < deg_to_rad(120.0),
        "fov {fov}"
    );
    for (x, y) in [(564, 433), (400, 500), (200, 550)] {
        let hit = geometry
            .convert_2d_to_3d_tolerant(&projection, x, y)
            .unwrap_or_else(|| panic!("no walk plane under ({x}, {y})"));
        let (bx, by) = projection.project(hit);
        assert!(
            (bx - x).abs() <= 1 && (by - y).abs() <= 1,
            "({x}, {y}) -> {hit:?} -> ({bx}, {by})"
        );
        let floor = geometry
            .height_at(hit + Vec3::new(0.0, 5.0, 0.0), 0.0)
            .unwrap();
        assert!((floor - hit.y).abs() < 1e-2);
    }
    // The sky is not a walk plane.
    assert!(geometry.convert_2d_to_3d(&projection, 400, 50).is_none());
}

#[test]
fn paths_on_the_demo_geometry_link_the_waypoints() {
    let Some(dir) = demo_dir() else { return };
    let bytes = std::fs::read(dir.join("geometry.3DS")).unwrap();
    let sidecar = std::fs::read(dir.join("geometry.geometry")).ok();
    let geometry =
        SceneGeometry::load_3ds(&bytes, &NodePatterns::load(sidecar.as_deref())).unwrap();
    let points: Vec<Vec3> = geometry.waypoints().copied().collect();
    assert!(points.len() >= 2);

    // The segment test does not depend on the direction, and at least one
    // pair of waypoints is separated by a crate (else they would be
    // pointless).
    let mut blocked_pairs = 0;
    for a in &points {
        for b in &points {
            let free = geometry.direct_path_exists(*a, *b);
            assert_eq!(free, geometry.direct_path_exists(*b, *a));
            if !free {
                blocked_pairs += 1;
            }
        }
    }
    assert!(blocked_pairs > 0);

    // Every waypoint reaches every other one over free segments at the
    // waypoint height, ending at the target.
    for a in &points {
        for b in &points {
            if a == b {
                continue;
            }
            let path = PathFinder::find_path(&geometry, *a, *b);
            assert!(path.is_ready());
            let steps = path.points();
            assert!(!steps.is_empty(), "no path {a:?} -> {b:?}");
            assert!(
                (*steps.last().unwrap() - *b).length() < 1e-2,
                "{a:?} -> {b:?} ends at {:?}",
                steps.last().unwrap()
            );
            let mut prev = *a;
            for p in steps {
                assert!(geometry.direct_path_exists(prev, *p), "{prev:?} -> {p:?}");
                let floor = geometry.height_at(*p, 0.0).unwrap();
                assert!((p.y - floor - 10.0).abs() < 1e-2, "{p:?} over {floor}");
                prev = *p;
            }
        }
    }
}
