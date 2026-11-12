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
    // An event handler keeps the script attached.
    a.event("Nothing");
    a.emit(Instruction::RetEvent);
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
    ENTITY { NAME=\"bell\" X=20 Y=12 SPRITE=\"art\\cue.sprite\" }
  }
}"
        .to_vec(),
    );
    pkg.add_file(
        "art\\cue.sprite",
        b"SPRITE { LOOPING=FALSE FRAME { DELAY=2000 IMAGE=\"art\\red.png\" SOUND=\"audio\\beep.wav\" } FRAME { DELAY=100 IMAGE=\"art\\green.png\" } }".to_vec(),
    );
    pkg.add_file("audio\\beep.wav", luma_audio::silent_wav(8000, 1, 300));
    pkg.add_file("intro.ogv", luma_video::synthetic_clip(32, 32, 10, 10));
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

const PROP: EntityRef = EntityRef::Node { layer: 0, index: 0 };
const BELL: EntityRef = EntityRef::Node { layer: 0, index: 1 };

/// `CBSound::Persist` for the sound of a sprite frame
/// (`CBFrame::m_Sound`): one still playing at the save plays on from
/// its position after the load.
#[test]
fn a_frame_sound_plays_on_after_a_load() {
    let (mut runtime, dir) = open(package(""));
    runtime.change_scene("scenes\\room.scene").unwrap();
    for _ in 0..6 {
        runtime.tick(16);
    }
    let bell = runtime.world.node_id(BELL).unwrap();
    assert_eq!(runtime.world.frame_sounds_of(bell), 1);
    save_and_load(&mut runtime, 2, |runtime| {
        // The sound ends (300 ms) while the frame stays (2 s).
        for _ in 0..25 {
            runtime.tick(16);
        }
        assert_eq!(runtime.world.frame_sounds_of(bell), 0);
    });
    assert_eq!(runtime.world.frame_sounds_of(bell), 1, "back, playing");
    // About 100 ms were played before the save: 200 ms are left.
    for _ in 0..8 {
        runtime.tick(16);
    }
    assert_eq!(runtime.world.frame_sounds_of(bell), 1, "not from the start");
    for _ in 0..8 {
        runtime.tick(16);
    }
    assert_eq!(runtime.world.frame_sounds_of(bell), 0, "from where it was");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `CVidTheoraPlayer::Persist` / `InitializeSimple`, `CAdEntity::Persist`
/// (`m_Theora`): the video an entity plays comes back after a load, from
/// its position, looping, with its alpha file, zoom and volume; one
/// saved paused shows a frame and stays paused.
#[test]
fn an_entity_video_comes_back_after_a_load() {
    let (mut runtime, dir) = open(package(""));
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    let prop = runtime.world.node_id(PROP).unwrap();
    assert!(
        runtime
            .world
            .play_entity_theora(prop, "intro.ogv", true, Some("art\\red.png"), 0)
    );
    for _ in 0..20 {
        runtime.tick(16);
    }
    let before = runtime.world.entity_video(prop).unwrap().frame_index();
    assert_eq!(before, Some(3), "about 340 ms at 10 fps");
    save_and_load(&mut runtime, 3, |runtime| {
        assert!(runtime.world.stop_entity_video(prop));
        runtime.tick(16);
    });
    let video = runtime.world.entity_video(prop).expect("the video is back");
    assert!(video.is_running() && video.looping);
    assert_eq!(video.filename, "intro.ogv");
    assert_eq!(video.alpha_file.as_deref(), Some("art\\red.png"));
    assert_eq!((video.zoom, video.volume), (100.0, 100));
    // It starts at the key frame at or after the saved position.
    let frame = video.frame_index().expect("a frame at once");
    assert!(frame >= 3, "{frame}");
    runtime.tick(16);
    for _ in 0..12 {
        runtime.tick(16);
    }
    let later = runtime.world.entity_video(prop).unwrap().frame_index();
    assert!(
        later > Some(frame) || later < Some(3),
        "it goes on: {later:?}"
    );

    // Saved paused: one frame, then held.
    assert!(
        runtime
            .world
            .play_entity_theora(prop, "intro.ogv", false, None, 0)
    );
    for _ in 0..10 {
        runtime.tick(16);
    }
    assert!(runtime.world.pause_entity_video(prop));
    let held = runtime.world.entity_video(prop).unwrap().frame_index();
    save_and_load(&mut runtime, 4, |runtime| {
        assert!(runtime.world.stop_entity_video(prop));
    });
    for _ in 0..30 {
        runtime.tick(16);
    }
    let video = runtime.world.entity_video(prop).expect("still there");
    assert!(video.paused && !video.looping);
    assert!(video.frame_index() >= held, "{:?}", video.frame_index());
    let shown = video.frame_index();
    assert!(runtime.world.resume_entity_video(prop));
    for _ in 0..12 {
        runtime.tick(16);
    }
    let video = runtime.world.entity_video(prop);
    assert!(video.is_none_or(|v| v.frame_index() > shown), "resumed");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The game's video (`Game.PlayTheora`) comes back after a load, the
/// game still frozen by it, and unfreezes the game at its end; a video
/// whose file is gone leaves the game unfrozen.
#[test]
fn the_game_video_comes_back_after_a_load() {
    use luma_adventure::VideoPlayback;
    let (mut runtime, dir) = open(package(""));
    std::fs::write(
        dir.join("loose.ogv"),
        luma_video::synthetic_clip(32, 32, 10, 10),
    )
    .unwrap();
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    for file in ["intro.ogv", "loose.ogv"] {
        assert!(runtime.world.play_theora(
            &mut runtime.engine,
            file,
            VideoPlayback::Center,
            0,
            0,
            false,
            false,
            None
        ));
        for _ in 0..25 {
            runtime.tick(16);
        }
        assert!(runtime.world.is_video_playing() && runtime.world.is_frozen());
        let slot = if file == "intro.ogv" { 5 } else { 6 };
        runtime.save_game(slot, "video", true).unwrap();
        assert!(runtime.world.stop_video(&mut runtime.engine));
        assert!(!runtime.world.is_frozen());
    }

    runtime.load_game(5).unwrap();
    no_errors(&runtime.world.take_log());
    assert!(runtime.world.is_frozen());
    let video = runtime.world.video().expect("the video is back");
    assert!(video.is_playing() && video.freeze_game && video.dont_drop_frames);
    assert_eq!(video.playback, VideoPlayback::Center);
    assert!(video.frame_index() >= Some(4), "{:?}", video.frame_index());
    let mut ticks = 0;
    while runtime.world.is_video_playing() && ticks < 100 {
        runtime.tick(16);
        ticks += 1;
    }
    assert!(ticks < 50, "from its position, not from the start: {ticks}");
    assert!(!runtime.world.is_frozen(), "the end unfreezes the game");

    // The file is gone: no video, and the game it froze runs.
    std::fs::remove_file(dir.join("loose.ogv")).unwrap();
    runtime.load_game(6).unwrap();
    let log = runtime.world.take_log();
    assert!(
        log.iter().any(|l| l.contains("loose.ogv")),
        "logged: {log:?}"
    );
    assert!(!runtime.world.is_video_playing());
    assert!(!runtime.world.is_frozen());
    std::fs::remove_dir_all(&dir).unwrap();
}

fn frame_of(color: [u8; 4]) -> Image {
    Image {
        width: 32,
        height: 32,
        pixels: color.repeat(32 * 32),
        has_alpha_channel: true,
    }
}

/// The image a sprite of the windows' bank shows.
fn ui_image(runtime: &Runtime, path: &str) -> Option<String> {
    let loaded = runtime.world.ui().bank.sprite(path)?;
    loaded.sprite.frames[0].subframes[0].image.clone()
}

/// `CBPersistMgr::InitSave` (the thumbnail taken at save time when none
/// was stored), `CBSaveThumbHelper::StoreThumbnail`, `CBSaveThumbFile`
/// (`savegame:<slot>` read as an image file), `CBImage::Grayscale`
/// (`savegame:<slot>,g`), `CBSprite::LoadFile` and `CBSurfaceD3D::Create`
/// on such names, `CBGame::EmptySaveSlot`, `IsSaveSlotUsed`,
/// `GetSaveSlotDescription`.
#[test]
fn a_save_takes_its_thumbnail_and_slots_show_it_as_an_image() {
    use luma_adventure::capture::Capture;
    let (mut runtime, dir) = open(package("THUMBNAIL_WIDTH=8 THUMBNAIL_HEIGHT=4"));
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    let key = luma_formats::DEFAULT_COLOR_KEY;
    let slot = dir.join("saves").join("save001.dsv");

    // A free slot has no image: a sprite of it is the placeholder.
    assert!(!runtime.world.is_save_slot_used(1));
    assert!(!runtime.world.vfs().exists("savegame:1"));
    assert!(runtime.world.vfs().read("savegame:1").is_err());
    assert!(runtime.world.load_ui_image("savegame:1"));
    assert!(runtime.world.load_ui_image("SaveGame:1,g"));
    assert_eq!(
        ui_image(&runtime, "savegame:1").as_deref(),
        Some("invalid.bmp")
    );

    // No thumbnail was stored: the save asks for the picture of its own
    // frame, which goes into the file when the front-end renders it.
    let path = runtime.save_game(1, "first", false).unwrap();
    assert_eq!(path, slot);
    assert_eq!(
        runtime.world.captures(),
        [Capture::SaveThumbnail { path: slot.clone() }]
    );
    assert!(runtime.world.wants_frame());
    assert_eq!(
        luma_adventure::read_save_header(&slot).unwrap().thumbnail,
        None
    );
    runtime.world.deliver_frame(&frame_of([200, 10, 10, 255]));
    no_errors(&runtime.world.take_log());
    assert!(!runtime.world.wants_frame());
    let header = luma_adventure::read_save_header(&slot).unwrap();
    assert_eq!(header.description, "first");
    assert!(header.timestamp > 1_700_000_000);
    let thumb = luma_formats::image::decode(header.thumbnail.as_deref().unwrap(), key).unwrap();
    assert_eq!((thumb.width, thumb.height), (8, 4));
    assert_eq!(runtime.world.save_slot_description(1), "first");

    // The slot's thumbnail is an image file, in colour and in grey.
    assert!(runtime.world.vfs().exists("savegame:1"));
    let bytes = runtime.world.vfs().read("SAVEGAME:1").unwrap();
    let image = luma_formats::image::decode(&bytes, key).unwrap();
    assert_eq!((image.width, image.height), (8, 4));
    let p = image.pixel(3, 2).unwrap();
    assert_eq!((p.r, p.g, p.b, p.a), (200, 10, 10, 255));
    let bytes = runtime.world.vfs().read("savegame:001,G").unwrap();
    let grey = luma_formats::image::decode(&bytes, key).unwrap();
    let p = grey.pixel(3, 2).unwrap();
    assert!(p.r == p.g && p.g == p.b && p.a == 255, "{p:?}");
    assert!((66..=67).contains(&p.r), "0.30 R + 0.59 G + 0.11 B: {p:?}");
    // Without the comma the name is the colour picture.
    let bytes = runtime.world.vfs().read("savegame:1g").unwrap();
    let plain = luma_formats::image::decode(&bytes, key).unwrap();
    assert_eq!(plain.pixel(3, 2).unwrap().r, 200);
    // The sprites made of the slot before it was written show it now.
    assert_eq!(
        ui_image(&runtime, "savegame:1").as_deref(),
        Some("savegame:1")
    );
    assert_eq!(
        ui_image(&runtime, "savegame:1,g").as_deref(),
        Some("savegame:1,g")
    );

    // A stored thumbnail is used as it is, and no picture is asked for.
    assert!(runtime.world.store_save_thumbnail());
    runtime.world.deliver_frame(&frame_of([0, 0, 250, 255]));
    runtime.save_game(2, "second", true).unwrap();
    assert!(runtime.world.captures().is_empty());
    let bytes = runtime.world.vfs().read("savegame:2").unwrap();
    let image = luma_formats::image::decode(&bytes, key).unwrap();
    assert_eq!(image.pixel(0, 0).unwrap().b, 250);
    // A thumbnail asked for and not rendered yet becomes the save's.
    assert!(runtime.world.store_save_thumbnail());
    runtime.save_game(3, "third", true).unwrap();
    assert_eq!(
        runtime.world.captures(),
        [Capture::SaveThumbnail {
            path: dir.join("saves").join("save003.dsv")
        }]
    );
    // A load drops the picture due: it would be of another game.
    runtime.load_game(2).unwrap();
    assert!(runtime.world.captures().is_empty());
    // The file whose header was completed afterwards loads too. (The
    // windows' sprites are the loaded game's: ask for the image again.)
    runtime.load_game(1).unwrap();
    no_errors(&runtime.world.take_log());
    assert!(runtime.world.load_ui_image("savegame:1"));
    assert_eq!(
        ui_image(&runtime, "savegame:1").as_deref(),
        Some("savegame:1")
    );

    assert_eq!(runtime.world.save_slot_description(2), "second");

    // Emptying the slot removes its image; its sprites are the
    // placeholder again.
    assert!(runtime.world.empty_save_slot(1));
    assert!(!runtime.world.is_save_slot_used(1));
    assert_eq!(runtime.world.save_slot_description(1), "");
    assert!(!runtime.world.vfs().exists("savegame:1"));
    assert_eq!(
        ui_image(&runtime, "savegame:1").as_deref(),
        Some("invalid.bmp")
    );
    // A slot without digits is slot 0, as `atoi` reads it.
    runtime.save_game(0, "zero", true).unwrap();
    runtime.world.deliver_frame(&frame_of([9, 9, 9, 255]));
    assert!(runtime.world.vfs().exists("savegame:"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `CBGame::SaveGame`, `LoadGame`, `DisplayIndicator` and the
/// `INDICATOR_*` keys: the frame of a save shows the saving screen and
/// the bar (not for a quick save), the frame before a load the loading
/// screen; the bar is `INDICATOR_WIDTH * progress / 100` long.
#[test]
fn the_frame_of_a_save_shows_the_indicator() {
    use luma_adventure::save::SaveDisplay;
    let (mut runtime, dir) = open(package(""));
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    assert_eq!(runtime.world.save_display(), None);
    assert_eq!(runtime.world.indicator_bar(), None);

    // A save: the frame that follows is the save's, the bar full, with
    // the original's defaults (the bottom 8 lines, the screen's width,
    // translucent red).
    runtime.save_game(1, "bar", false).unwrap();
    assert_eq!(
        runtime.world.save_display(),
        Some(SaveDisplay { indicator: true })
    );
    let (bar, color) = runtime.world.indicator_bar().unwrap();
    assert_eq!((bar.x, bar.y, bar.w, bar.h), (0.0, 24.0, 32.0, 8.0));
    assert_eq!((color.r, color.g, color.b, color.a), (255, 0, 0, 128));
    // The next frame is the game's again.
    runtime.tick(16);
    assert_eq!(runtime.world.save_display(), None);
    assert_eq!(runtime.world.indicator_bar(), None);

    // A quick save shows nothing (the game sets no thumbnail size).
    runtime.save_game(2, "quick", true).unwrap();
    assert_eq!(runtime.world.save_display(), None);

    // Before a load nothing is done yet: no bar.
    runtime.world.schedule_load(1);
    assert_eq!(runtime.world.indicator_bar(), None);
    runtime.tick(16);
    no_errors(&runtime.world.take_log());
    std::fs::remove_dir_all(&dir).unwrap();

    // The game file's own bar: a place, a size, a colour; a height of
    // zero draws none.
    let (mut runtime, dir) = open(package(
        "INDICATOR_X=4 INDICATOR_Y=10 INDICATOR_WIDTH=20 INDICATOR_HEIGHT=2 INDICATOR_COLOR { 1, 2, 3, 4 }",
    ));
    runtime.save_game(1, "bar", false).unwrap();
    let (bar, color) = runtime.world.indicator_bar().unwrap();
    assert_eq!((bar.x, bar.y, bar.w, bar.h), (4.0, 10.0, 20.0, 2.0));
    assert_eq!((color.r, color.g, color.b, color.a), (1, 2, 3, 4));
    std::fs::remove_dir_all(&dir).unwrap();
    let (mut runtime, dir) = open(package("INDICATOR_HEIGHT=0"));
    runtime.save_game(1, "bar", false).unwrap();
    assert_eq!(runtime.world.indicator_bar(), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Calls a script method of an object, as a script of the game would.
fn call_native(
    runtime: &mut Runtime,
    target: luma_script::NativeId,
    name: &str,
    args: &[Value],
) -> Value {
    use luma_script::Host;
    let game = runtime.world.game_id();
    let script = runtime.engine.scripts_of(game)[0];
    let ctx = luma_script::Context {
        engine: &mut runtime.engine,
        script,
        owner: Some(game),
        line: 0,
    };
    runtime
        .world
        .call_method(target, name, args, ctx)
        .map(|r| r.value)
        .unwrap_or(Value::Null)
}

/// `CBGame::Persist` (`Cleanup()` before loading), `CBGame::LoadGame`
/// (`AccessUnpause()`), `CBFader::Persist` (a system fade ends at a
/// load, a fade on the game timer goes on).
#[test]
fn a_load_clears_the_messages_the_pause_and_ends_a_system_fade() {
    let (mut runtime, dir) = open(package(""));
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    let game = runtime.world.game_id();
    for (slot, method) in [(1, "SystemFadeOutAsync"), (2, "FadeOutAsync")] {
        runtime.world.fader_mut().deactivate();
        call_native(&mut runtime, game, method, &[Value::Int(10_000)]);
        runtime.tick(16);
        assert!(!runtime.world.fader().ready, "{method}");
        runtime.save_game(slot, method, true).unwrap();
    }
    runtime.world.quick_message("still here?");
    assert!(
        runtime
            .world
            .display_loading_icon("art\\prop.sprite", 0, 0, true)
    );
    runtime.world.accessibility_mut().paused = true;

    runtime.load_game(1).unwrap();
    assert!(runtime.world.quick_messages().is_empty());
    assert!(runtime.world.loading_icon().is_none());
    assert!(!runtime.world.accessibility().paused);
    // The system fade of ten seconds is over at the first frame.
    runtime.tick(16);
    let fader = runtime.world.fader();
    assert!(fader.ready && fader.active);
    assert_eq!(fader.color().map(|c| c.a), Some(255));

    // The same fade on the game timer goes on where it was.
    runtime.load_game(2).unwrap();
    runtime.tick(16);
    let fader = runtime.world.fader();
    assert!(!fader.ready);
    assert!(
        fader.color().is_none_or(|c| c.a < 10),
        "{:?}",
        fader.color()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `Game.SaveGame` from a frozen menu: the freeze is part of the saved
/// game, which loads frozen and unfreezes as the menu would.
#[test]
fn a_game_saved_while_frozen_loads_frozen() {
    let (mut runtime, dir) = open(package(""));
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    runtime.world.freeze(&mut runtime.engine, false);
    runtime.tick(16);
    assert!(runtime.world.is_frozen());
    save_and_load(&mut runtime, 1, |runtime| {
        runtime.world.unfreeze(&mut runtime.engine);
        runtime.tick(16);
        assert!(!runtime.world.is_frozen());
    });
    assert!(runtime.world.is_frozen());
    let timer = runtime.world.timer();
    runtime.tick(16);
    assert_eq!(runtime.world.timer(), timer, "the game timer is held");
    runtime.world.unfreeze(&mut runtime.engine);
    runtime.tick(16);
    assert!(!runtime.world.is_frozen());
    assert!(runtime.world.timer() > timer);
    std::fs::remove_dir_all(&dir).unwrap();
}
