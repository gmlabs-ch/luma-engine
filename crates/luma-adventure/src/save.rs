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
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use luma_audio::{SoundId, SoundType};
use luma_script::{Engine, EngineSnapshot, NativeId};
use serde::{Deserialize, Serialize};

use crate::actor::Actor;
use crate::fader::Fader;
use crate::input::{Cursors, Keyboard, Mouse};
use crate::inventory::Inventory;
use crate::objects::Objects;
use crate::scene_state::SceneState;
use crate::scene_view::SceneView;
use crate::sound::{Crossfade, MUSIC_CHANNELS, MusicChannel, ObjectSfx, ObjectSound};
use crate::talk::TalkSkipButton;
use crate::ui::{ControlKind, Ui};
use crate::world::World;

/// The first bytes of a saved game.
pub const MAGIC: &[u8; 8] = b"LUMASAVE";

/// The version of the file layout written.
pub const VERSION: u32 = 2;

/// The extension of the slot files, `save000.dsv` and up.
pub const SAVE_EXTENSION: &str = "dsv";

/// The save directory a game does not name (`LOCAL_SAVE_DIR`).
pub const DEFAULT_SAVE_DIR: &str = "saves";

/// The file in the save directory naming the slot saved last
/// (`MostRecentSaveSlot`, in the original engine's registry).
const RECENT_FILE: &str = "recent.slot";

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
    /// Whether it was playing (or held by a frozen game).
    pub playing: bool,
    /// Whether a script paused it.
    pub paused: bool,
    /// Whether it loops.
    pub looping: bool,
    /// Where the loop restarts.
    pub loop_start_ms: u64,
    /// Its own volume in percent.
    pub private_volume: i32,
    /// The event raised on the owner when it ends.
    pub event: Option<String>,
}

/// An object's sound state as saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectSoundState {
    /// The sound, if the object has one.
    pub sound: Option<SoundState>,
    /// Where the next one starts.
    pub start_ms: u64,
    /// The object's sound volume.
    pub volume: i32,
}

/// Everything the world plays, as saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    scene: Option<SceneView>,
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
    timer: u64,
    clock: u64,
    rng: u64,
}

/// The body of a saved game as written.
#[derive(Serialize)]
struct SavedRef<'a> {
    engine: &'a EngineSnapshot,
    world: WorldRef<'a>,
}

/// The body of a saved game as read.
#[derive(Deserialize)]
struct Saved {
    engine: EngineSnapshot,
    world: WorldState,
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
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
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
    let mut body = Vec::new();
    ZlibDecoder::new(&bytes[offset..])
        .read_to_end(&mut body)
        .map_err(|e| format!("{}: {e}", path.display()))?;
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
    _quick: bool,
) -> Result<PathBuf, String> {
    let path = world.save_slot_path(slot);
    world.log(format!("Saving game '{}'...", path.display()));
    let game = world.game_id;
    engine.apply_event(world, game, "BeforeSave", true);
    let header = SaveHeader {
        game: world.game_file.name.clone(),
        description: description.to_string(),
        timestamp: now(),
        thumbnail: None,
    };
    let snapshot = engine.snapshot();
    let sounds = world.sounds_state();
    let body = postcard::to_allocvec(&SavedRef {
        engine: &snapshot,
        world: WorldRef::of(world, &sounds),
    })
    .map_err(|e| e.to_string())?;
    write_file(&path, &header, &body)?;
    world.set_most_recent_save_slot(slot);
    Ok(path)
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
    let sounds = saved.world.apply(world);
    world.after_restore(sounds);
    engine.restore(saved.engine, world)?;
    let game = world.game_id;
    engine.apply_event(world, game, "AfterLoad", true);
    Ok(())
}

impl World {
    /// The directory the game was opened from.
    pub fn game_directory(&self) -> PathBuf {
        self.vfs
            .directories()
            .first()
            .cloned()
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
    fn checked_save_directory(&mut self) -> PathBuf {
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
            }
        }
        self.save_directory()
    }

    /// The file of a slot (`GetSaveSlotFilename`).
    pub fn save_slot_path(&mut self, slot: i32) -> PathBuf {
        self.checked_save_directory()
            .join(format!("save{slot:03}.{SAVE_EXTENSION}"))
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
        std::fs::remove_file(path).is_ok()
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
        let state = |id: SoundId, filename: &str, event: Option<String>| SoundState {
            filename: filename.to_string(),
            position_ms: audio.position_ms(id),
            playing: audio.is_playing(id),
            paused: audio.is_paused(id) && !audio.is_freeze_paused(id),
            looping: audio.is_looping(id),
            loop_start_ms: audio.loop_start_ms(id),
            private_volume: audio.private_volume(id),
            event,
        };
        let mut objects: Vec<(NativeId, ObjectSoundState)> = self
            .sounds
            .objects
            .iter()
            .map(|(owner, sfx)| {
                (
                    *owner,
                    ObjectSoundState {
                        sound: sfx
                            .sound
                            .as_ref()
                            .map(|s| state(s.id, &s.filename, s.event.clone())),
                        start_ms: sfx.start_ms,
                        volume: sfx.volume,
                    },
                )
            })
            .collect();
        objects.sort_by_key(|(owner, _)| *owner);
        let music: Vec<Option<SoundState>> = self
            .sounds
            .music
            .iter()
            .map(|channel| channel.as_ref().map(|m| state(m.id, &m.filename, None)))
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
        }
    }

    /// Loads a saved sound again and puts it where it was.
    fn restore_sound(&mut self, saved: &SoundState, kind: SoundType) -> Option<SoundId> {
        let (clip, _) = self.load_clip(&saved.filename)?;
        let audio = &mut self.sounds.audio;
        let id = audio.add(clip, kind, saved.private_volume);
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
                        event: s.event,
                    })
            });
            self.sounds.objects.insert(
                owner,
                ObjectSfx {
                    sound,
                    start_ms: saved.start_ms,
                    volume: saved.volume,
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
        if self.ui.frozen {
            self.sounds.audio.pause_all(state.music_frozen);
        }
    }

    /// What a loaded game needs beyond its state (`InitAfterLoad`): the
    /// frame's leftovers cleared, the sprites, images, fonts and cursors
    /// the state names loaded again, and the sounds started where they
    /// were. A video is not kept.
    fn after_restore(&mut self, sounds: SoundsState) {
        self.video = None;
        self.video_texture = None;
        self.video_uploaded = 0;
        self.hits.clear();
        self.sentences.clear();
        self.quit = false;
        self.scheduled_load = None;
        self.pending_save = None;
        self.reload_assets();
        self.restore_sounds(sounds);
    }

    /// Loads again every file the restored state names.
    fn reload_assets(&mut self) {
        if let Some(view) = self.scene.as_mut() {
            view.reload_sprites(&self.vfs);
        }
        for (_, actor) in &mut self.actors {
            actor.reload_sprites(&self.vfs);
        }
        let mut fonts: Vec<String> = Vec::new();
        fonts.extend(self.system_font.clone());
        fonts.extend(self.video_font.clone());
        let mut images: Vec<String> = Vec::new();
        let mut cursors: Vec<String> = Vec::new();
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
            fonts.extend(item.def.font.clone());
            cursors.extend(item.def.cursor.clone());
            cursors.extend(item.def.cursor_hover.clone());
        }
        for (_, actor) in &self.actors {
            fonts.extend(actor.def.font.clone());
            cursors.extend(actor.def.cursor.clone());
            fonts.extend(actor.sentence.as_ref().and_then(|s| s.font.clone()));
        }
        if let Some(view) = &self.scene {
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
        for font in fonts {
            self.load_font(&font);
        }
        for cursor in cursors {
            self.load_cursor(&cursor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
