//! The game world as scripts see it: the `Game` object, the current
//! scene and its parts, the string table, time, and the scripts attached
//! to everything. The [`World`] is the script engine's host; the
//! [`Runtime`] pairs it with an engine and drives both once per frame.
//!
//! Written with reference to `BGame.cpp`, `AdGame.cpp`, `AdScene.cpp` and
//! `BScriptHolder.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan
//! Nedoma); see `NOTICE`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use luma_archive::Vfs;
use luma_formats::{Entity, Rgba};
use luma_formats::{GameFile, Scene, StringTable};
use luma_script::classes::{ClassHost, ClassObject, DirEntry, relative_path};
use luma_script::{Context, Engine, Host, NativeId, Natives, Return, State, Value};

use crate::accessibility::{Accessibility, SpeechKind};
use crate::actor::{Actor, ActorEvent, WalkContext, scene_obstacles};
use crate::error::{Error, Result};
use crate::fader::{Fader, SCENE_FADE_MS};
use crate::input::{Cursors, HitArea, Keyboard, Mouse};
use crate::inventory::Inventory;
use crate::objects::{Kind, Native, Objects};
use crate::path::Floor;
use crate::scene_state::SceneState;
use crate::scene_view::{Camera, Drawable, Overlays, SceneView, SpriteBank};
use crate::settings::GameSettings;
use crate::sound::Sounds;
use crate::talk::{Sentence, SentenceDraw, SubtitleMods, TalkSkipButton};
use crate::text::FontBank;
use crate::ui::{Control, Ui};
use crate::video::{EntityVideo, VideoPlayer};
use luma_audio::Audio;
use luma_formats::font::TextAlign;

/// A short on-screen message (`Game.Msg`, `CBGame::QuickMessage`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickMessage {
    /// The text shown.
    pub text: String,
    /// When it was posted, on the game timer.
    pub start: u64,
    /// Its height in the system font, over the screen's width.
    pub height: i32,
}

/// An image shown while the game saves or loads (`SetSavingScreen`,
/// `SetLoadingScreen`, `SAVE_IMAGE` and `LOAD_IMAGE` of the game file).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScreenImage {
    /// The image or sprite file.
    pub path: String,
    /// Where it is drawn.
    pub x: i32,
    /// See `x`.
    pub y: i32,
}

/// The sprite `Game.DisplayLoadingIcon` shows (`m_LoadingIcon`).
#[derive(Debug, Clone, PartialEq)]
pub struct LoadingIcon {
    /// The sprite file.
    pub path: String,
    /// Where it is drawn.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Whether it stays until `HideLoadingIcon`; else it shows for one
    /// frame, as the original engine deletes it after its first display.
    pub persistent: bool,
    /// Whether a frame showed it since it was asked.
    pub shown: bool,
    /// Its animation.
    pub anim: crate::scene_view::AnimState,
}

/// How long a quick message stays (`QUICK_MSG_DURATION`).
pub const QUICK_MSG_MS: u64 = 3000;

/// How many quick messages show at once (`MAX_QUICK_MSG`).
pub const MAX_QUICK_MSGS: usize = 5;

/// The game as scripts see it. Implements the script engine's [`Host`].
pub struct World {
    pub(crate) vfs: Vfs,
    pub(crate) settings: GameSettings,
    pub(crate) game_file: GameFile,
    strings: StringTable,
    pub(crate) objects: Objects,
    pub(crate) game_id: NativeId,
    pub(crate) keyboard_id: NativeId,
    /// The `Math` object scripts see as a global.
    pub(crate) math_id: NativeId,
    /// The `Directory` object scripts see as a global (`CSXDirectory`);
    /// a saved game brings its own back with the engine's globals.
    pub(crate) directory_id: NativeId,
    /// `SCENE_VIEWPORT` of the game file or `Game.SetSceneViewport`:
    /// `(x, y, width, height)` on the screen; `None` is the whole screen.
    pub(crate) scene_viewport: Option<(i32, i32, i32, i32)>,
    pub(crate) scene_id: NativeId,
    pub(crate) scene: Option<SceneView>,
    /// The game's own entities (`Game.CreateEntity`, `Game.LoadEntity`,
    /// attachments), which survive scene changes, with their sprites and
    /// states; named by `EntityRef::Game`.
    pub(crate) game_view: SceneView,
    pub(crate) prev_scene_name: String,
    pub(crate) prev_scene_filename: String,
    pub(crate) scheduled_scene: Option<(String, bool)>,
    pub(crate) scene_states: BTreeMap<String, SceneState>,
    pub(crate) offset_x: i32,
    pub(crate) offset_y: i32,
    pub(crate) target_offset_x: i32,
    pub(crate) target_offset_y: i32,
    pub(crate) auto_scroll: bool,
    pub(crate) scroll_pixels_x: i32,
    pub(crate) scroll_pixels_y: i32,
    pub(crate) scroll_time_x: u64,
    pub(crate) scroll_time_y: u64,
    pub(crate) last_scroll_x: u64,
    pub(crate) last_scroll_y: u64,
    pub(crate) actors: Vec<(NativeId, Actor)>,
    /// The particle emitters of entities and actors, by handle.
    pub(crate) emitters: Vec<(NativeId, crate::particles::Emitter)>,
    pub(crate) pending_events: Vec<(NativeId, String)>,
    pub(crate) sounds: Sounds,
    pub(crate) fonts: FontBank,
    pub(crate) system_font: Option<String>,
    pub(crate) subtitles: bool,
    pub(crate) subtitles_speed: i32,
    pub(crate) talk_skip_button: TalkSkipButton,
    pub(crate) sentences: Vec<SentenceDraw>,
    pub(crate) mouse: Mouse,
    pub(crate) keyboard: Keyboard,
    pub(crate) active_object: Option<NativeId>,
    pub(crate) selected_item: Option<NativeId>,
    pub(crate) cursors: Cursors,
    pub(crate) hits: Vec<HitArea>,
    pub(crate) fader: Fader,
    pub(crate) fader_id: NativeId,
    /// The scene's fader (`Scene.FadeOut`), drawn over the scene
    /// viewport under the windows; reset with every scene.
    pub(crate) scene_fader: Fader,
    pub(crate) scene_fader_id: NativeId,
    pub(crate) ui: Ui,
    /// The dialogue response box and its state.
    pub(crate) responses: crate::responses::Responses,
    /// The object the script waiting in `GetResponse` waits for.
    pub(crate) response_box_id: NativeId,
    /// The entities shown by the windows' entity containers.
    pub(crate) ui_entities: Vec<crate::ui::UiEntity>,
    pub(crate) inventory: Inventory,
    pub(crate) video: Option<VideoPlayer>,
    /// The videos playing in entities' places, by entity.
    pub(crate) entity_videos: HashMap<NativeId, EntityVideo>,
    /// Textures of ended videos, forgotten at the next upload.
    pub(crate) dropped_textures: Vec<luma_render::Texture>,
    pub(crate) video_texture: Option<luma_render::Texture>,
    pub(crate) video_uploaded: u64,
    pub(crate) video_subtitles: bool,
    pub(crate) video_font: Option<String>,
    pub(crate) video_skip_button: TalkSkipButton,
    pub(crate) interactive: bool,
    pub(crate) main_object: Option<NativeId>,
    pub(crate) quit: bool,
    pub(crate) name: String,
    pub(crate) caption: String,
    /// `Game.TextEncoding`: 0 for ANSI, 1 for UTF-8; set by the string
    /// table loaded (a byte order mark means UTF-8) and by scripts.
    pub(crate) text_encoding: i32,
    /// `Game.TextRTL` (`CBGame::m_TextRTL`): right-to-left text, set by
    /// a string table's `@right-to-left` entry and by scripts; the
    /// fonts follow it.
    pub(crate) text_rtl: bool,
    /// `Game.StartupScene`, from `STARTUP_SCENE` of the game file.
    pub(crate) startup_scene: Option<String>,
    /// Whether the front-end runs the game in a window
    /// (`Game.WindowedMode`); false without a window.
    pub(crate) windowed: bool,
    /// `Game.SuspendedRendering` (`CBGame::m_SuspendedRendering`): while
    /// true the front-end keeps the last frame on screen instead of
    /// presenting new ones; the game runs on.
    pub(crate) suspended_rendering: bool,
    /// Frames drawn in the last whole second (`Game.FPS`), and the
    /// counters behind it (`m_Fps`, `m_FpsTime`, `m_FramesRendered`).
    pub(crate) fps: i32,
    pub(crate) fps_time: u64,
    pub(crate) fps_frames: i32,
    /// The messages of `Game.Msg` on screen, oldest first.
    pub(crate) quick_messages: Vec<QuickMessage>,
    /// Screenshots and thumbnails asked for, done with the next frame.
    pub(crate) captures: Vec<crate::capture::Capture>,
    /// The image shown while a saved game loads (`m_LoadImageName`).
    pub(crate) loading_screen: Option<ScreenImage>,
    /// The image shown while the game saves (`m_SaveImageName`).
    pub(crate) saving_screen: Option<ScreenImage>,
    /// The loading icon shown over the frame.
    pub(crate) loading_icon: Option<LoadingIcon>,
    /// The thumbnail the next saved game carries (PNG), from
    /// `Game.StoreSaveThumbnail`.
    pub(crate) save_thumbnail: Option<Vec<u8>>,
    /// `Game.LockMouseRect`: left, top, right, bottom; the mouse stays
    /// inside while all four are non-zero (`m_MouseLockRect`).
    pub(crate) mouse_lock: (i32, i32, i32, i32),
    /// `Game.Reset` was called: the scene left next is not remembered
    /// (`m_TempDisableSaveState`).
    pub(crate) temp_disable_save_state: bool,
    pub(crate) timer: u64,
    /// How much the timer advanced this frame (`m_TimerDelta`); 0 while
    /// frozen.
    pub(crate) timer_delta: u64,
    pub(crate) clock: u64,
    pub(crate) log: Vec<String>,
    stubs: HashSet<String>,
    pub(crate) rng: u64,
    /// The slot `Game.LoadGame` asked for, loaded at the next frame.
    pub(crate) scheduled_load: Option<i32>,
    /// What `Game.SaveGame` asked for, done once the call returned.
    pub(crate) pending_save: Option<(i32, String, bool)>,
    /// Saved games go to the user's own directory (`PERSONAL_SAVEGAMES`,
    /// or the game's directory is not writable).
    pub(crate) personal_save: bool,
    /// Whether the game's save directory was tried.
    pub(crate) save_dir_checked: bool,
    /// The accessibility switches and state (`CBAccessMgr`).
    pub(crate) access: Accessibility,
}

impl World {
    /// Opens a game: its settings, `default.game` and, if the game names
    /// one, its string table; sound goes to the default audio device, or
    /// nowhere when there is none.
    pub fn new(vfs: Vfs) -> Result<World> {
        World::with_audio(vfs, Audio::new())
    }

    /// Opens a game with a given audio system (`Audio::null()` for tests
    /// and silent runs).
    pub fn with_audio(vfs: Vfs, audio: Audio) -> Result<World> {
        let settings = GameSettings::load(&vfs)?;
        let bytes = vfs.read(&settings.game_file)?;
        let game_file = GameFile::parse(&bytes).map_err(|source| Error::Format {
            path: settings.game_file.clone(),
            source,
        })?;
        let mut objects = Objects::new();
        let game_id = objects.insert(Kind::Game);
        let keyboard_id = objects.insert(Kind::Keyboard);
        let math_id = objects.insert(Kind::Math);
        let directory_id = objects.insert(Kind::Class(ClassObject::directory()));
        let response_box_id = objects.insert(Kind::ResponseBox);
        let scene_id = objects.insert(Kind::Scene);
        let fader_id = objects.insert(Kind::Fader);
        let scene_fader_id = objects.insert(Kind::SceneFader);
        let personal_savegames = game_file.personal_savegames;
        let startup_scene = game_file.startup_scene.clone();
        let scene_viewport = game_file.scene_viewport.map(|[x, y, w, h]| (x, y, w, h));
        let mut world = World {
            vfs,
            settings,
            name: game_file.name.clone(),
            caption: game_file.caption.clone(),
            game_file,
            strings: StringTable::new(),
            objects,
            game_id,
            keyboard_id,
            math_id,
            directory_id,
            scene_viewport,
            scene_id,
            scene: None,
            game_view: SceneView::game_level(),
            prev_scene_name: String::new(),
            prev_scene_filename: String::new(),
            scheduled_scene: None,
            scene_states: BTreeMap::new(),
            offset_x: 0,
            offset_y: 0,
            target_offset_x: 0,
            target_offset_y: 0,
            auto_scroll: true,
            scroll_pixels_x: 1,
            scroll_pixels_y: 1,
            scroll_time_x: 10,
            scroll_time_y: 10,
            last_scroll_x: 0,
            last_scroll_y: 0,
            actors: Vec::new(),
            emitters: Vec::new(),
            pending_events: Vec::new(),
            sounds: Sounds::new(audio),
            fonts: FontBank::new(),
            system_font: None,
            subtitles: true,
            subtitles_speed: 70,
            talk_skip_button: TalkSkipButton::Left,
            sentences: Vec::new(),
            mouse: Mouse::default(),
            keyboard: Keyboard::default(),
            active_object: None,
            selected_item: None,
            cursors: Cursors::default(),
            hits: Vec::new(),
            fader: Fader::default(),
            fader_id,
            scene_fader: Fader::default(),
            scene_fader_id,
            ui: Ui::default(),
            responses: crate::responses::Responses::default(),
            response_box_id,
            ui_entities: Vec::new(),
            inventory: Inventory::default(),
            video: None,
            entity_videos: HashMap::new(),
            dropped_textures: Vec::new(),
            video_texture: None,
            video_uploaded: 0,
            video_subtitles: false,
            video_font: None,
            video_skip_button: TalkSkipButton::Left,
            interactive: true,
            main_object: None,
            quit: false,
            text_encoding: 0,
            text_rtl: false,
            startup_scene,
            windowed: false,
            suspended_rendering: false,
            fps: 0,
            fps_time: 0,
            fps_frames: 0,
            quick_messages: Vec::new(),
            captures: Vec::new(),
            loading_screen: None,
            saving_screen: None,
            loading_icon: None,
            save_thumbnail: None,
            mouse_lock: (0, 0, 0, 0),
            temp_disable_save_state: false,
            timer: 0,
            timer_delta: 0,
            clock: 0,
            log: Vec::new(),
            stubs: HashSet::new(),
            rng: 0x2545_F491_4F6C_DD1D,
            scheduled_load: None,
            pending_save: None,
            personal_save: personal_savegames,
            save_dir_checked: false,
            access: Accessibility::new(),
        };
        // The settings' table first, then the game file's on top, as the
        // original engine loads them.
        let mut first = true;
        if let Some(path) = world.settings.string_table.clone() {
            world.load_string_table(&path, true);
            first = false;
        }
        if let Some(path) = world.game_file.string_table.clone() {
            world.load_string_table(&path, first);
        }
        world.subtitles = world.game_file.subtitles;
        world.subtitles_speed = world.game_file.subtitles_speed;
        world.talk_skip_button = world
            .game_file
            .talk_skip_button
            .as_deref()
            .map_or(TalkSkipButton::Left, TalkSkipButton::parse);
        if let Some(path) = world.game_file.system_font.clone() {
            if world.load_font(&path) {
                world.system_font = Some(path);
            }
        }
        if let Some(path) = world.game_file.video_font.clone() {
            if world.load_font(&path) {
                world.video_font = Some(path);
            }
        }
        world.video_subtitles = world.game_file.video_subtitles;
        world.video_skip_button = world
            .game_file
            .video_skip_button
            .as_deref()
            .map_or(TalkSkipButton::Left, TalkSkipButton::parse);
        world.load_cursors();
        let (load, save) = (
            world.game_file.load_image.clone(),
            world.game_file.save_image.clone(),
        );
        let (lx, ly) = (world.game_file.load_image_x, world.game_file.load_image_y);
        let (sx, sy) = (world.game_file.save_image_x, world.game_file.save_image_y);
        world.set_loading_screen(load.as_deref(), lx, ly);
        world.set_saving_screen(save.as_deref(), sx, sy);
        Ok(world)
    }

    /// The game's files.
    pub fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    /// The game's settings.
    pub fn settings(&self) -> &GameSettings {
        &self.settings
    }

    /// The game definition.
    pub fn game_file(&self) -> &GameFile {
        &self.game_file
    }

    /// The string table.
    pub fn strings(&self) -> &StringTable {
        &self.strings
    }

    /// The current scene, if one is loaded.
    pub fn scene(&self) -> Option<&SceneView> {
        self.scene.as_ref()
    }

    /// The current scene, mutable.
    pub fn scene_mut(&mut self) -> Option<&mut SceneView> {
        self.scene.as_mut()
    }

    /// The scroll offset of the scene (`Scene.OffsetX`, `OffsetY`).
    pub fn offset(&self) -> (i32, i32) {
        (self.offset_x, self.offset_y)
    }

    /// The part of the screen the scene is shown in: the scene's own
    /// viewport (`VIEWPORT` in the scene file, `Scene.SetViewport`) when
    /// it has one, else the game's (`SCENE_VIEWPORT`,
    /// `Game.SetSceneViewport`), as `CAdScene::GetViewportSize` picks
    /// them: `(x, y, width, height)`, the whole screen by default.
    pub fn viewport(&self) -> (i32, i32, i32, i32) {
        if let Some(rect) = self.scene.as_ref().and_then(|v| v.scene.viewport) {
            return (
                rect.left,
                rect.top,
                (rect.right - rect.left).max(0),
                (rect.bottom - rect.top).max(0),
            );
        }
        self.scene_viewport.unwrap_or((
            0,
            0,
            self.settings.width as i32,
            self.settings.height as i32,
        ))
    }

    /// The camera showing the scene at its current offset, in the
    /// scene viewport.
    pub fn camera(&self) -> Camera {
        let (vx, vy, vw, vh) = self.viewport();
        let mut camera = Camera::new(vw as f32, vh as f32);
        camera.offset_x = self.offset_x as f32;
        camera.offset_y = self.offset_y as f32;
        camera.origin_x = vx as f32;
        camera.origin_y = vy as f32;
        camera
    }

    /// Whether the game accepts input (`Game.Interactive`).
    pub fn is_interactive(&self) -> bool {
        self.interactive
    }

    /// Whether a window holds the game (`GAME_SEMI_FROZEN`,
    /// `GAME_FROZEN`): an exclusive window is up or the response box
    /// waits for a pick. Clicks then reach the windows even while the
    /// game is not interactive.
    pub fn semi_frozen(&self) -> bool {
        self.responses_visible()
            || self.ui.controls.values().any(|c| {
                c.window()
                    .is_some_and(|w| w.mode != crate::ui::WindowMode::Normal)
            })
    }

    /// Whether an object takes mouse events while the game is not
    /// interactive (`m_NonIntMouseEvents`): the windows, their controls
    /// and the containers' entities do by default, the scene's objects
    /// do not; scripts change it with `NonIntMouseEvents`.
    pub(crate) fn non_int_mouse_events(&self, id: NativeId) -> bool {
        self.objects
            .get(id)
            .is_some_and(|n| n.state.non_int_mouse_events)
    }

    /// Whether a script asked to quit (`Game.QuitGame`).
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Whether a script suspended rendering (`Game.SuspendedRendering`):
    /// a front-end then keeps the frame on screen instead of presenting
    /// new ones, while the game keeps running. The original engine draws
    /// no sprite and skips the flip; headless runs ignore it.
    pub fn suspended_rendering(&self) -> bool {
        self.suspended_rendering
    }

    /// Whether text is laid out right to left (`Game.TextRTL`).
    pub fn text_rtl(&self) -> bool {
        self.text_rtl
    }

    /// Sets right-to-left text (`Game.TextRTL`, a string table's
    /// `@right-to-left`): the TrueType fonts lay their lines out from
    /// the last glyph and the editors mirror their keys.
    pub fn set_text_rtl(&mut self, rtl: bool) {
        self.text_rtl = rtl;
        self.fonts.set_rtl(rtl);
    }

    /// The scene a script asked for, not yet loaded.
    pub fn scheduled_scene(&self) -> Option<&str> {
        self.scheduled_scene.as_ref().map(|(p, _)| p.as_str())
    }

    /// The `Game` object's handle.
    pub fn game_id(&self) -> NativeId {
        self.game_id
    }

    /// The scene object's handle.
    pub fn scene_id(&self) -> NativeId {
        self.scene_id
    }

    /// The native objects.
    pub fn objects(&self) -> &Objects {
        &self.objects
    }

    /// The game timer, in milliseconds.
    pub fn timer(&self) -> u64 {
        self.timer
    }

    /// Advances the clocks by `dt` milliseconds, and counts the frame
    /// for `Game.FPS`: the frames of each whole second (`InitLoop`).
    pub fn advance_time(&mut self, dt: u64) {
        if self.ui.frozen {
            self.timer_delta = 0;
        } else {
            self.timer += dt;
            self.timer_delta = dt;
        }
        self.clock += dt;
        self.fps_time += dt;
        self.fps_frames += 1;
        if self.fps_time > 1000 {
            self.fps = self.fps_frames;
            self.fps_frames = 0;
            self.fps_time = 0;
        }
    }

    /// Frames drawn in the last whole second (`Game.FPS`).
    pub fn fps(&self) -> i32 {
        self.fps
    }

    /// Sets the image shown while a saved game loads
    /// (`Game.SetLoadingScreen`); `None` removes it. It is drawn over
    /// black during the frame between `Game.LoadGame` and the load.
    pub fn set_loading_screen(&mut self, path: Option<&str>, x: i32, y: i32) {
        self.loading_screen = path.map(|path| {
            self.load_ui_image(path);
            ScreenImage {
                path: path.to_string(),
                x,
                y,
            }
        });
    }

    /// Sets the image shown while the game saves (`Game.SetSavingScreen`);
    /// `None` removes it. A Luma save completes inside the frame that
    /// asks for it, so the image is kept but never on screen.
    pub fn set_saving_screen(&mut self, path: Option<&str>, x: i32, y: i32) {
        self.saving_screen = path.map(|path| {
            self.load_ui_image(path);
            ScreenImage {
                path: path.to_string(),
                x,
                y,
            }
        });
    }

    /// The loading screen, if set.
    pub fn loading_screen(&self) -> Option<&ScreenImage> {
        self.loading_screen.as_ref()
    }

    /// The saving screen, if set.
    pub fn saving_screen(&self) -> Option<&ScreenImage> {
        self.saving_screen.as_ref()
    }

    /// Shows a sprite over the frame (`Game.DisplayLoadingIcon`): for
    /// one frame, or until [`World::hide_loading_icon`] when
    /// `persistent`. Whether the sprite loaded.
    pub fn display_loading_icon(&mut self, path: &str, x: i32, y: i32, persistent: bool) -> bool {
        self.loading_icon = None;
        if !self.load_ui_image(path) {
            return false;
        }
        self.loading_icon = Some(LoadingIcon {
            path: path.to_string(),
            x,
            y,
            persistent,
            shown: false,
            anim: crate::scene_view::AnimState::new(path),
        });
        true
    }

    /// Removes the loading icon (`Game.HideLoadingIcon`).
    pub fn hide_loading_icon(&mut self) {
        self.loading_icon = None;
    }

    /// The loading icon shown, if any.
    pub fn loading_icon(&self) -> Option<&LoadingIcon> {
        self.loading_icon.as_ref()
    }

    /// Ends a frame for the loading icon: one that is not persistent
    /// goes after the frame that showed it (`CAdGame::DisplayContent`
    /// deletes it after its first display); a persistent one animates.
    pub(crate) fn update_loading_icon(&mut self) {
        let timer = self.timer;
        let Some(icon) = self.loading_icon.as_mut() else {
            return;
        };
        if icon.shown && !icon.persistent {
            self.loading_icon = None;
            return;
        }
        icon.shown = true;
        if let Some(loaded) = self.ui.bank.sprite(&icon.path) {
            icon.anim.step(&loaded.sprite, timer);
        }
    }

    /// Draws the loading screen over black, when a load is scheduled
    /// and the game has one (`DisplayIndicator`); whether it did.
    fn draw_loading_screen<'a>(
        &'a self,
        list: &mut luma_render::DrawList<'a>,
        white: Option<&'a luma_render::Texture>,
    ) -> bool {
        let (Some(screen), true) = (&self.loading_screen, self.scheduled_load.is_some()) else {
            return false;
        };
        if let Some(white) = white {
            list.fill(
                white,
                luma_render::Rect::new(0.0, 0.0, list.width, list.height),
                Rgba {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            );
        }
        self.draw_ui_image(
            list,
            &screen.path,
            0,
            screen.x,
            screen.y,
            Rgba::OPAQUE_WHITE,
        );
        true
    }

    /// Draws the loading icon at its current frame.
    fn draw_loading_icon<'a>(&'a self, list: &mut luma_render::DrawList<'a>) {
        if let Some(icon) = &self.loading_icon {
            self.draw_ui_image(
                list,
                &icon.path,
                icon.anim.current_frame,
                icon.x,
                icon.y,
                Rgba::OPAQUE_WHITE,
            );
        }
    }

    /// Posts a short message drawn over the frame for three seconds
    /// (`Game.Msg`, `CBGame::QuickMessage`); the oldest goes when five
    /// are showing. Nothing shows without a system font, as in the
    /// original engine.
    pub fn quick_message(&mut self, text: &str) {
        if self.quick_messages.len() >= MAX_QUICK_MSGS {
            self.quick_messages.remove(0);
        }
        let width = self.settings.width as i32;
        let height = self
            .system_font
            .clone()
            .and_then(|path| self.fonts.get_mut(&path))
            .map_or(0, |font| font.text_height(text, width));
        self.quick_messages.push(QuickMessage {
            text: text.to_string(),
            start: self.timer,
            height,
        });
    }

    /// The quick messages on screen, oldest first.
    pub fn quick_messages(&self) -> &[QuickMessage] {
        &self.quick_messages
    }

    /// Drops the quick messages past their time and lays out the rest
    /// (`DisplayQuickMsg`'s update half).
    pub(crate) fn update_quick_messages(&mut self) {
        let timer = self.timer;
        self.quick_messages
            .retain(|m| timer.saturating_sub(m.start) < QUICK_MSG_MS);
        let width = self.settings.width as i32;
        if let Some(font) = self
            .system_font
            .clone()
            .and_then(|p| self.fonts.get_mut(&p))
        {
            for message in &self.quick_messages {
                font.prepare(&message.text, width, TextAlign::Left, -1);
            }
        }
    }

    /// Draws the quick messages from `y` 20 down, in the system font,
    /// over everything else (`DisplayQuickMsg`).
    fn draw_quick_messages<'a>(&'a self, list: &mut luma_render::DrawList<'a>) {
        let Some(font) = self.system_font.as_ref().and_then(|p| self.fonts.get(p)) else {
            return;
        };
        let width = self.settings.width as i32;
        let mut y = 20;
        for message in &self.quick_messages {
            font.draw(list, &message.text, 0, y, width, TextAlign::Left, -1);
            y += message.height;
        }
    }

    /// Confines the mouse to a rectangle (`Game.LockMouseRect`): the
    /// edges are put in order, and a rectangle with a zero edge unlocks,
    /// since the original engine applies the lock only while all four
    /// are non-zero. The lock clamps the position the game sees; the
    /// system cursor is not moved.
    pub fn lock_mouse_rect(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
        let (left, right) = if right < left {
            (right, left)
        } else {
            (left, right)
        };
        let (top, bottom) = if bottom < top {
            (bottom, top)
        } else {
            (top, bottom)
        };
        self.mouse_lock = (left, top, right, bottom);
    }

    /// The mouse lock rectangle, if one applies.
    pub fn mouse_lock(&self) -> Option<(i32, i32, i32, i32)> {
        let (l, t, r, b) = self.mouse_lock;
        (l != 0 && t != 0 && r != 0 && b != 0).then_some((l, t, r, b))
    }

    /// Puts the game back to its start (`Game.Reset`,
    /// `CAdGame::ResetContent`): the dialogue branches, the inventory,
    /// the remembered scene states and the "once" responses are
    /// cleared, the items file is loaded again, the scene left next is
    /// not remembered, and the script globals that are not natives go
    /// back to `null`. The current scene stays, as in the original
    /// engine.
    pub fn reset_content(&mut self, engine: &mut Engine) {
        self.responses.branches.clear();
        self.inventory.taken.clear();
        self.selected_item = None;
        self.scene_states.clear();
        self.responses.used_branch.clear();
        self.responses.used_game.clear();
        self.load_items(engine);
        self.temp_disable_save_state = true;
        engine.clear_globals(false);
    }

    /// Tells the world whether it runs in a window (`Game.WindowedMode`).
    pub fn set_windowed(&mut self, windowed: bool) {
        self.windowed = windowed;
    }

    /// The scene a start loads (`Game.StartupScene`).
    pub fn startup_scene(&self) -> Option<&str> {
        self.startup_scene.as_deref()
    }

    /// Opens a file or an address with the system's default application
    /// (`Game.OpenDocument`, `ShellExecute("open")`): `open` on macOS,
    /// `xdg-open` elsewhere, `start` on Windows. Whether it was launched.
    pub fn open_document(&mut self, target: &str) -> bool {
        use std::process::{Command, Stdio};
        if target.trim().is_empty() {
            return false;
        }
        let mut command = if cfg!(target_os = "macos") {
            let mut c = Command::new("open");
            c.arg(target);
            c
        } else if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", "start", "", target]);
            c
        } else {
            let mut c = Command::new("xdg-open");
            c.arg(target);
            c
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.spawn() {
            Ok(_) => true,
            Err(err) => {
                self.log(format!("cannot open document '{target}': {err}"));
                false
            }
        }
    }

    /// The CRC-32 of a file of the game (`Game.GetFileChecksum`), `None`
    /// when there is no such file.
    pub fn file_checksum(&self, path: &str) -> Option<u32> {
        self.vfs
            .read(path)
            .ok()
            .map(|bytes| crc32fast::hash(&bytes))
    }

    /// The log lines written since the last call.
    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    /// Writes a log line.
    pub fn log(&mut self, message: impl Into<String>) {
        self.log.push(message.into());
    }

    /// Logs once that a feature is not implemented yet.
    pub(crate) fn stub(&mut self, what: &str) {
        if self.stubs.insert(what.to_string()) {
            self.log.push(format!("not implemented: {what} (ignored)"));
        }
    }

    /// Writes a log line the first time it comes up (the original engine
    /// repeats some warnings at every call).
    pub(crate) fn log_once(&mut self, message: &str) {
        if self.stubs.insert(message.to_string()) {
            self.log.push(message.to_string());
        }
    }

    /// Loads a string table (`Game.LoadStringTable`).
    pub fn load_string_table(&mut self, path: &str, clear_old: bool) -> bool {
        match self.vfs.read(path) {
            Ok(bytes) => {
                let count = self.strings.load(&bytes, clear_old);
                self.log(format!("string table '{path}': {count} strings"));
                // A byte order mark switches the game to UTF-8 text
                // (`CBStringTable::LoadFile`); any other table back to ANSI.
                self.text_encoding = i32::from(self.strings.utf8);
                // `@right-to-left` turns right-to-left text on
                // (`CBStringTable::AddString`); nothing turns it off.
                if self.strings.right_to_left {
                    self.set_text_rtl(true);
                }
                true
            }
            Err(err) => {
                self.log(format!("cannot load string table '{path}': {err}"));
                false
            }
        }
    }

    /// Resolves a `/ID/text` reference (`Game.ExpandString`).
    pub fn expand(&self, text: &str) -> String {
        self.strings.expand(text)
    }

    /// Asks for a scene change at the start of the next frame
    /// (`Game.ChangeScene`).
    pub fn schedule_scene(&mut self, path: &str, fade_in: bool) {
        self.scheduled_scene = Some((path.to_string(), fade_in));
    }

    /// Attaches a script to an object (`AttachScript`, `SCRIPT=`): the
    /// same file is not attached twice while it runs.
    pub fn attach(&mut self, engine: &mut Engine, owner: NativeId, path: &str) -> bool {
        let running = engine.scripts_of(owner).into_iter().any(|id| {
            engine.script(id).is_some_and(|s| {
                s.filename().eq_ignore_ascii_case(path) && s.state() != State::Finished
            })
        });
        if running {
            let name = self.describe(owner);
            self.log(format!(
                "trying to add script '{path}' multiple times (obj: '{name}')"
            ));
            return true;
        }
        let Some(script) = engine.run_script(self, path, Some(owner)) else {
            return false;
        };
        // A new script takes its owner's freezability (`AddScript`); only
        // a container's entity can be non-freezable in Luma.
        if let Some(Kind::Entity(crate::scene_view::EntityRef::Ui(index))) =
            self.objects.get(owner).map(|n| &n.kind)
        {
            let freezable = self.ui_entities.get(*index).is_none_or(|e| e.freezable);
            if let Some(script) = engine.script_mut(script) {
                script.set_freezable(freezable);
            }
        }
        true
    }

    /// A name for an object, for messages.
    pub fn describe_object(&self, id: NativeId) -> String {
        format!("{} '{}'", self.class_name(id), self.describe(id))
    }

    /// A name for an object, for messages.
    pub(crate) fn describe(&self, id: NativeId) -> String {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Game) => self.name.clone(),
            Some(Kind::Scene) => self
                .scene
                .as_ref()
                .map(|s| s.scene.name.clone())
                .unwrap_or_default(),
            Some(Kind::Entity(r)) => self
                .entity_def(*r)
                .map(|e| e.name.clone())
                .unwrap_or_default(),
            Some(Kind::Region { layer, index }) => self
                .scene
                .as_ref()
                .and_then(|s| s.region(*layer, *index))
                .map(|r| r.name.clone())
                .unwrap_or_default(),
            Some(Kind::Layer(i)) => self
                .scene
                .as_ref()
                .and_then(|s| s.scene.layers.get(*i))
                .map(|l| l.name.clone())
                .unwrap_or_default(),
            Some(Kind::Keyboard) => "keyboard".to_string(),
            Some(Kind::Math) => "math".to_string(),
            Some(Kind::ResponseBox) => "response box".to_string(),
            Some(
                Kind::Sprite(_)
                | Kind::HoverSprite(_)
                | Kind::CursorSprite { .. }
                | Kind::ControlImage { .. },
            ) => "sprite".to_string(),
            Some(Kind::EntityRegion(owner)) => self
                .region_ref(crate::natives_region::RegionRef::Entity(*owner))
                .map(|r| r.name.clone())
                .unwrap_or_default(),
            Some(Kind::WaypointGroup(i)) => self
                .scene
                .as_ref()
                .and_then(|s| s.scene.waypoint_groups.get(*i))
                .map(|g| g.name.clone())
                .unwrap_or_default(),
            Some(Kind::Frame { .. }) => "frame".to_string(),
            Some(Kind::SubFrame { .. }) => "subframe".to_string(),
            Some(Kind::Emitter(_)) => "particle emitter".to_string(),
            Some(Kind::Fader) | Some(Kind::SceneFader) => "fader".to_string(),
            Some(Kind::Actor) => self
                .actor(id)
                .map(|a| a.def.name.clone())
                .unwrap_or_default(),
            Some(Kind::Control) => self.control(id).map(|c| c.name.clone()).unwrap_or_default(),
            Some(Kind::Item) => self
                .item(id)
                .map(|i| i.def.name.clone())
                .unwrap_or_default(),
            Some(Kind::Inert(kind)) => kind.clone(),
            Some(Kind::Object) => self
                .objects
                .get(id)
                .and_then(|n| n.props.get("Name"))
                .map(|v| v.get_string(self))
                .unwrap_or_else(|| "<unnamed>".to_string()),
            Some(Kind::Class(object)) => object.type_name().to_string(),
            None => "invalid".to_string(),
        }
    }

    /// An actor by handle.
    pub fn actor(&self, id: NativeId) -> Option<&Actor> {
        self.actors.iter().find(|(i, _)| *i == id).map(|(_, a)| a)
    }

    /// An actor by handle, mutable.
    pub fn actor_mut(&mut self, id: NativeId) -> Option<&mut Actor> {
        self.actors
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, a)| a)
    }

    /// An actor, mutable, with the game's files, for loading sprites.
    pub(crate) fn actor_and_vfs(&mut self, id: NativeId) -> Option<(&Vfs, &mut Actor)> {
        let vfs = &self.vfs;
        let actor = self
            .actors
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, a)| a)?;
        Some((vfs, actor))
    }

    /// The handles of the actors, the game's then the scene's, in load
    /// order.
    pub fn actor_ids(&self) -> Vec<NativeId> {
        self.actors.iter().map(|(id, _)| *id).collect()
    }

    /// Loads an actor (`Game.LoadActor`, `Scene.LoadActor`) and attaches
    /// its scripts; `None` when the file cannot be loaded.
    pub fn load_actor(
        &mut self,
        engine: &mut Engine,
        path: &str,
        scene_bound: bool,
    ) -> Option<NativeId> {
        let mut actor = match Actor::load(&self.vfs, path, scene_bound) {
            Ok(actor) => actor,
            Err(err) => {
                self.log(format!("Error loading ACTOR file '{path}': {err}"));
                return None;
            }
        };
        actor.def.caption = self.expand(&actor.def.caption);
        for (file, reason) in actor.bank.missing() {
            self.log(format!("  missing: {file}: {reason}"));
        }
        let id = self.objects.insert(Kind::Actor);
        if let Some(native) = self.objects.get_mut(id) {
            for property in &actor.def.properties {
                native
                    .props
                    .insert(property.name.clone(), Value::from(property.value.clone()));
            }
        }
        let scripts = actor.def.scripts.clone();
        self.actors.push((id, actor));
        for script in scripts {
            self.attach(engine, id, &script);
        }
        Some(id)
    }

    /// An empty actor of the game (`new Actor()`): no file, no sprites,
    /// in no scene, kept across scene changes like `Game.LoadActor`'s.
    pub fn new_actor(&mut self) -> NativeId {
        let actor =
            Actor::from_definition(luma_formats::Actor::default(), "", false, SpriteBank::new());
        let id = self.objects.insert(Kind::Actor);
        self.actors.push((id, actor));
        id
    }

    /// Removes an object scripts created (`Game.UnloadObject`): an actor,
    /// a `new Object` or an inert object, with its scripts. Returns
    /// whether the handle named one.
    pub fn unload_object(&mut self, engine: &mut Engine, id: NativeId) -> bool {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor | Kind::Inert(_) | Kind::Object) => {}
            Some(Kind::Control) => return self.unload_control(engine, id),
            // A free entity is one of the scene's objects (`m_Objects`);
            // a layer node is not, and stays.
            Some(Kind::Entity(crate::scene_view::EntityRef::Free(index))) => {
                let index = *index;
                self.remove_free_entity(engine, id, index);
                return true;
            }
            Some(Kind::Entity(crate::scene_view::EntityRef::Game(index))) => {
                let index = *index;
                self.remove_game_entity(engine, id, index);
                return true;
            }
            _ => return false,
        }
        self.remove_attachments_of(engine, id);
        for script in engine.scripts_of(id) {
            engine.finish(script, true);
        }
        self.stop_sfx(id, true);
        self.sounds.objects.remove(&id);
        self.actors.retain(|(i, _)| *i != id);
        if self.main_object == Some(id) {
            self.main_object = None;
        }
        if self.inventory.owner == Some(id) {
            self.set_inventory_owner(None);
        }
        self.forget_owned(id);
        self.objects.remove(id);
        true
    }

    /// The floor an actor walks on: the scene, the polygons of the other
    /// active objects, and every waypoint but its own.
    fn obstacles_for(
        &self,
        id: NativeId,
    ) -> (Vec<Vec<luma_formats::Point>>, Vec<luma_formats::Point>) {
        let Some(view) = &self.scene else {
            return (Vec::new(), Vec::new());
        };
        let (mut blockers, mut waypoints) = scene_obstacles(&view.scene);
        for (other, actor) in &self.actors {
            if *other == id || !actor.def.active {
                continue;
            }
            if let Some(polygon) = actor.block_polygon(&view.scene) {
                blockers.push(polygon);
            }
            waypoints.extend(actor.waypoints(&view.scene));
        }
        (blockers, waypoints)
    }

    /// Runs a closure with the floor seen by an actor.
    pub(crate) fn with_floor<T>(
        &mut self,
        id: NativeId,
        f: impl FnOnce(&mut Actor, &Floor<'_>) -> T,
    ) -> Option<T> {
        let (blockers, waypoints) = self.obstacles_for(id);
        let empty = Scene::default();
        let scene = self.scene.as_ref().map_or(&empty, |v| &v.scene);
        let floor = Floor {
            scene,
            blockers: &blockers,
            waypoints: &waypoints,
        };
        let actor = self
            .actors
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, a)| a)?;
        Some(f(actor, &floor))
    }

    /// Queues an event an update raised, applied by
    /// [`World::apply_pending_events`].
    pub(crate) fn queue_events(&mut self, owner: NativeId, events: Vec<ActorEvent>) {
        for event in events {
            match event {
                ActorEvent::OnActor(name) => self.pending_events.push((owner, name.to_string())),
                ActorEvent::FrameShown { sprite, frame } => {
                    let frame = self
                        .actor(owner)
                        .and_then(|a| a.bank.sprite(&sprite))
                        .and_then(|l| l.sprite.frames.get(frame))
                        .cloned();
                    if let Some(frame) = frame {
                        self.frame_shown(owner, &frame);
                    }
                }
                ActorEvent::OnRegion {
                    layer,
                    index,
                    event,
                } => {
                    if let Some(region) = self.region_id(layer, index) {
                        self.pending_events.push((region, event.to_string()));
                    }
                }
            }
        }
    }

    /// Queues an event on an object, applied by
    /// [`World::apply_pending_events`].
    pub(crate) fn queue_event(&mut self, owner: NativeId, event: String) {
        self.pending_events.push((owner, event));
    }

    /// A frame of an object's sprite became current (`CBSprite::Display`
    /// with `m_Changed`): a `KILL_SOUND` frame stops the sprite's earlier
    /// frame sounds, `FrameChanged` is raised on the sprite object when a
    /// script holds one, then the frame's `SOUND` plays and its
    /// `APPLY_EVENT`s are raised on the object
    /// (`CBFrame::OneTimeDisplay`). The events reach their scripts with
    /// the others raised during the frame.
    pub(crate) fn frame_shown(&mut self, owner: NativeId, frame: &luma_formats::Frame) {
        self.frame_shown_in(owner, crate::ui::ImageSlot::Image, frame);
    }

    /// [`World::frame_shown`] for the sprite in a slot of a control: the
    /// `FrameChanged` goes to that slot's sprite object.
    pub(crate) fn frame_shown_in(
        &mut self,
        owner: NativeId,
        slot: crate::ui::ImageSlot,
        frame: &luma_formats::Frame,
    ) {
        if frame.kill_sound {
            self.kill_frame_sounds(owner);
        }
        let sprite = self.objects.find(|k| match k {
            Kind::Sprite(o) => *o == owner,
            Kind::ControlImage { control, slot: s } => *control == owner && *s == slot,
            _ => false,
        });
        if let Some(sprite) = sprite {
            self.queue_event(sprite, "FrameChanged".to_string());
        }
        if let Some(file) = &frame.sound {
            self.play_frame_sound(owner, file);
        }
        for event in &frame.apply_events {
            self.queue_event(owner, event.clone());
        }
    }

    /// One frame of every active actor (`CAdScene::UpdateFreeObjects`):
    /// walks, turns and animations progress, then the view follows the
    /// main object. The events raised are queued.
    pub fn update_actors(&mut self) {
        let timer = self.timer;
        for id in self.actor_ids() {
            if !self.actor(id).is_some_and(|a| a.def.active) {
                continue;
            }
            let mut events = Vec::new();
            let random = self.next_random();
            self.with_floor(id, |actor, floor| {
                let ctx = WalkContext {
                    floor: Floor {
                        scene: floor.scene,
                        blockers: floor.blockers,
                        waypoints: floor.waypoints,
                    },
                    timer,
                    random,
                };
                actor.update(&ctx, &mut events);
            });
            self.queue_events(id, events);
        }
        if self.auto_scroll {
            if let Some(main) = self.main_object.filter(|id| self.objects.contains(*id)) {
                if let Some((x, y)) = self.position_of(main) {
                    self.scroll_to(x, y);
                }
            }
        }
        self.collect_sentences();
    }

    /// One frame of the scene's entities and of the game's own:
    /// animations and sentences; the frames that became current play
    /// their sounds and raise their events (see [`World::frame_shown`]).
    pub fn update_scene(&mut self) {
        let timer = self.timer;
        let random = self.next_random();
        let mut shown = Vec::new();
        if let Some(view) = self.scene.as_mut() {
            view.advance_with(timer, random);
            shown.extend(view.take_shown());
        }
        let random = self.next_random();
        self.game_view.advance_with(timer, random);
        shown.extend(self.game_view.take_shown());
        for (r, sprite, index) in shown {
            let frame = self
                .view_of(r)
                .and_then(|v| v.sprite(&sprite))
                .and_then(|l| l.sprite.frames.get(index))
                .cloned();
            if let (Some(frame), Some(owner)) = (frame, self.node_id(r)) {
                self.frame_shown(owner, &frame);
            }
        }
    }

    /// The view an entity lives in: the scene's for its nodes and free
    /// entities, the game's for the game-level ones; none for a
    /// container's entity.
    pub(crate) fn view_of(&self, r: crate::scene_view::EntityRef) -> Option<&SceneView> {
        match r {
            crate::scene_view::EntityRef::Game(_) => Some(&self.game_view),
            crate::scene_view::EntityRef::Ui(_) => None,
            _ => self.scene.as_ref(),
        }
    }

    /// See [`World::view_of`].
    pub(crate) fn view_of_mut(
        &mut self,
        r: crate::scene_view::EntityRef,
    ) -> Option<&mut SceneView> {
        match r {
            crate::scene_view::EntityRef::Game(_) => Some(&mut self.game_view),
            crate::scene_view::EntityRef::Ui(_) => None,
            _ => self.scene.as_mut(),
        }
    }

    /// The view an entity lives in, mutable, with the game's files, for
    /// loading sprites into it.
    pub(crate) fn view_and_vfs(
        &mut self,
        r: crate::scene_view::EntityRef,
    ) -> (&Vfs, Option<&mut SceneView>) {
        let view = match r {
            crate::scene_view::EntityRef::Game(_) => Some(&mut self.game_view),
            crate::scene_view::EntityRef::Ui(_) => None,
            _ => self.scene.as_mut(),
        };
        (&self.vfs, view)
    }

    /// The game's own entities (`Game.CreateEntity`, `Game.LoadEntity`,
    /// attachments) with their states.
    pub fn game_view(&self) -> &SceneView {
        &self.game_view
    }

    /// Raises the queued events on their objects.
    pub fn apply_pending_events(&mut self, engine: &mut Engine) {
        for (owner, event) in std::mem::take(&mut self.pending_events) {
            if self.objects.contains(owner) {
                engine.apply_event(self, owner, &event, false);
            }
        }
    }

    /// Moves the view towards its target (`CAdScene::TraverseNodes`):
    /// `scroll_pixels` every `scroll_time` milliseconds, each axis on its
    /// own clock.
    pub fn scroll_step(&mut self) {
        if !self.auto_scroll {
            return;
        }
        if self.timer.saturating_sub(self.last_scroll_x) >= self.scroll_time_x {
            self.last_scroll_x = self.timer;
            if self.offset_x < self.target_offset_x {
                self.offset_x = (self.offset_x + self.scroll_pixels_x).min(self.target_offset_x);
            } else if self.offset_x > self.target_offset_x {
                self.offset_x = (self.offset_x - self.scroll_pixels_x).max(self.target_offset_x);
            }
        }
        if self.timer.saturating_sub(self.last_scroll_y) >= self.scroll_time_y {
            self.last_scroll_y = self.timer;
            if self.offset_y < self.target_offset_y {
                self.offset_y = (self.offset_y + self.scroll_pixels_y).min(self.target_offset_y);
            } else if self.offset_y > self.target_offset_y {
                self.offset_y = (self.offset_y - self.scroll_pixels_y).max(self.target_offset_y);
            }
        }
    }

    /// Whether the view has reached its scroll target.
    pub fn is_scrolled(&self) -> bool {
        !self.auto_scroll
            || (self.offset_x == self.target_offset_x && self.offset_y == self.target_offset_y)
    }

    /// The position of an object, raised by half its height for actors
    /// and entities, as the view centres on it (`ScrollToObject`).
    pub(crate) fn position_of(&self, id: NativeId) -> Option<(i32, i32)> {
        match &self.native(id)?.kind {
            Kind::Actor => {
                let actor = self.actor(id)?;
                let height = self.scene.as_ref().map_or(0, |v| actor.height(&v.scene));
                Some((actor.x, actor.y - height / 2))
            }
            Kind::Entity(r) => {
                let e = self.entity_def(*r)?;
                Some((e.x, e.y - self.entity_height(*r) / 2))
            }
            Kind::Region { layer, index } => {
                let r = self.scene()?.region(*layer, *index)?;
                let b = r.bounds()?;
                Some((b.left, b.top))
            }
            _ => None,
        }
    }

    /// The free objects as drawn this frame (see [`World::drawables`]) and
    /// the particles of the scene's objects, drawn right after their
    /// owner (`CAdEntity::Display`, `CAdActor::Display`: the emitter after
    /// the sprite, through the entity's region when the emitter uses it).
    pub fn scene_draw_parts(&self) -> (Vec<Drawable<'_>>, Overlays<'_>) {
        let Some(view) = &self.scene else {
            return (Vec::new(), Overlays::default());
        };
        let drawables = self.drawables();
        let mut overlays = Overlays::default();
        for (index, (id, _)) in self.free_objects().into_iter().enumerate() {
            let Some(emitter) = self.emitter_of(id).and_then(|e| self.emitter(e)) else {
                continue;
            };
            // A game entity's emitter uses the entity's region like a
            // scene entity's; an actor has none.
            let region = match self.objects.get(id).map(|n| &n.kind) {
                Some(Kind::Entity(r)) => self.entity_def(*r).and_then(|e| e.region.as_ref()),
                _ => None,
            };
            let quads = emitter.quads(region);
            if !quads.is_empty() {
                overlays.others.insert(index, quads);
            }
        }
        for (_, emitter) in &self.emitters {
            let Some(Kind::Entity(r)) = self.native(emitter.owner).map(|n| &n.kind) else {
                continue;
            };
            let Some(entity) = view.entity(*r) else {
                continue;
            };
            if !entity.active {
                continue;
            }
            let quads = emitter.quads(entity.region.as_ref());
            if !quads.is_empty() {
                overlays.entities.insert(*r, quads);
            }
        }
        overlays.videos = self.video_overlays();
        (drawables, overlays)
    }

    /// The free objects as drawn this frame, in the order of
    /// [`World::free_objects`]: the actors with their attachments, then
    /// the game's own entities, each with the region it sticks to.
    pub fn drawables(&self) -> Vec<Drawable<'_>> {
        let Some(view) = &self.scene else {
            return Vec::new();
        };
        self.free_objects()
            .into_iter()
            .map(|(id, object)| {
                let mut drawable = match self.objects.get(id).map(|n| &n.kind) {
                    Some(Kind::Actor) => {
                        let actor = self.actor(id).expect("a free actor");
                        let (transform, tint) =
                            (actor.transform(&view.scene), actor.tint(&view.scene));
                        let blend = actor.def.blend_mode;
                        let mut d = actor.drawable(&view.scene).unwrap_or(Drawable {
                            x: actor.x,
                            y: actor.y,
                            sprite: None,
                            bank: &actor.bank,
                            frame: 0,
                            transform,
                            tint,
                            blend,
                            stick: None,
                            before: Vec::new(),
                            after: Vec::new(),
                        });
                        let around =
                            self.attachment_draws(id, actor.x, actor.y, transform, tint, blend);
                        d.before = around.before;
                        d.after = around.after;
                        d
                    }
                    Some(Kind::Entity(r)) => {
                        // A game-level entity is scaled, turned and tinted
                        // by the scene it is shown in.
                        let entity = self.game_view.entity(*r).expect("a game entity");
                        let (transform, tint) = (
                            crate::scene_view::Transform::of_entity(&view.scene, entity),
                            view.scene.entity_tint(entity),
                        );
                        let sprite = self
                            .game_view
                            .shown_sprite(*r)
                            .and_then(|p| self.game_view.sprite(&p));
                        let around = self.attachment_draws(
                            id,
                            entity.x,
                            entity.y,
                            transform,
                            tint,
                            entity.blend_mode,
                        );
                        Drawable {
                            x: entity.x,
                            y: entity.y,
                            sprite,
                            bank: &self.game_view.bank,
                            frame: self.game_view.frame_of(*r),
                            transform,
                            tint,
                            blend: entity.blend_mode,
                            stick: None,
                            before: around.before,
                            after: around.after,
                        }
                    }
                    _ => unreachable!("free objects are actors and game entities"),
                };
                drawable.stick = object.stick;
                drawable
            })
            .collect()
    }

    /// The free objects of the game drawn among the scene's: the active
    /// actors (the game's then the scene's) and the game's active
    /// entities that are not attachments, with their handles, where they
    /// stand and the region they stick to.
    pub(crate) fn free_objects(&self) -> Vec<(NativeId, crate::scene_view::FreeObject)> {
        let mut objects: Vec<(NativeId, crate::scene_view::FreeObject)> = self
            .actors
            .iter()
            .filter(|(_, a)| a.def.active)
            .map(|(id, a)| {
                (
                    *id,
                    crate::scene_view::FreeObject {
                        x: a.x,
                        y: a.y,
                        stick: self.stick_region_of(*id),
                    },
                )
            })
            .collect();
        for r in self.game_view.entity_refs() {
            let Some(entity) = self.game_view.entity(r) else {
                continue;
            };
            let Some(id) = self.node_id(r) else { continue };
            if !entity.active
                || entity.kind != luma_formats::EntityKind::Normal
                || self
                    .object_state(id)
                    .is_some_and(|s| s.attached_to.is_some())
            {
                continue;
            }
            objects.push((
                id,
                crate::scene_view::FreeObject {
                    x: entity.x,
                    y: entity.y,
                    stick: self.stick_region_of(id),
                },
            ));
        }
        objects
    }

    /// The attachments of the scene's own entities, drawn around them.
    pub(crate) fn scene_attachments(
        &self,
    ) -> std::collections::HashMap<crate::scene_view::EntityRef, crate::scene_view::AttachedDraws<'_>>
    {
        let mut attached = std::collections::HashMap::new();
        let Some(view) = &self.scene else {
            return attached;
        };
        for (id, native) in self.objects.iter() {
            if native.state.attachments.is_empty() {
                continue;
            }
            let Kind::Entity(
                r @ (crate::scene_view::EntityRef::Node { .. }
                | crate::scene_view::EntityRef::Free(_)),
            ) = native.kind
            else {
                continue;
            };
            let Some(entity) = view.entity(r) else {
                continue;
            };
            attached.insert(
                r,
                self.attachment_draws(
                    id,
                    entity.x,
                    entity.y,
                    crate::scene_view::Transform::of_entity(&view.scene, entity),
                    view.scene.entity_tint(entity),
                    entity.blend_mode,
                ),
            );
        }
        attached
    }

    /// The region a free entity of the scene sticks to.
    pub(crate) fn free_entity_stick(&self, index: usize) -> Option<(usize, usize)> {
        let id = self.node_id(crate::scene_view::EntityRef::Free(index))?;
        self.stick_region_of(id)
    }

    /// The order the scene's objects are drawn in this frame (see
    /// [`SceneView::draw_order`]): the `Other` items are the active
    /// actors, in the order of [`World::actor_ids`], then the game's
    /// entities.
    pub fn draw_order(&self) -> Vec<crate::scene_view::DrawItem> {
        let Some(view) = &self.scene else {
            return Vec::new();
        };
        let positions: Vec<crate::scene_view::FreeObject> =
            self.free_objects().into_iter().map(|(_, o)| o).collect();
        view.draw_order(&positions, |i| self.free_entity_stick(i))
    }

    /// Fills a draw list with the frame as the engine composes it: the
    /// scene with the actors, the sentences (when subtitles are on), the
    /// cursor, and the fader's colour over everything. `white` is the
    /// renderer's white texture, for the fader; without it no fade is
    /// drawn.
    pub fn draw<'a>(
        &'a self,
        list: &mut luma_render::DrawList<'a>,
        camera: &Camera,
        white: Option<&'a luma_render::Texture>,
    ) {
        if self.draw_loading_screen(list, white) {
            return;
        }
        if self.is_video_playing() {
            // Only the video is drawn while it plays (`DisplayContent`).
            self.draw_video(list);
            return;
        }
        if let Some(view) = &self.scene {
            let (drawables, overlays) = self.scene_draw_parts();
            view.draw_with_overlays(
                list,
                camera,
                &drawables,
                &self.scene_attachments(),
                &overlays,
                |i| self.free_entity_stick(i),
            );
            // The scene's fader covers the scene viewport, under the
            // windows (`CAdScene::TraverseNodes` displays it last).
            if let Some(white) = white {
                self.draw_scene_fade(list, white);
            }
        }
        self.draw_windows(list, true, white);
        self.draw_inventory(list, white);
        self.draw_windows(list, false, white);
        self.draw_responses(list, white);
        if let Some(white) = white {
            self.draw_access_hint(list, white);
        }
        if self.subtitles {
            for draw in &self.sentences {
                if let Some(font) = self.fonts.get(&draw.font) {
                    font.draw(list, &draw.text, draw.x, draw.y, draw.width, draw.align, -1);
                }
            }
        }
        self.draw_cursor(list);
        if let Some(white) = white {
            self.draw_fade(list, white);
        }
        self.draw_loading_icon(list);
        self.draw_quick_messages(list);
    }

    /// Uploads the textures of the scene, of the actors, of the cursors
    /// and of the fonts.
    pub fn upload(&mut self, renderer: &luma_render::Renderer) {
        if let Some(view) = self.scene.as_mut() {
            view.upload(&self.vfs, renderer);
        }
        self.game_view.upload(&self.vfs, renderer);
        for (_, actor) in &mut self.actors {
            actor.bank.upload(&self.vfs, renderer);
        }
        for (_, emitter) in &mut self.emitters {
            emitter.bank.upload(&self.vfs, renderer);
        }
        self.cursors.bank.upload(&self.vfs, renderer);
        self.ui.bank.upload(&self.vfs, renderer);
        self.upload_video(renderer);
        self.fonts.upload(renderer);
    }

    /// Loads a scene, replacing the current one (`CAdGame::ChangeScene`):
    /// the old scene gets `SceneShutdown`, its scripts and objects go, the
    /// new scene's scripts and its nodes' scripts are attached.
    pub fn load_scene(&mut self, engine: &mut Engine, path: &str) -> Result<()> {
        if let Some(old) = &self.scene {
            let (name, filename) = (old.scene.name.clone(), old.path.clone());
            let scene_id = self.scene_id;
            engine.apply_event(self, scene_id, "SceneShutdown", true);
            self.prev_scene_name = name;
            self.prev_scene_filename = filename;
            if !self.temp_disable_save_state {
                self.save_scene_state();
            }
            self.temp_disable_save_state = false;
        }
        // Objects of the old scene, and the scripts attached to them.
        let scene_actors: Vec<NativeId> = self
            .actors
            .iter()
            .filter(|(_, a)| a.scene_bound)
            .map(|(id, _)| *id)
            .collect();
        // The windows' container entities live outside the scene.
        // The windows' container entities and the game's own live outside
        // the scene; the scene's objects are its nodes, free entities and
        // the actors it loaded.
        let is_scene_object = |id: NativeId| {
            matches!(
                self.objects.get(id).map(|n| &n.kind),
                Some(Kind::Entity(
                    crate::scene_view::EntityRef::Node { .. }
                        | crate::scene_view::EntityRef::Free(_)
                ))
            ) || scene_actors.contains(&id)
        };
        let gone: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(id, n)| match n.kind {
                Kind::Layer(_) | Kind::Region { .. } | Kind::WaypointGroup(_) => true,
                Kind::Entity(_) | Kind::Actor => is_scene_object(*id),
                // The sprite, cursor and region objects of the scene's
                // objects, their frames and sub-frames go with them.
                Kind::Sprite(owner)
                | Kind::HoverSprite(owner)
                | Kind::CursorSprite { owner, .. }
                | Kind::EntityRegion(owner) => is_scene_object(owner),
                Kind::ControlImage { .. } => false,
                Kind::Frame { sprite, .. } | Kind::SubFrame { sprite, .. } => {
                    match self.objects.get(sprite).map(|n| &n.kind) {
                        Some(Kind::Sprite(owner)) => is_scene_object(*owner),
                        Some(Kind::ControlImage { .. }) => false,
                        _ => true,
                    }
                }
                // The particle emitters of the scene's objects go too.
                Kind::Emitter(owner) => is_scene_object(owner),
                _ => false,
            })
            .map(|(id, _)| id)
            .collect();
        self.actors.retain(|(id, _)| !scene_actors.contains(id));
        if self
            .main_object
            .is_some_and(|id| scene_actors.contains(&id))
        {
            self.main_object = None;
        }
        // The game's actors and entities are reset before the new scene
        // loads (`CAdGame::ChangeScene`).
        for (id, actor) in &mut self.actors {
            actor.reset();
            engine.reset_object(*id);
        }
        for r in self.game_view.entity_refs() {
            self.game_view.reset_anim(r);
            self.game_view.stop_talk(r);
            if let Some(id) = self.node_id(r) {
                engine.reset_object(id);
            }
        }
        self.pending_events.clear();
        for id in gone.iter().copied().chain([self.scene_id]) {
            for script in engine.scripts_of(id) {
                engine.finish(script, true);
            }
        }
        // The attachments of the scene's objects go with them, and the
        // inventory box goes back to the game's own when it showed one of
        // theirs.
        for id in gone.clone() {
            self.remove_attachments_of(engine, id);
            if self.inventory.owner == Some(id) {
                self.set_inventory_owner(None);
            }
        }
        for id in gone {
            self.objects.remove(id);
        }
        self.emitters.retain(|(id, _)| self.objects.contains(*id));
        self.drop_orphan_sounds();
        if let Some(scene) = self.objects.get_mut(self.scene_id) {
            scene.props.clear();
        }
        engine.remove_finished();

        let mut view = SceneView::load(&self.vfs, path)?;
        // Captions go through the string table when the objects load
        // (`CBObject::SetCaption`).
        for layer in &mut view.scene.layers {
            layer.caption = self.strings.expand(&layer.caption);
            for node in &mut layer.nodes {
                match node {
                    luma_formats::SceneNode::Entity(e) => {
                        e.caption = self.strings.expand(&e.caption)
                    }
                    luma_formats::SceneNode::Region(r) => {
                        r.caption = self.strings.expand(&r.caption)
                    }
                }
            }
        }
        for e in &mut view.scene.free_entities {
            e.caption = self.strings.expand(&e.caption);
        }
        self.log(format!(
            "scene '{}' loaded from '{path}': {} layer(s), {} entities",
            view.scene.name,
            view.scene.layers.len(),
            view.scene.entities().count()
        ));
        for (file, reason) in view.missing() {
            self.log(format!("  missing: {file}: {reason}"));
        }
        self.offset_x = 0;
        self.offset_y = 0;
        self.target_offset_x = 0;
        self.target_offset_y = 0;
        self.active_object = None;
        self.hits.clear();
        self.scene = Some(view);
        // Every scene has its own fader (`CAdScene::m_Fader`).
        self.scene_fader = Fader::default();
        self.restore_scene_state();
        // The regions the game's objects stuck to went with the old scene
        // (`CAdGame::ChangeScene`).
        for (_, slot) in self.objects.iter_mut() {
            slot.state.stick_region = None;
        }
        self.hide_taken_item_entities();
        for (_, actor) in &mut self.actors {
            actor.invalidate_regions();
        }

        // Attach scripts in file order: the scene's, then each layer's and
        // its nodes', then the free entities'.
        let scene_scripts = self
            .scene()
            .map(|s| s.scene.scripts.clone())
            .unwrap_or_default();
        for script in scene_scripts {
            self.attach(engine, self.scene_id, &script);
        }
        let layers = self.scene().map(|s| s.scene.layers.len()).unwrap_or(0);
        for li in 0..layers {
            let layer_id = self.objects.insert(Kind::Layer(li));
            let (layer_scripts, node_count) = {
                let layer = &self.scene().unwrap().scene.layers[li];
                (layer.scripts.clone(), layer.nodes.len())
            };
            for script in layer_scripts {
                self.attach(engine, layer_id, &script);
            }
            for ni in 0..node_count {
                let (kind, scripts) = {
                    match &self.scene().unwrap().scene.layers[li].nodes[ni] {
                        luma_formats::SceneNode::Entity(e) => (
                            Kind::Entity(crate::scene_view::EntityRef::Node {
                                layer: li,
                                index: ni,
                            }),
                            e.scripts.clone(),
                        ),
                        luma_formats::SceneNode::Region(r) => (
                            Kind::Region {
                                layer: li,
                                index: ni,
                            },
                            r.scripts.clone(),
                        ),
                    }
                };
                let id = self.objects.insert(kind);
                self.load_entity_sound(id);
                self.register_entity_region(id);
                for script in scripts {
                    self.attach(engine, id, &script);
                }
            }
        }
        let free = self
            .scene()
            .map(|s| s.scene.free_entities.len())
            .unwrap_or(0);
        for i in 0..free {
            let id = self
                .objects
                .insert(Kind::Entity(crate::scene_view::EntityRef::Free(i)));
            self.load_entity_sound(id);
            self.register_entity_region(id);
            let scripts = self.scene().unwrap().scene.free_entities[i].scripts.clone();
            for script in scripts {
                self.attach(engine, id, &script);
            }
        }
        Ok(())
    }

    /// Loads the `SOUND` of an entity without playing it (`CAdEntity`
    /// reads `SOUND`, `SOUND_START_TIME` and `SOUND_VOLUME`).
    pub(crate) fn load_entity_sound(&mut self, id: NativeId) {
        let Some(Kind::Entity(r)) = self.objects.get(id).map(|n| n.kind.clone()) else {
            return;
        };
        let Some((sound, start, volume)) = self
            .scene()
            .and_then(|v| v.entity(r))
            .map(|e| (e.sound.clone(), e.sound_start_ms, e.sound_volume))
        else {
            return;
        };
        if let Some(sound) = sound {
            self.set_sfx_time(id, start.max(0) as u64);
            self.set_sfx_volume(id, volume);
            self.play_sfx(id, Some(&sound), false, false, None, 0);
        }
    }

    /// The screen fader.
    pub fn fader(&self) -> &Fader {
        &self.fader
    }

    /// The screen fader, mutable.
    pub fn fader_mut(&mut self) -> &mut Fader {
        &mut self.fader
    }

    /// The fader's handle, for scripts to wait on.
    pub fn fader_id(&self) -> NativeId {
        self.fader_id
    }

    /// The current scene's fader (`Scene.FadeOut`), over the scene only.
    pub fn scene_fader(&self) -> &Fader {
        &self.scene_fader
    }

    /// The fonts loaded.
    pub fn fonts(&self) -> &FontBank {
        &self.fonts
    }

    /// The fonts loaded, mutable.
    pub fn fonts_mut(&mut self) -> &mut FontBank {
        &mut self.fonts
    }

    /// Loads a font (`CBFontStorage::AddFont`), logging a failure once.
    pub fn load_font(&mut self, path: &str) -> bool {
        let ok = self.fonts.load(&self.vfs, path);
        for (file, reason) in self.fonts.take_missing() {
            self.log(format!("Error loading font '{file}': {reason}"));
        }
        ok
    }

    /// The game's `SYSTEM_FONT`, when it loaded.
    pub fn system_font(&self) -> Option<&str> {
        self.system_font.as_deref()
    }

    /// Whether speech is subtitled (`Game.Subtitles`).
    pub fn subtitles_enabled(&self) -> bool {
        self.subtitles
    }

    /// The sentences shown this frame, in screen coordinates.
    pub fn sentences(&self) -> &[SentenceDraw] {
        &self.sentences
    }

    /// A random number, from the same generator as the scripts'.
    pub(crate) fn next_random(&mut self) -> u32 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as u32
    }

    /// The font an object talks with: its own, else the system font.
    pub fn font_of(&self, id: NativeId) -> Option<String> {
        let own = match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor) => self.actor(id).and_then(|a| a.def.font.clone()),
            Some(Kind::Entity(r)) => self.entity_def(*r).and_then(|e| e.font.clone()),
            _ => None,
        };
        own.filter(|f| self.fonts.get(f).is_some())
            .or_else(|| self.system_font.clone())
    }

    /// Sets an object's font (`SetFont`); `None` restores the system
    /// font. Returns whether the font loaded.
    pub fn set_font(&mut self, id: NativeId, path: Option<&str>) -> bool {
        let loaded = match path {
            Some(path) => self.load_font(path),
            None => true,
        };
        let value = if loaded {
            path.map(str::to_string)
        } else {
            None
        };
        match self.objects.get(id).map(|n| n.kind.clone()) {
            Some(Kind::Actor) => {
                if let Some(actor) = self.actor_mut(id) {
                    actor.def.font = value;
                }
            }
            Some(Kind::Entity(r)) => {
                if let Some(entity) = self.entity_def_mut(r) {
                    entity.font = value;
                }
            }
            _ => return false,
        }
        loaded
    }

    /// The subtitle placement of an object.
    pub fn subtitle_mods(&self, id: NativeId) -> SubtitleMods {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor) => self.actor(id).map(|a| a.subtitles).unwrap_or_default(),
            Some(Kind::Entity(r)) => self
                .view_of(*r)
                .and_then(|v| v.subtitles.get(r).copied())
                .unwrap_or_default(),
            _ => SubtitleMods::default(),
        }
    }

    /// Changes the subtitle placement of an object.
    pub fn set_subtitle_mods(&mut self, id: NativeId, mods: SubtitleMods) {
        match self.objects.get(id).map(|n| n.kind.clone()) {
            Some(Kind::Actor) => {
                if let Some(actor) = self.actor_mut(id) {
                    actor.subtitles = mods;
                }
            }
            Some(Kind::Entity(r)) => {
                if let Some(view) = self.view_of_mut(r) {
                    view.subtitles.insert(r, mods);
                }
            }
            _ => {}
        }
    }

    /// The height of an object (`GetHeight`).
    fn object_height(&self, id: NativeId) -> i32 {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor) => match (self.actor(id), &self.scene) {
                (Some(actor), Some(view)) => actor.height(&view.scene),
                _ => 0,
            },
            Some(Kind::Entity(r)) => self.entity_height(*r),
            _ => 0,
        }
    }

    /// Makes an object say a sentence (`CAdObject::Talk`): the text is
    /// expanded, the duration comes from the text when none is given,
    /// and the text box is placed above the object. Returns whether the
    /// object can talk.
    pub fn talk(
        &mut self,
        id: NativeId,
        text: &str,
        sound_file: Option<&str>,
        duration: i32,
        stances: Option<String>,
        align: TextAlign,
    ) -> bool {
        let kind = match self.objects.get(id).map(|n| n.kind.clone()) {
            Some(kind @ (Kind::Actor | Kind::Entity(_))) => kind,
            _ => return false,
        };
        // End the sentence being said, with its sound.
        self.stop_talk(id);
        self.stop_speech();
        let raw = text.to_string();
        let text = self.expand(text);
        let font = self.font_of(id);
        let mut duration = u64::try_from(duration).unwrap_or(0);
        // The speech file: the one given, else the one named after the
        // string identifier in a speech directory.
        let speech_file = match sound_file {
            Some(file) => Some(file.to_string()),
            None => self
                .strings
                .key_of(&raw)
                .and_then(|key| self.find_speech_file(&key)),
        };
        let mut sound = None;
        if let Some(file) = speech_file {
            if let Some((id, length)) = self.start_speech(&file) {
                sound = Some(id);
                if duration == 0 && length != 0 {
                    duration = length;
                }
            }
        }
        if duration == 0 {
            let length = text.chars().count() as u64;
            duration = (self.subtitles_speed.max(0) as u64 * length).max(1000);
        }
        // Without a speech file, the text-to-speech reads the sentence.
        if sound.is_none() {
            self.access_speak(&text, SpeechKind::Talk);
        }
        let mods = self.subtitle_mods(id);
        let (sw, sh) = (self.settings.width as i32, self.settings.height as i32);
        let (mut x, mut y) = match &kind {
            Kind::Actor => self.actor(id).map_or((0, 0), |a| (a.x, a.y)),
            Kind::Entity(r) => self.entity_def(*r).map_or((0, 0), |e| (e.x, e.y)),
            _ => (0, 0),
        };
        // A scene-independent object is placed on the screen already.
        let scene_independent = self.object_state(id).is_some_and(|s| s.scene_independent);
        if mods.relative && !scene_independent {
            x -= self.offset_x;
            y -= self.offset_y;
        }
        let width = if mods.width > 0 {
            mods.width
        } else if x < sw / 4 || f64::from(x) > f64::from(sw) * 0.75 {
            (sw / 4).max((x * 2).min((sw - x) * 2))
        } else {
            sw / 2
        };
        let height = match font.as_deref().and_then(|f| self.fonts.get_mut(f)) {
            Some(font) => font.text_height(&text, width),
            None => 0,
        };
        y = y - height - self.object_height(id) - 5;
        if mods.relative {
            x += mods.x;
            y += mods.y;
        } else {
            x = mods.x;
            y = mods.y;
        }
        if mods.x_center {
            x -= width / 2;
        }
        x = x.max(0).min(sw - width);
        y = y.max(0).min(sh - height);
        if mods.relative {
            x += self.offset_x;
            y += self.offset_y;
        }
        let sentence = Sentence {
            text,
            stances,
            current_stance: -1,
            duration,
            start_time: self.timer,
            font,
            x,
            y,
            width,
            align,
            fixed_pos: !mods.relative,
            sound,
        };
        let forced = self.forced_talk_sprite(id);
        match kind {
            Kind::Actor => {
                let forced = forced.filter(|path| {
                    self.actor_and_vfs(id)
                        .is_some_and(|(vfs, actor)| actor.bank.add_sprite(vfs, path))
                });
                if let Some(actor) = self.actor_mut(id) {
                    actor.talk(sentence, forced);
                }
            }
            Kind::Entity(r) => {
                let sprites: Vec<String> = self
                    .entity_def(r)
                    .map(|e| {
                        e.talk_sprites
                            .iter()
                            .chain(&e.talk_special_sprites)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                let vfs = &self.vfs;
                let view = match r {
                    crate::scene_view::EntityRef::Game(_) => Some(&mut self.game_view),
                    crate::scene_view::EntityRef::Ui(_) => None,
                    _ => self.scene.as_mut(),
                };
                if let Some(view) = view {
                    for sprite in sprites {
                        view.add_sprite(vfs, &sprite);
                    }
                    let forced = forced.filter(|path| view.add_sprite(vfs, path));
                    view.talk(r, sentence, forced);
                }
            }
            _ => {}
        }
        true
    }

    /// The sprite `ForceTalkAnim` named for the sentence starting on an
    /// object (`CAdTalkHolder::GetTalkStance`): the name is used once; a
    /// used one is forgotten when the next sentence starts
    /// (`CAdObject::Talk`). A file that does not load is logged and
    /// dropped.
    fn forced_talk_sprite(&mut self, id: NativeId) -> Option<String> {
        let state = self.object_state_mut(id)?;
        if state.forced_talk_used {
            state.forced_talk_anim = None;
            state.forced_talk_used = false;
        }
        let path = state.forced_talk_anim.clone()?;
        state.forced_talk_used = true;
        let loaded = match self.objects.get(id).map(|n| n.kind.clone()) {
            Some(Kind::Actor) => self
                .actor_and_vfs(id)
                .is_some_and(|(vfs, actor)| actor.bank.add_sprite(vfs, &path)),
            Some(Kind::Entity(r)) => {
                let vfs = &self.vfs;
                match r {
                    crate::scene_view::EntityRef::Game(_) => self.game_view.add_sprite(vfs, &path),
                    crate::scene_view::EntityRef::Ui(_) => false,
                    _ => self
                        .scene
                        .as_mut()
                        .is_some_and(|view| view.add_sprite(vfs, &path)),
                }
            }
            _ => false,
        };
        if !loaded {
            let who = self.describe(id);
            self.log(format!(
                "CAdTalkHolder::GetTalkStance: error loading talk sprite (object:\"{who}\" sprite:\"{path}\")"
            ));
            return None;
        }
        Some(path)
    }

    /// Ends an object's sentence (`StopTalk`); whether one was playing.
    pub fn stop_talk(&mut self, id: NativeId) -> bool {
        match self.objects.get(id).map(|n| n.kind.clone()) {
            Some(Kind::Actor) => self.actor_mut(id).is_some_and(|a| a.stop_talk()),
            Some(Kind::Entity(r)) => self.view_of_mut(r).is_some_and(|v| v.stop_talk(r)),
            _ => false,
        }
    }

    /// Whether an object is saying a sentence.
    pub fn is_talking(&self, id: NativeId) -> bool {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor) => self.actor(id).is_some_and(|a| a.is_talking()),
            Some(Kind::Entity(r)) => self.view_of(*r).is_some_and(|v| v.is_talking(*r)),
            _ => false,
        }
    }

    /// Ends every sentence older than 300 ms, as a click does while the
    /// game is not interactive (`CAdGame::FinishSentences`).
    pub fn skip_sentences(&mut self) {
        let timer = self.timer;
        for (_, actor) in &mut self.actors {
            if let Some(sentence) = &mut actor.sentence {
                if sentence.can_skip(timer) {
                    sentence.finish();
                }
            }
        }
        if let Some(view) = self.scene.as_mut() {
            view.skip_sentences(timer);
        }
        self.game_view.skip_sentences(timer);
        self.stop_speech();
        self.access.stop();
    }

    /// A left or right click while the game is not interactive skips
    /// the sentences, per `TALK_SKIP_BUTTON`.
    pub fn click_while_busy(&mut self, right: bool) {
        let skips = if right {
            self.talk_skip_button.skips_on_right()
        } else {
            self.talk_skip_button.skips_on_left()
        };
        if skips && !self.interactive {
            self.skip_sentences();
        }
    }

    /// Gathers the sentences shown this frame and lays their text out.
    fn collect_sentences(&mut self) {
        let (ox, oy, sw) = (self.offset_x, self.offset_y, self.settings.width as i32);
        let mut draws = Vec::new();
        for (_, actor) in &self.actors {
            if actor.show_sentence {
                if let Some(draw) = actor
                    .sentence
                    .as_ref()
                    .and_then(|s| SentenceDraw::of(s, ox, oy, sw))
                {
                    draws.push(draw);
                }
            }
        }
        let views = self.scene.iter().chain([&self.game_view]);
        for view in views {
            for (_, sentence) in view.sentences() {
                if let Some(draw) = SentenceDraw::of(sentence, ox, oy, sw) {
                    draws.push(draw);
                }
            }
        }
        for draw in &draws {
            if let Some(font) = self.fonts.get_mut(&draw.font) {
                font.prepare(&draw.text, draw.width, draw.align, -1);
            }
        }
        self.sentences = draws;
    }

    /// The object the view follows (`Game.MainObject`).
    pub fn main_object(&self) -> Option<NativeId> {
        self.main_object.filter(|id| self.objects.contains(*id))
    }

    /// Moves an actor to a point (`SkipTo`), raising the region events.
    pub fn place_actor(&mut self, id: NativeId, x: i32, y: i32) {
        let mut events = Vec::new();
        let scene = self.scene().map(|v| v.scene.clone());
        match (scene, self.actor_mut(id)) {
            (Some(scene), Some(actor)) => actor.skip_to(x, y, &scene, &mut events),
            (None, Some(actor)) => {
                actor.x = x;
                actor.y = y;
            }
            _ => {}
        }
        self.queue_events(id, events);
    }

    /// An entity's definition: a scene node, a free entity or a window
    /// container's entity.
    pub(crate) fn entity_def(&self, r: crate::scene_view::EntityRef) -> Option<&Entity> {
        match r {
            crate::scene_view::EntityRef::Ui(i) => self.ui_entities.get(i).map(|e| &e.def),
            r => self.view_of(r).and_then(|v| v.entity(r)),
        }
    }

    /// See [`World::entity_def`].
    pub(crate) fn entity_def_mut(
        &mut self,
        r: crate::scene_view::EntityRef,
    ) -> Option<&mut Entity> {
        match r {
            crate::scene_view::EntityRef::Ui(i) => self.ui_entities.get_mut(i).map(|e| &mut e.def),
            r => self.view_of_mut(r).and_then(|v| v.entity_mut(r)),
        }
    }

    /// The handle of a scene node, free entity or container entity.
    pub fn node_id(&self, r: crate::scene_view::EntityRef) -> Option<NativeId> {
        self.objects.find(|k| *k == Kind::Entity(r))
    }

    /// The handle of a region node.
    pub fn region_id(&self, layer: usize, index: usize) -> Option<NativeId> {
        self.objects.find(|k| *k == Kind::Region { layer, index })
    }

    /// The handle of a layer.
    pub fn layer_id(&self, index: usize) -> Option<NativeId> {
        self.objects.find(|k| *k == Kind::Layer(index))
    }

    /// The handle of the node with this name, as `Scene.GetNode` finds it:
    /// layer nodes first, then free entities.
    pub fn find_node(&self, name: &str) -> Option<NativeId> {
        let view = self.scene.as_ref()?;
        if let Some((layer, index)) = view.find_node(name) {
            return match view.node(layer, index)? {
                luma_formats::SceneNode::Entity(_) => {
                    self.node_id(crate::scene_view::EntityRef::Node { layer, index })
                }
                luma_formats::SceneNode::Region(_) => self.region_id(layer, index),
            };
        }
        let free = view.find_free(name)?;
        self.node_id(crate::scene_view::EntityRef::Free(free))
    }

    /// The file of the sprite an object's sprite object stands for
    /// (`CAdTalkHolder::m_Sprite`): an entity's sprite, the sprite
    /// `SetSprite` gave an actor, an item's image.
    pub(crate) fn own_sprite_path(&self, owner: NativeId) -> Option<String> {
        match &self.objects.get(owner)?.kind {
            Kind::Entity(r) => self.entity_def(*r)?.sprite.clone(),
            Kind::Actor => self.actor(owner)?.sprite_override().map(str::to_string),
            Kind::Item => self.item(owner)?.def.sprite.clone(),
            _ => None,
        }
    }

    /// The sprite object of an entity, an actor or an item, created on
    /// first request.
    pub(crate) fn sprite_object(&mut self, owner: NativeId) -> Option<NativeId> {
        self.own_sprite_path(owner)?;
        if let Some(id) = self.objects.find(|k| *k == Kind::Sprite(owner)) {
            return Some(id);
        }
        Some(self.objects.insert(Kind::Sprite(owner)))
    }

    /// The sprite object of an entity, an actor or an item
    /// (`GetSpriteObject`), created on first request; `None` when the
    /// object has no sprite of its own.
    pub fn sprite_object_id(&mut self, owner: NativeId) -> Option<NativeId> {
        self.sprite_object(owner)
    }

    /// Removes a sprite object with its frame and sub-frame objects, as
    /// when the owner goes.
    pub(crate) fn drop_sprite_object(&mut self, sprite: NativeId) {
        let parts: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(_, n)| {
                matches!(n.kind, Kind::Frame { sprite: s, .. } | Kind::SubFrame { sprite: s, .. } if s == sprite)
            })
            .map(|(id, _)| id)
            .collect();
        for part in parts {
            self.objects.remove(part);
        }
        self.objects.remove(sprite);
    }

    /// Creates an inert object of a kind (`LoadWindow`, `LoadActor`...).
    pub(crate) fn inert(&mut self, kind: &str) -> NativeId {
        self.objects.insert(Kind::Inert(kind.to_string()))
    }

    /// Runs `f` on a library object with the world free for the call: the
    /// object is taken out of its slot and put back afterwards. `None`
    /// when the handle is not a library object.
    pub(crate) fn with_class<T>(
        &mut self,
        id: NativeId,
        f: impl FnOnce(&mut ClassObject, &mut World) -> T,
    ) -> Option<T> {
        let native = self.objects.get_mut(id)?;
        if !matches!(native.kind, Kind::Class(_)) {
            return None;
        }
        let Kind::Class(mut object) =
            std::mem::replace(&mut native.kind, Kind::Inert(String::new()))
        else {
            return None;
        };
        let result = f(&mut object, self);
        if let Some(native) = self.objects.get_mut(id) {
            native.kind = Kind::Class(object);
        }
        Some(result)
    }

    pub(crate) fn native(&self, id: NativeId) -> Option<&Native> {
        self.objects.get(id)
    }

    /// The offset centring the view on a point, clamped to the scene
    /// (`CAdScene::ScrollTo`): the viewport's size counts, and a scene
    /// smaller than the viewport gives a negative offset, as in the
    /// original engine, so that it is drawn lower or further right.
    fn centred_offset(&self, x: i32, y: i32) -> (i32, i32) {
        let (_, _, w, h) = self.viewport();
        let (sw, sh) = match &self.scene {
            Some(view) => (view.scene.width(), view.scene.height()),
            None => (w, h),
        };
        (
            (x - w / 2).max(0).min(sw - w),
            (y - h / 2).max(0).min(sh - h),
        )
    }

    /// Centres the view on a point at once (`Scene.SkipTo`).
    pub fn skip_to(&mut self, x: i32, y: i32) {
        let (ox, oy) = self.centred_offset(x, y);
        self.offset_x = ox;
        self.offset_y = oy;
        self.target_offset_x = ox;
        self.target_offset_y = oy;
    }

    /// Scrolls the view towards a point (`Scene.ScrollTo`): the target
    /// the auto-scroll moves to.
    pub fn scroll_to(&mut self, x: i32, y: i32) {
        let (ox, oy) = self.centred_offset(x, y);
        self.target_offset_x = ox;
        self.target_offset_y = oy;
    }
}

/// A file name a script gives, made relative to the save directory when
/// it is spelled as an absolute path inside it (`Game.SaveDirectory +
/// "\\file"`): the prefix is dropped, in either separator, without
/// regard to case.
fn strip_root(name: &str, root: &std::path::Path) -> String {
    let root = root.to_string_lossy().replace('\\', "/");
    let root = root.trim_end_matches('/');
    let normalized = name.replace('\\', "/");
    if root.is_empty() || normalized.len() < root.len() {
        return name.to_string();
    }
    let (head, tail) = normalized.split_at(root.len());
    if head.eq_ignore_ascii_case(root) && (tail.is_empty() || tail.starts_with('/')) {
        tail.trim_start_matches('/').to_string()
    } else {
        name.to_string()
    }
}

impl Natives for World {
    fn class_name(&self, id: NativeId) -> String {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Game) => "game".to_string(),
            Some(Kind::Keyboard) => "keyboard".to_string(),
            Some(Kind::Math) => "math".to_string(),
            Some(Kind::ResponseBox) => "responses".to_string(),
            Some(Kind::Scene) => "scene".to_string(),
            Some(Kind::Layer(_)) => "layer".to_string(),
            Some(Kind::Entity(_)) => "entity".to_string(),
            Some(Kind::Region { .. } | Kind::EntityRegion(_)) => "region".to_string(),
            Some(Kind::WaypointGroup(_)) => "waypoint-group".to_string(),
            Some(
                Kind::Sprite(_)
                | Kind::HoverSprite(_)
                | Kind::CursorSprite { .. }
                | Kind::ControlImage { .. },
            ) => "sprite".to_string(),
            Some(Kind::Frame { .. }) => "frame".to_string(),
            Some(Kind::SubFrame { .. }) => "subframe".to_string(),
            Some(Kind::Emitter(_)) => "particle-emitter".to_string(),
            Some(Kind::Actor) => "actor".to_string(),
            Some(Kind::Fader) | Some(Kind::SceneFader) => "fader".to_string(),
            Some(Kind::Control) => self
                .control(id)
                .map_or("control", Control::type_name)
                .to_string(),
            Some(Kind::Item) => "item".to_string(),
            Some(Kind::Inert(kind)) => kind.to_ascii_lowercase(),
            Some(Kind::Object) => "object".to_string(),
            Some(Kind::Class(object)) => object.type_name().to_string(),
            None => String::new(),
        }
    }

    fn to_string(&self, id: NativeId) -> String {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Game) => "[game]".to_string(),
            Some(Kind::Class(object)) => object.to_string(self),
            Some(Kind::Entity(_) | Kind::Region { .. } | Kind::Layer(_) | Kind::Scene) => {
                format!("[{}]", self.describe(id))
            }
            Some(Kind::Actor) => "[actor object]".to_string(),
            Some(Kind::Control | Kind::Item) => format!("[{}]", self.describe(id)),
            Some(Kind::Sprite(_) | Kind::HoverSprite(_) | Kind::ControlImage { .. }) => {
                "[sprite]".to_string()
            }
            Some(Kind::Frame { .. }) => "[frame]".to_string(),
            Some(Kind::SubFrame { .. }) => "[subframe]".to_string(),
            Some(Kind::Object) => "[object]".to_string(),
            Some(Kind::Emitter(_)) => "[particle emitter]".to_string(),
            Some(_) => "[native object]".to_string(),
            None => "[invalid object]".to_string(),
        }
    }

    fn to_bool(&self, id: NativeId) -> bool {
        match self.objects.get(id).map(|n| &n.kind) {
            // The library classes keep the base class's `false`.
            Some(Kind::Class(_)) => false,
            Some(_) => true,
            None => false,
        }
    }

    fn compare(&self, a: NativeId, b: NativeId) -> Ordering {
        match (self.objects.get(a), self.objects.get(b)) {
            (Some(x), Some(y)) => match (&x.kind, &y.kind) {
                (Kind::Class(p), Kind::Class(q)) => p.compare(q, a.cmp(&b)),
                _ => a.cmp(&b),
            },
            _ => a.cmp(&b),
        }
    }

    fn get_property(&self, id: NativeId, name: &str) -> Option<Value> {
        if let Some(value) = self.native_get(id, name) {
            return Some(value);
        }
        self.objects.get(id)?.props.get(name).cloned()
    }

    fn set_property(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        if !self.objects.contains(id) {
            return false;
        }
        if self.native_set(id, name, value) {
            return true;
        }
        if let Some(native) = self.objects.get_mut(id) {
            native.props.insert(name.to_string(), value.clone());
        }
        true
    }

    fn assign_scalar(&mut self, id: NativeId, value: &Value) {
        if let Some(Native {
            kind: Kind::Class(object),
            ..
        }) = self.objects.get_mut(id)
        {
            object.assign_scalar(value);
        }
    }
}

impl ClassHost for World {
    /// A file a script wrote first, then the game's files.
    fn read_file(&mut self, name: &str) -> Option<Vec<u8>> {
        if let Some(path) = self.writable_path(name) {
            if path.is_file() {
                return std::fs::read(path).ok();
            }
        }
        self.vfs.read(name).ok()
    }

    /// Under the save directory (the game's, or the user's own when the
    /// game's is not writable), never outside it. A name spelled from
    /// the save directory itself (`Game.SaveDirectory + "\\notes.txt"`,
    /// `Directory.TempDirectory + "x"`) is taken relative to it.
    fn writable_path(&mut self, name: &str) -> Option<PathBuf> {
        let root = self.checked_save_directory();
        let relative = relative_path(&strip_root(name, &root))?;
        Some(root.join(relative))
    }

    fn writable_root(&self) -> PathBuf {
        self.save_directory()
    }

    /// The save directory itself for an empty name or its own path, else
    /// a directory under it.
    fn writable_dir(&mut self, name: &str) -> Option<PathBuf> {
        let root = self.checked_save_directory();
        let name = strip_root(name, &root);
        if name.trim_matches(['\\', '/', ' ']).is_empty() || name == "." {
            Some(root)
        } else {
            self.writable_path(&name)
        }
    }

    /// The files and sub-directories of the game's packages and loose
    /// directories under `dir` (the game's root for an empty name).
    fn game_entries(&mut self, dir: &str) -> Vec<DirEntry> {
        let root = self.save_directory();
        let dir = strip_root(dir, &root);
        let prefix = relative_path(&dir)
            .map(|p| format!("{}/", p.to_string_lossy()).to_ascii_uppercase())
            .unwrap_or_default();
        let mut entries: Vec<DirEntry> = Vec::new();
        let mut add = |name: String, is_dir: bool| {
            if !entries.iter().any(|e| e.name.eq_ignore_ascii_case(&name)) {
                entries.push(DirEntry { name, is_dir });
            }
        };
        for loose in self.vfs.directories() {
            let path = match relative_path(&dir) {
                Some(relative) => loose.join(relative),
                None => loose.clone(),
            };
            if let Ok(read) = std::fs::read_dir(path) {
                for entry in read.flatten() {
                    let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                    add(entry.file_name().to_string_lossy().into_owned(), is_dir);
                }
            }
        }
        for file in self.vfs.packaged_files() {
            let Some(rest) = file.entry.key().strip_prefix(&prefix) else {
                continue;
            };
            // The entry's own spelling, after the directory part.
            let name = file.entry.path_name();
            let name = &name[name.len() - rest.len()..];
            match name.split_once('/') {
                Some((sub, _)) => add(sub.to_string(), true),
                None => add(name.to_string(), false),
            }
        }
        entries
    }

    fn create_object(&mut self, object: ClassObject) -> NativeId {
        self.objects.insert(Kind::Class(object))
    }

    fn log(&mut self, message: &str) {
        self.log.push(message.to_string());
    }
}

impl Host for World {
    fn read_script(&mut self, filename: &str) -> Option<Vec<u8>> {
        self.vfs.read(filename).ok()
    }

    fn log(&mut self, message: &str) {
        self.log.push(message.to_string());
    }

    fn timer(&self) -> u64 {
        self.timer
    }

    fn clock(&self) -> u64 {
        self.clock
    }

    fn valid_object(&self, id: NativeId) -> bool {
        self.objects.contains(id)
    }

    fn is_ready(&self, id: NativeId) -> bool {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor) => self.actor(id).is_none_or(|a| a.ready),
            Some(Kind::Entity(r)) => self
                .view_of(*r)
                .is_none_or(|v| !v.is_talking(*r) && !v.is_playing_anim(*r)),
            Some(Kind::Scene) => self.is_scrolled(),
            Some(Kind::Fader) => self.fader.ready,
            Some(Kind::SceneFader) => self.scene_fader.ready,
            Some(Kind::ResponseBox) => self.responses.waiting.is_none(),
            Some(Kind::Control) => self
                .control(id)
                .is_none_or(|c| c.window().is_none_or(|w| w.ready)),
            _ => true,
        }
    }

    fn random(&mut self, from: i32, to: i32) -> i32 {
        // xorshift64*, seeded once: deterministic across runs.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let value = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32;
        let span = i64::from(to) - i64::from(from) + 1;
        if span <= 0 {
            return from;
        }
        (i64::from(from) + (value as i64).rem_euclid(span)) as i32
    }

    fn call_function(&mut self, name: &str, args: &[Value], ctx: Context<'_>) -> Option<Return> {
        match name {
            // The script library classes.
            "String" | "Array" | "Date" | "File" | "MemBuffer" => {
                let object = ClassObject::construct(name, args, self)?;
                let id = self.objects.insert(Kind::Class(object));
                Some(Return::constructed(Value::Native(id)))
            }
            // `new Object(script, ...)` (`CSXObject`): a bare object with
            // the given scripts attached; they run from the next tick.
            "Object" => {
                let id = self.objects.insert(Kind::Object);
                for arg in args {
                    let path = arg.get_string(self);
                    self.attach(ctx.engine, id, &path);
                }
                Some(Return::constructed(Value::Native(id)))
            }
            // `new Actor()` and `new Entity()` (`CAdGame::ExternalCall`):
            // an empty actor or entity. WME's object belongs to no list
            // and is never drawn or updated; Luma makes it one of the
            // game's own, so that a sprite given to it shows and
            // `Game.UnloadObject` removes it. A file argument, which WME
            // discards, loads the file as `Game.LoadActor` and
            // `Game.LoadEntity` do; a file that does not load leaves an
            // empty object, logged.
            "Actor" | "Entity" => {
                let path = match args.first() {
                    Some(Value::Null) | None => None,
                    Some(v) => Some(v.get_string(self)),
                };
                let id = if name == "Actor" {
                    match path.and_then(|p| self.load_actor(ctx.engine, &p, false)) {
                        Some(id) => id,
                        None => self.new_actor(),
                    }
                } else {
                    match path.and_then(|p| self.load_game_entity(ctx.engine, &p)) {
                        Some(id) => id,
                        None => self.create_game_entity(ctx.engine, None),
                    }
                };
                Some(Return::constructed(Value::Native(id)))
            }
            _ => None,
        }
    }

    fn call_method(
        &mut self,
        target: NativeId,
        name: &str,
        args: &[Value],
        ctx: Context<'_>,
    ) -> Option<Return> {
        self.native_call(target, name, args, ctx)
    }

    /// `Game.SaveGame` takes the snapshot once its `true` is on the
    /// caller's stack, so that the script resumes after the call when the
    /// game loads; a failure turns the result into `false`.
    fn after_call(&mut self, engine: &mut Engine, script: luma_script::ScriptId) {
        if let Some((slot, description, quick)) = self.pending_save.take() {
            if let Err(err) = crate::save::save_game(self, engine, slot, &description, quick) {
                self.log(format!("Error saving game to slot {slot}: {err}"));
                engine.set_result(script, Value::Bool(false));
            }
        }
    }
}

/// A script engine and the world it runs in, driven once per frame.
pub struct Runtime {
    /// The script engine.
    pub engine: Engine,
    /// The game world, the engine's host.
    pub world: World,
}

impl Runtime {
    /// Opens a game, with sound on the default audio device.
    pub fn new(vfs: Vfs) -> Result<Runtime> {
        Runtime::with_audio(vfs, Audio::new())
    }

    /// Opens a game with a given audio system.
    pub fn with_audio(vfs: Vfs, audio: Audio) -> Result<Runtime> {
        let world = World::with_audio(vfs, audio)?;
        let mut engine = Engine::new();
        // The engine publishes the game object to every script.
        engine.set_global("Game", Value::Native(world.game_id));
        engine.set_global("Math", Value::Native(world.math_id));
        engine.set_global("Directory", Value::Native(world.directory_id));
        Ok(Runtime { engine, world })
    }

    /// Attaches the game's scripts (`SCRIPT=` in `default.game`); they
    /// run on the first tick. The startup scene, if the game names one,
    /// is scheduled first.
    pub fn start(&mut self) {
        self.world.init_accessibility();
        if let Some(scene) = self.world.startup_scene.clone() {
            self.world.schedule_scene(&scene, true);
        }
        self.world.load_items(&mut self.engine);
        self.world.load_inventory_box(&mut self.engine);
        self.world.load_response_box(&mut self.engine);
        let game = self.world.game_id;
        for script in self.world.game_file.scripts.clone() {
            self.world.attach(&mut self.engine, game, &script);
        }
    }

    /// Loads a scene now (`ChangeScene` without the frame delay).
    pub fn change_scene(&mut self, path: &str) -> Result<()> {
        self.world.scheduled_scene = None;
        self.world.load_scene(&mut self.engine, path)
    }

    /// Saves the game to a slot now (`Game.SaveGame` outside a script):
    /// `BeforeSave` runs first. Returns the file written.
    pub fn save_game(
        &mut self,
        slot: i32,
        description: &str,
        quick: bool,
    ) -> std::result::Result<std::path::PathBuf, String> {
        crate::save::save_game(&mut self.world, &mut self.engine, slot, description, quick)
    }

    /// Loads the game of a slot now (`Game.LoadGame` without the frame
    /// delay): `AfterLoad` runs on the game once everything is back.
    pub fn load_game(&mut self, slot: i32) -> std::result::Result<(), String> {
        crate::save::load_game(&mut self.world, &mut self.engine, slot)
    }

    /// One frame: the clocks advance by `dt` milliseconds (the sounds too
    /// without an audio device), a scheduled scene change happens, the
    /// scripts run, the scene's animations and the actors advance, the
    /// view follows the main object, the sounds follow their objects,
    /// and the events raised reach their scripts. Returns the log lines
    /// of the frame.
    pub fn tick(&mut self, dt: u64) -> Vec<String> {
        if let Some(slot) = self.world.scheduled_load.take() {
            // A game loads before anything else of the frame, as the
            // original engine does between two frames.
            if let Err(err) = crate::save::load_game(&mut self.world, &mut self.engine, slot) {
                self.world
                    .log(format!("Error loading game from slot {slot}: {err}"));
            }
        }
        self.world.advance_time(dt);
        if let Some((path, fade_in)) = self.world.scheduled_scene.take() {
            match self.world.load_scene(&mut self.engine, &path) {
                Ok(()) => {
                    if fade_in {
                        let (timer, clock) = (self.world.timer, self.world.clock);
                        self.world.fader.fade_in(
                            luma_formats::Rgba {
                                r: 0,
                                g: 0,
                                b: 0,
                                a: 255,
                            },
                            SCENE_FADE_MS,
                            true,
                            timer,
                            clock,
                        );
                    }
                }
                Err(err) => {
                    self.world
                        .log(format!("cannot change scene to '{path}': {err}"));
                }
            }
        }
        self.world.sounds.audio.tick(dt);
        self.world.drop_finished_video();
        if self.world.is_video_playing() {
            // A video takes the frame: no scripts, no scene, no windows
            // (`CAdGame::DisplayContent`).
            self.world.update_video(&mut self.engine);
            return self.world.take_log();
        }
        self.world.update_speech();
        let (timer, clock) = (self.world.timer, self.world.clock);
        self.world.fader.update(timer, clock);
        self.world.scene_fader.update(timer, clock);
        self.engine.tick(&mut self.world);
        self.world.update_scene();
        self.world.update_entity_videos();
        self.world.update_actors();
        self.world.update_emitters();
        self.world.scroll_step();
        self.world.update_sounds();
        self.world.collect_hits();
        self.world.update_active_object();
        self.world.update_accessibility();
        self.world.update_ui(&mut self.engine);
        self.world.update_inventory(&mut self.engine);
        self.world.prepare_inventory_texts();
        self.world.update_quick_messages();
        self.world.update_loading_icon();
        self.world.update_cursor();
        self.world.update_item_cursor(&self.engine);
        self.world.apply_pending_events(&mut self.engine);
        self.world.take_log()
    }

    /// Fills a draw list with the frame as it is now; `white` is the
    /// renderer's white texture, for the fader.
    pub fn draw<'a>(
        &'a self,
        list: &mut luma_render::DrawList<'a>,
        white: Option<&'a luma_render::Texture>,
    ) {
        let camera = self.world.camera();
        self.world.draw(list, &camera, white);
    }

    /// Moves the mouse to a point in game pixels.
    pub fn mouse_move(&mut self, x: i32, y: i32) {
        self.world.set_mouse(x, y);
    }

    /// Whether the mouse is over the scene (`PointInViewport`).
    fn mouse_in_viewport(&self) -> bool {
        let (x, y) = (self.world.mouse.x, self.world.mouse.y);
        let (vx, vy, vw, vh) = self.world.viewport();
        self.world.scene.is_some() && x >= vx && y >= vy && x < vx + vw && y < vy + vh
    }

    /// Raises a mouse event (`CAdGame::OnMouseLeftDown` and the like):
    /// on the game first; when its scripts do not handle it, on the
    /// active object, else, for the events the adventure game dispatches
    /// itself (`scene_too`), on the scene when the mouse is over it.
    /// While the game is frozen, only the active object gets it.
    fn mouse_event(&mut self, event: &str, scene_too: bool) {
        let game = self.world.game_id;
        let running = !self.world.is_frozen();
        if running && self.engine.apply_event(&mut self.world, game, event, false) {
            return;
        }
        if let Some(active) = self.world.active_object() {
            self.engine
                .apply_event(&mut self.world, active, event, false);
        } else if scene_too && running && self.mouse_in_viewport() {
            let scene = self.world.scene_id;
            self.engine
                .apply_event(&mut self.world, scene, event, false);
        }
    }

    /// Whether a click at the mouse is the second of a double click: it
    /// follows `last` within 400 ms and 4 pixels (the Windows defaults
    /// the engine relied on). `last` is then cleared, or set to this
    /// click.
    fn note_click(&mut self, right: bool) -> bool {
        let (x, y, clock) = (self.world.mouse.x, self.world.mouse.y, self.world.clock);
        let last = if right {
            &mut self.world.mouse.last_right_click
        } else {
            &mut self.world.mouse.last_left_click
        };
        let double = last.is_some_and(|(at, lx, ly)| {
            clock.saturating_sub(at) < 400 && (lx - x).abs() <= 4 && (ly - y).abs() <= 4
        });
        *last = if double { None } else { Some((clock, x, y)) };
        double
    }

    /// The left button goes down: a click, or a double click when it
    /// follows another within 400 ms and 4 pixels. While the game is not
    /// interactive, the click only skips the sentences.
    pub fn mouse_left_down(&mut self) {
        let double = self.note_click(false);
        self.world.mouse.left_down = true;
        if self.world.is_video_playing() {
            if matches!(
                self.world.video_skip_button,
                TalkSkipButton::Left | TalkSkipButton::Both
            ) {
                self.world.stop_video(&mut self.engine);
            }
            return;
        }
        // A click while the game is not interactive only skips a
        // sentence, unless a window or the response box holds the game
        // (`GAME_SEMI_FROZEN`), where the windows still take clicks.
        if !self.world.interactive && !self.world.semi_frozen() {
            self.world.click_while_busy(false);
            return;
        }
        self.world.ui_mouse_down();
        if double {
            self.mouse_event("LeftDoubleClick", true);
        } else {
            self.mouse_event("LeftClick", true);
        }
    }

    /// The left button goes up (`LeftRelease`).
    pub fn mouse_left_up(&mut self) {
        self.world.mouse.left_down = false;
        self.world.ui_mouse_up();
        self.mouse_event("LeftRelease", true);
    }

    /// The right button goes down: `RightClick`, or `RightDoubleClick`
    /// when it follows another right click within 400 ms and 4 pixels
    /// (`CBGame::OnMouseRightDblClick`, which goes to the game then the
    /// active object, not to the scene). While the game is not
    /// interactive, a click only skips the sentences and a double click
    /// does nothing.
    pub fn mouse_right_down(&mut self) {
        let double = self.note_click(true);
        if self.world.is_video_playing() {
            if matches!(
                self.world.video_skip_button,
                TalkSkipButton::Right | TalkSkipButton::Both
            ) {
                self.world.stop_video(&mut self.engine);
            }
            return;
        }
        if !self.world.interactive {
            if !double {
                self.world.click_while_busy(true);
            }
            return;
        }
        if double {
            self.mouse_event("RightDoubleClick", false);
        } else {
            self.mouse_event("RightClick", true);
        }
    }

    /// The right button goes up (`RightRelease`).
    pub fn mouse_right_up(&mut self) {
        self.mouse_event("RightRelease", true);
    }

    /// The middle button goes down (`MiddleClick`, `CBGame::
    /// OnMouseMiddleDown`): to the game, else to the active object;
    /// nothing while a video plays or the game is not interactive.
    pub fn mouse_middle_down(&mut self) {
        if self.world.is_video_playing() || !self.world.interactive {
            return;
        }
        self.mouse_event("MiddleClick", false);
    }

    /// The middle button goes up (`MiddleRelease`).
    pub fn mouse_middle_up(&mut self) {
        if self.world.is_video_playing() {
            return;
        }
        self.mouse_event("MiddleRelease", false);
    }

    /// The wheel turns (`CBGame::HandleMouseWheel`): `MouseWheelUp` for a
    /// positive `delta`, `MouseWheelDown` for a negative one, raised on
    /// the focused window when its scripts handle that event, else on
    /// the game.
    pub fn mouse_wheel(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        let event = if delta < 0 {
            "MouseWheelDown"
        } else {
            "MouseWheelUp"
        };
        if let Some(window) = self.world.focused_window() {
            if self.engine.can_handle_event(window, event) {
                self.engine
                    .apply_event(&mut self.world, window, event, false);
                return;
            }
        }
        let game = self.world.game_id;
        self.engine.apply_event(&mut self.world, game, event, false);
    }

    /// The player asked to close the game's window (`CBGame::
    /// OnWindowClose`): when a game script handles `QuitGame`, the event
    /// is raised (unless the game is frozen) and the game goes on, so
    /// that the script may confirm and call `Game.QuitGame()` itself;
    /// `true` then. `false` when no script handles it: the window closes.
    pub fn window_close(&mut self) -> bool {
        let game = self.world.game_id;
        if !self.engine.can_handle_event(game, "QuitGame") {
            return false;
        }
        if !self.world.is_frozen() {
            self.engine
                .apply_event(&mut self.world, game, "QuitGame", false);
        }
        true
    }

    /// A key goes down (`CBGame::HandleKeypress`): the keyboard state is
    /// updated; the focused window takes the key first (`Tab`, its
    /// editor), else its `Keypress` handler when it has one, else the
    /// game's.
    pub fn key_down(
        &mut self,
        code: i32,
        text: Option<char>,
        shift: bool,
        alt: bool,
        control: bool,
    ) {
        self.world.key_down(code, text, shift, alt, control);
        if self.world.is_video_playing() {
            if code == 27 {
                self.world.stop_video(&mut self.engine);
            }
            return;
        }
        if self.world.access_key(&mut self.engine, code) {
            return;
        }
        let game = self.world.game_id;
        if let Some(window) = self.world.focused_window() {
            if self.world.window_key(window, code, text) {
                return;
            }
            if self.engine.can_handle_event(window, "Keypress") {
                self.engine
                    .apply_event(&mut self.world, window, "Keypress", false);
                return;
            }
        }
        self.engine
            .apply_event(&mut self.world, game, "Keypress", false);
    }

    /// A key goes up.
    pub fn key_up(&mut self, code: i32) {
        self.world.key_up(code);
    }
}
