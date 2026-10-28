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
use luma_formats::{GameFile, StringTable};
use luma_script::{Context, Engine, Host, NativeId, Natives, Return, State, Value};

use crate::error::{Error, Result};
use crate::objects::{Kind, Native, Objects};
use crate::scene_view::{Camera, SceneView};
use crate::settings::GameSettings;

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
            Some(Kind::Inert(kind)) => kind.clone(),
            None => "invalid".to_string(),
        }
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
        let gone: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(_, n)| {
                matches!(
                    n.kind,
                    Kind::Layer(_) | Kind::Entity(_) | Kind::Region { .. } | Kind::Sprite(_)
                )
            })
            .map(|(id, _)| id)
            .collect();
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
        for (file, reason) in &view.missing {
            self.log(format!("  missing: {file}: {reason}"));
        }
        self.offset_x = 0;
        self.offset_y = 0;
        self.scene = Some(view);

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

    /// Centres the view on a point (`Scene.SkipTo`), clamped to the scene.
    pub fn skip_to(&mut self, x: i32, y: i32) {
        let (w, h) = (self.settings.width as i32, self.settings.height as i32);
        let (sw, sh) = match &self.scene {
            Some(view) => (view.scene.width(), view.scene.height()),
            None => (w, h),
        };
        self.offset_x = (x - w / 2).max(0).min(sw - w);
        self.offset_y = (y - h / 2).max(0).min(sh - h);
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

    fn is_ready(&self, _id: NativeId) -> bool {
        // Nothing walks or talks yet.
        true
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
    /// scene change happens, the scripts run, the scene's animations
    /// advance. Returns the log lines of the frame.
    pub fn tick(&mut self, dt: u64) -> Vec<String> {
        self.world.advance_time(dt);
        if let Some((path, _fade_in)) = self.world.scheduled_scene.take() {
            if let Err(err) = self.world.load_scene(&mut self.engine, &path) {
                self.world
                    .log(format!("cannot change scene to '{path}': {err}"));
            }
        }
        self.engine.tick(&mut self.world);
        let timer = self.world.timer;
        if let Some(scene) = self.world.scene.as_mut() {
            scene.advance(timer);
        }
        self.world.take_log()
    }
}
