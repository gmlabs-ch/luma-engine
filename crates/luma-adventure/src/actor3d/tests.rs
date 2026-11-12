//! Unit tests of the 3D actor's state machine on an empty geometry.

use super::*;
use luma_3d::{Camera, CameraSettings, ViewSetup, Viewport};

const TRIANGLE: &str = "xof 0303txt 0032\nFrame F { FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; } Mesh { 3; 0;0;0;, 1;0;0;, 0;1;0;; 1; 3;0,1,2;; } }";

fn actor() -> Actor3d {
    let model = Model::parse(TRIANGLE.as_bytes(), "t.x").expect("fixture parses");
    Actor3d::from_definition(Actor3dDef::default(), "a.act3d", false, model)
}

fn projection() -> Projection {
    let camera = Camera {
        name: String::new(),
        position: Vec3::new(0.0, 0.0, -10.0),
        target: Vec3::ZERO,
        bank: 0.0,
        fov: 1.0,
    };
    let setup = ViewSetup::for_scene(
        (640, 480),
        (640, 480),
        false,
        Some(Viewport::new(0, 0, 640, 480)),
        (0, 0),
    );
    Projection::new(&camera, &CameraSettings::default(), setup)
}

/// Runs frames of an actor in a scene without geometry or camera.
fn run(actor: &mut Actor3d, frames: usize) {
    let scene = Scene::default();
    let geometry = SceneGeometry::default();
    let mut finder = PathFinder::new();
    let projection = projection();
    for _ in 0..frames {
        finder.run(&geometry);
        let mut ctx = Update3d {
            scene: &scene,
            geometry: &geometry,
            finder: &mut finder,
            projection: &projection,
            has_camera: false,
            floor: None,
            timer: 0,
            delta_ms: 40,
        };
        actor.update(&mut ctx, &mut Vec::new());
    }
}

#[test]
fn an_actor_without_geometry_or_camera_still_ends_its_walk_and_has_no_2d_position() {
    let mut actor = actor();
    actor.go_to_3d(Vec3::new(5.0, 0.0, 5.0), -1.0);
    assert!(!actor.ready);
    actor.velocity = 100.0;
    actor.angular_velocity = 10_000.0;
    run(&mut actor, 200);
    // The walk reaches its target, ends in `Ready` and the scripts waiting
    // for it go on, though the scene has no geometry and no camera.
    assert_eq!(actor.state, Actor3dState::Ready);
    assert!(actor.ready);
    assert_eq!((actor.position.x, actor.position.z), (5.0, 5.0));
    assert_eq!((actor.x, actor.y), (0, 0));
}

#[test]
fn an_empty_path_without_after_walk_angle_is_ready_even_after_an_interrupted_turn() {
    let mut actor = actor();
    // A walk interrupted during its first turn leaves this next state.
    actor.next_state = Actor3dState::FollowingPath;
    actor.after_walk_angle = -1.0;
    actor.arrive_empty();
    assert_eq!(actor.state, Actor3dState::Ready);
}

#[test]
fn a_script_animation_that_is_not_playing_ends_without_a_camera() {
    let mut actor = actor();
    actor.state = Actor3dState::PlayingAnim;
    actor.state_anim_channel = 0;
    actor.ready = false;
    run(&mut actor, 1);
    assert_eq!(actor.state, Actor3dState::Ready);
    assert!(actor.ready);
}

#[test]
fn turning_alone_under_direct_control_keeps_the_state_and_turns() {
    let mut actor = actor();
    actor.direct_turn(true, 90.0, None);
    run(&mut actor, 2);
    assert_eq!(actor.state, Actor3dState::DirectControl);
    assert!(actor.angle > 0.0);
}

#[test]
fn stop_anim_of_an_attachment_stops_every_channel() {
    let model = Model::parse(TRIANGLE.as_bytes(), "t.x").expect("fixture parses");
    let mut attachment = Attachment3d {
        id: NativeId(1),
        name: "a".into(),
        bone: "F".into(),
        model_file: "t.x".into(),
        model: ModelSlot {
            model: Some(AnimatedModel::new(model)),
            pending: None,
        },
        active: true,
        position: Vec3::ZERO,
        angle: 0.0,
        scale: 1.0,
        draw_backfaces: true,
        world_matrix: Mat4::IDENTITY,
        bounding_rect: None,
    };
    assert!(attachment.stop_all_anims());
    attachment.model = ModelSlot::default();
    assert!(!attachment.stop_all_anims());
}

fn vfs() -> Vfs {
    let mut vfs = Vfs::new();
    vfs.add_resource("main.x", TRIANGLE.as_bytes().to_vec());
    vfs.add_resource("shadow.x", TRIANGLE.as_bytes().to_vec());
    vfs.add_resource(
        "bob.act3d",
        b"ACTOR3DX { MODEL=\"main.x\" SHADOW_MODEL=\"shadow.x\" }".to_vec(),
    );
    vfs.add_resource(
        "plain.act3d",
        b"ACTOR3DX { MODEL=\"main.x\" SHADOW_MODEL=\"missing.x\" }".to_vec(),
    );
    vfs
}

#[test]
fn a_shadow_model_is_loaded_and_posed_after_the_main_model() {
    let vfs = vfs();
    let loaded = Actor3d::load(&vfs, "bob.act3d", false).expect("the actor loads");
    let mut actor = loaded.actor;
    assert!(actor.shadow_model.is_some());
    run(&mut actor, 1);
    let shadow = actor.shadow_model.as_ref().unwrap();
    assert_eq!(shadow.meshes().len(), 1);
    assert_eq!(shadow.meshes()[0].vertices.len(), 3);
}

#[test]
fn merge_anims_fails_on_a_bad_sidecar_although_the_sets_are_merged() {
    let mut vfs = vfs();
    vfs.add_resource("extra.x", TRIANGLE.as_bytes().to_vec());
    vfs.add_resource("extra.anim", b"BOGUS { }".to_vec());
    let mut actor = Actor3d::load(&vfs, "bob.act3d", false)
        .expect("the actor loads")
        .actor;
    assert!(actor.merge_anims(&vfs, "extra.x").is_err());
    // The path is remembered, so a saved game merges the sets again.
    assert_eq!(actor.merged_files, vec!["extra.x".to_string()]);
}

#[test]
fn a_shadow_model_that_cannot_be_loaded_leaves_the_actor_without_one() {
    let vfs = vfs();
    let loaded = Actor3d::load(&vfs, "plain.act3d", false).expect("the actor still loads");
    assert!(loaded.actor.shadow_model.is_none());
}
