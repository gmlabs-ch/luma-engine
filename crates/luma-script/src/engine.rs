//! The script engine: loads programs, runs scripts cooperatively, spawns
//! event and method threads, and resolves waits once per frame.
//!
//! Written with reference to `ScEngine.cpp`, `ScScript.cpp`,
//! `BScriptHolder.cpp` and the built-in functions of `BGame.cpp` of WME
//! Lite (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`. The
//! rules are summarised in `docs/scripting.md`.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::host::{Context, Effect, Host, Return};
use crate::instruction::Instruction;
use crate::program::Program;
use crate::script::{Script, ScriptId, State};
use crate::stack::{Slot, VarRef};
use crate::value::{NativeId, Value};

/// How many nested references are followed when dereferencing.
const MAX_REF_DEPTH: usize = 8;

/// An engine-wide global variable (`global x`, `const x`).
#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    /// The value.
    pub value: Value,
    /// Whether it was declared with `const`: it survives
    /// [`Engine::clear_globals`].
    pub is_const: bool,
}

/// How many scripts are in each state (`GetNumScripts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Scripts in total.
    pub total: usize,
    /// Running scripts.
    pub running: usize,
    /// Scripts waiting for an object, a time or a thread.
    pub waiting: usize,
    /// Persistent scripts, kept for their handlers.
    pub persistent: usize,
}

/// A script removed by [`Engine::remove_finished`], for the owner's
/// bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// The script.
    pub id: ScriptId,
    /// Its owner.
    pub owner: Option<NativeId>,
    /// Whether it was a thread.
    pub thread: bool,
}

/// The script engine.
#[derive(Debug, Default)]
pub struct Engine {
    scripts: Vec<Script>,
    globals: BTreeMap<String, Global>,
    cache: HashMap<String, Rc<Program>>,
    next_id: u64,
    current: Option<ScriptId>,
}

impl Engine {
    /// An engine with no scripts.
    pub fn new() -> Engine {
        Engine::default()
    }

    // ----------------------------------------------------------------
    // Programs and scripts
    // ----------------------------------------------------------------

    /// The program of a script file, loaded through the host and cached
    /// (case-insensitively) for the threads and the later runs of it.
    pub fn program(&mut self, host: &mut dyn Host, filename: &str) -> Option<Rc<Program>> {
        let key = filename.to_ascii_lowercase();
        if let Some(program) = self.cache.get(&key) {
            return Some(Rc::clone(program));
        }
        let Some(bytes) = host.read_script(filename) else {
            host.log(&format!("Error opening script '{filename}'"));
            return None;
        };
        match Program::parse(bytes) {
            Ok(program) => {
                let program = Rc::new(program);
                self.cache.insert(key, Rc::clone(&program));
                Some(program)
            }
            Err(err) => {
                host.log(&format!(
                    "File '{filename}' is not a valid compiled script: {err}"
                ));
                None
            }
        }
    }

    /// Forgets the cached programs (`Game.ClearScriptCache`); running
    /// scripts keep theirs.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Loads a script file and starts it, owned by `owner` (`self` and
    /// `this` in the script). Returns `None`, after logging, when the file
    /// cannot be read or is not a compiled script.
    pub fn run_script(
        &mut self,
        host: &mut dyn Host,
        filename: &str,
        owner: Option<NativeId>,
    ) -> Option<ScriptId> {
        let program = self.program(host, filename)?;
        let id = self.alloc_id();
        let script = Script::new(id, program, owner);
        let me = owner.map_or(Value::Null, Value::Native);
        {
            let mut globals = script.globals.borrow_mut();
            globals.insert("self".to_string(), me.clone());
            globals.insert("this".to_string(), me);
        }
        self.scripts.push(script);
        Some(id)
    }

    fn alloc_id(&mut self) -> ScriptId {
        self.next_id += 1;
        ScriptId(self.next_id)
    }

    fn index_of(&self, id: ScriptId) -> Option<usize> {
        self.scripts.iter().position(|s| s.id == id)
    }

    /// Every script, in execution order.
    pub fn scripts(&self) -> &[Script] {
        &self.scripts
    }

    /// A script by identifier.
    pub fn script(&self, id: ScriptId) -> Option<&Script> {
        self.scripts.iter().find(|s| s.id == id)
    }

    /// A script by identifier, mutable.
    pub fn script_mut(&mut self, id: ScriptId) -> Option<&mut Script> {
        self.scripts.iter_mut().find(|s| s.id == id)
    }

    /// The non-thread scripts attached to an object, in the order they
    /// were added.
    pub fn scripts_of(&self, owner: NativeId) -> Vec<ScriptId> {
        self.scripts
            .iter()
            .filter(|s| !s.thread && s.owner == Some(owner))
            .map(|s| s.id)
            .collect()
    }

    /// The script being executed, during a tick.
    pub fn current(&self) -> Option<ScriptId> {
        self.current
    }

    /// How many scripts are in each state.
    pub fn counts(&self) -> Counts {
        let mut counts = Counts {
            total: self.scripts.len(),
            ..Counts::default()
        };
        for script in &self.scripts {
            match script.state {
                State::Running => counts.running += 1,
                State::Waiting | State::Sleeping | State::WaitingScript => counts.waiting += 1,
                State::Persistent => counts.persistent += 1,
                _ => {}
            }
        }
        counts
    }

    // ----------------------------------------------------------------
    // Globals
    // ----------------------------------------------------------------

    /// An engine-wide global.
    pub fn global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name).map(|g| &g.value)
    }

    /// Sets an engine-wide global, creating it if needed.
    pub fn set_global(&mut self, name: &str, value: impl Into<Value>) {
        let value = value.into();
        match self.globals.get_mut(name) {
            Some(global) => global.value = value,
            None => {
                self.globals.insert(
                    name.to_string(),
                    Global {
                        value,
                        is_const: false,
                    },
                );
            }
        }
    }

    /// Every engine-wide global.
    pub fn globals(&self) -> &BTreeMap<String, Global> {
        &self.globals
    }

    /// Resets the non-constant globals to `null` (`ClearGlobals`), keeping
    /// natives unless asked otherwise.
    pub fn clear_globals(&mut self, including_natives: bool) {
        for global in self.globals.values_mut() {
            if !global.is_const && (!global.value.is_native() || including_natives) {
                global.value = Value::Null;
            }
        }
    }

    // ----------------------------------------------------------------
    // Threads, events and methods
    // ----------------------------------------------------------------

    fn spawn(
        &mut self,
        original: usize,
        ip: usize,
        event: &str,
        method: bool,
        unbreakable: bool,
    ) -> ScriptId {
        let id = self.alloc_id();
        let mut thread = Script::thread(id, &self.scripts[original], ip, event, method);
        thread.unbreakable = unbreakable;
        self.scripts.push(thread);
        id
    }

    /// Starts a thread running the script's handler for `event`, if it
    /// has one (`InvokeEventHandler`).
    pub fn invoke_event_handler(
        &mut self,
        script: ScriptId,
        event: &str,
        unbreakable: bool,
    ) -> Option<ScriptId> {
        let index = self.index_of(script)?;
        let position = self.scripts[index].program.event_position(event)?;
        Some(self.spawn(index, position as usize, event, false, unbreakable))
    }

    /// Raises an event on an object (`ApplyEvent`): every non-thread
    /// script attached to it that handles the event gets a thread. An
    /// unbreakable event runs its handlers to completion right away.
    /// Returns whether any handler ran.
    pub fn apply_event(
        &mut self,
        host: &mut dyn Host,
        owner: NativeId,
        event: &str,
        unbreakable: bool,
    ) -> bool {
        let mut handlers = 0;
        for id in self.scripts_of(owner) {
            if self.invoke_event_handler(id, event, unbreakable).is_some() {
                handlers += 1;
            }
        }
        if handlers > 0 && unbreakable {
            self.tick_unbreakable(host);
        }
        handlers > 0
    }

    /// Whether a script attached to the object handles the event.
    pub fn can_handle_event(&self, owner: NativeId, event: &str) -> bool {
        self.scripts
            .iter()
            .any(|s| !s.thread && s.owner == Some(owner) && s.can_handle_event(event))
    }

    /// Whether a script attached to the object defines the method.
    pub fn can_handle_method(&self, owner: NativeId, method: &str) -> bool {
        self.scripts
            .iter()
            .any(|s| !s.thread && s.owner == Some(owner) && s.can_handle_method(method))
    }

    /// Starts a thread running the method defined by the object's most
    /// recently attached script that has it (`InvokeMethodThread`). The
    /// caller pushes the arguments and waits for the thread.
    pub fn invoke_method_thread(&mut self, owner: NativeId, method: &str) -> Option<ScriptId> {
        let index = self
            .scripts
            .iter()
            .rposition(|s| !s.thread && s.owner == Some(owner) && s.can_handle_method(method))?;
        let position = self.scripts[index].program.method_position(method)?;
        Some(self.spawn(index, position as usize, method, true, false))
    }

    /// Pushes the arguments of a call onto a method thread's stack, as
    /// the caller laid them out (`CopyParameters`).
    fn copy_parameters(&mut self, thread: ScriptId, args: &[Value]) {
        if let Some(index) = self.index_of(thread) {
            let stack = &mut self.scripts[index].stack;
            for arg in args.iter().rev() {
                stack.push_value(arg.clone());
            }
            stack.push_value(args.len() as i32);
        }
    }

    // ----------------------------------------------------------------
    // Lifecycle
    // ----------------------------------------------------------------

    /// Ends a script, and with `including_threads` the threads it spawned
    /// (`Finish`).
    pub fn finish(&mut self, id: ScriptId, including_threads: bool) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        let script = &mut self.scripts[index];
        if script.state != State::Finished && including_threads {
            script.finish();
            self.finish_threads(index);
        } else {
            script.finish();
        }
    }

    /// Ends the threads of the script's file on the same owner
    /// (`FinishThreads`).
    fn finish_threads(&mut self, index: usize) {
        let owner = self.scripts[index].owner;
        let filename = self.scripts[index].filename().to_string();
        for script in &mut self.scripts {
            if script.thread
                && script.state != State::Finished
                && script.owner == owner
                && script.filename().eq_ignore_ascii_case(&filename)
            {
                script.finish();
            }
        }
    }

    /// Ends every script waiting for the object (`ResetObject`): a
    /// top-level script takes its threads with it, and scripts waiting
    /// for those scripts end too.
    pub fn reset_object(&mut self, object: NativeId) {
        let waiting: Vec<ScriptId> = self
            .scripts
            .iter()
            .filter(|s| s.state == State::Waiting && s.wait_object == Some(object))
            .map(|s| s.id)
            .collect();
        for id in waiting {
            self.reset_script(id);
            let is_thread = self.script(id).is_some_and(|s| s.thread || s.method_thread);
            self.finish(id, !is_thread);
        }
    }

    /// Ends every script waiting for the script's return (`ResetScript`).
    pub fn reset_script(&mut self, id: ScriptId) {
        for script in &mut self.scripts {
            if script.state == State::WaitingScript && script.wait_script == Some(id) {
                script.finish();
            }
        }
    }

    /// Pauses every freezable script but the one being executed
    /// (`PauseAll`).
    pub fn pause_all(&mut self, host: &mut dyn Host) {
        let current = self.current;
        for script in &mut self.scripts {
            if Some(script.id) != current && !script.pause() {
                host.log(&format!(
                    "Attempting to pause a paused script ('{}', line {})",
                    script.filename(),
                    script.line
                ));
            }
        }
    }

    /// Resumes the paused scripts (`ResumeAll`).
    pub fn resume_all(&mut self) {
        for script in &mut self.scripts {
            script.resume();
        }
    }

    /// Drops the finished and failed scripts (`RemoveFinishedScripts`),
    /// returning them so that owners can forget them.
    pub fn remove_finished(&mut self) -> Vec<Removed> {
        let mut removed = Vec::new();
        self.scripts.retain(|s| {
            if matches!(s.state, State::Finished | State::Error) {
                removed.push(Removed {
                    id: s.id,
                    owner: s.owner,
                    thread: s.thread,
                });
                false
            } else {
                true
            }
        });
        removed
    }

    // ----------------------------------------------------------------
    // Scheduling
    // ----------------------------------------------------------------

    /// One frame (`Tick`): resolves the waits that are over, runs every
    /// runnable script until it blocks, then drops the finished ones.
    pub fn tick(&mut self, host: &mut dyn Host) -> Vec<Removed> {
        for i in 0..self.scripts.len() {
            match self.scripts[i].state {
                State::Waiting => {
                    let object = self.scripts[i].wait_object;
                    match object {
                        Some(object) if host.valid_object(object) => {
                            if host.is_ready(object) {
                                self.scripts[i].run();
                            }
                        }
                        _ => self.scripts[i].finish(),
                    }
                }
                State::Sleeping => {
                    let script = &mut self.scripts[i];
                    let now = if script.wait_frozen {
                        host.clock()
                    } else {
                        host.timer()
                    };
                    if script.wait_time <= now {
                        script.run();
                    }
                }
                State::WaitingScript => self.resolve_waiting_script(i),
                _ => {}
            }
        }

        let mut i = 0;
        while i < self.scripts.len() {
            if self.scripts[i].state != State::Paused {
                self.execute(i, host);
            }
            i += 1;
        }
        self.remove_finished()
    }

    /// Runs the unbreakable scripts to completion (`TickUnbreakable`).
    pub fn tick_unbreakable(&mut self, host: &mut dyn Host) -> Vec<Removed> {
        let mut i = 0;
        while i < self.scripts.len() {
            if self.scripts[i].unbreakable {
                let id = self.scripts[i].id;
                self.current = Some(id);
                while self.scripts[i].state == State::Running {
                    if !self.step(i, host) {
                        break;
                    }
                }
                self.scripts[i].finish();
                self.current = None;
            }
            i += 1;
        }
        self.remove_finished()
    }

    fn resolve_waiting_script(&mut self, i: usize) {
        let Some(target) = self.scripts[i].wait_script else {
            self.scripts[i].stack.push_value(Value::Null);
            self.scripts[i].run();
            return;
        };
        match self.index_of(target) {
            Some(t) if self.scripts[t].state != State::Error => {
                if self.scripts[t].state == State::ThreadFinished {
                    let value = match self.scripts[t].stack.pop() {
                        Some(slot) => self.deref(slot),
                        None => Value::Null,
                    };
                    self.scripts[i].stack.push_value(value);
                    self.scripts[i].run();
                    self.scripts[i].wait_script = None;
                    self.scripts[t].finish();
                }
            }
            _ => {
                self.scripts[i].stack.push_value(Value::Null);
                self.scripts[i].wait_script = None;
                self.scripts[i].run();
            }
        }
    }

    /// Runs one script until it blocks, or until its time slice is used.
    fn execute(&mut self, i: usize, host: &mut dyn Host) {
        let id = self.scripts[i].id;
        self.current = Some(id);
        let start = host.clock();
        while self.scripts[i].state == State::Running {
            // The slice is read every time: a script may set it and loop
            // in the same tick.
            let slice = self.scripts[i].time_slice;
            if slice > 0 && host.clock().saturating_sub(start) >= slice {
                break;
            }
            if !self.step(i, host) {
                break;
            }
        }
        self.current = None;
    }

    // ----------------------------------------------------------------
    // Variables
    // ----------------------------------------------------------------

    /// Where a variable lives, looked up as the engine does: the top scope
    /// only, then the script's globals, then the engine's.
    fn find_var(&self, i: usize, name: &str) -> Option<VarRef> {
        let script = &self.scripts[i];
        if let Some(scope) = script.scopes.last() {
            if scope.contains_key(name) {
                return Some(VarRef::Scope {
                    script: script.id,
                    depth: script.scopes.len() - 1,
                    name: name.to_string(),
                });
            }
        }
        if script.globals.borrow().contains_key(name) {
            return Some(VarRef::ScriptGlobal {
                globals: Rc::clone(&script.globals),
                name: name.to_string(),
            });
        }
        if self.globals.contains_key(name) {
            return Some(VarRef::EngineGlobal {
                name: name.to_string(),
            });
        }
        None
    }

    /// A variable, created as `null` in the current scope (or the script's
    /// globals) with a warning when it does not exist (`GetVar`).
    fn get_var(&mut self, i: usize, name: &str, host: &mut dyn Host) -> VarRef {
        if let Some(var) = self.find_var(i, name) {
            return var;
        }
        let script = &mut self.scripts[i];
        host.log(&format!(
            "Warning: variable '{name}' is inaccessible in the current block. Consider changing the script (script:{}, line:{})",
            script.filename(),
            script.line
        ));
        match script.scopes.last_mut() {
            Some(scope) => {
                scope.insert(name.to_string(), Value::Null);
                VarRef::Scope {
                    script: script.id,
                    depth: script.scopes.len() - 1,
                    name: name.to_string(),
                }
            }
            None => {
                script
                    .globals
                    .borrow_mut()
                    .insert(name.to_string(), Value::Null);
                VarRef::ScriptGlobal {
                    globals: Rc::clone(&script.globals),
                    name: name.to_string(),
                }
            }
        }
    }

    fn read_var_depth(&self, var: &VarRef, depth: usize) -> Value {
        if depth > MAX_REF_DEPTH {
            return Value::Null;
        }
        match var {
            VarRef::Scope {
                script,
                depth: scope,
                name,
            } => self
                .script(*script)
                .and_then(|s| s.scopes.get(*scope))
                .and_then(|scope| scope.get(name))
                .cloned()
                .unwrap_or(Value::Null),
            VarRef::ScriptGlobal { globals, name } => {
                globals.borrow().get(name).cloned().unwrap_or(Value::Null)
            }
            VarRef::EngineGlobal { name } => self.global(name).cloned().unwrap_or(Value::Null),
            VarRef::Slot { script, index } => {
                match self.script(*script).and_then(|s| s.stack.slot(*index)) {
                    Some(Slot::Value(value)) => value.clone(),
                    Some(Slot::Ref(inner)) => self.read_var_depth(inner, depth + 1),
                    None => Value::Null,
                }
            }
        }
    }

    /// The value of a variable, `null` if it is gone.
    fn read_var(&self, var: &VarRef) -> Value {
        self.read_var_depth(var, 0)
    }

    /// Changes a variable in place; nothing happens if it is gone.
    fn write_var(&mut self, var: &VarRef, f: impl FnOnce(&mut Value)) {
        let mut var = var.clone();
        for _ in 0..=MAX_REF_DEPTH {
            match &var {
                VarRef::Scope {
                    script,
                    depth,
                    name,
                } => {
                    if let Some(value) = self
                        .script_mut(*script)
                        .and_then(|s| s.scopes.get_mut(*depth))
                        .and_then(|scope| scope.get_mut(name))
                    {
                        f(value);
                    }
                    return;
                }
                VarRef::ScriptGlobal { globals, name } => {
                    if let Some(value) = globals.borrow_mut().get_mut(name) {
                        f(value);
                    }
                    return;
                }
                VarRef::EngineGlobal { name } => {
                    if let Some(global) = self.globals.get_mut(name) {
                        f(&mut global.value);
                    }
                    return;
                }
                VarRef::Slot { script, index } => {
                    let next = match self
                        .script_mut(*script)
                        .and_then(|s| s.stack.slot_mut(*index))
                    {
                        Some(Slot::Value(value)) => {
                            f(value);
                            return;
                        }
                        Some(Slot::Ref(inner)) => inner.clone(),
                        None => return,
                    };
                    var = next;
                }
            }
        }
    }

    /// The value a stack entry stands for.
    fn deref(&self, slot: Slot) -> Value {
        match slot {
            Slot::Value(value) => value,
            Slot::Ref(var) => self.read_var(&var),
        }
    }

    /// Pops a value, dereferenced; `None` on underflow.
    fn pop_value(&mut self, i: usize) -> Option<Value> {
        let slot = self.scripts[i].stack.pop()?;
        Some(self.deref(slot))
    }

    /// Pops the argument count then the arguments of a call, first
    /// argument first.
    fn pop_args(&mut self, i: usize, host: &dyn Host) -> Vec<Value> {
        let count = self
            .pop_value(i)
            .map(|v| v.get_int(host).max(0) as usize)
            .unwrap_or(0);
        let mut args = Vec::with_capacity(count);
        for _ in 0..count {
            match self.pop_value(i) {
                Some(value) => args.push(value),
                None => break,
            }
        }
        args
    }

    fn push(&mut self, i: usize, value: impl Into<Value>) {
        self.scripts[i].stack.push_value(value);
    }

    // ----------------------------------------------------------------
    // Errors, sleeps and waits
    // ----------------------------------------------------------------

    fn runtime_error(&self, i: usize, host: &mut dyn Host, message: &str) {
        let script = &self.scripts[i];
        host.log(&format!(
            "Runtime error. Script '{}', line {}: {message}",
            script.filename(),
            script.line
        ));
    }

    fn sleep(&mut self, i: usize, host: &mut dyn Host, duration: u64) {
        if self.scripts[i].unbreakable {
            self.runtime_error(i, host, "Script cannot be interrupted.");
            return;
        }
        let frozen = host.is_frozen();
        let now = if frozen { host.clock() } else { host.timer() };
        let script = &mut self.scripts[i];
        script.state = State::Sleeping;
        script.wait_time = now + duration;
        script.wait_frozen = frozen;
    }

    fn wait_for(&mut self, i: usize, host: &mut dyn Host, object: NativeId) {
        if self.scripts[i].unbreakable {
            self.runtime_error(i, host, "Script cannot be interrupted.");
            return;
        }
        let script = &mut self.scripts[i];
        script.state = State::Waiting;
        script.wait_object = Some(object);
    }

    fn apply_return(&mut self, i: usize, host: &mut dyn Host, ret: Return) {
        self.push(i, ret.value);
        match ret.effect {
            Effect::None => {}
            Effect::Sleep(ms) => self.sleep(i, host, u64::from(ms)),
            Effect::WaitFor(object) => self.wait_for(i, host, object),
            Effect::WaitForExclusive(object) => {
                self.reset_object(object);
                self.wait_for(i, host, object);
            }
        }
    }

    fn context(&mut self, i: usize) -> Context<'_> {
        let script = &self.scripts[i];
        let id = script.id;
        let owner = script.owner;
        let line = script.line;
        Context {
            engine: self,
            script: id,
            owner,
            line,
        }
    }

    // ----------------------------------------------------------------
    // Built-in functions
    // ----------------------------------------------------------------

    /// The functions every game has (`CBGame::ExternalCall`).
    fn builtin(
        &mut self,
        i: usize,
        host: &mut dyn Host,
        name: &str,
        args: &[Value],
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "LOG" => {
                host.log(&format!("sc: {}", arg(0).get_string(host)));
                Return::null()
            }
            "Sleep" => Return::effect(Effect::Sleep(arg(0).get_int(host).max(0) as u32)),
            "WaitFor" => match arg(0).as_native() {
                Some(object) if host.valid_object(object) => {
                    Return::effect(Effect::WaitForExclusive(object))
                }
                _ => Return::null(),
            },
            "Random" => {
                let from = arg(0).get_int(host);
                let to = arg(1).get_int(host);
                let (from, to) = if to < from { (to, from) } else { (from, to) };
                Return::value(host.random(from, to))
            }
            "SetScriptTimeSlice" => {
                self.scripts[i].time_slice = arg(0).get_int(host).max(0) as u64;
                Return::null()
            }
            "MakeRGBA" | "MakeRGB" | "RGB" => {
                let r = arg(0).get_int(host);
                let g = arg(1).get_int(host);
                let b = arg(2).get_int(host);
                let a = match arg(3) {
                    Value::Null => 255,
                    other => other.get_int(host),
                };
                Return::value(rgba(r, g, b, a))
            }
            "GetRValue" => Return::value((arg(0).get_int(host) >> 16) & 0xff),
            "GetGValue" => Return::value((arg(0).get_int(host) >> 8) & 0xff),
            "GetBValue" => Return::value(arg(0).get_int(host) & 0xff),
            "GetAValue" => Return::value((arg(0).get_int(host) >> 24) & 0xff),
            "ToString" => Return::value(arg(0).get_string(host)),
            "ToInt" => Return::value(arg(0).get_int(host)),
            "ToFloat" => Return::value(arg(0).get_float(host)),
            "ToBool" => Return::value(arg(0).get_bool(host)),
            "Debug" => Return::null(),
            _ => return None,
        })
    }

    // ----------------------------------------------------------------
    // Instructions
    // ----------------------------------------------------------------

    /// Executes one instruction (`ExecuteInstruction`). Returns `false`
    /// when the script cannot continue.
    fn step(&mut self, i: usize, host: &mut dyn Host) -> bool {
        let id = self.scripts[i].id;
        let ip = self.scripts[i].ip;
        let (instruction, next) = match self.scripts[i].program.decode_at(ip) {
            Ok(decoded) => decoded,
            Err(err) => {
                let script = &mut self.scripts[i];
                host.log(&format!(
                    "Fatal: {err} ('{}', line {}, IP:{ip:#x})",
                    script.filename(),
                    script.line
                ));
                script.state = State::Finished;
                return false;
            }
        };
        self.scripts[i].ip = next;

        match instruction {
            Instruction::DefVar(sym) => {
                let name = self.symbol(i, sym);
                let script = &mut self.scripts[i];
                match script.scopes.last_mut() {
                    Some(scope) => {
                        scope.insert(name, Value::Null);
                    }
                    None => {
                        script.globals.borrow_mut().insert(name, Value::Null);
                    }
                }
            }
            Instruction::DefGlobVar(sym) | Instruction::DefConstVar(sym) => {
                let name = self.symbol(i, sym);
                let is_const = matches!(instruction, Instruction::DefConstVar(_));
                self.globals.entry(name).or_insert_with(|| Global {
                    value: Value::Null,
                    is_const,
                });
            }
            Instruction::Ret => {
                let script = &mut self.scripts[i];
                if !script.scopes.is_empty() && !script.call_stack.is_empty() {
                    script.scopes.pop();
                    script.ip = script.call_stack.pop().unwrap_or(next);
                } else if script.thread {
                    script.state = State::ThreadFinished;
                } else if script.program.is_persistent() {
                    script.state = State::Persistent;
                } else {
                    script.state = State::Finished;
                }
            }
            Instruction::RetEvent => self.scripts[i].state = State::Finished,
            Instruction::Call(address) => {
                let script = &mut self.scripts[i];
                script.call_stack.push(next);
                script.ip = address as usize;
            }
            Instruction::CallByExp => return self.call_by_exp(i, host),
            Instruction::ExternalCall(sym) => {
                let name = self.symbol(i, sym);
                let args = self.pop_args(i, host);
                let is_external = self.scripts[i]
                    .program
                    .externals()
                    .iter()
                    .any(|e| e.name == name);
                if is_external {
                    host.log("External functions are not supported on this platform.");
                    self.push(i, Value::Null);
                } else {
                    let ret = host
                        .call_function(&name, &args, self.context(i))
                        .or_else(|| self.builtin(i, host, &name, &args));
                    let Some(i) = self.index_of(id) else {
                        return false;
                    };
                    match ret {
                        Some(ret) => self.apply_return(i, host, ret),
                        None => {
                            self.runtime_error(
                                i,
                                host,
                                &format!("Call to undefined function '{name}'. Ignored."),
                            );
                            self.push(i, Value::Null);
                        }
                    }
                }
            }
            Instruction::Scope => self.scripts[i].scopes.push(BTreeMap::new()),
            Instruction::CorrectStack(expected) => {
                let count = self
                    .pop_value(i)
                    .map(|v| v.get_int(host).max(0) as usize)
                    .unwrap_or(0);
                self.scripts[i]
                    .stack
                    .correct_params(expected as usize, count);
            }
            Instruction::CreateObject => self.push(i, Value::object()),
            Instruction::PopEmpty => {
                self.scripts[i].stack.pop();
            }
            Instruction::PushVar(sym) => {
                let name = self.symbol(i, sym);
                let var = self.get_var(i, &name, host);
                let value = self.read_var(&var);
                self.push(i, value);
            }
            Instruction::PushVarRef(sym) => {
                let name = self.symbol(i, sym);
                let var = self.get_var(i, &name, host);
                self.scripts[i].stack.push(Slot::Ref(var));
            }
            Instruction::PopVar(sym) => {
                let name = self.symbol(i, sym);
                let var = self.get_var(i, &name, host);
                match self.pop_value(i) {
                    Some(value) => self.write_var(&var, |v| *v = value),
                    None => {
                        self.runtime_error(i, host, "Script stack corruption detected.");
                        self.write_var(&var, |v| *v = Value::Null);
                    }
                }
            }
            Instruction::PushVarThis => {
                let slot = match self.scripts[i].this_stack.last() {
                    Some(Some(var)) => Slot::Ref(var.clone()),
                    _ => Slot::Value(Value::Null),
                };
                self.scripts[i].stack.push(slot);
            }
            Instruction::PushInt(v) => self.push(i, v),
            Instruction::PushBool(v) => self.push(i, v),
            Instruction::PushFloat(v) => self.push(i, v),
            Instruction::PushString(s) => self.push(i, s),
            Instruction::PushNull => self.push(i, Value::Null),
            Instruction::PushThisFromStack => {
                let script = &mut self.scripts[i];
                let var = script
                    .stack
                    .top_index()
                    .map(|index| VarRef::Slot { script: id, index });
                script.this_stack.push(var);
            }
            Instruction::PushThis(sym) => {
                let name = self.symbol(i, sym);
                let var = self.get_var(i, &name, host);
                self.scripts[i].this_stack.push(Some(var));
            }
            Instruction::PopThis => {
                self.scripts[i].this_stack.pop();
            }
            Instruction::PushByExp => {
                let name = self.pop_string(i, host);
                let object = self.pop_value(i).unwrap_or(Value::Null);
                let value = object.get_property(&name, host).unwrap_or(Value::Null);
                self.push(i, value);
            }
            Instruction::PopByExp => {
                let name = self.pop_string(i, host);
                let target = self.scripts[i].stack.pop();
                let value = self.pop_value(i);
                if value.is_none() {
                    self.runtime_error(i, host, "Script stack corruption detected.");
                }
                let value = value.unwrap_or(Value::Null);
                match target {
                    Some(Slot::Ref(var)) => self.write_var(&var, |object| {
                        object.set_property(&name, value, host);
                    }),
                    Some(Slot::Value(mut object)) => {
                        // A copy on the stack: the property only reaches a
                        // native, which the host stores.
                        object.set_property(&name, value, host);
                    }
                    None => {}
                }
            }
            Instruction::PushReg1 => {
                let slot = self.scripts[i].reg1.clone();
                self.scripts[i].stack.push(slot);
            }
            Instruction::PopReg1 => {
                let slot = self.scripts[i]
                    .stack
                    .pop()
                    .unwrap_or(Slot::Value(Value::Null));
                self.scripts[i].reg1 = slot;
            }
            Instruction::Jmp(address) => self.scripts[i].ip = address as usize,
            Instruction::JmpFalse(address) => match self.pop_value(i) {
                Some(value) => {
                    if !value.get_bool(host) {
                        self.scripts[i].ip = address as usize;
                    }
                }
                None => self.runtime_error(
                    i,
                    host,
                    "Script corruption detected. Did you use '=' instead of '==' for comparison?",
                ),
            },
            Instruction::Add
            | Instruction::Sub
            | Instruction::Mul
            | Instruction::Div
            | Instruction::Modulo
            | Instruction::CmpEq
            | Instruction::CmpNe
            | Instruction::CmpL
            | Instruction::CmpG
            | Instruction::CmpLe
            | Instruction::CmpGe
            | Instruction::CmpStrictEq
            | Instruction::CmpStrictNe => {
                let op2 = self.pop_value(i).unwrap_or(Value::Null);
                let op1 = self.pop_value(i).unwrap_or(Value::Null);
                let result = match instruction {
                    Instruction::Add => op1.add(&op2, host),
                    Instruction::Sub => op1.sub(&op2, host),
                    Instruction::Mul => op1.mul(&op2, host),
                    Instruction::Div | Instruction::Modulo => {
                        let result = if instruction == Instruction::Div {
                            op1.div(&op2, host)
                        } else {
                            op1.modulo(&op2, host)
                        };
                        result.unwrap_or_else(|_| {
                            self.runtime_error(i, host, "Division by zero.");
                            Value::Null
                        })
                    }
                    Instruction::CmpEq => Value::Bool(op1.compare(&op2, host).is_eq()),
                    Instruction::CmpNe => Value::Bool(op1.compare(&op2, host).is_ne()),
                    Instruction::CmpL => Value::Bool(op1.compare(&op2, host).is_lt()),
                    Instruction::CmpG => Value::Bool(op1.compare(&op2, host).is_gt()),
                    Instruction::CmpLe => Value::Bool(op1.compare(&op2, host).is_le()),
                    Instruction::CmpGe => Value::Bool(op1.compare(&op2, host).is_ge()),
                    Instruction::CmpStrictEq => Value::Bool(op1.compare_strict(&op2, host).is_eq()),
                    _ => Value::Bool(op1.compare_strict(&op2, host).is_ne()),
                };
                self.push(i, result);
            }
            Instruction::Not => {
                let op1 = self.pop_value(i).unwrap_or(Value::Null);
                self.push(i, op1.not(host));
            }
            Instruction::And | Instruction::Or => {
                let op2 = self.pop_value(i);
                let op1 = self.pop_value(i);
                let result = match (op1, op2) {
                    (Some(a), Some(b)) => {
                        if instruction == Instruction::And {
                            a.get_bool(host) && b.get_bool(host)
                        } else {
                            a.get_bool(host) || b.get_bool(host)
                        }
                    }
                    _ => {
                        self.runtime_error(
                            i,
                            host,
                            "Script corruption detected. Did you use '=' instead of '==' for comparison?",
                        );
                        false
                    }
                };
                self.push(i, result);
            }
            Instruction::DbgLine(line) => self.scripts[i].line = line,
        }
        true
    }

    /// `CALL_BY_EXP`: a method call on whatever the stack holds.
    fn call_by_exp(&mut self, i: usize, host: &mut dyn Host) -> bool {
        let id = self.scripts[i].id;
        let name = self.pop_string(i, host);
        let target = self.pop_value(i).unwrap_or(Value::Null);
        let args = self.pop_args(i, host);
        let native = target.as_native();

        let mut result = None;
        let mut tried_native = false;
        {
            let script = &self.scripts[i];
            // Inside the method's own thread, the object's native method of
            // the same name is what the script means.
            if script.thread
                && script.method_thread
                && script.thread_event.as_deref() == Some(name.as_str())
                && native.is_some()
                && script.owner == native
            {
                tried_native = true;
                result =
                    host.call_method(native.unwrap_or(NativeId(0)), &name, &args, self.context(i));
            }
        }
        let Some(i) = self.index_of(id) else {
            return false;
        };

        if result.is_none() {
            if let Some(object) = native {
                if self.can_handle_method(object, &name) {
                    if self.scripts[i].unbreakable {
                        self.runtime_error(
                            i,
                            host,
                            &format!("Cannot call method '{name}'. Ignored."),
                        );
                        self.push(i, Value::Null);
                    } else {
                        match self.invoke_method_thread(object, &name) {
                            Some(thread) => {
                                let script = &mut self.scripts[i];
                                script.state = State::WaitingScript;
                                script.wait_script = Some(thread);
                                self.copy_parameters(thread, &args);
                            }
                            None => {
                                self.runtime_error(
                                    i,
                                    host,
                                    &format!("Error invoking method '{name}'."),
                                );
                                self.push(i, Value::Null);
                            }
                        }
                    }
                    return true;
                }
                if !tried_native {
                    result = host.call_method(object, &name, &args, self.context(i));
                }
            }
        }
        let Some(i) = self.index_of(id) else {
            return false;
        };
        match result {
            Some(ret) => self.apply_return(i, host, ret),
            None => {
                self.runtime_error(
                    i,
                    host,
                    &format!("Call to undefined method '{name}'. Ignored."),
                );
                self.push(i, Value::Null);
            }
        }
        true
    }

    fn symbol(&self, i: usize, index: u32) -> String {
        self.scripts[i]
            .program
            .symbol(index)
            .unwrap_or("?")
            .to_string()
    }

    fn pop_string(&mut self, i: usize, host: &dyn Host) -> String {
        self.pop_value(i)
            .map(|v| v.get_string(host))
            .unwrap_or_default()
    }
}

/// The engine's colour encoding: `0xAARRGGBB`.
pub fn rgba(r: i32, g: i32, b: i32, a: i32) -> i32 {
    ((a & 0xff) << 24) | ((r & 0xff) << 16) | ((g & 0xff) << 8) | (b & 0xff)
}
