//! What the virtual machine needs from the program that embeds it.

use crate::engine::Engine;
use crate::script::ScriptId;
use crate::value::{NativeId, Natives, Value};

/// A side effect requested by a native call on the calling script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Effect {
    /// Nothing beyond the returned value.
    #[default]
    None,
    /// Put the script to sleep for this many milliseconds (`Sleep`).
    Sleep(u32),
    /// Make the script wait until the object is ready (`WaitFor`).
    WaitFor(NativeId),
    /// Finish every script already waiting for the object, then wait for
    /// it (`WaitForExclusive`, used by `GoTo`, `Talk` and the like).
    WaitForExclusive(NativeId),
    /// The call was a constructor (`new Class(...)`): the returned value
    /// replaces the object on top of the `this` stack, the placeholder
    /// the compiler created for the expression, and the call itself
    /// yields `null`, as the engine's constructors do.
    Construct,
}

/// The result of a native call: a value, and an optional effect.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Return {
    /// The value pushed for the caller.
    pub value: Value,
    /// What happens to the calling script.
    pub effect: Effect,
}

impl Return {
    /// Returns `null` with no effect.
    pub fn null() -> Return {
        Return::default()
    }

    /// Returns a value with no effect.
    pub fn value(value: impl Into<Value>) -> Return {
        Return {
            value: value.into(),
            effect: Effect::None,
        }
    }

    /// Returns `null` with an effect.
    pub fn effect(effect: Effect) -> Return {
        Return {
            value: Value::Null,
            effect,
        }
    }

    /// The result of a constructor: the new object, which takes the place
    /// of the `this` object of the `new` expression.
    pub fn constructed(value: impl Into<Value>) -> Return {
        Return {
            value: value.into(),
            effect: Effect::Construct,
        }
    }
}

/// The calling script, handed to the host with every native call. The
/// engine is available for queries and for spawning event threads
/// (`ApplyEvent`, `AttachScript`); the host must not remove scripts
/// from it during the call.
pub struct Context<'a> {
    /// The engine running the call.
    pub engine: &'a mut Engine,
    /// The calling script.
    pub script: ScriptId,
    /// The object owning the calling script, if any (`self`).
    pub owner: Option<NativeId>,
    /// The source line being executed, for messages.
    pub line: u32,
}

impl Context<'_> {
    /// A runtime error in the calling script, worded as the engine logs
    /// its own (`Runtime error. Script '...', line N: message`), for the
    /// host to report ([`Host::script_error`]).
    pub fn runtime_error(&self, message: &str) -> String {
        let filename = self
            .engine
            .script(self.script)
            .map(|s| s.source().to_string())
            .unwrap_or_default();
        format!(
            "Runtime error. Script '{filename}', line {}: {message}",
            self.line
        )
    }
}

/// The program embedding the virtual machine: files, time, logging,
/// native objects and their methods, built-in functions.
pub trait Host: Natives {
    /// The bytes of a compiled script, by its path in the game.
    fn read_script(&mut self, filename: &str) -> Option<Vec<u8>>;

    /// A message for the game's log.
    fn log(&mut self, message: &str);

    /// A script runtime error, already worded (`Runtime error. Script
    /// '...', line N: message`). The original logs it and shows the
    /// player a quick message, `Script runtime error. View log for
    /// details.`, unless the game set `Game.SuppressScriptErrors`
    /// (`CScScript::RuntimeError`). The default only logs it.
    fn script_error(&mut self, message: &str) {
        self.log(message);
    }

    /// The game timer, in milliseconds. It stops while the game is frozen.
    fn timer(&self) -> u64;

    /// The wall clock, in milliseconds, used for sleeps started while the
    /// game is frozen.
    fn clock(&self) -> u64;

    /// A millisecond clock that goes on while scripts run (the original's
    /// `timeGetTime`): what a time slice (`SetScriptTimeSlice`) and the
    /// script profiler measure with. The default is [`Host::clock`],
    /// which a host may only advance between two ticks; such a host never
    /// interrupts a time-sliced script.
    fn ticks(&self) -> u64 {
        self.clock()
    }

    /// Compiles a script file that is still source text
    /// (`CScEngine::GetCompiledScript`): the engine calls it for a file
    /// that does not start with the magic of a compiled script, and runs
    /// what it returns. `None` means the file cannot be run; the host has
    /// logged why and told the player (`Script compiler error. View log
    /// for details.`). The default has no compiler, as the original
    /// without its compiler library.
    fn compile_script(&mut self, filename: &str, _source: &[u8]) -> Option<Vec<u8>> {
        self.log(&format!(
            "CScEngine::GetCompiledScript - script '{filename}' needs to be compiled but compiler is not available"
        ));
        None
    }

    /// Whether the game is frozen (a system window is open).
    fn is_frozen(&self) -> bool {
        false
    }

    /// Whether the native object still exists.
    fn valid_object(&self, id: NativeId) -> bool;

    /// Whether the native object has finished what a script waits for
    /// (walking, talking, playing an animation).
    fn is_ready(&self, id: NativeId) -> bool;

    /// A random integer in `from..=to` (`Random`).
    fn random(&mut self, from: i32, to: i32) -> i32;

    /// A global function (`EXTERNAL_CALL`) the host provides, such as the
    /// constructors `String`, `Array` or `Date`, or an engine function
    /// like `PlayMusic`. `None` means the host does not know it, and the
    /// engine tries its own built-ins (`Sleep`, `Random`, `ToString`...).
    fn call_function(&mut self, name: &str, args: &[Value], ctx: Context<'_>) -> Option<Return>;

    /// A method of a native object (`obj.Method(args)`). `None` means the
    /// object has no such method.
    fn call_method(
        &mut self,
        target: NativeId,
        name: &str,
        args: &[Value],
        ctx: Context<'_>,
    ) -> Option<Return>;

    /// Called once the value a native call returned is on the calling
    /// script's stack and its effect is applied: the moment a host can
    /// take a snapshot of the engine that resumes right after the call
    /// (`Game.SaveGame`). The default does nothing.
    fn after_call(&mut self, _engine: &mut Engine, _script: ScriptId) {}
}
