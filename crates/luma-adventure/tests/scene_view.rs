//! Loads a synthetic game from a package and draws its scene offscreen.
//! The GPU part is skipped when the machine has no graphics adapter.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Camera, GameSettings, SceneView, place};
use luma_archive::{PackageBuilder, Vfs};
use luma_formats::{Image, Rgba, SubFrame};
use luma_render::{DrawList, Gpu, Offscreen, Renderer};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-adventure-test-{}-{}",
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

/// A 64x32 scene in a 32x32 game: a blue background, a red prop with a
/// hotspot at its bottom, an inactive entity, a sound entity, a free
/// entity standing on the floor region, and a missing sprite.
fn game() -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { RESOLUTION { 32,32 } }".to_vec(),
    );
    pkg.add_stored_file("art\\bg.png", png(64, 32, [0, 0, 255, 255]));
    pkg.add_stored_file("art\\red.png", png(4, 4, [255, 0, 0, 255]));
    pkg.add_stored_file("art\\green.png", png(2, 2, [0, 255, 0, 255]));
    pkg.add_file(
        "art\\loop.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=100 IMAGE=\"art\\red.png\" MOVE {4, 0} } FRAME { DELAY=100 IMAGE=\"art\\green.png\" MOVE {4, 0} } }".to_vec(),
    );
    pkg.add_file(
        "art\\prop.sprite",
        b"SPRITE { FRAME { DELAY=100 IMAGE=\"art\\red.png\" HOTSPOT {0, 4} } FRAME { DELAY=100 IMAGE=\"art\\green.png\" } }".to_vec(),
    );
    pkg.add_file(
        "scenes\\room.scene",
        b"SCENE {
  NAME=\"Room\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=64 HEIGHT=32
    ENTITY { NAME=\"bg\" X=0 Y=0 SPRITE=\"art\\bg.png\" }
    REGION { NAME=\"floor\" POINT {0,16} POINT {64,16} POINT {64,32} POINT {0,32} }
    ENTITY { NAME=\"prop\" X=8 Y=12 SPRITE=\"art\\prop.sprite\" }
    ENTITY { NAME=\"hidden\" X=20 Y=0 ACTIVE=FALSE SPRITE=\"art\\red.png\" }
    ENTITY { NAME=\"hum\" SUBTYPE=\"sound\" X=0 Y=0 SOUND=\"audio\\hum.ogg\" }
    ENTITY { NAME=\"broken\" X=0 Y=0 SPRITE=\"art\\missing.sprite\" }
  }
  ENTITY { NAME=\"free\" X=40 Y=20 SPRITE=\"art\\green.png\" }
}"
        .to_vec(),
    );
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

#[test]
fn places_subframes_like_the_engine() {
    // `place` only needs a texture for its size; use the GPU when there
    // is one, otherwise skip (the maths is also covered by the pixel test).
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no adapter");
        return;
    };
    let renderer = Renderer::new(gpu);
    let texture = renderer.texture(
        "t",
        &Image {
            width: 10,
            height: 20,
            pixels: vec![0; 10 * 20 * 4],
            has_alpha_channel: true,
        },
    );
    let sub = SubFrame {
        image: Some("t".into()),
        hotspot_x: 2,
        hotspot_y: 20,
        ..SubFrame::default()
    };
    let owner = Rgba {
        r: 200,
        g: 200,
        b: 200,
        a: 128,
    };
    let q = place(&sub, &texture, 100, 300, 1.0, owner);
    assert_eq!(
        (q.dst.x, q.dst.y, q.dst.w, q.dst.h),
        (98.0, 280.0, 10.0, 20.0)
    );
    assert_eq!(q.src, None);
    assert_eq!(q.tint, owner, "untinted sub-frame takes the owner's tint");

    let q = place(&sub, &texture, 100, 300, 0.5, owner);
    assert_eq!(
        (q.dst.x, q.dst.y, q.dst.w, q.dst.h),
        (99.0, 290.0, 5.0, 10.0)
    );

    let tinted = SubFrame {
        rect: Some(luma_formats::Rect {
            left: 1,
            top: 2,
            right: 4,
            bottom: 8,
        }),
        tint: Rgba {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        mirror_x: true,
        ..sub.clone()
    };
    let q = place(&tinted, &texture, 0, 0, 1.0, owner);
    assert_eq!((q.dst.w, q.dst.h), (3.0, 6.0));
    assert_eq!(
        q.src.map(|r| (r.x, r.y, r.w, r.h)),
        Some((1.0, 2.0, 3.0, 6.0))
    );
    assert_eq!(q.tint, tinted.tint, "the sub-frame's own tint wins");
    assert!(q.flip_x && !q.flip_y);
}

#[test]
fn loads_and_draws_a_scene_from_a_package() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let settings = GameSettings::load(&vfs).unwrap();
    assert_eq!((settings.width, settings.height), (32, 32));

    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no adapter");
        return;
    };
    let renderer = Renderer::new(gpu);
    let mut view = SceneView::load(&vfs, "scenes/room.scene").unwrap();
    assert_eq!(view.scene.name, "Room");
    assert_eq!(view.texture_count(), 0, "nothing uploaded yet");
    view.upload(&vfs, &renderer);
    assert_eq!(view.texture_count(), 3, "bg, red, green");
    assert_eq!(view.missing().len(), 1);
    assert!(
        view.missing()[0].0.ends_with("missing.sprite"),
        "{:?}",
        view.missing()
    );
    assert!(
        view.sprite("ART/PROP.SPRITE").is_some(),
        "lookups ignore case"
    );

    let mut camera = Camera::new(32.0, 32.0);
    camera.offset_x = 1000.0;
    camera.clamp_to(&view.scene);
    assert_eq!(camera.offset_x, 32.0, "clamped to the scene width");
    camera.offset_x = 0.0;

    let target = Offscreen::new(renderer.gpu(), 32, 32);
    let mut list = DrawList::new(32.0, 32.0);
    view.draw(&mut list, &camera);
    assert_eq!(list.quads().len(), 3, "bg, prop frame 0, free entity");
    renderer
        .render(
            target.target(),
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            &list,
        )
        .unwrap();
    let out = target.read(renderer.gpu()).unwrap();
    let px = |x, y| {
        let p = out.pixel(x, y).unwrap();
        (p.r, p.g, p.b)
    };
    assert_eq!(px(1, 1), (0, 0, 255), "background");
    assert_eq!(px(9, 9), (255, 0, 0), "prop: 4x4 red at (8, 12 - 4)");
    assert_eq!(px(9, 13), (0, 0, 255), "below the prop's hotspot");
    assert_eq!(px(21, 1), (0, 0, 255), "inactive entity not drawn");
    assert_eq!(
        px(31, 20),
        (0, 0, 255),
        "free entity is off screen at offset 0"
    );

    // Scroll to the right: the free entity (drawn by the floor region)
    // and the second frame of the prop come into view.
    camera.offset_x = 32.0;
    // The prop's first frame starts on the first update and lasts 100 ms.
    view.advance(1);
    view.advance(50);
    let prop = view.find_node("prop").unwrap();
    let prop = luma_adventure::EntityRef::Node {
        layer: prop.0,
        index: prop.1,
    };
    assert_eq!(view.frame_of(prop), 0);
    view.advance(151);
    assert_eq!(view.frame_of(prop), 1);
    view.advance(300);
    assert!(view.anim(prop).unwrap().finished, "two frames, not looping");
    let mut list = DrawList::new(32.0, 32.0);
    view.draw(&mut list, &camera);
    renderer
        .render(
            target.target(),
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            &list,
        )
        .unwrap();
    let out = target.read(renderer.gpu()).unwrap();
    let p = out.pixel(8, 20).unwrap();
    assert_eq!((p.r, p.g, p.b), (0, 255, 0), "free entity at (40 - 32, 20)");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn play_anim_shows_a_sprite_once_then_gives_the_entity_its_own_back() {
    use luma_adventure::EntityRef;
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut view = SceneView::load(&vfs, "scenes/room.scene").unwrap();
    // "bg" is node 0 of the main layer; prop.sprite has two 100 ms frames.
    let bg = EntityRef::Node { layer: 0, index: 0 };
    assert!(!view.is_playing_anim(bg));
    assert!(!view.play_anim(&vfs, bg, "art\\nothing.sprite"));
    assert!(view.play_anim(&vfs, bg, "art\\prop.sprite"));
    assert!(view.is_playing_anim(bg));
    assert_eq!(view.shown_sprite(bg).as_deref(), Some("art\\prop.sprite"));
    view.advance(1);
    view.advance(150);
    assert!(view.is_playing_anim(bg), "second frame showing");
    view.advance(300);
    view.advance(320);
    assert!(!view.is_playing_anim(bg), "played to its end");
    assert_eq!(view.shown_sprite(bg).as_deref(), Some("art\\bg.png"));
    // Reset cuts a playing sprite short.
    assert!(view.play_anim(&vfs, bg, "art\\prop.sprite"));
    view.reset_anim(bg);
    assert!(!view.is_playing_anim(bg));
    // A looping sprite plays on, moving the entity by its frames' MOVE
    // (a character wandering or walking across the scene).
    let x = view.entity(bg).unwrap().x;
    assert!(view.play_anim(&vfs, bg, "art\\loop.sprite"));
    for t in (400..=2400).step_by(50) {
        view.advance(t);
    }
    assert!(view.is_playing_anim(bg), "a looping sprite never ends");
    assert!(
        view.entity(bg).unwrap().x >= x + 32,
        "{}",
        view.entity(bg).unwrap().x
    );
    view.reset_anim(bg);
    assert!(!view.is_playing_anim(bg));
}
