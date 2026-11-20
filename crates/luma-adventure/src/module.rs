//! The module API: how Rust code, in the same build as the engine, adds
//! script classes and receives engine events. It replaces the original
//! engine's plugin interface (`wme_*.dll`, `CBPlugin`, `CBPluginMgr`,
//! `CSXPluginObject`), which loads native Windows libraries and cannot
//! exist here; see `docs/modules.md` for the design and for how to
//! write a module.
//!
//! Written with reference to `BPluginMgr.cpp`, `BPlugin.cpp`,
//! `SXPluginObject.cpp`, `wme_plugin.h` and `CBGame::ExternalCall` of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan
//! Nedoma); see `NOTICE`.
//!
//! # Shape
//!
//! A [`Module`] is registered on the [`World`] (`World::register_module`)
//! and names the script classes it provides. When a script runs
//! `new Counter(5)` and no engine class has that name, the module that
//! provides `Counter` constructs the object ([`Module::construct`],
//! `CBPluginMgr::HandleExternalCall`, `CSXPluginObject::Construct`). The
//! script holds a native handle like any other; its method calls,
//! property reads and writes reach the module, and the engine's events
//! reach the objects that subscribed to them ([`ModuleEvent`],
//! `CBPluginMgr::ApplyEvent`). A saved game keeps each object's class,
//! its subscriptions and the bytes the module wrote for it
//! ([`Module::save`], `CSXPluginObject::Persist`).
//!
//! # What a module cannot do
//!
//! A module never sees the world. Each call gets a [`ModuleContext`]
//! that only holds copies (the clocks, the scene's path) and collects
//! what the module asks for (log lines, subscriptions), and the values
//! that cross the boundary are script values with bounded sizes. An
//! error a module returns, or a panic it raises, becomes a script error
//! and an entry in the log; after a panic the module is stopped, since
//! its own state can no longer be trusted, and every later call to it
//! fails the same way.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use luma_script::{Context, Host, NativeId, Return, Value};

use crate::objects::Kind;
use crate::world::World;

/// The longest string a module may take or return, in bytes.
pub const MAX_STRING_BYTES: usize = 64 * 1024;

/// The most arguments a call to a module may carry.
pub const MAX_ARGS: usize = 32;

/// The most state bytes one object may save.
pub const MAX_STATE_BYTES: usize = 16 * 1024 * 1024;

/// The most live objects one module may have.
pub const MAX_OBJECTS_PER_MODULE: usize = 4096;

/// The most modules a world takes.
pub const MAX_MODULES: usize = 64;

/// The longest error or panic message kept, in bytes; a longer one is cut.
pub const MAX_ERROR_BYTES: usize = 1024;

/// The most state bytes all the objects of a world may save together.
pub const MAX_TOTAL_STATE_BYTES: usize = 64 * 1024 * 1024;

/// The most log lines a module may write in one call; the rest are dropped.
pub const MAX_LOG_LINES_PER_CALL: usize = 32;

/// The longest log line, in bytes; a longer one is cut.
pub const MAX_LOG_LINE_BYTES: usize = 1024;

/// A module's own number for one of its objects. The module chooses it
/// when it constructs or restores an object (`m_ObjectID` of
/// `CSXPluginObject`) and gets it back with every later call. It must
/// not be reused while the module lives: a saved game load destroys the
/// old objects after it restored the new ones.
pub type ObjectId = u64;

/// What a module reports when it refuses a call. The message reaches the
/// script as a runtime error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleError(String);

impl ModuleError {
    /// An error with a message for the script's author.
    pub fn new(message: impl Into<String>) -> ModuleError {
        let mut message = message.into();
        if message.len() > MAX_ERROR_BYTES {
            let mut end = MAX_ERROR_BYTES;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        ModuleError(message)
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ModuleError {}

impl From<&str> for ModuleError {
    fn from(message: &str) -> ModuleError {
        ModuleError::new(message)
    }
}

impl From<String> for ModuleError {
    fn from(message: String) -> ModuleError {
        ModuleError::new(message)
    }
}

/// The result of a call into a module.
pub type ModuleResult<T> = Result<T, ModuleError>;

/// The engine events a module's objects can subscribe to
/// (`EWmeEvent` of `wme_plugin.h`, `CBPluginMgr::ApplyEvent`), in the
/// original's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ModuleEvent {
    /// `WME_EVENT_UPDATE`: once per frame after the scripts ran, while the
    /// game is not showing a video.
    Update,
    /// `WME_EVENT_SCENE_DRAW_BEGIN`: the scene is about to be drawn.
    SceneDrawBegin,
    /// `WME_EVENT_SCENE_DRAW_END`: the scene was drawn.
    SceneDrawEnd,
    /// `WME_EVENT_SCENE_INIT`: a scene was loaded; its path is
    /// [`ModuleContext::scene`].
    SceneInit,
    /// `WME_EVENT_SCENE_SHUTDOWN`: the scene is about to be left; its
    /// path is [`ModuleContext::scene`].
    SceneShutdown,
    /// `WME_EVENT_GAME_BEFORE_SAVE`: a game is about to be saved, before
    /// the objects' state is collected.
    GameBeforeSave,
    /// `WME_EVENT_GAME_AFTER_LOAD`: a game was loaded and the module's
    /// objects restored.
    GameAfterLoad,
}

impl ModuleEvent {
    /// Every event, in the original's numbering (`WME_EVENT_MAX` is 7).
    pub const ALL: [ModuleEvent; 7] = [
        ModuleEvent::Update,
        ModuleEvent::SceneDrawBegin,
        ModuleEvent::SceneDrawEnd,
        ModuleEvent::SceneInit,
        ModuleEvent::SceneShutdown,
        ModuleEvent::GameBeforeSave,
        ModuleEvent::GameAfterLoad,
    ];

    /// The event's number in the original (`WME_EVENT_UPDATE` is 0).
    pub fn number(self) -> u8 {
        self as u8
    }

    fn bit(self) -> u8 {
        1 << self.number()
    }
}

/// What one call hands a module: copies of what it may want to know
/// and the channels to what it may ask for. It holds no reference to
/// the world, so nothing a module does with it can reach the world
/// except through what the engine applies afterwards.
#[derive(Debug, Clone)]
pub struct ModuleContext {
    timer: u64,
    clock: u64,
    scene: Option<String>,
    events: u8,
    log: Vec<String>,
}

impl ModuleContext {
    /// The game timer, in milliseconds (it stops while the game is
    /// frozen).
    pub fn timer(&self) -> u64 {
        self.timer
    }

    /// The wall clock the game runs on, in milliseconds.
    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// The path of the scene that is loaded, if any; for the scene
    /// events, the scene they are about.
    pub fn scene(&self) -> Option<&str> {
        self.scene.as_deref()
    }

    /// Subscribes the object this call is about to an event
    /// (`IWmeGame::SubscribeEvent`). In `construct` and `restore` it is
    /// the object being made.
    pub fn subscribe(&mut self, event: ModuleEvent) {
        self.events |= event.bit();
    }

    /// Unsubscribes the object this call is about from an event
    /// (`IWmeGame::UnsubscribeEvent`).
    pub fn unsubscribe(&mut self, event: ModuleEvent) {
        self.events &= !event.bit();
    }

    /// Whether the object this call is about is subscribed to an event.
    pub fn is_subscribed(&self, event: ModuleEvent) -> bool {
        self.events & event.bit() != 0
    }

    /// Writes a line to the game's log.
    pub fn log(&mut self, message: impl Into<String>) {
        if self.log.len() >= MAX_LOG_LINES_PER_CALL {
            return;
        }
        let mut message = message.into();
        if message.len() > MAX_LOG_LINE_BYTES {
            let mut end = MAX_LOG_LINE_BYTES;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        self.log.push(message);
    }
}

/// A module: Rust code that adds script classes to a game. See the
/// [module documentation](self) and `docs/modules.md`.
///
/// Every method but [`Module::name`] and [`Module::classes`] concerns
/// the module's objects, which the module keeps itself and names by an
/// [`ObjectId`] of its own choosing. The engine calls a module from one
/// thread, one call at a time.
pub trait Module {
    /// The module's name, for the log and the error messages. It is
    /// unique among the modules of a world.
    fn name(&self) -> &str;

    /// The script classes this module provides, in the spelling scripts
    /// use after `new` (the match is exact, as the original's
    /// `strcmp`). A class an engine class already has, or another module
    /// provides, is refused at registration.
    fn classes(&self) -> Vec<String>;

    /// Makes an object of a class, from the arguments of
    /// `new Class(args...)` (`CSXPluginObject::Construct`). The arguments
    /// are scalars and native handles; a script object is passed as
    /// `null`.
    fn construct(
        &mut self,
        class: &str,
        args: &[Value],
        cx: &mut ModuleContext,
    ) -> ModuleResult<ObjectId>;

    /// Makes an object of a class from the bytes [`Module::save`] wrote
    /// for it, when a saved game loads (`ConstructEmpty` and
    /// `Serialize`). A module that does not support saved games returns
    /// an error, and the load is refused.
    fn restore(
        &mut self,
        class: &str,
        state: &[u8],
        cx: &mut ModuleContext,
    ) -> ModuleResult<ObjectId>;

    /// A method call on an object (`CSXPluginObject::ScCallMethod`).
    /// `Ok(None)` says the object has no such method, and the script
    /// gets the engine's error for it; a method with nothing to return
    /// answers `Ok(Some(Value::Null))`.
    fn call_method(
        &mut self,
        object: ObjectId,
        name: &str,
        args: &[Value],
        cx: &mut ModuleContext,
    ) -> ModuleResult<Option<Value>>;

    /// A property read (`ScGetProperty`); `None` is a property the object
    /// does not have. A script's own properties on the object are kept
    /// by the engine and need no support here.
    fn get_property(&self, _object: ObjectId, _name: &str) -> Option<Value> {
        None
    }

    /// A property write (`ScSetProperty`); `Ok(false)` says the object
    /// has no such property, and the engine keeps the value as a script
    /// property of the object.
    fn set_property(
        &mut self,
        _object: ObjectId,
        _name: &str,
        _value: &Value,
        _cx: &mut ModuleContext,
    ) -> ModuleResult<bool> {
        Ok(false)
    }

    /// The object is gone (`Game.UnloadObject`, or the game that held it
    /// was replaced by a load): free what it held.
    fn destruct(&mut self, object: ObjectId);

    /// The bytes that bring the object back when a saved game loads
    /// (`CSXPluginObject::Persist` with `Serialize`). The class, the
    /// subscriptions and `Ready` are kept by the engine.
    fn save(&mut self, _object: ObjectId) -> ModuleResult<Vec<u8>> {
        Ok(Vec::new())
    }

    /// An engine event an object subscribed to (`ReceiveEvent`). The
    /// answer says whether the module handled it (`ApplyEvent` returns
    /// whether any object did).
    fn event(&mut self, _object: ObjectId, _event: ModuleEvent, _cx: &mut ModuleContext) -> bool {
        false
    }
}

/// A module object as the world holds it: a native whose behaviour
/// the module answers for. It is the whole of what a saved game keeps
/// of the object on the engine's side.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModuleObject {
    /// The script class (`CSXPluginObject::m_Class`).
    pub class: String,
    /// The module's number for the object; meaningless in a saved game,
    /// where the module gives a new one back.
    pub id: ObjectId,
    /// The events the object subscribed to, a bit per
    /// [`ModuleEvent::number`] (`m_SubscribedEvents`).
    pub events: u8,
    /// `Ready` (`CBObject::m_Ready`), true by default: what
    /// `WaitFor(object)` waits for.
    pub ready: bool,
    /// The bytes the module wrote for the object, present in a saved
    /// game only.
    pub state: Vec<u8>,
}

/// A module as the world holds it.
struct Slot {
    module: Box<dyn Module>,
    name: String,
    /// Set by a panic: the module is not called again.
    stopped: Cell<bool>,
    /// The numbers of the module's objects that are alive.
    live: std::collections::HashSet<ObjectId>,
}

impl Slot {
    /// Runs a call into the module, turning a panic into an error and
    /// stopping the module.
    fn guard<T>(&mut self, call: impl FnOnce(&mut dyn Module) -> T) -> ModuleResult<T> {
        if self.stopped.get() {
            return Err(self.stopped_error());
        }
        let module = self.module.as_mut();
        catch_unwind(AssertUnwindSafe(|| call(module))).map_err(|panic| self.panicked(panic))
    }

    /// The same for a call that only reads.
    fn guard_ref<T>(&self, call: impl FnOnce(&dyn Module) -> T) -> ModuleResult<T> {
        if self.stopped.get() {
            return Err(self.stopped_error());
        }
        let module = self.module.as_ref();
        catch_unwind(AssertUnwindSafe(|| call(module))).map_err(|panic| self.panicked(panic))
    }

    fn stopped_error(&self) -> ModuleError {
        ModuleError::new(format!(
            "module '{}' was stopped after it panicked",
            self.name
        ))
    }

    fn panicked(&self, panic: Box<dyn std::any::Any + Send>) -> ModuleError {
        self.stopped.set(true);
        let reason = panic
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".to_string());
        ModuleError::new(format!("module '{}' panicked: {reason}", self.name))
    }
}

/// A module object made from a saved game, waiting for the world to be
/// replaced.
pub(crate) struct Restored {
    native: NativeId,
    id: ObjectId,
    events: u8,
    slot: usize,
}

/// The modules of a world and the classes they provide.
#[derive(Default)]
pub(crate) struct Modules {
    slots: Vec<Slot>,
    by_class: HashMap<String, usize>,
}

impl Modules {
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

/// The classes the engine itself constructs; a module may not take one.
const ENGINE_CLASSES: &[&str] = &[
    "String",
    "Array",
    "Date",
    "File",
    "MemBuffer",
    "TcpClient",
    "Object",
    "Actor",
    "Entity",
    "Directory",
    "Math",
];

/// Whether a class name is one a script can write after `new`.
fn valid_class_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A value that crosses into a module: a script object, which could be
/// large and holds other values, becomes `null`; a string longer than the
/// limit is an error.
fn arguments(args: &[Value]) -> ModuleResult<Vec<Value>> {
    if args.len() > MAX_ARGS {
        return Err(ModuleError::new(format!(
            "too many arguments ({}, at most {MAX_ARGS})",
            args.len()
        )));
    }
    args.iter()
        .map(|arg| match arg {
            Value::Object(_) => Ok(Value::Null),
            Value::String(s) if s.len() > MAX_STRING_BYTES => Err(ModuleError::new(format!(
                "a string argument of {} bytes is over the limit of {MAX_STRING_BYTES}",
                s.len()
            ))),
            other => Ok(other.clone()),
        })
        .collect()
}

impl World {
    /// Adds a module: the script classes it provides become available to
    /// `new`, and its objects can subscribe to the engine's events. The
    /// registration is refused, with the module dropped, when its name
    /// or one of its classes is empty or malformed, is taken by another
    /// module, or names a class of the engine; at most [`MAX_MODULES`]
    /// fit.
    ///
    /// Register modules before the scripts run. A saved game names the
    /// classes of its objects, and it only loads in a world that provides
    /// them all.
    pub fn register_module(&mut self, module: Box<dyn Module>) -> Result<(), ModuleError> {
        let name = catch_unwind(AssertUnwindSafe(|| module.name().to_string()))
            .map_err(|_| ModuleError::new("a module panicked in `name`"))?;
        let modules = &mut self.modules;
        if modules.slots.len() >= MAX_MODULES {
            return Err(ModuleError::new(format!(
                "cannot register module '{name}': at most {MAX_MODULES} modules"
            )));
        }
        if name.is_empty() || name.len() > 64 {
            return Err(ModuleError::new("a module needs a name of 1 to 64 bytes"));
        }
        if modules.slots.iter().any(|s| s.name == name) {
            return Err(ModuleError::new(format!(
                "a module named '{name}' is already registered"
            )));
        }
        // The class list is the module's own code: it may panic too.
        let classes = catch_unwind(AssertUnwindSafe(|| module.classes()))
            .map_err(|_| ModuleError::new(format!("module '{name}' panicked in `classes`")))?;
        for class in &classes {
            if !valid_class_name(class) {
                return Err(ModuleError::new(format!(
                    "module '{name}': '{class}' is not a class name a script can write"
                )));
            }
            if ENGINE_CLASSES.contains(&class.as_str()) {
                return Err(ModuleError::new(format!(
                    "module '{name}': '{class}' is a class of the engine"
                )));
            }
            if let Some(&other) = modules.by_class.get(class) {
                return Err(ModuleError::new(format!(
                    "module '{name}': class '{class}' is already provided by module '{}'",
                    modules.slots[other].name
                )));
            }
        }
        let mut seen = std::collections::HashSet::new();
        if !classes.iter().all(|c| seen.insert(c.as_str())) {
            return Err(ModuleError::new(format!(
                "module '{name}' lists a class twice"
            )));
        }
        let index = modules.slots.len();
        for class in classes {
            modules.by_class.insert(class, index);
        }
        modules.slots.push(Slot {
            module,
            name,
            stopped: Cell::new(false),
            live: std::collections::HashSet::new(),
        });
        Ok(())
    }

    /// The script classes the registered modules provide, in
    /// registration order: what a script compiler is told to accept as
    /// external names (`CBPluginMgr::RegisterClasses`).
    pub fn module_classes(&self) -> Vec<String> {
        let mut classes: Vec<(usize, &String)> = self
            .modules
            .by_class
            .iter()
            .map(|(class, &slot)| (slot, class))
            .collect();
        classes.sort();
        classes.into_iter().map(|(_, c)| c.clone()).collect()
    }

    fn module_context(&self, events: u8) -> ModuleContext {
        ModuleContext {
            timer: self.timer,
            clock: self.clock,
            scene: self.scene.as_ref().map(|view| view.path.clone()),
            events,
            log: Vec::new(),
        }
    }

    /// Writes what a module logged.
    fn flush_module_log(&mut self, slot: usize, cx: &mut ModuleContext) {
        let name = self.modules.slots[slot].name.clone();
        for line in std::mem::take(&mut cx.log) {
            self.log(format!("[{name}] {line}"));
        }
    }

    /// A module error reported to the calling script, as the engine's own
    /// runtime errors are (`Call to undefined function '%s'. Ignored.`).
    fn module_script_error(&mut self, ctx: &Context<'_>, what: &str, error: &ModuleError) {
        let message = ctx.runtime_error(&format!("{what}: {error}"));
        Host::script_error(self, &message);
    }

    /// A script value a module returned, checked: a script object is
    /// refused (a module cannot make one), a string over the limit too,
    /// and a native handle that names nothing becomes `null`.
    fn module_value(&self, value: Value) -> ModuleResult<Value> {
        match value {
            Value::Object(_) => Err(ModuleError::new(
                "a module cannot return a script object".to_string(),
            )),
            Value::String(s) if s.len() > MAX_STRING_BYTES => Err(ModuleError::new(format!(
                "a returned string of {} bytes is over the limit of {MAX_STRING_BYTES}",
                s.len()
            ))),
            Value::Native(id) if !self.objects.contains(id) => Ok(Value::Null),
            other => Ok(other),
        }
    }

    /// The module object a handle names, with the index of its module.
    fn module_object(&self, id: NativeId) -> Option<(usize, ModuleObject)> {
        let Kind::Module(object) = &self.objects.get(id)?.kind else {
            return None;
        };
        let slot = *self.modules.by_class.get(&object.class)?;
        Some((slot, object.clone()))
    }

    /// `new Class(...)` for a class a module provides, tried after the
    /// engine's own functions (`CBPluginMgr::HandleExternalCall`).
    pub(crate) fn module_construct(
        &mut self,
        class: &str,
        args: &[Value],
        ctx: &Context<'_>,
    ) -> Option<Return> {
        let slot = *self.modules.by_class.get(class)?;
        let fail = |world: &mut World, error: ModuleError| {
            world.module_script_error(ctx, &format!("new {class}"), &error);
            Some(Return::constructed(Value::Null))
        };
        if self.modules.slots[slot].live.len() >= MAX_OBJECTS_PER_MODULE {
            let error = ModuleError::new(format!(
                "module '{}' already has {MAX_OBJECTS_PER_MODULE} objects",
                self.modules.slots[slot].name
            ));
            return fail(self, error);
        }
        let args = match arguments(args) {
            Ok(args) => args,
            Err(error) => return fail(self, error),
        };
        let mut cx = self.module_context(0);
        let made = self.modules.slots[slot]
            .guard(|module| module.construct(class, &args, &mut cx))
            .and_then(|result| result);
        self.flush_module_log(slot, &mut cx);
        match made {
            Ok(id) if self.modules.slots[slot].live.contains(&id) => {
                let error = ModuleError::new(format!("object number {id} is already in use"));
                fail(self, error)
            }
            Ok(id) => {
                self.modules.slots[slot].live.insert(id);
                let native = self.objects.insert(Kind::Module(ModuleObject {
                    class: class.to_string(),
                    id,
                    events: cx.events,
                    ready: true,
                    state: Vec::new(),
                }));
                Some(Return::constructed(Value::Native(native)))
            }
            Err(error) => fail(self, error),
        }
    }

    /// A method of a module object. `None` when the module has no such
    /// method or failed (the failure is reported to the script).
    pub(crate) fn module_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
        ctx: &Context<'_>,
    ) -> Option<Return> {
        let (slot, object) = self.module_object(id)?;
        let fail = |world: &mut World, error: ModuleError| {
            world.module_script_error(ctx, &format!("{}.{name}", object.class), &error);
            Some(Return::null())
        };
        let args = match arguments(args) {
            Ok(args) => args,
            Err(error) => return fail(self, error),
        };
        let mut cx = self.module_context(object.events);
        let result = self.modules.slots[slot]
            .guard(|module| module.call_method(object.id, name, &args, &mut cx))
            .and_then(|result| result)
            .and_then(|value| value.map(|v| self.module_value(v)).transpose());
        self.flush_module_log(slot, &mut cx);
        self.store_subscriptions(id, cx.events);
        match result {
            Ok(value) => value.map(Return::value),
            Err(error) => fail(self, error),
        }
    }

    /// A property of a module object, if the module has it; `Ready`, the
    /// engine's, otherwise.
    pub(crate) fn module_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let (slot, object) = self.module_object(id)?;
        let answer = self.modules.slots[slot]
            .guard_ref(|module| module.get_property(object.id, name))
            .ok()
            .flatten()
            .and_then(|value| self.module_value(value).ok());
        match answer {
            Some(value) => Some(value),
            None if name == "Ready" => Some(Value::Bool(object.ready)),
            None => None,
        }
    }

    /// A property write on a module object; whether it was taken.
    /// An error of the module is logged and the write counts as taken:
    /// the property is the module's, not a script property.
    pub(crate) fn module_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let Some((slot, object)) = self.module_object(id) else {
            return false;
        };
        let value = match arguments(std::slice::from_ref(value)) {
            Ok(mut values) => values.remove(0),
            Err(error) => {
                self.log(format!("{}.{name}: {error}", object.class));
                return true;
            }
        };
        let mut cx = self.module_context(object.events);
        let result = self.modules.slots[slot]
            .guard(|module| module.set_property(object.id, name, &value, &mut cx))
            .and_then(|result| result);
        self.flush_module_log(slot, &mut cx);
        self.store_subscriptions(id, cx.events);
        match result {
            Ok(true) => true,
            Ok(false) if name == "Ready" => {
                // `CSXPluginObject::ScSetProperty`: `Ready` is the
                // object's own.
                let ready = value.get_bool(self);
                if let Some(Kind::Module(object)) =
                    self.objects.get_mut(id).map(|native| &mut native.kind)
                {
                    object.ready = ready;
                }
                true
            }
            Ok(false) => false,
            Err(error) => {
                self.log(format!("{}.{name}: {error}", object.class));
                true
            }
        }
    }

    fn store_subscriptions(&mut self, id: NativeId, events: u8) {
        if let Some(Kind::Module(object)) = self.objects.get_mut(id).map(|native| &mut native.kind)
        {
            object.events = events;
        }
    }

    /// Whether a module object is ready (`Ready`).
    pub(crate) fn module_ready(&self, id: NativeId) -> bool {
        match self.objects.get(id).map(|native| &native.kind) {
            Some(Kind::Module(object)) => object.ready,
            _ => true,
        }
    }

    /// An object of a module goes (`Game.UnloadObject`): the module
    /// frees it, and the engine removes the handle.
    pub(crate) fn module_destruct(&mut self, id: NativeId) {
        let Some((slot, object)) = self.module_object(id) else {
            return;
        };
        self.destruct_in(slot, object.id);
    }

    fn destruct_in(&mut self, slot: usize, object: ObjectId) {
        let entry = &mut self.modules.slots[slot];
        entry.live.remove(&object);
        if let Err(error) = entry.guard(|module| module.destruct(object)) {
            self.log(error.to_string());
        }
    }

    /// Delivers an engine event to the objects subscribed to it
    /// (`CBPluginMgr::ApplyEvent`); whether any of them handled it.
    pub(crate) fn apply_module_event(&mut self, event: ModuleEvent) -> bool {
        if self.modules.is_empty() {
            return false;
        }
        let targets: Vec<NativeId> = self
            .objects
            .iter()
            .filter_map(|(id, native)| match &native.kind {
                Kind::Module(object) if object.events & event.bit() != 0 => Some(id),
                _ => None,
            })
            .collect();
        let mut handled = false;
        for id in targets {
            // An earlier object's handler may have unloaded this one.
            let Some((slot, object)) = self.module_object(id) else {
                continue;
            };
            if object.events & event.bit() == 0 {
                continue;
            }
            let mut cx = self.module_context(object.events);
            let answer =
                self.modules.slots[slot].guard(|module| module.event(object.id, event, &mut cx));
            self.flush_module_log(slot, &mut cx);
            self.store_subscriptions(id, cx.events);
            match answer {
                Ok(true) => handled = true,
                Ok(false) => {}
                Err(error) => self.log(format!("{} ({event:?}): {error}", object.class)),
            }
        }
        handled
    }

    /// Asks every module object for its bytes and puts them in the
    /// objects, for a saved game to write; [`World::clear_module_state`]
    /// takes them out again. Fails, with nothing written, when a module
    /// refuses or writes more than [`MAX_STATE_BYTES`].
    pub(crate) fn collect_module_state(
        &mut self,
        engine: &mut luma_script::Engine,
    ) -> Result<(), String> {
        // The objects of a module that panicked cannot be saved: they
        // are dropped from the game, with a warning, so that the module's
        // failure does not stop every later save.
        let stopped: Vec<NativeId> = self
            .objects
            .iter()
            .filter_map(|(id, _)| {
                let (slot, _) = self.module_object(id)?;
                self.modules.slots[slot].stopped.get().then_some(id)
            })
            .collect();
        for id in stopped {
            if let Some((slot, object)) = self.module_object(id) {
                self.log(format!(
                    "Warning: a '{}' object of the stopped module '{}' is dropped from the saved game and the game",
                    object.class, self.modules.slots[slot].name
                ));
                self.modules.slots[slot].live.remove(&object.id);
                self.objects.remove(id);
            }
        }
        self.flush_removed_objects(engine);
        let mut total = 0usize;
        let handles: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(_, native)| matches!(native.kind, Kind::Module(_)))
            .map(|(id, _)| id)
            .collect();
        for id in handles {
            let Some((slot, object)) = self.module_object(id) else {
                return Err(format!(
                    "no module provides the class of a module object ('{}')",
                    self.module_class_of(id)
                ));
            };
            let bytes = self.modules.slots[slot]
                .guard(|module| module.save(object.id))
                .and_then(|result| result)
                .map_err(|e| format!("{}: cannot save: {e}", object.class))?;
            if bytes.len() > MAX_STATE_BYTES {
                return Err(format!(
                    "{}: {} bytes of state are over the limit of {MAX_STATE_BYTES}",
                    object.class,
                    bytes.len()
                ));
            }
            total += bytes.len();
            if total > MAX_TOTAL_STATE_BYTES {
                return Err(format!(
                    "the module objects' state is over the limit of {MAX_TOTAL_STATE_BYTES} bytes together"
                ));
            }
            if let Some(Kind::Module(o)) = self.objects.get_mut(id).map(|native| &mut native.kind) {
                o.state = bytes;
            }
        }
        Ok(())
    }

    fn module_class_of(&self, id: NativeId) -> String {
        match self.objects.get(id).map(|native| &native.kind) {
            Some(Kind::Module(object)) => object.class.clone(),
            _ => String::new(),
        }
    }

    /// Drops the bytes [`World::collect_module_state`] left in the objects.
    pub(crate) fn clear_module_state(&mut self) {
        for (_, native) in self.objects.iter_mut() {
            if let Kind::Module(object) = &mut native.kind {
                object.state = Vec::new();
            }
        }
    }

    /// Makes the module objects of a saved game, before the world is
    /// replaced: refused, with the world untouched and the objects made so
    /// far destroyed, when a class is not provided or a module refuses.
    /// Returns what [`World::finish_module_restore`] needs.
    pub(crate) fn restore_module_objects(
        &mut self,
        saved: &crate::objects::Objects,
    ) -> Result<Vec<Restored>, String> {
        let wanted: Vec<(NativeId, &ModuleObject)> = saved
            .iter()
            .filter_map(|(id, native)| match &native.kind {
                Kind::Module(object) => Some((id, object)),
                _ => None,
            })
            .collect();
        // Every class first, so that a missing one refuses the load
        // before any module did anything.
        for (_, object) in &wanted {
            if !self.modules.by_class.contains_key(&object.class) {
                return Err(format!(
                    "the saved game has objects of the module class '{}', which this game does not provide",
                    object.class
                ));
            }
        }
        // No module may be made to hold more objects than it may have.
        let mut per_slot: HashMap<usize, usize> = HashMap::new();
        for (_, object) in &wanted {
            let count = per_slot
                .entry(self.modules.by_class[&object.class])
                .or_insert(0);
            *count += 1;
            if *count > MAX_OBJECTS_PER_MODULE {
                return Err(format!(
                    "the saved game has more than {MAX_OBJECTS_PER_MODULE} objects of one module"
                ));
            }
        }
        let mut made: Vec<Restored> = Vec::new();
        for (native, object) in wanted {
            let slot = self.modules.by_class[&object.class];
            let mut cx = self.module_context(object.events);
            let result = self.modules.slots[slot]
                .guard(|module| module.restore(&object.class, &object.state, &mut cx))
                .and_then(|result| result);
            self.flush_module_log(slot, &mut cx);
            // A number alive already, an object of the world being
            // replaced or one just made, is not the module's to give
            // again: the load is refused, atomically like any failure.
            let result = result.and_then(|id| {
                if self.modules.slots[slot].live.contains(&id) {
                    Err(ModuleError::new(format!(
                        "object number {id} is already in use"
                    )))
                } else {
                    Ok(id)
                }
            });
            match result {
                Ok(id) => {
                    self.modules.slots[slot].live.insert(id);
                    made.push(Restored {
                        native,
                        id,
                        events: cx.events,
                        slot,
                    });
                }
                Err(error) => {
                    // What was made goes again.
                    for done in made {
                        self.destruct_in(done.slot, done.id);
                    }
                    return Err(format!(
                        "the module could not restore a '{}': {error}",
                        object.class
                    ));
                }
            }
        }
        Ok(made)
    }

    /// The module objects of the world about to be replaced, for
    /// [`World::finish_module_restore`].
    pub(crate) fn live_module_objects(&self) -> Vec<(usize, ObjectId)> {
        self.objects
            .iter()
            .filter_map(|(id, _)| self.module_object(id).map(|(slot, o)| (slot, o.id)))
            .collect()
    }

    /// After a saved game replaced the world: the restored objects get
    /// the module's numbers and subscriptions, and the objects of the
    /// world that was replaced are destroyed.
    pub(crate) fn finish_module_restore(
        &mut self,
        restored: Vec<Restored>,
        old: Vec<(usize, ObjectId)>,
    ) {
        for done in restored {
            if let Some(Kind::Module(object)) =
                self.objects.get_mut(done.native).map(|n| &mut n.kind)
            {
                object.id = done.id;
                object.events = done.events;
                object.state = Vec::new();
            }
        }
        for (slot, id) in old {
            self.destruct_in(slot, id);
        }
    }
}
