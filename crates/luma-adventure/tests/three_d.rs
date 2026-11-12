//! A synthetic 3D game: a scene with geometry (a floor, a wall, four
//! waypoints, a camera, a light), a 3D actor with a tiny `.X` model, and
//! scripts that load the actor, walk it around the wall, turn it, play an
//! animation, save and load; a second test attaches a model to a bone,
//! replaces a texture, hangs a particle emitter on a bone and saves.
//! Runs only with the `3d` feature.

#![cfg(feature = "3d")]

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_3d::Vec3;
use luma_3d::three_ds::Builder;
use luma_adventure::actor3d::world_matrix;
use luma_adventure::{Actor3dState, Audio, Kind, MaterialOverride, MaterialTexture, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_script::asm::Assembler;
use luma_script::{Instruction, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-3d-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An argument of a call in the assembled scripts.
enum Arg {
    Int(i32),
    Str(&'static str),
    Var(&'static str),
    /// `result.<key>`.
    Result(&'static str),
}

fn push_arg(a: &mut Assembler, arg: &Arg) {
    match arg {
        Arg::Int(i) => a.push_int(*i),
        Arg::Str(s) => a.push_string(s),
        Arg::Var(v) => a.push_var(v),
        Arg::Result(key) => get(a, "result", key),
    }
}

/// `object.Method(args)`, leaving the result on the stack.
fn call(a: &mut Assembler, object: &str, method: &str, args: &[Arg]) {
    for arg in args.iter().rev() {
        push_arg(a, arg);
    }
    a.push_int(args.len() as i32);
    a.push_var(object);
    a.push_string(method);
    a.emit(Instruction::CallByExp);
}

/// `object.Prop`, pushed.
fn get(a: &mut Assembler, object: &str, prop: &str) {
    a.push_var_ref(object);
    a.push_string(prop);
    a.emit(Instruction::PushByExp);
}

/// `object.Prop = <top of stack>`.
fn set(a: &mut Assembler, object: &str, prop: &str) {
    a.push_var_ref(object);
    a.push_string(prop);
    a.emit(Instruction::PopByExp);
}

/// `result.key = <top of stack>` (`result` is an engine global object).
fn store(a: &mut Assembler, key: &str) {
    set(a, "result", key);
}

/// `result = new Object(); Game.ChangeScene("scenes\room.scene");`
fn game_script() -> Vec<u8> {
    let mut a = Assembler::new("game.script");
    a.def_glob_var("result");
    a.emit(Instruction::CreateObject);
    a.pop_var("result");
    a.def_glob_var("bob");
    call(
        &mut a,
        "Game",
        "ChangeScene",
        &[Arg::Str("scenes\\room.scene"), Arg::Int(0), Arg::Int(0)],
    );
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::Ret);
    a.build()
}

/// The scene's script: loads the actor and inspects the geometry, then
/// handles the events the test raises.
fn room_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room.script");
    a.def_glob_var("result");
    a.def_glob_var("bob");
    a.def_glob_var("Scene");
    get(&mut a, "Game", "Scene");
    a.pop_var("Scene");
    // bob = Scene.LoadActor3D("actors\bob.act3d"); result.bob = bob;
    call(
        &mut a,
        "Scene",
        "LoadActor3D",
        &[Arg::Str("actors\\bob.act3d")],
    );
    a.pop_var("bob");
    a.push_var("bob");
    store(&mut a, "bob");
    get(&mut a, "bob", "Type");
    store(&mut a, "type");
    get(&mut a, "bob", "Caption");
    store(&mut a, "caption");
    // Game.LoadActor sniffs the file: a 3D actor too, unloaded at once.
    a.def_var("other");
    call(
        &mut a,
        "Game",
        "LoadActor",
        &[Arg::Str("actors\\bob.act3d")],
    );
    a.pop_var("other");
    get(&mut a, "other", "Type");
    store(&mut a, "type2");
    call(&mut a, "Game", "UnloadObject", &[Arg::Var("other")]);
    a.emit(Instruction::PopEmpty);
    // The scene's 3D side.
    get(&mut a, "Scene", "GeometryFile");
    store(&mut a, "geo");
    get(&mut a, "Scene", "NumLights");
    store(&mut a, "lights");
    call(&mut a, "Scene", "IsLightEnabled", &[Arg::Str("Omni01")]);
    store(&mut a, "lightOn");
    call(&mut a, "Scene", "GetLightName", &[Arg::Int(0)]);
    store(&mut a, "lightName");
    call(&mut a, "Scene", "DisableNode3D", &[Arg::Str("blk_wall")]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "Scene", "IsNode3DEnabled", &[Arg::Str("blk_wall")]);
    store(&mut a, "wallOff");
    call(&mut a, "Scene", "EnableNode3D", &[Arg::Str("blk_wall")]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "Scene", "IsNode3DEnabled", &[Arg::Str("blk_wall")]);
    store(&mut a, "wallOn");
    get(&mut a, "Game", "MaxShadowType");
    store(&mut a, "gameShadow");
    get(&mut a, "Game", "SupportsRealTimeShadows");
    store(&mut a, "realTime");
    a.push_int(1);
    set(&mut a, "Scene", "MaxShadowType");
    get(&mut a, "Scene", "MaxShadowType");
    store(&mut a, "sceneShadow");
    // Game.MainObject = bob;
    a.push_var("bob");
    set(&mut a, "Game", "MainObject");
    a.emit(Instruction::Ret);

    // on "walk": bob.GoTo(result.tx, result.ty); result.arrived = true;
    a.event("walk");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("bob");
    call(
        &mut a,
        "bob",
        "GoTo",
        &[Arg::Result("tx"), Arg::Result("ty")],
    );
    a.emit(Instruction::PopEmpty);
    a.push_bool(true);
    store(&mut a, "arrived");
    a.emit(Instruction::RetEvent);

    // on "turn": bob.TurnTo(DI_LEFT); result.turnedAngle = bob.Angle;
    a.event("turn");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("bob");
    call(&mut a, "bob", "TurnTo", &[Arg::Int(6)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "bob", "Angle");
    store(&mut a, "turnedAngle");
    get(&mut a, "bob", "Direction");
    store(&mut a, "turnedDir");
    a.emit(Instruction::RetEvent);

    // on "wave": result.waveStarted = bob.PlayAnimAsync("wave");
    // result.playing = bob.IsAnimPlaying("wave");
    // result.playingIdle = bob.IsAnimPlaying("idle");
    a.event("wave");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("bob");
    call(&mut a, "bob", "PlayAnimAsync", &[Arg::Str("wave")]);
    store(&mut a, "waveStarted");
    call(&mut a, "bob", "IsAnimPlaying", &[Arg::Str("wave")]);
    store(&mut a, "playing");
    call(&mut a, "bob", "IsAnimPlaying", &[Arg::Str("idle")]);
    store(&mut a, "playingIdle");
    get(&mut a, "bob", "Ready");
    store(&mut a, "readyWhileWaving");
    a.emit(Instruction::RetEvent);

    // on "attach": the attachment, texture, bone emitter and shadow
    // members (see `attachments_textures_and_bone_emitters`).
    a.event("attach");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("bob");
    a.def_glob_var("Scene");
    call(
        &mut a,
        "bob",
        "AddAttachment",
        &[Arg::Str("actors\\hat.x"), Arg::Str("hat"), Arg::Str("Root")],
    );
    store(&mut a, "added");
    call(
        &mut a,
        "bob",
        "AddAttachment",
        &[Arg::Str("actors\\hat.x"), Arg::Str("x"), Arg::Str("Nope")],
    );
    store(&mut a, "badBone");
    call(
        &mut a,
        "bob",
        "AddAttachment",
        &[
            Arg::Str("actors\\missing.x"),
            Arg::Str("y"),
            Arg::Str("Root"),
        ],
    );
    store(&mut a, "badFile");
    a.def_var("att");
    call(&mut a, "bob", "GetAttachment", &[Arg::Str("HAT")]);
    a.pop_var("att");
    a.push_var("att");
    store(&mut a, "att");
    call(&mut a, "bob", "GetAttachment", &[Arg::Int(0)]);
    store(&mut a, "att0");
    get(&mut a, "att", "Type");
    store(&mut a, "attType");
    get(&mut a, "att", "Name");
    store(&mut a, "attName");
    get(&mut a, "att", "Bone");
    store(&mut a, "attBone");
    get(&mut a, "bob", "NumAttachments");
    store(&mut a, "numAtt");
    a.push_float(4.0);
    set(&mut a, "att", "PosY");
    call(&mut a, "att", "PlayAnimAsync", &[Arg::Str("spin")]);
    store(&mut a, "attPlay");
    call(
        &mut a,
        "bob",
        "SetTexture",
        &[Arg::Str("body"), Arg::Str("actors\\tex.png")],
    );
    store(&mut a, "tex");
    call(
        &mut a,
        "bob",
        "SetTexture",
        &[Arg::Str("body"), Arg::Str("actors\\missing.png")],
    );
    store(&mut a, "texBad");
    call(
        &mut a,
        "bob",
        "SetTheoraTexture",
        &[Arg::Str("screen"), Arg::Str("actors\\clip.ogv")],
    );
    store(&mut a, "theora");
    call(
        &mut a,
        "bob",
        "CreateParticleEmitterBone",
        &[Arg::Str("Root"), Arg::Int(0), Arg::Int(1), Arg::Int(0)],
    );
    store(&mut a, "emitter");
    call(
        &mut a,
        "Scene",
        "SetShadowImage",
        &[Arg::Str("actors\\tex.png")],
    );
    store(&mut a, "sceneShadowSet");
    call(&mut a, "Scene", "GetShadowImage", &[]);
    store(&mut a, "sceneShadowImg");
    call(
        &mut a,
        "Scene",
        "SetLightPosition",
        &[Arg::Int(1), Arg::Int(2), Arg::Int(3)],
    );
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);

    // on "detach": the attachment goes, with its object.
    a.event("detach");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("bob");
    call(&mut a, "bob", "RemoveAttachment", &[Arg::Str("hat")]);
    store(&mut a, "removed");
    call(&mut a, "bob", "RemoveAttachment", &[Arg::Str("hat")]);
    store(&mut a, "removedAgain");
    call(&mut a, "bob", "GetAttachment", &[Arg::Str("hat")]);
    store(&mut a, "gone");
    call(&mut a, "Game", "ValidObject", &[Arg::Result("att")]);
    store(&mut a, "attValid");
    a.emit(Instruction::RetEvent);

    // on "SaveIt": result.saved = Game.SaveGame(3, "mid walk");
    a.event("SaveIt");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(
        &mut a,
        "Game",
        "SaveGame",
        &[Arg::Int(3), Arg::Str("mid walk")],
    );
    store(&mut a, "saved");
    a.emit(Instruction::RetEvent);

    // on "LoadIt": Game.LoadGame(3);
    a.event("LoadIt");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(&mut a, "Game", "LoadGame", &[Arg::Int(3)]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    a.build()
}

/// The actor's script: counts the `footstep` animation events.
fn bob_script() -> Vec<u8> {
    let mut a = Assembler::new("actors\\bob.script");
    a.emit(Instruction::Ret);
    a.event("footstep");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    get(&mut a, "result", "steps");
    a.push_int(1);
    a.emit(Instruction::Add);
    store(&mut a, "steps");
    a.emit(Instruction::RetEvent);
    a.build()
}

/// A closed box of twelve triangles.
fn box_mesh(name: &str, lo: Vec3, hi: Vec3) -> Vec<u8> {
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

/// The wall of the scene: `x` 40..60, `z` 20..80, 30 high.
const WALL_LO: Vec3 = Vec3::new(40.0, 0.0, 20.0);
const WALL_HI: Vec3 = Vec3::new(60.0, 30.0, 80.0);

/// The geometry: a 100x100 floor at height 0, the wall across its
/// middle, a waypoint at each corner of the wall's surroundings, a
/// camera on the `-z` side looking down at the floor, one light.
fn geometry() -> Vec<u8> {
    let mut objects = vec![
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
        box_mesh("blk_wall", WALL_LO, WALL_HI),
    ];
    for (name, x, z) in [
        ("wpt_a", 30.0, 10.0),
        ("wpt_b", 70.0, 10.0),
        ("wpt_c", 30.0, 90.0),
        ("wpt_d", 70.0, 90.0),
    ] {
        objects.push(box_mesh(
            name,
            Vec3::new(x - 1.0, 40.0, z - 1.0),
            Vec3::new(x + 1.0, 42.0, z + 1.0),
        ));
    }
    objects.push(Builder::camera(
        "Camera01",
        Vec3::new(50.0, 120.0, -120.0),
        Vec3::new(50.0, 0.0, 50.0),
        0.0,
        38.0,
    ));
    objects.push(Builder::light(
        "Omni01",
        Vec3::new(50.0, 200.0, 50.0),
        [1.0, 1.0, 1.0],
        None,
        false,
    ));
    Builder::file(&objects, &[])
}

/// A model of one quad, 2 wide and 4 high, standing on the origin and
/// facing `-z` (toward the camera), with three animation sets of one
/// second: `idle` (still), `walk` (a bob of the root) and `wave` (a lift).
const MODEL: &str = r#"xof 0303txt 0032

AnimTicksPerSecond { 4800; }

Frame Root {
 FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; }
 Mesh body {
  4;
  -1.0;0.0;0.0;,
  1.0;0.0;0.0;,
  1.0;4.0;0.0;,
  -1.0;4.0;0.0;;
  2;
  3;0,2,1;,
  3;0,3,2;;
  MeshNormals { 1; 0.0;0.0;-1.0;; 2; 3;0,0,0;, 3;0,0,0;; }
 }
}

AnimationSet idle {
 Animation {
  AnimationKey { 2; 2; 0;3;0.0,0.0,0.0;;, 4800;3;0.0,0.0,0.0;;; }
  { Root }
 }
}

AnimationSet walk {
 Animation {
  AnimationKey { 2; 3; 0;3;0.0,0.0,0.0;;, 2400;3;0.0,0.5,0.0;;, 4800;3;0.0,0.0,0.0;;; }
  { Root }
 }
}

AnimationSet wave {
 Animation {
  AnimationKey { 2; 2; 0;3;0.0,0.0,0.0;;, 4800;3;0.0,1.0,0.0;;; }
  { Root }
 }
}
"#;

/// A hat for the actor: one quad, 1 wide and 1 high, standing on its
/// origin, with a one-second `spin` animation that lifts it.
const HAT: &str = r#"xof 0303txt 0032

AnimTicksPerSecond { 4800; }

Frame HatRoot {
 FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; }
 Mesh hat {
  4;
  -0.5;0.0;0.0;,
  0.5;0.0;0.0;,
  0.5;1.0;0.0;,
  -0.5;1.0;0.0;;
  2;
  3;0,2,1;,
  3;0,3,2;;
  MeshNormals { 1; 0.0;0.0;-1.0;; 2; 3;0,0,0;, 3;0,0,0;; }
 }
}

AnimationSet spin {
 Animation {
  AnimationKey { 2; 2; 0;3;0.0,0.0,0.0;;, 4800;3;0.0,0.5,0.0;;; }
  { HatRoot }
 }
}
"#;

const ACTOR: &str = r#"ACTOR3DX
{
  NAME = "bob"
  CAPTION = "Bob"
  X = 20.0
  Y = 0.0
  Z = 20.0
  ANGLE = 0.0
  SCALE = 100
  VELOCITY = 40.0
  ANGULAR_VELOCITY = 720.0
  MODEL = "actors\bob.x"
  SCRIPT = "actors\bob.script"
  ANIMATION { NAME = "idle"  LOOPING = TRUE }
  ANIMATION
  {
    NAME = "walk"
    LOOPING = TRUE
    EVENT { FRAME = 1  NAME = "footstep" }
  }
}
"#;

fn game() -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 800,600 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Test3D\" SCRIPT=\"game.script\" } AD_GAME { }".to_vec(),
    );
    pkg.add_file("game.script", game_script());
    pkg.add_file("scenes\\room.script", room_script());
    pkg.add_file("actors\\bob.script", bob_script());
    pkg.add_file(
        "scenes\\room.scene",
        b"SCENE {
  NAME=\"Room\"
  GEOMETRY=\"scenes\\geo.3ds\"
  CAMERA=\"Camera01\"
  SCRIPT=\"scenes\\room.script\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=800 HEIGHT=600
    REGION { NAME=\"floor\" POINT {0,0} POINT {800,0} POINT {800,600} POINT {0,600} }
  }
}"
        .to_vec(),
    );
    pkg.add_file("scenes\\geo.3ds", geometry());
    pkg.add_file("actors\\bob.x", MODEL.as_bytes().to_vec());
    pkg.add_file("actors\\bob.act3d", ACTOR.as_bytes().to_vec());
    pkg.add_file("actors\\hat.x", HAT.as_bytes().to_vec());
    // Only looked up, never decoded: a texture and a video for the
    // material overrides and the shadow image.
    pkg.add_file("actors\\tex.png", b"not an image".to_vec());
    pkg.add_file("actors\\clip.ogv", b"not a video".to_vec());
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

fn result(runtime: &Runtime, key: &str) -> Value {
    runtime
        .engine
        .global("result")
        .and_then(|r| r.get_property(key, &()))
        .unwrap_or(Value::Null)
}

fn set_result(runtime: &mut Runtime, key: &str, value: Value) {
    let mut object = runtime.engine.global("result").cloned().unwrap();
    object.set_property(key, value, &mut runtime.world);
    runtime.engine.set_global("result", object);
}

fn raise(runtime: &mut Runtime, event: &str) {
    let scene = runtime.world.scene_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, scene, event, false);
}

fn inside_wall(p: Vec3) -> bool {
    p.x > WALL_LO.x && p.x < WALL_HI.x && p.z > WALL_LO.z && p.z < WALL_HI.z
}

#[test]
fn a_3d_actor_walks_turns_animates_and_survives_a_save() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    set_result(&mut runtime, "steps", Value::Int(0));
    // Tick 2: the scene loads with its geometry, the scene script runs.
    let log = runtime.tick(16);
    assert!(
        log.iter()
            .all(|l| !l.starts_with("Runtime error") && !l.contains("Error")),
        "{log:?}"
    );
    assert_eq!(result(&runtime, "type"), Value::from("actor3dx"));
    assert_eq!(result(&runtime, "type2"), Value::from("actor3dx"));
    assert_eq!(result(&runtime, "caption"), Value::from("Bob"));
    assert_eq!(result(&runtime, "geo"), Value::from("scenes\\geo.3ds"));
    assert_eq!(result(&runtime, "lights"), Value::Int(1));
    assert_eq!(result(&runtime, "lightOn"), Value::Bool(true));
    assert_eq!(result(&runtime, "lightName"), Value::from("Omni01"));
    assert_eq!(result(&runtime, "wallOff"), Value::Bool(false));
    assert_eq!(result(&runtime, "wallOn"), Value::Bool(true));
    assert_eq!(result(&runtime, "gameShadow"), Value::Int(3));
    assert_eq!(result(&runtime, "realTime"), Value::Bool(true));
    assert_eq!(result(&runtime, "sceneShadow"), Value::Int(1));

    let bob = result(&runtime, "bob").as_native().expect("the actor");
    assert_eq!(
        runtime.world.objects().get(bob).unwrap().kind,
        Kind::Actor3D
    );
    assert_eq!(runtime.world.actor3d_ids(), vec![bob]);
    let scene3d = runtime.world.scene3d().expect("geometry");
    assert_eq!(scene3d.geometry.walkplanes.len(), 1);
    assert_eq!(scene3d.geometry.blocks.len(), 1);
    assert_eq!(scene3d.geometry.waypoints().count(), 4);
    assert_eq!(scene3d.geometry.active_camera().unwrap().name, "Camera01");

    // The actor stands where the file put it, on the floor, and its 2D
    // position is the projection of its origin.
    let projection = runtime.world.projection_3d().expect("a camera");
    let actor = runtime.world.actor3d(bob).unwrap();
    assert!(actor.sound_panning, "`m_AutoSoundPanning` starts true");
    assert_eq!(actor.position, Vec3::new(20.0, 0.0, 20.0));
    assert_eq!((actor.x, actor.y), projection.project(actor.position));
    assert!(
        (0..800).contains(&actor.x) && (0..600).contains(&actor.y),
        "on the layer: {} {}",
        actor.x,
        actor.y
    );
    let (left, top, right, bottom) = actor.bounding_rect().expect("projected box");
    assert!(
        left < actor.x && actor.x < right,
        "{left} {} {right}",
        actor.x
    );
    assert!(
        top < actor.y && actor.y <= bottom,
        "{top} {} {bottom}",
        actor.y
    );
    assert!(actor.height() > 0);
    // The round trip of the 2D position lands back on the floor nearby.
    let back = scene3d
        .geometry
        .convert_2d_to_3d(&projection, actor.x, actor.y)
        .unwrap();
    assert!((back - actor.position).length() < 2.0, "{back:?}");
    assert_eq!(actor.state, Actor3dState::Ready);
    assert!(actor.ready);
    assert_eq!(actor.model().unwrap().current_animation(0), Some("idle"));

    // The drawing step sees one actor, lit by the light, with the scene's
    // lower shadow type.
    let (frame, draws) = runtime.world.three_d_draws().expect("a 3D frame");
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].id, bob);
    assert_eq!(draws[0].other_index, 0);
    assert_eq!(draws[0].lights.len(), 1);
    assert_eq!(draws[0].shadow_type, luma_3d::ShadowType::Simple);
    assert_eq!(
        draws[0].world.translation_part(),
        Vec3::new(20.0, 0.0, 20.0)
    );
    assert!(frame.fog.is_none());
    assert_eq!(frame.projection.project(actor.position), (actor.x, actor.y));

    // Hit testing: the centre of the projected rectangle hits the model,
    // a point outside the rectangle does not.
    let (hx, hy, hw, hh) = runtime.world.hit_rect(bob).expect("a hit area");
    assert_eq!((hx, hy, hw, hh), (left, top, right - left, bottom - top));
    assert_eq!(runtime.world.object_at(hx + hw / 2, hy + hh / 2), Some(bob));
    assert_eq!(runtime.world.object_at(hx - 5, hy + hh / 2), None);
    let actor = runtime.world.actor3d(bob).unwrap();
    assert!(!actor.is_transparent_at(&projection, hx + hw / 2, hy + hh / 2));
    assert!(actor.is_transparent_at(&projection, hx - 5, hy + hh / 2));

    // Walk to the far corner behind the wall: the finder routes around
    // it, the actor turns first, then arrives.
    let target = Vec3::new(70.0, 0.0, 90.0);
    let (tx, ty) = projection.project(target);
    set_result(&mut runtime, "tx", Value::Int(tx));
    set_result(&mut runtime, "ty", Value::Int(ty));
    raise(&mut runtime, "walk");
    runtime.tick(16);
    let actor = runtime.world.actor3d(bob).unwrap();
    assert!(
        matches!(
            actor.state,
            Actor3dState::SearchingPath | Actor3dState::WaitingPath
        ),
        "{:?}",
        actor.state
    );
    assert!(!actor.ready);
    assert_eq!(result(&runtime, "arrived"), Value::Null, "the script waits");
    let mut saw_turn = false;
    let mut saw_walk = false;
    let mut saw_walk_anim = false;
    let mut max_points = 0;
    let mut ticks = 0;
    while !runtime.world.actor3d(bob).unwrap().ready && ticks < 1000 {
        runtime.tick(16);
        ticks += 1;
        let actor = runtime.world.actor3d(bob).unwrap();
        assert!(
            !inside_wall(actor.position),
            "through the wall: {:?}",
            actor.position
        );
        assert_eq!(actor.position.y, 0.0, "kept on the floor");
        match actor.state {
            Actor3dState::Turning => saw_turn = true,
            Actor3dState::FollowingPath => {
                saw_walk = true;
                max_points = max_points.max(actor.path.points().len());
                if actor.model().unwrap().current_animation(0) == Some("walk") {
                    saw_walk_anim = true;
                }
            }
            _ => {}
        }
    }
    let actor = runtime.world.actor3d(bob).unwrap();
    assert!(saw_turn && saw_walk, "turned then walked");
    assert!(saw_walk_anim, "the walk animation played");
    assert!(
        max_points >= 2,
        "around the wall through a waypoint: {max_points}"
    );
    assert_eq!(actor.state, Actor3dState::Ready);
    assert!(
        (actor.position - target).length() < 1.0,
        "{:?}",
        actor.position
    );
    // The waiting script resumes on the next tick.
    runtime.tick(16);
    assert_eq!(result(&runtime, "arrived"), Value::Bool(true));
    assert!(
        matches!(result(&runtime, "steps"), Value::Int(n) if n >= 1),
        "the walk's footstep events reached the actor's script: {:?}",
        result(&runtime, "steps")
    );
    // Auto-scroll followed the main object.
    assert_eq!(runtime.world.main_object(), Some(bob));

    // Turn to the left direction (90 degrees); the script waits for it.
    raise(&mut runtime, "turn");
    runtime.tick(16);
    assert_eq!(
        runtime.world.actor3d(bob).unwrap().state,
        Actor3dState::Turning
    );
    for _ in 0..20 {
        runtime.tick(16);
    }
    let actor = runtime.world.actor3d(bob).unwrap();
    assert_eq!(actor.angle, 90.0);
    assert_eq!(actor.direction(), 6);
    assert_eq!(result(&runtime, "turnedAngle"), Value::Float(90.0));
    assert_eq!(result(&runtime, "turnedDir"), Value::Int(6));

    // Play an animation: the state follows it until it ends.
    raise(&mut runtime, "wave");
    runtime.tick(16);
    assert_eq!(result(&runtime, "waveStarted"), Value::Bool(true));
    assert_eq!(result(&runtime, "playing"), Value::Bool(true));
    assert_eq!(result(&runtime, "playingIdle"), Value::Bool(false));
    assert_eq!(result(&runtime, "readyWhileWaving"), Value::Bool(false));
    assert_eq!(
        runtime.world.actor3d(bob).unwrap().state,
        Actor3dState::PlayingAnim
    );
    for _ in 0..80 {
        runtime.tick(16);
    }
    let actor = runtime.world.actor3d(bob).unwrap();
    assert_eq!(actor.state, Actor3dState::Ready);
    assert!(!actor.model().unwrap().is_playing(0, Some("wave")));
    assert_eq!(actor.model().unwrap().current_animation(0), Some("idle"));

    // Walk back, save mid-walk, walk on, then load: the walk resumes from
    // the saved position and ends at the same target.
    let target = Vec3::new(20.0, 0.0, 20.0);
    let (tx, ty) = projection.project(target);
    set_result(&mut runtime, "tx", Value::Int(tx));
    set_result(&mut runtime, "ty", Value::Int(ty));
    set_result(&mut runtime, "arrived", Value::Null);
    raise(&mut runtime, "walk");
    let mut ticks = 0;
    while runtime.world.actor3d(bob).unwrap().state != Actor3dState::FollowingPath && ticks < 100 {
        runtime.tick(16);
        ticks += 1;
    }
    for _ in 0..10 {
        runtime.tick(16);
    }
    let saved_position = runtime.world.actor3d(bob).unwrap().position;
    let saved_angle = runtime.world.actor3d(bob).unwrap().angle;
    assert_eq!(
        runtime.world.actor3d(bob).unwrap().state,
        Actor3dState::FollowingPath
    );
    raise(&mut runtime, "SaveIt");
    runtime.tick(16);
    assert_eq!(result(&runtime, "saved"), Value::Bool(true));
    for _ in 0..30 {
        runtime.tick(16);
    }
    assert!(
        (runtime.world.actor3d(bob).unwrap().position - saved_position).length() > 5.0,
        "walked on after the save"
    );
    raise(&mut runtime, "LoadIt");
    runtime.tick(16);
    let log = runtime.tick(16);
    assert!(
        log.iter().all(|l| !l.contains("Error")),
        "the geometry and the model load again: {log:?}"
    );
    let actor = runtime.world.actor3d(bob).expect("the actor came back");
    assert!(actor.model().is_some(), "the model was read again");
    assert!(
        (actor.position - saved_position).length() < 2.0,
        "{:?}",
        actor.position
    );
    assert_eq!(actor.angle, saved_angle);
    assert_eq!(actor.state, Actor3dState::FollowingPath);
    assert_eq!(actor.model().unwrap().current_animation(0), Some("walk"));
    assert!(runtime.world.scene3d().is_some(), "the geometry came back");
    assert_eq!(runtime.world.scene3d().unwrap().geometry.blocks.len(), 1);
    assert_eq!(runtime.world.main_object(), Some(bob));
    let mut ticks = 0;
    while !runtime.world.actor3d(bob).unwrap().ready && ticks < 1000 {
        runtime.tick(16);
        ticks += 1;
        assert!(!inside_wall(runtime.world.actor3d(bob).unwrap().position));
    }
    let actor = runtime.world.actor3d(bob).unwrap();
    assert!(
        (actor.position - target).length() < 1.0,
        "{:?}",
        actor.position
    );
    runtime.tick(16);
    assert_eq!(
        result(&runtime, "arrived"),
        Value::Bool(true),
        "the saved script resumed"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Starts the game and runs it until the scene script has loaded the
/// actor; the actor's handle.
fn start(runtime: &mut Runtime) -> luma_script::NativeId {
    runtime.start();
    runtime.tick(16);
    set_result(runtime, "steps", Value::Int(0));
    runtime.tick(16);
    result(runtime, "bob").as_native().expect("the actor")
}

#[test]
fn attachments_textures_and_bone_emitters() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    let bob = start(&mut runtime);
    let projection = runtime.world.projection_3d().expect("a camera");

    raise(&mut runtime, "attach");
    let log = runtime.tick(16);
    // The bad bone and the missing file raise the original's errors.
    assert!(
        log.iter()
            .any(|l| l.contains("Bone 'Nope' cannot be found")),
        "{log:?}"
    );
    assert!(
        log.iter().any(|l| l.contains("Error adding attachment")),
        "{log:?}"
    );
    assert_eq!(result(&runtime, "added"), Value::Bool(true));
    assert_eq!(result(&runtime, "badBone"), Value::Bool(false));
    assert_eq!(result(&runtime, "badFile"), Value::Bool(false));
    let att = result(&runtime, "att").as_native().expect("the attachment");
    assert_eq!(result(&runtime, "att0"), Value::Native(att), "by index too");
    assert_eq!(
        runtime.world.objects().get(att).unwrap().kind,
        Kind::Attachment3D(bob)
    );
    assert_eq!(result(&runtime, "attType"), Value::from("attachment"));
    assert_eq!(result(&runtime, "attName"), Value::from("hat"));
    assert_eq!(result(&runtime, "attBone"), Value::from("Root"));
    assert_eq!(result(&runtime, "numAtt"), Value::Int(1));
    assert_eq!(result(&runtime, "attPlay"), Value::Bool(true));
    assert_eq!(result(&runtime, "tex"), Value::Bool(true));
    assert_eq!(result(&runtime, "texBad"), Value::Bool(false));
    assert_eq!(result(&runtime, "theora"), Value::Bool(true));
    assert_eq!(result(&runtime, "sceneShadowSet"), Value::Bool(true));
    assert_eq!(
        result(&runtime, "sceneShadowImg"),
        Value::from("actors\\tex.png")
    );
    let scene = runtime.world.scene_id();
    let state = runtime.world.object_state(scene).unwrap();
    assert_eq!(state.shadow_image.as_deref(), Some("actors\\tex.png"));
    assert_eq!(state.shadow_light, Some([1.0, 2.0, 3.0]));

    // The attachment follows the bone: its matrix is its own offset, the
    // bone's combined matrix, the actor's world matrix; it animates on
    // its own channel.
    let actor = runtime.world.actor3d(bob).unwrap();
    assert_eq!(actor.attachments.len(), 1);
    let hat = actor.attachment("hat").expect("the hat");
    assert_eq!(hat.id, att);
    assert_eq!(hat.position, Vec3::new(0.0, 4.0, 0.0));
    let bone = *actor.model().unwrap().bone_matrix("Root").unwrap();
    let expected = world_matrix(Vec3::new(0.0, 4.0, 0.0), 0.0, 1.0) * bone * actor.world_matrix();
    assert_eq!(hat.world_matrix(), expected);
    assert_eq!(hat.model().unwrap().current_animation(0), Some("spin"));
    // The hat stands on the body's top: its rectangle is above the body's
    // and the actor is clickable through it.
    let (_, body_top, _, _) = actor.bounding_rect().unwrap();
    let (l, t, r, b) = hat.bounding_rect().expect("projected hat");
    let (cx, cy) = ((l + r) / 2, (t + b) / 2);
    assert!(
        cy < body_top,
        "hat {t}..{b} above the body's top {body_top}"
    );
    assert!(!actor.is_transparent_at(&projection, cx, cy));
    assert_eq!(actor.hit_rects().len(), 2);
    assert_eq!(runtime.world.object_at(cx, cy), Some(bob));

    // The drawing step gets the attachment and the material overrides.
    let (_, draws) = runtime.world.three_d_draws().expect("a 3D frame");
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].attachments.len(), 1);
    assert_eq!(draws[0].attachments[0].id, att);
    assert_eq!(draws[0].attachments[0].bone, "Root");
    assert_eq!(draws[0].attachments[0].world, expected);
    assert_eq!(
        draws[0].materials,
        &[
            MaterialOverride {
                material: "body".to_string(),
                texture: MaterialTexture::Sprite("actors\\tex.png".to_string()),
            },
            MaterialOverride {
                material: "screen".to_string(),
                texture: MaterialTexture::Theora("actors\\clip.ogv".to_string()),
            },
        ]
    );

    // The emitter follows the bone's projected position, offset by one
    // unit up the bone.
    let emitter = result(&runtime, "emitter")
        .as_native()
        .expect("the emitter");
    let actor = runtime.world.actor3d(bob).unwrap();
    assert_eq!(actor.part_bone.as_deref(), Some("Root"));
    let bone_point = actor
        .bone_position("Root", Vec3::new(0.0, 1.0, 0.0))
        .unwrap();
    let e = runtime.world.emitter(emitter).expect("the emitter lives");
    assert_eq!((e.x, e.y), projection.project(bone_point));
    assert_ne!((e.x, e.y), (actor.x, actor.y), "not the actor's own point");

    // Save, play on, load: the attachment, its offset and animation, the
    // overrides and the bone emitter come back.
    raise(&mut runtime, "SaveIt");
    runtime.tick(16);
    assert_eq!(result(&runtime, "saved"), Value::Bool(true));
    for _ in 0..5 {
        runtime.tick(16);
    }
    raise(&mut runtime, "LoadIt");
    runtime.tick(16);
    let log = runtime.tick(16);
    assert!(log.iter().all(|l| !l.contains("Error")), "{log:?}");
    let actor = runtime.world.actor3d(bob).expect("the actor came back");
    assert_eq!(actor.attachments.len(), 1);
    let hat = &actor.attachments[0];
    assert_eq!((hat.name.as_str(), hat.bone.as_str()), ("hat", "Root"));
    assert_eq!(hat.id, att);
    assert_eq!(hat.position, Vec3::new(0.0, 4.0, 0.0));
    assert!(hat.model().is_some(), "the hat's model was read again");
    assert!(hat.model().unwrap().is_playing(0, Some("spin")));
    let bone = *actor.model().unwrap().bone_matrix("Root").unwrap();
    assert_eq!(
        hat.world_matrix(),
        world_matrix(Vec3::new(0.0, 4.0, 0.0), 0.0, 1.0) * bone * actor.world_matrix()
    );
    assert_eq!(actor.materials.len(), 2);
    assert_eq!(actor.part_bone.as_deref(), Some("Root"));
    assert_eq!(
        runtime.world.objects().get(att).unwrap().kind,
        Kind::Attachment3D(bob)
    );
    let e = runtime
        .world
        .emitter(emitter)
        .expect("the emitter came back");
    let bone_point = actor
        .bone_position("Root", Vec3::new(0.0, 1.0, 0.0))
        .unwrap();
    assert_eq!((e.x, e.y), projection.project(bone_point));

    // Removal drops the attachment and its object.
    raise(&mut runtime, "detach");
    runtime.tick(16);
    assert_eq!(result(&runtime, "removed"), Value::Bool(true));
    assert_eq!(result(&runtime, "removedAgain"), Value::Bool(false));
    assert_eq!(result(&runtime, "gone"), Value::Null);
    assert_eq!(result(&runtime, "attValid"), Value::Bool(false));
    assert!(runtime.world.actor3d(bob).unwrap().attachments.is_empty());
    assert!(runtime.world.objects().get(att).is_none());
    let (_, draws) = runtime.world.three_d_draws().unwrap();
    assert!(draws[0].attachments.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
