//! The module API: a module defined here provides the script class
//! `Counter`, which a compiled script constructs with `new`, calls,
//! reads and writes; it receives engine events, survives a saved game,
//! and its errors and panics surface as script errors and never reach
//! the world.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{
    Audio, Kind, Module, ModuleContext, ModuleError, ModuleEvent, ModuleResult, ObjectId, Runtime,
};
use luma_archive::{PackageBuilder, Vfs};
use luma_compiler::{Compiler, Natives};
use luma_script::Value;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-modules-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What the test sees of the module from outside.
#[derive(Default)]
struct Seen {
    /// The counters alive, by the module's number.
    values: HashMap<ObjectId, i32>,
    /// The `Update` events each counter received.
    updates: HashMap<ObjectId, u32>,
    /// Every event delivered, in order.
    events: Vec<ModuleEvent>,
    /// The scene path each scene event carried.
    scenes: Vec<Option<String>>,
    /// The numbers destroyed.
    destroyed: Vec<ObjectId>,
    next: ObjectId,
}

type Shared = Rc<RefCell<Seen>>;

struct CounterModule {
    seen: Shared,
}

impl Module for CounterModule {
    fn name(&self) -> &str {
        "counter"
    }

    fn classes(&self) -> Vec<String> {
        vec!["Counter".to_string()]
    }

    fn construct(
        &mut self,
        class: &str,
        args: &[Value],
        cx: &mut ModuleContext,
    ) -> ModuleResult<ObjectId> {
        assert_eq!(class, "Counter");
        let start = match args.first() {
            Some(Value::Int(n)) => *n,
            None => 0,
            Some(_) => return Err(ModuleError::new("Counter takes a number")),
        };
        let mut seen = self.seen.borrow_mut();
        seen.next += 1;
        let id = seen.next;
        seen.values.insert(id, start);
        cx.subscribe(ModuleEvent::Update);
        cx.subscribe(ModuleEvent::SceneDrawBegin);
        cx.subscribe(ModuleEvent::SceneDrawEnd);
        cx.subscribe(ModuleEvent::SceneInit);
        cx.subscribe(ModuleEvent::SceneShutdown);
        cx.subscribe(ModuleEvent::GameBeforeSave);
        cx.subscribe(ModuleEvent::GameAfterLoad);
        cx.log(format!("counter {id} starts at {start}"));
        Ok(id)
    }

    fn restore(
        &mut self,
        class: &str,
        state: &[u8],
        _cx: &mut ModuleContext,
    ) -> ModuleResult<ObjectId> {
        assert_eq!(class, "Counter");
        let bytes: [u8; 8] = state
            .try_into()
            .map_err(|_| ModuleError::new("a counter's state is 8 bytes"))?;
        let mut seen = self.seen.borrow_mut();
        seen.next += 1;
        let id = seen.next;
        seen.values
            .insert(id, i32::from_le_bytes(bytes[..4].try_into().unwrap()));
        seen.updates
            .insert(id, u32::from_le_bytes(bytes[4..].try_into().unwrap()));
        Ok(id)
    }

    fn call_method(
        &mut self,
        object: ObjectId,
        name: &str,
        args: &[Value],
        _cx: &mut ModuleContext,
    ) -> ModuleResult<Option<Value>> {
        let mut seen = self.seen.borrow_mut();
        let value = seen.values.get_mut(&object).unwrap();
        match name {
            "Add" => {
                if let Some(Value::Int(n)) = args.first() {
                    *value += n;
                }
                Ok(Some(Value::Null))
            }
            "Get" => Ok(Some(Value::Int(*value))),
            "Updates" => Ok(Some(Value::Int(
                seen.updates.get(&object).copied().unwrap_or(0) as i32,
            ))),
            "Fail" => Err(ModuleError::new("the counter refuses")),
            "Boom" => panic!("the counter blew up"),
            "Huge" => Ok(Some(Value::String("x".repeat(1 << 20)))),
            "Object" => Ok(Some(Value::object())),
            "Stop" => {
                _cx.unsubscribe(ModuleEvent::Update);
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        }
    }

    fn get_property(&self, object: ObjectId, name: &str) -> Option<Value> {
        match name {
            "Value" => self
                .seen
                .borrow()
                .values
                .get(&object)
                .map(|v| Value::Int(*v)),
            _ => None,
        }
    }

    fn set_property(
        &mut self,
        object: ObjectId,
        name: &str,
        value: &Value,
        _cx: &mut ModuleContext,
    ) -> ModuleResult<bool> {
        match (name, value) {
            ("Value", Value::Int(n)) => {
                self.seen.borrow_mut().values.insert(object, *n);
                Ok(true)
            }
            ("Value", _) => Err(ModuleError::new("Value is a number")),
            _ => Ok(false),
        }
    }

    fn destruct(&mut self, object: ObjectId) {
        let mut seen = self.seen.borrow_mut();
        seen.values.remove(&object);
        seen.destroyed.push(object);
    }

    fn save(&mut self, object: ObjectId) -> ModuleResult<Vec<u8>> {
        let seen = self.seen.borrow();
        let mut bytes = seen.values[&object].to_le_bytes().to_vec();
        bytes.extend(
            seen.updates
                .get(&object)
                .copied()
                .unwrap_or(0)
                .to_le_bytes(),
        );
        Ok(bytes)
    }

    fn event(&mut self, object: ObjectId, event: ModuleEvent, cx: &mut ModuleContext) -> bool {
        let mut seen = self.seen.borrow_mut();
        seen.events.push(event);
        if event == ModuleEvent::Update {
            *seen.updates.entry(object).or_insert(0) += 1;
        }
        if matches!(event, ModuleEvent::SceneInit | ModuleEvent::SceneShutdown) {
            seen.scenes.push(cx.scene().map(str::to_string));
        }
        true
    }
}

fn compile(source: &str, name: &str) -> Vec<u8> {
    Compiler::new(Natives::wme().function("Counter"))
        .compile(source.as_bytes(), name, &mut |_: &[u8]| None)
        .unwrap()
}

const GAME_SCRIPT: &str = r#"
global result = new Object();
global c = new Counter(5);
c.Add(3);
result.got = c.Get();
result.value = c.Value;
c.Value = 40;
result.value2 = c.Value;
result.text = "" + c;
result.ready = c.Ready;
c.Ready = false;
result.ready2 = c.Ready;
c.own = 7;
result.own = c.own;
result.valid = Game.ValidObject(c);
result.missing = c.NoSuchMethod();
result.err = c.Fail();
result.afterErr = c.Get();
result.bad = new Counter("text");
result.huge = c.Huge();
result.object = c.Object();
Game.ChangeScene("scenes\a.scene");
"#;

fn game(script: &str) -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 32,32 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Modules\" SCRIPT=\"game.script\" }".to_vec(),
    );
    pkg.add_file("game.script", compile(script, "game.script"));
    for scene in ["a", "b"] {
        pkg.add_file(
            format!("scenes\\{scene}.scene"),
            b"SCENE { LAYER { NAME=\"main\" MAIN=TRUE WIDTH=32 HEIGHT=32 } }".to_vec(),
        );
    }
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

fn open(dir: &std::path::Path, seen: Option<&Shared>) -> Runtime {
    let vfs = Vfs::open_game(dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    if let Some(seen) = seen {
        runtime
            .world
            .register_module(Box::new(CounterModule { seen: seen.clone() }))
            .unwrap();
    }
    runtime.start();
    runtime
}

fn result(runtime: &Runtime, key: &str) -> Value {
    runtime
        .engine
        .global("result")
        .and_then(Value::as_native)
        .and_then(|id| runtime.world.objects().get(id))
        .and_then(|native| native.props.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

fn errors(log: &[String]) -> Vec<&String> {
    log.iter()
        .filter(|l| l.starts_with("Runtime error"))
        .collect()
}

/// The handle and the module object of the only `Counter`.
fn counter(runtime: &Runtime) -> (luma_script::NativeId, luma_adventure::ModuleObject) {
    let found: Vec<_> = runtime
        .world
        .objects()
        .iter()
        .filter_map(|(id, n)| match &n.kind {
            Kind::Module(o) => Some((id, o.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

#[test]
fn a_script_constructs_calls_reads_and_writes_a_module_object() {
    let dir = game(GAME_SCRIPT);
    let seen: Shared = Shared::default();
    let mut runtime = open(&dir, Some(&seen));
    let mut log = runtime.tick(16);
    log.extend(runtime.tick(16));

    assert_eq!(result(&runtime, "got"), Value::Int(8));
    assert_eq!(result(&runtime, "value"), Value::Int(8), "a property read");
    assert_eq!(
        result(&runtime, "value2"),
        Value::Int(40),
        "a property write"
    );
    assert_eq!(result(&runtime, "text"), Value::from("[module object]"));
    assert_eq!(result(&runtime, "ready"), Value::Bool(true));
    assert_eq!(result(&runtime, "ready2"), Value::Bool(false));
    assert_eq!(result(&runtime, "own"), Value::Int(7), "a script property");
    assert_eq!(result(&runtime, "valid"), Value::Bool(true));
    assert!(
        log.iter().any(|l| l == "[counter] counter 1 starts at 5"),
        "{log:?}"
    );

    // What the module refuses or lacks is a script error and a null.
    assert_eq!(result(&runtime, "missing"), Value::Null);
    assert_eq!(result(&runtime, "err"), Value::Null);
    assert_eq!(result(&runtime, "afterErr"), Value::Int(40), "still alive");
    assert_eq!(result(&runtime, "bad"), Value::Null);
    assert_eq!(result(&runtime, "huge"), Value::Null, "over the size limit");
    assert_eq!(result(&runtime, "object"), Value::Null, "no script objects");
    let errs = errors(&log);
    for needle in [
        "NoSuchMethod",
        "the counter refuses",
        "Counter takes a number",
        "over the limit",
        "script object",
    ] {
        assert!(
            errs.iter().any(|l| l.contains(needle)),
            "{needle}: {errs:?}"
        );
    }
    assert!(errs.iter().all(|l| l.contains("game.script")), "{errs:?}");
}

#[test]
fn a_module_object_gets_the_events_it_subscribed_to() {
    let dir = game(GAME_SCRIPT);
    let seen: Shared = Shared::default();
    let mut runtime = open(&dir, Some(&seen));
    // One update per frame since the object exists; the scene change the
    // script asked for waits for the fade to black.
    let mut frames = 0;
    while seen.borrow().scenes.is_empty() {
        let log = runtime.tick(16);
        frames += 1;
        assert!(frames < 200, "the scene never came: {log:?}");
    }
    let (_, object) = counter(&runtime);
    {
        let seen = seen.borrow();
        assert_eq!(seen.updates[&object.id], frames);
        // `Game.ChangeScene` happens at a frame; the scene init came with
        // it and carries the scene's path.
        assert!(seen.events.contains(&ModuleEvent::SceneInit));
        assert_eq!(seen.scenes, vec![Some("scenes\\a.scene".to_string())]);
        // The drawing events come in pairs once a scene is there.
        let begin = seen
            .events
            .iter()
            .position(|e| *e == ModuleEvent::SceneDrawBegin)
            .expect("a draw begin");
        assert_eq!(seen.events[begin + 1], ModuleEvent::SceneDrawEnd);
    }
    runtime.change_scene("scenes\\b.scene").unwrap();
    let seen = seen.borrow();
    assert_eq!(
        seen.scenes[1..],
        [
            Some("scenes\\a.scene".to_string()),
            Some("scenes\\b.scene".to_string())
        ],
        "shutdown names the old scene, init the new one"
    );
}

#[test]
fn a_module_object_can_unsubscribe_and_is_destroyed_with_unload_object() {
    let script = format!("{GAME_SCRIPT}\nglobal early = c.Updates();\nc.Stop();\n");
    let dir = game(&script);
    let seen: Shared = Shared::default();
    let mut runtime = open(&dir, Some(&seen));
    runtime.tick(16);
    let before = seen.borrow().updates.values().copied().next().unwrap_or(0);
    runtime.tick(16);
    runtime.tick(16);
    let after = seen.borrow().updates.values().copied().next().unwrap_or(0);
    assert_eq!(before, after, "unsubscribed from Update");
    let (id, object) = counter(&runtime);
    let engine = &mut runtime.engine;
    assert!(runtime.world.unload_object(engine, id));
    assert_eq!(seen.borrow().destroyed, vec![object.id]);
    assert!(!runtime.world.objects().contains(id));
}

#[test]
fn a_module_object_survives_a_saved_game() {
    let dir = game(GAME_SCRIPT);
    let seen: Shared = Shared::default();
    let mut runtime = open(&dir, Some(&seen));
    runtime.tick(16);
    runtime.tick(16);
    let (handle, before) = counter(&runtime);
    assert_eq!(seen.borrow().values[&before.id], 40);
    let updates = seen.borrow().updates[&before.id];

    runtime.save_game(1, "modules", true).unwrap();
    assert!(
        seen.borrow().events.contains(&ModuleEvent::GameBeforeSave),
        "BeforeSave reached the module"
    );
    // The state wrote itself into the file, not into the live object.
    assert!(counter(&runtime).1.state.is_empty());

    // The game moves on, then the save is loaded.
    seen.borrow_mut().values.insert(before.id, 999);
    runtime.tick(16);
    runtime.tick(16);
    runtime.load_game(1).unwrap();
    let log = runtime.world.take_log();
    assert!(errors(&log).is_empty(), "{log:?}");

    let (same_handle, after) = counter(&runtime);
    assert_eq!(handle, same_handle, "the script's handle is still good");
    assert_ne!(before.id, after.id, "the module numbered it again");
    assert_eq!(after.class, "Counter");
    assert!(after.state.is_empty());
    assert_ne!(after.events & 1, 0, "subscribed to Update, as saved");
    {
        let seen = seen.borrow();
        assert_eq!(seen.values[&after.id], 40, "the saved value, not 999");
        assert_eq!(seen.updates[&after.id], updates);
        assert!(seen.destroyed.contains(&before.id), "the old object went");
        assert!(seen.events.contains(&ModuleEvent::GameAfterLoad));
    }
    // And it goes on being updated and answering the script's variable.
    runtime.tick(16);
    assert_eq!(seen.borrow().updates[&after.id], updates + 1);
    assert_eq!(
        runtime.engine.global("c").and_then(|v| v.as_native()),
        Some(handle)
    );
    assert_eq!(
        runtime.world.objects().get(handle).map(|n| n.kind.clone()),
        Some(Kind::Module(after))
    );
}

#[test]
fn a_load_that_needs_a_class_nobody_provides_is_refused_and_changes_nothing() {
    let dir = game(GAME_SCRIPT);
    let seen: Shared = Shared::default();
    let mut with = open(&dir, Some(&seen));
    with.tick(16);
    with.tick(16);
    with.save_game(2, "modules", true).unwrap();
    drop(with);

    // The same game, run without the module.
    let mut without = open(&dir, None);
    without.tick(16);
    let before = without.world.objects().len();
    let err = without.load_game(2).unwrap_err();
    assert!(
        err.contains("Counter") && err.contains("does not provide"),
        "{err}"
    );
    assert_eq!(without.world.objects().len(), before, "the world is intact");
    without.tick(16);
}

#[test]
fn a_module_that_cannot_restore_refuses_the_load_and_destroys_what_it_made() {
    let dir = game(GAME_SCRIPT);
    let seen: Shared = Shared::default();
    let mut runtime = open(&dir, Some(&seen));
    runtime.tick(16);
    runtime.tick(16);
    runtime.save_game(3, "modules", true).unwrap();
    // A game whose module of that name cannot read the file's state.
    struct Empty(Shared);
    impl Module for Empty {
        fn name(&self) -> &str {
            "counter"
        }
        fn classes(&self) -> Vec<String> {
            vec!["Counter".into()]
        }
        fn construct(
            &mut self,
            _: &str,
            _: &[Value],
            _: &mut ModuleContext,
        ) -> ModuleResult<ObjectId> {
            Ok(1)
        }
        fn restore(&mut self, _: &str, _: &[u8], _: &mut ModuleContext) -> ModuleResult<ObjectId> {
            self.0.borrow_mut().next += 1;
            Err(ModuleError::new("old format"))
        }
        fn call_method(
            &mut self,
            _: ObjectId,
            _: &str,
            _: &[Value],
            _: &mut ModuleContext,
        ) -> ModuleResult<Option<Value>> {
            Ok(None)
        }
        fn destruct(&mut self, _: ObjectId) {}
    }
    let other: Shared = Shared::default();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut refusing = Runtime::with_audio(vfs, Audio::null()).unwrap();
    refusing
        .world
        .register_module(Box::new(Empty(other.clone())))
        .unwrap();
    refusing.start();
    refusing.tick(16);
    let err = refusing.load_game(3).unwrap_err();
    assert!(err.contains("old format"), "{err}");
    assert_eq!(other.borrow().next, 1, "the module was asked once");
}

#[test]
fn a_module_that_panics_is_stopped_and_the_world_goes_on() {
    let script = format!("{GAME_SCRIPT}\nglobal boom = c.Boom();\nglobal again = c.Get();\n");
    let dir = game(&script);
    let seen: Shared = Shared::default();
    let mut runtime = open(&dir, Some(&seen));
    let mut log = runtime.tick(16);
    log.extend(runtime.tick(16));
    log.extend(runtime.tick(16));
    let errs = errors(&log);
    assert!(
        errs.iter()
            .any(|l| l.contains("panicked: the counter blew up")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|l| l.contains("was stopped after it panicked")),
        "{errs:?}"
    );
    assert_eq!(
        runtime
            .engine
            .global("again")
            .cloned()
            .unwrap_or(Value::Null),
        Value::Null
    );
    // The world still runs and a save still works: the stopped module's
    // objects are dropped, with a warning, instead of blocking it.
    runtime.tick(16);
    runtime.save_game(4, "x", true).unwrap();
    let log = runtime.world.take_log();
    assert!(
        log.iter()
            .any(|l| l.starts_with("Warning:") && l.contains("stopped module 'counter'")),
        "{log:?}"
    );
    assert_eq!(module_objects(&runtime), 0);
    runtime.load_game(4).unwrap();
    assert!(runtime.world.objects().len() > 3);
}

/// A module with set numbers and sizes, for the limits.
struct Fixed {
    id: ObjectId,
    step: ObjectId,
    state: usize,
    panic_in_name: bool,
}

impl Fixed {
    fn new(id: ObjectId, step: ObjectId, state: usize) -> Fixed {
        Fixed {
            id,
            step,
            state,
            panic_in_name: false,
        }
    }

    fn next(&mut self) -> ObjectId {
        let id = self.id;
        self.id += self.step;
        id
    }
}

impl Module for Fixed {
    fn name(&self) -> &str {
        assert!(!self.panic_in_name, "no name");
        "fixed"
    }
    fn classes(&self) -> Vec<String> {
        vec!["Counter".into()]
    }
    fn construct(&mut self, _: &str, _: &[Value], _: &mut ModuleContext) -> ModuleResult<ObjectId> {
        Ok(self.next())
    }
    fn restore(&mut self, _: &str, _: &[u8], _: &mut ModuleContext) -> ModuleResult<ObjectId> {
        Ok(self.next())
    }
    fn call_method(
        &mut self,
        _: ObjectId,
        _: &str,
        _: &[Value],
        _: &mut ModuleContext,
    ) -> ModuleResult<Option<Value>> {
        Ok(None)
    }
    fn destruct(&mut self, _: ObjectId) {}
    fn save(&mut self, _: ObjectId) -> ModuleResult<Vec<u8>> {
        Ok(vec![0; self.state])
    }
}

fn open_fixed(script: &str, fixed: Fixed) -> Runtime {
    let dir = game(script);
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.world.register_module(Box::new(fixed)).unwrap();
    runtime.start();
    runtime
}

fn module_objects(runtime: &Runtime) -> usize {
    runtime
        .world
        .objects()
        .iter()
        .filter(|(_, n)| matches!(n.kind, Kind::Module(_)))
        .count()
}

#[test]
fn a_module_cannot_hand_out_a_number_that_is_alive() {
    let script = "global a = new Counter(); global b = new Counter();";
    let mut runtime = open_fixed(script, Fixed::new(7, 0, 0));
    let log = runtime.tick(16);
    assert!(
        errors(&log).iter().any(|l| l.contains("already in use")),
        "{log:?}"
    );
    assert_eq!(module_objects(&runtime), 1, "the second was refused");
    assert_eq!(runtime.engine.global("b").cloned(), Some(Value::Null));
}

#[test]
fn a_load_whose_numbers_collide_is_refused_and_changes_nothing() {
    let mut runtime = open_fixed("global a = new Counter();", Fixed::new(7, 0, 0));
    runtime.tick(16);
    runtime.save_game(5, "x", true).unwrap();
    // The module answers 7 again for the restored object, the number of
    // the live one.
    let err = runtime.load_game(5).unwrap_err();
    assert!(err.contains("already in use"), "{err}");
    assert_eq!(module_objects(&runtime), 1);
    runtime.tick(16);
}

#[test]
fn the_state_of_all_objects_together_is_bounded() {
    let script = "global a = new Counter(); global b = new Counter(); \
        global c = new Counter(); global d = new Counter(); global e = new Counter();";
    let big = luma_adventure::module::MAX_STATE_BYTES;
    let mut runtime = open_fixed(script, Fixed::new(1, 1, big));
    runtime.tick(16);
    assert_eq!(module_objects(&runtime), 5);
    // Each is within its own limit; together they are over 64 MiB.
    let err = runtime.save_game(6, "big", true).unwrap_err();
    assert!(err.contains("together"), "{err}");
    // Nothing of the failed save stays in the live objects.
    assert!(
        runtime
            .world
            .objects()
            .iter()
            .all(|(_, n)| !matches!(&n.kind, Kind::Module(o) if !o.state.is_empty()))
    );
}

#[test]
fn error_messages_are_capped_on_a_character_boundary() {
    let long = "\u{e9}".repeat(4000);
    let error = ModuleError::new(long.clone());
    assert!(error.message().len() <= luma_adventure::module::MAX_ERROR_BYTES);
    assert!(long.starts_with(error.message()));
    assert_eq!(ModuleError::from(long).message(), error.message());
}

#[test]
fn a_module_that_panics_in_name_is_refused() {
    let dir = game(GAME_SCRIPT);
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    let mut fixed = Fixed::new(0, 0, 0);
    fixed.panic_in_name = true;
    let err = runtime.world.register_module(Box::new(fixed)).unwrap_err();
    assert!(err.message().contains("name"), "{err}");
}

#[test]
fn registration_refuses_what_would_confuse_a_script() {
    struct Named(&'static str, Vec<&'static str>);
    impl Module for Named {
        fn name(&self) -> &str {
            self.0
        }
        fn classes(&self) -> Vec<String> {
            self.1.iter().map(|c| c.to_string()).collect()
        }
        fn construct(
            &mut self,
            _: &str,
            _: &[Value],
            _: &mut ModuleContext,
        ) -> ModuleResult<ObjectId> {
            Ok(0)
        }
        fn restore(&mut self, _: &str, _: &[u8], _: &mut ModuleContext) -> ModuleResult<ObjectId> {
            Ok(0)
        }
        fn call_method(
            &mut self,
            _: ObjectId,
            _: &str,
            _: &[Value],
            _: &mut ModuleContext,
        ) -> ModuleResult<Option<Value>> {
            Ok(None)
        }
        fn destruct(&mut self, _: ObjectId) {}
    }
    let dir = game(GAME_SCRIPT);
    let mut runtime = open(&dir, None);
    let world = &mut runtime.world;
    assert!(
        world
            .register_module(Box::new(Named("a", vec!["One", "Two"])))
            .is_ok()
    );
    assert_eq!(world.module_classes(), vec!["One", "Two"]);
    for (module, why) in [
        (Named("a", vec!["Three"]), "name taken"),
        (Named("b", vec!["One"]), "class taken"),
        (Named("c", vec!["Array"]), "engine class"),
        (Named("d", vec!["Not Valid"]), "bad name"),
        (Named("e", vec![""]), "empty class"),
        (Named("", vec!["Four"]), "empty module name"),
        (Named("f", vec!["Dup", "Dup"]), "listed twice"),
    ] {
        assert!(world.register_module(Box::new(module)).is_err(), "{why}");
    }
    assert_eq!(world.module_classes(), vec!["One", "Two"], "nothing leaked");
}
