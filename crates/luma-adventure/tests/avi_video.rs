//! `Game.PlayVideo`: the AVI player of the engine (`CVidPlayer`), from
//! the script call to the end of the video. The AVI files are written
//! in code (`luma_video::build_avi`).

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Runtime, VideoPlayback};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_formats::Image;
use luma_script::{Natives as _, Value};
use luma_video::{AviSound, AviSpec, build_avi, synthetic_avi};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-avi-game-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const GAME_SCRIPT: &str = r#"
global result = new Object();

on "PlayAvi"
{
  result.started = Game.PlayVideo("intro.avi");
  result.after = true;
}

on "PlayAviPos"
{
  Game.PlayVideo("intro.avi", 0, 4, 6, false, "other.sub");
  result.afterPos = true;
}

on "PlayAviCenter"
{
  Game.PlayVideo("intro.avi", 2, 4, 6);
  result.afterCenter = true;
}

on "PlayAviOddType"
{
  Game.PlayVideo("intro.avi", 7);
}

on "PlayDivx"
{
  result.divx = Game.PlayVideo("divx.avi");
  result.afterDivx = true;
}

on "PlayMissing"
{
  result.missing = Game.PlayVideo("none.avi");
}

on "PlaySoundOnly"
{
  result.soundOnly = Game.PlayVideo("sound.avi");
}

on "PlayBroken"
{
  result.broken = Game.PlayVideo("broken.avi");
}

on "PlayMp3"
{
  result.mp3 = Game.PlayVideo("mp3.avi");
}

on "PlayOgg"
{
  result.ogg = Game.PlayVideo("clip.ogv", 0, 1, 2, true, "other.sub");
}
"#;

fn compile(source: &str, name: &str) -> Vec<u8> {
    Compiler::new(Natives::wme())
        .compile(source.as_bytes(), name, &mut |_: &[u8]| None)
        .unwrap()
}

/// An AVI file of one uncompressed frame with the fields changed by
/// `change`.
fn avi_with(change: impl FnOnce(&mut AviSpec)) -> Vec<u8> {
    let mut spec = AviSpec {
        width: 2,
        height: 2,
        bit_count: 24,
        rate: 10,
        scale: 1,
        frames: vec![vec![0; 16]],
        index: true,
        ..AviSpec::default()
    };
    change(&mut spec);
    build_avi(&spec)
}

fn game() -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 64,48 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Avi\" SCRIPT=\"game.script\" SYSTEM_FONT=\"sys.font\" }".to_vec(),
    );
    pkg.add_file(
        "sys.font",
        b"FONT { IMAGE=\"dot.png\" COLUMNS=4 TILE_WIDTH=1 TILE_HEIGHT=1 DEFAULT_WIDTH=1 }".to_vec(),
    );
    pkg.add_file(
        "dot.png",
        Image {
            width: 4,
            height: 4,
            pixels: [255, 255, 255, 255].repeat(16),
            has_alpha_channel: true,
        }
        .to_png()
        .unwrap(),
    );
    pkg.add_file("game.script", compile(GAME_SCRIPT, "game.script"));
    // Ten frames of 16x8 at 10 per second, with a second of sound.
    pkg.add_file("intro.avi", synthetic_avi(16, 8, 10, 10, 1000));
    pkg.add_file("intro.sub", b"{1}{1}First\r\n{3}{5}Hello|avi".to_vec());
    pkg.add_file("other.sub", b"{0}{0}Other".to_vec());
    pkg.add_file(
        "divx.avi",
        avi_with(|spec| spec.compression = u32::from_le_bytes(*b"DIVX")),
    );
    pkg.add_file(
        "mp3.avi",
        avi_with(|spec| {
            spec.sound = Some(AviSound {
                tag: 0x55,
                channels: 2,
                sample_rate: 44100,
                block_align: 1,
                bits: 0,
                extra: vec![0; 12],
                chunks: vec![vec![0xff; 64]],
            });
        }),
    );
    // An AVI file without a video stream: the stream is made a sound
    // stream of no known format.
    let mut sound_only = avi_with(|_| {});
    let kind = sound_only.windows(4).position(|w| w == b"vids").unwrap();
    sound_only[kind..kind + 4].copy_from_slice(b"auds");
    pkg.add_file("sound.avi", sound_only);
    pkg.add_file("broken.avi", b"RIFF\x10\0\0\0AVI LIST".to_vec());
    pkg.add_file("clip.ogv", luma_video::synthetic_clip(32, 32, 3, 10));
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

fn start() -> (Runtime, std::path::PathBuf) {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    (runtime, dir)
}

fn result(runtime: &Runtime, key: &str) -> Value {
    runtime
        .engine
        .global("result")
        .and_then(Value::as_native)
        .and_then(|id| runtime.world.objects().get(id))
        .and_then(|native| native.props.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

fn raise(runtime: &mut Runtime, event: &str) -> Vec<String> {
    let game = runtime.world.game_id();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, event, false);
    runtime.tick(16)
}

#[test]
fn an_avi_video_freezes_the_game_and_runs_on_the_clock() {
    let (mut runtime, dir) = start();
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "VideoSubtitles", &Value::Bool(true));
    let log = raise(&mut runtime, "PlayAvi");
    assert!(
        log.iter()
            .any(|l| l.contains("Game.PlayVideo() is now deprecated")),
        "{log:?}"
    );
    assert!(runtime.world.is_video_playing());
    assert!(runtime.world.is_frozen());
    assert_eq!(result(&runtime, "started"), Value::Null, "the call sleeps");
    let player = runtime.world.video().unwrap();
    assert!(player.avi.is_some());
    assert_eq!(player.avi.unwrap().total_ms, 1000);
    assert_eq!((player.info.width, player.info.height), (16, 8));
    // Stretched on 64x48: four times, centred.
    assert_eq!(player.playback, VideoPlayback::Stretch);
    assert_eq!((player.x, player.y, player.zoom), (0, 8, 400.0));
    assert_eq!(player.picture_offset(), (0.0, 0.0));
    assert_eq!(player.frame_index(), Some(0));
    assert!(player.sound.is_some(), "the PCM track plays");
    // A `{1}{1}` line is a subtitle here, shown from the start.
    assert_eq!(player.subtitler.as_ref().unwrap().shown(), Some("First"));
    assert!(player.subtitle_height > 0);
    // 20 ticks of 16 ms: 320 ms, the fourth frame and its subtitle.
    let frozen_at = runtime.world.timer();
    for _ in 0..20 {
        runtime.tick(16);
    }
    assert_eq!(runtime.world.timer(), frozen_at, "the game timer stops");
    assert_eq!(result(&runtime, "after"), Value::Null, "scripts do not run");
    let player = runtime.world.video().unwrap();
    assert_eq!(player.frame_index(), Some(3));
    assert_eq!(
        player.subtitler.as_ref().unwrap().shown(),
        Some("Hello\navi")
    );
    // The AVI player looks at `VideoSubtitles` when it loads the file
    // only: turned off, the subtitle is still measured and drawn.
    runtime
        .world
        .set_property(game, "VideoSubtitles", &Value::Bool(false));
    runtime.tick(16);
    let player = runtime.world.video().unwrap();
    assert!(player.subtitler.as_ref().unwrap().shown().is_some());
    // One long frame: the picture of that instant.
    runtime.tick(400);
    assert_eq!(runtime.world.video().unwrap().frame_index(), Some(7));
    // The video ends a second after its start, whatever the sound does.
    let mut waited = 16 * 21 + 400;
    while runtime.world.is_video_playing() && waited < 5000 {
        runtime.tick(16);
        waited += 16;
    }
    assert!((1000..1016).contains(&waited), "{waited} ms");
    assert!(!runtime.world.is_frozen());
    runtime.tick(16);
    assert!(runtime.world.video().is_none());
    assert_eq!(result(&runtime, "started"), Value::Bool(true));
    assert_eq!(result(&runtime, "after"), Value::Bool(true));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_avi_video_is_placed_and_skipped() {
    let (mut runtime, dir) = start();
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "VideoSubtitles", &Value::Bool(true));
    // At a position, with the subtitle file of the sixth argument;
    // skipped by a click.
    raise(&mut runtime, "PlayAviPos");
    let player = runtime.world.video().unwrap();
    assert_eq!(player.playback, VideoPlayback::Pos);
    assert_eq!((player.x, player.y, player.zoom), (4, 6, 100.0));
    assert_eq!(player.subtitler.as_ref().unwrap().shown(), Some("Other"));
    runtime.mouse_left_down();
    assert!(!runtime.world.is_video_playing());
    assert!(!runtime.world.is_frozen());
    runtime.mouse_left_up();
    runtime.tick(16);
    assert_eq!(result(&runtime, "afterPos"), Value::Bool(true));
    // Centred at 100 %; skipped by Escape, other keys do nothing.
    raise(&mut runtime, "PlayAviCenter");
    let player = runtime.world.video().unwrap();
    assert_eq!((player.x, player.y, player.zoom), (24, 20, 100.0));
    runtime.key_down(65, Some('a'), false, false, false);
    assert!(runtime.world.is_video_playing());
    runtime.key_down(27, None, false, false, false);
    assert!(!runtime.world.is_video_playing());
    assert!(!runtime.world.is_frozen());
    runtime.tick(16);
    assert_eq!(result(&runtime, "afterCenter"), Value::Bool(true));
    // A type that is none of the three stretches.
    raise(&mut runtime, "PlayAviOddType");
    assert_eq!(runtime.world.video().unwrap().zoom, 400.0);
    assert!(runtime.world.stop_video(&mut runtime.engine));
    assert!(!runtime.world.is_frozen());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_video_that_cannot_play_is_logged_and_the_script_goes_on() {
    let (mut runtime, dir) = start();
    // A codec Luma does not decode: named, and the call answers false
    // at once, as when a video cannot be opened.
    let log = raise(&mut runtime, "PlayDivx");
    assert!(
        log.iter().any(|l| l
            == "Error: Unsupported AVI format (file 'divx.avi'): video codec 'DIVX' is not supported"),
        "{log:?}"
    );
    assert_eq!(result(&runtime, "divx"), Value::Bool(false));
    assert_eq!(result(&runtime, "afterDivx"), Value::Bool(true));
    assert!(!runtime.world.is_video_playing());
    assert!(!runtime.world.is_frozen());
    let log = raise(&mut runtime, "PlayMissing");
    assert_eq!(result(&runtime, "missing"), Value::Bool(false));
    assert!(
        log.iter()
            .any(|l| l.starts_with("Error opening AVI file 'none.avi'")),
        "{log:?}"
    );
    let log = raise(&mut runtime, "PlaySoundOnly");
    assert_eq!(result(&runtime, "soundOnly"), Value::Bool(false));
    assert!(
        log.iter()
            .any(|l| l == "Error finding video stream in AVI file 'sound.avi'"),
        "{log:?}"
    );
    let log = raise(&mut runtime, "PlayBroken");
    assert_eq!(result(&runtime, "broken"), Value::Bool(false));
    assert!(log.iter().any(|l| l.starts_with("Error")), "{log:?}");
    assert!(!runtime.world.is_frozen());
    // A sound track Luma does not decode: named, and the video plays
    // without it.
    let log = raise(&mut runtime, "PlayMp3");
    assert!(
        log.iter().any(|l| l
            == "Error initializing sound buffer for AVI file 'mp3.avi': unsupported codec: sound format tag 0x0055 (MP3)"),
        "{log:?}"
    );
    let player = runtime.world.video().unwrap();
    assert!(player.sound.is_none());
    assert_eq!(player.frame_index(), Some(0));
    // One frame at 10 per second: over after 100 ms.
    for _ in 0..7 {
        runtime.tick(16);
    }
    assert!(!runtime.world.is_video_playing());
    runtime.tick(16);
    assert_eq!(result(&runtime, "mp3"), Value::Bool(true));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn play_video_plays_an_ogg_theora_file_too() {
    let (mut runtime, dir) = start();
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "VideoSubtitles", &Value::Bool(true));
    raise(&mut runtime, "PlayOgg");
    let player = runtime.world.video().unwrap();
    assert!(player.avi.is_none(), "the Theora player's rules");
    assert_eq!((player.x, player.y), (1, 2));
    // The sixth argument is the subtitle file, as `PlayVideo` has it.
    assert_eq!(player.subtitler.as_ref().unwrap().shown(), Some("Other"));
    assert!(runtime.world.stop_video(&mut runtime.engine));
    std::fs::remove_dir_all(&dir).unwrap();
}
