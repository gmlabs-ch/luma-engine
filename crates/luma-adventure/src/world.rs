//! The game world as scripts see it: the `Game` object, the current
//! scene and its parts, the string table, time, and the scripts attached
//! to everything. The [`World`] is the script engine's host; the
//! [`Runtime`] pairs it with an engine and drives both once per frame.
//!
//! Written with reference to `BGame.cpp`, `AdGame.cpp`, `AdScene.cpp` and
//! `BScriptHolder.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan
//! Nedoma); see `NOTICE`.

use std::collections::HashSet;

use luma_archive::Vfs;
use luma_formats::{GameFile, Scene, StringTable};
use luma_script::{Context, Engine, Host, NativeId, Natives, Return, State, Value};

use crate::actor::{Actor, ActorEvent, WalkContext, scene_obstacles};
use crate::error::{Error, Result};
use crate::objects::{Kind, Native, Objects};
use crate::path::Floor;
use crate::scene_view::{Camera, Drawable, SceneView};
use crate::settings::GameSettings;
use crate::talk::{Sentence, SentenceDraw, SubtitleMods, TalkSkipButton};
use crate::text::FontBank;
use luma_formats::font::TextAlign;

/// The game as scripts see it. Implements the script engine's [`Host`].
pub struct World {
    pub(crate) vfs: Vfs,
    settings: GameSettings,
    game_file: GameFile,
    strings: StringTable,
    pub(crate) objects: Objects,
    pub(crate) game_id: NativeId,
    pub(crate) keyboard_id: NativeId,
    pub(crate) scene_id: NativeId,
    pub(crate) scene: Option<SceneView>,
    pub(crate) prev_scene_name: String,
    pub(crate) prev_scene_filename: String,
    pub(crate) scheduled_scene: Option<(String, bool)>,
    pub(crate) offset_x: i32,
    pub(crate) offset_y: i32,
    pub(crate) target_offset_x: i32,
    pub(crate) target_offset_y: i32,
    pub(crate) auto_scroll: bool,
    pub(crate) scroll_pixels_x: i32,
    pub(crate) scroll_pixels_y: i32,
    pub(crate) scroll_time_x: u64,
    pub(crate) scroll_time_y: u64,
    last_scroll_x: u64,
    last_scroll_y: u64,
    pub(crate) actors: Vec<(NativeId, Actor)>,
    pending_events: Vec<(NativeId, &'static str)>,
    pub(crate) fonts: FontBank,
    pub(crate) system_font: Option<String>,
    pub(crate) subtitles: bool,
    pub(crate) subtitles_speed: i32,
    pub(crate) talk_skip_button: TalkSkipButton,
    sentences: Vec<SentenceDraw>,
    pub(crate) interactive: bool,
    pub(crate) main_object: Option<NativeId>,
    pub(crate) quit: bool,
    pub(crate) name: String,
    pub(crate) caption: String,
    timer: u64,
    clock: u64,
    log: Vec<String>,
    stubs: HashSet<String>,
    rng: u64,
}

impl World {
    /// Opens a game: its settings, `default.game` and, if the game names
    /// one, its string table.
    pub fn new(vfs: Vfs) -> Result<World> {
        let settings = GameSettings::load(&vfs)?;
        let bytes = vfs.read(&settings.game_file)?;
        let game_file = GameFile::parse(&bytes).map_err(|source| Error::Format {
            path: settings.game_file.clone(),
            source,
        })?;
        let mut objects = Objects::new();
        let game_id = objects.insert(Kind::Game);
        let keyboard_id = objects.insert(Kind::Keyboard);
        let scene_id = objects.insert(Kind::Scene);
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
            scene_id,
            scene: None,
            prev_scene_name: String::new(),
            prev_scene_filename: String::new(),
            scheduled_scene: None,
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
            fonts: FontBank::new(),
            system_font: None,
            subtitles: true,
            subtitles_speed: 70,
            talk_skip_button: TalkSkipButton::Left,
            sentences: Vec::new(),
            interactive: true,
            main_object: None,
            quit: false,
            timer: 0,
            clock: 0,
            log: Vec::new(),
            stubs: HashSet::new(),
            rng: 0x2545_F491_4F6C_DD1D,
        };
        if let Some(path) = world.game_file.string_table.clone() {
            world.load_string_table(&path, true);
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

    /// The camera showing the scene at its current offset.
    pub fn camera(&self) -> Camera {
        let mut camera = Camera::new(self.settings.width as f32, self.settings.height as f32);
        camera.offset_x = self.offset_x as f32;
        camera.offset_y = self.offset_y as f32;
        camera
    }

    /// Whether the game accepts input (`Game.Interactive`).
    pub fn is_interactive(&self) -> bool {
        self.interactive
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
        self.timer += dt;
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
    pub(crate) fn describe(&self, id: NativeId) -> String {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Game) => self.name.clone(),
            Some(Kind::Scene) => self
                .scene
                .as_ref()
                .map(|s| s.scene.name.clone())
                .unwrap_or_default(),
            Some(Kind::Entity(r)) => self
                .scene
                .as_ref()
                .and_then(|s| s.entity(*r))
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
            Some(Kind::Sprite(_)) => "sprite".to_string(),
            Some(Kind::Actor) => self
                .actor(id)
                .map(|a| a.def.name.clone())
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
        let actor = match Actor::load(&self.vfs, path, scene_bound) {
            Ok(actor) => actor,
            Err(err) => {
                self.log(format!("Error loading ACTOR file '{path}': {err}"));
                return None;
            }
        };
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
            _ => return false,
        }
        for script in engine.scripts_of(id) {
            engine.finish(script, true);
        }
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
                ActorEvent::OnActor(name) => self.pending_events.push((owner, name)),
                ActorEvent::OnRegion {
                    layer,
                    index,
                    event,
                } => {
                    if let Some(region) = self.region_id(layer, index) {
                        self.pending_events.push((region, event));
                    }
                }
            }
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
                engine.apply_event(self, owner, event, false);
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

    /// Fills a draw list with the scene, the actors and the sentences
    /// (when subtitles are on).
    pub fn draw<'a>(&'a self, list: &mut luma_render::DrawList<'a>, camera: &Camera) {
        if let Some(view) = &self.scene {
            view.draw_with(list, camera, &self.drawables());
        }
        if self.subtitles {
            for draw in &self.sentences {
                if let Some(font) = self.fonts.get(&draw.font) {
                    font.draw(list, &draw.text, draw.x, draw.y, draw.width, draw.align, -1);
                }
            }
        }
    }

    /// Uploads the textures of the scene, of the actors and of the fonts.
    pub fn upload(&mut self, renderer: &luma_render::Renderer) {
        if let Some(view) = self.scene.as_mut() {
            view.upload(&self.vfs, renderer);
        }
        for (_, actor) in &mut self.actors {
            actor.bank.upload(&self.vfs, renderer);
        }
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
                matches!(
                    n.kind,
                    Kind::Layer(_) | Kind::Entity(_) | Kind::Region { .. } | Kind::Sprite(_)
                ) || scene_actors.contains(id)
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
        if let Some(scene) = self.objects.get_mut(self.scene_id) {
            scene.props.clear();
        }
        engine.remove_finished();

        let view = SceneView::load(&self.vfs, path)?;
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
        self.scene = Some(view);
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
            let scripts = self.scene().unwrap().scene.free_entities[i].scripts.clone();
            for script in scripts {
                self.attach(engine, id, &script);
            }
        }
        Ok(())
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
        duration: i32,
        stances: Option<String>,
        align: TextAlign,
    ) -> bool {
        let kind = match self.objects.get(id).map(|n| n.kind.clone()) {
            Some(kind @ (Kind::Actor | Kind::Entity(_))) => kind,
            _ => return false,
        };
        let text = self.expand(text);
        let font = self.font_of(id);
        let mut duration = u64::try_from(duration).unwrap_or(0);
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

    /// The handle of a scene node or free entity.
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

    /// The offset centring the view on a point, clamped to the scene.
    fn centred_offset(&self, x: i32, y: i32) -> (i32, i32) {
        let (w, h) = (self.settings.width as i32, self.settings.height as i32);
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
            Some(Kind::Scene) => "scene".to_string(),
            Some(Kind::Layer(_)) => "layer".to_string(),
            Some(Kind::Entity(_)) => "entity".to_string(),
            Some(Kind::Region { .. }) => "region".to_string(),
            Some(Kind::Sprite(_)) => "sprite".to_string(),
            Some(Kind::Actor) => "actor".to_string(),
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
            Some(Kind::Entity(r)) => self.scene().is_none_or(|v| !v.is_talking(*r)),
            Some(Kind::Scene) => self.is_scrolled(),
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
}

/// A script engine and the world it runs in, driven once per frame.
pub struct Runtime {
    /// The script engine.
    pub engine: Engine,
    /// The game world, the engine's host.
    pub world: World,
}

impl Runtime {
    /// Opens a game.
    pub fn new(vfs: Vfs) -> Result<Runtime> {
        let world = World::new(vfs)?;
        let mut engine = Engine::new();
        // The engine publishes the game object to every script.
        engine.set_global("Game", Value::Native(world.game_id));
        Ok(Runtime { engine, world })
    }

    /// Attaches the game's scripts (`SCRIPT=` in `default.game`); they
    /// run on the first tick. The startup scene, if the game names one,
    /// is scheduled first.
    pub fn start(&mut self) {
        if let Some(scene) = self.world.game_file.startup_scene.clone() {
            self.world.schedule_scene(&scene, true);
        }
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

    /// One frame: the clocks advance by `dt` milliseconds, a scheduled
    /// scene change happens, the scripts run, the scene's animations and
    /// the actors advance, the view follows the main object, and the
    /// events the actors raised reach their scripts. Returns the log
    /// lines of the frame.
    pub fn tick(&mut self, dt: u64) -> Vec<String> {
        self.world.advance_time(dt);
        if let Some((path, _fade_in)) = self.world.scheduled_scene.take() {
            if let Err(err) = self.world.load_scene(&mut self.engine, &path) {
                self.world
                    .log(format!("cannot change scene to '{path}': {err}"));
            }
        }
        self.engine.tick(&mut self.world);
        self.world.update_scene();
        self.world.update_actors();
        self.world.scroll_step();
        self.world.apply_pending_events(&mut self.engine);
        self.world.take_log()
    }

    /// Fills a draw list with the scene and the actors as they are now.
    pub fn draw<'a>(&'a self, list: &mut luma_render::DrawList<'a>) {
        let camera = self.world.camera();
        self.world.draw(list, &camera);
    }
}
