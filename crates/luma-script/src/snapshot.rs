//! Snapshots of the engine for saved games (feature `serde`): every
//! script with its stacks and state, the globals a script shares with
//! its threads, the engine's globals. A script in the middle of its
//! code keeps its bytecode in the snapshot, so that it resumes the code
//! it was running even if the game's file changed since; an idle one
//! (waiting for events) is named by the file it was loaded from and
//! read again when the snapshot is restored, as the original engine
//! does (`CScScript::Persist`, `AfterLoad`).
//!
//! Written with reference to `ScScript.cpp` (`Persist`, `AfterLoad`) and
//! `ScEngine.cpp` (`Persist`) of Wintermute Engine 1.x (MIT License,
//! Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`. The rules are
//! documented in `docs/saving.md`.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::engine::{Engine, Global};
use crate::host::Host;
use crate::program::Program;
use crate::script::{Script, ScriptId, State};
use crate::stack::{Globals, Slot, Stack, VarRef};
use crate::value::{NativeId, Value};

/// A reference to a variable, with the shared globals named by their
/// index in [`EngineSnapshot::tables`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VarRefSnapshot {
    /// A local of a script's scope.
    Scope {
        /// The script.
        script: ScriptId,
        /// The scope's depth.
        depth: usize,
        /// The variable's name.
        name: String,
    },
    /// A global of a script.
    ScriptGlobal {
        /// The globals table.
        table: usize,
        /// The variable's name.
        name: String,
    },
    /// An engine-wide global.
    EngineGlobal {
        /// The variable's name.
        name: String,
    },
    /// A slot of a script's value stack.
    Slot {
        /// The script.
        script: ScriptId,
        /// The slot's index from the bottom.
        index: usize,
    },
}

/// A stack slot: a value or a reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SlotSnapshot {
    /// A value.
    Value(Value),
    /// A reference to a variable.
    Ref(VarRefSnapshot),
}

/// One script as saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptSnapshot {
    /// The script's identifier.
    pub id: ScriptId,
    /// The path the program was loaded from.
    pub source: String,
    /// The script's bytecode, by index in [`EngineSnapshot::programs`],
    /// for a script that is neither idle nor finished; `None` reads the
    /// file again.
    pub program: Option<usize>,
    /// The instruction pointer.
    pub ip: usize,
    /// The source line.
    pub line: u32,
    /// Every slot of the value stack, live or not.
    pub slots: Vec<SlotSnapshot>,
    /// How many slots are live.
    pub stack_len: usize,
    /// The `this` stack.
    pub this_stack: Vec<Option<VarRefSnapshot>>,
    /// The scopes, innermost last.
    pub scopes: Vec<BTreeMap<String, Value>>,
    /// The return addresses of the calls in progress.
    pub call_stack: Vec<usize>,
    /// The globals table shared with the script's threads.
    pub globals: usize,
    /// The `reg1` register.
    pub reg1: SlotSnapshot,
    /// The state.
    pub state: State,
    /// The state before a pause.
    pub orig_state: State,
    /// The object waited for.
    pub wait_object: Option<NativeId>,
    /// The time slept until.
    pub wait_time: u64,
    /// Whether the sleep runs on the wall clock.
    pub wait_frozen: bool,
    /// The thread waited for.
    pub wait_script: Option<ScriptId>,
    /// The time slice.
    pub time_slice: u64,
    /// Whether this is a thread.
    pub thread: bool,
    /// Whether this is a method thread.
    pub method_thread: bool,
    /// The event or method of a thread.
    pub thread_event: Option<String>,
    /// The owner.
    pub owner: Option<NativeId>,
    /// Whether the game may pause the script.
    pub freezable: bool,
    /// Whether the script runs to completion in one go.
    pub unbreakable: bool,
    /// The script a thread was spawned from.
    pub parent: Option<ScriptId>,
}

/// The engine as saved.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EngineSnapshot {
    /// The scripts, in execution order.
    pub scripts: Vec<ScriptSnapshot>,
    /// The globals tables the scripts share with their threads.
    pub tables: Vec<BTreeMap<String, Value>>,
    /// The engine-wide globals.
    pub globals: BTreeMap<String, Global>,
    /// The next script identifier.
    pub next_id: u64,
    /// The bytecode of the scripts saved in the middle of their code,
    /// once per program (a script and its threads share one).
    pub programs: Vec<Vec<u8>>,
}

/// Numbers the programs by identity as they are met.
#[derive(Default)]
struct Programs {
    index: HashMap<*const Program, usize>,
    programs: Vec<Vec<u8>>,
}

impl Programs {
    fn index_of(&mut self, program: &Rc<Program>) -> usize {
        let key = Rc::as_ptr(program);
        if let Some(&index) = self.index.get(&key) {
            return index;
        }
        let index = self.programs.len();
        self.index.insert(key, index);
        self.programs.push(program.bytes().to_vec());
        index
    }
}

/// Numbers the globals tables by identity as they are met.
#[derive(Default)]
struct Tables {
    index: HashMap<*const RefCell<BTreeMap<String, Value>>, usize>,
    tables: Vec<BTreeMap<String, Value>>,
}

impl Tables {
    fn index_of(&mut self, globals: &Globals) -> usize {
        let key = Rc::as_ptr(globals);
        if let Some(&index) = self.index.get(&key) {
            return index;
        }
        let index = self.tables.len();
        self.index.insert(key, index);
        self.tables.push(globals.borrow().clone());
        index
    }

    fn var_ref(&mut self, var: &VarRef) -> VarRefSnapshot {
        match var {
            VarRef::Scope {
                script,
                depth,
                name,
            } => VarRefSnapshot::Scope {
                script: *script,
                depth: *depth,
                name: name.clone(),
            },
            VarRef::ScriptGlobal { globals, name } => VarRefSnapshot::ScriptGlobal {
                table: self.index_of(globals),
                name: name.clone(),
            },
            VarRef::EngineGlobal { name } => VarRefSnapshot::EngineGlobal { name: name.clone() },
            VarRef::Slot { script, index } => VarRefSnapshot::Slot {
                script: *script,
                index: *index,
            },
        }
    }

    fn slot(&mut self, slot: &Slot) -> SlotSnapshot {
        match slot {
            Slot::Value(value) => SlotSnapshot::Value(value.clone()),
            Slot::Ref(var) => SlotSnapshot::Ref(self.var_ref(var)),
        }
    }
}

/// Puts the shared tables back, by index.
struct Restored {
    tables: Vec<Globals>,
}

impl Restored {
    fn table(&self, index: usize) -> Option<Globals> {
        self.tables.get(index).map(Rc::clone)
    }

    fn var_ref(&self, var: VarRefSnapshot) -> Option<VarRef> {
        Some(match var {
            VarRefSnapshot::Scope {
                script,
                depth,
                name,
            } => VarRef::Scope {
                script,
                depth,
                name,
            },
            VarRefSnapshot::ScriptGlobal { table, name } => VarRef::ScriptGlobal {
                globals: self.table(table)?,
                name,
            },
            VarRefSnapshot::EngineGlobal { name } => VarRef::EngineGlobal { name },
            VarRefSnapshot::Slot { script, index } => VarRef::Slot { script, index },
        })
    }

    fn slot(&self, slot: SlotSnapshot) -> Option<Slot> {
        Some(match slot {
            SlotSnapshot::Value(value) => Slot::Value(value),
            SlotSnapshot::Ref(var) => Slot::Ref(self.var_ref(var)?),
        })
    }
}

impl Engine {
    /// Everything a saved game keeps of the engine: the scripts as they
    /// are now, the globals they share and the engine's own.
    pub fn snapshot(&self) -> EngineSnapshot {
        let mut tables = Tables::default();
        let mut programs = Programs::default();
        let scripts = self
            .scripts
            .iter()
            .map(|script| {
                let globals = tables.index_of(&script.globals);
                // "don't save idle/finished scripts": their code is
                // read from the file again.
                let idle = matches!(
                    script.state,
                    State::Persistent | State::Finished | State::ThreadFinished
                );
                ScriptSnapshot {
                    id: script.id,
                    source: script.source.clone(),
                    program: (!idle).then(|| programs.index_of(&script.program)),
                    ip: script.ip,
                    line: script.line,
                    slots: script
                        .stack
                        .slots()
                        .iter()
                        .map(|slot| tables.slot(slot))
                        .collect(),
                    stack_len: script.stack.len(),
                    this_stack: script
                        .this_stack
                        .iter()
                        .map(|var| var.as_ref().map(|v| tables.var_ref(v)))
                        .collect(),
                    scopes: script.scopes.clone(),
                    call_stack: script.call_stack.clone(),
                    globals,
                    reg1: tables.slot(&script.reg1),
                    state: script.state,
                    orig_state: script.orig_state,
                    wait_object: script.wait_object,
                    wait_time: script.wait_time,
                    wait_frozen: script.wait_frozen,
                    wait_script: script.wait_script,
                    time_slice: script.time_slice,
                    thread: script.thread,
                    method_thread: script.method_thread,
                    thread_event: script.thread_event.clone(),
                    owner: script.owner,
                    freezable: script.freezable,
                    unbreakable: script.unbreakable,
                    parent: script.parent,
                }
            })
            .collect();
        EngineSnapshot {
            scripts,
            tables: tables.tables,
            globals: self.globals.clone(),
            next_id: self.next_id,
            programs: programs.programs,
        }
    }

    /// Replaces every script and global with a snapshot's. A script
    /// saved with its bytecode runs it; the programs of the others are
    /// read again through the host. A script whose program cannot be
    /// read (or whose saved bytecode is not a program) is logged and
    /// dropped, as the original engine ends it (`CScScript::AfterLoad`). A snapshot that names a globals table
    /// it does not hold is refused and leaves the engine empty.
    pub fn restore(&mut self, snapshot: EngineSnapshot, host: &mut dyn Host) -> Result<(), String> {
        self.scripts.clear();
        self.globals.clear();
        self.cache.clear();
        self.current = None;
        let restored = Restored {
            tables: snapshot
                .tables
                .into_iter()
                .map(|table| Rc::new(RefCell::new(table)))
                .collect(),
        };
        // The saved programs, parsed once each; a script and its
        // threads share one again.
        let saved_programs: Vec<Option<Rc<Program>>> = snapshot
            .programs
            .into_iter()
            .map(|bytes| Program::parse(bytes).ok().map(Rc::new))
            .collect();
        let mut scripts = Vec::with_capacity(snapshot.scripts.len());
        for saved in snapshot.scripts {
            let program = match saved.program {
                Some(index) => saved_programs.get(index).cloned().flatten(),
                None => self.program(host, &saved.source),
            };
            let Some(program) = program else {
                host.log(&format!(
                    "Error reinitializing script '{}' after load. Script will be terminated.",
                    saved.source
                ));
                continue;
            };
            let missing = || {
                format!(
                    "script '{}' names a globals table the saved game lacks",
                    saved.source
                )
            };
            let globals = restored.table(saved.globals).ok_or_else(missing)?;
            let slots = saved
                .slots
                .into_iter()
                .map(|slot| restored.slot(slot))
                .collect::<Option<Vec<Slot>>>()
                .ok_or_else(missing)?;
            let this_stack = saved
                .this_stack
                .into_iter()
                .map(|var| match var {
                    Some(var) => restored.var_ref(var).map(Some),
                    None => Some(None),
                })
                .collect::<Option<Vec<Option<VarRef>>>>()
                .ok_or_else(missing)?;
            let reg1 = restored.slot(saved.reg1).ok_or_else(missing)?;
            scripts.push(Script {
                id: saved.id,
                program,
                source: saved.source,
                ip: saved.ip,
                line: saved.line,
                stack: Stack::from_parts(slots, saved.stack_len),
                this_stack,
                scopes: saved.scopes,
                call_stack: saved.call_stack,
                globals,
                reg1,
                state: saved.state,
                orig_state: saved.orig_state,
                wait_object: saved.wait_object,
                wait_time: saved.wait_time,
                wait_frozen: saved.wait_frozen,
                wait_script: saved.wait_script,
                time_slice: saved.time_slice,
                thread: saved.thread,
                method_thread: saved.method_thread,
                thread_event: saved.thread_event,
                owner: saved.owner,
                freezable: saved.freezable,
                unbreakable: saved.unbreakable,
                parent: saved.parent,
            });
        }
        self.scripts = scripts;
        self.globals = snapshot.globals;
        self.next_id = snapshot
            .next_id
            .max(self.scripts.iter().map(|s| s.id.0).max().unwrap_or(0));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::Assembler;
    use crate::host::{Context, Return};
    use crate::instruction::Instruction;
    use crate::value::Natives;

    /// A host serving one program under two names.
    struct Files(Vec<(String, Vec<u8>)>, Vec<String>);

    impl Natives for Files {
        fn class_name(&self, _id: NativeId) -> String {
            String::new()
        }

        fn get_property(&self, _id: NativeId, _name: &str) -> Option<Value> {
            None
        }

        fn set_property(&mut self, _id: NativeId, _name: &str, _value: &Value) -> bool {
            false
        }
    }

    impl Host for Files {
        fn read_script(&mut self, filename: &str) -> Option<Vec<u8>> {
            self.0
                .iter()
                .find(|(name, _)| name == filename)
                .map(|(_, bytes)| bytes.clone())
        }

        fn log(&mut self, message: &str) {
            self.1.push(message.to_string());
        }

        fn timer(&self) -> u64 {
            0
        }

        fn clock(&self) -> u64 {
            0
        }

        fn valid_object(&self, _id: NativeId) -> bool {
            true
        }

        fn is_ready(&self, _id: NativeId) -> bool {
            true
        }

        fn random(&mut self, from: i32, _to: i32) -> i32 {
            from
        }

        fn call_function(
            &mut self,
            _name: &str,
            _args: &[Value],
            _ctx: Context<'_>,
        ) -> Option<Return> {
            None
        }

        fn call_method(
            &mut self,
            _target: NativeId,
            _name: &str,
            _args: &[Value],
            _ctx: Context<'_>,
        ) -> Option<Return> {
            None
        }
    }

    /// `var n = 1; on "bump" { n = n + 1; Sleep(10); n = n + 10; }`,
    /// with a reference on the stack across the sleep.
    fn program() -> Vec<u8> {
        let mut a = Assembler::new("a.script");
        a.def_var("n");
        a.push_int(1);
        a.pop_var("n");
        a.emit(Instruction::Ret);
        a.event("bump");
        a.emit(Instruction::Scope);
        a.push_var_ref("n");
        a.push_var("n");
        a.push_int(1);
        a.emit(Instruction::Add);
        a.pop_var("n");
        a.push_int(10);
        a.push_int(1);
        a.external_call("Sleep");
        a.emit(Instruction::PopEmpty);
        a.push_var("n");
        a.push_int(10);
        a.emit(Instruction::Add);
        a.pop_var("n");
        a.emit(Instruction::PopEmpty);
        a.emit(Instruction::RetEvent);
        a.build()
    }

    /// `CScScript::Persist`: a script saved in the middle of its code
    /// carries its bytecode and resumes it although the file changed; an
    /// idle one takes the new file.
    #[test]
    fn a_running_script_keeps_its_code_when_the_file_changed() {
        let mut host = Files(vec![("a.script".to_string(), program())], Vec::new());
        let mut engine = Engine::new();
        let owner = NativeId(3);
        let id = engine
            .run_script(&mut host, "a.script", Some(owner))
            .unwrap();
        engine.tick(&mut host);
        engine.apply_event(&mut host, owner, "bump", false);
        engine.tick(&mut host);
        let thread = engine.scripts()[1].id();
        assert_eq!(engine.script(thread).unwrap().state(), State::Sleeping);
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.scripts[0].program, None, "idle: read again");
        assert_eq!(snapshot.scripts[1].program, Some(0), "sleeping: kept");
        assert_eq!(snapshot.programs.len(), 1);

        // The game is patched: the file is another program now, whose
        // handler adds 100 where the old one added 10.
        let mut a = Assembler::new("a.script");
        a.def_var("n");
        a.emit(Instruction::Ret);
        a.event("bump");
        a.emit(Instruction::Scope);
        a.push_var("n");
        a.push_int(100);
        a.emit(Instruction::Add);
        a.pop_var("n");
        a.emit(Instruction::RetEvent);
        let mut later = Later(Files(vec![("a.script".to_string(), a.build())], Vec::new()));
        let mut engine = Engine::new();
        engine.restore(snapshot.clone(), &mut later).unwrap();
        assert!(later.0.1.is_empty(), "{:?}", later.0.1);
        // The sleeping thread ends the old handler: 2 + 10.
        engine.tick(&mut later);
        assert_eq!(engine.script(id).unwrap().global("n"), Some(Value::Int(12)));
        // The idle script answers the next event with the new file.
        engine.apply_event(&mut later, owner, "bump", false);
        engine.tick(&mut later);
        assert_eq!(
            engine.script(id).unwrap().global("n"),
            Some(Value::Int(112))
        );

        // Bytecode that is not a program ends the script, logged.
        let mut broken = snapshot;
        broken.programs[0] = b"not a program".to_vec();
        let mut engine = Engine::new();
        engine.restore(broken, &mut later).unwrap();
        assert_eq!(engine.scripts().len(), 1);
        assert!(
            later
                .0
                .1
                .iter()
                .any(|l| l.contains("Error reinitializing script 'a.script'")),
            "{:?}",
            later.0.1
        );
    }

    /// The host a second later, for the sleeps to end.
    struct Later(Files);

    impl Natives for Later {
        fn class_name(&self, id: NativeId) -> String {
            self.0.class_name(id)
        }

        fn get_property(&self, id: NativeId, name: &str) -> Option<Value> {
            self.0.get_property(id, name)
        }

        fn set_property(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
            self.0.set_property(id, name, value)
        }
    }

    impl Host for Later {
        fn read_script(&mut self, filename: &str) -> Option<Vec<u8>> {
            self.0.read_script(filename)
        }

        fn log(&mut self, message: &str) {
            self.0.log(message);
        }

        fn timer(&self) -> u64 {
            1000
        }

        fn clock(&self) -> u64 {
            1000
        }

        fn valid_object(&self, id: NativeId) -> bool {
            self.0.valid_object(id)
        }

        fn is_ready(&self, id: NativeId) -> bool {
            self.0.is_ready(id)
        }

        fn random(&mut self, from: i32, to: i32) -> i32 {
            self.0.random(from, to)
        }

        fn call_method(
            &mut self,
            target: NativeId,
            name: &str,
            args: &[Value],
            ctx: Context<'_>,
        ) -> Option<Return> {
            self.0.call_method(target, name, args, ctx)
        }

        fn call_function(
            &mut self,
            name: &str,
            args: &[Value],
            ctx: Context<'_>,
        ) -> Option<Return> {
            self.0.call_function(name, args, ctx)
        }
    }

    #[test]
    fn a_snapshot_resumes_the_scripts_where_they_were() {
        let mut host = Files(vec![("a.script".to_string(), program())], Vec::new());
        let mut engine = Engine::new();
        engine.set_global("Game", Value::Int(7));
        let owner = NativeId(3);
        let id = engine
            .run_script(&mut host, "a.script", Some(owner))
            .unwrap();
        engine.tick(&mut host);
        assert_eq!(engine.script(id).unwrap().state(), State::Persistent);
        engine.apply_event(&mut host, owner, "bump", false);
        engine.tick(&mut host);
        let thread = engine.scripts()[1].id();
        assert_eq!(engine.script(thread).unwrap().state(), State::Sleeping);
        assert_eq!(engine.script(id).unwrap().global("n"), Some(Value::Int(2)));

        let snapshot = engine.snapshot();
        assert_eq!(snapshot.scripts.len(), 2);
        assert_eq!(
            snapshot.tables.len(),
            1,
            "the thread shares its parent's globals"
        );
        assert_eq!(snapshot.globals["Game"].value, Value::Int(7));
        let bytes = postcard::to_allocvec(&snapshot).unwrap();
        let back: EngineSnapshot = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, snapshot);

        // A fresh engine takes the snapshot and the thread goes on.
        let mut engine = Engine::new();
        engine.restore(back, &mut host).unwrap();
        assert_eq!(engine.scripts().len(), 2);
        assert_eq!(engine.global("Game"), Some(&Value::Int(7)));
        let thread = engine.scripts()[1].id();
        assert_eq!(engine.script(thread).unwrap().state(), State::Sleeping);
        // Time passes: the sleep ends, the thread finishes its handler.
        struct Later(Files);
        engine.tick(&mut host);
        assert_eq!(engine.script(thread).unwrap().state(), State::Sleeping);
        let mut later = Later(host);
        impl Natives for Later {
            fn class_name(&self, id: NativeId) -> String {
                self.0.class_name(id)
            }
            fn get_property(&self, id: NativeId, name: &str) -> Option<Value> {
                self.0.get_property(id, name)
            }
            fn set_property(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
                self.0.set_property(id, name, value)
            }
        }
        impl Host for Later {
            fn read_script(&mut self, filename: &str) -> Option<Vec<u8>> {
                self.0.read_script(filename)
            }
            fn log(&mut self, message: &str) {
                self.0.log(message)
            }
            fn timer(&self) -> u64 {
                100
            }
            fn clock(&self) -> u64 {
                100
            }
            fn valid_object(&self, _id: NativeId) -> bool {
                true
            }
            fn is_ready(&self, _id: NativeId) -> bool {
                true
            }
            fn random(&mut self, from: i32, _to: i32) -> i32 {
                from
            }
            fn call_function(
                &mut self,
                _name: &str,
                _args: &[Value],
                _ctx: Context<'_>,
            ) -> Option<Return> {
                None
            }
            fn call_method(
                &mut self,
                _target: NativeId,
                _name: &str,
                _args: &[Value],
                _ctx: Context<'_>,
            ) -> Option<Return> {
                None
            }
        }
        engine.tick(&mut later);
        assert_eq!(engine.scripts().len(), 1, "the thread finished");
        assert_eq!(engine.script(id).unwrap().global("n"), Some(Value::Int(12)));
        assert!(
            later.0.1.iter().all(|l| !l.contains("error")),
            "{:?}",
            later.0.1
        );

        // A program that cannot be read: its script is dropped, logged.
        let snapshot = engine.snapshot();
        let mut empty = Files(Vec::new(), Vec::new());
        let mut engine = Engine::new();
        engine.restore(snapshot, &mut empty).unwrap();
        assert!(engine.scripts().is_empty());
        assert!(
            empty
                .1
                .iter()
                .any(|l| l.contains("Error reinitializing script 'a.script'")),
            "{:?}",
            empty.1
        );
    }
}
