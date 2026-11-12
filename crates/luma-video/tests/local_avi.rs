//! Decodes local AVI files from end to end. Opt-in: no AVI file is in
//! the repository, and the test passes without them.
//!
//! ```text
//! LUMA_TEST_AVI=/some/dir cargo test -p luma-video --test local_avi -- --ignored --nocapture
//! ```
//!
//! reads every `.avi` file of the directory and prints a line per file:
//! the codec, the number of frames, the length of the video and of the
//! sound. A file whose codec Luma does not decode is reported as such.
//!
//! When a file `<name>.avi.rgb24` lies next to a video (its frames as
//! raw RGB, 3 bytes per pixel, one after the other, as another decoder
//! gives them: `ffmpeg -i x.avi -pix_fmt rgb24 -f rawvideo x.avi.rgb24`),
//! the frames are compared with it, and `<name>.avi.s16le` (the sound
//! as 16-bit samples: `ffmpeg -i x.avi -vn -f s16le x.avi.s16le`) is
//! compared with the sound. The largest and the mean differences are
//! printed; the test fails when the pictures differ by more than 3 on
//! average (decoders round colour conversions differently) or the
//! 16-bit sound by more than 1.

use std::path::PathBuf;

use luma_video::{AviVideo, Error, FrameSource};

/// The largest and the mean absolute difference of two byte strings.
fn difference(a: &[u8], b: &[u8]) -> (u8, f64) {
    let mut max = 0;
    let mut sum = 0u64;
    for (x, y) in a.iter().zip(b) {
        let d = x.abs_diff(*y);
        max = max.max(d);
        sum += u64::from(d);
    }
    (max, sum as f64 / a.len().min(b.len()).max(1) as f64)
}

#[test]
#[ignore = "needs local AVI files"]
fn decodes_the_local_avi_files() {
    let Some(dir) = std::env::var_os("LUMA_TEST_AVI")
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
    else {
        eprintln!("no local AVI files (LUMA_TEST_AVI): skipped");
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("avi"))
        })
        .collect();
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        let mut video = match AviVideo::open(bytes) {
            Ok(video) => video,
            Err(Error::UnsupportedCodec(codec)) => {
                println!("{name}: not decoded: {codec}");
                continue;
            }
            Err(err) => panic!("{name}: {err}"),
        };
        let format = video.file().video_format.clone();
        let sound = match video.sound_track() {
            Ok(Some(wav)) => {
                let clip = luma_audio::decode(&wav).unwrap_or_else(|e| panic!("{name}: {e}"));
                let reference = std::fs::read(path.with_extension("avi.s16le")).ok();
                let compared = match reference {
                    // 16-bit sound only: the data starts after the
                    // 44 bytes of the WAV header.
                    Some(reference) if wav[34] == 16 => {
                        let ours: Vec<i16> = wav[44..]
                            .chunks_exact(2)
                            .map(|b| i16::from_le_bytes([b[0], b[1]]))
                            .collect();
                        let theirs: Vec<i16> = reference
                            .chunks_exact(2)
                            .map(|b| i16::from_le_bytes([b[0], b[1]]))
                            .collect();
                        let max = ours
                            .iter()
                            .zip(&theirs)
                            .map(|(a, b)| a.abs_diff(*b))
                            .max()
                            .unwrap_or(0);
                        assert!(max <= 1, "{name}: sound differs by {max}");
                        format!(
                            ", {} samples against {}, largest difference {max}",
                            ours.len(),
                            theirs.len()
                        )
                    }
                    _ => String::new(),
                };
                format!("{} ms of sound{compared}", clip.duration_ms())
            }
            Ok(None) => "no sound".to_string(),
            Err(err) => format!("sound not decoded: {err}"),
        };
        let reference = std::fs::read(path.with_extension("avi.rgb24")).ok();
        let frame_bytes = format.width as usize * format.height as usize * 3;
        let mut frames = 0usize;
        let (mut worst, mut total) = (0u8, 0.0f64);
        while let Some(frame) = video.next_frame().unwrap_or_else(|e| panic!("{name}: {e}")) {
            if let Some(theirs) = reference
                .as_ref()
                .and_then(|r| r.get(frames * frame_bytes..(frames + 1) * frame_bytes))
            {
                let ours: Vec<u8> = frame
                    .rgba
                    .chunks_exact(4)
                    .flat_map(|p| [p[0], p[1], p[2]])
                    .collect();
                let (max, mean) = difference(&ours, theirs);
                worst = worst.max(max);
                total += mean;
            }
            frames += 1;
        }
        let compared = match &reference {
            Some(_) => {
                let mean = total / frames.max(1) as f64;
                assert!(
                    mean <= 3.0,
                    "{name}: pictures differ by {mean:.2} on average"
                );
                format!(", pictures: largest difference {worst}, mean {mean:.3}")
            }
            None => String::new(),
        };
        println!(
            "{name}: {} {}x{} {} bits, {frames} frames, {} ms, {sound}{compared}",
            luma_video::fourcc_name(format.compression),
            format.width,
            format.height,
            format.bit_count,
            video.duration_ms(),
        );
        assert_eq!(frames as u64, video.frame_count(), "{name}");
    }
}
