//! A synthetic game whose scripts attach particle emitters to an entity
//! and an actor: the emitters emit on the game timer, raise their emit
//! event, follow their owner, are drawn after it, survive a saved game
//! and go with their scene.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_formats::{BlendMode, Image};
use luma_render::{DrawList, Gpu, Renderer};
use luma_script::asm::Assembler;
use luma_script::{Instruction, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-particles-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    Image {
        width,
        height,
        pixels: color.repeat((width * height) as usize),
        has_alpha_channel: true,
    }
    .to_png()
    .unwrap()
}

/// An argument of a call in the assembled scripts.
enum Arg {
    Int(i32),
    Float(f64),
    Bool(bool),
    Str(&'static str),
    Var(&'static str),
}

fn push_arg(a: &mut Assembler, arg: &Arg) {
    match arg {
        Arg::Int(i) => a.push_int(*i),
        Arg::Float(f) => a.push_float(*f),
        Arg::Bool(b) => a.push_bool(*b),
        Arg::Str(s) => a.push_string(s),
        Arg::Var(v) => a.push_var(v),
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

/// `object.Prop = <arg>`.
fn set_to(a: &mut Assembler, object: &str, prop: &str, arg: Arg) {
    push_arg(a, &arg);
    set(a, object, prop);
}

fn store(a: &mut Assembler, key: &str) {
    set(a, "result", key);
}

fn game_script() -> Vec<u8> {
    let mut a = Assembler::new("game.script");
    a.def_glob_var("result");
    a.emit(Instruction::CreateObject);
    a.pop_var("result");
    a.push_int(0);
    store(&mut a, "puffs");
    a.def_glob_var("em");
    a.def_glob_var("bobEm");
    a.def_glob_var("bob");
    call(
        &mut a,
        "Game",
        "LoadActor",
        &[Arg::Str("actors\\bob.actor")],
    );
    a.pop_var("bob");
    // bob.CreateParticleEmitter(true, 2, -4), following the actor.
    call(
        &mut a,
        "bob",
        "CreateParticleEmitter",
        &[Arg::Bool(true), Arg::Int(2), Arg::Int(-4)],
    );
    a.pop_var("bobEm");
    call(
        &mut a,
        "bobEm",
        "AddSprite",
        &[Arg::Str("art\\puff.sprite")],
    );
    a.emit(Instruction::PopEmpty);
    set_to(&mut a, "bobEm", "MaxParticles", Arg::Int(3));
    set_to(&mut a, "bobEm", "GenerationAmount", Arg::Int(3));
    set_to(&mut a, "bobEm", "LifeTime1", Arg::Int(0));
    set_to(&mut a, "bobEm", "LifeTime2", Arg::Int(0));
    call(&mut a, "bobEm", "Start", &[Arg::Int(0)]);
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "ChangeScene",
        &[Arg::Str("scenes\\room.scene")],
    );
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::Ret);

    // on "Setup": the scene's entity gets its emitter.
    a.event("Setup");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("em");
    a.def_var("fire");
    a.def_var("scene");
    get(&mut a, "Game", "Scene");
    a.pop_var("scene");
    call(&mut a, "scene", "GetNode", &[Arg::Str("fire")]);
    a.pop_var("fire");
    get(&mut a, "fire", "ParticleEmitter");
    store(&mut a, "before");
    call(
        &mut a,
        "fire",
        "CreateParticleEmitter",
        &[Arg::Bool(false), Arg::Int(0), Arg::Int(0)],
    );
    a.pop_var("em");
    get(&mut a, "fire", "ParticleEmitter");
    store(&mut a, "same");
    get(&mut a, "em", "Type");
    store(&mut a, "type");
    get(&mut a, "em", "Scale1");
    store(&mut a, "defaultScale");
    get(&mut a, "em", "MaxParticles");
    store(&mut a, "defaultMax");
    get(&mut a, "em", "EmitEvent");
    store(&mut a, "defaultEvent");
    set_to(&mut a, "em", "X", Arg::Int(10));
    set_to(&mut a, "em", "Y", Arg::Int(10));
    set_to(&mut a, "em", "Width", Arg::Int(4));
    set_to(&mut a, "em", "Height", Arg::Int(4));
    set_to(&mut a, "em", "Scale1", Arg::Float(50.0));
    set_to(&mut a, "em", "Scale2", Arg::Float(150.0));
    set_to(&mut a, "em", "Velocity1", Arg::Float(10.0));
    set_to(&mut a, "em", "Velocity2", Arg::Float(20.0));
    set_to(&mut a, "em", "Angle1", Arg::Int(0));
    set_to(&mut a, "em", "Angle2", Arg::Int(360));
    set_to(&mut a, "em", "LifeTime1", Arg::Int(2000));
    set_to(&mut a, "em", "LifeTime2", Arg::Int(2000));
    set_to(&mut a, "em", "MaxParticles", Arg::Int(5));
    set_to(&mut a, "em", "GenerationAmount", Arg::Int(2));
    set_to(&mut a, "em", "GenerationInterval", Arg::Int(0));
    set_to(&mut a, "em", "Alpha1", Arg::Int(300));
    set_to(&mut a, "em", "Rotation1", Arg::Int(90));
    set_to(&mut a, "em", "Rotation2", Arg::Int(90));
    set_to(&mut a, "em", "BlendMode", Arg::Int(1));
    get(&mut a, "em", "BlendMode");
    store(&mut a, "blend");
    set_to(&mut a, "em", "EmitEvent", Arg::Str("Puff"));
    call(&mut a, "em", "AddSprite", &[Arg::Str("art\\puff.sprite")]);
    store(&mut a, "addOk");
    call(&mut a, "em", "AddSprite", &[Arg::Str("art\\nope.sprite")]);
    store(&mut a, "addMissing");
    call(
        &mut a,
        "em",
        "AddGlobalForce",
        &[Arg::Str("wind"), Arg::Float(90.0), Arg::Float(50.0)],
    );
    store(&mut a, "force");
    call(
        &mut a,
        "em",
        "SetBorder",
        &[Arg::Int(0), Arg::Int(0), Arg::Int(200), Arg::Int(200)],
    );
    a.emit(Instruction::PopEmpty);
    call(&mut a, "em", "Start", &[Arg::Int(0)]);
    store(&mut a, "started");
    get(&mut a, "em", "Scale1");
    store(&mut a, "scale1");
    get(&mut a, "em", "Alpha1");
    store(&mut a, "alpha1");
    get(&mut a, "em", "EmitEvent");
    store(&mut a, "event");
    get(&mut a, "em", "NumLiveParticles");
    store(&mut a, "live0");
    a.emit(Instruction::RetEvent);

    // on "Count": result.live = em.NumLiveParticles
    a.event("Count");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("em");
    get(&mut a, "em", "NumLiveParticles");
    store(&mut a, "live");
    a.emit(Instruction::RetEvent);

    // on "Pause" / "Resume" / "StopIt"
    a.event("Pause");
    a.emit(Instruction::Scope);
    a.def_glob_var("em");
    call(&mut a, "em", "Pause", &[]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    a.event("Resume");
    a.emit(Instruction::Scope);
    a.def_glob_var("em");
    call(&mut a, "em", "Resume", &[]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    a.event("StopIt");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("em");
    call(&mut a, "em", "Stop", &[]);
    store(&mut a, "stopped");
    a.emit(Instruction::RetEvent);

    // on "Kill": the entity deletes its emitter.
    a.event("Kill");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("em");
    a.def_var("fire");
    a.def_var("scene");
    get(&mut a, "Game", "Scene");
    a.pop_var("scene");
    call(&mut a, "scene", "GetNode", &[Arg::Str("fire")]);
    a.pop_var("fire");
    call(&mut a, "fire", "DeleteParticleEmitter", &[]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "fire", "ParticleEmitter");
    store(&mut a, "afterDelete");
    call(&mut a, "Game", "ValidObject", &[Arg::Var("em")]);
    store(&mut a, "valid");
    a.emit(Instruction::RetEvent);
    a.build()
}

fn fire_script() -> Vec<u8> {
    let mut a = Assembler::new("fire.script");
    a.emit(Instruction::Ret);
    // on "Puff" { result.puffs = result.puffs + 1; }
    a.event("Puff");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.push_int(1);
    get(&mut a, "result", "puffs");
    a.emit(Instruction::Add);
    store(&mut a, "puffs");
    a.emit(Instruction::RetEvent);
    a.build()
}

fn game() -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 32,32 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Particles\" SCRIPT=\"game.script\" }".to_vec(),
    );
    pkg.add_file("game.script", game_script());
    pkg.add_file("fire.script", fire_script());
    pkg.add_file("art\\red.png", png(2, 2, [255, 0, 0, 255]));
    pkg.add_file("art\\green.png", png(2, 2, [0, 255, 0, 255]));
    pkg.add_file("art\\dot.png", png(2, 2, [255, 255, 255, 255]));
    pkg.add_file(
        "art\\puff.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=100 IMAGE=\"art\\dot.png\" HOTSPOT {1,1} } FRAME { DELAY=100 IMAGE=\"art\\dot.png\" HOTSPOT {1,1} } }".to_vec(),
    );
    pkg.add_file(
        "art\\stand.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=100 IMAGE=\"art\\green.png\" HOTSPOT {0,2} } }"
            .to_vec(),
    );
    pkg.add_file(
        "actors\\bob.actor",
        b"ACTOR {
  NAME=\"bob\" X=10 Y=24 SCALABLE=FALSE
  STAND { SPRITESET { LEFT=\"art\\stand.sprite\" } }
}"
        .to_vec(),
    );
    pkg.add_file(
        "scenes\\room.scene",
        b"SCENE {
  NAME=\"Room\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=32 HEIGHT=32
    ENTITY { NAME=\"fire\" X=16 Y=16 SPRITE=\"art\\red.png\" SCRIPT=\"fire.script\" }
  }
}"
        .to_vec(),
    );
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

fn event(runtime: &mut Runtime, name: &str) {
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, name, false);
    runtime.tick(16);
}

#[test]
fn emitters_emit_follow_their_owner_and_raise_their_event() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");

    // The actor's emitter follows it, scaled offsets included.
    let bob_em = runtime.engine.global("bobEm").unwrap().as_native().unwrap();
    let bob = runtime.engine.global("bob").unwrap().as_native().unwrap();
    assert_eq!(runtime.world.emitter_of(bob), Some(bob_em));
    let e = runtime.world.emitter(bob_em).unwrap();
    assert!(e.follow_parent);
    assert_eq!((e.x, e.y), (12, 20), "bob at (10, 24), offset (2, -4)");
    assert!(e.running);
    assert_eq!(e.num_live(), 3, "a batch of three on the first frame");
    for p in e.particles.iter() {
        assert_eq!((p.x as i32, p.y as i32), (12, 20), "a point emitter");
        assert_eq!(p.life_time, 0, "living for ever");
    }

    event(&mut runtime, "Setup");
    assert_eq!(result(&runtime, "before"), Value::Null);
    let em = runtime.engine.global("em").unwrap().as_native().unwrap();
    assert_eq!(result(&runtime, "same"), Value::Native(em));
    assert_eq!(result(&runtime, "type"), Value::from("particle-emitter"));
    assert_eq!(result(&runtime, "defaultScale"), Value::Float(100.0));
    assert_eq!(result(&runtime, "defaultMax"), Value::Int(100));
    assert_eq!(result(&runtime, "defaultEvent"), Value::Null);
    assert_eq!(result(&runtime, "addOk"), Value::Bool(true));
    assert_eq!(result(&runtime, "addMissing"), Value::Bool(false));
    assert_eq!(result(&runtime, "force"), Value::Bool(true));
    assert_eq!(result(&runtime, "started"), Value::Bool(true));
    assert_eq!(result(&runtime, "scale1"), Value::Float(50.0));
    assert_eq!(result(&runtime, "alpha1"), Value::Int(255), "clamped");
    assert_eq!(result(&runtime, "event"), Value::from("Puff"));
    assert_eq!(
        result(&runtime, "live0"),
        Value::Int(0),
        "nothing before a frame"
    );
    let fire = runtime.world.find_node("fire").unwrap();
    assert_eq!(runtime.world.emitter_of(fire), Some(em));
    {
        let e = runtime.world.emitter(em).unwrap();
        assert_eq!(e.sprites, vec!["art\\puff.sprite".to_string()]);
        assert_eq!(e.forces.len(), 1);
        assert!(
            (e.forces[0].dir_x - 50.0).abs() < 1e-3,
            "90 degrees is right"
        );
        assert_eq!(e.border.right, 200);
        assert_eq!(e.emit_event.as_deref(), Some("Puff"));
    }

    // The frame after Setup emitted two; the event reached the entity.
    event(&mut runtime, "Count");
    assert_eq!(result(&runtime, "live"), Value::Int(2));
    assert_eq!(result(&runtime, "puffs"), Value::Int(1));
    runtime.tick(16);
    runtime.tick(16);
    event(&mut runtime, "Count");
    assert_eq!(
        result(&runtime, "live"),
        Value::Int(5),
        "capped at MaxParticles"
    );
    let puffs = result(&runtime, "puffs");
    assert!(matches!(puffs, Value::Int(n) if n >= 3), "{puffs:?}");
    {
        let e = runtime.world.emitter(em).unwrap();
        for p in e.particles.iter() {
            assert!((50.0..=150.0).contains(&p.scale), "{}", p.scale);
            assert!(p.x >= 9.0 && p.x <= 16.0, "{}", p.x);
        }
    }

    // Pause holds the count, Resume goes on, Stop empties.
    let puffs_before = result(&runtime, "puffs");
    event(&mut runtime, "Pause");
    assert!(!runtime.world.emitter(em).unwrap().running);
    runtime.tick(16);
    assert_eq!(
        result(&runtime, "puffs"),
        puffs_before,
        "no batch while paused"
    );
    event(&mut runtime, "Resume");
    assert!(runtime.world.emitter(em).unwrap().running);
    event(&mut runtime, "StopIt");
    assert_eq!(result(&runtime, "stopped"), Value::Bool(true));
    {
        let e = runtime.world.emitter(em).unwrap();
        assert!(!e.running);
        assert!(e.particles.is_empty());
    }

    // Deleting the emitter invalidates its handle.
    event(&mut runtime, "Kill");
    assert_eq!(result(&runtime, "afterDelete"), Value::Null);
    assert_eq!(result(&runtime, "valid"), Value::Bool(false));
    assert!(runtime.world.emitter(em).is_none());
    assert_eq!(runtime.world.emitter_of(fire), None);
    assert!(
        runtime.world.emitter(bob_em).is_some(),
        "the actor keeps its own"
    );
}

#[test]
fn emitters_are_drawn_after_their_owner_survive_a_saved_game_and_go_with_the_scene() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    event(&mut runtime, "Setup");
    for _ in 0..4 {
        runtime.tick(16);
    }
    let em = runtime.engine.global("em").unwrap().as_native().unwrap();
    let bob_em = runtime.engine.global("bobEm").unwrap().as_native().unwrap();
    let fire = runtime.world.find_node("fire").unwrap();
    assert_eq!(runtime.world.emitter(em).unwrap().num_live(), 5);
    assert_eq!(runtime.world.emitter(bob_em).unwrap().num_live(), 3);

    // Drawing: the entity, its five particles, the actor, its three.
    if let Ok(gpu) = Gpu::headless() {
        let renderer = Renderer::new(gpu);
        runtime.world.upload(&renderer);
        let mut list = DrawList::new(32.0, 32.0);
        runtime.draw(&mut list, None);
        let quads = list.quads();
        let attributes = list.attributes();
        assert_eq!(quads.len(), 1 + 5 + 1 + 3, "{:?}", quads.len());
        assert_eq!((quads[0].dst.x, quads[0].dst.y), (16.0, 16.0), "the entity");
        assert_eq!(result(&runtime, "blend"), Value::Int(1));
        for (quad, attrs) in quads[1..6].iter().zip(&attributes[1..6]) {
            // A particle of the entity's emitter: 2x2 scaled by 50..150
            // percent around its hotspot, near the emission rectangle,
            // turned a quarter around its position (the hotspot, scaled
            // from the quad's corner) and added to what is under it.
            assert!(quad.dst.w >= 1.0 && quad.dst.w <= 3.0, "{:?}", quad.dst);
            assert!(quad.dst.x >= 5.0 && quad.dst.x <= 18.0, "{:?}", quad.dst);
            assert_eq!(quad.tint.a, 255);
            assert_eq!(attrs.rotation, 90.0);
            // The hotspot (1, 1) scaled by 50..150 percent from the
            // truncated corner; the quad's size is truncated too, so
            // only the range is known here.
            let (px, py) = (attrs.pivot.0 - quad.dst.x, attrs.pivot.1 - quad.dst.y);
            assert!(
                (0.5..=1.5).contains(&px) && (0.5..=1.5).contains(&py),
                "{:?}",
                attrs
            );
            assert_eq!(attrs.blend, BlendMode::Additive);
        }
        assert_eq!((quads[6].dst.x, quads[6].dst.y), (10.0, 22.0), "the actor");
        for (quad, attrs) in quads[7..].iter().zip(&attributes[7..]) {
            assert_eq!(
                (quad.dst.x, quad.dst.y),
                (11.0, 19.0),
                "point emitter at (12, 20)"
            );
            assert_eq!(attrs.rotation, 0.0, "no rotation range set");
            assert_eq!(attrs.blend, BlendMode::Normal);
        }
    } else {
        eprintln!("skipped drawing: no adapter");
    }

    // A saved game keeps the emitters and their live particles.
    runtime.save_game(3, "particles", false).unwrap();
    let saved: Vec<_> = runtime.world.emitter(em).unwrap().particles.clone();
    let saved_bob = runtime.world.emitter(bob_em).unwrap().particles.clone();
    for _ in 0..3 {
        runtime.tick(16);
    }
    assert_ne!(
        runtime.world.emitter(em).unwrap().particles,
        saved,
        "the particles moved on"
    );
    runtime.load_game(3).unwrap();
    runtime.tick(16);
    let e = runtime.world.emitter(em).unwrap();
    assert_eq!(e.owner, fire);
    assert_eq!(e.particles.len(), 5);
    assert_eq!(e.sprites, vec!["art\\puff.sprite".to_string()]);
    assert!(
        e.bank.sprite("art\\puff.sprite").is_some(),
        "the sprites were loaded again"
    );
    assert_eq!(e.forces.len(), 1);
    assert_eq!(e.max_particles, 5);
    assert_eq!(e.emit_event.as_deref(), Some("Puff"));
    // One frame after the load: the saved particles, moved by one frame.
    for (now, then) in e.particles.iter().zip(&saved) {
        assert_eq!(now.creation_time, then.creation_time);
        assert_eq!(now.life_time, then.life_time);
        assert!(
            (now.x - then.x - now.vx * 0.016).abs() < 0.01,
            "{} {}",
            now.x,
            then.x
        );
    }
    let b = runtime.world.emitter(bob_em).unwrap();
    assert_eq!(b.particles.len(), saved_bob.len());
    assert!(b.follow_parent);
    assert_eq!(runtime.world.emitter_of(fire), Some(em));

    // A scene change takes the scene entity's emitter, not the game
    // actor's.
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    assert!(runtime.world.emitter(em).is_none());
    assert!(runtime.world.emitter(bob_em).is_some());
    let fire = runtime.world.find_node("fire").unwrap();
    assert_eq!(runtime.world.emitter_of(fire), None);
}
