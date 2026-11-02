//! The library classes (`String`, `Array`, `Date`, `File`, `MemBuffer`),
//! driven by assembled scripts through a host that owns the objects, as
//! the adventure layer does.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use luma_script::asm::Assembler;
use luma_script::classes::{ClassHost, ClassObject, relative_path};
use luma_script::{Context, Engine, Host, Instruction, NativeId, Natives, Return, Value};

/// A fresh directory for the files a test writes.
fn temp_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-classes-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, AtomicOrdering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 2023-11-14 22:13:20, a Tuesday, as the host's local time.
const NOW: i64 = 1_700_000_000;

struct ClassesHost {
    files: HashMap<String, Vec<u8>>,
    dir: PathBuf,
    objects: HashMap<NativeId, (ClassObject, BTreeMap<String, Value>)>,
    next_id: u64,
    log: Vec<String>,
    now: i64,
}

impl ClassesHost {
    fn new(script: Vec<u8>) -> ClassesHost {
        let mut files = HashMap::new();
        files.insert("main.script".to_string(), script);
        ClassesHost {
            files,
            dir: temp_dir(),
            objects: HashMap::new(),
            next_id: 1,
            log: Vec::new(),
            now: NOW,
        }
    }

    fn errors(&self) -> Vec<&str> {
        self.log
            .iter()
            .filter(|l| l.starts_with("Runtime error") || l.starts_with("Fatal"))
            .map(String::as_str)
            .collect()
    }

    fn object(&self, id: NativeId) -> &ClassObject {
        &self.objects[&id].0
    }

    /// Runs a class method with the object taken out of the table, so
    /// that the host is free for the call.
    fn with_object<T>(
        &mut self,
        id: NativeId,
        f: impl FnOnce(&mut ClassObject, &mut ClassesHost) -> T,
    ) -> Option<T> {
        let (mut object, props) = self.objects.remove(&id)?;
        let result = f(&mut object, self);
        self.objects.insert(id, (object, props));
        Some(result)
    }
}

impl Natives for ClassesHost {
    fn class_name(&self, id: NativeId) -> String {
        self.object(id).type_name().to_string()
    }

    fn to_string(&self, id: NativeId) -> String {
        self.object(id).to_string(self)
    }

    fn compare(&self, a: NativeId, b: NativeId) -> Ordering {
        self.object(a).compare(self.object(b), a.cmp(&b))
    }

    fn get_property(&self, id: NativeId, name: &str) -> Option<Value> {
        let (object, props) = self.objects.get(&id)?;
        object
            .get_property(name)
            .or_else(|| props.get(name).cloned())
    }

    fn set_property(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let handled = self.with_object(id, |object, host| object.set_property(name, value, host));
        match handled {
            Some(true) => true,
            Some(false) => {
                if let Some((_, props)) = self.objects.get_mut(&id) {
                    props.insert(name.to_string(), value.clone());
                }
                true
            }
            None => false,
        }
    }

    fn assign_scalar(&mut self, id: NativeId, value: &Value) {
        if let Some((object, _)) = self.objects.get_mut(&id) {
            object.assign_scalar(value);
        }
    }
}

impl ClassHost for ClassesHost {
    fn read_file(&mut self, name: &str) -> Option<Vec<u8>> {
        if let Some(path) = self.writable_path(name) {
            if path.is_file() {
                return std::fs::read(path).ok();
            }
        }
        self.files.get(name).cloned()
    }

    fn writable_path(&mut self, name: &str) -> Option<PathBuf> {
        relative_path(name).map(|p| self.dir.join(p))
    }

    fn local_time(&self) -> i64 {
        self.now
    }

    fn create_object(&mut self, object: ClassObject) -> NativeId {
        let id = NativeId(self.next_id);
        self.next_id += 1;
        self.objects.insert(id, (object, BTreeMap::new()));
        id
    }

    fn log(&mut self, message: &str) {
        self.log.push(message.to_string());
    }
}

impl Host for ClassesHost {
    fn read_script(&mut self, filename: &str) -> Option<Vec<u8>> {
        self.files.get(filename).cloned()
    }

    fn log(&mut self, message: &str) {
        self.log.push(message.to_string());
    }

    fn timer(&self) -> u64 {
        0
    }

    fn clock(&self) -> u64 {
        0
    }

    fn valid_object(&self, id: NativeId) -> bool {
        self.objects.contains_key(&id)
    }

    fn is_ready(&self, _id: NativeId) -> bool {
        true
    }

    fn random(&mut self, from: i32, _to: i32) -> i32 {
        from
    }

    fn call_function(&mut self, name: &str, args: &[Value], _ctx: Context<'_>) -> Option<Return> {
        let object = ClassObject::construct(name, args, self)?;
        let id = self.create_object(object);
        Some(Return::constructed(Value::Native(id)))
    }

    fn call_method(
        &mut self,
        target: NativeId,
        name: &str,
        args: &[Value],
        ctx: Context<'_>,
    ) -> Option<Return> {
        self.with_object(target, |object, host| {
            object.call_method(name, args, host, &ctx)
        })
        .flatten()
    }
}

/// An argument of a call in the assembled scripts.
enum Arg {
    Int(i32),
    Float(f64),
    Bool(bool),
    Str(&'static str),
    Null,
    Var(&'static str),
    /// `new Class(args)` as an expression.
    New(&'static str, Vec<Arg>),
}

/// A script under construction; `result` is an engine global object
/// collecting what the test checks.
struct Script(Assembler);

impl Script {
    fn new() -> Script {
        let mut a = Assembler::new("main.script");
        a.def_glob_var("result");
        a.emit(Instruction::CreateObject);
        a.pop_var("result");
        Script(a)
    }

    fn arg(&mut self, arg: &Arg) {
        match arg {
            Arg::Int(i) => self.0.push_int(*i),
            Arg::Float(f) => self.0.push_float(*f),
            Arg::Bool(b) => self.0.push_bool(*b),
            Arg::Str(s) => self.0.push_string(s),
            Arg::Null => self.0.emit(Instruction::PushNull),
            Arg::Var(v) => self.0.push_var(v),
            Arg::New(class, args) => self.new_object(class, args),
        }
    }

    /// `new Class(args)`, as the compiler emits it: a placeholder object
    /// becomes `this`, the constructor fills it, the call's own result is
    /// dropped and the object stays on the stack.
    fn new_object(&mut self, class: &str, args: &[Arg]) {
        self.0.emit(Instruction::CreateObject);
        self.0.emit(Instruction::PushThisFromStack);
        for arg in args.iter().rev() {
            self.arg(arg);
        }
        self.0.push_int(args.len() as i32);
        self.0.external_call(class);
        self.0.emit(Instruction::PopThis);
        self.0.emit(Instruction::PopEmpty);
    }

    /// `var name = new Class(args);`
    fn var_new(&mut self, name: &str, class: &str, args: &[Arg]) {
        self.0.def_var(name);
        self.new_object(class, args);
        self.0.pop_var(name);
    }

    /// `name = <arg>;`
    fn assign(&mut self, name: &str, arg: Arg) {
        self.arg(&arg);
        self.0.pop_var(name);
    }

    /// `var.Method(args)`, result left on the stack.
    fn call(&mut self, var: &str, method: &str, args: &[Arg]) {
        for arg in args.iter().rev() {
            self.arg(arg);
        }
        self.0.push_int(args.len() as i32);
        self.0.push_var(var);
        self.0.push_string(method);
        self.0.emit(Instruction::CallByExp);
    }

    /// `result.key = var.Method(args);`
    fn call_store(&mut self, var: &str, method: &str, args: &[Arg], key: &str) {
        self.call(var, method, args);
        self.store(key);
    }

    /// `var.Method(args);`
    fn call_drop(&mut self, var: &str, method: &str, args: &[Arg]) {
        self.call(var, method, args);
        self.0.emit(Instruction::PopEmpty);
    }

    /// `var[index]` or `var.prop`, pushed.
    fn get(&mut self, var: &str, prop: Arg) {
        self.0.push_var_ref(var);
        self.arg(&prop);
        self.0.emit(Instruction::PushByExp);
    }

    /// `result.key = var.prop;`
    fn get_store(&mut self, var: &str, prop: Arg, key: &str) {
        self.get(var, prop);
        self.store(key);
    }

    /// `var.prop = value;`
    fn set(&mut self, var: &str, prop: Arg, value: Arg) {
        self.arg(&value);
        self.0.push_var_ref(var);
        self.arg(&prop);
        self.0.emit(Instruction::PopByExp);
    }

    /// `result.key = <top of stack>;`
    fn store(&mut self, key: &str) {
        self.0.push_var_ref("result");
        self.0.push_string(key);
        self.0.emit(Instruction::PopByExp);
    }

    /// `result.key = lhs <op> rhs;`
    fn binop(&mut self, lhs: Arg, op: Instruction, rhs: Arg, key: &str) {
        self.arg(&lhs);
        self.arg(&rhs);
        self.0.emit(op);
        self.store(key);
    }

    /// `result.key = "" + var;`
    fn text(&mut self, var: &'static str, key: &str) {
        self.binop(Arg::Str(""), Instruction::Add, Arg::Var(var), key);
    }

    /// Runs the script once and returns `result` with the host.
    fn run(mut self) -> (BTreeMap<String, Value>, ClassesHost) {
        self.run_with(|_| {})
    }

    fn run_with(
        &mut self,
        setup: impl FnOnce(&mut ClassesHost),
    ) -> (BTreeMap<String, Value>, ClassesHost) {
        self.0.emit(Instruction::Ret);
        let mut host = ClassesHost::new(self.0.clone().build());
        setup(&mut host);
        let mut engine = Engine::new();
        engine.run_script(&mut host, "main.script", None).unwrap();
        engine.tick(&mut host);
        let result = engine
            .global("result")
            .and_then(Value::properties)
            .cloned()
            .unwrap_or_default();
        (result, host)
    }
}

fn s(text: &str) -> Value {
    Value::from(text)
}

fn i(n: i32) -> Value {
    Value::Int(n)
}

#[test]
fn new_fills_the_placeholder_the_compiler_pushed() {
    let mut p = Script::new();
    // var arr = new Array(); arr.Push(new String("q")); result.q = "" + arr[0];
    p.var_new("arr", "Array", &[]);
    p.call_drop("arr", "Push", &[Arg::New("String", vec![Arg::Str("q")])]);
    p.get("arr", Arg::Int(0));
    p.0.def_var("first");
    p.0.pop_var("first");
    p.text("first", "q");
    p.get_store("first", Arg::Str("Type"), "firstType");
    // var o = new Object(); o.x = 2; result.ox = o.x;
    p.var_new("o", "Object", &[]);
    p.set("o", Arg::Str("x"), Arg::Int(2));
    p.get_store("o", Arg::Str("x"), "ox");
    // var u = new Unknown(); result.u = u;
    p.var_new("u", "Unknown", &[]);
    p.0.push_var("u");
    p.store("u");
    let (result, host) = p.run();
    assert_eq!(result["q"], s("q"));
    assert_eq!(result["firstType"], s("string"));
    assert_eq!(result["ox"], i(2));
    assert!(result["u"].is_object(), "{:?}", result["u"]);
    assert_eq!(
        host.errors(),
        [
            "Runtime error. Script 'main.script', line 0: Call to undefined function 'Unknown'. Ignored."
        ]
    );
}

#[test]
fn string_construction_conversions_and_assignment() {
    let mut p = Script::new();
    p.var_new("a", "String", &[Arg::Str("héllo")]);
    p.get_store("a", Arg::Str("Type"), "type");
    p.get_store("a", Arg::Str("Length"), "len");
    p.get_store("a", Arg::Str("Capacity"), "cap");
    p.text("a", "text");
    p.binop(Arg::Var("a"), Instruction::CmpEq, Arg::Str("héllo"), "eq");
    p.binop(Arg::Var("a"), Instruction::Add, Arg::Int(1), "plus");
    p.var_new("b", "String", &[Arg::Int(10)]);
    p.get_store("b", Arg::Str("Length"), "blen");
    p.get_store("b", Arg::Str("Capacity"), "bcap");
    p.text("b", "btext");
    p.var_new("c", "String", &[]);
    p.text("c", "c");
    p.var_new("d", "String", &[Arg::Float(3.5)]);
    p.text("d", "d");
    p.var_new("e", "String", &[Arg::Bool(true)]);
    p.text("e", "e");
    p.set("a", Arg::Str("Capacity"), Arg::Int(2));
    p.get_store("a", Arg::Str("Capacity"), "capLow");
    p.set("a", Arg::Str("Capacity"), Arg::Int(50));
    p.get_store("a", Arg::Str("Capacity"), "capHigh");
    p.set("a", Arg::Str("foo"), Arg::Int(1));
    p.get_store("a", Arg::Str("foo"), "foo");
    p.call_store("a", "Trim", &[], "trim");
    // Assigning a plain value replaces the object (POP_VAR copies it).
    p.assign("a", Arg::Str("new text"));
    p.text("a", "assigned");
    p.get_store("a", Arg::Str("Type"), "replacedType");
    p.get_store("a", Arg::Str("Length"), "replacedLength");
    // Two objects compare by text; a float replaces the object.
    p.var_new("x", "String", &[Arg::Str("apple")]);
    p.var_new("y", "String", &[Arg::Str("banana")]);
    p.var_new("z", "String", &[Arg::Str("apple")]);
    p.binop(Arg::Var("x"), Instruction::CmpL, Arg::Var("y"), "lt");
    p.binop(Arg::Var("x"), Instruction::CmpEq, Arg::Var("z"), "sameText");
    p.binop(
        Arg::Var("x"),
        Instruction::CmpStrictEq,
        Arg::Var("z"),
        "strictSame",
    );
    p.assign("z", Arg::Float(1.0));
    p.0.push_var("z");
    p.store("replaced");
    let (result, host) = p.run();
    assert_eq!(result["type"], s("string"));
    assert_eq!(result["len"], i(5));
    assert_eq!(result["cap"], i(6));
    assert_eq!(result["text"], s("héllo"));
    assert_eq!(result["eq"], Value::Bool(true));
    assert_eq!(result["plus"], Value::Float(1.0), "a String converts to 0");
    assert_eq!(result["blen"], i(0));
    assert_eq!(result["bcap"], i(10));
    assert_eq!(result["btext"], s(""));
    assert_eq!(result["c"], s("[null]"));
    assert_eq!(result["d"], s("3.500000"));
    assert_eq!(result["e"], s("yes"));
    assert_eq!(result["capLow"], i(6));
    assert_eq!(result["capHigh"], i(50));
    assert_eq!(result["foo"], Value::Null);
    assert_eq!(result["assigned"], s("new text"));
    assert_eq!(result["replacedType"], Value::Null, "a plain string now");
    assert_eq!(result["replacedLength"], i(8));
    assert_eq!(result["lt"], Value::Bool(true));
    assert_eq!(result["sameText"], Value::Bool(true));
    assert_eq!(result["strictSame"], Value::Bool(true));
    assert_eq!(result["replaced"], Value::Float(1.0));
    assert_eq!(result["trim"], Value::Null);
    assert!(
        host.log
            .contains(&"Warning: cannot lower string capacity".to_string()),
        "{:?}",
        host.log
    );
    assert_eq!(
        host.errors(),
        ["Runtime error. Script 'main.script', line 0: Call to undefined method 'Trim'. Ignored."]
    );
}

#[test]
fn string_substring_and_substr() {
    let mut p = Script::new();
    p.var_new("t", "String", &[Arg::Str("abcdef")]);
    let substring: [(i32, i32, &str); 7] = [
        (1, 3, "bcd"),
        (3, 1, "bcd"),
        (-2, 1, "ab"),
        (2, 100, "cdef"),
        (10, 20, ""),
        (0, 0, "a"),
        (4, 4, "e"),
    ];
    for (n, (start, end, _)) in substring.iter().enumerate() {
        p.call_store(
            "t",
            "Substring",
            &[Arg::Int(*start), Arg::Int(*end)],
            &format!("sub{n}"),
        );
    }
    let substr: [(i32, Arg, &str); 8] = [
        (1, Arg::Int(3), "bcd"),
        (1, Arg::Null, "bcdef"),
        (1, Arg::Int(0), ""),
        (1, Arg::Int(-5), ""),
        (4, Arg::Int(100), "ef"),
        (10, Arg::Int(2), ""),
        (-3, Arg::Int(2), "ab"),
        (0, Arg::Null, "abcdef"),
    ];
    for (n, (start, length, _)) in substr.iter().enumerate() {
        let args = match length {
            Arg::Null => vec![Arg::Int(*start)],
            Arg::Int(l) => vec![Arg::Int(*start), Arg::Int(*l)],
            _ => unreachable!(),
        };
        p.call_store("t", "Substr", &args, &format!("substr{n}"));
    }
    let (result, host) = p.run();
    for (n, (start, end, expected)) in substring.iter().enumerate() {
        assert_eq!(
            result[&format!("sub{n}")],
            s(expected),
            "Substring({start}, {end})"
        );
    }
    for (n, (start, _, expected)) in substr.iter().enumerate() {
        assert_eq!(
            result[&format!("substr{n}")],
            s(expected),
            "Substr({start}, ..)"
        );
    }
    assert!(host.errors().is_empty(), "{:?}", host.log);
}

#[test]
fn string_case_index_and_split() {
    let mut p = Script::new();
    p.var_new("t", "String", &[Arg::Str("Héllo ß")]);
    p.call_store("t", "ToUpperCase", &[], "upper");
    p.call_store("t", "ToLowerCase", &[], "lower");
    p.text("t", "unchanged");
    p.var_new("u", "String", &[Arg::Str("abcabc")]);
    p.call_store("u", "IndexOf", &[Arg::Str("c")], "i1");
    p.call_store("u", "IndexOf", &[Arg::Str("c"), Arg::Int(3)], "i2");
    p.call_store("u", "IndexOf", &[Arg::Str("z")], "i3");
    p.call_store("u", "IndexOf", &[Arg::Str("a"), Arg::Int(-5)], "i4");
    p.call_store("u", "IndexOf", &[Arg::Str("a"), Arg::Int(100)], "i5");
    p.call_store("u", "IndexOf", &[Arg::Str(""), Arg::Int(2)], "i6");
    p.call_store("u", "IndexOf", &[Arg::Str(""), Arg::Int(100)], "i7");
    p.call_store("u", "IndexOf", &[Arg::Str("bca")], "i8");
    p.var_new("v", "String", &[Arg::Str("a,b,,c")]);
    p.0.def_var("parts");
    p.call("v", "Split", &[]);
    p.0.pop_var("parts");
    p.get_store("parts", Arg::Str("Type"), "splitType");
    p.get_store("parts", Arg::Str("Length"), "splitLen");
    p.get_store("parts", Arg::Int(0), "part0");
    p.get_store("parts", Arg::Int(2), "part2");
    p.get_store("parts", Arg::Int(3), "part3");
    p.text("parts", "splitText");
    p.var_new("w", "String", &[Arg::Str("a;b c")]);
    p.0.def_var("parts2");
    p.call("w", "Split", &[Arg::Str("; ")]);
    p.0.pop_var("parts2");
    p.get_store("parts2", Arg::Str("Length"), "split2Len");
    p.get_store("parts2", Arg::Int(1), "split2b");
    p.var_new("e", "String", &[Arg::Str("")]);
    p.0.def_var("parts3");
    p.call("e", "Split", &[]);
    p.0.pop_var("parts3");
    p.get_store("parts3", Arg::Str("Length"), "split3Len");
    p.get_store("parts3", Arg::Int(0), "split3a");
    p.var_new("f", "String", &[Arg::Str("a,")]);
    p.0.def_var("parts4");
    p.call("f", "Split", &[]);
    p.0.pop_var("parts4");
    p.text("parts4", "split4Text");
    p.get_store("parts4", Arg::Str("Length"), "split4Len");
    let (result, host) = p.run();
    assert_eq!(result["upper"], s("HÉLLO ß"));
    assert_eq!(result["lower"], s("héllo ß"));
    assert_eq!(result["unchanged"], s("Héllo ß"));
    assert_eq!(result["i1"], i(2));
    assert_eq!(result["i2"], i(5));
    assert_eq!(result["i3"], i(-1));
    assert_eq!(result["i4"], i(0));
    assert_eq!(result["i5"], i(-1));
    assert_eq!(result["i6"], i(2));
    assert_eq!(result["i7"], i(6));
    assert_eq!(result["i8"], i(1));
    assert_eq!(result["splitType"], s("array"));
    assert_eq!(result["splitLen"], i(4));
    assert_eq!(result["part0"], s("a"));
    assert_eq!(result["part2"], s(""));
    assert_eq!(result["part3"], s("c"));
    assert_eq!(result["splitText"], s("a,b,,c"));
    assert_eq!(result["split2Len"], i(3));
    assert_eq!(result["split2b"], s("b"));
    assert_eq!(result["split3Len"], i(1));
    assert_eq!(result["split3a"], s(""));
    assert_eq!(result["split4Text"], s("a,"));
    assert_eq!(result["split4Len"], i(2));
    assert!(host.errors().is_empty(), "{:?}", host.log);
}

#[test]
fn arrays() {
    let mut p = Script::new();
    p.var_new("a", "Array", &[]);
    p.get_store("a", Arg::Str("Length"), "len0");
    p.get_store("a", Arg::Str("Type"), "type");
    p.call_store("a", "Push", &[Arg::Str("x")], "p1");
    p.call_store("a", "Push", &[Arg::Str("y"), Arg::Str("z")], "p2");
    p.get_store("a", Arg::Int(1), "a1");
    p.text("a", "str");
    p.call_store("a", "Pop", &[], "pop");
    p.get_store("a", Arg::Str("Length"), "lenAfterPop");
    p.var_new("b", "Array", &[Arg::Int(5)]);
    p.get_store("b", Arg::Str("Length"), "blen");
    p.text("b", "bstr");
    p.get_store("b", Arg::Int(0), "b0");
    p.call_store("b", "Pop", &[], "bpop");
    p.get_store("b", Arg::Str("Length"), "blen2");
    p.var_new("c", "Array", &[Arg::Str("p"), Arg::Int(2), Arg::Bool(true)]);
    p.get_store("c", Arg::Str("Length"), "clen");
    p.set("c", Arg::Int(7), Arg::Str("far"));
    p.get_store("c", Arg::Str("Length"), "clen2");
    p.get_store("c", Arg::Str("007"), "c007");
    p.get_store("c", Arg::Float(1.0), "cfloat");
    p.get_store("c", Arg::Str("-1"), "cneg");
    p.get_store("c", Arg::Str("foo"), "cfoo");
    p.set("c", Arg::Str("Length"), Arg::Int(2));
    p.get_store("c", Arg::Int(7), "c7gone");
    p.text("c", "cstr");
    p.set("c", Arg::Str("Length"), Arg::Int(4));
    p.get_store("c", Arg::Str("Length"), "clen4");
    p.set("c", Arg::Int(2), Arg::Null);
    p.text("c", "cnull");
    p.set("c", Arg::Str("Length"), Arg::Int(-3));
    p.get_store("c", Arg::Str("Length"), "clenNeg");
    p.var_new("d", "Array", &[Arg::Str("7")]);
    p.get_store("d", Arg::Str("Length"), "dlen");
    p.set("d", Arg::Str("foo"), Arg::Int(3));
    p.get_store("d", Arg::Str("foo"), "dfoo");
    // Assigning a plain value replaces the object (POP_VAR copies it).
    p.assign("d", Arg::Str("text"));
    p.get_store("d", Arg::Str("Type"), "dAssign");
    p.get_store("d", Arg::Str("Length"), "dAssignLen");
    p.var_new("e", "Array", &[]);
    p.call_store("e", "Pop", &[], "epop");
    p.var_new("n", "Array", &[Arg::Int(-3)]);
    p.get_store("n", Arg::Str("Length"), "neg");
    // Elements are copies: var o = new Object(); o.x = 1; f.Push(o); o.x = 2;
    p.var_new("o", "Object", &[]);
    p.set("o", Arg::Str("x"), Arg::Int(1));
    p.var_new("f", "Array", &[]);
    p.call_drop("f", "Push", &[Arg::Var("o")]);
    p.set("o", Arg::Str("x"), Arg::Int(2));
    p.get("f", Arg::Int(0));
    p.0.push_string("x");
    p.0.emit(Instruction::PushByExp);
    p.store("fx");
    // Two arrays are never equal; an array equals itself.
    p.binop(Arg::Var("a"), Instruction::CmpEq, Arg::Var("b"), "eqOther");
    p.binop(Arg::Var("a"), Instruction::CmpEq, Arg::Var("a"), "eqSelf");
    let (result, host) = p.run();
    assert_eq!(result["len0"], i(0));
    assert_eq!(result["type"], s("array"));
    assert_eq!(result["p1"], i(1));
    assert_eq!(result["p2"], i(3));
    assert_eq!(result["a1"], s("y"));
    assert_eq!(result["str"], s("x,y,z"));
    assert_eq!(result["pop"], s("z"));
    assert_eq!(result["lenAfterPop"], i(2));
    assert_eq!(result["blen"], i(5));
    assert_eq!(result["bstr"], s(",,,,"));
    assert_eq!(result["b0"], Value::Null);
    assert_eq!(result["bpop"], Value::Null);
    assert_eq!(result["blen2"], i(4));
    assert_eq!(result["clen"], i(3));
    assert_eq!(result["clen2"], i(8));
    assert_eq!(result["c007"], s("far"));
    assert_eq!(result["cfloat"], Value::Null);
    assert_eq!(result["cneg"], Value::Null);
    assert_eq!(result["cfoo"], Value::Null);
    assert_eq!(result["c7gone"], Value::Null);
    assert_eq!(result["cstr"], s("p,2"));
    assert_eq!(result["clen4"], i(4));
    assert_eq!(result["cnull"], s("p,2,[null],"));
    assert_eq!(result["clenNeg"], i(0));
    assert_eq!(result["dlen"], i(7));
    assert_eq!(result["dfoo"], Value::Null);
    assert_eq!(result["dAssign"], Value::Null);
    assert_eq!(result["dAssignLen"], i(4));
    assert_eq!(result["epop"], Value::Null);
    assert_eq!(result["neg"], i(0));
    assert_eq!(result["fx"], i(1));
    assert_eq!(result["eqOther"], Value::Bool(false));
    assert_eq!(result["eqSelf"], Value::Bool(true));
    assert!(host.errors().is_empty(), "{:?}", host.log);
}

#[test]
fn dates() {
    let mut p = Script::new();
    p.var_new(
        "d",
        "Date",
        &[
            Arg::Int(2003),
            Arg::Int(12),
            Arg::Int(31),
            Arg::Int(13),
            Arg::Int(30),
            Arg::Int(25),
        ],
    );
    for getter in [
        "GetYear",
        "GetMonth",
        "GetDate",
        "GetHours",
        "GetMinutes",
        "GetSeconds",
        "GetWeekday",
    ] {
        p.call_store("d", getter, &[], getter);
    }
    p.text("d", "str");
    p.get_store("d", Arg::Str("Type"), "type");
    p.var_new("n", "Date", &[]);
    for getter in [
        "GetYear",
        "GetMonth",
        "GetDate",
        "GetHours",
        "GetMinutes",
        "GetSeconds",
        "GetWeekday",
    ] {
        p.call_store("n", getter, &[], &format!("now{getter}"));
    }
    // Setters normalise like mktime.
    p.call_drop("d", "SetMonth", &[Arg::Int(13)]);
    p.call_store("d", "GetYear", &[], "y2");
    p.call_store("d", "GetMonth", &[], "m2");
    p.call_drop("d", "SetDate", &[Arg::Int(0)]);
    p.call_store("d", "GetYear", &[], "y3");
    p.call_store("d", "GetMonth", &[], "m3");
    p.call_store("d", "GetDate", &[], "d3");
    p.call_drop("d", "SetHours", &[Arg::Int(24)]);
    p.call_store("d", "GetDate", &[], "d4");
    p.call_store("d", "GetHours", &[], "h4");
    p.call_drop("d", "SetSeconds", &[Arg::Int(-1)]);
    p.call_store("d", "GetMinutes", &[], "mi5");
    p.call_store("d", "GetSeconds", &[], "s5");
    p.call_drop("d", "SetMinutes", &[Arg::Int(60)]);
    p.call_store("d", "GetHours", &[], "h6");
    p.call_store("d", "GetMinutes", &[], "mi6");
    p.call_drop("d", "SetYear", &[Arg::Int(2000)]);
    p.call_drop("d", "SetMonth", &[Arg::Int(2)]);
    p.call_drop("d", "SetDate", &[Arg::Int(29)]);
    p.call_store("d", "GetDate", &[], "leap");
    p.call_store("d", "GetWeekday", &[], "leapWd");
    p.call_store("d", "SetCurrentTime", &[], "setCurrent");
    p.call_store("d", "GetYear", &[], "cur");
    p.set("d", Arg::Str("foo"), Arg::Int(1));
    p.get_store("d", Arg::Str("foo"), "foo");
    p.var_new("e", "Date", &[Arg::Int(2020)]);
    p.call_store("e", "GetYear", &[], "ey");
    p.call_store("e", "GetMonth", &[], "em");
    p.call_store("e", "GetDate", &[], "ed");
    p.var_new("a", "Date", &[Arg::Int(2001), Arg::Int(1), Arg::Int(1)]);
    p.var_new("b", "Date", &[Arg::Int(2001), Arg::Int(1), Arg::Int(2)]);
    p.var_new("c", "Date", &[Arg::Int(2001), Arg::Int(1), Arg::Int(1)]);
    p.binop(Arg::Var("a"), Instruction::CmpL, Arg::Var("b"), "lt");
    p.binop(Arg::Var("b"), Instruction::CmpG, Arg::Var("a"), "gt");
    p.binop(Arg::Var("a"), Instruction::CmpEq, Arg::Var("c"), "eq");
    p.binop(Arg::Var("a"), Instruction::CmpEq, Arg::Var("b"), "ne");
    let (result, host) = p.run();
    assert_eq!(result["GetYear"], i(2003));
    assert_eq!(result["GetMonth"], i(12));
    assert_eq!(result["GetDate"], i(31));
    assert_eq!(result["GetHours"], i(13));
    assert_eq!(result["GetMinutes"], i(30));
    assert_eq!(result["GetSeconds"], i(25));
    assert_eq!(result["GetWeekday"], i(3), "a Wednesday");
    assert_eq!(result["str"], s("Wed Dec 31 13:30:25 2003\n"));
    assert_eq!(result["type"], s("date"));
    assert_eq!(result["nowGetYear"], i(2023));
    assert_eq!(result["nowGetMonth"], i(11));
    assert_eq!(result["nowGetDate"], i(14));
    assert_eq!(result["nowGetHours"], i(22));
    assert_eq!(result["nowGetMinutes"], i(13));
    assert_eq!(result["nowGetSeconds"], i(20));
    assert_eq!(result["nowGetWeekday"], i(2), "a Tuesday");
    assert_eq!((&result["y2"], &result["m2"]), (&i(2004), &i(1)));
    assert_eq!(
        (&result["y3"], &result["m3"], &result["d3"]),
        (&i(2003), &i(12), &i(31))
    );
    assert_eq!((&result["d4"], &result["h4"]), (&i(1), &i(0)));
    assert_eq!((&result["mi5"], &result["s5"]), (&i(29), &i(59)));
    assert_eq!((&result["h6"], &result["mi6"]), (&i(1), &i(0)));
    assert_eq!(result["leap"], i(29));
    assert_eq!(result["leapWd"], i(2), "29 February 2000 was a Tuesday");
    assert_eq!(result["setCurrent"], Value::Null);
    assert_eq!(result["cur"], i(2023));
    assert_eq!(result["foo"], Value::Null);
    assert_eq!(
        (&result["ey"], &result["em"], &result["ed"]),
        (&i(2019), &i(11), &i(30)),
        "month 0, day 0 of 2020"
    );
    assert_eq!(result["lt"], Value::Bool(true));
    assert_eq!(result["gt"], Value::Bool(true));
    assert_eq!(result["eq"], Value::Bool(true));
    assert_eq!(result["ne"], Value::Bool(false));
    assert!(host.errors().is_empty(), "{:?}", host.log);
}

#[test]
fn text_files_read_from_the_game() {
    let mut p = Script::new();
    p.var_new("f", "File", &[Arg::Str("notes.txt")]);
    p.text("f", "str");
    p.get_store("f", Arg::Str("Filename"), "fname");
    p.get_store("f", Arg::Str("Type"), "type");
    p.get_store("f", Arg::Str("AccessMode"), "mode0");
    p.call_store("f", "OpenAsText", &[Arg::Int(1)], "open");
    p.get_store("f", Arg::Str("Length"), "len");
    p.get_store("f", Arg::Str("TextMode"), "text");
    p.get_store("f", Arg::Str("AccessMode"), "mode");
    for n in 1..=5 {
        p.call_store("f", "ReadLine", &[], &format!("l{n}"));
    }
    p.get_store("f", Arg::Str("Position"), "pos");
    p.call_store("f", "SetPosition", &[Arg::Int(7)], "seek");
    p.call_store("f", "ReadText", &[Arg::Int(3)], "t");
    p.get_store("f", Arg::Str("Position"), "pos2");
    p.call_store("f", "SetPosition", &[Arg::Int(100)], "seekBad");
    p.call_store("f", "SetPosition", &[Arg::Int(-1)], "seekNeg");
    p.call_store("f", "ReadText", &[Arg::Int(0)], "rt0");
    p.call_drop("f", "SetPosition", &[Arg::Int(19)]);
    p.call_store("f", "ReadText", &[Arg::Int(2)], "rtEof");
    p.call_store("f", "ReadByte", &[], "rb");
    p.call_store("f", "WriteLine", &[Arg::Str("x")], "wl");
    p.call_store("f", "Close", &[], "close");
    p.get_store("f", Arg::Str("AccessMode"), "modeClosed");
    p.get_store("f", Arg::Str("TextMode"), "textClosed");
    p.get_store("f", Arg::Str("Position"), "posClosed");
    p.get_store("f", Arg::Str("Length"), "lenClosed");
    p.call_store("f", "SetPosition", &[Arg::Int(0)], "seekClosed");
    p.var_new("g", "File", &[Arg::Str("missing.txt")]);
    p.call_store("g", "OpenAsText", &[Arg::Int(1)], "openMissing");
    p.get_store("g", Arg::Str("AccessMode"), "gmode");
    p.call_store("g", "OpenAsText", &[Arg::Int(9)], "badMode");
    p.var_new("h", "File", &[]);
    p.text("h", "hstr");
    p.get_store("h", Arg::Str("Filename"), "hname");
    p.call_store("h", "OpenAsText", &[Arg::Int(1)], "hopen");
    p.call_store("h", "SetFilename", &[Arg::Str("notes.txt")], "setName");
    p.call_store("h", "OpenAsText", &[], "hopen2");
    p.get_store("h", Arg::Str("Filename"), "hname2");
    p.set("h", Arg::Str("custom"), Arg::Int(5));
    p.get_store("h", Arg::Str("custom"), "hcustom");
    p.call_store("h", "SetFilename", &[Arg::Str("other.txt")], "setName2");
    p.get_store("h", Arg::Str("AccessMode"), "hmodeAfterRename");
    let (result, host) = p.run_with(|host| {
        host.files
            .insert("notes.txt".to_string(), b"first\r\nsecond\n\nlast".to_vec());
    });
    assert_eq!(result["str"], s("notes.txt"));
    assert_eq!(result["fname"], s("notes.txt"));
    assert_eq!(result["type"], s("file"));
    assert_eq!(result["mode0"], i(0));
    assert_eq!(result["open"], Value::Bool(true));
    assert_eq!(result["len"], i(19));
    assert_eq!(result["text"], Value::Bool(true));
    assert_eq!(result["mode"], i(1));
    assert_eq!(result["l1"], s("first"));
    assert_eq!(result["l2"], s("second"));
    assert_eq!(result["l3"], s(""));
    assert_eq!(result["l4"], s("last"));
    assert_eq!(result["l5"], Value::Null);
    assert_eq!(result["pos"], i(19));
    assert_eq!(result["seek"], Value::Bool(true));
    assert_eq!(result["t"], s("sec"));
    assert_eq!(result["pos2"], i(10));
    assert_eq!(result["seekBad"], Value::Bool(false));
    assert_eq!(result["seekNeg"], Value::Bool(false));
    assert_eq!(result["rt0"], s(""));
    assert_eq!(result["rtEof"], Value::Null);
    assert_eq!(result["rb"], Value::Null);
    assert_eq!(result["wl"], Value::Bool(false));
    assert_eq!(result["close"], Value::Null);
    assert_eq!(result["modeClosed"], i(0));
    assert_eq!(result["textClosed"], Value::Bool(false));
    assert_eq!(result["posClosed"], i(0));
    assert_eq!(result["lenClosed"], i(0));
    assert_eq!(result["seekClosed"], Value::Bool(false));
    assert_eq!(result["openMissing"], Value::Bool(false));
    assert_eq!(result["gmode"], i(0));
    assert_eq!(result["badMode"], Value::Bool(false));
    assert_eq!(result["hstr"], s("[file object]"));
    assert_eq!(result["hname"], s(""));
    assert_eq!(result["hopen"], Value::Bool(false));
    assert_eq!(result["setName"], Value::Null);
    assert_eq!(result["hopen2"], Value::Bool(true), "a missing mode reads");
    assert_eq!(result["hname2"], s("notes.txt"));
    assert_eq!(result["hcustom"], i(5), "script properties are kept");
    assert_eq!(result["hmodeAfterRename"], i(0), "SetFilename closes");
    let prefix = "Runtime error. Script 'main.script', line 0: ";
    assert_eq!(
        host.errors(),
        [
            format!("{prefix}File.ReadByte: File must be open for reading in binary mode."),
            format!("{prefix}File.WriteLine: File must be open for writing in text mode."),
            format!("{prefix}File.SetPosition: File is not open"),
            format!("{prefix}File.OpenAsText: invalid access mode. Setting read mode."),
        ]
    );
}

#[test]
fn text_files_written_copied_and_deleted() {
    let mut p = Script::new();
    p.var_new("w", "File", &[Arg::Str("out\\log.txt")]);
    p.call_store("w", "OpenAsText", &[Arg::Int(2)], "w");
    p.get_store("w", Arg::Str("AccessMode"), "wmode");
    p.get_store("w", Arg::Str("TextMode"), "wtext");
    p.call_store("w", "WriteLine", &[Arg::Str("héllo")], "wl");
    p.call_store("w", "WriteText", &[Arg::Str("tail")], "wt");
    p.get_store("w", Arg::Str("Position"), "wpos");
    p.get_store("w", Arg::Str("Length"), "wlen");
    p.call_store("w", "ReadLine", &[], "rl");
    p.call_store("w", "WriteInt", &[Arg::Int(1)], "wi");
    p.call_drop("w", "Close", &[]);
    p.call_store("w", "OpenAsText", &[Arg::Int(1)], "ro");
    p.call_store("w", "ReadLine", &[], "r1");
    p.call_store("w", "ReadLine", &[], "r2");
    p.call_store("w", "ReadLine", &[], "r3");
    p.call_drop("w", "Close", &[]);
    p.call_store("w", "OpenAsText", &[Arg::Int(3)], "ao");
    p.get_store("w", Arg::Str("Position"), "apos");
    p.call_drop("w", "WriteLine", &[Arg::Str("more")]);
    p.get_store("w", Arg::Str("Length"), "alen");
    p.get_store("w", Arg::Str("Position"), "apos2");
    p.call_drop("w", "Close", &[]);
    p.call_drop("w", "OpenAsText", &[Arg::Int(1)]);
    p.call_store("w", "ReadLine", &[], "a1");
    p.call_store("w", "ReadLine", &[], "a2");
    p.call_store("w", "ReadLine", &[], "a3");
    p.call_store("w", "Copy", &[Arg::Str("out\\copy.txt")], "copy");
    p.get_store("w", Arg::Str("AccessMode"), "modeAfterCopy");
    p.call_store(
        "w",
        "Copy",
        &[Arg::Str("out\\copy.txt"), Arg::Bool(false)],
        "copyNo",
    );
    p.call_store(
        "w",
        "Copy",
        &[Arg::Str("out\\copy.txt"), Arg::Bool(true)],
        "copyYes",
    );
    p.var_new("c", "File", &[Arg::Str("out/copy.txt")]);
    p.call_drop("c", "OpenAsText", &[Arg::Int(1)]);
    p.call_store("c", "ReadLine", &[], "c1");
    p.call_drop("c", "Close", &[]);
    p.call_store("w", "Delete", &[], "del");
    p.call_store("w", "OpenAsText", &[Arg::Int(1)], "openDeleted");
    p.call_store("w", "Delete", &[], "del2");
    p.var_new("bad", "File", &[Arg::Str("..\\escape.txt")]);
    p.call_store("bad", "OpenAsText", &[Arg::Int(2)], "badOpen");
    p.var_new("abs", "File", &[Arg::Str("/tmp/luma-escape.txt")]);
    p.call_store("abs", "OpenAsText", &[Arg::Int(2)], "absOpen");
    // A write-mode open truncates an existing file.
    p.var_new("t", "File", &[Arg::Str("out\\copy.txt")]);
    p.call_drop("t", "OpenAsText", &[Arg::Int(2)]);
    p.get_store("t", Arg::Str("Length"), "truncated");
    p.call_drop("t", "Close", &[]);
    let (result, host) = p.run();
    assert_eq!(result["w"], Value::Bool(true));
    assert_eq!(result["wmode"], i(2));
    assert_eq!(result["wtext"], Value::Bool(true));
    assert_eq!(result["wl"], Value::Bool(true));
    assert_eq!(result["wt"], Value::Bool(true));
    assert_eq!(result["wpos"], i(10));
    assert_eq!(result["wlen"], i(10));
    assert_eq!(result["rl"], Value::Null);
    assert_eq!(result["wi"], Value::Bool(false));
    assert_eq!(result["ro"], Value::Bool(true));
    assert_eq!(result["r1"], s("héllo"));
    assert_eq!(result["r2"], s("tail"));
    assert_eq!(result["r3"], Value::Null);
    assert_eq!(result["ao"], Value::Bool(true));
    assert_eq!(result["apos"], i(10));
    assert_eq!(result["alen"], i(15));
    assert_eq!(result["apos2"], i(15));
    assert_eq!(result["a1"], s("héllo"));
    assert_eq!(result["a2"], s("tailmore"));
    assert_eq!(result["a3"], Value::Null);
    assert_eq!(result["copy"], Value::Bool(true));
    assert_eq!(result["modeAfterCopy"], i(0), "Copy closes the file");
    assert_eq!(result["copyNo"], Value::Bool(false));
    assert_eq!(result["copyYes"], Value::Bool(true));
    assert_eq!(result["c1"], s("héllo"));
    assert_eq!(result["del"], Value::Bool(true));
    assert_eq!(result["openDeleted"], Value::Bool(false));
    assert_eq!(result["del2"], Value::Bool(false));
    assert_eq!(result["badOpen"], Value::Bool(false));
    assert_eq!(result["absOpen"], Value::Bool(false));
    assert_eq!(result["truncated"], i(0));
    assert!(!host.dir.join("out/log.txt").exists());
    assert_eq!(
        std::fs::read(host.dir.join("out/copy.txt")).unwrap(),
        b"",
        "truncated by the last open"
    );
    assert!(!host.dir.join("../escape.txt").exists());
    let prefix = "Runtime error. Script 'main.script', line 0: ";
    assert_eq!(
        host.errors(),
        [
            format!("{prefix}File.ReadLine: File must be open in text mode."),
            format!("{prefix}File.WriteInt: File must be open for writing in binary mode."),
        ]
    );
    std::fs::remove_dir_all(&host.dir).ok();
}

#[test]
fn binary_files() {
    let mut p = Script::new();
    p.var_new("b", "File", &[Arg::Str("data.bin")]);
    p.call_store("b", "OpenAsBinary", &[Arg::Int(2)], "o");
    p.call_store("b", "WriteBool", &[Arg::Bool(true)], "wb");
    p.call_drop("b", "WriteByte", &[Arg::Int(300)]);
    p.call_drop("b", "WriteShort", &[Arg::Int(-2)]);
    p.call_drop("b", "WriteInt", &[Arg::Int(-123_456)]);
    p.call_drop("b", "WriteLong", &[Arg::Int(7)]);
    p.call_drop("b", "WriteFloat", &[Arg::Float(1.5)]);
    p.call_drop("b", "WriteDouble", &[Arg::Float(2.25)]);
    p.call_drop("b", "WriteString", &[Arg::Str("héllo")]);
    p.get_store("b", Arg::Str("Position"), "pos");
    p.get_store("b", Arg::Str("TextMode"), "textMode");
    p.call_store("b", "WriteText", &[Arg::Str("x")], "wtext");
    p.call_store("b", "ReadInt", &[], "readWhileWriting");
    p.call_drop("b", "Close", &[]);
    p.call_store("b", "OpenAsBinary", &[Arg::Int(1)], "ro");
    p.get_store("b", Arg::Str("Length"), "len");
    p.call_store("b", "ReadBool", &[], "rbool");
    p.call_store("b", "ReadByte", &[], "rbyte");
    p.call_store("b", "ReadShort", &[], "rshort");
    p.call_store("b", "ReadInt", &[], "rint");
    p.call_store("b", "ReadLong", &[], "rlong");
    p.call_store("b", "ReadFloat", &[], "rfloat");
    p.call_store("b", "ReadDouble", &[], "rdouble");
    p.call_store("b", "ReadString", &[], "rstring");
    p.call_store("b", "ReadByte", &[], "reof");
    p.call_store("b", "ReadLine", &[], "rline");
    p.call_store("b", "WriteByte", &[Arg::Int(1)], "wInRead");
    p.call_store("b", "SetPosition", &[Arg::Int(1)], "seek");
    p.call_store("b", "ReadByte", &[], "rbyte2");
    p.call_drop("b", "SetPosition", &[Arg::Int(31)]);
    p.call_store("b", "ReadInt", &[], "rshortRead");
    p.call_drop("b", "SetPosition", &[Arg::Int(20)]);
    p.call_store("b", "ReadString", &[], "rstringShort");
    let (result, host) = p.run();
    assert_eq!(result["o"], Value::Bool(true));
    assert_eq!(result["wb"], Value::Bool(true));
    assert_eq!(result["pos"], i(33));
    assert_eq!(result["textMode"], Value::Bool(false));
    assert_eq!(result["wtext"], Value::Bool(false));
    assert_eq!(result["readWhileWriting"], Value::Null);
    assert_eq!(result["ro"], Value::Bool(true));
    assert_eq!(result["len"], i(33));
    assert_eq!(result["rbool"], Value::Bool(true));
    assert_eq!(result["rbyte"], i(44), "300 truncated to a byte");
    assert_eq!(result["rshort"], i(65_534), "65536 + the signed value");
    assert_eq!(result["rint"], i(-123_456));
    assert_eq!(result["rlong"], i(7));
    assert_eq!(result["rfloat"], Value::Float(1.5));
    assert_eq!(result["rdouble"], Value::Float(2.25));
    assert_eq!(result["rstring"], s("héllo"));
    assert_eq!(result["reof"], Value::Null);
    assert_eq!(result["rline"], Value::Null);
    assert_eq!(result["wInRead"], Value::Bool(false));
    assert_eq!(result["seek"], Value::Bool(true));
    assert_eq!(result["rbyte2"], i(44));
    assert_eq!(result["rshortRead"], Value::Null, "not enough bytes left");
    assert_eq!(
        result["rstringShort"],
        Value::Null,
        "a length whose bytes are missing"
    );
    assert_eq!(std::fs::read(host.dir.join("data.bin")).unwrap().len(), 33);
    let prefix = "Runtime error. Script 'main.script', line 0: ";
    assert_eq!(
        host.errors(),
        [
            format!("{prefix}File.WriteText: File must be open for writing in text mode."),
            format!("{prefix}File.ReadInt: File must be open for reading in binary mode."),
            format!("{prefix}File.ReadLine: File must be open in text mode."),
            format!("{prefix}File.WriteByte: File must be open for writing in binary mode."),
        ]
    );
    std::fs::remove_dir_all(&host.dir).ok();
}

#[test]
fn memory_buffers() {
    let mut p = Script::new();
    p.var_new("m", "MemBuffer", &[Arg::Int(16)]);
    p.get_store("m", Arg::Str("Size"), "size");
    p.get_store("m", Arg::Str("Type"), "type");
    p.text("m", "str");
    p.call_store("m", "SetBool", &[Arg::Int(0), Arg::Bool(true)], "sb");
    p.call_store("m", "SetByte", &[Arg::Int(1), Arg::Int(300)], "sbyte");
    p.call_store("m", "SetShort", &[Arg::Int(2), Arg::Int(-2)], "sshort");
    p.call_store("m", "SetInt", &[Arg::Int(4), Arg::Int(-123_456)], "sint");
    p.call_store(
        "m",
        "SetDouble",
        &[Arg::Int(8), Arg::Float(2.25)],
        "sdouble",
    );
    p.call_store("m", "GetBool", &[Arg::Int(0)], "gb");
    p.call_store("m", "GetByte", &[Arg::Int(1)], "gbyte");
    p.call_store("m", "GetShort", &[Arg::Int(2)], "gshort");
    p.call_store("m", "GetInt", &[Arg::Int(4)], "gint");
    p.call_store("m", "GetLong", &[Arg::Int(4)], "glong");
    p.call_store("m", "GetDouble", &[Arg::Int(8)], "gdouble");
    p.call_store("m", "SetLong", &[Arg::Int(8), Arg::Int(9)], "slong");
    p.call_store("m", "GetInt", &[Arg::Int(8)], "gint2");
    p.call_store("m", "SetFloat", &[Arg::Int(12), Arg::Float(1.5)], "sfloat");
    p.call_store("m", "GetFloat", &[Arg::Int(12)], "gfloat");
    p.call_store("m", "GetInt", &[Arg::Int(13)], "oob");
    p.call_store("m", "GetByte", &[Arg::Int(-1)], "neg");
    p.call_store("m", "SetInt", &[Arg::Int(14), Arg::Int(1)], "setOob");
    p.call_store("m", "SetString", &[Arg::Int(0), Arg::Str("hi")], "ss");
    p.call_store("m", "GetString", &[Arg::Int(0), Arg::Int(0)], "gs");
    p.call_store("m", "GetString", &[Arg::Int(0)], "gsDefault");
    p.call_store("m", "GetString", &[Arg::Int(0), Arg::Int(2)], "gs2");
    p.call_store("m", "GetString", &[Arg::Int(0), Arg::Int(1)], "gs1");
    p.call_store("m", "GetString", &[Arg::Int(0), Arg::Int(5)], "gsTrunc");
    p.call_store("m", "SetString", &[Arg::Int(14), Arg::Str("abc")], "ssOob");
    p.call_store("m", "SetString", &[Arg::Int(13), Arg::Str("ab")], "ssFits");
    p.call_store("m", "GetString", &[Arg::Int(13), Arg::Int(0)], "gsEnd");
    p.call_store("m", "SetSize", &[Arg::Int(4)], "resize");
    p.get_store("m", Arg::Str("Size"), "size2");
    p.call_store("m", "SetSize", &[Arg::Int(6)], "resizeUp");
    p.call_store("m", "GetByte", &[Arg::Int(5)], "zero");
    p.call_store("m", "GetString", &[Arg::Int(0), Arg::Int(0)], "hi");
    p.var_new("z", "MemBuffer", &[]);
    p.get_store("z", Arg::Str("Size"), "zsize");
    p.call_store("z", "GetByte", &[Arg::Int(0)], "zget");
    p.call_store("m", "GetPointer", &[Arg::Int(0)], "ptr");
    p.call_store("m", "SetPointer", &[Arg::Int(0), Arg::Var("m")], "sptr");
    p.call_store("m", "DEBUG_Dump", &[], "dump");
    p.set("m", Arg::Str("tag"), Arg::Str("t"));
    p.get_store("m", Arg::Str("tag"), "tag");
    p.var_new("m2", "MemBuffer", &[Arg::Int(4)]);
    p.binop(Arg::Var("m"), Instruction::CmpEq, Arg::Var("m"), "eqSelf");
    p.binop(Arg::Var("m"), Instruction::CmpEq, Arg::Var("m2"), "eqOther");
    p.binop(Arg::Var("m"), Instruction::CmpL, Arg::Var("m2"), "lt");
    p.binop(Arg::Var("m"), Instruction::CmpG, Arg::Var("m2"), "gt");
    p.var_new("n", "MemBuffer", &[Arg::Int(2)]);
    p.call_drop("n", "SetByte", &[Arg::Int(0), Arg::Int(65)]);
    p.call_drop("n", "SetByte", &[Arg::Int(1), Arg::Int(66)]);
    p.call_store("n", "GetString", &[Arg::Int(0), Arg::Int(0)], "noNul");
    p.call_store("n", "GetString", &[Arg::Int(0), Arg::Int(2)], "noNul2");
    p.call_store("n", "GetString", &[Arg::Int(0), Arg::Int(-1)], "negLen");
    let (result, host) = p.run();
    assert_eq!(result["size"], i(16));
    assert_eq!(result["type"], s("membuffer"));
    assert_eq!(result["str"], s("[membuffer object]"));
    for key in [
        "sb", "sbyte", "sshort", "sint", "sdouble", "slong", "sfloat", "ss", "ssFits",
    ] {
        assert_eq!(result[key], Value::Bool(true), "{key}");
    }
    assert_eq!(result["gb"], Value::Bool(true));
    assert_eq!(result["gbyte"], i(44));
    assert_eq!(result["gshort"], i(65_534));
    assert_eq!(result["gint"], i(-123_456));
    assert_eq!(result["glong"], i(-123_456));
    assert_eq!(result["gdouble"], Value::Float(2.25));
    assert_eq!(result["gint2"], i(9));
    assert_eq!(result["gfloat"], Value::Float(1.5));
    assert_eq!(result["oob"], Value::Null);
    assert_eq!(result["neg"], Value::Null);
    assert_eq!(result["setOob"], Value::Bool(false));
    assert_eq!(result["gs"], s("hi"));
    assert_eq!(result["gsDefault"], s("hi"));
    assert_eq!(result["gs2"], s("hi"));
    assert_eq!(result["gs1"], s("h"));
    assert_eq!(result["gsTrunc"], s("hi"));
    assert_eq!(result["ssOob"], Value::Bool(false));
    assert_eq!(result["gsEnd"], s("ab"));
    assert_eq!(result["resize"], Value::Bool(true));
    assert_eq!(result["size2"], i(4));
    assert_eq!(result["resizeUp"], Value::Bool(true));
    assert_eq!(result["zero"], i(0));
    assert_eq!(result["hi"], s("hi"));
    assert_eq!(result["zsize"], i(0));
    assert_eq!(result["zget"], Value::Null);
    assert_eq!(result["ptr"], Value::Null);
    assert_eq!(result["sptr"], Value::Bool(false));
    assert_eq!(result["dump"], Value::Null);
    assert_eq!(result["tag"], s("t"));
    assert_eq!(result["eqSelf"], Value::Bool(true));
    assert_eq!(result["eqOther"], Value::Bool(false));
    assert_eq!(result["lt"], Value::Bool(false));
    assert_eq!(result["gt"], Value::Bool(true));
    assert_eq!(result["noNul"], Value::Null);
    assert_eq!(result["noNul2"], s("AB"));
    assert_eq!(result["negLen"], Value::Null);
    let prefix = "Runtime error. Script 'main.script', line 0: ";
    let oob = format!("{prefix}Set/Get method call is out of bounds");
    assert_eq!(
        host.errors(),
        [
            oob.as_str(),
            oob.as_str(),
            oob.as_str(),
            oob.as_str(),
            oob.as_str(),
            &format!("{prefix}MemBuffer.GetPointer: memory pointers are not available"),
            &format!("{prefix}MemBuffer.SetPointer: memory pointers are not available"),
            oob.as_str(),
            oob.as_str(),
        ]
    );
}

#[cfg(feature = "serde")]
#[test]
fn class_objects_survive_a_snapshot() {
    let mut p = Script::new();
    p.var_new("f", "File", &[Arg::Str("notes.txt")]);
    p.call_drop("f", "OpenAsText", &[Arg::Int(1)]);
    p.call_drop("f", "ReadLine", &[]);
    p.var_new("w", "File", &[Arg::Str("w.txt")]);
    p.call_drop("w", "OpenAsText", &[Arg::Int(2)]);
    p.var_new("a", "Array", &[Arg::Str("x"), Arg::Int(2)]);
    p.var_new("d", "Date", &[Arg::Int(2001), Arg::Int(2), Arg::Int(3)]);
    p.var_new("m", "MemBuffer", &[Arg::Int(2)]);
    p.var_new("s", "String", &[Arg::Str("text")]);
    let (_, host) = p.run_with(|host| {
        host.files
            .insert("notes.txt".to_string(), b"one\ntwo\n".to_vec());
    });
    let mut restored = Vec::new();
    for (object, _) in host.objects.values() {
        let bytes = postcard::to_allocvec(object).unwrap();
        let back: ClassObject = postcard::from_bytes(&bytes).unwrap();
        match &back {
            ClassObject::File(file) if file.filename() == Some("notes.txt") => {
                assert_eq!(file.mode(), luma_script::classes::Mode::Read);
                assert_eq!(file.position(), 4, "the position is kept");
                assert!(file.is_text());
            }
            ClassObject::File(file) => {
                assert_eq!(
                    file.mode(),
                    luma_script::classes::Mode::Closed,
                    "writing closes"
                );
                assert!(!file.is_text());
            }
            _ => assert_eq!(&back, object),
        }
        restored.push(back);
    }
    assert_eq!(restored.len(), 6);
    std::fs::remove_dir_all(&host.dir).ok();
}
