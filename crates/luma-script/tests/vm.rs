//! The virtual machine, exercised with assembled programs and a mock host.

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use luma_script::asm::Assembler;
use luma_script::{
    CallType, Context, Effect, Engine, ExternalType, Host, Instruction, NativeId, Natives, Return,
    ScriptId, State, Value, rgba,
};

/// A native object of the mock host: a class, a name, whether it is
/// "ready" for scripts waiting on it, and the properties scripts set.
#[derive(Default)]
struct Object {
    class: &'static str,
    name: String,
    ready: bool,
    alive: bool,
    props: BTreeMap<String, Value>,
}

#[derive(Default)]
struct MockHost {
    files: HashMap<String, Vec<u8>>,
    log: Vec<String>,
    timer: u64,
    clock: Cell<u64>,
    frozen: bool,
    objects: HashMap<NativeId, Object>,
    /// Method calls received, as `name(args)` on `object`.
    calls: Vec<String>,
    /// Events the host was asked to raise through `ApplyEvent`.
    raised: Vec<(NativeId, String)>,
}

impl MockHost {
    fn with_script(bytes: Vec<u8>) -> MockHost {
        let mut host = MockHost::default();
        host.files.insert("main.script".to_string(), bytes);
        host
    }

    fn add_file(&mut self, name: &str, bytes: Vec<u8>) {
        self.files.insert(name.to_string(), bytes);
    }

    fn add_object(&mut self, id: u64, class: &'static str, name: &str) -> NativeId {
        self.objects.insert(
            NativeId(id),
            Object {
                class,
                name: name.to_string(),
                ready: true,
                alive: true,
                props: BTreeMap::new(),
            },
        );
        NativeId(id)
    }

    fn errors(&self) -> Vec<&str> {
        self.log
            .iter()
            .filter(|l| l.starts_with("Runtime error") || l.starts_with("Fatal"))
            .map(String::as_str)
            .collect()
    }
}

impl Natives for MockHost {
    fn class_name(&self, id: NativeId) -> String {
        self.objects
            .get(&id)
            .map(|o| o.class.to_string())
            .unwrap_or_default()
    }

    fn to_string(&self, id: NativeId) -> String {
        self.objects
            .get(&id)
            .map(|o| o.name.clone())
            .unwrap_or_default()
    }

    fn to_bool(&self, id: NativeId) -> bool {
        self.objects.contains_key(&id)
    }

    fn compare(&self, a: NativeId, b: NativeId) -> Ordering {
        a.cmp(&b)
    }

    fn get_property(&self, id: NativeId, name: &str) -> Option<Value> {
        let object = self.objects.get(&id)?;
        match name {
            "Name" => Some(Value::string(&object.name)),
            "Ready" => Some(Value::Bool(object.ready)),
            _ => object.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let Some(object) = self.objects.get_mut(&id) else {
            return false;
        };
        match name {
            "Name" => object.name = value.get_string(&()),
            "Ready" => object.ready = value.get_bool(&()),
            _ => {
                object.props.insert(name.to_string(), value.clone());
            }
        }
        true
    }
}

impl Host for MockHost {
    fn read_script(&mut self, filename: &str) -> Option<Vec<u8>> {
        self.files.get(filename).cloned()
    }

    fn log(&mut self, message: &str) {
        self.log.push(message.to_string());
    }

    fn timer(&self) -> u64 {
        self.timer
    }

    fn clock(&self) -> u64 {
        // Every look at the clock costs a millisecond, so that time
        // slices run out.
        let now = self.clock.get();
        self.clock.set(now + 1);
        now
    }

    fn is_frozen(&self) -> bool {
        self.frozen
    }

    fn valid_object(&self, id: NativeId) -> bool {
        self.objects.get(&id).is_some_and(|o| o.alive)
    }

    fn is_ready(&self, id: NativeId) -> bool {
        self.objects.get(&id).is_some_and(|o| o.ready)
    }

    fn random(&mut self, from: i32, _to: i32) -> i32 {
        from
    }

    fn call_function(&mut self, name: &str, args: &[Value], ctx: Context<'_>) -> Option<Return> {
        match name {
            "Echo" => Some(Return::value(args.first().cloned().unwrap_or(Value::Null))),
            "Count" => Some(Return::value(args.len() as i32)),
            "Line" => Some(Return::value(ctx.line as i32)),
            "Owner" => Some(Return::value(ctx.owner.map_or(Value::Null, Value::Native))),
            "ScriptCount" => Some(Return::value(ctx.engine.scripts().len() as i32)),
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
        let shown: Vec<String> = args.iter().map(|a| a.get_string(&())).collect();
        self.calls.push(format!(
            "{}.{name}({})",
            self.objects[&target].name,
            shown.join(",")
        ));
        match name {
            "GoTo" => {
                self.objects.get_mut(&target)?.ready = false;
                Some(Return::effect(Effect::WaitForExclusive(target)))
            }
            "Nap" => Some(Return::effect(Effect::Sleep(
                args.first().map_or(0, |a| a.get_int(&())) as u32,
            ))),
            "Rename" => {
                self.objects.get_mut(&target)?.name = args.first()?.get_string(&());
                Some(Return::value(true))
            }
            "ApplyEvent" => {
                let event = args.first()?.get_string(&());
                self.raised.push((target, event.clone()));
                let handled = ctx.engine.apply_event(self, target, &event, false);
                Some(Return::value(handled))
            }
            "Native" => Some(Return::value("native")),
            _ => None,
        }
    }
}

/// Runs `main.script` with no owner and ticks once.
fn run(bytes: Vec<u8>) -> (Engine, MockHost, ScriptId) {
    let mut host = MockHost::with_script(bytes);
    let mut engine = Engine::new();
    let id = engine.run_script(&mut host, "main.script", None).unwrap();
    engine.tick(&mut host);
    (engine, host, id)
}

/// Stores the top of the stack in the engine global `result` and ends.
fn store_result(a: &mut Assembler) {
    a.def_glob_var("result");
    a.pop_var("result");
    a.emit(Instruction::Ret);
}

fn result(engine: &Engine) -> Value {
    engine.global("result").cloned().unwrap_or(Value::Null)
}

#[test]
fn arithmetic_and_globals() {
    // result = (1 + 2) * 3 - 10 / 4 + "x"
    let mut a = Assembler::new("main.script");
    a.push_int(1);
    a.push_int(2);
    a.emit(Instruction::Add);
    a.push_int(3);
    a.emit(Instruction::Mul);
    a.push_int(10);
    a.push_int(4);
    a.emit(Instruction::Div);
    a.emit(Instruction::Sub);
    a.push_string("x");
    a.emit(Instruction::Add);
    store_result(&mut a);
    let (engine, host, id) = run(a.build());
    assert_eq!(result(&engine), Value::from("6.500000x"));
    assert!(host.errors().is_empty(), "{:?}", host.log);
    // A script without handlers is gone after the tick.
    assert!(engine.script(id).is_none());
    assert_eq!(engine.counts().total, 0);
}

#[test]
fn comparisons_and_logic() {
    let mut a = Assembler::new("main.script");
    a.def_glob_var("result");
    a.emit(Instruction::CreateObject);
    a.pop_var("result");
    for (lhs, rhs, op, name) in [
        (1, 2, Instruction::CmpL, "l"),
        (2, 2, Instruction::CmpLe, "le"),
        (3, 2, Instruction::CmpG, "g"),
        (3, 3, Instruction::CmpGe, "ge"),
        (3, 3, Instruction::CmpEq, "eq"),
        (3, 4, Instruction::CmpNe, "ne"),
    ] {
        a.push_int(lhs);
        a.push_int(rhs);
        a.emit(op);
        a.push_var_ref("result");
        a.push_string(name);
        a.emit(Instruction::PopByExp);
    }
    a.push_int(1);
    a.push_bool(true);
    a.emit(Instruction::CmpStrictEq);
    a.push_var_ref("result");
    a.push_string("strict");
    a.emit(Instruction::PopByExp);
    a.push_int(1);
    a.push_bool(true);
    a.emit(Instruction::CmpStrictNe);
    a.push_var_ref("result");
    a.push_string("strict_ne");
    a.emit(Instruction::PopByExp);
    a.push_bool(true);
    a.push_int(0);
    a.emit(Instruction::And);
    a.push_var_ref("result");
    a.push_string("and");
    a.emit(Instruction::PopByExp);
    a.push_bool(false);
    a.push_string("yes");
    a.emit(Instruction::Or);
    a.push_var_ref("result");
    a.push_string("or");
    a.emit(Instruction::PopByExp);
    a.emit(Instruction::PushNull);
    a.emit(Instruction::Not);
    a.push_var_ref("result");
    a.push_string("not_null");
    a.emit(Instruction::PopByExp);
    a.emit(Instruction::Ret);
    let (engine, host, _) = run(a.build());
    let props = result(&engine).properties().cloned().unwrap();
    let get = |k: &str| props[k].get_bool(&());
    assert!(get("l") && get("le") && get("g") && get("ge") && get("eq") && get("ne"));
    assert!(!get("strict") && get("strict_ne"));
    assert!(!get("and") && get("or") && get("not_null"));
    assert!(host.errors().is_empty());
}

#[test]
fn loops_with_locals() {
    // var i = 0; var sum = 0; while (i < 5) { i = i + 1; sum = sum + i; }
    let mut a = Assembler::new("main.script");
    a.def_var("i");
    a.push_int(0);
    a.pop_var("i");
    a.def_var("sum");
    a.push_int(0);
    a.pop_var("sum");
    let top = a.label();
    let exit = a.label();
    a.bind(top);
    a.push_var("i");
    a.push_int(5);
    a.emit(Instruction::CmpL);
    a.jmp_false(exit);
    a.push_var("i");
    a.push_int(1);
    a.emit(Instruction::Add);
    a.pop_var("i");
    a.push_var("sum");
    a.push_var("i");
    a.emit(Instruction::Add);
    a.pop_var("sum");
    a.jmp(top);
    a.bind(exit);
    a.push_var("sum");
    store_result(&mut a);
    let (engine, host, _) = run(a.build());
    assert_eq!(result(&engine), Value::Int(15));
    assert!(host.errors().is_empty());
}

#[test]
fn functions_take_parameters_and_return_values() {
    // function add(x, y) { return x + y; }   result = add(40, 2, 99)
    let mut a = Assembler::new("main.script");
    let add = a.label();
    let end = a.label();
    a.push_int(99);
    a.push_int(2);
    a.push_int(40);
    a.push_int(3);
    a.call(add);
    store_result(&mut a);
    a.jmp(end);
    a.bind(add);
    a.function("add");
    a.emit(Instruction::Scope);
    a.emit(Instruction::CorrectStack(2));
    a.def_var("x");
    a.pop_var("x");
    a.def_var("y");
    a.pop_var("y");
    a.push_var("x");
    a.push_var("y");
    a.emit(Instruction::Add);
    a.emit(Instruction::Ret);
    a.bind(end);
    a.emit(Instruction::Ret);
    let (engine, host, _) = run(a.build());
    assert_eq!(result(&engine), Value::Int(42));
    assert!(host.errors().is_empty(), "{:?}", host.log);
}

#[test]
fn missing_parameters_are_null_and_locals_shadow_globals() {
    // global g = 1; function f(p, q) { var g = p; return q; } result = f(5)
    let mut a = Assembler::new("main.script");
    let f = a.label();
    let end = a.label();
    a.def_glob_var("g");
    a.push_int(1);
    a.pop_var("g");
    a.push_int(5);
    a.push_int(1);
    a.call(f);
    store_result(&mut a);
    a.jmp(end);
    a.bind(f);
    a.emit(Instruction::Scope);
    a.emit(Instruction::CorrectStack(2));
    a.def_var("p");
    a.pop_var("p");
    a.def_var("q");
    a.pop_var("q");
    a.def_var("g");
    a.push_var("p");
    a.pop_var("g");
    a.push_var("q");
    a.emit(Instruction::Ret);
    a.bind(end);
    a.emit(Instruction::Ret);
    let (engine, host, _) = run(a.build());
    assert_eq!(result(&engine), Value::Null);
    assert_eq!(engine.global("g"), Some(&Value::Int(1)));
    assert!(host.errors().is_empty());
}

#[test]
fn undefined_variables_are_created_with_a_warning() {
    let mut a = Assembler::new("main.script");
    a.push_var("nobody");
    store_result(&mut a);
    let (engine, host, _) = run(a.build());
    assert_eq!(result(&engine), Value::Null);
    assert!(
        host.log
            .iter()
            .any(|l| l.contains("variable 'nobody' is inaccessible")),
        "{:?}",
        host.log
    );
}

#[test]
fn objects_and_properties() {
    // var o = new Object(); o.x = 1; o.y = o.x + 1; result = o
    let mut a = Assembler::new("main.script");
    a.def_var("o");
    a.emit(Instruction::CreateObject);
    a.pop_var("o");
    a.push_int(1);
    a.push_var_ref("o");
    a.push_string("x");
    a.emit(Instruction::PopByExp);
    a.push_var_ref("o");
    a.push_string("x");
    a.emit(Instruction::PushByExp);
    a.push_int(1);
    a.emit(Instruction::Add);
    a.push_var_ref("o");
    a.push_string("y");
    a.emit(Instruction::PopByExp);
    // A property set through a copy is lost.
    a.push_int(9);
    a.push_var("o");
    a.push_string("z");
    a.emit(Instruction::PopByExp);
    a.push_var("o");
    store_result(&mut a);
    let (engine, host, _) = run(a.build());
    let mut expected = Value::object();
    expected.set_property("x", Value::Int(1), &mut ());
    expected.set_property("y", Value::Int(2), &mut ());
    assert_eq!(result(&engine), expected);
    assert!(host.errors().is_empty());
}

#[test]
fn switch_uses_the_register() {
    // switch (2) { case 1: r = "one"; case 2: r = "two"; }
    let mut a = Assembler::new("main.script");
    a.def_glob_var("result");
    a.push_int(2);
    a.emit(Instruction::PopReg1);
    let case2 = a.label();
    let end = a.label();
    a.emit(Instruction::PushReg1);
    a.push_int(1);
    a.emit(Instruction::CmpEq);
    a.jmp_false(case2);
    a.push_string("one");
    a.pop_var("result");
    a.jmp(end);
    a.bind(case2);
    a.emit(Instruction::PushReg1);
    a.push_int(2);
    a.emit(Instruction::CmpEq);
    a.jmp_false(end);
    a.push_string("two");
    a.pop_var("result");
    a.bind(end);
    a.emit(Instruction::Ret);
    let (engine, _, _) = run(a.build());
    assert_eq!(result(&engine), Value::from("two"));
}

#[test]
fn self_and_this_name_the_owner() {
    // result = self.Name; this.Caption = "hi"
    let mut a = Assembler::new("main.script");
    a.push_var_ref("self");
    a.push_string("Name");
    a.emit(Instruction::PushByExp);
    a.def_glob_var("result");
    a.pop_var("result");
    a.push_string("hi");
    a.push_var_ref("this");
    a.push_string("Caption");
    a.emit(Instruction::PopByExp);
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let crate_ = host.add_object(1, "Entity", "crate");
    let mut engine = Engine::new();
    engine
        .run_script(&mut host, "main.script", Some(crate_))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::from("crate"));
    assert_eq!(host.objects[&crate_].props["Caption"], Value::from("hi"));
}

#[test]
fn this_stack_feeds_method_calls() {
    // obj.Rename("box"); result = this.Name   (with `this` = obj)
    let mut a = Assembler::new("main.script");
    a.def_glob_var("obj");
    a.push_var("self");
    a.pop_var("obj");
    a.push_this("obj");
    a.push_string("box");
    a.push_int(1);
    a.push_var("obj");
    a.push_string("Rename");
    a.emit(Instruction::CallByExp);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::PushVarThis);
    a.push_string("Name");
    a.emit(Instruction::PushByExp);
    a.emit(Instruction::PopThis);
    store_result(&mut a);
    let mut host = MockHost::with_script(a.build());
    let obj = host.add_object(1, "Entity", "crate");
    let mut engine = Engine::new();
    engine
        .run_script(&mut host, "main.script", Some(obj))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(host.calls, ["crate.Rename(box)"]);
    assert_eq!(result(&engine), Value::from("box"));
}

#[test]
fn this_from_the_stack() {
    // (self).Name through PUSH_THIS_FROM_STACK and PUSH_VAR_THIS
    let mut a = Assembler::new("main.script");
    a.push_var("self");
    a.emit(Instruction::PushThisFromStack);
    a.emit(Instruction::PushVarThis);
    a.push_string("Name");
    a.emit(Instruction::PushByExp);
    a.def_glob_var("result");
    a.pop_var("result");
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::PopThis);
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let obj = host.add_object(1, "Entity", "crate");
    let mut engine = Engine::new();
    engine
        .run_script(&mut host, "main.script", Some(obj))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::from("crate"));
}

#[test]
fn builtin_functions() {
    let mut a = Assembler::new("main.script");
    a.def_glob_var("result");
    a.emit(Instruction::CreateObject);
    a.pop_var("result");
    let store = |a: &mut Assembler, name: &str| {
        a.push_var_ref("result");
        a.push_string(name);
        a.emit(Instruction::PopByExp);
    };
    a.push_float(2.5);
    a.push_int(1);
    a.external_call("ToString");
    store(&mut a, "str");
    a.push_string("12x");
    a.push_int(1);
    a.external_call("ToInt");
    store(&mut a, "int");
    a.push_string("1.5");
    a.push_int(1);
    a.external_call("ToFloat");
    store(&mut a, "float");
    a.push_string("yes");
    a.push_int(1);
    a.external_call("ToBool");
    store(&mut a, "bool");
    a.push_int(9);
    a.push_int(3);
    a.push_int(2);
    a.external_call("Random");
    store(&mut a, "random");
    a.push_int(3);
    a.push_int(2);
    a.push_int(1);
    a.push_int(3);
    a.external_call("RGB");
    store(&mut a, "rgb");
    a.push_int(rgba(1, 2, 3, 4));
    a.push_int(1);
    a.external_call("GetAValue");
    store(&mut a, "alpha");
    a.push_string("hello");
    a.push_int(1);
    a.external_call("LOG");
    store(&mut a, "log");
    a.push_int(7);
    a.push_int(1);
    a.external_call("Echo");
    store(&mut a, "echo");
    a.push_int(0);
    a.external_call("Line");
    store(&mut a, "line");
    a.push_int(0);
    a.external_call("Nothing");
    store(&mut a, "nothing");
    a.emit(Instruction::DbgLine(12));
    a.push_int(0);
    a.external_call("Line");
    store(&mut a, "line12");
    a.emit(Instruction::Ret);
    let (engine, host, _) = run(a.build());
    let props = result(&engine).properties().cloned().unwrap();
    assert_eq!(props["str"], Value::from("2.500000"));
    assert_eq!(props["int"], Value::Int(12));
    assert_eq!(props["float"], Value::Float(1.5));
    assert_eq!(props["bool"], Value::Bool(true));
    assert_eq!(props["random"], Value::Int(3));
    assert_eq!(props["rgb"], Value::Int(rgba(1, 2, 3, 255)));
    assert_eq!(props["alpha"], Value::Int(4));
    assert_eq!(props["log"], Value::Null);
    assert_eq!(props["echo"], Value::Int(7));
    assert_eq!(props["line"], Value::Int(0));
    assert_eq!(props["line12"], Value::Int(12));
    assert_eq!(props["nothing"], Value::Null);
    assert!(host.log.contains(&"sc: hello".to_string()));
    assert_eq!(host.errors().len(), 1);
    assert!(host.errors()[0].contains("Call to undefined function 'Nothing'"));
}

#[test]
fn external_dll_functions_are_unsupported() {
    let mut a = Assembler::new("main.script");
    a.external(
        "user32.dll",
        "MessageBeep",
        CallType::StdCall,
        ExternalType::Bool,
        &[ExternalType::Long],
    );
    a.push_int(0);
    a.push_int(1);
    a.external_call("MessageBeep");
    store_result(&mut a);
    let (engine, host, _) = run(a.build());
    assert_eq!(result(&engine), Value::Null);
    assert!(
        host.log
            .iter()
            .any(|l| l.contains("External functions are not supported"))
    );
}

#[test]
fn sleep_waits_for_the_game_timer() {
    let mut a = Assembler::new("main.script");
    a.def_glob_var("result");
    a.push_int(1);
    a.pop_var("result");
    a.push_int(100);
    a.push_int(1);
    a.external_call("Sleep");
    a.emit(Instruction::PopEmpty);
    a.push_int(2);
    a.pop_var("result");
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let mut engine = Engine::new();
    let id = engine.run_script(&mut host, "main.script", None).unwrap();
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::Int(1));
    assert_eq!(engine.script(id).unwrap().state(), State::Sleeping);
    host.timer = 99;
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::Int(1));
    host.timer = 100;
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::Int(2));
    assert!(engine.script(id).is_none());
}

#[test]
fn sleep_while_frozen_uses_the_clock() {
    let mut a = Assembler::new("main.script");
    a.push_int(50);
    a.push_int(1);
    a.external_call("Sleep");
    a.emit(Instruction::PopEmpty);
    a.push_int(1);
    store_result(&mut a);
    let mut host = MockHost::with_script(a.build());
    host.frozen = true;
    host.clock.set(1000);
    let mut engine = Engine::new();
    let id = engine.run_script(&mut host, "main.script", None).unwrap();
    engine.tick(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Sleeping);
    host.timer = 10_000;
    engine.tick(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Sleeping);
    host.clock.set(1100);
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::Int(1));
}

#[test]
fn waiting_for_an_object() {
    // self.GoTo(); result = "arrived"
    let mut a = Assembler::new("main.script");
    a.push_int(0);
    a.push_var("self");
    a.push_string("GoTo");
    a.emit(Instruction::CallByExp);
    a.emit(Instruction::PopEmpty);
    a.push_string("arrived");
    store_result(&mut a);
    let mut host = MockHost::with_script(a.build());
    let actor = host.add_object(1, "Actor", "girl");
    let mut engine = Engine::new();
    let id = engine
        .run_script(&mut host, "main.script", Some(actor))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Waiting);
    assert_eq!(engine.script(id).unwrap().wait_object(), Some(actor));
    assert_eq!(engine.counts().waiting, 1);
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::Null);
    host.objects.get_mut(&actor).unwrap().ready = true;
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::from("arrived"));
}

#[test]
fn waiting_for_a_vanished_object_ends_the_script() {
    let mut a = Assembler::new("main.script");
    a.push_int(0);
    a.push_var("self");
    a.push_string("GoTo");
    a.emit(Instruction::CallByExp);
    a.emit(Instruction::PopEmpty);
    a.push_string("arrived");
    store_result(&mut a);
    let mut host = MockHost::with_script(a.build());
    let actor = host.add_object(1, "Actor", "girl");
    let mut engine = Engine::new();
    let id = engine
        .run_script(&mut host, "main.script", Some(actor))
        .unwrap();
    engine.tick(&mut host);
    host.objects.get_mut(&actor).unwrap().alive = false;
    engine.tick(&mut host);
    assert!(engine.script(id).is_none());
    assert_eq!(result(&engine), Value::Null);
}

#[test]
fn an_exclusive_wait_ends_the_other_waiters() {
    let mut a = Assembler::new("main.script");
    a.push_int(0);
    a.push_var("self");
    a.push_string("GoTo");
    a.emit(Instruction::CallByExp);
    a.emit(Instruction::PopEmpty);
    a.push_string("arrived");
    store_result(&mut a);
    let mut host = MockHost::with_script(a.build());
    let actor = host.add_object(1, "Actor", "girl");
    let mut engine = Engine::new();
    let first = engine
        .run_script(&mut host, "main.script", Some(actor))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(engine.script(first).unwrap().state(), State::Waiting);
    let second = engine
        .run_script(&mut host, "main.script", Some(actor))
        .unwrap();
    engine.tick(&mut host);
    assert!(engine.script(first).is_none());
    assert_eq!(engine.script(second).unwrap().state(), State::Waiting);
    assert_eq!(host.calls, ["girl.GoTo()", "girl.GoTo()"]);
}

#[test]
fn event_handlers_run_as_threads_sharing_the_globals() {
    // var count = 0;  on "Tick" { count = count + 1; result = count; }
    let mut a = Assembler::new("main.script");
    a.def_var("count");
    a.push_int(0);
    a.pop_var("count");
    let end = a.label();
    a.jmp(end);
    a.event("Tick");
    a.emit(Instruction::Scope);
    a.push_var("count");
    a.push_int(1);
    a.emit(Instruction::Add);
    a.pop_var("count");
    a.push_var("count");
    a.def_glob_var("result");
    a.pop_var("result");
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let obj = host.add_object(1, "Entity", "clock");
    let mut engine = Engine::new();
    let id = engine
        .run_script(&mut host, "main.script", Some(obj))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Persistent);
    assert!(engine.can_handle_event(obj, "tick"));
    assert!(!engine.can_handle_event(obj, "Tock"));
    assert!(engine.apply_event(&mut host, obj, "TICK", false));
    assert!(!engine.apply_event(&mut host, obj, "Tock", false));
    assert_eq!(engine.counts().total, 2);
    let thread = engine.scripts()[1].id();
    assert!(engine.script(thread).unwrap().is_thread());
    assert_eq!(engine.script(thread).unwrap().parent(), Some(id));
    assert_eq!(engine.script(thread).unwrap().thread_event(), Some("TICK"));
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::Int(1));
    assert!(engine.script(thread).is_none());
    engine.apply_event(&mut host, obj, "Tick", false);
    engine.apply_event(&mut host, obj, "Tick", false);
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::Int(3));
    assert_eq!(
        engine.script(id).unwrap().global("count"),
        Some(Value::Int(3))
    );
    assert!(host.errors().is_empty());
}

#[test]
fn unbreakable_events_run_at_once_and_cannot_sleep() {
    let mut a = Assembler::new("main.script");
    let end = a.label();
    a.jmp(end);
    a.event("Shutdown");
    a.emit(Instruction::Scope);
    a.push_int(100);
    a.push_int(1);
    a.external_call("Sleep");
    a.emit(Instruction::PopEmpty);
    a.push_string("done");
    a.def_glob_var("result");
    a.pop_var("result");
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let obj = host.add_object(1, "Entity", "scene");
    let mut engine = Engine::new();
    engine
        .run_script(&mut host, "main.script", Some(obj))
        .unwrap();
    engine.tick(&mut host);
    assert!(engine.apply_event(&mut host, obj, "Shutdown", true));
    assert_eq!(result(&engine), Value::from("done"));
    assert_eq!(engine.counts().total, 1);
    assert!(host.errors()[0].contains("Script cannot be interrupted"));
}

#[test]
fn script_methods_run_as_threads_and_return_values() {
    // target.script: method Twice(x) { return x * 2; }  method Silent() {}
    let mut t = Assembler::new("target.script");
    let end = t.label();
    t.jmp(end);
    t.method("Twice");
    t.emit(Instruction::Scope);
    t.emit(Instruction::CorrectStack(1));
    t.def_var("x");
    t.pop_var("x");
    t.push_var("x");
    t.push_int(2);
    t.emit(Instruction::Mul);
    t.emit(Instruction::Ret);
    t.method("Silent");
    t.emit(Instruction::Scope);
    t.emit(Instruction::CorrectStack(0));
    t.emit(Instruction::RetEvent);
    t.bind(end);
    t.emit(Instruction::Ret);
    // main.script: result = obj.Twice(21); silent = obj.Silent(); missing = obj.Nope()
    let mut a = Assembler::new("main.script");
    a.push_int(21);
    a.push_int(1);
    a.push_var("obj");
    a.push_string("Twice");
    a.emit(Instruction::CallByExp);
    a.def_glob_var("result");
    a.pop_var("result");
    a.push_int(0);
    a.push_var("obj");
    a.push_string("Silent");
    a.emit(Instruction::CallByExp);
    a.def_glob_var("silent");
    a.pop_var("silent");
    a.push_int(0);
    a.push_var("obj");
    a.push_string("Nope");
    a.emit(Instruction::CallByExp);
    a.def_glob_var("missing");
    a.pop_var("missing");
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    host.add_file("target.script", t.build());
    let obj = host.add_object(1, "Entity", "target");
    let mut engine = Engine::new();
    engine.set_global("obj", Value::Native(obj));
    engine.set_global("silent", Value::Int(-1));
    engine.set_global("missing", Value::Int(-1));
    engine
        .run_script(&mut host, "target.script", Some(obj))
        .unwrap();
    let main = engine.run_script(&mut host, "main.script", None).unwrap();
    assert!(engine.can_handle_method(obj, "Twice"));
    assert!(!engine.can_handle_method(obj, "twice"));

    engine.tick(&mut host);
    assert_eq!(engine.script(main).unwrap().state(), State::WaitingScript);
    let thread = engine.script(main).unwrap().wait_script().unwrap();
    assert_eq!(
        engine.script(thread).unwrap().state(),
        State::ThreadFinished
    );
    assert!(engine.script(thread).unwrap().is_method_thread());
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::Int(42));
    assert!(engine.script(thread).is_none());
    // Silent ended with RET_EVENT: its thread was removed before the
    // caller looked, so the caller got null.
    assert_eq!(engine.script(main).unwrap().state(), State::WaitingScript);
    engine.tick(&mut host);
    assert_eq!(engine.global("silent"), Some(&Value::Null));
    assert_eq!(engine.global("missing"), Some(&Value::Null));
    assert!(host.errors()[0].contains("Call to undefined method 'Nope'"));
    assert_eq!(host.calls, ["target.Nope()"]);
}

#[test]
fn a_method_thread_calls_the_native_method_of_its_own_name() {
    // target.script: method Native() { return this.Native() + "!"; }
    let mut t = Assembler::new("target.script");
    let end = t.label();
    t.jmp(end);
    t.method("Native");
    t.emit(Instruction::Scope);
    t.emit(Instruction::CorrectStack(0));
    t.push_int(0);
    t.push_var("self");
    t.push_string("Native");
    t.emit(Instruction::CallByExp);
    t.push_string("!");
    t.emit(Instruction::Add);
    t.emit(Instruction::Ret);
    t.bind(end);
    t.emit(Instruction::Ret);
    let mut a = Assembler::new("main.script");
    a.push_int(0);
    a.push_var("obj");
    a.push_string("Native");
    a.emit(Instruction::CallByExp);
    store_result(&mut a);
    let mut host = MockHost::with_script(a.build());
    host.add_file("target.script", t.build());
    let obj = host.add_object(1, "Entity", "target");
    let mut engine = Engine::new();
    engine.set_global("obj", Value::Native(obj));
    engine
        .run_script(&mut host, "target.script", Some(obj))
        .unwrap();
    engine.run_script(&mut host, "main.script", None).unwrap();
    engine.tick(&mut host);
    engine.tick(&mut host);
    assert_eq!(result(&engine), Value::from("native!"));
    assert_eq!(host.calls, ["target.Native()"]);
    assert!(host.errors().is_empty(), "{:?}", host.log);
}

#[test]
fn natives_can_raise_events_during_a_call() {
    // self.ApplyEvent("Ping"); on "Ping" { result = "pong" }
    let mut a = Assembler::new("main.script");
    a.push_string("Ping");
    a.push_int(1);
    a.push_var("self");
    a.push_string("ApplyEvent");
    a.emit(Instruction::CallByExp);
    a.def_glob_var("handled");
    a.pop_var("handled");
    let end = a.label();
    a.jmp(end);
    a.event("Ping");
    a.emit(Instruction::Scope);
    a.push_string("pong");
    a.def_glob_var("result");
    a.pop_var("result");
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let obj = host.add_object(1, "Entity", "bell");
    let mut engine = Engine::new();
    engine
        .run_script(&mut host, "main.script", Some(obj))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(engine.global("handled"), Some(&Value::Bool(true)));
    assert_eq!(result(&engine), Value::from("pong"));
    assert_eq!(host.raised, [(obj, "Ping".to_string())]);
}

#[test]
fn time_slices_interrupt_a_busy_script() {
    // SetScriptTimeSlice(50); while (true) { result = result + 1 }
    // The mock clock advances by a millisecond at every look.
    let mut a = Assembler::new("main.script");
    a.def_glob_var("result");
    a.push_int(0);
    a.pop_var("result");
    a.push_int(50);
    a.push_int(1);
    a.external_call("SetScriptTimeSlice");
    a.emit(Instruction::PopEmpty);
    let top = a.label();
    a.bind(top);
    a.push_var("result");
    a.push_int(1);
    a.emit(Instruction::Add);
    a.pop_var("result");
    a.jmp(top);
    let (engine, _, id) = run(a.build());
    assert_eq!(engine.script(id).unwrap().state(), State::Running);
    let after_one_tick = result(&engine).get_int(&());
    assert!(after_one_tick > 0);
}

#[test]
fn errors_are_logged_and_scripts_go_on() {
    let mut a = Assembler::new("main.script");
    a.def_glob_var("result");
    a.push_int(1);
    a.push_int(0);
    a.emit(Instruction::Div);
    a.pop_var("result");
    let after = a.label();
    a.jmp_false(after); // nothing to pop
    a.bind(after);
    a.emit(Instruction::PushNull);
    a.emit(Instruction::And); // one operand missing
    a.emit(Instruction::PopEmpty);
    a.push_string("still here");
    a.pop_var("result");
    a.emit(Instruction::Ret);
    let (engine, host, _) = run(a.build());
    assert_eq!(result(&engine), Value::from("still here"));
    let errors = host.errors();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors[0].contains("Division by zero"));
    assert!(errors[1].contains("Script corruption detected"));
    assert!(errors[2].contains("Script corruption detected"));
}

#[test]
fn a_corrupt_program_stops_fatally() {
    let mut a = Assembler::new("main.script");
    a.push_var("x");
    a.emit(Instruction::Ret);
    let mut bytes = a.build();
    // Point the PUSH_VAR symbol index past the table.
    let program = luma_script::Program::parse(bytes.clone()).unwrap();
    let at = program.code_start() + 4;
    bytes[at..at + 4].copy_from_slice(&42u32.to_le_bytes());
    let (engine, host, id) = run(bytes);
    assert!(engine.script(id).is_none());
    assert!(
        host.log.iter().any(|l| l.starts_with("Fatal:")),
        "{:?}",
        host.log
    );
}

#[test]
fn missing_and_invalid_files_are_reported() {
    let mut host = MockHost::default();
    host.add_file("text.script", b"var x = 1;".to_vec());
    let mut engine = Engine::new();
    assert!(engine.run_script(&mut host, "nope.script", None).is_none());
    assert!(engine.run_script(&mut host, "text.script", None).is_none());
    assert_eq!(host.log.len(), 2);
    assert!(host.log[0].contains("Error opening script 'nope.script'"));
    assert!(host.log[1].contains("not a valid compiled script"));
}

#[test]
fn programs_are_cached_case_insensitively() {
    let mut a = Assembler::new("main.script");
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let mut engine = Engine::new();
    let first = engine.program(&mut host, "main.script").unwrap();
    host.files.clear();
    let second = engine.program(&mut host, "MAIN.SCRIPT").unwrap();
    assert!(std::rc::Rc::ptr_eq(&first, &second));
    engine.clear_cache();
    assert!(engine.program(&mut host, "main.script").is_none());
}

#[test]
fn pause_resume_finish_and_removal() {
    let mut a = Assembler::new("main.script");
    a.push_int(1000);
    a.push_int(1);
    a.external_call("Sleep");
    a.emit(Instruction::PopEmpty);
    let end = a.label();
    a.jmp(end);
    a.event("Any");
    a.emit(Instruction::Scope);
    a.push_int(1000);
    a.push_int(1);
    a.external_call("Sleep");
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let obj = host.add_object(1, "Entity", "obj");
    let mut engine = Engine::new();
    let id = engine
        .run_script(&mut host, "main.script", Some(obj))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Sleeping);
    engine.pause_all(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Paused);
    engine.pause_all(&mut host);
    assert!(host.log.iter().any(|l| l.contains("pause a paused script")));
    host.timer = 5000;
    engine.tick(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Paused);
    engine.resume_all();
    assert_eq!(engine.script(id).unwrap().state(), State::Sleeping);
    engine.tick(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Persistent);
    // A persistent script is not paused.
    engine.pause_all(&mut host);
    assert_eq!(engine.script(id).unwrap().state(), State::Persistent);
    // Finishing with threads takes the sleeping handler along.
    engine.apply_event(&mut host, obj, "Any", false);
    engine.tick(&mut host);
    assert_eq!(engine.counts().total, 2);
    assert_eq!(engine.scripts_of(obj), [id]);
    engine.finish(id, true);
    let removed = engine.remove_finished();
    assert_eq!(removed.len(), 2);
    assert_eq!(removed[0].id, id);
    assert_eq!(removed[0].owner, Some(obj));
    assert!(!removed[0].thread);
    assert!(removed[1].thread);
    assert_eq!(engine.counts().total, 0);
}

#[test]
fn clear_globals_keeps_constants_and_natives() {
    let mut a = Assembler::new("main.script");
    a.def_glob_var("plain");
    a.push_int(1);
    a.pop_var("plain");
    a.def_const_var("fixed");
    a.push_int(2);
    a.pop_var("fixed");
    a.def_glob_var("obj");
    a.push_var("self");
    a.pop_var("obj");
    // Redeclaring keeps the value.
    a.def_glob_var("plain");
    a.emit(Instruction::Ret);
    let mut host = MockHost::with_script(a.build());
    let obj = host.add_object(1, "Entity", "obj");
    let mut engine = Engine::new();
    engine
        .run_script(&mut host, "main.script", Some(obj))
        .unwrap();
    engine.tick(&mut host);
    assert_eq!(engine.global("plain"), Some(&Value::Int(1)));
    assert!(engine.globals()["fixed"].is_const);
    engine.clear_globals(false);
    assert_eq!(engine.global("plain"), Some(&Value::Null));
    assert_eq!(engine.global("fixed"), Some(&Value::Int(2)));
    assert_eq!(engine.global("obj"), Some(&Value::Native(obj)));
    engine.clear_globals(true);
    assert_eq!(engine.global("obj"), Some(&Value::Null));
}

#[test]
fn host_functions_see_the_engine() {
    let mut a = Assembler::new("main.script");
    a.push_int(0);
    a.external_call("ScriptCount");
    a.push_int(0);
    a.external_call("Owner");
    a.emit(Instruction::Add);
    store_result(&mut a);
    let mut host = MockHost::with_script(a.build());
    let obj = host.add_object(7, "Entity", "seven");
    let mut engine = Engine::new();
    engine
        .run_script(&mut host, "main.script", Some(obj))
        .unwrap();
    engine.tick(&mut host);
    // 1 script + the owner converted to a float (0.0)
    assert_eq!(result(&engine), Value::Float(1.0));
}

#[test]
fn argument_counts_are_corrected_in_host_calls() {
    let mut a = Assembler::new("main.script");
    a.push_int(3);
    a.push_int(2);
    a.push_int(1);
    a.push_int(3);
    a.external_call("Count");
    store_result(&mut a);
    let (engine, _, _) = run(a.build());
    assert_eq!(result(&engine), Value::Int(3));
}

/// Every compiled script of the local test game runs on an empty host:
/// natives are missing, so most calls are runtime errors, but nothing
/// may panic or be fatal, and every script must end or block.
#[test]
#[ignore = "needs the local test game in testdata/local"]
fn runs_every_local_script_without_panicking() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/local/white-chamber/unpacked");
    if !root.is_dir() {
        eprintln!("skipped: {} is absent", root.display());
        return;
    }
    let mut scripts = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("script"))
            {
                scripts.push(path);
            }
        }
    }
    assert!(!scripts.is_empty());
    let mut runtime_errors = 0;
    let mut persistent = 0;
    for path in &scripts {
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut host = MockHost::default();
        host.add_file(&name, std::fs::read(path).unwrap());
        let owner = host.add_object(1, "Entity", "owner");
        let mut engine = Engine::new();
        let id = engine.run_script(&mut host, &name, Some(owner)).unwrap();
        engine.script_mut(id).unwrap().set_time_slice(20);
        for tick in 0..50 {
            host.timer = tick * 1000;
            engine.tick(&mut host);
            if engine
                .script(id)
                .is_none_or(|s| s.state() != State::Running)
            {
                break;
            }
        }
        for event in ["LeftClick", "LookAt", "Take", "MouseEntry", "MouseLeave"] {
            engine.apply_event(&mut host, owner, event, false);
        }
        for tick in 50..60 {
            host.timer = tick * 1000;
            engine.tick(&mut host);
        }
        assert!(
            !host.log.iter().any(|l| l.starts_with("Fatal")),
            "{name}: {:?}",
            host.log
        );
        runtime_errors += host.errors().len();
        if engine
            .script(id)
            .is_some_and(|s| s.state() == State::Persistent)
        {
            persistent += 1;
        }
    }
    eprintln!(
        "{} scripts, {persistent} persistent, {runtime_errors} runtime errors on the empty host",
        scripts.len()
    );
}
