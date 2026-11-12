//! Saved games: what a save keeps and a load brings back, one test per
//! kind of state, on a small game built in code.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, EntityRef, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_formats::Image;
use luma_script::asm::Assembler;
use luma_script::{Instruction, Natives, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-saved-games-test-{}-{}",
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

/// A script that does nothing: the game needs one to be called through.
fn idle_script(name: &str) -> Vec<u8> {
    let mut a = Assembler::new(name);
    a.emit(Instruction::Ret);
    a.build()
}

/// The files of the test game, before the per-test additions.
fn package(game_keys: &str) -> PackageBuilder {
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 32,32 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        format!("GAME {{ NAME=\"Saves\" SCRIPT=\"game.script\" {game_keys} }}").into_bytes(),
    );
    pkg.add_file("game.script", idle_script("game.script"));
    pkg.add_file("art\\red.png", png(4, 4, [255, 0, 0, 255]));
    pkg.add_file("art\\green.png", png(4, 4, [0, 255, 0, 255]));
    pkg.add_file(
        "art\\prop.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=100 IMAGE=\"art\\red.png\" } FRAME { DELAY=100 IMAGE=\"art\\green.png\" } }".to_vec(),
    );
    pkg.add_file(
        "scenes\\hall.scene",
        b"SCENE {
  NAME=\"Hall\" PERSISTENT_STATE=TRUE
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=32 HEIGHT=32
    ENTITY { NAME=\"door\" X=4 Y=4 SPRITE=\"art\\prop.sprite\" }
    REGION { NAME=\"exit\" POINT {0,0} POINT {8,0} POINT {8,8} POINT {0,8} }
    REGION { NAME=\"rug\" POINT {8,8} POINT {16,8} POINT {16,16} POINT {8,16} }
  }
  WAYPOINTS { NAME=\"wp\" POINT {16,16} POINT {24,24} }
  ENTITY { NAME=\"lamp\" X=20 Y=20 SPRITE=\"art\\prop.sprite\" }
}"
        .to_vec(),
    );
    pkg.add_file(
        "scenes\\room.scene",
        b"SCENE {
  NAME=\"Room\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=32 HEIGHT=32
    ENTITY { NAME=\"prop\" X=8 Y=12 SPRITE=\"art\\prop.sprite\" }
  }
}"
        .to_vec(),
    );
    pkg
}

/// Writes the package and opens the game, started, with silent audio.
fn open(pkg: PackageBuilder) -> (Runtime, std::path::PathBuf) {
    let dir = temp_dir();
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    (runtime, dir)
}

fn no_errors(log: &[String]) {
    assert!(
        log.iter()
            .all(|l| !l.contains("Error") && !l.contains("error")),
        "{log:?}"
    );
}

/// Saves to a slot, changes what the test names, and loads the slot.
fn save_and_load(runtime: &mut Runtime, slot: i32, between: impl FnOnce(&mut Runtime)) {
    runtime.save_game(slot, "test", true).unwrap();
    between(runtime);
    runtime.load_game(slot).unwrap();
    no_errors(&runtime.world.take_log());
}

/// `CAdScene::PersistState`, `CAdSceneState`, `CAdNodeState`: what a
/// persistent scene remembers between visits (every caption slot, the
/// regions unless `SaveState` is off, the free entities, the waypoint
/// groups' `Active`), and that the remembered states travel in a saved
/// game.
#[test]
fn a_scene_remembers_captions_regions_and_waypoint_groups_across_a_save() {
    let (mut runtime, dir) = open(package(""));
    runtime.change_scene("scenes\\hall.scene").unwrap();
    runtime.tick(16);
    let door = runtime
        .world
        .node_id(EntityRef::Node { layer: 0, index: 0 })
        .unwrap();
    let lamp = runtime.world.find_node("lamp").unwrap();
    let exit = runtime.world.region_id(0, 1).unwrap();
    let rug = runtime.world.region_id(0, 2).unwrap();
    let group = runtime.world.waypoint_group_object(0).unwrap();
    runtime.world.set_caption(door, "Door", 1);
    runtime.world.set_caption(door, "the door", 2);
    runtime.world.set_caption(door, "of the door", 7);
    runtime.world.set_caption(lamp, "to the lamp", 4);
    for id in [exit, rug, group] {
        assert!(
            runtime
                .world
                .set_property(id, "Active", &Value::Bool(false))
        );
    }
    // A region whose `SaveState` is off is not remembered.
    assert_eq!(
        runtime.world.get_property(rug, "SaveState"),
        Some(Value::Bool(true))
    );
    assert!(
        runtime
            .world
            .set_property(rug, "SaveState", &Value::Bool(false))
    );
    assert_eq!(
        runtime.world.get_property(rug, "SaveState"),
        Some(Value::Bool(false))
    );

    // Leaving notes the state.
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    let hall = runtime.world.scene_states()["scenes\\hall.scene"].clone();
    assert_eq!(hall.nodes["door"].caption.as_deref(), Some("Door"));
    assert_eq!(hall.nodes["door"].captions[&2], "the door");
    assert_eq!(hall.nodes["door"].captions[&7], "of the door");
    assert_eq!(hall.nodes["lamp"].captions[&4], "to the lamp");
    assert!(!hall.nodes["exit"].active);
    assert!(!hall.nodes["wp"].active);
    assert!(!hall.nodes.contains_key("rug"), "{hall:?}");

    // The remembered states are part of a saved game.
    save_and_load(&mut runtime, 1, |runtime| {
        runtime.world.reset_content(&mut runtime.engine);
        assert!(runtime.world.scene_states().is_empty());
    });
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    assert_eq!(runtime.world.scene_states()["scenes\\hall.scene"], hall);

    // Coming back after the load: everything is as it was left.
    runtime.change_scene("scenes\\hall.scene").unwrap();
    runtime.tick(16);
    let door = runtime
        .world
        .node_id(EntityRef::Node { layer: 0, index: 0 })
        .unwrap();
    let lamp = runtime.world.find_node("lamp").unwrap();
    assert_eq!(runtime.world.caption_of(door, 1), "Door");
    assert_eq!(runtime.world.caption_of(door, 2), "the door");
    assert_eq!(runtime.world.caption_of(door, 3), "");
    assert_eq!(runtime.world.caption_of(door, 7), "of the door");
    assert_eq!(runtime.world.caption_of(lamp, 4), "to the lamp");
    let view = runtime.world.scene().unwrap();
    assert!(!view.region(0, 1).unwrap().active);
    assert!(view.region(0, 2).unwrap().active, "not remembered");
    assert!(!view.scene.waypoint_groups[0].active);
    std::fs::remove_dir_all(&dir).unwrap();
}
