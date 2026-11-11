//! Decodes the videos of a local test game from end to end. Opt-in: the
//! files are not in the repository, and the test passes without them.
//!
//! ```text
//! cargo test -p luma-video --test local_videos -- --ignored --nocapture
//! ```
//!
//! reads every `.ogg` and `.ogv` file of the directory named by
//! `LUMA_TEST_VIDEOS`, or of the White Chamber source pack under
//! `testdata/local/`, and prints a line per file to compare with what
//! `ffprobe` says of it: the number of frames, when the last one shows
//! and how long the sound track is once decoded.

use std::path::PathBuf;

fn video_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os("LUMA_TEST_VIDEOS") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/white-chamber/source/WhiteChamber/assets"),
    };
    dir.is_dir().then_some(dir)
}

#[test]
#[ignore = "needs the videos of a local test game"]
fn decodes_the_local_videos() {
    let Some(dir) = video_dir() else {
        eprintln!("no local videos: skipped");
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ogg") || e.eq_ignore_ascii_case("ogv"))
        })
        .collect();
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        let sound = luma_video::sound_track(&bytes).map(|track| {
            luma_audio::decode(&track)
                .unwrap_or_else(|e| panic!("{name}: sound: {e}"))
                .duration_ms()
        });
        let mut video = match luma_video::Video::open(bytes) {
            Ok(video) => video,
            // A sound file with the same extension.
            Err(luma_video::Error::NoVideoStream) => continue,
            Err(err) => panic!("{name}: {err}"),
        };
        let info = video.info().clone();
        assert_eq!(info.has_audio, sound.is_some(), "{name}");
        let mut frames = 0u64;
        let mut last_ms = 0;
        while let Some(frame) = video.next_frame().unwrap_or_else(|e| panic!("{name}: {e}")) {
            assert_eq!(frame.index, frames, "{name}");
            assert!(frame.time_ms >= last_ms, "{name}: frame {frames} goes back");
            assert_eq!(
                frame.rgba.len(),
                (info.width * info.height * 4) as usize,
                "{name}"
            );
            last_ms = frame.time_ms;
            frames += 1;
        }
        assert!(frames > 0, "{name}: no frame");
        // The last frame shows within a second of the end of the sound.
        if let Some(sound_ms) = sound {
            assert!(
                last_ms.abs_diff(sound_ms) < 1000,
                "{name}: last frame at {last_ms} ms, {sound_ms} ms of sound"
            );
        }
        println!(
            "{name}: {}x{} {}/{} fps, {frames} frames, last at {last_ms} ms, sound: {}",
            info.width,
            info.height,
            info.fps_num,
            info.fps_den,
            sound.map_or("none".to_string(), |ms| format!("{ms} ms"))
        );
    }
}
