//! The value stack of a script, and references to variables.
//!
//! Written with reference to `ScStack.cpp` and `ScScript.cpp` of WME Lite
//! (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::script::ScriptId;
use crate::value::Value;

/// The variables of one script's globals, shared by the script and the
/// threads it spawns.
pub type Globals = Rc<RefCell<BTreeMap<String, Value>>>;

/// A reference to a variable, as pushed by `PUSH_VAR_REF` and kept on the
/// `this` stack. References are transient: they are dereferenced before a
/// value is stored anywhere.
#[derive(Debug, Clone)]
pub enum VarRef {
    /// A local of a script's scope.
    Scope {
        /// The script.
        script: ScriptId,
        /// The scope's depth on the scope stack.
        depth: usize,
        /// The variable's name.
        name: String,
    },
    /// A global of a script (shared with its threads).
    ScriptGlobal {
        /// The globals table.
        globals: Globals,
        /// The variable's name.
        name: String,
    },
    /// An engine-wide global (`global x`).
    EngineGlobal {
        /// The variable's name.
        name: String,
    },
    /// A slot of a script's value stack (`PUSH_THIS_FROM_STACK`).
    Slot {
        /// The script.
        script: ScriptId,
        /// The slot's index from the bottom.
        index: usize,
    },
}

/// What a stack slot holds: a value, or a reference to a variable.
#[derive(Debug, Clone)]
pub enum Slot {
    /// A value, copied onto the stack.
    Value(Value),
    /// A reference to a variable.
    Ref(VarRef),
}

impl Slot {
    /// The value, if the slot holds one.
    pub fn value(&self) -> Option<&Value> {
        match self {
            Slot::Value(v) => Some(v),
            Slot::Ref(_) => None,
        }
    }
}

/// A script's value stack.
///
/// Like the engine's, the stack keeps its slots when values are popped:
/// a reference to a slot (`PUSH_THIS_FROM_STACK`) stays valid and sees
/// whatever is written there next. Only `correct_params` removes slots.
#[derive(Debug, Clone, Default)]
pub struct Stack {
    slots: Vec<Slot>,
    len: usize,
}

impl Stack {
    /// An empty stack.
    pub fn new() -> Self {
        Stack::default()
    }

    /// The number of live entries.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no entry is live.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Pushes a slot.
    pub fn push(&mut self, slot: Slot) {
        if self.len < self.slots.len() {
            self.slots[self.len] = slot;
        } else {
            self.slots.push(slot);
        }
        self.len += 1;
    }

    /// Pushes a value.
    pub fn push_value(&mut self, value: impl Into<Value>) {
        self.push(Slot::Value(value.into()));
    }

    /// Pops the top entry, or `None` on underflow.
    pub fn pop(&mut self) -> Option<Slot> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.slots[self.len].clone())
    }

    /// The top entry.
    pub fn top(&self) -> Option<&Slot> {
        self.len.checked_sub(1).map(|i| &self.slots[i])
    }

    /// The index of the top entry, for a slot reference.
    pub fn top_index(&self) -> Option<usize> {
        self.len.checked_sub(1)
    }

    /// The entry `depth` below the top (0 is the top).
    pub fn at(&self, depth: usize) -> Option<&Slot> {
        self.len.checked_sub(depth + 1).map(|i| &self.slots[i])
    }

    /// A slot by index from the bottom, live or not.
    pub fn slot(&self, index: usize) -> Option<&Slot> {
        self.slots.get(index)
    }

    /// A slot by index from the bottom, mutable.
    pub fn slot_mut(&mut self, index: usize) -> Option<&mut Slot> {
        self.slots.get_mut(index)
    }

    /// `CorrectParams`: pops the argument count pushed by a call, then
    /// drops surplus arguments (the last ones, deepest in the stack) or
    /// pads missing ones with `null`, so that exactly `expected`
    /// arguments remain with the first one on top. Returns the count that
    /// was pushed.
    pub fn correct_params(&mut self, expected: usize, count: usize) -> usize {
        let count = count.min(self.len);
        if expected < count {
            for _ in 0..(count - expected) {
                let index = self.len - 1 - expected;
                self.slots.remove(index);
                self.len -= 1;
            }
        } else if expected > count {
            for _ in 0..(expected - count) {
                let index = self.len - count;
                self.slots.insert(index, Slot::Value(Value::Null));
                self.len += 1;
            }
            self.slots.truncate(self.len);
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(stack: &Stack) -> Vec<i32> {
        (0..stack.len())
            .map(|i| match stack.slot(i) {
                Some(Slot::Value(Value::Int(v))) => *v,
                Some(Slot::Value(Value::Null)) => -1,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn slots_survive_pops() {
        let mut stack = Stack::new();
        stack.push_value(1);
        stack.push_value(2);
        assert_eq!(stack.top_index(), Some(1));
        assert!(matches!(stack.pop(), Some(Slot::Value(Value::Int(2)))));
        assert_eq!(stack.len(), 1);
        assert!(matches!(stack.slot(1), Some(Slot::Value(Value::Int(2)))));
        stack.push_value(3);
        assert!(matches!(stack.slot(1), Some(Slot::Value(Value::Int(3)))));
        assert!(matches!(stack.at(1), Some(Slot::Value(Value::Int(1)))));
        assert!(stack.at(2).is_none());
        stack.pop();
        stack.pop();
        assert!(stack.pop().is_none());
        assert!(stack.is_empty());
    }

    #[test]
    fn correct_params_trims_the_last_arguments() {
        // Call f(1, 2, 3): pushed 3, 2, 1 (first on top).
        let mut stack = Stack::new();
        stack.push_value(99);
        stack.push_value(3);
        stack.push_value(2);
        stack.push_value(1);
        assert_eq!(stack.correct_params(1, 3), 3);
        assert_eq!(ints(&stack), [99, 1]);
    }

    #[test]
    fn correct_params_pads_the_missing_arguments_with_null() {
        let mut stack = Stack::new();
        stack.push_value(99);
        stack.push_value(1);
        assert_eq!(stack.correct_params(3, 1), 1);
        assert_eq!(ints(&stack), [99, -1, -1, 1]);
        let mut stack = Stack::new();
        stack.push_value(99);
        assert_eq!(stack.correct_params(2, 0), 0);
        assert_eq!(ints(&stack), [99, -1, -1]);
    }

    #[test]
    fn correct_params_keeps_exact_counts() {
        let mut stack = Stack::new();
        stack.push_value(2);
        stack.push_value(1);
        assert_eq!(stack.correct_params(2, 2), 2);
        assert_eq!(ints(&stack), [2, 1]);
        assert_eq!(stack.correct_params(0, 0), 0);
        assert_eq!(ints(&stack), [2, 1]);
    }
}
