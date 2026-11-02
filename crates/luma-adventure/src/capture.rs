//! Frame captures: `Game.Screenshot` and `ScreenshotEx` write the frame
//! to an image file, `StoreSaveThumbnail` keeps a small copy for the
//! next saved game and `DeleteSaveThumbnail` drops it. The world does
//! not draw by itself, so a capture is a request the front-end fulfils
//! with the next frame it renders ([`World::wants_frame`],
//! [`World::deliver_frame`]); the original engine reads the screen back
//! at the call. See `docs/saving.md`.
//!
//! Written with reference to `BGame.cpp` (`Screenshot`, `ScreenshotEx`,
//! `StoreSaveThumbnail`, `DeleteSaveThumbnail`), `BSaveThumbHelper.cpp`
//! and `BPersistMgr.cpp` of Wintermute Engine 1.x (MIT License,
//! Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use std::path::{Path, PathBuf};

use luma_formats::Image;

use crate::world::World;

/// A capture a script asked for, done with the next frame rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture {
    /// The frame written to a file, scaled to a size when one is given.
    Screenshot {
        /// Where to write; PNG for a `.png` name, BMP otherwise.
        path: PathBuf,
        /// The size to scale to (`ScreenshotEx`).
        size: Option<(u32, u32)>,
    },
    /// The frame scaled to the game's thumbnail size and kept for the
    /// next saved game.
    Thumbnail,
}

/// Encodes an image as the file's extension says: PNG for `.png`, else
/// a 24-bit BMP, the original engine's format (`SaveBMPFile`).
fn encode(path: &Path, image: &Image) -> Result<Vec<u8>, String> {
    let png = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));
    if png {
        image.to_png().map_err(|e| e.to_string())
    } else {
        image.to_bmp().map_err(|e| e.to_string())
    }
}

fn write_image(path: &Path, image: &Image) -> Result<(), String> {
    let bytes = encode(path, image)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

impl World {
    /// A path a script gave for a file to write: as is when absolute,
    /// else under the game's directory, which is the original engine's
    /// working directory.
    fn output_path(&self, filename: &str) -> PathBuf {
        let path = Path::new(&filename.replace('\\', "/")).to_path_buf();
        if path.is_absolute() {
            path
        } else {
            self.game_directory().join(path)
        }
    }

    /// Asks for the frame to be written to `<name>NNN.bmp`, the first
    /// number whose file does not exist (`Game.Screenshot`); the game's
    /// name when no name is given. Returns the file that will be
    /// written.
    pub fn request_screenshot(&mut self, name: Option<&str>) -> PathBuf {
        let base = self.output_path(name.unwrap_or(&self.name));
        let mut number = 0;
        let path = loop {
            let candidate = base.with_file_name(format!(
                "{}{number:03}.bmp",
                base.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            let pending = self
                .captures
                .iter()
                .any(|c| matches!(c, Capture::Screenshot { path, .. } if *path == candidate));
            if !pending && !candidate.exists() {
                break candidate;
            }
            number += 1;
        };
        self.captures.push(Capture::Screenshot {
            path: path.clone(),
            size: None,
        });
        path
    }

    /// Asks for the frame to be written to a file at a size
    /// (`Game.ScreenshotEx`); a size of zero keeps the frame's own.
    pub fn request_screenshot_sized(&mut self, filename: &str, width: i32, height: i32) {
        let path = self.output_path(filename);
        let size = (width > 0 && height > 0).then_some((width as u32, height as u32));
        self.captures.push(Capture::Screenshot { path, size });
    }

    /// The thumbnail size of the game (`THUMBNAIL_WIDTH`,
    /// `THUMBNAIL_HEIGHT` of `default.game`), `None` when the game sets
    /// none: its saved games then carry no thumbnail, as in the
    /// original engine.
    pub fn thumbnail_size(&self) -> Option<(u32, u32)> {
        let (w, h) = (
            self.game_file.thumbnail_width,
            self.game_file.thumbnail_height,
        );
        (w > 0 && h > 0).then_some((w as u32, h as u32))
    }

    /// Asks for the frame to become the thumbnail of the next saved game
    /// (`Game.StoreSaveThumbnail`). Nothing is asked when the game has
    /// no thumbnail size, which the original engine also takes as
    /// success.
    pub fn store_save_thumbnail(&mut self) -> bool {
        self.save_thumbnail = None;
        if self.thumbnail_size().is_some() && !self.captures.contains(&Capture::Thumbnail) {
            self.captures.push(Capture::Thumbnail);
        }
        true
    }

    /// Drops the thumbnail kept for the next saved game
    /// (`Game.DeleteSaveThumbnail`), asked or already taken.
    pub fn delete_save_thumbnail(&mut self) {
        self.save_thumbnail = None;
        self.captures.retain(|c| *c != Capture::Thumbnail);
    }

    /// The thumbnail the next saved game will carry, as PNG.
    pub fn save_thumbnail(&self) -> Option<&[u8]> {
        self.save_thumbnail.as_deref()
    }

    /// Whether a script asked for a capture, so that the front-end
    /// should render the next frame where it can be read back and pass
    /// it to [`World::deliver_frame`].
    pub fn wants_frame(&self) -> bool {
        !self.captures.is_empty()
    }

    /// The captures asked for and not delivered yet.
    pub fn captures(&self) -> &[Capture] {
        &self.captures
    }

    /// Fulfils the captures asked for with a rendered frame: the
    /// screenshots are written (a failure is logged) and the thumbnail
    /// kept.
    pub fn deliver_frame(&mut self, frame: &Image) {
        for capture in std::mem::take(&mut self.captures) {
            match capture {
                Capture::Screenshot { path, size } => {
                    let result = match size {
                        Some((w, h)) if (w, h) != (frame.width, frame.height) => {
                            write_image(&path, &frame.resized(w, h))
                        }
                        _ => write_image(&path, frame),
                    };
                    match result {
                        Ok(()) => self.log(format!("Screenshot written to '{}'", path.display())),
                        Err(err) => self.log(format!("Error writing screenshot: {err}")),
                    }
                }
                Capture::Thumbnail => {
                    if let Some((w, h)) = self.thumbnail_size() {
                        match frame.resized(w, h).to_png() {
                            Ok(png) => self.save_thumbnail = Some(png),
                            Err(err) => self.log(format!("Error storing thumbnail: {err}")),
                        }
                    }
                }
            }
        }
    }
}
