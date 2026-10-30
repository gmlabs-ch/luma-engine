//! The game world as scripts see it: the `Game` object, the current
//! scene and its parts, the string table, time, and the scripts attached
//! to everything. The [`World`] is the script engine's host; the
//! [`Runtime`] pairs it with an engine and drives both once per frame.
//!
//! Written with reference to `BGame.cpp`, `AdGame.cpp`, `AdScene.cpp` and
//! `BScriptHolder.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan
//! Nedoma); see `NOTICE`.

use std::collections::{BTreeMap, HashSet};

use luma_archive::Vfs;
use luma_formats::Entity;
use luma_formats::{GameFile, Scene, StringTable};
use luma_script::{Context, Engine, Host, NativeId, Natives, Return, State, Value};

use crate::actor::{Actor, ActorEvent, WalkContext, scene_obstacles};
use crate::error::{Error, Result};
use crate::fader::{Fader, SCENE_FADE_MS};
use crate::input::{Cursors, HitArea, Keyboard, Mouse};
use crate::inventory::Inventory;
use crate::objects::{Kind, Native, Objects};
use crate::path::Floor;
use crate::scene_state::SceneState;
use crate::scene_view::{Camera, Drawable, SceneView};
use crate::settings::GameSettings;
use crate::sound::Sounds;
use crate::talk::{Sentence, SentenceDraw, SubtitleMods, TalkSkipButton};
use crate::text::FontBank;
use crate::ui::{Control, Ui};
use crate::video::VideoPlayer;
use luma_audio::Audio;
use luma_formats::font::TextAlign;

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
    /// `SCENE_VIEWPORT` of the game file or `Game.SetSceneViewport`:
    /// `(x, y, width, height)` on the screen; `None` is the whole screen.
    pub(crate) scene_viewport: Option<(i32, i32, i32, i32)>,
    pub(crate) scene_id: NativeId,
    pub(crate) scene: Option<SceneView>,
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
    pub(crate) ui: Ui,
    /// The dialogue response box and its state.
    pub(crate) responses: crate::responses::Responses,
    /// The object the script waiting in `GetResponse` waits for.
    pub(crate) response_box_id: NativeId,
    /// The entities shown by the windows' entity containers.
    pub(crate) ui_entities: Vec<crate::ui::UiEntity>,
    pub(crate) inventory: Inventory,
    pub(crate) video: Option<VideoPlayer>,
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
    pub(crate) timer: u64,
    pub(crate) clock: u64,
    log: Vec<String>,
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
        let response_box_id = objects.insert(Kind::ResponseBox);
        let scene_id = objects.insert(Kind::Scene);
        let fader_id = objects.insert(Kind::Fader);
        let personal_savegames = game_file.personal_savegames;
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
            scene_viewport,
            scene_id,
            scene: None,
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
            ui: Ui::default(),
            responses: crate::responses::Responses::default(),
            response_box_id,
            ui_entities: Vec::new(),
            inventory: Inventory::default(),
            video: None,
            video_texture: None,
            video_uploaded: 0,
            video_subtitles: false,
            video_font: None,
            video_skip_button: TalkSkipButton::Left,
            interactive: true,
            main_object: None,
            quit: false,
            timer: 0,
            clock: 0,
            log: Vec::new(),
            stubs: HashSet::new(),
            rng: 0x2545_F491_4F6C_DD1D,
            scheduled_load: None,
            pending_save: None,
            personal_save: personal_savegames,
            save_dir_checked: false,
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

    /// The part of the screen the scene is shown in (`SCENE_VIEWPORT`,
    /// `Game.SetSceneViewport`): `(x, y, width, height)`, the whole
    /// screen by default.
    pub fn viewport(&self) -> (i32, i32, i32, i32) {
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
    /// and the containers' entities do, the scene's objects do not.
    pub(crate) fn non_int_mouse_events(&self, id: NativeId) -> bool {
        matches!(
            self.objects.get(id).map(|n| &n.kind),
            Some(Kind::Control | Kind::Entity(crate::scene_view::EntityRef::Ui(_)))
        )
    }

    /// Whether a script asked to quit (`Game.QuitGame`).
    pub fn quit_requested(&self) -> bool {
        self.quit
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

    /// Advances the clocks by `dt` milliseconds.
    pub fn advance_time(&mut self, dt: u64) {
        if !self.ui.frozen {
            self.timer += dt;
        }
        self.clock += dt;
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

    /// Loads a string table (`Game.LoadStringTable`).
    pub fn load_string_table(&mut self, path: &str, clear_old: bool) -> bool {
        match self.vfs.read(path) {
            Ok(bytes) => {
                let count = self.strings.load(&bytes, clear_old);
                self.log(format!("string table '{path}': {count} strings"));
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
        engine.run_script(self, path, Some(owner)).is_some()
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
            Some(Kind::Sprite(_)) => "sprite".to_string(),
            Some(Kind::Fader) => "fader".to_string(),
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

    /// Removes an object scripts created (`Game.UnloadObject`): an actor
    /// or an inert object, with its scripts. Returns whether the handle
    /// named one.
    pub fn unload_object(&mut self, engine: &mut Engine, id: NativeId) -> bool {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor | Kind::Inert(_)) => {}
            Some(Kind::Control) => return self.unload_control(engine, id),
            _ => return false,
        }
        for script in engine.scripts_of(id) {
            engine.finish(script, true);
        }
        self.stop_sfx(id, true);
        self.sounds.objects.remove(&id);
        self.actors.retain(|(i, _)| *i != id);
        if self.main_object == Some(id) {
            self.main_object = None;
        }
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

    /// One frame of the scene's entities: animations and sentences.
    pub fn update_scene(&mut self) {
        let timer = self.timer;
        let random = self.next_random();
        if let Some(view) = self.scene.as_mut() {
            view.advance_with(timer, random);
        }
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
                let e = self.scene()?.entity(*r)?;
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

    /// The actors as drawn this frame, the game's then the scene's.
    pub fn drawables(&self) -> Vec<Drawable<'_>> {
        let Some(view) = &self.scene else {
            return Vec::new();
        };
        self.actors
            .iter()
            .filter_map(|(_, a)| a.drawable(&view.scene))
            .collect()
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
        if self.is_video_playing() {
            // Only the video is drawn while it plays (`DisplayContent`).
            self.draw_video(list);
            return;
        }
        if let Some(view) = &self.scene {
            view.draw_with(list, camera, &self.drawables());
        }
        self.draw_windows(list, true, white);
        self.draw_inventory(list, white);
        self.draw_windows(list, false, white);
        self.draw_responses(list, white);
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
    }

    /// Uploads the textures of the scene, of the actors, of the cursors
    /// and of the fonts.
    pub fn upload(&mut self, renderer: &luma_render::Renderer) {
        if let Some(view) = self.scene.as_mut() {
            view.upload(&self.vfs, renderer);
        }
        for (_, actor) in &mut self.actors {
            actor.bank.upload(&self.vfs, renderer);
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
            self.save_scene_state();
        }
        // Objects of the old scene, and the scripts attached to them.
        let scene_actors: Vec<NativeId> = self
            .actors
            .iter()
            .filter(|(_, a)| a.scene_bound)
            .map(|(id, _)| *id)
            .collect();
        let gone: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(id, n)| {
                // The windows' container entities live outside the scene.
                let scene_entity = matches!(
                    n.kind,
                    Kind::Entity(r) if !matches!(r, crate::scene_view::EntityRef::Ui(_))
                );
                scene_entity
                    || matches!(
                        n.kind,
                        Kind::Layer(_) | Kind::Region { .. } | Kind::Sprite(_)
                    )
                    || scene_actors.contains(id)
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
        // The game's actors are reset before the new scene loads.
        for (id, actor) in &mut self.actors {
            actor.reset();
            engine.reset_object(*id);
        }
        self.pending_events.clear();
        for id in gone.iter().copied().chain([self.scene_id]) {
            for script in engine.scripts_of(id) {
                engine.finish(script, true);
            }
        }
        for id in gone {
            self.objects.remove(id);
        }
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
        self.restore_scene_state();
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
            let scripts = self.scene().unwrap().scene.free_entities[i].scripts.clone();
            for script in scripts {
                self.attach(engine, id, &script);
            }
        }
        Ok(())
    }

    /// Loads the `SOUND` of an entity without playing it (`CAdEntity`
    /// reads `SOUND`, `SOUND_START_TIME` and `SOUND_VOLUME`).
    fn load_entity_sound(&mut self, id: NativeId) {
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
            Some(Kind::Entity(r)) => self
                .scene()
                .and_then(|v| v.entity(*r))
                .and_then(|e| e.font.clone()),
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
                if let Some(entity) = self.scene_mut().and_then(|v| v.entity_mut(r)) {
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
                .scene()
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
                if let Some(view) = self.scene_mut() {
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
        let mods = self.subtitle_mods(id);
        let (sw, sh) = (self.settings.width as i32, self.settings.height as i32);
        let (mut x, mut y) = match &kind {
            Kind::Actor => self.actor(id).map_or((0, 0), |a| (a.x, a.y)),
            Kind::Entity(r) => self
                .scene()
                .and_then(|v| v.entity(*r))
                .map_or((0, 0), |e| (e.x, e.y)),
            _ => (0, 0),
        };
        if mods.relative {
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
        match kind {
            Kind::Actor => {
                if let Some(actor) = self.actor_mut(id) {
                    actor.talk(sentence);
                }
            }
            Kind::Entity(r) => {
                let sprites: Vec<String> = self
                    .scene()
                    .and_then(|v| v.entity(r))
                    .map(|e| {
                        e.talk_sprites
                            .iter()
                            .chain(&e.talk_special_sprites)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(view) = self.scene.as_mut() {
                    for sprite in sprites {
                        view.add_sprite(&self.vfs, &sprite);
                    }
                    view.talk(r, sentence);
                }
            }
            _ => {}
        }
        true
    }

    /// Ends an object's sentence (`StopTalk`); whether one was playing.
    pub fn stop_talk(&mut self, id: NativeId) -> bool {
        match self.objects.get(id).map(|n| n.kind.clone()) {
            Some(Kind::Actor) => self.actor_mut(id).is_some_and(|a| a.stop_talk()),
            Some(Kind::Entity(r)) => self.scene_mut().is_some_and(|v| v.stop_talk(r)),
            _ => false,
        }
    }

    /// Whether an object is saying a sentence.
    pub fn is_talking(&self, id: NativeId) -> bool {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor) => self.actor(id).is_some_and(|a| a.is_talking()),
            Some(Kind::Entity(r)) => self.scene().is_some_and(|v| v.is_talking(*r)),
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
        self.stop_speech();
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
        if let Some(view) = &self.scene {
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
            r => self.scene().and_then(|v| v.entity(r)),
        }
    }

    /// See [`World::entity_def`].
    pub(crate) fn entity_def_mut(
        &mut self,
        r: crate::scene_view::EntityRef,
    ) -> Option<&mut Entity> {
        match r {
            crate::scene_view::EntityRef::Ui(i) => self.ui_entities.get_mut(i).map(|e| &mut e.def),
            r => self.scene_mut().and_then(|v| v.entity_mut(r)),
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

    /// The sprite object of an entity, created on first request.
    pub(crate) fn sprite_object(&mut self, owner: NativeId) -> Option<NativeId> {
        let Kind::Entity(r) = self.objects.get(owner)?.kind else {
            return None;
        };
        self.scene.as_ref()?.entity(r)?.sprite.as_ref()?;
        if let Some(id) = self.objects.find(|k| *k == Kind::Sprite(owner)) {
            return Some(id);
        }
        Some(self.objects.insert(Kind::Sprite(owner)))
    }

    /// The sprite object of an entity (`GetSpriteObject`), created on
    /// first request; `None` when the handle is not an entity with a
    /// sprite.
    pub fn sprite_object_id(&mut self, owner: NativeId) -> Option<NativeId> {
        self.sprite_object(owner)
    }

    /// Creates an inert object of a kind (`LoadWindow`, `LoadActor`...).
    pub(crate) fn inert(&mut self, kind: &str) -> NativeId {
        self.objects.insert(Kind::Inert(kind.to_string()))
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
            Some(Kind::Region { .. }) => "region".to_string(),
            Some(Kind::Sprite(_)) => "sprite".to_string(),
            Some(Kind::Actor) => "actor".to_string(),
            Some(Kind::Fader) => "fader".to_string(),
            Some(Kind::Control) => self
                .control(id)
                .map_or("control", Control::type_name)
                .to_string(),
            Some(Kind::Item) => "item".to_string(),
            Some(Kind::Inert(kind)) => kind.to_ascii_lowercase(),
            None => String::new(),
        }
    }

    fn to_string(&self, id: NativeId) -> String {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Game) => "[game]".to_string(),
            Some(Kind::Entity(_) | Kind::Region { .. } | Kind::Layer(_) | Kind::Scene) => {
                format!("[{}]", self.describe(id))
            }
            Some(Kind::Actor) => "[actor object]".to_string(),
            Some(Kind::Control | Kind::Item) => format!("[{}]", self.describe(id)),
            Some(_) => "[native object]".to_string(),
            None => "[invalid object]".to_string(),
        }
    }

    fn to_bool(&self, id: NativeId) -> bool {
        self.objects.contains(id)
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
                .scene()
                .is_none_or(|v| !v.is_talking(*r) && !v.is_playing_anim(*r)),
            Some(Kind::Scene) => self.is_scrolled(),
            Some(Kind::Fader) => self.fader.ready,
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

    fn call_function(&mut self, name: &str, _args: &[Value], _ctx: Context<'_>) -> Option<Return> {
        match name {
            // Constructors of the engine's script classes.
            "Actor" | "Entity" | "String" | "Array" | "Date" | "File" | "MemBuffer" | "Object" => {
                self.stub(&format!("new {name}()"));
                let id = self.inert(name);
                Some(Return::value(Value::Native(id)))
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
        Ok(Runtime { engine, world })
    }

    /// Attaches the game's scripts (`SCRIPT=` in `default.game`); they
    /// run on the first tick. The startup scene, if the game names one,
    /// is scheduled first.
    pub fn start(&mut self) {
        if let Some(scene) = self.world.game_file.startup_scene.clone() {
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
        self.engine.tick(&mut self.world);
        self.world.update_scene();
        self.world.update_actors();
        self.world.scroll_step();
        self.world.update_sounds();
        self.world.collect_hits();
        self.world.update_active_object();
        self.world.update_ui(&mut self.engine);
        self.world.update_inventory(&mut self.engine);
        self.world.prepare_inventory_texts();
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
    /// active object, else on the scene when the mouse is over it. While
    /// the game is frozen, only the active object gets it.
    fn mouse_event(&mut self, event: &str) {
        let game = self.world.game_id;
        let running = !self.world.is_frozen();
        if running && self.engine.apply_event(&mut self.world, game, event, false) {
            return;
        }
        if let Some(active) = self.world.active_object() {
            self.engine
                .apply_event(&mut self.world, active, event, false);
        } else if running && self.mouse_in_viewport() {
            let scene = self.world.scene_id;
            self.engine
                .apply_event(&mut self.world, scene, event, false);
        }
    }

    /// The left button goes down: a click, or a double click when it
    /// follows another within 400 ms and 4 pixels. While the game is not
    /// interactive, the click only skips the sentences.
    pub fn mouse_left_down(&mut self) {
        let (x, y, clock) = (self.world.mouse.x, self.world.mouse.y, self.world.clock);
        let double = self
            .world
            .mouse
            .last_left_click
            .is_some_and(|(at, lx, ly)| {
                clock.saturating_sub(at) < 400 && (lx - x).abs() <= 4 && (ly - y).abs() <= 4
            });
        self.world.mouse.last_left_click = if double { None } else { Some((clock, x, y)) };
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
            self.mouse_event("LeftDoubleClick");
        } else {
            self.mouse_event("LeftClick");
        }
    }

    /// The left button goes up (`LeftRelease`).
    pub fn mouse_left_up(&mut self) {
        self.world.mouse.left_down = false;
        self.world.ui_mouse_up();
        self.mouse_event("LeftRelease");
    }

    /// The right button goes down (`RightClick`); while the game is not
    /// interactive, it only skips the sentences.
    pub fn mouse_right_down(&mut self) {
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
            self.world.click_while_busy(true);
            return;
        }
        self.mouse_event("RightClick");
    }

    /// The right button goes up (`RightRelease`).
    pub fn mouse_right_up(&mut self) {
        self.mouse_event("RightRelease");
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
