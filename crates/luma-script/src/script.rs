//! A running script: its program, instruction pointer, stacks and state.
//!
//! Written with reference to `ScScript.cpp` of WME Lite (MIT License,
//! Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::program::Program;
use crate::stack::{Globals, Slot, Stack, VarRef};
use crate::value::{NativeId, Value};

/// Identifies a script for the lifetime of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScriptId(pub u64);

/// The state of a script, as the engine's scheduler sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum State {
    /// Executing instructions.
    Running,
    /// Waiting for a native object to be ready.
    Waiting,
    /// Sleeping until a time.
    Sleeping,
    /// Done; removed at the end of the tick.
    Finished,
    /// Top-level code done, kept for its event handlers and methods.
    Persistent,
    /// Failed; removed at the end of the tick.
    Error,
    /// Paused by the game (a system window is open).
    Paused,
    /// Waiting for a method thread to return.
    WaitingScript,
    /// A thread that returned a value, kept until the waiting script
    /// takes it.
    ThreadFinished,
}

/// A script loaded in the engine, or a thread spawned from one.
#[derive(Debug)]
pub struct Script {
    pub(crate) id: ScriptId,
    pub(crate) program: Rc<Program>,
    pub(crate) source: String,
    pub(crate) ip: usize,
    pub(crate) line: u32,
    pub(crate) stack: Stack,
    pub(crate) this_stack: Vec<Option<VarRef>>,
    pub(crate) scopes: Vec<BTreeMap<String, Value>>,
    pub(crate) call_stack: Vec<usize>,
    pub(crate) globals: Globals,
    pub(crate) reg1: Slot,
    pub(crate) state: State,
    pub(crate) orig_state: State,
    pub(crate) wait_object: Option<NativeId>,
    pub(crate) wait_time: u64,
    pub(crate) wait_frozen: bool,
    pub(crate) wait_script: Option<ScriptId>,
    pub(crate) time_slice: u64,
    pub(crate) thread: bool,
    pub(crate) method_thread: bool,
    pub(crate) thread_event: Option<String>,
    pub(crate) owner: Option<NativeId>,
    pub(crate) freezable: bool,
    pub(crate) unbreakable: bool,
    pub(crate) parent: Option<ScriptId>,
}

impl Script {
    /// A fresh top-level script, ready to run from the code start.
    pub(crate) fn new(
        id: ScriptId,
        program: Rc<Program>,
        source: &str,
        owner: Option<NativeId>,
    ) -> Script {
        let ip = program.code_start();
        Script {
            id,
            program,
            source: source.to_string(),
            ip,
            line: 0,
            stack: Stack::new(),
            this_stack: Vec::new(),
            scopes: Vec::new(),
            call_stack: Vec::new(),
            globals: Rc::new(RefCell::new(BTreeMap::new())),
            reg1: Slot::Value(Value::Null),
            state: State::Running,
            orig_state: State::Running,
            wait_object: None,
            wait_time: 0,
            wait_frozen: false,
            wait_script: None,
            time_slice: 0,
            thread: false,
            method_thread: false,
            thread_event: None,
            owner,
            freezable: true,
            unbreakable: false,
            parent: None,
        }
    }

    /// A thread of `original` starting at `ip`, sharing its globals.
    pub(crate) fn thread(
        id: ScriptId,
        original: &Script,
        ip: usize,
        event: &str,
        method: bool,
    ) -> Script {
        let mut script = Script::new(
            id,
            Rc::clone(&original.program),
            &original.source,
            original.owner,
        );
        script.ip = ip;
        script.globals = Rc::clone(&original.globals);
        script.thread = true;
        script.method_thread = method;
        script.thread_event = Some(event.to_string());
        script.time_slice = original.time_slice;
        script.freezable = original.freezable;
        script.parent = Some(original.id);
        script
    }

    /// The script's identifier.
    pub fn id(&self) -> ScriptId {
        self.id
    }

    /// The program being executed.
    pub fn program(&self) -> &Rc<Program> {
        &self.program
    }

    /// The script's path in the game, as recorded by the compiler.
    pub fn filename(&self) -> &str {
        self.program.filename()
    }

    /// The path the program was loaded from, which a saved game names
    /// it by.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The current state.
    pub fn state(&self) -> State {
        self.state
    }

    /// The source line being executed, from the last `DBG_LINE`.
    pub fn line(&self) -> u32 {
        self.line
    }

    /// The object owning the script (`self`), if any.
    pub fn owner(&self) -> Option<NativeId> {
        self.owner
    }

    /// Whether this is a thread spawned for an event or a method.
    pub fn is_thread(&self) -> bool {
        self.thread
    }

    /// Whether this is a thread spawned for a method call.
    pub fn is_method_thread(&self) -> bool {
        self.method_thread
    }

    /// The event or method name a thread runs.
    pub fn thread_event(&self) -> Option<&str> {
        self.thread_event.as_deref()
    }

    /// The script a thread was spawned from.
    pub fn parent(&self) -> Option<ScriptId> {
        self.parent
    }

    /// Whether the script can be paused by the game.
    pub fn is_freezable(&self) -> bool {
        self.freezable
    }

    /// Sets whether the script can be paused by the game.
    pub fn set_freezable(&mut self, freezable: bool) {
        self.freezable = freezable;
    }

    /// The time slice in milliseconds, 0 for none (`SetScriptTimeSlice`).
    pub fn time_slice(&self) -> u64 {
        self.time_slice
    }

    /// Limits how long the script runs per tick, in milliseconds.
    pub fn set_time_slice(&mut self, milliseconds: u64) {
        self.time_slice = milliseconds;
    }

    /// Whether the script runs to completion in one go.
    pub fn is_unbreakable(&self) -> bool {
        self.unbreakable
    }

    /// The object the script waits for, when `Waiting`.
    pub fn wait_object(&self) -> Option<NativeId> {
        self.wait_object
    }

    /// The thread the script waits for, when `WaitingScript`.
    pub fn wait_script(&self) -> Option<ScriptId> {
        self.wait_script
    }

    /// Whether the program has a handler for this event.
    pub fn can_handle_event(&self, name: &str) -> bool {
        self.program.event_position(name).is_some()
    }

    /// Whether the program has this method.
    pub fn can_handle_method(&self, name: &str) -> bool {
        self.program.method_position(name).is_some()
    }

    /// The value on top of the script's stack, if it is a value.
    pub fn stack_top(&self) -> Option<&Value> {
        self.stack.top().and_then(Slot::value)
    }

    /// A global of this script (not an engine global).
    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.borrow().get(name).cloned()
    }

    pub(crate) fn run(&mut self) {
        self.state = State::Running;
    }

    /// Ends the script. The engine removes it at the end of the tick.
    pub(crate) fn finish(&mut self) {
        self.state = State::Finished;
    }

    pub(crate) fn pause(&mut self) -> bool {
        if self.state == State::Paused {
            return false;
        }
        if !self.freezable || self.state == State::Persistent {
            return true;
        }
        self.orig_state = self.state;
        self.state = State::Paused;
        true
    }

    pub(crate) fn resume(&mut self) {
        if self.state == State::Paused {
            self.state = self.orig_state;
        }
    }
}
