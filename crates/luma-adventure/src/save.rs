//! Saved games: the whole state of the engine and of the world written
//! to a slot and read back (`Game.SaveGame`, `Game.LoadGame`), the save
//! directory and its slots, and what is loaded again after a game loads.
//! The file format is Luma's own; see `docs/saving.md`.
//!
//! Written with reference to `BGame.cpp` (`SaveGame`, `LoadGame`,
//! `InitAfterLoad`, `GetSaveDir`, `GetSaveSlotFilename`,
//! `IsSaveSlotUsed`, `GetSaveSlotDescription`, `EmptySaveSlot`),
//! `AdGame.cpp` (`Persist`) and `BPersistMgr.cpp` of Wintermute Engine
//! 1.x (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use luma_audio::{SoundId, SoundType};
use luma_script::{Engine, EngineSnapshot, NativeId};
use serde::{Deserialize, Serialize};

use crate::actor::Actor;
use crate::capture::Capture;
use crate::fader::Fader;
use crate::input::{Cursors, Keyboard, Mouse};
use crate::inventory::Inventory;
use crate::objects::Objects;
use crate::scene_state::SceneState;
use crate::scene_view::SceneView;
use crate::sound::{Crossfade, MUSIC_CHANNELS, MusicChannel, ObjectSfx, ObjectSound, SfxEffect};
use crate::talk::TalkSkipButton;
use crate::ui::{ControlKind, Ui};
use crate::world::World;

/// The first bytes of a saved game.
pub const MAGIC: &[u8; 8] = b"LUMASAVE";

/// The version of the file layout written.
pub const VERSION: u32 = 21;

/// The extension of the slot files, `save000.dsv` and up.
pub const SAVE_EXTENSION: &str = "dsv";

/// The save directory a game does not name (`LOCAL_SAVE_DIR`).
pub const DEFAULT_SAVE_DIR: &str = "saves";

/// The file in the save directory naming the slot saved last
/// (`MostRecentSaveSlot`, in the original engine's registry).
const RECENT_FILE: &str = "recent.slot";

/// The start of the name of a saved game's thumbnail read as an image
/// file: `savegame:<slot>`, `savegame:<slot>,g` for a grey copy
/// (`CBSaveThumbFile`).
pub const THUMBNAIL_PREFIX: &str = "savegame:";

/// Whether a file name is a saved game's thumbnail
/// (`strnicmp(Filename, "savegame:", 9)`).
pub fn is_thumbnail_path(path: &str) -> bool {
    path.get(..THUMBNAIL_PREFIX.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(THUMBNAIL_PREFIX))
}

/// Where the slot files are, for what reads them without the world: the
/// thumbnails served as image files (`CBSaveThumbFile::Open`).
#[derive(Debug, Clone)]
pub(crate) struct SlotFiles {
    /// The game's own save directory.
    local: PathBuf,
    /// The user's own save directory for the game.
    private: Option<PathBuf>,
    /// Whether saved games go to the user's directory; shared with the
    /// world, which switches it when the game's is not writable.
    personal: Arc<AtomicBool>,
    /// The extension of the slot files.
    extension: String,
    /// The game's `NAME`.
    game: String,
}

impl SlotFiles {
    /// The file of a slot (`GetSaveSlotFilename`, without the write
    /// test of the save directory).
    fn path(&self, slot: i32) -> PathBuf {
        let dir = match (&self.private, self.personal.load(Ordering::Relaxed)) {
            (Some(private), true) => private,
            _ => &self.local,
        };
        dir.join(format!("save{slot:03}.{}", self.extension))
    }

    /// The image file of a `savegame:` name (`CBSaveThumbFile::Open`,
    /// `CBImage::LoadFile`): the thumbnail of the slot's saved game, as
    /// a PNG; grey when the name ends in `,g`. `None` when the slot is
    /// free, holds another game's file or carries no thumbnail.
    fn thumbnail(&self, name: &str) -> Option<Vec<u8>> {
        let rest = name.get(THUMBNAIL_PREFIX.len()..)?;
        // The digits after the colon, read as `atoi` reads them.
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let slot = digits.bytes().fold(0i32, |n, b| {
            n.wrapping_mul(10).wrapping_add(i32::from(b - b'0'))
        });
        let header = read_save_header(&self.path(slot)).ok()?;
        if !header.game.eq_ignore_ascii_case(&self.game) {
            return None;
        }
        let thumbnail = header.thumbnail?;
        if thumbnail.len() > MAX_THUMBNAIL {
            return None;
        }
        let bytes = rest.as_bytes();
        let grey = bytes.len() >= 2
            && bytes[bytes.len() - 2] == b','
            && bytes[bytes.len() - 1].eq_ignore_ascii_case(&b'g');
        if !grey {
            return Some(thumbnail);
        }
        let mut image =
            luma_formats::image::decode(&thumbnail, luma_formats::DEFAULT_COLOR_KEY).ok()?;
        if u64::from(image.width) * u64::from(image.height) > MAX_THUMBNAIL_PIXELS {
            return None;
        }
        image.grayscale();
        image.to_png().ok()
    }
}

/// How the frame that follows a save is drawn (`CBGame::SaveGame`,
/// `CBPersistMgr::InitSave`): the original engine draws the game
/// without its normal windows, sentences, cursor and fader
/// (`DisplayContent(false)`) while it saves, to take the thumbnail and
/// to show the saving screen with the progress indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveDisplay {
    /// Whether the saving screen and the indicator are shown (not for a
    /// quick save).
    pub indicator: bool,
}

/// What a saved game says about itself before its data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveHeader {
    /// The game's `NAME`; a file of another game is refused.
    pub game: String,
    /// The description the script gave.
    pub description: String,
    /// When it was saved, in seconds since the Unix epoch.
    pub timestamp: u64,
    /// A PNG thumbnail, when one was stored.
    pub thumbnail: Option<Vec<u8>>,
}

/// A sound as saved: its file and where it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundState {
    /// The file it came from.
    pub filename: String,
    /// Its position in milliseconds.
    pub position_ms: u64,
    /// Whether it was playing, or held by a frozen game. The original
    /// engine records a held sound as not playing and without its
    /// position (`CBSoundBuffer::IsPlaying`), so that it starts over at
    /// the unfreeze after a load; Luma keeps where it was.
    pub playing: bool,
    /// Whether a script paused it.
    pub paused: bool,
    /// Whether it loops.
    pub looping: bool,
    /// Where the loop restarts.
    pub loop_start_ms: u64,
    /// Its own volume in percent.
    pub private_volume: i32,
    /// Where every later play of it starts (`CBSound::m_SoundPosition`).
    pub start_ms: u64,
}

/// An object's sound state as saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectSoundState {
    /// The sound, if the object has one.
    pub sound: Option<SoundState>,
    /// Where the next one starts.
    pub start_ms: u64,
    /// The object's sound volume.
    pub volume: i32,
    /// The object's sound effect.
    pub effect: Option<SfxEffect>,
    /// The event raised on the object when its sound is not playing.
    pub event: Option<String>,
}

/// Everything the world plays, as saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoundsState {
    /// The objects' sounds.
    pub objects: Vec<(NativeId, ObjectSoundState)>,
    /// The music channels.
    pub music: Vec<Option<SoundState>>,
    /// Where each channel's next music starts.
    pub music_start_time: Vec<u64>,
    /// The crossfade in progress.
    pub crossfade: Option<Crossfade>,
    /// The speech directories.
    pub speech_dirs: Vec<String>,
    /// Whether the frozen game held the music too.
    pub music_frozen: bool,
    /// The sounds of sprite frames still playing: the object showing
    /// the sprite, the file, the position in milliseconds.
    pub frame_sounds: Vec<(NativeId, String, u64)>,
}

/// The world's state as written (`WorldRef`, borrowing the world) and
/// as read (`WorldState`, owning it): the same fields in the same
/// order, which the binary layout relies on.
macro_rules! world_state {
    ($($field:ident : $ty:ty),* $(,)?) => {
        #[derive(Serialize)]
        struct WorldRef<'a> {
            sounds: &'a SoundsState,
            $($field: &'a $ty,)*
        }

        #[derive(Deserialize)]
        struct WorldState {
            sounds: SoundsState,
            $($field: $ty,)*
        }

        impl<'a> WorldRef<'a> {
            fn of(world: &'a World, sounds: &'a SoundsState) -> Self {
                WorldRef {
                    sounds,
                    $($field: &world.$field,)*
                }
            }
        }

        impl WorldState {
            /// Puts the state on a world and returns the sounds to rebuild.
            fn apply(self, world: &mut World) -> SoundsState {
                $(world.$field = self.$field;)*
                self.sounds
            }
        }
    };
}

world_state! {
    objects: Objects,
    game_id: NativeId,
    keyboard_id: NativeId,
    math_id: NativeId,
    scene_viewport: Option<(i32, i32, i32, i32)>,
    scene_id: NativeId,
    fader_id: NativeId,
    scene_fader_id: NativeId,
    scene: Option<SceneView>,
    game_view: SceneView,
    prev_scene_name: String,
    prev_scene_filename: String,
    scheduled_scene: Option<(String, bool)>,
    scene_states: BTreeMap<String, SceneState>,
    offset_x: i32,
    offset_y: i32,
    target_offset_x: i32,
    target_offset_y: i32,
    auto_scroll: bool,
    scroll_pixels_x: i32,
    scroll_pixels_y: i32,
    scroll_time_x: u64,
    scroll_time_y: u64,
    last_scroll_x: u64,
    last_scroll_y: u64,
    actors: Vec<(NativeId, Actor)>,
    actors3d: Vec<(NativeId, crate::actor3d::Actor3d)>,
    max_shadow_type: u8,
    shadow_image: Option<String>,
    emitters: Vec<(NativeId, crate::particles::Emitter)>,
    pending_events: Vec<(NativeId, String)>,
    system_font: Option<String>,
    subtitles: bool,
    subtitles_speed: i32,
    talk_skip_button: TalkSkipButton,
    mouse: Mouse,
    keyboard: Keyboard,
    active_object: Option<NativeId>,
    selected_item: Option<NativeId>,
    cursors: Cursors,
    fader: Fader,
    scene_fader: Fader,
    ui: Ui,
    ui_entities: Vec<crate::ui::UiEntity>,
    responses: crate::responses::Responses,
    response_box_id: NativeId,
    inventory: Inventory,
    video_subtitles: bool,
    video_font: Option<String>,
    video_skip_button: TalkSkipButton,
    interactive: bool,
    main_object: Option<NativeId>,
    name: String,
    caption: String,
    text_encoding: i32,
    text_rtl: bool,
    suspended_rendering: bool,
    suppress_script_errors: bool,
    startup_scene: Option<String>,
    mouse_lock: (i32, i32, i32, i32),
    temp_disable_save_state: bool,
    loading_screen: Option<crate::world::ScreenImage>,
    saving_screen: Option<crate::world::ScreenImage>,
    timer: u64,
    clock: u64,
    rng: u64,
}

/// The body of a saved game as written.
#[derive(Serialize)]
struct SavedRef<'a> {
    engine: &'a EngineSnapshot,
    world: WorldRef<'a>,
    videos: &'a crate::video::VideosState,
}

/// The body of a saved game as read.
#[derive(Deserialize)]
struct Saved {
    engine: EngineSnapshot,
    world: WorldState,
    videos: crate::video::VideosState,
}

/// Seconds since the Unix epoch.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Writes a saved game: the magic, the layout version, the header and
/// the compressed body.
fn write_file(path: &Path, header: &SaveHeader, body: &[u8]) -> Result<(), String> {
    let header_bytes = postcard::to_allocvec(header).map_err(|e| e.to_string())?;
    let header_len =
        u32::try_from(header_bytes.len()).map_err(|_| "header too large".to_string())?;
    let mut out = Vec::with_capacity(16 + header_bytes.len() + body.len() / 2);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(&header_bytes);
    let mut encoder = ZlibEncoder::new(out, Compression::default());
    encoder.write_all(body).map_err(|e| e.to_string())?;
    let out = encoder.finish().map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    write_atomically(path, &out)
}

/// Writes a file so that a crash or a full disk cannot leave half of it:
/// the bytes go to a file beside it, which then replaces it.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let result = std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads the header of a saved game and the offset of its body.
fn read_header_bytes(bytes: &[u8]) -> Result<(SaveHeader, usize), String> {
    if bytes.len() < 16 || &bytes[..8] != MAGIC {
        return Err("not a Luma Engine saved game".to_string());
    }
    let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if version > VERSION {
        return Err(format!(
            "saved game version {version} is newer than this engine's ({VERSION})"
        ));
    }
    let len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let end = 16usize
        .checked_add(len)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| "truncated saved game".to_string())?;
    let header: SaveHeader = postcard::from_bytes(&bytes[16..end]).map_err(|e| e.to_string())?;
    Ok((header, end))
}

/// Reads the header of a saved game file: what it is and when it was
/// saved, without reading the game itself.
pub fn read_save_header(path: &Path) -> Result<SaveHeader, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    read_header_bytes(&bytes).map(|(header, _)| header)
}

/// Reads a saved game file: its header and its body, decompressed.
fn read_file(path: &Path) -> Result<(SaveHeader, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (header, offset) = read_header_bytes(&bytes)?;
    let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if version != VERSION {
        return Err(format!(
            "saved game version {version} is older than this engine's ({VERSION})"
        ));
    }
    let mut body = Vec::new();
    ZlibDecoder::new(&bytes[offset..])
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if body.len() as u64 > MAX_BODY {
        return Err(format!(
            "{}: the saved game is larger than {MAX_BODY} bytes once unpacked",
            path.display()
        ));
    }
    Ok((header, body))
}

/// The directory the platform keeps a user's application data in.
fn data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
    }
}

/// A game's name as a directory name (`GetSaveDir`): the characters a
/// file system refuses become underscores.
fn directory_name(name: &str) -> String {
    name.chars()
        .map(|c| if "\\|*?:\"<>/".contains(c) { '_' } else { c })
        .collect()
}

/// Saves the game to a slot (`CBGame::SaveGame`): `BeforeSave` runs on
/// the game first, then the engine and the world are written as they
/// are. Returns the file written.
pub fn save_game(
    world: &mut World,
    engine: &mut Engine,
    slot: i32,
    description: &str,
    quick: bool,
) -> Result<PathBuf, String> {
    let path = world.save_slot_path(slot);
    world.log(format!("Saving game '{}'...", path.display()));
    let game = world.game_id;
    engine.apply_event(world, game, "BeforeSave", true);
    // The thumbnail `StoreSaveThumbnail` kept goes into this file and no
    // other (`BPersistMgr::InitSave` destroys the cached one once used).
    let header = SaveHeader {
        game: world.game_file.name.clone(),
        description: description.to_string(),
        timestamp: now(),
        thumbnail: world.save_thumbnail.take(),
    };
    // Without one, the original takes it now, of the game drawn without
    // its menus (`StoreThumbnail(true)`). The world does not draw by
    // itself: the picture is the frame the front-end renders next, and
    // goes into the file then (`World::deliver_frame`).
    let late_thumbnail = header.thumbnail.is_none() && world.thumbnail_size().is_some();
    let snapshot = engine.snapshot();
    let sounds = world.sounds_state();
    let videos = world.videos_state();
    let body = postcard::to_allocvec(&SavedRef {
        engine: &snapshot,
        world: WorldRef::of(world, &sounds),
        videos: &videos,
    })
    .map_err(|e| e.to_string())?;
    write_file(&path, &header, &body)?;
    world.captures.retain(|c| {
        !matches!(c, Capture::Thumbnail)
            && !matches!(c, Capture::SaveThumbnail { path: other } if *other == path)
    });
    if late_thumbnail {
        world
            .captures
            .push(Capture::SaveThumbnail { path: path.clone() });
    }
    // The frame of the save: the saving screen and the indicator unless
    // the save is a quick one, the thumbnail's picture when one is due.
    if !quick || late_thumbnail {
        world.save_display = Some(SaveDisplay { indicator: !quick });
        world.indicator_progress = 100;
    }
    world.refresh_thumbnails();
    world.set_most_recent_save_slot(slot);
    Ok(path)
}

/// Puts a thumbnail into the header of a saved game already written:
/// the picture of a save that had none stored comes with the next frame
/// rendered.
pub(crate) fn patch_thumbnail(path: &Path, thumbnail: Vec<u8>) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (mut header, offset) = read_header_bytes(&bytes)?;
    header.thumbnail = Some(thumbnail);
    let header_bytes = postcard::to_allocvec(&header).map_err(|e| e.to_string())?;
    let header_len =
        u32::try_from(header_bytes.len()).map_err(|_| "header too large".to_string())?;
    let mut out = Vec::with_capacity(16 + header_bytes.len() + bytes.len() - offset);
    // The magic and the layout version stay the file's own.
    out.extend_from_slice(&bytes[..12]);
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(&header_bytes);
    out.extend_from_slice(&bytes[offset..]);
    write_atomically(path, &out)
}

/// Loads the game of a slot (`CBGame::LoadGame`): everything the file
/// holds replaces the world and the scripts, the files they name are
/// loaded again, and `AfterLoad` runs on the game.
pub fn load_game(world: &mut World, engine: &mut Engine, slot: i32) -> Result<(), String> {
    let path = world.save_slot_path(slot);
    world.log(format!("Loading game '{}'...", path.display()));
    let (header, body) = read_file(&path)?;
    if !header.game.eq_ignore_ascii_case(&world.game_file.name) {
        return Err("Saved game name doesn't match current game".to_string());
    }
    let saved: Saved = postcard::from_bytes(&body).map_err(|e| e.to_string())?;
    // The random generator is the running game's, not the saved one's:
    // the original's `rand()` state is no part of a saved game, so
    // loading twice does not replay the same draws. The value is still
    // written, to keep the file's layout.
    let rng = world.rng;
    let sounds = saved.world.apply(world);
    world.rng = rng;
    world.after_restore(sounds);
    engine.restore(saved.engine, world)?;
    // `AfterLoadTheora`: the videos play again where they were.
    world.restore_videos(engine, saved.videos);
    let game = world.game_id;
    engine.apply_event(world, game, "AfterLoad", true);
    Ok(())
}

impl World {
    /// The directory the game was opened from.
    pub fn game_directory(&self) -> PathBuf {
        self.vfs
            .game_directory()
            .map(Path::to_path_buf)
            .or_else(|| self.vfs.directories().first().cloned())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// The game's own save directory (`LOCAL_SAVE_DIR`, `saves` by
    /// default), under the game's directory.
    fn local_save_dir(&self) -> PathBuf {
        self.game_directory().join(
            self.game_file
                .local_save_dir
                .as_deref()
                .unwrap_or(DEFAULT_SAVE_DIR),
        )
    }

    /// The user's own save directory for this game: the platform's
    /// application data, `Luma Engine`, then the game's caption or name.
    pub fn private_save_dir(&self) -> Option<PathBuf> {
        let name = if self.game_file.caption.is_empty() {
            &self.game_file.name
        } else {
            &self.game_file.caption
        };
        Some(data_dir()?.join("Luma Engine").join(directory_name(name)))
    }

    /// Where saved games go (`Game.SaveDirectory`): the game's own
    /// directory, or the user's when the game asks (`PERSONAL_SAVEGAMES`)
    /// or when the game's is not writable.
    pub fn save_directory(&self) -> PathBuf {
        if self.personal_save {
            if let Some(dir) = self.private_save_dir() {
                return dir;
            }
        }
        self.local_save_dir()
    }

    /// The save directory, after trying the game's own once
    /// (`GetSaveDir`): when a file cannot be written there, saves go to
    /// the user's directory from then on.
    pub(crate) fn checked_save_directory(&mut self) -> PathBuf {
        if !self.personal_save && !self.save_dir_checked {
            self.save_dir_checked = true;
            let dir = self.local_save_dir();
            let test = dir.join("writest.$$$");
            let writable =
                std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&test, b"").is_ok();
            if writable {
                let _ = std::fs::remove_file(&test);
            } else {
                self.log("Save directory is not writable. Switching to personal savegames.");
                self.personal_save = true;
                self.save_personal.store(true, Ordering::Relaxed);
            }
        }
        self.save_directory()
    }

    /// The file of a slot (`GetSaveSlotFilename`).
    pub fn save_slot_path(&mut self, slot: i32) -> PathBuf {
        // `SAVED_GAME_EXT` of the settings, `dsv` by default.
        let extension = self.settings.save_extension().to_string();
        self.checked_save_directory()
            .join(format!("save{slot:03}.{extension}"))
    }

    /// Whether a slot holds a saved game (`IsSaveSlotUsed`).
    pub fn is_save_slot_used(&mut self, slot: i32) -> bool {
        self.save_slot_path(slot).is_file()
    }

    /// The description of a slot's saved game (`GetSaveSlotDescription`),
    /// empty when the slot is free or holds another game's file.
    pub fn save_slot_description(&mut self, slot: i32) -> String {
        let path = self.save_slot_path(slot);
        match read_save_header(&path) {
            Ok(header) if header.game.eq_ignore_ascii_case(&self.game_file.name) => {
                header.description
            }
            _ => String::new(),
        }
    }

    /// Deletes a slot's saved game (`EmptySaveSlot`); whether there was
    /// one.
    pub fn empty_save_slot(&mut self, slot: i32) -> bool {
        let path = self.save_slot_path(slot);
        let removed = std::fs::remove_file(path).is_ok();
        self.refresh_thumbnails();
        removed
    }

    /// Serves the thumbnails of the saved games as image files
    /// (`savegame:<slot>`, `savegame:<slot>,g`), so that a sprite or a
    /// window's image can show them (`CBFileManager::OpenFileRaw`,
    /// `CBSaveThumbFile`).
    pub(crate) fn register_thumbnail_files(&mut self) {
        let slots = SlotFiles {
            local: self.local_save_dir(),
            private: self.private_save_dir(),
            personal: Arc::clone(&self.save_personal),
            extension: self.settings.save_extension().to_string(),
            game: self.game_file.name.clone(),
        };
        self.vfs
            .add_virtual_source(THUMBNAIL_PREFIX, move |name| slots.thumbnail(name));
    }

    /// Looks at the thumbnails again after a slot changed: the images
    /// shown from `savegame:` names are decoded anew, and one whose slot
    /// has no thumbnail any more becomes the placeholder.
    pub(crate) fn refresh_thumbnails(&mut self) {
        let mut stale = self.ui.bank.refresh_prefixed(&self.vfs, THUMBNAIL_PREFIX);
        stale.extend(self.game_view.refresh_thumbnails(&self.vfs));
        if let Some(view) = self.scene.as_mut() {
            stale.extend(view.refresh_thumbnails(&self.vfs));
        }
        self.dropped_textures.extend(stale);
    }

    /// How the frame after a save is drawn, until the next frame starts.
    pub fn save_display(&self) -> Option<SaveDisplay> {
        self.save_display
    }

    /// The slot saved last (`Game.MostRecentSaveSlot`), `-1` when none.
    pub fn most_recent_save_slot(&self) -> i32 {
        let path = self.save_directory().join(RECENT_FILE);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(-1)
    }

    fn set_most_recent_save_slot(&mut self, slot: i32) {
        let path = self.checked_save_directory().join(RECENT_FILE);
        if let Err(err) = std::fs::write(&path, slot.to_string()) {
            self.log(format!("cannot write '{}': {err}", path.display()));
        }
    }

    /// Asks for a slot to load at the start of the next frame
    /// (`Game.LoadGame`).
    pub fn schedule_load(&mut self, slot: i32) {
        self.scheduled_load = Some(slot);
    }

    /// The slot a script asked to load, not loaded yet.
    pub fn scheduled_load(&self) -> Option<i32> {
        self.scheduled_load
    }

    /// Everything the world plays, as a saved game keeps it.
    pub(crate) fn sounds_state(&self) -> SoundsState {
        let audio = &self.sounds.audio;
        let state = |id: SoundId, filename: &str| SoundState {
            filename: filename.to_string(),
            position_ms: audio.position_ms(id),
            playing: audio.is_playing(id) || audio.is_freeze_paused(id),
            paused: audio.is_paused(id),
            looping: audio.is_looping(id),
            loop_start_ms: audio.loop_start_ms(id),
            private_volume: audio.private_volume(id),
            start_ms: audio.start_ms(id),
        };
        let mut objects: Vec<(NativeId, ObjectSoundState)> = self
            .sounds
            .objects
            .iter()
            .map(|(owner, sfx)| {
                (
                    *owner,
                    ObjectSoundState {
                        sound: sfx.sound.as_ref().map(|s| state(s.id, &s.filename)),
                        start_ms: sfx.start_ms,
                        volume: sfx.volume,
                        effect: sfx.effect,
                        event: sfx.event.clone(),
                    },
                )
            })
            .collect();
        objects.sort_by_key(|(owner, _)| *owner);
        let music: Vec<Option<SoundState>> = self
            .sounds
            .music
            .iter()
            .map(|channel| channel.as_ref().map(|m| state(m.id, &m.filename)))
            .collect();
        let music_frozen = self
            .sounds
            .music
            .iter()
            .flatten()
            .any(|m| audio.is_freeze_paused(m.id));
        SoundsState {
            objects,
            music,
            music_start_time: self.sounds.music_start_time.clone(),
            crossfade: self.sounds.crossfade.clone(),
            speech_dirs: self.sounds.speech_dirs.clone(),
            music_frozen,
            frame_sounds: self.frame_sounds_state(),
        }
    }

    /// Loads a saved sound again and puts it where it was.
    fn restore_sound(&mut self, saved: &SoundState, kind: SoundType) -> Option<SoundId> {
        let (clip, _) = self.load_clip(&saved.filename)?;
        let audio = &mut self.sounds.audio;
        // Objects' sounds and music are the ones the engine streams.
        let id = audio.add_streamed(clip, kind, saved.private_volume);
        if saved.loop_start_ms != 0 {
            audio.set_loop_start_ms(id, saved.loop_start_ms);
        }
        if saved.playing || saved.paused {
            audio.set_position_ms(id, saved.position_ms);
            audio.play(id, saved.looping);
            if saved.paused {
                audio.pause(id);
            }
        }
        audio.set_start_ms(id, saved.start_ms);
        Some(id)
    }

    /// Replaces every sound with the saved ones (`AfterLoadSound`); a
    /// frozen game holds them again.
    fn restore_sounds(&mut self, state: SoundsState) {
        self.sounds.audio.clear();
        self.sounds.objects.clear();
        self.sounds.music = vec![None; MUSIC_CHANNELS];
        let mut start_times = state.music_start_time;
        start_times.resize(MUSIC_CHANNELS, 0);
        self.sounds.music_start_time = start_times;
        self.sounds.crossfade = state.crossfade;
        self.sounds.speech_dirs = state.speech_dirs;
        for (owner, saved) in state.objects {
            let sound = saved.sound.and_then(|s| {
                self.restore_sound(&s, SoundType::Sfx)
                    .map(|id| ObjectSound {
                        id,
                        filename: s.filename,
                    })
            });
            if let Some(id) = sound.as_ref().map(|s| s.id) {
                self.sounds
                    .audio
                    .set_effect(id, saved.effect.map(Into::into));
            }
            self.sounds.objects.insert(
                owner,
                ObjectSfx {
                    sound,
                    start_ms: saved.start_ms,
                    volume: saved.volume,
                    effect: saved.effect,
                    event: saved.event,
                },
            );
        }
        for (channel, saved) in state.music.into_iter().enumerate().take(MUSIC_CHANNELS) {
            if let Some(s) = saved {
                if let Some(id) = self.restore_sound(&s, SoundType::Music) {
                    self.sounds.music[channel] = Some(MusicChannel {
                        id,
                        filename: s.filename,
                    });
                }
            }
        }
        self.restore_speech();
        self.restore_frame_sounds(state.frame_sounds);
        if self.ui.frozen {
            self.sounds.audio.pause_all(state.music_frozen);
        }
    }

    /// What a loaded game needs beyond its state (`InitAfterLoad`): the
    /// frame's leftovers cleared, the sprites, images, fonts and cursors
    /// the state names loaded again, and the sounds started where they
    /// were. The videos follow once the scripts are back
    /// (`World::restore_videos`).
    fn after_restore(&mut self, sounds: SoundsState) {
        self.hits.clear();
        self.sentences.clear();
        self.quit = false;
        self.scheduled_load = None;
        self.pending_save = None;
        self.save_display = None;
        self.indicator_progress = 0;
        // `CBGame::Persist` runs `Cleanup()` before it loads: the
        // messages of `Game.Msg` and the loading icon of the game left
        // go.
        self.quick_messages.clear();
        self.loading_icon = None;
        // `AccessUnpause()`: the Ctrl+Space pause of the game left is
        // over; its shield window went with that game's windows.
        self.access.paused = false;
        self.access.shield = None;
        self.fader.after_load();
        self.scene_fader.after_load();
        // A picture due to a save of the game left is not taken of this
        // one.
        self.captures
            .retain(|c| !matches!(c, Capture::SaveThumbnail { .. }));
        self.reload_assets();
        self.restore_sounds(sounds);
    }

    /// Loads again every file the restored state names.
    fn reload_assets(&mut self) {
        if let Some(view) = self.scene.as_mut() {
            view.reload_sprites(&self.vfs);
        }
        self.game_view.reload_sprites(&self.vfs);
        for (_, actor) in &mut self.actors {
            actor.reload_sprites(&self.vfs);
        }
        for (_, emitter) in &mut self.emitters {
            emitter.reload_sprites(&self.vfs);
        }
        // The scene's geometry and the 3D actors' models (`3d` feature).
        self.reload_3d();
        let mut fonts: Vec<String> = Vec::new();
        fonts.extend(self.system_font.clone());
        fonts.extend(self.video_font.clone());
        let mut images: Vec<String> = Vec::new();
        let mut cursors: Vec<String> = Vec::new();
        images.extend(self.loading_screen.as_ref().map(|s| s.path.clone()));
        images.extend(self.saving_screen.as_ref().map(|s| s.path.clone()));
        if let Some(def) = &self.responses.def {
            fonts.extend(def.font.clone());
            fonts.extend(def.font_hover.clone());
        }
        for entity in &self.ui_entities {
            images.extend(entity.def.sprite.clone());
            images.extend(entity.def.talk_sprites.iter().cloned());
            images.extend(entity.def.talk_special_sprites.iter().cloned());
            fonts.extend(entity.def.font.clone());
        }
        for control in self.ui.controls.values() {
            images.extend(control.image.clone());
            images.extend(control.back.as_ref().and_then(|b| b.image.clone()));
            fonts.extend(control.font.clone());
            cursors.extend(control.cursor.clone());
            match &control.kind {
                ControlKind::Window(w) => {
                    images.extend(w.image_inactive.clone());
                    images.extend(w.back_inactive.as_ref().and_then(|b| b.image.clone()));
                    fonts.extend(w.font_inactive.clone());
                }
                ControlKind::Button(b) => {
                    images.extend(
                        [
                            &b.image_hover,
                            &b.image_press,
                            &b.image_disable,
                            &b.image_focus,
                        ]
                        .into_iter()
                        .flatten()
                        .cloned(),
                    );
                    images.extend(
                        [&b.back_hover, &b.back_press, &b.back_disable, &b.back_focus]
                            .into_iter()
                            .flatten()
                            .filter_map(|t| t.image.clone()),
                    );
                    fonts.extend(
                        [&b.font_hover, &b.font_press, &b.font_disable, &b.font_focus]
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
                }
                ControlKind::Static(_) => {}
                ControlKind::Edit(e) => fonts.extend(e.font_selected.clone()),
                ControlKind::EntityContainer(_) => {}
            }
        }
        for (_, item) in &self.inventory.items {
            images.extend(item.def.sprite.clone());
            images.extend(item.def.sprite_hover.clone());
            // What a talking or animated item shows.
            images.extend(item.def.talk.iter().cloned());
            images.extend(item.def.talk_special.iter().cloned());
            images.extend(item.anim_sprite.clone());
            images.extend(item.talk_sprite.clone());
            images.extend(item.forced_talk.clone());
            if let Some(sentence) = &item.sentence {
                fonts.extend(sentence.font.clone());
                if let Some(talk) = &sentence.talk {
                    images.extend(talk.def.sprite_paths().into_iter().map(str::to_string));
                }
            }
            fonts.extend(item.def.font.clone());
            cursors.extend(item.def.cursor.clone());
            cursors.extend(item.def.cursor_hover.clone());
        }
        for (_, actor) in &self.actors {
            fonts.extend(actor.def.font.clone());
            cursors.extend(actor.def.cursor.clone());
            fonts.extend(actor.sentence.as_ref().and_then(|s| s.font.clone()));
        }
        #[cfg(feature = "3d")]
        for (_, actor) in &self.actors3d {
            fonts.extend(actor.font.clone());
            cursors.extend(actor.cursor.clone());
            fonts.extend(actor.sentence.as_ref().and_then(|s| s.font.clone()));
        }
        for view in self.scene.iter().chain([&self.game_view]) {
            for r in view.entity_refs() {
                if let Some(e) = view.entity(r) {
                    fonts.extend(e.font.clone());
                    cursors.extend(e.cursor.clone());
                }
            }
            fonts.extend(view.sentences().filter_map(|(_, s)| s.font.clone()));
        }
        cursors.extend(self.cursors.normal.clone());
        cursors.extend(self.cursors.active.clone());
        cursors.extend(self.cursors.noninteractive.clone());
        cursors.extend(self.cursors.item_cursor.clone());
        cursors.extend(self.cursors.shown.clone());
        for image in images {
            self.load_ui_image(&image);
        }
        self.fonts.set_rtl(self.text_rtl);
        self.fonts.set_utf8(self.text_encoding == 1);
        for font in fonts {
            self.load_font(&font);
        }
        for cursor in cursors {
            self.load_cursor(&cursor);
        }
    }
}

/// The most bytes a saved game's body may unpack to: far above any game's
/// state, far below what a crafted file could ask the engine to hold.
const MAX_BODY: u64 = 512 * 1024 * 1024;

/// The largest thumbnail, in bytes of its image file and in pixels, that
/// the `savegame:` images serve.
const MAX_THUMBNAIL: usize = 8 * 1024 * 1024;
const MAX_THUMBNAIL_PIXELS: u64 = 4096 * 4096;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_replaces_the_file_whole_and_leaves_nothing_beside_it() {
        let dir = std::env::temp_dir().join(format!("luma-atomic-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("save001.dsv");
        write_atomically(&path, b"first").unwrap();
        write_atomically(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["save001.dsv"]);
        // A destination that cannot be written leaves no temporary file.
        let missing = dir.join("no").join("such").join("save.dsv");
        assert!(write_atomically(&missing, b"x").is_err());
        assert!(!dir.join("no").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn headers_round_trip_and_foreign_files_are_refused() {
        let header = SaveHeader {
            game: "Test".into(),
            description: "Hall, with the key".into(),
            timestamp: 1_761_000_000,
            thumbnail: None,
        };
        let dir = std::env::temp_dir().join(format!("luma-save-test-{}", std::process::id()));
        let path = dir.join("saves").join("save007.dsv");
        write_file(&path, &header, b"body bytes").unwrap();
        assert_eq!(read_save_header(&path).unwrap(), header);
        let (back, body) = read_file(&path).unwrap();
        assert_eq!(back, header);
        assert_eq!(body, b"body bytes");
        std::fs::write(&path, b"SAVE2 of another engine").unwrap();
        assert!(read_save_header(&path).unwrap_err().contains("not a Luma"));
        let mut newer = Vec::new();
        newer.extend_from_slice(MAGIC);
        newer.extend_from_slice(&(VERSION + 1).to_le_bytes());
        newer.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(&path, newer).unwrap();
        assert!(read_save_header(&path).unwrap_err().contains("newer"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            directory_name("The White: Chamber/2"),
            "The White_ Chamber_2"
        );
    }
}
