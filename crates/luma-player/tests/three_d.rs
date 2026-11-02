//! Runs the player headless on a synthetic 3D game and reads the
//! screenshot back: a lit model standing on a painted background, with
//! each kind of shadow. Skipped when the machine has no graphics adapter.

use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use luma::archive::PackageBuilder;
use luma::formats::{Image, Rgba};
use luma::script::Instruction;
use luma::script::asm::Assembler;
use luma::three_d::three_ds::Builder;
use luma::three_d::{CameraSettings, NodePatterns, Projection, SceneGeometry, Vec3, ViewSetup};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

fn player(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_luma-player"))
        .args(args)
        .output()
        .expect("run luma-player")
}

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-player-3d-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Game.ChangeScene("scenes\room.scene")`.
fn game_script() -> Vec<u8> {
    let mut a = Assembler::new("game.script");
    a.push_int(0);
    a.push_int(0);
    a.push_string("scenes\\room.scene");
    a.push_int(3);
    a.push_var("Game");
    a.push_string("ChangeScene");
    a.emit(Instruction::CallByExp);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::Ret);
    a.build()
}

/// `Game.LoadActor3D("actors\bob.act3d")`.
fn room_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room.script");
    a.push_string("actors\\bob.act3d");
    a.push_int(1);
    a.push_var("Game");
    a.push_string("LoadActor3D");
    a.emit(Instruction::CallByExp);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::Ret);
    a.build()
}

/// The camera of the scene: above and in front of the floor, looking at
/// its middle.
const CAMERA_POSITION: Vec3 = Vec3::new(50.0, 60.0, -80.0);
const CAMERA_TARGET: Vec3 = Vec3::new(50.0, 10.0, 50.0);
/// The actor stands in the middle of the floor.
const ACTOR: Vec3 = Vec3::new(50.0, 0.0, 50.0);

/// A 100x100 floor at height 0 (a walk plane, which receives shadows),
/// the camera and a white light above and in front of the actor.
fn geometry() -> Vec<u8> {
    let objects = vec![
        Builder::mesh(
            "walk_01",
            &[
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 100.0),
                Vec3::new(100.0, 0.0, 100.0),
                Vec3::new(100.0, 0.0, 0.0),
            ],
            &[[0, 1, 2], [0, 2, 3]],
        ),
        Builder::camera("Camera01", CAMERA_POSITION, CAMERA_TARGET, 0.0, 38.0),
        Builder::light(
            "Omni01",
            Vec3::new(50.0, 200.0, -100.0),
            [1.0, 1.0, 1.0],
            None,
            false,
        ),
    ];
    Builder::file(&objects, &[])
}

/// A white quad 20 wide and 40 high, standing on the origin and facing
/// `-z`, toward the camera, with a still `idle` animation.
const MODEL: &str = r#"xof 0303txt 0032

AnimTicksPerSecond { 4800; }

Frame Root {
 FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; }
 Mesh body {
  4;
  -10.0;0.0;0.0;,
  10.0;0.0;0.0;,
  10.0;40.0;0.0;,
  -10.0;40.0;0.0;;
  2;
  3;0,2,1;,
  3;0,3,2;;
  MeshNormals { 1; 0.0;0.0;-1.0;; 2; 3;0,0,0;, 3;0,0,0;; }
  MeshMaterialList {
   1; 2; 0, 0;;
   Material { 1.0;1.0;1.0;1.0;; 0.0; 0.0;0.0;0.0;; 0.0;0.0;0.0;; }
  }
 }
}

AnimationSet idle {
 Animation {
  AnimationKey { 2; 2; 0;3;0.0,0.0,0.0;;, 4800;3;0.0,0.0,0.0;;; }
  { Root }
 }
}
"#;

const ACTOR_FILE: &str = r#"ACTOR3DX
{
  NAME = "bob"
  X = 50.0
  Y = 0.0
  Z = 50.0
  ANGLE = 0.0
  SCALE = 100
  MODEL = "actors\bob.x"
  LIGHT_POSITION { -100.0, 150.0, -40.0 }
  ANIMATION { NAME = "idle"  LOOPING = TRUE }
}
"#;

/// Builds the game with the scene's `MAX_SHADOW_TYPE` and returns its
/// directory.
fn game(max_shadow_type: u8) -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        format!("SETTINGS {{ GAME=\"default.game\" RESOLUTION {{ {WIDTH},{HEIGHT} }} }}")
            .into_bytes(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Test3D\" SCRIPT=\"game.script\" } AD_GAME { }".to_vec(),
    );
    pkg.add_file("game.script", game_script());
    pkg.add_file("scenes\\room.script", room_script());
    pkg.add_stored_file(
        "bg.png",
        Image {
            width: WIDTH,
            height: HEIGHT,
            pixels: [0u8, 255, 0, 255].repeat((WIDTH * HEIGHT) as usize),
            has_alpha_channel: true,
        }
        .to_png()
        .unwrap(),
    );
    pkg.add_file(
        "scenes\\room.scene",
        format!(
            "SCENE {{
  NAME=\"Room\"
  GEOMETRY=\"scenes\\geo.3ds\"
  CAMERA=\"Camera01\"
  MAX_SHADOW_TYPE={max_shadow_type}
  SCRIPT=\"scenes\\room.script\"
  LAYER {{
    NAME=\"main\" MAIN=TRUE WIDTH={WIDTH} HEIGHT={HEIGHT}
    ENTITY {{ NAME=\"bg\" SPRITE=\"bg.png\" }}
    REGION {{ NAME=\"floor\" POINT {{0,0}} POINT {{{WIDTH},0}} POINT {{{WIDTH},{HEIGHT}}} POINT {{0,{HEIGHT}}} }}
  }}
}}"
        )
        .into_bytes(),
    );
    pkg.add_file("scenes\\geo.3ds", geometry());
    pkg.add_file("actors\\bob.x", MODEL.as_bytes().to_vec());
    pkg.add_file("actors\\bob.act3d", ACTOR_FILE.as_bytes().to_vec());
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

/// The projection the player uses for the scene, to find the pixels of
/// world points.
fn projection() -> Projection {
    let geometry = SceneGeometry::load_3ds(&geometry(), &NodePatterns::load(None)).unwrap();
    let setup =
        ViewSetup::full_screen((WIDTH as i32, HEIGHT as i32), (WIDTH as i32, HEIGHT as i32));
    Projection::new(
        geometry.active_camera().unwrap(),
        &CameraSettings::default(),
        setup,
    )
}

fn pixel_at(image: &Image, projection: &Projection, world: Vec3) -> Rgba {
    let (x, y) = projection.project(world);
    image
        .pixel(x as u32, y as u32)
        .unwrap_or_else(|| panic!("{world:?} projects outside the screen: ({x}, {y})"))
}

const GREEN: (u8, u8, u8) = (0, 255, 0);

/// Runs the game headless for a few frames and screenshots it; `None`
/// when there is no graphics adapter.
fn screenshot(max_shadow_type: u8) -> Option<Image> {
    let dir = game(max_shadow_type);
    let shot = dir.join("shot.png");
    let out = player(&[
        dir.to_str().unwrap(),
        "--headless",
        "--mute",
        "--ticks",
        "4",
        "--screenshot",
        shot.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stderr.contains("no graphics adapter") {
        eprintln!("skipped: {stderr}");
        return None;
    }
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(
        !stdout.contains("Error") && !stdout.contains("error"),
        "{stdout}"
    );
    let image = luma::formats::image::decode(
        &std::fs::read(&shot).unwrap(),
        luma::formats::DEFAULT_COLOR_KEY,
    )
    .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    Some(image)
}

fn rgb(p: Rgba) -> (u8, u8, u8) {
    (p.r, p.g, p.b)
}

#[test]
fn draws_the_actor_on_the_background_with_each_shadow_type() {
    let projection = projection();
    // The middle of the body, a floor point far from everything, a
    // floor point in the flat and stencil shadow (the shadow light is
    // high to the left and in front of the actor, so the body's shadow
    // lies on the floor behind it to the right, where the camera sees
    // it past the body) and a floor point just in front of the feet,
    // under the simple shadow.
    let body = ACTOR + Vec3::new(0.0, 20.0, 0.0);
    let far = Vec3::new(5.0, 0.0, 95.0);
    let cast = Vec3::new(75.0, 0.0, 58.0);
    let under = Vec3::new(50.0, 0.0, 45.0);

    // Scene maximum 0: the model alone.
    let Some(image) = screenshot(0) else { return };
    assert_eq!((image.width, image.height), (WIDTH, HEIGHT));
    let p = pixel_at(&image, &projection, body);
    assert!(p.r > 150 && p.g > 150 && p.b > 150, "lit body, got {p:?}");
    assert_eq!(rgb(pixel_at(&image, &projection, far)), GREEN);
    assert_eq!(rgb(pixel_at(&image, &projection, cast)), GREEN);
    assert_eq!(rgb(pixel_at(&image, &projection, under)), GREEN);

    // Simple: the dark blob under the feet, nothing behind.
    let Some(image) = screenshot(1) else { return };
    let p = pixel_at(&image, &projection, under);
    assert!(
        p.r == 0 && p.b == 0 && p.g < 220,
        "simple shadow, got {p:?}"
    );
    assert_eq!(rgb(pixel_at(&image, &projection, cast)), GREEN);
    assert_eq!(rgb(pixel_at(&image, &projection, far)), GREEN);

    // Flat and stencil: the silhouette behind the actor, half black.
    for max in [2, 3] {
        let Some(image) = screenshot(max) else { return };
        let p = pixel_at(&image, &projection, cast);
        assert!(
            p.r == 0 && p.b == 0 && p.g > 100 && p.g < 160,
            "shadow type {max}, got {p:?}"
        );
        assert_eq!(rgb(pixel_at(&image, &projection, far)), GREEN);
        assert_eq!(rgb(pixel_at(&image, &projection, under)), GREEN);
        let p = pixel_at(&image, &projection, body);
        assert!(p.r > 150 && p.g > 150 && p.b > 150, "body {max}: {p:?}");
    }
}
